use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

fn mcp_command_basename(v: &Value, path: &[&str]) -> String {
    let mut cur: &Value = v;
    for p in path {
        cur = &cur[*p];
    }
    let s = cur.as_str().expect("command string");
    Path::new(s)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(s)
        .to_string()
}

fn cg() -> Command {
    Command::cargo_bin("cg").unwrap()
}

#[test]
fn init_creates_code_grasp_and_all_editor_mcp_configs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    cg().arg("init").current_dir(root).assert().success();

    assert!(root.join(".code-grasp").is_dir());

    let cursor_path = root.join(".cursor").join("mcp.json");
    assert!(cursor_path.is_file());
    let raw = fs::read_to_string(&cursor_path).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        mcp_command_basename(&v, &["mcpServers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
    assert_eq!(v["mcpServers"]["code-grasp"]["args"], Value::Array(vec![]));
    assert_eq!(v["mcpServers"]["code-grasp"]["env"]["RUST_LOG"], "info");

    let vscode_path = root.join(".vscode").join("mcp.json");
    assert!(vscode_path.is_file());
    let vs = fs::read_to_string(&vscode_path).unwrap();
    let vvs: Value = serde_json::from_str(&vs).unwrap();
    assert_eq!(vvs["servers"]["code-grasp"]["type"], "stdio");
    assert_eq!(
        mcp_command_basename(&vvs, &["servers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
    assert_eq!(vvs["servers"]["code-grasp"]["env"]["RUST_LOG"], "info");

    let zed_path = root.join(".zed").join("settings.json");
    assert!(zed_path.is_file());
    let zs = fs::read_to_string(&zed_path).unwrap();
    let vz: Value = serde_json::from_str(&zs).unwrap();
    assert_eq!(vz["context_servers"]["code-grasp"]["source"], "custom");
    assert_eq!(
        mcp_command_basename(&vz, &["context_servers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
    assert_eq!(
        vz["context_servers"]["code-grasp"]["env"]["RUST_LOG"],
        "info"
    );
}

#[test]
fn init_merges_preserving_other_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cursor = root.join(".cursor");
    let vscode = root.join(".vscode");
    fs::create_dir_all(&cursor).unwrap();
    fs::create_dir_all(&vscode).unwrap();
    fs::write(
        cursor.join("mcp.json"),
        r#"{
  "mcpServers": {
    "other": {
      "command": "other-mcp",
      "args": ["--foo"]
    }
  }
}"#,
    )
    .unwrap();
    fs::write(
        vscode.join("mcp.json"),
        r#"{
  "servers": {
    "other-vs": {
      "type": "stdio",
      "command": "other",
      "args": []
    }
  }
}"#,
    )
    .unwrap();

    cg().arg("init").current_dir(root).assert().success();

    let raw = fs::read_to_string(cursor.join("mcp.json")).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["mcpServers"]["other"]["command"], "other-mcp");
    assert_eq!(
        mcp_command_basename(&v, &["mcpServers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );

    let vs = fs::read_to_string(vscode.join("mcp.json")).unwrap();
    let vvs: Value = serde_json::from_str(&vs).unwrap();
    assert_eq!(vvs["servers"]["other-vs"]["command"], "other");
    assert_eq!(
        mcp_command_basename(&vvs, &["servers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
}

#[test]
fn init_merges_zed_preserving_unrelated_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let zed = root.join(".zed");
    fs::create_dir_all(&zed).unwrap();
    fs::write(
        zed.join("settings.json"),
        r#"{
  "theme": "One Dark",
  "context_servers": {
    "other": {
      "source": "custom",
      "command": "other-mcp",
      "args": []
    }
  }
}"#,
    )
    .unwrap();

    cg().arg("init").current_dir(root).assert().success();

    let zs = fs::read_to_string(zed.join("settings.json")).unwrap();
    let vz: Value = serde_json::from_str(&zs).unwrap();
    assert_eq!(vz["theme"], "One Dark");
    assert_eq!(vz["context_servers"]["other"]["command"], "other-mcp");
    assert_eq!(vz["context_servers"]["code-grasp"]["source"], "custom");
}

#[test]
fn init_no_mcp_skips_all_editor_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    cg().args(["init", "--no-mcp"])
        .current_dir(root)
        .assert()
        .success();

    assert!(root.join(".code-grasp").is_dir());
    assert!(!root.join(".cursor").join("mcp.json").exists());
    assert!(!root.join(".vscode").join("mcp.json").exists());
    assert!(!root.join(".zed").join("settings.json").exists());
}

#[test]
fn init_second_run_skips_all_without_force() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    cg().arg("init").current_dir(root).assert().success();
    let c1 = fs::read_to_string(root.join(".cursor").join("mcp.json")).unwrap();
    let v1 = fs::read_to_string(root.join(".vscode").join("mcp.json")).unwrap();
    let z1 = fs::read_to_string(root.join(".zed").join("settings.json")).unwrap();

    cg().arg("init").current_dir(root).assert().success();
    let c2 = fs::read_to_string(root.join(".cursor").join("mcp.json")).unwrap();
    let v2 = fs::read_to_string(root.join(".vscode").join("mcp.json")).unwrap();
    let z2 = fs::read_to_string(root.join(".zed").join("settings.json")).unwrap();

    assert_eq!(c1, c2);
    assert_eq!(v1, v2);
    assert_eq!(z1, z2);
}

#[test]
fn init_force_rewrites_all_editor_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    cg().arg("init").current_dir(root).assert().success();

    let cursor_path = root.join(".cursor").join("mcp.json");
    let vscode_path = root.join(".vscode").join("mcp.json");
    let zed_path = root.join(".zed").join("settings.json");

    fs::write(
        &cursor_path,
        r#"{"mcpServers":{"code-grasp":{"command":"custom-cursor","args":["x"]}}}"#,
    )
    .unwrap();
    fs::write(
        &vscode_path,
        r#"{"servers":{"code-grasp":{"type":"stdio","command":"custom-vs","args":[]}}}"#,
    )
    .unwrap();
    fs::write(
        &zed_path,
        r#"{"context_servers":{"code-grasp":{"source":"custom","command":"custom-zed","args":[]}}}"#,
    )
    .unwrap();

    cg().args(["init", "--force"])
        .current_dir(root)
        .assert()
        .success();

    let v: Value = serde_json::from_str(&fs::read_to_string(&cursor_path).unwrap()).unwrap();
    assert_eq!(
        mcp_command_basename(&v, &["mcpServers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );

    let vvs: Value = serde_json::from_str(&fs::read_to_string(&vscode_path).unwrap()).unwrap();
    assert_eq!(
        mcp_command_basename(&vvs, &["servers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
    assert_eq!(vvs["servers"]["code-grasp"]["type"], "stdio");

    let vz: Value = serde_json::from_str(&fs::read_to_string(&zed_path).unwrap()).unwrap();
    assert_eq!(
        mcp_command_basename(&vz, &["context_servers", "code-grasp", "command"]),
        "code-grasp-mcp"
    );
}
