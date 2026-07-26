use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserFamily {
    Edge,
    Chrome,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserRuntimeOrigin {
    WindowsNative,
    WslTestBridge,
    UnixFallback,
    EnvOverride,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRuntime {
    pub executable_path: PathBuf,
    pub family: BrowserFamily,
    pub origin: BrowserRuntimeOrigin,
}

impl BrowserRuntime {
    pub fn is_windows_executable(&self) -> bool {
        let lower = self.executable_path.to_string_lossy().to_ascii_lowercase();
        lower.ends_with(".exe")
            && (self.origin == BrowserRuntimeOrigin::WslTestBridge || lower.starts_with("/mnt/"))
    }

    pub fn diagnostic_summary(&self) -> String {
        format!(
            "family={} origin={} path={}",
            self.family.label(),
            self.origin.label(),
            self.executable_path.display()
        )
    }
}

impl BrowserFamily {
    pub fn label(self) -> &'static str {
        match self {
            BrowserFamily::Edge => "edge",
            BrowserFamily::Chrome => "chrome",
            BrowserFamily::Unknown => "unknown",
        }
    }
}

impl BrowserRuntimeOrigin {
    pub fn label(self) -> &'static str {
        match self {
            BrowserRuntimeOrigin::WindowsNative => "windows_native",
            BrowserRuntimeOrigin::WslTestBridge => "wsl_test_bridge",
            BrowserRuntimeOrigin::UnixFallback => "unix_fallback",
            BrowserRuntimeOrigin::EnvOverride => "env_override",
        }
    }

    pub fn user_description(self) -> &'static str {
        match self {
            BrowserRuntimeOrigin::WindowsNative => "Windows native product runtime",
            BrowserRuntimeOrigin::WslTestBridge => "WSL test bridge runtime",
            BrowserRuntimeOrigin::UnixFallback => "Unix fallback runtime",
            BrowserRuntimeOrigin::EnvOverride => "user-provided browser runtime override",
        }
    }
}

pub fn wsl_path_to_windows_path(path: &Path) -> Option<String> {
    let path_str = path.to_str()?;
    let remainder = path_str.strip_prefix("/mnt/")?;
    let (drive, rest) = remainder.split_once('/')?;
    if drive.len() != 1 || !drive.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }

    let drive = drive.to_ascii_uppercase();
    let rest = rest.replace('/', "\\");
    Some(format!(r"{}:\{}", drive, rest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrowserCandidate {
    family: BrowserFamily,
    path: &'static str,
}

const WINDOWS_BROWSER_CANDIDATES: &[BrowserCandidate] = &[
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Chrome,
        path: r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Chrome,
        path: r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    },
];

const WSL_BROWSER_CANDIDATES: &[BrowserCandidate] = &[
    BrowserCandidate {
        family: BrowserFamily::Chrome,
        path: "/mnt/c/Program Files/Google/Chrome/Application/chrome.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Chrome,
        path: "/mnt/c/Program Files (x86)/Google/Chrome/Application/chrome.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: "/mnt/c/Program Files/Microsoft/Edge/Application/msedge.exe",
    },
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: "/mnt/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    },
];

const UNIX_BROWSER_CANDIDATES: &[BrowserCandidate] = &[
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: "/usr/bin/microsoft-edge",
    },
    BrowserCandidate {
        family: BrowserFamily::Edge,
        path: "/usr/bin/microsoft-edge-stable",
    },
    BrowserCandidate {
        family: BrowserFamily::Chrome,
        path: "/usr/bin/google-chrome",
    },
];

pub fn resolve_browser_runtime() -> Option<BrowserRuntime> {
    resolve_browser_runtime_from(env_var_candidates(), default_browser_candidates())
}

