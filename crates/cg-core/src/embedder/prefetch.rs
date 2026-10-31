//! Download Hugging Face artifacts for the configured fastembed model **without** loading ONNX.
//!
//! This uses `hf-hub` only (HTTP + disk cache), matching the cache root rules in `fastembed`'s
//! `pull_from_hf`: `HF_HOME` if set, otherwise [`crate::paths::models_cache_dir`].

use std::collections::BTreeSet;
use std::path::PathBuf;

use fastembed::TextEmbedding;
use hf_hub::Cache;
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

/// Resumed attempts per file after a failed or interrupted download.
const DOWNLOAD_RETRIES: usize = 5;

/// Attempts to fetch the whole file set (first with the saved token, then anonymously).
const FETCH_ATTEMPTS: usize = 4;

/// Download ONNX weights and tokenizer JSON into the HF cache used by `FastEmbedder`.
///
/// Does **not** create an ONNX session, so it avoids the common `ort` segfault path while still
/// populating files for a later `cg index`. Files already in the cache are not re-downloaded.
///
/// [`FastEmbedder::new`](super::FastEmbedder::new) calls this first so fastembed only ever reads
/// from the cache, which lets downloads recover from a stale saved Hugging Face token.
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

    let mut rel: BTreeSet<String> = BTreeSet::new();
    rel.insert(info.model_file.clone());
    for f in &info.additional_files {
        rel.insert(f.clone());
    }
    for t in TOKENIZER_FILES {
        rel.insert((*t).to_string());
    }

    // `ApiBuilder` always attaches the token saved by `huggingface-cli login`
    // (`~/.cache/huggingface/token`). The supported models are public, but the Hub answers an
    // expired or revoked token with 401 even for public files, so later attempts go anonymous.
    let builder = || {
        ApiBuilder::new()
            .with_cache_dir(cache_root.clone())
            .with_endpoint(endpoint.clone())
            .with_progress(show_download_progress)
            // hf-hub defaults to 0 retries; each retry resumes the partial file via a Range request,
            // so a dropped connection mid-way through the ~130 MB ONNX file does not fail indexing.
            .with_retries(DOWNLOAD_RETRIES)
    };
    // The first attempt uses the saved token (if any); every later attempt is anonymous. Whole-set
    // retries also cover transient failures of the initial request, which hf-hub does not retry.
    // Files already cached are skipped, so a retry only fetches what is still missing.
    let mut with_token = Cache::default().token().is_some();
    let mut token_error: Option<String> = None;
    let mut last_error = String::new();
    let mut out = None;
    for attempt in 0..FETCH_ATTEMPTS {
        let b = if with_token {
            builder()
        } else {
            builder().with_token(None)
        };
        match fetch_files(b, &info.model_code, &rel) {
            Ok(files) => {
                out = Some(files);
                break;
            }
            Err(e) if with_token => {
                with_token = false;
                token_error = Some(e);
            }
            Err(e) => {
                tracing::debug!(attempt, error = %e, "model download attempt failed");
                last_error = e;
                std::thread::sleep(std::time::Duration::from_secs(1 << attempt.min(3)));
            }
        }
    }
    let Some(out) = out else {
        let mut msg = last_error;
        if let Some(t) = token_error {
            msg = format!("{t} (retrying without the saved Hugging Face token also failed: {msg})");
        }
        return Err(CgError::Embedding(msg));
    };
    if let Some(t) = token_error {
        tracing::warn!(
            token_file = %Cache::default().token_path().display(),
            error = %t,
            "download with the saved Hugging Face token failed but succeeded anonymously; \
             if the token is expired, run `huggingface-cli login` again or delete the token file"
        );
    }

    tracing::info!(
        hub_repo = %info.model_code,
        cache_root = %cache_root.display(),
        files = out.len(),
        "prefetched fastembed model files (onnx not loaded)"
    );

    Ok(out)
}

/// Resolve every file in `rel` from the local cache, downloading missing ones.
fn fetch_files(
    builder: ApiBuilder,
    model_code: &str,
    rel: &BTreeSet<String>,
) -> Result<Vec<PathBuf>, String> {
    let api = builder.build().map_err(|e| format!("hf hub client: {e}"))?;
    let repo = api.model(model_code.to_string());
    rel.iter()
        .map(|path| {
            repo.get(path)
                .map_err(|e| format!("fetch `{path}` from {model_code}: {e}"))
        })
        .collect()
}
