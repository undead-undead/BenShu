//! Governed written-document project tool.
//!
//! This tool stores contracts, ledgers, sections, audits, revisions, and
//! exports for non-code writing artifacts. It does not generate prose by
//! itself; the worker LLM writes the content and uses this tool to keep the
//! artifact stable across turns and long tasks.

use async_trait::async_trait;
use serde_json::json;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use benshu_infra::error::Error;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_runtime_policy_core::resolve_language_contract;
use benshu_state::{ArtifactLifecycle, ArtifactManager};

use crate::tool::{register_tool_output_artifact, ToolArtifactRegistration};

use super::model::*;
use super::project_lock::{acquire_project_lock, ProjectOperationGuard};
use super::quality::{
    bounded_section_context, mechanical_audit, section_has_passed_audit, MAX_CONTEXT_SECTIONS,
    MAX_CONTEXT_SUMMARY_CHARS,
};
use super::storage::atomic_write_file;
use crate::tool::writing::path_recovery::recoverable_path_error_result;
use crate::tool::writing::policy;

const SCHEMA_VERSION: &str = "benshu.writing_document.v1";
const MAX_SINGLE_TEXT_BYTES: usize = 8 * 1024 * 1024;

pub struct WritingStudioTool {
    workspace: PathBuf,
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl WritingStudioTool {
    pub fn new(workspace: PathBuf, agent_id: impl Into<String>) -> Self {
        Self {
            workspace,
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for WritingStudioTool {
    fn name(&self) -> String {
        "writing_studio".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "writing_studio".to_string(),
            description: "Create and maintain governed writing projects for articles, papers, essays, reports, and other non-code documents: contract, ledger, sections, audits, revision, and TXT/Markdown export.".to_string(),
            parameters: writing_studio_parameters(),
            parameters_ts: Some(writing_studio_parameters_ts().to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Use for governed non-code writing when the artifact needs stable terminology, structure, claims, evidence, or revision state across multiple steps. For simple short text files, write_file is enough. For long-form fiction and multi-chapter story continuity, prefer novel_studio.".to_string(),
            ),
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let value: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "writing_studio".into(),
                message: e.to_string(),
            })?;
        if value
            .get("action")
            .and_then(|value| value.as_str())
            .is_none()
        {
            return Ok(serde_json::to_string_pretty(
                &missing_writing_action_result(),
            )?);
        }
        let args: WritingStudioArgs =
            serde_json::from_value(value).map_err(|e| Error::ToolArguments {
                tool_name: "writing_studio".into(),
                message: e.to_string(),
            })?;
        let _operation_guard = match self.lock_operation(&args).await {
            Ok(guard) => guard,
            Err(error) => {
                if let Some(value) = recoverable_path_error_result(
                    &error,
                    "writing_studio",
                    &args.action,
                    &args.project_path,
                    &self.workspace,
                    self.output_root_for_args(&args).as_ref(),
                ) {
                    return Ok(serde_json::to_string_pretty(&value)?);
                }
                return Err(error);
            }
        };

        macro_rules! run_action {
            ($future:expr) => {
                match $future.await {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(value) = recoverable_path_error_result(
                            &error,
                            "writing_studio",
                            &args.action,
                            &args.project_path,
                            &self.workspace,
                            self.output_root_for_args(&args).as_ref(),
                        ) {
                            value
                        } else if let Some(value) =
                            recoverable_project_schema_error_result(&error, &args)
                        {
                            value
                        } else {
                            return Err(error);
                        }
                    }
                }
            };
        }

        let result = match args.action.as_str() {
            "list_documents" => run_action!(self.list_documents(&args)),
            "draft_document" => run_action!(self.draft_document(&args)),
            "update_draft" => run_action!(self.update_draft(&args)),
            "show_draft" => run_action!(self.show_draft(&args)),
            "approve_draft" => run_action!(self.approve_draft(&args)),
            "discard_draft" => run_action!(self.discard_draft(&args)),
            "init_document" => run_action!(self.init_document(&args)),
            "set_contract" => run_action!(self.set_contract(&args)),
            "update_ledger" => run_action!(self.update_ledger(&args)),
            "read_ledger" => run_action!(self.read_ledger(&args)),
            "compose_context" => run_action!(self.compose_context(&args)),
            "write_section" => run_action!(self.write_section(&args)),
            "audit_section" => run_action!(self.audit_section(&args)),
            "revise_section" => run_action!(self.revise_section(&args)),
            "status" => run_action!(self.status(&args)),
            "export" => run_action!(self.export(&args)),
            other => json!({"success": false, "error": format!("unknown action: {other}")}),
        };

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

fn missing_writing_action_result() -> serde_json::Value {
    json!({
        "success": false,
        "error": "missing required action",
        "recoverable": true,
        "available_actions": [
            "draft_document",
            "update_draft",
            "show_draft",
            "approve_draft",
            "discard_draft",
            "init_document",
            "set_contract",
            "update_ledger",
            "read_ledger",
            "compose_context",
            "write_section",
            "audit_section",
            "revise_section",
            "export",
            "status"
        ],
        "next_step_hint": "Call writing_studio again with an explicit action and the required document or section fields."
    })
}

fn recoverable_project_schema_error_result(
    error: &anyhow::Error,
    args: &WritingStudioArgs,
) -> Option<serde_json::Value> {
    let message = error.to_string();
    if !message.contains("project schema mismatch") {
        return None;
    }
    Some(json!({
        "success": false,
        "recoverable": true,
        "error_kind": "project_schema_mismatch",
        "action": args.action,
        "attempted_path": args.project_path,
        "error": message,
        "next_step_hint": "Use the tool that owns this project schema, or create/select a writing_studio project before calling this action."
    }))
}

impl WritingStudioTool {
    async fn lock_operation(
        &self,
        args: &WritingStudioArgs,
    ) -> anyhow::Result<Option<ProjectOperationGuard>> {
        if args.action == "list_documents" {
            return Ok(None);
        }
        let key = if !args.draft_path.trim().is_empty() {
            self.resolve_workspace_path(args.draft_path.trim())?
        } else if !args.project_path.trim().is_empty() {
            self.resolve_workspace_path(args.project_path.trim())?
        } else {
            let root = self.resolve_workspace_path(self.output_root_for_args(args).as_ref())?;
            if args.action == "draft_document" {
                root.join("drafts")
            } else {
                root.join(slugify(first_non_empty(&[
                    &args.title,
                    &args.brief,
                    "untitled document",
                ])))
            }
        };
        acquire_project_lock(&self.workspace, key).await.map(Some)
    }

    async fn list_documents(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let root = self.resolve_workspace_path(self.output_root_for_args(args).as_ref())?;
        let mut documents = Vec::new();
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(_) => {
                return Ok(json!({
                    "success": true,
                    "root": root.to_string_lossy(),
                    "documents": documents
                }));
            }
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let manifest_path = path.join("project.json");
            if !manifest_path.exists() {
                continue;
            }
            if let Ok(raw) = tokio::fs::read_to_string(&manifest_path).await {
                if let Ok(manifest) = serde_json::from_str::<WritingDocumentManifest>(&raw) {
                    if manifest.schema_version == SCHEMA_VERSION {
                        documents.push(json!({
                            "title": manifest.title,
                            "document_type": manifest.document_type,
                            "path": path.to_string_lossy(),
                            "sections": manifest.sections.len(),
                            "updated_at": manifest.updated_at
                        }));
                    }
                }
            }
        }
        Ok(json!({
            "success": true,
            "root": root.to_string_lossy(),
            "documents": documents
        }))
    }

    async fn draft_document(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let now = now_iso();
        let title = first_non_empty(&[&args.title, &args.brief, "untitled document"]);
        let draft = WritingCreationDraft {
            schema_version: "benshu.writing_creation_draft.v1".to_string(),
            title: title.to_string(),
            document_type: normalize_field(&args.document_type, "document"),
            language: infer_writing_language(args),
            audience: args.audience.trim().to_string(),
            purpose: args.purpose.trim().to_string(),
            brief: args.brief.trim().to_string(),
            target_units: args.target_units,
            section_unit_target: args.section_unit_target.filter(|value| *value > 0),
            export_format: normalize_export_format(args.format.trim()),
            export_when_complete: args.export_when_complete.unwrap_or(false),
            approved_only: args.approved_only.unwrap_or(false),
            thesis_or_premise: args.thesis_or_premise.trim().to_string(),
            required_structure: clean_list(&args.required_structure),
            style_rules: clean_list(&args.style_rules),
            evidence_rules: clean_list(&args.evidence_rules),
            entities: clean_list(&args.entities),
            terms: clean_list(&args.terms),
            claims: clean_list(&args.claims),
            sources: clean_list(&args.sources),
            forbidden_drift: clean_list(&args.forbidden_drift),
            open_questions: clean_list(&args.open_questions),
            revision_policy: args.revision_policy.trim().to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let draft_path = if args.draft_path.trim().is_empty() {
            self.new_draft_path(args, &draft.title).await?
        } else {
            self.resolve_workspace_path(&args.draft_path)?
        };
        self.write_draft_file(&draft_path, &draft).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.drafted",
            "draft_path": draft_path.to_string_lossy(),
            "draft": writing_draft_summary(&draft),
            "next_action": "approve_draft or update_draft",
            "receipt": {
                "kind": "writing_creation_draft",
                "tool": "writing_studio",
                "commits_to": ["init_document", "set_contract", "write_section", "export"]
            }
        }))
    }

