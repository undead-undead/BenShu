//! Writing project and delegation policy.
//!
//! This module owns workflow-stage/project policy and delegation guidance.
//! Turn-level intent stays in `intent_policy`, fiction sizing/retry policy in
//! `longform_policy`, and naming policy in `naming/`.

use serde_json::{json, Value};

const FICTION_WORKFLOW: &[&str] = &[
    "source_intake",
    "project_contract",
    "volume_contract",
    "chapter_contract",
    "context_package",
    "body_draft",
    "post_body_summary",
    "title_confirmation",
    "audit",
    "approval",
    "authority_update",
    "export",
];

const DOCUMENT_WORKFLOW: &[&str] = &[
    "contract", "ledger", "context", "writer", "auditor", "reviser", "export",
];

const FICTION_REMINDERS: &[&str] = &[
    "Keep a stable whole-book contract before long-form drafting.",
    "Use volume contracts and chapter contracts as execution boundaries; do not let a draft invent a new book, volume, protagonist, or ending.",
    "Write chapter body first, then summarize the body and confirm the final chapter title from the written events.",
    "Use selected source/context as evidence, not as prose to copy.",
    "Preserve title_state, character ledger, world rules, timeline, and unresolved hook debt across chapters.",
    "Run audit before approval or export when a draft changes; only approved chapters may update truth, summaries, hooks, and volume state.",
    "Revise only against concrete audit issues or explicit user feedback.",
    "Export from manifest authority, not from arbitrary prose headings.",
];

const DOCUMENT_REMINDERS: &[&str] = &[
    "Keep thesis, structure, evidence rules, and terminology in the contract.",
    "Use the ledger to carry stable claims, sources, and open questions across sections.",
    "Audit each section against the contract before export.",
    "Revise against audit findings without changing unrelated accepted sections.",
];

pub(crate) fn fiction_stage_policy(stage: &str, next_action: &str) -> Value {
    json!({
        "kind": "fiction",
        "workflow": FICTION_WORKFLOW,
        "current_stage": stage,
        "next_action": next_action,
        "reminders": FICTION_REMINDERS
    })
}

pub(crate) fn document_stage_policy(stage: &str, next_action: &str) -> Value {
    json!({
        "kind": "document",
        "workflow": DOCUMENT_WORKFLOW,
        "current_stage": stage,
        "next_action": next_action,
        "reminders": DOCUMENT_REMINDERS
    })
}

pub(crate) fn fiction_project_policy(
    sources: usize,
    has_contract: bool,
    truth_files: usize,
    plans: usize,
    architectures: usize,
    chapters: usize,
    latest_needs_revision: bool,
) -> Value {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if !has_contract {
        blockers.push("story_contract_required_before_long_form_drafting");
    }
    if chapters > 0 && truth_files == 0 {
        warnings.push("truth_or_continuity_files_missing_after_drafting");
    }
    if plans > chapters {
        warnings.push("planner_output_waiting_for_draft");
    }
    if architectures > chapters {
        warnings.push("architect_output_waiting_for_draft");
    }
    if latest_needs_revision {
        blockers.push("latest_draft_requires_revision_before_approval_or_export");
    }
    if sources == 0 {
        warnings.push("no_source_material_attached");
    }
    json!({
        "kind": "fiction",
        "workflow": FICTION_WORKFLOW,
        "passed": blockers.is_empty(),
        "blockers": blockers,
        "warnings": warnings,
        "reminders": FICTION_REMINDERS
    })
}

pub(crate) fn document_project_policy(
    has_contract: bool,
    sections: usize,
    audits: usize,
    exports: usize,
) -> Value {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if !has_contract {
        blockers.push("document_contract_required_for_governed_writing");
    }
    if sections > audits {
        warnings.push("one_or_more_sections_need_audit");
    }
    if exports > 0 && sections > audits {
        warnings.push("export_exists_before_all_sections_were_audited");
    }
    json!({
        "kind": "document",
        "workflow": DOCUMENT_WORKFLOW,
        "passed": blockers.is_empty(),
        "blockers": blockers,
        "warnings": warnings,
        "reminders": DOCUMENT_REMINDERS
    })
}

pub(crate) fn revision_next_action(verdict: &str) -> &'static str {
    let lowered = verdict.trim().to_ascii_lowercase();
    if matches!(
        lowered.as_str(),
        "pass"
            | "passed"
            | "approve"
            | "approved"
            | "accept"
            | "accepted"
            | "通过"
            | "批准"
            | "接受"
    ) {
        "approve_or_export"
    } else {
        "revise"
    }
}

