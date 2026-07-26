//! Text chunking for document indexing
//!
//! Splits documents into overlapping segments for vector search with high-precision
//! boundary detection and multi-language support.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, trace};
use unicode_normalization::UnicodeNormalization;

/// Configuration for the chunker
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChunkerConfig {
    /// Target chunk size (in characters)
    pub chunk_size: usize,
    /// Overlap between chunks (in characters)
    pub chunk_overlap: usize,
    /// Minimum valid chunk size
    pub min_chunk_size: usize,
    /// Maximum lookback distance for natural boundaries
    pub max_lookback: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 64,
            min_chunk_size: 50,
            max_lookback: 100,
        }
    }
}

/// A text chunk with precise positional metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub text: String,
    /// Absolute start offset in original text (in characters)
    pub start_offset: usize,
    /// Absolute end offset in original text (in characters)
    pub end_offset: usize,
    pub sequence: usize,
    pub is_natural: bool,
}

impl Chunk {
    /// Precise trimming that maintains coordinate integrity
    pub fn smart_trim(&mut self) {
        let original = self.text.clone();
        let trimmed = original.trim_start();

        let lead_spaces = original.chars().count() - trimmed.chars().count();
        if lead_spaces > 0 {
            self.start_offset += lead_spaces;
        }

        let final_trimmed = trimmed.trim_end();
        let tail_spaces = trimmed.chars().count() - final_trimmed.chars().count();
        if tail_spaces > 0 {
            self.end_offset -= tail_spaces;
        }

        self.text = final_trimmed.to_string();
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChunkStats {
    pub total_chunks: usize,
    pub avg_size: f64,
    pub natural_splits: usize,
}

pub struct Chunker {
    config: Arc<ChunkerConfig>,
    boundaries: HashSet<char>,
}

impl Chunker {
    pub fn new(config: ChunkerConfig) -> Self {
        let boundaries = HashSet::from(['。', '！', '？', '；', '：', '…', '.', '!', '?', '\n']);
        Self {
            config: Arc::new(config),
            boundaries,
        }
    }

    /// Optimized chunking using character iterators to avoid heavy allocations
    pub fn chunk(&self, text: &str) -> (Vec<Chunk>, ChunkStats) {
        let start_time = std::time::Instant::now();
        let all_chars: Vec<char> = text.chars().collect();
        let total_len = all_chars.len();

        let mut chunks = Vec::new();
        let mut stats = ChunkStats::default();

        if total_len == 0 {
            return (chunks, stats);
        }

        let mut start = 0;
        let mut sequence = 0;

        while start < total_len {
            let mut end = (start + self.config.chunk_size).min(total_len);
            let mut is_natural = false;

            // Search for natural boundary within lookback window
            if end < total_len {
                let lookback_limit = end.saturating_sub(self.config.max_lookback).max(start);
                for i in (lookback_limit..end).rev() {
                    if self.boundaries.contains(&all_chars[i]) {
                        end = i + 1;
                        is_natural = true;
                        break;
                    }
                }
            } else {
                is_natural = true; // End of document is a natural boundary
            }

            let content: String = all_chars[start..end].iter().collect();
            let mut chunk = Chunk {
                text: content,
                start_offset: start,
                end_offset: end,
                sequence,
                is_natural,
            };

            chunk.smart_trim();

            if chunk.len() >= self.config.min_chunk_size
                || (end == total_len && !chunk.text.is_empty())
            {
                if is_natural {
                    stats.natural_splits += 1;
                }
                chunks.push(chunk);
                sequence += 1;
            }

            // Move start pointer considering overlap
            let next_start = if end >= total_len {
                total_len
            } else {
                end.saturating_sub(self.config.chunk_overlap).max(start + 1)
            };

            if next_start <= start && end < total_len {
                // Safety: Ensure forward progress
                start = end;
            } else {
                start = next_start;
            }
        }

        stats.total_chunks = chunks.len();
        if stats.total_chunks > 0 {
            stats.avg_size =
                chunks.iter().map(|c| c.len()).sum::<usize>() as f64 / stats.total_chunks as f64;
        }

        debug!(
            "Chunking completed in {:.2}ms: {} chunks produced",
            start_time.elapsed().as_secs_f64() * 1000.0,
            chunks.len()
        );

        (chunks, stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_trim_coordinates() {
        let mut chunk = Chunk {
            text: "  hello  ".to_string(),
            start_offset: 10,
            end_offset: 19,
            sequence: 0,
            is_natural: true,
        };
        chunk.smart_trim();
        assert_eq!(chunk.text, "hello");
        assert_eq!(chunk.start_offset, 12);
        assert_eq!(chunk.end_offset, 17);
    }

    #[test]
    fn test_natural_boundary_detection() {
        let chunker = Chunker::new(ChunkerConfig {
            chunk_size: 20,
            chunk_overlap: 5,
            max_lookback: 10,
            min_chunk_size: 5,
            ..Default::default()
        });
        let text = "Hello world. This is a test for chunker.";
        let (chunks, _) = chunker.chunk(text);

        // Should break after "Hello world."
        assert!(chunks[0].text.contains("Hello world."));
        assert!(chunks[0].is_natural);
    }
}
