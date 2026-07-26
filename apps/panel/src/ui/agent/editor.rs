use crate::app::ClawPanel;
use crate::common::{palette, task::spawn_task};
use eframe::egui::{self, Color32, RichText, Stroke};
use poll_promise::Promise;
use serde_json::{json, Map, Value};

fn artifact_policy_to_yaml(value: Value) -> String {
    benshu_brain::config::AgentConfigOverrides {
        artifact_policy: Some(value),
        ..Default::default()
    }
    .artifact_policy_yaml()
}

fn parse_artifact_policy_editor_value(raw: &str) -> Result<Value, String> {
    let parsed = benshu_brain::config::AgentConfigOverrides::parse_artifact_policy_yaml(raw)?;
    Ok(parsed.unwrap_or_else(|| json!({ "handles": [] })))
}

fn ensure_policy_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({ "handles": [] });
    }
    value.as_object_mut().expect("policy object")
}

fn ensure_handles(value: &mut Value) -> &mut Vec<Value> {
    let object = ensure_policy_object(value);
    let handles = object
        .entry("handles".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !handles.is_array() {
        *handles = Value::Array(Vec::new());
    }
    handles.as_array_mut().expect("handles array")
}

fn ensure_handle_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("handle object")
}

fn string_field(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn set_optional_string(object: &mut Map<String, Value>, key: &str, value: String) {
    let value = value.trim();
    if value.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn array_field_as_csv(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn set_csv_array(object: &mut Map<String, Value>, key: &str, csv: String) {
    let items = csv
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| Value::String(item.to_string()))
        .collect::<Vec<_>>();
    if items.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.to_string(), Value::Array(items));
    }
}

fn multiline_array_field(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn set_multiline_array(object: &mut Map<String, Value>, key: &str, text: String) {
    let items = text
        .lines()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| Value::String(item.to_string()))
        .collect::<Vec<_>>();
    if items.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.to_string(), Value::Array(items));
    }
}

fn builtin_primary_tools() -> &'static [(&'static str, &'static str)] {
    &[
        ("novel_studio", "Long-form fiction studio"),
        ("writing_studio", "Structured writing studio"),
        ("web_search", "Web search"),
        ("browser", "Interactive browser"),
        ("web_fetch", "Web page reader"),
        ("knowledge", "Knowledge search/import"),
        ("fs", "Filesystem"),
        ("git", "Git operations"),
        ("command_exec", "Command execution"),
        ("windows_control", "Windows control"),
        ("document_understand", "Document understanding"),
        ("visual", "Visual analysis"),
        ("ocr", "Text extraction"),
        ("chart", "Chart generation"),
        ("mailer", "Email"),
        ("data_transform", "Data transform"),
        ("voice", "Voice STT/TTS"),
        ("crypto", "Cipher / encryption"),
        ("notify", "Notifications"),
        ("runtime_surface", "Runtime surface"),
    ]
}

fn tool_label(tool: &str) -> String {
    builtin_primary_tools()
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(name, label)| format!("{name} — {label}"))
        .unwrap_or_else(|| tool.to_string())
}

fn ensure_tool_config<'a>(policy: &'a mut Value, tool: &str) -> &'a mut Map<String, Value> {
    let object = ensure_policy_object(policy);
    let configs = object
        .entry("tool_config".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !configs.is_object() {
        *configs = Value::Object(Map::new());
    }
    let configs = configs.as_object_mut().expect("tool_config object");
    let config = configs
        .entry(tool.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !config.is_object() {
        *config = Value::Object(Map::new());
    }
    config.as_object_mut().expect("single tool config object")
}

fn render_config_string_field(
    ui: &mut egui::Ui,
    config: &mut Map<String, Value>,
    key: &str,
    label: &str,
    hint: &str,
) -> bool {
    let mut text = string_field(config, key);
    let changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).small());
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width())
                    .hint_text(hint),
            )
            .changed()
        })
        .inner;
    if changed {
        set_optional_string(config, key, text);
    }
    changed
}

fn render_config_bool_field(
    ui: &mut egui::Ui,
    config: &mut Map<String, Value>,
    key: &str,
    label: &str,
    default: bool,
) -> bool {
    let mut value = config.get(key).and_then(Value::as_bool).unwrap_or(default);
    let changed = ui
        .checkbox(&mut value, RichText::new(label).small())
        .changed();
    if changed {
        config.insert(key.to_string(), Value::Bool(value));
    }
    changed
}

fn render_config_integer_field(
    ui: &mut egui::Ui,
    config: &mut Map<String, Value>,
    key: &str,
    label: &str,
    default: i64,
    range: std::ops::RangeInclusive<i64>,
) -> bool {
    let mut value = config.get(key).and_then(Value::as_i64).unwrap_or(default);
    let changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).small());
            ui.add(egui::DragValue::new(&mut value).range(range).speed(100.0))
                .changed()
        })
        .inner;
    if changed {
        config.insert(key.to_string(), json!(value));
    }
    changed
}

