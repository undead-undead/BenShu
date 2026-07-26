use async_trait::async_trait;
use benshu_infra::traits::kernel::KernelCapability;
use benshu_infra::traits::resource::{AllocationRequest, AllocationResponse, ThrottleLevel};
use benshu_infra::traits::tool::{Tool, ToolDefinition};
use benshu_sensory::{SensoryHub, SensoryOutput};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// Advanced visual analysis tool with SOM and Context-Aware prompting.
pub struct VisualAnalysisTool {
    capability: Arc<dyn KernelCapability>,
    sensory: Arc<SensoryHub>,
    #[cfg(feature = "browser")]
    browser: Option<Arc<crate::tool::BrowserTool>>,
    provider: Option<Arc<dyn benshu_brain::agent::provider::Provider>>,
}

impl VisualAnalysisTool {
    pub fn new(
        capability: Arc<dyn KernelCapability>,
        sensory: Arc<SensoryHub>,
        #[cfg(feature = "browser")] browser: Option<Arc<crate::tool::BrowserTool>>,
        provider: Option<Arc<dyn benshu_brain::agent::provider::Provider>>,
    ) -> Self {
        Self {
            capability,
            sensory,
            #[cfg(feature = "browser")]
            browser,
            provider,
        }
    }
}

#[derive(Deserialize)]
struct VisualArgs {
    action: String,
    prompt: String,
    #[serde(default)]
    som: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    model: Option<String>, // Dynamic model selection
}

#[async_trait]
impl Tool for VisualAnalysisTool {
    fn name(&self) -> String {
        "visual_analysis".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "visual_analysis".to_string(),
            description: "Deep visual analysis of images or browser state. Supports Point-and-Click reasoning via SOM (Set-of-Mark) labels. Best for: 'What is in this image?', 'Where is the blue button? (using @eN refs)'.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["analyze_browser", "analyze_image"], "description": "Source of visual data" },
                    "prompt": { "type": "string", "description": "The question or instruction for the vision model" },
                    "som": { "type": "boolean", "description": "Enable visual UID labels (Set-of-Mark) for browser elements", "default": true },
                    "path": { "type": "string", "description": "Path to image if using 'analyze_image'" }
                },
                "required": ["action", "prompt"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use 'analyze_browser' to see the current page with UID tags. If the vision model refers to a tag like @e5, you can then use browser_browse(action='click', selector='@e5').".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: VisualArgs = serde_json::from_str(arguments)?;

        // Phase 10: Mandatory Resource Arbitration
        // Vision tasks require heavy VRAM/RAM. Pre-allocating 2GB for local vision as safety budget.
        let request = AllocationRequest {
            agent_id: "visual_tool".into(),
            role: ThrottleLevel::Medium,
            vram_mb: 2048,
            ram_mb: 512,
            cpu_cores: Some(0.5),
        };

        match self.capability.request_resource(request).await {
            AllocationResponse::Granted { .. } | AllocationResponse::Throttled { .. } => {
                match args.action.as_str() {
                    "analyze_browser" => self.analyze_browser(&args).await,
                    "analyze_image" => self.analyze_image(&args).await,
                    _ => Err(anyhow::anyhow!("Unknown visual action: {}", args.action)),
                }
            }
            AllocationResponse::Denied(reason) => Err(anyhow::anyhow!(
                "Visual analysis denied by Kernel Resource Arbiter: {}",
                reason
            )),
        }
    }
}

impl VisualAnalysisTool {
    #[cfg(feature = "browser")]
    async fn analyze_browser(&self, args: &VisualArgs) -> anyhow::Result<String> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Browser tool not available"))?;

        let png_data = browser.screenshot_binary(args.som).await?;
        let img = image::load_from_memory(&png_data)?;

        let output_res = if let (Some(provider), Some(model)) = (&self.provider, args.model.clone())
        {
            // Priority 1: Try the Agent's own provider
            let handler = ProviderVisionHandler::new(Arc::clone(provider), model);
            let cloud_plugin = benshu_sensory::vision::cloud::CloudVisionPlugin::new(
                "agent-cloud-vision",
                Arc::new(handler),
            );

            match benshu_sensory::vision::VisionPlugin::process(
                &cloud_plugin,
                &img,
                Some(&args.prompt),
            )
            .await
            {
                Ok(out) => Ok(out),
                Err(e) => {
                    tracing::warn!("Agent provider vision failed (Brain is blind?): {}. Falling back to local SensoryHub.", e);
                    // Priority 2: Fallback to local SensoryHub (Hardware Eye)
                    self.sensory
                        .vision_check(img, Some(&args.prompt), args.model.as_deref())
                        .await
                }
            }
        } else {
            // No brain provider, directly use local SensoryHub
            self.sensory
                .vision_check(img, Some(&args.prompt), args.model.as_deref())
                .await
        };

