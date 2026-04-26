//! SHA-256 helpers for file content hashing.

use sha2::{Digest, Sha256};

/// Returns lowercase hex SHA-256 of `bytes`.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
