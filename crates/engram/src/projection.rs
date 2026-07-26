//! Phase 19.1: Hierarchical File Projection via Poincaré Embedding.
//!
//! Converts a hierarchical tree (like a Windows Filesystem) into low-dimensional
//! coordinates in a Poincaré Ball. This avoids "crowding" and allows Jarvis to
//! understand global file topology at scale.

use crate::error::Result;
use crate::vector_store::VectorStore;
use crate::QuantLevel;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

pub struct TreeProjector {
    dimension: usize,
    /// Distance factor for each depth level (how fast we move toward the edge)
    depth_factor: f32,
    /// Cache of mtimes to prevent redundant projection
    projected_files: std::sync::Arc<parking_lot::RwLock<HashMap<String, u64>>>,
}

impl TreeProjector {
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            depth_factor: 0.8,
            projected_files: std::sync::Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Projects an entire filesystem tree into the VectorStore with production optimization.
    pub async fn project_filesystem_tree(&self, root: &Path, vs: &VectorStore) -> Result<usize> {
        info!(
            "Initiating Production-grade Poincaré Projection for tree: {}",
            root.display()
        );
        let mut count = 0;
        let root_coords = vec![0.0f32; self.dimension];

        self.project_recursive(root, &root_coords, 0, vs, &mut count)
            .await?;

        info!(
            "Successfully projected {} nodes. Filesystem topology is now live.",
            count
        );
        Ok(count)
    }

    #[async_recursion::async_recursion]
    async fn project_recursive(
        &self,
        current: &Path,
        parent_coords: &[f32],
        depth: usize,
        vs: &VectorStore,
        count: &mut usize,
    ) -> Result<()> {
        let current_path = current.to_path_buf();
        let entries = tokio::task::spawn_blocking(move || {
            std::fs::read_dir(current_path).map(|e| e.filter_map(|r| r.ok()).collect::<Vec<_>>())
        })
        .await
        .ok()
        .and_then(|result| result.ok())
        .unwrap_or_default();

        let num_children = entries.len();
        if num_children == 0 {
            return Ok(());
        }

        // Parallel processing of siblings using Rayon for heavy math
        let children_data: Vec<(PathBuf, Vec<f32>, String, u64)> = entries
            .par_iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                let mtime = metadata
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs();
                let path_str = path.to_string_lossy().to_string();

                // Incremental check: Skip if mtime matches
                {
                    let cache = self.projected_files.read();
                    if let Some(&old_mtime) = cache.get(&path_str) {
                        if old_mtime == mtime {
                            return None;
                        }
                    }
                }

                let coords = self.compute_child_coords(parent_coords, i, num_children, depth + 1);
                Some((path, coords, path_str, mtime))
            })
            .collect();

        for (child_path, child_coords, path_str, mtime) in children_data {
            let key = format!("tree:{}", path_str);

            // Production Fix: Correct add_at_level signature
            match vs.add_at_level(
                "filesystem",           // collection
                &path_str,              // path
                &key,                   // docid
                0,                      // chunk_seq
                child_coords.clone(),   // embedding
                QuantLevel::Background, // level
            ) {
                Ok(_) => {
                    self.projected_files.write().insert(path_str, mtime);
                    *count += 1;
                }
                Err(e) => warn!("Failed to project node {}: {}", path_str, e),
            }

            if child_path.is_dir() && depth < 32 {
                self.project_recursive(&child_path, &child_coords, depth + 1, vs, count)
                    .await?;
            }
        }

        Ok(())
    }

    /// Möbius-based Hyperbolic Projection
    fn compute_child_coords(
        &self,
        parent: &[f32],
        index: usize,
        total: usize,
        depth: usize,
    ) -> Vec<f32> {
        let mut child = parent.to_vec();
        let radius = 1.0 - (self.depth_factor).powi(depth as i32);

        let parent_norm_sq: f32 = parent.iter().map(|x| x * x).sum();
        let parent_norm = parent_norm_sq.sqrt();

        // Generate branch vector (Mobius Step)
        let step = self.mobius_step(index, total, depth);

        // Apply Mobius Addition
        for (i, (p, s)) in parent.iter().zip(step.iter()).enumerate() {
            child[i] = self.mobius_add(*p, *s, parent_norm);
        }

        // Normalize to target radius for strict hierarchy
        let child_norm_sq: f32 = child.iter().map(|x| x * x).sum();
        let child_norm = child_norm_sq.sqrt().max(1e-6);
        let scale = radius / child_norm;

        for x in child.iter_mut() {
            *x *= scale;
        }
        child
    }

    fn mobius_add(&self, a: f32, b: f32, norm: f32) -> f32 {
        let eps = 1e-4;
        // Simple scalar approximation of Poincare Mobius addition for axis-aligned branches
        (a + b) / (1.0 + a * b * norm + eps)
    }

    fn mobius_step(&self, index: usize, total: usize, depth: usize) -> Vec<f32> {
        let mut step = vec![0.0f32; self.dimension];
        let angle = (index as f32 / total as f32) * 2.0 * std::f32::consts::PI;

        // Angular separation scales with depth to prevent overcrowding near origin
        let magnitude = 0.2 / (depth as f32).sqrt();

        if self.dimension >= 2 {
            step[0] = magnitude * angle.cos();
            step[1] = magnitude * angle.sin();
        }

        for i in 2..self.dimension {
            step[i] = (magnitude * 0.1) * (index + i) as f32 / (total + 1) as f32;
        }
        step
    }
}
