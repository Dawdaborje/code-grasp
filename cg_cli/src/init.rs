//! `cg init`: scaffold `.code-grasp/` and merge CodeGrasp MCP into Cursor, VS Code, and Zed configs.

use std::path::Path;

use cg_core::CgError;
use cg_core::paths;
use serde_json::{Map, Value, json};

const SERVER_KEY: &str = "code-grasp";

/// Path to `code-grasp-mcp` for editor MCP configs.
///
/// GUIs often spawn MCP without your shell `PATH`, so `~/.local/bin` may be missing.
/// Override with `CODEGRASP_MCP_COMMAND` if needed.
fn resolve_code_grasp_mcp_command() -> String {
    if let Ok(p) = std::env::var("CODEGRASP_MCP_COMMAND") {
        let p = p.trim();
        if !p.is_empty() {
            return p.to_string();
        }
    }
    if let Ok(o) = std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v code-grasp-mcp 2>/dev/null")
        .output()
        && o.status.success()
    {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let candidate = Path::new(&home).join(".local/bin/code-grasp-mcp");
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }

    "code-grasp-mcp".to_string()
}

fn env_rust_log() -> Value {
    json!({
        "RUST_LOG": "info"
    })
}

fn cursor_entry(command: &str) -> Value {
    json!({
        "command": command,
        "args": Value::Array(vec![]),
        "env": env_rust_log(),
    })
}

/// VS Code / Copilot workspace MCP: `.vscode/mcp.json` — see
/// <https://code.visualstudio.com/docs/copilot/customization/mcp-servers>
fn vscode_entry(command: &str) -> Value {
    json!({
        "type": "stdio",
        "command": command,
        "args": Value::Array(vec![]),
        "env": env_rust_log(),
    })
}

/// Zed project settings: `.zed/settings.json` — top-level `context_servers` with `source: "custom"`.
fn zed_entry(command: &str) -> Value {
    json!({
        "source": "custom",
        "command": command,
        "args": Value::Array(vec![]),
        "env": env_rust_log(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeOutcome {
    Wrote,
    SkippedExisting,
}

fn write_pretty_json(path: &Path, value: &Value) -> Result<(), CgError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(CgError::Io)?;
    }
    let out = serde_json::to_string_pretty(value)
        .map_err(|e| CgError::Config(format!("serialize {}: {e}", path.display())))?;
    std::fs::write(path, out).map_err(CgError::Io)?;
    Ok(())
}

/// Cursor: top-level `mcpServers`.
fn merge_cursor_mcp(path: &Path, command: &str, force: bool) -> Result<MergeOutcome, CgError> {
    let mut doc = if path.is_file() {
        let raw = std::fs::read_to_string(path).map_err(CgError::Io)?;
        serde_json::from_str::<Value>(&raw)
            .map_err(|e| CgError::Config(format!("{}: invalid JSON: {e}", path.display())))?
    } else {
        json!({ "mcpServers": Value::Object(Map::new()) })
    };

    let root = doc.as_object_mut().ok_or_else(|| {
        CgError::Config(format!(
            "{}: expected top-level JSON object",
            path.display()
        ))
    })?;

    if !root.contains_key("mcpServers") {
        root.insert("mcpServers".to_string(), Value::Object(Map::new()));
    }

    let servers = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            CgError::Config(format!(
                "{}: mcpServers must be a JSON object",
                path.display()
            ))
        })?;

    if servers.contains_key(SERVER_KEY) && !force {
        return Ok(MergeOutcome::SkippedExisting);
    }

    servers.insert(SERVER_KEY.to_string(), cursor_entry(command));
    write_pretty_json(path, &doc)?;
    Ok(MergeOutcome::Wrote)
}

