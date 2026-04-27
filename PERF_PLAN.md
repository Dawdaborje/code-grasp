# CodeGrasp indexing performance — implementation plan

This document records what will change, why, data flow before/after, and risks. Implementation follows this plan.

---

## Improvement 1 — Producer–consumer pipeline (`facade.rs`)

### What

Replace the outer `files_to_process.chunks(64)` loop plus inline embed drain with:

- A **`std::sync::mpsc::sync_channel`** of capacity `INDEX_CHANNEL_CAPACITY` (4), item type `Vec<Chunk>` (one embedding batch per message, size ≤ `INDEX_PIPELINE_CHUNK_BATCH` except possibly the final tail).
- A **std::thread** producer that runs **`rayon::par_iter`** over **all** `files_to_process` paths (remove `INDEX_FILE_PARALLEL_BATCH`).
- A **consumer** on the existing `spawn_blocking` thread: `for batch in chunk_rx { embed_write_chunk_batch(...) }`.
- **Chunk staging**: a **`Mutex<Vec<Chunk>>`** guarded buffer; each parallel task chunks a file, then under the lock appends chunks and **flushes** full `INDEX_PIPELINE_CHUNK_BATCH` slices to `send`. This overlaps **embedding** (consumer) with **chunking** (rayon) without materializing all chunks in memory before the first send (unlike a collect-then-send design).

### Why

Previously chunking for a 64-file wave finished before embedding that wave’s chunks, so ONNX and Rayon alternated. Overlap lets the embedder work while other files are still being parsed/chunked.

### Before / after data flow

**Before:** `par_iter(64 files) → Vec<Vec<Chunk>> → append pipeline → while ≥2048 embed` (sequential phases per wave).

**After:** `par_iter(all files) → (parallel) chunk → lock → push/flush send` **∥** `recv → embed → SQLite → usearch` with up to 4 batches buffered.

### Risks

- **Mutex contention** on the staging buffer: worst case serializes append after chunking; chunking remains parallel; acceptable trade-off vs holding two full copies of the codebase in RAM.
- **Error propagation**: consumer failure drops the receiver; producer `send` errors must map to `CgError::Index` (disconnect). **`join`**: map join failure to `CgError::Index` (panic). Return **consumer error first** if both fail.
- **Path lookup**: producer needs `Send` data; use **`Arc<Vec<SourceFile>>`** plus **`Arc<HashMap<String, usize>>`** (path → index) built before spawning—no duplicate file bodies.

### Constants

- Remove `INDEX_FILE_PARALLEL_BATCH`.
- Add `INDEX_CHANNEL_CAPACITY = 4`.
- Keep `INDEX_PIPELINE_CHUNK_BATCH = 2048`.

---

## Improvement 2 — SQLite pragmas (`store/mod.rs`)

### What

Add private `apply_pragmas(conn: &Connection) -> Result<(), CgError>` executing one `execute_batch`:

`journal_mode=WAL`, `synchronous=NORMAL`, `cache_size=-65536`, `temp_store=MEMORY`, `mmap_size=268435456`.

Call it immediately after **`Connection::open`** in `ChunkStore::open` (before `init_schema`).

### Why

WAL + `synchronous=NORMAL` improves bulk insert throughput; larger cache and mmap help reads (search) and FTS maintenance.

### Before / after

**Before:** default SQLite settings on every open.

**After:** same code paths, connection configured up front for all opens (index + search).

### Risks

- **`journal_mode`** returns a row; `execute_batch` still switches the DB file to WAL persistently.
- **`mmap_size` / `cache_size`**: memory hints; acceptable for a desktop indexing tool. Rebuild on corruption remains `cg index --force`.

---

## Improvement 3 — Vector index reserve + `len` (`usearch.rs`, `embed_write_chunk_batch`)

### What

- Implement **`VectorIndex::len`** → `index.size()`, **`is_empty`** → `len() == 0`.
- Document existing **`reserve`** as “total capacity hint including current size” (USearch semantics).
- In **`embed_write_chunk_batch`**, before the `add` loop: `vindex.reserve(vindex.len().checked_add(n).ok_or(...)?)?`.

### Why

Reduces reallocations inside HNSW during bulk `add`.

