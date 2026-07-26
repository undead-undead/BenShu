//! Hardware detection and optimization strategies.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use sysinfo::System;
use tracing::warn;

lazy_static::lazy_static! {
    static ref HARDWARE_STATUS_CACHE: Mutex<Option<(Instant, HardwareStatus)>> = Mutex::new(None);
}

const DETECT_CACHE_TTL: Duration = Duration::from_secs(2);
const FALLBACK_RAM_BUDGET_MB: u64 = 16 * 1024;
const FALLBACK_VRAM_BUDGET_MB: u64 = 8 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GpuProbeConfidence {
    Native,
    Tooling,
    Heuristic,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GpuProbeSource {
    Dxgi,
    Wmic,
    NvidiaSmi,
    RocmSmi,
    RocmInfo,
    Lspci,
    AppleUnifiedMemory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccelerationProfile {
    CudaPreferred,
    VulkanPreferred,
    MetalPreferred,
    CpuOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryTopology {
    DedicatedGpu,
    SharedGpu,
    UnifiedMemory,
    CpuOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HardwareBudgets {
    pub max_vram_bytes: u64,
    pub max_ram_bytes: u64,
    pub separate_vram_pool: bool,
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareTelemetry {
    pub acceleration_profile: AccelerationProfile,
    pub gpu_vendor: Option<GpuVendor>,
    pub gpu_probe_confidence: GpuProbeConfidence,
    pub gpu_probe_source: Option<GpuProbeSource>,
    pub memory_topology: MemoryTopology,
    pub vram_total_mb: u64,
    pub vram_budget_mb: Option<u64>,
    pub vram_used_mb: u64,
    pub shared_memory_total_mb: Option<u64>,
    pub shared_memory_budget_mb: Option<u64>,
    pub ram_total_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareStatus {
    pub has_gpu: bool,
    pub gpu_name: Option<String>,
    pub gpu_vendor: Option<GpuVendor>,
    pub gpu_probe_confidence: GpuProbeConfidence,
    pub gpu_probe_source: Option<GpuProbeSource>,
    pub memory_topology: MemoryTopology,
    pub vram_total_mb: u64,
    pub vram_budget_mb: Option<u64>,
    pub vram_used_mb: u64,
    pub shared_memory_total_mb: Option<u64>,
    pub shared_memory_budget_mb: Option<u64>,
    pub vulkan_supported: bool,
    pub cpu_cores: usize,
    pub ram_total_mb: u64,
    pub avx512_supported: bool,
    pub vnni_supported: bool,
    pub amx_supported: bool,
    pub cuda_available: bool,
    pub rocm_available: bool,
    pub gpu_compute_capability: Option<(u32, u32)>,
}

impl HardwareStatus {
    pub fn detect() -> Self {
        if let Some((captured_at, status)) = HARDWARE_STATUS_CACHE.lock().clone() {
            if captured_at.elapsed() < DETECT_CACHE_TTL {
                return status;
            }
        }

        let mut sys = System::new_all();
        sys.refresh_all();
        let status = Self::detect_with_system(&mut sys);
        *HARDWARE_STATUS_CACHE.lock() = Some((Instant::now(), status.clone()));
        status
    }

    pub fn detect_with_system(sys: &mut System) -> Self {
        sys.refresh_all();
        let cpu_cores = sys.cpus().len();
        let ram_total_mb = sys.total_memory() / (1024 * 1024);

        let vulkan_supported = check_vulkan();

        // Consolidated GPU/VRAM Detection - Passing sys to reuse memory info for fallbacks
        let probe = get_gpu_vram_consolidated(vulkan_supported, sys);

        let (avx512_supported, vnni_supported, amx_supported) = detect_cpu_features();

        #[allow(unused_mut)]
        let mut status = Self {
            has_gpu: probe.has_gpu,
            gpu_name: probe.gpu_name,
            gpu_vendor: probe.gpu_vendor,
            gpu_probe_confidence: probe.gpu_probe_confidence,
            gpu_probe_source: probe.gpu_probe_source,
            memory_topology: probe.memory_topology,
            vram_total_mb: probe.vram_total_mb,
            vram_budget_mb: probe.vram_budget_mb,
            vram_used_mb: probe.vram_used_mb,
            shared_memory_total_mb: probe.shared_memory_total_mb,
            shared_memory_budget_mb: probe.shared_memory_budget_mb,
            vulkan_supported,
            cpu_cores,
            ram_total_mb,
            avx512_supported,
            vnni_supported,
            amx_supported,
            cuda_available: candle_core::utils::cuda_is_available(),
            rocm_available: detect_rocm_runtime(),
            gpu_compute_capability: None,
        };

        // Refined CUDA capability detection
        #[cfg(feature = "cuda")]
        {
            if status.cuda_available {
                if let Ok(candle_core::Device::Cuda(cuda_dev)) = candle_core::Device::new_cuda(0) {
                    status.gpu_compute_capability =
                        Some((cuda_dev.major() as u32, cuda_dev.minor() as u32));
                }
            }
        }

        status
    }

    pub fn acceleration_profile(&self) -> AccelerationProfile {
        if self.has_gpu
            && candle_core::utils::metal_is_available()
            && matches!(self.gpu_vendor, Some(GpuVendor::Apple))
        {
            AccelerationProfile::MetalPreferred
        } else if self.has_gpu
            && self.cuda_available
            && matches!(self.gpu_vendor, Some(GpuVendor::Nvidia))
        {
            AccelerationProfile::CudaPreferred
        } else if self.has_gpu && self.vulkan_supported {
            AccelerationProfile::VulkanPreferred
        } else if self.has_gpu && candle_core::utils::metal_is_available() {
            AccelerationProfile::MetalPreferred
        } else {
            AccelerationProfile::CpuOnly
        }
    }

    pub fn supports_tensorrt(&self) -> bool {
        self.has_gpu
            && self.cuda_available
            && matches!(self.gpu_vendor, Some(GpuVendor::Nvidia))
            && self.gpu_compute_capability.unwrap_or((0, 0)).0 >= 7
    }

    pub fn telemetry(&self) -> HardwareTelemetry {
        HardwareTelemetry {
            acceleration_profile: self.acceleration_profile(),
            gpu_vendor: self.gpu_vendor,
            gpu_probe_confidence: self.gpu_probe_confidence,
            gpu_probe_source: self.gpu_probe_source,
            memory_topology: self.memory_topology,
            vram_total_mb: self.vram_total_mb,
            vram_budget_mb: self.vram_budget_mb,
            vram_used_mb: self.vram_used_mb,
            shared_memory_total_mb: self.shared_memory_total_mb,
            shared_memory_budget_mb: self.shared_memory_budget_mb,
            ram_total_mb: self.ram_total_mb,
        }
    }

    pub fn current_vram_usage() -> u64 {
        Self::detect().vram_used_mb
    }

    pub fn budgets(&self) -> HardwareBudgets {
        let ram_total_mb = if self.ram_total_mb > 0 {
            self.ram_total_mb
        } else {
            FALLBACK_RAM_BUDGET_MB
        };
        let max_ram_bytes = ((ram_total_mb * 1024 * 1024) as u128 * 8 / 10) as u64;

        let has_dedicated_vram = matches!(self.memory_topology, MemoryTopology::DedicatedGpu);
        let effective_vram_mb = if has_dedicated_vram {
            self.vram_budget_mb
                .filter(|budget| *budget > 0)
                .map(|budget| {
                    if self.vram_total_mb > 0 {
                        budget.min(self.vram_total_mb)
                    } else {
                        budget
                    }
                })
                .unwrap_or(self.vram_total_mb)
        } else if self.has_gpu && self.vram_total_mb > 0 {
            0
        } else {
            0
        };
        let max_vram_bytes = if has_dedicated_vram {
            ((effective_vram_mb * 1024 * 1024) as u128 * 9 / 10) as u64
        } else {
            0
        };

        HardwareBudgets {
            max_vram_bytes: if has_dedicated_vram {
                max_vram_bytes
            } else if self.has_gpu {
                0
            } else {
                0
            },
            max_ram_bytes,
            separate_vram_pool: has_dedicated_vram,
            used_fallback: self.ram_total_mb == 0,
        }
    }

    pub fn suggest_quantization(&self, param_count_billions: f32) -> String {
        // Safe factor (90% VRAM, 80% RAM) to account for OS overhead
        let vram_available = (self.vram_total_mb as f32 * 0.9) / 1024.0;
        let ram_available = (self.ram_total_mb as f32 * 0.8) / 1024.0;

        let fp16_req = param_count_billions * 2.0;
        let q8_req = param_count_billions * 1.0;
        let q4_req = param_count_billions * 0.55;
        let q2_req = param_count_billions * 0.35;

        if vram_available >= fp16_req {
            "F16 (Maximum Precision)".to_string()
        } else if vram_available >= q8_req {
            "Q8_0 (High Quality)".to_string()
        } else if vram_available >= q4_req {
            "Q4_K_M (Balanced)".to_string()
        } else if ram_available >= q4_req {
            "Q4_K_M (CPU Optimized)".to_string()
        } else if ram_available >= q2_req {
            "IQ2_XS (Low Memory Mode)".to_string()
        } else {
            "Model exceeds available memory limits".to_string()
        }
    }

    pub fn device(&self) -> candle_core::Device {
        match self.acceleration_profile() {
            AccelerationProfile::CudaPreferred => match candle_core::Device::new_cuda(0) {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        "CUDA detected but failed to initialize: {}. Falling back to CPU.",
                        e
                    );
                    candle_core::Device::Cpu
                }
            },
            AccelerationProfile::MetalPreferred => match candle_core::Device::new_metal(0) {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        "Metal detected but failed to initialize: {}. Falling back to CPU.",
                        e
                    );
                    candle_core::Device::Cpu
                }
            },
            AccelerationProfile::VulkanPreferred | AccelerationProfile::CpuOnly => {
                candle_core::Device::Cpu
            }
        }
    }
}

#[derive(Debug, Clone)]
struct GpuProbeResult {
    has_gpu: bool,
    gpu_name: Option<String>,
    gpu_vendor: Option<GpuVendor>,
    gpu_probe_confidence: GpuProbeConfidence,
    gpu_probe_source: Option<GpuProbeSource>,
    memory_topology: MemoryTopology,
    vram_total_mb: u64,
    vram_budget_mb: Option<u64>,
    vram_used_mb: u64,
    shared_memory_total_mb: Option<u64>,
    shared_memory_budget_mb: Option<u64>,
}

impl Default for GpuProbeResult {
    fn default() -> Self {
        Self {
            has_gpu: false,
            gpu_name: None,
            gpu_vendor: None,
            gpu_probe_confidence: GpuProbeConfidence::Unavailable,
            gpu_probe_source: None,
            memory_topology: MemoryTopology::CpuOnly,
            vram_total_mb: 0,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
        }
    }
}

fn get_gpu_vram_consolidated(_vulkan_supported: bool, sys: &mut System) -> GpuProbeResult {
    let mut probe = GpuProbeResult::default();

    #[cfg(target_os = "windows")]
    {
        if let Some((
            name,
            vendor,
            total_mb,
            budget_mb,
            used_mb,
            shared_total_mb,
            shared_budget_mb,
        )) = get_windows_gpu_vram_via_dxgi()
        {
            probe.gpu_name = name;
            probe.gpu_vendor = vendor;
            probe.vram_total_mb = total_mb;
            probe.vram_budget_mb = budget_mb;
            probe.vram_used_mb = used_mb;
            probe.shared_memory_total_mb = shared_total_mb;
            probe.shared_memory_budget_mb = shared_budget_mb;
            probe.has_gpu = probe.gpu_name.is_some() || probe.vram_total_mb > 0;
            probe.gpu_probe_confidence = GpuProbeConfidence::Native;
            probe.gpu_probe_source = Some(GpuProbeSource::Dxgi);
            probe.memory_topology = if probe.has_gpu && probe.vram_total_mb > 0 {
                MemoryTopology::DedicatedGpu
            } else if probe.has_gpu {
                MemoryTopology::SharedGpu
            } else {
                MemoryTopology::CpuOnly
            };
        } else {
            warn!(
                "Windows DXGI GPU probe unavailable. Falling back to legacy shell-based detection."
            );

            // Legacy fallback: Use CSV format to handle GPU names with spaces/numbers correctly
            let output = std::process::Command::new("wmic")
                .args([
                    "path",
                    "win32_VideoController",
                    "get",
                    "Name,AdapterRAM",
                    "/format:csv",
                ])
                .output();

            if let Ok(out) = output {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines().skip(1).filter(|l| !l.trim().is_empty()) {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 3 {
                        let name = parts[1].trim();
                        let ram_str = parts[2].trim();
                        if !name.is_empty() {
                            probe.gpu_name = Some(name.to_string());
                            probe.gpu_vendor = derive_vendor_from_name(Some(name));
                            probe.has_gpu = true;
                            probe.gpu_probe_confidence = GpuProbeConfidence::Tooling;
                            probe.gpu_probe_source = Some(GpuProbeSource::Wmic);
                            probe.memory_topology = MemoryTopology::DedicatedGpu;
                        }
                        if let Ok(bytes) = ram_str.parse::<u64>() {
                            probe.vram_total_mb = bytes / (1024 * 1024);
                        }
                        break;
                    }
                }

                // Get Used VRAM via Performance Counters (Legacy fallback for non-DX12 tools)
                let usage_output = std::process::Command::new("wmic")
                    .args([
                        "path",
                        "Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapter",
                        "get",
                        "GPUVIDPnMemoryUsage",
                    ])
                    .output();
                if let Ok(u_out) = usage_output {
                    let u_s = String::from_utf8_lossy(&u_out.stdout);
                    if let Some(val) = u_s.lines().skip(1).find(|l| !l.trim().is_empty()) {
                        probe.vram_used_mb = val.trim().parse::<u64>().unwrap_or(0);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 1. NVIDIA
        let nv_output = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total,memory.used",
                "--format=csv,noheader,nounits",
            ])
            .output();
        if let Ok(out) = nv_output {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = s.lines().next() {
                let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
                if parts.len() == 3 {
                    probe.gpu_name = Some(parts[0].to_string());
                    probe.gpu_vendor = Some(GpuVendor::Nvidia);
                    probe.vram_total_mb = parts[1].parse().unwrap_or(0);
                    probe.vram_used_mb = parts[2].parse().unwrap_or(0);
                    probe.has_gpu = true;
                    probe.gpu_probe_confidence = GpuProbeConfidence::Tooling;
                    probe.gpu_probe_source = Some(GpuProbeSource::NvidiaSmi);
                    probe.memory_topology = MemoryTopology::DedicatedGpu;
                }
            }
        }

        // 2. ROCm (AMD) - Parsing used memory and product name properly
        if !probe.has_gpu {
            let amd_output = std::process::Command::new("rocm-smi")
                .args(["--showmeminfo", "vram", "--showproductname", "--unit", "MB"])
                .output();
            if let Ok(out) = amd_output {
                let s = String::from_utf8_lossy(&out.stdout);

                if let Some(pos) = s.find("Product Name:") {
                    let sub = &s[pos + 13..];
                    probe.gpu_name = Some(sub.lines().next().unwrap_or("").trim().to_string());
                    probe.gpu_vendor = Some(GpuVendor::Amd);
                }
                if let Some(pos) = s.find("Total Memory (MB):") {
                    probe.vram_total_mb = s[pos..]
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split(':')
                        .nth(1)
                        .unwrap_or("0")
                        .trim()
                        .parse::<f64>()
                        .unwrap_or(0.0) as u64;
                    probe.has_gpu = true;
                    probe.gpu_probe_confidence = GpuProbeConfidence::Tooling;
                    probe.gpu_probe_source = Some(GpuProbeSource::RocmSmi);
                    probe.memory_topology = if probe.vram_total_mb > 0 {
                        MemoryTopology::DedicatedGpu
                    } else {
                        MemoryTopology::SharedGpu
                    };
                }
                if let Some(pos) = s.find("Used Memory (MB):") {
                    probe.vram_used_mb = s[pos..]
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split(':')
                        .nth(1)
                        .unwrap_or("0")
                        .trim()
                        .parse::<f64>()
                        .unwrap_or(0.0) as u64;
                }
            }
        }

        // 2.5. ROCm info fallback for WSL2/AMD, where rocm-smi may be unavailable.
        if !probe.has_gpu {
            let amd_output = std::process::Command::new("rocminfo").output();
            if let Ok(out) = amd_output {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some((name, total_mb)) = parse_rocm_info_probe(&s) {
                    probe.gpu_name = Some(name);
                    probe.gpu_vendor = Some(GpuVendor::Amd);
                    probe.vram_total_mb = total_mb;
                    probe.has_gpu = true;
                    probe.gpu_probe_confidence = GpuProbeConfidence::Tooling;
                    probe.gpu_probe_source = Some(GpuProbeSource::RocmInfo);
                    probe.memory_topology = if total_mb > 0 {
                        MemoryTopology::DedicatedGpu
                    } else {
                        MemoryTopology::SharedGpu
                    };
                }
            }
        }

        // 3. lspci Fallback
        if !probe.has_gpu {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg("lspci | grep -iE 'vga|3d|display'")
                .output();
            if let Ok(out) = output {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some(first) = s.lines().next() {
                    probe.has_gpu = true;
                    probe.gpu_name = Some(first.trim().to_string());
                    probe.gpu_vendor = derive_vendor_from_name(probe.gpu_name.as_deref());
                    probe.vram_total_mb = (sys.total_memory() / (1024 * 1024)) / 4;
                    probe.gpu_probe_confidence = GpuProbeConfidence::Heuristic;
                    probe.gpu_probe_source = Some(GpuProbeSource::Lspci);
                    probe.memory_topology = MemoryTopology::SharedGpu;
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        probe.has_gpu = true;
        probe.gpu_name = Some("Apple Silicon (Unified Memory)".to_string());
        probe.gpu_vendor = Some(GpuVendor::Apple);
        probe.gpu_probe_confidence = GpuProbeConfidence::Native;
        probe.gpu_probe_source = Some(GpuProbeSource::AppleUnifiedMemory);
        probe.memory_topology = MemoryTopology::UnifiedMemory;
        sys.refresh_memory();
        probe.vram_total_mb = sys.total_memory() / (1024 * 1024);
        probe.vram_used_mb = sys.used_memory() / (1024 * 1024);
    }

    probe
}

fn check_vulkan() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new("C:\\Windows\\System32\\vulkan-1.dll").exists()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("ldconfig -p | grep -q libvulkan")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            || std::process::Command::new("vulkaninfo")
                .arg("--summary")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        // Generic Vulkan/MoltenVK Framework search
        std::path::Path::new("/Library/Frameworks/Vulkan.framework").exists()
            || std::path::Path::new("/usr/local/lib/libMoltenVK.dylib").exists()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

fn detect_cpu_features() -> (bool, bool, bool) {
    #[cfg(target_arch = "x86_64")]
    {
        let avx512 = is_x86_feature_detected!("avx512f");
        let vnni = is_x86_feature_detected!("avx512vnni") || is_x86_feature_detected!("avxvnni");
        // AMX tile detection is currently unstable in some Rust versions.
        // We can use a runtime check via leaf 7 or just disable if not using nightly.
        let amx = false;
        (avx512, vnni, amx)
    }
    #[cfg(target_arch = "aarch64")]
    {
        (false, false, false)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        (false, false, false)
    }
}

impl Default for HardwareBudgets {
    fn default() -> Self {
        Self {
            max_vram_bytes: FALLBACK_VRAM_BUDGET_MB * 1024 * 1024,
            max_ram_bytes: ((FALLBACK_RAM_BUDGET_MB * 1024 * 1024) as u128 * 8 / 10) as u64,
            separate_vram_pool: true,
            used_fallback: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_vendor_from_name_identifies_major_gpu_families() {
        assert_eq!(
            derive_vendor_from_name(Some("NVIDIA GeForce RTX 4090")),
            Some(GpuVendor::Nvidia)
        );
        assert_eq!(
            derive_vendor_from_name(Some("AMD Radeon RX 7900 XTX")),
            Some(GpuVendor::Amd)
        );
        assert_eq!(
            derive_vendor_from_name(Some("Intel Arc A770")),
            Some(GpuVendor::Intel)
        );
        assert_eq!(
            derive_vendor_from_name(Some("Apple Silicon (Unified Memory)")),
            Some(GpuVendor::Apple)
        );
    }

    #[test]
    fn acceleration_profile_prefers_cuda_for_nvidia_when_available() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("NVIDIA GeForce RTX 4090".into()),
            gpu_vendor: Some(GpuVendor::Nvidia),
            gpu_probe_confidence: GpuProbeConfidence::Native,
            gpu_probe_source: Some(GpuProbeSource::Dxgi),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(20 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: Some(32 * 1024),
            shared_memory_budget_mb: Some(16 * 1024),
            vulkan_supported: true,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: true,
            rocm_available: false,
            gpu_compute_capability: Some((8, 9)),
        };

        assert_eq!(
            status.acceleration_profile(),
            AccelerationProfile::CudaPreferred
        );
        assert!(status.supports_tensorrt());
    }

    #[test]
    fn acceleration_profile_prefers_vulkan_for_amd_when_available() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Tooling,
            gpu_probe_source: Some(GpuProbeSource::RocmSmi),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: None,
            shared_memory_budget_mb: None,
            vulkan_supported: true,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: false,
            gpu_compute_capability: None,
        };

        assert_eq!(
            status.acceleration_profile(),
            AccelerationProfile::VulkanPreferred
        );
        assert!(!status.supports_tensorrt());
    }

    #[test]
    fn budgets_prefer_runtime_vram_budget_when_present() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("NVIDIA RTX".into()),
            gpu_vendor: Some(GpuVendor::Nvidia),
            gpu_probe_confidence: GpuProbeConfidence::Native,
            gpu_probe_source: Some(GpuProbeSource::Dxgi),
            memory_topology: MemoryTopology::DedicatedGpu,
            vram_total_mb: 24 * 1024,
            vram_budget_mb: Some(20 * 1024),
            vram_used_mb: 0,
            shared_memory_total_mb: Some(32 * 1024),
            shared_memory_budget_mb: Some(16 * 1024),
            vulkan_supported: true,
            cpu_cores: 16,
            ram_total_mb: 64 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: true,
            rocm_available: false,
            gpu_compute_capability: Some((8, 9)),
        };

        let budgets = status.budgets();
        assert!(budgets.separate_vram_pool);
        assert_eq!(budgets.max_vram_bytes, 20_u64 * 1024 * 1024 * 1024 * 9 / 10);
    }

    #[test]
    fn telemetry_exposes_windows_budget_fields() {
        let status = HardwareStatus {
            has_gpu: true,
            gpu_name: Some("AMD Radeon RX 7900 XTX".into()),
            gpu_vendor: Some(GpuVendor::Amd),
            gpu_probe_confidence: GpuProbeConfidence::Native,
            gpu_probe_source: Some(GpuProbeSource::Dxgi),
            memory_topology: MemoryTopology::SharedGpu,
            vram_total_mb: 0,
            vram_budget_mb: None,
            vram_used_mb: 0,
            shared_memory_total_mb: Some(16 * 1024),
            shared_memory_budget_mb: Some(8 * 1024),
            vulkan_supported: true,
            cpu_cores: 16,
            ram_total_mb: 32 * 1024,
            avx512_supported: false,
            vnni_supported: false,
            amx_supported: false,
            cuda_available: false,
            rocm_available: false,
            gpu_compute_capability: None,
        };

        let telemetry = status.telemetry();
        assert_eq!(telemetry.memory_topology, MemoryTopology::SharedGpu);
        assert_eq!(telemetry.shared_memory_budget_mb, Some(8 * 1024));
        assert_eq!(
            telemetry.acceleration_profile,
            AccelerationProfile::VulkanPreferred
        );
    }

    #[test]
    fn parse_rocm_info_probe_extracts_wsl_gpu_identity() {
        let sample = r#"
*******                  
Agent 1                  
*******                  
  Name:                    AMD Ryzen 9 7950X3D 16-Core Processor
  Device Type:             CPU
*******                  
Agent 2                  
*******                  
  Name:                    gfx1100
  Marketing Name:          AMD Radeon RX 7900 XTX
  Device Type:             GPU
  Pool Info:
    Pool 1
      Segment:                 GLOBAL; FLAGS: COARSE GRAINED
      Size:                    25105724(0x17f153c) KB
"#;

        let probe = parse_rocm_info_probe(sample).expect("parsed rocm info");
        assert_eq!(probe.0, "AMD Radeon RX 7900 XTX");
        assert!(probe.1 > 20 * 1024);
    }
}

#[cfg(target_os = "windows")]
fn get_windows_gpu_vram_via_dxgi() -> Option<(
    Option<String>,
    Option<GpuVendor>,
    u64,
    Option<u64>,
    u64,
    Option<u64>,
    Option<u64>,
)> {
    use std::mem::MaybeUninit;
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_DESC1,
        DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
        DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1().ok()? };
    let mut index = 0;
    let mut best_adapter: Option<(
        u64,
        Option<String>,
        Option<GpuVendor>,
        Option<u64>,
        u64,
        Option<u64>,
        Option<u64>,
    )> = None;

    loop {
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(_) => break,
        };
        index += 1;

        let desc: DXGI_ADAPTER_DESC1 = match unsafe { adapter.GetDesc1() } {
            Ok(desc) => desc,
            Err(_) => continue,
        };

        if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }

        let dedicated_mb = (desc.DedicatedVideoMemory / (1024 * 1024)) as u64;
        let shared_total_mb = if desc.SharedSystemMemory > 0 {
            Some((desc.SharedSystemMemory / (1024 * 1024)) as u64)
        } else {
            None
        };
        let mut budget_mb = None;
        let mut used_mb = 0;
        let mut shared_budget_mb = None;

        if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
            let mut local_info = MaybeUninit::<DXGI_QUERY_VIDEO_MEMORY_INFO>::uninit();
            if unsafe {
                adapter3.QueryVideoMemoryInfo(
                    0,
                    DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
                    local_info.as_mut_ptr(),
                )
            }
            .is_ok()
            {
                let info = unsafe { local_info.assume_init() };
                used_mb = (info.CurrentUsage / (1024 * 1024)) as u64;
                budget_mb = Some((info.Budget / (1024 * 1024)) as u64);
            }

            let mut non_local_info = MaybeUninit::<DXGI_QUERY_VIDEO_MEMORY_INFO>::uninit();
            if unsafe {
                adapter3.QueryVideoMemoryInfo(
                    0,
                    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
                    non_local_info.as_mut_ptr(),
                )
            }
            .is_ok()
            {
                let info = unsafe { non_local_info.assume_init() };
                shared_budget_mb = Some((info.Budget / (1024 * 1024)) as u64);
            }
        }

        let name = utf16_cstr_to_string(&desc.Description);
        let vendor = gpu_vendor_from_pci_id(desc.VendorId);
        let replace = match &best_adapter {
            Some((best_mb, _, _, _, _, _, _)) => dedicated_mb > *best_mb,
            None => true,
        };

        if replace {
            best_adapter = Some((
                dedicated_mb,
                name,
                vendor,
                budget_mb,
                used_mb,
                shared_total_mb,
                shared_budget_mb,
            ));
        }
    }

    best_adapter.map(
        |(dedicated_mb, name, vendor, budget_mb, used_mb, shared_total_mb, shared_budget_mb)| {
            (
                name,
                vendor,
                dedicated_mb,
                budget_mb,
                used_mb,
                shared_total_mb,
                shared_budget_mb,
            )
        },
    )
}

