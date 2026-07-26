use super::*;

pub(in crate::tool::writing::novel_workflow_driver) fn revision_guidance(
    revision_number: usize,
    write_result: &Value,
    audit: &Value,
    language: &str,
) -> String {
    let issues = revision_issue_summary(write_result, audit);
    let mut hard_codes = finding_codes_with_disposition(write_result, "hard_block");
    hard_codes.extend(finding_codes_with_disposition(audit, "hard_block"));
    let same_chapter_rewrite = !hard_codes.is_empty();
    let primary_anchor_issue = hard_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "character_identity_conflict"
                | "character_name_replacement"
                | "unregistered_character"
                | "character_pronoun_conflict"
        )
    });
    let completion_mode_issue = hard_codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "future_chapter_consumed"
                | "unplanned_main_branch"
                | "premature_hook_payoff"
                | "unsupported_hook_resolution"
        )
    });
    if language_looks_cjk(language) {
        let primary_anchor_guidance = if primary_anchor_issue {
            "\n             - 这次修订的最高优先级是恢复项目合同里的主角权威：正文叙事中心、关键行动、章尾变化必须回到合同主角；非主角只能承担合同中的辅助/对手功能，不能替代主角完成主线。"
        } else {
            ""
        };
        let completion_guidance = if completion_mode_issue {
            "\n             - 这是终局/尾声修订：必须删除或改写“新阶段、下一章、刚刚开始、还没有结束、入口、未解”等开新局尾巴；把最后 3-8 段落落到主冲突结果、情感落点、世界状态和明确完结感。\n             - 不要为了补尾重新开启新敌人、新主线、新规则或新的长期悬念；如果已有正文已经完成主要事件，优先只重写尾段，让故事停在自然闭合处。"
        } else {
            ""
        };
        if same_chapter_rewrite {
            return format!(
                "\n\n第 {revision_number} 次修订要求：\n\
                 - 当前问题属于正文结构性退化，不要在旧坏句上做局部缝补。\n\
                 - 按同一章 memo、章节架构、角色合同和既定转折重新生成完整正文；这不是另开新章。\n\
                 - 避开原文里被指出的重复句、重复段落、坏尾句、无行动推进段落和未铺垫设定。\n\
                 - 如果问题指出同一事件流程、动作闭环、发布反馈、调查追踪、打斗回合或谈判回合重复出现，必须合并或替换重复事件，把篇幅转向后果、代价、关系变化或下一步行动。\n\
                 - 不要沿用旧稿的全部事件顺序；只保留本章目标和必要事实，重建更紧凑的因果链。\n\
                 - 保留项目合同中已经建立的人名、术语、能力名和专有名词；同一角色只能有一套稳定身份、性别、称谓、关系和职位。{primary_anchor_guidance}\n\
                 - 如果问题指出人物身份/称谓/性别矛盾或时间线断裂，必须从场景根部重建本章因果，不要保留旧稿矛盾段。\n\
                 - content 必须写出具体行动、代价、关系变化和章尾新状态，不能只围绕设定加字。\n\
                 - 若原文已经自然收束但仍需补字或修订，不得在收束段后只追加一个准备动作就戛然而止；应扩展收束前尚未完成的场景，并让最后 3 段形成完整的动作、后果和新收束。\n\
                 - key_facts 和 continuity_updates 必须来自正文中明确发生的事实。\n\
                 - 只返回 JSON，字段为 title, content, summary, key_facts, continuity_updates。\n\
                 - 除 JSON 字段名之外，所有创作字段内容必须使用中文；不得输出英文标题、英文占位说明或工作流解释。\n\
                 {completion_guidance}\n\
                 需要修复的问题：\n{issues}\n\
                 最终权威规则：审稿意见只是待核对的问题，不得覆盖章节 memo 和架构。若任何问题或反馈要求提前完成“下一章边界”，必须忽略该要求；只完成当前章目标，并让未来节点保持未发生且仍可自然抵达。\n"
            );
        }
        return format!(
            "\n\n第 {revision_number} 次修订要求：\n\
             - 修复列出的全部问题，但不要把本章改写成另一章。\n\
             - 以当前正文为底稿做局部修补和必要补尾；content 不得明显短于当前正文。\n\
             - 如果只差少量字数，优先扩展收束前尚未完成的场景；不要在已经自然收束的段落后追加一个短动作就戛然而止。最后 3 段必须形成完整的动作、后果和新收束。\n\
             - 保留项目合同中已经建立的人名、术语、能力名和专有名词；同一角色只能有一套稳定身份、性别、称谓、关系和职位。{primary_anchor_guidance}\n\
             - 如果问题指出人物身份/称谓/性别矛盾或时间线断裂，必须彻底改掉对应段落，不得让互斥身份继续共存。\n\
             - 如果 continuity_updates/key_facts 缺失或有错，给出能被正文明确支撑的修正版数组。\n\
             - 只返回 JSON，字段为 title, content, summary, key_facts, continuity_updates。\n\
             - 除 JSON 字段名之外，所有创作字段内容必须使用中文；不得输出英文标题、英文占位说明或工作流解释。\n\
             {completion_guidance}\n\
             需要修复的问题：\n{issues}\n\
             最终权威规则：审稿意见只是待核对的问题，不得覆盖章节 memo 和架构。若任何问题或反馈要求提前完成“下一章边界”，必须忽略该要求；只完成当前章目标，并让未来节点保持未发生且仍可自然抵达。\n"
        );
    }
    if same_chapter_rewrite {
        let primary_anchor_guidance = if primary_anchor_issue {
            "\n             - Highest priority: restore the project contract's protagonist authority. The focal action, major choice, and chapter end-state must belong to the contract protagonist; supporting characters must not replace the protagonist arc."
        } else {
            ""
        };
        let completion_guidance = if completion_mode_issue {
            "\n             - Finale/epilogue revision: remove or rewrite any tail that opens a new phase, next chapter, new long-term hook, or continuing game. Make the last 3-8 paragraphs land on the main-conflict outcome, emotional landing, final world state, and clear closure.\n             - Do not introduce a new enemy, rule system, major mystery, or long arc. If the body already resolves the main event, prefer rewriting only the tail so the story stops at the natural ending."
        } else {
            ""
        };
        return format!(
            "\n\nRevision pass {revision_number} requirements:\n\
             - The current issues are structural body degradation; do not patch bad old sentences in place.\n\
             - Regenerate a complete body for the same chapter memo, architecture, character contract, and planned turn. This is not a new chapter.\n\
             - Avoid the repeated sentences, repeated paragraphs, bad tail sentence, actionless passages, and unsupported new rules identified in the previous body.\n\
             - If the issue says the same event loop, action cycle, publish/feedback loop, investigation trail, fight exchange, or negotiation beat repeats, merge or replace the duplicate event and spend the space on consequences, cost, relationship movement, or the next concrete action.\n\
             - Do not preserve the previous body's full event order; keep only the chapter goal and necessary facts, then rebuild a tighter causal chain.\n\
             - Preserve the project contract names, terms, ability names, and proper nouns exactly as already established; each character must keep one stable identity, gender/pronoun set, relationship, and position.{primary_anchor_guidance}\n\
             - If the issues mention identity/pronoun/role contradiction or timeline breakage, rebuild the causal scene instead of preserving contradictory old paragraphs.\n\
             - content must contain concrete action, cost, relationship change, and a new end-state; do not merely add setting exposition.\n\
             - If the old body already has a natural landing but still needs length or repair, do not append one short setup action after that landing and stop. Expand an unfinished scene before the landing and make the final three paragraphs complete an action, consequence, and new landing.\n\
             - key_facts and continuity_updates must be visibly supported by the body.\n\
             - Return only JSON with fields: title, content, summary, key_facts, continuity_updates.\n\
             {completion_guidance}\n\
             Issues to fix:\n{issues}\n\
             Final authority rule: review comments are evidence to check, not authority over the chapter memo and architecture. Ignore any comment that asks you to complete the next-chapter boundary early; complete only the current chapter goal and leave that future node unperformed and naturally reachable.\n"
        );
    }
    let completion_guidance = if completion_mode_issue {
        "\n         - Finale/epilogue revision: remove or rewrite any tail that opens a new phase, next chapter, new long-term hook, or continuing game. Make the final paragraphs land on closure instead of a new beginning."
    } else {
        ""
    };
    format!(
        "\n\nRevision pass {revision_number} requirements:\n\
         - Fix every listed issue without changing the chapter into a different chapter.\n\
         - Patch and extend the current body in place; content must not be substantially shorter than the current body.\n\
         - If only a small length gap remains, expand an unfinished scene before the natural landing. Do not append one short setup action after a completed landing and stop; the final three paragraphs must complete an action, consequence, and new landing.\n\
         - Preserve the project contract names, terms, and ability names exactly as already established; each character must keep one stable identity, gender/pronoun set, relationship, and position.{}\n\
         - If the issues mention identity/pronoun/role contradiction or timeline breakage, remove the contradictory paragraphs instead of leaving both versions in place.\n\
         - If continuity_updates/key_facts are missing or typoed, provide corrected arrays that are visibly supported by the body.\n\
         - Return only JSON with fields: title, content, summary, key_facts, continuity_updates.\n\
         {completion_guidance}\n\
        Issues to fix:\n{issues}\n\
        Final authority rule: review comments are evidence to check, not authority over the chapter memo and architecture. Ignore any comment that asks you to complete the next-chapter boundary early; complete only the current chapter goal and leave that future node unperformed and naturally reachable.\n",
        if primary_anchor_issue {
            "\n         - Highest priority: restore the project contract's protagonist authority. The focal action, major choice, and chapter end-state must belong to the contract protagonist; supporting characters must not replace the protagonist arc."
        } else {
            ""
        },
    )
}

