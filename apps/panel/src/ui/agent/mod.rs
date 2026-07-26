use crate::app::ClawPanel;
use crate::app_state::{AgentSubTab, AgentTaskSubTab};
use crate::common::palette;
use crate::i18n::t;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use poll_promise::Promise;

pub mod chat;
pub mod editor;
mod telemetry_views;

use telemetry_views::{
    parse_verification_sources_json, parse_verification_string_list, render_runtime_tasks_card,
    render_truth_verification_filter_row, render_truth_verification_query_results,
};

pub fn render_agent_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let subtab_font = FontId::proportional(15.0);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    panel.state.agent_subtab == AgentSubTab::Editor,
                    RichText::new(t("tabs.create_agent", panel.state.language))
                        .font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.agent_subtab = AgentSubTab::Editor;
            }
            ui.label("|");
            if ui
                .selectable_label(
                    panel.state.agent_subtab == AgentSubTab::Chat,
                    RichText::new(t("tabs.chat", panel.state.language)).font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.agent_subtab = AgentSubTab::Chat;
            }
            ui.label("|");
            if ui
                .selectable_label(
                    panel.state.agent_subtab == AgentSubTab::Tasks,
                    RichText::new(t("tabs.agent_tasks", panel.state.language))
                        .font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.agent_subtab = AgentSubTab::Tasks;
            }
            ui.label("|");
            if ui
                .selectable_label(
                    panel.state.agent_subtab == AgentSubTab::A2A,
                    RichText::new(t("tabs.a2a", panel.state.language)).font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.agent_subtab = AgentSubTab::A2A;
            }
            ui.label("|");
            if ui
                .selectable_label(
                    panel.state.agent_subtab == AgentSubTab::Metrics,
                    RichText::new(t("tabs.metrics", panel.state.language))
                        .font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.agent_subtab = AgentSubTab::Metrics;
            }
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(12.0);

        match panel.state.agent_subtab {
            AgentSubTab::Editor => {
                if panel.state.is_adding_agent || panel.state.is_editing_identity {
                    editor::render_agent_editor(panel, ui, ctx);
                } else {
                    render_agent_hub(panel, ui, ctx);
                }
            }
            AgentSubTab::Chat => chat::render_chat_tab(panel, ui, ctx),
            AgentSubTab::Tasks => render_agent_tasks_subtab(panel, ui, ctx),
            AgentSubTab::A2A => render_a2a_tab(panel, ui, ctx),
            AgentSubTab::Metrics => render_metrics_subtab(panel, ui, ctx),
        }

        show_confirmation_dialogs(panel, ctx);
    });
}

fn render_agent_tasks_subtab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    panel.state.agent_task_subtab == AgentTaskSubTab::HighRisk,
                    t("tabs.approvals", panel.state.language),
                )
                .clicked()
            {
                panel.state.agent_task_subtab = AgentTaskSubTab::HighRisk;
            }
            ui.label("|");
            if ui
                .selectable_label(
                    panel.state.agent_task_subtab == AgentTaskSubTab::Scheduled,
                    t("tabs.cron", panel.state.language),
                )
                .clicked()
            {
                panel.state.agent_task_subtab = AgentTaskSubTab::Scheduled;
            }
        });
        ui.add_space(8.0);

        match panel.state.agent_task_subtab {
            AgentTaskSubTab::HighRisk => render_approvals_tab(panel, ui, ctx),
            AgentTaskSubTab::Scheduled => render_cron_tab(panel, ui, ctx),
        }
    });
}

