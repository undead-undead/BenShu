pub use benshu_infra::skill::{ModelSpec, SkillExecutionConfig, SkillMetadata};
pub use benshu_infra::traits::runtime::SkillRuntime;
use std::sync::Arc;

pub mod cc;
pub mod python;
pub mod python_utils;
pub mod quickjs;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use cc::SmartCCRuntime;
pub use python::SmartPythonRuntime;
pub use quickjs::QuickJSRuntime;

#[cfg(feature = "wasm")]
pub use wasm::WasmRuntime;

/// Returns the appropriate runtime for a given runtime name.
pub fn get_runtime(name: &str) -> Arc<dyn SkillRuntime> {
    match name.to_lowercase().as_str() {
        "qjs" | "quickjs" | "js" | "javascript" | "node" | "nodejs" | "bun" => {
            Arc::new(QuickJSRuntime::new())
        }
        "python" | "python3" | "py" => Arc::new(SmartPythonRuntime::new()),
        "c" | "cpp" | "gcc" | "g++" | "cc" | "c++" => Arc::new(SmartCCRuntime::new()),
        #[cfg(feature = "wasm")]
        "wasm" => Arc::new(WasmRuntime::new()),
        _ => Arc::new(benshu_security::sandbox::NativeShellRuntime::new()),
    }
}
