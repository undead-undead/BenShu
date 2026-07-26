use crate::app::ClawPanel;
use crate::app_state::{ChatAttachmentDraft, ChatMessage};
use crate::common::palette;
use crate::ui::open_target;
use eframe::egui::{self, Color32, RichText, Stroke};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

pub fn render_chat_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        panel.state.chat_selected_role = "benshu".to_string();
        let current_role = "benshu".to_string();
        let current_session = panel
            .state
            .active_chat_session
            .get(&current_role)
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        maybe_refresh_chat_task_progress(panel, ctx, &current_session);

        ui.label(
            RichText::new("CHAT WITH BenShu")
                .small()
                .strong()
                .color(palette::ACCENT),
        );
        ui.label(
            RichText::new(
                "Specialist execution is delegated in the background. Use A2A Records and runtime evidence to inspect delegation and feedback."
            )
            .small()
            .color(palette::text_dim(panel.state.night_mode)),
        );
        ui.horizontal_wrapped(|ui| {
            let mut thinking_enabled = panel.state.llama_reasoning_mode != "off";
            let response = ui
                .checkbox(&mut thinking_enabled, "Local model thinking")
                .on_hover_text(
                    "Shortcut for Llama.cpp Runtime reasoning mode. Turning it off reduces hidden thinking output and restarts the local runtime after save.",
                );
            if response.changed() {
                if thinking_enabled {
                    panel.state.llama_reasoning_mode = "auto".to_string();
                    panel.state.llama_reasoning_format = "auto".to_string();
                    panel.state.llama_reasoning_budget.clear();
                } else {
                    panel.state.llama_reasoning_mode = "off".to_string();
                    panel.state.llama_reasoning_format = "none".to_string();
                    panel.state.llama_reasoning_budget.clear();
                    panel.state.llama_reasoning_budget_message.clear();
                }
                panel
                    .state
                    .do_save_llama_cpp_runtime_settings(&panel.rt, ctx);
            }
            ui.label(
                RichText::new(format!(
                    "mode: {} / {}",
                    panel.state.llama_reasoning_mode, panel.state.llama_reasoning_format
                ))
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
            );
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Session Tabs & Agent Selection
        ui.horizontal(|ui| {
            let current_role = "benshu".to_string();

            // Ensure entry exists
            if !panel.state.chat_sessions.contains_key(&current_role) {
                panel
                    .state
                    .chat_sessions
                    .insert(current_role.clone(), vec!["default".to_string()]);
            }
            if !panel.state.active_chat_session.contains_key(&current_role) {
                panel
                    .state
                    .active_chat_session
                    .insert(current_role.clone(), "default".to_string());
            }

            let mut switch_to = None;
            let mut add_new = false;
            let mut clear_clicked = false;

            let active_session_val = panel
                .state
                .active_chat_session
                .get(&current_role)
                .cloned()
                .unwrap_or_else(|| "default".to_string());
            let sessions_list = panel
                .state
                .chat_sessions
                .get(&current_role)
                .cloned()
                .unwrap_or_else(|| vec!["default".to_string()]);

            // Session Switcher Tabs
            egui::ScrollArea::horizontal()
                .id_salt("chat_sessions_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, session) in sessions_list.iter().enumerate() {
                            let is_active = active_session_val == *session;
                            let res =
                                ui.selectable_label(is_active, RichText::new(session).strong());
                            if res.clicked() {
                                switch_to = Some(session.clone());
                            }
                            if res.hovered() {
                                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                            }
                            // Optional: delete session
                            if is_active && session != "default" {
                                if ui
                                    .small_button("x")
                                    .on_hover_text("Delete mission")
                                    .clicked()
                                {
                                    panel.state.pending_delete_session =
                                        Some((current_role.clone(), i));
                                }
                            }
                        }

                        if ui.button("+").on_hover_text("New Mission").clicked() {
                            add_new = true;
                        }
                    });
                });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear History").clicked() {
                    clear_clicked = true;
                }
            });

            // Execute deferred actions (switch/add only, delete via confirmation)
            if let Some(target) = switch_to {
                panel
                    .state
                    .active_chat_session
                    .insert(current_role.clone(), target.clone());
                panel.state.do_load_session_history(&panel.rt, ctx, target);
            }
            if add_new {
                if let Some(sessions) = panel.state.chat_sessions.get_mut(&current_role) {
                    let new_id = format!("mission-{}", sessions.len());
                    sessions.push(new_id.clone());
                    panel
                        .state
                        .active_chat_session
                        .insert(current_role.clone(), new_id.clone());
                    panel
                        .state
                        .do_load_session_history(&panel.rt, ctx, "default".to_string());
                }
            }
            if clear_clicked {
                let history_key = format!("{}:{}", active_session_val, current_role);
                panel.state.chat_histories.remove(&history_key);
            }
        });
        ui.add_space(8.0);
        ui.separator();

        // Use bottom_up to pin input to the bottom
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);

            // Input Area at the very bottom
            ui.horizontal(|ui| {
                let text_edit = egui::TextEdit::singleline(&mut panel.state.chat_input)
                    .hint_text("Type a message, or attach a file for this turn...")
                    .desired_width(ui.available_width() - 250.0);

                let response = ui.add(text_edit);
                if response.has_focus()
                    && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V))
                {
                    match try_clipboard_image_attachment() {
                        ClipboardImagePaste::Attached(draft) => {
                            if !panel
                                .state
                                .chat_attachments
                                .iter()
                                .any(|existing| existing.path == draft.path)
                            {
                                let display_name = draft.display_name.clone();
                                panel.state.chat_attachments.push(draft);
                                panel.state.set_status(
                                    format!("Attached clipboard screenshot: {display_name}"),
                                    false,
                                );
                            }
                        }
                        ClipboardImagePaste::NoImage => {}
                        ClipboardImagePaste::Failed(error) => {
                            panel
                                .state
                                .set_status(format!("Clipboard image paste failed: {error}"), true);
                        }
                    }
                }
                ui.add_space(4.0);

                if ui
                    .button("Attach...")
                    .on_hover_text(
                        "Attach files for this chat turn only. They are parsed as temporary context and are not imported into Knowledge.",
                    )
                    .clicked()
                {
                    if let Some(paths) = rfd::FileDialog::new()
                        .add_filter(
                            "Supported attachments",
                            &[
                                "png", "jpg", "jpeg", "webp", "bmp", "gif", "pdf", "docx",
                                "xlsx", "pptx", "txt", "md", "json", "rs", "toml", "yaml",
                                "yml", "js", "ts", "tsx", "py", "sh", "html", "css", "xml",
                                "csv", "mp3", "wav", "ogg", "m4a", "flac", "aac", "opus",
                                "mp4", "mov", "avi", "mkv", "webm",
                            ],
                        )
                        .pick_files()
                    {
                        for path in paths {
                            let draft = ChatAttachmentDraft::from_path(path);
                            if !panel
                                .state
                                .chat_attachments
                                .iter()
                                .any(|existing| existing.path == draft.path)
                            {
                                panel.state.chat_attachments.push(draft);
                            }
                        }
                    }
                }

                if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button("  Send  ").clicked()
                {
                    let current_role = "benshu".to_string();
                    let session = panel
                        .state
                        .active_chat_session
                        .get(&current_role)
                        .cloned()
                        .unwrap_or_else(|| "default".to_string());
                    panel.state.do_chat_send(&panel.rt, ctx, session);
                    response.request_focus();
                }

                // Phase 11-B: Red STOP button for task cancellation
                if panel.state.chat_loading {
                    let stop_btn =
                        egui::Button::new(RichText::new("🛑 STOP").color(Color32::WHITE).strong())
                            .fill(Color32::from_rgb(200, 40, 40));

                    if ui.add(stop_btn).clicked() {
                        let current_role = "benshu".to_string();
                        let session = panel
                            .state
                            .active_chat_session
                            .get(&current_role)
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                        panel.state.do_cancel_chat_task(&panel.rt, ctx, session);
                    }
                }

                // Poll cancel promise
                if let Some(promise) = panel.state.cancel_promise.take() {
                    match promise.try_take() {
                        Ok(Ok(())) => {
                            panel.state.set_status("Task cancelled.", false);
                            panel.state.chat_loading = false;
                        }
                        Ok(Err(e)) => {
                            panel.state.set_status(format!("Cancel error: {}", e), true);
                        }
                        Err(promise) => {
                            panel.state.cancel_promise = Some(promise);
                        }
                    }
                }
            });

            if !panel.state.chat_attachments.is_empty() {
                ui.add_space(6.0);
                let mut remove_index = None;
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Turn attachments")
                            .small()
                            .strong()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                    for (idx, attachment) in panel.state.chat_attachments.iter().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{}: {}",
                                        attachment.media_type, attachment.display_name
                                    ))
                                    .small()
                                    .color(palette::text_bright(panel.state.night_mode)),
                                )
                                .on_hover_text(&attachment.path);
                                if ui.small_button("x").on_hover_text("Remove attachment").clicked()
                                {
                                    remove_index = Some(idx);
                                }
                            });
                        });
                    }
                });
                if let Some(idx) = remove_index {
                    panel.state.chat_attachments.remove(idx);
                }
                ui.label(
                    RichText::new("These files are only sent as temporary context for this message; Knowledge Import is separate.")
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            }

            ui.add_space(8.0);

            let history_key = format!("{}:{}", current_session, current_role);
            let mut rollback_target = None;
            if let Some(history) = panel.state.chat_histories.get(&history_key).cloned() {
                render_virtual_chat_history(panel, ui, &history, &mut rollback_target);
            }

            if let Some((orig, bak)) = rollback_target {
                panel.state.do_rollback(&panel.rt, ctx, orig, bak);
            }

            render_chat_task_progress(panel, ui, &current_session);
        });
    });
}

