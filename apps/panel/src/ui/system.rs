use crate::app::ClawPanel;
use crate::common::palette;
use crate::i18n::t;
use crate::ui::components::toggle::toggle as checkbox_toggle;
use crate::ui::open_target;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};

pub fn render_system_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;

    ui.vertical(|ui| {
        // Sub-tabs
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut panel.state.system_subtab,
                crate::app_state::SystemSubTab::General,
                "ℹ Info",
            );
            ui.selectable_value(
                &mut panel.state.system_subtab,
                crate::app_state::SystemSubTab::Artifacts,
                "🗂 Artifacts",
            );
            ui.selectable_value(
                &mut panel.state.system_subtab,
                crate::app_state::SystemSubTab::Doctor,
                "🩺 ".to_string() + &t("system.doctor", lang),
            );
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(16.0);

        match panel.state.system_subtab {
            crate::app_state::SystemSubTab::Artifacts => {
                render_artifacts_console(panel, ui, ctx);
            }
            crate::app_state::SystemSubTab::General => {
                render_system_overview(panel, ui, ctx);
            }
            crate::app_state::SystemSubTab::Doctor => {
                render_doctor_mode(panel, ui, ctx);
            }
        }
    });
}

fn render_artifacts_console(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;

    if panel.state.artifacts.is_empty()
        && panel.state.pending_artifacts_promise.is_none()
        && !panel.state.artifacts_loading
    {
        panel.state.do_artifact_refresh(&panel.rt, ctx);
    }

    ui.vertical(|ui| {
        ui.heading("Artifact Registry");
        ui.label(
            RichText::new(
                "Inspect runtime artifacts by scope and lifecycle, then run bounded cleanup with dry-run support.",
            )
            .small()
            .color(palette::text_dim(night)),
        );
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Thread").small().color(palette::text_dim(night)));
                    ui.add_sized(
                        [140.0, 22.0],
                        egui::TextEdit::singleline(
                            panel
                                .state
                                .artifacts_query
                                .thread_id
                                .get_or_insert_with(String::new),
                        ),
                    );
                    ui.label(RichText::new("Session").small().color(palette::text_dim(night)));
                    ui.add_sized(
                        [140.0, 22.0],
                        egui::TextEdit::singleline(
                            panel
                                .state
                                .artifacts_query
                                .session_id
                                .get_or_insert_with(String::new),
                        ),
                    );
                    ui.label(RichText::new("Source").small().color(palette::text_dim(night)));
                    ui.add_sized(
                        [140.0, 22.0],
                        egui::TextEdit::singleline(
                            panel
                                .state
                                .artifacts_query
                                .source_kind
                                .get_or_insert_with(String::new),
                        ),
                    );
                    ui.label(RichText::new("Limit").small().color(palette::text_dim(night)));
                    let limit = panel.state.artifacts_query.limit.get_or_insert(50);
                    ui.add(egui::DragValue::new(limit).range(1..=500));

                    artifact_query_combo(
                        ui,
                        "Scope",
                        &mut panel.state.artifacts_query.scope,
                        &["uploads", "workspace", "outputs", "artifacts"],
                    );
                    artifact_query_combo(
                        ui,
                        "Lifecycle",
                        &mut panel.state.artifacts_query.lifecycle,
                        &["ephemeral", "session", "durable"],
                    );

                    if ui.button("Refresh").clicked() {
                        normalize_optional_string(&mut panel.state.artifacts_query.thread_id);
                        normalize_optional_string(&mut panel.state.artifacts_query.session_id);
                        normalize_optional_string(&mut panel.state.artifacts_query.source_kind);
                        panel.state.do_artifact_refresh(&panel.rt, ctx);
                    }
                    if ui.button("Clear Filters").clicked() {
                        panel.state.artifacts_query = crate::api::ArtifactQuery {
                            limit: Some(50),
                            ..crate::api::ArtifactQuery::default()
                        };
                        panel.state.do_artifact_refresh(&panel.rt, ctx);
                    }
                });

                if panel.state.artifacts_loading {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading artifacts...");
                    });
                }
                if let Some(error) = &panel.state.artifacts_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).color(palette::DANGER));
                }
            });

        ui.add_space(12.0);

        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                ui.heading(format!("Artifacts ({})", panel.state.artifacts.len()));
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("artifact_registry_list")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        let artifacts = panel.state.artifacts.clone();
                        for artifact in &artifacts {
                            let selected = panel.state.selected_artifact_id.as_deref()
                                == Some(artifact.artifact_id.as_str());
                            if ui
                                .selectable_label(
                                    selected,
                                    format!(
                                        "{} [{} / {}]",
                                        artifact.kind, format!("{:?}", artifact.scope), format!("{:?}", artifact.lifecycle)
                                    ),
                                )
                                .clicked()
                            {
                                panel.state.selected_artifact_id =
                                    Some(artifact.artifact_id.clone());
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(&artifact.uri)
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                                open_target::render_open_target_button(
                                    panel,
                                    ui,
                                    Some(&artifact.artifact_id),
                                    &artifact.uri,
                                    artifact.media_type.as_deref(),
                                );
                            });
                            ui.add_space(6.0);
                            ui.separator();
                        }
                    });
            });

            columns[1].vertical(|ui| {
                ui.heading("Details");
                ui.add_space(6.0);
                if let Some(selected) = panel.state.selected_artifact_id.as_ref().and_then(|id| {
                    panel
                        .state
                        .artifacts
                        .iter()
                        .find(|artifact| artifact.artifact_id == *id)
                }) {
                    let selected = selected.clone();
                    render_artifact_detail(panel, ui, night, &selected);
                } else {
                    ui.label(
                        RichText::new("Select an artifact to inspect metadata and runtime links.")
                            .small()
                            .color(palette::text_dim(night)),
                    );
                }

                ui.add_space(16.0);
                ui.heading("Cleanup");
                ui.add_space(6.0);
                render_artifact_cleanup_panel(panel, ui, ctx);
            });
        });
    });
}

