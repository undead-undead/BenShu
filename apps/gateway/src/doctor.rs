use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheckResult {
    pub name: String,
    pub success: bool,
    pub message: String,
    pub recommendation: Option<String>,
    pub can_repair: bool,
}

pub async fn run_doctor() -> Result<()> {
    use colored::*;
    println!("{}", "Running BenShu Doctor...".bold().blue());

    let checks = check_all().await;
    let mut failures = 0;

    for res in &checks {
        if res.success {
            println!("{} {}", "✓".green(), res.name);
        } else {
            failures += 1;
            println!("{} {} - {}", "✗".red(), res.name, res.message);
            if let Some(rec) = &res.recommendation {
                println!("  {}", rec.yellow());
            }
        }
    }

    println!("\n{}", "Diagnostic Summary".bold().underline());
    println!("Total Checks: {}", checks.len());
    println!(
        "Passed:       {}",
        (checks.len() - failures).to_string().green()
    );
    println!("Failed:       {}", failures.to_string().red());

    Ok(())
}

pub async fn check_all() -> Vec<DoctorCheckResult> {
    let mut results = Vec::new();

    results.push(check_sandbox());
    results.push(check_vectordb());
    results.push(check_rag_mode());
    results.push(check_pixi());
    results.push(check_uv());
    results.push(check_bash());
    results.push(check_gcc());
    results.push(check_node());
    results.push(check_env());
    results.push(check_hardware_accel());
    results.push(await_ollama().await);

    results
}