    async fn update_draft(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let mut draft = self.read_draft_file(&draft_path).await?;
        let before = writing_draft_summary(&draft);
        apply_writing_draft_updates(&mut draft, args);
        draft.updated_at = now_iso();
        self.write_draft_file(&draft_path, &draft).await?;
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.updated",
            "draft_path": draft_path.to_string_lossy(),
            "before": before,
            "draft": writing_draft_summary(&draft),
            "next_action": "approve_draft or update_draft"
        }))
    }

    async fn show_draft(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let draft = self.read_draft_file(&draft_path).await?;
        Ok(json!({
            "success": true,
            "read_only": true,
            "draft_path": draft_path.to_string_lossy(),
            "draft": writing_draft_summary(&draft),
            "next_action": "approve_draft or update_draft"
        }))
    }

    async fn approve_draft(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let draft = self.read_draft_file(&draft_path).await?;
        let mut init_args = args.clone();
        init_args.action = "init_document".to_string();
        init_args.title = draft.title.clone();
        init_args.document_type = draft.document_type.clone();
        init_args.language = draft.language.clone();
        init_args.audience = draft.audience.clone();
        init_args.purpose = draft.purpose.clone();
        init_args.brief = draft.brief.clone();
        init_args.target_units = draft.target_units;
        init_args.section_unit_target = draft.section_unit_target;
        init_args.format = draft.export_format.clone();
        init_args.export_when_complete = Some(draft.export_when_complete);
        init_args.approved_only = Some(draft.approved_only);
        init_args.thesis_or_premise = draft.thesis_or_premise.clone();
        init_args.required_structure = draft.required_structure.clone();
        init_args.style_rules = draft.style_rules.clone();
        init_args.evidence_rules = draft.evidence_rules.clone();
        init_args.entities = draft.entities.clone();
        init_args.terms = draft.terms.clone();
        init_args.claims = draft.claims.clone();
        init_args.sources = draft.sources.clone();
        init_args.forbidden_drift = draft.forbidden_drift.clone();
        init_args.open_questions = draft.open_questions.clone();
        init_args.revision_policy = draft.revision_policy.clone();
        let init = self.init_document(&init_args).await?;
        let project_path = init
            .get("project_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("init_document did not return project_path"))?
            .to_string();
        let _ = tokio::fs::remove_file(&draft_path).await;
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.approved",
            "draft_path": draft_path.to_string_lossy(),
            "project_path": project_path,
            "state": init.get("state").cloned().unwrap_or_else(|| json!({})),
            "draft": writing_draft_summary(&draft),
            "init": init,
            "next_action": "compose_context or write_section"
        }))
    }

    async fn discard_draft(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let draft_path = self.require_draft_path(args)?;
        let existed = tokio::fs::try_exists(&draft_path).await.unwrap_or(false);
        if existed {
            tokio::fs::remove_file(&draft_path).await?;
        }
        Ok(json!({
            "success": true,
            "runtime_effect": "contract.discarded",
            "draft_path": draft_path.to_string_lossy(),
            "existed": existed
        }))
    }

    async fn init_document(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let title = first_non_empty(&[&args.title, &args.brief, "untitled document"]);
        let root = self.resolve_workspace_path(self.output_root_for_args(args).as_ref())?;
        let project_dir = if args.project_path.trim().is_empty() {
            root.join(slugify(title))
        } else {
            self.resolve_workspace_path(&args.project_path)?
        };
        if project_dir.exists() && !args.overwrite.unwrap_or(false) {
            anyhow::bail!(
                "writing document already exists at {}; set overwrite=true or choose another path",
                project_dir.display()
            );
        }

        tokio::fs::create_dir_all(project_dir.join("sections")).await?;
        tokio::fs::create_dir_all(project_dir.join("audits")).await?;
        tokio::fs::create_dir_all(project_dir.join("exports")).await?;
        tokio::fs::create_dir_all(project_dir.join("runtime")).await?;

        let now = now_iso();
        let contract = if has_contract_input(args) {
            Some(contract_from_args(args, &now))
        } else {
            None
        };
        let manifest = WritingDocumentManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: title.to_string(),
            document_type: normalize_field(&args.document_type, "document"),
            language: infer_writing_language(args),
            audience: args.audience.trim().to_string(),
            purpose: args.purpose.trim().to_string(),
            brief: args.brief.trim().to_string(),
            target_units: args.target_units,
            section_unit_target: args.section_unit_target.filter(|value| *value > 0),
            export_format: Some(normalize_export_format(args.format.trim())),
            export_when_complete: args.export_when_complete.unwrap_or(false),
            approved_only: args.approved_only.unwrap_or(false),
            created_at: now.clone(),
            updated_at: now,
            ledger_path: "ledger.md".to_string(),
            contract,
            sections: Vec::new(),
            audits: Vec::new(),
            exports: Vec::new(),
        };
        self.write_manifest(&project_dir, &manifest).await?;
        self.write_readme(&project_dir, &manifest).await?;
        self.write_contract_file(&project_dir, &manifest).await?;
        self.write_ledger_file(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": state_summary(&manifest),
            "contract_path": project_dir.join("contract.md").to_string_lossy(),
            "ledger_path": project_dir.join(&manifest.ledger_path).to_string_lossy()
        }))
    }

    async fn set_contract(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let now = now_iso();
        let mut contract = manifest
            .contract
            .take()
            .unwrap_or_else(|| contract_from_args(args, &now));
        apply_contract_updates(&mut contract, args, &now);
        manifest.contract = Some(contract);
        if !args.title.trim().is_empty() {
            manifest.title = args.title.trim().to_string();
        }
        if !args.document_type.trim().is_empty() {
            manifest.document_type = args.document_type.trim().to_string();
        }
        if !args.language.trim().is_empty() {
            manifest.language = args.language.trim().to_string();
        }
        if !args.audience.trim().is_empty() {
            manifest.audience = args.audience.trim().to_string();
        }
        if !args.purpose.trim().is_empty() {
            manifest.purpose = args.purpose.trim().to_string();
        }
        if args.target_units.is_some() {
            manifest.target_units = args.target_units;
        }
        if args.section_unit_target.is_some() {
            manifest.section_unit_target = args.section_unit_target.filter(|value| *value > 0);
        }
        if !args.format.trim().is_empty() {
            manifest.export_format = Some(normalize_export_format(args.format.trim()));
        }
        if let Some(export_when_complete) = args.export_when_complete {
            manifest.export_when_complete = export_when_complete;
        }
        if let Some(approved_only) = args.approved_only {
            manifest.approved_only = approved_only;
        }
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        self.write_contract_file(&project_dir, &manifest).await?;
        self.write_ledger_file(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": state_summary(&manifest),
            "contract_path": project_dir.join("contract.md").to_string_lossy(),
            "ledger_path": project_dir.join(&manifest.ledger_path).to_string_lossy()
        }))
    }

    async fn update_ledger(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let mut contract = manifest
            .contract
            .clone()
            .unwrap_or_else(|| contract_from_args(args, &now_iso()));
        merge_list(&mut contract.entities, &args.entities);
        merge_list(&mut contract.terms, &args.terms);
        merge_list(&mut contract.claims, &args.claims);
        merge_list(&mut contract.sources, &args.sources);
        merge_list(&mut contract.open_questions, &args.open_questions);
        merge_list(&mut contract.forbidden_drift, &args.forbidden_drift);
        merge_list(&mut contract.required_structure, &args.required_structure);
        merge_list(&mut contract.style_rules, &args.style_rules);
        merge_list(&mut contract.evidence_rules, &args.evidence_rules);
        if !args.thesis_or_premise.trim().is_empty() {
            contract.thesis_or_premise = args.thesis_or_premise.trim().to_string();
        }
        if !args.revision_policy.trim().is_empty() {
            contract.revision_policy = args.revision_policy.trim().to_string();
        }
        contract.updated_at = now_iso();
        manifest.contract = Some(contract);
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        self.write_contract_file(&project_dir, &manifest).await?;
        self.write_ledger_file(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "ledger_path": project_dir.join(&manifest.ledger_path).to_string_lossy(),
            "state": state_summary(&manifest)
        }))
    }

    async fn read_ledger(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let path = project_dir.join(&manifest.ledger_path);
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "ledger_path": path.to_string_lossy(),
            "ledger": content,
            "state": state_summary(&manifest)
        }))
    }

    async fn compose_context(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let section_id = next_section_id(&manifest, &args.section_id);
        let (previous_sections, omitted_sections) = bounded_section_context(&manifest.sections);
        let context = json!({
            "title": manifest.title,
            "document_type": manifest.document_type,
            "language": manifest.language,
            "audience": manifest.audience,
            "purpose": manifest.purpose,
            "brief": manifest.brief,
            "section_id": section_id,
            "section_title": args.section_title.trim(),
            "contract": manifest.contract,
            "previous_sections": previous_sections,
            "context_budget": {
                "max_summary_chars": MAX_CONTEXT_SUMMARY_CHARS,
                "max_sections": MAX_CONTEXT_SECTIONS,
                "omitted_sections": omitted_sections
            }
        });
        let path = project_dir
            .join("runtime")
            .join(format!("{}_context.json", slugify(&section_id)));
        atomic_write_file(path.clone(), serde_json::to_string_pretty(&context)?).await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "context_path": path.to_string_lossy(),
            "context": context
        }))
    }

    async fn write_section(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for write_section");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let id = next_section_id(&manifest, &args.section_id);
        let title = first_non_empty(&[&args.section_title, &args.summary, &id]);
        let previous = manifest.sections.iter().find(|item| item.id == id).cloned();
        let path = previous
            .as_ref()
            .map(|section| section.path.clone())
            .unwrap_or_else(|| format!("sections/{}.md", slugify(&id)));
        let full_path = project_dir.join(&path);
        let now = now_iso();
        let rendered = render_section_file(args, &manifest, &id, title);
        atomic_write_file(full_path.clone(), rendered).await?;

        let record = WritingSectionRecord {
            id: id.clone(),
            title: title.to_string(),
            path: path.clone(),
            summary: args.summary.trim().to_string(),
            unit_count: count_units(&args.content, &manifest.language),
            status: "draft".to_string(),
            evidence_refs: clean_list(&args.evidence_refs),
            revision: previous
                .as_ref()
                .map(|section| section.revision.saturating_add(1))
                .unwrap_or(1),
            created_at: previous
                .as_ref()
                .map(|section| section.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        upsert_section(&mut manifest.sections, record.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;

        Ok(json!({
            "success": true,
            "stage": "writer",
            "next_action": "audit_section",
            "runtime_effect": "artifact.written",
            "artifact_path": full_path.to_string_lossy(),
            "writing_policy": policy::document_stage_policy("writer", "audit_section"),
            "project_path": project_dir.to_string_lossy(),
            "section": record,
            "state": state_summary(&manifest)
        }))
    }

    async fn audit_section(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let section_id = if args.section_id.trim().is_empty() {
            latest_section_id(&manifest).unwrap_or_default()
        } else {
            args.section_id.trim().to_string()
        };
        if section_id.is_empty() {
            anyhow::bail!("section_id is required when the document has no sections");
        }
        let Some(section_index) = manifest
            .sections
            .iter()
            .position(|item| item.id == section_id)
        else {
            anyhow::bail!("section '{}' not found", section_id);
        };
        let section = manifest.sections[section_index].clone();
        let raw = tokio::fs::read_to_string(project_dir.join(&section.path)).await?;
        let body = section_body(&raw);
        let mut issues = clean_list(&args.issues);
        issues.extend(mechanical_audit(&manifest, &section, body));
        issues.sort();
        issues.dedup();
        let requested_verdict = args.verdict.trim().to_ascii_lowercase();
        let verdict = if !issues.is_empty() {
            "revise".to_string()
        } else if matches!(
            requested_verdict.as_str(),
            "pass" | "passed" | "approve" | "approved"
        ) || requested_verdict.is_empty()
        {
            "pass".to_string()
        } else {
            "revise".to_string()
        };
        let feedback = if !args.feedback.trim().is_empty() {
            args.feedback.trim().to_string()
        } else if issues.is_empty() {
            "No mechanical contract drift detected.".to_string()
        } else {
            issues.join("\n")
        };
        let next_action = if policy::revision_next_action(&verdict) == "approve_or_export" {
            "export"
        } else {
            "revise_section"
        };
        let record = WritingAuditRecord {
            section_id: section_id.clone(),
            verdict: verdict.clone(),
            issues,
            feedback,
            section_revision: section.revision,
            created_at: now_iso(),
        };
        let audit_path = project_dir.join("audits").join(format!(
            "{}_{}.json",
            slugify(&section_id),
            manifest.audits.len() + 1
        ));
        atomic_write_file(audit_path.clone(), serde_json::to_string_pretty(&record)?).await?;
        manifest.audits.push(record.clone());
        manifest.sections[section_index].status = if verdict == "pass" {
            "approved".to_string()
        } else {
            "needs_revision".to_string()
        };
        manifest.sections[section_index].updated_at = now_iso();
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        let auto_export = if verdict == "pass" {
            self.maybe_auto_export(args, &project_dir, &manifest)
                .await?
        } else {
            None
        };
        let state = auto_export
            .as_ref()
            .and_then(|result| result.get("state"))
            .cloned()
            .unwrap_or_else(|| state_summary(&manifest));

        Ok(json!({
            "success": true,
            "stage": "auditor",
            "next_action": next_action,
            "writing_policy": policy::document_stage_policy("auditor", next_action),
            "project_path": project_dir.to_string_lossy(),
            "audit_path": audit_path.to_string_lossy(),
            "audit": record,
            "auto_export": auto_export,
            "state": state
        }))
    }

    async fn revise_section(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        ensure_text_size(&args.content, "content")?;
        if args.content.trim().is_empty() {
            anyhow::bail!("content is required for revise_section");
        }
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let section_id = if args.section_id.trim().is_empty() {
            latest_section_id(&manifest).unwrap_or_default()
        } else {
            args.section_id.trim().to_string()
        };
        if section_id.is_empty() {
            anyhow::bail!("section_id is required when the document has no sections");
        }
        let Some(index) = manifest
            .sections
            .iter()
            .position(|item| item.id == section_id)
        else {
            anyhow::bail!("section '{}' not found", section_id);
        };
        let section = manifest.sections[index].clone();
        let title = first_non_empty(&[&args.section_title, &section.title, &section_id]);
        let rendered = render_section_file(args, &manifest, &section_id, title);
        atomic_write_file(project_dir.join(&section.path), rendered).await?;
        manifest.sections[index].title = title.to_string();
        manifest.sections[index].summary = args.summary.trim().to_string();
        manifest.sections[index].unit_count = count_units(&args.content, &manifest.language);
        manifest.sections[index].status = "revised".to_string();
        manifest.sections[index].evidence_refs = clean_list(&args.evidence_refs);
        manifest.sections[index].revision = manifest.sections[index].revision.saturating_add(1);
        manifest.sections[index].updated_at = now_iso();
        let audit = WritingAuditRecord {
            section_id,
            verdict: "revised".to_string(),
            issues: Vec::new(),
            feedback: if args.revision_notes.trim().is_empty() {
                "Content changed; the previous audit is no longer authoritative.".to_string()
            } else {
                args.revision_notes.trim().to_string()
            },
            section_revision: manifest.sections[index].revision,
            created_at: now_iso(),
        };
        manifest.audits.push(audit);
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        Ok(json!({
            "success": true,
            "stage": "reviser",
            "next_action": "audit_section",
            "writing_policy": policy::document_stage_policy("reviser", "audit_section"),
            "project_path": project_dir.to_string_lossy(),
            "section": manifest.sections[index],
            "state": state_summary(&manifest)
        }))
    }

    async fn status(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let manifest = self.read_manifest(&project_dir).await?;
        let document_structure_issues = self
            .document_structure_issues(&project_dir, &manifest)
            .await?;
        Ok(json!({
            "success": true,
            "project_path": project_dir.to_string_lossy(),
            "state": state_summary(&manifest),
            "sections": manifest.sections,
            "audits": manifest.audits,
            "exports": manifest.exports,
            "document_structure_issues": document_structure_issues
        }))
    }

    async fn export(&self, args: &WritingStudioArgs) -> anyhow::Result<serde_json::Value> {
        let project_dir = self.require_project_path(args)?;
        let mut manifest = self.read_manifest(&project_dir).await?;
        let requested_format = if args.format.trim().is_empty() {
            manifest.export_format.as_deref().unwrap_or("txt")
        } else {
            args.format.trim()
        };
        let format = match requested_format {
            "" | "txt" => "txt",
            "md" => "md",
            other => anyhow::bail!("unsupported export format: {other}"),
        };
        let approved_only = args.approved_only.unwrap_or(manifest.approved_only);
        let document_structure_issues = self
            .document_structure_issues(&project_dir, &manifest)
            .await?;
        if approved_only && !document_structure_issues.is_empty() {
            anyhow::bail!(
                "approved-only export is blocked by document structure: {}",
                document_structure_issues.join("; ")
            );
        }
        let output_path = if args.output.trim().is_empty() {
            project_dir
                .join("exports")
                .join(format!("{}.{}", slugify(&manifest.title), format))
        } else {
            self.resolve_workspace_path(&args.output)?
        };
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let rendered = self
            .render_export(&project_dir, &manifest, format, approved_only)
            .await?;
        atomic_write_file(output_path.clone(), rendered.clone()).await?;
        let unit_count = count_units(&rendered, &manifest.language);
        let record = WritingExportRecord {
            path: output_path.to_string_lossy().to_string(),
            format: format.to_string(),
            unit_count,
            created_at: now_iso(),
        };
        manifest.exports.push(record.clone());
        manifest.updated_at = now_iso();
        self.write_manifest(&project_dir, &manifest).await?;
        let artifact_registration = self
            .register_export_artifact(&manifest, &output_path, format, unit_count)
            .await?;
        let bytes = tokio::fs::metadata(&output_path)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        Ok(json!({
            "success": true,
            "runtime_effect": "artifact.written",
            "project_path": project_dir.to_string_lossy(),
            "output_path": output_path.to_string_lossy(),
            "path": output_path.to_string_lossy(),
            "format": format,
            "approved_only": approved_only,
            "document_structure_issues": document_structure_issues,
            "bytes": bytes,
            "unit_count": unit_count,
            "state": state_summary(&manifest),
            "artifact_registration": artifact_registration
        }))
    }

    async fn render_export(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
        format: &str,
        approved_only: bool,
    ) -> anyhow::Result<String> {
        let mut out = String::new();
        if format == "md" {
            out.push_str(&format!("# {}\n\n", manifest.title));
        } else {
            out.push_str(&format!("{}\n\n", manifest.title));
        }
        if !manifest.document_type.trim().is_empty() || !manifest.purpose.trim().is_empty() {
            if format == "md" {
                out.push_str("## Metadata\n\n");
            }
            out.push_str(&format!("Document type: {}\n", manifest.document_type));
            if !manifest.purpose.trim().is_empty() {
                out.push_str(&format!("Purpose: {}\n", manifest.purpose));
            }
            out.push('\n');
        }
        for section in &manifest.sections {
            if approved_only && !section_has_passed_audit(manifest, &section.id) {
                continue;
            }
            let raw = tokio::fs::read_to_string(project_dir.join(&section.path)).await?;
            let body = section_body(&raw);
            if format == "md" {
                out.push_str(body);
            } else {
                out.push_str(&strip_markdown_heading(body));
            }
            out.push_str("\n\n");
        }
        Ok(out)
    }

    async fn document_structure_issues(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<Vec<String>> {
        let Some(contract) = manifest.contract.as_ref() else {
            return Ok(Vec::new());
        };
        if contract.required_structure.is_empty() {
            return Ok(Vec::new());
        }
        let mut searchable = String::new();
        for section in &manifest.sections {
            searchable.push_str(&section.title);
            searchable.push('\n');
            let raw = tokio::fs::read_to_string(project_dir.join(&section.path)).await?;
            searchable.push_str(section_body(&raw));
            searchable.push('\n');
        }
        let searchable = searchable.to_ascii_lowercase();
        let mut issues = Vec::new();
        for required in &contract.required_structure {
            let required = required.trim();
            if !required.is_empty() && !searchable.contains(&required.to_ascii_lowercase()) {
                issues.push(format!("required document section is missing: {required}"));
            }
        }
        Ok(issues)
    }

    async fn maybe_auto_export(
        &self,
        args: &WritingStudioArgs,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        if !manifest.export_when_complete {
            return Ok(None);
        }
        let Some(target_units) = manifest.target_units else {
            return Ok(None);
        };
        let approved_units = manifest
            .sections
            .iter()
            .filter(|section| section_has_passed_audit(manifest, &section.id))
            .map(|section| section.unit_count)
            .sum::<usize>();
        if approved_units < target_units
            || !self
                .document_structure_issues(project_dir, manifest)
                .await?
                .is_empty()
        {
            return Ok(None);
        }
        let mut export_args = args.clone();
        export_args.action = "export".to_string();
        export_args.project_path = project_dir.to_string_lossy().to_string();
        export_args.approved_only = Some(true);
        self.export(&export_args).await.map(Some)
    }

    async fn register_export_artifact(
        &self,
        manifest: &WritingDocumentManifest,
        output_path: &Path,
        format: &str,
        unit_count: usize,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let Some(manager) = self.artifact_manager.as_deref() else {
            return Ok(None);
        };
        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), manifest.title.clone());
        metadata.insert("document_type".to_string(), manifest.document_type.clone());
        metadata.insert("format".to_string(), format.to_string());
        metadata.insert("sections".to_string(), manifest.sections.len().to_string());
        metadata.insert("audits".to_string(), manifest.audits.len().to_string());
        metadata.insert("units".to_string(), unit_count.to_string());
        let record = register_tool_output_artifact(
            manager,
            &self.agent_id,
            "writing_studio",
            &output_path.to_string_lossy(),
            ArtifactLifecycle::Session,
            "writing_export",
            metadata,
        )
        .await?;
        Ok(Some(
            ToolArtifactRegistration::from_record(&record).as_json(),
        ))
    }

    async fn write_readme(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<()> {
        let content = format!(
            "# {}\n\nType: {}\nLanguage: {}\n\nThis folder stores the document contract, ledger, section drafts, audits, and exports for a governed writing artifact.\n",
            manifest.title, manifest.document_type, manifest.language
        );
        atomic_write_file(project_dir.join("README.md"), content).await?;
        Ok(())
    }

    async fn write_contract_file(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<()> {
        atomic_write_file(project_dir.join("contract.md"), render_contract(manifest)).await?;
        Ok(())
    }

    async fn write_ledger_file(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<()> {
        atomic_write_file(
            project_dir.join(&manifest.ledger_path),
            render_ledger(manifest),
        )
        .await?;
        Ok(())
    }

    fn require_project_path(&self, args: &WritingStudioArgs) -> anyhow::Result<PathBuf> {
        if args.project_path.trim().is_empty() {
            anyhow::bail!("project_path is required for {}", args.action);
        }
        self.resolve_workspace_path(&args.project_path)
    }

    fn resolve_workspace_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let raw = path.trim();
        if raw.is_empty() {
            anyhow::bail!("path is empty");
        }
        let joined = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace.join(raw)
        };
        reject_parent_components(&joined)?;
        let workspace = canonical_or_self(&self.workspace);
        let candidate = canonical_parent_join(&joined)?;
        if candidate.starts_with(&workspace) {
            return Ok(candidate);
        }
        if let Ok(trusted) = benshu_brain::skills::CURRENT_WORKSPACES.try_with(|w| w.clone()) {
            for root in trusted {
                let root = canonical_or_self(&root);
                if candidate.starts_with(root) {
                    return Ok(candidate);
                }
            }
        }
        anyhow::bail!(
            "Access Denied: path '{}' is outside authorized workspaces",
            path
        )
    }

    fn default_output_root(&self) -> &'static str {
        if self
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("data"))
        {
            "generated/writing"
        } else {
            "data/generated/writing"
        }
    }

    fn output_root_for_args<'a>(&self, args: &'a WritingStudioArgs) -> Cow<'a, str> {
        if args.output_root.trim().is_empty() {
            Cow::Borrowed(self.default_output_root())
        } else {
            Cow::Borrowed(args.output_root.trim())
        }
    }

    async fn read_manifest(&self, project_dir: &Path) -> anyhow::Result<WritingDocumentManifest> {
        let raw = tokio::fs::read_to_string(project_dir.join("project.json")).await?;
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        let schema = value
            .get("schema_version")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        if schema != SCHEMA_VERSION {
            anyhow::bail!(
                "project schema mismatch: expected '{}', found '{}' at {}",
                SCHEMA_VERSION,
                schema,
                project_dir.display()
            );
        }
        let manifest: WritingDocumentManifest = serde_json::from_str(&raw)?;
        validate_manifest_paths(project_dir, &manifest)?;
        Ok(manifest)
    }

    async fn write_manifest(
        &self,
        project_dir: &Path,
        manifest: &WritingDocumentManifest,
    ) -> anyhow::Result<()> {
        atomic_write_file(
            project_dir.join("project.json"),
            serde_json::to_string_pretty(manifest)?,
        )
        .await?;
        Ok(())
    }

    async fn new_draft_path(
        &self,
        args: &WritingStudioArgs,
        title: &str,
    ) -> anyhow::Result<PathBuf> {
        let root = self
            .resolve_workspace_path(self.output_root_for_args(args).as_ref())?
            .join("drafts");
        tokio::fs::create_dir_all(&root).await?;
        Ok(root.join(format!(
            "{}-{}.json",
            slugify(title),
            uuid::Uuid::new_v4().simple()
        )))
    }

    fn require_draft_path(&self, args: &WritingStudioArgs) -> anyhow::Result<PathBuf> {
        if args.draft_path.trim().is_empty() {
            anyhow::bail!("draft_path is required for {}", args.action);
        }
        self.resolve_workspace_path(&args.draft_path)
    }

    async fn read_draft_file(&self, path: &Path) -> anyhow::Result<WritingCreationDraft> {
        let raw = tokio::fs::read_to_string(path).await?;
        let draft: WritingCreationDraft = serde_json::from_str(&raw)?;
        if draft.schema_version != "benshu.writing_creation_draft.v1" {
            anyhow::bail!(
                "draft schema mismatch: expected benshu.writing_creation_draft.v1, found {}",
                draft.schema_version
            );
        }
        Ok(draft)
    }

    async fn write_draft_file(
        &self,
        path: &Path,
        draft: &WritingCreationDraft,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        atomic_write_file(path.to_path_buf(), serde_json::to_string_pretty(draft)?).await?;
        Ok(())
    }
}

