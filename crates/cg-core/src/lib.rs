//! # CodeGrasp (`cg_core`)
//!
//! Library for **walking**, **chunking**, **embedding**, and **hybrid search** over a local
//! codebase. State lives under `<project>/.code-grasp/` (SQLite + USearch + manifest).
//!
//! ## Features
//!
//! | Feature | Purpose |
//! |---------|---------|
//! *(none)* | Default: [`FastEmbedder`](embedder::FastEmbedder), AST [`AstChunker`](chunker::AstChunker), local index. |
//! `lsp` | Enables `LspChunker` in [`chunker`]; currently matches AST behavior until LSP is wired. |
//! `openai` | Enables `OpenAiEmbedder` in [`embedder`]; requires `CODEGRASP_OPENAI_API_KEY` (or legacy `CODEGASP_OPENAI_API_KEY`). |
//!
//! ## Architecture (short)
//!
//! 1. [`walker::walk_sources`] — discover text files (`.gitignore`, `.cgignore`, size, binary sniff,
//!    built-in extensions + optional `[indexing] extra_extensions`, well-known extensionless names).
//! 2. [`chunker::AstChunker`] — tree-sitter chunks + sliding-window fallback.
//! 3. [`embedder::FastEmbedder`] — dense vectors (default provider in [`CodeGrasp::index`](CodeGrasp::index)).
//! 4. [`index::VectorIndex`] + [`store::ChunkStore`] — approximate nearest neighbors and FTS5 BM25.
//! 5. [`CodeGrasp`] — async facade; CPU-bound work uses [`tokio::task::spawn_blocking`]. Indexing chunks
//!    files in parallel (rayon), pipelines embed + SQLite bulk transactions + vector adds, and optional
//!    `ort-cuda` enables the ONNX Runtime CUDA execution provider.
//!
//! ## Library usage
//!
//! Call from a **`tokio`** runtime. Load merged [`Settings`], then construct [`CodeGrasp`]:
//!
//! ```ignore
//! use std::path::Path;
//! use cg_core::{CgError, CodeGrasp, Settings};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), CgError> {
//!     let project = Path::new(".");
//!     let settings = Settings::load(project, None)?;
//!     let cg = CodeGrasp::new(project.to_path_buf(), settings);
//!
//!     let stats = cg.index(false).await?;
//!     println!("indexed {} files, {} chunks", stats.files_indexed, stats.chunks_written);
//!
//!     let hits = cg.search("authentication", 10).await?;
//!     for h in &hits {
//!         println!("{}:{}-{} {}", h.file_path, h.start_line, h.end_line, h.score);
//!     }
//!
//!     let st = cg.status().await?;
//!     println!("indexed={} chunks={}", st.indexed, st.chunk_count);
//!     Ok(())
//! }
//! ```
//!
//! ## CLI and MCP
//!
//! End-user workflows use the **`cg`** binary and **`code-grasp-mcp`** server. See the workspace
//! **README.md** for install paths, commands, and MCP tool names. Generate API HTML locally:
//!
//! ```text
//! cargo doc -p cg_core -p cg_proto --no-deps --open
//! ```
//!
//! ## Configuration
//!
//! Merge order for [`Settings::load`](Settings::load): defaults → `~/.config/code-grasp/config.toml`
//! → `<project>/.code-grasp/config.toml` → `CODEGRASP_*` env → optional CLI overlay.

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod chunker;
pub mod embedder;
pub mod error;
pub mod index;
pub mod manifest;
pub mod paths;
pub mod settings;
pub mod store;
pub mod walker;

mod facade;

pub use error::CgError;
pub use facade::{CodeGrasp, IndexStats, SearchHit, Status};
pub use settings::Settings;