fn render_config_integer_enum_field(
    ui: &mut egui::Ui,
    config: &mut Map<String, Value>,
    key: &str,
    label: &str,
    options: &[i64],
    default: i64,
) -> bool {
    let mut value = config.get(key).and_then(Value::as_i64).unwrap_or(default);
    if !options.iter().any(|option| *option == value) {
        value = default;
    }
    let before = value;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small());
        egui::ComboBox::from_id_salt(format!("tool_config_{key}"))
            .selected_text(value.to_string())
            .show_ui(ui, |ui| {
                for option in options {
                    ui.selectable_value(&mut value, *option, option.to_string());
                }
            });
    });
    let changed = value != before;
    if changed {
        config.insert(key.to_string(), json!(value));
    }
    changed
}

fn render_config_enum_field(
    ui: &mut egui::Ui,
    config: &mut Map<String, Value>,
    key: &str,
    label: &str,
    options: &[&str],
    default: &str,
) -> bool {
    let mut value = config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string();
    let before = value.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small());
        egui::ComboBox::from_id_salt(format!("tool_config_{key}"))
            .selected_text(&value)
            .show_ui(ui, |ui| {
                for option in options {
                    ui.selectable_value(&mut value, (*option).to_string(), *option);
                }
            });
    });
    let changed = value != before;
    if changed {
        config.insert(key.to_string(), Value::String(value));
    }
    changed
}

fn render_policy_csv_field(
    ui: &mut egui::Ui,
    object: &mut Map<String, Value>,
    key: &str,
    label: &str,
    hint: &str,
) -> bool {
    let mut text = array_field_as_csv(object, key);
    let changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).small());
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width())
                    .hint_text(hint),
            )
            .changed()
        })
        .inner;
    if changed {
        set_csv_array(object, key, text);
    }
    changed
}

fn render_policy_string_field(
    ui: &mut egui::Ui,
    object: &mut Map<String, Value>,
    key: &str,
    label: &str,
    hint: &str,
) -> bool {
    let mut text = string_field(object, key);
    let changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).small());
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width())
                    .hint_text(hint),
            )
            .changed()
        })
        .inner;
    if changed {
        set_optional_string(object, key, text);
    }
    changed
}

fn render_policy_url_field(
    ui: &mut egui::Ui,
    object: &mut Map<String, Value>,
    key: &str,
    label: &str,
) -> bool {
    let mut text = multiline_array_field(object, key);
    let changed = ui
        .vertical(|ui| {
            ui.label(RichText::new(label).small());
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(ui.available_width())
                    .desired_rows(2)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("每行一个 URL"),
            )
            .changed()
        })
        .inner;
    if changed {
        set_multiline_array(object, key, text);
    }
    changed
}

fn normalize_source_adapter(value: &mut Value) -> &mut Map<String, Value> {
    if let Some(name) = value.as_str().map(str::to_string) {
        *value = json!({ "name": name });
    }
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("source adapter object")
}

fn render_source_adapter_form(
    ui: &mut egui::Ui,
    adapter: &mut Value,
    index: usize,
) -> (bool, bool) {
    let mut changed = false;
    let mut remove = false;
    let object = normalize_source_adapter(adapter);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Source #{}", index + 1))
                .small()
                .strong(),
        );
        if ui.button("Remove").clicked() {
            remove = true;
        }
    });

    changed |= render_policy_string_field(ui, object, "name", "Name", "pubmed / github / browser");

    let mut capability = string_field(object, "capability");
    egui::ComboBox::from_id_salt(format!("artifact_policy_capability_{index}"))
        .selected_text(if capability.is_empty() {
            "default".to_string()
        } else {
            capability.clone()
        })
        .show_ui(ui, |ui| {
            for value in ["", "public", "browser", "api", "cookie"] {
                let label = if value.is_empty() { "default" } else { value };
                if ui
                    .selectable_value(&mut capability, value.to_string(), label)
                    .changed()
                {
                    changed = true;
                }
            }
        });
    if changed {
        set_optional_string(object, "capability", capability);
    }

    changed |= render_policy_csv_field(
        ui,
        object,
        "domains",
        "Domains",
        "pubmed.ncbi.nlm.nih.gov, github.com",
    );
    changed |= render_policy_csv_field(
        ui,
        object,
        "fallback_sources",
        "Fallback",
        "browser, general_web",
    );

    let mut weight = object.get("weight").and_then(Value::as_f64).unwrap_or(1.0);
    if ui
        .add(egui::Slider::new(&mut weight, 0.1..=3.0).text("Weight"))
        .changed()
    {
        object.insert("weight".to_string(), json!(weight));
        changed = true;
    }

    for (key, label) in [
        ("requires_browser", "Needs browser"),
        ("requires_auth", "Needs login/cookie"),
        ("challenge_prone", "Challenge-prone site"),
    ] {
        let mut flag = object.get(key).and_then(Value::as_bool).unwrap_or(false);
        if ui.checkbox(&mut flag, label).changed() {
            if flag {
                object.insert(key.to_string(), Value::Bool(true));
            } else {
                object.remove(key);
            }
            changed = true;
        }
    }

    (changed, remove)
}

