use std::collections::HashMap;

use crate::runtime_contract::{append_metadata_notes, metadata_value};

const SKILL_LOADING_NOTE_PROJECTIONS: &[(&str, &str)] = &[
    ("matched_skill_manuals", "skill_manual_match"),
    ("matched_skill_assets", "skill_asset_match"),
    ("read_skill_manuals", "skill_manual_read"),
    ("read_skill_assets", "skill_asset_read"),
    ("skill_asset_followups", "skill_asset_followup"),
    (
        "skill_asset_execution_surfaces",
        "skill_asset_execution_surface",
    ),
    (
        "skill_surface_classifications",
        "skill_surface_classification",
    ),
    ("skill_surface_executions", "skill_surface_execution"),
    ("skill_surface_runtimes", "skill_surface_runtime"),
    ("skill_surface_kinds", "skill_surface_kind"),
];

const RUNTIME_SKILL_LOADING_NOTE_PROJECTIONS: &[(&str, &str)] = &[
    ("matched_skill_manuals", "runtime_matched_skill_manuals"),
    ("matched_skill_assets", "runtime_matched_skill_assets"),
    ("read_skill_manuals", "runtime_read_skill_manuals"),
    ("read_skill_assets", "runtime_read_skill_assets"),
    ("skill_asset_followups", "runtime_skill_asset_followups"),
    (
        "skill_asset_execution_surfaces",
        "runtime_skill_asset_execution_surfaces",
    ),
    (
        "skill_surface_classifications",
        "runtime_skill_surface_classifications",
    ),
    (
        "skill_surface_executions",
        "runtime_skill_surface_executions",
    ),
    ("skill_surface_runtimes", "runtime_skill_surface_runtimes"),
    ("skill_surface_kinds", "runtime_skill_surface_kinds"),
    (
        "skill_manual_read_happened",
        "runtime_skill_manual_read_happened",
    ),
    (
        "skill_asset_read_happened",
        "runtime_skill_asset_read_happened",
    ),
    (
        "skill_asset_followup_happened",
        "runtime_skill_asset_followup_happened",
    ),
    (
        "skill_asset_execution_surface_happened",
        "runtime_skill_asset_execution_surface_happened",
    ),
    (
        "skill_surface_contract_happened",
        "runtime_skill_surface_contract_happened",
    ),
    (
        "skill_loading_contract_core_complete",
        "runtime_skill_loading_contract_core_complete",
    ),
    (
        "skill_loading_contract_complete",
        "runtime_skill_loading_contract_complete",
    ),
    (
        "skill_loading_surface_note_core_complete",
        "runtime_skill_loading_surface_note_core_complete",
    ),
    (
        "skill_loading_surface_note_complete",
        "runtime_skill_loading_surface_note_complete",
    ),
    (
        "skill_surface_contract_core_complete",
        "runtime_skill_surface_contract_core_complete",
    ),
    (
        "skill_surface_contract_complete",
        "runtime_skill_surface_contract_complete",
    ),
];

const BOOLEAN_SKILL_LOADING_NOTES: &[&str] = &[
    "skill_manual_gate_active",
    "skill_asset_gate_active",
    "skill_manual_read_happened",
    "skill_asset_read_happened",
    "skill_asset_followup_happened",
    "skill_asset_execution_surface_happened",
    "skill_surface_contract_happened",
    "skill_loading_contract_core_complete",
    "skill_loading_contract_complete",
    "skill_loading_surface_note_core_complete",
    "skill_loading_surface_note_complete",
];

pub fn append_skill_loading_notes(notes: &mut Vec<String>, metadata: &HashMap<String, String>) {
    append_metadata_notes(notes, metadata, SKILL_LOADING_NOTE_PROJECTIONS);

    for key in BOOLEAN_SKILL_LOADING_NOTES {
        if metadata_value(metadata, key) == Some("true") {
            notes.push((*key).to_string());
        }
    }

    append_metadata_notes(notes, metadata, RUNTIME_SKILL_LOADING_NOTE_PROJECTIONS);

    let has_skill_loading_activity = metadata.contains_key("matched_skill_manuals")
        || metadata.contains_key("matched_skill_assets")
        || metadata.contains_key("read_skill_manuals")
        || metadata.contains_key("read_skill_assets")
        || metadata.contains_key("skill_manual_gate_active")
        || metadata.contains_key("skill_asset_gate_active");
    let manual_chain_complete = !metadata.contains_key("matched_skill_manuals")
        || metadata.contains_key("read_skill_manuals");
    let asset_chain_complete = !metadata.contains_key("matched_skill_assets")
        || metadata.contains_key("read_skill_assets");
    let followup_chain_complete = !metadata.contains_key("skill_asset_followups")
        || metadata.contains_key("skill_asset_read_happened");
    let execution_surface_chain_complete = !metadata.contains_key("skill_asset_followups")
        || metadata.contains_key("skill_asset_execution_surfaces");
    let surface_contract_core_complete = !metadata.contains_key("matched_skill_manuals")
        || (metadata.contains_key("skill_surface_classifications")
            && metadata.contains_key("skill_surface_executions")
            && metadata.contains_key("skill_surface_kinds"));
    let surface_contract_complete = !metadata.contains_key("matched_skill_manuals")
        || (surface_contract_core_complete && metadata.contains_key("skill_surface_runtimes"));

    maybe_push_flag(
        notes,
        has_skill_loading_activity && manual_chain_complete && asset_chain_complete,
        "skill_loading_contract_core_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity
            && manual_chain_complete
            && asset_chain_complete
            && followup_chain_complete
            && execution_surface_chain_complete
            && surface_contract_complete,
        "skill_loading_contract_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && manual_chain_complete && asset_chain_complete,
        "skill_loading_surface_note_core_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity
            && manual_chain_complete
            && asset_chain_complete
            && followup_chain_complete
            && execution_surface_chain_complete
            && surface_contract_complete,
        "skill_loading_surface_note_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && manual_chain_complete && asset_chain_complete,
        "runtime_skill_loading_contract_core_complete:true",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity
            && manual_chain_complete
            && asset_chain_complete
            && followup_chain_complete
            && execution_surface_chain_complete
            && surface_contract_complete,
        "runtime_skill_loading_contract_complete:true",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && manual_chain_complete && asset_chain_complete,
        "runtime_skill_loading_surface_note_core_complete:true",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity
            && manual_chain_complete
            && asset_chain_complete
            && followup_chain_complete
            && execution_surface_chain_complete
            && surface_contract_complete,
        "runtime_skill_loading_surface_note_complete:true",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && surface_contract_core_complete,
        "skill_surface_contract_core_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && surface_contract_complete,
        "skill_surface_contract_complete",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && surface_contract_core_complete,
        "runtime_skill_surface_contract_core_complete:true",
    );
    maybe_push_flag(
        notes,
        has_skill_loading_activity && surface_contract_complete,
        "runtime_skill_surface_contract_complete:true",
    );
}

fn maybe_push_flag(notes: &mut Vec<String>, active: bool, note: &str) {
    if active && !notes.iter().any(|existing| existing == note) {
        notes.push(note.to_string());
    }
}