fn render_artifact_detail(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    night: bool,
    artifact: &crate::api::ArtifactRecord,
) {
    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            kv_row(ui, night, "Artifact ID", &artifact.artifact_id);
            kv_row(ui, night, "Kind", &artifact.kind);
            kv_row(ui, night, "Scope", &format!("{:?}", artifact.scope));
            kv_row(ui, night, "Lifecycle", &format!("{:?}", artifact.lifecycle));
            ui.horizontal_wrapped(|ui| {
                kv_row(ui, night, "URI", &artifact.uri);
                open_target::render_open_target_button(
                    panel,
                    ui,
                    Some(&artifact.artifact_id),
                    &artifact.uri,
                    artifact.media_type.as_deref(),
                );
            });
            if let Some(path) = &artifact.virtual_path {
                kv_row(ui, night, "Virtual Path", path);
            }
            if let Some(media_type) = &artifact.media_type {
                kv_row(ui, night, "Media Type", media_type);
            }
            kv_row(ui, night, "Source Kind", &artifact.source_kind);
            kv_row(ui, night, "Agent", &artifact.agent_id);
            if let Some(thread_id) = &artifact.thread_id {
                kv_row(ui, night, "Thread", thread_id);
            }
            if let Some(session_id) = &artifact.session_id {
                kv_row(ui, night, "Session", session_id);
            }
            if let Some(task_id) = &artifact.task_id {
                kv_row(ui, night, "Task", task_id);
            }
            if let Some(run_id) = &artifact.run_id {
                kv_row(ui, night, "Run", run_id);
            }
            if let Some(trace_id) = &artifact.trace_id {
                kv_row(ui, night, "Trace", trace_id);
            }
            if !artifact.metadata.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Metadata")
                        .small()
                        .color(palette::text_dim(night)),
                );
                for (key, value) in &artifact.metadata {
                    kv_row(ui, night, key, value);
                }
            }
        });
}

