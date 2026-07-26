use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{chapter_is_approved, count_units, NovelProjectManifest};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct TextScanReport {
    pub(super) chars: usize,
    pub(super) units: usize,
    pub(super) lines: usize,
    pub(super) cjk_chars: usize,
    pub(super) ascii_letters: usize,
}

impl TextScanReport {
    pub(super) fn scan(text: &str, language: &str) -> Self {
        let chars = text.chars().count();
        let lines = text.lines().count();
        let cjk_chars = text
            .chars()
            .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            .count();
        let ascii_letters = text.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
        Self {
            chars,
            units: count_units(text, language),
            lines,
            cjk_chars,
            ascii_letters,
        }
    }

    pub(super) fn add_text(&mut self, text: &str, language: &str) {
        let next = Self::scan(text, language);
        self.chars += next.chars;
        self.units += next.units;
        self.lines += next.lines;
        self.cjk_chars += next.cjk_chars;
        self.ascii_letters += next.ascii_letters;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChapterIndexEntry {
    pub(super) number: usize,
    pub(super) title: String,
    pub(super) volume_id: String,
    pub(super) volume_title: String,
    pub(super) path: String,
    pub(super) status: String,
    pub(super) unit_count: usize,
    pub(super) updated_at: String,
    pub(super) approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ProjectCache {
    pub(super) schema: String,
    pub(super) title: String,
    pub(super) language: String,
    pub(super) approved_only: bool,
    pub(super) chapter_index: Vec<ChapterIndexEntry>,
}

impl ProjectCache {
    pub(super) fn from_manifest(manifest: &NovelProjectManifest, approved_only: bool) -> Self {
        let chapter_index = manifest
            .chapters
            .iter()
            .filter(|chapter| !approved_only || chapter_is_approved(chapter))
            .map(|chapter| ChapterIndexEntry {
                number: chapter.number,
                title: chapter.title.clone(),
                volume_id: chapter.volume_id.clone(),
                volume_title: chapter.volume_title.clone(),
                path: chapter.path.clone(),
                status: chapter.status.clone(),
                unit_count: chapter.unit_count,
                updated_at: chapter.updated_at.clone(),
                approved: chapter_is_approved(chapter),
            })
            .collect();
        Self {
            schema: "benshu.novel_project_cache.v1".to_string(),
            title: super::canonical_project_title(manifest).to_string(),
            language: manifest.language.clone(),
            approved_only,
            chapter_index,
        }
    }

    pub(super) fn signature(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }

    pub(super) fn chapter_path(&self, project_dir: &Path, number: usize) -> Option<PathBuf> {
        self.chapter_index
            .iter()
            .find(|entry| entry.number == number)
            .map(|entry| project_dir.join(&entry.path))
    }
}
