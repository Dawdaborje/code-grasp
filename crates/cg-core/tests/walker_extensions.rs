//! Walker indexes shell, markdown, well-known extensionless names, and `extra_extensions`.

use std::fs;

use cg_core::walker::walk_sources;
use tempfile::tempdir;

#[test]
fn indexes_shell_makefile_and_arch_package_list() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("build.sh"), "#!/bin/sh\necho hi\n").unwrap();
    fs::write(root.join("Makefile"), "all:\n\ttrue\n").unwrap();
    fs::write(root.join("packages.x86_64"), "base\nlinux\n").unwrap();
    fs::write(root.join("readme.md"), "# doc\n").unwrap();

    let files = walk_sources(root, 1024 * 1024, &[]).unwrap();
    let mut names: Vec<String> = files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "Makefile".to_string(),
            "build.sh".to_string(),
            "packages.x86_64".to_string(),
            "readme.md".to_string(),
        ]
    );
}

#[test]
fn extra_extensions_merges_unknown_suffix() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("data.custom"), "x\n").unwrap();

    assert!(walk_sources(root, 1024 * 1024, &[]).unwrap().is_empty());

    let extra = vec!["custom".to_string()];
    let files = walk_sources(root, 1024 * 1024, &extra).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.to_string_lossy(), "data.custom");
}

#[test]
fn builtin_install_extension_indexed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("50-hooks.install"), "hooks\n").unwrap();
    let files = walk_sources(root, 1024 * 1024, &[]).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.to_string_lossy(), "50-hooks.install");
}
