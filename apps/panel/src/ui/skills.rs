use crate::app::ClawPanel;
use crate::app_state::SkillsSubTab;
use crate::common::palette;
use crate::i18n::t;
use eframe::egui::{self, Color32, FontId, RichText, Stroke};

pub fn render_skills_tab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    let lang = panel.state.language;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            // Level 2: Sub-tabs (20px)
            let subtab_font = FontId::new(20.0, egui::FontFamily::Proportional);
            if ui
                .selectable_label(
                    panel.state.skills_subtab == SkillsSubTab::Installed,
                    RichText::new(t("skills.installed", lang)).font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.skills_subtab = SkillsSubTab::Installed;
            }

            if ui
                .selectable_label(
                    panel.state.skills_subtab == SkillsSubTab::Manual,
                    RichText::new(t("skills.manual", lang)).font(subtab_font.clone()),
                )
                .clicked()
            {
                panel.state.skills_subtab = SkillsSubTab::Manual;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if panel.state.skills_promise.is_some() {
                    ui.add_space(8.0);
                    ui.spinner();
                }
            });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(2.0); // Reduced bottom space for a tighter look

        match panel.state.skills_subtab {
            SkillsSubTab::Installed => render_installed_skills(panel, ui, ctx),

            SkillsSubTab::Manual => render_manual_install_subtab(panel, ui, ctx),
        }
    });
}

fn render_installed_skills(panel: &mut ClawPanel, ui: &mut egui::Ui, _ctx: &egui::Context) {
    if panel.state.skills_promise.is_some() && panel.state.skills.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.spinner();
            ui.label(
                RichText::new("Discovering skills...")
                    .color(palette::text_dim(panel.state.night_mode)),
            );
        });
        return;
    }

    if panel.state.skills.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                RichText::new("No skills loaded.")
                    .color(palette::text_dim(panel.state.night_mode))
                    .font(FontId::new(14.0, egui::FontFamily::Monospace)),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Click 'Refresh' or connect to a running benshu-gateway.")
                    .color(palette::text_dim(panel.state.night_mode))
                    .small(),
            );
        });
        return;
    }

    let mut expand_target: Option<String> = None;
    let mut uninstall_target: Option<String> = None;

    let row_height = 38.0;
    egui::ScrollArea::vertical()
        .id_salt("installed_skills_scroll")
        .show_rows(ui, row_height, panel.state.skills.len(), |ui, row_range| {
            for i in row_range {
                let skill = &panel.state.skills[i];
                egui::Frame::new()
                    .fill(panel.theme_bg_deep())
                    .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .outer_margin(egui::Margin::symmetric(0, 4))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(palette::SUCCESS).small());
                                ui.add_space(8.0);

                                let name_resp = ui.add(
                                    egui::Button::new(
                                        RichText::new(&skill.name)
                                            .strong()
                                            .color(palette::ACCENT)
                                            .font(FontId::new(
                                                16.0,
                                                egui::FontFamily::Proportional,
                                            )),
                                    )
                                    .frame(false),
                                );
                                if name_resp.clicked() {
                                    expand_target = Some(skill.name.clone());
                                }

                                if let Some(rt) = &skill.runtime {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(format!("runtime: {}", rt))
                                            .small()
                                            .color(palette::text_dim(panel.state.night_mode))
                                            .italics(),
                                    );
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        let btn_size = egui::vec2(75.0, 20.0);

                                        if ui
                                            .add_sized(
                                                btn_size,
                                                egui::Button::new(
                                                    RichText::new("Details").small().color(
                                                        palette::text_dim(panel.state.night_mode),
                                                    ),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::new(
                                                    1.0,
                                                    palette::border(panel.state.night_mode),
                                                )),
                                            )
                                            .clicked()
                                        {
                                            expand_target = Some(skill.name.clone());
                                        }

                                        if ui
                                            .add_sized(
                                                btn_size,
                                                egui::Button::new(
                                                    RichText::new("Uninstall")
                                                        .small()
                                                        .color(palette::DANGER),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::new(1.0, palette::DANGER)),
                                            )
                                            .clicked()
                                        {
                                            uninstall_target = Some(skill.name.clone());
                                        }

                                        ui.add_sized(
                                            btn_size,
                                            egui::Label::new(
                                                RichText::new("Loaded")
                                                    .color(palette::SUCCESS)
                                                    .small(),
                                            ),
                                        );
                                    },
                                );
                            });
                        });
                    });
            }
        });

    if let Some(name) = expand_target {
        panel.state.expanded_skill = Some(name);
    }
    if let Some(name) = uninstall_target {
        panel.state.skills.retain(|s| s.name != name);
        let client = panel.state.client.clone();
        let rt = panel.rt.clone();
        crate::common::task::spawn_task(&rt, async move {
            let _ = client.uninstall_skill(&name).await;
        });
    }
}

