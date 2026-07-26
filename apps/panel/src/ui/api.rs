use crate::app::ClawPanel;
use crate::app_state::{ApiSubTab, VaultEntry};
use crate::common::palette;
use crate::i18n::t;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};

fn human_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / GIB)
    } else {
        format!("{:.1} MB", bytes as f64 / MIB)
    }
}

const LLAMA_CTX_SIZE_PRESETS: &[(u32, &str)] = &[
    (8_192, "8K - lightweight"),
    (16_384, "16K - compact"),
    (32_768, "32K - stable"),
    (65_536, "64K - long"),
    (98_304, "96K - extended"),
    (131_072, "128K - high"),
    (262_144, "256K - stress"),
];

fn llama_ctx_size_label(ctx_size: u32) -> String {
    LLAMA_CTX_SIZE_PRESETS
        .iter()
        .find_map(|(value, label)| (*value == ctx_size).then(|| format!("{label} ({value})")))
        .unwrap_or_else(|| format!("Current custom ({ctx_size})"))
}

fn llama_ctx_capacity_reference(
    ctx_size: u32,
    gpu_layers: u32,
    profile: &str,
    kv_offload: bool,
) -> String {
    let ctx_band = match ctx_size {
        0..=8192 => "current ctx is a small/interactive window",
        8193..=32768 => "current ctx is a medium long-context window",
        32769..=65536 => "current ctx is a large long-context window",
        65537..=131072 => "current ctx is an extra-large long-context window",
        _ => "current ctx is a stress-test context window",
    };
    let kv_mb = benshu_inference::estimate_llama_kv_cache_budget_mb(ctx_size, profile);
    let kv_gib = kv_mb as f64 / 1024.0;
    let kv_location = if kv_offload { "VRAM" } else { "RAM/commit" };
    format!(
        "Reference only, not enforced: model size, layer count, cache type, KV offload, and RAM decide the real ceiling. Current ctx={ctx_size}, gpu_layers={gpu_layers}; {ctx_band}. Estimated KV cache at this ctx is about {kv_mb}MiB ({kv_gib:.1}GiB) on {kv_location}."
    )
}

fn artifact_matches_role(artifact: &crate::api::LocalModelArtifact, role_key: &str) -> bool {
    let label = artifact.label.to_lowercase();
    if label.contains("mmproj") {
        return false;
    }

    let kind = artifact.artifact_kind.as_str();
    let roles = &artifact.llama_cpp.role_support;
    let ready_small_model = matches!(kind, "onnx_directory" | "onnx_file");
    let text_gguf = kind == "gguf" && artifact.llama_cpp.compatibility != "not_llama_cpp_artifact";
    let multimodal_gguf =
        kind == "gguf" && artifact.llama_cpp.compatibility == "multimodal_compatible";

    match role_key {
        "embedding" | "rerank" => {
            ready_small_model
                || kind == "safetensors_directory"
                || roles
                    .iter()
                    .any(|role| matches!(role.as_str(), "embedding" | "rerank"))
        }
        "vision" => {
            ready_small_model
                || multimodal_gguf
                || kind == "safetensors_directory"
                || roles
                    .iter()
                    .any(|role| matches!(role.as_str(), "vision" | "vlm" | "llm"))
                || label.contains("vision")
                || label.contains("vlm")
                || label.contains("llava")
                || label.contains("moondream")
        }
        "ocr" => ready_small_model || multimodal_gguf || kind == "safetensors_directory",
        "slm_tactical" => {
            text_gguf
                || roles
                    .iter()
                    .any(|role| matches!(role.as_str(), "slm" | "llm"))
        }
        "fact_check" => {
            ready_small_model
                || text_gguf
                || roles
                    .iter()
                    .any(|role| matches!(role.as_str(), "validation" | "nlu" | "llm" | "slm"))
        }
        "speech_to_text" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("whisper")
                || label.contains("stt")
                || label.contains("asr")
        }
        "audio_understanding" => {
            ready_small_model
                || kind == "safetensors_directory"
                || multimodal_gguf
                || label.contains("audio")
                || label.contains("hearing")
                || label.contains("listen")
                || label.contains("qwen2-audio")
        }
        "realtime_vad" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("vad")
                || label.contains("silero")
        }
        "duplex_voice" => {
            ready_small_model
                || kind == "safetensors_directory"
                || multimodal_gguf
                || label.contains("duplex")
                || label.contains("realtime")
                || label.contains("speech")
                || label.contains("voice")
                || label.contains("audio")
        }
        "text_to_speech" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("tts")
                || label.contains("piper")
                || label.contains("kokoro")
        }
        "image_generation" => {
            matches!(
                kind,
                "diffusers_directory" | "image_onnx_directory" | "image_bridge"
            ) || label.contains("sd")
                || label.contains("sdxl")
                || label.contains("flux")
                || label.contains("kontext")
                || label.contains("diffusion")
        }
        "image_edit" => {
            matches!(
                kind,
                "diffusers_directory" | "image_onnx_directory" | "image_bridge"
            ) || label.contains("edit")
                || label.contains("inpaint")
                || label.contains("img2img")
                || label.contains("kontext")
                || label.contains("qwen-image-edit")
                || label.contains("flux")
        }
        "local_classifier" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("classif")
                || label.contains("moderation")
                || label.contains("detector")
        }
        "local_router" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("router")
                || label.contains("route")
                || label.contains("selector")
        }
        "local_safety" => {
            ready_small_model
                || kind == "safetensors_directory"
                || label.contains("safety")
                || label.contains("guard")
                || label.contains("moderation")
                || label.contains("toxicity")
                || label.contains("nsfw")
        }
        _ => true,
    }
}

fn artifact_role_rank(artifact: &crate::api::LocalModelArtifact, role_key: &str) -> i32 {
    let kind = artifact.artifact_kind.as_str();
    match role_key {
        "slm_tactical" => match kind {
            "gguf" => 0,
            "onnx_directory" | "onnx_file" => 1,
            "safetensors_directory" => 2,
            _ => 3,
        },
        "vision" => match kind {
            "gguf" => 0,
            "onnx_directory" | "onnx_file" => 1,
            "safetensors_directory" => 2,
            _ => 3,
        },
        "embedding"
        | "rerank"
        | "ocr"
        | "speech_to_text"
        | "text_to_speech"
        | "audio_understanding"
        | "realtime_vad"
        | "duplex_voice"
        | "local_classifier"
        | "local_router"
        | "local_safety" => match kind {
            "onnx_directory" | "onnx_file" => 0,
            "safetensors_directory" => 1,
            "gguf" => 2,
            _ => 3,
        },
        "fact_check" => match kind {
            "onnx_directory" | "onnx_file" => 0,
            "gguf" => 1,
            "safetensors_directory" => 2,
            _ => 3,
        },
        "image_generation" => match kind {
            "image_onnx_directory" => 0,
            "diffusers_directory" => 1,
            "image_bridge" => 2,
            _ => 3,
        },
        "image_edit" => match kind {
            "image_onnx_directory" => 0,
            "diffusers_directory" => 1,
            "image_bridge" => 2,
            _ => 3,
        },
        _ => 9,
    }
}

fn discovered_model_picker(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    role_key: &str,
    role_label: &str,
    current_model: &str,
) -> Option<String> {
    let night = panel.state.night_mode;
    let artifacts = panel
        .state
        .local_model_artifacts
        .as_ref()
        .map(|catalog| {
            let mut artifacts = catalog
                .artifacts
                .iter()
                .filter(|artifact| artifact_matches_role(artifact, role_key))
                .cloned()
                .collect::<Vec<_>>();
            artifacts.sort_by(|a, b| {
                artifact_role_rank(a, role_key)
                    .cmp(&artifact_role_rank(b, role_key))
                    .then_with(|| a.label.cmp(&b.label))
            });
            artifacts
        })
        .unwrap_or_default();

    let selected_text = artifacts
        .iter()
        .find(|artifact| artifact.path == current_model)
        .map(|artifact| artifact.label.clone())
        .unwrap_or_else(|| "Choose discovered local model...".to_string());

    let mut selected_model = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Detected Local Models")
                .small()
                .color(palette::text_dim(night)),
        );
        if ui
            .small_button("Refresh")
            .on_hover_text("Rescan the local models directory under models/")
            .clicked()
        {
            panel
                .state
                .do_local_model_artifacts_refresh(&panel.rt, ui.ctx());
        }
        if panel.state.local_model_artifacts_loading {
            ui.label(RichText::new("scanning...").small().color(palette::INFO));
        }
        if let Some(error) = &panel.state.local_model_artifacts_error {
            ui.label(RichText::new(error).small().color(palette::DANGER));
        }
    });

    egui::ComboBox::from_id_salt(format!("{}_discovered_local_models", role_key))
        .selected_text(selected_text)
        .width(340.0)
        .show_ui(ui, |ui| {
            if artifacts.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "No matching local artifacts discovered under models/ for {}",
                        role_label
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
            } else {
                for artifact in &artifacts {
                    let is_selected = current_model == artifact.path;
                    let display = format!(
                        "{}  [{} | {}]",
                        artifact.label,
                        artifact.artifact_kind,
                        human_bytes(artifact.size_bytes)
                    );
                    let response = ui.selectable_label(is_selected, display);
                    if response.clicked() {
                        selected_model = Some(artifact.path.clone());
                        ui.close_menu();
                    }
                    response.on_hover_ui(|ui| {
                        ui.set_max_width(420.0);
                        ui.label(
                            RichText::new(format!("Role: {}", role_label))
                                .small()
                                .strong(),
                        );
                        ui.label(RichText::new(&artifact.relative_path).small().monospace());
                        ui.label(
                            RichText::new(format!(
                                "llama.cpp: {}",
                                artifact.llama_cpp.compatibility
                            ))
                            .small(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "host: {}",
                                artifact.llama_cpp.current_host_status
                            ))
                            .small(),
                        );
                        ui.label(
                            RichText::new(format!("mmproj: {}", artifact.llama_cpp.mmproj_status))
                                .small(),
                        );
                        ui.label(
                            RichText::new(&artifact.llama_cpp.note)
                                .small()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "runtime: {}{}",
                                artifact.llama_cpp.server_note,
                                artifact
                                    .llama_cpp
                                    .server_build
                                    .map(|build| format!(" build b{build}"))
                                    .unwrap_or_default()
                            ))
                            .small()
                            .color(
                                if artifact.llama_cpp.server_supported {
                                    palette::SUCCESS
                                } else {
                                    palette::WARNING
                                },
                            ),
                        );
                    });
                }
            }
        });

    selected_model
}

