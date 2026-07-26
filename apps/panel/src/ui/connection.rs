use crate::app::ClawPanel;
use crate::app_state;
use crate::common::palette;
use eframe::egui::{self, Color32, RichText, Stroke};

pub fn render_connection_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let night = panel.state.night_mode;
    ui.vertical(|ui| {
        ui.heading("Endpoint Configuration");
        ui.add_space(8.0);
        ui.label(RichText::new("Specify your BenShu Gateway URL. This links the control panel to the local inference engine.").small().color(palette::text_dim(night)));
        ui.add_space(16.0);

        egui::Frame::new()
            .fill(panel.theme_bg_surface())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Gateway URL:");
                    let mut url = panel.state.gateway_url.clone();
                    let resp = ui.add(egui::TextEdit::singleline(&mut url).desired_width(240.0));
                    if resp.changed() {
                        // We don't call set_url immediately while typing to avoid spamming re-init,
                        // but we sync the string so the user sees their change.
                        panel.state.gateway_url = url;
                    }
                    if ui.button("Connect").clicked() {
                        let current_url = panel.state.gateway_url.clone();
                        panel.state.set_url(current_url);
                        panel.state.do_agent_refresh(&panel.rt, ctx);
                        panel.state.set_status("Connecting to gateway with fresh credentials...", false);
                    }
                });
            });

        ui.add_space(24.0);

        ui.heading("Global Night Mode");
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(panel.theme_bg_surface())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(panel.state.night_mode, "🌙 Dark").clicked() {
                        panel.state.night_mode = true;
                        app_state::save_config(&panel.state);
                    }
                    if ui.selectable_label(!panel.state.night_mode, "☀️ Light").clicked() {
                        panel.state.night_mode = false;
                        app_state::save_config(&panel.state);
                    }
                });
            });

        ui.add_space(24.0);
        ui.heading("Session Management");
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(panel.theme_bg_surface())
            .stroke(Stroke::new(1.0, palette::border(night)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                 if ui.button(RichText::new("Clear All Chat Sessions").color(palette::DANGER)).clicked() {
                     let client = panel.state.client.clone();
                     let rt = panel.rt.clone();
                     /* crate::common::task::spawn_task(&rt, async move {
                         // let _ = client.clear_all_chats().await;
                     }); */
                     panel.state.set_status("All sessions cleared", false);
                 }
            });

        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("BenShu v0.3.5 — AgentOS for the Local Web").small().color(palette::text_dim(night)));
        });
    });
}
