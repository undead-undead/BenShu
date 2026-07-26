use super::contract_text::*;
use super::*;

/// Boundary text gate for raw model contract output.
///
/// This intentionally checks malformed, noisy, or lossy text surfaces before
/// they are admitted into the typed creation contract. The typed
/// `NovelCreationContract` remains the authority for story-contract readiness.
pub fn generated_contract_boundary_text_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    let language = draft.language.to_ascii_lowercase();
    let expects_chinese = language.starts_with("zh") || draft.language.contains("中文");
    if expects_chinese {
        if let Some(fragment) = unexpected_non_cjk_script_fragment(contract_text) {
            issues.push(format!("中文合同混入非中文脚本残片：{fragment}"));
        }
        if let Some(fragment) = latex_or_escape_residue_fragment(contract_text) {
            issues.push(format!("中文合同混入转义或 LaTeX 残片：{fragment}"));
        }
        if let Some(fragment) = cjk_underscore_fragment(contract_text) {
            issues.push(format!("中文合同混入异常下划线残片：{fragment}"));
        }
        if let Some(fragment) = malformed_contract_bullet_prefix_fragment(contract_text) {
            issues.push(format!("中文合同混入异常列表前缀：{fragment}"));
        }
    }
    if let Some(fragment) = degenerate_repetition_fragment(contract_text) {
        issues.push(format!("合同出现连续重复退化片段：{fragment}"));
    }
    issues.sort();
    issues.dedup();
    issues
}
