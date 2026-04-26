//! Canonical paths for CodeGrasp project and global state.
//!
//! All project-local artifacts live under [`project_data_dir`]
//! (typically `<repo>/.code-grasp/`). Configuration overrides use [`project_config_path`];
//! see [`crate::Settings::load`] for merge order.

use std::path::{Path, PathBuf};

/// Directory name under the project root (`.code-grasp/`).
pub const DOT_DIR: &str = ".code-grasp";

/// Relative path to the vector index file.
pub const INDEX_FILE: &str = "index.usearch";

/// Relative path to the SQLite database.
pub const STORE_DB: &str = "store.db";

/// Relative path to the incremental manifest.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Relative path to per-project configuration overrides.
pub const PROJECT_CONFIG: &str = "config.toml";

/// Returns `<project_root>/.code-grasp`.
pub fn project_data_dir(project_root: &Path) -> PathBuf {
    project_root.join(DOT_DIR)
}

/// Returns `<project_root>/.code-grasp/store.db`.
pub fn store_db_path(project_root: &Path) -> PathBuf {
    project_data_dir(project_root).join(STORE_DB)
}

/// Returns `<project_root>/.code-grasp/index.usearch`.
pub fn index_path(project_root: &Path) -> PathBuf {
    project_data_dir(project_root).join(INDEX_FILE)
}

/// Returns `<project_root>/.code-grasp/manifest.json`.
pub fn manifest_path(project_root: &Path) -> PathBuf {
    project_data_dir(project_root).join(MANIFEST_FILE)
}

/// Returns `<project_root>/.code-grasp/config.toml`.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_data_dir(project_root).join(PROJECT_CONFIG)
}

/// Returns `~/.config/code-grasp/config.toml` when available.
pub fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("code-grasp").join("config.toml"))
}

/// Returns `~/.cache/code-grasp/models` when available.
pub fn models_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|p| p.join("code-grasp").join("models"))
}
