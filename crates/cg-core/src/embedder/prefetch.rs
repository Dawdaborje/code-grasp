//! Download Hugging Face artifacts for the configured fastembed model **without** loading ONNX.
//!
//! This uses `hf-hub` only (HTTP + disk cache), matching the cache root rules in `fastembed`'s
//! `pull_from_hf`: `HF_HOME` if set, otherwise [`crate::paths::models_cache_dir`].

use std::collections::BTreeSet;
use std::path::PathBuf;

use fastembed::TextEmbedding;
use hf_hub::api::sync::ApiBuilder;

use super::fastembed::fastembed_model_for_id;
use crate::error::CgError;
use crate::paths;

/// Same relative paths `fastembed` reads via `load_tokenizer_hf_hub`.
const TOKENIZER_FILES: &[&str] = &[
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

/// Download ONNX weights and tokenizer JSON into the HF cache used by `FastEmbedder`.
///
/// Does **not** create an ONNX session, so it avoids the common `ort` segfault path while still
/// populating files for a later `cg index`.
pub fn prefetch_fastembed_model_weights(
    model_name: &str,
    show_download_progress: bool,
) -> Result<Vec<PathBuf>, CgError> {
    let default_cache = paths::models_cache_dir()
        .ok_or_else(|| CgError::Embedding("no cache directory (dirs::cache_dir)".to_string()))?;
    std::fs::create_dir_all(&default_cache).map_err(CgError::Io)?;

    let cache_root = std::env::var("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or(default_cache);

    let embedding_model = fastembed_model_for_id(model_name)?;
    let info = TextEmbedding::get_model_info(&embedding_model)
        .map_err(|e| CgError::Embedding(e.to_string()))?;

    let endpoint =
        std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".to_string());

    let api = ApiBuilder::new()
        .with_cache_dir(cache_root.clone())
        .with_endpoint(endpoint)
        .with_progress(show_download_progress)
        .build()
        .map_err(|e| CgError::Embedding(format!("hf hub client: {e}")))?;

    let repo = api.model(info.model_code.clone());

    let mut rel: BTreeSet<String> = BTreeSet::new();
    rel.insert(info.model_file.clone());
    for f in &info.additional_files {
        rel.insert(f.clone());
    }
    for t in TOKENIZER_FILES {
        rel.insert((*t).to_string());
    }

    let mut out = Vec::new();
    for path in rel {
        let p = repo.get(&path).map_err(|e| {
            CgError::Embedding(format!("fetch `{path}` from {}: {e}", info.model_code))
        })?;
        out.push(p);
    }

    tracing::info!(
        hub_repo = %info.model_code,
        cache_root = %cache_root.display(),
        files = out.len(),
        "prefetched fastembed model files (onnx not loaded)"
    );

    Ok(out)
}
