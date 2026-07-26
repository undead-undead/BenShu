use super::contract_text::{
    count_explicit_chapter_plan_lines, generated_contract_field, generated_fiction_character_lines,
    generated_fiction_outline,
};

pub(crate) fn generated_fiction_contract_planning_issues(
    contract_text: &str,
    _require_divergent_naming: bool,
) -> Vec<String> {
    let mut issues = Vec::new();
    let required_fields: &[(&[&str], &[&str], &str)] = &[
        (
            &[
                "终局方向",
                "结局方向",
                "结尾承诺",
                "终局承诺",
                "Ending Direction",
            ],
            &["结尾", "结局", "终局", "收束", "ending"],
            "小说合同缺少终局方向字段，书名和全书规划无法从结局倒推",
        ),
        (
            &["主角弧线", "主角弧光", "成长线", "Protagonist Arc"],
            &["主角", "成长", "弧线", "弧光", "protagonist"],
            "小说合同缺少主角弧线字段，角色长期一致性不足",
        ),
        (
            &[
                "世界观意象",
                "世界观意意象",
                "世界意象",
                "核心意象",
                "关键意象",
                "World Imagery",
            ],
            &["世界观", "世界规则", "力量体系", "设定", "意象", "world"],
            "小说合同缺少世界观意象字段，书名缺少当前故事独有依据",
        ),
        (
            &[
                "总主线因果链",
                "主线因果链",
                "主线因果",
                "Main Causal Spine",
            ],
            &["主线", "因果", "核心矛盾", "关键转折", "本章目标", "causal"],
            "小说合同缺少总主线因果链字段，长篇推进容易变成散章",
        ),
        (
            &[
                "命名依据合同",
                "命名理由",
                "书名理由",
                "标题理由",
                "Title Rationale",
            ],
            &[
                "命名依据合同",
                "命名理由",
                "书名来自",
                "标题来自",
                "书名取自",
                "title rationale",
            ],
            "小说合同缺少书名命名理由，无法验证书名是否来自剧情和结局",
        ),
    ];
    for (labels, semantic_terms, issue) in required_fields {
        if generated_contract_field(contract_text, labels).is_none()
            && !semantic_terms.iter().any(|term| {
                contract_text.contains(term)
                    || contract_text
                        .to_ascii_lowercase()
                        .contains(&term.to_ascii_lowercase())
            })
        {
            issues.push((*issue).to_string());
        }
    }

    let outline = generated_fiction_outline(contract_text);
    if outline.trim().is_empty() {
        issues.push("小说合同缺少全书大纲、分卷/阶段安排或逐章规划".to_string());
    }
    let plot_evidence = format!("{contract_text}\n{outline}");
    let has_plot_chain = [
        "主要情节",
        "情节链",
        "主线",
        "因果",
        "关键转折",
        "高潮",
        "收束",
    ]
    .iter()
    .any(|term| plot_evidence.contains(term))
        || plot_evidence.to_ascii_lowercase().contains("plot");
    if !has_plot_chain && count_explicit_chapter_plan_lines(&plot_evidence) < 2 {
        issues.push("小说合同缺少主要情节链或关键转折安排".to_string());
    }

    if generated_fiction_character_lines(contract_text).is_empty()
        && !contract_text.contains("主角")
        && !contract_text.to_ascii_lowercase().contains("protagonist")
    {
        issues.push("小说合同缺少可解析的角色职责锚点".to_string());
    }

    issues
}
