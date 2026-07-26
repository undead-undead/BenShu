//! Document indexer for active knowledge ingestion
//!
//! Watches filesystem and indexes documents into the Engram store.

use crate::error::Result;
use crate::store::EngramStore;
use std::path::Path;
use tracing::info;

/// Default limit for file indexing to prevent OOM (20MB)
const MAX_INDEX_SIZE: u64 = 20 * 1024 * 1024;

/// Index a file into the store with robustness checks
pub fn index_file(store: &EngramStore, collection: &str, file_path: &Path) -> Result<bool> {
    // 1. Get file metadata
    let metadata = std::fs::metadata(file_path).map_err(|e| crate::error::EngramError::Io(e))?;

    // 2. Resource Guard: Size Check
    if metadata.len() > MAX_INDEX_SIZE {
        tracing::warn!(
            path = %file_path.display(),
            size = metadata.len(),
            "Skipping file: Exceeds 20MB limit"
        );
        return Ok(false);
    }

    // 3. Robust Path & Title Handling
    let path_str = file_path.to_string_lossy().to_string();
    let title = file_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());

    // 4. Incremental Check: mtime
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();

    if let Ok(Some(existing)) = store.get_by_path(collection, &path_str) {
        if let Some(existing_mtime) = existing.metadata.get("mtime") {
            if existing_mtime == &mtime {
                // Skip if not modified
                return Ok(false);
            }
        }
    }

    // 5. Robust Encoding: Byte-read + Lenient Decode
    let bytes = std::fs::read(file_path)?;
    let (content, _encoding_used, had_errors) = encoding_rs::UTF_8.decode(&bytes);

    if had_errors {
        tracing::debug!(path = %path_str, "File contains non-UTF8 sequence, used lenient decoding");
    }

    // 6. Store with mtime metadata for future incremental checks
    let mut doc_metadata = std::collections::HashMap::new();
    doc_metadata.insert("mtime".to_string(), mtime);
    doc_metadata.insert("size".to_string(), metadata.len().to_string());

    store.store_document(collection, &path_str, &title, &content, false, doc_metadata)?;

    info!("Indexed file: {}", path_str);
    Ok(true)
}

/// Index all matching files in a directory with progress feedback
pub fn index_directory(
    store: &EngramStore,
    collection: &str,
    dir: &Path,
    pattern: &str,
) -> Result<usize> {
    let glob_pattern = format!("{}/{}", dir.display(), pattern);
    let mut count = 0;
    let mut processed = 0;

    let entries: Vec<_> = glob::glob(&glob_pattern)
        .map_err(|e| crate::error::EngramError::InvalidInput(e.to_string()))?
        .collect();

    let total = entries.len();
    info!("Starting index: {} documents in target", total);

    for entry in entries {
        processed += 1;
        match entry {
            Ok(path) => {
                if path.is_file() {
                    match index_file(store, collection, &path) {
                        Ok(true) => count += 1,
                        Ok(false) => { /* Skipped (size or mtime) */ }
                        Err(e) => tracing::warn!("Failed to index {}: {}", path.display(), e),
                    }
                }
            }
            Err(e) => tracing::warn!("Glob entry error: {}", e),
        }

        // Periodic progress feedback for long-running batches
        if processed % 50 == 0 || processed == total {
            info!(
                "Progress: {}/{} processed, {} indexed",
                processed, total, count
            );
        }
    }

    info!(
        "Finished: Indexed {} new/updated files from {}",
        count,
        dir.display()
    );
    Ok(count)
}