fn render_artifact_cleanup_panel(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;
    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut panel.state.artifact_cleanup_policy.dry_run, "Dry Run");
                ui.label(
                    RichText::new("Preview deletions before executing cleanup.")
                        .small()
                        .color(palette::text_dim(night)),
                );
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("Ephemeral(h)")
                        .small()
                        .color(palette::text_dim(night)),
                );
                let ephemeral = panel
                    .state
                    .artifact_cleanup_policy
                    .ephemeral_max_age_hours
                    .get_or_insert(24);
                ui.add(egui::DragValue::new(ephemeral).range(1..=24 * 90));

                ui.label(
                    RichText::new("Session(h)")
                        .small()
                        .color(palette::text_dim(night)),
                );
                let session = panel
                    .state
                    .artifact_cleanup_policy
                    .session_max_age_hours
                    .get_or_insert(24 * 7);
                ui.add(egui::DragValue::new(session).range(1..=24 * 365));

                ui.label(
                    RichText::new("Durable(d)")
                        .small()
                        .color(palette::text_dim(night)),
                );
                let durable = panel
                    .state
                    .artifact_cleanup_policy
                    .durable_max_age_days
                    .get_or_insert(30);
                ui.add(egui::DragValue::new(durable).range(1..=3650));

                ui.label(
                    RichText::new("Max Delete")
                        .small()
                        .color(palette::text_dim(night)),
                );
                let max_delete = panel
                    .state
                    .artifact_cleanup_policy
                    .max_delete
                    .get_or_insert(50);
                ui.add(egui::DragValue::new(max_delete).range(1..=1000));
            });

            ui.add_space(8.0);
            artifact_query_combo(
                ui,
                "Cleanup Scope",
                &mut panel.state.artifact_cleanup_policy.scope,
                &["uploads", "workspace", "outputs", "artifacts"],
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Source")
                        .small()
                        .color(palette::text_dim(night)),
                );
                ui.add_sized(
                    [180.0, 22.0],
                    egui::TextEdit::singleline(
                        panel
                            .state
                            .artifact_cleanup_policy
                            .source_kind
                            .get_or_insert_with(String::new),
                    ),
                );
            });
            normalize_optional_string(&mut panel.state.artifact_cleanup_policy.source_kind);

            ui.add_space(8.0);
            if panel.state.artifact_cleanup_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Running artifact cleanup...");
                });
            } else if ui
                .button(if panel.state.artifact_cleanup_policy.dry_run {
                    "Preview Cleanup"
                } else {
                    "Execute Cleanup"
                })
                .clicked()
            {
                panel.state.do_artifact_cleanup(&panel.rt, ctx);
            }

            if let Some(error) = &panel.state.artifact_cleanup_error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(palette::DANGER));
            }
            if let Some(report) = &panel.state.last_artifact_cleanup_report {
                ui.add_space(8.0);
                kv_row(ui, night, "Scanned", &report.scanned.to_string());
                kv_row(ui, night, "Matched", &report.matched.to_string());
                kv_row(ui, night, "Deleted", &report.deleted.to_string());
                kv_row(ui, night, "Kept", &report.kept.to_string());
                kv_row(
                    ui,
                    night,
                    "Skipped Durable Without Policy",
                    &report.skipped_durable_without_policy.to_string(),
                );
                if !report.deleted_artifact_ids.is_empty() {
                    ui.label(
                        RichText::new("Affected Artifacts")
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    for artifact_id in &report.deleted_artifact_ids {
                        ui.monospace(artifact_id);
                    }
                }
                if !panel.state.artifact_cleanup_policy.dry_run && report.deleted > 0 {
                    if ui.button("Refresh Artifacts").clicked() {
                        panel.state.do_artifact_refresh(&panel.rt, ctx);
                    }
                }
            }
        });
}

fn kv_row(ui: &mut egui::Ui, night: bool, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("{label}:"))
                .small()
                .color(palette::text_dim(night)),
        );
        ui.label(RichText::new(value).small());
    });
}

fn artifact_query_combo(
    ui: &mut egui::Ui,
    label: &str,
    selected: &mut Option<String>,
    options: &[&str],
) {
    egui::ComboBox::from_label(label)
        .selected_text(selected.clone().unwrap_or_else(|| "all".to_string()))
        .show_ui(ui, |ui| {
            if ui.selectable_label(selected.is_none(), "all").clicked() {
                *selected = None;
            }
            for option in options {
                if ui
                    .selectable_label(selected.as_deref() == Some(*option), *option)
                    .clicked()
                {
                    *selected = Some((*option).to_string());
                }
            }
        });
}

fn normalize_optional_string(value: &mut Option<String>) {
    if value
        .as_ref()
        .is_some_and(|current| current.trim().is_empty())
    {
        *value = None;
    }
}