fn render_artifact_policy_form(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    ui.label(RichText::new("Artifact Policy").size(10.0).italics());
    ui.label(
        RichText::new(
            "Fill-in routing hints for this worker. YAML remains available under Advanced.",
        )
        .weak()
        .small(),
    );

    let parsed = parse_artifact_policy_editor_value(&panel.state.agent_role_artifact_policy_yaml);
    let mut policy = match parsed {
        Ok(value) => value,
        Err(err) => {
            panel.state.agent_role_artifact_policy_error = Some(err);
            ui.label(
                RichText::new(
                    "The current policy YAML cannot be rendered as a form. Fix it below.",
                )
                .small()
                .color(palette::DANGER),
            );
            render_artifact_policy_yaml_editor(panel, ui);
            return;
        }
    };

    let mut changed = false;
    let mut remove_index = None;
    let handles = ensure_handles(&mut policy);

    if handles.is_empty() {
        ui.label(
            RichText::new(
                "No policy rules yet. Add one when this worker should be discoverable by intent.",
            )
            .weak()
            .small(),
        );
    }

    for (index, handle) in handles.iter_mut().enumerate() {
        let title = {
            let object = ensure_handle_object(handle);
            let artifact = string_field(object, "artifact");
            if artifact.is_empty() {
                format!("Rule {}", index + 1)
            } else {
                format!("Rule {}: {}", index + 1, artifact)
            }
        };

        egui::CollapsingHeader::new(title)
            .default_open(index == 0)
            .show(ui, |ui| {
                let object = ensure_handle_object(handle);
                ui.horizontal(|ui| {
                    if ui.button("Use selected tools").clicked() {
                        object.insert(
                            "tools".to_string(),
                            Value::Array(
                                panel
                                    .state
                                    .agent_role_tools
                                    .iter()
                                    .cloned()
                                    .map(Value::String)
                                    .collect(),
                            ),
                        );
                        changed = true;
                    }
                    if ui.button("Remove rule").clicked() {
                        remove_index = Some(index);
                    }
                });
                changed |= render_policy_string_field(
                    ui,
                    object,
                    "artifact",
                    "Artifact",
                    "academic_paper / web_page / knowledge_import",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "intents",
                    "Intents",
                    "search, import, parse",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "triggers",
                    "Triggers",
                    "论文, GitHub, 保存进知识库",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "tools",
                    "Tools",
                    "web_search, web_fetch, browser",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "preferred_hosts",
                    "Preferred hosts",
                    "pubmed.ncbi.nlm.nih.gov, github.com",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "domains",
                    "Domains",
                    "pubmed.ncbi.nlm.nih.gov, github.com",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "evidence_hints",
                    "Evidence hints",
                    "official, record, open access",
                );
                changed |= render_policy_csv_field(
                    ui,
                    object,
                    "direct_record_hints",
                    "Record hints",
                    "doi, repo, issue",
                );
                changed |= render_policy_url_field(ui, object, "seed_urls", "Seed URLs");
                changed |= render_policy_url_field(ui, object, "record_urls", "Record URLs");

                ui.add_space(4.0);
                ui.label(RichText::new("Source adapters").small().strong());
                let adapters = object
                    .entry("source_adapters".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if !adapters.is_array() {
                    *adapters = Value::Array(Vec::new());
                    changed = true;
                }
                let adapters = adapters.as_array_mut().expect("source adapters array");
                let mut adapter_to_remove = None;
                for (adapter_index, adapter) in adapters.iter_mut().enumerate() {
                    ui.group(|ui| {
                        let (adapter_changed, remove) =
                            render_source_adapter_form(ui, adapter, adapter_index);
                        changed |= adapter_changed;
                        if remove {
                            adapter_to_remove = Some(adapter_index);
                        }
                    });
                }
                if let Some(adapter_index) = adapter_to_remove {
                    adapters.remove(adapter_index);
                    changed = true;
                }
                if ui.button("+ Add source adapter").clicked() {
                    adapters.push(json!({
                        "name": "browser",
                        "capability": "browser",
                        "weight": 1.0
                    }));
                    changed = true;
                }
                if adapters.is_empty() {
                    object.remove("source_adapters");
                }
            });
    }

    if let Some(index) = remove_index {
        handles.remove(index);
        changed = true;
    }

    ui.horizontal(|ui| {
        if ui.button("+ Add policy rule").clicked() {
            ensure_handles(&mut policy).push(json!({
                "artifact": "web_page",
                "intents": ["search"],
                "triggers": ["搜索"],
                "tools": panel.state.agent_role_tools.clone()
            }));
            changed = true;
        }
        if ui.button("Clear policy").clicked() {
            policy = json!({ "handles": [] });
            changed = true;
        }
    });

    if changed {
        panel.state.agent_role_artifact_policy_yaml = artifact_policy_to_yaml(policy);
        panel.state.agent_role_artifact_policy_error = None;
        panel.state.agent_role_artifact_policy_dirty = true;
    }

    render_artifact_policy_yaml_editor(panel, ui);
}

fn render_primary_tool_selector(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    ui.label(RichText::new("Primary Tool").size(10.0).italics());
    ui.label(
        RichText::new(
            "A worker is single-purpose: pick one tool, confirm it, then configure that tool below.",
        )
        .weak()
        .small(),
    );

    if panel.state.agent_role_pending_tool.is_empty() {
        panel.state.agent_role_pending_tool = panel
            .state
            .agent_role_tools
            .first()
            .cloned()
            .unwrap_or_default();
    }

    let selected_text = if panel.state.agent_role_pending_tool.trim().is_empty() {
        "Select a tool".to_string()
    } else {
        tool_label(&panel.state.agent_role_pending_tool)
    };

    egui::ComboBox::from_id_salt("agent_primary_tool")
        .selected_text(selected_text)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut panel.state.agent_role_pending_tool,
                String::new(),
                "No primary tool",
            );
            for (name, label) in builtin_primary_tools() {
                ui.selectable_value(
                    &mut panel.state.agent_role_pending_tool,
                    (*name).to_string(),
                    format!("{name} — {label}"),
                );
            }

            if !panel.state.skills.is_empty() {
                ui.separator();
                ui.label(RichText::new("Installed local skills").small().weak());
                for skill in &panel.state.skills {
                    let label = if skill.description.is_empty() {
                        format!("{} — local skill", skill.name)
                    } else {
                        format!("{} — {}", skill.name, skill.description)
                    };
                    ui.selectable_value(
                        &mut panel.state.agent_role_pending_tool,
                        skill.name.clone(),
                        label,
                    );
                }
            }
        });

    let confirmed = panel
        .state
        .agent_role_tools
        .first()
        .map(String::as_str)
        .unwrap_or_default();
    let needs_confirm = panel.state.agent_role_tools.len() != 1
        || confirmed != panel.state.agent_role_pending_tool.trim();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(needs_confirm, egui::Button::new("Confirm tool"))
            .clicked()
        {
            panel.state.confirm_agent_primary_tool();
        }
        if panel.state.agent_role_tools.len() > 1 {
            ui.label(
                RichText::new(format!(
                    "Legacy multi-tool worker detected: {} tools. Confirming will keep only the selected primary tool.",
                    panel.state.agent_role_tools.len()
                ))
                .small()
                .color(palette::WARNING),
            );
        } else if let Some(tool) = panel.state.agent_role_tools.first() {
            ui.label(RichText::new(format!("Confirmed: {}", tool_label(tool))).small().weak());
        }
    });
}

