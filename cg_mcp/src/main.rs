//! CodeGrasp MCP server: JSON-RPC over stdio (logs on stderr only).

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use cg_core::{CodeGrasp, Settings};
use cg_proto::{
    ClearIndexInput, ClearIndexOutput, GetStatusInput, GetStatusOutput, IndexCodebaseInput,
    IndexCodebaseOutput, SearchCodeInput, SearchCodeOutput, SearchHit,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{
    ErrorData, ServerHandler, handler::server::router::tool::ToolRouter, model::CallToolResult,
    model::Content, serve_server, tool, tool_handler, tool_router,
};

/// Stateless MCP tool server backed by on-disk indexes.
#[derive(Clone)]
pub struct CodeGraspMcp {
    tool_router: ToolRouter<Self>,
}

impl CodeGraspMcp {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CodeGraspMcp {}

#[tool_router(router = tool_router)]
impl CodeGraspMcp {
    #[tool(
        name = "index_codebase",
        description = "Index a local codebase directory. Uses fastembed locally; writes `.code-grasp/` under the project."
    )]
    async fn index_codebase(
        &self,
        Parameters(input): Parameters<IndexCodebaseInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = PathBuf::from(input.path.trim());
        let settings = Settings::load(&root, None)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let cg = CodeGrasp::new(root, settings);
        let stats = cg
            .index(input.force)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out = IndexCodebaseOutput {
            message: format!(
                "Indexed {} files, wrote {} chunks.",
                stats.files_indexed, stats.chunks_written
            ),
            files_indexed: stats.files_indexed,
            chunks_written: stats.chunks_written,
        };
        Ok(CallToolResult::success(vec![Content::json(out)?]))
    }

    #[tool(
        name = "search_code",
        description = "Hybrid vector + BM25 search over an indexed codebase (path must already be indexed)."
    )]
    async fn search_code(
        &self,
        Parameters(input): Parameters<SearchCodeInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = PathBuf::from(input.path.trim());
        let settings = Settings::load(&root, None)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let cg = CodeGrasp::new(root, settings);
        let hits = cg
            .search(&input.query, input.limit)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let hits: Vec<SearchHit> = hits
            .into_iter()
            .map(|h| SearchHit {
                score: h.score,
                file_path: h.file_path,
                start_line: h.start_line,
                end_line: h.end_line,
                content: h.content,
            })
            .collect();
        let out = SearchCodeOutput { hits };
        Ok(CallToolResult::success(vec![Content::json(out)?]))
    }

    #[tool(
        name = "get_status",
        description = "Return indexing stats for a project path (chunk/file counts and last indexed time if known)."
    )]
    async fn get_status(
        &self,
        Parameters(input): Parameters<GetStatusInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = PathBuf::from(input.path.trim());
        let settings = Settings::load(&root, None)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let cg = CodeGrasp::new(root, settings);
        let st = cg
            .status()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out = GetStatusOutput {
            indexed: st.indexed,
            file_count: st.file_count,
            chunk_count: st.chunk_count,
            last_indexed: st.last_indexed.map(|t| t.to_string()),
        };
        Ok(CallToolResult::success(vec![Content::json(out)?]))
    }

    #[tool(
        name = "clear_index",
        description = "Delete `.code-grasp` index data (SQLite, USearch, manifest) for the given project path."
    )]
    async fn clear_index(
        &self,
        Parameters(input): Parameters<ClearIndexInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = PathBuf::from(input.path.trim());
        let settings = Settings::load(&root, None)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let cg = CodeGrasp::new(root, settings);
        cg.clear()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out = ClearIndexOutput {
            message: "Index cleared.".to_string(),
        };
        Ok(CallToolResult::success(vec![Content::json(out)?]))
    }
}

fn append_diag_file(line: &str) {
    let Ok(path) = std::env::var("CODEGRASP_MCP_LOG_FILE") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// When `RUST_LOG` is unset, Cursor (and other GUIs) often spawn the process with no directives;
/// `EnvFilter::from_default_env()` then suppresses almost all `tracing` output — use an explicit
/// default so startup and MCP lifecycle are visible on stderr.
fn init_mcp_logging() {
    let use_ansi = std::io::stderr().is_terminal();
    let default_directives =
        "code_grasp_mcp=info,cg_core=info,rmcp=info,warn,tokio=warn,runtime=warn";

    let filter = if std::env::var("RUST_LOG")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        match tracing_subscriber::EnvFilter::try_from_default_env() {
            Ok(f) => f,
            Err(e) => {
                let msg = format!(
                    "[code-grasp-mcp] RUST_LOG is set but invalid ({e}); using built-in default: {default_directives}"
                );
                let _ = writeln!(std::io::stderr(), "{msg}");
                append_diag_file(&msg);
                default_directives
                    .parse()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            }
        }
    } else {
        default_directives
            .parse()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    let banner = format!(
        "[code-grasp-mcp] v{} — MCP stdio server; logs on stderr (RUST_LOG unset → INFO for this binary). \
         Optional file: CODEGRASP_MCP_LOG_FILE=/path/to.log",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(std::io::stderr(), "{banner}");
    let _ = std::io::stderr().flush();
    append_diag_file(&banner);

    let res = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(use_ansi)
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
    if let Err(e) = res {
        let msg = format!("[code-grasp-mcp] tracing subscriber not installed (already initialized?): {e}");
        let _ = writeln!(std::io::stderr(), "{msg}");
        append_diag_file(&msg);
    }
}

fn print_help_or_version() -> bool {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "code-grasp-mcp {} — MCP server (stdio JSON-RPC).\n\
                     Started by the editor; not for interactive use.\n\
                     Logs go to stderr (default INFO when RUST_LOG is unset).\n\
                     Optional: RUST_LOG=debug, CODEGRASP_MCP_LOG_FILE=/tmp/code-grasp-mcp.log\n\
                     stdout must remain JSON-RPC only.",
                    env!("CARGO_PKG_VERSION")
                );
                return true;
            }
            "--version" | "-V" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                return true;
            }
            _ => {}
        }
    }
    false
}

fn main() -> anyhow::Result<()> {
    if print_help_or_version() {
        return Ok(());
    }
    init_mcp_logging();
    async_main()
}

#[tokio::main]
async fn async_main() -> anyhow::Result<()> {
    tracing::info!("tokio runtime up; entering MCP stdio transport");

    let service = CodeGraspMcp::new();
    let transport = rmcp::transport::stdio();
    let running = serve_server(service, transport)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "MCP handshake failed (initialize / transport)");
            let msg = format!("[code-grasp-mcp] MCP handshake failed: {e}");
            let _ = writeln!(std::io::stderr(), "{msg}");
            append_diag_file(&msg);
            anyhow::anyhow!("{e}")
        })?;

    tracing::info!("MCP session ready (initialize completed)");
    running
        .waiting()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "MCP session ended with error");
            let msg = format!("[code-grasp-mcp] MCP session error: {e}");
            let _ = writeln!(std::io::stderr(), "{msg}");
            append_diag_file(&msg);
            anyhow::anyhow!("{e}")
        })?;
    tracing::info!("MCP session shut down normally");
    Ok(())
}