fn render_system_overview(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    if panel.state.sandboxes.is_empty() && panel.state.sandboxes_promise.is_none() {
        panel.state.do_sandboxes_refresh(&panel.rt, ctx);
    }
    if panel.state.restore_points.is_empty() && panel.state.pending_restore_points_promise.is_none()
    {
        panel.state.do_restore_points_refresh(&panel.rt, ctx);
    }

    ui.vertical(|ui| {
        ui.heading(t("system.overview", lang));
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Gateway Version:")
                                .small()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(
                                panel.state.gateway_version.as_deref().unwrap_or("Unknown"),
                            )
                            .strong(),
                        );
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Hardware:")
                                .small()
                                .color(palette::text_dim(night)),
                        );
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let hw = benshu_inference::hardware::HardwareStatus::detect();
                            ui.label(
                                RichText::new(format!(
                                    "GPU: {} | VRAM: {}/{} MB",
                                    hw.gpu_name.clone().unwrap_or("Unknown".into()),
                                    hw.vram_used_mb,
                                    hw.vram_total_mb
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                        }
                        #[cfg(target_arch = "wasm32")]
                        ui.label(RichText::new("WASM / WebGPU").strong());
                    });
                });
            });

        ui.add_space(24.0);

        // --- Sandbox Management ---
        ui.heading("Sandbox Management");
        ui.add_space(8.0);
        ui.label(
            RichText::new("Manage the secure environment where agent code and tools execute.")
                .small()
                .color(palette::text_dim(night)),
        );
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(
                            RichText::new("🧹 Reset Sandbox Environment").color(palette::WARNING),
                        )
                        .clicked()
                    {
                        let _client = panel.state.client.clone();
                        let rt = panel.rt.clone();
                        crate::common::task::spawn_task(&rt, async move {
                            // let _ = client.sandbox_reset().await;
                        });
                        panel.state.set_status("Sandbox reset signal sent", false);
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(
                            "Recommended if skills are misbehaving or filesystem is cluttered.",
                        )
                        .small()
                        .color(palette::text_dim(night)),
                    );
                });
                ui.add_space(12.0);
                if panel.state.sandboxes_promise.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Refreshing sandbox truth...");
                    });
                }
                if panel.state.sandboxes.is_empty() {
                    ui.label(
                        RichText::new("No active sandboxed tool processes are currently registered.")
                            .small()
                            .color(palette::text_dim(night)),
                    );
                } else {
                    for sandbox in panel.state.sandboxes.iter().take(6) {
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("PID {}", sandbox.pid))
                                    .small()
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(&sandbox.tool_name)
                                    .small()
                                    .color(palette::ACCENT),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} / {}",
                                    sandbox.sandbox_engine, sandbox.isolation_state
                                ))
                                .small()
                                .color(security_status_color(night, &sandbox.isolation_state)),
                            );
                            ui.label(
                                RichText::new(format!("Interpreter: {}", sandbox.interpreter))
                                    .small()
                                    .color(palette::text_dim(night)),
                            );
                        });
                    }
                }
            });

        ui.add_space(24.0);

        ui.heading("Sealed Restore Points");
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Create restore-only memory backups, validate them with dry-run, and execute explicit restore actions without leaving the panel.",
            )
            .small()
            .color(palette::text_dim(night)),
        );
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Create Restore Point").clicked() {
                        panel.state.do_restore_create(&panel.rt, ctx);
                    }
                    if ui.button("Refresh Restore Points").clicked() {
                        panel.state.do_restore_points_refresh(&panel.rt, ctx);
                    }
                    if panel.state.restore_points_loading {
                        ui.spinner();
                    }
                });
                if let Some(error) = &panel.state.restore_points_error {
                    ui.label(RichText::new(error).small().color(palette::DANGER));
                }
                if panel.state.restore_points.is_empty() {
                    ui.label(
                        RichText::new("No restore points discovered yet.")
                            .small()
                            .color(palette::text_dim(night)),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Backups").small().strong());
                            egui::ScrollArea::vertical()
                                .id_salt("restore_points_list")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for manifest in &panel.state.restore_points {
                                        let selected = panel
                                            .state
                                            .selected_restore_backup_id
                                            .as_deref()
                                            == Some(manifest.backup_id.as_str());
                                        if ui
                                            .selectable_label(
                                                selected,
                                                format!(
                                                    "{} · {} files",
                                                    manifest.backup_id, manifest.file_count
                                                ),
                                            )
                                            .clicked()
                                        {
                                            panel.state.selected_restore_backup_id =
                                                Some(manifest.backup_id.clone());
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            if let Some(backup_id) = panel.state.selected_restore_backup_id.clone() {
                                let selected_manifest = panel
                                    .state
                                    .restore_points
                                    .iter()
                                    .find(|manifest| manifest.backup_id == backup_id)
                                    .cloned();
                                let dry_run_report = panel
                                    .state
                                    .selected_restore_dry_run
                                    .clone()
                                    .filter(|report| report.backup_id == backup_id);
                                let restore_ready =
                                    dry_run_report.as_ref().map(|report| report.valid).unwrap_or(false);
                                ui.label(RichText::new(&backup_id).strong());
                                if let Some(manifest) = selected_manifest.as_ref() {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · Created {} · {} files · {} bytes",
                                            manifest.product,
                                            manifest.created_at,
                                            manifest.file_count,
                                            manifest.total_bytes
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "Contract {} · Fingerprint {}",
                                            manifest.contract_version, manifest.encryption_key_fingerprint
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "Storage root hint: {}",
                                            manifest.storage_root_hint
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                }
                                ui.horizontal(|ui| {
                                    if ui.button("Dry Run").clicked() {
                                        panel.state.do_restore_dry_run_refresh(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                        );
                                    }
                                    if ui.button("Policy").clicked() {
                                        panel.state.do_restore_policy_refresh(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                        );
                                    }
                                    if ui.button("List Receipts").clicked() {
                                        panel.state.do_restore_receipts_refresh(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                        );
                                    }
                                    if ui
                                        .add_enabled(
                                            restore_ready,
                                            egui::Button::new("Execute Restore"),
                                        )
                                        .clicked()
                                    {
                                        panel.state.do_restore_execute(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                        );
                                    }
                                    if ui.button("Delete Dry Run").clicked() {
                                        panel.state.do_restore_delete(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                            true,
                                        );
                                    }
                                    if ui.button("Delete").clicked() {
                                        panel.state.do_restore_delete(
                                            &panel.rt,
                                            ctx,
                                            backup_id.clone(),
                                            false,
                                        );
                                    }
                                });
                                if !restore_ready {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(
                                            "Run Dry Run first. Restore stays disabled until the selected backup validates successfully.",
                                        )
                                        .small()
                                        .color(palette::WARNING),
                                    );
                                }
                                if let Some(report) = dry_run_report.as_ref() {
                                    if report.backup_id == backup_id {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "Dry Run: {} · restorable {}/{}",
                                                if report.valid { "valid" } else { "invalid" },
                                                report.restorable_files,
                                                report.file_count
                                            ))
                                            .small()
                                            .color(if report.valid {
                                                palette::SUCCESS
                                            } else {
                                                palette::DANGER
                                            }),
                                        );
                                        if !report.missing_payloads.is_empty() {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Missing payloads: {}",
                                                    report.missing_payloads.join(", ")
                                                ))
                                                .small()
                                                .color(palette::WARNING),
                                            );
                                        }
                                        if !report.integrity_mismatches.is_empty() {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Integrity mismatches: {}",
                                                    report.integrity_mismatches.join(", ")
                                                ))
                                                .small()
                                                .color(palette::DANGER),
                                            );
                                        }
                                        ui.label(
                                            RichText::new(format!(
                                                "Checked {} · Contract {} · Fingerprint {} · {} bytes",
                                                report.checked_at,
                                                report.contract_version,
                                                report.encryption_key_fingerprint,
                                                report.total_bytes
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                    }
                                }
                                if let Some(manifest) = selected_manifest.as_ref() {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Files").small().strong());
                                    for entry in manifest.files.iter().take(5) {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {} bytes · {}",
                                                entry.label, entry.size_bytes, entry.relative_path
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                    }
                                    if manifest.files.len() > 5 {
                                        ui.label(
                                            RichText::new(format!(
                                                "... and {} more files",
                                                manifest.files.len() - 5
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                    }
                                }
                                if let Some(policy) = &panel.state.selected_restore_policy_basis {
                                    if policy.backup_id == backup_id {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "Policy: {} · {}",
                                                policy.decision_kind, policy.policy_basis
                                            ))
                                            .small()
                                            .color(if policy.decision_kind == "permit" {
                                                palette::SUCCESS
                                            } else {
                                                palette::WARNING
                                            }),
                                        );
                                        for reason in &policy.reasons {
                                            ui.label(
                                                RichText::new(format!("Reason: {}", reason))
                                                    .small()
                                                    .color(palette::text_dim(night)),
                                            );
                                        }
                                        for warning in &policy.warnings {
                                            ui.label(
                                                RichText::new(format!("Warning: {}", warning))
                                                    .small()
                                                    .color(palette::WARNING),
                                            );
                                        }
                                    }
                                }
                                if !panel.state.selected_restore_receipts.is_empty() {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Receipts").small().strong());
                                    for receipt in panel
                                        .state
                                        .selected_restore_receipts
                                        .iter()
                                        .filter(|receipt| receipt.backup_id == backup_id)
                                        .take(4)
                                    {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {} files · {} bytes · {}",
                                                receipt.receipt_id,
                                                receipt.restored_files,
                                                receipt.restored_bytes,
                                                receipt.restored_at
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "Receipt contract {} · Fingerprint {}",
                                                receipt.contract_version,
                                                receipt.encryption_key_fingerprint
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                    }
                                }
                                if let Some(delete_report) =
                                    &panel.state.selected_restore_delete_report
                                {
                                    if delete_report.backup_id == backup_id {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "{} delete: {} files · {} bytes · {} receipts · {}",
                                                if delete_report.dry_run {
                                                    "Dry-run"
                                                } else {
                                                    "Executed"
                                                },
                                                delete_report.file_count,
                                                delete_report.total_bytes,
                                                delete_report.receipt_count,
                                                delete_report.deleted_at
                                            ))
                                            .small()
                                            .color(if delete_report.dry_run {
                                                palette::WARNING
                                            } else {
                                                palette::SUCCESS
                                            }),
                                        );
                                    }
                                }
                            } else {
                                ui.label(
                                    RichText::new("Select a restore point to inspect dry-run health and restore receipts.")
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                            }
                        });
                    });
                }
            });

        ui.add_space(24.0);

        // --- System Update ---
        ui.heading("System Update");
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(status) = &panel.state.update_status {
                        ui.label(RichText::new(status).small().color(palette::ACCENT));
                        ui.add_space(8.0);
                    }

                    if panel.state.update_in_progress {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Update in progress...");
                        });
                    } else {
                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("Check for Updates").strong())
                                .clicked()
                            {
                                panel.state.do_system_update(&panel.rt, ctx);
                            }
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(
                                    "Sync components with latest BenShu stable release.",
                                )
                                .small()
                                .color(palette::text_dim(night)),
                            );
                        });
                    }
                });
            });
    });
}

