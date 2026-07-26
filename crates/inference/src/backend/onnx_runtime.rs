use crate::backend::{InferenceError, Result};
use crate::{detect_windows_native_runtime_status, diagnose_windows_native_small_model_error};

fn pending_runtime_error(model_id: &str) -> InferenceError {
    let status = detect_windows_native_runtime_status();
    let diagnosis = diagnose_windows_native_small_model_error(
        None,
        &InferenceError::Temporary(format!(
            "Windows-native ONNX small-model runtime selected for {model_id}, but it is not executable on this host yet ({}, {}).",
            status.small_model_runtime_readiness, status.small_model_runtime_reason
        )),
    );
    InferenceError::Temporary(format!(
        "Windows-native ONNX small-model runtime selected for {model_id}, but it is not executable on this host yet ({}, {}).",
        status.small_model_runtime_readiness,
        status.small_model_runtime_reason
    ) + &format!(
        " [windows_native_outcome={} strategy={}] {}",
        diagnosis.outcome, diagnosis.strategy, diagnosis.note
    ))
}

fn score_from_logits(logits: &[f32]) -> Result<f32> {
    match logits {
        [single] => Ok(1.0 / (1.0 + (-single).exp())),
        [first, second] => {
            let pivot = first.max(*second);
            let first_exp = (*first - pivot).exp();
            let second_exp = (*second - pivot).exp();
            Ok(second_exp / (first_exp + second_exp))
        }
        [] => Err(InferenceError::Execution(
            "ONNX rerank runtime returned an empty logits tensor".to_string(),
            "onnx-rerank".to_string(),
        )),
        _ => Err(InferenceError::Execution(
            format!(
                "ONNX rerank runtime expected 1 or 2 logits, but received {} values",
                logits.len()
            ),
            "onnx-rerank".to_string(),
        )),
    }
}

fn mean_pool_last_hidden_state(
    hidden_state: &[f32],
    seq_len: usize,
    hidden_size: usize,
    attention_mask: &[i64],
) -> Result<Vec<f32>> {
    if hidden_size == 0 {
        return Err(InferenceError::Execution(
            "Cannot mean-pool an ONNX embedding tensor with hidden_size=0".to_string(),
            "onnx-embedding".to_string(),
        ));
    }

    if hidden_state.len() < seq_len.saturating_mul(hidden_size) {
        return Err(InferenceError::Execution(
            format!(
                "ONNX embedding tensor is smaller than expected for seq_len={seq_len} hidden_size={hidden_size}"
            ),
            "onnx-embedding".to_string(),
        ));
    }

    let mut pooled = vec![0.0f32; hidden_size];
    let mut active = 0usize;

    for token_index in 0..seq_len {
        if attention_mask.get(token_index).copied().unwrap_or(0) <= 0 {
            continue;
        }

        let start = token_index * hidden_size;
        let end = start + hidden_size;
        for (dst, src) in pooled.iter_mut().zip(&hidden_state[start..end]) {
            *dst += *src;
        }
        active += 1;
    }

    if active == 0 {
        return Ok(pooled);
    }

    let denom = active as f32;
    for value in &mut pooled {
        *value /= denom;
    }
    Ok(pooled)
}

#[cfg(all(target_os = "windows", feature = "windows_native_onnx"))]
mod imp {
    use super::{mean_pool_last_hidden_state, score_from_logits};
    use crate::backend::{DeviceType, EmbeddingBackend, InferenceError, RerankBackend, Result};
    use async_trait::async_trait;
    use ort::{
        ep::{
            self,
            directml::{DeviceFilter, PerformancePreference},
        },
        session::Session,
        value::Tensor,
    };
    use parking_lot::Mutex;
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use tokenizers::{Encoding, Tokenizer};

    #[derive(Clone)]
    struct OnnxModelFiles {
        model_path: PathBuf,
        tokenizer_path: PathBuf,
        config_path: Option<PathBuf>,
    }

