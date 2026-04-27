//! High-level orchestration: index, search, status, and clear.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use thread_local::ThreadLocal;

use crate::chunker::{AstChunker, Chunk, Chunker};
use crate::embedder::{Embedder, FastEmbedder};
use crate::error::CgError;
use crate::index::VectorIndex;
use crate::manifest::Manifest;
use crate::paths;
use crate::settings::Settings;
use crate::store::{ChunkStore, reciprocal_rank_fusion};
use crate::walker;

/// Summary written after a successful index run.
#[derive(Debug, Clone)]
pub struct IndexStats {
    /// Source files that contributed at least one chunk in this run (after incremental diff).
    pub files_indexed: u64,
    /// Walked files whose content hash matched the manifest and were not re-processed this run.
    pub files_skipped: u64,
    /// Total chunk rows written or updated during indexing.
    pub chunks_written: u64,
    /// Wall-clock time for the full `blocking_index` pass, including walk and manifest I/O.
    pub elapsed_ms: u64,
}

/// One ranked search hit (hybrid vector + FTS, fused by RRF).
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Fused relevance score (higher is better).
    pub score: f64,
    /// File path relative to the project root.
    pub file_path: String,
    /// 1-based start line of the chunk in the file.
    pub start_line: u32,
    /// 1-based end line (inclusive) of the chunk in the file.
    pub end_line: u32,
    /// Chunk source text.
    pub content: String,
}

/// Lightweight view of on-disk index state.
#[derive(Debug, Clone)]
pub struct Status {
    /// `true` if the SQLite store reports prior indexing metadata.
    pub indexed: bool,
    /// Distinct source files referenced in the chunk store (best-effort count).
    pub file_count: u64,
    /// Number of chunk rows in the store.
    pub chunk_count: u64,
    /// Unix epoch seconds when the index was last written, if recorded.
    pub last_indexed: Option<i64>,
}

/// Primary entry type for indexing and search over a single project directory.
///
/// Construct with [`CodeGrasp::new`](CodeGrasp::new) after loading [`Settings`](crate::Settings).
///
/// # Current limitations
///
/// [`index`](CodeGrasp::index) validates that `settings.embedding.provider` is **`fastembed`**;
/// other providers return [`CgError::Config`](crate::CgError::Config) until wired through this facade.
#[derive(Debug, Clone)]
pub struct CodeGrasp {
    project_root: PathBuf,
    settings: Settings,
}

impl CodeGrasp {
    /// Create a handle for `project_root` using the supplied merged [`Settings`].
    pub fn new(project_root: PathBuf, settings: Settings) -> Self {
        Self {
            project_root,
            settings,
        }
    }

    /// Index the project directory (blocking work runs on the blocking thread-pool).
    pub async fn index(&self, force: bool) -> Result<IndexStats, CgError> {
        let root = self.project_root.clone();
        let settings = self.settings.clone();
        tokio::task::spawn_blocking(move || blocking_index(root, settings, force))
            .await
            .map_err(|e| CgError::State(format!("index join: {e}")))?
    }

    /// Hybrid dense + BM25 search with RRF fusion.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, CgError> {
        let root = self.project_root.clone();
        let settings = self.settings.clone();
        let q = query.to_string();
        let lim = limit.max(1);
        tokio::task::spawn_blocking(move || blocking_search(root, settings, &q, lim))
            .await
            .map_err(|e| CgError::State(format!("search join: {e}")))?
    }

    /// Return counts and timestamps from the SQLite store.
    pub async fn status(&self) -> Result<Status, CgError> {
        let root = self.project_root.clone();
        tokio::task::spawn_blocking(move || blocking_status(root))
            .await
            .map_err(|e| CgError::State(format!("status join: {e}")))?
    }

    /// Remove all indexed data for this project (SQLite + USearch + manifest).
    pub async fn clear(&self) -> Result<(), CgError> {
        let root = self.project_root.clone();
        tokio::task::spawn_blocking(move || blocking_clear(root))
            .await
            .map_err(|e| CgError::State(format!("clear join: {e}")))?
    }
}

fn now_unix() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

/// Chunk count per embed pass, one SQLite transaction, and USearch `add` calls.
const INDEX_PIPELINE_CHUNK_BATCH: usize = 2048;
/// Number of full chunk batches buffered between producer (chunking) and consumer (embedding).
const INDEX_CHANNEL_CAPACITY: usize = 4;

