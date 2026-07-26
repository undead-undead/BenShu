use super::{render_trace_chip, trace_metadata, trace_metadata_is_true, ClawPanel};
use crate::common::palette;
use crate::i18n::t;
use crate::ui::open_target;
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct VerificationSourceView {
    title: String,
    uri: String,
}

pub(super) fn parse_verification_sources_json(value: Option<&str>) -> Vec<VerificationSourceView> {
    value
        .and_then(|raw| serde_json::from_str::<Vec<VerificationSourceView>>(raw).ok())
        .unwrap_or_default()
}

pub(super) fn parse_verification_string_list(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

fn render_verification_sources_block(
    ui: &mut egui::Ui,
    night: bool,
    label: &str,
    sources: &[VerificationSourceView],
) {
    if sources.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new(label).small().color(palette::ACCENT));
    for source in sources.iter().take(3) {
        ui.label(
            RichText::new(format!("- {}: {}", source.title, source.uri))
                .small()
                .color(palette::text_dim(night)),
        );
    }
    if sources.len() > 3 {
        ui.label(
            RichText::new(format!("+ {} more", sources.len() - 3))
                .small()
                .color(palette::text_dim(night)),
        );
    }
}

fn render_verification_string_list_block(
    ui: &mut egui::Ui,
    night: bool,
    label: &str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new(label).small().color(palette::ACCENT));
    for value in values.iter().take(3) {
        ui.label(
            RichText::new(format!("- {}", value))
                .small()
                .color(palette::text_dim(night)),
        );
    }
    if values.len() > 3 {
        ui.label(
            RichText::new(format!("+ {} more", values.len() - 3))
                .small()
                .color(palette::text_dim(night)),
        );
    }
}

pub(super) fn render_runtime_tasks_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;
    let session_label = panel
        .state
        .session_runtime_tasks_session_id
        .as_deref()
        .unwrap_or("default")
        .to_string();

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(egui::Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t("runtime_tasks.title", lang))
                            .strong()
                            .color(palette::ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                t("runtime_tasks.session", lang),
                                session_label
                            ))
                            .small()
                            .color(palette::text_dim(night)),
                        );
                    });
                });
                ui.label(
                    RichText::new(t("runtime_tasks.subtitle", lang))
                        .small()
                        .color(palette::text_dim(night)),
                );
                ui.add_space(8.0);

                if panel.state.session_runtime_tasks_loading {
                    ui.label(
                        RichText::new(t("runtime_tasks.loading", lang))
                            .color(palette::text_dim(night)),
                    );
                    return;
                }

                if let Some(error) = &panel.state.session_runtime_tasks_error {
                    ui.label(RichText::new(error).color(palette::DANGER));
                    return;
                }

                if panel.state.session_runtime_tasks.is_empty() {
                    ui.label(
                        RichText::new(t("runtime_tasks.empty", lang))
                            .color(palette::text_dim(night)),
                    );
                    return;
                }

                let tasks = panel.state.session_runtime_tasks.clone();
                for task in &tasks {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&task.name)
                                .strong()
                                .color(palette::text_bright(night)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                t("runtime_tasks.status", lang),
                                task.status
                            ))
                            .small()
                            .color(palette::ACCENT),
                        );
                    });
                    ui.label(
                        RichText::new(&task.description)
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    if let Some(detail) = &task.status_detail {
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                t("runtime_tasks.detail", lang),
                                detail
                            ))
                            .small()
                            .color(palette::text_dim(night)),
                        );
                    }
                    if !task.checkpoints.is_empty() {
                        ui.label(
                            RichText::new(t("runtime_tasks.checkpoints", lang))
                                .small()
                                .color(palette::ACCENT),
                        );
                        for checkpoint in task.checkpoints.iter().rev().take(3) {
                            let summary = checkpoint.summary.as_deref().unwrap_or("");
                            ui.label(
                                RichText::new(format!(
                                    "- #{} {} {}",
                                    checkpoint.step, checkpoint.label, summary
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                        }
                    }
                    if !task.artifacts.is_empty() {
                        ui.label(
                            RichText::new(t("runtime_tasks.artifacts", lang))
                                .small()
                                .color(palette::ACCENT),
                        );
                        for artifact in task.artifacts.iter().take(3) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "- {} {}: {}",
                                        artifact.kind, artifact.artifact_id, artifact.uri
                                    ))
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
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if let Some(run_id) = &task.run_id {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    t("runtime_tasks.run_id", lang),
                                    run_id
                                ))
                                .monospace()
                                .small()
                                .color(palette::text_dim(night)),
                            );
                        }
                        if let Some(trace_id) = &task.trace_id {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    t("runtime_tasks.trace_id", lang),
                                    trace_id
                                ))
                                .monospace()
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            let trace_loaded =
                                panel.state.selected_run_trace_id.as_deref() == Some(trace_id);
                            if ui
                                .small_button(if trace_loaded {
                                    t("runtime_tasks.trace_reload", lang)
                                } else {
                                    t("runtime_tasks.trace_view", lang)
                                })
                                .clicked()
                            {
                                panel
                                    .state
                                    .do_run_trace_refresh(&panel.rt, ctx, trace_id.clone());
                            }
                        }
                        if let Some(witness_id) = &task.witness_id {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    t("runtime_tasks.trace_witness_id", lang),
                                    witness_id
                                ))
                                .monospace()
                                .small()
                                .color(palette::text_dim(night)),
                            );
                            let witness_loaded =
                                panel.state.selected_witness_id.as_deref() == Some(witness_id);
                            if ui
                                .small_button(if witness_loaded {
                                    t("runtime_tasks.witness_reload", lang)
                                } else {
                                    t("runtime_tasks.witness_view", lang)
                                })
                                .clicked()
                            {
                                panel
                                    .state
                                    .do_witness_refresh(&panel.rt, ctx, witness_id.clone());
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        let wait_selected = panel.state.selected_task_wait_task_id.as_deref()
                            == Some(task.id.as_str());
                        let output_selected = panel.state.selected_task_output_task_id.as_deref()
                            == Some(task.id.as_str());
                        if ui
                            .add_enabled(
                                !(panel.state.selected_task_wait_loading && wait_selected),
                                egui::Button::new(t("runtime_tasks.wait", lang)),
                            )
                            .clicked()
                        {
                            panel
                                .state
                                .do_runtime_task_wait(&panel.rt, ctx, task.id.clone());
                        }
                        if ui
                            .add_enabled(
                                !(panel.state.selected_task_output_loading && output_selected),
                                egui::Button::new(t("runtime_tasks.output", lang)),
                            )
                            .clicked()
                        {
                            panel.state.do_runtime_task_output_refresh(
                                &panel.rt,
                                ctx,
                                task.id.clone(),
                            );
                        }
                        if ui
                            .add_enabled(
                                !runtime_task_is_terminal(&task.status)
                                    && panel.state.pending_task_cancel_promise.is_none(),
                                egui::Button::new(t("runtime_tasks.cancel", lang)),
                            )
                            .clicked()
                        {
                            panel
                                .state
                                .do_runtime_task_cancel(&panel.rt, ctx, task.id.clone());
                        }
                    });
                    if panel.state.selected_task_wait_task_id.as_deref() == Some(task.id.as_str()) {
                        if let Some(error) = &panel.state.selected_task_wait_error {
                            ui.label(RichText::new(error).small().color(palette::DANGER));
                        } else if panel.state.selected_task_wait_loading {
                            ui.label(
                                RichText::new(t("runtime_tasks.waiting", lang))
                                    .small()
                                    .color(palette::text_dim(night)),
                            );
                        } else if let Some(notice) = &panel.state.selected_task_wait_notice {
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}",
                                    t("runtime_tasks.wait_reason", lang),
                                    notice
                                ))
                                .small()
                                .color(palette::text_dim(night)),
                            );
                        }
                    }
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            t("runtime_tasks.updated_at", lang),
                            task.updated_at
                        ))
                        .small()
                        .color(palette::text_dim(night)),
                    );
                }

                render_selected_task_output_card(panel, ui);
                render_runtime_task_graph(ui, lang, night, &tasks);
                render_session_delegation_card(panel, ui, ctx);
                render_selected_trace_card(panel, ui);
                render_selected_witness_card(panel, ui);
            });
        });
}

fn runtime_task_is_terminal(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "canceled"
    )
}