### Batch add API

**usearch 2.25.1** exposes per-vector `add` only; no `add_batch` in the public `Index` API. **No loop replacement.**

### Risks

- `reserve` over-shoot is harmless; under-shoot still grows dynamically.

---

## Improvement 4 — Avoid `String` clones at embed call site (`facade.rs` / `Embedder`)

### What

**Audit only.** `embed_write_chunk_batch` already uses `Vec<&str>` from `content.as_str()`; **`Embedder::embed` already takes `&[&str]`**. **No trait or call-site change.** Documented here as satisfied.

---

## Improvement 5 — Observability + `IndexStats` (`facade.rs`, `lib.rs`, `cg_cli`)

### What

- `Instant::now()` at start of **`blocking_index`** (before walk, per spec).
- After manifest diff: **`tracing::info!`** with `total_walked`, `to_index`, `to_remove`, `skipped_unchanged` where  
  `skipped_unchanged = current_hashes.len().saturating_sub(files_to_process.len())`  
  (walked files not in the re-index set; **not** mixing in `removed`, which are absent from `current_hashes`).
- After pipeline: log `files_indexed`, `chunks_written`, `elapsed_ms`.
- Extend **`IndexStats`**: `files_skipped: u64`, `elapsed_ms: u64` (both **documented**).
- **CLI** (`cg_cli/src/main.rs`): print indexed count, skipped count, chunks, elapsed seconds.

### Why

Operators can confirm incremental indexing and end-to-end duration.

### Risks

- **`IndexStats` is a public type** — adding fields is an API extension (call sites: only `facade` construction today). **cg_mcp** left unchanged per scope; it continues to expose prior JSON fields only.

---

## Configuration note (embedding batch size)

- Requested path **`config/default-settings.toml`** does not exist in this repo; comments will be added to **`crates/cg-core/default-settings.toml`** (the merged defaults source) and this plan notes the mapping.
- Add TOML comments describing tuning `embedding.batch_size` (32 / 64 / 128) and benchmarking with `cg index --force`.

---

## Tests

| Location | Test |
|----------|------|
| `store/mod.rs` `#[cfg(test)]` | `pragmas_are_applied_on_open` — temp DB, `PRAGMA journal_mode` → `wal` |
| `index/usearch.rs` `#[cfg(test)]` | `len` / `is_empty` / `reserve` smoke; isolated `Index::add` in the unit-test binary SIGSEGV’d on this toolchain, so “len after vectors exist” is asserted in `index_pipeline_perf` via `chunk_count` vs `VectorIndex::len` |
| `crates/cg-core/tests/index_pipeline_perf.rs` (new) | producer–consumer full index + search; incremental skip/reindex; concurrent search during index |

Integration tests use **`tempfile::TempDir`**, synthetic `.rs` trees, **`Settings::default()`** (or `load` if needed), **`#[tokio::test]`**, and **`CodeGrasp::new`**. They invoke **real fastembed** (may download weights on first CI run).

### Risks

- Slow CI / network for first model fetch.
- Concurrent search test: if **`database is locked`** appears, consider `PRAGMA busy_timeout` in a follow-up (out of scope unless test fails).

---

## Files touched (exhaustive)

| File | Action |
|------|--------|
| `PERF_PLAN.md` | This plan |
| `crates/cg-core/src/facade.rs` | Pipeline, stats, logging, embed reserve |
| `crates/cg-core/src/store/mod.rs` | `apply_pragmas`, unit test |
| `crates/cg-core/src/index/usearch.rs` | `len`, `is_empty`, rustdoc on `reserve`, tests |
| `crates/cg-core/src/lib.rs` | Doc example for `IndexStats` if needed |
| `crates/cg-core/default-settings.toml` | Comments on `batch_size` |
| `cg_cli/src/main.rs` | Index completion line |
| `crates/cg-core/tests/index_pipeline_perf.rs` | New integration tests |

**Not modified:** `cg_mcp/`, `chunker/`, `manifest/`, `walker/`, `ARCHITECTURE.md`, existing tests (only additions).

---

## Verification

- `cargo fmt --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- Grep: no new `.unwrap()` / `.expect()` in non-test `crates/`
