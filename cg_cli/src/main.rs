//! CodeGrasp CLI (`cg`): index and search local codebases.

mod init;

use std::path::PathBuf;
use std::process::ExitCode;

use cg_core::embedder::prefetch_fastembed_model_weights;
use cg_core::{CgError, CodeGrasp, Settings};
use clap::{ArgAction, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "cg",
    version,
    about = "CodeGrasp — local semantic codebase search",
    disable_help_subcommand = true
)]
struct Cli {
    /// More detail: logging to stderr (`-v`≈info, `-vv`≈debug, `-vvv`≈trace) when `RUST_LOG` is unset;
    /// on failure, print full error `Debug` and enable `RUST_BACKTRACE` for panics if not already set.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download Hugging Face weight/tokenizer files for the configured fastembed model
    #[command(subcommand)]
    Models(ModelsCommands),
    /// Create `.code-grasp/` and merge CodeGrasp MCP into Cursor, VS Code, and Zed project configs
    Init {
        /// Project root (directory containing sources)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Do not write or update editor MCP configs (`.cursor/mcp.json`, `.vscode/mcp.json`, `.zed/settings.json`)
        #[arg(long)]
        no_mcp: bool,
        /// Replace an existing `code-grasp` MCP entry if present
        #[arg(long)]
        force: bool,
    },
    /// Index a codebase directory
    Index {
        /// Project root to index
        path: PathBuf,
        /// Drop existing index and rebuild from scratch
        #[arg(long)]
        force: bool,
        /// Reserved for LSP-enriched chunking (requires `lsp` feature build)
        #[arg(long)]
        lsp: bool,
    },
    /// Search an indexed codebase
    Search {
        path: PathBuf,
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show indexing status
    Status { path: PathBuf },
    /// Remove index data for a project
    Clear { path: PathBuf },
    /// Print merged configuration (defaults + files + env) for the given project root
    Config {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ModelsCommands {
    /// Fetch ONNX + tokenizer files (HTTP only; does not load ONNX — avoids that crash path)
    Download {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    apply_verbose_env(&cli);
    init_tracing(cli.verbose);

    match run(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            print_cg_error(&e, cli.verbose);
            exit_for_cg_error(&e)
        }
    }
}

/// If the user asked for verbosity, make panics (e.g. in native embed deps) show a stack trace
/// unless they already configured backtraces.
fn apply_verbose_env(cli: &Cli) {
    if cli.verbose == 0 {
        return;
    }
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        // SAFETY: `set_var` is only unsafe in Rust 2024+ due to concurrent env mutation on some
        // platforms. Here we run once at startup before any `.await` in this binary, so no other
        // library threads should read `RUST_BACKTRACE` concurrently with this write.
        unsafe {
            std::env::set_var("RUST_BACKTRACE", "full");
        }
    }
}

fn init_tracing(verbose: u8) {
    let rust_log_set = std::env::var("RUST_LOG")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let filter = if rust_log_set {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(match verbose {
                0 => "warn",
                1 => "info",
                2 => "debug",
                _ => "trace",
            })
        })
    } else {
        let level = match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        };
        // Narrow scope so dependency noise stays manageable until -vvv.
        EnvFilter::new(format!(
            "{level},cg_core={level},code_grasp={level},hyper=warn,reqwest=warn"
        ))
    };

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init();
}

fn print_cg_error(e: &CgError, verbose: u8) {
    eprintln!("{e}");
    if verbose > 0 {
        eprintln!("--- error (verbose Debug) ---\n{e:?}");
        eprintln!("--- end ---");
        if verbose >= 2 {
            eprintln!(
                "hint: for native crashes (e.g. embedding), keep `RUST_BACKTRACE=full` and try `-vv` logging; `RUST_LOG` overrides `-v` levels."
            );
        }
    }
}

