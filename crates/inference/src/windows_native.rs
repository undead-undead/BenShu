use crate::backend::InferenceError;
use crate::model_contract::describe_local_model_contract;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsNativeRuntimeStatus {
    pub host_runtime: String,
    pub deployment_lane: String,
    pub deployment_strategy: String,
    pub deployment_note: String,
    pub product_mainline: String,
    pub validation_tracks: Vec<String>,
    pub windows_native_priority: bool,
    pub small_model_runtime_target: String,
    pub small_model_execution_linked: bool,
    pub small_model_execution_provider: String,
    pub small_model_device_target: String,
    pub small_model_fallback_mode: String,
    pub small_model_runtime_outcome: String,
    pub small_model_runtime_strategy: String,
    pub small_model_runtime_readiness: String,
    pub small_model_runtime_reason: String,
    pub main_brain_runtime_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsNativeRuntimeDiagnosis {
    pub outcome: String,
    pub strategy: String,
    pub note: String,
}

pub fn windows_native_small_model_execution_linked() -> bool {
    cfg!(all(target_os = "windows", feature = "windows_native_onnx"))
}

pub fn windows_native_onnx_runtime_dylib_present() -> bool {
    #[cfg(all(target_os = "windows", feature = "windows_native_onnx"))]
    {
        if let Some(path) = std::env::var_os("ORT_DYLIB_PATH") {
            let path = std::path::PathBuf::from(path);
            if path.exists() {
                return true;
            }
            if path.is_relative() {
                if let Ok(current_exe) = std::env::current_exe() {
                    if let Some(parent) = current_exe.parent() {
                        if parent.join(&path).exists() {
                            return true;
                        }
                    }
                }
            }
        }

        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                if parent.join("onnxruntime.dll").exists() {
                    return true;
                }
            }
        }

        return false;
    }

    #[allow(unreachable_code)]
    false
}

fn windows_native_contract_or_shape_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "tokenizer.json",
        "model.onnx",
        "unsupported onnx",
        "input contract",
        "output rank",
        "output shape",
        "missing required input",
        "missing model.onnx",
        "requires tokenizer.json",
        "unexpected 2d embedding output shape",
        "unexpected 3d embedding output shape",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

fn windows_native_cpu_fallback_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "cpu fallback",
        "fallback to cpu",
        "fall back to cpu",
        "cpu execution provider",
        "cpu ep",
        "using cpu provider",
        "running on cpu fallback",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

fn windows_native_no_accelerator_route_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "no directml device",
        "no compatible directml",
        "no supported directml",
        "no accelerator route",
        "no compatible gpu adapter",
        "no suitable gpu adapter",
        "adapter does not support directml",
        "no dml adapter",
        "no suitable accelerator",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

fn windows_native_provider_execution_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "directml",
        "dml",
        "execution provider",
        "ep failure",
        "provider failure",
        "gpu provider",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

pub fn diagnose_windows_native_small_model_error(
    model_path: Option<&Path>,
    error: &InferenceError,
) -> WindowsNativeRuntimeDiagnosis {
    if let Some(path) = model_path {
        let contract = describe_local_model_contract(path);
        if !contract.ready_for_windows_native_small_model_runtime {
            return WindowsNativeRuntimeDiagnosis {
                outcome: "model_contract_incompatible".to_string(),
                strategy: "rebind_model_contract".to_string(),
                note: contract.reason,
            };
        }
    }

    let runtime = detect_windows_native_runtime_status();
    if runtime.small_model_runtime_readiness != "windows_native_ready" {
        return WindowsNativeRuntimeDiagnosis {
            outcome: runtime.small_model_runtime_outcome,
            strategy: runtime.small_model_runtime_strategy,
            note: runtime.small_model_runtime_reason,
        };
    }

    let rendered = error.to_string();
    if windows_native_contract_or_shape_error(&rendered)
        || matches!(
            error,
            InferenceError::FormatError(_) | InferenceError::InvalidInput(_)
        )
    {
        return WindowsNativeRuntimeDiagnosis {
            outcome: "model_contract_incompatible".to_string(),
            strategy: "rebind_model_contract".to_string(),
            note: format!(
                "The configured ONNX small-model package does not match the Windows-native runtime contract: {rendered}"
            ),
        };
    }

    if matches!(error, InferenceError::ResourceExhausted(_))
        || rendered.to_ascii_lowercase().contains("out of memory")
        || rendered.to_ascii_lowercase().contains("oom")
    {
        return WindowsNativeRuntimeDiagnosis {
            outcome: "accelerator_resource_exhausted".to_string(),
            strategy: "fallback_runtime".to_string(),
            note: format!(
                "The Windows-native small-model runtime exhausted accelerator resources and should keep an explicit fallback path available: {rendered}"
            ),
        };
    }

    if windows_native_cpu_fallback_error(&rendered) {
        if windows_native_no_accelerator_route_error(&rendered) {
            return WindowsNativeRuntimeDiagnosis {
                outcome: "cpu_fallback_no_accelerator_route".to_string(),
                strategy: "inspect_device_route".to_string(),
                note: format!(
                    "The Windows-native small-model runtime fell back to CPU because no compatible Windows-native accelerator route was available: {rendered}"
                ),
            };
        }

        if windows_native_provider_execution_error(&rendered) {
            return WindowsNativeRuntimeDiagnosis {
                outcome: "cpu_fallback_provider_downgrade".to_string(),
                strategy: "inspect_execution_provider".to_string(),
                note: format!(
                    "The Windows-native small-model runtime downgraded from the preferred execution provider to CPU and should expose that provider-level downgrade explicitly: {rendered}"
                ),
            };
        }

        return WindowsNativeRuntimeDiagnosis {
            outcome: "cpu_fallback_active".to_string(),
            strategy: "inspect_cpu_fallback".to_string(),
            note: format!(
                "The Windows-native small-model runtime fell back to CPU execution and should expose that downgrade explicitly: {rendered}"
            ),
        };
    }

    if windows_native_provider_execution_error(&rendered) {
        return WindowsNativeRuntimeDiagnosis {
            outcome: "windows_native_provider_execution_failed".to_string(),
            strategy: "inspect_execution_provider".to_string(),
            note: format!(
                "The Windows-native execution provider failed during load or inference and should be inspected separately from generic runtime failures: {rendered}"
            ),
        };
    }

    WindowsNativeRuntimeDiagnosis {
        outcome: "windows_native_execution_failed".to_string(),
        strategy: "inspect_windows_native_runtime".to_string(),
        note: format!(
            "The Windows-native small-model runtime failed during load or execution and should be inspected before it becomes the only execution path: {rendered}"
        ),
    }
}