fn render_agent_hub(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    // Support auto-refresh if needed
    if panel.state.agent_list.is_empty() && panel.state.agent_list_promise.is_none() {
        panel.state.do_agent_refresh(&panel.rt, ctx);
    }
    ui.vertical(|ui| {
        // --- 1. Top Integrated Dashboard: Selection Hub ---
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new(t("agent_tab.title", lang)).color(palette::text_bright(night)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("↻ {}", t("btn.refresh", lang))).clicked() {
                    panel.state.do_agent_refresh(&panel.rt, ctx);
                }

                if ui
                    .add(egui::Button::new(
                        RichText::new(format!("📥 {}", t("agent.import", lang))).strong(),
                    ))
                    .clicked()
                {
                    panel.state.agent_show_import_window = true;
                }

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("➕ {}", t("btn.new_agent", lang)))
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(palette::ACCENT)
                        .min_size(egui::vec2(120.0, 32.0)),
                    )
                    .clicked()
                {
                    panel.state.is_adding_agent = true;

                    // Generate a stable random-ish ID (Machine ID)
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    panel.state.agent_role_selected = format!("agent_{:x}", now);

                    panel.state.agent_role_name = String::new();
                    panel.state.agent_role_description = String::new();
                    panel.state.agent_role_content = String::new();
                    panel.state.agent_role_loaded = true;
                    panel.state.is_editing_identity = true;
                }
            });
        });
        ui.add_space(12.0);

        // --- 2. Existing Agents (Responsive Grid) ---
        ui.label(
            RichText::new(t("dashboard.active_agents", lang).to_uppercase())
                .strong()
                .color(palette::ACCENT)
                .small(),
        );
        ui.add_space(8.0);

        // Use a wrapping layout to prevent "breaking" the window frame
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                let agents: Vec<(String, Option<String>)> = panel
                    .state
                    .agent_list
                    .iter()
                    .map(|a| (a.id.clone(), a.name.clone()))
                    .collect();

                for (agent_id, agent_name) in agents {
                    let is_active = agent_id == panel.state.agent_role_selected;

                    // --- MINI SLIM CARD (Hard Fixed Dimensions) ---
                    let card_size = egui::vec2(210.0, 32.0);
                    let (rect, _response) = ui.allocate_exact_size(card_size, egui::Sense::hover());

                    let hovered = ui.rect_contains_pointer(rect);
                    let fill = if hovered {
                        palette::bg_surface(night)
                    } else {
                        palette::bg_deep(night)
                    };
                    let stroke = Stroke::new(
                        1.0,
                        if hovered {
                            palette::ACCENT.gamma_multiply(0.5)
                        } else {
                            palette::border(night)
                        },
                    );

                    ui.allocate_ui_at_rect(rect, |ui| {
                        egui::Frame::new()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 0)) // Vertical margin 0 as we use centered layout
                            .show(ui, |ui| {
                                ui.set_height(32.0);
                                ui.set_width(210.0);

                                // 🚀 INTERNAL: Keep Center for pixel-perfect text
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        // 1. IDENTITY NAME
                                        ui.set_max_width(140.0);
                                        let display_name =
                                            agent_name.clone().unwrap_or_else(|| agent_id.clone());
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(display_name)
                                                    .strong()
                                                    .size(14.0)
                                                    .color(palette::ACCENT),
                                            )
                                            .truncate(),
                                        );

                                        // 2. FILLER to push menu to the right
                                        ui.allocate_space(ui.available_size());
                                    },
                                );

                                // 2. ACTION MENU (Precisely Centered)
                                let menu_rect = egui::Rect::from_min_size(
                                    rect.right_top() + egui::vec2(-22.0, 4.0), // Perfect flush with clipped overflow (210 - 22 + 24 = 212)
                                    egui::vec2(24.0, 24.0),
                                );

                                ui.allocate_ui_at_rect(menu_rect, |ui| {
                                    ui.with_layout(
                                        egui::Layout::centered_and_justified(
                                            egui::Direction::LeftToRight,
                                        ),
                                        |ui| {
                                            ui.menu_button(
                                                RichText::new("···").size(14.0).strong(),
                                                |ui| {
                                                    ui.set_min_width(120.0);

                                                    if ui
                                                        .button(format!(
                                                            "⚙ {}",
                                                            t("btn.apply", lang)
                                                        ))
                                                        .clicked()
                                                    {
                                                        panel.state.agent_role_selected =
                                                            agent_id.clone();
                                                        panel.state.do_load_agent(&panel.rt, ctx);
                                                        panel.state.is_editing_identity = true;
                                                        ui.close_menu();
                                                    }

                                                    if ui
                                                        .button(format!(
                                                            "💬 {}",
                                                            t("tabs.chat", lang)
                                                        ))
                                                        .clicked()
                                                    {
                                                        panel.state.agent_role_selected =
                                                            agent_id.clone();
                                                        panel.state.agent_subtab =
                                                            AgentSubTab::Chat;
                                                        ui.close_menu();
                                                    }

                                                    if agent_id != "benshu" {
                                                        if ui
                                                            .button(format!(
                                                                "📤 {}",
                                                                t("agent.export", lang)
                                                            ))
                                                            .clicked()
                                                        {
                                                            panel.state.agent_role_selected =
                                                                agent_id.clone();
                                                            panel.state.agent_show_export_window =
                                                                true;
                                                            ui.close_menu();
                                                        }
                                                    }

                                                    if agent_id != "benshu" {
                                                        ui.separator();
                                                        if ui
                                                            .button(
                                                                RichText::new(format!(
                                                                    "🗑 {}",
                                                                    t("btn.delete", lang)
                                                                ))
                                                                .color(palette::DANGER),
                                                            )
                                                            .clicked()
                                                        {
                                                            panel.state.pending_delete_agent =
                                                                Some(agent_id.clone());
                                                            ui.close_menu();
                                                        }
                                                    }
                                                },
                                            );
                                        },
                                    );
                                });
                            });
                    });
                }
            },
        );
    });
}