    #[derive(Clone)]
    struct OnnxInputPlan {
        input_ids: String,
        attention_mask: Option<String>,
        token_type_ids: Option<String>,
        position_ids: Option<String>,
    }

    struct OnnxTextRuntime {
        session: Mutex<Session>,
        tokenizer: Tokenizer,
        input_plan: OnnxInputPlan,
        output_name: String,
        model_path: PathBuf,
        hidden_size: Option<usize>,
    }

    fn load_error(context: &str, detail: impl std::fmt::Display) -> InferenceError {
        InferenceError::LoadFailed(format!("{context}: {detail}"))
    }

    fn execution_error(model_id: &str, detail: impl std::fmt::Display) -> InferenceError {
        InferenceError::Execution(detail.to_string(), model_id.to_string())
    }

    fn resolve_model_files(path: &Path) -> Result<OnnxModelFiles> {
        let model_path = if path.is_dir() {
            path.join("model.onnx")
        } else {
            path.to_path_buf()
        };

        if !model_path.exists() {
            return Err(load_error(
                "Windows-native ONNX model bundle is missing model.onnx",
                model_path.display(),
            ));
        }

        let base_dir = model_path.parent().unwrap_or_else(|| Path::new("."));
        let tokenizer_path = base_dir.join("tokenizer.json");
        if !tokenizer_path.exists() {
            return Err(load_error(
                "Windows-native text ONNX bundle requires tokenizer.json",
                tokenizer_path.display(),
            ));
        }

        let config_path = base_dir.join("config.json");
        Ok(OnnxModelFiles {
            model_path,
            tokenizer_path,
            config_path: config_path.exists().then_some(config_path),
        })
    }

    fn create_session(model_path: &Path) -> Result<Session> {
        let builder = Session::builder()
            .map_err(|err| load_error("Failed to create ONNX session builder", err))?;
        let builder = builder
            .with_execution_providers([ep::DirectML::default()
                .with_device_filter(DeviceFilter::Any)
                .with_performance_preference(PerformancePreference::HighPerformance)
                .build()])
            .map_err(|err| load_error("Failed to attach DirectML execution provider", err))?;

        builder
            .commit_from_file(model_path)
            .map_err(|err| load_error("Failed to load ONNX session", err))
    }

    fn inspect_input_plan(session: &Session) -> Result<OnnxInputPlan> {
        let mut input_ids = None;
        let mut attention_mask = None;
        let mut token_type_ids = None;
        let mut position_ids = None;

        for input in session.inputs() {
            match input.name() {
                "input_ids" | "tokens" => input_ids = Some(input.name().to_string()),
                "attention_mask" | "mask" => attention_mask = Some(input.name().to_string()),
                "token_type_ids" | "segment_ids" => {
                    token_type_ids = Some(input.name().to_string())
                }
                "position_ids" => position_ids = Some(input.name().to_string()),
                other => {
                    return Err(load_error(
                        "Unsupported ONNX text-model input",
                        format!("{other} (supported: input_ids/tokens, attention_mask/mask, token_type_ids/segment_ids, position_ids)"),
                    ))
                }
            }
        }

        Ok(OnnxInputPlan {
            input_ids: input_ids.ok_or_else(|| {
                load_error(
                    "Unsupported ONNX text-model input contract",
                    "missing required input_ids/tokens input",
                )
            })?,
            attention_mask,
            token_type_ids,
            position_ids,
        })
    }

    fn select_output_name(session: &Session, preferred: &[&str]) -> Result<String> {
        for candidate in preferred {
            if session
                .outputs()
                .iter()
                .any(|output| output.name() == *candidate)
            {
                return Ok((*candidate).to_string());
            }
        }

        session
            .outputs()
            .first()
            .map(|output| output.name().to_string())
            .ok_or_else(|| load_error("ONNX session has no outputs", "empty outputs list"))
    }