fn render_selected_task_output_card(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);
    ui.label(
        RichText::new(t("runtime_tasks.output_panel_title", lang))
            .small()
            .color(palette::ACCENT),
    );

    if panel.state.selected_task_output_loading {
        ui.label(
            RichText::new(t("runtime_tasks.output_loading", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    }

    if let Some(error) = &panel.state.selected_task_output_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
        return;
    }

    let Some(output) = panel.state.selected_task_output.clone() else {
        ui.label(
            RichText::new(t("runtime_tasks.output_empty", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("task: {}", output.task.id))
                .small()
                .monospace()
                .color(palette::text_dim(night)),
        );
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.status", lang),
                output.task.status
            ))
            .small()
            .color(palette::text_dim(night)),
        );
    });

    if let Some(result) = &output.result {
        let rendered = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
        ui.label(
            RichText::new(t("runtime_tasks.output_result", lang))
                .small()
                .color(palette::ACCENT),
        );
        ui.code(rendered);
    }

    if output.artifact_previews.is_empty() {
        ui.label(
            RichText::new(t("runtime_tasks.output_no_artifacts", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    }

    for artifact in &output.artifact_previews {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{}: {}", artifact.kind, artifact.uri))
                    .small()
                    .monospace()
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
        if let Some(preview) = &artifact.preview {
            ui.code(preview);
            if artifact.truncated {
                ui.label(
                    RichText::new(t("runtime_tasks.output_truncated", lang))
                        .small()
                        .color(palette::text_dim(night)),
                );
            }
        }
    }
}

fn render_runtime_task_graph(
    ui: &mut egui::Ui,
    lang: crate::i18n::Language,
    night: bool,
    tasks: &[crate::api::SessionTaskInfo],
) {
    use std::collections::BTreeMap;

    if tasks.is_empty() {
        return;
    }

    let mut by_id: BTreeMap<String, crate::api::SessionTaskInfo> = BTreeMap::new();
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots: Vec<String> = Vec::new();

    for task in tasks {
        by_id.insert(task.id.clone(), task.clone());
    }

    for task in tasks {
        if let Some(parent_id) = &task.parent_task_id {
            if by_id.contains_key(parent_id) {
                children
                    .entry(parent_id.clone())
                    .or_default()
                    .push(task.id.clone());
                continue;
            }
        }
        roots.push(task.id.clone());
    }

    roots.sort();
    for ids in children.values_mut() {
        ids.sort();
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);
    ui.label(
        RichText::new(t("runtime_tasks.graph_title", lang))
            .small()
            .color(palette::ACCENT),
    );
    ui.label(
        RichText::new(t("runtime_tasks.graph_subtitle", lang))
            .small()
            .color(palette::text_dim(night)),
    );
    ui.add_space(6.0);

    for root_id in roots {
        render_runtime_task_graph_node(ui, lang, night, &by_id, &children, &root_id, 0);
    }
}

fn render_runtime_task_graph_node(
    ui: &mut egui::Ui,
    lang: crate::i18n::Language,
    night: bool,
    by_id: &std::collections::BTreeMap<String, crate::api::SessionTaskInfo>,
    children: &std::collections::BTreeMap<String, Vec<String>>,
    task_id: &str,
    depth: usize,
) {
    let Some(task) = by_id.get(task_id) else {
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.add_space(depth as f32 * 18.0);
        ui.label(
            RichText::new(if depth == 0 { "●" } else { "↳" })
                .small()
                .color(palette::ACCENT),
        );
        ui.label(
            RichText::new(&task.name)
                .small()
                .strong()
                .color(palette::text_bright(night)),
        );
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.status", lang),
                task.status
            ))
            .small()
            .color(palette::text_dim(night)),
        );
        if let Some(delegation_state) = &task.delegation_state {
            ui.label(
                RichText::new(format!("delegation: {}", delegation_state))
                    .small()
                    .monospace()
                    .color(palette::text_dim(night)),
            );
        }
        if let Some(delegated_to) = &task.delegated_to {
            ui.label(
                RichText::new(format!("to: {}", delegated_to))
                    .small()
                    .monospace()
                    .color(palette::text_dim(night)),
            );
        }
        if let Some(root_task_id) = &task.root_task_id {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.graph_root", lang),
                    root_task_id
                ))
                .small()
                .monospace()
                .color(palette::text_dim(night)),
            );
        }
    });

    if let Some(child_ids) = children.get(task_id) {
        for child_id in child_ids {
            render_runtime_task_graph_node(ui, lang, night, by_id, children, child_id, depth + 1);
        }
    }
}