pub fn resolve_browser_runtime_from(
    env_vars: &[&str],
    candidates: &[BrowserRuntime],
) -> Option<BrowserRuntime> {
    for env_var in env_vars {
        let Ok(value) = std::env::var(env_var) else {
            continue;
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        let executable_path = PathBuf::from(trimmed);
        let family = infer_family(&executable_path);
        if executable_path.is_file() && is_supported_browser_family(family) {
            return Some(BrowserRuntime {
                family,
                executable_path,
                origin: BrowserRuntimeOrigin::EnvOverride,
            });
        }
    }

    candidates
        .iter()
        .find(|candidate| {
            candidate.executable_path.is_file() && is_supported_browser_family(candidate.family)
        })
        .cloned()
}

pub fn env_var_candidates() -> &'static [&'static str] {
    &["BENSHU_BROWSER_PATH", "CHROME", "BROWSER"]
}

pub fn default_browser_candidates() -> &'static [BrowserRuntime] {
    #[cfg(target_os = "windows")]
    {
        windows_browser_candidates()
    }
    #[cfg(not(target_os = "windows"))]
    {
        if is_wsl() {
            wsl_browser_candidates()
        } else {
            unix_browser_candidates()
        }
    }
}

fn windows_browser_candidates() -> &'static [BrowserRuntime] {
    static CANDIDATES: std::sync::OnceLock<Vec<BrowserRuntime>> = std::sync::OnceLock::new();
    CANDIDATES.get_or_init(|| {
        runtime_list(
            WINDOWS_BROWSER_CANDIDATES,
            BrowserRuntimeOrigin::WindowsNative,
        )
    })
}

fn wsl_browser_candidates() -> &'static [BrowserRuntime] {
    static CANDIDATES: std::sync::OnceLock<Vec<BrowserRuntime>> = std::sync::OnceLock::new();
    CANDIDATES
        .get_or_init(|| runtime_list(WSL_BROWSER_CANDIDATES, BrowserRuntimeOrigin::WslTestBridge))
}

fn unix_browser_candidates() -> &'static [BrowserRuntime] {
    static CANDIDATES: std::sync::OnceLock<Vec<BrowserRuntime>> = std::sync::OnceLock::new();
    CANDIDATES
        .get_or_init(|| runtime_list(UNIX_BROWSER_CANDIDATES, BrowserRuntimeOrigin::UnixFallback))
}

fn runtime_list(
    candidates: &[BrowserCandidate],
    origin: BrowserRuntimeOrigin,
) -> Vec<BrowserRuntime> {
    candidates
        .iter()
        .map(|candidate| BrowserRuntime {
            executable_path: PathBuf::from(candidate.path),
            family: candidate.family,
            origin,
        })
        .collect()
}

fn infer_family(path: &Path) -> BrowserFamily {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.contains("edge") || lower.contains("msedge") {
        BrowserFamily::Edge
    } else if lower.contains("chrome") {
        BrowserFamily::Chrome
    } else {
        BrowserFamily::Unknown
    }
}

fn is_supported_browser_family(family: BrowserFamily) -> bool {
    matches!(family, BrowserFamily::Edge | BrowserFamily::Chrome)
}

