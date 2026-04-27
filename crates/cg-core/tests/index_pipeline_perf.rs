//! Integration tests for indexing pipeline overlap, incremental stats, and WAL concurrency.

use std::fs;
use std::path::Path;
use std::time::Duration;

use cg_core::index::VectorIndex;
use cg_core::paths;
use cg_core::store::ChunkStore;
use cg_core::{CgError, CodeGrasp, Settings};
use tempfile::tempdir;

fn rust_module_body(i: usize) -> String {
    let mut s = format!("//! synthetic module {i}\npub mod inner_{i} {{\n");
    for j in 0..6 {
        s.push_str(&format!("    pub fn fn_{i}_{j}() -> u32 {{ {j} }}\n"));
    }
    s.push_str("}\n");
    s
}

fn mk_project_with_rs_files(root: &Path, n: usize) -> Result<(), CgError> {
    let src = root.join("src");
    fs::create_dir_all(&src).map_err(CgError::Io)?;
    for i in 0..n {
        let path = src.join(format!("mod_{i}.rs"));
        let mut body = rust_module_body(i);
        if i == 0 {
            body.push_str("\npub fn codegrasp_ix_marker_zeta_unique() -> u32 { 424242 }\n");
        }
        fs::write(&path, body).map_err(CgError::Io)?;
    }
    Ok(())
}

#[tokio::test]
async fn producer_consumer_pipeline_indexes_correctly() -> Result<(), CgError> {
    let td = tempdir().map_err(CgError::Io)?;
    let root = td.path().canonicalize().map_err(CgError::Io)?;
    mk_project_with_rs_files(&root, 35)?;
    let settings = Settings::load(&root, None)?;
    let cg = CodeGrasp::new(root.clone(), settings);
    let stats = cg.index(false).await?;
    assert_eq!(stats.files_indexed, 35);
    assert!(stats.chunks_written > 0);
    let hits = cg.search("codegrasp_ix_marker_zeta_unique", 12).await?;
    assert!(
        hits.iter()
            .any(|h| h.content.contains("codegrasp_ix_marker")),
        "expected at least one hit for injected marker"
    );

    let store_path = paths::store_db_path(&root);
    let index_path = paths::index_path(&root);
    let store = ChunkStore::open(&store_path)?;
    let dim: usize = store
        .get_meta("embedding_dim")?
        .ok_or_else(|| CgError::State("missing embedding_dim".into()))?
        .parse()
        .map_err(|_| CgError::State("invalid embedding_dim".into()))?;
    let vidx = VectorIndex::open_or_create(&index_path, dim)?;
    assert_eq!(
        vidx.len() as u64,
        store.chunk_count()?,
        "vector index size should match chunk rows after indexing"
    );
    Ok(())
}

#[tokio::test]
async fn incremental_index_skips_unchanged_files() -> Result<(), CgError> {
    let td = tempdir().map_err(CgError::Io)?;
    let root = td.path().canonicalize().map_err(CgError::Io)?;
    mk_project_with_rs_files(&root, 20)?;
    let settings = Settings::load(&root, None)?;
    let cg = CodeGrasp::new(root.clone(), settings);
    let first = cg.index(false).await?;
    assert_eq!(first.files_indexed, 20);
    assert!(first.chunks_written > 0);
    let second = cg.index(false).await?;
    assert_eq!(second.files_indexed, 0);
    assert_eq!(second.files_skipped, 20);
    assert_eq!(second.chunks_written, 0);
    Ok(())
}

#[tokio::test]
async fn incremental_index_reindexes_only_changed_file() -> Result<(), CgError> {
    let td = tempdir().map_err(CgError::Io)?;
    let root = td.path().canonicalize().map_err(CgError::Io)?;
    mk_project_with_rs_files(&root, 12)?;
    let settings = Settings::load(&root, None)?;
    let cg = CodeGrasp::new(root.clone(), settings);
    cg.index(false).await?;
    let path = root.join("src").join("mod_3.rs");
    let mut s = fs::read_to_string(&path).map_err(CgError::Io)?;
    s.push_str("\npub fn codegrasp_touch_only_one() {}\n");
    fs::write(&path, s).map_err(CgError::Io)?;
    let third = cg.index(false).await?;
    assert_eq!(third.files_indexed, 1);
    assert_eq!(third.files_skipped, 11);
    Ok(())
}

#[tokio::test]
async fn wal_mode_survives_concurrent_reads_during_index() -> Result<(), CgError> {
    let td = tempdir().map_err(CgError::Io)?;
    let root = td.path().canonicalize().map_err(CgError::Io)?;
    mk_project_with_rs_files(&root, 18)?;
    let settings = Settings::load(&root, None)?;
    let cg = CodeGrasp::new(root.clone(), settings.clone());
    let cg2 = CodeGrasp::new(root, settings);

    let index_handle = tokio::spawn(async move { cg.index(false).await });

    for _ in 0..120 {
        if let Err(e) = cg2.search("pub fn", 5).await {
            let msg = e.to_string().to_lowercase();
            assert!(
                !msg.contains("database is locked"),
                "search returned DB lock during WAL index: {e}"
            );
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    index_handle
        .await
        .map_err(|e| CgError::State(format!("index task join: {e}")))??;
    Ok(())
}
