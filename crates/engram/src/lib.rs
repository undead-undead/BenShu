#![cfg_attr(docsrs, feature(doc_cfg))]

//! # Engram — High-Performance Memory Engine for BenShu
//!
//! Engram is a sophisticated, agent-centric knowledge storage and retrieval engine designed
//! for long-term multi-user operations on consumer hardware.
//!
//! ## Core Architecture
//! - **Tiered Storage**: Abstract (L0), Overview (L1), and Full Content (L2).
//! - **Hybrid Retrieval**: Seamlessly fuses BM25 keyword search with Vector semantic similarity.
//! - **Resource Efficiency**: LRU-cached posting lists and quantized vector search for low memory footprint.
//! - **Active Learning**: Automatic indexing, deduplication, and cognitive experience tracking.
//!
//! ## Quick Start
//! ```rust,no_run
//! use benshu_engram::{EngramStore, HybridSearchEngine, HybridSearchConfig};
//! use std::path::PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Initialize the store
//! let store = EngramStore::new("engram.db")?;
//!
//! // 2. Configure the search engine
//! let config = HybridSearchConfig::default();
//! let engine = HybridSearchEngine::new(config, None)?;
//!
//! // 3. Index a document
//! store.store_document(
//!     "knowledge",
//!     "rust-basics.md",
//!     "Rust Basics",
//!     "Rust is a systems programming language...",
//!     false,
//!     Default::default()
//! )?;
//!
//! // 4. Search
//! let results = engine.search("systems programming", 5)?;
//! println!("Found {} results", results.len());
//! # Ok(())
//! # }
//! ```

// --- Core Logic ---
pub mod content_hash;
pub mod error;
pub mod storage;
pub mod store;

// --- Search & Retrieval ---
pub mod fts;
pub mod hybrid_search;
pub mod model_pool;
pub mod reranker;
pub mod rrf;

// --- Vector Search (Feature Gated) ---
#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub mod embedder;
#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub mod local_reranker;
#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub mod vector_store;

// --- Agent & File System Integration ---
pub mod agent_memory;
pub mod chunker;
pub mod indexer;
pub mod intent;
mod metadata_contract;
pub mod projection;
pub mod retriever;
mod runtime_bridge;
pub mod tool;
pub mod virtual_path;
pub mod watcher;

// --- Grouped Re-exports ---

/// Error handling and results
pub mod prelude {
    pub use crate::error::{EngramError, Result};
    pub use crate::hybrid_search::{
        HybridSearchConfig, HybridSearchEngine, HybridSearchResult, HybridSearchStats,
    };
    pub use crate::store::{Collection, Document, EngramStore, StoreStats};
}

// Flatten most common types to root for "Direct Access"
pub use crate::prelude::*;

// Storage & Low-level
pub use content_hash::{get_docid, hash_content, normalize_docid, validate_docid};
pub use storage::redb_impl::EngramKV;
pub use storage::Storage;

// Models & Ranking
pub use model_pool::{ModelPool, ModelResource};
pub use reranker::{NoOpReranker, Reranker};
pub use rrf::{FusedResult, RrfConfig, RrfFusion};

// Intelligence & Retrieval
pub use chunker::{Chunk, ChunkStats, Chunker, ChunkerConfig};
pub use intent::{ContextType, IntentAnalyzer, QueryPlan, TypedQuery};
pub use retriever::{HierarchicalRetriever, RecursiveSearchOutcome, RetrievalReport};
pub use tool::KnowledgeSearchTool;

// Agent & Systems
pub use agent_memory::EngramMemory;
pub use virtual_path::VirtualPath;
pub use watcher::FileWatcher;

// External Inference Re-exports
pub use benshu_inference::{
    dot_product_int4_f32, CachePage, InferenceConfig, KvEngine, QuantLevel, Quantizer,
    ScalarQuantizer, TernaryQuantizer,
};

#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub use embedder::Embedder;
#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub use local_reranker::LocalCandleReranker;
#[cfg(feature = "vector")]
#[cfg_attr(docsrs, doc(cfg(feature = "vector")))]
pub use vector_store::{VectorEntry, VectorQuantMetadata, VectorStore};