fn render_cron_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("↻ Refresh").clicked() {
                    panel.state.last_cron_refresh_time = -999.0;
                    panel.state.do_cron_refresh(&panel.rt, ctx);
                }
                ui.label(
                    RichText::new(format!("{} jobs", panel.state.cron_jobs.len()))
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Schedule recurring agent tasks.")
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
        );
        ui.add_space(12.0);

        if let Some(err) = &panel.state.cron_error.clone() {
            ui.label(RichText::new(err).color(palette::DANGER).small());
            ui.add_space(6.0);
        }

        // New job form
        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New Job").color(palette::ACCENT).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .selectable_label(
                                panel.state.cron_visual_mode,
                                t("cron.visual_mode", lang),
                            )
                            .clicked()
                        {
                            panel.state.cron_visual_mode = true;
                            panel.state.cron_form_schedule = "cron".to_string();
                        }
                        if ui
                            .selectable_label(
                                !panel.state.cron_visual_mode,
                                t("cron.advanced", lang),
                            )
                            .clicked()
                        {
                            panel.state.cron_visual_mode = false;
                        }
                    });
                });
                ui.add_space(8.0);

                egui::Grid::new("cron_form")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Name")
                                .color(palette::text_dim(night))
                                .small(),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.state.cron_form_name)
                                .hint_text("e.g. Daily Digest")
                                .desired_width(240.0),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new(t("cron.target_agent", lang))
                                .color(palette::text_dim(night))
                                .small(),
                        );
                        egui::ComboBox::from_id_salt("agent_role_select_cron")
                            .selected_text(&panel.state.cron_form_role)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                if !panel.state.agent_list.is_empty() {
                                    for agent in &panel.state.agent_list {
                                        ui.selectable_value(
                                            &mut panel.state.cron_form_role,
                                            agent.id.clone(),
                                            &agent.id,
                                        );
                                    }
                                } else {
                                    ui.selectable_value(
                                        &mut panel.state.cron_form_role,
                                        "benshu".to_string(),
                                        "benshu",
                                    );
                                }
                            });
                        ui.end_row();

                        if panel.state.cron_visual_mode {
                            ui.label(
                                RichText::new(t("cron.frequency", lang))
                                    .color(palette::text_dim(night))
                                    .small(),
                            );
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut panel.state.cron_visual_freq,
                                    "hourly".to_string(),
                                    t("cron.freq_hourly", lang),
                                );
                                ui.selectable_value(
                                    &mut panel.state.cron_visual_freq,
                                    "daily".to_string(),
                                    t("cron.freq_daily", lang),
                                );
                                ui.selectable_value(
                                    &mut panel.state.cron_visual_freq,
                                    "weekly".to_string(),
                                    t("cron.freq_weekly", lang),
                                );
                            });
                            ui.end_row();

                            if panel.state.cron_visual_freq == "weekly" {
                                ui.label(
                                    RichText::new(t("cron.day_of_week", lang))
                                        .color(palette::text_dim(night))
                                        .small(),
                                );
                                ui.horizontal(|ui| {
                                    for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                                        ui.selectable_value(
                                            &mut panel.state.cron_visual_weekday,
                                            day.to_string(),
                                            day,
                                        );
                                    }
                                });
                                ui.end_row();
                            }

                            if panel.state.cron_visual_freq != "hourly" {
                                ui.label(
                                    RichText::new(t("cron.time", lang))
                                        .color(palette::text_dim(night))
                                        .small(),
                                );
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut panel.state.cron_visual_hour)
                                            .range(0..=23)
                                            .suffix("h"),
                                    );
                                    ui.label(":");
                                    ui.add(
                                        egui::DragValue::new(&mut panel.state.cron_visual_minute)
                                            .range(0..=59)
                                            .suffix("m"),
                                    );
                                });
                                ui.end_row();
                            }

                            ui.label(
                                RichText::new(t("cron.schedule_hint", lang))
                                    .color(palette::text_dim(night))
                                    .small(),
                            );
                            let expr = match panel.state.cron_visual_freq.as_str() {
                                "hourly" => format!("0 * * * *"),
                                "daily" => format!(
                                    "{} {} * * *",
                                    panel.state.cron_visual_minute, panel.state.cron_visual_hour
                                ),
                                "weekly" => {
                                    let day_idx = match panel.state.cron_visual_weekday.as_str() {
                                        "Mon" => 1,
                                        "Tue" => 2,
                                        "Wed" => 3,
                                        "Thu" => 4,
                                        "Fri" => 5,
                                        "Sat" => 6,
                                        "Sun" => 0,
                                        _ => 1,
                                    };
                                    format!(
                                        "{} {} * * {}",
                                        panel.state.cron_visual_minute,
                                        panel.state.cron_visual_hour,
                                        day_idx
                                    )
                                }
                                _ => "0 * * * *".to_string(),
                            };
                            panel.state.cron_form_expr = expr.clone();
                            ui.label(RichText::new(&expr).strong().color(palette::ACCENT));
                            ui.end_row();
                        } else {
                            ui.label(
                                RichText::new("Schedule Type")
                                    .color(palette::text_dim(night))
                                    .small(),
                            );
                            ui.horizontal(|ui| {
                                for kind in ["every", "cron"] {
                                    let active = panel.state.cron_form_schedule == kind;
                                    if ui.selectable_label(active, kind).clicked() {
                                        panel.state.cron_form_schedule = kind.to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            if panel.state.cron_form_schedule == "every" {
                                ui.label(
                                    RichText::new("Interval (sec)")
                                        .color(palette::text_dim(night))
                                        .small(),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut panel.state.cron_form_interval)
                                        .hint_text("3600")
                                        .desired_width(100.0),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Cron Expr")
                                        .color(palette::text_dim(night))
                                        .small(),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut panel.state.cron_form_expr)
                                        .hint_text("0 9 * * *")
                                        .desired_width(160.0),
                                );
                            }
                            ui.end_row();
                        }

                        ui.label(
                            RichText::new("Prompt")
                                .color(palette::text_dim(night))
                                .small(),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut panel.state.cron_form_prompt)
                                .hint_text("What should the agent do?")
                                .desired_rows(2)
                                .desired_width(320.0),
                        );
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("  + Add Job  ").strong()).clicked() {
                        panel.state.submit_cron_job(&panel.rt, ctx);
                    }
                    if ui.button("Clear Form").clicked() {
                        panel.state.cron_form_name.clear();
                        panel.state.cron_form_prompt.clear();
                    }
                });
            });

        ui.add_space(12.0);

        if panel.state.cron_loading {
            ui.label(
                RichText::new("Loading…")
                    .color(palette::text_dim(panel.state.night_mode))
                    .small(),
            );
            return;
        }

        if panel.state.cron_jobs.is_empty() {
            ui.label(
                RichText::new("No scheduled jobs yet.")
                    .color(palette::text_dim(panel.state.night_mode))
                    .small(),
            );
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let jobs = panel.state.cron_jobs.clone();
            for job in &jobs {
                egui::Frame::new()
                    .fill(panel.theme_bg_deep())
                    .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let status_color = if job.enabled {
                                palette::SUCCESS
                            } else {
                                palette::DANGER
                            };
                            ui.label(
                                RichText::new(if job.enabled { "●" } else { "○" })
                                    .color(status_color),
                            );
                            ui.label(
                                RichText::new(&job.name)
                                    .color(palette::text_bright(panel.state.night_mode))
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(&job.payload_kind)
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let job_id = job.id.clone();
                                    let client = panel.state.client.clone();
                                    let ctx2 = ctx.clone();
                                    let rt = panel.rt.clone();
                                    if ui.small_button("🗑 Delete").clicked() {
                                        let id = job_id.clone();
                                        let c = client.clone();
                                        crate::common::task::spawn_task(&rt, async move {
                                            let _ = c.delete_cron_job(&id).await;
                                            ctx2.request_repaint();
                                        });
                                    }
                                    if ui
                                        .small_button(if job.enabled {
                                            "⏸ Pause"
                                        } else {
                                            "▶ Resume"
                                        })
                                        .clicked()
                                    {
                                        let id = job_id.clone();
                                        let c = client.clone();
                                        let next = !job.enabled;
                                        /* crate::common::task::spawn_task(&rt, async move {
                                            let _ = c.set_cron_job_enabled(&id, next).await;
                                            ctx2.request_repaint();
                                        }); */
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(20.0);
                            let cron_expr = job
                                .schedule
                                .get("cron")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Dynamic");
                            ui.label(RichText::new(cron_expr).small().color(palette::ACCENT));
                            ui.label(
                                RichText::new(format!("Target: {}", job.name))
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                        });
                    });
                ui.add_space(4.0);
            }
        });
    });
}

