//! AST-guided chunking via tree-sitter; falls back to line-aligned windows for unknown languages.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

/// Upper bound on chunk bytes per configured token (see [`AstChunker::max_bytes`]).
const BYTES_PER_TOKEN_CAP: usize = 8;

/// Token counts are approximated as **whitespace-delimited word counts** (no external tokenizer).
///
/// Chunks cover the whole file: matched declarations (functions, classes, impls, ...) become
/// chunks when they fit `max_tokens`; larger ones are broken down into their nested declarations,
/// and code between declarations (imports, constants, top-level statements) forms its own chunks.
pub struct AstChunker {
    pub min_tokens: u32,
    pub max_tokens: u32,
    parsers: Mutex<HashMap<Language, Parser>>,
    queries: Mutex<HashMap<Language, Arc<Query>>>,
}

impl Default for AstChunker {
    fn default() -> Self {
        Self {
            min_tokens: 20,
            max_tokens: 512,
            parsers: Mutex::new(HashMap::new()),
            queries: Mutex::new(HashMap::new()),
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
            queries: Mutex::new(HashMap::new()),
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

    /// Byte cap per chunk so whitespace-poor text (minified JS, long literals) cannot produce a
    /// "small" word count that still overflows the embedding model's context.
    fn max_bytes(&self) -> usize {
        (self.max_tokens.max(1) as usize).saturating_mul(BYTES_PER_TOKEN_CAP)
    }

    /// Narrow `[start, end)` to its first and last non-whitespace bytes.
    fn trim_range(source: &str, start: usize, end: usize) -> (usize, usize) {
        let (start, end) = utf8_clamp_range(source, start, end);
        let text = &source[start..end];
        let lead = text.len() - text.trim_start().len();
        let trail = text.len() - text.trim_end().len();
        let s = start + lead;
        (s, end.saturating_sub(trail).max(s))
    }

    /// Line-aligned pieces of `[start, end)`; lines longer than the byte cap are cut on char boundaries.
    fn line_pieces(&self, source: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
        let cap = self.max_bytes();
        let mut pieces = Vec::new();
        let mut line_start = start;
        while line_start < end {
            let line_end = source[line_start..end]
                .find('\n')
                .map_or(end, |i| line_start + i + 1);
            let mut s = line_start;
            while line_end - s > cap {
                let mut e = source.floor_char_boundary(s + cap);
                if e <= s {
                    e = source.ceil_char_boundary(s + 1);
                }
                pieces.push((s, e));
                s = e;
            }
            if s < line_end {
                pieces.push((s, line_end));
            }
            line_start = line_end;
        }
        pieces
    }

    /// Split `[start, end)` into line-aligned windows bounded by `max_tokens` words and the byte cap,
    /// overlapping by roughly an eighth of a window. Whitespace-only ranges yield nothing.
    fn split_windows(
        &self,
        path: &Path,
        lang: Language,
        source: &str,
        start: usize,
        end: usize,
    ) -> Vec<Chunk> {
        let (start, end) = Self::trim_range(source, start, end);
        if start >= end {
            return Vec::new();
        }
        let max_words = self.max_tokens.max(1);
        let max_bytes = self.max_bytes();
        if Self::word_count(&source[start..end]) <= max_words && end - start <= max_bytes {
            return vec![Self::make_chunk(path, lang, source, start, end)];
        }

        let pieces = self.line_pieces(source, start, end);
        let words: Vec<u32> = pieces
            .iter()
            .map(|&(s, e)| Self::word_count(&source[s..e]))
            .collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < pieces.len() {
            // Grow the window; always take at least one piece.
            let mut j = i;
            let mut w = 0u32;
            while j < pieces.len() {
                let next_w = w + words[j];
                let next_b = pieces[j].1 - pieces[i].0;
                if j > i && (next_w > max_words || next_b > max_bytes) {
                    break;
                }
                w = next_w;
                j += 1;
            }
            let (s, e) = Self::trim_range(source, pieces[i].0, pieces[j - 1].1);
            if s < e {
                out.push(Self::make_chunk(path, lang, source, s, e));
            }
            if j >= pieces.len() {
                break;
            }
            // Step back over trailing pieces for overlap, but always advance by at least one.
            let mut k = j;
            let mut ow = 0u32;
            while k - 1 > i
                && ow + words[k - 1] <= max_words / 8
                && pieces[j - 1].1 - pieces[k - 1].0 <= max_bytes / 8
            {
                ow += words[k - 1];
                k -= 1;
            }
            i = k;
        }
        out
    }

    /// Emit chunks covering `[start, end)` given the query captures inside it (sorted by start
    /// ascending, end descending). Captures that fit become one chunk; oversized captures recurse
    /// into their nested captures; uncovered code between captures becomes gap chunks.
    #[allow(clippy::too_many_arguments)]
    fn emit_region(
        &self,
        path: &Path,
        lang: Language,
        source: &str,
        start: usize,
        end: usize,
        caps: &[(usize, usize)],
        out: &mut Vec<Chunk>,
    ) {
        let mut cursor = start;
        let mut i = 0;
        while i < caps.len() {
            let (s, e) = (caps[i].0.max(cursor), caps[i].1.min(end));
            // Captures starting before `e` are nested inside this one.
            let mut j = i + 1;
            while j < caps.len() && caps[j].0 < e {
                j += 1;
            }
            if e <= s {
                i = j;
                continue;
            }
            out.extend(self.split_windows(path, lang, source, cursor, s));
            let children = &caps[i + 1..j];
            let fits =
                Self::word_count(&source[s..e]) <= self.max_tokens && e - s <= self.max_bytes();
            if fits || children.is_empty() {
                out.extend(self.split_windows(path, lang, source, s, e));
            } else {
                self.emit_region(path, lang, source, s, e, children, out);
            }
            cursor = e;
            i = j;
        }
        out.extend(self.split_windows(path, lang, source, cursor, end));
    }

    /// Extend `into` so it also covers `other` (which follows it), recomputing content and hash.
    fn absorb(into: &mut Chunk, other: &Chunk, source: &str) {
        // `max` keeps a chunk from shrinking if `other` ends earlier (e.g. nested ranges).
        if other.end_byte > into.end_byte {
            into.end_byte = other.end_byte;
            into.end_line = other.end_line;
        }
        let (s, e) = utf8_clamp_range(source, into.start_byte, into.end_byte);
        into.content = source[s..e].to_string();
        into.content_hash = hash_bytes(into.content.as_bytes());
    }

    /// Merge chunks under `min_tokens` into a neighbour while the result stays within `max_tokens`.
    fn merge_small(&self, chunks: Vec<Chunk>, source: &str) -> Vec<Chunk> {
        let min = self.min_tokens;
        let max = self.max_tokens;
        let combined_fits = |a: &Chunk, b: &Chunk| {
            let s = a.start_byte.min(b.start_byte);
            let e = a.end_byte.max(b.end_byte);
            let (s, e) = utf8_clamp_range(source, s, e);
            Self::word_count(&source[s..e]) <= max && e - s <= self.max_bytes()
        };

        // Forward pass: a small chunk absorbs the one after it.
        let mut out: Vec<Chunk> = Vec::new();
        let mut iter = chunks.into_iter();
        let Some(mut cur) = iter.next() else {
            return out;
        };
        for next in iter {
            if Self::word_count(&cur.content) < min && combined_fits(&cur, &next) {
                Self::absorb(&mut cur, &next, source);
            } else {
                out.push(cur);
                cur = next;
            }
        }
        out.push(cur);

        // Backward pass: a small trailing chunk joins the one before it.
        let mut fixed: Vec<Chunk> = Vec::new();
        for c in out {
            if let Some(prev) = fixed.last_mut()
                && Self::word_count(&c.content) < min
                && combined_fits(prev, &c)
            {
                Self::absorb(prev, &c, source);
                continue;
            }
            fixed.push(c);
        }
        fixed
    }

    fn query_for(&self, lang: Language) -> Result<Option<Arc<Query>>, CgError> {
        let mut guard = self
            .queries
            .lock()
            .map_err(|_| CgError::Chunking("query cache mutex poisoned".to_string()))?;
        if let Some(q) = guard.get(&lang) {
            return Ok(Some(Arc::clone(q)));
        }
        let (Some(ts_lang), Some(q_src)) = (Self::ts_language(lang), Self::query_source(lang))
        else {
            return Ok(None);
        };
        let query = Arc::new(
            Query::new(&ts_lang, q_src)
                .map_err(|e| CgError::Chunking(format!("query compile: {e}")))?,
        );
        guard.insert(lang, Arc::clone(&query));
        Ok(Some(query))
    }

    fn ast_chunks_for_file(
        &self,
        file: &SourceFile,
        lang: Language,
    ) -> Result<Vec<Chunk>, CgError> {
        let path = &file.path;
        let source = &file.content;
        let (Some(tree), Some(query)) = (self.parse(lang, source)?, self.query_for(lang)?) else {
            return Ok(self.fallback_windows(path, lang, source));
        };
        let mut qc = QueryCursor::new();
        let mut captures: Vec<(usize, usize)> = Vec::new();
        let mut matches = qc.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let n = cap.node;
                if n.end_byte() > n.start_byte() {
                    captures.push((n.start_byte(), n.end_byte()));
                }
            }
        }
        // Outer nodes before the nodes nested inside them.
        captures.sort_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
        captures.dedup();

        let mut raw = Vec::new();
        self.emit_region(path, lang, source, 0, source.len(), &captures, &mut raw);
        Ok(self.merge_small(raw, source))
    }

    fn fallback_windows(&self, path: &Path, lang: Language, source: &str) -> Vec<Chunk> {
        let out = self.split_windows(path, lang, source, 0, source.len());
        self.merge_small(out, source)
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