fn is_wsl() -> bool {
    std::env::var("WSL_DISTRO_NAME").is_ok()
        || std::env::var("WSL_INTEROP").is_ok()
        || std::fs::read_to_string("/proc/version")
            .map(|version| version.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_browser_runtime_from, BrowserFamily, BrowserRuntime, BrowserRuntimeOrigin,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn resolve_browser_runtime_from_prefers_supported_env_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        let browser_path = temp_dir.path().join("msedge.exe");
        std::fs::write(&browser_path, b"#!/bin/sh\n").unwrap();
        std::env::set_var("BENSHU_BROWSER_PATH", &browser_path);

        let resolved = resolve_browser_runtime_from(
            &["BENSHU_BROWSER_PATH"],
            &[BrowserRuntime {
                executable_path: PathBuf::from("/definitely/missing/browser"),
                family: BrowserFamily::Edge,
                origin: BrowserRuntimeOrigin::WindowsNative,
            }],
        );

        std::env::remove_var("BENSHU_BROWSER_PATH");

        assert_eq!(
            resolved,
            Some(BrowserRuntime {
                executable_path: browser_path,
                family: BrowserFamily::Edge,
                origin: BrowserRuntimeOrigin::EnvOverride,
            })
        );
    }

    #[test]
    fn resolve_browser_runtime_from_rejects_unsupported_env_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        let browser_path = temp_dir.path().join("custom-browser");
        std::fs::write(&browser_path, b"#!/bin/sh\n").unwrap();
        std::env::set_var("BENSHU_BROWSER_PATH", &browser_path);

        let resolved = resolve_browser_runtime_from(&["BENSHU_BROWSER_PATH"], &[]);

        std::env::remove_var("BENSHU_BROWSER_PATH");

        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_browser_runtime_from_falls_back_to_existing_candidate_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let browser_path = temp_dir.path().join("edge.exe");
        std::fs::write(&browser_path, b"stub").unwrap();

        let resolved = resolve_browser_runtime_from(
            &["MISSING_BROWSER_ENV"],
            &[BrowserRuntime {
                executable_path: browser_path.clone(),
                family: BrowserFamily::Edge,
                origin: BrowserRuntimeOrigin::WindowsNative,
            }],
        );

        assert_eq!(
            resolved,
            Some(BrowserRuntime {
                executable_path: browser_path,
                family: BrowserFamily::Edge,
                origin: BrowserRuntimeOrigin::WindowsNative,
            })
        );
    }

    #[test]
    fn resolve_browser_runtime_from_preserves_edge_first_priority() {
        let temp_dir = tempfile::tempdir().unwrap();
        let edge_path = temp_dir.path().join("msedge.exe");
        let chrome_path = temp_dir.path().join("chrome.exe");
        std::fs::write(&edge_path, b"edge").unwrap();
        std::fs::write(&chrome_path, b"chrome").unwrap();

        let resolved = resolve_browser_runtime_from(
            &["MISSING_BROWSER_ENV"],
            &[
                BrowserRuntime {
                    executable_path: edge_path.clone(),
                    family: BrowserFamily::Edge,
                    origin: BrowserRuntimeOrigin::WindowsNative,
                },
                BrowserRuntime {
                    executable_path: chrome_path,
                    family: BrowserFamily::Chrome,
                    origin: BrowserRuntimeOrigin::WindowsNative,
                },
            ],
        );

        assert_eq!(
            resolved,
            Some(BrowserRuntime {
                executable_path: edge_path,
                family: BrowserFamily::Edge,
                origin: BrowserRuntimeOrigin::WindowsNative,
            })
        );
    }

    #[test]
    fn wsl_path_to_windows_path_converts_mount_style_paths() {
        let converted = super::wsl_path_to_windows_path(Path::new(
            "/mnt/c/Users/Public/AppData/Local/BenShu/browser-profile",
        ));
        assert_eq!(
            converted.as_deref(),
            Some(r"C:\Users\Public\AppData\Local\BenShu\browser-profile")
        );
    }

    #[test]
    fn wsl_path_to_windows_path_rejects_non_mount_paths() {
        assert_eq!(
            super::wsl_path_to_windows_path(Path::new("/tmp/browser-profile")),
            None
        );
    }

    #[test]
    fn browser_runtime_exposes_stable_diagnostic_summary() {
        let runtime = BrowserRuntime {
            executable_path: PathBuf::from(
                "/mnt/c/Program Files/Microsoft/Edge/Application/msedge.exe",
            ),
            family: BrowserFamily::Edge,
            origin: BrowserRuntimeOrigin::WslTestBridge,
        };

        assert_eq!(runtime.family.label(), "edge");
        assert_eq!(runtime.origin.label(), "wsl_test_bridge");
        assert!(runtime
            .diagnostic_summary()
            .contains("origin=wsl_test_bridge"));
        assert!(runtime
            .origin
            .user_description()
            .contains("WSL test bridge"));
    }
}