fn render_approvals_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;
    let now = ctx.input(|i| i.time);
    if let Some(session_id) = panel.state.current_runtime_session_id() {
        let stale = now - panel.state.last_session_runtime_tasks_refresh_time > 5.0;
        let switched_session =
            panel.state.session_runtime_tasks_session_id.as_deref() != Some(session_id.as_str());
        if panel.state.pending_session_runtime_tasks_promise.is_none()
            && (stale || switched_session || panel.state.session_runtime_tasks.is_empty())
        {
            panel
                .state
                .do_session_runtime_tasks_refresh(&panel.rt, ctx, session_id.clone());
            panel
                .state
                .do_session_delegation_refresh(&panel.rt, ctx, session_id);
        }
    }

    ui.vertical(|ui| {
        render_runtime_tasks_card(panel, ui, ctx);
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t("approvals.title", lang))
                    .strong()
                    .color(palette::ACCENT),
            );
            ui.label(
                RichText::new(t("approvals.subtitle", lang))
                    .small()
                    .color(palette::text_dim(night)),
            );
        });
        ui.add_space(8.0);

        if panel.state.pending_approval_promise.is_some()
            || panel.state.approval_resolve_promise.is_some()
        {
            ui.label(
                RichText::new(t("approvals.loading", lang))
                    .small()
                    .color(palette::text_dim(night)),
            );
            ui.add_space(6.0);
        }

        if panel.state.approvals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new(t("approvals.empty", lang)).color(palette::text_dim(night)));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let approvals = panel.state.approvals.clone();
                for approval in &approvals {
                    egui::Frame::new()
                        .fill(panel.theme_bg_deep())
                        .stroke(Stroke::new(1.0, palette::border(night)))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&approval.tool_name)
                                            .strong()
                                            .color(palette::ACCENT),
                                    );
                                    ui.label(
                                        RichText::new("requested action")
                                            .small()
                                            .color(palette::text_dim(night)),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new("HIGH RISK")
                                                    .color(palette::DANGER)
                                                    .small()
                                                    .strong(),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(t("approvals.tool", lang)).strong());
                                    ui.label(
                                        RichText::new(&approval.tool_name)
                                            .color(Color32::from_rgb(255, 165, 0)),
                                    );
                                });

                                if !approval.arguments.is_empty() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {}",
                                            t("approvals.args", lang),
                                            approval.arguments
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                }
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!(
                                        "Decision: {:?} | Policy: {}",
                                        approval.decision_kind, approval.policy_basis
                                    ))
                                    .small()
                                    .color(palette::text_dim(night)),
                                );
                                if let Some(reason) = &approval.escalation_reason {
                                    ui.label(
                                        RichText::new(format!("Escalation: {}", reason))
                                            .small()
                                            .color(palette::WARNING),
                                    );
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "{}: {}",
                                        t("approvals.challenge", lang),
                                        approval.challenge_code
                                    ))
                                    .small()
                                    .monospace()
                                    .color(palette::text_dim(night)),
                                );
                                ui.label(
                                    RichText::new(format!("Created: {}", approval.created_at))
                                        .small()
                                        .color(palette::text_dim(night)),
                                );
                                if approval.trace_id.is_some()
                                    || approval.run_id.is_some()
                                    || approval.task_id.is_some()
                                    || approval.session_id.is_some()
                                {
                                    let mut refs = Vec::new();
                                    if let Some(trace_id) = &approval.trace_id {
                                        refs.push(format!("trace {}", trace_id));
                                    }
                                    if let Some(run_id) = &approval.run_id {
                                        refs.push(format!("run {}", run_id));
                                    }
                                    if let Some(task_id) = &approval.task_id {
                                        refs.push(format!("task {}", task_id));
                                    }
                                    if let Some(session_id) = &approval.session_id {
                                        refs.push(format!("session {}", session_id));
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "Runtime refs: {}",
                                            refs.join(" · ")
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                }

                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(
                                            RichText::new(t("approvals.approve", lang))
                                                .color(palette::SUCCESS),
                                        )
                                        .clicked()
                                    {
                                        panel.state.do_resolve_approval(
                                            &panel.rt,
                                            ctx,
                                            approval.id.clone(),
                                            true,
                                        );
                                    }
                                    if ui
                                        .button(
                                            RichText::new(t("approvals.reject", lang))
                                                .color(palette::DANGER),
                                        )
                                        .clicked()
                                    {
                                        panel.state.do_resolve_approval(
                                            &panel.rt,
                                            ctx,
                                            approval.id.clone(),
                                            false,
                                        );
                                    }
                                });

                                let receipts: Vec<_> = panel
                                    .state
                                    .approval_receipts
                                    .iter()
                                    .filter(|receipt| receipt.approval_id == approval.id)
                                    .cloned()
                                    .collect();
                                if !receipts.is_empty() {
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("Decision Receipts")
                                            .small()
                                            .strong()
                                            .color(palette::ACCENT),
                                    );
                                    for receipt in receipts.iter().rev().take(2) {
                                        ui.label(
                                            RichText::new(format!(
                                                "{:?} · {} · {}",
                                                receipt.decision_kind,
                                                receipt.policy_basis,
                                                receipt
                                                    .resolved_at
                                                    .as_deref()
                                                    .unwrap_or(receipt.created_at.as_str())
                                            ))
                                            .small()
                                            .color(palette::text_dim(night)),
                                        );
                                    }
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
            });
        }
    });
}

pub(super) fn trace_metadata<'a>(
    trace: &'a benshu_telemetry::RunTrace,
    key: &str,
) -> Option<&'a str> {
    trace
        .metadata
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

