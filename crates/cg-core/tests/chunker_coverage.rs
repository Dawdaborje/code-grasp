//! AST chunks cover top-level code, do not duplicate nested declarations, and respect size bounds.

use std::path::PathBuf;

use cg_core::chunker::{AstChunker, Chunk, Chunker};
use cg_core::walker::SourceFile;

fn chunk(path: &str, content: &str, min: u32, max: u32) -> Vec<Chunk> {
    let file = SourceFile {
        path: PathBuf::from(path),
        content: content.to_string(),
    };
    AstChunker::new(min, max).chunk(&file).expect("chunk")
}

fn all_content(chunks: &[Chunk]) -> String {
    chunks
        .iter()
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn top_level_code_outside_declarations_is_indexed() {
    let src = "use std::collections::HashMap;\n\
               const LIMIT_MARKER: usize = 42;\n\
               static GREETING_MARKER: &str = \"hi\";\n\n\
               fn helper() -> usize { LIMIT_MARKER }\n";
    let chunks = chunk("lib.rs", src, 1, 512);
    let text = all_content(&chunks);
    assert!(text.contains("use std::collections::HashMap"), "{text}");
    assert!(text.contains("LIMIT_MARKER: usize = 42"), "{text}");
    assert!(text.contains("GREETING_MARKER"), "{text}");
    assert!(text.contains("fn helper()"), "{text}");

    let py = "import os\nTOP_LEVEL_MARKER = os.getcwd()\nprint(TOP_LEVEL_MARKER)\n\ndef f():\n    return 1\n";
    let text = all_content(&chunk("script.py", py, 1, 512));
    assert!(text.contains("TOP_LEVEL_MARKER = os.getcwd()"), "{text}");
    assert!(text.contains("print(TOP_LEVEL_MARKER)"), "{text}");
}

#[test]
fn nested_declarations_are_not_indexed_twice_when_parent_fits() {
    let src = "struct S;\n\
               impl S {\n    fn method_marker_one(&self) -> u32 { 1 }\n    fn method_marker_two(&self) -> u32 { 2 }\n}\n";
    let chunks = chunk("s.rs", src, 1, 512);
    let text = all_content(&chunks);
    assert_eq!(text.matches("method_marker_one").count(), 1, "{chunks:#?}");
    assert_eq!(text.matches("method_marker_two").count(), 1, "{chunks:#?}");
}

#[test]
fn oversized_parent_is_split_into_children_and_keeps_header_and_footer() {
    let mut body = String::from("impl Big {\n");
    for i in 0..20 {
        body.push_str(&format!(
            "    fn method_{i}(&self) -> u32 {{ let a = {i}; let b = a + 1; b * 2 }}\n"
        ));
    }
    body.push_str("}\n");
    let chunks = chunk("big.rs", &body, 1, 40);
    let text = all_content(&chunks);
    for i in 0..20 {
        assert_eq!(
            text.matches(&format!("fn method_{i}(")).count(),
            1,
            "method_{i} should appear exactly once: {chunks:#?}"
        );
    }
    assert!(text.contains("impl Big {"), "impl header must be indexed");
    for c in &chunks {
        assert!(
            c.content.split_whitespace().count() <= 40,
            "chunk over max: {c:?}"
        );
    }
}

#[test]
fn merging_small_nested_chunks_never_truncates() {
    // A tiny impl (under `min_tokens`) containing a method: the merged chunk must still end at `}`.
    let src = "impl Default for X { fn default() -> Self { Self } }\n";
    let chunks = chunk("x.rs", src, 20, 512);
    assert_eq!(chunks.len(), 1, "{chunks:#?}");
    assert_eq!(chunks[0].content, src.trim_end());
}

#[test]
fn chunks_are_ordered_and_disjoint() {
    let mut src = String::from("use a::b;\n\n");
    for i in 0..30 {
        src.push_str(&format!(
            "pub fn func_{i}(x: u32) -> u32 {{\n    let y = x * {i};\n    y + 1\n}}\n\n"
        ));
    }
    let chunks = chunk("many.rs", &src, 10, 30);
    for w in chunks.windows(2) {
        assert!(w[0].end_byte <= w[1].start_byte, "overlap: {w:#?}");
    }
}

#[test]
fn windows_follow_lines_and_token_limit() {
    let src: String = (0..200)
        .map(|i| format!("line {i} has some words here\n"))
        .collect();
    let chunks = chunk("notes.txt", &src, 5, 50);
    assert!(chunks.len() > 1);
    for c in &chunks {
        assert!(c.content.split_whitespace().count() <= 50, "{c:?}");
        assert!(
            c.content.starts_with("line "),
            "window should start at a line: {c:?}"
        );
        assert!(
            c.content.ends_with("here"),
            "window should end at a line: {c:?}"
        );
    }
    let text = all_content(&chunks);
    assert!(text.contains("line 0 has"));
    assert!(text.contains("line 199 has"));
}

#[test]
fn minified_single_line_is_split_by_bytes() {
    let src = "x".repeat(20_000);
    let chunks = chunk("bundle.min.js", &src, 1, 64);
    assert!(chunks.len() > 1);
    let total: usize = chunks.iter().map(|c| c.content.len()).sum();
    assert!(total >= src.len());
}
