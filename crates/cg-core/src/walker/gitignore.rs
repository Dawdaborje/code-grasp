//! Supported source extensions and extension checks.

use std::path::Path;

/// Extensions indexed by CodeGrasp (lowercase, without leading dot).
pub fn supported_extensions() -> &'static [&'static str] {
    &[
        "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "java", "c", "h", "cc", "cpp",
        "cxx", "hpp", "hh", "hxx",
    ]
}

/// Returns true if `path` has a supported source extension.
pub fn is_supported_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let Some(ext) = ext else {
        return false;
    };
    supported_extensions().contains(&ext.as_str())
}

#[cfg(test)]
mod tests {
    use super::{is_supported_extension, supported_extensions};
    use std::path::Path;

    #[test]
    fn extensions_cover_rust_and_ts() {
        assert!(is_supported_extension(Path::new("foo.rs")));
        assert!(is_supported_extension(Path::new("bar.ts")));
        assert!(is_supported_extension(Path::new("baz.tsx")));
        assert!(!is_supported_extension(Path::new("readme.md")));
        assert!(!supported_extensions().is_empty());
    }
}