pub(super) fn trace_metadata_is_true(trace: &benshu_telemetry::RunTrace, key: &str) -> bool {
    trace_metadata(trace, key) == Some("true")
}

pub(super) fn render_trace_chip(
    ui: &mut egui::Ui,
    night: bool,
    label: &str,
    value: &str,
    tint: Color32,
) {
    egui::Frame::new()
        .fill(palette::bg_surface(night))
        .stroke(Stroke::new(1.0, palette::border(night)))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{}: {}", label, value))
                    .small()
                    .monospace()
                    .color(tint),
            );
        });
}

fn render_a2a_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    // Auto-refresh every 5 seconds if tab is active
    let now = ctx.input(|i| i.time);
    if now - panel.state.last_a2a_refresh_time > 5.0 {
        panel.state.do_a2a_refresh(&panel.rt, ctx);
    }

    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(t("a2a.title", lang))
                .font(FontId::new(32.0, egui::FontFamily::Proportional))
                .color(palette::ACCENT)
                .strong(),
        );
        ui.label(RichText::new(t("a2a.subtitle", lang)).color(palette::text_dim(night)));
    });
    ui.add_space(24.0);

    if let Some(err) = &panel.state.a2a_error {
        ui.label(RichText::new(err).color(palette::DANGER));
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                // Column 1: Communications Registry
                egui::Frame::new()
                    .fill(palette::bg_deep(night))
                    .stroke(Stroke::new(1.0, palette::border(night)))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(20))
                    .show(&mut cols[0], |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t("a2a.active_agents", lang))
                                    .size(18.0)
                                    .color(palette::ACCENT)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} NODES",
                                            panel.state.a2a_agents.len()
                                        ))
                                        .small()
                                        .color(palette::text_dim(night)),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(12.0);

                        if panel.state.a2a_agents.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.spinner();
                                ui.label(RichText::new(t("misc.no_data", lang)).italics());
                            });
                        } else {
                            for agent in &panel.state.a2a_agents {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("●").color(palette::SUCCESS).size(12.0));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(agent).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new("MEMORY")
                                                    .small()
                                                    .color(palette::INFO),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                ui.separator();
                                ui.add_space(6.0);
                            }
                        }

                        ui.add_space(30.0);
                        ui.label(
                            RichText::new(t("a2a.throttle_title", lang))
                                .color(palette::ACCENT)
                                .strong(),
                        );
                        ui.add_space(8.0);

                        egui::Grid::new("throttle_control_grid")
                            .num_columns(2)
                            .spacing([12.0, 12.0])
                            .show(ui, |ui| {
                                ui.label(t("a2a.target_tenant", lang));
                                ui.text_edit_singleline(&mut panel.state.a2a_throttle_tenant);
                                ui.end_row();

                                ui.label(t("a2a.target_role", lang));
                                ui.text_edit_singleline(&mut panel.state.a2a_throttle_role);
                                ui.end_row();

                                ui.label(t("a2a.limit_label", lang));
                                ui.add(
                                    egui::DragValue::new(&mut panel.state.a2a_throttle_limit)
                                        .range(1..=1000),
                                );
                                ui.end_row();
                            });

                        ui.add_space(12.0);
                        if ui
                            .button(RichText::new(t("a2a.btn_apply", lang)).strong())
                            .clicked()
                        {
                            let client = panel.state.client.clone();
                            let tenant = if panel.state.a2a_throttle_tenant.is_empty() {
                                None
                            } else {
                                Some(panel.state.a2a_throttle_tenant.clone())
                            };
                            let role = if panel.state.a2a_throttle_role.is_empty() {
                                None
                            } else {
                                Some(panel.state.a2a_throttle_role.clone())
                            };
                            let limit = panel.state.a2a_throttle_limit;

                            panel.state.a2a_throttle_promise =
                                Some(Promise::spawn_async(async move {
                                    client
                                        .set_a2a_throttle(tenant, role, limit)
                                        .await
                                        .map_err(|e| e.to_string())
                                }));
                        }

                        if let Some(promise) = &panel.state.a2a_throttle_promise {
                            match promise.ready() {
                                Some(Ok(_)) => {
                                    ui.label(
                                        RichText::new(format!("● {}", t("a2a.applied", lang)))
                                            .color(palette::SUCCESS)
                                            .small(),
                                    );
                                }
                                Some(Err(e)) => {
                                    ui.label(
                                        RichText::new(format!("● Error: {}", e))
                                            .color(palette::DANGER)
                                            .small(),
                                    );
                                }
                                None => {
                                    ui.spinner();
                                }
                            }
                        }
                    });

                // Column 2: Health & Telemetry
                egui::Frame::new()
                    .fill(palette::bg_deep(night))
                    .stroke(Stroke::new(1.0, palette::border(night)))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(20))
                    .show(&mut cols[1], |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(t("a2a.shared_board", lang))
                                .size(18.0)
                                .color(palette::ACCENT)
                                .strong(),
                        );
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(12.0);

                        if panel.state.a2a_board.is_empty() {
                            ui.label(
                                RichText::new(t("a2a.empty_board", lang))
                                    .color(palette::text_dim(night))
                                    .italics()
                                    .small(),
                            );
                        } else {
                            egui::Grid::new("a2a_board_grid")
                                .num_columns(2)
                                .spacing([32.0, 16.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    let mut keys: Vec<_> = panel.state.a2a_board.keys().collect();
                                    keys.sort();
                                    for key in keys {
                                        let value = &panel.state.a2a_board[key];
                                        ui.label(
                                            RichText::new(key).color(palette::text_dim(night)),
                                        );

                                        let val_color = if value == "Active"
                                            || value == "Normal"
                                            || value == "A2A Active"
                                        {
                                            palette::SUCCESS
                                        } else {
                                            palette::text_bright(night)
                                        };
                                        ui.label(RichText::new(value).strong().color(val_color));
                                        ui.end_row();
                                    }
                                });
                        }

                        ui.add_space(40.0);
                        ui.label(
                            RichText::new("INFRASTRUCTURE")
                                .color(palette::ACCENT)
                                .strong(),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Local Root:").small());
                            ui.label(RichText::new("localhost:3400").small().strong());
                        });
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Bus Core:").small());
                            ui.label(RichText::new("Connected").small().color(palette::SUCCESS));
                        });
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Identity:").small());
                            ui.label(
                                RichText::new("benshu-gateway-01")
                                    .small()
                                    .italics()
                                    .color(palette::text_dim(night)),
                            );
                        });
                    });
            });
        });
}