fn embed_write_chunk_batch(
    embedder: &FastEmbedder,
    store: &ChunkStore,
    vindex: &VectorIndex,
    dim: usize,
    batch: Vec<Chunk>,
) -> Result<usize, CgError> {
    if batch.is_empty() {
        return Ok(0);
    }
    let n = batch.len();
    let texts: Vec<&str> = batch.iter().map(|c| c.content.as_str()).collect();
    let embeddings = embedder.embed(&texts)?;
    if embeddings.len() != n {
        return Err(CgError::State("embedding batch size mismatch".into()));
    }
    if let Some(v0) = embeddings.first()
        && v0.len() != dim
    {
        return Err(CgError::Embedding(format!(
            "embedding length {} does not match model dimension {}",
            v0.len(),
            dim
        )));
    }
    let row_ids = store.insert_chunks_bulk(&batch)?;
    if row_ids.len() != n {
        return Err(CgError::State(
            "insert_chunks_bulk returned unexpected id count".into(),
        ));
    }
    let target_cap = vindex
        .len()
        .checked_add(n)
        .ok_or_else(|| CgError::Index("vector index capacity overflow".into()))?;
    vindex.reserve(target_cap)?;
    for (id, vec) in row_ids.iter().zip(embeddings.iter()) {
        let k = *id as u64;
        // SQLite may reuse `chunks.id` after deletes; stale USearch keys must not remain or
        // `add` fails with "Duplicate keys". `remove` returns Ok when the key is absent.
        vindex.remove(k)?;
        vindex.add(k, vec)?;
    }
    Ok(n)
}

/// Runs parallel chunking and pushes `INDEX_PIPELINE_CHUNK_BATCH`-sized `Vec<Chunk>` messages to `chunk_tx`.
fn chunk_producer_run(
    files_to_process: Vec<PathBuf>,
    walker_arc: Arc<Vec<walker::SourceFile>>,
    path_to_idx: Arc<HashMap<String, usize>>,
    min_t: u32,
    max_t: u32,
    chunk_tx: std::sync::mpsc::SyncSender<Vec<Chunk>>,
) -> Result<(), CgError> {
    let tls: ThreadLocal<AstChunker> = ThreadLocal::new();
    let shared = Mutex::new(Vec::<Chunk>::with_capacity(INDEX_PIPELINE_CHUNK_BATCH * 2));
    files_to_process.par_iter().try_for_each(|rel| {
        let key = rel.to_string_lossy().to_string();
        let idx = path_to_idx
            .get(&key)
            .ok_or_else(|| CgError::State(format!("missing walked file `{key}`")))?;
        let sf = walker_arc
            .get(*idx)
            .ok_or_else(|| CgError::State(format!("missing walk entry for `{key}`")))?;
        let chunker = tls.get_or(|| AstChunker::new(min_t, max_t));
        let chunks = chunker.chunk(sf)?;
        let mut guard = shared
            .lock()
            .map_err(|_| CgError::State("chunk staging mutex poisoned".into()))?;
        for c in chunks {
            guard.push(c);
            if guard.len() >= INDEX_PIPELINE_CHUNK_BATCH {
                let taken: Vec<Chunk> = guard.drain(..INDEX_PIPELINE_CHUNK_BATCH).collect();
                drop(guard);
                chunk_tx.send(taken).map_err(|_| {
                    CgError::Index("consumer disconnected before producer finished".into())
                })?;
                guard = shared
                    .lock()
                    .map_err(|_| CgError::State("chunk staging mutex poisoned".into()))?;
            }
        }
        Ok::<(), CgError>(())
    })?;
    let last = shared
        .into_inner()
        .map_err(|_| CgError::State("chunk staging mutex poisoned".into()))?;
    if !last.is_empty() {
        chunk_tx
            .send(last)
            .map_err(|_| CgError::Index("consumer disconnected before producer finished".into()))?;
    }
    Ok(())
}

