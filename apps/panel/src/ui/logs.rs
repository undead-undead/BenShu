use crate::app::ClawPanel;
use crate::common::palette;
use eframe::egui::{self, Color32, RichText, Stroke};

pub fn render_logs_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let next_poll_in = (2.0 - (now - panel.state.last_log_poll_time)).max(0.0);

    // Keep repainting every second so the countdown timer updates without mouse input
    if panel.state.auto_log_poll {
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Manual trigger
                if ui.button("▶ Poll Now").clicked() {
                    panel.state.last_log_poll_time = now;
                    panel.state.do_log_poll(&panel.rt, ctx);
                }

                ui.add_space(12.0); // Space between Poll and Auto-ON

                if panel.state.auto_log_poll {
                    ui.label(
                        RichText::new(format!("next: {:.0}s", next_poll_in))
                            .small()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                }

                // Auto-refresh toggle
                let auto_label = if panel.state.auto_log_poll {
                    RichText::new("⏱ Auto ON").small().color(palette::SUCCESS)
                } else {
                    RichText::new("⏱ Auto OFF")
                        .small()
                        .color(palette::text_dim(panel.state.night_mode))
                };
                if ui
                    .add(egui::Button::new(auto_label).fill(Color32::TRANSPARENT))
                    .clicked()
                {
                    panel.state.auto_log_poll = !panel.state.auto_log_poll;
                }

                ui.add_space(12.0); // Space between Auto-ON and Clear

                if ui.small_button("✕ Clear").clicked() {
                    panel.state.log_lines.clear();
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Firewall intercepts & skill execution events.")
                    .small()
                    .color(palette::text_dim(panel.state.night_mode)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} entries", panel.state.log_lines.len()))
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
            });
        });
        ui.add_space(8.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
            .corner_radius(egui::CornerRadius::ZERO) // No corner rounding for better 'flush' look
            .inner_margin(egui::Margin::symmetric(14, 10)) // Some side padding for text readability inside
            .outer_margin(egui::Margin::symmetric(0, 0)) // Truly flush with edges
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if panel.state.log_lines.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.label(
                                    RichText::new("No log entries yet.")
                                        .color(palette::text_dim(panel.state.night_mode)),
                                );
                                ui.add_space(20.0);
                            });
                        } else {
                            for line in &panel.state.log_lines {
                                // Simple color based on content
                                let color = if line.contains("ERROR") || line.contains("FAILED") {
                                    palette::DANGER
                                } else if line.contains("WARN") {
                                    palette::WARNING
                                } else if line.contains("SUCCESS")
                                    || line.contains("COMPLETED")
                                    || line.contains("OK")
                                {
                                    palette::SUCCESS
                                } else {
                                    palette::text_bright(panel.state.night_mode)
                                };
                                ui.label(
                                    RichText::new(line)
                                        .color(color)
                                        .font(egui::FontId::monospace(12.0)),
                                );
                            }
                        }
                    });
            });
    });
}
