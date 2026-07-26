use crate::app::ClawPanel;
use crate::app_state::VaultEntry;
use crate::common::palette;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};

pub fn render_channels_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        ui.add_space(8.0);

        if panel.state.channels_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Loading channel schemas...")
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            });
            return;
        }

        if let Some(error) = &panel.state.channels_error {
            ui.colored_label(
                palette::DANGER,
                RichText::new(format!("Failed to load communication schemas: {error}")).strong(),
            );
            ui.add_space(8.0);
            if ui.button("Retry").clicked() {
                panel.state.do_channel_refresh(&panel.rt, ctx);
            }
            return;
        }

        if panel.state.channel_metadata.is_empty() {
            panel.state.do_channel_refresh(&panel.rt, ctx);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Loading channel schemas...")
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            });
            return;
        }

        let mut need_refresh = false;
        for meta in &panel.state.channel_metadata {
            let observability = panel.state.channel_observability.get(&meta.id);
            let required_fields: Vec<_> = meta.fields.iter().filter(|f| f.required).collect();
            let is_configured = !required_fields.is_empty()
                && required_fields.iter().all(|field| {
                    panel
                        .state
                        .vault_entries
                        .iter()
                        .find(|entry| entry.key == field.key.to_uppercase())
                        .is_some_and(|entry| !entry.value.trim().is_empty())
                });
            let is_running = panel.state.running_channels.contains(&meta.id);

            let (frame_fill, border_color, border_width) = if is_running {
                (Color32::from_rgb(15, 25, 15), palette::SUCCESS, 1.5) // Very dark green background, success border
            } else if is_configured {
                (Color32::from_rgb(15, 15, 25), palette::ACCENT, 1.0) // Very dark blue background, accent border
            } else {
                (
                    panel.theme_bg_deep(),
                    palette::border(panel.state.night_mode),
                    1.0,
                )
            };

            egui::Frame::new()
                .fill(frame_fill)
                .stroke(Stroke::new(border_width, border_color))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(12))
                .outer_margin(egui::Margin::symmetric(0, 0)) // No side margin
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let status_dot = if is_running { "●" } else { "○" };
                        let dot_color = if is_running {
                            palette::SUCCESS
                        } else {
                            palette::text_dim(panel.state.night_mode)
                        };
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(status_dot)
                                .color(dot_color)
                                .font(FontId::new(14.0, egui::FontFamily::Monospace)),
                        );
                        ui.add_space(10.0); // Clear gap between dot and text

                        ui.vertical(|ui| {
                            ui.heading(
                                RichText::new(format!("{}  {}", meta.icon, meta.name))
                                    .color(palette::text_bright(panel.state.night_mode)),
                            );
                            ui.label(
                                RichText::new(&meta.description)
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                            if let Some(observability) = observability {
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "in:{} out:{}",
                                            observability.inbound_total,
                                            observability.outbound_total
                                        ))
                                        .small()
                                        .monospace()
                                        .color(palette::text_dim(panel.state.night_mode)),
                                    );
                                    if let Some(chat_id) = &observability.last_chat_id {
                                        ui.label(
                                            RichText::new(format!("chat:{}", chat_id))
                                                .small()
                                                .monospace()
                                                .color(palette::text_dim(panel.state.night_mode)),
                                        );
                                    }
                                    if let Some(failure_kind) = &observability.last_failure_kind {
                                        ui.label(
                                            RichText::new(format!("last_failure:{}", failure_kind))
                                                .small()
                                                .monospace()
                                                .color(palette::DANGER),
                                        );
                                    }
                                });
                            }

                            ui.add_space(8.0);

                            for field in &meta.fields {
                                ui.label(
                                    RichText::new(&field.label).strong().color(palette::ACCENT),
                                );

                                ui.horizontal(|ui| {
                                    // Find vault entry
                                    let vault_key = field.key.to_uppercase();

                                    // Ensure entry exists
                                    if !panel.state.vault_entries.iter().any(|e| e.key == vault_key)
                                    {
                                        panel.state.vault_entries.push(VaultEntry {
                                            key: vault_key.clone(),
                                            saved: false,
                                            ..Default::default()
                                        });
                                    }

                                    if let Some(entry) = panel
                                        .state
                                        .vault_entries
                                        .iter_mut()
                                        .find(|e| e.key == vault_key)
                                    {
                                        let mut textedit =
                                            egui::TextEdit::singleline(&mut entry.value)
                                                .desired_width(ui.available_width());
                                        if field.field_type == "password"
                                            && !panel.state.vault_show_value
                                        {
                                            textedit = textedit.password(true);
                                        }
                                        ui.add(textedit);
                                    }
                                });
                                ui.label(
                                    RichText::new(&field.description)
                                        .small()
                                        .color(palette::text_dim(panel.state.night_mode)),
                                );
                                ui.add_space(4.0);
                            }

                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                if ui
                                    .button(
                                        RichText::new(" Save & Hot-Reload ")
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .clicked()
                                {
                                    panel.state.status_msg =
                                        Some(("Sending reload signal...".to_string(), false));

                                    let mut values = std::collections::HashMap::new();
                                    for field in &meta.fields {
                                        let vault_key = field.key.to_uppercase();
                                        if let Some(entry) = panel
                                            .state
                                            .vault_entries
                                            .iter()
                                            .find(|e| e.key == vault_key)
                                        {
                                            values.insert(field.key.clone(), entry.value.clone());
                                        }
                                    }

                                    let client = panel.state.client.clone();
                                    let channel_id = meta.id.clone();
                                    let ctx2 = ctx.clone();
                                    let rt = panel.rt.clone();

                                    crate::common::task::spawn_task(&rt, async move {
                                        println!(
                                            "Panel: Sending config for channel '{}'...",
                                            channel_id
                                        );
                                        match client.save_channel_config(&channel_id, values).await
                                        {
                                            Ok(_) => {
                                                println!(
                                                    "Panel: Config update SUCCESS for '{}'",
                                                    channel_id
                                                );
                                            }
                                            Err(e) => {
                                                eprintln!(
                                                    "Panel: Config update FAILED for '{}': {}",
                                                    channel_id, e
                                                );
                                            }
                                        }
                                        ctx2.request_repaint();
                                    });

                                    need_refresh = true;
                                }

                                if is_running {
                                    ui.label(
                                        RichText::new("Connected").small().color(palette::SUCCESS),
                                    );
                                } else if is_configured {
                                    ui.label(
                                        RichText::new("Configured").small().color(palette::ACCENT),
                                    );
                                }
                            });
                        });
                    });
                });
            ui.add_space(8.0);
        }
        if need_refresh {
            panel.state.do_channel_refresh(&panel.rt, ctx);
        }
    });
}
