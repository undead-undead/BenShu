use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSegmentKind {
    Static,
    Dynamic,
    ToolSurface,
    Background,
    Governance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSurfaceSegment {
    pub kind: PromptSegmentKind,
    pub label: String,
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSurfaceReport {
    pub profile: String,
    pub segments: Vec<PromptSurfaceSegment>,
    pub visible_tool_count: usize,
    pub deferred_tool_count: usize,
    pub total_tool_count: usize,
    pub tool_surface_mode: String,
}

impl PromptSurfaceReport {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            segments: Vec::new(),
            visible_tool_count: 0,
            deferred_tool_count: 0,
            total_tool_count: 0,
            tool_surface_mode: "unknown".to_string(),
        }
    }

    pub fn add_segment(
        &mut self,
        kind: PromptSegmentKind,
        label: impl Into<String>,
        content: &str,
    ) {
        self.add_segment_chars(kind, label, content.chars().count());
    }

    pub fn add_segment_chars(
        &mut self,
        kind: PromptSegmentKind,
        label: impl Into<String>,
        chars: usize,
    ) {
        self.segments.push(PromptSurfaceSegment {
            kind,
            label: label.into(),
            chars,
        });
    }

    pub fn set_tool_surface(
        &mut self,
        visible_tool_count: usize,
        deferred_tool_count: usize,
        total_tool_count: usize,
        tool_surface_mode: impl Into<String>,
    ) {
        self.visible_tool_count = visible_tool_count;
        self.deferred_tool_count = deferred_tool_count;
        self.total_tool_count = total_tool_count;
        self.tool_surface_mode = tool_surface_mode.into();
    }

    pub fn total_chars(&self) -> usize {
        self.segments.iter().map(|segment| segment.chars).sum()
    }

    pub fn chars_for(&self, kind: PromptSegmentKind) -> usize {
        self.segments
            .iter()
            .filter(|segment| segment.kind == kind)
            .map(|segment| segment.chars)
            .sum()
    }

    pub fn write_to_extra_params(&self, extra: &mut serde_json::Value) {
        let Some(map) = extra.as_object_mut() else {
            return;
        };

        map.insert(
            "prompt_surface_profile".to_string(),
            serde_json::json!(self.profile),
        );
        map.insert(
            "prompt_surface_total_chars".to_string(),
            serde_json::json!(self.total_chars()),
        );
        map.insert(
            "prompt_static_chars".to_string(),
            serde_json::json!(self.chars_for(PromptSegmentKind::Static)),
        );
        map.insert(
            "prompt_dynamic_chars".to_string(),
            serde_json::json!(
                self.chars_for(PromptSegmentKind::Dynamic)
                    + self.chars_for(PromptSegmentKind::Background)
            ),
        );
        map.insert(
            "prompt_governance_chars".to_string(),
            serde_json::json!(self.chars_for(PromptSegmentKind::Governance)),
        );
        map.insert(
            "prompt_tool_surface_visible_count".to_string(),
            serde_json::json!(self.visible_tool_count),
        );
        map.insert(
            "prompt_tool_surface_deferred_count".to_string(),
            serde_json::json!(self.deferred_tool_count),
        );
        map.insert(
            "prompt_tool_surface_total_count".to_string(),
            serde_json::json!(self.total_tool_count),
        );
        map.insert(
            "prompt_tool_surface_mode".to_string(),
            serde_json::json!(self.tool_surface_mode),
        );
    }
}