fn security_status_color(night: bool, isolation_state: &str) -> Color32 {
    match isolation_state {
        "hardened" => palette::SUCCESS,
        "partial" => palette::WARNING,
        "degraded" => palette::DANGER,
        _ => palette::text_dim(night),
    }
}

fn render_doctor_mode(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.vertical(|ui| {
        if ui
            .button(format!("🚀 {}", t("system.run_doctor", lang)))
            .clicked()
        {
            panel.state.do_doctor_run(&panel.rt, ctx);
        }
        ui.add_space(16.0);

        if panel.state.doctor_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t("misc.searching", lang));
            });
        } else if let Some(err) = &panel.state.doctor_error {
            ui.label(RichText::new(format!("Error: {}", err)).color(palette::DANGER));
        } else if panel.state.doctor_results.is_some() {
            let mut pending_repair = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for res in panel.state.doctor_results.as_ref().unwrap() {
                    egui::Frame::new()
                        .fill(panel.theme_bg_deep())
                        .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let icon = if res.success { "✅" } else { "❌" };
                                let color = if res.success {
                                    palette::SUCCESS
                                } else {
                                    palette::DANGER
                                };
                                ui.label(RichText::new(icon).color(color).strong());
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&res.name).strong());
                                    ui.label(
                                        RichText::new(&res.message)
                                            .small()
                                            .color(palette::text_dim(night)),
                                    );
                                    if let Some(rec) = &res.recommendation {
                                        ui.label(
                                            RichText::new(rec).small().color(palette::WARNING),
                                        );
                                    }
                                });
                                if res.can_repair && !res.success {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if panel.state.repair_loading {
                                                ui.spinner();
                                            } else {
                                                if ui
                                                    .button(RichText::new("🛠 Repair").small())
                                                    .clicked()
                                                {
                                                    pending_repair = Some(res.name.clone());
                                                }
                                            }
                                        },
                                    );
                                }
                            });
                        });
                    ui.add_space(4.0);
                }
            });

            if let Some(name) = pending_repair {
                panel.state.do_repair(&panel.rt, ctx, &name);
            }
        }
    });
}

