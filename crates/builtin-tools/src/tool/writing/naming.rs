mod chapter_title;
mod character;
mod title;
mod title_lexicon;
mod title_policy;

#[cfg(test)]
pub(crate) use chapter_title::chapter_title_needs_post_body_repair;
pub(crate) use chapter_title::{
    chapter_title_core, chapter_title_template, evaluate_chapter_title_candidate,
    fatigue_issues as chapter_title_fatigue_issues,
    generic_stage_label as generic_chapter_stage_label,
    prose_grammar_fragment as chapter_title_prose_grammar_fragment,
    registry_issues as chapter_title_registry_issues, select_final_chapter_title_from_body,
    sentence_fragment_edge as chapter_title_sentence_fragment_edge, title_body_fragment_issue,
    title_is_default_chapter_heading, title_matches_project_or_volume, title_template_connector,
    ChapterTitleCandidate, ChapterTitleContext, ChapterTitleEvidence,
};
pub(crate) use character::{allocate_character_name, audit_character_name_candidate};
#[cfg(test)]
pub(crate) use title::title_language_mismatch;
pub(crate) use title::{
    book_title_candidate_rationale_from_story_evidence,
    declared_book_title_candidates_from_contract_evidence,
    generated_project_title_looks_stale_for_task, prefers_chinese_output,
    select_book_title_candidate_decision, title_anchor_tokens, title_contract_basis_issue,
    title_formality_issue, title_rationale_is_concrete, BookTitleCandidate, BookTitleEvidence,
};
pub(crate) use title_lexicon::title_meta_discussion_markers;
