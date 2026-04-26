//! Unified error type for CodeGrasp core operations.

use std::path::PathBuf;

/// Errors returned by `cg_core` public APIs.
#[derive(Debug, thiserror::Error)]
pub enum CgError {
    /// Low-level I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// SQLite failure.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Embedding provider failure.
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// Chunking failure.
    #[error("Chunking error: {0}")]
    Chunking(String),

    /// Vector index failure.
    #[error("Index error: {0}")]
    Index(String),

    /// Search or read attempted before indexing.
    #[error("Codebase at path {path} is not indexed")]
    NotIndexed { path: PathBuf },

    /// File extension or language not supported by the AST chunker.
    #[error("Language not supported: {0}")]
    UnsupportedLanguage(String),

    /// Configuration load or validation failure.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Language server protocol failure.
    #[error("LSP error: {0}")]
    Lsp(String),

    /// Serialization or deserialization failure.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// TOML deserialization failure.
    #[error("TOML error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    /// UTF-8 decode failure for source text.
    #[error("Invalid UTF-8 in source file")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// UTF-8 decode failure for path or static str.
    #[error("Invalid UTF-8")]
    Utf8Str(#[from] std::str::Utf8Error),

    /// Manifest or index metadata inconsistency.
    #[error("State error: {0}")]
    State(String),
}
