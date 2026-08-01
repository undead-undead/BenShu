use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GovernanceClauseKind {
    Hard,
    CharacterBottomLine,
    PremiseOrOutline,
}

#[derive(Clone, Debug)]
struct GovernanceClause {
    text: String,
    kind: GovernanceClauseKind,
}

#[derive(Clone, Debug, Default)]
pub(in crate::tool::writing::novel_studio) struct ContractGovernanceLeakageReport {
    pub(in crate::tool::writing::novel_studio) blocking: Vec<String>,
    pub(in crate::tool::writing::novel_studio) warnings: Vec<String>,
}

pub(in crate::tool::writing::novel_studio) fn contract_governance_leakage_report(
    manifest: &NovelProjectManifest,
    content: &str,
) -> ContractGovernanceLeakageReport {
    if content.trim().is_empty() {
        return ContractGovernanceLeakageReport::default();
    }
    let sentences = prose_sentences_for_leakage_probe(content);
    if sentences.is_empty() {
        return ContractGovernanceLeakageReport::default();
    }
    let mut blocking = Vec::new();
    let mut warnings = Vec::new();
    for clause in contract_governance_clauses(manifest) {
        let normalized_clause = normalize_cjk_contract_probe(&clause.text);
        if normalized_clause.chars().count() >= 4 {
            if let Some(sentence) = sentences.iter().find(|sentence| {
                sentence_has_contract_meta_surface(sentence)
                    && normalize_cjk_contract_probe(sentence).contains(&normalized_clause)
            }) {
                blocking.push(format!(
                    "chapter body appears to quote a contract/governance clause instead of dramatizing it: {}",
                    preview_chars(sentence, 48)
                ));
                continue;
            }
        }
        if normalized_clause.chars().count() < 10 {
            continue;
        }
        let terms = cjk_probe_terms(&normalized_clause, 4);
        if terms.len() < 4 {
            continue;
        }
        let Some((sentence, severity)) = sentences.iter().find_map(|sentence| {
            contract_clause_leak_severity(&terms, sentence, clause.kind)
                .map(|severity| (sentence, severity))
        }) else {
            continue;
        };
        let issue = format!(
                "chapter body appears to quote a contract/governance clause instead of dramatizing it: {}",
                preview_chars(sentence, 48)
            );
        match severity {
            ContractLeakageSeverity::Blocking => blocking.push(issue),
            ContractLeakageSeverity::Warning => warnings.push(issue),
        }
    }
    blocking.sort();
    blocking.dedup();
    warnings.sort();
    warnings.dedup();
    ContractGovernanceLeakageReport { blocking, warnings }
}

fn contract_governance_clauses(manifest: &NovelProjectManifest) -> Vec<GovernanceClause> {
    let mut clauses = Vec::new();
    if let Some(contract) = &manifest.contract {
        clauses.push(GovernanceClause {
            text: contract.premise.clone(),
            kind: GovernanceClauseKind::PremiseOrOutline,
        });
        clauses.push(GovernanceClause {
            text: contract.outline.clone(),
            kind: GovernanceClauseKind::PremiseOrOutline,
        });
        clauses.extend(
            contract
                .themes
                .iter()
                .cloned()
                .map(|text| GovernanceClause {
                    text,
                    kind: GovernanceClauseKind::PremiseOrOutline,
                }),
        );
        for character in &contract.characters {
            clauses.extend(contract_labeled_governance_values(character));
        }
        clauses.extend(
            contract
                .must_avoid
                .iter()
                .cloned()
                .map(|text| GovernanceClause {
                    text,
                    kind: GovernanceClauseKind::Hard,
                }),
        );
    }
    for character in &manifest.character_ledger {
        clauses.push(GovernanceClause {
            text: character.bottom_line.clone(),
            kind: GovernanceClauseKind::CharacterBottomLine,
        });
        clauses.extend(
            character
                .forbidden_renames
                .iter()
                .cloned()
                .map(|text| GovernanceClause {
                    text,
                    kind: GovernanceClauseKind::Hard,
                }),
        );
    }
    if let Some(bible) = &manifest.story_bible {
        for character in &bible.character_ledger {
            clauses.push(GovernanceClause {
                text: character.bottom_line.clone(),
                kind: GovernanceClauseKind::CharacterBottomLine,
            });
        }
    }
    for character in &manifest.structured_contract_v2.character_voice_ledger {
        clauses.extend(character.forbidden_expressions.iter().cloned().map(|text| {
            GovernanceClause {
                text,
                kind: GovernanceClauseKind::Hard,
            }
        }));
    }
    clauses
        .into_iter()
        .map(|mut clause| {
            clause.text = clause.text.trim().to_string();
            clause
        })
        .filter(|clause| !clause.text.is_empty())
        .collect()
}

