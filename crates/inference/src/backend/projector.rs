use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use std::collections::HashSet;
use std::path::Path;

/// Projector types supported by the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProjectorType {
    LlavaV15,    // Multi-layer MLP
    LlavaSimple, // Single linear layer
    QwenVL,      // Qwen-VL style projector
    Qwen2VL,     // Qwen2-VL style
    InternVL2,   // InternVL2 style
    GLM4V,       // GLM-4V style
}

fn gguf_tensor_names<P: AsRef<Path>>(path: P) -> Result<HashSet<String>> {
    let path = path.as_ref();
    let mut file = std::fs::File::open(path)
        .context(format!("Failed to open GGUF file: {}", path.display()))?;
    let content = candle_core::quantized::gguf_file::Content::read(&mut file)
        .map_err(|e| anyhow!("GGUF read error: {}", e))?;
    Ok(content.tensor_infos.keys().cloned().collect())
}

fn infer_projector_candidates_from_tensor_names(
    tensor_names: &HashSet<String>,
) -> Vec<ProjectorType> {
    let mut candidates = Vec::new();

    if tensor_names.contains("vision_proj.linear_1.weight")
        || tensor_names.contains("vision_proj.linear_2.weight")
    {
        candidates.push(ProjectorType::GLM4V);
    }

    if tensor_names.contains("vision_proj.0.weight")
        || tensor_names.contains("vision_proj.2.weight")
    {
        candidates.push(ProjectorType::InternVL2);
    }

    if tensor_names.contains("visual_proj.weight") || tensor_names.contains("linear.weight") {
        candidates.push(ProjectorType::Qwen2VL);
    }

    if tensor_names.contains("mm.0.weight") && tensor_names.contains("mm.2.weight") {
        candidates.push(ProjectorType::LlavaV15);
    } else if tensor_names.contains("mm.0.weight") {
        candidates.push(ProjectorType::LlavaSimple);
    }

    candidates
}

fn infer_projector_candidates_from_filename<P: AsRef<Path>>(path: P) -> Vec<ProjectorType> {
    let lowered = path
        .as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut candidates = Vec::new();

    if lowered.contains("glm") {
        candidates.push(ProjectorType::GLM4V);
    }
    if lowered.contains("internvl") {
        candidates.push(ProjectorType::InternVL2);
    }
    if lowered.contains("qwen") {
        candidates.push(ProjectorType::Qwen2VL);
    }
    if lowered.contains("llava") {
        candidates.push(ProjectorType::LlavaV15);
    }

    candidates
}

pub fn projector_candidates_for_path<P: AsRef<Path>>(path: P) -> Vec<ProjectorType> {
    let path = path.as_ref();
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut candidates = Vec::new();

    if extension.eq_ignore_ascii_case("gguf") {
        if let Ok(tensor_names) = gguf_tensor_names(path) {
            candidates.extend(infer_projector_candidates_from_tensor_names(&tensor_names));
        }
    }

    if candidates.is_empty() {
        candidates.extend(infer_projector_candidates_from_filename(path));
    }

    for fallback in [
        ProjectorType::LlavaV15,
        ProjectorType::LlavaSimple,
        ProjectorType::Qwen2VL,
        ProjectorType::InternVL2,
        ProjectorType::GLM4V,
    ] {
        if !candidates.contains(&fallback) {
            candidates.push(fallback);
        }
    }

    candidates
}

