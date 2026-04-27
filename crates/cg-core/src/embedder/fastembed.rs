//! Local embeddings via `fastembed` (default: BGE-small-en, 384-d).

use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use ort::execution_providers::CPUExecutionProvider;

use crate::embedder::Embedder;
use crate::error::CgError;
use crate::paths;

static ORT_ENV_INIT: OnceLock<Result<(), String>> = OnceLock::new();

fn execution_provider_stack() -> Vec<ort::execution_providers::ExecutionProviderDispatch> {
    #[cfg(feature = "ort-cuda")]
    {
        use ort::execution_providers::CUDAExecutionProvider;
        let cuda = CUDAExecutionProvider::default().build();
        let cpu = CPUExecutionProvider::default().build();
        vec![cuda, cpu]
    }
    #[cfg(not(feature = "ort-cuda"))]
    {
        vec![CPUExecutionProvider::default().build()]
    }
}

fn ensure_ort_environment() -> Result<(), CgError> {
    match ORT_ENV_INIT.get_or_init(|| {
        ort::init()
            .with_name("code-grasp")
            .with_telemetry(false)
            .commit()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }) {
        Ok(()) => Ok(()),
        Err(s) => Err(CgError::Embedding(format!("onnx runtime environment: {s}"))),
    }
}

/// Local embedding via `fastembed` (CPU by default; build with `ort-cuda` to prefer CUDA).
pub struct FastEmbedder {
    model: Mutex<TextEmbedding>,
    dimension: usize,
    batch_size: usize,
}

impl FastEmbedder {
    /// Create embedder for `model_name` (HuggingFace id, e.g. `BAAI/bge-small-en-v1.5`) with batch size.
    pub fn new(model_name: &str, batch_size: usize) -> Result<Self, CgError> {
        let cache_dir = paths::models_cache_dir().ok_or_else(|| {
            CgError::Embedding("no cache directory (dirs::cache_dir)".to_string())
        })?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            CgError::Embedding(format!("create cache dir {}: {e}", cache_dir.display()))
        })?;

        let embedding_model = fastembed_model_for_id(model_name)?;
        let info = TextEmbedding::get_model_info(&embedding_model)
            .map_err(|e| CgError::Embedding(e.to_string()))?;
        let dimension = info.dim;

        ensure_ort_environment()?;

        let execution_providers = execution_provider_stack();
        tracing::info!(
            model = %info.model_code,
            dim = dimension,
            cache = %cache_dir.display(),
            ort_build = %ort::info(),
            ort_cuda = cfg!(feature = "ort-cuda"),
            ort_ep_count = execution_providers.len(),
            "initializing fastembed (model files may download on first use)"
        );

        let opts = InitOptions::new(embedding_model)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true)
            .with_execution_providers(execution_providers);
        let model = TextEmbedding::try_new(opts).map_err(|e| CgError::Embedding(e.to_string()))?;

        Ok(Self {
            model: Mutex::new(model),
            dimension,
            batch_size: batch_size.max(1),
        })
    }

    fn embed_batch_inner(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, CgError> {
        let m = self
            .model
            .lock()
            .map_err(|_| CgError::Embedding("fastembed mutex poisoned".to_string()))?;
        m.embed(texts.to_vec(), None)
            .map_err(|e| CgError::Embedding(e.to_string()))
    }
}

/// Map settings `embedding.model` string to the `fastembed` enum (also used for HF prefetch).
pub(crate) fn fastembed_model_for_id(name: &str) -> Result<EmbeddingModel, CgError> {
    match name {
        "BAAI/bge-small-en-v1.5" => Ok(EmbeddingModel::BGESmallENV15),
        "sentence-transformers/all-MiniLM-L6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),
        other => Err(CgError::Embedding(format!(
            "unknown fastembed model id `{other}` (supported: BAAI/bge-small-en-v1.5, sentence-transformers/all-MiniLM-L6-v2)"
        ))),
    }
}

impl Embedder for FastEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, CgError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut all = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(self.batch_size) {
            let batch: Vec<&str> = chunk.to_vec();
            let mut v = self.embed_batch_inner(&batch)?;
            all.append(&mut v);
        }
        Ok(all)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