fn blocking_index(root: PathBuf, settings: Settings, force: bool) -> Result<IndexStats, CgError> {
    let start = Instant::now();
    if settings.embedding.provider != "fastembed" {
        return Err(CgError::Config(
            "only `fastembed` embedding provider is supported in this release (set `[embedding] provider = \"fastembed\"`)".into(),
        ));
    }

    std::fs::create_dir_all(paths::project_data_dir(&root)).map_err(CgError::Io)?;

    let store_path = paths::store_db_path(&root);
    let index_path = paths::index_path(&root);
    let manifest_path = paths::manifest_path(&root);

    let walker_files = walker::walk_sources(
        &root,
        settings.indexing.max_file_size_bytes,
        &settings.indexing.extra_extensions,
    )?;
    let mut current_hashes: HashMap<String, String> = HashMap::new();
    for sf in &walker_files {
        current_hashes.insert(
            sf.path.to_string_lossy().to_string(),
            crate::manifest::hash_bytes(sf.content.as_bytes()),
        );
    }

    let mut manifest = Manifest::load(&manifest_path)?;
    let store = ChunkStore::open(&store_path)?;

    let embedder = FastEmbedder::new(&settings.embedding.model, settings.embedding.batch_size)?;
    let dim = embedder.dimension();

    let mut full_reindex = force;
    if let Some(prev) = store.get_meta("embedding_dim")?
        && prev != dim.to_string()
    {
        full_reindex = true;
    }

    if full_reindex {
        store.clear_all()?;
        std::fs::remove_file(&index_path).ok();
        manifest = Manifest::default();
    }

    store.set_meta("embedding_dim", &dim.to_string())?;
    store.set_meta("embedding_provider", &settings.embedding.provider)?;

    let (files_to_process, removed): (Vec<PathBuf>, Vec<String>) = if full_reindex {
        (
            walker_files.iter().map(|f| f.path.clone()).collect(),
            Vec::new(),
        )
    } else {
        let diff = manifest.diff(&current_hashes);
        (diff.added_or_changed, diff.removed)
    };

    let skipped_unchanged = current_hashes.len().saturating_sub(files_to_process.len());
    tracing::info!(
        total_walked = current_hashes.len(),
        to_index = files_to_process.len(),
        to_remove = removed.len(),
        skipped_unchanged,
        "incremental index plan"
    );

    let vindex = VectorIndex::open_or_create(&index_path, dim)?;
    if vindex.dimensions() != dim {
        return Err(CgError::State(format!(
            "vector index on disk expects {} dimensions but the embedding model produces {}; remove `{}` or run `cg index --force`",
            vindex.dimensions(),
            dim,
            index_path.display()
        )));
    }

    for rel in &removed {
        let ids = store.chunk_ids_for_file(rel)?;
        for id in ids {
            vindex.remove(id as u64)?;
        }
        store.delete_chunks_for_file(rel)?;
        store.delete_file_row(rel)?;
    }

    for rel in &files_to_process {
        let key = rel.to_string_lossy().to_string();
        let ids = store.chunk_ids_for_file(&key)?;
        for id in ids {
            vindex.remove(id as u64)?;
        }
        store.delete_chunks_for_file(&key)?;
    }

    let path_to_idx: Arc<HashMap<String, usize>> = Arc::new(
        walker_files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.to_string_lossy().to_string(), i))
            .collect(),
    );
    let walker_arc = Arc::new(walker_files);

    let min_t = settings.indexing.min_chunk_tokens;
    let max_t = settings.indexing.max_chunk_tokens;

    // USearch `reserve` takes desired **total** capacity (including vectors kept for unchanged files).
    // Passing only a headroom number can be < current `len()` and corrupt the index on incremental runs.
    if !files_to_process.is_empty() {
        let headroom = files_to_process
            .len()
            .saturating_mul(4)
            .clamp(64, 2_000_000);
        let target = vindex
            .len()
            .checked_add(headroom)
            .ok_or_else(|| CgError::Index("vector index capacity overflow".into()))?;
        vindex.reserve(target)?;
    }

    let mut chunks_written: u64 = 0;
    if !files_to_process.is_empty() {
        let (chunk_tx, chunk_rx) = sync_channel::<Vec<Chunk>>(INDEX_CHANNEL_CAPACITY);
        let producer_files = files_to_process.clone();
        let w = Arc::clone(&walker_arc);
        let p = Arc::clone(&path_to_idx);
        let producer =
            thread::spawn(move || chunk_producer_run(producer_files, w, p, min_t, max_t, chunk_tx));
        let consumer_result: Result<(), CgError> = (|| {
            for batch in chunk_rx {
                if batch.is_empty() {
                    continue;
                }
                let n = embed_write_chunk_batch(&embedder, &store, &vindex, dim, batch)?;
                chunks_written += n as u64;
            }
            Ok::<(), CgError>(())
        })();
        let producer_join = producer
            .join()
            .map_err(|_| CgError::Index("chunking thread panicked".into()))?;
        consumer_result?;
        producer_join?;
    }

    tracing::info!(
        files_indexed = files_to_process.len(),
        chunks_written,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "indexing complete"
    );

    let ts = now_unix();
    for rel in &files_to_process {
        let key = rel.to_string_lossy().to_string();
        let hash = current_hashes
            .get(&key)
            .ok_or_else(|| CgError::State(format!("missing hash for `{key}`")))?;
        store.upsert_file(&key, hash, ts)?;
    }

    manifest.replace_all(current_hashes);
    manifest.save(&manifest_path)?;

    vindex.save()?;

    let elapsed_ms = start.elapsed().as_millis() as u64;
    Ok(IndexStats {
        files_indexed: files_to_process.len() as u64,
        files_skipped: skipped_unchanged as u64,
        chunks_written,
        elapsed_ms,
    })
}