fn has_contract_input(args: &WritingStudioArgs) -> bool {
    !args.thesis_or_premise.trim().is_empty()
        || !args.required_structure.is_empty()
        || !args.style_rules.is_empty()
        || !args.evidence_rules.is_empty()
        || !args.entities.is_empty()
        || !args.terms.is_empty()
        || !args.claims.is_empty()
        || !args.sources.is_empty()
        || !args.forbidden_drift.is_empty()
        || !args.open_questions.is_empty()
        || !args.revision_policy.trim().is_empty()
}

fn contract_from_args(args: &WritingStudioArgs, now: &str) -> WritingContract {
    WritingContract {
        thesis_or_premise: args.thesis_or_premise.trim().to_string(),
        required_structure: clean_list(&args.required_structure),
        style_rules: clean_list(&args.style_rules),
        evidence_rules: clean_list(&args.evidence_rules),
        entities: clean_list(&args.entities),
        terms: clean_list(&args.terms),
        claims: clean_list(&args.claims),
        sources: clean_list(&args.sources),
        forbidden_drift: clean_list(&args.forbidden_drift),
        open_questions: clean_list(&args.open_questions),
        revision_policy: args.revision_policy.trim().to_string(),
        updated_at: now.to_string(),
    }
}

fn apply_contract_updates(contract: &mut WritingContract, args: &WritingStudioArgs, now: &str) {
    if !args.thesis_or_premise.trim().is_empty() {
        contract.thesis_or_premise = args.thesis_or_premise.trim().to_string();
    }
    replace_list_if_present(&mut contract.required_structure, &args.required_structure);
    replace_list_if_present(&mut contract.style_rules, &args.style_rules);
    replace_list_if_present(&mut contract.evidence_rules, &args.evidence_rules);
    replace_list_if_present(&mut contract.entities, &args.entities);
    replace_list_if_present(&mut contract.terms, &args.terms);
    replace_list_if_present(&mut contract.claims, &args.claims);
    replace_list_if_present(&mut contract.sources, &args.sources);
    replace_list_if_present(&mut contract.forbidden_drift, &args.forbidden_drift);
    replace_list_if_present(&mut contract.open_questions, &args.open_questions);
    if !args.revision_policy.trim().is_empty() {
        contract.revision_policy = args.revision_policy.trim().to_string();
    }
    contract.updated_at = now.to_string();
}

