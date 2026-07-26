use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::RwLock;

/// Hardware utilization metrics (CPU/RAM/GPU/VRAM)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareMetrics {
    pub cpu_load_percent: f32,
    pub ram_total_bytes: u64,
    pub ram_used_bytes: u64,
    pub gpu_vram_total_bytes: Option<u64>,
    pub gpu_vram_used_bytes: Option<u64>,
    pub gpu_util_percent: Option<f32>,
    pub thermal_cpu_temp: Option<f32>,
}

/// Dynamic Agent process metrics (Memory consumption)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessMetrics {
    pub pid: u32,
    pub memory_rss_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub cpu_usage: f32,
}

pub struct MetricsMonitor {
    sys: Arc<RwLock<System>>,
}

impl MetricsMonitor {
    pub async fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys: Arc::new(RwLock::new(sys)),
        }
    }

    pub async fn snapshot_hardware(&self) -> HardwareMetrics {
        let mut sys = self.sys.write().await;
        sys.refresh_all();

        HardwareMetrics {
            cpu_load_percent: sys.global_cpu_usage(),
            ram_total_bytes: sys.total_memory(),
            ram_used_bytes: sys.used_memory(),
            gpu_vram_total_bytes: None, // Requires NVML/Metal binding (Phase 22+)
            gpu_vram_used_bytes: None,
            gpu_util_percent: None,
            thermal_cpu_temp: None,
        }
    }
}
