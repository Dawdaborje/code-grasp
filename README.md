# CodeGrasp

CodeGrasp indexes a local codebase into **SQLite (FTS5)** plus a **usearch** vector index, then answers **hybrid** (lexical + vector) queries. Use it from the terminal (`cg`) or from AI agents via **MCP** (`code-grasp-mcp`). Defaults use **fastembed** on your machine; optional **OpenAI** embeddings are available behind a feature flag.

## Requirements

- Rust toolchain (2024 edition) as pinned by the repo
- For default embeddings: disk space for the fastembed model cache (first run may download)

## Install (from source)

```bash
cargo install --path cg_cli
cargo install --path cg_mcp
```

Binaries: **`cg`** (package `code-grasp`) and **`code-grasp-mcp`**.

On Linux you can instead run **`./scripts/install_linux.sh`** from a clone: it builds release binaries and copies **`cg`** and **`code-grasp-mcp`** into **`~/.local/bin`** (or **`BIN_DIR`**). MCP is **stdio** — your editor spawns that binary; **do not** run it as a systemd service for normal MCP.

### Feature flags (build time)

- **`--features lsp`** on the CLI: forwards to `cg_core/lsp` (currently AST-equivalent; reserved for future LSP chunking).
- **`openai`**: build `cg_core` with `openai` and set `embedding.provider` appropriately; API key via **`CODEGRASP_OPENAI_API_KEY`** (legacy typo **`CODEGASP_OPENAI_API_KEY`** is also accepted in the OpenAI embedder).

## Usage

### CLI (`cg`)

All commands take a **project root** path (directory containing your sources). Index data is written to **`<project>/.code-grasp/`** (SQLite, USearch index, manifest, optional project `config.toml`).

| Command | Purpose |
|---------|---------|
| `cg index <path>` | Walk, chunk, embed, and persist the index. |
| `cg search <path> <query>` | Hybrid search; optional `--limit N` (default **10**). |
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
max_file_size_bytes = 1048576
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
| `search_code` | Search (`path`, `query`, optional `limit`). |
| `get_status` | Counters and indexed flag for `path`. |
| `clear_index` | Remove index data for `path`. |

Payload shapes are defined in the **`cg_proto`** crate (also used by the CLI internally where applicable).

**Logging:** `tracing` writes to **stderr** only so **stdout** stays JSON-RPC-clean. Use **`RUST_LOG`** for verbosity.

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
