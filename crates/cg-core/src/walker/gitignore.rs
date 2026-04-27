//! Indexed path rules: extensions, optional extras, and well-known extensionless files.

use std::path::Path;

/// Programming, web, config, and common text extensions (lowercase, no leading dot).
/// Curated for typical repos: original language set, shell, markup, JVM/CLR, mobile,
/// functional stacks, infra-as-code, and other frequent plain-text suffixes.
const BUILTIN_INDEX_EXTENSIONS: &[&str] = &[
    "adoc",
    "bash",
    "bib",
    "c",
    "cc",
    "cfg",
    "cjs",
    "clj",
    "cljs",
    "cmake",
    "conf",
    "cpp",
    "cs",
    "csproj",
    "csx",
    "css",
    "cxx",
    "dart",
    "dhall",
    "ejs",
    "edn",
    "elm",
    "erl",
    "ex",
    "exs",
    "fish",
    "fs",
    "fsproj",
    "fsi",
    "fsx",
    "gemspec",
    "go",
    "gradle",
    "graphql",
    "gql",
    "groovy",
    "gvy",
    "h",
    "hcl",
    "hh",
    "hpp",
    "hrl",
    "hs",
    "htm",
    "html",
    "hxx",
    "ini",
    "install",
    "ipynb",
    "java",
    "js",
    "json",
    "jsx",
    "kt",
    "kts",
    "less",
    "lhs",
    "liquid",
    "lock",
    "lua",
    "md",
    "mjs",
    "mk",
    "ml",
    "mli",
    "mod",
    "nim",
    "nix",
    "org",
    "php",
    "phtml",
    "pl",
    "pm",
    "properties",
    "proto",
    "ps1",
    "psd1",
    "psm1",
    "pug",
    "purs",
    "py",
    "r",
    "rb",
    "rs",
    "rst",
    "rules",
    "sass",
    "scala",
    "sc",
    "scss",
    "service",
    "sh",
    "sln",
    "socket",
    "sql",
    "svelte",
    "sv",
    "swift",
    "tf",
    "tfvars",
    "thrift",
    "toml",
    "ts",
    "tsv",
    "tsx",
    "vb",
    "vbs",
    "vim",
    "vue",
    "v",
    "xhtml",
    "xml",
    "xsd",
    "xsl",
    "yaml",
    "yml",
    "zig",
    "zsh",
];

/// Extensionless filenames (ASCII lowercase) treated as UTF-8 text for indexing.
const WELL_KNOWN_TEXT_NAMES: &[&str] = &[
    "makefile",
    "gnumakefile",
    "dockerfile",
    "containerfile",
    "pkgbuild",
    "justfile",
    "packages.x86_64",
    "packages.aarch64",
    "packages.riscv64",
];

/// Built-in extensions indexed when no project `extra_extensions` apply.
pub fn supported_extensions() -> &'static [&'static str] {
    BUILTIN_INDEX_EXTENSIONS
}

/// True if basename (case-insensitive) is a well-known text file without relying on extension.
pub fn is_well_known_text_basename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    WELL_KNOWN_TEXT_NAMES.contains(&lower.as_str())
}

/// Whether `path` should be walked for indexing: built-in extension, `extra_extensions`, or well-known basename.
pub fn should_index_path(path: &Path, extra_extensions: &[String]) -> bool {
    if is_well_known_text_basename(path) {
        return true;
    }
    let Some(ext) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
    else {
        return false;
    };
    if BUILTIN_INDEX_EXTENSIONS.contains(&ext.as_str()) {
        return true;
    }
    extra_extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext.as_str()))
}

/// Returns true if `path` would be indexed with **no** extra extensions (compat helper).
pub fn is_supported_extension(path: &Path) -> bool {
    should_index_path(path, &[])
}

#[cfg(test)]
mod tests {
    use super::{is_supported_extension, should_index_path, supported_extensions};
    use std::path::Path;

    #[test]
    fn extensions_cover_rust_ts_shell_and_markdown() {
        assert!(should_index_path(Path::new("foo.rs"), &[]));
        assert!(should_index_path(Path::new("bar.ts"), &[]));
        assert!(should_index_path(Path::new("baz.tsx"), &[]));
        assert!(should_index_path(Path::new("readme.md"), &[]));
        assert!(should_index_path(Path::new("script.sh"), &[]));
        assert!(should_index_path(Path::new("index.html"), &[]));
        assert!(should_index_path(Path::new("styles.css"), &[]));
        assert!(should_index_path(Path::new("Main.java"), &[]));
        assert!(should_index_path(Path::new("app.cfg"), &[]));
        assert!(!supported_extensions().is_empty());
        assert!(is_supported_extension(Path::new("x.py")));
    }

    #[test]
    fn well_known_names_without_extension() {
        assert!(should_index_path(Path::new("Makefile"), &[]));
        assert!(should_index_path(Path::new("subdir/Makefile"), &[]));
        assert!(should_index_path(Path::new("packages.x86_64"), &[]));
        assert!(should_index_path(Path::new("PKGBUILD"), &[]));
    }

    #[test]
    fn extra_extensions_merge() {
        assert!(!should_index_path(Path::new("foo.xyz"), &[]));
        let extra = vec!["xyz".to_string()];
        assert!(should_index_path(Path::new("foo.xyz"), &extra));
    }
}
