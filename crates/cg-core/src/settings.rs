//! Merged configuration: defaults, global TOML, project TOML, environment, CLI overrides.

use std::path::Path;

use config::{Config, Environment, File, FileFormat};
use serde::Deserialize;

use crate::error::CgError;
use crate::paths;

/// Root settings container matching TOML tables `[embedding]`, `[indexing]`, and `[lsp]`.
#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub embedding: EmbeddingSection,
    #[serde(default)]
    pub indexing: IndexingSection,
    #[serde(default)]
    pub lsp: LspSection,
}

/// Embedding provider and model options (`[embedding]` in TOML).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct EmbeddingSection {
    /// Provider name; the [`crate::CodeGrasp`] index path accepts **`fastembed`** only today.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Model id passed to fastembed (e.g. Hugging Face id).
    #[serde(default = "default_fastembed_model")]
    pub model: String,
    /// Batch size for embedding calls during indexing.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

/// Indexing limits and chunking parameters (`[indexing]` in TOML).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct IndexingSection {
    /// Files larger than this (bytes) are skipped by the walker.
    #[serde(default = "default_max_file_size")]
    pub max_file_size_bytes: u64,
    /// Soft minimum chunk size in heuristic token counts (chunker may merge small pieces).
    #[serde(default = "default_min_chunk_tokens")]
    pub min_chunk_tokens: u32,
    /// Soft maximum chunk size in heuristic token counts.
    #[serde(default = "default_max_chunk_tokens")]
    pub max_chunk_tokens: u32,
    /// Default result cap for search when the caller does not override `limit`.
    #[serde(default = "default_search_limit")]
    pub default_limit: usize,
    /// Extra filename extensions to index (lowercase entries without dot), merged with built-in list.
    #[serde(default)]
    pub extra_extensions: Vec<String>,
}

/// Paths to language servers (`[lsp]` in TOML; used when the `lsp` feature is enabled).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct LspSection {
    /// Executable or name on `PATH` for rust-analyzer.
    #[serde(default = "default_rust_analyzer")]
    pub rust_analyzer_path: String,
    /// Pyright entrypoint.
    #[serde(default = "default_pyright")]
    pub pyright_path: String,
    /// TypeScript language server entrypoint.
    #[serde(default = "default_tsserver")]
    pub tsserver_path: String,
}

impl Default for EmbeddingSection {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_fastembed_model(),
            batch_size: default_batch_size(),
        }
    }
}

impl Default for IndexingSection {
    fn default() -> Self {
        Self {
            max_file_size_bytes: default_max_file_size(),
            min_chunk_tokens: default_min_chunk_tokens(),
            max_chunk_tokens: default_max_chunk_tokens(),
            default_limit: default_search_limit(),
            extra_extensions: Vec::new(),
        }
    }
}

impl Default for LspSection {
    fn default() -> Self {
        Self {
            rust_analyzer_path: default_rust_analyzer(),
            pyright_path: default_pyright(),
            tsserver_path: default_tsserver(),
        }
    }
}

fn default_provider() -> String {
    "fastembed".to_string()
}

fn default_fastembed_model() -> String {
    "BAAI/bge-small-en-v1.5".to_string()
}

fn default_batch_size() -> usize {
    64
}

fn default_max_file_size() -> u64 {
    10_485_760
}

fn default_min_chunk_tokens() -> u32 {
    20
}

fn default_max_chunk_tokens() -> u32 {
    512
}

fn default_search_limit() -> usize {
    10
}

fn default_rust_analyzer() -> String {
    "rust-analyzer".to_string()
}

fn default_pyright() -> String {
    "pyright".to_string()
}

fn default_tsserver() -> String {
    "typescript-language-server".to_string()
}

const DEFAULT_SETTINGS_TOML: &str = include_str!("../default-settings.toml");

impl Settings {
    /// Load merged settings for a project directory.
    ///
    /// Merge order: built-in defaults → global `~/.config/code-grasp/config.toml` →
    /// `<project>/.code-grasp/config.toml` → environment variables (`CODEGRASP_*`) →
    /// optional CLI overlay (last wins).
    pub fn load(project_root: &Path, cli_overlay: Option<&Self>) -> Result<Self, CgError> {
        let mut builder =
            Config::builder().add_source(File::from_str(DEFAULT_SETTINGS_TOML, FileFormat::Toml));

        if let Some(path) = paths::global_config_path()
            && path.is_file()
        {
            builder = builder.add_source(File::from(path).format(FileFormat::Toml).required(false));
        }

        let proj_cfg = paths::project_config_path(project_root);
        if proj_cfg.is_file() {
            builder = builder.add_source(
                File::from(proj_cfg)
                    .format(FileFormat::Toml)
                    .required(false),
            );
        }

        builder = builder.add_source(
            Environment::with_prefix("CODEGRASP")
                .separator("__")
                .try_parsing(true)
                .ignore_empty(true),
        );

        let cfg = builder
            .build()
            .map_err(|e| CgError::Config(e.to_string()))?;
        let mut s: Settings = cfg
            .try_deserialize()
            .map_err(|e| CgError::Config(e.to_string()))?;

        if let Some(o) = cli_overlay {
            s.merge_cli(o);
        }

        Ok(s)
    }

    fn merge_cli(&mut self, o: &Self) {
        self.embedding.provider.clone_from(&o.embedding.provider);
        self.embedding.model.clone_from(&o.embedding.model);
        if o.embedding.batch_size != 0 {
            self.embedding.batch_size = o.embedding.batch_size;
        }
        if o.indexing.max_file_size_bytes != 0 {
            self.indexing.max_file_size_bytes = o.indexing.max_file_size_bytes;
        }
        if o.indexing.min_chunk_tokens != 0 {
            self.indexing.min_chunk_tokens = o.indexing.min_chunk_tokens;
        }
        if o.indexing.max_chunk_tokens != 0 {
            self.indexing.max_chunk_tokens = o.indexing.max_chunk_tokens;
        }
        if o.indexing.default_limit != 0 {
            self.indexing.default_limit = o.indexing.default_limit;
        }
        if !o.indexing.extra_extensions.is_empty() {
            self.indexing.extra_extensions.clone_from(&o.indexing.extra_extensions);
        }
        self.lsp
            .rust_analyzer_path
            .clone_from(&o.lsp.rust_analyzer_path);
        self.lsp.pyright_path.clone_from(&o.lsp.pyright_path);
        self.lsp.tsserver_path.clone_from(&o.lsp.tsserver_path);
    }
}
