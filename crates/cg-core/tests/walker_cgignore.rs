//! Walker respects `.cgignore` alongside supported extensions.

use std::fs;

use cg_core::walker::walk_sources;
use tempfile::tempdir;

#[test]
fn cgignore_excludes_matching_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join(".cgignore"), "*.skip\n").unwrap();
    fs::write(root.join("keep.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("x.skip"), "ignored\n").unwrap();

    let files = walk_sources(root, 1024 * 1024).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.to_string_lossy(), "keep.rs");
}

#[test]
fn skips_oversized_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("small.rs"), "fn a() {}\n").unwrap();
    let big = vec![b' '; 4096];
    fs::write(root.join("big.rs"), big).unwrap();

    let files = walk_sources(root, 100).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.to_string_lossy(), "small.rs");
}
