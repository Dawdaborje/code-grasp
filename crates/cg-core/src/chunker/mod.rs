//! Source splitting into searchable chunks (AST-first, with fallbacks).

mod ast;

#[cfg(feature = "lsp")]
mod lsp;

pub use ast::AstChunker;

#[cfg(feature = "lsp")]
pub use lsp::LspChunker;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CgError;
use crate::walker::SourceFile;

/// Detected source language for a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
    C,
    Cpp,
    Unknown,
}

/// One logical slice of source with stable metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub content: String,
    pub file_path: std::path::PathBuf,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    pub language: Language,
    pub content_hash: String,
}

/// Abstraction over chunking strategies (AST, LSP-enriched, etc.).
pub trait Chunker: Send + Sync {
    /// Split `file` into chunks.
    fn chunk(&self, file: &SourceFile) -> Result<Vec<Chunk>, CgError>;

    /// Languages this chunker targets for AST paths.
    fn supported_languages(&self) -> &[Language];
}

impl Language {
    /// Infer language from file path extension.
    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("rs") => Language::Rust,
            Some("py") => Language::Python,
            Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Language::JavaScript,
            Some("ts") => Language::TypeScript,
            Some("tsx") => Language::Tsx,
            Some("go") => Language::Go,
            Some("java") => Language::Java,
            Some("c") | Some("h") => Language::C,
            Some("cc") | Some("cpp") | Some("cxx") | Some("hpp") | Some("hh") | Some("hxx") => {
                Language::Cpp
            }
            _ => Language::Unknown,
        }
    }
}