        match output_res? {
            SensoryOutput::Text(t) => Ok(t),
            SensoryOutput::Coordinates { x, y, label } => {
                let label_str = label.unwrap_or_else(|| "unlabeled".to_string());

                // Try to resolve coordinate to @eN ref if browser tool is present
                if let Some(browser) = &self.browser {
                    if let Ok(ref_id) = browser.resolve_ref_from_coords(x, y).await {
                        return Ok(format!(
                            "Found target at {} ([{}, {}]) - {}",
                            ref_id, x, y, label_str
                        ));
                    }
                }

                Ok(format!(
                    "Point of Interest: [{}, {}] - Context: {}",
                    x, y, label_str
                ))
            }
            _ => Err(anyhow::anyhow!(
                "Unsupported output type from sensory hub (expected Text/Coordinates)"
            )),
        }
    }

    #[cfg(not(feature = "browser"))]
    async fn analyze_browser(&self, _args: &VisualArgs) -> anyhow::Result<String> {
        Err(anyhow::anyhow!(
            "Browser analysis requires 'browser' feature"
        ))
    }

    async fn analyze_image(&self, args: &VisualArgs) -> anyhow::Result<String> {
        let path = args
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Image path required for 'analyze_image'"))?;
        let img = image::open(path)?;

        let output_res = if let (Some(provider), Some(model)) = (&self.provider, args.model.clone())
        {
            let handler = ProviderVisionHandler::new(Arc::clone(provider), model);
            let cloud_plugin = benshu_sensory::vision::cloud::CloudVisionPlugin::new(
                "agent-cloud-vision",
                Arc::new(handler),
            );

            match benshu_sensory::vision::VisionPlugin::process(
                &cloud_plugin,
                &img,
                Some(&args.prompt),
            )
            .await
            {
                Ok(out) => Ok(out),
                Err(e) => {
                    tracing::warn!(
                        "Agent provider vision failed: {}. Falling back to local SensoryHub.",
                        e
                    );
                    self.sensory
                        .vision_check(img, Some(&args.prompt), args.model.as_deref())
                        .await
                }
            }
        } else {
            self.sensory
                .vision_check(img, Some(&args.prompt), args.model.as_deref())
                .await
        };

        match output_res? {
            SensoryOutput::Text(t) => Ok(t),
            SensoryOutput::Coordinates { x, y, label } => Ok(format!(
                "Point of Interest: [{}, {}] - Context: {}",
                x,
                y,
                label.unwrap_or_default()
            )),
            _ => Err(anyhow::anyhow!(
                "Unsupported output type from sensory hub (expected Text/Coordinates)"
            )),
        }
    }
}

/// A bridge that allows regular LLM providers (OpenAI, Gemini) to act as CloudVisionHandlers.
pub struct ProviderVisionHandler {
    provider: Arc<dyn benshu_brain::agent::provider::Provider>,
    model: String,
}

impl ProviderVisionHandler {
    pub fn new(provider: Arc<dyn benshu_brain::agent::provider::Provider>, model: String) -> Self {
        Self { provider, model }
    }
}

#[async_trait]
impl benshu_sensory::vision::cloud::CloudVisionHandler for ProviderVisionHandler {
    async fn analyze(
        &self,
        image: &image::DynamicImage,
        prompt: Option<&str>,
    ) -> anyhow::Result<String> {
        use base64::Engine;
        use benshu_brain::agent::message::{Content, ContentPart, ImageSource, Message, Role};
        use benshu_brain::agent::provider::ChatRequest;

        // 1. Encode image to Base64
        let mut buffer = std::io::Cursor::new(Vec::new());
        image.write_to(&mut buffer, image::ImageFormat::Png)?;
        let base64_data = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());

        // 2. Prepare request
        let mut parts = vec![ContentPart::Text {
            text: prompt.unwrap_or("What is in this image?").to_string(),
        }];

        // Add specific grounding instructions if needed
        if prompt
            .map(|p| p.contains("location") || p.contains("position"))
            .unwrap_or(false)
        {
            parts[0] = ContentPart::Text {
                text: format!(
                    "{}\nIMPORTANT: Return bounding boxes in [ymin, xmin, ymax, xmax] format normalized to 0-1000.",
                    prompt.unwrap_or("Locate objects")
                )
            };
        }

        parts.push(ContentPart::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: base64_data,
            },
        });

        let req = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::new(Role::User, Content::Parts(parts))],
            ..Default::default()
        };

        // 3. Execute and collect
        let stream = self.provider.stream_completion(req).await?;
        let text = stream.collect_text().await?;

        Ok(text)
    }
}