    fn derive_hidden_size(session: &Session, config_path: Option<&Path>) -> Option<usize> {
        if let Some(path) = config_path {
            if let Ok(raw) = std::fs::read_to_string(path) {
                if let Ok(json) = serde_json::from_str::<Value>(&raw) {
                    if let Some(hidden_size) = json.get("hidden_size").and_then(Value::as_u64) {
                        return Some(hidden_size as usize);
                    }
                    if let Some(hidden_size) = json.get("d_model").and_then(Value::as_u64) {
                        return Some(hidden_size as usize);
                    }
                }
            }
        }

        session
            .outputs()
            .first()
            .and_then(|outlet| outlet.dtype().tensor_shape())
            .and_then(|shape| shape.last().copied())
            .filter(|dim| *dim > 0)
            .map(|dim| dim as usize)
    }

    fn build_encoding_inputs(
        input_plan: &OnnxInputPlan,
        encoding: &Encoding,
    ) -> Result<Vec<(String, ort::value::DynTensor)>> {
        let ids: Vec<i64> = encoding
            .get_ids()
            .iter()
            .map(|value| i64::from(*value))
            .collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|value| i64::from(*value))
            .collect();
        let token_type_ids: Vec<i64> = encoding
            .get_type_ids()
            .iter()
            .map(|value| i64::from(*value))
            .collect();
        let position_ids: Vec<i64> = (0..ids.len()).map(|index| index as i64).collect();

        let seq_len = ids.len();
        let mut inputs = Vec::new();
        inputs.push((
            input_plan.input_ids.clone(),
            Tensor::from_array(([1usize, seq_len], ids))
                .map_err(|err| load_error("Failed to build input_ids tensor", err))?
                .upcast(),
        ));

        if let Some(name) = &input_plan.attention_mask {
            inputs.push((
                name.clone(),
                Tensor::from_array(([1usize, seq_len], attention_mask))
                    .map_err(|err| load_error("Failed to build attention_mask tensor", err))?
                    .upcast(),
            ));
        }

        if let Some(name) = &input_plan.token_type_ids {
            inputs.push((
                name.clone(),
                Tensor::from_array(([1usize, seq_len], token_type_ids))
                    .map_err(|err| load_error("Failed to build token_type_ids tensor", err))?
                    .upcast(),
            ));
        }

        if let Some(name) = &input_plan.position_ids {
            inputs.push((
                name.clone(),
                Tensor::from_array(([1usize, seq_len], position_ids))
                    .map_err(|err| load_error("Failed to build position_ids tensor", err))?
                    .upcast(),
            ));
        }

