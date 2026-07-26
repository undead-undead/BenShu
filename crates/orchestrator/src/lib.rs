//! AgentOS Orchestration & Resource Arbitration (BenShu-ORCHESTRATOR)
//!
//! Manages hardware resource contention between Inference, Sensory, and Execute domains.
//! Implements the VRAM-Safe Fallback and pressure-aware scheduling.

use async_trait::async_trait;
use benshu_infra::traits::resource::{
    AllocationRequest, AllocationResponse, ResourceArbiterProvider, ResourceSensor, ThrottleLevel,
};
use benshu_infra::{HealthCheck, HealthStatus};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Resource arbitration strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrationStrategy {
    /// Prioritize Inference (LLM/Reasoning)
    InferenceFirst,
    /// Prioritize Sensory (Real-time Vision/Audio)
    SensoryFirst,
    /// Balanced allocation
    Balanced,
    /// Minimum power consumption
    Efficiency,
}

/// Resource control configuration per agent role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQuota {
    pub max_vram_mb: u64,
    pub priority: ThrottleLevel,
}

pub struct ResourceArbiter {
    strategy: RwLock<ArbitrationStrategy>,
    quotas: RwLock<HashMap<String, AgentQuota>>,
    active_allocations: RwLock<HashMap<String, (u64, u64)>>, // agent_id -> (vram, ram)
    total_allocated_vram: std::sync::atomic::AtomicU64,
    max_vram_mb: std::sync::atomic::AtomicU64,
    sensor: Option<Arc<RwLock<dyn ResourceSensor>>>,
}

#[async_trait::async_trait]
impl HealthCheck for ResourceArbiter {
    async fn check_health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
    fn module_name(&self) -> &'static str {
        "benshu-orchestrator"
    }
}

#[async_trait::async_trait]
impl ResourceArbiterProvider for ResourceArbiter {
    async fn request_allocation(&self, request: AllocationRequest) -> AllocationResponse {
        let _strategy = *self.strategy.read();
        let mut allocations = self.active_allocations.write();
        let quotas = self.quotas.read();

        // Phase 15.3: Physical VRAM HardCap (95%)
        if let Some(sensor_lock) = &self.sensor {
            let stats = sensor_lock.write().check_resources(false);
            if stats.vram_pressure_pct() > 95.0 {
                return AllocationResponse::Denied(
                    "Physical VRAM HardCap reached (95%+) - System Safety Triggered".into(),
                );
            }
        }

        // 1. Quota Check
        if let Some(quota) = quotas.get(&request.agent_id) {
            if request.vram_mb > quota.max_vram_mb && request.role != ThrottleLevel::High {
                return AllocationResponse::Denied(format!(
                    "Requested VRAM {}MB exceeds quota {}MB",
                    request.vram_mb, quota.max_vram_mb
                ));
            }
        }

        // 2. Global Strategy & Preemption
        let current_total = self
            .total_allocated_vram
            .load(std::sync::atomic::Ordering::Relaxed);

        // Simple logic for Phase 11.4:
        // If High priority (Commander) and we are near limit, try to deny or throttle others
        if request.role == ThrottleLevel::High {
            // Priority granted
            self.total_allocated_vram
                .fetch_add(request.vram_mb, std::sync::atomic::Ordering::SeqCst);
            allocations.insert(request.agent_id.clone(), (request.vram_mb, request.ram_mb));
            return AllocationResponse::Granted {
                allocated_vram: request.vram_mb,
                allocated_ram: request.ram_mb,
            };
        }

        // For non-high priority, check if we have space
        let max_vram = self.max_vram_mb.load(std::sync::atomic::Ordering::Relaxed);
        if current_total + request.vram_mb > max_vram && max_vram > 0 {
            return AllocationResponse::Throttled {
                wait_ms: 1000,
                suggested: (request.vram_mb / 2, request.ram_mb),
            };
        }

        self.total_allocated_vram
            .fetch_add(request.vram_mb, std::sync::atomic::Ordering::SeqCst);
        allocations.insert(request.agent_id.clone(), (request.vram_mb, request.ram_mb));

        AllocationResponse::Granted {
            allocated_vram: request.vram_mb,
            allocated_ram: request.ram_mb,
        }
    }