fn format_units(units: u64) -> String {
    if units >= 10_000 {
        format!("{:.1} 万", units as f64 / 10_000.0)
    } else {
        units.to_string()
    }
}

fn render_novel_project_panel(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    if panel.state.novel_projects.is_empty()
        && panel.state.pending_novel_projects_promise.is_none()
        && !panel.state.novel_projects_loading
        && panel.state.novel_projects_root.is_empty()
        && panel.state.novel_projects_error.is_none()
    {
        panel.state.do_novel_projects_refresh(&panel.rt, ctx);
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(RichText::new("Novel Projects").size(10.0).italics());
    ui.label(
        RichText::new(
            "Projects written by this tool are stored as artifacts. Chat shows progress; full text stays in files.",
        )
        .weak()
        .small(),
    );

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !panel.state.novel_projects_loading,
                egui::Button::new("Refresh projects"),
            )
            .clicked()
        {
            panel.state.do_novel_projects_refresh(&panel.rt, ctx);
        }
        if panel.state.novel_projects_loading {
            ui.spinner();
            ui.label(RichText::new("Loading...").small().weak());
        } else if !panel.state.novel_projects_root.is_empty() {
            ui.label(
                RichText::new(format!("Root: {}", panel.state.novel_projects_root))
                    .small()
                    .weak(),
            );
        }
    });

    if let Some(error) = &panel.state.novel_projects_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    }

    if panel.state.novel_projects.is_empty() {
        ui.label(
            RichText::new("No novel projects found yet. Start a writing task from chat first.")
                .weak()
                .small(),
        );
        return;
    }

    let mut selected = panel
        .state
        .selected_novel_project_path
        .clone()
        .unwrap_or_else(|| panel.state.novel_projects[0].path.clone());
    let selected_label = panel
        .state
        .novel_projects
        .iter()
        .find(|project| project.path == selected)
        .map(|project| {
            format!(
                "{} · {}章 · {}字",
                if project.title.trim().is_empty() {
                    &project.id
                } else {
                    &project.title
                },
                project.chapter_count,
                format_units(project.total_units)
            )
        })
        .unwrap_or_else(|| "Select a project".to_string());

    egui::ComboBox::from_id_salt("novel_project_selector")
        .selected_text(selected_label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for project in &panel.state.novel_projects {
                let title = if project.title.trim().is_empty() {
                    &project.id
                } else {
                    &project.title
                };
                ui.selectable_value(
                    &mut selected,
                    project.path.clone(),
                    format!(
                        "{} · {}章 · {}字",
                        title,
                        project.chapter_count,
                        format_units(project.total_units)
                    ),
                );
            }
        });
    panel.state.selected_novel_project_path = Some(selected.clone());

    let Some(project) = panel
        .state
        .novel_projects
        .iter()
        .find(|project| project.path == selected)
        .cloned()
    else {
        return;
    };

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("Chapters: {}", project.chapter_count)).small());
        ui.label(RichText::new(format!("Approved: {}", project.approved_chapters)).small());
        ui.label(RichText::new(format!("Drafts: {}", project.drafted_chapters)).small());
        ui.label(
            RichText::new(format!(
                "Needs revision: {}",
                project.needs_revision_chapters
            ))
            .small(),
        );
        if let Some(target) = project.target_units {
            ui.label(
                RichText::new(format!(
                    "Progress: {} / {}",
                    format_units(project.total_units),
                    format_units(target)
                ))
                .small(),
            );
        } else {
            ui.label(
                RichText::new(format!("Total: {}", format_units(project.total_units))).small(),
            );
        }
    });
    if !project.path.is_empty() {
        ui.label(
            RichText::new(format!("Project path: {}", project.path))
                .small()
                .weak(),
        );
    }
    if let Some(path) = &project.latest_export_path {
        ui.label(
            RichText::new(format!("Latest export: {}", path))
                .small()
                .weak(),
        );
    }

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("novel_export_format")
            .selected_text(&panel.state.novel_export_format)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut panel.state.novel_export_format,
                    "txt".to_string(),
                    "TXT",
                );
                ui.selectable_value(
                    &mut panel.state.novel_export_format,
                    "md".to_string(),
                    "Markdown",
                );
            });
        ui.checkbox(
            &mut panel.state.novel_export_approved_only,
            RichText::new("Approved chapters only").small(),
        );
        if ui
            .add_enabled(
                !panel.state.novel_export_loading,
                egui::Button::new("Export selected project"),
            )
            .clicked()
        {
            panel.state.do_novel_export(
                &panel.rt,
                ctx,
                project.path.clone(),
                panel.state.novel_export_format.clone(),
                panel.state.novel_export_approved_only,
            );
        }
        if panel.state.novel_export_loading {
            ui.spinner();
        }
    });

    if let Some(error) = &panel.state.novel_export_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    }
    if let Some(report) = &panel.state.last_novel_export {
        if report.exported {
            if let Some(path) = &report.output_path {
                ui.label(
                    RichText::new(format!("Exported: {}", path))
                        .small()
                        .strong(),
                );
            }
        }
    }
}

