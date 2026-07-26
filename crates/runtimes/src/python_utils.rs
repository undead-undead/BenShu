use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Find a suitable Python interpreter on the system.
pub async fn find_python() -> Option<PathBuf> {
    // 1. Try which python3 / python
    if let Ok(path) = which::which("python3") {
        return Some(path);
    }
    if let Ok(path) = which::which("python") {
        return Some(path);
    }
    None
}

/// Provision Python using UV if not found on system.
pub async fn provision_python_via_uv() -> Result<PathBuf> {
    info!("System Python not found. Attempting to provision via UV...");

    // Check if UV is available
    let uv_path = which::which("uv")?;

    // We can use 'uv run python' or just 'uv' to manage environments.
    // However, the tools Expect a binary path.
    // We can tell uv to provide a python path:
    let output = tokio::process::Command::new(&uv_path)
        .args(["python", "find"])
        .output()
        .await?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path_str.is_empty() {
            return Ok(PathBuf::from(path_str));
        }
    }

    // Fallback: install a python version via uv
    let _ = tokio::process::Command::new(&uv_path)
        .args(["python", "install", "3.11"])
        .status()
        .await?;

    let output = tokio::process::Command::new(&uv_path)
        .args(["python", "find", "3.11"])
        .output()
        .await?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok(PathBuf::from(path_str));
    }

    Err(anyhow::anyhow!("Failed to provision Python via UV"))
}

/// Ensure a virtual environment exists with the given dependencies.
pub async fn ensure_venv(base_python: &Path, name: &str, deps: &[String]) -> Result<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let venv_dir = home.join(".benshu").join("data").join("venvs").join(name);

    if !venv_dir.exists() {
        info!("Creating virtual environment: {}", name);
        std::fs::create_dir_all(&venv_dir)?;

        let _ = tokio::process::Command::new(base_python)
            .args(["-m", "venv", &venv_dir.to_string_lossy()])
            .status()
            .await?;
    }

    let python_bin = if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    };

    if !deps.is_empty() {
        debug!("Ensuring dependencies for {}: {:?}", name, deps);
        let mut cmd = tokio::process::Command::new(&python_bin);
        cmd.args(["-m", "pip", "install"]);
        for dep in deps {
            cmd.arg(dep);
        }
        let _ = cmd.status().await?;
    }

    Ok(python_bin)
}
