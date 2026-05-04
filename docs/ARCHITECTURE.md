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
AstChunker (tree-sitter per language, sliding-window fallback)
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

A **manifest** (JSON with per-file content hashes under `.code-grasp/`) drives updates: changed paths are re-chunked and re-embedded; removed paths drop rows and vector keys. If embedding **dimension** or provider metadata disagrees with the store, the index is cleared and rebuilt.

## Configuration

Merged sources (later wins): embedded defaults, optional global `~/.config/code-grasp/config.toml`, project `.code-grasp/config.toml`, environment (`CODEGRASP_*`), CLI flags where applicable. See crate `settings` for keys.

## MCP

The MCP server uses **rmcp** 1.5 with tool macros, stdio transport, and **tracing** to stderr only so stdout stays JSON-RPC clean. Tools map directly to `CodeGrasp::index`, `search`, `status`, and `clear`.

## Optional features

- **`lsp`**: reserved; `LspChunker` currently delegates to AST behavior so the feature compiles without async LSP wiring.
- **`openai`**: enables `OpenAiEmbedder` when provider and API key are configured.