fn folder_picker_button(
    ui: &mut egui::Ui,
    button_label: &str,
    current_model: &str,
) -> Option<String> {
    if ui
        .small_button(button_label)
        .on_hover_text(
            "Open a system folder picker and use the selected directory as this model path.",
        )
        .clicked()
    {
        let mut dialog = rfd::FileDialog::new();
        if !current_model.is_empty() && !current_model.starts_with("api:") {
            dialog = dialog.set_directory(current_model);
        }
        if let Some(path) = dialog.pick_folder() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

fn file_picker_button(
    ui: &mut egui::Ui,
    button_label: &str,
    current_directory: &str,
) -> Option<Vec<String>> {
    if ui
        .small_button(button_label)
        .on_hover_text("Open a system file picker and import the selected text knowledge files.")
        .clicked()
    {
        let mut dialog = rfd::FileDialog::new();
        if !current_directory.is_empty() {
            dialog = dialog.set_directory(current_directory);
        }
        if let Some(paths) = dialog.pick_files() {
            return Some(
                paths
                    .into_iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
            );
        }
    }
    None
}

fn persist_global_model_binding(panel: &mut ClawPanel, ctx: &egui::Context) {
    panel.state.do_save_sensory_settings(&panel.rt, ctx);
    panel.state.do_local_model_stack_refresh(&panel.rt, ctx);
    panel.state.do_local_model_artifacts_refresh(&panel.rt, ctx);
}

fn render_global_binding_picker_row(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    role_key: &str,
    role_label: &str,
    model_value: &mut String,
    helper_text: &str,
) {
    let night = panel.state.night_mode;
    let current_model = model_value.clone();
    if let Some(new_model) =
        discovered_model_picker(panel, ui, role_key, role_label, &current_model)
    {
        *model_value = new_model;
        persist_global_model_binding(panel, ctx);
    }
    ui.horizontal_wrapped(|ui| {
        let current_model = model_value.clone();
        if let Some(new_model) = folder_picker_button(ui, "Choose Folder…", &current_model) {
            *model_value = new_model;
            persist_global_model_binding(panel, ctx);
        }
        ui.label(
            RichText::new(helper_text)
                .small()
                .color(palette::text_dim(night)),
        );
    });
}

pub fn render_models_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let is_cloud = panel.state.api_subtab == ApiSubTab::Cloud;
            let is_local = panel.state.api_subtab == ApiSubTab::Local;

            let subtab_font = FontId::new(20.0, egui::FontFamily::Proportional);
            if ui
                .selectable_label(
                    is_cloud,
                    RichText::new(t("tabs.cloud", panel.state.language)).font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.api_subtab = ApiSubTab::Cloud;
            }
            if ui
                .selectable_label(
                    is_local,
                    RichText::new(t("tabs.local", panel.state.language)).font(subtab_font),
                )
                .clicked()
            {
                panel.state.api_subtab = ApiSubTab::Local;
            }
        });
        ui.separator();
        ui.add_space(8.0);

        match panel.state.api_subtab {
            ApiSubTab::Cloud => render_cloud_models(panel, ui, ctx),
            ApiSubTab::Local => render_local_models(panel, ui, ctx),
        }
    });
}

fn render_cloud_models(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;

    ui.vertical(|ui| {
        ui.heading("Cloud Models");
        ui.label(
            RichText::new(
                "Manage cloud providers, shared speech synthesis, and creative generation backends from one unified model surface.",
            )
            .small()
            .color(palette::text_dim(night)),
        );
        ui.add_space(16.0);
        render_api_keys(panel, ui, ctx);
        ui.add_space(20.0);
        render_api_speech(panel, ui, ctx);
        ui.add_space(20.0);
        render_api_creative(panel, ui, ctx);
    });
}

fn render_local_models(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;

    if panel.state.local_model_stack.is_none()
        && panel.state.pending_local_model_stack_promise.is_none()
        && !panel.state.local_model_stack_loading
    {
        panel.state.do_local_model_stack_refresh(&panel.rt, ctx);
    }
    if panel.state.local_model_artifacts.is_none()
        && panel.state.pending_local_model_artifacts_promise.is_none()
        && !panel.state.local_model_artifacts_loading
    {
        panel.state.do_local_model_artifacts_refresh(&panel.rt, ctx);
    }

    ui.vertical(|ui| {
        ui.heading("Local Models");
        ui.label(
            RichText::new(
                "Read and configure the local model stack, media runtime, and workspace-side local execution surfaces without splitting them across System pages.",
            )
            .small()
            .color(palette::text_dim(night)),
        );
        ui.add_space(16.0);
        render_local_model_stack_overview(panel, ui, ctx);
        ui.add_space(16.0);
        render_llama_cpp_runtime_controls(panel, ui, ctx);
        ui.add_space(16.0);
        render_windows_ml_onnx_runtime_controls(panel, ui, ctx);
        ui.add_space(16.0);
        render_knowledge_import_controls(panel, ui, ctx);
        ui.add_space(16.0);
        render_local_media_contracts(panel, ui);
        ui.add_space(16.0);
        render_local_ocr_card(panel, ui);
        ui.add_space(20.0);
        render_local_workspaces(panel, ui, ctx);
    });
}

fn render_local_model_stack_overview(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
) {
    let night = panel.state.night_mode;

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Local Model Stack Overview").strong().color(palette::ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        panel.state.do_local_model_stack_refresh(&panel.rt, ctx);
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Unified read-only view of model roles, bound sources, readiness, runtime profile, and media follow-up contracts.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            if panel.state.local_model_stack_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading local model stack...");
                });
            }

            if let Some(error) = &panel.state.local_model_stack_error {
                ui.label(RichText::new(error).small().color(palette::DANGER));
            }

            if let Some(stack) = &panel.state.local_model_stack {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Product Mainline: {}", stack.product_mainline))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.label(
                        RichText::new(format!("Host Runtime: {}", stack.host_runtime))
                            .small()
                            .color(if stack.host_runtime == "windows_native_mainline" {
                                palette::SUCCESS
                            } else {
                                palette::WARNING
                            }),
                    );
                ui.label(
                    RichText::new(format!(
                        "Windows Native Priority: {}",
                        if stack.windows_native_priority { "enabled" } else { "disabled" }
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!("Deployment Lane: {}", stack.deployment_lane))
                        .small()
                        .color(if stack.deployment_lane == "product_mainline" {
                            palette::SUCCESS
                        } else {
                            palette::WARNING
                        }),
                );
                ui.label(
                    RichText::new(format!(
                        "Voice: {}",
                            if stack.media_runtime.global_voice_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        ))
                        .small()
                        .color(palette::text_dim(night)),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Vision: {} ({})",
                            if stack.media_runtime.local_vision_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            },
                            stack.media_runtime.local_vision_status
                        ))
                        .small()
                        .color(palette::text_dim(night)),
                    );
                });
                if !stack.validation_tracks.is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "Validation Tracks: {}",
                            stack.validation_tracks.join(", ")
                        ))
                        .small()
                        .color(palette::text_dim(night)),
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "Deployment Strategy: {}",
                        stack.deployment_strategy
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!("Deployment Note: {}", stack.deployment_note))
                        .small()
                        .color(if stack.deployment_lane == "product_mainline" {
                            palette::text_dim(night)
                        } else {
                            palette::WARNING
                        }),
                );
                ui.label(
                    RichText::new(format!(
                        "Small Model Target: {} ({})",
                        stack.small_model_runtime_target, stack.small_model_runtime_readiness
                    ))
                    .small()
                    .color(if stack.small_model_runtime_readiness == "windows_native_ready" {
                        palette::SUCCESS
                    } else {
                        palette::WARNING
                    }),
                );
                ui.label(
                    RichText::new(format!(
                        "Main Brain Target: {}",
                        stack.main_brain_runtime_target
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Small Model Runtime Note: {}",
                        stack.small_model_runtime_reason
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Execution Backend Linked: {}",
                        if stack.small_model_execution_linked {
                            "yes"
                        } else {
                            "no"
                        }
                    ))
                    .small()
                    .color(if stack.small_model_execution_linked {
                        palette::SUCCESS
                    } else {
                        palette::WARNING
                    }),
                );
                ui.label(
                    RichText::new(format!(
                        "Execution Provider: {}",
                        stack.small_model_execution_provider
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Device Target: {}",
                        stack.small_model_device_target
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Fallback Mode: {}",
                        stack.small_model_fallback_mode
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Small Model Outcome: {}",
                        stack.small_model_runtime_outcome
                    ))
                    .small()
                    .color(if stack.small_model_runtime_outcome == "windows_native_active" {
                        palette::SUCCESS
                    } else {
                        palette::WARNING
                    }),
                );
                ui.label(
                    RichText::new(format!(
                        "Small Model Strategy: {}",
                        stack.small_model_runtime_strategy
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Source Contracts: {}",
                        stack.media_runtime.source_contracts.join(", ")
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Follow-up Contracts: {}",
                        stack.media_runtime.followup_contracts.join(", ")
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.add_space(12.0);

                for entry in &stack.entries {
                    egui::Frame::new()
                        .fill(palette::bg_surface(night))
                        .stroke(Stroke::new(1.0, palette::border(night)))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(entry.role.to_uppercase())
                                        .strong()
                                        .color(palette::ACCENT),
                                );
                                ui.label(
                                    RichText::new(format!("source={}", entry.source))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                                ui.label(
                                    RichText::new(format!("readiness={}", entry.readiness))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                                ui.label(
                                    RichText::new(format!("profile={}", entry.runtime_profile))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                                ui.label(
                                    RichText::new(format!("track={}", entry.product_track))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                            });
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("Binding: {}", entry.configured_model))
                                    .small(),
                            );
                            if let Some(factory_id) = &entry.factory_id {
                                ui.label(
                                    RichText::new(format!("Factory: {}", factory_id))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                            }
                            if !entry.declared_roles.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "Declared Roles: {}",
                                        entry.declared_roles.join(", ")
                                    ))
                                    .small()
                                    .color(palette::text_dim(night)),
                                );
                            }
                            ui.label(
                                RichText::new(format!(
                                    "Preferred Backend: {}",
                                    entry.preferred_backend
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Current Backend: {}",
                                    entry.current_backend
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Execution Provider: {}",
                                    entry.execution_provider
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Artifact Kind: {}",
                                    entry.artifact_kind
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp Compatibility: {}",
                                    entry.llama_cpp.compatibility
                                ))
                                .small()
                                .color(
                                    if entry.llama_cpp.compatibility
                                        == "multimodal_compatible"
                                        || entry.llama_cpp.compatibility == "text_compatible"
                                    {
                                        palette::SUCCESS
                                    } else if entry.llama_cpp.compatibility == "unconfigured" {
                                        palette::text_dim(night)
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            if !entry.llama_cpp.role_support.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "llama.cpp Roles: {}",
                                        entry.llama_cpp.role_support.join(", ")
                                    ))
                                    .small()
                                    .color(palette::text_dim(night)),
                                );
                            }
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp mmproj: {}",
                                    entry.llama_cpp.mmproj_status
                                ))
                                .small()
                                .color(
                                    if entry.llama_cpp.mmproj_status == "resolved"
                                        || entry.llama_cpp.mmproj_status
                                            == "text_only_no_mmproj"
                                        || entry.llama_cpp.mmproj_status == "not_applicable"
                                    {
                                        palette::text_dim(night)
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp Host Status: {}",
                                    entry.llama_cpp.current_host_status
                                ))
                                .small()
                                .color(
                                    if entry.llama_cpp.current_host_status
                                        == "llama_cpp_runtime_selected"
                                        || entry.llama_cpp.current_host_status
                                            == "llama_cpp_multimodal_runtime_selected"
                                        || entry.llama_cpp.current_host_status
                                            == "llama_cpp_text_runtime_selected"
                                        || entry.llama_cpp.current_host_status
                                            == "main_brain_llama_cpp_track_active"
                                    {
                                        palette::SUCCESS
                                    } else if entry.llama_cpp.current_host_status
                                        == "not_applicable"
                                        || entry.llama_cpp.current_host_status == "unconfigured"
                                    {
                                        palette::text_dim(night)
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp Note: {}",
                                    entry.llama_cpp.note
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp Host Note: {}",
                                    entry.llama_cpp.current_host_note
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "llama.cpp Runtime: {}{}",
                                    entry.llama_cpp.server_note,
                                    entry
                                        .llama_cpp
                                        .server_path
                                        .as_ref()
                                        .map(|path| format!(" ({path})"))
                                        .unwrap_or_default()
                                ))
                                .small()
                                .color(if entry.llama_cpp.server_supported {
                                    palette::SUCCESS
                                } else {
                                    palette::WARNING
                                }),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Effective Runtime: {}",
                                    entry.effective_runtime_state
                                ))
                                .small()
                                .color(
                                    if entry.effective_runtime_state == "windows_native_active"
                                        || entry.effective_runtime_state
                                            == "main_brain_runtime_active"
                                        || entry.effective_runtime_state
                                            == "specialized_runtime_active"
                                    {
                                        palette::SUCCESS
                                    } else if entry.effective_runtime_state == "unconfigured" {
                                        palette::text_dim(night)
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Runtime Note: {}",
                                    entry.effective_runtime_reason
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Effective Outcome: {}",
                                    entry.effective_runtime_outcome
                                ))
                                .small()
                                .color(
                                    if entry.effective_runtime_outcome
                                        == "windows_native_active"
                                        || entry.effective_runtime_outcome
                                            == "main_brain_runtime_active"
                                        || entry.effective_runtime_outcome
                                            == "specialized_runtime_active"
                                    {
                                        palette::SUCCESS
                                    } else if entry.effective_runtime_outcome == "unconfigured" {
                                        palette::text_dim(night)
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Effective Class: {}",
                                    entry.effective_runtime_class
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            if let Some(failure_reason) = &entry.effective_runtime_failure_reason {
                                ui.label(
                                    RichText::new(format!(
                                        "Failure Reason: {}",
                                        failure_reason
                                    ))
                                    .small()
                                    .color(palette::WARNING),
                                );
                            }
                            ui.label(
                                RichText::new(format!(
                                    "Effective Strategy: {}",
                                    entry.effective_runtime_strategy
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Windows-native Plan: {}",
                                    entry.windows_native_plan_status
                                ))
                                .small()
                                .color(
                                    if entry.windows_native_plan_status
                                        == "windows_native_target_active"
                                        || entry.windows_native_plan_status
                                            == "main_brain_llama_cpp"
                                        || entry.windows_native_plan_status
                                            == "specialized_runtime_intentional"
                                    {
                                        palette::SUCCESS
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Plan Note: {}",
                                    entry.windows_native_plan_note
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Target Readiness: {}",
                                    entry.target_readiness
                                ))
                                .small()
                                .color(if entry.target_readiness == "target_contract_ready"
                                    || entry
                                        .target_readiness
                                        .starts_with("target_contract_ready:windows_native_ready")
                                {
                                    palette::SUCCESS
                                } else {
                                    palette::WARNING
                                }),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Target Note: {}",
                                    entry.target_reason
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Host Validation: {}",
                                    entry.host_validation_status
                                ))
                                .small()
                                .color(
                                    if entry.host_validation_status
                                        == "validated_on_current_windows_host"
                                        || entry.host_validation_status
                                            == "not_required_specialized_runtime"
                                        || entry.host_validation_status
                                            == "not_required_main_brain_track"
                                        || entry.host_validation_status == "not_applicable"
                                    {
                                        palette::SUCCESS
                                    } else {
                                        palette::WARNING
                                    },
                                ),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Host Validation Note: {}",
                                    entry.host_validation_note
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            if let Some(fallback) = &entry.fallback_hint {
                                ui.label(
                                    RichText::new(format!("Fallback: {}", fallback))
                                        .small()
                                        .color(palette::WARNING),
                                );
                            }
                        });
                    ui.add_space(8.0);
                }
            }
        });
}