fn render_session_delegation_card(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("Delegation").strong().color(palette::ACCENT));
        if panel.state.selected_session_delegation_loading {
            ui.label(
                RichText::new(t("runtime_tasks.loading", lang))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    });

    if let Some(error) = &panel.state.selected_session_delegation_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
        return;
    }

    let Some(trace) = panel.state.selected_session_delegation_trace.clone() else {
        ui.label(
            RichText::new("No delegation evidence for this session yet.")
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("active_role: {}", trace.active_role))
                .small()
                .monospace()
                .color(palette::text_dim(night)),
        );
        if let Some(runtime_profile) = &trace.runtime_profile {
            ui.label(
                RichText::new(format!("runtime_profile: {}", runtime_profile))
                    .small()
                    .monospace()
                    .color(palette::text_dim(night)),
            );
        }
    });

    if let Some(owner_rollup) = &trace.owner_rollup {
        ui.add_space(4.0);
        ui.label(RichText::new("Owner rollup").small().color(palette::ACCENT));
        let summary =
            serde_json::to_string_pretty(owner_rollup).unwrap_or_else(|_| owner_rollup.to_string());
        ui.code(summary);
    }

    ui.add_space(6.0);
    ui.label(
        RichText::new("Recent A2A inbox")
            .small()
            .color(palette::ACCENT),
    );

    if trace.inbox.is_empty() {
        ui.label(
            RichText::new("No recent A2A inbox entries for this session.")
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    }

    for entry in &trace.inbox {
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&entry.kind)
                        .small()
                        .strong()
                        .color(palette::text_bright(night)),
                );
                if let Some(state) = &entry.delegation_state {
                    ui.label(
                        RichText::new(format!("state: {}", state))
                            .small()
                            .monospace()
                            .color(palette::ACCENT),
                    );
                }
                if let Some(request_id) = &entry.request_id {
                    ui.label(
                        RichText::new(format!("request: {}", request_id))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
            });
            ui.label(
                RichText::new(&entry.summary)
                    .small()
                    .color(palette::text_dim(night)),
            );
            ui.horizontal_wrapped(|ui| {
                if let Some(trace_id) = &entry.trace_id {
                    if ui
                        .small_button(
                            RichText::new(format!("Open trace {}", trace_id))
                                .small()
                                .monospace(),
                        )
                        .clicked()
                    {
                        panel
                            .state
                            .do_run_trace_refresh(&panel.rt, ctx, trace_id.clone());
                    }
                }
                if let Some(task_id) = &entry.task_id {
                    ui.label(
                        RichText::new(format!("task: {}", task_id))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
                if let Some(root_task_id) = &entry.root_task_id {
                    ui.label(
                        RichText::new(format!("root: {}", root_task_id))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
                if let Some(delegated_to) = &entry.delegated_to {
                    ui.label(
                        RichText::new(format!("to: {}", delegated_to))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
                if let Some(owner) = &entry.final_response_owner {
                    ui.label(
                        RichText::new(format!("final_owner: {}", owner))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
                if let Some(owner) = &entry.visible_owner {
                    ui.label(
                        RichText::new(format!("visible_owner: {}", owner))
                            .small()
                            .monospace()
                            .color(palette::text_dim(night)),
                    );
                }
            });
        });
    }
}

pub(super) fn render_selected_trace_card(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t("runtime_tasks.trace_panel_title", lang))
                .strong()
                .color(palette::ACCENT),
        );
        if let Some(trace_id) = &panel.state.selected_run_trace_id {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_id", lang),
                    trace_id
                ))
                .small()
                .monospace()
                .color(palette::text_dim(night)),
            );
        }
    });

    if let Some(trace_id) = panel.state.selected_run_trace_id.clone() {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Replay").clicked() {
                panel
                    .state
                    .do_run_replay_refresh(&panel.rt, ui.ctx(), trace_id.clone());
            }
            if ui.small_button("Profiler").clicked() {
                panel
                    .state
                    .do_profiler_refresh(&panel.rt, ui.ctx(), trace_id.clone());
            }
            if let Some(trace) = &panel.state.selected_run_trace {
                let query = benshu_telemetry::ProfilerArtifactQuery {
                    run_id: Some(trace.run_id),
                    limit: Some(16),
                    ..benshu_telemetry::ProfilerArtifactQuery::default()
                };
                if ui.small_button("Profiler Query").clicked() {
                    panel
                        .state
                        .do_profiler_query_refresh(&panel.rt, ui.ctx(), query.clone());
                }
                if ui.small_button("Profiler Export").clicked() {
                    panel
                        .state
                        .do_profiler_export_refresh(&panel.rt, ui.ctx(), query);
                }
            }
        });
    }

    if panel.state.selected_run_trace_loading {
        ui.label(
            RichText::new(t("runtime_tasks.trace_loading", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    }

    if let Some(error) = &panel.state.selected_run_trace_error {
        ui.label(RichText::new(error).color(palette::DANGER));
        return;
    }

    let Some(trace) = panel.state.selected_run_trace.clone() else {
        ui.label(
            RichText::new(t("runtime_tasks.trace_empty", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "{}: {:?}",
                t("runtime_tasks.trace_status", lang),
                trace.status
            ))
            .small()
            .color(palette::ACCENT),
        );
        if let Some(provider) = &trace.provider {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_provider", lang),
                    provider
                ))
                .small()
                .color(palette::text_dim(night)),
            );
        }
        if let Some(model) = &trace.model {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_model", lang),
                    model
                ))
                .small()
                .color(palette::text_dim(night)),
            );
        }
    });

    ui.label(
        RichText::new(format!(
            "{}: {}",
            t("runtime_tasks.trace_started", lang),
            trace.started_at
        ))
        .small()
        .color(palette::text_dim(night)),
    );
    if let Some(finished_at) = &trace.finished_at {
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.trace_finished", lang),
                finished_at
            ))
            .small()
            .color(palette::text_dim(night)),
        );
    }

    ui.horizontal_wrapped(|ui| {
        if let Some(prompt_tokens) = trace.prompt_tokens {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_prompt_tokens", lang),
                    prompt_tokens
                ))
                .small()
                .color(palette::text_dim(night)),
            );
        }
        if let Some(completion_tokens) = trace.completion_tokens {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_completion_tokens", lang),
                    completion_tokens
                ))
                .small()
                .color(palette::text_dim(night)),
            );
        }
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.trace_tools", lang),
                trace.tools.len()
            ))
            .small()
            .color(palette::text_dim(night)),
        );
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.trace_artifacts", lang),
                trace.artifacts.len()
            ))
            .small()
            .color(palette::text_dim(night)),
        );
    });

    render_runtime_governance_status_card(panel, ui, &trace);
    render_context_budget_diagnostics_card(panel, ui, &trace);
    render_continuation_runtime_card(panel, ui, &trace);
    render_background_continuity_card(panel, ui, &trace);

    if trace.degradation_notes.is_empty() {
        ui.label(
            RichText::new(t("runtime_tasks.trace_no_degradation", lang))
                .small()
                .color(palette::SUCCESS),
        );
    } else {
        ui.label(
            RichText::new(t("runtime_tasks.trace_degradation", lang))
                .small()
                .color(palette::ACCENT),
        );
        for note in &trace.degradation_notes {
            ui.label(
                RichText::new(format!("- {}", note))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    }

    if let Some(witness) = trace.witness.clone() {
        ui.add_space(6.0);
        let witness_id = witness.witness_id.to_string();
        let witness_loaded = panel.state.selected_witness_id.as_deref() == Some(&witness_id);
        ui.horizontal_wrapped(|ui| {
            render_witness_summary_block(ui, lang, night, &witness);
            if ui
                .small_button(if witness_loaded {
                    t("runtime_tasks.witness_reload", lang)
                } else {
                    t("runtime_tasks.witness_view", lang)
                })
                .clicked()
            {
                panel
                    .state
                    .do_witness_refresh(&panel.rt, ui.ctx(), witness_id);
            }
        });
    }

    if !trace.tools.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(t("runtime_tasks.trace_tool_list", lang))
                .small()
                .color(palette::ACCENT),
        );
        for tool in &trace.tools {
            ui.label(
                RichText::new(format!("- {} ({:?})", tool.tool_name, tool.status))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    }

    if !trace.artifacts.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(t("runtime_tasks.trace_artifact_list", lang))
                .small()
                .color(palette::ACCENT),
        );
        for artifact in &trace.artifacts {
            ui.label(
                RichText::new(format!("- {} [{}]", artifact.kind, artifact.artifact_id))
                    .small()
                    .color(palette::text_bright(night)),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        t("runtime_tasks.trace_artifact_uri", lang),
                        artifact.uri
                    ))
                    .small()
                    .monospace()
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
            if let Some(media_type) = &artifact.media_type {
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        t("runtime_tasks.trace_artifact_media_type", lang),
                        media_type
                    ))
                    .small()
                    .color(palette::text_dim(night)),
                );
            }
        }
    }

    if panel.state.selected_run_replay_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Loading replay...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_run_replay_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if let Some(replay) = &panel.state.selected_run_replay {
        ui.add_space(6.0);
        ui.collapsing("Replay Projection", |ui| {
            let rendered =
                serde_json::to_string_pretty(replay).unwrap_or_else(|_| format!("{:#?}", replay));
            ui.code(rendered);
        });
    }

    if panel.state.selected_profiler_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Loading profiler artifact...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_profiler_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if let Some(profiler) = &panel.state.selected_profiler_artifact {
        ui.add_space(6.0);
        ui.collapsing("Profiler Artifact", |ui| {
            let rendered = serde_json::to_string_pretty(profiler)
                .unwrap_or_else(|_| format!("{:#?}", profiler));
            ui.code(rendered);
        });
    }

    if panel.state.selected_profiler_query_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Querying profiler artifacts...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_profiler_query_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if !panel.state.selected_profiler_query_results.is_empty() {
        ui.add_space(6.0);
        ui.collapsing("Profiler Query Results", |ui| {
            for artifact in &panel.state.selected_profiler_query_results {
                ui.label(
                    RichText::new(format!(
                        "{} · run {} · trace {}",
                        artifact.profiler_id, artifact.run_id, artifact.trace_id
                    ))
                    .small()
                    .monospace()
                    .color(palette::text_dim(night)),
                );
            }
        });
    }

    if panel.state.selected_profiler_export_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Exporting profiler artifacts...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_profiler_export_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if let Some(export) = &panel.state.selected_profiler_export {
        ui.add_space(6.0);
        ui.collapsing("Profiler Export", |ui| {
            let rendered =
                serde_json::to_string_pretty(export).unwrap_or_else(|_| format!("{:#?}", export));
            ui.code(rendered);
        });
    }
}

