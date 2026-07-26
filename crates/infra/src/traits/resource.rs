use serde::{Deserialize, Serialize};

/// Resource throttling priority levels (Roadmap Phase 7.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThrottleLevel {
    /// Foreground / Interactive (e.g. Real-time inference)
    High,
    /// Standard Task (Default)
    Medium,
    /// Background / Swarm mission (Minimizes host impact)
    Low,
}

impl Default for ThrottleLevel {
    fn default() -> Self {
        Self::Medium
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostResources {
    pub cpu_usage: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub free_memory_pct: f32,
    pub is_low_memory: bool,
    pub total_disk_gb: u64,
    pub used_disk_gb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_core_cpu: Option<Vec<f32>>,

    // Extensions (e.g., GPU/NPU)
    #[serde(default)]
    pub accelerators: Vec<AcceleratorInfo>,
}

impl HostResources {
    pub fn vram_pressure_pct(&self) -> f32 {
        self.accelerators
            .iter()
            .map(|acc| acc.vram_pressure_pct)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }

    /// Phase 15.3: Calculate a safe threshold based on total system VRAM
    /// Stricter for small cards (4GB), more relaxed for large cards (24GB).
    pub fn dynamic_metabolic_threshold(&self) -> f32 {
        let total_vram_gb = self
            .accelerators
            .iter()
            .map(|acc| acc.vram_total_mb)
            .sum::<u64>() as f32
            / 1024.0;

        if total_vram_gb < 6.0 {
            75.0 // High pressure for 4GB-6GB cards
        } else if total_vram_gb < 12.0 {
            85.0 // Moderate pressure for 8GB-12GB cards
        } else {
            92.0 // Relaxed for 16GB+ cards
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratorInfo {
    pub name: String,
    pub kind: String, // "gpu", "npu", "tpu"
    pub vram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_pressure_pct: f32,
}

pub trait ResourceSensor: Send + Sync {
    fn check_resources(&mut self, detailed: bool) -> HostResources;

    fn suggest_throttle_level(&mut self, config_threshold: Option<f32>) -> ThrottleLevel {
        let stats = self.check_resources(false);
        let low_threshold = config_threshold.unwrap_or_else(|| stats.dynamic_metabolic_threshold());
        let medium_threshold = low_threshold * 0.8;

        // 1. High pressure (Configurable) -> Low (Strict Throttling)
        if stats.cpu_usage > low_threshold || (100.0 - stats.free_memory_pct) > low_threshold {
            return ThrottleLevel::Low;
        }

        // Check accelerators for pressure
        for acc in &stats.accelerators {
            if acc.vram_pressure_pct > low_threshold {
                return ThrottleLevel::Low;
            }
        }

        // 2. Moderate pressure -> Medium
        if stats.cpu_usage > medium_threshold || (100.0 - stats.free_memory_pct) > medium_threshold
        {
            return ThrottleLevel::Medium;
        }

        for acc in &stats.accelerators {
            if acc.vram_pressure_pct > (medium_threshold + 10.0).min(90.0) {
                return ThrottleLevel::Medium;
            }
        }

        // 3. Idle / Plenty of resources -> High (Full Speed)
        ThrottleLevel::High
    }
}

/// Request for hardware resource allocation (Phase 11.4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationRequest {
    pub agent_id: String,
    pub role: ThrottleLevel, // Resource priority tied to throttle level
    pub vram_mb: u64,
    pub ram_mb: u64,
    pub cpu_cores: Option<f32>,
}

/// Response from ResourceArbiter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AllocationResponse {
    Granted {
        allocated_vram: u64,
        allocated_ram: u64,
    },
    Throttled {
        wait_ms: u64,
        suggested: (u64, u64),
    },
    Denied(String),
}

/// ResourceArbiter Trait for Cross-Domain Resource Governance
#[async_trait::async_trait]
pub trait ResourceArbiterProvider: Send + Sync {
    /// Request an allocation for a specific agent task
    async fn request_allocation(&self, request: AllocationRequest) -> AllocationResponse;

    /// Release resources after task completion
    async fn release_allocation(&self, agent_id: &str, vram_mb: u64, ram_mb: u64);

    /// Dynamic usage update for autonomous governance (Phase 10)
    async fn update_allocation(&self, agent_id: &str, vram_mb: usize);

    /// Check current pressure-aware status
    fn current_pressure(&self) -> ThrottleLevel;

    /// Update dynamic strategy (InferenceFirst, SensoryFirst, etc.)
    fn set_strategy(&self, strategy_name: &str);

    /// Update the global VRAM budget for agent inference
    fn set_vram_limit(&self, vram_mb: u64);
}
