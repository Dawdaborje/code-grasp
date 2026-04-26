//! # `cg_proto`
//!
//! Serializable types shared by the **`cg`** CLI and **`code-grasp-mcp`** server. Each struct maps
//! to an MCP tool request or response body and carries [`schemars::JsonSchema`] for tool
//! registration with hosts that support JSON Schema.
//!
//! ## MCP tools
//!
//! | Tool | Input | Output |
//! |------|-------|--------|
//! | `index_codebase` | [`IndexCodebaseInput`] | [`IndexCodebaseOutput`] |
//! | `search_code` | [`SearchCodeInput`] | [`SearchCodeOutput`] |
//! | `get_status` | [`GetStatusInput`] | [`GetStatusOutput`] |
//! | `clear_index` | [`ClearIndexInput`] | [`ClearIndexOutput`] |
//!
//! ## Documentation
//!
//! ```text
//! cargo doc -p cg_proto --no-deps --open
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod types;

pub use types::*;
