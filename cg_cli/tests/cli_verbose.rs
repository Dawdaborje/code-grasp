use assert_cmd::Command;

fn cg() -> Command {
    Command::cargo_bin("cg").unwrap()
}

#[test]
fn verbose_on_error_includes_debug_repr() {
    let tmp = tempfile::tempdir().unwrap();
    let output = cg()
        .args(["-v", "search", ".", "anything"])
        .current_dir(tmp.path())
        .output()
        .expect("run cg");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verbose Debug"),
        "expected verbose error banner, got:\n{stderr}"
    );
    assert!(
        stderr.contains("NotIndexed") || stderr.to_lowercase().contains("not indexed"),
        "expected NotIndexed in Debug, got:\n{stderr}"
    );
}

#[test]
fn non_verbose_error_is_display_only() {
    let tmp = tempfile::tempdir().unwrap();
    let output = cg()
        .args(["search", ".", "anything"])
        .current_dir(tmp.path())
        .output()
        .expect("run cg");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("verbose Debug"),
        "did not expect verbose block, got:\n{stderr}"
    );
}
