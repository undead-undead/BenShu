use crate::traits::resource::{AcceleratorInfo, HostResources, ResourceSensor};
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};

pub struct CapabilitySensor {
    sys: System,
    disks: Disks,
    // Simple cache for static GPU info
    gpu_cache: Option<(bool, Option<String>, u64)>,
    last_gpu_refresh: Instant,
}

impl CapabilitySensor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let disks = Disks::new_with_refreshed_list();
        Self {
            sys,
            disks,
            gpu_cache: None,
            last_gpu_refresh: Instant::now() - Duration::from_secs(3600),
        }
    }

    /// Detect GPU using multi-platform shell commands (logic moved from inference)
    fn detect_gpu_stats(&mut self) -> (bool, Option<String>, u64, u64) {
        let mut has_gpu = false;
        let mut gpu_name = None;
        let mut vram_total = 0;
        let mut vram_used = 0;

        #[cfg(target_os = "windows")]
        {
            if let Ok(out) = std::process::Command::new("wmic")
                .args([
                    "path",
                    "win32_VideoController",
                    "get",
                    "Name,AdapterRAM",
                    "/format:csv",
                ])
                .output()
            {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines().skip(1).filter(|l| !l.trim().is_empty()) {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 3 {
                        gpu_name = Some(parts[1].trim().to_string());
                        vram_total = parts[2].trim().parse::<u64>().unwrap_or(0) / (1024 * 1024);
                        has_gpu = true;
                        break;
                    }
                }

                // Used VRAM via performance counters
                if let Ok(out) = std::process::Command::new("wmic")
                    .args([
                        "path",
                        "Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapter",
                        "get",
                        "GPUVIDPnMemoryUsage",
                    ])
                    .output()
                {
                    let s = String::from_utf8_lossy(&out.stdout);
                    if let Some(val) = s.lines().skip(1).find(|l| !l.trim().is_empty()) {
                        vram_used = val.trim().parse::<u64>().unwrap_or(0);
                    }
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Try NVIDIA-SMI
            if let Ok(out) = std::process::Command::new("nvidia-smi")
                .args([
                    "--query-gpu=name,memory.total,memory.used",
                    "--format=csv,noheader,nounits",
                ])
                .output()
            {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = s.lines().next() {
                    let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
                    if parts.len() == 3 {
                        gpu_name = Some(parts[0].to_string());
                        vram_total = parts[1].parse().unwrap_or(0);
                        vram_used = parts[2].parse().unwrap_or(0);
                        has_gpu = true;
                    }
                }
            }
            // Try ROCM-SMI
            if !has_gpu {
                if let Ok(out) = std::process::Command::new("rocm-smi")
                    .args(["--showmeminfo", "vram", "--showproductname", "--unit", "MB"])
                    .output()
                {
                    let s = String::from_utf8_lossy(&out.stdout);
                    if let Some(pos) = s.find("Product Name:") {
                        gpu_name = Some(
                            s[pos + 13..]
                                .lines()
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                        );
                    }
                    if let Some(pos) = s.find("Total Memory (MB):") {
                        vram_total = s[pos..]
                            .lines()
                            .next()
                            .unwrap_or("")
                            .split(':')
                            .nth(1)
                            .unwrap_or("0")
                            .trim()
                            .parse::<f64>()
                            .unwrap_or(0.0) as u64;
                        has_gpu = true;
                    }
                    if let Some(pos) = s.find("Used Memory (MB):") {
                        vram_used = s[pos..]
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
        }

        #[cfg(target_os = "macos")]
        {
            has_gpu = true;
            gpu_name = Some("Apple Silicon (Unified Memory)".to_string());
            vram_total = self.sys.total_memory() / (1024 * 1024);
            vram_used = self.sys.used_memory() / (1024 * 1024);
        }

        (has_gpu, gpu_name, vram_total, vram_used)
    }
}

impl ResourceSensor for CapabilitySensor {
    fn check_resources(&mut self, detailed: bool) -> HostResources {
        self.sys.refresh_memory();
        self.sys.refresh_cpu_all();
        self.disks.refresh(true);

        let total = self.sys.total_memory();
        let used = self.sys.used_memory();
        let free_pct = if total > 0 {
            (total - used) as f32 / total as f32
        } else {
            0.0
        };

        let mut total_disk = 0;
        let mut used_disk = 0;
        for disk in self.disks.iter() {
            total_disk += disk.total_space();
            used_disk += disk.total_space() - disk.available_space();
        }

        let per_core_cpu = if detailed {
            Some(self.sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect())
        } else {
            None
        };

        // Cache refresh for static info
        if self.gpu_cache.is_none() || self.last_gpu_refresh.elapsed().as_secs() > 3600 {
            let (has, name, total, _) = self.detect_gpu_stats();
            self.gpu_cache = Some((has, name, total));
            self.last_gpu_refresh = Instant::now();
        }

        let (has_gpu, gpu_name, vram_total_mb) = self.gpu_cache.clone().unwrap_or((false, None, 0));
        let (_, _, _, vram_used_mb) = self.detect_gpu_stats();

        let vram_pressure_pct = if vram_total_mb > 0 {
            (vram_used_mb as f32 / vram_total_mb as f32) * 100.0
        } else {
            0.0
        };

        let mut accelerators = Vec::new();
        if has_gpu {
            accelerators.push(AcceleratorInfo {
                name: gpu_name.unwrap_or_else(|| "Unknown GPU".to_string()),
                kind: "gpu".to_string(),
                vram_total_mb,
                vram_used_mb,
                vram_pressure_pct,
            });
        }

        HostResources {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used_mb: used / 1024 / 1024,
            mem_total_mb: total / 1024 / 1024,
            free_memory_pct: free_pct * 100.0,
            is_low_memory: free_pct < 0.1,
            total_disk_gb: total_disk / 1024 / 1024 / 1024,
            used_disk_gb: used_disk / 1024 / 1024 / 1024,
            per_core_cpu,
            accelerators,
        }
    }
}

impl Default for CapabilitySensor {
    fn default() -> Self {
        Self::new()
    }
}