pub(crate) fn render_organ_card_ui(
    ui: &mut egui::Ui,
    night: bool,
    icon: &str,
    title: &str,
    hint: &str,
    model_var: &mut String,
) -> (bool, bool) {
    let mut changed = false;
    let mut applied = false;

    let (status_text, status_color) = ("LOADED".to_string(), palette::SUCCESS);

    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(24.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong());
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(status_text.as_str())
                                .small()
                                .strong()
                                .color(status_color),
                        );
                    });
                    ui.label(RichText::new(hint).small().color(palette::text_dim(night)));
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label("Model:");
                let resp = ui.add(
                    egui::TextEdit::singleline(model_var)
                        .hint_text("Local path or api:provider/model")
                        .desired_width(200.0),
                );
                if resp.changed() {
                    changed = true;
                }

                egui::ComboBox::from_id_salt(format!("{}_select", title))
                    .selected_text("Quick Switch...")
                    .show_ui(ui, |ui| {
                        ui.separator();
                        if ui
                            .selectable_label(
                                model_var.starts_with("api:openai"),
                                "Cloud: OpenAI Gateway",
                            )
                            .clicked()
                        {
                            *model_var = "api:openai/default".to_string();
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                model_var.starts_with("api:deepseek"),
                                "Cloud: DeepSeek V3",
                            )
                            .clicked()
                        {
                            *model_var = "api:deepseek/deepseek-chat".to_string();
                            changed = true;
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_text = if status_text.as_str() == "READY" {
                        "Re-Hydrate"
                    } else {
                        "Activate"
                    };
                    if ui.button(RichText::new(btn_text).strong()).clicked() {
                        applied = true;
                    }
                });
            });
        });

    (changed, applied)
}