fn contract_labeled_governance_values(value: &str) -> Vec<GovernanceClause> {
    let labels = ["bottom_line", "底线", "must_avoid", "必须避免", "禁忌"];
    let mut out = Vec::new();
    for label in labels {
        let mut rest = value;
        while let Some(index) = rest.find(label) {
            let after_label = &rest[index + label.len()..];
            let after_separator = after_label
                .trim_start()
                .strip_prefix([':', '：'])
                .unwrap_or(after_label)
                .trim_start();
            let end = after_separator
                .find([';', '；', '\n', '\r'])
                .unwrap_or(after_separator.len());
            let candidate = after_separator[..end].trim();
            if !candidate.is_empty() {
                out.push(GovernanceClause {
                    text: candidate.to_string(),
                    kind: if matches!(label, "bottom_line" | "底线") {
                        GovernanceClauseKind::CharacterBottomLine
                    } else {
                        GovernanceClauseKind::Hard
                    },
                });
            }
            rest = &after_separator[end.min(after_separator.len())..];
        }
    }
    out
}

fn prose_sentences_for_leakage_probe(content: &str) -> Vec<String> {
    content
        .split(|ch| matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n' | '\r'))
        .map(str::trim)
        .filter(|sentence| sentence.chars().filter(|ch| is_cjk_unified(*ch)).count() >= 8)
        .map(ToString::to_string)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContractLeakageSeverity {
    Blocking,
    Warning,
}

fn contract_clause_leak_severity(
    terms: &[String],
    sentence: &str,
    kind: GovernanceClauseKind,
) -> Option<ContractLeakageSeverity> {
    let normalized_sentence = normalize_cjk_contract_probe(sentence);
    if normalized_sentence.chars().count() < 8 {
        return None;
    }
    let matches = terms
        .iter()
        .filter(|term| normalized_sentence.contains(term.as_str()))
        .count();
    match kind {
        GovernanceClauseKind::Hard => (matches >= 3 && matches * 2 >= terms.len().min(12))
            .then_some(ContractLeakageSeverity::Blocking),
        GovernanceClauseKind::CharacterBottomLine => {
            let near_exact = matches >= 3 && matches * 2 >= terms.len().min(12);
            if sentence_has_contract_meta_surface(sentence) && matches >= 3 {
                Some(ContractLeakageSeverity::Blocking)
            } else {
                near_exact.then_some(ContractLeakageSeverity::Warning)
            }
        }
        GovernanceClauseKind::PremiseOrOutline => {
            let near_exact = matches >= 4 && matches * 100 >= terms.len().min(16) * 70;
            if sentence_has_contract_meta_surface(sentence) && matches >= 3 {
                Some(ContractLeakageSeverity::Blocking)
            } else {
                near_exact.then_some(ContractLeakageSeverity::Warning)
            }
        }
    }
}

fn sentence_has_contract_meta_surface(sentence: &str) -> bool {
    [
        "创作合同",
        "写作合同",
        "小说合同",
        "故事合同",
        "合同字段",
        "合同要求",
        "合同约束",
        "合同草案",
        "项目约束",
        "大纲",
        "提纲",
        "设定",
        "本章",
        "主角弧线",
        "终局",
        "世界观",
        "必须避免",
        "创作",
        "读者",
        "剧情要求",
    ]
    .iter()
    .any(|marker| sentence.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::sentence_has_contract_meta_surface;

    #[test]
    fn in_story_contract_dialogue_is_not_writing_meta_surface() {
        assert!(!sentence_has_contract_meta_surface(
            "那你的球员合同就失效了，除非赛季结束前找到新赞助商"
        ));
        assert!(!sentence_has_contract_meta_surface(
            "他在桌上签下商业合同，当晚就接管了工厂"
        ));
    }

    #[test]
    fn explicit_writing_contract_surface_remains_meta() {
        assert!(sentence_has_contract_meta_surface(
            "本章必须遵守写作合同要求并完成主角弧线"
        ));
        assert!(sentence_has_contract_meta_surface(
            "按照小说合同字段，终局必须兑现伏笔"
        ));
    }

    #[test]
    fn in_story_use_of_story_is_not_writing_meta_surface() {
        assert!(!sentence_has_contract_meta_surface(
            "这片地下空间像一个曾经承载过生命且带有某种未竟故事的遗迹"
        ));
        assert!(sentence_has_contract_meta_surface(
            "这份故事合同要求本章必须完成失踪案线索"
        ));
    }

    #[test]
    fn ordinary_compound_noun_containing_ben_wen_is_not_writing_meta_surface() {
        assert!(!sentence_has_contract_meta_surface(
            "他怀中紧紧抱着那本文明记录本，生怕冰晶生物将它夺走"
        ));
    }
}

fn normalize_cjk_contract_probe(value: &str) -> String {
    value
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .collect::<String>()
}

fn cjk_probe_terms(value: &str, window: usize) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() < window {
        return Vec::new();
    }
    let mut terms = chars
        .windows(window)
        .map(|window| window.iter().collect::<String>())
        .filter(|term| !contract_probe_term_is_too_generic(term))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn contract_probe_term_is_too_generic(term: &str) -> bool {
    ["不会", "不能", "不要", "无谓", "解释", "关系"]
        .iter()
        .any(|generic| term == *generic)
}