fn render_section_file(
    args: &WritingStudioArgs,
    manifest: &WritingDocumentManifest,
    section_id: &str,
    title: &str,
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("document_title: {}\n", manifest.title));
    out.push_str(&format!("document_type: {}\n", manifest.document_type));
    out.push_str(&format!("section_id: {}\n", section_id));
    out.push_str(&format!("section_title: {}\n", title));
    if !args.summary.trim().is_empty() {
        out.push_str(&format!("summary: {}\n", args.summary.trim()));
    }
    if !args.evidence_refs.is_empty() {
        out.push_str("evidence_refs:\n");
        for item in clean_list(&args.evidence_refs) {
            out.push_str(&format!("  - {}\n", item));
        }
    }
    if !args.revision_notes.trim().is_empty() {
        out.push_str(&format!("revision_notes: {}\n", args.revision_notes.trim()));
    }
    out.push_str("---\n\n");
    if !title.trim().is_empty() {
        out.push_str(&format!("## {}\n\n", title.trim()));
    }
    out.push_str(args.content.trim());
    out.push('\n');
    out
}

fn render_contract(manifest: &WritingDocumentManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Contract: {}\n\n", manifest.title));
    out.push_str(&format!("- Document type: {}\n", manifest.document_type));
    out.push_str(&format!("- Language: {}\n", manifest.language));
    if !manifest.audience.trim().is_empty() {
        out.push_str(&format!("- Audience: {}\n", manifest.audience));
    }
    if !manifest.purpose.trim().is_empty() {
        out.push_str(&format!("- Purpose: {}\n", manifest.purpose));
    }
    if !manifest.brief.trim().is_empty() {
        out.push_str(&format!("- Brief: {}\n", manifest.brief));
    }
    if let Some(contract) = &manifest.contract {
        out.push_str("\n## Thesis Or Premise\n\n");
        out.push_str(non_empty_or(&contract.thesis_or_premise, "(unset)"));
        out.push_str("\n\n");
        append_list_section(&mut out, "Required Structure", &contract.required_structure);
        append_list_section(&mut out, "Style Rules", &contract.style_rules);
        append_list_section(&mut out, "Evidence Rules", &contract.evidence_rules);
        append_list_section(&mut out, "Entities", &contract.entities);
        append_list_section(&mut out, "Terms", &contract.terms);
        append_list_section(&mut out, "Claims", &contract.claims);
        append_list_section(&mut out, "Sources", &contract.sources);
        append_list_section(&mut out, "Forbidden Drift", &contract.forbidden_drift);
        append_list_section(&mut out, "Open Questions", &contract.open_questions);
        if !contract.revision_policy.trim().is_empty() {
            out.push_str("## Revision Policy\n\n");
            out.push_str(contract.revision_policy.trim());
            out.push_str("\n\n");
        }
    }
    out
}