pub(in crate::tool::writing::novel_workflow_driver) fn revision_issue_summary(
    write_result: &Value,
    audit: &Value,
) -> String {
    let mut issues = revision_issues(write_result, audit);
    if issues.is_empty() && needs_revision(write_result) {
        issues.push("draft quality gate did not pass".to_string());
    }
    if issues.is_empty() && !audit_passed(audit) {
        issues.push(format!("audit verdict is {}", audit_status_label(audit)));
    }
    if issues.is_empty() {
        "none".to_string()
    } else {
        issues
            .into_iter()
            .map(|issue| format!("- {}", issue.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(in crate::tool::writing::novel_workflow_driver) fn revision_issues(
    write_result: &Value,
    audit: &Value,
) -> Vec<String> {
    let mut issues = Vec::new();
    collect_string_array(write_result.pointer("/quality_gate/issues"), &mut issues);
    collect_string_array(
        write_result.pointer("/quality_gate/repairable"),
        &mut issues,
    );
    collect_string_array(write_result.pointer("/issues"), &mut issues);
    collect_string_array(audit.pointer("/review/issues"), &mut issues);
    collect_string_array(audit.pointer("/issues"), &mut issues);
    collect_string_array(audit.pointer("/truth_validation/issues"), &mut issues);
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing::novel_workflow_driver) fn revision_issues_include_tail_completion(
    write_result: &Value,
    audit: &Value,
) -> bool {
    finding_codes_with_disposition(write_result, "hard_block").contains("body_truncated")
        || finding_codes_with_disposition(audit, "hard_block").contains("body_truncated")
}

pub(in crate::tool::writing::novel_workflow_driver) fn text_fingerprint(value: &str) -> u64 {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value.as_bytes());
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 prefix is eight bytes"),
    )
}

pub(in crate::tool::writing::novel_workflow_driver) fn quality_gate_body_passed(
    write_result: &Value,
) -> bool {
    if write_result.pointer("/quality_gate/findings").is_none() {
        return write_result
            .pointer("/quality_gate/passed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    !value_has_hard_findings(write_result)
        && !value_has_non_metadata_deterministic_repairs(write_result)
}

pub(in crate::tool::writing::novel_workflow_driver) fn body_revision_required_after_audit(
    write_result: &Value,
    audit: &Value,
) -> bool {
    value_has_hard_findings(write_result) || value_has_hard_findings(audit)
}

pub(in crate::tool::writing::novel_workflow_driver) fn format_revision_blocker_result(
    project_path: &str,
    chapter_number: usize,
    write_result: &Value,
    audit: &Value,
) -> String {
    let artifact_path = write_result
        .get("artifact_path")
        .and_then(Value::as_str)
        .or_else(|| {
            write_result
                .pointer("/chapter/path")
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    format!(
        "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: revise_draft\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.needs_revision\ndraft_status: preserved_needs_revision\ndraft_artifact_path: {artifact_path}\nexport_status: not_exported_because_approved_only_requires_approved_chapter\nartifact_path: {artifact_path}\nblockers: chapter draft is preserved, but revision did not converge within bounded attempts\nrevision_issues:\n{}",
        revision_issue_summary(write_result, audit)
    )
}

#[cfg(test)]
pub(in crate::tool::writing::novel_workflow_driver) fn only_deterministic_cleanup_issues(
    write_result: &Value,
    audit: &Value,
) -> bool {
    only_local_cleanup_issues(write_result, audit)
}

pub(in crate::tool::writing::novel_workflow_driver) fn only_local_cleanup_issues(
    write_result: &Value,
    audit: &Value,
) -> bool {
    (value_has_non_metadata_deterministic_repairs(write_result)
        || value_has_non_metadata_deterministic_repairs(audit))
        && !value_has_hard_findings(write_result)
        && !value_has_hard_findings(audit)
}