fn render_windows_ml_onnx_runtime_controls(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
) {
    let night = panel.state.night_mode;

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Windows ML / ONNX Runtime")
                    .strong()
                    .color(palette::ACCENT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Global small-model runtime surface for system capabilities. This card manages the shared Windows-native execution lane and the capability bindings that do not require per-agent loadout.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            if let Some(stack) = &panel.state.local_model_stack {
                ui.label(RichText::new("Runtime Summary").strong());
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Runtime Family: {}",
                        if stack.small_model_runtime_target == "onnx_runtime_directml_winml" {
                            "Windows ML / ONNX Runtime"
                        } else {
                            stack.small_model_runtime_target.as_str()
                        }
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Execution Provider Preference: {}",
                        stack.small_model_execution_provider
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Device Target: {}",
                        stack.small_model_device_target
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "CPU Fallback Policy: {}",
                        stack.small_model_fallback_mode
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!("Readiness: {}", stack.small_model_runtime_readiness))
                        .small()
                        .color(if stack.small_model_runtime_readiness == "windows_native_ready" {
                            palette::SUCCESS
                        } else {
                            palette::WARNING
                        }),
                );
                ui.label(
                    RichText::new(format!(
                        "Validation Note: {}",
                        stack.small_model_runtime_reason
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.add_space(12.0);
            }

            ui.label(RichText::new("Runtime Controls").strong());
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                ui.label("Runtime Family");
                egui::ComboBox::from_id_salt("windows_ml_runtime_family")
                    .selected_text(&panel.state.windows_ml_runtime_family)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut panel.state.windows_ml_runtime_family,
                            "windows_ml_onnx_runtime".to_string(),
                            "Windows ML / ONNX Runtime",
                        );
                    });

                ui.label("Execution Provider");
                egui::ComboBox::from_id_salt("windows_ml_execution_provider")
                    .selected_text(&panel.state.windows_ml_execution_provider_preference)
                    .show_ui(ui, |ui| {
                        for option in ["directml", "cpu", "auto"] {
                            ui.selectable_value(
                                &mut panel.state.windows_ml_execution_provider_preference,
                                option.to_string(),
                                option,
                            );
                        }
                    });

                ui.label("Device Target");
                egui::ComboBox::from_id_salt("windows_ml_device_target")
                    .selected_text(&panel.state.windows_ml_device_target)
                    .show_ui(ui, |ui| {
                        for option in ["auto", "gpu", "cpu", "npu"] {
                            ui.selectable_value(
                                &mut panel.state.windows_ml_device_target,
                                option.to_string(),
                                option,
                            );
                        }
                    });

                ui.label("CPU Fallback");
                egui::ComboBox::from_id_salt("windows_ml_cpu_fallback_policy")
                    .selected_text(&panel.state.windows_ml_cpu_fallback_policy)
                    .show_ui(ui, |ui| {
                        for (label, value) in [
                            ("Allow", "allow"),
                            ("Strict Accelerator", "strict"),
                            ("Provider Downgrade", "provider_downgrade"),
                        ] {
                            ui.selectable_value(
                                &mut panel.state.windows_ml_cpu_fallback_policy,
                                value.to_string(),
                                label,
                            );
                        }
                    });
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("Graph Optimization");
                egui::ComboBox::from_id_salt("windows_ml_graph_optimization")
                    .selected_text(&panel.state.windows_ml_graph_optimization_level)
                    .show_ui(ui, |ui| {
                        for option in ["disable", "basic", "extended", "all"] {
                            ui.selectable_value(
                                &mut panel.state.windows_ml_graph_optimization_level,
                                option.to_string(),
                                option,
                            );
                        }
                    });
                ui.label("Intra Threads");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.state.windows_ml_intra_threads)
                        .desired_width(64.0),
                );
                ui.label("Inter Threads");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.state.windows_ml_inter_threads)
                        .desired_width(64.0),
                );
            });

            ui.add_space(10.0);
            ui.label(RichText::new("Capability Tuning").strong());
            ui.add_space(6.0);

            egui::Grid::new("windows_ml_capability_tuning")
                .num_columns(4)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Text").strong());
                    ui.label("Batch");
                    ui.add(
                        egui::Slider::new(&mut panel.state.windows_ml_text_batch_size, 1..=64),
                    );
                    ui.label(format!(
                        "Max Seq {}",
                        panel.state.windows_ml_text_max_sequence_length
                    ));
                    ui.end_row();

                    ui.label("");
                    ui.label("Max Sequence");
                    ui.add(
                        egui::Slider::new(
                            &mut panel.state.windows_ml_text_max_sequence_length,
                            128..=8192,
                        ),
                    );
                    ui.label("");
                    ui.end_row();

                    ui.label(RichText::new("Vision / OCR").strong());
                    ui.label("Max Image Side");
                    ui.add(
                        egui::Slider::new(
                            &mut panel.state.windows_ml_vision_max_image_side,
                            256..=2048,
                        ),
                    );
                    ui.label("Resize Policy");
                    ui.end_row();

                    ui.label("");
                    egui::ComboBox::from_id_salt("windows_ml_vision_resize_policy")
                        .selected_text(&panel.state.windows_ml_vision_resize_policy)
                        .show_ui(ui, |ui| {
                            for option in ["fit", "fill", "longest_side"] {
                                ui.selectable_value(
                                    &mut panel.state.windows_ml_vision_resize_policy,
                                    option.to_string(),
                                    option,
                                );
                            }
                        });
                    ui.label("");
                    ui.label("");
                    ui.end_row();

                    ui.label(RichText::new("Audio / STT").strong());
                    ui.label("Sample Rate");
                    ui.add(
                        egui::Slider::new(
                            &mut panel.state.windows_ml_audio_sample_rate_hz,
                            8_000..=48_000,
                        ),
                    );
                    ui.label(format!("Chunk {} ms", panel.state.windows_ml_audio_chunk_ms));
                    ui.end_row();

                    ui.label("");
                    ui.label("Chunk");
                    ui.add(
                        egui::Slider::new(&mut panel.state.windows_ml_audio_chunk_ms, 1000..=60000),
                    );
                    ui.label("");
                    ui.label("");
                    ui.end_row();

                    ui.label(RichText::new("Image Gen / Edit").strong());
                    ui.label("Width");
                    ui.add(egui::Slider::new(&mut panel.state.windows_ml_image_width, 256..=2048));
                    ui.label("Height");
                    ui.end_row();

                    ui.label("");
                    ui.label("Height");
                    ui.add(egui::Slider::new(&mut panel.state.windows_ml_image_height, 256..=2048));
                    ui.label(format!("Steps {}", panel.state.windows_ml_image_steps));
                    ui.end_row();

                    ui.label("");
                    ui.label("Steps");
                    ui.add(egui::Slider::new(&mut panel.state.windows_ml_image_steps, 1..=80));
                    ui.label("Guidance");
                    ui.end_row();

                    ui.label("");
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.state.windows_ml_image_guidance)
                            .desired_width(72.0),
                    );
                    ui.label("");
                    ui.label("");
                    ui.end_row();

                    ui.label(RichText::new("Realtime Voice").strong());
                    ui.label(format!(
                        "VAD Window {} ms",
                        panel.state.windows_ml_realtime_vad_window_ms
                    ));
                    ui.add(
                        egui::Slider::new(
                            &mut panel.state.windows_ml_realtime_vad_window_ms,
                            10..=200,
                        ),
                    );
                    ui.label(format!(
                        "Duplex Frame {} ms",
                        panel.state.windows_ml_duplex_frame_ms
                    ));
                    ui.end_row();

                    ui.label("");
                    ui.label("Duplex Frame");
                    ui.add(
                        egui::Slider::new(
                            &mut panel.state.windows_ml_duplex_frame_ms,
                            10..=200,
                        ),
                    );
                    ui.label("");
                    ui.label("");
                    ui.end_row();

                    ui.label(RichText::new("Safety / Router").strong());
                    ui.label("Threshold");
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.state.windows_ml_safety_threshold)
                            .desired_width(72.0),
                    );
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Apply Windows ML Runtime").strong())
                    .clicked()
                {
                    panel
                        .state
                        .do_save_windows_ml_runtime_settings(&panel.rt, ctx);
                }
                if ui.button("Reset Runtime Defaults").clicked() {
                    panel.state.windows_ml_runtime_family = "windows_ml_onnx_runtime".to_string();
                    panel.state.windows_ml_execution_provider_preference = "directml".to_string();
                    panel.state.windows_ml_device_target = "auto".to_string();
                    panel.state.windows_ml_cpu_fallback_policy = "allow".to_string();
                    panel.state.windows_ml_graph_optimization_level = "all".to_string();
                    panel.state.windows_ml_intra_threads.clear();
                    panel.state.windows_ml_inter_threads.clear();
                    panel.state.windows_ml_text_batch_size = 8;
                    panel.state.windows_ml_text_max_sequence_length = 1024;
                    panel.state.windows_ml_vision_max_image_side = 1024;
                    panel.state.windows_ml_vision_resize_policy = "fit".to_string();
                    panel.state.windows_ml_audio_sample_rate_hz = 16_000;
                    panel.state.windows_ml_audio_chunk_ms = 30_000;
                    panel.state.windows_ml_image_width = 1024;
                    panel.state.windows_ml_image_height = 1024;
                    panel.state.windows_ml_image_steps = 20;
                    panel.state.windows_ml_image_guidance = "7.5".to_string();
                    panel.state.windows_ml_realtime_vad_window_ms = 30;
                    panel.state.windows_ml_duplex_frame_ms = 20;
                    panel.state.windows_ml_safety_threshold = "0.5".to_string();
                }
            });

            ui.add_space(16.0);
            ui.label(RichText::new("Shared Runtime Controls").strong());
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                let voice_label = if panel.state.enable_global_voice {
                    "Voice runtime enabled"
                } else {
                    "Voice runtime disabled"
                };
                if ui.checkbox(&mut panel.state.enable_global_voice, voice_label).changed() {
                    crate::app_state::save_config(&panel.state);
                    panel.state.do_save_sensory_settings(&panel.rt, ui.ctx());
                }

                let consolidation_label = if panel.state.auto_consolidation_enabled {
                    "Background consolidation enabled"
                } else {
                    "Background consolidation disabled"
                };
                if ui
                    .checkbox(&mut panel.state.auto_consolidation_enabled, consolidation_label)
                    .changed()
                {
                    crate::app_state::save_config(&panel.state);
                    panel.state.do_save_sensory_settings(&panel.rt, ui.ctx());
                }
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label("RAM Budget");
                let ram_resp = ui.add(
                    egui::Slider::new(&mut panel.state.model_ram_limit_gb, 1..=32).suffix(" GB"),
                );
                ui.label("VRAM Budget");
                let vram_resp = ui.add(
                    egui::Slider::new(&mut panel.state.model_vram_limit_gb, 1..=32).suffix(" GB"),
                );
                if ram_resp.changed() || vram_resp.changed() {
                    crate::app_state::save_config(&panel.state);
                    panel.state.do_save_sensory_settings(&panel.rt, ui.ctx());
                }
            });

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);
            ui.label(RichText::new("Global Capability Bindings").strong());
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Bind system-wide capabilities such as Embedding, Rerank, OCR, STT, TTS, tactical small models, and image generation. These are global runtimes, not per-agent large-model loadouts.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);
            render_local_role_bindings_content(panel, ui, ctx);
        });
}