#[cfg(target_os = "windows")]
fn utf16_cstr_to_string(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(target_os = "windows")]
fn gpu_vendor_from_pci_id(vendor_id: u32) -> Option<GpuVendor> {
    match vendor_id {
        0x10DE => Some(GpuVendor::Nvidia),
        0x1002 | 0x1022 => Some(GpuVendor::Amd),
        0x8086 => Some(GpuVendor::Intel),
        _ => Some(GpuVendor::Unknown),
    }
}

fn derive_vendor_from_name(name: Option<&str>) -> Option<GpuVendor> {
    let normalized = name?.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if normalized.contains("nvidia")
        || normalized.contains("geforce")
        || normalized.contains("quadro")
    {
        Some(GpuVendor::Nvidia)
    } else if normalized.contains("amd")
        || normalized.contains("radeon")
        || normalized.contains("rocm")
    {
        Some(GpuVendor::Amd)
    } else if normalized.contains("apple") {
        Some(GpuVendor::Apple)
    } else if normalized.contains("intel")
        || normalized.contains("arc")
        || normalized.contains("iris")
    {
        Some(GpuVendor::Intel)
    } else {
        Some(GpuVendor::Unknown)
    }
}

fn detect_rocm_runtime() -> bool {
    #[cfg(target_os = "linux")]
    {
        let has_hip_runtime = std::process::Command::new("sh")
            .arg("-c")
            .arg("ldconfig -p | grep -q libamdhip64")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let has_hsa_runtime = std::process::Command::new("sh")
            .arg("-c")
            .arg("ldconfig -p | grep -q libhsa-runtime64")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let hip_platform_amd = std::process::Command::new("hipconfig")
            .arg("--platform")
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).to_ascii_lowercase())
            .map(|text| text.contains("amd"))
            .unwrap_or(false);
        (has_hip_runtime && has_hsa_runtime) || hip_platform_amd
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn parse_rocm_info_probe(output: &str) -> Option<(String, u64)> {
    let mut current_name: Option<String> = None;
    let mut current_marketing_name: Option<String> = None;
    let mut current_device_type: Option<String> = None;
    let mut current_pool_is_coarse = false;
    let mut current_memory_kb = 0_u64;

    let finish_agent = |name: &mut Option<String>,
                        marketing_name: &mut Option<String>,
                        device_type: &mut Option<String>,
                        memory_kb: &mut u64|
     -> Option<(String, u64)> {
        if matches!(device_type.as_deref(), Some("GPU")) {
            let chosen_name = marketing_name
                .take()
                .or_else(|| name.take())
                .filter(|value| !value.trim().is_empty())?;
            let total_mb = *memory_kb / 1024;
            *device_type = None;
            *memory_kb = 0;
            return Some((chosen_name, total_mb));
        }

        *name = None;
        *marketing_name = None;
        *device_type = None;
        *memory_kb = 0;
        None
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Agent ") {
            if let Some(result) = finish_agent(
                &mut current_name,
                &mut current_marketing_name,
                &mut current_device_type,
                &mut current_memory_kb,
            ) {
                return Some(result);
            }
        } else if let Some(value) = trimmed.strip_prefix("Name:") {
            current_name = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Marketing Name:") {
            current_marketing_name = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Device Type:") {
            current_device_type = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Segment:") {
            current_pool_is_coarse = value.contains("COARSE GRAINED");
        } else if current_pool_is_coarse {
            if let Some(value) = trimmed.strip_prefix("Size:") {
                let size_kb = value
                    .split('(')
                    .next()
                    .unwrap_or("0")
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0);
                if size_kb > current_memory_kb {
                    current_memory_kb = size_kb;
                }
                current_pool_is_coarse = false;
            }
        }
    }

    finish_agent(
        &mut current_name,
        &mut current_marketing_name,
        &mut current_device_type,
        &mut current_memory_kb,
    )
}

pub fn enable_amx() -> bool {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        const ARCH_REQ_XCOMP_PERM: i32 = 0x1023;
        const XFEATURE_XTILEDATA: u64 = 18;
        unsafe {
            let res = libc::syscall(
                libc::SYS_arch_prctl,
                ARCH_REQ_XCOMP_PERM,
                XFEATURE_XTILEDATA,
            );
            res == 0
        }
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
    {
        false
    }
}