        Ok(inputs)
    }

    impl OnnxTextRuntime {
        fn load(path: &Path, preferred_outputs: &[&str]) -> Result<Self> {
            let files = resolve_model_files(path)?;
            let tokenizer = Tokenizer::from_file(&files.tokenizer_path).map_err(|err| {
                load_error(
                    "Failed to load tokenizer.json for Windows-native ONNX bundle",
                    err,
                )
            })?;
            let session = create_session(&files.model_path)?;
            let input_plan = inspect_input_plan(&session)?;
            let output_name = select_output_name(&session, preferred_outputs)?;
            let hidden_size = derive_hidden_size(&session, files.config_path.as_deref());

            Ok(Self {
                session: Mutex::new(session),
                tokenizer,
                input_plan,
                output_name,
                model_path: files.model_path,
                hidden_size,
            })
        }

        fn encode_single(&self, text: &str) -> Result<Encoding> {
            self.tokenizer
                .encode(text, true)
                .map_err(|err| load_error("ONNX tokenizer encode failed", err))
        }

        fn encode_pair(&self, left: &str, right: &str) -> Result<Encoding> {
            self.tokenizer
                .encode((left, right), true)
                .map_err(|err| load_error("ONNX tokenizer pair-encode failed", err))
        }

        fn run(&self, encoding: &Encoding) -> Result<ort::session::SessionOutputs<'_>> {
            let inputs = build_encoding_inputs(&self.input_plan, encoding)?;
            let mut session = self.session.lock();
            session
                .run(inputs)
                .map_err(|err| load_error("ONNX session execution failed", err))
        }
    }

    pub struct WindowsNativeOnnxEmbeddingBackend {
        model_id: String,
        runtime: OnnxTextRuntime,
        dimension: usize,
    }

    impl WindowsNativeOnnxEmbeddingBackend {
        pub fn load(path: &Path, model_id: impl Into<String>) -> Result<Self> {
            let runtime = OnnxTextRuntime::load(
                path,
                &["sentence_embedding", "embeddings", "last_hidden_state"],
            )?;
            let dimension = runtime.hidden_size.unwrap_or(0);
            Ok(Self {
                model_id: model_id.into(),
                runtime,
                dimension,
            })
        }
    }

    #[async_trait]
    impl EmbeddingBackend for WindowsNativeOnnxEmbeddingBackend {
        fn model_info(&self) -> String {
            format!("onnx-embedding:{}", self.model_id)
        }

        fn dimension(&self) -> usize {
            self.dimension
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Gpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            self.runtime
                .model_path
                .metadata()
                .map(|metadata| (metadata.len() as f64 * 1.2) as u64)
                .unwrap_or(256 * 1024 * 1024)
        }

        async fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let encoding = self.runtime.encode_single(text)?;
            let attention_mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|value| i64::from(*value))
                .collect();
            let outputs = self.runtime.run(&encoding)?;
            let value = outputs
                .get(&self.runtime.output_name)
                .unwrap_or(&outputs[0]);
            let (shape, data) = value
                .try_extract_tensor::<f32>()
                .map_err(|err| execution_error(&self.model_id, err))?;

            match shape.len() {
                2 => {
                    let hidden_size = shape
                        .get(1)
                        .copied()
                        .filter(|value| *value > 0)
                        .map(|value| value as usize)
                        .ok_or_else(|| {
                            execution_error(
                                &self.model_id,
                                format!("Unexpected 2D embedding output shape: {:?}", shape),
                            )
                        })?;
                    Ok(data[..hidden_size].to_vec())
                }
                3 => {
                    let seq_len = shape
                        .get(1)
                        .copied()
                        .filter(|value| *value > 0)
                        .map(|value| value as usize)
                        .ok_or_else(|| {
                            execution_error(
                                &self.model_id,
                                format!("Unexpected 3D embedding output shape: {:?}", shape),
                            )
                        })?;
                    let hidden_size = shape
                        .get(2)
                        .copied()
                        .filter(|value| *value > 0)
                        .map(|value| value as usize)
                        .ok_or_else(|| {
                            execution_error(
                                &self.model_id,
                                format!("Unexpected 3D embedding output shape: {:?}", shape),
                            )
                        })?;
                    mean_pool_last_hidden_state(data, seq_len, hidden_size, &attention_mask)
                        .map_err(|err| match err {
                            InferenceError::Execution(message, _) => {
                                execution_error(&self.model_id, message)
                            }
                            other => other,
                        })
                }
                _ => Err(execution_error(
                    &self.model_id,
                    format!(
                        "Unsupported ONNX embedding output rank {} for shape {:?}",
                        shape.len(),
                        shape
                    ),
                )),
            }
        }
    }

    pub struct WindowsNativeOnnxRerankBackend {
        model_id: String,
        runtime: OnnxTextRuntime,
    }

    impl WindowsNativeOnnxRerankBackend {
        pub fn load(path: &Path, model_id: impl Into<String>) -> Result<Self> {
            Ok(Self {
                model_id: model_id.into(),
                runtime: OnnxTextRuntime::load(path, &["logits", "scores"])?,
            })
        }
    }

    #[async_trait]
    impl RerankBackend for WindowsNativeOnnxRerankBackend {
        fn model_info(&self) -> String {
            format!("onnx-rerank:{}", self.model_id)
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Gpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            self.runtime
                .model_path
                .metadata()
                .map(|metadata| (metadata.len() as f64 * 1.2) as u64)
                .unwrap_or(256 * 1024 * 1024)
        }

        async fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<f32>> {
            let mut scores = Vec::with_capacity(documents.len());
            for document in documents {
                let encoding = self.runtime.encode_pair(query, document)?;
                let outputs = self.runtime.run(&encoding)?;
                let value = outputs
                    .get(&self.runtime.output_name)
                    .unwrap_or(&outputs[0]);
                let (_, logits) = value
                    .try_extract_tensor::<f32>()
                    .map_err(|err| execution_error(&self.model_id, err))?;
                scores.push(score_from_logits(logits).map_err(|err| match err {
                    InferenceError::Execution(message, _) => {
                        execution_error(&self.model_id, message)
                    }
                    other => other,
                })?);
            }
            Ok(scores)
        }
    }
}

