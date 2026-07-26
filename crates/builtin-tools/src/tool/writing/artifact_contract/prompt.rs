use super::{ArtifactQualityContract, ArtifactQualityReport};
use benshu_compression::ellipsize;

pub(crate) fn build_file_artifact_prompt(
    path: &str,
    original_request: &str,
    evidence: &str,
    contract: &ArtifactQualityContract,
) -> String {
    let evidence_preview = ellipsize(evidence, 12_000);
    let section_list = contract.required_sections.join("、");
    let maximum = contract
        .max_chars
        .map(|value| format!("\n- 长度上限：不超过 {value} 个非空白字符。"))
        .unwrap_or_default();
    let citations = if contract.min_citations == 0 {
        "无强制引用数量；不得虚构来源。".to_string()
    } else {
        format!(
            "至少引用 {} 个能在证据回执中核验的不同来源。",
            contract.min_citations
        )
    };
    let delivery = match contract.delivery_scope {
        super::ArtifactDeliveryScope::Final => {
            "本轮交付最终完整产物，不得用阶段稿冒充完成。".to_string()
        }
        super::ArtifactDeliveryScope::Stage => format!(
            "本轮只交付可恢复的阶段产物；{}不得声称全文已经完成。",
            contract
                .final_target_chars
                .map(|target| format!("最终目标为 {target} 个非空白字符，"))
                .unwrap_or_default()
        ),
    };

    format!(
        "你是 BenShu 的文件产物 worker。根据原始请求和已验证证据生成可直接保存的完整文件正文。\n\n\
         目标路径：{path}\n\n\
         原始请求：\n{original_request}\n\n\
         已验证证据：\n{evidence_preview}\n\n\
         产物合同：\n\
         - artifact_type：{}\n\
         - delivery_scope：{}。{delivery}\n\
         - 最低交付规模：不少于 {} 个非空白字符。{}\n\
         - 必需结构：{}。\n\
         - 引用要求：{citations}\n\
         - 只输出文件正文，不要输出工具调用、JSON、内部推理、自检记录或解释性前言。\n\
         - 保留真实段落、列表和 Markdown 结构。\n\
         - 证据不足时明确说明缺失内容及已有来源，不得补造事实或引用。\n\
         - 研究或知识驱动创作只能提炼结构规律，不得复刻来源作品的专有角色、情节或设定。\n\
         - 使用用户请求的语言。",
        contract.artifact_type,
        contract.delivery_scope.as_str(),
        contract.min_chars,
        maximum,
        section_list
    )
}

pub(crate) fn build_file_artifact_revision_prompt(
    task: &str,
    path: &str,
    previous: &str,
    quality: &ArtifactQualityReport,
    contract: &ArtifactQualityContract,
    attempt: usize,
) -> String {
    let previous_preview = ellipsize(previous, 12_000);
    let blockers = issue_lines(&quality.blockers);
    let repairable = issue_lines(&quality.repairable);
    let warnings = issue_lines(&quality.warnings);
    let maximum = contract
        .max_chars
        .map(|value| format!("\n         - 全文不超过 {value} 个字符（按非空白字符计）。"))
        .unwrap_or_default();
    let instruction = if !quality.blockers.is_empty() {
        "存在会让产物不可用的硬错误。只重写受污染或缺失的部分；未受影响的有效正文必须保留。"
    } else {
        "只修复列出的结构、证据或长度问题；未涉及的正文、事实、措辞和段落顺序必须保留。"
    };

    format!(
        "你是 BenShu 的文件产物 worker。第 {attempt} 次定向修订未通过质量合同的文件。\n\n\
         目标路径：{path}\n\n\
         原任务：\n{task}\n\n\
         硬阻塞：\n{blockers}\n\n\
         可修复问题：\n{repairable}\n\n\
         警告：\n{warnings}\n\n\
         上一版正文：\n{previous_preview}\n\n\
         修订要求：\n\
         - {instruction}\n\
         - 最低交付规模仍为 {} 个非空白字符。{maximum}\n\
         - 返回完整修订文件，以便原子替换；不要只返回补丁或说明。\n\
         - 不要输出内部推理、自检记录、工具标签或解释性前言。",
        contract.min_chars
    )
}

fn issue_lines(values: &[String]) -> String {
    if values.is_empty() {
        return "- 无".to_string();
    }
    values
        .iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}