fn render_ledger(manifest: &WritingDocumentManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Ledger: {}\n\n", manifest.title));
    if let Some(contract) = &manifest.contract {
        append_list_section(&mut out, "Stable Entities", &contract.entities);
        append_list_section(&mut out, "Stable Terms", &contract.terms);
        append_list_section(&mut out, "Stable Claims", &contract.claims);
        append_list_section(&mut out, "Sources", &contract.sources);
        append_list_section(&mut out, "Required Structure", &contract.required_structure);
        append_list_section(&mut out, "Style Rules", &contract.style_rules);
        append_list_section(&mut out, "Evidence Rules", &contract.evidence_rules);
        append_list_section(&mut out, "Forbidden Drift", &contract.forbidden_drift);
        append_list_section(&mut out, "Open Questions", &contract.open_questions);
    }
    if !manifest.sections.is_empty() {
        out.push_str("## Sections\n\n");
        for section in &manifest.sections {
            out.push_str(&format!(
                "- {} | {} | {} units | {}\n",
                section.id, section.title, section.unit_count, section.status
            ));
        }
        out.push('\n');
    }
    out
}

fn append_list_section(out: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push_str(&format!("## {}\n\n", title));
    for value in values {
        out.push_str(&format!("- {}\n", value));
    }
    out.push('\n');
}

