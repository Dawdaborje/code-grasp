//! SQLite + FTS5 basic roundtrip (no embeddings).

use std::path::PathBuf;

use cg_core::chunker::{Chunk, Language};
use cg_core::store::ChunkStore;

fn sample_chunk(path: &str, text: &str) -> Chunk {
    Chunk {
        content: text.to_string(),
        file_path: PathBuf::from(path),
        start_byte: 0,
        end_byte: text.len(),
        start_line: 1,
        end_line: 1,
        language: Language::Rust,
        content_hash: "00".into(),
    }
}

#[test]
fn insert_chunk_and_fts_match() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("store.db");
    let store = ChunkStore::open(&db).unwrap();
    let c = sample_chunk("src/lib.rs", "fn authenticate_user() {}");
    store.insert_chunk(&c).unwrap();
    let ids = store.fts_search("authenticate", 10).unwrap();
    assert!(!ids.is_empty());
}
