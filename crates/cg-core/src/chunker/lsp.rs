//! LSP-enriched chunking (optional `lsp` feature). Degrades to [`AstChunker`] output when LSP is unavailable.

use crate::chunker::{AstChunker, Chunk, Chunker, Language};
use crate::error::CgError;
use crate::walker::SourceFile;

/// Wraps [`AstChunker`]; LSP metadata hooks are reserved for future releases.
pub struct LspChunker {
    inner: AstChunker,
}

impl LspChunker {
    /// Create an LSP-aware chunker using the same token bounds as `inner`.
    pub fn new(inner: AstChunker) -> Self {
        Self { inner }
    }
}

impl Chunker for LspChunker {
    fn chunk(&self, file: &SourceFile) -> Result<Vec<Chunk>, CgError> {
        // V2: spawn rust-analyzer / pyright / tsserver and enrich chunks. For now, pure AST.
        self.inner.chunk(file)
    }

    fn supported_languages(&self) -> &[Language] {
        self.inner.supported_languages()
    }
}