fn render_knowledge_import_controls(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Knowledge Import")
                        .strong()
                        .color(palette::ACCENT),
                );
                if panel.state.knowledge_import_loading {
                    ui.spinner();
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Import local knowledge files into RAG. Text files are indexed directly; PDF and Office Open XML files (.pdf, .docx, .xlsx, .pptx) are parsed first. Legacy .doc/.xls/.ppt files are not supported. Single file limit: 20MB.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            ui.horizontal_wrapped(|ui| {
                ui.label("Collection");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut panel.state.knowledge_import_collection)
                        .desired_width(180.0)
                        .hint_text("knowledge"),
                );
                if response.changed() {
                    panel.state.knowledge_import_collection =
                        panel.state.knowledge_import_collection.trim().to_string();
                }

                let current_directory = panel
                    .state
                    .trusted_workspaces
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if let Some(folder) = folder_picker_button(ui, "Choose Folder…", &current_directory)
                {
                    panel
                        .state
                        .do_knowledge_import(&panel.rt, ctx, Some(folder), Vec::new());
                }
                if let Some(files) = file_picker_button(ui, "Import Files…", &current_directory) {
                    panel
                        .state
                        .do_knowledge_import(&panel.rt, ctx, None, files);
                }
                if ui.button("Refresh Documents").clicked() {
                    panel
                        .state
                        .do_knowledge_documents_refresh(&panel.rt, ctx);
                }
            });

            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Recommended: import cleaned text sources, not PDF/image/audio originals. Folder imports recurse automatically and preserve relative paths inside the chosen collection.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.label(
                RichText::new(
                    "Single-file limit: files larger than 20MB will be skipped during import.",
                )
                .small()
                .color(palette::WARNING),
            );

            if let Some(error) = &panel.state.knowledge_import_error {
                ui.add_space(8.0);
                ui.label(RichText::new(error).small().color(palette::DANGER));
            }
            if let Some(error) = &panel.state.knowledge_documents_error {
                ui.add_space(8.0);
                ui.label(RichText::new(error).small().color(palette::DANGER));
            }

            if let Some(report) = &panel.state.last_knowledge_import_report {
                ui.add_space(12.0);
                ui.label(RichText::new("Last Import Report").strong());
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Collection: {}", report.collection))
                        .small()
                        .color(palette::text_dim(night)),
                );
                ui.label(
                    RichText::new(format!(
                        "Imported: {} | Unchanged: {} | Unsupported: {} | Too Large: {} | Missing: {} | Failed: {}",
                        report.imported_count,
                        report.skipped_unchanged_count,
                        report.skipped_unsupported_count,
                        report.skipped_too_large_count,
                        report.skipped_missing_count,
                        report.failed_count
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
                if !report.imported_paths.is_empty() {
                    let preview = report
                        .imported_paths
                        .iter()
                        .take(6)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.label(
                        RichText::new(format!("Imported Paths: {}", preview))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                }
                if !report.warnings.is_empty() {
                    for warning in report.warnings.iter().take(4) {
                        ui.label(RichText::new(warning).small().color(palette::WARNING));
                    }
                }
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Knowledge Documents").strong());
                if panel.state.knowledge_documents_loading {
                    ui.spinner();
                }
            });
            ui.label(
                RichText::new(
                    "Panel delete is a physical delete from the local knowledge index. Natural-language delete requests still require confirmation.",
                )
                .small()
                .color(palette::WARNING),
            );

            let documents = panel.state.knowledge_documents.clone();
            if documents.is_empty() {
                ui.label(
                    RichText::new("No documents loaded. Click Refresh Documents.")
                        .small()
                        .color(palette::text_dim(night)),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for doc in documents.iter().take(80) {
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&doc.title).strong());
                                    ui.label(
                                        RichText::new(format!(
                                            "{}/{}",
                                            doc.collection, doc.path
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                    if let Some(source_url) = &doc.source_url {
                                        ui.label(
                                            RichText::new(source_url)
                                                .small()
                                                .color(palette::text_dim(night)),
                                        );
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(RichText::new("Physical Delete").color(
                                                palette::DANGER,
                                            ))
                                            .clicked()
                                        {
                                            panel.state.do_knowledge_document_delete(
                                                &panel.rt,
                                                ctx,
                                                doc.collection.clone(),
                                                doc.path.clone(),
                                            );
                                        }
                                    },
                                );
                            });
                        }
                    });
            }
        });
}

