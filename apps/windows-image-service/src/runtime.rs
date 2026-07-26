use benshu_inference::windows_native::detect_windows_native_runtime_status;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ImageRuntimeStatus {
    pub host_runtime: String,
    pub product_mainline: String,
    pub small_model_runtime_target: String,
    pub small_model_runtime_readiness: String,
    pub small_model_runtime_reason: String,
    pub windows_native_priority: bool,
}

impl ImageRuntimeStatus {
    pub fn detect() -> Self {
        let status = detect_windows_native_runtime_status();
        Self {
            host_runtime: status.host_runtime,
            product_mainline: status.product_mainline,
            small_model_runtime_target: status.small_model_runtime_target,
            small_model_runtime_readiness: status.small_model_runtime_readiness,
            small_model_runtime_reason: status.small_model_runtime_reason,
            windows_native_priority: status.windows_native_priority,
        }
    }
}
