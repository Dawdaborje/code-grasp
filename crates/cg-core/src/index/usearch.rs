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

    /// Pre-allocate index capacity (reduces reallocations during bulk `add`).
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