fn llama_mode_combo(ui: &mut egui::Ui, id: &str, current: &mut String, options: &[(&str, &str)]) {
    let selected = options
        .iter()
        .find(|(value, _)| *value == current.as_str())
        .map(|(_, label)| *label)
        .unwrap_or("Auto");

    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (value, label) in options {
                if ui
                    .selectable_label(current.as_str() == *value, *label)
                    .clicked()
                {
                    *current = (*value).to_string();
                    ui.close_menu();
                }
            }
        });
}

fn render_llama_cpp_runtime_controls(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
) {
    let night = panel.state.night_mode;

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Llama.cpp Runtime")
                    .strong()
                    .color(palette::ACCENT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Large local GGUF runtime knobs mapped to official llama-server flags. Screenshot-style settings without a real upstream flag are intentionally left out instead of becoming fake switches.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            egui::CollapsingHeader::new("Auto Tune")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Default: BenShu estimates model size, VRAM/RAM budget, KV cache, and applies a safe runtime plan on save. Manual mode keeps your advanced overrides.",
                        )
                        .small()
                        .color(palette::text_dim(night)),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Tuning mode");
                        llama_mode_combo(
                            ui,
                            "llama_tuning_mode",
                            &mut panel.state.llama_tuning_mode,
                            &[("auto", "Auto"), ("manual", "Manual")],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Performance profile");
                        llama_mode_combo(
                            ui,
                            "llama_performance_profile",
                            &mut panel.state.llama_performance_profile,
                            &[
                                ("balanced", "Balanced"),
                                ("low_vram", "Low VRAM"),
                                ("speed", "Speed"),
                            ],
                        );
                    });
                    if !panel.state.llama_runtime_diagnostics.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(panel.state.llama_runtime_diagnostics.clone())
                                .small()
                                .color(palette::text_dim(night)),
                        );
                    }
                });

            egui::CollapsingHeader::new("Context And Offload")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Context length");
                        egui::ComboBox::from_id_salt("llama_ctx_size_preset")
                            .selected_text(llama_ctx_size_label(panel.state.llama_ctx_size))
                            .show_ui(ui, |ui| {
                                for (value, label) in LLAMA_CTX_SIZE_PRESETS {
                                    ui.selectable_value(
                                        &mut panel.state.llama_ctx_size,
                                        *value,
                                        format!("{label} ({value})"),
                                    );
                                }
                            });
                    });
                    ui.label(
                        RichText::new(llama_ctx_capacity_reference(
                            panel.state.llama_ctx_size,
                            panel.state.llama_gpu_layers,
                            &panel.state.llama_performance_profile,
                            panel.state.llama_kv_offload,
                        ))
                        .small()
                        .color(palette::text_dim(night)),
                    );
                    ui.horizontal(|ui| {
                        ui.label("GPU offload layers");
                        ui.add(egui::Slider::new(&mut panel.state.llama_gpu_layers, 0..=120));
                        ui.label(panel.state.llama_gpu_layers.to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Max concurrent slots");
                        ui.add(egui::Slider::new(
                            &mut panel.state.llama_parallel_slots,
                            1..=16,
                        ));
                        ui.label(panel.state.llama_parallel_slots.to_string());
                    });
                });

            egui::CollapsingHeader::new("Advanced")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("CPU threads");
                        ui.add(egui::Slider::new(&mut panel.state.llama_threads, 1..=64));
                        ui.label(panel.state.llama_threads.to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Threads batch");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_threads_batch)
                                .hint_text("blank = same as threads")
                                .desired_width(180.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Eval batch size");
                        ui.add(
                            egui::Slider::new(&mut panel.state.llama_batch_size, 32..=4096)
                                .logarithmic(true),
                        );
                        ui.label(panel.state.llama_batch_size.to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Physical ubatch size");
                        ui.add(
                            egui::Slider::new(&mut panel.state.llama_ubatch_size, 32..=2048)
                                .logarithmic(true),
                        );
                        ui.label(panel.state.llama_ubatch_size.to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Seed");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_seed)
                                .hint_text("blank = random")
                                .desired_width(180.0),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut panel.state.llama_cache_prompt, "Cache prompt");
                        ui.checkbox(&mut panel.state.llama_cont_batching, "Continuous batching");
                        ui.checkbox(&mut panel.state.llama_warmup, "Warmup");
                        ui.checkbox(&mut panel.state.llama_context_shift, "Context shift");
                        ui.checkbox(&mut panel.state.llama_jinja, "Jinja template");
                    });
                });

            egui::CollapsingHeader::new("Thinking And Template")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Reasoning mode");
                        llama_mode_combo(
                            ui,
                            "llama_reasoning_mode",
                            &mut panel.state.llama_reasoning_mode,
                            &[("auto", "Auto"), ("on", "On"), ("off", "Off")],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Reasoning format");
                        llama_mode_combo(
                            ui,
                            "llama_reasoning_format",
                            &mut panel.state.llama_reasoning_format,
                            &[
                                ("auto", "Auto"),
                                ("none", "None"),
                                ("deepseek", "DeepSeek"),
                                ("deepseek-legacy", "DeepSeek Legacy"),
                            ],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Reasoning budget");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_reasoning_budget)
                                .hint_text("blank = model default, 0 = end thinking immediately")
                                .desired_width(220.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Reasoning budget message");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_reasoning_budget_message,
                            )
                            .hint_text("optional message injected when budget is exhausted")
                            .desired_width(320.0),
                        );
                    });
                });

            egui::CollapsingHeader::new("Sampling")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "These are llama.cpp server-side sampling defaults. They are separate from per-agent temperature.",
                        )
                        .small()
                        .color(palette::text_dim(night)),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Temperature");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_temperature,
                            )
                            .hint_text("0.8")
                            .desired_width(120.0),
                        );
                        ui.label("Top-k");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_sampling_top_k)
                                .hint_text("40")
                                .desired_width(120.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Top-p");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_sampling_top_p)
                                .hint_text("0.95")
                                .desired_width(120.0),
                        );
                        ui.label("Min-p");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_sampling_min_p)
                                .hint_text("0.05")
                                .desired_width(120.0),
                        );
                        ui.label("Typical-p");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_typical_p,
                            )
                            .hint_text("1.0")
                            .desired_width(120.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Repeat penalty");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_repeat_penalty,
                            )
                            .hint_text("1.0")
                            .desired_width(120.0),
                        );
                        ui.label("Presence penalty");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_presence_penalty,
                            )
                            .hint_text("0.0")
                            .desired_width(120.0),
                        );
                        ui.label("Frequency penalty");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_frequency_penalty,
                            )
                            .hint_text("0.0")
                            .desired_width(120.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Mirostat");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_sampling_mirostat)
                                .hint_text("0")
                                .desired_width(120.0),
                        );
                        ui.label("Mirostat eta");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_mirostat_eta,
                            )
                            .hint_text("0.1")
                            .desired_width(120.0),
                        );
                        ui.label("Mirostat tau");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_sampling_mirostat_tau,
                            )
                            .hint_text("5.0")
                            .desired_width(120.0),
                        );
                    });
                });

            egui::CollapsingHeader::new("KV, RoPE And Memory")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut panel.state.llama_kv_offload, "KV offload");
                        ui.checkbox(&mut panel.state.llama_mmap, "mmap");
                        ui.checkbox(&mut panel.state.llama_mlock, "mlock");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Flash Attention");
                        llama_mode_combo(
                            ui,
                            "llama_flash_attn_mode",
                            &mut panel.state.llama_flash_attn_mode,
                            &[("auto", "Auto"), ("on", "On"), ("off", "Off")],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Prompt cache RAM MiB");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_cache_ram)
                                .hint_text("256")
                                .desired_width(120.0),
                        );
                        ui.label("Context checkpoints");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut panel.state.llama_ctx_checkpoints,
                            )
                            .hint_text("0")
                            .desired_width(120.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("RoPE scaling");
                        llama_mode_combo(
                            ui,
                            "llama_rope_scaling",
                            &mut panel.state.llama_rope_scaling,
                            &[
                                ("", "Auto"),
                                ("none", "None"),
                                ("linear", "Linear"),
                                ("yarn", "YaRN"),
                            ],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("RoPE scale");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_rope_scale)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                        ui.label("RoPE freq base");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_rope_freq_base)
                                .hint_text("blank = model default")
                                .desired_width(150.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("RoPE freq scale");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_rope_freq_scale)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("YaRN orig ctx");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_yarn_orig_ctx)
                                .hint_text("blank = model training ctx")
                                .desired_width(150.0),
                        );
                        ui.label("YaRN ext factor");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_yarn_ext_factor)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("YaRN attn factor");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_yarn_attn_factor)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                        ui.label("YaRN beta slow");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_yarn_beta_slow)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("YaRN beta fast");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_yarn_beta_fast)
                                .hint_text("blank = auto")
                                .desired_width(150.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Cache type K");
                        llama_mode_combo(
                            ui,
                            "llama_cache_type_k",
                            &mut panel.state.llama_cache_type_k,
                            &[
                                ("", "Auto"),
                                ("f16", "f16"),
                                ("bf16", "bf16"),
                                ("q8_0", "q8_0"),
                                ("q4_0", "q4_0"),
                                ("q4_1", "q4_1"),
                            ],
                        );
                        ui.label("Cache type V");
                        llama_mode_combo(
                            ui,
                            "llama_cache_type_v",
                            &mut panel.state.llama_cache_type_v,
                            &[
                                ("", "Auto"),
                                ("f16", "f16"),
                                ("bf16", "bf16"),
                                ("q8_0", "q8_0"),
                                ("q4_0", "q4_0"),
                                ("q4_1", "q4_1"),
                            ],
                        );
                    });
                });

            egui::CollapsingHeader::new("Multi-GPU And MoE")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Device");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_device)
                                .hint_text("blank = auto / default")
                                .desired_width(220.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Split mode");
                        llama_mode_combo(
                            ui,
                            "llama_split_mode",
                            &mut panel.state.llama_split_mode,
                            &[
                                ("", "Auto"),
                                ("none", "None"),
                                ("layer", "Layer"),
                                ("row", "Row"),
                            ],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Tensor split");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_tensor_split)
                                .hint_text("e.g. 3,1")
                                .desired_width(180.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Main GPU");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_main_gpu)
                                .hint_text("blank = default")
                                .desired_width(140.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Fit");
                        llama_mode_combo(
                            ui,
                            "llama_fit_mode",
                            &mut panel.state.llama_fit_mode,
                            &[("on", "On"), ("off", "Off")],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Fit target (MiB list)");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_fit_target)
                                .hint_text("e.g. 1024 or 1024,2048")
                                .desired_width(220.0),
                        );
                        ui.label("Fit ctx");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_fit_ctx)
                                .hint_text("blank = default")
                                .desired_width(140.0),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut panel.state.llama_cpu_moe, "CPU MoE");
                    });
                    ui.horizontal(|ui| {
                        ui.label("CPU MoE first N layers");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_n_cpu_moe)
                                .hint_text("blank = disabled")
                                .desired_width(160.0),
                        );
                    });
                });

            egui::CollapsingHeader::new("Multimodal And Vision Runtime")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(
                            &mut panel.state.llama_mmproj_offload,
                            "mmproj offload",
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Image min tokens");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_image_min_tokens)
                                .hint_text("blank = model default")
                                .desired_width(160.0),
                        );
                        ui.label("Image max tokens");
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.llama_image_max_tokens)
                                .hint_text("blank = model default")
                                .desired_width(160.0),
                        );
                    });
                });

            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Still intentionally not exposed: low-level CPU affinity / NUMA / override-tensor style knobs that are better kept out of a product-facing panel until we have an advanced diagnostics surface.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Apply Llama.cpp Runtime").strong())
                    .clicked()
                {
                    panel
                        .state
                        .do_save_llama_cpp_runtime_settings(&panel.rt, ctx);
                }
                if ui.button("Reset To Defaults").clicked() {
                    panel.state.llama_ctx_size = 8192;
                    panel.state.llama_gpu_layers = 24;
                    panel.state.llama_threads = crate::app_state::default_llama_runtime_threads();
                    panel.state.llama_threads_batch.clear();
                    panel.state.llama_batch_size = 2048;
                    panel.state.llama_ubatch_size = 512;
                    panel.state.llama_parallel_slots = 1;
                    panel.state.llama_cache_ram = "256".to_string();
                    panel.state.llama_ctx_checkpoints = "0".to_string();
                    panel.state.llama_flash_attn_mode = "auto".to_string();
                    panel.state.llama_kv_offload = true;
                    panel.state.llama_mmap = true;
                    panel.state.llama_mlock = false;
                    panel.state.llama_cache_prompt = false;
                    panel.state.llama_cont_batching = false;
                    panel.state.llama_warmup = true;
                    panel.state.llama_context_shift = false;
                    panel.state.llama_jinja = true;
                    panel.state.llama_rope_scaling.clear();
                    panel.state.llama_rope_scale.clear();
                    panel.state.llama_rope_freq_base.clear();
                    panel.state.llama_rope_freq_scale.clear();
                    panel.state.llama_yarn_orig_ctx.clear();
                    panel.state.llama_yarn_ext_factor.clear();
                    panel.state.llama_yarn_attn_factor.clear();
                    panel.state.llama_yarn_beta_slow.clear();
                    panel.state.llama_yarn_beta_fast.clear();
                    panel.state.llama_cache_type_k.clear();
                    panel.state.llama_cache_type_v.clear();
                    panel.state.llama_device.clear();
                    panel.state.llama_split_mode.clear();
                    panel.state.llama_tensor_split.clear();
                    panel.state.llama_main_gpu.clear();
                    panel.state.llama_fit_mode = "on".to_string();
                    panel.state.llama_fit_target.clear();
                    panel.state.llama_fit_ctx.clear();
                    panel.state.llama_cpu_moe = false;
                    panel.state.llama_n_cpu_moe.clear();
                    panel.state.llama_mmproj_offload = true;
                    panel.state.llama_image_min_tokens.clear();
                    panel.state.llama_image_max_tokens.clear();
                    panel.state.llama_reasoning_mode = "auto".to_string();
                    panel.state.llama_reasoning_format = "auto".to_string();
                    panel.state.llama_reasoning_budget.clear();
                    panel.state.llama_reasoning_budget_message.clear();
                    panel.state.llama_sampling_temperature = "0.8".to_string();
                    panel.state.llama_sampling_top_k = "40".to_string();
                    panel.state.llama_sampling_top_p = "0.95".to_string();
                    panel.state.llama_sampling_min_p = "0.05".to_string();
                    panel.state.llama_sampling_typical_p = "1.0".to_string();
                    panel.state.llama_sampling_repeat_penalty = "1.0".to_string();
                    panel.state.llama_sampling_presence_penalty = "0.0".to_string();
                    panel.state.llama_sampling_frequency_penalty = "0.0".to_string();
                    panel.state.llama_sampling_mirostat = "0".to_string();
                    panel.state.llama_sampling_mirostat_eta = "0.1".to_string();
                    panel.state.llama_sampling_mirostat_tau = "5.0".to_string();
                    panel.state.llama_seed.clear();
                }
            });
        });
}