fn build_windows_host_status(
    directml_present: bool,
    backend_linked: bool,
    onnx_runtime_present: bool,
) -> WindowsNativeRuntimeStatus {
    WindowsNativeRuntimeStatus {
        host_runtime: "windows_native_mainline".to_string(),
        deployment_lane: "product_mainline".to_string(),
        deployment_strategy: "stay_on_windows_native_host".to_string(),
        deployment_note: "Current host matches the Windows-native product mainline.".to_string(),
        product_mainline: "windows_native_mainline".to_string(),
        validation_tracks: vec![
            "wsl2_validation".to_string(),
            "linux_validation".to_string(),
        ],
        windows_native_priority: true,
        small_model_runtime_target: "onnx_runtime_directml_winml".to_string(),
        small_model_execution_linked: backend_linked,
        small_model_execution_provider: "directml_winml".to_string(),
        small_model_device_target: "windows_native_accelerator".to_string(),
        small_model_fallback_mode: "cpu_fallback_with_explicit_reason".to_string(),
        small_model_runtime_outcome: if directml_present && backend_linked && onnx_runtime_present {
            "windows_native_active".to_string()
        } else if directml_present && backend_linked {
            "runtime_missing".to_string()
        } else if directml_present {
            "backend_unlinked".to_string()
        } else {
            "accelerator_unavailable".to_string()
        },
        small_model_runtime_strategy: if directml_present && backend_linked && onnx_runtime_present
        {
            "active".to_string()
        } else if directml_present && backend_linked {
            "ship_runtime_dylib".to_string()
        } else if directml_present {
            "link_windows_native_backend".to_string()
        } else {
            "fallback_runtime".to_string()
        },
        small_model_runtime_readiness: if directml_present && backend_linked && onnx_runtime_present
        {
            "windows_native_ready".to_string()
        } else if directml_present && backend_linked {
            "windows_native_runtime_missing".to_string()
        } else if directml_present {
            "windows_native_backend_unlinked".to_string()
        } else {
            "windows_native_runtime_missing".to_string()
        },
        small_model_runtime_reason: if directml_present && backend_linked && onnx_runtime_present {
            "DirectML runtime detected on Windows host, the Windows-native ONNX execution backend is linked, and onnxruntime.dll is available."
                .to_string()
        } else if directml_present && backend_linked {
            "DirectML runtime detected on Windows host and the Windows-native ONNX execution backend is linked, but onnxruntime.dll could not be located. Set ORT_DYLIB_PATH or ship onnxruntime.dll alongside BenShu."
                .to_string()
        } else if directml_present {
            "DirectML runtime detected on Windows host, but BenShu has not linked the Windows-native ONNX small-model execution backend yet."
                .to_string()
        } else {
            "DirectML runtime not detected on Windows host.".to_string()
        },
        main_brain_runtime_target: "llama.cpp".to_string(),
    }
}