pub fn load_adaptive_projector<P: AsRef<Path>>(
    path: P,
    device: &Device,
) -> Result<(Box<dyn VisionProjector>, ProjectorType)> {
    let path = path.as_ref();
    let candidates = projector_candidates_for_path(path);
    let mut last_err: Option<anyhow::Error> = None;

    for candidate in candidates {
        let attempt: Result<Box<dyn VisionProjector>> = match candidate {
            ProjectorType::LlavaV15 => MLPProjector::load(path, device, ProjectorType::LlavaV15)
                .map(|proj| Box::new(proj) as Box<dyn VisionProjector>),
            ProjectorType::LlavaSimple => {
                MLPProjector::load(path, device, ProjectorType::LlavaSimple)
                    .map(|proj| Box::new(proj) as Box<dyn VisionProjector>)
            }
            ProjectorType::QwenVL | ProjectorType::Qwen2VL => {
                LinearProjector::load(path, device, ProjectorType::Qwen2VL)
                    .map(|proj| Box::new(proj) as Box<dyn VisionProjector>)
            }
            ProjectorType::InternVL2 => MLPProjector::load(path, device, ProjectorType::InternVL2)
                .map(|proj| Box::new(proj) as Box<dyn VisionProjector>),
            ProjectorType::GLM4V => MLPProjector::load(path, device, ProjectorType::GLM4V)
                .map(|proj| Box::new(proj) as Box<dyn VisionProjector>),
        };

        match attempt {
            Ok(projector) => return Ok((projector, candidate)),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        anyhow!(
            "Failed to load adaptive projector from {} with any known projector layout",
            path.display()
        )
    }))
}

/// Common trait for all vision-to-language projectors
pub trait VisionProjector: Send + Sync {
    /// Project vision features to language embedding space
    fn project(&self, features: &Tensor) -> Result<Tensor>;

    /// Get the type of this projector
    fn projector_type(&self) -> ProjectorType;

    /// Get input/output dimensions (input_dim, output_dim)
    fn dimensions(&self) -> (usize, usize);

    /// Get the execution device
    fn device(&self) -> &Device;

    /// Pre-load kernels and tensors into memory
    fn warmup(&self) -> Result<()> {
        let (in_dim, _) = self.dimensions();
        // Use 1 as batch and 1 as sequence length for warmup
        let dummy = Tensor::zeros((1, 1, in_dim), DType::F32, self.device())?;
        let _ = self.project(&dummy).context("Warmup projection failed")?;
        Ok(())
    }
}

/// Helper to load variables from either Safetensors or GGUF
fn load_var_builder<P: AsRef<Path>>(path: P, device: &Device) -> Result<VarBuilder<'_>> {
    let path = path.as_ref();
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match extension {
        "safetensors" => {
            unsafe { VarBuilder::from_mmaped_safetensors(&[path], DType::F32, device) }.context(
                format!("Failed to load safetensors from {}", path.display()),
            )
        }
        "gguf" => {
            let mut file = std::fs::File::open(path)
                .context(format!("Failed to open GGUF file: {}", path.display()))?;
            let content = candle_core::quantized::gguf_file::Content::read(&mut file)
                .map_err(|e| anyhow!("GGUF read error: {}", e))?;

            let mut tensors = std::collections::HashMap::new();
            for name in content.tensor_infos.keys() {
                let qtensor = content
                    .tensor(&mut file, name, device)
                    .map_err(|e| anyhow!("Failed to read GGUF tensor {}: {}", name, e))?;
                // MMProj weights are typically small, we dequantize them to F32 for the Linear layer
                let t = qtensor
                    .dequantize(device)
                    .map_err(|e| anyhow!("Failed to dequantize GGUF tensor {}: {}", name, e))?;
                tensors.insert(name.to_string(), t);
            }
            Ok(VarBuilder::from_tensors(tensors, DType::F32, device))
        }
        _ => Err(anyhow!(
            "Unsupported model format: .{} (expected .safetensors or .gguf)",
            extension
        )),
    }
}

/// Multi-layer Perceptron (MLP) for LLaVA/InternVL/GLM
pub struct MLPProjector {
    mm_0: Linear,
    mm_2: Option<Linear>,
    device: Device,
    proj_type: ProjectorType,
}

