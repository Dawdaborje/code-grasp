//! Persistent HNSW index (cosine on f32 vectors).

use std::path::Path;

use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::error::CgError;

/// Thin wrapper around a USearch index keyed by SQLite `chunks.id` (`u64`).
pub struct VectorIndex {
    index: Index,
    path: std::path::PathBuf,
}

impl VectorIndex {
    /// Open an existing index file or create an empty index at `path`.
    pub fn open_or_create(path: impl AsRef<Path>, dimensions: usize) -> Result<Self, CgError> {
        let path = path.as_ref().to_path_buf();
        let path_str = path
            .to_str()
            .ok_or_else(|| CgError::Index("index path is not UTF-8".to_string()))?;

        let opts = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            ..Default::default()
        };

        let index = Index::new(&opts).map_err(|e| CgError::Index(e.to_string()))?;
        if path.is_file() {
            index
                .load(path_str)
                .map_err(|e| CgError::Index(e.to_string()))?;
        }

        Ok(Self { index, path })
    }

    /// Returns the number of vectors currently stored in the index.
    pub fn len(&self) -> usize {
        self.index.size()
    }

    /// Returns `true` when the index contains no vectors.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pre-allocate capacity for at least `capacity` vectors total (including vectors already stored).
    ///
    /// This is a hint to the underlying USearch index; growth still succeeds if the hint is too low.
    pub fn reserve(&self, capacity: usize) -> Result<(), CgError> {
        self.index
            .reserve(capacity)
            .map_err(|e| CgError::Index(e.to_string()))
    }

    /// Insert or update a vector for `key` (chunk row id).
    pub fn add(&self, key: u64, vector: &[f32]) -> Result<(), CgError> {
        let expected = self.index.dimensions();
        if vector.len() != expected {
            return Err(CgError::Index(format!(
                "vector length {} does not match index dimensions {} (wrong model or corrupt index file?)",
                vector.len(),
                expected
            )));
        }
        if vector.iter().any(|x| !x.is_finite()) {
            return Err(CgError::Index(
                "vector contains NaN or infinite values".to_string(),
            ));
        }
        self.index
            .add(key, vector)
            .map_err(|e| CgError::Index(e.to_string()))
    }

    /// Remove a vector by key.
    pub fn remove(&self, key: u64) -> Result<(), CgError> {
        self.index
            .remove(key)
            .map_err(|e| CgError::Index(e.to_string()))?;
        Ok(())
    }

    /// Approximate nearest neighbors; returns `(chunk_id, distance)` (USearch distance for cosine metric).
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(u64, f32)>, CgError> {
        let expected = self.index.dimensions();
        if query.len() != expected {
            return Err(CgError::Index(format!(
                "query vector length {} does not match index dimensions {}",
                query.len(),
                expected
            )));
        }
        if query.iter().any(|x| !x.is_finite()) {
            return Err(CgError::Index(
                "query vector contains NaN or infinite values".to_string(),
            ));
        }
        let m = self
            .index
            .search(query, k)
            .map_err(|e| CgError::Index(e.to_string()))?;
        let mut out = Vec::with_capacity(m.keys.len());
        for i in 0..m.keys.len() {
            out.push((m.keys[i], m.distances[i]));
        }
        Ok(out)
    }

    /// Persist index to disk.
    pub fn save(&self) -> Result<(), CgError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(CgError::Io)?;
        }
        let path_str = self
            .path
            .to_str()
            .ok_or_else(|| CgError::Index("index path is not UTF-8".to_string()))?;
        self.index
            .save(path_str)
            .map_err(|e| CgError::Index(e.to_string()))
    }

    /// Vector dimensions configured for this index.
    pub fn dimensions(&self) -> usize {
        self.index.dimensions()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use tempfile::tempdir;

    use super::VectorIndex;

    #[test]
    fn len_returns_zero_on_empty_index() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path().join("idx.usr");
        let idx = VectorIndex::open_or_create(&p, 8).expect("create");
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
    }

    /// `Index::add` is covered end-to-end by `tests/index_pipeline_perf.rs`; an isolated `add`
    /// in this crate’s unit-test binary has triggered native SIGSEGV with usearch 2.25.1 on
    /// some targets, so we only assert `len` wiring on an empty persisted index here.
    #[test]
    fn len_stable_across_reopen_empty_index() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path().join("idx.usr");
        {
            let idx = VectorIndex::open_or_create(&p, 4).expect("create");
            assert_eq!(idx.len(), 0);
            idx.save().expect("save");
        }
        let idx2 = VectorIndex::open_or_create(&p, 4).expect("reopen");
        assert_eq!(idx2.len(), 0);
        assert!(idx2.is_empty());
    }

    #[test]
    fn reserve_does_not_panic_on_valid_capacity() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path().join("idx.usr");
        let idx = VectorIndex::open_or_create(&p, 4).expect("create");
        idx.reserve(10_000).expect("reserve");
    }

    #[test]
    fn remove_absent_key_is_ok() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path().join("idx.usr");
        let idx = VectorIndex::open_or_create(&p, 4).expect("create");
        idx.remove(99_999).expect("remove absent");
    }
}