fn state_summary(manifest: &WritingDocumentManifest) -> serde_json::Value {
    let units: usize = manifest
        .sections
        .iter()
        .map(|section| section.unit_count)
        .sum();
    let approved_units = manifest
        .sections
        .iter()
        .filter(|section| section_has_passed_audit(manifest, &section.id))
        .map(|section| section.unit_count)
        .sum::<usize>();
    let target_reached = manifest
        .target_units
        .is_some_and(|target| approved_units >= target);
    json!({
        "schema_version": manifest.schema_version,
        "title": manifest.title,
        "document_type": manifest.document_type,
        "language": manifest.language,
        "sections": manifest.sections.len(),
        "audits": manifest.audits.len(),
        "exports": manifest.exports.len(),
        "units": units,
        "approved_units": approved_units,
        "target_reached": target_reached,
        "target_units": manifest.target_units,
        "section_unit_target": manifest.section_unit_target,
        "export_format": manifest.export_format,
        "export_when_complete": manifest.export_when_complete,
        "approved_only": manifest.approved_only,
        "has_contract": manifest.contract.is_some(),
        "updated_at": manifest.updated_at,
        "writing_policy": policy::document_project_policy(
            manifest.contract.is_some(),
            manifest.sections.len(),
            manifest.audits.len(),
            manifest.exports.len(),
        )
    })
}

fn writing_draft_summary(draft: &WritingCreationDraft) -> serde_json::Value {
    json!({
        "schema_version": draft.schema_version,
        "title": draft.title,
        "document_type": draft.document_type,
        "language": draft.language,
        "audience": draft.audience,
        "purpose": draft.purpose,
        "brief": draft.brief,
        "target_units": draft.target_units,
        "section_unit_target": draft.section_unit_target,
        "export_format": draft.export_format,
        "export_when_complete": draft.export_when_complete,
        "approved_only": draft.approved_only,
        "thesis_or_premise": draft.thesis_or_premise,
        "required_structure": draft.required_structure,
        "style_rules": draft.style_rules,
        "evidence_rules": draft.evidence_rules,
        "entities": draft.entities,
        "terms": draft.terms,
        "claims": draft.claims,
        "sources": draft.sources,
        "forbidden_drift": draft.forbidden_drift,
        "open_questions": draft.open_questions,
        "revision_policy": draft.revision_policy,
        "updated_at": draft.updated_at
    })
}

fn apply_writing_draft_updates(draft: &mut WritingCreationDraft, args: &WritingStudioArgs) {
    if !args.title.trim().is_empty() {
        draft.title = args.title.trim().to_string();
    }
    if !args.document_type.trim().is_empty() {
        draft.document_type = args.document_type.trim().to_string();
    }
    if !args.language.trim().is_empty() {
        draft.language = args.language.trim().to_string();
    }
    if !args.audience.trim().is_empty() {
        draft.audience = args.audience.trim().to_string();
    }
    if !args.purpose.trim().is_empty() {
        draft.purpose = args.purpose.trim().to_string();
    }
    if !args.brief.trim().is_empty() {
        draft.brief = args.brief.trim().to_string();
    }
    if args.target_units.is_some() {
        draft.target_units = args.target_units;
    }
    if args.section_unit_target.is_some() {
        draft.section_unit_target = args.section_unit_target.filter(|value| *value > 0);
    }
    if !args.format.trim().is_empty() {
        draft.export_format = normalize_export_format(args.format.trim());
    }
    if let Some(export_when_complete) = args.export_when_complete {
        draft.export_when_complete = export_when_complete;
    }
    if let Some(approved_only) = args.approved_only {
        draft.approved_only = approved_only;
    }
    if !args.thesis_or_premise.trim().is_empty() {
        draft.thesis_or_premise = args.thesis_or_premise.trim().to_string();
    }
    replace_list_if_present(&mut draft.required_structure, &args.required_structure);
    replace_list_if_present(&mut draft.style_rules, &args.style_rules);
    replace_list_if_present(&mut draft.evidence_rules, &args.evidence_rules);
    replace_list_if_present(&mut draft.entities, &args.entities);
    replace_list_if_present(&mut draft.terms, &args.terms);
    replace_list_if_present(&mut draft.claims, &args.claims);
    replace_list_if_present(&mut draft.sources, &args.sources);
    replace_list_if_present(&mut draft.forbidden_drift, &args.forbidden_drift);
    replace_list_if_present(&mut draft.open_questions, &args.open_questions);
    if !args.revision_policy.trim().is_empty() {
        draft.revision_policy = args.revision_policy.trim().to_string();
    }
}

fn normalize_export_format(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "md" | "markdown" => "md".to_string(),
        _ => "txt".to_string(),
    }
}

fn replace_list_if_present(target: &mut Vec<String>, values: &[String]) {
    if !values.is_empty() {
        *target = clean_list(values);
    }
}

fn upsert_section(sections: &mut Vec<WritingSectionRecord>, record: WritingSectionRecord) {
    if let Some(existing) = sections.iter_mut().find(|item| item.id == record.id) {
        *existing = record;
    } else {
        sections.push(record);
    }
}

fn merge_list(target: &mut Vec<String>, updates: &[String]) {
    let mut seen: BTreeSet<String> = target.iter().map(|item| item.trim().to_string()).collect();
    for item in clean_list(updates) {
        if seen.insert(item.clone()) {
            target.push(item);
        }
    }
}

fn latest_section_id(manifest: &WritingDocumentManifest) -> Option<String> {
    manifest.sections.last().map(|section| section.id.clone())
}

fn next_section_id(manifest: &WritingDocumentManifest, requested: &str) -> String {
    let requested = requested.trim();
    if !requested.is_empty() {
        return requested.to_string();
    }
    format!("section-{:04}", manifest.sections.len() + 1)
}

fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("untitled")
}

