# CodeGrasp architecture

CodeGrasp is a Rust workspace: **`cg_core`** (indexing and search), **`cg_proto`** (MCP-facing serde types and JSON Schema), **`code-grasp`** CLI (`cg`), and **`code-grasp-mcp`** (stdio MCP server).

## Data flow

```text
project root
    │
    ▼
walk_sources (ignore crate: .gitignore, .cgignore, hidden dirs, size, binary sniff, extensions)
    │
    ▼
AstChunker (tree-sitter per language, line-aligned window fallback)
    │
    ▼
FastEmbedder (default) or OpenAIEmbedder (feature `openai`, env API key)
    │
    ├──────────────────────────┐
    ▼                          ▼
VectorIndex (usearch 2.x,    ChunkStore (SQLite: chunks + FTS5
cosine, f32)                  external content + BM25)
    │                          │
    └────────── hybrid ────────┘
               search
```

## Hybrid search

Query text is embedded once. **Vector** search returns nearest neighbors by distance. **Lexical** search uses SQLite FTS5 BM25 on chunk text. Results are merged with **reciprocal rank fusion (RRF)** so a hit strong in only one channel can still surface.

## Incremental indexing

A **manifest** (JSON with per-file content hashes under `.code-grasp/`) drives updates: changed paths are re-chunked and re-embedded; removed paths drop rows and vector keys. If the embedding **provider**, **model**, or **dimension** recorded in the store differs from the configuration, the index is cleared and rebuilt (search refuses to run against an index built with a different model). Writers (`index`, `clear`) hold an exclusive OS lock on `.code-grasp/index.lock`, so concurrent CLI/MCP runs fail fast instead of corrupting the index.

## Chunking

Tree-sitter queries capture declarations (functions, classes, impls, ...). A declaration that fits `max_chunk_tokens` (whitespace word count, plus a byte cap for minified text) is one chunk; a larger one is broken into its nested declarations, with its header/footer and other uncaptured code as separate chunks, so every non-blank byte is covered exactly once. Code between declarations (imports, constants, top-level statements) forms its own chunks. Chunks under `min_chunk_tokens` merge into a neighbour when the result still fits. Unsupported languages use line-aligned windows with a small overlap.

## Configuration

Merged sources (later wins): embedded defaults, optional global `~/.config/code-grasp/config.toml`, project `.code-grasp/config.toml`, environment (`CODEGRASP_*`), CLI flags where applicable. See crate `settings` for keys.

## MCP

The MCP server uses **rmcp** 1.5 with tool macros, stdio transport, and **tracing** to stderr only so stdout stays JSON-RPC clean. Tools map directly to `CodeGrasp::index`, `search`, `status`, and `clear`.

## Optional features

- **`lsp`**: reserved; `LspChunker` currently delegates to AST behavior so the feature compiles without async LSP wiring.
- **`openai`**: enables `OpenAiEmbedder` when provider and API key are configured.
