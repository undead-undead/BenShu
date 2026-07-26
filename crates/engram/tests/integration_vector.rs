use benshu_engram::prelude::*;
use benshu_engram::vector_store::{VectorMetric, VectorStore};
use benshu_inference::QuantLevel;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_vector_store_basic_lifecycle() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("vector.db");
    let store_coord = EngramStore::new(&db_path).unwrap();
    let kv = store_coord.kv_arc();

    // 384 dimensions (standard for bge-small)
    let store = VectorStore::new(kv.clone(), 384, 1000, VectorMetric::Cosine);

    // 1. Add some dummy vectors
    let v1 = vec![1.0; 384];
    let v2 = vec![-1.0; 384];

    store
        .add_at_level("test", "v1.md", "doc1", 0, v1.clone(), QuantLevel::Full)
        .unwrap();
    store
        .add_at_level("test", "v2.md", "doc2", 0, v2.clone(), QuantLevel::Full)
        .unwrap();

    assert_eq!(store.len(), 2);

    // 2. Search (Cosine similarity)
    let query = vec![0.9; 384];
    let results = store.search("test", &query, 10).unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].path, "v1.md");
    assert!(results[0].score > 0.99); // v1 is almost identical to query
    assert!(results[1].score < 0.0); // v2 is inverse of query
}

#[tokio::test]
async fn test_vector_quantization_migration() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("quant.db");
    let store_coord = EngramStore::new(&db_path).unwrap();
    let kv = store_coord.kv_arc();
    let store = VectorStore::new(kv.clone(), 384, 1000, VectorMetric::Cosine);

    // Initial: Full precision
    let v1 = vec![0.5; 384];
    store
        .add_at_level("migrate", "p1.md", "d1", 0, v1.clone(), QuantLevel::Full)
        .unwrap();

    // Migrate to Cold (Scalar Quantization)
    store
        .change_level("migrate", "p1.md", QuantLevel::Cold)
        .unwrap();

    // Verify it still works after migration
    let query = vec![0.5; 384];
    let results = store.search("migrate", &query, 1).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].level, QuantLevel::Cold);
    assert!(results[0].score > 0.95); // Some precision loss expected but should still rank high
}

#[tokio::test]
async fn test_vector_tombstones() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("tomb.db");
    let store_coord = EngramStore::new(&db_path).unwrap();
    let kv = store_coord.kv_arc();
    let store = VectorStore::new(kv.clone(), 384, 1000, VectorMetric::Cosine);

    store
        .add_at_level("del", "target.md", "d", 0, vec![1.0; 384], QuantLevel::Full)
        .unwrap();
    assert_eq!(store.len(), 1);

    let before = store.search("del", &vec![1.0; 384], 1).unwrap();
    assert_eq!(before.len(), 1);

    // Remove it
    store.remove("del", "target.md").unwrap();

    // Should be gone despite HNSW still having the ID (tombstone should catch it)
    let after = store.search("del", &vec![1.0; 384], 1).unwrap();
    assert_eq!(after.len(), 0);
}

#[tokio::test]
async fn test_vector_poincare_distance() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("hyperbolic.db");
    let store_coord = EngramStore::new(&db_path).unwrap();
    let kv = store_coord.kv_arc();

    // Poincare metric for hierarchical embeddings
    let store = VectorStore::new(kv.clone(), 128, 1000, VectorMetric::Poincare);

    // Close points in hyperbolic space
    let v1 = vec![0.1; 128];
    let v2 = vec![0.11; 128];
    // Far point
    let v3 = vec![0.9; 128];

    store
        .add_at_level("h", "p1", "d1", 0, v1.clone(), QuantLevel::Full)
        .unwrap();
    store
        .add_at_level("h", "p2", "d2", 0, v2.clone(), QuantLevel::Full)
        .unwrap();
    store
        .add_at_level("h", "p3", "d3", 0, v3.clone(), QuantLevel::Full)
        .unwrap();

    let results = store.search("h", &v1, 2).unwrap();
    assert_eq!(results[0].path, "p1");
    assert_eq!(results[1].path, "p2");
}