pub(super) fn render_selected_witness_card(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t("runtime_tasks.witness_panel_title", lang))
                .strong()
                .color(palette::ACCENT),
        );
        if let Some(witness_id) = &panel.state.selected_witness_id {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t("runtime_tasks.trace_witness_id", lang),
                    witness_id
                ))
                .small()
                .monospace()
                .color(palette::text_dim(night)),
            );
        }
    });

    if let Some(witness_id) = panel.state.selected_witness_id.clone() {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Witness Bundle").clicked() {
                panel
                    .state
                    .do_witness_bundle_refresh(&panel.rt, ui.ctx(), witness_id.clone());
            }
            if ui.small_button("Witness Log").clicked() {
                panel
                    .state
                    .do_witness_log_refresh(&panel.rt, ui.ctx(), witness_id.clone());
            }
        });
    }

    if panel.state.selected_witness_loading {
        ui.label(
            RichText::new(t("runtime_tasks.witness_loading", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    }

    if let Some(error) = &panel.state.selected_witness_error {
        ui.label(RichText::new(error).color(palette::DANGER));
        return;
    }

    let Some(witness) = &panel.state.selected_witness_summary else {
        ui.label(
            RichText::new(t("runtime_tasks.witness_empty", lang))
                .small()
                .color(palette::text_dim(night)),
        );
        return;
    };

    render_witness_summary_block(ui, lang, night, witness);

    if panel.state.selected_witness_bundle_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Loading witness bundle...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_witness_bundle_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if let Some(bundle) = &panel.state.selected_witness_bundle {
        ui.add_space(6.0);
        ui.collapsing("Witness Bundle", |ui| {
            let rendered =
                serde_json::to_string_pretty(bundle).unwrap_or_else(|_| format!("{:#?}", bundle));
            ui.code(rendered);
        });
    }

    if panel.state.selected_witness_log_loading {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Loading witness log...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_witness_log_error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if let Some(log) = panel.state.selected_witness_log.clone() {
        ui.add_space(6.0);
        ui.collapsing("Witness Log", |ui| {
            let rendered =
                serde_json::to_string_pretty(&log).unwrap_or_else(|_| format!("{:#?}", log));
            ui.code(rendered);
        });

        render_truth_verification_witness_filters(panel, ui, &log);
        render_windows_native_witness_filters(panel, ui, &log);
    }
}

fn render_witness_summary_block(
    ui: &mut egui::Ui,
    lang: crate::i18n::Language,
    night: bool,
    witness: &benshu_telemetry::WitnessSummary,
) {
    ui.label(
        RichText::new(format!(
            "{}: {}",
            t("runtime_tasks.trace_witness", lang),
            witness.verdict
        ))
        .small()
        .color(palette::ACCENT),
    );
    ui.label(
        RichText::new(format!(
            "{}: {}",
            t("runtime_tasks.trace_witness_id", lang),
            witness.witness_id
        ))
        .small()
        .monospace()
        .color(palette::text_dim(night)),
    );
    ui.label(
        RichText::new(format!(
            "{}: {}",
            t("runtime_tasks.trace_witness_replayable", lang),
            if witness.replayable {
                t("runtime_tasks.trace_yes", lang)
            } else {
                t("runtime_tasks.trace_no", lang)
            }
        ))
        .small()
        .color(palette::text_dim(night)),
    );
    if let Some(benchmark_fingerprint) = &witness.benchmark_fingerprint {
        ui.label(
            RichText::new(format!(
                "{}: {}",
                t("runtime_tasks.trace_witness_fingerprint", lang),
                benchmark_fingerprint
            ))
            .small()
            .monospace()
            .color(palette::text_dim(night)),
        );
    }
    if let Some(scorecard) = &witness.scorecard {
        let rendered =
            serde_json::to_string_pretty(scorecard).unwrap_or_else(|_| scorecard.to_string());
        ui.label(
            RichText::new(t("runtime_tasks.trace_witness_scorecard", lang))
                .small()
                .color(palette::ACCENT),
        );
        ui.label(
            RichText::new(rendered)
                .small()
                .monospace()
                .color(palette::text_dim(night)),
        );
    }
    if !witness.notes.is_empty() {
        ui.label(
            RichText::new(t("runtime_tasks.trace_witness_notes", lang))
                .small()
                .color(palette::ACCENT),
        );
        for note in &witness.notes {
            ui.label(
                RichText::new(format!("- {}", note))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    }
}

pub(super) fn render_context_budget_diagnostics_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    trace: &benshu_telemetry::RunTrace,
) {
    let night = panel.state.night_mode;
    let prompt_tokens = trace.prompt_tokens.unwrap_or_default();
    let completion_tokens = trace.completion_tokens.unwrap_or_default();
    let total_tokens = prompt_tokens + completion_tokens;
    let runtime_profile = panel
        .state
        .selected_session_delegation_trace
        .as_ref()
        .and_then(|trace| trace.runtime_profile.as_deref());
    let policy_decision = panel
        .state
        .selected_witness_log
        .as_ref()
        .and_then(|log| log.policy_decision.as_deref());
    let fallback_reason = panel
        .state
        .selected_witness_log
        .as_ref()
        .and_then(|log| log.fallback_reason.as_deref());

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(
            RichText::new("Context & Budgeting")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "prompt_tokens",
                &prompt_tokens.to_string(),
                palette::INFO,
            );
            render_trace_chip(
                ui,
                night,
                "completion_tokens",
                &completion_tokens.to_string(),
                palette::INFO,
            );
            render_trace_chip(
                ui,
                night,
                "total_tokens",
                &total_tokens.to_string(),
                palette::ACCENT,
            );
            if let Some(profile) = runtime_profile {
                render_trace_chip(ui, night, "runtime_profile", profile, palette::INFO);
            }
        });

        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "budget_surface",
                if trace_metadata_is_true(trace, "subagent_budget_surface_note_complete") {
                    "complete"
                } else if trace_metadata_is_true(trace, "subagent_budget_surface_note_present") {
                    "partial"
                } else {
                    "missing"
                },
                if trace_metadata_is_true(trace, "subagent_budget_surface_note_complete") {
                    palette::SUCCESS
                } else {
                    palette::WARNING
                },
            );
            render_trace_chip(
                ui,
                night,
                "memory_session",
                if trace_metadata_is_true(trace, "memory_session_contract_complete") {
                    "complete"
                } else {
                    "partial"
                },
                if trace_metadata_is_true(trace, "memory_session_contract_complete") {
                    palette::SUCCESS
                } else {
                    palette::WARNING
                },
            );
            render_trace_chip(
                ui,
                night,
                "delegation",
                trace_metadata(trace, "delegation_present").unwrap_or("n/a"),
                palette::text_dim(night),
            );
            render_trace_chip(
                ui,
                night,
                "handover",
                trace_metadata(trace, "handover_present").unwrap_or("n/a"),
                palette::text_dim(night),
            );
        });

        ui.horizontal_wrapped(|ui| {
            for (label, key) in [("parallel_tools", "max_parallel_tools")] {
                if let Some(value) = trace_metadata(trace, key) {
                    render_trace_chip(ui, night, label, value, palette::text_dim(night));
                }
            }
        });

        if let Some(policy_decision) = policy_decision {
            ui.label(
                RichText::new(format!("Policy decision: {}", policy_decision))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
        if let Some(fallback_reason) = fallback_reason {
            ui.label(
                RichText::new(format!("Fallback reason: {}", fallback_reason))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    });
}

pub(super) fn render_continuation_runtime_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    trace: &benshu_telemetry::RunTrace,
) {
    let night = panel.state.night_mode;
    let keys = [
        ("mode", "provider_continuation_mode"),
        ("source", "provider_continuation_cache_source"),
        ("prompt", "provider_continuation_prompt_tokens"),
        ("miss", "provider_continuation_miss_reason"),
        ("frontier", "runtime_continuation_frontier_id"),
        (
            "prompt_fp",
            "runtime_continuation_visible_prompt_fingerprint",
        ),
    ];
    let present = keys
        .iter()
        .any(|(_, key)| trace_metadata(trace, key).is_some());
    if !present {
        return;
    }

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(
            RichText::new("Continuation Runtime")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            for (label, key) in keys {
                if let Some(value) = trace_metadata(trace, key) {
                    let tint = match key {
                        "provider_continuation_cache_source" if value.contains("context") => {
                            palette::SUCCESS
                        }
                        "provider_continuation_miss_reason" => palette::WARNING,
                        _ => palette::text_dim(night),
                    };
                    render_trace_chip(ui, night, label, value, tint);
                }
            }
        });
    });
}

pub(super) fn render_background_continuity_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    trace: &benshu_telemetry::RunTrace,
) {
    let night = panel.state.night_mode;
    let background_present = trace_metadata(trace, "background_present");
    let background_revision = trace_metadata(trace, "background_revision");
    let background_previous_revision = trace_metadata(trace, "background_previous_revision");
    let background_quality_signal = trace_metadata(trace, "background_quality_signal");
    let background_update_reason = trace_metadata(trace, "background_update_reason");
    let background_decision = trace_metadata(trace, "background_decision");
    let background_used_slm = trace_metadata(trace, "background_used_slm");
    let background_contract_complete =
        trace_metadata_is_true(trace, "background_contract_complete");
    let background_total_attempts = trace_metadata(trace, "background_total_attempts");
    let background_skip_count = trace_metadata(trace, "background_skip_count");
    let background_reject_count = trace_metadata(trace, "background_reject_count");
    let background_refresh_session_count =
        trace_metadata(trace, "background_refresh_session_count");
    let background_promote_relationship_count =
        trace_metadata(trace, "background_promote_relationship_count");
    let background_rewrite_count = trace_metadata(trace, "background_rewrite_count");
    let background_session_persistence_status =
        trace_metadata(trace, "background_session_persistence_status");
    let background_durable_promotion_status =
        trace_metadata(trace, "background_durable_promotion_status");
    let background_durable_promotion_pending =
        trace_metadata(trace, "background_durable_promotion_pending");
    let background_review_reason = trace_metadata(trace, "background_review_reason");
    let background_review_source = trace_metadata(trace, "background_review_source");
    let background_source_ref_count = trace_metadata(trace, "background_source_ref_count");

    if background_present.is_none()
        && background_revision.is_none()
        && background_quality_signal.is_none()
        && background_decision.is_none()
        && background_total_attempts.is_none()
        && background_session_persistence_status.is_none()
        && background_durable_promotion_status.is_none()
    {
        return;
    }

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(
            RichText::new("Background Continuity")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "present",
                background_present.unwrap_or("unknown"),
                if background_present == Some("true") {
                    palette::SUCCESS
                } else {
                    palette::WARNING
                },
            );
            if let Some(value) = background_revision {
                render_trace_chip(ui, night, "revision", value, palette::ACCENT);
            }
            if let Some(value) = background_previous_revision {
                render_trace_chip(ui, night, "prev", value, palette::text_dim(night));
            }
            if let Some(value) = background_quality_signal {
                render_trace_chip(ui, night, "quality", value, palette::INFO);
            }
            if let Some(value) = background_decision {
                render_trace_chip(ui, night, "decision", value, palette::ACCENT);
            }
        });

        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "contract",
                if background_contract_complete {
                    "complete"
                } else {
                    "partial"
                },
                if background_contract_complete {
                    palette::SUCCESS
                } else {
                    palette::WARNING
                },
            );
            if let Some(value) = background_session_persistence_status {
                render_trace_chip(ui, night, "session", value, palette::INFO);
            }
            if let Some(value) = background_durable_promotion_status {
                render_trace_chip(ui, night, "durable", value, palette::INFO);
            }
            if let Some(value) = background_review_reason {
                render_trace_chip(ui, night, "review", value, palette::WARNING);
            }
            if let Some(value) = background_durable_promotion_pending {
                render_trace_chip(ui, night, "pending", value, palette::text_dim(night));
            }
            if let Some(value) = background_used_slm {
                render_trace_chip(ui, night, "slm", value, palette::text_dim(night));
            }
            if let Some(value) = background_source_ref_count {
                render_trace_chip(ui, night, "sources", value, palette::ACCENT);
            }
        });

        ui.horizontal_wrapped(|ui| {
            if let Some(value) = background_total_attempts {
                render_trace_chip(ui, night, "attempts", value, palette::ACCENT);
            }
            if let Some(value) = background_skip_count {
                render_trace_chip(ui, night, "skip", value, palette::text_dim(night));
            }
            if let Some(value) = background_reject_count {
                render_trace_chip(ui, night, "reject", value, palette::WARNING);
            }
            if let Some(value) = background_refresh_session_count {
                render_trace_chip(ui, night, "refresh", value, palette::INFO);
            }
            if let Some(value) = background_promote_relationship_count {
                render_trace_chip(ui, night, "promote", value, palette::INFO);
            }
            if let Some(value) = background_rewrite_count {
                render_trace_chip(ui, night, "rewrite", value, palette::INFO);
            }
        });

        if let Some(value) = background_update_reason {
            ui.label(
                RichText::new(format!("Update reason: {value}"))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
        if let Some(value) = background_review_source {
            ui.label(
                RichText::new(format!("Review source: {value}"))
                    .small()
                    .color(palette::text_dim(night)),
            );
        }
    });
}