fn render_virtual_chat_history(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    history: &[ChatMessage],
    rollback_target: &mut Option<(String, String)>,
) {
    const ROW_HEIGHT: f32 = 228.0;
    ui.spacing_mut().item_spacing.y = 8.0;
    egui::ScrollArea::vertical()
        .id_salt("chat_history")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show_rows(ui, ROW_HEIGHT, history.len(), |ui, row_range| {
            for idx in row_range {
                if let Some(msg) = history.get(idx) {
                    ui.set_min_height(ROW_HEIGHT - 8.0);
                    render_chat_message_bubble(panel, ui, msg, idx, rollback_target);
                    ui.add_space(8.0);
                }
            }
        });
}

fn render_chat_message_bubble(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    idx: usize,
    rollback_target: &mut Option<(String, String)>,
) {
    let is_user = msg.role == "user";
    let bg = if is_user {
        palette::bg_deep(panel.state.night_mode).gamma_multiply(1.5)
    } else {
        panel.theme_bg_deep()
    };

    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            if let Some(name) = &msg.agent_name {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("🤖 {}", name))
                            .small()
                            .color(palette::ACCENT)
                            .strong(),
                    );
                    if let Some(route) = &msg.chat_route {
                        let route_color = if route == "fast" {
                            palette::SUCCESS
                        } else {
                            palette::WARNING
                        };
                        ui.label(
                            RichText::new(format!(" {} ", route.to_uppercase()))
                                .small()
                                .color(route_color)
                                .strong(),
                        );
                    }
                    if let Some(mode) = &msg.tool_surface_mode {
                        ui.label(
                            RichText::new(format!("tools:{}", mode))
                                .small()
                                .color(palette::text_dim(panel.state.night_mode)),
                        );
                    }
                });
            } else if is_user {
                ui.label(
                    RichText::new("👤 YOU")
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            }

            if let Some(thought) = &msg.reasoning {
                egui::CollapsingHeader::new(
                    RichText::new("💭 Thought Process")
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                )
                .id_salt(format!("thought_{idx}"))
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(chat_display_preview(thought, 1_200))
                            .italics()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                });
            }

            ui.horizontal_wrapped(|ui| {
                if let Some(status) = &msg.runtime_persistence_status {
                    ui.label(
                        RichText::new(format!("persist:{}", status))
                            .small()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                }
                if let Some(task_id) = &msg.task_id {
                    ui.label(
                        RichText::new(format!("task: {}", task_id))
                            .small()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                }
            });

            if !msg.tool_calls.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for tool in &msg.tool_calls {
                        let (icon, color) = if tool.result.is_some() {
                            ("✓", palette::SUCCESS)
                        } else {
                            ("⚙", palette::WARNING)
                        };
                        ui.label(
                            RichText::new(format!(" {} {} ", icon, tool.name))
                                .small()
                                .color(color)
                                .strong(),
                        );

                        if let Some(bak) = &tool.backup {
                            if ui
                                .button(RichText::new("↩ Undo").small().color(palette::WARNING))
                                .on_hover_text(format!("Rollback changes to {}", bak.original_path))
                                .clicked()
                            {
                                *rollback_target =
                                    Some((bak.original_path.clone(), bak.backup_path.clone()));
                            }
                        }
                    }
                });
            }

            if is_creation_contract_message(msg) {
                render_creation_contract_card(panel, ui, msg, idx);
            } else {
                let preview = chat_display_preview(&msg.content, 1_200);
                render_markdown_text(panel, ui, &preview);
            }
            render_chat_artifact_buttons(panel, ui, msg);
            open_target::render_open_targets_from_text(panel, ui, &msg.content);
        });
}

