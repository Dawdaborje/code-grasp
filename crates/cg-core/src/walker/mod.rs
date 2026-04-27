//! Directory walking with `.gitignore`, `.cgignore`, extension filtering, and size limits.

mod gitignore;

pub use gitignore::{is_supported_extension, should_index_path, supported_extensions};

use std::fs::File;
use std::io::Read;
use std::path::Path;

use ignore::WalkBuilder;

use crate::error::CgError;

/// A source file discovered during a walk, with UTF-8 content.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path relative to the walk root (canonicalized root stripped).
    pub path: std::path::PathBuf,
    /// Full decoded UTF-8 source (files that are not valid UTF-8 are skipped during walk).
    pub content: String,
}

const PREFIX_SCAN: usize = 8192;

/// Walk `root` and yield source files according to `max_file_size_bytes` and path rules.
///
/// Behavior:
///
/// - Root is [`canonicalize`](std::path::Path::canonicalize)d; returned [`SourceFile::path`](SourceFile::path) values are relative to it.
/// - **Hidden** path segments are skipped (`ignore` builder `hidden(true)` skips dot dirs/files as configured).
/// - **`.gitignore`** and **`.cgignore`** are honored (see [`ignore::WalkBuilder`]).
/// - Only paths for which [`gitignore::should_index_path`](crate::walker::should_index_path) is true (built-in extensions, well-known basenames, plus `extra_extensions`) are kept.
/// - Files larger than `max_file_size_bytes` or empty are skipped.
/// - If the first 8 KiB (or less for small files) contains a **NUL** byte, the file is treated as binary and skipped.
pub fn walk_sources(
    root: &Path,
    max_file_size_bytes: u64,
    extra_extensions: &[String],
) -> Result<Vec<SourceFile>, CgError> {
    let root = root.canonicalize().map_err(CgError::Io)?;
    let mut out = Vec::new();

    let mut builder = WalkBuilder::new(&root);
    builder.hidden(true);
    builder.git_ignore(true);
    builder.add_custom_ignore_filename(".cgignore");

    for ent in builder.build().flatten() {
        if !ent.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = ent.path();
        if !gitignore::should_index_path(path, extra_extensions) {
            continue;
        }
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let len = meta.len();
        if len > max_file_size_bytes || len == 0 {
            continue;
        }

        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let scan = std::cmp::min(PREFIX_SCAN, len as usize);
        let mut buf = vec![0u8; scan];
        let n = match f.read(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        buf.truncate(n);
        if buf.contains(&0) {
            continue;
        }

        let rel = path.strip_prefix(&root).unwrap_or(path);
        let content = std::fs::read_to_string(path).map_err(CgError::Io)?;
        out.push(SourceFile {
            path: rel.to_path_buf(),
            content,
        });
    }

    Ok(out)
}