pub(super) fn render_runtime_governance_status_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    trace: &benshu_telemetry::RunTrace,
) {
    let night = panel.state.night_mode;
    let clarification_status = trace_metadata(trace, "clarification_status_kind");
    let clarification_prompt = trace_metadata(trace, "clarification_prompt");
    let visible_owner = trace_metadata(trace, "visible_owner");
    let memory_owner = trace_metadata(trace, "memory_owner");
    let approval_owner = trace_metadata(trace, "approval_owner");
    let truth_status = trace_metadata(trace, benshu_telemetry::runtime_contract::META_TRUTH_STATUS);
    let verification_domain = trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_DOMAIN,
    );
    let verification_mode = trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_MODE,
    );
    let verification_outcome = trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_OUTCOME,
    );
    let verification_sources = parse_verification_sources_json(trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_SOURCES_JSON,
    ));
    let verification_execution_evidence = parse_verification_string_list(trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_EXECUTION_EVIDENCE_JSON,
    ));
    let verification_state_evidence = parse_verification_string_list(trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_STATE_EVIDENCE_JSON,
    ));
    let source_posture = trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_SOURCE_POSTURE,
    );
    let verification_last_tool = trace_metadata(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_LAST_TOOL,
    );
    let truth_verification_guidance_active = trace_metadata_is_true(
        trace,
        benshu_telemetry::runtime_contract::META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE,
    );
    let verification_complete = trace_metadata_is_true(
        trace,
        benshu_telemetry::runtime_contract::META_VERIFICATION_SURFACE_NOTE_COMPLETE,
    );
    let runtime_evidence_complete =
        trace_metadata_is_true(trace, "runtime_evidence_contract_complete");
    let degraded = !trace.degradation_notes.is_empty()
        || panel
            .state
            .selected_witness_log
            .as_ref()
            .map(|log| log.degraded)
            .unwrap_or(false);
    let budget_exhausted = panel
        .state
        .selected_witness_log
        .as_ref()
        .map(|log| log.budget_exhausted)
        .unwrap_or(false);

    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(
            RichText::new("Runtime Governance")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "visible",
                visible_owner.unwrap_or("n/a"),
                palette::INFO,
            );
            render_trace_chip(
                ui,
                night,
                "memory",
                memory_owner.unwrap_or("n/a"),
                palette::INFO,
            );
            render_trace_chip(
                ui,
                night,
                "approval",
                approval_owner.unwrap_or("n/a"),
                palette::INFO,
            );
            render_trace_chip(
                ui,
                night,
                "clarification",
                clarification_status.unwrap_or("none"),
                if clarification_status.is_some() {
                    palette::ACCENT
                } else {
                    palette::text_dim(night)
                },
            );
            render_trace_chip(
                ui,
                night,
                "runtime_evidence",
                if runtime_evidence_complete {
                    "complete"
                } else {
                    "partial"
                },
                if runtime_evidence_complete {
                    palette::SUCCESS
                } else {
                    palette::WARNING
                },
            );
        });

        if let Some(prompt) = clarification_prompt {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Clarification prompt")
                    .small()
                    .color(palette::ACCENT),
            );
            ui.label(
                RichText::new(prompt)
                    .small()
                    .color(palette::text_dim(night)),
            );
        }

        if truth_status.is_some()
            || verification_outcome.is_some()
            || verification_mode.is_some()
            || source_posture.is_some()
            || !verification_execution_evidence.is_empty()
            || !verification_state_evidence.is_empty()
        {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Truth & Verification")
                    .small()
                    .color(palette::ACCENT),
            );
            ui.horizontal_wrapped(|ui| {
                if let Some(value) = truth_status {
                    render_truth_status_chip(ui, night, "truth", value);
                }
                if let Some(value) = verification_domain {
                    render_trace_chip(ui, night, "domain", value, palette::INFO);
                }
                if let Some(value) = verification_mode {
                    render_trace_chip(ui, night, "mode", value, palette::INFO);
                }
                if let Some(value) = verification_outcome {
                    render_verification_outcome_chip(ui, night, "outcome", value);
                }
                if let Some(value) = source_posture {
                    render_trace_chip(ui, night, "sources", value, palette::ACCENT);
                }
                if let Some(value) = verification_last_tool {
                    render_trace_chip(ui, night, "verified_by", value, palette::INFO);
                }
                if !verification_execution_evidence.is_empty() {
                    render_trace_chip(
                        ui,
                        night,
                        "execution_evidence",
                        &verification_execution_evidence.len().to_string(),
                        palette::ACCENT,
                    );
                }
                if !verification_state_evidence.is_empty() {
                    render_trace_chip(
                        ui,
                        night,
                        "state_evidence",
                        &verification_state_evidence.len().to_string(),
                        palette::ACCENT,
                    );
                }
                render_trace_chip(
                    ui,
                    night,
                    "prompt_guidance",
                    if truth_verification_guidance_active {
                        "active"
                    } else {
                        "inactive"
                    },
                    if truth_verification_guidance_active {
                        palette::SUCCESS
                    } else {
                        palette::text_dim(night)
                    },
                );
                render_trace_chip(
                    ui,
                    night,
                    "verification_contract",
                    if verification_complete {
                        "complete"
                    } else {
                        "partial"
                    },
                    if verification_complete {
                        palette::SUCCESS
                    } else {
                        palette::WARNING
                    },
                );
            });
            render_verification_sources_block(ui, night, "Observed Sources", &verification_sources);
            render_verification_string_list_block(
                ui,
                night,
                "Execution Evidence",
                &verification_execution_evidence,
            );
            render_verification_string_list_block(
                ui,
                night,
                "State Evidence",
                &verification_state_evidence,
            );
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            render_trace_chip(
                ui,
                night,
                "degraded",
                if degraded { "yes" } else { "no" },
                if degraded {
                    palette::WARNING
                } else {
                    palette::SUCCESS
                },
            );
            render_trace_chip(
                ui,
                night,
                "budget",
                if budget_exhausted { "exhausted" } else { "ok" },
                if budget_exhausted {
                    palette::WARNING
                } else {
                    palette::SUCCESS
                },
            );
            render_trace_chip(
                ui,
                night,
                "tools",
                &trace.tools.len().to_string(),
                palette::ACCENT,
            );
            render_trace_chip(
                ui,
                night,
                "stages",
                &trace.stages.len().to_string(),
                palette::ACCENT,
            );
            render_trace_chip(
                ui,
                night,
                "artifacts",
                &trace.artifacts.len().to_string(),
                palette::ACCENT,
            );
        });
    });
}