fn is_creation_contract_message(msg: &ChatMessage) -> bool {
    msg.tool_surface_mode.as_deref() == Some("creation_contract")
}

fn render_creation_contract_card(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    idx: usize,
) {
    ui.add_space(4.0);
    ui.label(
        RichText::new("Writing Contract Draft")
            .small()
            .strong()
            .color(palette::ACCENT),
    );
    ui.label(
        RichText::new("确认前不会写正文。可以直接用自然语言继续修改，或者回复“开始写”。")
            .small()
            .color(palette::text_dim(panel.state.night_mode)),
    );

    let summary = contract_summary_lines(&msg.content);
    if !summary.is_empty() {
        ui.add_space(6.0);
        for line in summary {
            ui.label(RichText::new(line).color(palette::text_bright(panel.state.night_mode)));
        }
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(RichText::new("Copy Contract").small())
            .on_hover_text("Copy the full contract text")
            .clicked()
        {
            ui.ctx().copy_text(msg.content.clone());
        }
    });

    egui::CollapsingHeader::new(
        RichText::new("Full Contract")
            .small()
            .color(palette::text_dim(panel.state.night_mode)),
    )
    .id_salt(format!("creation_contract_{idx}"))
    .default_open(false)
    .show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                render_markdown_text(panel, ui, &msg.content);
            });
    });
}

