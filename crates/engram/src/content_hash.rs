//! Content-addressable storage (CAS) utilities
//!
//! High-performance hashing and unique ID management for Engram.

use sha2::{Digest, Sha256};
use std::fmt;
use tracing::trace;

/// Error type for document ID operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum DocIdError {
    #[error("Document ID length mismatch: expected {expected}, got {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("Invalid characters in Document ID (hex only)")]
    InvalidFormat,
    #[error("Empty input provided")]
    Empty,
}

/// A high-performance, collision-resistant Document ID
///
/// Uses 12 hex characters (6 bytes / 48 bits) to balance memory and safety.
/// Collision probability is extremely low for repositories up to 1M+ documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId([u8; 6]);

impl DocId {
    /// Create DocId from a hex string (handles normalization)
    pub fn from_hex(s: &str) -> Result<Self, DocIdError> {
        let clean = s.trim_start_matches('#').trim();
        if clean.is_empty() {
            return Err(DocIdError::Empty);
        }
        if clean.len() < 12 {
            return Err(DocIdError::LengthMismatch {
                expected: 12,
                actual: clean.len(),
            });
        }

        let mut bytes = [0u8; 6];
        for i in 0..6 {
            let chunk = &clean[i * 2..i * 2 + 2];
            bytes[i] = u8::from_str_radix(chunk, 16).map_err(|_| DocIdError::InvalidFormat)?;
        }
        Ok(Self(bytes))
    }

    /// Extract DocId from a SHA-256 hash prefix
    pub fn from_hash(hash: &str) -> Result<Self, DocIdError> {
        if hash.len() < 12 {
            return Err(DocIdError::LengthMismatch {
                expected: 12,
                actual: hash.len(),
            });
        }
        Self::from_hex(&hash[..12])
    }

    /// Fast hex representation without heavy formatting overhead
    pub fn to_string(&self) -> String {
        const HEX_CHARS: &[u8] = b"0123456789abcdef";
        let mut s = String::with_capacity(12);
        for &b in &self.0 {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
        }
        s
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// Compute SHA-256 of content and return hex string
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();

    // Fast hex encoding
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for &b in result.as_slice() {
        s.push(HEX_CHARS[(b >> 4) as usize] as char);
        s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    s
}

/// Primary entry point: Get standardized string docid from content hash
pub fn get_docid(hash: &str) -> String {
    // We default to returning the hex string for compatibility with Document struct,
    // but internally validate it represents a valid 12-char prefix.
    match DocId::from_hash(hash) {
        Ok(id) => id.to_string(),
        Err(_) => {
            // Fallback: if hash is malformed, still return a truncated version for safety,
            // but log a warning.
            trace!(
                "DocID extraction from hash '{}' failed, using raw truncation",
                hash
            );
            hash.chars().take(12).collect()
        }
    }
}

pub fn content_to_docid(content: &str) -> String {
    let hash = hash_content(content);
    get_docid(&hash)
}

pub fn normalize_docid(docid: &str) -> String {
    let clean = docid.trim_start_matches('#').trim();
    clean.chars().take(12).collect::<String>().to_lowercase()
}

pub fn validate_docid(docid: &str) -> bool {
    let clean = docid.trim_start_matches('#').trim();
    clean.len() >= 12 && clean[..12].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collision_space() {
        let content = "engram test content";
        let h = hash_content(content);
        let id = get_docid(&h);
        assert_eq!(id.len(), 12);
    }

    #[test]
    fn test_fast_hex() {
        let bytes = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34];
        let id = DocId(bytes);
        assert_eq!(id.to_string(), "deadbeef1234");
    }
}
