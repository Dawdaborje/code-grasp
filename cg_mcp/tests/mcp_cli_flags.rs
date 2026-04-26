use assert_cmd::Command;

#[test]
fn version_exits_zero() {
    Command::cargo_bin("code-grasp-mcp")
        .unwrap()
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn help_exits_zero() {
    Command::cargo_bin("code-grasp-mcp")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