/// VS Code: top-level `servers` in `.vscode/mcp.json`.
fn merge_vscode_mcp(path: &Path, command: &str, force: bool) -> Result<MergeOutcome, CgError> {
    let mut doc = if path.is_file() {
        let raw = std::fs::read_to_string(path).map_err(CgError::Io)?;
        serde_json::from_str::<Value>(&raw)
            .map_err(|e| CgError::Config(format!("{}: invalid JSON: {e}", path.display())))?
    } else {
        json!({ "servers": Value::Object(Map::new()) })
    };

    let root = doc.as_object_mut().ok_or_else(|| {
        CgError::Config(format!(
            "{}: expected top-level JSON object",
            path.display()
        ))
    })?;

    if !root.contains_key("servers") {
        root.insert("servers".to_string(), Value::Object(Map::new()));
    }

    let servers = root
        .get_mut("servers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            CgError::Config(format!("{}: servers must be a JSON object", path.display()))
        })?;

    if servers.contains_key(SERVER_KEY) && !force {
        return Ok(MergeOutcome::SkippedExisting);
    }

    servers.insert(SERVER_KEY.to_string(), vscode_entry(command));
    write_pretty_json(path, &doc)?;
    Ok(MergeOutcome::Wrote)
}

/// Zed: `context_servers` inside `.zed/settings.json`.
fn merge_zed_context_server(
    path: &Path,
    command: &str,
    force: bool,
) -> Result<MergeOutcome, CgError> {
    let mut doc = if path.is_file() {
        let raw = std::fs::read_to_string(path).map_err(CgError::Io)?;
        serde_json::from_str::<Value>(&raw)
            .map_err(|e| CgError::Config(format!("{}: invalid JSON: {e}", path.display())))?
    } else {
        Value::Object(Map::new())
    };

    let root = doc.as_object_mut().ok_or_else(|| {
        CgError::Config(format!(
            "{}: expected top-level JSON object",
            path.display()
        ))
    })?;

    if !root.contains_key("context_servers") {
        root.insert("context_servers".to_string(), Value::Object(Map::new()));
    }

    let ctx = root
        .get_mut("context_servers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            CgError::Config(format!(
                "{}: context_servers must be a JSON object",
                path.display()
            ))
        })?;

    if ctx.contains_key(SERVER_KEY) && !force {
        return Ok(MergeOutcome::SkippedExisting);
    }

    ctx.insert(SERVER_KEY.to_string(), zed_entry(command));
    write_pretty_json(path, &doc)?;
    Ok(MergeOutcome::Wrote)
}

/// Create project data dir and merge CodeGrasp stdio MCP into Cursor, VS Code, and Zed configs.
pub fn run(project_root: &Path, no_mcp: bool, force: bool) -> Result<(), CgError> {
    let data_dir = paths::project_data_dir(project_root);
    std::fs::create_dir_all(&data_dir).map_err(CgError::Io)?;

    if no_mcp {
        println!("Initialized `{}`.", data_dir.display());
        println!("Skipped MCP configs for Cursor, VS Code, and Zed (--no-mcp).");
        return Ok(());
    }

    let mcp_command = resolve_code_grasp_mcp_command();

    let cursor_path = project_root.join(".cursor").join("mcp.json");
    let vscode_path = project_root.join(".vscode").join("mcp.json");
    let zed_path = project_root.join(".zed").join("settings.json");

    let cursor = merge_cursor_mcp(&cursor_path, &mcp_command, force)?;
    let vscode = merge_vscode_mcp(&vscode_path, &mcp_command, force)?;
    let zed = merge_zed_context_server(&zed_path, &mcp_command, force)?;

    println!("Initialized `{}`.", data_dir.display());

    for (label, path, outcome) in [
        ("Cursor", cursor_path.as_path(), cursor),
        ("VS Code", vscode_path.as_path(), vscode),
        ("Zed", zed_path.as_path(), zed),
    ] {
        match outcome {
            MergeOutcome::Wrote => println!(
                "Merged MCP server `{SERVER_KEY}` into {} ({label}).",
                path.display()
            ),
            MergeOutcome::SkippedExisting => println!(
                "MCP server `{SERVER_KEY}` already in {}; skipped (use --force for {label}).",
                path.display()
            ),
        }
    }

    if mcp_command == "code-grasp-mcp" {
        println!(
            "Note: `code-grasp-mcp` was not resolved to a full path during init; editors may fail to spawn it. \
             Install to e.g. ~/.local/bin, fix PATH for the GUI, or set CODEGRASP_MCP_COMMAND and run `cg init --force`."
        );
    }

    Ok(())
}