pub async fn repair(name: &str) -> Result<String> {
    match name {
        "Pixi Environment" => {
            // We use the same logic as onboard/launch
            #[cfg(target_os = "windows")]
            {
                let output = Command::new("powershell")
                    .arg("-Command")
                    .arg("iwr -useb https://pixi.sh/install.ps1 | iex")
                    .output()?;
                if !output.status.success() {
                    anyhow::bail!(
                        "Pixi installation failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let output = Command::new("bash")
                    .arg("-c")
                    .arg("curl -fsSL https://pixi.sh/install.sh | bash")
                    .output()?;
                if !output.status.success() {
                    anyhow::bail!(
                        "Pixi installation failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Ok("Pixi installation triggered successfully.".to_string())
        }
        "UV Fast-Pip" => {
            let output = Command::new("powershell")
                .arg("-Command")
                .arg("pip install uv") // Fallback simple install
                .output()?;
            if output.status.success() {
                Ok("UV installed via pip.".to_string())
            } else {
                Ok("Please install UV manually: https://github.com/astral-sh/uv".to_string())
            }
        }
        "Portable Bash/Shell" => {
            #[cfg(target_os = "windows")]
            {
                let cmd = "Invoke-WebRequest -Uri 'https://github.com/git-for-windows/git/releases/download/v2.53.0.windows.1/MinGit-2.53.0-64-bit.zip' -OutFile 'bin/mingit.zip'; Expand-Archive -Path 'bin/mingit.zip' -DestinationPath 'bin/git-bash' -Force; Remove-Item 'bin/mingit.zip'";
                let output = Command::new("powershell")
                    .arg("-Command")
                    .arg(cmd)
                    .output()?;
                if output.status.success() {
                    Ok(
                        "Bundled Bash (MinGit) downloaded and extracted to bin/git-bash"
                            .to_string(),
                    )
                } else {
                    anyhow::bail!(
                        "Bash repair failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err(anyhow::anyhow!(
                    "Automated repair for Bash only available on Windows."
                ))
            }
        }
        "C Compiler (GCC/Clang)" => {
            #[cfg(target_os = "windows")]
            {
                let cmd = "Invoke-WebRequest -Uri 'https://github.com/skeeto/w64devkit/releases/download/v1.21.0/w64devkit-1.21.0.zip' -OutFile 'bin/mingw.zip'; Expand-Archive -Path 'bin/mingw.zip' -DestinationPath 'bin/mingw_tmp' -Force; Move-Item -Path 'bin/mingw_tmp/w64devkit/*' -Destination 'bin/mingw' -Force; Remove-Item 'bin/mingw.zip', 'bin/mingw_tmp' -Recurse";
                let output = Command::new("powershell")
                    .arg("-Command")
                    .arg(cmd)
                    .output()?;
                if output.status.success() {
                    Ok("Bundled GCC (w64devkit) downloaded and extracted to bin/mingw".to_string())
                } else {
                    anyhow::bail!(
                        "GCC repair failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err(anyhow::anyhow!(
                    "Automated repair for GCC only available on Windows."
                ))
            }
        }
        "Hardware Acceleration" => {
            #[cfg(target_os = "windows")]
            {
                Ok("Please download and install the latest drivers for your GPU (NVIDIA/AMD/Intel). If drivers are installed, ensure the Vulkan Runtime is active.".to_string())
            }
            #[cfg(target_os = "linux")]
            {
                let cmd = "sudo apt-get update && sudo apt-get install -y mesa-vulkan-drivers libvulkan1 vulkan-tools";
                Ok(format!(
                    "Please run the following command to install Vulkan drivers:\n  {}",
                    cmd
                ))
            }
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            {
                Err(anyhow::anyhow!(
                    "Automated repair for Hardware Acceleration not available on this OS."
                ))
            }
        }
        _ => Err(anyhow::anyhow!(
            "No automated repair available for '{}'",
            name
        )),
    }
}

fn check_sandbox() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "Native Sandbox".to_string(),
        success: true,
        message: "Functional".to_string(),
        recommendation: None,
        can_repair: false,
    };

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("bwrap").arg("--version").output();
        if let Err(_) = output {
            res.success = false;
            res.message = "bubblewrap (bwrap) not found".to_string();
            res.recommendation = Some("Install with: sudo apt install bubblewrap".to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        res.message = "Windows Job Objects available".to_string();
    }

    res
}

fn check_pixi() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "Pixi Environment".to_string(),
        success: true,
        message: "Installed".to_string(),
        recommendation: None,
        can_repair: true,
    };

    if which::which("pixi").is_err() {
        let base = std::env::current_dir().unwrap_or_default();
        let managed =
            base.join("infra")
                .join("bin")
                .join(if cfg!(windows) { "pixi.exe" } else { "pixi" });
        if !managed.exists() {
            res.success = false;
            res.message = "Binary not found in PATH or infra/bin".to_string();
            res.recommendation = Some("Run 'benshu-gateway onboard' or click Repair".to_string());
        }
    }
    res
}

fn check_uv() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "UV Fast-Pip".to_string(),
        success: true,
        message: "Installed".to_string(),
        recommendation: None,
        can_repair: true,
    };

    if which::which("uv").is_err() {
        let base = std::env::current_dir().unwrap_or_default();
        let managed =
            base.join("infra")
                .join("bin")
                .join(if cfg!(windows) { "uv.exe" } else { "uv" });
        if !managed.exists() {
            res.success = false;
            res.message = "Binary not found".to_string();
            res.recommendation =
                Some("Install UV for much faster Python skill loading".to_string());
        }
    }
    res
}

fn check_bash() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "Portable Bash/Shell".to_string(),
        success: true,
        message: "Available".to_string(),
        recommendation: None,
        can_repair: cfg!(target_os = "windows"),
    };

    #[cfg(target_os = "windows")]
    {
        if which::which("bash").is_err() {
            let base = std::env::current_dir().unwrap_or_default();
            // Check bundled MinGit bash
            let mini_bash = base
                .join("bin")
                .join("git-bash")
                .join("usr")
                .join("bin")
                .join("bash.exe");
            let alternate_bash = base.join("bin").join("git-bash").join("bin").join("sh.exe");

            if !mini_bash.exists() && !alternate_bash.exists() {
                res.success = false;
                res.message = "Bundled Bash not found in bin/git-bash".to_string();
                res.recommendation =
                    Some("Click Repair to download portable MinGit environment".to_string());
            } else {
                res.message = "Using bundled MinGit Bash".to_string();
            }
        }
    }
    res
}