fn render_markdown_text(panel: &ClawPanel, ui: &mut egui::Ui, text: &str) {
    let mut cache = CommonMarkCache::default();
    let text_color = palette::text_bright(panel.state.night_mode);
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(text_color);
        CommonMarkViewer::new()
            .explicit_image_uri_scheme(true)
            .show(ui, &mut cache, text);
    });
}

fn contract_summary_lines(content: &str) -> Vec<String> {
    let priorities = [
        "书名",
        "标题",
        "题材",
        "类型",
        "总目标字数",
        "总字数",
        "每章",
        "预计章节数",
        "主角",
        "角色",
        "终局",
        "结局",
        "大纲",
    ];
    let mut lines = Vec::new();
    for raw in content.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', ' ', '\t']).trim();
        if line.is_empty() || line.chars().count() > 180 {
            continue;
        }
        if priorities.iter().any(|term| line.contains(term)) {
            push_unique_contract_summary_line(&mut lines, line);
        }
        if lines.len() >= 8 {
            break;
        }
    }
    if lines.is_empty() {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(6)
            .map(ToOwned::to_owned)
            .collect()
    } else {
        lines
    }
}

fn push_unique_contract_summary_line(lines: &mut Vec<String>, line: &str) {
    if !lines.iter().any(|existing| existing == line) {
        lines.push(line.to_string());
    }
}

fn render_chat_artifact_buttons(panel: &mut ClawPanel, ui: &mut egui::Ui, msg: &ChatMessage) {
    if msg.artifacts.is_empty() {
        return;
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Artifacts")
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
        );
        for artifact in &msg.artifacts {
            open_target::render_open_target_button_with_label(
                panel,
                ui,
                Some(&artifact.artifact_id),
                &artifact.uri,
                artifact.media_type.as_deref(),
                Some(&artifact.label),
            );
        }
    });
}

fn chat_display_preview(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars).collect();
    format!(
        "{head}\n\n[聊天界面只显示预览：原文共 {count} 字符。长正文应保存在 artifact 文件里；需要全文时请打开文件路径或任务产物。]"
    )
}

fn maybe_refresh_chat_task_progress(
    panel: &mut ClawPanel,
    ctx: &egui::Context,
    current_session: &str,
) {
    let has_active_task =
        panel.state.session_runtime_tasks.iter().any(|task| {
            task.thread_id.as_deref() == Some(current_session) && !task_is_terminal(task)
        });
    if !panel.state.chat_loading && !has_active_task {
        return;
    }

    let now = ctx.input(|i| i.time);
    let switched_session =
        panel.state.session_runtime_tasks_session_id.as_deref() != Some(current_session);
    let stale = now - panel.state.last_session_runtime_tasks_refresh_time > 2.0;
    if panel.state.pending_session_runtime_tasks_promise.is_none() && (switched_session || stale) {
        panel
            .state
            .do_session_runtime_tasks_refresh(&panel.rt, ctx, current_session.to_string());
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(800));
}

