# Search benchmarks

Compares **`cg search`** (full subprocess: settings load, query embedding, hybrid index) against **`rg -F`** with globs approximating CodeGrasp’s indexed file types (see `crates/cg-core/src/walker/gitignore.rs`).

## Run

From the **code-grasp** repository root:

```bash
export BENCH_ROOT=/path/to/your/repo   # optional; default is arvora os-distro path in script
export BENCH_ITER=30                   # optional; default 30
export BENCH_RELEASE=1                 # optional; 0 = debug build for `cg`

python3 scripts/bench_search.py
```

Outputs:

- `bench/results-<UTC>.md` — summary table
- `bench/results-<UTC>.csv` — raw per-iteration times

## Interpretation

- **`cg_search`** times include **cold-ish** `cargo run --release -p code-grasp -- search …` each iteration (realistic CLI use, pessimistic for “warm long-running daemon”).
- **`rg`** uses `--iglob` for each built-in extension plus well-known extensionless names; it is **not** identical to the chunker’s logic but is stable for A/B timing.
- On small corpora, **ripgrep is often much faster in wall-clock** than spawning `cg` + ONNX per query; gains from indexing show up more when comparing **semantic** retrieval quality or **large** trees, not raw `rg -F` latency.
