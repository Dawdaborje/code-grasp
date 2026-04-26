use assert_cmd::Command;

#[test]
fn models_download_help_exits_zero() {
    let mut cmd = Command::cargo_bin("cg").unwrap();
    cmd.arg("models")
        .arg("download")
        .arg("--help")
        .assert()
        .success();
}