fn render_primary_tool_config(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    let Some(tool) = panel.state.agent_role_tools.first().cloned() else {
        ui.label(
            RichText::new("Confirm a primary tool to show its configuration.")
                .weak()
                .small(),
        );
        render_artifact_policy_yaml_editor(panel, ui);
        return;
    };

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!("{} Config", tool_label(&tool)))
            .size(10.0)
            .italics(),
    );
    ui.label(
        RichText::new(
            "These defaults belong to this worker-tool pairing. Runtime user requests can override them for one task.",
        )
        .weak()
        .small(),
    );

    let parsed = parse_artifact_policy_editor_value(&panel.state.agent_role_artifact_policy_yaml);
    let mut policy = match parsed {
        Ok(value) => value,
        Err(err) => {
            panel.state.agent_role_artifact_policy_error = Some(err);
            ui.label(
                RichText::new(
                    "The current policy YAML cannot be rendered as a form. Fix it below.",
                )
                .small()
                .color(palette::DANGER),
            );
            render_artifact_policy_yaml_editor(panel, ui);
            return;
        }
    };

    let mut changed = false;
    {
        let config = ensure_tool_config(&mut policy, &tool);
        match tool.as_str() {
            "novel_studio" | "novel" => {
                changed |= render_config_string_field(ui, config, "language", "Language", "zh-CN");
                changed |=
                    render_config_string_field(ui, config, "genre", "Genre", "玄幻 / 科幻 / 言情");
                changed |= render_config_integer_field(
                    ui,
                    config,
                    "target_units",
                    "Default target characters",
                    500_000,
                    1_000..=10_000_000,
                );
                changed |= render_config_integer_enum_field(
                    ui,
                    config,
                    "chapter_unit_target",
                    "Chapter character target",
                    &[2_500, 5_000],
                    2_500,
                );
                changed |= render_config_enum_field(
                    ui,
                    config,
                    "export_format",
                    "Export format",
                    &["txt", "md"],
                    "txt",
                );
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "export_when_complete",
                    "Auto export",
                    true,
                );
                changed |=
                    render_config_bool_field(ui, config, "audit_enabled", "Audit chapters", true);
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "continuity_enabled",
                    "Maintain truth/continuity",
                    true,
                );
            }
            "writing_studio" | "writing" => {
                changed |= render_config_string_field(
                    ui,
                    config,
                    "document_type",
                    "Document type",
                    "paper / report / essay / copy",
                );
                changed |= render_config_string_field(ui, config, "language", "Language", "zh-CN");
                changed |=
                    render_config_string_field(ui, config, "audience", "Audience", "general");
                changed |= render_config_enum_field(
                    ui,
                    config,
                    "export_format",
                    "Export format",
                    &["txt", "md"],
                    "md",
                );
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "evidence_required",
                    "Require evidence",
                    false,
                );
                changed |=
                    render_config_bool_field(ui, config, "audit_enabled", "Audit sections", true);
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "revise_enabled",
                    "Revise after audit",
                    true,
                );
            }
            "web_search" => {
                changed |= render_config_integer_field(
                    ui,
                    config,
                    "max_results",
                    "Default max results",
                    10,
                    1..=50,
                );
                changed |= render_config_string_field(ui, config, "language", "Language", "auto");
                changed |= render_config_string_field(ui, config, "region", "Region", "auto");
                changed |=
                    render_config_bool_field(ui, config, "structured", "Structured result", true);
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "allow_browser_fallback",
                    "Allow browser fallback",
                    true,
                );
            }
            "browser" | "browser_browse" => {
                changed |= render_config_enum_field(
                    ui,
                    config,
                    "mode",
                    "Browser mode",
                    &["auto", "headed", "headless"],
                    "auto",
                );
                changed |= render_config_enum_field(
                    ui,
                    config,
                    "wait_until",
                    "Wait until",
                    &["domcontentloaded", "load", "networkidle"],
                    "domcontentloaded",
                );
                changed |= render_config_integer_field(
                    ui,
                    config,
                    "max_results",
                    "Default max results",
                    5,
                    1..=50,
                );
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "preserve_session",
                    "Preserve session",
                    false,
                );
                changed |=
                    render_config_bool_field(ui, config, "safe_clicks", "Safe click guard", true);
            }
            "web_fetch" => {
                changed |= render_config_integer_field(
                    ui,
                    config,
                    "max_chars",
                    "Context preview characters",
                    20_000,
                    1_000..=500_000,
                );
                changed |=
                    render_config_bool_field(ui, config, "structured", "Structured result", true);
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "save_large_result",
                    "Save large result as artifact",
                    true,
                );
            }
            "knowledge" => {
                changed |= render_config_string_field(
                    ui,
                    config,
                    "default_collection",
                    "Default collection",
                    "default",
                );
                changed |= render_config_bool_field(
                    ui,
                    config,
                    "allow_import",
                    "Allow knowledge import",
                    true,
                );
                changed |= render_config_integer_field(
                    ui,
                    config,
                    "retrieval_limit",
                    "Retrieval limit",
                    8,
                    1..=50,
                );
            }
            _ => {
                ui.label(
                    RichText::new(
                        "This tool has no dedicated panel form yet. Use Advanced YAML for worker-tool defaults.",
                    )
                    .weak()
                    .small(),
                );
            }
        }
    }

    if changed {
        panel.state.agent_role_artifact_policy_yaml = artifact_policy_to_yaml(policy);
        panel.state.agent_role_artifact_policy_error = None;
        panel.state.agent_role_artifact_policy_dirty = true;
    }

    if matches!(tool.as_str(), "novel_studio" | "novel") {
        let ctx = ui.ctx().clone();
        render_novel_project_panel(panel, ui, &ctx);
    }

    render_artifact_policy_form(panel, ui);
}

