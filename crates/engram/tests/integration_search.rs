use benshu_engram::prelude::*;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_hybrid_search_basic() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("search_test.db");

    // 1. Setup Engine (FTS only for unit test without embedding models)
    let mut config = HybridSearchConfig::default();
    config.db_path = db_path;
    config.use_vector = false; // Disable vector for basic FTS integration test

    let engine = HybridSearchEngine::new(config, None).expect("Engine init failed");

    // 2. Index diverse content
    let docs = vec![
        (
            "tech",
            "rust.md",
            "Rust Language",
            "Rust is a systems programming language that runs blazingly fast.",
        ),
        (
            "tech",
            "python.md",
            "Python Language",
            "Python is an interpreted, high-level, general-purpose programming language.",
        ),
        (
            "cooking",
            "pizza.md",
            "Pizza Recipe",
            "To make pizza, you need flour, water, yeast, and tomato sauce.",
        ),
    ];

    for (coll, path, title, text) in docs {
        engine
            .index_at_level(
                coll,
                path,
                title,
                text,
                benshu_inference::QuantLevel::Full,
                false,
                Default::default(),
            )
            .unwrap();
    }

    // 3. Test FTS Keyword Search
    let results = engine.search("systems programming", 5).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].document.title, "Rust Language");

    let cooking_results = engine.search("tomato sauce", 5).unwrap();
    assert_eq!(cooking_results[0].document.title, "Pizza Recipe");

    // 4. Test Collection Scope
    let tech_only = engine.search_in_collection("language", "tech", 5).unwrap();
    assert_eq!(tech_only.len(), 2);
    for res in tech_only {
        assert_eq!(res.document.collection, "tech");
    }
}

#[tokio::test]
async fn test_search_not_found() {
    let dir = tempdir().unwrap();
    let config = HybridSearchConfig {
        db_path: dir.path().join("empty.db"),
        use_vector: false,
        ..Default::default()
    };
    let engine = HybridSearchEngine::new(config, None).unwrap();

    let results = engine.search("nonexistent", 5).unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_search_unverified_filtering() {
    let dir = tempdir().unwrap();
    let config = HybridSearchConfig {
        db_path: dir.path().join("verify.db"),
        use_vector: false,
        ..Default::default()
    };
    let engine = HybridSearchEngine::new(config, None).unwrap();

    // Store one verified, one unverified
    engine
        .index_at_level(
            "c",
            "v.md",
            "Verified",
            "Content",
            benshu_inference::QuantLevel::Full,
            false,
            Default::default(),
        )
        .unwrap();
    engine
        .index_at_level(
            "c",
            "u.md",
            "Unverified",
            "Content",
            benshu_inference::QuantLevel::Full,
            true,
            Default::default(),
        )
        .unwrap();

    // Regular search should only return verified
    let results = engine.search("Content", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].document.title, "Verified");

    // But list_unverified should find the other
    let unverified = engine.list_unverified(10).unwrap();
    assert_eq!(unverified.len(), 1);
    assert_eq!(unverified[0].title, "Unverified");
}