fn build_validation_status(host_runtime: &str) -> WindowsNativeRuntimeStatus {
    WindowsNativeRuntimeStatus {
        host_runtime: host_runtime.to_string(),
        deployment_lane: "validation_only".to_string(),
        deployment_strategy: "switch_to_windows_native_host".to_string(),
        deployment_note:
            "Current host is only for validation; Windows-native deployment remains the product mainline."
                .to_string(),
        product_mainline: "windows_native_mainline".to_string(),
        validation_tracks: match host_runtime {
            "macos_validation" => vec![
                "wsl2_validation".to_string(),
                "linux_validation".to_string(),
                "macos_validation".to_string(),
            ],
            _ => vec!["wsl2_validation".to_string(), "linux_validation".to_string()],
        },
        windows_native_priority: true,
        small_model_runtime_target: "onnx_runtime_directml_winml".to_string(),
        small_model_execution_linked: false,
        small_model_execution_provider: "validation_only".to_string(),
        small_model_device_target: "windows_native_accelerator".to_string(),
        small_model_fallback_mode: "validation_only".to_string(),
        small_model_runtime_outcome: "validation_only".to_string(),
        small_model_runtime_strategy: "validation_host_only".to_string(),
        small_model_runtime_readiness: "validation_only".to_string(),
        small_model_runtime_reason:
            "Current host is a validation path; Windows-native small-model runtime is not active on this host."
                .to_string(),
        main_brain_runtime_target: "llama.cpp".to_string(),
    }
}

