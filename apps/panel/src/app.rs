use crate::api::GatewayClient;
use crate::app_state::{
    ActiveTab, AgentSubTab, AgentTaskSubTab, ApiSubTab, AppState, MetricsSnapshot, SkillsSubTab,
    VaultEntry,
};
use crate::i18n::{t, Language};
use benshu_brain::config::AgentConfigOverrides;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use egui_plot::{Line, Plot, PlotPoints};
use poll_promise::Promise;

use crate::common::{palette, task::spawn_task};
use crate::ui::components::toggle::toggle;

/// Colour palette — dynamic based on theme.

// ── ClawPanel struct ─────────────────────────────────────────────────────────

pub struct ClawPanel {
    pub state: AppState,
    #[cfg(not(target_arch = "wasm32"))]
    pub rt: tokio::runtime::Handle,
    #[cfg(target_os = "windows")]
    pub tray_icon: Option<tray_icon::TrayIcon>,
    #[cfg(target_os = "windows")]
    pub abort_item_id: Option<tray_icon::menu::MenuId>,
    #[cfg(target_os = "windows")]
    pub quit_item_id: Option<tray_icon::menu::MenuId>,
}

impl ClawPanel {
    fn do_full_shutdown(&mut self, _ctx: &egui::Context) {
        let client = self.state.client.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            spawn_task(&self.rt, async move {
                let _ = client.shutdown_gateway().await;
                // The gateway will exit in 1s. We close ourselves now.
                std::process::exit(0);
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            spawn_task(async move {
                let _ = client.shutdown_gateway().await;
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: tokio::runtime::Handle,
        token: Option<String>,
    ) -> Self {
        Self::init(token, cc, rt)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::init(None, cc)
    }

    fn init(
        token: Option<String>,
        cc: &eframe::CreationContext<'_>,
        #[cfg(not(target_arch = "wasm32"))] rt: tokio::runtime::Handle,
    ) -> Self {
        let mut visuals = if cc.egui_ctx.style().visuals.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        // Initial theme application
        let night = true; // AppState::new() defaults to night=true
        visuals.panel_fill = palette::bg_deep(night);
        visuals.window_fill = palette::bg_surface(night);
        visuals.widgets.noninteractive.bg_fill = palette::bg_surface(night);
        visuals.widgets.inactive.bg_fill = palette::bg_surface(night);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette::border(night));
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette::ACCENT);
        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(102, 178, 255, 60);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(28.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            FontId::new(19.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            FontId::new(16.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            FontId::new(15.0, egui::FontFamily::Monospace),
        );
        cc.egui_ctx.set_style(style);

        // Sub-phase 4: Font setup (P11)
        Self::setup_fonts(&cc.egui_ctx);

        let mut panel = Self {
            state: AppState::new(token),
            #[cfg(not(target_arch = "wasm32"))]
            rt,
            #[cfg(target_os = "windows")]
            tray_icon: None,
        };

        #[cfg(target_os = "windows")]
        panel.init_tray();

        panel.state.trigger_refresh(&panel.rt, &cc.egui_ctx);
        panel
    }

    #[cfg(target_os = "windows")]
    fn init_tray(&mut self) {
        use tray_icon::{
            menu::{Menu, MenuItem},
            Icon, TrayIconBuilder,
        };

        let menu = Menu::new();
        let abort_item = MenuItem::new("🛑 Abort All Agents", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        self.abort_item_id = Some(abort_item.id().clone());
        self.quit_item_id = Some(quit_item.id().clone());
        let _ = menu.append_items(&[&abort_item, &quit_item]);

        // Create a solid blue 16x16 icon
        let size = 16;
        let mut pixels = vec![0u8; size * size * 4];
        for i in 0..size * size {
            pixels[i * 4] = 59; // R
            pixels[i * 4 + 1] = 130; // G
            pixels[i * 4 + 2] = 246; // B
            pixels[i * 4 + 3] = 255; // A
        }

        if let Ok(icon) = Icon::from_rgba(pixels, size as u32, size as u32) {
            match TrayIconBuilder::new()
                .with_tooltip("BenShu Control Panel")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .build()
            {
                Ok(tray) => {
                    self.tray_icon = Some(tray);
                }
                Err(e) => {
                    tracing::warn!("Failed to build tray icon: {}", e);
                }
            }
        }
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    fn setup_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        // 1. 注入内置字体 (作为保底)
        fonts.font_data.insert(
            "noto_sans_sc".to_owned(),
            egui::FontData::from_static(include_bytes!("../assets/NotoSansSC-Regular.subset.ttf"))
                .into(),
        );
        // 2. 尝试从用户系统加载商业级字体 (增强显示效果)
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut sys_font_paths = Vec::new();

            #[cfg(target_os = "windows")]
            {
                sys_font_paths.push("C:\\Windows\\Fonts\\msyh.ttc");
                sys_font_paths.push("C:\\Windows\\Fonts\\msyh.ttf");
            }

            #[cfg(target_os = "macos")]
            {
                sys_font_paths.push("/System/Library/Fonts/PingFang.ttc");
            }

            #[cfg(target_os = "linux")]
            {
                sys_font_paths.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc");
                sys_font_paths.push("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc");
                sys_font_paths.push("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc");
            }
            for path in sys_font_paths {
                if std::path::Path::new(path).exists() {
                    if let Ok(data) = std::fs::read(path) {
                        fonts.font_data.insert(
                            "system_fallback".to_owned(),
                            egui::FontData::from_owned(data).into(),
                        );
                        break;
                    }
                }
            }
        }
        // 3. 设置优先级：优先内置字体 → 系统后备 → egui 默认
        let families = [egui::FontFamily::Proportional, egui::FontFamily::Monospace];
        for family in families {
            let list = fonts.families.get_mut(&family).unwrap();
            list.insert(0, "noto_sans_sc".to_owned());
            if fonts.font_data.contains_key("system_fallback") {
                list.insert(1, "system_fallback".to_owned());
            }
        }

        ctx.set_fonts(fonts);
    }

    pub fn theme_bg_deep(&self) -> Color32 {
        palette::bg_deep(self.state.night_mode)
    }

    pub fn theme_bg_surface(&self) -> Color32 {
        palette::bg_surface(self.state.night_mode)
    }
}

// ── Platform-agnostic task spawner ───────────────────────────────────────────

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl eframe::App for ClawPanel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── 1. Startup Snap: 50% of Screen Resolution ──────────────────
        // This runs only once per session to set the fixed size and ensure windowed mode.
        if !self.state.initial_resize_done {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                if monitor_size.x > 100.0 && monitor_size.y > 100.0 {
                    // Force windowed mode first
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));

                    let new_size = egui::vec2(monitor_size.x * 0.5, monitor_size.y * 0.5);

                    // Triple-lock the size to force X11/Win/Mac to comply
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(new_size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(new_size));
                    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(new_size));

                    self.state.initial_resize_done = true;
                    ctx.request_repaint();
                }
            } else {
                // Fallback for environments where monitor_size is None
                let fallback = egui::vec2(1280.0, 720.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(fallback));
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(fallback));
                ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(fallback));
                self.state.initial_resize_done = true;
            }
        }

        // ── 2. Initialize UI Styles ─────────────────────────────────────
        if self.state.last_ui_scale == 0.0 {
            self.state.last_ui_scale = 1.0;
            let mut style = (*ctx.style()).clone();
            style.text_styles.insert(
                egui::TextStyle::Heading,
                FontId::new(28.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Body,
                FontId::new(19.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                FontId::new(16.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Small,
                FontId::new(14.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::new(15.0, egui::FontFamily::Monospace),
            );

            // 🔥 Snappy tooltips: standard 0.5s is too slow for power users
            style.interaction.tooltip_delay = 0.1;

            ctx.set_style(style);
        }

        // Theme persistence and real-time syncing
        let is_dark_in_ctx = ctx.style().visuals.dark_mode;
        if self.state.night_mode != is_dark_in_ctx {
            let night = self.state.night_mode;
            let mut visuals = if night {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };

            visuals.panel_fill = palette::bg_deep(night);
            visuals.window_fill = palette::bg_surface(night);
            visuals.widgets.noninteractive.bg_fill = palette::bg_surface(night);
            visuals.widgets.inactive.bg_fill = if night {
                Color32::from_rgb(14, 14, 18)
            } else {
                Color32::from_rgb(245, 245, 250)
            };
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette::border(night));
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette::ACCENT);
            visuals.selection.bg_fill = Color32::from_rgba_premultiplied(102, 178, 255, 60);

            ctx.set_visuals(visuals);
            crate::app_state::save_config(&self.state);
        }

        // Handle Tray Icon Events
        #[cfg(target_os = "windows")]
        {
            if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                use tray_icon::TrayIconEvent;
                match event {
                    TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. } => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    _ => {}
                }
            }
        }

        // Unified Header (Control Center) - Phase 5.3 UX Interaction
        egui::TopBottomPanel::top("control_center")
            .frame(
                egui::Frame::new()
                    .fill(palette::bg_deep(self.state.night_mode).gamma_multiply(0.85)) // Glassmorphism-style
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .stroke(egui::Stroke::new(
                        1.0,
                        palette::border(self.state.night_mode).gamma_multiply(0.5),
                    )),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("◈ BenShu")
                            .color(palette::ACCENT)
                            .font(FontId::new(20.0, egui::FontFamily::Monospace))
                            .strong(),
                    );

                    ui.add_space(20.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // Health Stats (Control Center)
                    if let Some(metrics) = &self.state.last_metrics {
                        if let Some(host) = &metrics.host {
                            ui.label(
                                RichText::new("CPU")
                                    .small()
                                    .color(palette::text_dim(self.state.night_mode)),
                            );
                            ui.label(
                                RichText::new(format!("{:.0}%", host.cpu_usage_percent))
                                    .strong()
                                    .color(palette::ACCENT),
                            );
                            ui.add_space(12.0);

                            ui.label(
                                RichText::new("MEM")
                                    .small()
                                    .color(palette::text_dim(self.state.night_mode)),
                            );
                            ui.label(
                                RichText::new(format!("{}MB", host.memory_used_mb))
                                    .strong()
                                    .color(palette::ACCENT),
                            );
                            ui.add_space(12.0);

                            if let Some(host) = &metrics.host {
                                ui.label(
                                    RichText::new("VRAM")
                                        .small()
                                        .color(palette::text_dim(self.state.night_mode)),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{}/{} MB",
                                        host.gpu_vram_used_mb, host.gpu_vram_total_mb
                                    ))
                                    .strong()
                                    .color(palette::ACCENT),
                                );
                                ui.add_space(12.0);
                            }

                            ui.label(
                                RichText::new("AGENTS")
                                    .small()
                                    .color(palette::text_dim(self.state.night_mode)),
                            );
                            ui.label(
                                RichText::new(host.active_agent_processes.to_string())
                                    .strong()
                                    .color(palette::ACCENT),
                            );
                            ui.add_space(12.0);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.state.cancel_promise.is_some() {
                            ui.spinner();
                        }

                        // Lang & Theme
                        if ui
                            .button(if self.state.language == Language::Zh {
                                "EN"
                            } else {
                                "中"
                            })
                            .clicked()
                        {
                            self.state.language = if self.state.language == Language::Zh {
                                Language::En
                            } else {
                                Language::Zh
                            };
                            crate::app_state::save_config(&self.state);
                        }
                        if ui
                            .button(if self.state.night_mode { "☀" } else { "🌙" })
                            .clicked()
                        {
                            self.state.night_mode = !self.state.night_mode;
                            crate::app_state::save_config(&self.state);
                        }

                        ui.separator();

                        // Connection Dot
                        let (dot, dot_color) = match self.state.connected {
                            Some(true) => ("●", palette::SUCCESS),
                            Some(false) => ("●", palette::DANGER),
                            None => ("○", palette::text_dim(self.state.night_mode)),
                        };
                        ui.label(RichText::new(dot).color(dot_color).small());
                    });
                });
            });

        // Tab bar — full-width adaptive
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(self.theme_bg_deep())
                    .inner_margin(egui::Margin::symmetric(0, 0)),
            )
            .show(ctx, |ui| {
                let avail_width = ui.available_width();
                let tabs = [
                    ("tabs.dashboard", ActiveTab::Dashboard),
                    ("tabs.skills", ActiveTab::Skills),
                    ("tabs.agent", ActiveTab::Agent),
                    ("tabs.models", ActiveTab::Models),
                    ("tabs.logs", ActiveTab::Logs),
                    ("tabs.system", ActiveTab::System),
                    ("tabs.connection", ActiveTab::Connection),
                ];
                let n_tabs = tabs.len() as f32;
                let btn_width = avail_width / n_tabs;
                let btn_height = 46.0;
                // Level 1: Top Navigation - Largest (24px)
                let font_size = 24.0;

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (key, tab) in tabs {
                        let label = t(key, self.state.language);
                        let is_active = self.state.tab == tab;
                        let text = RichText::new(label)
                            .font(FontId::new(font_size, egui::FontFamily::Proportional))
                            .color(if is_active {
                                palette::text_bright(self.state.night_mode)
                            } else {
                                palette::text_dim(self.state.night_mode)
                            });
                        let response = ui.add_sized(
                            [btn_width, btn_height],
                            egui::Button::new(text)
                                .fill(if is_active {
                                    palette::bg_deep(self.state.night_mode)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::NONE)
                                .corner_radius(0.0),
                        );
                        if response.clicked() {
                            self.state.tab = tab.clone();
                            crate::app_state::save_config(&self.state);
                            if self.state.tab == ActiveTab::Agent {
                                self.state.do_agent_refresh(&self.rt, ctx);
                            }
                        }
                    }
                });
            });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(self.theme_bg_deep())
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                if let Some((msg, is_error)) = &self.state.status_msg {
                    let color = if *is_error {
                        palette::DANGER
                    } else {
                        palette::text_dim(self.state.night_mode)
                    };
                    ui.label(RichText::new(msg).small().color(color));
                }
            });

        // Main content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(self.theme_bg_surface())
                    .inner_margin(egui::Margin::symmetric(0, 14)), // Flush horizontal
            )
            .show(ctx, |ui| {
                // Logs and Chat manage their own internal ScrollAreas.
                // Wrapping them in an outer ScrollArea causes double-layout negotiation
                // which produces severe lag when resizing the window.
                match self.state.tab {
                    ActiveTab::Logs => crate::ui::logs::render_logs_tab(self, ui, ctx),
                    _ => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| match self.state.tab.clone() {
                                ActiveTab::Skills => {
                                    crate::ui::skills::render_skills_tab(self, ui, ctx)
                                }
                                ActiveTab::Models => {
                                    crate::ui::api::render_models_tab(self, ui, ctx)
                                }
                                ActiveTab::Agent => {
                                    crate::ui::agent::render_agent_tab(self, ui, ctx)
                                }
                                ActiveTab::Connection => {
                                    crate::ui::connection::render_connection_tab(self, ui, ctx)
                                }
                                ActiveTab::Dashboard => {
                                    crate::ui::dashboard::render_dashboard_tab(self, ui, ctx)
                                }
                                ActiveTab::System => {
                                    crate::ui::system::render_system_tab(self, ui, ctx)
                                }
                                ActiveTab::Channels => {
                                    crate::ui::channels::render_channels_tab(self, ui, ctx)
                                }
                                ActiveTab::Logs => crate::ui::logs::render_logs_tab(self, ui, ctx),
                            });
                    }
                }
            });

        self.state.poll_all_promises(&self.rt, ctx);
        self.state.auto_refresh(&self.rt, ctx);
        crate::ui::agent::show_confirmation_dialogs(self, ctx);

        // Skill detail popup
        if self.state.expanded_skill.is_some() {
            crate::ui::skills::render_skill_detail_window(self, ctx);
        }

        if let Some(json) = self.state.agent_export_json.clone() {
            if let Some(path) = self.state.agent_export_save_path.clone() {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::error!(
                        "VESSEL_WRITE_ERROR: Failed to write to {}: {}",
                        path.display(),
                        e
                    );
                } else {
                    tracing::info!("VESSEL_AUTO_WRITTEN: {}", path.display());
                }
            }
            self.state.agent_export_json = None;
            self.state.agent_export_save_path = None;
            self.state.agent_export_loading = false;
        }

        if self.state.agent_export_loading {
            egui::Window::new(t("tabs.agent", self.state.language))
                .collapsible(false)
                .resizable(false)
                .auto_sized()
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Extracting knowledge graph and memory slices...")
                                .small(),
                        );
                        ui.add_space(12.0);

                        if ui.button(t("btn.cancel", self.state.language)).clicked() {
                            self.state.agent_export_loading = false;
                            self.state.agent_export_json = None;
                            self.state.agent_export_promise = None;
                            self.state.agent_export_save_path = None;
                        }
                        ui.add_space(8.0);
                    });
                });
        }

        if self.state.agent_show_import_window {
            crate::ui::agent::show_import_window(self, ctx);
        }

        if self.state.agent_show_export_window {
            crate::ui::agent::show_export_window(self, ctx);
        }

        // ── Tray Events (Phase 5.2 Windows Native) ──────────────────────────
        #[cfg(target_os = "windows")]
        {
            if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if Some(&event.id) == self.abort_item_id.as_ref() {
                    let client = self.state.client.clone();
                    let (sender, promise) = Promise::new();
                    self.state.cancel_promise = Some(promise);
                    spawn_task(&self.rt, async move {
                        sender.send(client.cancel_task().await.map_err(|e| e.to_string()));
                    });
                } else if Some(&event.id) == self.quit_item_id.as_ref() {
                    self.do_full_shutdown(ctx);
                }
            }
        }

        // ── 🌟 Smart Exit Guard ───────────────────────────────────────────
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.state.show_exit_dialog {
                self.state.show_exit_dialog = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }

        if self.state.show_exit_dialog {
            egui::Window::new("Exit BenShu")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Choose exit strategy:").strong());
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui.button("  Option A: Full Shutdown  ").clicked() {
                                self.state.exit_in_progress = true;
                                self.do_full_shutdown(ctx);
                            }
                            if ui.button("  Option B: Minimize to Tray  ").clicked() {
                                // Minimize app, let it run in background
                                self.state.show_exit_dialog = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                            }
                        });

                        if self.state.exit_in_progress {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Shutting down gateway...");
                            });
                        }

                        ui.add_space(8.0);
                        if ui.link("Cancel").clicked() {
                            self.state.show_exit_dialog = false;
                        }
                    });
                });
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

impl ClawPanel {
    pub fn trigger_refresh(&mut self, ctx: &egui::Context) {
        self.state.trigger_refresh(&self.rt, ctx);
    }
}