pub fn show_import_window(panel: &mut ClawPanel, ctx: &egui::Context) {
    let mut open = panel.state.agent_show_import_window;
    let mut do_import = false;

    egui::Window::new(t("agent.import_title", panel.state.language))
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("📦").size(32.0));
                ui.add_space(4.0);
                ui.label(RichText::new(t("agent.import_hint", panel.state.language))
                    .small()
                    .color(palette::text_dim(panel.state.night_mode)));
                ui.add_space(12.0);

                if ui.button(RichText::new(t("agent.import_select", panel.state.language)).strong()).clicked() {
                    // Force the use of .vessel extension only
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Agent Vessel (.vessel)", &["vessel"])
                        .pick_file() {
                        if !path.exists() { return; }
                        if let Ok(content) = std::fs::read_to_string(path) {
                            // Schema Validation
                            if let Ok(_) = serde_json::from_str::<benshu_brain::agent::layered_agent::vessel_pack::VesselPackage>(&content) {
                                panel.state.agent_import_json = content;
                            } else {
                                panel.state.agent_import_json = String::new();
                                tracing::error!("INVALID_SCHEMA: File picked but it is not a valid Agent Vessel.");
                            }
                        }
                    }
                }

                if !panel.state.agent_import_json.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("✅ Vessel Recognized").color(palette::SUCCESS).small());
                } else if panel.state.agent_show_import_window {
                     ui.add_space(4.0);
                     ui.label(RichText::new("⚠️ Please select a valid .vessel file").color(palette::DANGER).small());
                }

                ui.add_space(12.0);

                let can_import = !panel.state.agent_import_json.is_empty();
                ui.add_enabled_ui(can_import, |ui| {
                    if ui.button(RichText::new(t("agent.import_btn", panel.state.language)).strong()).clicked() {
                        do_import = true;
                    }
                });

                ui.add_space(8.0);
            });
        });

    panel.state.agent_show_import_window = open;

    if do_import {
        let client = panel.state.client.clone();
        let vessel_json = panel.state.agent_import_json.clone();
        panel.state.agent_import_promise = Some(Promise::spawn_async(async move {
            client
                .import_vessel(vessel_json)
                .await
                .map_err(|e| e.to_string())
        }));

        panel.state.agent_show_import_window = false;
        panel.state.agent_import_json = String::new();
    }
}

pub fn show_export_result_window(panel: &mut ClawPanel, ctx: &egui::Context) {
    let mut close = false;
    let mut save_path = None;
    let lang = panel.state.language;

    if let Some(json) = &panel.state.agent_export_json {
        egui::Window::new(t("agent.export_result", lang))
            .pivot(egui::Align2::CENTER_CENTER)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(280.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new("💾").size(32.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new(t("agent.export_success", lang)).strong());
                    ui.add_space(16.0);

                    if ui
                        .button(RichText::new(t("agent.export_save", lang)).strong())
                        .clicked()
                    {
                        let default_name = format!(
                            "{}.vessel",
                            panel.state.agent_role_selected.replace(' ', "_")
                        );
                        save_path = rfd::FileDialog::new()
                            .set_file_name(default_name)
                            .add_filter("Agent Vessel (.vessel)", &["vessel"])
                            .save_file();
                        ctx.request_repaint();
                    }

                    ui.add_space(8.0);
                    if ui.button(t("btn.cancel", lang)).clicked() {
                        close = true;
                    }
                });
            });

        if let Some(path) = save_path {
            if let Ok(_) = std::fs::write(&path, json) {
                tracing::info!("VESSEL_SAVED: {}", path.display());
                close = true;
            } else {
                tracing::error!(
                    "SAVE_FAILED: Permission denied or invalid path: {}",
                    path.display()
                );
            }
        }
    }

    if close {
        panel.state.agent_export_json = None;
    }
}

pub fn show_export_window(panel: &mut ClawPanel, ctx: &egui::Context) {
    let mut open = panel.state.agent_show_export_window;
    let lang = panel.state.language;
    egui::Window::new(t("agent.export", lang))
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(8.0);

                ui.vertical_centered(|ui| {
                    if ui
                        .button(RichText::new(t("agent.export_start", lang)).strong())
                        .clicked()
                    {
                        let default_name = format!(
                            "{}.vessel",
                            panel.state.agent_role_selected.replace(' ', "_")
                        );
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(default_name)
                            .add_filter("Agent Vessel (.vessel)", &["vessel"])
                            .save_file()
                        {
                            panel.state.agent_export_save_path = Some(path);
                            let client = panel.state.client.clone();
                            let role = panel.state.agent_role_selected.clone();
                            panel.state.agent_export_promise =
                                Some(Promise::spawn_async(async move {
                                    client
                                        .export_agent(&role, 999_999)
                                        .await
                                        .map_err(|e| e.to_string())
                                }));
                            panel.state.agent_export_loading = true;
                            panel.state.agent_show_export_window = false;
                        }
                    }

                    ui.add_space(8.0);
                    if ui.button(t("btn.cancel", lang)).clicked() {
                        panel.state.agent_show_export_window = false;
                    }
                });
            });
        });
    if !open {
        panel.state.agent_show_export_window = false;
    }
}

