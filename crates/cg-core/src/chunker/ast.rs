//! AST-guided chunking via tree-sitter; falls back to sliding windows for unknown languages.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;
use std::sync::Mutex;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Parser, Query, QueryCursor};

use crate::chunker::{Chunk, Chunker, Language};
use crate::error::CgError;
use crate::manifest::hash_bytes;
use crate::walker::SourceFile;

/// Clamp byte offsets to valid UTF-8 boundaries so `&source[start..end]` never splits a scalar value.
#[inline]
fn utf8_clamp_range(source: &str, start: usize, end: usize) -> (usize, usize) {
    let len = source.len();
    let start = source.floor_char_boundary(start.min(len));
    let end = source.floor_char_boundary(end.min(len));
    (start, end.max(start))
}

/// Token counts are approximated as **whitespace-delimited word counts** (no external tokenizer).
pub struct AstChunker {
    pub min_tokens: u32,
    pub max_tokens: u32,
    parsers: Mutex<HashMap<Language, Parser>>,
}

impl Default for AstChunker {
    fn default() -> Self {
        Self {
            min_tokens: 20,
            max_tokens: 512,
            parsers: Mutex::new(HashMap::new()),
        }
    }
}

impl AstChunker {
    /// Create chunker with the given token bounds (word-count heuristic).
    pub fn new(min_tokens: u32, max_tokens: u32) -> Self {
        Self {
            min_tokens,
            max_tokens,
            parsers: Mutex::new(HashMap::new()),
        }
    }

    fn ts_language(lang: Language) -> Option<TsLanguage> {
        Some(match lang {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Unknown => return None,
        })
    }