fn render_local_role_bindings_content(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
) {
    let night = panel.state.night_mode;

    egui::ScrollArea::vertical()
        .id_salt("local_role_bindings_scroll")
        .max_height(520.0)
        .show(ui, |ui| {
                    let (c_embed, a_embed) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "📊",
                        "Embedding",
                        "Semantic indexing and memory-vector role binding.",
                        &mut panel.state.organ_embed_model,
                    );
                    if c_embed || a_embed {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut embed_model = panel.state.organ_embed_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "embedding",
                        "Embedding",
                        &mut embed_model,
                        "Choose a discovered embedding package or point at a local model folder. Diagnostics will explain whether it already matches the Windows-native ML contract.",
                    );
                    if embed_model != panel.state.organ_embed_model {
                        panel.state.organ_embed_model = embed_model;
                    }
                    ui.add_space(8.0);

                    let (c_rerank, a_rerank) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🎯",
                        "Rerank",
                        "Context ranking role for retrieval and consolidation.",
                        &mut panel.state.organ_rerank_model,
                    );
                    if c_rerank || a_rerank {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut rerank_model = panel.state.organ_rerank_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "rerank",
                        "Rerank",
                        &mut rerank_model,
                        "Choose a discovered reranker package or point at a local model folder. Diagnostics will explain whether it already matches the Windows-native ML contract.",
                    );
                    if rerank_model != panel.state.organ_rerank_model {
                        panel.state.organ_rerank_model = rerank_model;
                    }
                    ui.add_space(8.0);

                    let (c_ocr, a_ocr) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "📄",
                        "OCR",
                        "Document OCR runtime role for page, image, and frame extraction.",
                        &mut panel.state.organ_ocr_model,
                    );
                    if c_ocr || a_ocr {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut ocr_model = panel.state.organ_ocr_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "ocr",
                        "OCR",
                        &mut ocr_model,
                        "Choose a discovered OCR-capable package or select a folder manually. Diagnostics will tell you whether this remains a specialized OCR runtime or is still pending adaptation.",
                    );
                    if ocr_model != panel.state.organ_ocr_model {
                        panel.state.organ_ocr_model = ocr_model;
                    }
                    ui.add_space(8.0);

                    let (c_vision, a_vision) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "👁",
                        "Vision / Local Multimodal",
                        "Global local perception role for image understanding, visual attachment analysis, and reusable local multimodal support outside per-agent large-model loadout.",
                        &mut panel.state.organ_vision_model,
                    );
                    if c_vision || a_vision {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut vision_model = panel.state.organ_vision_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "vision",
                        "Vision",
                        &mut vision_model,
                        "Choose a discovered vision / multimodal package or select a folder manually. Diagnostics will show whether it is ready for provider-backed or bridge-backed multimodal routing.",
                    );
                    if vision_model != panel.state.organ_vision_model {
                        panel.state.organ_vision_model = vision_model;
                    }
                    ui.add_space(8.0);

                    let (c_slm, a_slm) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🔄",
                        "SLM Tactical (Global)",
                        "Global small tactical model used for local pre-pass and strategy shaping across agents.",
                        &mut panel.state.tactical_model,
                    );
                    if c_slm || a_slm {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut tactical_model = panel.state.tactical_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "slm_tactical",
                        "SLM Tactical",
                        &mut tactical_model,
                        "Choose a discovered tactical model or select a folder manually. Diagnostics will show whether it is already aligned with the Windows-native ML lane or still on a migration track.",
                    );
                    if tactical_model != panel.state.tactical_model {
                        panel.state.tactical_model = tactical_model;
                    }
                    ui.add_space(8.0);

                    let (c_image_gen, a_image_gen) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🎨",
                        "Image Generation (Global)",
                        "Global image generation backend used for text-to-image and future image editing flows.",
                        &mut panel.state.image_gen_model,
                    );
                    if c_image_gen || a_image_gen {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut image_gen_model = panel.state.image_gen_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "image_generation",
                        "Image Generation",
                        &mut image_gen_model,
                        "Choose a discovered image model package or pick a folder manually. The diagnostics panel will tell you whether it is bridge-ready / ONNX / diffusers, and the Windows ML bridge config will auto-link the effective runtime target.",
                    );
                    if image_gen_model != panel.state.image_gen_model {
                        panel.state.image_gen_model = image_gen_model;
                    }
                    ui.add_space(8.0);

                    let (c_image_edit, a_image_edit) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🪄",
                        "Image Edit (Global)",
                        "Global image editing backend used for inpaint, img2img, and future multimodal edit flows.",
                        &mut panel.state.organ_image_edit_model,
                    );
                    if c_image_edit || a_image_edit {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut image_edit_model = panel.state.organ_image_edit_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "image_edit",
                        "Image Edit",
                        &mut image_edit_model,
                        "Choose a discovered image edit package or pick a folder manually. Diagnostics will show whether it already matches the Windows ML / ONNX Runtime lane or still needs a specialized edit runtime.",
                    );
                    if image_edit_model != panel.state.organ_image_edit_model {
                        panel.state.organ_image_edit_model = image_edit_model;
                    }
                    ui.add_space(8.0);

                    let (c_fact, a_fact) = crate::ui::system::render_fact_check_card_ui(
                        ui,
                        night,
                        "⚖️",
                        "Fact Check",
                        "Optional verification role for risky generations and grounding checks.",
                        &mut panel.state.organ_fact_check_model,
                        &mut panel.state.fact_check_enabled,
                    );
                    if c_fact || a_fact {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut fact_model = panel.state.organ_fact_check_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "fact_check",
                        "Fact Check",
                        &mut fact_model,
                        "Choose a discovered validation model or select a folder manually. Diagnostics will show whether it already matches the Windows-native ML contract or still needs migration.",
                    );
                    if fact_model != panel.state.organ_fact_check_model {
                        panel.state.organ_fact_check_model = fact_model;
                    }
                    ui.add_space(8.0);

                    let (c_stt, a_stt) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "👂",
                        "Speech-to-Text",
                        "Shared local voice input role.",
                        &mut panel.state.organ_stt_model,
                    );
                    if c_stt || a_stt {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut stt_model = panel.state.organ_stt_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "speech_to_text",
                        "Speech-to-Text",
                        &mut stt_model,
                        "Choose a discovered STT package or select a folder manually. Diagnostics will show whether it stays specialized today or is ready for a Windows-native ML lane later.",
                    );
                    if stt_model != panel.state.organ_stt_model {
                        panel.state.organ_stt_model = stt_model;
                    }
                    ui.add_space(8.0);

                    let (c_audio_understanding, a_audio_understanding) =
                        crate::ui::system::render_organ_card_ui(
                            ui,
                            night,
                            "🎧",
                            "Audio Understanding",
                            "Shared local audio comprehension role for non-transcription understanding and future audio-native assistance flows.",
                            &mut panel.state.organ_audio_understanding_model,
                        );
                    if c_audio_understanding || a_audio_understanding {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut audio_understanding_model =
                        panel.state.organ_audio_understanding_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "audio_understanding",
                        "Audio Understanding",
                        &mut audio_understanding_model,
                        "Choose a discovered local audio-understanding package or select a folder manually. Diagnostics will show whether it already aligns with the Windows ML / ONNX Runtime lane.",
                    );
                    if audio_understanding_model != panel.state.organ_audio_understanding_model {
                        panel.state.organ_audio_understanding_model = audio_understanding_model;
                    }
                    ui.add_space(8.0);

                    let (c_realtime_vad, a_realtime_vad) =
                        crate::ui::system::render_organ_card_ui(
                            ui,
                            night,
                            "🎙",
                            "Realtime VAD",
                            "Shared voice-activity detection role for low-latency speech segmentation and duplex pipelines.",
                            &mut panel.state.organ_realtime_vad_model,
                        );
                    if c_realtime_vad || a_realtime_vad {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut realtime_vad_model = panel.state.organ_realtime_vad_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "realtime_vad",
                        "Realtime VAD",
                        &mut realtime_vad_model,
                        "Choose a discovered VAD package or select a folder manually. Diagnostics will show whether it already fits the Windows ML / ONNX Runtime lane.",
                    );
                    if realtime_vad_model != panel.state.organ_realtime_vad_model {
                        panel.state.organ_realtime_vad_model = realtime_vad_model;
                    }
                    ui.add_space(8.0);

                    let (c_tts, a_tts) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🗣",
                        "Text-to-Speech",
                        "Shared local voice output role.",
                        &mut panel.state.organ_tts_model,
                    );
                    if c_tts || a_tts {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut tts_model = panel.state.organ_tts_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "text_to_speech",
                        "Text-to-Speech",
                        &mut tts_model,
                        "Choose a discovered TTS package or select a folder manually. Diagnostics will show whether it remains on a specialized runtime or is pending a Windows-native ML path.",
                    );
                    if tts_model != panel.state.organ_tts_model {
                        panel.state.organ_tts_model = tts_model;
                    }
                    ui.add_space(8.0);

                    let (c_duplex_voice, a_duplex_voice) =
                        crate::ui::system::render_organ_card_ui(
                            ui,
                            night,
                            "🔁",
                            "Duplex Voice",
                            "Shared realtime duplex voice role for low-latency speech-in / speech-out orchestration.",
                            &mut panel.state.organ_duplex_voice_model,
                        );
                    if c_duplex_voice || a_duplex_voice {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut duplex_voice_model = panel.state.organ_duplex_voice_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "duplex_voice",
                        "Duplex Voice",
                        &mut duplex_voice_model,
                        "Choose a discovered duplex / realtime voice package or select a folder manually. Diagnostics will show whether it already fits the Windows ML / ONNX Runtime lane or still needs a specialized realtime voice runtime.",
                    );
                    if duplex_voice_model != panel.state.organ_duplex_voice_model {
                        panel.state.organ_duplex_voice_model = duplex_voice_model;
                    }
                    ui.add_space(8.0);

                    let (c_classifier, a_classifier) =
                        crate::ui::system::render_organ_card_ui(
                            ui,
                            night,
                            "🏷",
                            "Local Classifier",
                            "Shared classifier role for lightweight local intent, label, and moderation-style decisions.",
                            &mut panel.state.organ_local_classifier_model,
                        );
                    if c_classifier || a_classifier {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut classifier_model = panel.state.organ_local_classifier_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "local_classifier",
                        "Local Classifier",
                        &mut classifier_model,
                        "Choose a discovered classifier package or select a folder manually. Diagnostics will show whether it already fits the Windows ML / ONNX Runtime lane.",
                    );
                    if classifier_model != panel.state.organ_local_classifier_model {
                        panel.state.organ_local_classifier_model = classifier_model;
                    }
                    ui.add_space(8.0);

                    let (c_router, a_router) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🧭",
                        "Local Router",
                        "Shared router role for lightweight local routing, tool-surface decisions, and backend selection hints.",
                        &mut panel.state.organ_local_router_model,
                    );
                    if c_router || a_router {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut router_model = panel.state.organ_local_router_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "local_router",
                        "Local Router",
                        &mut router_model,
                        "Choose a discovered router package or select a folder manually. Diagnostics will show whether it already fits the Windows ML / ONNX Runtime lane.",
                    );
                    if router_model != panel.state.organ_local_router_model {
                        panel.state.organ_local_router_model = router_model;
                    }
                    ui.add_space(8.0);

                    let (c_safety, a_safety) = crate::ui::system::render_organ_card_ui(
                        ui,
                        night,
                        "🛡",
                        "Local Safety Checker",
                        "Shared safety / moderation role for lightweight local filtering, guard, and policy checks.",
                        &mut panel.state.organ_local_safety_model,
                    );
                    if c_safety || a_safety {
                        persist_global_model_binding(panel, ctx);
                    }
                    let mut safety_model = panel.state.organ_local_safety_model.clone();
                    render_global_binding_picker_row(
                        panel,
                        ui,
                        ctx,
                        "local_safety",
                        "Local Safety",
                        &mut safety_model,
                        "Choose a discovered safety / guard package or select a folder manually. Diagnostics will show whether it already fits the Windows ML / ONNX Runtime lane.",
                    );
                    if safety_model != panel.state.organ_local_safety_model {
                        panel.state.organ_local_safety_model = safety_model;
                    }
        });
}

