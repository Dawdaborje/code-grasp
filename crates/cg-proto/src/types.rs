//! Serde types for MCP tool payloads and shared responses.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Arguments for the MCP tool **`index_codebase`** (and CLI `cg index`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexCodebaseInput {
    /// Absolute or relative path to the project root to index.
    pub path: String,
    /// When `true`, drop existing vectors, SQLite rows, and manifest, then rebuild.
    #[serde(default)]
    pub force: bool,
}

/// Result body after indexing completes successfully.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexCodebaseOutput {
    /// Human-readable summary line for agents.
    pub message: String,
    /// Count of source files processed in the indexing pass.
    pub files_indexed: u64,
    /// Chunk rows written in the indexing pass.
    pub chunks_written: u64,
}

/// Arguments for the MCP tool **`search_code`** (and CLI `cg search`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchCodeInput {
    /// Project root that was previously indexed (must contain `.code-grasp/`).
    pub path: String,
    /// Natural-language or keyword query; embedded for dense search and passed to FTS5.
    pub query: String,
    /// Maximum number of hits to return (default **10**).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

/// One ranked hit in a search result list.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHit {
    /// Hybrid fused score (higher is better).
    pub score: f64,
    /// Path to the file relative to the project root.
    pub file_path: String,
    /// 1-based start line of the matching chunk.
    pub start_line: u32,
    /// 1-based end line of the matching chunk.
    pub end_line: u32,
    /// Snippet of source text stored for the chunk.
    pub content: String,
}

/// Result body for **`search_code`**.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchCodeOutput {
    /// Ranked hits, typically sorted by descending score.
    pub hits: Vec<SearchHit>,
}

/// Arguments for the MCP tool **`get_status`** (and CLI `cg status`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetStatusInput {
    /// Project root to inspect.
    pub path: String,
}

/// Index presence and counters for **`get_status`**.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetStatusOutput {
    /// Whether an index appears to exist for this project.
    pub indexed: bool,
    /// Approximate distinct files in the chunk store.
    pub file_count: u64,
    /// Number of stored chunks.
    pub chunk_count: u64,
    /// Last index time as an RFC 3339 / ISO-style string when available.
    pub last_indexed: Option<String>,
}

/// Arguments for the MCP tool **`clear_index`** (and CLI `cg clear`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClearIndexInput {
    /// Project root whose `.code-grasp/` data should be removed.
    pub path: String,
}

/// Acknowledgement after **`clear_index`** succeeds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClearIndexOutput {
    /// Short confirmation message.
    pub message: String,
}
