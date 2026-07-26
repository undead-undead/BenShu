use std::path::PathBuf;

use super::runtime::resolve_browser_runtime;

pub mod guard;
pub mod takeover;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSessionConfig {
    pub session_name: String,
    pub user_data_dir: Option<PathBuf>,
    pub state_vault_key: String,
}

impl BrowserSessionConfig {
    pub fn managed_default() -> Self {
        let session_name = std::env::var("BENSHU_BROWSER_SESSION")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "default".to_string());
        let state_vault_key = format!("browser_session_{}", session_name);
        Self {
            user_data_dir: resolve_managed_user_data_dir(None),
            session_name,
            state_vault_key,
        }
    }

    pub fn with_user_data_dir(user_data_dir: Option<PathBuf>) -> Self {
        let mut config = Self::managed_default();
        config.user_data_dir = resolve_managed_user_data_dir(user_data_dir);
        config
    }
}

pub fn resolve_managed_user_data_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(resolve_default_managed_user_data_dir)
}

pub fn resolve_default_managed_user_data_dir() -> Option<PathBuf> {
    if is_wsl_with_windows_browser() {
        return std::env::var("BENSHU_WINDOWS_BROWSER_USER_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                windows_local_appdata_from_wsl()
                    .map(|base| base.join("BenShu").join("browser-profile"))
            })
            .or_else(|| {
                writable_windows_users_profile_dir().map(|base| {
                    base.join("AppData")
                        .join("Local")
                        .join("BenShu")
                        .join("browser-profile")
                })
            });
    }

    std::env::var("BENSHU_BROWSER_USER_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|base| base.join("BenShu").join("browser-profile")))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(PathBuf::from)
                .map(|home| home.join(".benshu").join("browser-profile"))
        })
}

fn windows_local_appdata_from_wsl() -> Option<PathBuf> {
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "echo", "%LOCALAPPDATA%"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find_map(windows_path_to_wsl_mount_path)
}

fn windows_path_to_wsl_mount_path(path: &str) -> Option<PathBuf> {
    let path = path.trim_matches('"').trim();
    let bytes = path.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'\\' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let rest = path[3..].replace('\\', "/");
    Some(PathBuf::from(format!("/mnt/{drive}/{rest}")))
}

fn writable_windows_users_profile_dir() -> Option<PathBuf> {
    let users_dir = PathBuf::from("/mnt/c/Users");
    let entries = std::fs::read_dir(users_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "public" | "default" | "default user" | "all users" | "desktop.ini"
        ) {
            continue;
        }
        let local = path.join("AppData").join("Local");
        if local.is_dir() {
            return Some(path);
        }
    }
    None
}

fn is_wsl_with_windows_browser() -> bool {
    is_wsl() && resolve_browser_runtime().is_some_and(|runtime| runtime.is_windows_executable())
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
        resolve_managed_user_data_dir, windows_path_to_wsl_mount_path, BrowserSessionConfig,
    };
    use std::path::PathBuf;

    #[test]
    fn explicit_browser_user_data_dir_wins() {
        let explicit = PathBuf::from("/tmp/custom-browser-profile");
        assert_eq!(
            resolve_managed_user_data_dir(Some(explicit.clone())),
            Some(explicit)
        );
    }

    #[test]
    fn managed_default_session_uses_default_name() {
        std::env::remove_var("BENSHU_BROWSER_SESSION");
        let config = BrowserSessionConfig::managed_default();
        assert_eq!(config.session_name, "default");
        assert_eq!(config.state_vault_key, "browser_session_default");
    }

    #[test]
    fn windows_local_appdata_path_converts_to_wsl_mount_path() {
        assert_eq!(
            windows_path_to_wsl_mount_path(r"C:\Users\admin\AppData\Local"),
            Some(PathBuf::from("/mnt/c/Users/admin/AppData/Local"))
        );
    }
}