fn render_local_media_contracts(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    let night = panel.state.night_mode;

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Media Contracts")
                    .strong()
                    .color(palette::ACCENT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Traceable source and follow-up contracts exposed by the current local media runtime.",
                )
                .small()
                .color(palette::text_dim(night)),
            );
            ui.add_space(12.0);

            if let Some(stack) = &panel.state.local_model_stack {
                ui.label(RichText::new("Source Contracts").strong());
                ui.horizontal_wrapped(|ui| {
                    for contract in &stack.media_runtime.source_contracts {
                        render_contract_chip(ui, contract, night, palette::ACCENT);
                    }
                });
                ui.add_space(10.0);
                ui.label(RichText::new("Follow-up Contracts").strong());
                ui.horizontal_wrapped(|ui| {
                    for contract in &stack.media_runtime.followup_contracts {
                        render_contract_chip(ui, contract, night, palette::WARNING);
                    }
                });
            } else {
                ui.label(
                    RichText::new(
                        "Media contract evidence appears here after the local model stack summary loads.",
                    )
                    .small()
                    .color(palette::text_dim(night)),
                );
            }
        });
}

fn render_contract_chip(ui: &mut egui::Ui, label: &str, night: bool, accent: Color32) {
    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, accent))
        .corner_radius(egui::CornerRadius::same(24))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(label.replace('_', " ")).small().color(accent));
        });
}

fn render_api_keys(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width()); // Force expansion at the tab root level
        ui.add_space(8.0);

        let n = panel.state.vault_entries.len();
        let mut delete_idx: Option<usize> = None;
        let bg_color = panel.theme_bg_deep();

        for i in 0..n {
            let key_name = &panel.state.vault_entries[i].key;
            let is_channel_key = panel.state.channel_metadata.iter().any(|meta| {
                meta.fields
                    .iter()
                    .any(|f| f.key.to_uppercase() == *key_name)
            });

            if is_channel_key {
                continue; // Skip channel API keys, they belong in the Communication tab
            }

            let entry = &mut panel.state.vault_entries[i];

            egui::Frame::new()
                .fill(bg_color)
                .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 12))
                .outer_margin(egui::Margin::symmetric(0, 0)) // Remove side margin to fill right
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&entry.key)
                                .font(FontId::new(13.0, egui::FontFamily::Monospace))
                                .color(palette::ACCENT),
                        );

                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // 1. Right-side Buttons
                                    let standard = [
                                        "OPENAI_API_KEY",
                                        "ANTHROPIC_API_KEY",
                                        "GEMINI_API_KEY",
                                        "DEEPSEEK_API_KEY",
                                        "MINIMAX_API_KEY",
                                        "MOONSHOT_API_KEY",
                                        "ZHIPU_API_KEY",
                                        "QWEN_API_KEY",
                                        "DOUBAO_API_KEY",
                                    ];
                                    if !standard.contains(&entry.key.as_str()) {
                                        if ui
                                            .button(RichText::new("🗑").color(palette::DANGER))
                                            .clicked()
                                        {
                                            let client = panel.state.client.clone();
                                            let key = entry.key.clone();
                                            let ctx2 = ctx.clone();
                                            let rt = panel.rt.clone();
                                            crate::common::task::spawn_task(&rt, async move {
                                                let _ = client.delete_vault_secret(&key).await;
                                                ctx2.request_repaint();
                                            });

                                            panel
                                                .state
                                                .deleted_vault_keys
                                                .insert(entry.key.clone());
                                            delete_idx = Some(i);
                                        }
                                    }

                                    if ui.small_button("💾 Save").clicked()
                                        && !entry.value.is_empty()
                                    {
                                        let client = panel.state.client.clone();
                                        let key = entry.key.clone();
                                        let val = entry.value.clone();
                                        entry.error = None;
                                        let ctx2 = ctx.clone();
                                        let rt = panel.rt.clone();

                                        crate::common::task::spawn_task(&rt, async move {
                                            if let Ok(_) =
                                                client.save_vault_secret(&key, &val).await
                                            {
                                                ctx2.request_repaint();
                                            }
                                        });

                                        entry.value.clear();
                                        entry.saved = true;
                                    }

                                    // 2. Input takes ALL REMAINING space to the left
                                    let pw = egui::TextEdit::singleline(&mut entry.value)
                                        .password(!panel.state.vault_show_value)
                                        .hint_text("Enter key value…")
                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                        .desired_width(ui.available_width());
                                    ui.add(pw);
                                },
                            );
                        });
                    });
                });
        }

        // 执行延迟删除
        if let Some(idx) = delete_idx {
            panel.state.vault_entries.remove(idx);
        }

        // If any save/delete was clicked (indicated by 0.0), trigger refresh

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let show_lbl = if panel.state.vault_show_value {
                "Hide values"
            } else {
                "Show values"
            };
            if ui.small_button(show_lbl).clicked() {
                panel.state.vault_show_value = !panel.state.vault_show_value;
            }
        });

        ui.add_space(16.0);
        ui.label(
            RichText::new("Add Custom Key")
                .color(palette::text_dim(panel.state.night_mode))
                .small(),
        );
        ui.horizontal(|ui| {
            let avg_width = (ui.available_width() - 80.0) / 2.0;
            ui.add(
                egui::TextEdit::singleline(&mut panel.state.new_vault_key)
                    .hint_text("KEY_NAME")
                    .font(FontId::new(12.0, egui::FontFamily::Monospace))
                    .desired_width(avg_width.max(120.0)),
            );
            ui.add(
                egui::TextEdit::singleline(&mut panel.state.new_vault_value)
                    .hint_text("value")
                    .password(true)
                    .font(FontId::new(12.0, egui::FontFamily::Monospace))
                    .desired_width(avg_width.max(160.0)),
            );
            if ui.button("Add").clicked() && !panel.state.new_vault_key.is_empty() {
                let key_raw = panel.state.new_vault_key.drain(..).collect::<String>();
                let val = panel.state.new_vault_value.drain(..).collect::<String>();

                let mut key = key_raw.trim().to_uppercase();
                if !key.ends_with("_API_KEY") {
                    key = format!("{}_API_KEY", key);
                }

                panel.state.deleted_vault_keys.remove(&key);

                let idx = panel.state.vault_entries.len();
                panel.state.vault_entries.push(VaultEntry {
                    key: key.clone(),
                    value: String::new(),
                    saved: false,
                    ..Default::default()
                });

                let client = panel.state.client.clone();
                let ctx2 = ctx.clone();
                let rt = panel.rt.clone();
                crate::common::task::spawn_task(&rt, async move {
                    let _ = client.save_vault_secret(&key, &val).await;
                    ctx2.request_repaint();
                });

                if let Some(e) = panel.state.vault_entries.get_mut(idx) {
                    e.saved = true;
                }
            }
        });

        ui.add_space(12.0);
    });
}