pub(super) fn render_truth_verification_witness_filters(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    log: &benshu_telemetry::WitnessLogEntry,
) {
    let night = panel.state.night_mode;
    let truth_status = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_TRUTH_STATUS)
        .cloned();
    let verification_domain = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_DOMAIN)
        .cloned();
    let verification_requirement = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_REQUIREMENT)
        .cloned();
    let verification_mode = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_MODE)
        .cloned();
    let verification_outcome = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_OUTCOME)
        .cloned();
    let verification_answer_readiness = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_ANSWER_READINESS)
        .cloned();
    let verification_route_reason = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_ROUTE_REASON)
        .cloned();
    let verification_continuation = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_CONTINUATION)
        .cloned();
    let verification_termination = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_TERMINATION)
        .cloned();
    let verification_requires_followup = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_REQUIRES_FOLLOWUP)
        .cloned();
    let verification_can_finalize_answer = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_CAN_FINALIZE_ANSWER)
        .cloned();
    let verification_next_tools = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_NEXT_TOOLS)
        .cloned();
    let verification_cite_required = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_CITE_REQUIRED)
        .cloned();
    let verification_sources = parse_verification_sources_json(
        log.metadata
            .get(benshu_telemetry::runtime_contract::META_VERIFICATION_SOURCES_JSON)
            .map(String::as_str),
    );
    let verification_execution_evidence = parse_verification_string_list(
        log.metadata
            .get(benshu_telemetry::runtime_contract::META_VERIFICATION_EXECUTION_EVIDENCE_JSON)
            .map(String::as_str),
    );
    let verification_state_evidence = parse_verification_string_list(
        log.metadata
            .get(benshu_telemetry::runtime_contract::META_VERIFICATION_STATE_EVIDENCE_JSON)
            .map(String::as_str),
    );
    let source_posture = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_SOURCE_POSTURE)
        .cloned();
    let verification_last_tool = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_VERIFICATION_LAST_TOOL)
        .cloned();
    let truth_verification_guidance_active = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE)
        .cloned();

    if truth_status.is_none()
        && verification_domain.is_none()
        && verification_requirement.is_none()
        && verification_mode.is_none()
        && verification_outcome.is_none()
        && verification_answer_readiness.is_none()
        && verification_route_reason.is_none()
        && verification_continuation.is_none()
        && verification_termination.is_none()
        && verification_requires_followup.is_none()
        && verification_can_finalize_answer.is_none()
        && verification_next_tools.is_none()
        && verification_cite_required.is_none()
        && verification_sources.is_empty()
        && verification_execution_evidence.is_empty()
        && verification_state_evidence.is_empty()
        && source_posture.is_none()
        && verification_last_tool.is_none()
        && truth_verification_guidance_active.is_none()
    {
        return;
    }

    ui.add_space(6.0);
    ui.collapsing("Truth & Verification Filters", |ui| {
        render_truth_verification_filter_row(
            panel,
            ui,
            night,
            &log.suite_id,
            truth_status,
            verification_domain,
            verification_requirement,
            verification_mode,
            verification_outcome,
            verification_answer_readiness,
            verification_route_reason,
            verification_continuation,
            verification_termination,
            verification_requires_followup,
            verification_can_finalize_answer,
            verification_next_tools,
            verification_cite_required,
            verification_sources,
            verification_execution_evidence,
            verification_state_evidence,
            source_posture,
            verification_last_tool,
            truth_verification_guidance_active,
        );
        render_truth_verification_query_results(panel, ui, night);
    });
}

