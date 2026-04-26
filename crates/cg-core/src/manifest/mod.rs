//! Incremental indexing manifest: path → content hash.

mod hasher;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use hasher::hash_bytes;

use crate::error::CgError;

/// On-disk manifest: maps relative file path to hex SHA-256 of file bytes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    /// Relative POSIX-style paths for stable JSON across OS (normalized at save).
    #[serde(default)]
    pub files: HashMap<String, String>,
}

/// Result of comparing a walk with the stored manifest.
#[derive(Debug, Default)]
pub struct ManifestDiff {
    pub added_or_changed: Vec<PathBuf>,
    pub removed: Vec<String>,
}

impl Manifest {
    /// Load manifest from JSON bytes, or empty manifest if missing/invalid.
    pub fn load(path: &Path) -> Result<Self, CgError> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(CgError::Io)?;
        serde_json::from_str(&text).map_err(CgError::Serialization)
    }

    /// Write manifest to JSON file (atomic replace via temp file).
    pub fn save(&self, path: &Path) -> Result<(), CgError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(CgError::Io)?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(CgError::Serialization)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &data).map_err(CgError::Io)?;
        std::fs::rename(&tmp, path).map_err(CgError::Io)?;
        Ok(())
    }

    /// Compute which paths are new/changed and which manifest keys disappeared.
    pub fn diff(&self, current_hashes: &HashMap<String, String>) -> ManifestDiff {
        let mut added_or_changed = Vec::new();
        for (path, hash) in current_hashes {
            match self.files.get(path) {
                None => added_or_changed.push(PathBuf::from(path)),
                Some(old) if old != hash => added_or_changed.push(PathBuf::from(path)),
                _ => {}
            }
        }
        let mut removed = Vec::new();
        for path in self.files.keys() {
            if !current_hashes.contains_key(path) {
                removed.push(path.clone());
            }
        }
        ManifestDiff {
            added_or_changed,
            removed,
        }
    }

    /// Update in-memory manifest to match `current_hashes` exactly.
    pub fn replace_all(&mut self, current_hashes: HashMap<String, String>) {
        self.files = current_hashes;
    }
}

#[cfg(test)]
mod tests {
    use super::Manifest;
    use std::collections::HashMap;

    #[test]
    fn diff_detects_added_changed_removed() {
        let mut old = Manifest::default();
        old.files.insert("a.rs".into(), "h1".into());
        old.files.insert("b.rs".into(), "h2".into());

        let mut cur = HashMap::new();
        cur.insert("a.rs".into(), "h1_new".into()); // changed
        cur.insert("c.rs".into(), "h3".into()); // added
        // b.rs removed

        let d = old.diff(&cur);
        assert!(
            d.added_or_changed
                .iter()
                .any(|p| p.to_string_lossy() == "a.rs")
        );
        assert!(
            d.added_or_changed
                .iter()
                .any(|p| p.to_string_lossy() == "c.rs")
        );
        assert!(d.removed.contains(&"b.rs".to_string()));
    }
}
