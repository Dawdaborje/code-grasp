# Backlog

Prioritized follow-ups for CodeGrasp beyond the current MVP.

## Near term

1. **Real LSP chunking** — Wire `async-lsp` (or similar) and rust-analyzer for semantic ranges; keep AST fallback when the server is missing.
2. **More CLI / MCP tests** — `assert_cmd` flows for `index`, `search`, `status`, `clear` on synthetic trees (some may stay `#[ignore]` if they require model download).
3. **Chunker unit tests** — Boundary cases for merge/split heuristics per language snippet fixtures.

## Medium term

4. **VCS-aware ignores** — Confirm `.gitignore` behavior in non-git directories; document when users should init git or rely on `.cgignore`.
5. **Embedding cache policy** — Tunable cache dir, eviction, and offline diagnostics for fastembed first-run downloads.
6. **Search tuning** — Expose RRF `k`, vector vs lexical weights, or per-query mode in CLI/MCP.

## Longer term (“V3” ideas)

7. **Multi-root workspaces** — Index several roots with stable ids for monorepos.
8. **Incremental vector index** — Explore usearch maintenance APIs for large corpora where full rebuild is costly.
9. **Pluggable stores** — Optional remote or alternate backends while keeping the same `CodeGrasp` facade.