pub fn show_confirmation_dialogs(panel: &mut ClawPanel, ctx: &egui::Context) {
    let lang = panel.state.language;
    if let Some(role) = panel.state.pending_delete_agent.clone() {
        egui::Window::new(t("agent.confirm_delete_title", lang))
            .pivot(egui::Align2::CENTER_CENTER)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(t("agent.confirm_delete_msg", lang).replace("{0}", &role))
                        .strong(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new(t("btn.yes_delete", lang)).color(palette::DANGER))
                        .clicked()
                    {
                        panel.state.do_delete_agent(&panel.rt, ctx, role);
                        panel.state.pending_delete_agent = None;
                    }
                    if ui.button(t("btn.cancel", lang)).clicked() {
                        panel.state.pending_delete_agent = None;
                    }
                });
            });
    }

    // Session Deletion Confirmation
    if let Some((role, idx)) = panel.state.pending_delete_session.clone() {
        let session_name = if let Some(sessions) = panel.state.chat_sessions.get(&role) {
            sessions
                .get(idx)
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            "Unknown".to_string()
        };

        egui::Window::new(t("session.confirm_delete_title", lang))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        t("session.confirm_delete_msg", lang)
                            .replace("{0}", &session_name)
                            .replace("{1}", &role),
                    )
                    .strong(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t("btn.confirm", lang)).clicked() {
                        if let Some(sessions) = panel.state.chat_sessions.get_mut(&role) {
                            if let Some(session_id) = sessions.get(idx).cloned() {
                                sessions.remove(idx);
                                let history_key = format!("{}:{}", session_id, role);
                                panel.state.chat_histories.remove(&history_key);
                                let client = panel.state.client.clone();
                                let rt = panel.rt.clone();
                                crate::common::task::spawn_task(&rt, async move {
                                    if let Err(error) = client.delete_session(&session_id).await {
                                        tracing::warn!(
                                            "Failed to delete persisted chat session {}: {}",
                                            session_id,
                                            error
                                        );
                                    }
                                });
                            }
                            panel
                                .state
                                .active_chat_session
                                .insert(role.clone(), "default".to_string());
                            panel.state.do_load_session_history(
                                &panel.rt,
                                ctx,
                                "default".to_string(),
                            );
                        }
                        panel.state.pending_delete_session = None;
                    }
                    if ui.button(t("btn.cancel", lang)).clicked() {
                        panel.state.pending_delete_session = None;
                    }
                });
            });
    }
}