fn render_artifact_policy_yaml_editor(panel: &mut ClawPanel, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Advanced YAML")
        .default_open(false)
        .show(ui, |ui| {
            let policy_resp = ui.add(
                egui::TextEdit::multiline(&mut panel.state.agent_role_artifact_policy_yaml)
                    .desired_width(ui.available_width())
                    .desired_rows(6)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("handles:\n  - artifact: web_page\n    triggers: [网页, website]"),
            );
            if policy_resp.changed() {
                panel.state.agent_role_artifact_policy_dirty = true;
                panel.state.agent_role_artifact_policy_error = None;
            }
        });
    if let Some(error) = &panel.state.agent_role_artifact_policy_error {
        ui.label(RichText::new(error).small().color(palette::DANGER));
    }
}

pub fn render_agent_editor(panel: &mut ClawPanel, ui: &mut egui::Ui, ctx: &egui::Context) {
    if !panel.state.agent_role_loaded && panel.state.agent_role_promise.is_none() {
        panel.state.do_load_agent(&panel.rt, ctx);
    }
    if panel.state.agent_list_promise.is_none() && panel.state.agent_list.is_empty() {
        panel.state.do_agent_refresh(&panel.rt, ctx);
    }
    if panel.state.local_model_artifacts.is_none()
        && panel.state.pending_local_model_artifacts_promise.is_none()
    {
        panel.state.do_local_model_artifacts_refresh(&panel.rt, ctx);
    }

    let screen_height = ctx.screen_rect().height();
    let is_primary_agent = panel.state.agent_role_selected == "benshu";
    // let desired_height = (screen_height - 350.0).max(400.0);

    // ── Header Section ──────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui.button("⮜ Back to Hub").clicked() {
            panel.state.is_adding_agent = false;
            panel.state.is_editing_identity = false;
        }
        ui.add_space(8.0);
        ui.heading(
            RichText::new("Agent Identity Editor")
                .color(palette::text_bright(panel.state.night_mode)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("💾 Save").strong().color(Color32::WHITE))
                        .fill(palette::ACCENT)
                        .min_size(egui::vec2(100.0, 32.0)),
                )
                .clicked()
            {
                let save_agent = panel.state.agent_role_dirty || panel.state.is_adding_agent;
                let save_policy = panel.state.agent_role_artifact_policy_dirty || save_agent;

                if save_agent {
                    panel.state.update_agent_content_from_fields();
                }
                if panel.state.agent_role_artifact_policy_error.is_some() {
                    return;
                }

                let (sender, promise) = Promise::new();
                let client = panel.state.client.clone();
                let role = panel.state.agent_role_selected.clone();
                let agent_update = if save_agent {
                    Some((panel.state.agent_role_content.clone(), None))
                } else {
                    None
                };
                let artifact_policy = if save_policy {
                    panel.state.current_agent_artifact_policy()
                } else {
                    None
                };
                if panel.state.agent_role_artifact_policy_error.is_some() {
                    return;
                }
                let ctx2 = ctx.clone();
                let rt = panel.rt.clone();

                spawn_task(&rt, async move {
                    let res = async {
                        if let Some((content, runtime)) = agent_update {
                            client.put_agent(&role, content, runtime, None).await?;
                        }
                        if save_policy {
                            client
                                .put_agent_artifact_policy(&role, artifact_policy)
                                .await?;
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    .map_err(|e| e.to_string());
                    sender.send(res);
                    ctx2.request_repaint();
                });

                panel.state.agent_save_promise = Some(promise);
                panel.state.agent_role_dirty = false;
                panel.state.agent_role_artifact_policy_dirty = false;
                panel.state.is_adding_agent = false;
                panel.state.is_editing_identity = false;
                panel.state.do_agent_refresh(&panel.rt, ctx);
            }
        });
    });
    ui.add_space(4.0);

    // ── Parameters Grid ──────────────────────────────────────────────────
    egui::Frame::new()
        .fill(panel.theme_bg_deep().gamma_multiply(0.5))
        .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            egui::Grid::new("agent_editor_grid_v22")
                .num_columns(2)
                .spacing([24.0, 14.0])
                .min_col_width(100.0)
                .show(ui, |ui| {

                    // ROW 1: Identity & Identification
                    ui.label(RichText::new("IDENTITY").strong().color(palette::ACCENT).small());
                    ui.horizontal(|ui| {
                        // Machine ID is now internal/immutable
                        ui.label("Display Name:");
                        let name_resp = ui.add(egui::TextEdit::singleline(&mut panel.state.agent_role_name)
                            .desired_width(180.0)
                            .hint_text("Human-readable Name...")
                            .interactive(panel.state.agent_role_selected != "benshu")); // Block renaming for default agent
                        if name_resp.changed() { panel.state.agent_role_dirty = true; }

                        ui.add_space(20.0);
                        if is_primary_agent {
                            ui.label(
                                RichText::new(
                                    "Model runtime is configured in Models, not on the agent identity page.",
                                )
                                .small()
                                .color(palette::text_dim(panel.state.night_mode)),
                            );
                        } else {
                            ui.label(
                                RichText::new("Worker agents inherit runtime from BenShu.")
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                        }
                    });
                    ui.end_row();

                    // ROW 1.5: Short Description
                    ui.label(RichText::new("").small()); // Gap
                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        let desc_resp = ui.add(egui::TextEdit::singleline(&mut panel.state.agent_role_description)
                            .desired_width(ui.available_width() - 100.0)
                            .hint_text("Short tagline for this agent..."));
                        if desc_resp.changed() { panel.state.agent_role_dirty = true; }
                    });
                    ui.end_row();

                    if is_primary_agent {
                        // ROW 2: Backend Selection & Authentication Status
                        ui.label(RichText::new("BACKEND").strong().color(palette::ACCENT).small());
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("BenShu uses the active model selected in Models. This page only edits identity, memory behavior, and tool/worker policy.")
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                            ui.label(
                                RichText::new("Worker agents inherit the active runtime and only expose their own tools/routing policy.")
                                    .small()
                                    .color(palette::text_dim(panel.state.night_mode)),
                            );
                        });
                        ui.end_row();

                        // ROW 3: Reasoning & Advanced
                        ui.label(RichText::new("REASONING").strong().color(palette::ACCENT).small());
                        ui.horizontal(|ui| {
                            let resp = ui.checkbox(&mut panel.state.agent_role_auto_consolidation, RichText::new("Reflection: Consolidate Memory").strong());
                            if resp.changed() { panel.state.agent_role_dirty = true; }

                        // (i) Use the most compatible ASCII-friendly structure for now to ensure no rendering issues on Linux/Win/Mac
                        let info_btn = ui.add(egui::Button::new(RichText::new("(i) INFO").color(palette::INFO).small().strong()).frame(false));

                        info_btn.on_hover_ui(|ui: &mut egui::Ui| {
                            ui.set_max_width(320.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Cognitive Reflection Loop (AgentOS)").strong().color(palette::ACCENT));
                                ui.add_space(4.0);
                                ui.separator();
                                ui.label(RichText::new("Autonomous background intelligence that processes during idle/sleep periods:").small());
                                ui.label("• Purification: Compresses chat history into stable facts.");
                                ui.label("• Conflict Resolution: Identifies and fixes semantic paradoxes.");
                                ui.label("• Memory Thinning: Decays low-relevance seasonal noise.");
                                ui.label("• Distillation: Extracts abstract principles from experiences.");
                                ui.label("• Context Aging: Safely transitions old data to deep archives.");
                                ui.add_space(4.0);
                                ui.label(RichText::new("Ensures long-term stability and prevents hallucination 'drift'.").italics().size(9.0).color(palette::text_dim(panel.state.night_mode)));
                            });
                        });

                        });
                        ui.end_row();
                    }
                });
        });

    ui.add_space(16.0);

    ui.columns(3, |columns| {
        // COLUMN A: Personality Matrix (OCEAN)
        columns[0].vertical(|ui| {
             ui.label(RichText::new(if is_primary_agent { "Personality Matrix & Tuning" } else { "Worker Tuning" }).strong().color(palette::ACCENT).small());
             ui.add_space(4.0);
             egui::Frame::new()
                .fill(panel.theme_bg_deep().gamma_multiply(0.3))
                .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Model Temperature:").small());
                        let mut temp: f32 = panel.state.agent_role_temperature.parse().unwrap_or(0.7);
                        let slider = egui::Slider::new(&mut temp, 0.0..=2.0)
                            .step_by(0.1)
                            .show_value(true);
                        if ui.add(slider).changed() {
                            panel.state.agent_role_temperature = format!("{:.1}", temp);
                            panel.state.agent_role_dirty = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    if is_primary_agent {
                        egui::Grid::new("ocean_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                            let labels = [
                                ("Openness", &mut panel.state.agent_ocean_openness),
                                ("Conscientiousness", &mut panel.state.agent_ocean_conscientiousness),
                                ("Extraversion", &mut panel.state.agent_ocean_extraversion),
                                ("Agreeableness", &mut panel.state.agent_ocean_agreeableness),
                                ("Neuroticism", &mut panel.state.agent_ocean_neuroticism),
                            ];
                            for (label, val) in labels {
                                ui.label(RichText::new(label).small());
                                if ui.add(egui::Slider::new(val, 0.0..=10.0).show_value(true)).changed() {
                                    panel.state.agent_role_dirty = true;
                                }
                                ui.end_row();
                            }
                        });
                    } else {
                        ui.label(
                            RichText::new(
                                "Workers are single-purpose executors. They inherit BenShu's runtime and only need temperature, tools, and routing policy.",
                            )
                            .small()
                            .color(palette::text_dim(panel.state.night_mode)),
                        );
                    }
                });
        });

        // COLUMN B: Capability Authorization (Skill List)
        columns[1].vertical(|ui| {
            ui.label(RichText::new("Capability Authorization").strong().color(palette::ACCENT).small());
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(panel.theme_bg_deep().gamma_multiply(0.3))
                .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_height(145.0);
                    egui::ScrollArea::vertical()
                        .id_salt("skill_auth_scroll")
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                if is_primary_agent {
                                    ui.label(
                                        RichText::new(
                                            "BenShu owns chat, memory, routing, delegation, and voice. Worker tools are configured on worker agents.",
                                        )
                                        .weak()
                                        .small(),
                                    );
                                    render_artifact_policy_yaml_editor(panel, ui);
                                } else {
                                    render_primary_tool_selector(panel, ui);
                                    render_primary_tool_config(panel, ui);
                                }
                            });
                        });
                });
        });

        // COLUMN C: Persona & Narrative (Tone & Backstory)
        columns[2].vertical(|ui| {
            ui.label(RichText::new(if is_primary_agent { "Persona & Tone" } else { "Worker Scope" }).strong().color(palette::ACCENT).small());
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(panel.theme_bg_deep().gamma_multiply(0.3))
                .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_height(145.0);
                    ui.vertical(|ui| {
                        if is_primary_agent {
                            ui.label(RichText::new("Role Tone (Narrative Style):").size(10.0).italics());
                            let tone_resp = ui.add(egui::TextEdit::singleline(&mut panel.state.agent_role_tone)
                                .desired_width(ui.available_width())
                                .hint_text("e.g., Professional, Casual, Socratic..."));
                            if tone_resp.changed() { panel.state.agent_role_dirty = true; }

                            ui.add_space(8.0);
                            ui.label(RichText::new("Narrative Backstory / Goal:").size(10.0).italics());
                            let back_resp = ui.add(egui::TextEdit::multiline(&mut panel.state.agent_role_backstory)
                                .desired_width(ui.available_width())
                                .desired_rows(3)
                                .hint_text("Agent's history, mission, or specific motivation..."));
                            if back_resp.changed() { panel.state.agent_role_dirty = true; }
                        } else {
                            ui.label(
                                RichText::new(
                                    "Keep worker behavior narrow. Use the unified core text below for role instructions, and use tools/artifact policy for what BenShu may delegate here.",
                                )
                                .small()
                                .color(palette::text_dim(panel.state.night_mode)),
                            );
                        }
                    });
                });
        });
    });

    ui.add_space(16.0);

    // ── AGENT & IDENTITY Unified Editor ───────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("UNIFIED CORE (AGENT.md + IDENTITY.md)")
                .color(palette::ACCENT)
                .small()
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("AgentOS Multi-Layer Sync Active")
                    .size(9.0)
                    .color(palette::SUCCESS),
            );
        });
    });

    let mut role_content = panel.state.agent_role_content.clone();
    let editor_height = (screen_height - 300.0).clamp(300.0, 700.0);

    egui::Frame::new()
        .fill(panel.theme_bg_deep())
        .stroke(Stroke::new(1.0, palette::border(panel.state.night_mode)))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("prompt_v22")
                .min_scrolled_height(editor_height)
                .max_height(editor_height)
                .show(ui, |ui| {
                    let response = ui.add_sized(
                        [ui.available_width(), editor_height],
                        egui::TextEdit::multiline(&mut role_content)
                            .hint_text(
                                "Enter the core system personality and behavioral rules here...",
                            )
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .lock_focus(true)
                            .frame(false),
                    );
                    if response.changed() {
                        panel.state.agent_role_content = role_content;
                        panel.state.agent_role_dirty = true;
                        panel.state.update_agent_fields_from_content(None, None);
                    }
                });
        });
}
