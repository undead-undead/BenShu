use serde_json::json;

use crate::tool::writing::longform_policy;

pub(super) fn parameters_ts() -> String {
    r#"interface NovelStudioArgs {
  action: 'list_projects' | 'draft_project' | 'update_draft' | 'show_draft' | 'approve_draft' | 'discard_draft' | 'init_project' | 'update_project' | 'clone_project' | 'add_source' | 'import_chapters' | 'update_style' | 'read_style' | 'set_contract' | 'run_next_chapter' | 'run_project' | 'persist_execution_package' | 'write_draft' | 'audit_chapter' | 'repair_chapter_metadata' | 'revise_draft' | 'settle_chapter_state' | 'validate_chapter_state' | 'repair_project_state' | 'compose_context' | 'add_chapter' | 'read_chapter' | 'review_chapter' | 'revise_chapter' | 'approve_chapter' | 'reject_chapter' | 'approve_all' | 'read_truth' | 'snapshot' | 'restore_snapshot' | 'analytics' | 'audit' | 'status' | 'export';
  project_path?: string;
  draft_path?: string;
  output_root?: string;
  source_project_path?: string;
  snapshot_id?: string;
  overwrite?: boolean;
  allow_title_conflict?: boolean;
  approved_only?: boolean;
  include_draft?: boolean;
  minimal_context?: boolean;
  title?: string;
  language?: string;
  genre?: string;
  brief?: string;
  target_units?: number;
  chapter_unit_target?: 2500 | 5000;
  max_chapters_per_turn?: number;
  source_title?: string;
  source_url?: string;
  notes?: string;
  content?: string;
  split_pattern?: string;
  premise?: string;
  ending_direction?: string;
  authority_contract?: Record<string, unknown>;
  protagonist_arc?: string;
  world_imagery?: string;
  main_causal_spine?: string;
  title_rationale?: string;
  themes?: string[];
  characters?: string[];
  world_rules?: string[];
  style_rules?: string[];
  must_avoid?: string[];
  outline?: string;
  field_requirements?: Record<string, string>;
  resource_economy?: Record<string, unknown>;
  emotional_contract?: Record<string, unknown>;
  emotional_state_ledger?: Record<string, unknown>[];
  relationship_ledger?: Record<string, unknown>[];
  power_progression?: Record<string, unknown>;
  social_order?: Record<string, unknown>;
  geography_model?: Record<string, unknown>;
  time_model?: Record<string, unknown>;
  artifact_ledger?: Record<string, unknown>[];
  antagonist_pressure?: Record<string, unknown>;
  payoff_matrix?: Record<string, unknown>[];
  narration_contract?: Record<string, unknown>;
  scene_type_mix?: Record<string, unknown>;
  character_voice_ledger?: Record<string, unknown>[];
  reader_promise?: Record<string, unknown>;
  chapter_ending_rotation?: Record<string, unknown>;
  conflict_pressure_curve?: Record<string, unknown>;
  motif_ledger?: Record<string, unknown>[];
  reveal_schedule?: Record<string, unknown>[];
  relationship_interaction_quotas?: Record<string, unknown>[];
  plan?: string;
  chapter_number?: number;
  chapter_title?: string;
  scene_goal?: string;
  conflict?: string;
  choice?: string;
  cost?: string;
  reveal?: string;
  emotional_beat?: string;
  relationship_delta?: string;
  power_delta?: string;
  resource_delta?: string;
  hook_opened?: string[];
  hook_paid_off?: string[];
  character_change?: string;
  world_change?: string;
  payoff_target?: string;
  new_character_requests?: Record<string, unknown>[];
  summary?: string;
  key_facts?: string[];
  continuity_updates?: string[];
  issues?: string[];
  findings?: Record<string, unknown>[];
  advisories?: string[];
  score?: number;
  feedback?: string;
  verdict?: string;
  section?: string;
  revision_notes?: string;
  status?: string;
  format?: 'txt' | 'md';
  output?: string;
  export_when_complete?: boolean;
}"#
        .to_string()
}

pub(super) const PUBLIC_ACTIONS: &[&str] = &[
    "list_projects",
    "draft_project",
    "update_draft",
    "show_draft",
    "approve_draft",
    "discard_draft",
    "init_project",
    "update_project",
    "clone_project",
    "add_source",
    "import_chapters",
    "update_style",
    "read_style",
    "set_contract",
    "run_next_chapter",
    "run_project",
    "persist_execution_package",
    "write_draft",
    "audit_chapter",
    "repair_chapter_metadata",
    "revise_draft",
    "settle_chapter_state",
    "validate_chapter_state",
    "repair_project_state",
    "compose_context",
    "add_chapter",
    "read_chapter",
    "review_chapter",
    "revise_chapter",
    "approve_chapter",
    "reject_chapter",
    "approve_all",
    "read_truth",
    "snapshot",
    "restore_snapshot",
    "analytics",
    "audit",
    "status",
    "export",
];