fn render_metrics_subtab(panel: &mut ClawPanel, ui: &mut egui::Ui, _ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;

    ui.vertical(|ui| {
        ui.label(
            RichText::new(t("dashboard.token_usage_title", lang))
                .font(FontId::new(22.0, egui::FontFamily::Proportional))
                .strong()
                .color(palette::text_bright(night)),
        );
        ui.add_space(20.0);

        if let Some(metrics) = &panel.state.last_metrics {
            // First Row: Tokens
            ui.columns(3, |columns| {
                columns[0].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.total_tokens", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.heading(
                        RichText::new(metrics.total_tokens.unwrap_or(0).to_string())
                            .color(Color32::from_rgb(139, 92, 246)),
                    ); // Purple
                });
                columns[1].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.prompt_tokens", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.heading(
                        RichText::new(metrics.prompt_tokens.unwrap_or(0).to_string())
                            .color(palette::ACCENT),
                    );
                });
                columns[2].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.completion_tokens", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.heading(
                        RichText::new(metrics.completion_tokens.unwrap_or(0).to_string())
                            .color(palette::SUCCESS),
                    );
                });
            });

            ui.add_space(32.0);

            // Second Row: General Calls
            ui.label(
                RichText::new(t("dashboard.calls_title", lang))
                    .font(FontId::new(18.0, egui::FontFamily::Proportional))
                    .strong()
                    .color(palette::text_bright(night)),
            );
            ui.add_space(12.0);

            ui.columns(3, |columns| {
                columns[0].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.total_calls", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.heading(
                        RichText::new(metrics.total_calls.unwrap_or(0).to_string())
                            .color(palette::text_bright(night)),
                    );
                });
                columns[1].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.avg_latency", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    ui.heading(
                        RichText::new(format!("{:.0}ms", metrics.avg_latency_ms.unwrap_or(0.0)))
                            .color(palette::text_bright(night)),
                    );
                });
                columns[2].vertical_centered(|ui| {
                    ui.label(
                        RichText::new(t("dashboard.success_rate", lang))
                            .small()
                            .color(palette::text_dim(night)),
                    );
                    let rate = metrics.success_rate.unwrap_or(0.0) * 100.0;
                    let color = if rate > 95.0 {
                        palette::SUCCESS
                    } else {
                        palette::DANGER
                    };
                    ui.heading(RichText::new(format!("{:.1}%", rate)).color(color));
                });
            });

            if let Some(engram) = &metrics.engram {
                ui.add_space(32.0);
                ui.label(
                    RichText::new("Engram Windows Native")
                        .font(FontId::new(18.0, egui::FontFamily::Proportional))
                        .strong()
                        .color(palette::text_bright(night)),
                );
                ui.add_space(12.0);

                ui.columns(2, |columns| {
                    columns[0].group(|ui| {
                        ui.label(
                            RichText::new("Embedding")
                                .small()
                                .strong()
                                .color(palette::ACCENT),
                        );
                        let outcome = engram
                            .windows_native_embed_outcome
                            .as_deref()
                            .unwrap_or("not_reported");
                        let strategy = engram
                            .windows_native_embed_strategy
                            .as_deref()
                            .unwrap_or("inspect_runtime_path");
                        let class = engram
                            .windows_native_embed_class
                            .as_deref()
                            .unwrap_or("not_observed");
                        let note = engram
                            .windows_native_embed_note
                            .as_deref()
                            .unwrap_or("No Windows-native embedding telemetry reported yet.");
                        let provider = engram
                            .windows_native_embed_provider
                            .as_deref()
                            .unwrap_or("not_reported");
                        let device_target = engram
                            .windows_native_embed_device_target
                            .as_deref()
                            .unwrap_or("not_reported");
                        let fallback_mode = engram
                            .windows_native_embed_fallback_mode
                            .as_deref()
                            .unwrap_or("not_reported");

                        let color = if outcome == "windows_native_active" {
                            palette::SUCCESS
                        } else if outcome == "not_observed" || outcome == "not_reported" {
                            palette::text_dim(night)
                        } else {
                            palette::WARNING
                        };

                        ui.label(
                            RichText::new(format!("Outcome: {}", outcome))
                                .small()
                                .monospace()
                                .color(color),
                        );
                        ui.label(
                            RichText::new(format!("Class: {}", class))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Strategy: {}", strategy))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Provider: {}", provider))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Device: {}", device_target))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Fallback: {}", fallback_mode))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(RichText::new(note).small().color(palette::text_dim(night)));
                    });

                    columns[1].group(|ui| {
                        ui.label(
                            RichText::new("Rerank")
                                .small()
                                .strong()
                                .color(palette::ACCENT),
                        );
                        let outcome = engram
                            .windows_native_rerank_outcome
                            .as_deref()
                            .unwrap_or("not_reported");
                        let strategy = engram
                            .windows_native_rerank_strategy
                            .as_deref()
                            .unwrap_or("inspect_runtime_path");
                        let class = engram
                            .windows_native_rerank_class
                            .as_deref()
                            .unwrap_or("not_observed");
                        let note = engram
                            .windows_native_rerank_note
                            .as_deref()
                            .unwrap_or("No Windows-native rerank telemetry reported yet.");
                        let provider = engram
                            .windows_native_rerank_provider
                            .as_deref()
                            .unwrap_or("not_reported");
                        let device_target = engram
                            .windows_native_rerank_device_target
                            .as_deref()
                            .unwrap_or("not_reported");
                        let fallback_mode = engram
                            .windows_native_rerank_fallback_mode
                            .as_deref()
                            .unwrap_or("not_reported");

                        let color = if outcome == "windows_native_active" {
                            palette::SUCCESS
                        } else if outcome == "not_observed" || outcome == "not_reported" {
                            palette::text_dim(night)
                        } else {
                            palette::WARNING
                        };

                        ui.label(
                            RichText::new(format!("Outcome: {}", outcome))
                                .small()
                                .monospace()
                                .color(color),
                        );
                        ui.label(
                            RichText::new(format!("Class: {}", class))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Strategy: {}", strategy))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Provider: {}", provider))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Device: {}", device_target))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(
                            RichText::new(format!("Fallback: {}", fallback_mode))
                                .small()
                                .monospace()
                                .color(palette::text_dim(night)),
                        );
                        ui.label(RichText::new(note).small().color(palette::text_dim(night)));
                    });
                });
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(t("metrics.no_data", lang)).color(palette::text_dim(night)));
            });
        }

        if let Some(trace) = panel.state.selected_run_trace.clone() {
            let truth_status = trace_metadata(&trace, "truth_status").map(str::to_string);
            let verification_domain =
                trace_metadata(&trace, "verification_domain").map(str::to_string);
            let verification_requirement =
                trace_metadata(&trace, "verification_requirement").map(str::to_string);
            let verification_mode = trace_metadata(&trace, "verification_mode").map(str::to_string);
            let verification_outcome =
                trace_metadata(&trace, "verification_outcome").map(str::to_string);
            let verification_answer_readiness =
                trace_metadata(&trace, "verification_answer_readiness").map(str::to_string);
            let verification_route_reason =
                trace_metadata(&trace, "verification_route_reason").map(str::to_string);
            let verification_continuation =
                trace_metadata(&trace, "verification_continuation").map(str::to_string);
            let verification_termination =
                trace_metadata(&trace, "verification_termination").map(str::to_string);
            let verification_requires_followup =
                trace_metadata(&trace, "verification_requires_followup").map(str::to_string);
            let verification_can_finalize_answer =
                trace_metadata(&trace, "verification_can_finalize_answer").map(str::to_string);
            let verification_next_tools =
                trace_metadata(&trace, "verification_next_tools").map(str::to_string);
            let verification_cite_required =
                trace_metadata(&trace, "verification_cite_required").map(str::to_string);
            let verification_sources = parse_verification_sources_json(trace_metadata(
                &trace,
                "verification_sources_json",
            ));
            let verification_execution_evidence = parse_verification_string_list(trace_metadata(
                &trace,
                "verification_execution_evidence_json",
            ));
            let verification_state_evidence = parse_verification_string_list(trace_metadata(
                &trace,
                "verification_state_evidence_json",
            ));
            let source_posture = trace_metadata(&trace, "source_posture").map(str::to_string);
            let verification_last_tool =
                trace_metadata(&trace, "verification_last_tool").map(str::to_string);
            let truth_verification_guidance_active =
                trace_metadata(&trace, "truth_verification_guidance_active").map(str::to_string);

            if truth_status.is_some()
                || verification_domain.is_some()
                || verification_requirement.is_some()
                || verification_mode.is_some()
                || verification_outcome.is_some()
                || verification_answer_readiness.is_some()
                || verification_route_reason.is_some()
                || verification_continuation.is_some()
                || verification_termination.is_some()
                || verification_requires_followup.is_some()
                || verification_can_finalize_answer.is_some()
                || verification_next_tools.is_some()
                || verification_cite_required.is_some()
                || !verification_sources.is_empty()
                || !verification_execution_evidence.is_empty()
                || !verification_state_evidence.is_empty()
                || source_posture.is_some()
                || verification_last_tool.is_some()
                || truth_verification_guidance_active.is_some()
            {
                ui.add_space(32.0);
                ui.label(
                    RichText::new("Truth & Verification Metrics")
                        .font(FontId::new(18.0, egui::FontFamily::Proportional))
                        .strong()
                        .color(palette::text_bright(night)),
                );
                ui.add_space(12.0);
                render_truth_verification_filter_row(
                    panel,
                    ui,
                    night,
                    "runtime_main_path",
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
            }
        }
    });
}
