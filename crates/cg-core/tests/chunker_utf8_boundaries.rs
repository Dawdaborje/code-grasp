//! Sliding windows use byte length; chunk boundaries must stay on UTF-8 scalar boundaries.

use std::path::PathBuf;

use cg_core::chunker::{AstChunker, Chunker, Language};
use cg_core::walker::SourceFile;

/// Build content where a fixed 400-byte window end lands on the second byte of `ó` (U+00F3).
fn content_with_multibyte_at_byte_1800() -> String {
    let mut s = "a".repeat(1799);
    s.push('ó');
    s.push_str(&"b".repeat(800));
    debug_assert!(s.is_char_boundary(1799));
    debug_assert!(!s.is_char_boundary(1800));
    s
}

#[test]
fn fallback_windows_do_not_panic_when_window_end_splits_utf8() {
    let chunker = AstChunker::new(20, 512);
    let file = SourceFile {
        path: PathBuf::from("manifest.data"),
        content: content_with_multibyte_at_byte_1800(),
    };
    assert_eq!(Language::from_path(&file.path), Language::Unknown);
    let chunks = chunker.chunk(&file).expect("chunk");
    assert!(!chunks.is_empty());
    for c in &chunks {
        assert!(c.content.is_char_boundary(0));
        assert!(c.content.is_char_boundary(c.content.len()));
    }
}

#[test]
fn python_oversized_split_does_not_panic_on_utf8() {
    let mut inner = "word ".repeat(80);
    inner.push_str(&"a".repeat(1799));
    inner.push('ó');
    inner.push_str(&"b".repeat(200));
    let body = format!("def big():\n    x = '''\n{inner}\n'''\n    return x\n");
    let chunker = AstChunker::new(2, 20);
    let file = SourceFile {
        path: PathBuf::from("big_string.py"),
        content: body,
    };
    let chunks = chunker.chunk(&file).expect("chunk");
    assert!(!chunks.is_empty());
}
