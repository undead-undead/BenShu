pub use crate::traits::resource::{AcceleratorInfo, HostResources, ResourceSensor, ThrottleLevel};

/// Standard OS-level resource sensor using sysinfo
pub struct SystemSensor {
    sys: sysinfo::System,
    disks: sysinfo::Disks,
}

impl SystemSensor {
    pub fn new() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        Self { sys, disks }
    }
}

impl ResourceSensor for SystemSensor {
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

        HostResources {
            cpu_usage: self.sys.global_cpu_usage(),
            mem_used_mb: used / 1024 / 1024,
            mem_total_mb: total / 1024 / 1024,
            free_memory_pct: free_pct * 100.0,
            is_low_memory: free_pct < 0.1,
            total_disk_gb: total_disk / 1024 / 1024 / 1024,
            used_disk_gb: used_disk / 1024 / 1024 / 1024,
            per_core_cpu,
            accelerators: Vec::new(), // System sensor doesn't know about GPUs by default
        }
    }
}

impl Default for SystemSensor {
    fn default() -> Self {
        Self::new()
    }
}