fn render_manual_install_subtab(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t("install.title", panel.state.language))
                    .font(FontId::new(16.0, egui::FontFamily::Monospace))
                    .color(palette::text_bright(panel.state.night_mode))
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let style = ui.style_mut();
                style.visuals.hyperlink_color = palette::ACCENT;
                ui.hyperlink_to(RichText::new("🌐 skills.sh →").small(), "https://skills.sh");
                ui.add_space(12.0);
                ui.hyperlink_to(
                    RichText::new("🌐 clawhub.ai →").small(),
                    "https://clawhub.ai",
                );
                ui.add_space(12.0);
                ui.hyperlink_to(
                    RichText::new("🌐 skillhub.tencent →").small(),
                    "https://skillhub.tencent.com/",
                );
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(t("install.hint", panel.state.language))
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
        );
        ui.add_space(14.0);

        egui::Frame::new()
            .fill(panel.theme_bg_deep())
            .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(t("install.subtitle", panel.state.language))
                        .strong()
                        .color(palette::ACCENT),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t("install.paste_hint", panel.state.language))
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Source")
                            .small()
                            .color(palette::text_dim(panel.state.night_mode)),
                    );
                    ui.add_space(8.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut panel.state.store_install_url)
                            .hint_text("https://github.com/user/repo or local SKILL.md folder")
                            .desired_width(f32::INFINITY),
                    );
                    if resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !panel.state.store_installing
                    {
                        panel.state.do_install_skill(&panel.rt, ctx);
                    }
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !panel.state.store_installing,
                            egui::Button::new(RichText::new("  Choose Folder…  ").small()),
                        )
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose BenShu skill folder")
                            .pick_folder()
                        {
                            panel.state.store_install_url = path.display().to_string();
                        }
                    }

                    let btn = ui.add_enabled(
                        !panel.state.store_installing && !panel.state.store_install_url.is_empty(),
                        egui::Button::new(
                            RichText::new(if panel.state.store_installing {
                                "  Installing…  "
                            } else {
                                "  ↓ Install  "
                            })
                            .strong(),
                        ),
                    );
                    if btn.clicked() {
                        panel.state.do_install_skill(&panel.rt, ctx);
                    }
                });
            });
    });
}

pub fn render_skill_detail_window(panel: &mut ClawPanel, ctx: &egui::Context) {
    let skill_name = match &panel.state.expanded_skill {
        Some(n) => n.clone(),
        None => return,
    };

    let skill = match panel.state.skills.iter().find(|s| s.name == skill_name) {
        Some(s) => s.clone(),
        None => {
            panel.state.expanded_skill = None;
            return;
        }
    };

    let mut open = true;
    egui::Window::new(&skill.name)
        .open(&mut open)
        .resizable(true)
        .min_width(480.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::new()
                .fill(panel.theme_bg_deep())
                .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let dot = if skill.enabled { "●" } else { "○" };
                let dot_color = if skill.enabled {
                    palette::SUCCESS
                } else {
                    palette::DANGER
                };
                ui.label(RichText::new(dot).color(dot_color).strong());
                ui.label(
                    RichText::new(&skill.name)
                        .font(FontId::new(16.0, egui::FontFamily::Monospace))
                        .color(palette::text_bright(panel.state.night_mode))
                        .strong(),
                );
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(&skill.description).color(palette::text_dim(panel.state.night_mode)),
            );
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            egui::Grid::new("skill_meta_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    let mut kv = |ui: &mut egui::Ui, key: &str, val: &str| {
                        ui.label(
                            RichText::new(key)
                                .small()
                                .color(palette::text_dim(panel.state.night_mode)),
                        );
                        ui.label(
                            RichText::new(val)
                                .small()
                                .color(palette::text_bright(panel.state.night_mode))
                                .font(FontId::new(12.0, egui::FontFamily::Monospace)),
                        );
                        ui.end_row();
                    };
                    kv(ui, "Runtime:", skill.runtime.as_deref().unwrap_or("—"));
                    kv(ui, "Kind:", &skill.kind);
                    kv(ui, "Version:", skill.version.as_deref().unwrap_or("—"));
                    kv(ui, "Author:", skill.author.as_deref().unwrap_or("—"));
                    kv(
                        ui,
                        "Status:",
                        if skill.enabled { "Enabled" } else { "Disabled" },
                    );

                    if let Some(hp) = &skill.homepage {
                        ui.label(
                            RichText::new("Homepage:")
                                .small()
                                .color(palette::text_dim(panel.state.night_mode)),
                        );
                        ui.hyperlink_to(RichText::new(hp).small().color(palette::ACCENT), hp);
                        ui.end_row();
                    }
                });

            if !skill.dependencies.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Dependencies:")
                        .small()
                        .color(palette::text_dim(panel.state.night_mode)),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for dep in &skill.dependencies {
                        egui::Frame::new()
                            .fill(palette::TAG_BG)
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(dep)
                                        .small()
                                        .color(palette::ACCENT)
                                        .font(FontId::new(11.0, egui::FontFamily::Monospace)),
                                );
                            });
                    }
                });
            }
        });

    if !open {
        panel.state.expanded_skill = None;
    }
}
