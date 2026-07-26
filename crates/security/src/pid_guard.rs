use std::process;
use sysinfo::{Pid, System};
use tracing::{info, warn};

/// Security utility to verify the parent process identity.
pub struct PidGuard {
    parent_pid: u32,
}

impl PidGuard {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let current_pid = Pid::from(process::id() as usize);
        let parent_pid = if let Some(process) = sys.process(current_pid) {
            process.parent().map(|p| p.as_u32()).unwrap_or(0)
        } else {
            0
        };

        info!("PidGuard initialized. Parent PID: {}", parent_pid);
        Self { parent_pid }
    }

    /// Verify if a request's claim matches the expected parent PID.
    /// This provides protection against other local processes attempting to imagentte the Panel.
    pub fn verify_parent(&self, claimed_pid: u32) -> bool {
        if self.parent_pid == 0 {
            // If we can't detect parent (e.g. running in some weird container), we might allow or deny
            // For Zero-Trust, we should probably be cautious.
            return false;
        }

        if claimed_pid == self.parent_pid {
            return true;
        }

        warn!(
            "PID mismatch detected! Claimed: {}, Actual Parent: {}",
            claimed_pid, self.parent_pid
        );
        false
    }

    pub fn get_parent_pid(&self) -> u32 {
        self.parent_pid
    }

    /// Watch the parent process and resolve when it dies.
    /// This is used for "Secure Closing" to prevent orphaned processes.
    pub async fn watch_parent(&self) {
        if self.parent_pid == 0 {
            return;
        }

        let pid = Pid::from(self.parent_pid as usize);
        let mut sys = System::new();

        info!("Starting Parent Death Watcher for PID: {}", self.parent_pid);

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
            if sys.process(pid).is_none() {
                warn!(
                    "Parent process (PID {}) died. Securely closing...",
                    self.parent_pid
                );
                break;
            }
        }
    }
}

impl Default for PidGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_guard_initialization() {
        let guard = PidGuard::new();
        // The parent PID might be 0 depending on the test environment context (e.g. systemd or no obvious parent).
        // It should at least initialize properly without panicking.
        let parent = guard.get_parent_pid();

        if parent > 0 {
            // If parent > 0, verifying with the exact parent PID should return true
            assert!(guard.verify_parent(parent));
        }

        // Verifying with an arbitrary wrong PID should return false
        assert!(!guard.verify_parent(9999999));
    }
}
