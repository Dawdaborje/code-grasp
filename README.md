# CodeGrasp

CodeGrasp indexes a local codebase into **SQLite (FTS5)** plus a **usearch** vector index, then answers **hybrid** (lexical + vector) queries. Use it from the terminal (`cg`) or from AI agents via **MCP** (`code-grasp-mcp`). Defaults use **fastembed** on your machine; optional **OpenAI** embeddings are available behind a feature flag.

## Requirements

- Rust toolchain (2024 edition) as pinned by the repo
- For default embeddings: disk space for the fastembed model cache (first run may download)

**Model download:** the first `cg index` (or `cg models download`) fetches the public ONNX model from Hugging Face into `~/.cache/code-grasp/models` (or `HF_HOME`), resuming interrupted transfers. If a token saved by `huggingface-cli login` (`~/.cache/huggingface/token`) is expired or revoked, the Hub rejects it even for public models; CodeGrasp then retries anonymously and logs a warning. Log in again or delete that file to silence it.

## Platforms (cross-platform)

The **Rust workspace** is intended to build and run on **Linux, macOS, and Windows** (paths use `std::path`; SQLite is bundled via `rusqlite`; ONNX Runtime comes in via `ort` / fastembed defaults). CI or per-OS quirks can still appear around native stacks (OpenSSL/TLS, ORT binaries, AV scanners on Windows).

- **Default embeddings:** CPU ONNX (`ort-bundled`). Optional **`ort-cuda`** is **NVIDIA-only** (not available on Apple Silicon or typical AMD iGPU).
- **GPU elsewhere:** not wired yet; see future work for DirectML / ROCm / CoreML.

## Install (from source)

```bash
cargo install --path cg_cli
cargo install --path cg_mcp
```

Binaries: **`cg`** (package `code-grasp`) and **`code-grasp-mcp`**.

### Install scripts (from a clone)

Each script builds **release** binaries and copies them into **`BIN_DIR`** if set, otherwise **`~/.local/bin`** (macOS/Linux) or **`%USERPROFILE%\.local\bin`** (Windows). Override with `BIN_DIR` / `$env:BIN_DIR`.

| OS | Command |
|----|---------|
| **Linux** | `bash scripts/install_linux.sh` |
| **macOS** | `bash scripts/install_mac.sh` |
| **Windows** | `pwsh -ExecutionPolicy Bypass -File .\scripts\install_windows.ps1` |

MCP is **stdio** — your editor spawns **`code-grasp-mcp`**; **do not** run it as a systemd service on Linux for normal MCP.

**System ONNX (optional):** set **`CODEGRASP_ORT_DYNAMIC=1`** when running the install script, then point **`ORT_DYLIB_PATH`** at a compatible `libonnxruntime.so` / `libonnxruntime.dylib` / `onnxruntime.dll` (see script comments and `cg_core` features).

### Feature flags (build time)

- **`--features lsp`** on the CLI: forwards to `cg_core/lsp` (currently AST-equivalent; reserved for future LSP chunking).
- **`openai`**: build `cg_core` with `openai` and set `embedding.provider` appropriately; API key via **`CODEGRASP_OPENAI_API_KEY`** (legacy typo **`CODEGASP_OPENAI_API_KEY`** is also accepted in the OpenAI embedder).

## Usage

### CLI (`cg`)

All commands take a **project root** path (directory containing your sources). Index data is written to **`<project>/.code-grasp/`** (SQLite, USearch index, manifest, optional project `config.toml`).

| Command | Purpose |
|---------|---------|
| `cg index <path>` | Walk, chunk, embed, and persist the index. |
| `cg init [path]` | Create `.code-grasp/` and add the `code-grasp` MCP server to `.cursor/mcp.json`, `.vscode/mcp.json`, and `.zed/settings.json` (`--no-mcp` to skip, `--force` to overwrite). |
| `cg search <path> <query>` | Hybrid search; optional `--limit N` (default: `[indexing] default_limit`, **10**). |
| `cg status <path>` | Show whether the project is indexed and basic counts. |
| `cg clear <path>` | Delete this project’s `.code-grasp` data (SQLite + vectors + manifest). |
| `cg config [path]` | Print merged **TOML** configuration (defaults + files + env). Default `path` is **`.`**. |

Examples:

```bash
cd /path/to/your/repo
cg index .
cg search . "where is authentication handled?"
cg search . "config reload" --limit 5
cg status .
cg clear .
cg config .
```

**Exit codes** (CLI): **0** success; **1** user-facing errors (e.g. not indexed, bad config, embedding failure); **2** unexpected/internal failures.

**Logging:** set **`RUST_LOG`** (e.g. `RUST_LOG=info`) for `tracing` output on **stderr**.

### Configuration (TOML + environment)

Merge order (later wins): built-in defaults → **`~/.config/code-grasp/config.toml`** → **`<project>/.code-grasp/config.toml`** → environment → CLI overlay where applicable.

Default tables (see also `crates/cg-core/default-settings.toml`):

```toml
[embedding]
provider = "fastembed"
model = "BAAI/bge-small-en-v1.5"
batch_size = 32

[indexing]
max_file_size_bytes = 10485760
min_chunk_tokens = 20
max_chunk_tokens = 512
default_limit = 10

[lsp]
rust_analyzer_path = "rust-analyzer"
pyright_path = "pyright"
tsserver_path = "typescript-language-server"
```

**Environment:** variables prefixed with **`CODEGRASP_`** are merged into settings; use **`__`** for nesting (e.g. `CODEGRASP_EMBEDDING__BATCH_SIZE=64`).

### MCP (`code-grasp-mcp`)

Run the server on **stdio** (your MCP client configures this command):

```bash
code-grasp-mcp
```

**Tools** (stable names for agents):

| Tool | Role |
|------|------|
| `index_codebase` | Index a directory (`path`, optional `force`). |
| `search_code` | Search (`path`, `query`, optional `limit`; default `[indexing] default_limit`). |
| `get_status` | Counters and indexed flag for `path`. |
| `clear_index` | Remove index data for `path`. |

Payload shapes are defined in the **`cg_proto`** crate (also used by the CLI internally where applicable).

**Logging:** `tracing` writes to **stderr** only so **stdout** stays JSON-RPC-clean. Use **`RUST_LOG`** for verbosity.

**Editor configs are portable:** `cg init` writes the command as plain **`code-grasp-mcp`**, never an absolute path, so the generated `.cursor/`, `.vscode/`, and `.zed/` files can be committed and work for anyone who clones the repo. Each user just needs `code-grasp-mcp` on the `PATH` their editor sees (the install scripts put it in `~/.local/bin`). `CODEGRASP_MCP_COMMAND` overrides the command for a local, uncommitted setup.

### Rust library (`cg_core`)

For programmatic use (async, `tokio`), see the **`cg_core`** crate documentation:

```bash
cargo doc -p cg_core -p cg_proto --no-deps --open
```

The root type is **`CodeGrasp`**: load **`Settings::load`**, then **`index`**, **`search`**, **`status`**, **`clear`**. The high-level index path currently requires **`embedding.provider = "fastembed"`** in settings.

## Quick start (copy-paste)

```bash
cd /path/to/your/repo
cg index .
cg search . "where is authentication handled?"
cg status .
```

Add **`.code-grasp/`** to `.gitignore` if you do not want index artifacts committed (this repo’s `.gitignore` already lists it).

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See **[ARCHITECTURE.md](ARCHITECTURE.md)** for pipeline details and **[BACKLOG.md](BACKLOG.md)** for planned work.

## License

MIT (see workspace `Cargo.toml`).