fn render_local_ocr_card(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                RichText::new("👁 Privacy-First Local OCR (WASM)")
                    .strong()
                    .color(palette::ACCENT),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Extract text from images locally using WASM-based Tesseract. Cloud fallback remains available when the local OCR path is disabled or unavailable.",
                )
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                let mut use_ocr = panel.state.use_local_ocr.unwrap_or(false);
                if ui.checkbox(&mut use_ocr, "Enable Local WASM OCR").changed() {
                    panel.state.use_local_ocr = Some(use_ocr);
                    crate::app_state::save_config(&panel.state);
                }
            });
        });
}

fn render_api_speech(panel: &mut ClawPanel, ui: &mut egui::Ui, _ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.vertical(|ui| {
        ui.add_space(8.0);

        // ── Section: OpenAI TTS ──────────────────────────────────────────
        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(t("speech.openai_tts", lang))
                        .strong()
                        .color(palette::ACCENT),
                );
                ui.add_space(12.0);

                // Model Selection
                ui.horizontal(|ui| {
                    ui.label(format!("{}:", t("speech.model", lang)));
                    let models = ["tts-1", "tts-1-hd"];
                    for m in models {
                        let is_sel = panel.state.voice_tts_model == m;
                        if ui.selectable_label(is_sel, m).clicked() {
                            panel.state.voice_tts_model = m.to_string();
                            panel.state.set_status("Speech settings updated", false);
                            crate::app_state::save_config(&panel.state);
                        }
                    }
                });
                ui.add_space(8.0);

                // Voice Selection
                ui.horizontal(|ui| {
                    ui.label(format!("{}:", t("speech.voice", lang)));
                    let voices = ["alloy", "echo", "fable", "onyx", "nova", "shimmer"];
                    ui.horizontal_wrapped(|ui| {
                        for v in voices {
                            let is_sel = panel.state.voice_tts_voice == v;
                            if ui.selectable_label(is_sel, v).clicked() {
                                panel.state.voice_tts_voice = v.to_string();
                                panel.state.set_status("Speech settings updated", false);
                                crate::app_state::save_config(&panel.state);
                            }
                        }
                    });
                });
            });
    });
}

fn render_api_creative(panel: &mut ClawPanel, ui: &mut egui::Ui, _ctx: &egui::Context) {
    let night = panel.state.night_mode;

    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.add_space(8.0);

        ui.heading(RichText::new("🎨 Creative Matrix").color(palette::ACCENT));
        ui.label(
            RichText::new("Configure image generation and multimodal creation backends.")
                .small()
                .color(palette::text_dim(night)),
        );
        ui.add_space(16.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Unified Image Generation")
                        .strong()
                        .color(palette::ACCENT),
                );
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label("Provider/Model:");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut panel.state.image_gen_model)
                            .hint_text("api:openai/gpt-image-1, local image model folder, or bridge-image:http://host:port/v1|model")
                            .desired_width(250.0),
                    );
                    if resp.changed() {
                        panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                    }

                    let current_image_gen_model = panel.state.image_gen_model.clone();
                    if let Some(new_model) =
                        folder_picker_button(ui, "Choose Folder…", &current_image_gen_model)
                    {
                        panel.state.image_gen_model = new_model;
                        panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                    }

                    egui::ComboBox::from_id_salt("image_gen_model_select")
                        .selected_text("Quick Switch...")
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut panel.state.image_gen_model,
                                    "api:openai/dall-e-3".into(),
                                    "Cloud: DALL-E 3 (OpenAI)",
                                )
                                .clicked()
                            {
                                panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                            }
                            if ui
                                .selectable_value(
                                    &mut panel.state.image_gen_model,
                                    "api:openai/gpt-image-1".into(),
                                    "Cloud: GPT-Image-1 (OpenAI)",
                                )
                                .clicked()
                            {
                                panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                            }
                            if ui
                                .selectable_value(
                                    &mut panel.state.image_gen_model,
                                    "api:openai/dall-e-2".into(),
                                    "Cloud: DALL-E 2 (OpenAI)",
                                )
                                .clicked()
                            {
                                panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                            }
                            if ui
                                .selectable_value(
                                    &mut panel.state.image_gen_model,
                                    "bridge-image:http://127.0.0.1:8022/v1|local-image-model".into(),
                                    "Windows Bridge: Local integrated image runtime",
                                )
                                .clicked()
                            {
                                panel.state.do_save_sensory_settings(&panel.rt, _ctx);
                            }
                            ui.separator();
                        });
                });

                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "Bridge syntax: bridge-image:http://host:port/v1|model-name. Preferred for Windows-first local image runtimes, where BenShu forwards requests into a dedicated image service and the concrete model package remains user-selectable.",
                    )
                    .small()
                    .color(palette::text_dim(night)),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Engine Status:").small());
                    let status_color = if panel.state.image_gen_status == "Ready" {
                        palette::SUCCESS
                    } else {
                        palette::WARNING
                    };
                    ui.label(
                        RichText::new(&panel.state.image_gen_status)
                            .small()
                            .color(status_color)
                            .strong(),
                    );
                });
            });

        ui.add_space(24.0);

        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("🔄 Sync Creative Config to Gateway").strong())
                .clicked()
            {
                let client = panel.state.client.clone();
                let model = panel.state.image_gen_model.clone();
                let ctx2 = _ctx.clone();
                let rt = panel.rt.clone();

                crate::common::task::spawn_task(&rt, async move {
                    let _ = client.save_vault_secret("IMAGE_GEN_MODEL", &model).await;
                    ctx2.request_repaint();
                });

                panel
                    .state
                    .set_status("Creative settings committed to Gateway Vault", false);
            }
            ui.label(
                RichText::new("💡 Tip: leave this empty until you choose a local image model package or a cloud image backend.")
                    .small()
                    .color(palette::text_dim(night)),
            );
        });
    });
}

fn render_local_workspaces(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;
    let lang = panel.state.language;
    let bg_deep = panel.theme_bg_deep();

    ui.vertical(|ui| {
        ui.add_space(8.0);

        // Description
        ui.label(RichText::new("Authorized Workspaces").strong().color(palette::ACCENT));
        ui.label(RichText::new("Agents are sandboxed by default. Folders listed here are authorized for file operations (read/write/list).").small().color(palette::text_dim(night)));
        ui.add_space(12.0);

        // Add new workspace
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.text_edit_singleline(&mut panel.state.workspace_form_path);
            if ui.button(t("btn.add", lang)).clicked() && !panel.state.workspace_form_path.is_empty() {
                let client = panel.state.client.clone();
                let path = panel.state.workspace_form_path.clone();
                let ctx2 = ctx.clone();
                let rt = panel.rt.clone();
                panel.state.workspace_loading = true;
                crate::common::task::spawn_task(&rt, async move {
                    let _ = client.add_workspace(&path).await;
                    let _ = client.list_workspaces().await; // Trigger refresh
                    ctx2.request_repaint();
                });
                panel.state.workspace_form_path.clear();
            }
        });
        ui.add_space(12.0);

        // List existing
        if panel.state.workspace_loading && panel.state.trusted_workspaces.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading workspaces...");
            });
        }

        let mut to_remove = None;
        egui::ScrollArea::vertical()
            .id_salt("workspaces_scroll")
            .max_height(300.0)
            .show(ui, |ui| {
            for path in &panel.state.trusted_workspaces {
                egui::Frame::new()
                    .fill(bg_deep)
                    .stroke(Stroke::new(1.0, palette::border(night)))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .outer_margin(egui::Margin::symmetric(0, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(path).font(FontId::monospace(12.0)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("🗑").color(palette::DANGER)).clicked() {
                                    to_remove = Some(path.clone());
                                }
                            });
                        });
                    });
            }
        });

        if let Some(path) = to_remove {
            let client = panel.state.client.clone();
            let ctx2 = ctx.clone();
            let rt = panel.rt.clone();
            panel.state.workspace_loading = true;
            let path_clone = path.clone();
            crate::common::task::spawn_task(&rt, async move {
                let _ = client.remove_workspace(&path_clone).await;
                ctx2.request_repaint();
            });
            panel.state.trusted_workspaces.retain(|p| p != &path);
        }
    });
}
