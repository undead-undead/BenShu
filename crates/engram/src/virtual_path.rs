//! Virtual path handling for BenShu Engram
//!
//! Standardizes logical access to knowledge via benshu:// URIs.
//! Fully aligned with storage key patterns (collection:path).

use crate::error::{EngramError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{debug, warn};
use unicode_normalization::UnicodeNormalization as _;

/// VirtualPath represents a logical URI: benshu://collection/path/file.ext
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct VirtualPath {
    pub collection: String,
    pub path: String,
}

impl VirtualPath {
    /// Internal validation only allows certain segments
    fn is_safe_segment(s: &str) -> bool {
        // Prevent empty, traversal, or hidden file patterns
        !s.is_empty() && s != ".." && s != "."
    }

    /// Optimized parser with strict traversal protection
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        let normalized: String = trimmed.nfc().collect();

        // Strip prefix
        let rest = if let Some(r) = normalized.strip_prefix("benshu://") {
            r
        } else if let Some(r) = normalized.strip_prefix("//") {
            r
        } else {
            return Err(EngramError::InvalidPath("Missing benshu:// prefix".into()));
        };

        // Split collection and path
        let (collection, inner_path) = match rest.split_once('/') {
            Some((c, p)) => (c, p),
            None => (rest, ""),
        };

        // Validate collection
        if collection.is_empty() || collection.contains(|c: char| c.is_whitespace() || c == ':') {
            return Err(EngramError::InvalidPath("Invalid collection name".into()));
        }

        // Validate path segments for traversal
        if !inner_path.is_empty() {
            for segment in inner_path.split('/') {
                if !Self::is_safe_segment(segment) && !segment.is_empty() {
                    warn!("Blocked path traversal attempt: {}", inner_path);
                    return Err(EngramError::InvalidPath("Path traversal detected".into()));
                }
            }
        }

        Ok(Self {
            collection: collection.to_string(),
            path: inner_path.trim_matches('/').to_string(),
        })
    }

    /// Build a virtual path from components
    pub fn build(collection: &str, path: &str) -> String {
        let path = path.trim_matches('/');
        if path.is_empty() {
            format!("benshu://{}", collection)
        } else {
            format!("benshu://{}/{}", collection, path)
        }
    }

    /// Aligns with EngramStore's internal key format (collection:path)
    pub fn to_storage_key(&self) -> String {
        format!("{}:{}", self.collection, self.path)
    }

    /// Human-friendly display (collection/path)
    pub fn display(&self) -> String {
        if self.path.is_empty() {
            self.collection.clone()
        } else {
            format!("{}/{}", self.collection, self.path)
        }
    }

    /// Get extension (lowercase)
    pub fn extension(&self) -> Option<String> {
        self.path
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_lowercase())
    }
}

impl fmt::Display for VirtualPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::build(&self.collection, &self.path))
    }
}

impl std::str::FromStr for VirtualPath {
    type Err = EngramError;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_parsing() {
        // Should work with dots (file extensions)
        let vp = VirtualPath::parse("benshu://trading/sol.md").unwrap();
        assert_eq!(vp.collection, "trading");
        assert_eq!(vp.path, "sol.md");
        assert_eq!(vp.to_storage_key(), "trading:sol.md");

        // Should block traversal
        assert!(VirtualPath::parse("benshu://trading/../etc/passwd").is_err());
        assert!(VirtualPath::parse("benshu://trading/./hidden").is_err());
    }

    #[test]
    fn test_storage_alignment() {
        let vp = VirtualPath {
            collection: "docs".into(),
            path: "intro.md".into(),
        };
        assert_eq!(vp.to_storage_key(), "docs:intro.md");
    }
}
