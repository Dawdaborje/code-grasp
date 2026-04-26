//! Optional OpenAI embeddings (`text-embedding-3-small`).

use std::time::Duration;

use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::CreateEmbeddingRequestArgs;

use crate::error::CgError;

const DEFAULT_MODEL: &str = "text-embedding-3-small";
const DEFAULT_DIM: usize = 1536;

/// OpenAI cloud embedder (requires `CODEGRASP_OPENAI_API_KEY` or legacy `CODEGASP_OPENAI_API_KEY`).
#[derive(Clone)]
pub struct OpenAiEmbedder {
    client: Client<OpenAIConfig>,
    model: String,
    batch_size: usize,
}

impl OpenAiEmbedder {
    /// Build client from environment API key.
    pub fn new(model: Option<String>, batch_size: usize) -> Result<Self, CgError> {
        let key = std::env::var("CODEGRASP_OPENAI_API_KEY")
            .or_else(|_| std::env::var("CODEGASP_OPENAI_API_KEY"))
            .map_err(|_| {
                CgError::Embedding(
                    "missing CODEGRASP_OPENAI_API_KEY (or legacy CODEGASP_OPENAI_API_KEY)"
                        .to_string(),
                )
            })?;
        let mut cfg = OpenAIConfig::new();
        cfg = cfg.with_api_key(key);
        let client = Client::with_config(cfg);
        Ok(Self {
            client,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            batch_size: batch_size.max(1).min(2048),
        })
    }

    /// Async embedding call (invoke from `async` indexing/search paths).
    pub async fn embed_async(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, CgError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut all = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(self.batch_size) {
            let batch: Vec<&str> = chunk.to_vec();
            let req = CreateEmbeddingRequestArgs::default()
                .model(&self.model)
                .input(batch.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
                .build()
                .map_err(|e| CgError::Embedding(e.to_string()))?;

            let mut attempt = 0u32;
            loop {
                match self.client.embeddings().create(req.clone()).await {
                    Ok(resp) => {
                        for d in resp.data {
                            all.push(d.embedding.clone());
                        }
                        break;
                    }
                    Err(e) => {
                        let s = e.to_string();
                        let retry = s.contains("429") || s.to_lowercase().contains("rate");
                        attempt += 1;
                        if !retry || attempt > 5 {
                            return Err(CgError::Embedding(s));
                        }
                        let wait = Duration::from_millis(200 * (1 << attempt.min(8)));
                        tokio::time::sleep(wait).await;
                    }
                }
            }
        }
        Ok(all)
    }

    /// Vector dimension for the configured model (default `text-embedding-3-small`).
    pub fn dimension(&self) -> usize {
        DEFAULT_DIM
    }
}
