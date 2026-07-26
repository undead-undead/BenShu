use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct WritingStudioArgs {
    pub(super) action: String,
    #[serde(default)]
    pub(super) project_path: String,
    #[serde(default)]
    pub(super) draft_path: String,
    #[serde(default)]
    pub(super) output_root: String,
    #[serde(default)]
    pub(super) overwrite: Option<bool>,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) document_type: String,
    #[serde(default)]
    pub(super) language: String,
    #[serde(default)]
    pub(super) audience: String,
    #[serde(default)]
    pub(super) purpose: String,
    #[serde(default)]
    pub(super) thesis_or_premise: String,
    #[serde(default)]
    pub(super) brief: String,
    #[serde(default)]
    pub(super) target_units: Option<usize>,
    #[serde(default)]
    pub(super) section_unit_target: Option<usize>,
    #[serde(default)]
    pub(super) content: String,
    #[serde(default)]
    pub(super) section_id: String,
    #[serde(default)]
    pub(super) section_title: String,
    #[serde(default)]
    pub(super) summary: String,
    #[serde(default)]
    pub(super) evidence_refs: Vec<String>,
    #[serde(default)]
    pub(super) required_structure: Vec<String>,
    #[serde(default)]
    pub(super) style_rules: Vec<String>,
    #[serde(default)]
    pub(super) evidence_rules: Vec<String>,
    #[serde(default)]
    pub(super) entities: Vec<String>,
    #[serde(default)]
    pub(super) terms: Vec<String>,
    #[serde(default)]
    pub(super) claims: Vec<String>,
    #[serde(default)]
    pub(super) sources: Vec<String>,
    #[serde(default)]
    pub(super) forbidden_drift: Vec<String>,
    #[serde(default)]
    pub(super) open_questions: Vec<String>,
    #[serde(default)]
    pub(super) revision_policy: String,
    #[serde(default)]
    pub(super) issues: Vec<String>,
    #[serde(default)]
    pub(super) feedback: String,
    #[serde(default)]
    pub(super) verdict: String,
    #[serde(default)]
    pub(super) revision_notes: String,
    #[serde(default)]
    pub(super) format: String,
    #[serde(default)]
    pub(super) output: String,
    #[serde(default)]
    pub(super) export_when_complete: Option<bool>,
    #[serde(default)]
    pub(super) approved_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingDocumentManifest {
    pub(super) schema_version: String,
    pub(super) title: String,
    pub(super) document_type: String,
    pub(super) language: String,
    pub(super) audience: String,
    pub(super) purpose: String,
    pub(super) brief: String,
    #[serde(default)]
    pub(super) target_units: Option<usize>,
    #[serde(default)]
    pub(super) section_unit_target: Option<usize>,
    #[serde(default)]
    pub(super) export_format: Option<String>,
    #[serde(default)]
    pub(super) export_when_complete: bool,
    #[serde(default)]
    pub(super) approved_only: bool,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) ledger_path: String,
    #[serde(default)]
    pub(super) contract: Option<WritingContract>,
    #[serde(default)]
    pub(super) sections: Vec<WritingSectionRecord>,
    #[serde(default)]
    pub(super) audits: Vec<WritingAuditRecord>,
    #[serde(default)]
    pub(super) exports: Vec<WritingExportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingContract {
    pub(super) thesis_or_premise: String,
    pub(super) required_structure: Vec<String>,
    pub(super) style_rules: Vec<String>,
    pub(super) evidence_rules: Vec<String>,
    pub(super) entities: Vec<String>,
    pub(super) terms: Vec<String>,
    pub(super) claims: Vec<String>,
    pub(super) sources: Vec<String>,
    pub(super) forbidden_drift: Vec<String>,
    pub(super) open_questions: Vec<String>,
    pub(super) revision_policy: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingSectionRecord {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) path: String,
    pub(super) summary: String,
    pub(super) unit_count: usize,
    pub(super) status: String,
    pub(super) evidence_refs: Vec<String>,
    #[serde(default)]
    pub(super) revision: u64,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingAuditRecord {
    pub(super) section_id: String,
    pub(super) verdict: String,
    pub(super) issues: Vec<String>,
    pub(super) feedback: String,
    #[serde(default)]
    pub(super) section_revision: u64,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingExportRecord {
    pub(super) path: String,
    pub(super) format: String,
    pub(super) unit_count: usize,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WritingCreationDraft {
    pub(super) schema_version: String,
    pub(super) title: String,
    pub(super) document_type: String,
    pub(super) language: String,
    pub(super) audience: String,
    pub(super) purpose: String,
    pub(super) brief: String,
    pub(super) target_units: Option<usize>,
    pub(super) section_unit_target: Option<usize>,
    pub(super) export_format: String,
    pub(super) export_when_complete: bool,
    pub(super) approved_only: bool,
    pub(super) thesis_or_premise: String,
    pub(super) required_structure: Vec<String>,
    pub(super) style_rules: Vec<String>,
    pub(super) evidence_rules: Vec<String>,
    pub(super) entities: Vec<String>,
    pub(super) terms: Vec<String>,
    pub(super) claims: Vec<String>,
    pub(super) sources: Vec<String>,
    pub(super) forbidden_drift: Vec<String>,
    pub(super) open_questions: Vec<String>,
    pub(super) revision_policy: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}
