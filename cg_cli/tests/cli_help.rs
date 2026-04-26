use assert_cmd::Command;

#[test]
fn cg_help_exits_zero() {
    let mut cmd = Command::cargo_bin("cg").unwrap();
    cmd.arg("--help").assert().success();
}