pub(super) fn render_truth_verification_filter_row(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    night: bool,
    suite_id: &str,
    truth_status: Option<String>,
    verification_domain: Option<String>,
    verification_requirement: Option<String>,
    verification_mode: Option<String>,
    verification_outcome: Option<String>,
    verification_answer_readiness: Option<String>,
    verification_route_reason: Option<String>,
    verification_continuation: Option<String>,
    verification_termination: Option<String>,
    verification_requires_followup: Option<String>,
    verification_can_finalize_answer: Option<String>,
    verification_next_tools: Option<String>,
    verification_cite_required: Option<String>,
    verification_sources: Vec<VerificationSourceView>,
    verification_execution_evidence: Vec<String>,
    verification_state_evidence: Vec<String>,
    source_posture: Option<String>,
    verification_last_tool: Option<String>,
    truth_verification_guidance_active: Option<String>,
) {
    let failure_reason = truth_verification_failure_reason(
        verification_domain.as_deref(),
        verification_outcome.as_deref(),
    );
    let source_required_missing = verification_cite_required.as_deref() == Some("true")
        && source_posture.as_deref() == Some("SourcesRequiredButMissing");
    let local_context_only = verification_requirement.as_deref() == Some("LocalContextAllowed")
        && (verification_mode.as_deref() == Some("LocalContextOnly")
            || verification_answer_readiness.as_deref() == Some("local_context_only"));

    ui.horizontal_wrapped(|ui| {
        if let Some(value) = truth_status.as_ref() {
            render_truth_status_chip(ui, night, "Truth", value);
        }
        if let Some(value) = verification_domain.as_ref() {
            render_trace_chip(ui, night, "Domain", value, palette::INFO);
        }
        if let Some(value) = verification_requirement.as_ref() {
            render_trace_chip(ui, night, "Requirement", value, palette::text_dim(night));
        }
        if let Some(value) = verification_mode.as_ref() {
            render_trace_chip(ui, night, "Mode", value, palette::INFO);
        }
        if let Some(value) = verification_outcome.as_ref() {
            render_verification_outcome_chip(ui, night, "Outcome", value);
        }
        if let Some(value) = verification_answer_readiness.as_ref() {
            render_trace_chip(ui, night, "Readiness", value, palette::INFO);
        }
        if let Some(value) = verification_route_reason.as_ref() {
            render_trace_chip(ui, night, "Route Reason", value, palette::INFO);
        }
        if let Some(value) = verification_continuation.as_ref() {
            render_trace_chip(ui, night, "Continuation", value, palette::ACCENT);
        }
        if let Some(value) = verification_termination.as_ref() {
            render_trace_chip(ui, night, "Termination", value, palette::SUCCESS);
        }
        if let Some(value) = verification_requires_followup.as_ref() {
            render_trace_chip(ui, night, "Follow-up", value, palette::WARNING);
        }
        if let Some(value) = verification_can_finalize_answer.as_ref() {
            render_trace_chip(ui, night, "Can Finalize", value, palette::SUCCESS);
        }
        if let Some(value) = verification_next_tools.as_ref() {
            render_trace_chip(ui, night, "Next Tool", value, palette::ACCENT);
        }
        if let Some(value) = verification_cite_required.as_ref() {
            render_trace_chip(ui, night, "Cite Required", value, palette::WARNING);
        }
        if !verification_sources.is_empty() {
            render_trace_chip(
                ui,
                night,
                "Observed Sources",
                &verification_sources.len().to_string(),
                palette::ACCENT,
            );
        }
        if !verification_execution_evidence.is_empty() {
            render_trace_chip(
                ui,
                night,
                "Execution Evidence",
                &verification_execution_evidence.len().to_string(),
                palette::ACCENT,
            );
        }
        if !verification_state_evidence.is_empty() {
            render_trace_chip(
                ui,
                night,
                "State Evidence",
                &verification_state_evidence.len().to_string(),
                palette::ACCENT,
            );
        }
        if let Some(value) = source_posture.as_ref() {
            render_trace_chip(ui, night, "Sources", value, palette::ACCENT);
        }
        if let Some(value) = verification_last_tool.as_ref() {
            render_trace_chip(ui, night, "Verified By", value, palette::INFO);
        }
        if let Some(value) = truth_verification_guidance_active.as_ref() {
            render_trace_chip(ui, night, "Prompt Guidance", value, palette::SUCCESS);
        }
        if let Some(value) = failure_reason.as_ref() {
            render_trace_chip(ui, night, "Failure", value, palette::WARNING);
        }
        if local_context_only {
            render_trace_chip(
                ui,
                night,
                "Local Context",
                "Allowed but Unverified",
                palette::WARNING,
            );
        }
        if source_required_missing {
            render_trace_chip(
                ui,
                night,
                "Source Required",
                "Still Missing",
                palette::WARNING,
            );
        }
        if ui.small_button("Filter Witness Logs").clicked() {
            let query = build_truth_fields(
                truth_status.clone(),
                verification_domain.clone(),
                verification_requirement.clone(),
                verification_mode.clone(),
                verification_outcome.clone(),
                verification_answer_readiness.clone(),
                verification_route_reason.clone(),
                verification_continuation.clone(),
                verification_termination.clone(),
                verification_requires_followup.clone(),
                verification_can_finalize_answer.clone(),
                verification_next_tools.clone(),
                verification_cite_required.clone(),
                source_posture.clone(),
                verification_last_tool.clone(),
            );
            panel.state.do_witness_query_refresh(
                &panel.rt,
                ui.ctx(),
                build_truth_witness_query(suite_id, query, failure_reason.clone()),
            );
        }
        if ui.small_button("Filter Scorecards").clicked() {
            let query = build_truth_fields(
                truth_status,
                verification_domain,
                verification_requirement,
                verification_mode,
                verification_outcome,
                verification_answer_readiness,
                verification_route_reason,
                verification_continuation,
                verification_termination,
                verification_requires_followup,
                verification_can_finalize_answer,
                verification_next_tools,
                verification_cite_required,
                source_posture,
                verification_last_tool,
            );
            panel.state.do_scorecard_query_refresh(
                &panel.rt,
                ui.ctx(),
                build_truth_scorecard_query(suite_id, query, failure_reason),
            );
        }
        if local_context_only && ui.small_button("Filter Local Context").clicked() {
            refresh_truth_queries(
                panel,
                ui.ctx(),
                suite_id,
                build_truth_fields(
                    None,
                    None,
                    Some("LocalContextAllowed".to_string()),
                    Some("LocalContextOnly".to_string()),
                    None,
                    Some("local_context_only".to_string()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                None,
            );
        }
        if source_required_missing && ui.small_button("Filter Source Missing").clicked() {
            refresh_truth_queries(
                panel,
                ui.ctx(),
                suite_id,
                build_truth_fields(
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some("true".to_string()),
                    Some("SourcesRequiredButMissing".to_string()),
                    None,
                ),
                Some("verification::source_required::still_missing".to_string()),
            );
        }
    });

    render_verification_sources_block(ui, night, "Observed Sources", &verification_sources);
    render_verification_string_list_block(
        ui,
        night,
        "Execution Evidence",
        &verification_execution_evidence,
    );
    render_verification_string_list_block(
        ui,
        night,
        "State Evidence",
        &verification_state_evidence,
    );
}

pub(super) fn render_truth_verification_query_results(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    night: bool,
) {
    if panel.state.selected_witness_query_loading {
        ui.label(
            RichText::new("Loading filtered witness logs...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_witness_query_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if !panel.state.selected_witness_query_results.is_empty() {
        ui.label(
            RichText::new("Matching Witness Logs")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        for log in &panel.state.selected_witness_query_results {
            ui.horizontal_wrapped(|ui| {
                render_trace_chip(
                    ui,
                    night,
                    "Witness",
                    &log.witness_id.to_string(),
                    palette::ACCENT,
                );
                if let Some(value) = log
                    .metadata
                    .get(benshu_telemetry::runtime_contract::META_TRUTH_STATUS)
                {
                    render_truth_status_chip(ui, night, "Truth", value);
                }
                if let Some(value) = log
                    .metadata
                    .get(benshu_telemetry::runtime_contract::META_VERIFICATION_OUTCOME)
                {
                    render_verification_outcome_chip(ui, night, "Outcome", value);
                }
                if let Some(value) = log
                    .metadata
                    .get(benshu_telemetry::runtime_contract::META_VERIFICATION_LAST_TOOL)
                {
                    render_trace_chip(ui, night, "Tool", value, palette::INFO);
                }
            });
        }
    }

    if panel.state.selected_scorecard_query_loading {
        ui.label(
            RichText::new("Loading filtered scorecards...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_scorecard_query_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if !panel.state.selected_scorecard_query_results.is_empty() {
        ui.label(
            RichText::new("Matching Scorecards")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        for scorecard in &panel.state.selected_scorecard_query_results {
            ui.horizontal_wrapped(|ui| {
                render_trace_chip(
                    ui,
                    night,
                    "Scorecard",
                    &scorecard.scorecard_id,
                    palette::ACCENT,
                );
                render_trace_chip(
                    ui,
                    night,
                    "Trials",
                    &scorecard.total_trials.to_string(),
                    palette::text_dim(night),
                );
                render_trace_chip(
                    ui,
                    night,
                    "Average",
                    &format!("{:.2}", scorecard.average_score),
                    palette::text_dim(night),
                );
            });
        }
    }
}

pub(super) fn render_windows_native_witness_filters(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    log: &benshu_telemetry::WitnessLogEntry,
) {
    let night = panel.state.night_mode;
    let embed_outcome = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_WINDOWS_NATIVE_EMBED_OUTCOME)
        .cloned();
    let embed_strategy = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_WINDOWS_NATIVE_EMBED_STRATEGY)
        .cloned();
    let rerank_outcome = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_WINDOWS_NATIVE_RERANK_OUTCOME)
        .cloned();
    let rerank_strategy = log
        .metadata
        .get(benshu_telemetry::runtime_contract::META_WINDOWS_NATIVE_RERANK_STRATEGY)
        .cloned();

    if embed_outcome.is_none()
        && embed_strategy.is_none()
        && rerank_outcome.is_none()
        && rerank_strategy.is_none()
    {
        return;
    }

    ui.add_space(6.0);
    ui.collapsing("Windows-native Filters", |ui| {
        if embed_outcome.is_some() || embed_strategy.is_some() {
            render_windows_native_role_filter_row(
                panel,
                ui,
                night,
                "Embedding",
                &log.suite_id,
                embed_outcome.clone(),
                embed_strategy.clone(),
                None,
                None,
            );
        }
        if rerank_outcome.is_some() || rerank_strategy.is_some() {
            render_windows_native_role_filter_row(
                panel,
                ui,
                night,
                "Rerank",
                &log.suite_id,
                None,
                None,
                rerank_outcome.clone(),
                rerank_strategy.clone(),
            );
        }

        render_windows_native_query_results(panel, ui, night);
    });
}

fn render_windows_native_role_filter_row(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    night: bool,
    role_label: &str,
    suite_id: &str,
    embed_outcome: Option<String>,
    embed_strategy: Option<String>,
    rerank_outcome: Option<String>,
    rerank_strategy: Option<String>,
) {
    let class = windows_native_outcome_class(
        embed_outcome
            .as_ref()
            .or(rerank_outcome.as_ref())
            .map(|value| value.as_str()),
    );
    let failure_reason = windows_native_failure_reason(
        role_label,
        embed_outcome
            .as_ref()
            .or(rerank_outcome.as_ref())
            .map(|value| value.as_str()),
    );

    ui.label(
        RichText::new(role_label)
            .small()
            .strong()
            .color(palette::ACCENT),
    );
    ui.horizontal_wrapped(|ui| {
        if let Some(value) = embed_outcome.as_ref().or(rerank_outcome.as_ref()) {
            render_trace_chip(ui, night, "Outcome", value, palette::ACCENT);
        }
        if let Some(value) = embed_strategy.as_ref().or(rerank_strategy.as_ref()) {
            render_trace_chip(ui, night, "Strategy", value, palette::text_dim(night));
        }
        if let Some(value) = class.as_ref() {
            render_trace_chip(ui, night, "Class", value, palette::text_dim(night));
        }
        if let Some(value) = failure_reason.as_ref() {
            render_trace_chip(ui, night, "Failure", value, palette::WARNING);
        }
        if ui.small_button("Filter Witness Logs").clicked() {
            let query = build_windows_fields(
                embed_outcome.clone(),
                if embed_outcome.is_some() {
                    class.clone()
                } else {
                    None
                },
                embed_strategy.clone(),
                rerank_outcome.clone(),
                if rerank_outcome.is_some() {
                    class.clone()
                } else {
                    None
                },
                rerank_strategy.clone(),
                failure_reason.clone(),
            );
            panel.state.do_witness_query_refresh(
                &panel.rt,
                ui.ctx(),
                build_windows_witness_query(suite_id, query),
            );
        }
        if ui.small_button("Filter Scorecards").clicked() {
            let query = build_windows_fields(
                embed_outcome,
                if role_label == "Embedding" {
                    class.clone()
                } else {
                    None
                },
                embed_strategy,
                rerank_outcome,
                if role_label == "Rerank" {
                    class.clone()
                } else {
                    None
                },
                rerank_strategy,
                failure_reason,
            );
            panel.state.do_scorecard_query_refresh(
                &panel.rt,
                ui.ctx(),
                build_windows_scorecard_query(suite_id, query),
            );
        }
    });
}

fn build_truth_fields(
    truth_status: Option<String>,
    verification_domain: Option<String>,
    verification_requirement: Option<String>,
    verification_mode: Option<String>,
    verification_outcome: Option<String>,
    verification_answer_readiness: Option<String>,
    verification_route_reason: Option<String>,
    verification_continuation: Option<String>,
    verification_termination: Option<String>,
    verification_requires_followup: Option<String>,
    verification_can_finalize_answer: Option<String>,
    verification_next_tools: Option<String>,
    verification_cite_required: Option<String>,
    source_posture: Option<String>,
    verification_last_tool: Option<String>,
) -> benshu_telemetry::TruthVerificationQueryFields {
    benshu_telemetry::TruthVerificationQueryFields {
        truth_status,
        verification_domain,
        verification_requirement,
        verification_mode,
        verification_outcome,
        verification_answer_readiness,
        verification_route_reason,
        verification_continuation,
        verification_termination,
        verification_requires_followup,
        verification_can_finalize_answer,
        verification_next_tools,
        verification_cite_required,
        source_posture,
        verification_last_tool,
    }
}

fn truth_status_tint(value: &str, night: bool) -> Color32 {
    match value {
        "Verified" => palette::SUCCESS,
        "Inferred" | "Unverified" | "Uncertain" | "ClarificationRequired" => palette::WARNING,
        _ => palette::text_dim(night),
    }
}

fn render_truth_status_chip(ui: &mut egui::Ui, night: bool, label: &str, value: &str) {
    render_trace_chip(ui, night, label, value, truth_status_tint(value, night));
}

fn render_verification_outcome_chip(ui: &mut egui::Ui, night: bool, label: &str, value: &str) {
    render_trace_chip(
        ui,
        night,
        label,
        value,
        if value == "VerificationSucceeded" {
            palette::SUCCESS
        } else {
            palette::WARNING
        },
    );
}

fn build_windows_fields(
    windows_native_embed_outcome: Option<String>,
    windows_native_embed_class: Option<String>,
    windows_native_embed_strategy: Option<String>,
    windows_native_rerank_outcome: Option<String>,
    windows_native_rerank_class: Option<String>,
    windows_native_rerank_strategy: Option<String>,
    windows_native_failure_reason: Option<String>,
) -> benshu_telemetry::WindowsNativeQueryFields {
    benshu_telemetry::WindowsNativeQueryFields {
        windows_native_embed_outcome,
        windows_native_embed_class,
        windows_native_embed_strategy,
        windows_native_rerank_outcome,
        windows_native_rerank_class,
        windows_native_rerank_strategy,
        windows_native_failure_reason,
    }
}

fn build_truth_witness_query(
    suite_id: &str,
    truth_verification: benshu_telemetry::TruthVerificationQueryFields,
    text: Option<String>,
) -> benshu_telemetry::WitnessLogQuery {
    benshu_telemetry::WitnessLogQuery {
        suite_id: Some(suite_id.to_string()),
        truth_verification,
        text,
        limit: Some(20),
        ..Default::default()
    }
}

fn build_truth_scorecard_query(
    suite_id: &str,
    truth_verification: benshu_telemetry::TruthVerificationQueryFields,
    text: Option<String>,
) -> benshu_telemetry::ScorecardQuery {
    benshu_telemetry::ScorecardQuery {
        suite_id: Some(suite_id.to_string()),
        truth_verification,
        text,
        limit: Some(20),
        ..Default::default()
    }
}

fn refresh_truth_queries(
    panel: &mut ClawPanel,
    ctx: &egui::Context,
    suite_id: &str,
    truth_verification: benshu_telemetry::TruthVerificationQueryFields,
    text: Option<String>,
) {
    panel.state.do_witness_query_refresh(
        &panel.rt,
        ctx,
        build_truth_witness_query(suite_id, truth_verification.clone(), text.clone()),
    );
    panel.state.do_scorecard_query_refresh(
        &panel.rt,
        ctx,
        build_truth_scorecard_query(suite_id, truth_verification, text),
    );
}

fn build_windows_witness_query(
    suite_id: &str,
    windows_native: benshu_telemetry::WindowsNativeQueryFields,
) -> benshu_telemetry::WitnessLogQuery {
    benshu_telemetry::WitnessLogQuery {
        suite_id: Some(suite_id.to_string()),
        windows_native,
        limit: Some(20),
        ..Default::default()
    }
}

fn build_windows_scorecard_query(
    suite_id: &str,
    windows_native: benshu_telemetry::WindowsNativeQueryFields,
) -> benshu_telemetry::ScorecardQuery {
    benshu_telemetry::ScorecardQuery {
        suite_id: Some(suite_id.to_string()),
        windows_native,
        limit: Some(20),
        ..Default::default()
    }
}

fn windows_native_outcome_class(outcome: Option<&str>) -> Option<String> {
    let outcome = outcome?;
    if outcome.is_empty()
        || matches!(
            outcome,
            "windows_native_active" | "active" | "not_observed" | "not_reported"
        )
    {
        return None;
    }
    let class = match outcome {
        "cpu_fallback_provider_downgrade" => "provider_downgrade",
        "cpu_fallback_no_accelerator_route" => "no_accelerator_route",
        "cpu_fallback_active" => "cpu_fallback",
        "windows_native_provider_execution_failed" => "provider_failure",
        "windows_native_execution_failed" => "runtime_failure",
        "fallback_runtime_active" | "migrate_to_windows_native_runtime" => "fallback_runtime",
        "backend_unlinked" | "runtime_missing" | "validation_only" => "pending_runtime",
        "model_contract_incompatible" => "contract_incompatible",
        "accelerator_resource_exhausted" => "resource_exhausted",
        "accelerator_unavailable" => "accelerator_unavailable",
        _ => "other",
    };
    Some(class.to_string())
}

fn windows_native_failure_reason(role_label: &str, outcome: Option<&str>) -> Option<String> {
    let role = match role_label {
        "Embedding" => "embed",
        "Rerank" => "rerank",
        _ => return None,
    };
    let outcome = outcome?;
    if outcome.is_empty()
        || matches!(
            outcome,
            "windows_native_active" | "active" | "not_observed" | "not_reported"
        )
    {
        return None;
    }
    Some(format!("windows_native::{role}::{outcome}"))
}

fn render_windows_native_query_results(panel: &mut ClawPanel, ui: &mut egui::Ui, night: bool) {
    if panel.state.selected_witness_query_loading {
        ui.label(
            RichText::new("Loading filtered witness logs...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_witness_query_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if !panel.state.selected_witness_query_results.is_empty() {
        ui.label(
            RichText::new("Matching Witness Logs")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        for log in &panel.state.selected_witness_query_results {
            ui.horizontal_wrapped(|ui| {
                render_trace_chip(
                    ui,
                    night,
                    "Witness",
                    &log.witness_id.to_string(),
                    palette::ACCENT,
                );
                render_trace_chip(ui, night, "Verdict", &log.verdict, palette::text_dim(night));
                render_trace_chip(
                    ui,
                    night,
                    "Scenario",
                    &log.scenario,
                    palette::text_dim(night),
                );
            });
        }
    }

    if panel.state.selected_scorecard_query_loading {
        ui.label(
            RichText::new("Loading filtered scorecards...")
                .small()
                .color(palette::text_dim(night)),
        );
    } else if let Some(error) = &panel.state.selected_scorecard_query_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    } else if !panel.state.selected_scorecard_query_results.is_empty() {
        ui.label(
            RichText::new("Matching Scorecards")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        for scorecard in &panel.state.selected_scorecard_query_results {
            ui.horizontal_wrapped(|ui| {
                render_trace_chip(
                    ui,
                    night,
                    "Scorecard",
                    &scorecard.scorecard_id,
                    palette::ACCENT,
                );
                render_trace_chip(
                    ui,
                    night,
                    "Trials",
                    &scorecard.total_trials.to_string(),
                    palette::text_dim(night),
                );
                render_trace_chip(
                    ui,
                    night,
                    "Average",
                    &format!("{:.2}", scorecard.average_score),
                    palette::text_dim(night),
                );
            });
        }
    }
}

fn truth_verification_failure_reason(
    verification_domain: Option<&str>,
    verification_outcome: Option<&str>,
) -> Option<String> {
    let domain = match verification_domain? {
        "KnowledgeFact" => "knowledge_fact",
        "ToolFact" => "tool_fact",
        "ExecutionFact" => "execution_fact",
        "StateFact" => "state_fact",
        other => {
            return Some(format!(
                "verification::{}::unknown_outcome",
                other.to_lowercase()
            ))
        }
    };
    let outcome = verification_outcome?;
    if outcome.is_empty() || outcome == "VerificationSucceeded" {
        return None;
    }
    Some(format!(
        "verification::{domain}::{}",
        to_snake_case(outcome)
    ))
}

fn to_snake_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut prev_was_sep = false;
    for (idx, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 && !prev_was_sep {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_was_sep = false;
        } else if ch == '-' || ch == ' ' {
            if !prev_was_sep && !out.is_empty() {
                out.push('_');
            }
            prev_was_sep = true;
        } else {
            out.push(ch);
            prev_was_sep = ch == '_';
        }
    }
    out
}