pub fn detect_windows_native_runtime_status() -> WindowsNativeRuntimeStatus {
    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let directml_path = std::path::Path::new(&system_root)
            .join("System32")
            .join("DirectML.dll");
        let directml_present = directml_path.exists();
        let backend_linked = windows_native_small_model_execution_linked();
        let onnx_runtime_present = windows_native_onnx_runtime_dylib_present();
        return build_windows_host_status(directml_present, backend_linked, onnx_runtime_present);
    }

    #[cfg(target_os = "linux")]
    {
        let is_wsl = std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::fs::read_to_string("/proc/version")
                .map(|version| version.to_ascii_lowercase().contains("microsoft"))
                .unwrap_or(false);
        return build_validation_status(if is_wsl {
            "wsl2_validation"
        } else {
            "linux_validation"
        });
    }

    #[cfg(target_os = "macos")]
    {
        return build_validation_status("macos_validation");
    }

    #[allow(unreachable_code)]
    WindowsNativeRuntimeStatus {
        host_runtime: "unknown_host".to_string(),
        deployment_lane: "unknown".to_string(),
        deployment_strategy: "inspect_host_runtime".to_string(),
        deployment_note: "Host runtime could not be classified.".to_string(),
        product_mainline: "windows_native_mainline".to_string(),
        validation_tracks: vec!["validation_only".to_string()],
        windows_native_priority: true,
        small_model_runtime_target: "onnx_runtime_directml_winml".to_string(),
        small_model_execution_linked: false,
        small_model_execution_provider: "unknown".to_string(),
        small_model_device_target: "windows_native_accelerator".to_string(),
        small_model_fallback_mode: "unknown".to_string(),
        small_model_runtime_outcome: "unknown_host".to_string(),
        small_model_runtime_strategy: "fallback_runtime".to_string(),
        small_model_runtime_readiness: "unknown_host".to_string(),
        small_model_runtime_reason: "Host runtime could not be classified.".to_string(),
        main_brain_runtime_target: "llama.cpp".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_validation_status, build_windows_host_status, detect_windows_native_runtime_status,
        diagnose_windows_native_small_model_error,
    };
    use crate::backend::InferenceError;
    use std::path::Path;

    #[test]
    fn windows_host_status_reports_active_runtime_when_everything_is_present() {
        let status = build_windows_host_status(true, true, true);
        assert_eq!(status.deployment_lane, "product_mainline");
        assert_eq!(status.small_model_runtime_readiness, "windows_native_ready");
        assert_eq!(status.small_model_runtime_outcome, "windows_native_active");
        assert_eq!(status.small_model_runtime_strategy, "active");
    }

    #[test]
    fn windows_host_status_reports_missing_runtime_dylib() {
        let status = build_windows_host_status(true, true, false);
        assert_eq!(
            status.small_model_runtime_readiness,
            "windows_native_runtime_missing"
        );
        assert_eq!(status.small_model_runtime_outcome, "runtime_missing");
        assert_eq!(status.small_model_runtime_strategy, "ship_runtime_dylib");
    }

    #[test]
    fn windows_host_status_reports_unlinked_backend() {
        let status = build_windows_host_status(true, false, false);
        assert_eq!(
            status.small_model_runtime_readiness,
            "windows_native_backend_unlinked"
        );
        assert_eq!(status.small_model_runtime_outcome, "backend_unlinked");
        assert_eq!(
            status.small_model_runtime_strategy,
            "link_windows_native_backend"
        );
    }

    #[test]
    fn windows_host_status_reports_accelerator_unavailable() {
        let status = build_windows_host_status(false, false, false);
        assert_eq!(
            status.small_model_runtime_readiness,
            "windows_native_runtime_missing"
        );
        assert_eq!(
            status.small_model_runtime_outcome,
            "accelerator_unavailable"
        );
        assert_eq!(status.small_model_runtime_strategy, "fallback_runtime");
    }

    #[test]
    fn validation_status_requires_switching_back_to_windows_native_host() {
        let status = build_validation_status("linux_validation");
        assert_eq!(status.deployment_lane, "validation_only");
        assert_eq!(status.deployment_strategy, "switch_to_windows_native_host");
        assert_eq!(status.small_model_runtime_outcome, "validation_only");
        assert_eq!(status.small_model_runtime_strategy, "validation_host_only");
    }

    #[test]
    fn diagnosis_reports_contract_incompatibility_for_non_onnx_paths() {
        let diagnosis = diagnose_windows_native_small_model_error(
            Some(Path::new("/tmp/model.gguf")),
            &InferenceError::LoadFailed("not an onnx package".to_string()),
        );
        assert_eq!(diagnosis.outcome, "model_contract_incompatible");
        assert_eq!(diagnosis.strategy, "rebind_model_contract");
    }

    #[test]
    fn diagnosis_reports_contract_incompatibility_for_tokenizer_errors() {
        let diagnosis = diagnose_windows_native_small_model_error(
            None,
            &InferenceError::LoadFailed(
                "Windows-native text ONNX bundle requires tokenizer.json".to_string(),
            ),
        );
        if detect_windows_native_runtime_status().small_model_runtime_readiness
            == "windows_native_ready"
        {
            assert_eq!(diagnosis.outcome, "model_contract_incompatible");
            assert_eq!(diagnosis.strategy, "rebind_model_contract");
        } else {
            assert!(!diagnosis.outcome.is_empty());
        }
    }

    #[test]
    fn diagnosis_reports_cpu_fallback_separately() {
        let diagnosis = diagnose_windows_native_small_model_error(
            None,
            &InferenceError::Execution(
                "Fallback to CPU execution provider".to_string(),
                "onnx-embedding".to_string(),
            ),
        );
        if detect_windows_native_runtime_status().small_model_runtime_readiness
            == "windows_native_ready"
        {
            assert_eq!(diagnosis.outcome, "cpu_fallback_active");
            assert_eq!(diagnosis.strategy, "inspect_cpu_fallback");
        } else {
            assert!(!diagnosis.outcome.is_empty());
        }
    }

    #[test]
    fn diagnosis_reports_provider_cpu_downgrade_separately() {
        let diagnosis = diagnose_windows_native_small_model_error(
            None,
            &InferenceError::Execution(
                "DirectML execution provider failed, fallback to CPU execution provider"
                    .to_string(),
                "onnx-embedding".to_string(),
            ),
        );
        if detect_windows_native_runtime_status().small_model_runtime_readiness
            == "windows_native_ready"
        {
            assert_eq!(diagnosis.outcome, "cpu_fallback_provider_downgrade");
            assert_eq!(diagnosis.strategy, "inspect_execution_provider");
        } else {
            assert!(!diagnosis.outcome.is_empty());
        }
    }

    #[test]
    fn diagnosis_reports_no_accelerator_route_cpu_fallback_separately() {
        let diagnosis = diagnose_windows_native_small_model_error(
            None,
            &InferenceError::Execution(
                "No compatible DirectML device found, fallback to CPU execution provider"
                    .to_string(),
                "onnx-rerank".to_string(),
            ),
        );
        if detect_windows_native_runtime_status().small_model_runtime_readiness
            == "windows_native_ready"
        {
            assert_eq!(diagnosis.outcome, "cpu_fallback_no_accelerator_route");
            assert_eq!(diagnosis.strategy, "inspect_device_route");
        } else {
            assert!(!diagnosis.outcome.is_empty());
        }
    }

    #[test]
    fn diagnosis_reports_provider_execution_failure_separately() {
        let diagnosis = diagnose_windows_native_small_model_error(
            None,
            &InferenceError::Execution(
                "DirectML execution provider initialization failed".to_string(),
                "onnx-rerank".to_string(),
            ),
        );
        if detect_windows_native_runtime_status().small_model_runtime_readiness
            == "windows_native_ready"
        {
            assert_eq!(
                diagnosis.outcome,
                "windows_native_provider_execution_failed"
            );
            assert_eq!(diagnosis.strategy, "inspect_execution_provider");
        } else {
            assert!(!diagnosis.outcome.is_empty());
        }
    }
}
