use crate::app::ClawPanel;
use crate::app_state::ActiveTab;
use crate::common::palette;
use crate::i18n::t;
use eframe::egui::{self, Color32, FontId, RichText};
use egui_plot::{Line, Plot, PlotPoints};

pub fn render_dashboard_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, _ctx: &egui::Context) {
    let lang = panel.state.language;
    let night = panel.state.night_mode;
    ui.vertical(|ui| {
        // --- Doctor Mode: Proactive GPU/VRAM Diagnostics (FIXED: Moved away from per-frame detect) ---
        if let Some(metrics) = &panel.state.last_metrics {
            if let Some(host) = &metrics.host {
                // Use existing metrics info to check for GPU health instead of a blocking detect()
                if host.gpu_vram_total_mb == 0 && !cfg!(target_os = "macos") {
                    egui::Frame::new()
                        .fill(palette::DANGER.gamma_multiply(0.1))
                        .stroke(egui::Stroke::new(1.0, palette::DANGER))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⚠").size(22.0).color(palette::DANGER));
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(t("dashboard.gpu_limited", lang))
                                            .strong()
                                            .color(palette::DANGER),
                                    );
                                    ui.label(
                                        RichText::new(t("dashboard.vram_not_detected", lang))
                                            .small(),
                                    );
                                    if ui
                                        .button(
                                            RichText::new(format!(
                                                "🛠 {}",
                                                t("dashboard.open_doctor", lang)
                                            ))
                                            .small(),
                                        )
                                        .clicked()
                                    {
                                        panel.state.tab = ActiveTab::System;
                                        panel.state.system_subtab =
                                            crate::app_state::SystemSubTab::Doctor;
                                    }
                                });
                            });
                        });
                    ui.add_space(20.0);
                }
            }
        }

        if let Some(metrics) = &panel.state.last_metrics {
            // Third Row: Host Resources (Phase 7.1)
            ui.label(
                RichText::new(t("dashboard.host_title", lang))
                    .font(FontId::new(20.0, egui::FontFamily::Proportional))
                    .strong()
                    .color(palette::text_bright(night)),
            );
            ui.add_space(12.0);

            if let Some(host) = &metrics.host {
                // Optimized: Pre-calculate points to avoid redundant work in columns
                let cpu_p: PlotPoints = panel
                    .state
                    .metrics_history
                    .iter()
                    .enumerate()
                    .map(|(i, m)| [i as f64, m.cpu_usage as f64])
                    .collect();
                let ram_p: PlotPoints = panel
                    .state
                    .metrics_history
                    .iter()
                    .enumerate()
                    .map(|(i, m)| [i as f64, (m.ram_usage * 100.0) as f64])
                    .collect();
                let vram_p: PlotPoints = panel
                    .state
                    .metrics_history
                    .iter()
                    .enumerate()
                    .map(|(i, m)| [i as f64, (m.vram_usage * 100.0) as f64])
                    .collect();

                // ABSOLUTE PROTECTION: Wrap in disabled UI to kill ALL mouse interaction/picking
                ui.add_enabled_ui(false, |ui| {
                    ui.columns(3, |columns| {
                        columns[0].vertical_centered(|ui| {
                            ui.label(
                                RichText::new(t("dashboard.cpu_usage", lang))
                                    .small()
                                    .color(palette::text_dim(night)),
                            );
                            ui.heading(
                                RichText::new(format!("{:.1}%", host.cpu_usage_percent))
                                    .color(palette::ACCENT),
                            );

                            ui.add_space(4.0);
                            Plot::new("cpu_plot_optimized")
                                .height(120.0)
                                .show_axes([false, false])
                                .show_grid(false)
                                .show_background(false)
                                .allow_drag(false)
                                .allow_zoom(false)
                                .allow_scroll(false)
                                .show_x(false)
                                .show_y(false)
                                .label_formatter(|_, _| String::new())
                                .show(ui, |plot_ui| {
                                    plot_ui.line(Line::new(cpu_p).color(palette::ACCENT).width(2.0))
                                });
                        });
                        columns[1].vertical_centered(|ui| {
                            ui.label(
                                RichText::new(t("dashboard.memory_usage", lang))
                                    .small()
                                    .color(palette::text_dim(night)),
                            );
                            let ram_perc =
                                (host.memory_used_mb as f32 / host.memory_total_mb as f32) * 100.0;
                            ui.heading(
                                RichText::new(format!("{:.0}%", ram_perc))
                                    .color(palette::text_bright(night)),
                            );

                            ui.add_space(4.0);
                            Plot::new("ram_plot_optimized")
                                .height(120.0)
                                .show_axes([false, false])
                                .show_grid(false)
                                .show_background(false)
                                .allow_drag(false)
                                .allow_zoom(false)
                                .allow_scroll(false)
                                .show_x(false)
                                .show_y(false)
                                .label_formatter(|_, _| String::new())
                                .show(ui, |plot_ui| {
                                    plot_ui.line(
                                        Line::new(ram_p)
                                            .color(palette::text_bright(night).gamma_multiply(0.6))
                                            .width(2.0),
                                    )
                                });
                        });
                        columns[2].vertical_centered(|ui| {
                            ui.label(
                                RichText::new("GPU VRAM")
                                    .small()
                                    .color(palette::text_dim(night)),
                            );
                            let vram_perc = (host.gpu_vram_used_mb as f32
                                / host.gpu_vram_total_mb as f32)
                                * 100.0;
                            ui.heading(
                                RichText::new(format!("{:.1}%", vram_perc.max(0.1)))
                                    .color(palette::WARNING),
                            );

                            ui.add_space(4.0);
                            Plot::new("vram_plot_optimized")
                                .height(120.0)
                                .show_axes([false, false])
                                .show_grid(false)
                                .show_background(false)
                                .allow_drag(false)
                                .allow_zoom(false)
                                .allow_scroll(false)
                                .show_x(false)
                                .show_y(false)
                                .label_formatter(|_, _| String::new())
                                .show(ui, |plot_ui| {
                                    plot_ui
                                        .line(Line::new(vram_p).color(palette::WARNING).width(2.0))
                                });
                        });
                    });
                });
            }
        }
    });
}