fn render_chat_task_progress(panel: &mut ClawPanel, ui: &mut egui::Ui, current_session: &str) {
    let latest = panel
        .state
        .session_runtime_tasks
        .iter()
        .filter(|task| task.thread_id.as_deref() == Some(current_session))
        .max_by(|left, right| {
            task_activity_rank(left)
                .cmp(&task_activity_rank(right))
                .then_with(|| left.updated_at.cmp(&right.updated_at))
        });

    if panel.state.chat_loading && latest.is_none() {
        ui.add_space(8.0);
        render_progress_frame(
            ui,
            panel.state.night_mode,
            "BenShu 正在思考...",
            "等待 gateway 创建运行时任务；创建后这里会显示检索、入库、委派和写作进度。",
            &[],
            None,
        );
        return;
    }

    let Some(task) = latest else {
        return;
    };
    if task_is_terminal(task) && !panel.state.chat_loading {
        return;
    }

    let checkpoint = task
        .checkpoints
        .iter()
        .rev()
        .find_map(|checkpoint| checkpoint.summary.as_deref())
        .unwrap_or("任务已启动，正在等待下一条运行时进度。");
    let recent_steps = task
        .checkpoints
        .iter()
        .rev()
        .filter_map(|checkpoint| {
            checkpoint.summary.as_deref().map(|summary| {
                format!(
                    "{} · {}",
                    checkpoint.label,
                    compact_progress_line(summary, 180)
                )
            })
        })
        .take(5)
        .collect::<Vec<_>>();
    let title = format!("BenShu 正在执行任务 [{}]", task.status);
    let task_line = format!("task: {}", task.id);

    ui.add_space(8.0);
    render_progress_frame(
        ui,
        panel.state.night_mode,
        &title,
        checkpoint,
        &recent_steps,
        Some(&task_line),
    );
}

fn render_progress_frame(
    ui: &mut egui::Ui,
    night: bool,
    title: &str,
    detail: &str,
    recent_steps: &[String],
    footer: Option<&str>,
) {
    egui::Frame::new()
        .fill(palette::bg_deep(night).gamma_multiply(1.25))
        .stroke(Stroke::new(1.0, palette::WARNING))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(title)
                        .small()
                        .strong()
                        .color(palette::WARNING),
                );
            });
            ui.label(
                RichText::new(detail)
                    .small()
                    .color(palette::text_bright(night)),
            );
            if !recent_steps.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("最近执行步骤")
                        .small()
                        .strong()
                        .color(palette::text_dim(night)),
                );
                for step in recent_steps {
                    ui.label(
                        RichText::new(format!("• {}", step))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                }
            }
            if let Some(footer) = footer {
                ui.label(
                    RichText::new(footer)
                        .small()
                        .color(palette::text_dim(night)),
                );
            }
        });
}

fn compact_progress_line(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = collapsed.chars().take(max_chars).collect::<String>();
    if collapsed.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn task_activity_rank(task: &crate::api::SessionTaskInfo) -> u8 {
    if task_is_terminal(task) {
        0
    } else {
        1
    }
}

fn task_is_terminal(task: &crate::api::SessionTaskInfo) -> bool {
    matches!(
        task.status.as_str(),
        "completed" | "succeeded" | "failed" | "blocked" | "cancelled" | "canceled"
    )
}

enum ClipboardImagePaste {
    Attached(ChatAttachmentDraft),
    NoImage,
    Failed(String),
}

#[cfg(not(target_arch = "wasm32"))]
fn try_clipboard_image_attachment() -> ClipboardImagePaste {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(error) => return ClipboardImagePaste::Failed(error.to_string()),
    };

    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(arboard::Error::ContentNotAvailable) => return ClipboardImagePaste::NoImage,
        Err(error) => return ClipboardImagePaste::Failed(error.to_string()),
    };

    let path = clipboard_image_path();
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return ClipboardImagePaste::Failed(error.to_string());
        }
    }

    let width = match u32::try_from(image.width) {
        Ok(width) => width,
        Err(error) => return ClipboardImagePaste::Failed(error.to_string()),
    };
    let height = match u32::try_from(image.height) {
        Ok(height) => height,
        Err(error) => return ClipboardImagePaste::Failed(error.to_string()),
    };

    if let Err(error) = image::save_buffer_with_format(
        &path,
        image.bytes.as_ref(),
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    ) {
        return ClipboardImagePaste::Failed(error.to_string());
    }

    ClipboardImagePaste::Attached(ChatAttachmentDraft::from_path(path))
}

#[cfg(target_arch = "wasm32")]
fn try_clipboard_image_attachment() -> ClipboardImagePaste {
    ClipboardImagePaste::NoImage
}

#[cfg(not(target_arch = "wasm32"))]
fn clipboard_image_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    std::env::temp_dir()
        .join("benshu-panel-clipboard")
        .join(format!("clipboard-screenshot-{millis}.png"))
}