impl MLPProjector {
    pub fn load<P: AsRef<Path>>(
        path: P,
        device: &Device,
        proj_type: ProjectorType,
    ) -> Result<Self> {
        if !path.as_ref().exists() {
            return Err(anyhow!(
                "Vision weights not found at {}",
                path.as_ref().display()
            ));
        }

        let vb = load_var_builder(&path, device)?;

        // Auto-detect names and load
        let (mm_0_name, mm_2_name) = match proj_type {
            ProjectorType::LlavaV15 | ProjectorType::LlavaSimple => ("mm.0", "mm.2"),
            ProjectorType::InternVL2 => ("vision_proj.0", "vision_proj.2"),
            ProjectorType::GLM4V => ("vision_proj.linear_1", "vision_proj.linear_2"),
            _ => ("mm.0", "mm.2"),
        };

        let mm_0 = {
            let w_name = format!("{}.weight", mm_0_name);
            let b_name = format!("{}.bias", mm_0_name);
            let weight = vb
                .get_with_hints((), &w_name, Default::default())
                .context(format!("Missing tensor: {}", w_name))?;
            let bias = vb.get_with_hints((), &b_name, Default::default()).ok();
            Linear::new(weight, bias)
        };

        let mm_2 = if vb.contains_tensor(&format!("{}.weight", mm_2_name)) {
            let weight =
                vb.get_with_hints((), &format!("{}.weight", mm_2_name), Default::default())?;
            let bias = vb
                .get_with_hints((), &format!("{}.bias", mm_2_name), Default::default())
                .ok();
            Some(Linear::new(weight, bias))
        } else {
            None
        };

        Ok(Self {
            mm_0,
            mm_2,
            device: device.clone(),
            proj_type,
        })
    }
}

impl VisionProjector for MLPProjector {
    fn project(&self, features: &Tensor) -> Result<Tensor> {
        let features = features.to_device(&self.device)?;
        let x = self.mm_0.forward(&features)?;
        if let Some(mm_2) = &self.mm_2 {
            let x = x.gelu().context("GELU activation failed")?;
            Ok(mm_2.forward(&x).context("MLP layer 2 forward failed")?)
        } else {
            Ok(x)
        }
    }

    fn projector_type(&self) -> ProjectorType {
        self.proj_type
    }

    fn dimensions(&self) -> (usize, usize) {
        let in_dim = self.mm_0.weight().dim(1).unwrap_or(0);
        let out_dim = self
            .mm_2
            .as_ref()
            .map(|l| l.weight().dim(0).unwrap_or(0))
            .unwrap_or(self.mm_0.weight().dim(0).unwrap_or(0));
        (in_dim, out_dim)
    }

    fn device(&self) -> &Device {
        &self.device
    }
}

/// Linear (Qwen-VL style)
pub struct LinearProjector {
    model: Linear,
    device: Device,
    proj_type: ProjectorType,
}

impl LinearProjector {
    pub fn load<P: AsRef<Path>>(
        path: P,
        device: &Device,
        proj_type: ProjectorType,
    ) -> Result<Self> {
        let vb = load_var_builder(&path, device)?;

        // Multi-strategy prefix detection
        let prefix = if vb.contains_tensor("visual_proj.weight") {
            "visual_proj"
        } else if vb.contains_tensor("linear.weight") {
            "linear"
        } else {
            "mm"
        };

        let weight = vb
            .get_with_hints((), &format!("{}.weight", prefix), Default::default())
            .context(format!("Missing weight for {}", prefix))?;
        let bias = vb
            .get_with_hints((), &format!("{}.bias", prefix), Default::default())
            .ok();
        let model = Linear::new(weight, bias);

        Ok(Self {
            model,
            device: device.clone(),
            proj_type,
        })
    }
}

impl VisionProjector for LinearProjector {
    fn project(&self, features: &Tensor) -> Result<Tensor> {
        let features = features.to_device(&self.device)?;
        Ok(self
            .model
            .forward(&features)
            .context("Linear projection failed")?)
    }

    fn projector_type(&self) -> ProjectorType {
        self.proj_type
    }

    fn dimensions(&self) -> (usize, usize) {
        let w = self.model.weight();
        (w.dim(1).unwrap_or(0), w.dim(0).unwrap_or(0))
    }

    fn device(&self) -> &Device {
        &self.device
    }
}