pub(crate) fn task_requests_existing_artifact_revision(task: &str) -> bool {
    let lowered = task.to_lowercase();
    let mutation_terms = [
        "revise", "revision", "edit", "update", "modify", "fix", "polish", "complete", "refine",
    ];
    let existing_terms = [
        "existing", "current", "previous", "prior", "saved", "local", "project", "chapter",
        "section", "draft", "file",
    ];
    let has_mutation = mutation_terms.iter().any(|term| lowered.contains(term))
        || [
            "修订", "修改", "修正", "更新", "补全", "完善", "校订", "编辑", "润色", "处理",
        ]
        .iter()
        .any(|term| task.contains(term));
    let has_existing_context = existing_terms.iter().any(|term| lowered.contains(term))
        || [
            "已有",
            "现有",
            "当前",
            "刚才",
            "之前",
            "本地",
            "项目",
            "章节",
            "第一章",
            "第二章",
            "第三章",
            "章",
            "文件",
            "文档",
        ]
        .iter()
        .any(|term| task.contains(term));

    has_mutation && has_existing_context
}

pub(crate) fn task_requests_local_writing_context(task: &str) -> bool {
    let lowered = task.to_ascii_lowercase();
    if lowered.contains("http://")
        || lowered.contains("https://")
        || lowered.contains("www.")
        || lowered.contains("browser_browse")
        || lowered.contains("web_search")
        || task.contains("公网")
        || task.contains("网页")
        || task.contains("网站")
        || task.contains("站点")
        || task.contains("浏览器")
        || task.contains("搜索引擎")
    {
        return false;
    }

    let local_context = [
        "搜索历史",
        "历史记录",
        "知识库",
        "记忆",
        "上下文",
        "刚才",
        "之前",
        "已有",
        "当前",
        "本地",
        "项目",
        "continuity",
        "context",
        "memory",
        "knowledge base",
        "local",
        "project",
    ];
    let writing_context = [
        "章节",
        "第一章",
        "第二章",
        "第三章",
        "章",
        "正文",
        "草稿",
        "标题",
        "作品名",
        "人物",
        "角色",
        "主角",
        "世界观",
        "设定",
        "伏笔",
        "情节",
        "剧情",
        "摘要",
        "关键事实",
        "连续性",
        "真相文件",
        "chapter",
        "draft",
        "title",
        "character",
        "worldbuilding",
        "setting",
        "plot",
        "summary",
        "facts",
    ];

    let has_local_context = local_context
        .iter()
        .any(|term| task.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    let has_writing_context = writing_context
        .iter()
        .any(|term| task.contains(term) || lowered.contains(&term.to_ascii_lowercase()));

    has_local_context && has_writing_context
}

pub(crate) fn worker_contract_guidance() -> &'static str {
    "Writing artifact contract:\n\
     - You own written artifacts for this task. Do not hand articles, fiction, papers, essays, reports, drafts, or TXT/Markdown prose files to coder.\n\
     - For direct short written artifacts, write the content with `write_file` when a file output is requested or implied.\n\
     - For governed articles, papers, essays, reports, or other structured non-code documents, use `writing_studio`: initialize a document, set a contract, keep a ledger for stable terms/claims/entities/evidence, compose section context, write sections, audit/revise drift, and export TXT/Markdown when requested.\n\
     - For governed long-form fiction or multi-chapter story work, use `novel_studio` as the canonical project runtime: initialize a project, set a story contract, plan/compose/architect each chapter, persist drafts/chapters, audit/revise before approval, and export TXT/Markdown when requested. Do not collapse these projects into a direct `write_file`, `writing_studio`, planning document, or generic text continuation path.\n\
     - A plan, outline, research note, contract, or setup document is not completion evidence when the original request asks for fiction body text. It is only an intermediate artifact; continue with `novel_studio` until a draft/chapter/export receipt reports the requested writing scope or return a blocker naming the missing scope.\n\
     - If the user did not provide a title, infer a fresh non-hardcoded title and write it into the artifact before the body.\n\
     - When the task depends on knowledge-base material, use `tiered_search` or `fetch_document` before drafting. Do not use personal chat memory as source evidence unless the original request explicitly asks for prior conversation memory.\n\
     - For multi-step writing, keep the title, target audience, structure, terms, names, claims, and source references stable through the writing ledger instead of re-deciding them every turn.\n\
     - Return `status`, the executed writing/file tool, saved path or exported path, completion scope, and blockers. A requested saved file is not complete until a writing/file tool reports the path.\n\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_review_verdict_is_never_approved_by_substring() {
        assert_eq!(revision_next_action("未通过"), "revise");
        assert_eq!(revision_next_action("not passed"), "revise");
        assert_eq!(revision_next_action("passed"), "approve_or_export");
    }
}