#[cfg(test)]
pub(super) const INTERNAL_COMPAT_ACTIONS: &[&str] = &[
    "compose_chapter",
    "plan_chapter",
    "architect_chapter",
    "add_chapter_plan",
    "repair_latest_chapter_metadata",
    "record_candidate_decision",
];

pub(super) fn public_actions_json() -> serde_json::Value {
    json!(PUBLIC_ACTIONS)
}

pub(super) fn internal_compat_action_hint(action: &str) -> Option<&'static str> {
    match action {
        "compose_chapter" => Some("Use `compose_context` to build a context package, or `write_draft` when you already have chapter body content."),
        "plan_chapter" | "architect_chapter" | "add_chapter_plan" => {
            Some("Use `persist_execution_package` for planned chapter execution packages, or `run_next_chapter` / `run_project` for the normal workflow.")
        }
        "repair_latest_chapter_metadata" => Some("Use the public `repair_chapter_metadata` action."),
        "record_candidate_decision" => Some(
            "Use the public `revise_draft` workflow; candidate scoring and best-version persistence are internal.",
        ),
        _ => None,
    }
}

pub(super) fn novel_studio_parameters() -> serde_json::Value {
    let mut schema: serde_json::Value = serde_json::from_str(
        r#"{
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [],
                    "description": "Operation to perform."
                },
                "project_path": { "type": "string" },
                "draft_path": { "type": "string" },
                "output_root": { "type": "string" },
                "source_project_path": { "type": "string" },
                "snapshot_id": { "type": "string" },
                "overwrite": { "type": "boolean" },
                "allow_title_conflict": { "type": "boolean" },
                "approved_only": { "type": "boolean" },
                "include_draft": { "type": "boolean" },
                "minimal_context": { "type": "boolean" },
                "title": { "type": "string" },
                "language": { "type": "string" },
                "genre": { "type": "string" },
                "brief": { "type": "string" },
                "target_units": { "type": "integer" },
                "chapter_unit_target": { "type": "integer" },
                "max_chapters_per_turn": { "type": "integer" },
                "source_title": { "type": "string" },
                "source_url": { "type": "string" },
                "notes": { "type": "string" },
                "content": { "type": "string" },
                "split_pattern": { "type": "string" },
                "premise": { "type": "string" },
                "ending_direction": { "type": "string" },
                "authority_contract": { "type": "object", "description": "Complete typed creation contract authority. Scalar compatibility fields are only used when this is absent." },
                "protagonist_arc": { "type": "string" },
                "world_imagery": { "type": "string" },
                "main_causal_spine": { "type": "string" },
                "title_rationale": { "type": "string" },
                "themes": { "type": "array", "items": { "type": "string" } },
                "characters": { "type": "array", "items": { "type": "string" } },
                "world_rules": { "type": "array", "items": { "type": "string" } },
                "style_rules": { "type": "array", "items": { "type": "string" } },
                "must_avoid": { "type": "array", "items": { "type": "string" } },
                "outline": { "type": "string" },
                "field_requirements": { "type": "object", "additionalProperties": { "type": "string" } },
                "resource_economy": { "type": "object", "description": "Structured resource, currency, scarcity, trade, and class-impact contract." },
                "emotional_contract": { "type": "object", "description": "Structured reader emotion, emotional promise, emotional beats, relief beats, payoff, and ending emotion contract." },
                "emotional_state_ledger": { "type": "array", "items": { "type": "object" }, "description": "Chapter-level emotional state changes." },
                "relationship_ledger": { "type": "array", "items": { "type": "object" }, "description": "Structured relationship states and planned relationship turns." },
                "power_progression": { "type": "object", "description": "Structured growth, ability, technology, or status progression contract." },
                "social_order": { "type": "object", "description": "Structured institutions, ranks, laws, promotion, and authority conflict contract." },
                "geography_model": { "type": "object", "description": "Structured regions, locations, distance, travel, and location-change contract." },
                "time_model": { "type": "object", "description": "Structured calendar, elapsed time, ages, deadlines, and time-skip rules." },
                "artifact_ledger": { "type": "array", "items": { "type": "object" }, "description": "Structured object, clue, evidence, or symbolic item ledger." },
	                "antagonist_pressure": { "type": "object", "description": "Structured antagonist or external-pressure contract." },
	                "payoff_matrix": { "type": "array", "items": { "type": "object" }, "description": "Promises, setup, payoff target, status, and evidence." },
	                "narration_contract": { "type": "object", "description": "POV, tense, narrative distance, dialogue, density, pacing, and style-drift constraints." },
	                "scene_type_mix": { "type": "object", "description": "Structured balance of action, dialogue, everyday, reveal, emotional, and turning-point scenes." },
	                "character_voice_ledger": { "type": "array", "items": { "type": "object" }, "description": "Per-character voice style, catchphrases, forbidden expressions, and dialogue rules." },
	                "reader_promise": { "type": "object", "description": "Core reader hook, pleasure points, curiosity engine, and payoff style." },
	                "chapter_ending_rotation": { "type": "object", "description": "Planned rotation of suspense, emotional landing, reversal, and closure endings." },
	                "conflict_pressure_curve": { "type": "object", "description": "Global and volume-level pressure, release, peak, and recovery curve." },
	                "motif_ledger": { "type": "array", "items": { "type": "object" }, "description": "Recurring motifs, meanings, evolution, and payoff targets." },
	                "reveal_schedule": { "type": "array", "items": { "type": "object" }, "description": "Who knows each secret, when it is revealed, and current reveal status." },
	                "relationship_interaction_quotas": { "type": "array", "items": { "type": "object" }, "description": "Cadence and required interactions for relationship arcs." },
	                "plan": { "type": "string" },
                "chapter_number": { "type": "integer" },
                "chapter_title": { "type": "string" },
                "scene_goal": { "type": "string" },
                "conflict": { "type": "string" },
                "choice": { "type": "string" },
                "cost": { "type": "string" },
                "reveal": { "type": "string" },
                "emotional_beat": { "type": "string" },
                "relationship_delta": { "type": "string" },
                "power_delta": { "type": "string" },
                "resource_delta": { "type": "string" },
                "hook_opened": { "type": "array", "items": { "type": "string" } },
                "hook_paid_off": { "type": "array", "items": { "type": "string" } },
                "character_change": { "type": "string" },
                "world_change": { "type": "string" },
                "payoff_target": { "type": "string" },
                "new_character_requests": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "request_id": { "type": "string" },
                            "role": { "type": "string" },
                            "importance": { "type": "string", "enum": ["project_core", "volume_recurring", "chapter_temporary"] },
                            "narrative_purpose": { "type": "string" },
                            "planned_entry": { "type": "string" },
                            "planned_exit": { "type": "string" },
                            "relationship_to_existing": { "type": "string" },
                            "desire": { "type": "string" },
                            "fear": { "type": "string" },
                            "bottom_line": { "type": "string" },
                            "arc_start": { "type": "string" },
                            "arc_end": { "type": "string" },
                            "voice_style": { "type": "string" }
                        }
                    }
                },
                "summary": { "type": "string" },
                "key_facts": { "type": "array", "items": { "type": "string" } },
                "continuity_updates": { "type": "array", "items": { "type": "string" } },
                "issues": { "type": "array", "items": { "type": "string" } },
                "findings": { "type": "array", "items": { "type": "object" } },
                "advisories": { "type": "array", "items": { "type": "string" } },
                "score": { "type": "integer", "minimum": 0, "maximum": 100 },
                "feedback": { "type": "string" },
                "verdict": { "type": "string" },
                "section": { "type": "string" },
                "revision_notes": { "type": "string" },
                "status": { "type": "string" },
                "format": { "type": "string", "enum": ["txt", "md"] },
                "output": { "type": "string" },
                "export_when_complete": { "type": "boolean" }
            },
            "required": ["action"]
        }"#,
    )
    .expect("novel_studio tool schema must be valid JSON");
    if let Some(action) = schema.pointer_mut("/properties/action") {
        action["enum"] = public_actions_json();
    }
    if let Some(chapter_unit_target) = schema
        .pointer_mut("/properties/chapter_unit_target")
        .and_then(|value| value.as_object_mut())
    {
        chapter_unit_target.insert(
            "enum".to_string(),
            json!(longform_policy::novel_chapter_unit_bands()),
        );
        chapter_unit_target.insert(
            "description".to_string(),
            json!(format!(
                "Novel chapter target band. User-facing fiction projects support only {} characters per chapter; non-band requests are normalized to the nearest band.",
                longform_policy::novel_chapter_unit_band_label()
            )),
        );
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_surface_covers_json_schema_fields_and_actions() {
        let schema = novel_studio_parameters();
        let ts = parameters_ts();
        let properties = schema["properties"]
            .as_object()
            .expect("tool schema properties");
        for field in properties.keys() {
            assert!(
                ts.contains(&format!("  {field}")),
                "TypeScript tool surface is missing JSON schema field `{field}`"
            );
        }
        for action in PUBLIC_ACTIONS {
            assert!(
                ts.contains(&format!("'{action}'")),
                "TypeScript tool surface is missing action `{action}`"
            );
        }
    }
}
