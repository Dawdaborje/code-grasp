//! Text embedding backends (local `fastembed`, optional OpenAI).

mod fastembed;
mod prefetch;

#[cfg(feature = "openai")]
mod openai;

pub use fastembed::FastEmbedder;
pub use prefetch::prefetch_fastembed_model_weights;

#[cfg(feature = "openai")]
pub use openai::OpenAiEmbedder;

use crate::error::CgError;

/// Embedding backend abstraction.
pub trait Embedder: Send + Sync {
    /// Embed each input string in order; batching is internal to the implementation.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, CgError>;

    /// Vector dimension for this model.
    fn dimension(&self) -> usize;
}