fn normalize_field(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn infer_writing_language(args: &WritingStudioArgs) -> String {
    if !args.language.trim().is_empty() {
        return args.language.trim().to_string();
    }
    let context = [
        args.title.trim(),
        args.document_type.trim(),
        args.audience.trim(),
        args.purpose.trim(),
        args.thesis_or_premise.trim(),
        args.brief.trim(),
        args.content.trim(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join("\n");
    let inferred = resolve_language_contract(&context).artifact_language;
    normalize_field(&inferred, "auto")
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

fn clean_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn ensure_text_size(text: &str, field: &str) -> anyhow::Result<()> {
    if text.len() > MAX_SINGLE_TEXT_BYTES {
        anyhow::bail!("{field} exceeds the 8MB single-call safety limit");
    }
    Ok(())
}

fn count_units(content: &str, language: &str) -> usize {
    if language.to_ascii_lowercase().starts_with("en") {
        content
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .count()
    } else {
        content.chars().filter(|ch| !ch.is_whitespace()).count()
    }
}

fn section_body(raw: &str) -> &str {
    raw.split_once("\n---\n")
        .map(|(_, body)| body.trim())
        .unwrap_or(raw.trim())
}

fn strip_markdown_heading(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                trimmed.trim_start_matches('#').trim_start().to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reject_parent_components(path: &Path) -> anyhow::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("path traversal is not allowed: {}", path.display());
    }
    Ok(())
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn canonical_parent_join(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let Some(parent) = path.parent() else {
        return Ok(path.to_path_buf());
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid path: {}", path.display()))?;
    let parent = if parent.exists() {
        parent.canonicalize()?
    } else {
        parent.to_path_buf()
    };
    Ok(parent.join(file_name))
}

fn validate_manifest_paths(
    project_dir: &Path,
    manifest: &WritingDocumentManifest,
) -> anyhow::Result<()> {
    validate_project_relative_path(project_dir, &manifest.ledger_path)?;
    for section in &manifest.sections {
        validate_project_relative_path(project_dir, &section.path)?;
    }
    Ok(())
}

fn validate_project_relative_path(project_dir: &Path, relative: &str) -> anyhow::Result<()> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!(
            "project manifest contains an unsafe member path: {}",
            relative.display()
        );
    }
    let project = canonical_or_self(project_dir);
    let candidate = canonical_parent_join(&project_dir.join(relative))?;
    if !candidate.starts_with(project) {
        anyhow::bail!(
            "project manifest member escapes the project directory: {}",
            relative.display()
        );
    }
    Ok(())
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            out.push(ch);
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        format!("writing-{}", uuid::Uuid::new_v4().simple())
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn writing_studio_parameters() -> serde_json::Value {
    serde_json::from_str(
        r#"{
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "list_documents",
                        "draft_document",
                        "update_draft",
                        "show_draft",
                        "approve_draft",
                        "discard_draft",
                        "init_document",
                        "set_contract",
                        "update_ledger",
                        "read_ledger",
                        "compose_context",
                        "write_section",
                        "audit_section",
                        "revise_section",
                        "status",
                        "export"
                    ],
                    "description": "Operation to perform."
                },
                "project_path": { "type": "string" },
                "draft_path": { "type": "string" },
                "output_root": { "type": "string" },
                "overwrite": { "type": "boolean" },
                "title": { "type": "string" },
                "document_type": { "type": "string" },
                "language": { "type": "string" },
                "audience": { "type": "string" },
                "purpose": { "type": "string" },
                "thesis_or_premise": { "type": "string" },
                "brief": { "type": "string" },
                "target_units": { "type": "integer" },
                "section_unit_target": { "type": "integer" },
                "content": { "type": "string" },
                "section_id": { "type": "string" },
                "section_title": { "type": "string" },
                "summary": { "type": "string" },
                "evidence_refs": { "type": "array", "items": { "type": "string" } },
                "required_structure": { "type": "array", "items": { "type": "string" } },
                "style_rules": { "type": "array", "items": { "type": "string" } },
                "evidence_rules": { "type": "array", "items": { "type": "string" } },
                "entities": { "type": "array", "items": { "type": "string" } },
                "terms": { "type": "array", "items": { "type": "string" } },
                "claims": { "type": "array", "items": { "type": "string" } },
                "sources": { "type": "array", "items": { "type": "string" } },
                "forbidden_drift": { "type": "array", "items": { "type": "string" } },
                "open_questions": { "type": "array", "items": { "type": "string" } },
                "revision_policy": { "type": "string" },
                "issues": { "type": "array", "items": { "type": "string" } },
                "feedback": { "type": "string" },
                "verdict": { "type": "string" },
                "revision_notes": { "type": "string" },
                "format": { "type": "string", "enum": ["txt", "md"] },
                "output": { "type": "string" },
                "export_when_complete": { "type": "boolean" },
                "approved_only": { "type": "boolean" }
            },
            "required": ["action"]
        }"#,
    )
    .expect("writing_studio tool schema must be valid JSON")
}

