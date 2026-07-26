use crate::app::ClawPanel;
use crate::common::palette;
use eframe::egui::{self, RichText};
use std::collections::BTreeSet;

const MAX_INLINE_TARGETS: usize = 6;

pub fn render_open_target_button(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    artifact_id: Option<&str>,
    target: &str,
    media_type: Option<&str>,
) {
    render_open_target_button_with_label(panel, ui, artifact_id, target, media_type, None);
}

pub fn render_open_target_button_with_label(
    panel: &mut ClawPanel,
    ui: &mut egui::Ui,
    artifact_id: Option<&str>,
    target: &str,
    media_type: Option<&str>,
    label: Option<&str>,
) {
    let target = clean_target(target);
    if target.is_empty() || !target_looks_openable(&target, media_type) {
        return;
    }

    let enabled = panel.state.open_target_promise.is_none();
    let label = label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| open_button_label(&target, media_type));
    if ui
        .add_enabled(enabled, egui::Button::new(RichText::new(label).small()))
        .on_hover_text("Open with the operating system default application")
        .clicked()
    {
        panel.state.do_open_artifact_target(
            &panel.rt,
            ui.ctx(),
            artifact_id.map(ToOwned::to_owned),
            Some(target),
        );
    }
}

pub fn render_open_targets_from_text(panel: &mut ClawPanel, ui: &mut egui::Ui, text: &str) {
    let targets = extract_open_targets(text);
    if targets.is_empty() {
        return;
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Openable targets")
                .small()
                .color(palette::text_dim(panel.state.night_mode)),
        );
        for target in targets {
            render_open_target_button(panel, ui, None, &target, None);
        }
    });
}

fn extract_open_targets(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for raw in text.split_whitespace() {
        let candidate = clean_target(raw);
        if candidate.is_empty() || !target_looks_openable(&candidate, None) {
            continue;
        }
        if seen.insert(candidate.clone()) {
            out.push(candidate);
        }
        if out.len() >= MAX_INLINE_TARGETS {
            break;
        }
    }

    out
}

fn clean_target(raw: &str) -> String {
    raw.trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | '，'
                    | '。'
                    | ';'
                    | '；'
                    | ':'
                    | '：'
            )
        })
        .to_string()
}

fn target_looks_openable(target: &str, media_type: Option<&str>) -> bool {
    let lowered = target.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") {
        return true;
    }
    if media_type
        .map(|media| media_type_looks_openable(media))
        .unwrap_or(false)
    {
        return true;
    }
    if !(target.starts_with('/')
        || lowered.starts_with("file://")
        || looks_like_windows_absolute(target)
        || target.starts_with("data/")
        || target.starts_with("./")
        || target.starts_with("../"))
    {
        return false;
    }
    file_extension(target)
        .map(|ext| allowed_open_extension(&ext) && !blocked_open_extension(&ext))
        .unwrap_or(false)
}

fn open_button_label(target: &str, media_type: Option<&str>) -> &'static str {
    if target.to_ascii_lowercase().starts_with("http") {
        return "Open link";
    }
    if let Some(media) = media_type {
        if media.contains("pdf") {
            return "Open PDF";
        }
        if media.starts_with("image/") {
            return "Open image";
        }
        if media.starts_with("audio/") {
            return "Open audio";
        }
        if media.starts_with("video/") {
            return "Open video";
        }
        if media.contains("wordprocessingml") || media.contains("msword") {
            return "Open document";
        }
        if media.contains("spreadsheetml") || media.contains("excel") {
            return "Open sheet";
        }
        if media.contains("presentationml") || media.contains("powerpoint") {
            return "Open slides";
        }
        if media.starts_with("text/") {
            return "Open text";
        }
    }
    match file_extension(target).as_deref() {
        Some("pdf") => "Open PDF",
        Some("txt" | "log" | "md" | "markdown") => "Open text",
        Some("html" | "htm") => "Open HTML",
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg") => "Open image",
        Some("mp3" | "wav" | "ogg" | "m4a" | "flac") => "Open audio",
        Some("mp4" | "mov" | "avi" | "mkv" | "webm") => "Open video",
        Some("doc" | "docx" | "rtf" | "odt") => "Open document",
        Some("xls" | "xlsx" | "ods" | "csv") => "Open sheet",
        Some("ppt" | "pptx" | "odp") => "Open slides",
        _ => "Open file",
    }
}

fn media_type_looks_openable(media_type: &str) -> bool {
    let lowered = media_type.to_ascii_lowercase();
    lowered.starts_with("text/")
        || lowered.starts_with("image/")
        || lowered.starts_with("audio/")
        || lowered.starts_with("video/")
        || lowered.contains("pdf")
        || lowered.contains("json")
        || lowered.contains("xml")
        || lowered.contains("wordprocessingml")
        || lowered.contains("spreadsheetml")
        || lowered.contains("presentationml")
}

fn file_extension(target: &str) -> Option<String> {
    let without_query = target
        .split(['?', '#'])
        .next()
        .unwrap_or(target)
        .trim_end_matches('/');
    let last = without_query
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(without_query);
    last.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty() && ext.len() <= 12)
}

fn looks_like_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || value.starts_with("\\\\")
}

fn blocked_open_extension(ext: &str) -> bool {
    matches!(
        ext,
        "exe"
            | "bat"
            | "cmd"
            | "com"
            | "ps1"
            | "psm1"
            | "vbs"
            | "vbe"
            | "js"
            | "jse"
            | "msi"
            | "msp"
            | "scr"
            | "lnk"
            | "reg"
            | "hta"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "app"
    )
}

fn allowed_open_extension(ext: &str) -> bool {
    matches!(
        ext,
        "pdf"
            | "txt"
            | "log"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "jsonl"
            | "html"
            | "htm"
            | "xml"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "svg"
            | "mp3"
            | "wav"
            | "ogg"
            | "m4a"
            | "flac"
            | "mp4"
            | "mov"
            | "avi"
            | "mkv"
            | "webm"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "ppt"
            | "pptx"
            | "rtf"
            | "odt"
            | "ods"
            | "odp"
    )
}