    async fn release_allocation(&self, agent_id: &str, vram_mb: u64, _ram_mb: u64) {
        let mut allocations = self.active_allocations.write();
        if let Some((old_vram, _)) = allocations.remove(agent_id) {
            self.total_allocated_vram
                .fetch_sub(old_vram, std::sync::atomic::Ordering::SeqCst);
        }
    }

    async fn update_allocation(&self, agent_id: &str, vram_mb: usize) {
        let mut allocations = self.active_allocations.write();
        let new_vram = vram_mb as u64;

        if let Some((old_vram, _)) = allocations.get_mut(agent_id) {
            // Update the global total first
            self.total_allocated_vram
                .fetch_sub(*old_vram, std::sync::atomic::Ordering::SeqCst);
            self.total_allocated_vram
                .fetch_add(new_vram, std::sync::atomic::Ordering::SeqCst);
            *old_vram = new_vram;
        } else {
            // Unregistered agent reporting: let's track it
            self.total_allocated_vram
                .fetch_add(new_vram, std::sync::atomic::Ordering::SeqCst);
            allocations.insert(agent_id.to_string(), (new_vram, 0));
        }
    }

    fn current_pressure(&self) -> ThrottleLevel {
        let strategy = *self.strategy.read();
        match strategy {
            ArbitrationStrategy::Efficiency => ThrottleLevel::Low,
            _ => ThrottleLevel::High,
        }
    }

    fn set_strategy(&self, strategy_name: &str) {
        let mut strategy = self.strategy.write();
        *strategy = match strategy_name {
            "inference" => ArbitrationStrategy::InferenceFirst,
            "sensory" => ArbitrationStrategy::SensoryFirst,
            "balanced" => ArbitrationStrategy::Balanced,
            "efficiency" => ArbitrationStrategy::Efficiency,
            _ => ArbitrationStrategy::Balanced,
        };
    }

    fn set_vram_limit(&self, vram_mb: u64) {
        self.max_vram_mb
            .store(vram_mb, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ResourceArbiter {
    pub fn new(
        strategy: ArbitrationStrategy,
        max_vram_mb: u64,
        sensor: Option<Arc<RwLock<dyn ResourceSensor>>>,
    ) -> Self {
        Self {
            strategy: RwLock::new(strategy),
            quotas: RwLock::new(HashMap::new()),
            active_allocations: RwLock::new(HashMap::new()),
            total_allocated_vram: std::sync::atomic::AtomicU64::new(0),
            max_vram_mb: std::sync::atomic::AtomicU64::new(max_vram_mb),
            sensor,
        }
    }

    pub fn set_max_vram_mb(&self, mb: u64) {
        self.max_vram_mb
            .store(mb, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn set_quota(&self, agent_id: String, quota: AgentQuota) {
        self.quotas.write().insert(agent_id, quota);
    }

    /// Evaluates current hardware pressure and returns an allocation decision
    pub fn decide_vram_allocation(&self, metrics: &HardwareMetrics) -> VramDecision {
        let strategy = *self.strategy.read();

        // Basic pressure logic
        if metrics.vram_pressure_pct > 90.0 {
            return VramDecision::CriticalThrottle;
        }

        match strategy {
            ArbitrationStrategy::InferenceFirst => VramDecision::AllowInference,
            _ => VramDecision::AllowBoth,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramDecision {
    AllowBoth,
    AllowInference,
    AllowSensory,
    CriticalThrottle,
    TriggerFallback,
    Recover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareMetrics {
    pub vram_pressure_pct: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
}
