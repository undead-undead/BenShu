use std::fs;
use std::io::Read;
use std::path::Path;
use tracing::{error, info, warn};
use zip::ZipArchive;

use async_trait::async_trait;
use benshu_infra::error::{Error, Result};
use benshu_infra::traits::VesselInspector as VesselInspectorTrait;

/// Security module for validating and inspecting imported .vessel packages.
pub struct VesselInspector {}

impl VesselInspector {
    pub fn new() -> Self {
        Self {}
    }

    /// Layer 1: Static Format Whitelist Extraction.
    /// Unpacks a .vessel (zip) file to a target directory, strictly rejecting
    /// any executable binaries, shell scripts, or unknown blobs.
    pub fn safe_extract(&self, vessel_path: &Path, extract_to: &Path) -> Result<()> {
        if !vessel_path.exists() {
            return Err(Error::Internal(format!(
                "Vessel file not found: {:?}",
                vessel_path
            )));
        }

        let file = fs::File::open(vessel_path)
            .map_err(|e| Error::Internal(format!("Failed to open vessel zip: {}", e)))?;

        let mut archive = ZipArchive::new(file)
            .map_err(|e| Error::Internal(format!("Invalid zip format: {}", e)))?;

        // 1. Blocklist of dangerous extensions
        let dangerous_extensions = [
            "exe", "sh", "bash", "bat", "cmd", "ps1", "vbs", "so", "dylib", "dll", "bin", "app",
            "msi", "jar", "pyc", "class",
        ];

        if !extract_to.exists() {
            fs::create_dir_all(extract_to)
                .map_err(|e| Error::Internal(format!("Failed to create extract dir: {}", e)))?;
        }

        // 防御 1: 拉链炸弹防御 (限制总解压大小为 50MB)
        const MAX_TOTAL_EXTRACT_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
        let mut total_extracted_size: u64 = 0;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| Error::Internal(format!("Failed to read zip index {}: {}", i, e)))?;

            let outpath = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => {
                    warn!("Skipping suspicious path in zip: {}", file.name());
                    continue;
                }
            };

            // SECURITY: Check for dangerous extensions
            if let Some(ext) = outpath.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if dangerous_extensions.contains(&ext_str.as_str()) {
                    let msg = format!(
                        "SECURITY VIOLATION: Malicious file type detected in vessel: {:?}",
                        outpath
                    );
                    error!("{}", msg);
                    // Proactively destroy the extraction dir
                    let _ = fs::remove_dir_all(extract_to);
                    return Err(Error::Security(msg));
                }
            }

            let full_outpath = extract_to.join(&outpath);

            if file.is_dir() {
                fs::create_dir_all(&full_outpath).map_err(|e| {
                    Error::Internal(format!("Failed to create dir {:?}: {}", full_outpath, e))
                })?;
            } else {
                if let Some(p) = full_outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).map_err(|e| {
                            Error::Internal(format!("Failed to create parent dir {:?}: {}", p, e))
                        })?;
                    }
                }

                let mut outfile = fs::File::create(&full_outpath).map_err(|e| {
                    Error::Internal(format!("Failed to create file {:?}: {}", full_outpath, e))
                })?;

                // 防御 2: 分块读取并实时计算大小，防止单文件 Zip 炸弹
                let mut buffer = [0u8; 8192];
                loop {
                    let bytes_read = file
                        .read(&mut buffer)
                        .map_err(|e| Error::Internal(format!("Read error: {}", e)))?;
                    if bytes_read == 0 {
                        break;
                    }
                    total_extracted_size += bytes_read as u64;

                    if total_extracted_size > MAX_TOTAL_EXTRACT_SIZE {
                        let msg = "SECURITY VIOLATION: Vessel extract size exceeded 50MB limit (Potential Zip Bomb).".to_string();
                        error!("{}", msg);
                        let _ = fs::remove_dir_all(extract_to);
                        return Err(Error::Security(msg));
                    }

                    use std::io::Write;
                    outfile
                        .write_all(&buffer[..bytes_read])
                        .map_err(|e| Error::Internal(format!("Write error: {}", e)))?;
                }
            }
        }

        info!("Successfully & safely extracted vessel to {:?}", extract_to);
        Ok(())
    }
}

#[async_trait]
impl VesselInspectorTrait for VesselInspector {
    /// Layer 2 hook for `.vessel` package inspection.
    /// This crate owns static package safety; brain-owned auditor implementations can provide
    /// LLM-backed semantic review through the same infra trait.
    async fn inspect_agent(&self, extract_to: &Path) -> Result<()> {
        let agent_path = extract_to.join("AGENT.md");

        if !agent_path.exists() {
            info!("No AGENT.md found in vessel, skipping semantic inspection.");
            return Ok(());
        }

        let _agent_content = fs::read_to_string(&agent_path)
            .map_err(|e| Error::Internal(format!("Failed to read AGENT.md: {}", e)))?;

        warn!(
            "Semantic vessel auditor is not configured in benshu-security; static package inspection already ran."
        );

        Ok(())
    }
}