fn exit_for_cg_error(e: &CgError) -> ExitCode {
    match e {
        CgError::NotIndexed { .. }
        | CgError::Config(_)
        | CgError::UnsupportedLanguage(_)
        | CgError::Embedding(_) => ExitCode::from(1),
        _ => ExitCode::from(2),
    }
}

async fn run(cli: &Cli) -> Result<(), CgError> {
    match &cli.command {
        Commands::Models(ModelsCommands::Download { path }) => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            if settings.embedding.provider != "fastembed" {
                return Err(CgError::Config(
                    "`cg models download` only applies when `[embedding] provider = \"fastembed\"`"
                        .into(),
                ));
            }
            let model_id = settings.embedding.model.clone();
            let show_progress = true;
            let paths = tokio::task::spawn_blocking(move || {
                prefetch_fastembed_model_weights(&model_id, show_progress)
            })
            .await
            .map_err(|e| CgError::State(format!("models download join: {e}")))?;
            let paths = paths?;
            println!(
                "Downloaded {} artifact(s) for `{}`. Cache root follows HF_HOME or `{}` (same rules as indexing).",
                paths.len(),
                settings.embedding.model,
                cg_core::paths::models_cache_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(unknown)".into())
            );
            println!(
                "Note: `cg index` still loads ONNX and may segfault in native code; this command only prefetches files."
            );
        }
        Commands::Init {
            path,
            no_mcp,
            force,
        } => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            init::run(&root, *no_mcp, *force)?;
        }
        Commands::Index { path, force, lsp } => {
            #[cfg(not(feature = "lsp"))]
            if *lsp {
                return Err(CgError::Config(
                    "rebuild with: cargo build -p code-grasp --features lsp".into(),
                ));
            }
            #[cfg(feature = "lsp")]
            if *lsp {
                tracing::warn!(
                    "LSP enrichment is not fully wired yet; indexing uses AST chunking only."
                );
            }

            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            let cg = CodeGrasp::new(root.clone(), settings);

            let pb = ProgressBar::new_spinner();
            let style = ProgressStyle::with_template("{spinner:.green} {wide_msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner());
            pb.set_style(style);
            pb.enable_steady_tick(std::time::Duration::from_millis(120));
            pb.set_message("Indexing… (first run may download an embedding model)");

            let res = cg.index(*force).await;
            pb.finish_and_clear();
            let stats = res?;
            let secs = stats.elapsed_ms as f64 / 1000.0;
            println!(
                "Done. Indexed {} file(s) ({} unchanged skipped), {} chunk(s) in {:.1}s.",
                stats.files_indexed, stats.files_skipped, stats.chunks_written, secs
            );
        }
        Commands::Search { path, query, limit } => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            let cg = CodeGrasp::new(root, settings);
            let hits = cg.search(query, *limit).await?;
            for h in hits {
                println!(
                    "[{:.2}] {}:{}-{}",
                    h.score, h.file_path, h.start_line, h.end_line
                );
                for line in h.content.lines().take(12) {
                    println!("  {line}");
                }
                if h.content.lines().count() > 12 {
                    println!("  …");
                }
                println!();
            }
        }
        Commands::Status { path } => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            let cg = CodeGrasp::new(root, settings);
            let st = cg.status().await?;
            println!("indexed: {}", st.indexed);
            println!("files: {}", st.file_count);
            println!("chunks: {}", st.chunk_count);
            if let Some(ts) = st.last_indexed {
                println!("last_indexed_unix: {ts}");
            }
        }
        Commands::Clear { path } => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            let cg = CodeGrasp::new(root, settings);
            cg.clear().await?;
            println!("Cleared index for project.");
        }
        Commands::Config { path } => {
            let root = path.canonicalize().map_err(CgError::Io)?;
            let settings = Settings::load(&root, None)?;
            let s = toml::to_string_pretty(&settings)
                .map_err(|e| CgError::Config(format!("serialize settings: {e}")))?;
            println!("{s}");
        }
    }
    Ok(())
}