fn blocking_search(
    root: PathBuf,
    settings: Settings,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, CgError> {
    if settings.embedding.provider != "fastembed" {
        return Err(CgError::Config(
            "only `fastembed` embedding provider is supported in this release".into(),
        ));
    }

    let store_path = paths::store_db_path(&root);
    let index_path = paths::index_path(&root);
    if !store_path.is_file() || !index_path.is_file() {
        return Err(CgError::NotIndexed { path: root });
    }

    let store = ChunkStore::open(&store_path)?;
    let dim: usize = store
        .get_meta("embedding_dim")?
        .ok_or_else(|| CgError::State("missing embedding_dim metadata".into()))?
        .parse()
        .map_err(|_| CgError::State("invalid embedding_dim metadata".into()))?;

    let embedder = FastEmbedder::new(&settings.embedding.model, settings.embedding.batch_size)?;
    if embedder.dimension() != dim {
        return Err(CgError::State(
            "configured embedding model dimension does not match index".into(),
        ));
    }

    let v = embedder.embed(&[query])?;
    let qv = v
        .first()
        .ok_or_else(|| CgError::Embedding("empty query embedding".into()))?;

    let vindex = VectorIndex::open_or_create(&index_path, dim)?;
    if vindex.dimensions() != dim {
        return Err(CgError::State(format!(
            "vector index on disk expects {} dimensions but the embedding model produces {}; remove `{}` or run `cg index --force`",
            vindex.dimensions(),
            dim,
            index_path.display()
        )));
    }
    let dense = vindex.search(qv, 50)?;
    let dense_ids: Vec<i64> = dense.iter().map(|(k, _)| *k as i64).collect();

    let sparse_ids = store.fts_search(query, 50)?;

    let top: Vec<(i64, f64)> = reciprocal_rank_fusion(&dense_ids, &sparse_ids)
        .into_iter()
        .take(limit)
        .collect();
    let ids: Vec<i64> = top.iter().map(|(id, _)| *id).collect();
    let score_map: HashMap<i64, f64> = top.into_iter().collect();

    let mut hits = store.fetch_chunks(&ids)?;
    for h in &mut hits {
        h.score = score_map.get(&h.id).copied().unwrap_or(0.0);
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));

    Ok(hits
        .into_iter()
        .map(|h| SearchHit {
            score: h.score,
            file_path: h.file_path,
            start_line: h.start_line as u32,
            end_line: h.end_line as u32,
            content: h.content,
        })
        .collect())
}

fn blocking_status(root: PathBuf) -> Result<Status, CgError> {
    let store_path = paths::store_db_path(&root);
    if !store_path.is_file() {
        return Ok(Status {
            indexed: false,
            file_count: 0,
            chunk_count: 0,
            last_indexed: None,
        });
    }
    let store = ChunkStore::open(&store_path)?;
    let chunk_count = store.chunk_count()?;
    let file_count = store.file_count()?;
    let last_indexed = store.last_indexed()?;
    Ok(Status {
        indexed: chunk_count > 0,
        file_count,
        chunk_count,
        last_indexed,
    })
}

fn blocking_clear(root: PathBuf) -> Result<(), CgError> {
    let store_path = paths::store_db_path(&root);
    let index_path = paths::index_path(&root);
    let manifest_path = paths::manifest_path(&root);
    if store_path.is_file() {
        let store = ChunkStore::open(&store_path)?;
        store.clear_all()?;
    }
    std::fs::remove_file(&index_path).ok();
    std::fs::remove_file(&manifest_path).ok();
    Ok(())
}