#[cfg(not(all(target_os = "windows", feature = "windows_native_onnx")))]
mod imp {
    use super::pending_runtime_error;
    use crate::backend::{DeviceType, EmbeddingBackend, RerankBackend, Result};
    use async_trait::async_trait;
    use std::path::Path;

    pub struct WindowsNativeOnnxEmbeddingBackend {
        model_id: String,
    }

    impl WindowsNativeOnnxEmbeddingBackend {
        pub fn load(_path: &Path, model_id: impl Into<String>) -> Result<Self> {
            Ok(Self {
                model_id: model_id.into(),
            })
        }
    }

    #[async_trait]
    impl EmbeddingBackend for WindowsNativeOnnxEmbeddingBackend {
        fn model_info(&self) -> String {
            format!("onnx-embedding:{}", self.model_id)
        }

        fn dimension(&self) -> usize {
            0
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Cpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            256 * 1024 * 1024
        }

        async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
            Err(pending_runtime_error(&self.model_id))
        }
    }

    pub struct WindowsNativeOnnxRerankBackend {
        model_id: String,
    }

    impl WindowsNativeOnnxRerankBackend {
        pub fn load(_path: &Path, model_id: impl Into<String>) -> Result<Self> {
            Ok(Self {
                model_id: model_id.into(),
            })
        }
    }

    #[async_trait]
    impl RerankBackend for WindowsNativeOnnxRerankBackend {
        fn model_info(&self) -> String {
            format!("onnx-rerank:{}", self.model_id)
        }

        fn device_info(&self) -> DeviceType {
            DeviceType::Cpu
        }

        fn estimated_memory_usage(&self) -> u64 {
            256 * 1024 * 1024
        }

        async fn rerank(&self, _query: &str, _documents: &[String]) -> Result<Vec<f32>> {
            Err(pending_runtime_error(&self.model_id))
        }
    }
}

pub use imp::{WindowsNativeOnnxEmbeddingBackend, WindowsNativeOnnxRerankBackend};

#[cfg(test)]
mod tests {
    use super::{mean_pool_last_hidden_state, score_from_logits};

    #[test]
    fn mean_pool_last_hidden_state_uses_attention_mask() {
        let pooled = mean_pool_last_hidden_state(
            &[
                1.0, 2.0, 3.0, 4.0, //
                5.0, 6.0, 7.0, 8.0, //
                9.0, 10.0, 11.0, 12.0,
            ],
            3,
            4,
            &[1, 1, 0],
        )
        .expect("pool");

        assert_eq!(pooled, vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn score_from_logits_accepts_single_logit() {
        let score = score_from_logits(&[0.0]).expect("score");
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn score_from_logits_accepts_binary_logits() {
        let score = score_from_logits(&[0.0, 1.0]).expect("score");
        assert!(score > 0.73 && score < 0.74);
    }
}