fn check_gcc() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "C Compiler (GCC/Clang)".to_string(),
        success: true,
        message: "Available".to_string(),
        recommendation: None,
        can_repair: cfg!(target_os = "windows"),
    };

    if which::which("gcc").is_err()
        && which::which("clang").is_err()
        && which::which("cl.exe").is_err()
    {
        #[cfg(target_os = "windows")]
        {
            let base = std::env::current_dir().unwrap_or_default();
            let bundled_gcc = base.join("bin").join("mingw").join("bin").join("gcc.exe");
            if bundled_gcc.exists() {
                res.message = "Using bundled GCC (w64devkit)".to_string();
                return res;
            }
        }

        res.success = false;
        res.message = "No C compiler found".to_string();
        #[cfg(target_os = "linux")]
        {
            res.recommendation = Some("Install with: sudo apt install build-essential".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            res.recommendation = Some("Install with: xcode-select --install".to_string());
        }
        #[cfg(target_os = "windows")]
        {
            res.recommendation = Some(
                "Install Visual Studio Build Tools or click Repair to download Bundled GCC"
                    .to_string(),
            );
        }
    }
    res
}

fn check_node() -> DoctorCheckResult {
    let mut res = DoctorCheckResult {
        name: "JS Runtime (Node/Bun)".to_string(),
        success: true,
        message: "Available".to_string(),
        recommendation: None,
        can_repair: false,
    };

    if which::which("bun").is_err() && which::which("node").is_err() {
        res.success = false;
        res.message = "Neither Bun nor Node.js found".to_string();
        res.recommendation =
            Some("Install Bun (recommended) or Node.js to run JavaScript skills".to_string());
    }
    res
}

fn check_vectordb() -> DoctorCheckResult {
    let base = std::env::current_dir().unwrap_or_default();
    let data_dir = base.join("data");

    if !data_dir.exists() {
        let _ = std::fs::create_dir_all(&data_dir);
    }

    DoctorCheckResult {
        name: "Vector DB Path".to_string(),
        success: data_dir.exists(),
        message: format!("{:?}", data_dir),
        recommendation: None,
        can_repair: false,
    }
}

fn check_rag_mode() -> DoctorCheckResult {
    DoctorCheckResult {
        name: "RAG Engine Mode".to_string(),
        success: true,
        message: "Tiered (Hybrid)".to_string(),
        recommendation: None,
        can_repair: false,
    }
}

fn check_env() -> DoctorCheckResult {
    let keys = [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "DEEPSEEK_API_KEY",
    ];
    let mut found = 0;
    for k in keys {
        if std::env::var(k).is_ok() {
            found += 1;
        }
    }

    DoctorCheckResult {
        name: "Smithery ENV".to_string(),
        success: found > 0 || std::path::Path::new("benshu.yaml").exists(),
        message: if found > 0 {
            format!("{} keys loaded", found)
        } else {
            "Using benshu.yaml".to_string()
        },
        recommendation: if found == 0 {
            Some("Configure at least one LLM provider in Panel -> API Keys".to_string())
        } else {
            None
        },
        can_repair: false,
    }
}

async fn await_ollama() -> DoctorCheckResult {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .unwrap();
    let url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());

    match client.get(format!("{}/api/tags", url)).send().await {
        Ok(r) if r.status().is_success() => DoctorCheckResult {
            name: "Local LLM (Ollama)".to_string(),
            success: true,
            message: "Running".to_string(),
            recommendation: None,
            can_repair: false,
        },
        _ => DoctorCheckResult {
            name: "Local LLM (Ollama)".to_string(),
            success: false,
            message: "Not detected".to_string(),
            recommendation: Some(
                "Start Ollama desktop app for local inference support".to_string(),
            ),
            can_repair: false,
        },
    }
}

fn check_hardware_accel() -> DoctorCheckResult {
    let hw = benshu_inference::hardware::HardwareStatus::detect();
    DoctorCheckResult {
        name: "Hardware Acceleration".to_string(),
        success: hw.vulkan_supported,
        message: if hw.vulkan_supported {
            format!(
                "Vulkan Active ({}). VRAM: {}MB / {}MB. RAM: {}MB.",
                hw.gpu_name.unwrap_or_else(|| "Unknown".to_string()),
                hw.vram_used_mb,
                hw.vram_total_mb,
                hw.ram_total_mb
            )
        } else {
            format!(
                "Vulkan not detected. CPU fallback ({} cores) active. RAM: {}MB.",
                hw.cpu_cores, hw.ram_total_mb
            )
        },
        recommendation: if !hw.vulkan_supported {
            Some("Install Vulkan drivers or SDK for GPU acceleration".to_string())
        } else {
            None
        },
        can_repair: true,
    }
}