    fn query_source(lang: Language) -> Option<&'static str> {
        Some(match lang {
            Language::Rust => {
                "(function_item) @c (impl_item) @c (struct_item) @c (enum_item) @c (trait_item) @c (mod_item) @c"
            }
            Language::Python => "(function_definition) @c (class_definition) @c",
            Language::JavaScript | Language::TypeScript | Language::Tsx => {
                "(function_declaration) @c (class_declaration) @c (method_definition) @c (lexical_declaration) @c"
            }
            Language::Go => {
                "(function_declaration) @c (method_declaration) @c (type_declaration) @c"
            }
            Language::Java => {
                "(method_declaration) @c (class_declaration) @c (interface_declaration) @c"
            }
            Language::C => "(function_definition) @c (struct_specifier) @c",
            Language::Cpp => {
                "(function_definition) @c (class_specifier) @c (namespace_definition) @c"
            }
            Language::Unknown => return None,
        })
    }

    fn parse(&self, lang: Language, source: &str) -> Result<Option<tree_sitter::Tree>, CgError> {
        let Some(ts_lang) = Self::ts_language(lang) else {
            return Ok(None);
        };
        let mut guard = self
            .parsers
            .lock()
            .map_err(|_| CgError::Chunking("parser mutex poisoned".to_string()))?;
        let p = match guard.entry(lang) {
            Entry::Vacant(v) => {
                let mut parser = Parser::new();
                parser
                    .set_language(&ts_lang)
                    .map_err(|e| CgError::Chunking(format!("{e:?}")))?;
                v.insert(parser)
            }
            Entry::Occupied(o) => o.into_mut(),
        };
        Ok(p.parse(source, None))
    }

    fn line_for_byte(source: &str, byte: usize) -> u32 {
        let byte = source.floor_char_boundary(byte.min(source.len()));
        source[..byte].bytes().filter(|&b| b == b'\n').count() as u32 + 1
    }

    fn word_count(s: &str) -> u32 {
        s.split_whitespace().count() as u32
    }

    fn make_chunk(
        file_path: &Path,
        lang: Language,
        source: &str,
        start: usize,
        end: usize,
    ) -> Chunk {
        let (start, end) = utf8_clamp_range(source, start, end);
        let content = source[start..end].to_string();
        let start_line = Self::line_for_byte(source, start);
        let end_byte = end.saturating_sub(1).min(source.len().saturating_sub(1));
        let end_line = Self::line_for_byte(source, end_byte);
        let content_hash = hash_bytes(content.as_bytes());
        Chunk {
            content,
            file_path: file_path.to_path_buf(),
            start_byte: start,
            end_byte: end,
            start_line,
            end_line,
            language: lang,
            content_hash,
        }
    }

    fn split_oversized(
        &self,
        file_path: &Path,
        lang: Language,
        source: &str,
        start: usize,
        end: usize,
    ) -> Vec<Chunk> {
        let (s, e) = utf8_clamp_range(source, start, end);
        let text = &source[s..e];
        if Self::word_count(text) <= self.max_tokens {
            return vec![Self::make_chunk(file_path, lang, source, s, e)];
        }
        const WIN: usize = 400;
        const OVERLAP: usize = 50;
        let mut out = Vec::new();
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let end_i = (i + WIN).min(b.len());
            let abs_s = s + i;
            let abs_e = s + end_i;
            out.push(Self::make_chunk(file_path, lang, source, abs_s, abs_e));
            if end_i >= b.len() {
                break;
            }
            i = (end_i.saturating_sub(OVERLAP)).max(i + 1);
        }
        out
    }

    fn merge_small(chunks: Vec<Chunk>, min_tokens: u32, source: &str) -> Vec<Chunk> {
        if chunks.is_empty() {
            return chunks;
        }
        let mut out: Vec<Chunk> = Vec::new();
        let mut cur = chunks[0].clone();
        for next in chunks.into_iter().skip(1) {
            if Self::word_count(&cur.content) < min_tokens {
                cur.end_byte = next.end_byte;
                cur.end_line = next.end_line;
                let (s, e) = utf8_clamp_range(source, cur.start_byte, cur.end_byte);
                cur.content = source[s..e].to_string();
                cur.content_hash = hash_bytes(cur.content.as_bytes());
            } else {
                out.push(cur);
                cur = next;
            }
        }
        out.push(cur);

        let mut fixed: Vec<Chunk> = Vec::new();
        for c in out {
            if let Some(prev) = fixed.last_mut()
                && Self::word_count(&c.content) < min_tokens
            {
                prev.end_byte = c.end_byte;
                prev.end_line = c.end_line;
                let (s, e) = utf8_clamp_range(source, prev.start_byte, prev.end_byte);
                prev.content = source[s..e].to_string();
                prev.content_hash = hash_bytes(prev.content.as_bytes());
                continue;
            }
            fixed.push(c);
        }
        fixed
    }

    fn ast_chunks_for_file(
        &self,
        file: &SourceFile,
        lang: Language,
    ) -> Result<Vec<Chunk>, CgError> {
        let path = &file.path;
        let source = &file.content;
        let Some(tree) = self.parse(lang, source)? else {
            return Ok(self.fallback_windows(path, lang, source));
        };
        let Some(q_src) = Self::query_source(lang) else {
            return Ok(self.fallback_windows(path, lang, source));
        };
        let ts_lang = Self::ts_language(lang)
            .ok_or_else(|| CgError::UnsupportedLanguage(format!("{lang:?}")))?;
        let query = Query::new(&ts_lang, q_src)
            .map_err(|e| CgError::Chunking(format!("query compile: {e}")))?;
        let mut qc = QueryCursor::new();
        let mut captures: Vec<(usize, usize)> = Vec::new();
        let root = tree.root_node();
        let mut matches = qc.matches(&query, root, source.as_bytes());
        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let n = cap.node;
                captures.push((n.start_byte(), n.end_byte()));
            }
        }

        captures.sort_by_key(|x| x.0);
        captures.dedup();

        let mut raw: Vec<Chunk> = Vec::new();
        for (s, e) in captures {
            if e <= s {
                continue;
            }
            let parts = self.split_oversized(path, lang, source, s, e);
            raw.extend(parts);
        }

        if raw.is_empty() {
            return Ok(self.fallback_windows(path, lang, source));
        }

        // Nested/overlapping query captures can yield chunks out of document order; merge
        // assumes monotonic `start_byte`.
        raw.sort_by_key(|c| (c.start_byte, c.end_byte));

        let merged = Self::merge_small(raw, self.min_tokens, source);
        Ok(merged)
    }

    fn fallback_windows(&self, path: &Path, lang: Language, source: &str) -> Vec<Chunk> {
        const WIN: usize = 400;
        const OVERLAP: usize = 50;
        let mut out = Vec::new();
        let b = source.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let end = (i + WIN).min(b.len());
            out.push(Self::make_chunk(path, lang, source, i, end));
            if end >= b.len() {
                break;
            }
            i = (end.saturating_sub(OVERLAP)).max(i + 1);
        }
        if out.is_empty() && !source.is_empty() {
            out.push(Self::make_chunk(path, lang, source, 0, source.len()));
        }
        out
    }
}

impl Chunker for AstChunker {
    fn chunk(&self, file: &SourceFile) -> Result<Vec<Chunk>, CgError> {
        let lang = Language::from_path(&file.path);
        if lang == Language::Unknown {
            return Ok(self.fallback_windows(&file.path, lang, &file.content));
        }
        self.ast_chunks_for_file(file, lang)
    }

    fn supported_languages(&self) -> &[Language] {
        const L: &[Language] = &[
            Language::Rust,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Tsx,
            Language::Go,
            Language::Java,
            Language::C,
            Language::Cpp,
        ];
        L
    }
}