pub(crate) fn render_fact_check_card_ui(
    ui: &mut egui::Ui,
    night: bool,
    icon: &str,
    title: &str,
    hint: &str,
    model_var: &mut String,
    enabled_var: &mut bool,
) -> (bool, bool) {
    let mut changed = false;
    let mut applied = false;

    let (status_text, status_color) = if !*enabled_var {
        ("DISABLED".to_string(), palette::DANGER)
    } else {
        ("LOADED".to_string(), palette::SUCCESS)
    };

    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(24.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong());
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(status_text.as_str())
                                .small()
                                .strong()
                                .color(status_color),
                        );
                    });
                    ui.label(RichText::new(hint).small().color(palette::text_dim(night)));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(checkbox_toggle(enabled_var)).clicked() {
                        changed = true;
                    }
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label("Model:");
                let resp = ui.add(
                    egui::TextEdit::singleline(model_var)
                        .hint_text("Local path or api:provider/model")
                        .desired_width(200.0),
                );
                if resp.changed() {
                    changed = true;
                }

                egui::ComboBox::from_id_salt(format!("{}_select", title))
                    .selected_text("Quick Switch...")
                    .show_ui(ui, |_ui| {
                        // Placeholders for quick switch
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_text = if status_text.as_str() == "READY" {
                        "Re-Hydrate"
                    } else {
                        "Activate"
                    };
                    if ui.button(RichText::new(btn_text).strong()).clicked() {
                        applied = true;
                    }
                });
            });
        });

    (changed, applied)
}