fn writing_studio_parameters_ts() -> &'static str {
    r#"interface WritingStudioArgs {
  action: 'list_documents' | 'draft_document' | 'update_draft' | 'show_draft' | 'approve_draft' | 'discard_draft' | 'init_document' | 'set_contract' | 'update_ledger' | 'read_ledger' | 'compose_context' | 'write_section' | 'audit_section' | 'revise_section' | 'status' | 'export';
  project_path?: string;
  draft_path?: string;
  output_root?: string;
  overwrite?: boolean;
  title?: string;
  document_type?: string;
  language?: string;
  audience?: string;
  purpose?: string;
  thesis_or_premise?: string;
  brief?: string;
  target_units?: number;
  section_unit_target?: number;
  content?: string;
  section_id?: string;
  section_title?: string;
  summary?: string;
  evidence_refs?: string[];
  required_structure?: string[];
  style_rules?: string[];
  evidence_rules?: string[];
  entities?: string[];
  terms?: string[];
  claims?: string[];
  sources?: string[];
  forbidden_drift?: string[];
  open_questions?: string[];
  revision_policy?: string;
  issues?: string[];
  feedback?: string;
  verdict?: string;
  revision_notes?: string;
  format?: 'txt' | 'md';
  output?: string;
  export_when_complete?: boolean;
  approved_only?: boolean;
}"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn writing_studio_missing_action_returns_recoverable_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");

        let result = tool.call("{}").await.expect("guidance");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert!(value["available_actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action == "init_document"));
    }

    #[tokio::test]
    async fn writing_studio_path_boundary_returns_recoverable_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let outside = dir
            .path()
            .parent()
            .expect("parent")
            .join("outside-writing-project");

        let result = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Boundary Test",
                    "project_path": outside
                })
                .to_string(),
            )
            .await
            .expect("guidance");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "path_outside_workspace");
        assert_eq!(value["safe_output_root"], "data/generated/writing");
    }

    #[tokio::test]
    async fn writing_studio_avoids_nested_data_root_when_workspace_is_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("data");
        tokio::fs::create_dir_all(&data_dir).await.expect("mkdir");
        let tool = WritingStudioTool::new(data_dir.clone(), "writer");

        let result = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Workspace Data Root"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        let project_path = value["project_path"].as_str().expect("project path");

        assert!(project_path.contains("/data/generated/writing/"));
        assert!(!project_path.contains("/data/data/generated/writing/"));
    }

    #[tokio::test]
    async fn writing_studio_creates_audits_and_exports_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");

        let init = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Stable Concepts",
                    "document_type": "article",
                    "language": "en",
                    "thesis_or_premise": "A durable ledger reduces drift.",
                    "terms": ["ledger", "contract"],
                    "required_structure": ["Conclusion"],
                    "evidence_rules": ["cite evidence_refs when using sources"]
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_json: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_json["project_path"].as_str().expect("project path");

        let write = tool
            .call(
                &json!({
                    "action": "write_section",
                    "project_path": project_path,
                    "section_title": "Conclusion",
                    "content": "Conclusion\nA ledger and contract keep the argument stable.",
                    "summary": "Explains the anti-drift mechanism.",
                    "evidence_refs": ["local-note:1"]
                })
                .to_string(),
            )
            .await
            .expect("write section");
        let write_json: serde_json::Value = serde_json::from_str(&write).expect("json");
        assert_eq!(write_json["runtime_effect"], "artifact.written");
        assert!(write_json["artifact_path"]
            .as_str()
            .expect("artifact path")
            .ends_with(".md"));

        let audit = tool
            .call(
                &json!({
                    "action": "audit_section",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("audit");
        assert!(audit.contains("\"verdict\": \"pass\""));

        let export = tool
            .call(
                &json!({
                    "action": "export",
                    "project_path": project_path,
                    "format": "txt"
                })
                .to_string(),
            )
            .await
            .expect("export");
        let export_json: serde_json::Value = serde_json::from_str(&export).expect("json");
        assert_eq!(export_json["runtime_effect"], "artifact.written");
        let output_path = PathBuf::from(export_json["path"].as_str().expect("path"));
        let exported = tokio::fs::read_to_string(output_path)
            .await
            .expect("exported");
        assert!(exported.contains("Stable Concepts"));
        assert!(exported.contains("A ledger and contract keep the argument stable."));
    }

    #[tokio::test]
    async fn writing_studio_draft_approval_commits_real_document_parameters() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");

        let draft = tool
            .call(
                &json!({
                    "action": "draft_document",
                    "title": "心脏病治疗综述",
                    "document_type": "review_paper",
                    "language": "zh",
                    "audience": "clinicians",
                    "purpose": "总结治疗证据",
                    "brief": "根据已入库论文写综述。",
                    "target_units": 12000,
                    "section_unit_target": 2000,
                    "format": "md",
                    "approved_only": true,
                    "thesis_or_premise": "治疗方案应基于证据分层。",
                    "required_structure": ["摘要", "背景", "治疗进展", "局限性", "参考文献"],
                    "evidence_rules": ["每个关键结论要有来源"],
                    "claims": ["新治疗策略需要区分适用人群"],
                    "sources": ["knowledge:cardiology"],
                    "forbidden_drift": ["不要虚构医学结论"],
                    "revision_policy": "审查证据和结构后再导出。"
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let draft_path = draft["draft_path"].as_str().expect("draft path");

        let approved = tool
            .call(
                &json!({
                    "action": "approve_draft",
                    "draft_path": draft_path
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approve json");
        let project_path = approved["project_path"].as_str().expect("project path");
        let raw = tokio::fs::read_to_string(PathBuf::from(project_path).join("project.json"))
            .await
            .expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest json");

        assert_eq!(manifest["target_units"], 12000);
        assert_eq!(manifest["section_unit_target"], 2000);
        assert_eq!(manifest["export_format"], "md");
        assert_eq!(manifest["approved_only"], true);
        assert_eq!(manifest["contract"]["required_structure"][0], "摘要");
        assert_eq!(
            manifest["contract"]["thesis_or_premise"],
            "治疗方案应基于证据分层。"
        );
    }

    #[tokio::test]
    async fn writing_studio_wrong_project_schema_returns_recoverable_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_dir = dir.path().join("other-project");
        tokio::fs::create_dir_all(&project_dir)
            .await
            .expect("mkdir");
        tokio::fs::write(
            project_dir.join("project.json"),
            r#"{"schema_version":"benshu.novel_project.v1","title":"Novel"}"#,
        )
        .await
        .expect("write manifest");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");

        let result = tool
            .call(
                &json!({
                    "action": "write_section",
                    "project_path": project_dir,
                    "content": "This content belongs to another project type."
                })
                .to_string(),
            )
            .await
            .expect("recoverable");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "project_schema_mismatch");
    }

    #[tokio::test]
    async fn mechanical_issues_cannot_be_overridden_by_pass_verdict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let init = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Evidence Contract",
                    "evidence_rules": ["claims require evidence"]
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &json!({
                "action": "write_section",
                "project_path": project_path,
                "section_id": "section-0001",
                "content": "A claim without an evidence reference."
            })
            .to_string(),
        )
        .await
        .expect("write");
        let audit = tool
            .call(
                &json!({
                    "action": "audit_section",
                    "project_path": project_path,
                    "section_id": "section-0001",
                    "verdict": "pass"
                })
                .to_string(),
            )
            .await
            .expect("audit");
        let audit: Value = serde_json::from_str(&audit).expect("audit json");
        assert_eq!(audit["audit"]["verdict"], "revise");
        assert!(audit["audit"]["issues"]
            .as_array()
            .is_some_and(|issues| !issues.is_empty()));
    }

    #[tokio::test]
    async fn revising_content_invalidates_previous_passed_audit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let init = tool
            .call(&json!({"action": "init_document", "title": "Revision"}).to_string())
            .await
            .expect("init");
        let init: Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &json!({
                "action": "write_section",
                "project_path": project_path,
                "section_id": "section-0001",
                "content": "Approved original body."
            })
            .to_string(),
        )
        .await
        .expect("write");
        tool.call(
            &json!({
                "action": "audit_section",
                "project_path": project_path,
                "section_id": "section-0001"
            })
            .to_string(),
        )
        .await
        .expect("audit");
        tool.call(
            &json!({
                "action": "revise_section",
                "project_path": project_path,
                "section_id": "section-0001",
                "content": "Changed body awaiting a new audit."
            })
            .to_string(),
        )
        .await
        .expect("revise");
        let export = tool
            .call(
                &json!({
                    "action": "export",
                    "project_path": project_path,
                    "approved_only": true
                })
                .to_string(),
            )
            .await
            .expect("export");
        let export: Value = serde_json::from_str(&export).expect("export json");
        let content = tokio::fs::read_to_string(export["path"].as_str().expect("path"))
            .await
            .expect("read export");
        assert!(!content.contains("Changed body"));
    }

    #[tokio::test]
    async fn required_structure_is_checked_at_document_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let init = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Structured Document",
                    "required_structure": ["Introduction", "Conclusion"]
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &json!({
                "action": "write_section",
                "project_path": project_path,
                "section_id": "introduction",
                "section_title": "Introduction",
                "content": "This section introduces the argument."
            })
            .to_string(),
        )
        .await
        .expect("write");
        let audit = tool
            .call(
                &json!({
                    "action": "audit_section",
                    "project_path": project_path,
                    "section_id": "introduction"
                })
                .to_string(),
            )
            .await
            .expect("audit");
        let audit: Value = serde_json::from_str(&audit).expect("audit json");
        assert_eq!(audit["audit"]["verdict"], "pass");
        let status = tool
            .call(&json!({"action": "status", "project_path": project_path}).to_string())
            .await
            .expect("status");
        let status: Value = serde_json::from_str(&status).expect("status json");
        assert_eq!(
            status["document_structure_issues"]
                .as_array()
                .expect("issues")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn completed_approved_document_auto_exports() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let init = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Auto Export",
                    "target_units": 5,
                    "export_when_complete": true,
                    "approved_only": true
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &json!({
                "action": "write_section",
                "project_path": project_path,
                "section_id": "section-0001",
                "content": "This body is long enough."
            })
            .to_string(),
        )
        .await
        .expect("write");
        let audit = tool
            .call(
                &json!({
                    "action": "audit_section",
                    "project_path": project_path,
                    "section_id": "section-0001"
                })
                .to_string(),
            )
            .await
            .expect("audit");
        let audit: Value = serde_json::from_str(&audit).expect("audit json");
        assert!(audit["auto_export"].is_object());
    }

    #[tokio::test]
    async fn contract_updates_can_disable_boolean_options() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let init = tool
            .call(
                &json!({
                    "action": "init_document",
                    "title": "Boolean Patch",
                    "export_when_complete": true,
                    "approved_only": true
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        let updated = tool
            .call(
                &json!({
                    "action": "set_contract",
                    "project_path": project_path,
                    "export_when_complete": false,
                    "approved_only": false
                })
                .to_string(),
            )
            .await
            .expect("update");
        let updated: Value = serde_json::from_str(&updated).expect("update json");
        assert_eq!(updated["state"]["export_when_complete"], false);
        assert_eq!(updated["state"]["approved_only"], false);
    }

    #[tokio::test]
    async fn manifest_member_paths_cannot_escape_project_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = WritingStudioTool::new(dir.path().to_path_buf(), "writer");
        let project = dir.path().join("unsafe-manifest");
        tokio::fs::create_dir_all(&project).await.expect("mkdir");
        tokio::fs::write(
            project.join("project.json"),
            serde_json::to_string(&json!({
                "schema_version": SCHEMA_VERSION,
                "title": "Unsafe",
                "document_type": "document",
                "language": "en",
                "audience": "",
                "purpose": "",
                "brief": "",
                "created_at": now_iso(),
                "updated_at": now_iso(),
                "ledger_path": "../outside.md",
                "sections": [],
                "audits": [],
                "exports": []
            }))
            .expect("manifest json"),
        )
        .await
        .expect("write manifest");

        let error = tool.read_manifest(&project).await.expect_err("unsafe path");
        assert!(error.to_string().contains("unsafe member path"), "{error}");
    }
}
