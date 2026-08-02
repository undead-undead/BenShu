use super::*;

mod character_identity;
mod contract_leakage;
mod surface_noise;

pub(super) use character_identity::*;
pub(super) use contract_leakage::contract_governance_leakage_report;
pub(super) use surface_noise::*;

pub(super) fn anchor_malformed_predicate_issues(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    let anchors = explicit_manifest_character_anchors(manifest);
    if anchors.is_empty() {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for anchor in &anchors {
        if anchor.chars().count() < 2 || anchor.chars().count() > 6 {
            continue;
        }
        if let Some(fragment) = malformed_anchor_phrase(content, &anchor) {
            issues.push(format!(
                "chapter body contains malformed phrase near stable character anchor `{anchor}`: {fragment}"
            ));
        }
        for other in &anchors {
            if anchor == other {
                continue;
            }
            let joined = format!("{anchor}{other}");
            if content.contains(&joined) {
                issues.push(format!(
                    "chapter body contains adjacent stable character anchors without syntax boundary `{anchor}` + `{other}`: {joined}"
                ));
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(super) fn malformed_anchor_phrase(content: &str, anchor: &str) -> Option<String> {
    let tails = [
        "识到",
        "觉到",
        "到，",
        "到。",
        "静地",
        "冷地",
        "孔",
        "吸",
        "光",
        "呼吸，",
        "中涌",
        "理会",
        "神一凛",
        "头一震",
        "脏猛",
        "睛",
        "原地",
        "一种",
        "一阵",
        "一个",
    ];
    for tail in tails {
        let candidate = format!("{anchor}{tail}");
        for (index, _) in content.match_indices(&candidate) {
            if !malformed_anchor_has_left_name_boundary(content, index) {
                continue;
            }
            if malformed_anchor_tail_is_normal_usage(content, index + candidate.len(), tail) {
                continue;
            }
            return Some(candidate);
        }
    }
    let chars = content.chars().collect::<Vec<_>>();
    let anchor_chars = anchor.chars().collect::<Vec<_>>();
    if anchor_chars.is_empty() || chars.len() < anchor_chars.len() + 2 {
        return None;
    }
    let particles = ['吗', '吧', '嘛', '呀', '呢'];
    for index in 0..=chars.len().saturating_sub(anchor_chars.len() + 2) {
        if chars[index..index + anchor_chars.len()] != anchor_chars[..] {
            continue;
        }
        if !malformed_anchor_has_left_name_boundary_chars(&chars, index) {
            continue;
        }
        if let Some(fragment) =
            demonstrative_anchor_fragment(&chars, index, anchor_chars.len(), anchor)
        {
            return Some(fragment);
        }
        let particle_index = index + anchor_chars.len();
        if !particles.contains(&chars[particle_index]) {
            continue;
        }
        let next = chars.get(particle_index + 1).copied();
        if malformed_anchor_particle_starts_normal_word(&chars, particle_index) {
            continue;
        }
        if next.is_some_and(is_cjk_unified) {
            let end = (particle_index + 6).min(chars.len());
            return Some(chars[index..end].iter().collect());
        }
    }
    for index in 0..=chars.len().saturating_sub(anchor_chars.len() + 2) {
        if chars[index..index + anchor_chars.len()] != anchor_chars[..] {
            continue;
        }
        if !malformed_anchor_has_left_name_boundary_chars(&chars, index) {
            continue;
        }
        let verb_index = index + anchor_chars.len();
        if chars.get(verb_index) != Some(&'到') {
            continue;
        }
        let next = chars.get(verb_index + 1).copied();
        if !next.is_some_and(is_cjk_unified)
            || next.is_some_and(|ch| matches!(ch, '了' | '达' | '底' | '处' | '站' | '场' | '口'))
        {
            continue;
        }
        let end = (verb_index + 5).min(chars.len());
        return Some(chars[index..end].iter().collect());
    }
    None
}

fn malformed_anchor_particle_starts_normal_word(chars: &[char], particle_index: usize) -> bool {
    matches!(
        (chars.get(particle_index), chars.get(particle_index + 1)),
        (Some('呢'), Some('喃'))
    )
}

fn malformed_anchor_has_left_name_boundary(content: &str, byte_index: usize) -> bool {
    let prev = content[..byte_index].chars().next_back();
    prev.is_none_or(|ch| !is_cjk_unified(ch))
}

fn malformed_anchor_has_left_name_boundary_chars(chars: &[char], index: usize) -> bool {
    index == 0
        || chars
            .get(index.saturating_sub(1))
            .is_none_or(|ch| !is_cjk_unified(*ch))
}

fn malformed_anchor_tail_is_normal_usage(
    content: &str,
    byte_after_candidate: usize,
    tail: &str,
) -> bool {
    if tail == "一个" {
        return content[byte_after_candidate..].chars().next() == Some('人');
    }
    false
}

pub(super) fn demonstrative_anchor_fragment(
    chars: &[char],
    index: usize,
    anchor_len: usize,
    _anchor: &str,
) -> Option<String> {
    let after = index + anchor_len;
    if chars.get(after) != Some(&'那') {
        return None;
    }
    let end = (after + 12).min(chars.len());
    let fragment = chars[index..end].iter().collect::<String>();
    if fragment.contains('旁') || fragment.contains('前') || fragment.contains('后') {
        Some(fragment)
    } else {
        None
    }
}

pub(super) fn chapter_is_title_reference_candidate(chapter: &ChapterRecord) -> bool {
    !chapter_lifecycle::status_is_rejected(&chapter.status)
        && title_has_enough_signal(&chapter.title)
}

pub(super) fn chinese_chapter_title_core(title: &str) -> String {
    naming::chapter_title_core(title)
}

pub(super) fn chapter_progression_contract_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if chapter.unit_count < 500 {
        return issues;
    }
    if chapter.key_facts.is_empty() {
        issues.push("chapter has no key_facts showing what actually changed".to_string());
    }
    if chapter.continuity_updates.is_empty() {
        issues.push("chapter has no continuity_updates for the next chapter".to_string());
    }
    let evidence = format!(
        "{}\n{}\n{}\n{}",
        chapter.summary,
        chapter.key_facts.join("\n"),
        chapter.continuity_updates.join("\n"),
        chapter_progression_evidence_body(content)
    );
    if !contains_state_change_signal(&evidence, &manifest.language) {
        issues
            .push("chapter does not show a durable state change or irreversible event".to_string());
    }
    if !contains_specific_state_change_signal(&evidence, &manifest.language) {
        issues.push(
            "chapter progression is too generic; key_facts/continuity_updates must name a concrete action, object, relationship, place, or consequence"
                .to_string(),
        );
    }
    issues
}

fn chapter_progression_evidence_body(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    if chars.len() <= 4_000 {
        return content.to_string();
    }
    let head = chars.iter().take(2_000).collect::<String>();
    let tail = chars
        .iter()
        .skip(chars.len().saturating_sub(2_000))
        .collect::<String>();
    format!("{head}\n{tail}")
}

pub(super) fn chapter_completion_mode_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    if !chapter_is_completion_mode_candidate(manifest, chapter) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    let evidence = format!(
        "{}\n{}\n{}\n{}",
        chapter.title,
        chapter.summary,
        chapter.key_facts.join("\n"),
        chapter.continuity_updates.join("\n")
    );
    if contains_new_open_hook_signal(&evidence) || ending_looks_like_cliffhanger(content) {
        issues.push(
            "completion-mode chapter appears to create a new unresolved hook instead of closing the project"
                .to_string(),
        );
    }
    if tail_reopens_after_closure(&manifest.language, content) {
        issues.push(
            "completion-mode chapter closes the story and then reopens a new phase; stop at the natural ending or make the epilogue bounded"
                .to_string(),
        );
    }
    if !contains_closure_signal(&evidence) && !contains_closure_signal(content) {
        issues.push(
            "completion-mode chapter lacks a clear closure/payoff signal for the ending"
                .to_string(),
        );
    }
    issues.extend(completion_obligation_issues(manifest, content));
    issues
}

pub(super) fn completion_obligation_issues(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if contract_requires_relationship_payoff(manifest)
        && !content_has_relationship_payoff(manifest, content)
    {
        issues.push(
            "completion-mode chapter does not visibly pay off the promised relationship/emotional arc"
                .to_string(),
        );
    }
    if contract_requires_antagonist_payoff(manifest)
        && !content_has_antagonist_payoff(manifest, content)
    {
        issues.push(
            "completion-mode chapter does not visibly resolve the promised antagonist/opposition arc"
                .to_string(),
        );
    }
    issues
}

pub(super) fn contract_requires_relationship_payoff(manifest: &NovelProjectManifest) -> bool {
    structured_contracts_for_manifest(manifest).any(|contract| {
        !contract.relationship_ledger.is_empty()
            || !contract.emotional_contract.payoff_requirements.is_empty()
            || !contract
                .emotional_contract
                .ending_emotional_state
                .trim()
                .is_empty()
    }) || legacy_contract_requires_relationship_payoff(manifest)
}

pub(super) fn content_has_relationship_payoff(
    manifest: &NovelProjectManifest,
    content: &str,
) -> bool {
    structured_contracts_for_manifest(manifest).any(|contract| {
        contract.relationship_ledger.iter().any(|entry| {
            contract_target_visible_in_content(
                content,
                &[
                    entry.desired_end_state.as_str(),
                    entry.next_expected_stage.as_str(),
                    entry.relationship_type.as_str(),
                ],
            )
        }) || contract_target_visible_in_content(
            content,
            &[
                contract.emotional_contract.ending_emotional_state.as_str(),
                contract.emotional_contract.emotional_promise.as_str(),
            ],
        ) || contract
            .emotional_contract
            .payoff_requirements
            .iter()
            .any(|target| contract_target_visible_in_content(content, &[target.as_str()]))
    }) || legacy_contract_relationship_payoff_visible(manifest, content)
}

pub(super) fn contract_requires_antagonist_payoff(manifest: &NovelProjectManifest) -> bool {
    structured_contracts_for_manifest(manifest)
        .any(|contract| !contract.antagonist_pressure.antagonists.is_empty())
        || legacy_contract_requires_antagonist_payoff(manifest)
}

pub(super) fn content_has_antagonist_payoff(
    manifest: &NovelProjectManifest,
    content: &str,
) -> bool {
    structured_contracts_for_manifest(manifest).any(|contract| {
        contract
            .antagonist_pressure
            .antagonists
            .iter()
            .any(|entry| {
                let name_visible =
                    !entry.name.trim().is_empty() && content.contains(entry.name.trim());
                let target_visible = contract_target_visible_in_content(
                    content,
                    &[entry.defeat_condition.as_str(), entry.current_move.as_str()],
                );
                name_visible && (target_visible || contains_closure_signal(content))
            })
    }) || legacy_contract_antagonist_payoff_visible(manifest, content)
}

fn legacy_contract_requires_relationship_payoff(manifest: &NovelProjectManifest) -> bool {
    let Some(contract) = manifest.contract.as_ref() else {
        return false;
    };
    let text = format!("{}\n{}", contract.premise, contract.outline);
    text.contains("情感")
        || text.contains("爱情")
        || text.contains("抱得美人归")
        || contract
            .characters
            .iter()
            .any(|character| character.contains("女主") || character.contains("恋人"))
}

fn legacy_contract_relationship_payoff_visible(
    manifest: &NovelProjectManifest,
    content: &str,
) -> bool {
    let Some(contract) = manifest.contract.as_ref() else {
        return false;
    };
    let relationship_names = contract
        .characters
        .iter()
        .filter(|character| character.contains("女主") || character.contains("恋人"))
        .filter_map(|character| legacy_contract_character_name(character))
        .collect::<Vec<_>>();
    !relationship_names.is_empty()
        && relationship_names
            .iter()
            .any(|name| content.contains(name.as_str()))
        && (contains_closure_signal(content)
            || ["相守", "承诺", "选择", "真心", "并肩", "告白", "婚", "归来"]
                .iter()
                .any(|term| content.contains(term)))
}

fn legacy_contract_requires_antagonist_payoff(manifest: &NovelProjectManifest) -> bool {
    let Some(contract) = manifest.contract.as_ref() else {
        return false;
    };
    let text = format!("{}\n{}", contract.premise, contract.outline);
    text.contains("反派")
        || text.contains("坏人")
        || text.contains("打败")
        || contract
            .characters
            .iter()
            .any(|character| character.contains("反派") || character.contains("对手"))
}

fn legacy_contract_antagonist_payoff_visible(
    manifest: &NovelProjectManifest,
    content: &str,
) -> bool {
    let Some(contract) = manifest.contract.as_ref() else {
        return false;
    };
    let antagonist_names = contract
        .characters
        .iter()
        .filter(|character| character.contains("反派") || character.contains("对手"))
        .filter_map(|character| legacy_contract_character_name(character))
        .collect::<Vec<_>>();
    !antagonist_names.is_empty()
        && antagonist_names
            .iter()
            .any(|name| content.contains(name.as_str()))
        && (contains_closure_signal(content)
            || ["打败", "击败", "伏法", "审判", "失败", "瓦解", "认输"]
                .iter()
                .any(|term| content.contains(term)))
}

fn legacy_contract_character_name(value: &str) -> Option<String> {
    let raw = value
        .split_once("name:")
        .map(|(_, tail)| tail)
        .or_else(|| value.split_once("name：").map(|(_, tail)| tail))
        .unwrap_or(value)
        .split([';', '；', ',', '，'])
        .next()
        .unwrap_or(value)
        .trim();
    let name = raw
        .chars()
        .take_while(|ch| is_cjk_unified(*ch) || ch.is_ascii_alphabetic())
        .collect::<String>();
    (!name.trim().is_empty()).then_some(name)
}

fn structured_contracts_for_manifest(
    manifest: &NovelProjectManifest,
) -> impl Iterator<Item = &NovelContractV2> {
    manifest
        .contract
        .as_ref()
        .map(|contract| &contract.structured_contract_v2)
        .into_iter()
        .chain(std::iter::once(&manifest.structured_contract_v2))
        .chain(
            manifest
                .story_bible
                .as_ref()
                .map(|bible| &bible.structured_contract_v2),
        )
}

fn contract_target_visible_in_content(content: &str, targets: &[&str]) -> bool {
    targets
        .iter()
        .flat_map(|target| stable_contract_terms(target))
        .any(|term| content.contains(term.as_str()))
}

fn stable_contract_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut run = Vec::new();
    for ch in value.chars() {
        if is_cjk_unified(ch) {
            run.push(ch);
            continue;
        }
        collect_stable_contract_terms_from_run(&mut terms, &run);
        run.clear();
    }
    collect_stable_contract_terms_from_run(&mut terms, &run);
    terms.sort();
    terms.dedup();
    terms
}

fn collect_stable_contract_terms_from_run(out: &mut Vec<String>, run: &[char]) {
    if run.len() < 2 {
        return;
    }
    if run.len() <= 8 {
        out.push(run.iter().collect());
    }
    for window in 2..=4 {
        if run.len() < window {
            continue;
        }
        for start in 0..=run.len() - window {
            out.push(run[start..start + window].iter().collect());
        }
    }
}

pub(super) fn tail_reopens_after_closure(language: &str, content: &str) -> bool {
    let tail = text_tail_chars(content, 1200);
    contains_closure_signal(&tail)
        && (contains_new_open_hook_signal(&tail)
            || ending_looks_like_cliffhanger(&tail)
            || text_has_midstory_tail_signal(language, &tail))
}

pub(super) fn text_has_midstory_tail_signal(language: &str, text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    if is_chinese_language(language) || text.chars().any(is_cjk_unified) {
        [
            "新阶段",
            "新纪元",
            "新的变量",
            "新的危机",
            "新的敌人",
            "新的主线",
            "新的博弈",
            "新的演化",
            "继续深入",
            "继续演化",
            "继续博弈",
            "刚刚开始",
            "还没有结束",
            "下一章",
            "入口",
        ]
        .iter()
        .any(|term| text.contains(term))
    } else {
        [
            "new phase",
            "new era",
            "new threat",
            "new enemy",
            "only begun",
            "not over",
            "next chapter",
        ]
        .iter()
        .any(|term| lowered.contains(term))
    }
}

pub(super) fn project_title_registry_warnings(manifest: &NovelProjectManifest) -> Vec<String> {
    let mut warnings = Vec::new();
    for (index, chapter) in manifest.chapters.iter().enumerate() {
        if !title_has_enough_signal(&chapter.title) {
            continue;
        }
        for other in manifest.chapters.iter().skip(index + 1) {
            if !title_has_enough_signal(&other.title) {
                continue;
            }
            let left = normalized_title_key(&chapter.title);
            let right = normalized_title_key(&other.title);
            if left.is_empty() || right.is_empty() {
                continue;
            }
            let score = title_similarity(
                &normalize_project_lookup_key(&chapter.title),
                &normalize_project_lookup_key(&other.title),
            );
            if left == right || score >= 0.82 {
                warnings.push(format!(
                    "Chapter titles may be too similar: {} '{}' and {} '{}'",
                    chapter.number, chapter.title, other.number, other.title
                ));
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

pub(super) fn latest_truth_validation_issues(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> Vec<String> {
    manifest
        .truth_validations
        .iter()
        .rev()
        .find(|record| record.chapter_number == chapter_number)
        .map(|record| record.issues.clone())
        .unwrap_or_default()
}

pub(super) fn chapter_would_reach_target(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> bool {
    let Some(target) = manifest.target_units.filter(|target| *target > 0) else {
        return false;
    };
    let approved_units: usize = manifest
        .chapters
        .iter()
        .filter(|item| item.number != chapter.number)
        .filter(|item| chapter_is_approved(item))
        .map(|item| item.unit_count)
        .sum();
    approved_units + chapter.unit_count >= target
}

pub(super) fn chapter_is_completion_mode_candidate(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> bool {
    if planned_final_chapter(manifest).is_some_and(|final_chapter| chapter.number < final_chapter) {
        return false;
    }
    chapter_would_reach_target(manifest, chapter)
}

fn planned_final_chapter(manifest: &NovelProjectManifest) -> Option<usize> {
    manifest
        .volumes
        .iter()
        .filter_map(|volume| volume.end_chapter)
        .max()
        .or_else(|| manifest.chapter_plans.iter().map(|plan| plan.number).max())
}

pub(super) fn normalized_title_key(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if is_cjk_unified(ch) {
                Some(ch)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn title_has_enough_signal(value: &str) -> bool {
    let key = normalized_title_key(value);
    if key.chars().count() < 3 {
        return false;
    }
    let lowered = value.to_ascii_lowercase();
    ![
        "第1章", "第2章", "第3章", "chapter1", "chapter2", "chapter3", "untitled",
    ]
    .iter()
    .any(|term| key == normalized_title_key(term) || lowered == *term)
}

pub(super) fn normalize_duplicate_probe_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() || is_cjk_unified(ch) {
                Some(ch)
            } else if ch.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn text_shingle_similarity(left: &str, right: &str) -> f64 {
    let left_shingles = text_shingles(left);
    let right_shingles = text_shingles(right);
    if left_shingles.is_empty() || right_shingles.is_empty() {
        return 0.0;
    }
    let intersection = left_shingles.intersection(&right_shingles).count();
    let union = left_shingles.union(&right_shingles).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

pub(super) fn text_shingles(value: &str) -> BTreeSet<String> {
    let chars = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    let window = if chars.iter().any(|ch| is_cjk_unified(*ch)) {
        24
    } else {
        8
    };
    let step = if window >= 24 { 12 } else { 4 };
    let mut shingles = BTreeSet::new();
    if chars.len() < window {
        if chars.len() >= window / 2 {
            shingles.insert(chars.iter().collect());
        }
        return shingles;
    }
    let mut index = 0usize;
    while index + window <= chars.len() {
        shingles.insert(chars[index..index + window].iter().collect());
        index += step;
    }
    shingles
}

pub(super) fn contains_state_change_signal(text: &str, language: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let terms = [
        "决定",
        "选择",
        "发现",
        "揭示",
        "确认",
        "夺回",
        "失去",
        "得到",
        "获得",
        "拿到",
        "夺取",
        "击败",
        "斩杀",
        "救下",
        "承认",
        "离开",
        "进入",
        "抵达",
        "现身",
        "建立",
        "引发",
        "触发",
        "领悟",
        "突破",
        "掌控",
        "苏醒",
        "解开",
        "关闭",
        "闭合",
        "签下",
        "交出",
        "牺牲",
        "背叛",
        "完成",
        "改变",
        "新局面",
        "不可逆",
        "状态变化",
        "代价",
        "后果",
        "discovers",
        "chooses",
        "reveals",
        "confirms",
        "wins",
        "loses",
        "leaves",
        "arrives",
        "changes",
        "irreversible",
        "state change",
        "consequence",
        "cost",
        "pays off",
    ];
    terms
        .iter()
        .any(|term| text.contains(term) || lowered.contains(term))
        || (!is_chinese_language(language) && lowered.contains("decides"))
}

fn contains_specific_state_change_signal(text: &str, language: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let compact = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let concrete_terms = [
        "得到",
        "获得",
        "失去",
        "击败",
        "斩杀",
        "救下",
        "交出",
        "签下",
        "夺回",
        "夺取",
        "拿到",
        "离开",
        "进入",
        "抵达",
        "现身",
        "建立",
        "引发",
        "触发",
        "发现",
        "揭示",
        "确认",
        "暴露",
        "领悟",
        "突破",
        "掌控",
        "解开",
        "关闭",
        "闭合",
        "背叛",
        "牺牲",
        "封印",
        "觉醒",
        "苏醒",
        "反噬",
        "受伤",
        "结盟",
        "决裂",
        "晋升",
        "降级",
        "死亡",
        "逃出",
        "追杀",
        "通关",
        "公开",
        "证据",
        "玉简",
        "契约",
        "令牌",
        "钥匙",
        "账本",
        "法门",
        "传承",
        "裂痕",
        "印记",
        "discovers",
        "obtains",
        "loses",
        "defeats",
        "rescues",
        "leaves",
        "arrives",
        "exposes",
        "betrays",
        "sacrifices",
        "alliance",
        "breaks",
    ];
    let concrete_hits = concrete_terms
        .iter()
        .filter(|term| compact.contains(**term) || lowered.contains(**term))
        .count();
    if concrete_hits == 0 {
        return false;
    }
    let generic_only = [
        "命运已经彻底改变",
        "旅程才刚刚开始",
        "道路上不断前行",
        "书写属于自己的传奇",
        "改变他的一生",
        "新的道路",
        "前所未有的考验",
    ];
    if concrete_hits == 1 && generic_only.iter().any(|term| compact.contains(*term)) {
        return false;
    }
    !is_chinese_language(language) || compact.chars().count() >= 20
}

pub(super) fn contains_new_open_hook_signal(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "新伏笔",
        "新的伏笔",
        "新悬念",
        "新的悬念",
        "未解",
        "尚未",
        "仍未",
        "待解决",
        "待回收",
        "悬而未决",
        "new hook",
        "new mystery",
        "unresolved",
        "pending",
        "not yet resolved",
    ]
    .iter()
    .any(|term| text.contains(term) || lowered.contains(term))
}

pub(super) fn contains_closure_signal(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "结局",
        "终局",
        "尾声",
        "收束",
        "解决",
        "兑现",
        "揭示",
        "打败",
        "完成",
        "落定",
        "和解",
        "归于",
        "尘埃落定",
        "已解决",
        "已兑现",
        "已收束",
        "finale",
        "epilogue",
        "resolved",
        "paid off",
        "closed",
        "settled",
        "defeated",
        "fulfilled",
    ]
    .iter()
    .any(|term| text.contains(term) || lowered.contains(term))
}

pub(super) fn ending_looks_like_cliffhanger(content: &str) -> bool {
    let tail = text_tail_chars(content, 500);
    let lowered = tail.to_ascii_lowercase();
    [
        "未完待续",
        "下一章",
        "新的敌人",
        "新的危机",
        "刚刚开始",
        "还没有结束",
        "真正的",
        "to be continued",
        "next chapter",
        "had only begun",
        "not over yet",
    ]
    .iter()
    .any(|term| tail.contains(term) || lowered.contains(term))
}

pub(super) fn text_tail_chars(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

pub(super) fn chinese_title_language_issues(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Some("title is empty".to_string());
    }
    let lowered = trimmed.to_ascii_lowercase();
    if !trimmed.chars().any(is_cjk_unified) {
        return Some("title contains no Chinese characters".to_string());
    }
    let control_markers = [
        "chapter",
        "contract",
        "workflow",
        "continuity",
        "entities",
        "rules",
        "project",
        "artifact",
        "title",
    ];
    if control_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return Some("title appears to contain English control/workflow text".to_string());
    }
    let cjk_workflow_markers = [
        "推进", "继承", "承接", "延续", "转折", "收束", "阶段", "变化", "冲突", "目标", "任务",
        "完成", "状态", "线索", "伏笔", "关系", "本章", "章节", "章尾", "落点", "入口", "出口",
        "展开", "段落",
    ];
    if cjk_workflow_markers
        .iter()
        .any(|marker| trimmed == *marker || trimmed.contains(marker))
    {
        return Some("title appears to contain Chinese workflow/control text".to_string());
    }
    let cjk_category_markers = [
        "小说", "故事", "题材", "类型", "玄幻", "奇幻", "科幻", "言情", "悬疑", "推理", "都市",
        "历史", "武侠", "仙侠", "长篇", "短篇",
    ];
    if cjk_category_markers
        .iter()
        .any(|marker| trimmed == *marker || trimmed.ends_with(marker))
    {
        return Some(
            "title appears to be a genre/category label rather than a chapter event".to_string(),
        );
    }
    let core = chinese_chapter_title_core(trimmed);
    let prose_connectors = [
        "随着", "当他", "当她", "当那", "于是", "然而", "但是", "因为", "如果",
    ];
    if prose_connectors
        .iter()
        .any(|marker| core.starts_with(marker))
        || cjk_title_candidate_has_sentence_fragment_edge(&core)
        || cjk_title_core_has_prose_grammar_fragment(&core)
    {
        return Some("title appears to be a clipped prose sentence fragment".to_string());
    }
    let sensory_fragments = [
        "到一阵",
        "到一种",
        "到自己",
        "到耳膜",
        "觉到",
        "感到",
        "看到",
        "听到",
        "想到",
    ];
    if sensory_fragments
        .iter()
        .any(|marker| core.starts_with(marker) || core.contains(marker))
    {
        return Some("title appears to be a clipped prose sensory fragment".to_string());
    }
    let demonstrative_fragments = [
        "时那", "时这", "那股", "那种", "那片", "那道", "那个", "这股", "这种", "这片", "这道",
        "这个",
    ];
    if demonstrative_fragments
        .iter()
        .any(|marker| core.starts_with(marker))
    {
        return Some("title appears to be a clipped prose demonstrative fragment".to_string());
    }
    if title_has_unsupported_latin_surface(trimmed) {
        return Some("title contains Latin letters".to_string());
    }
    None
}

fn title_has_unsupported_latin_surface(title: &str) -> bool {
    let mut current = String::new();
    let mut has_unsupported = false;
    for ch in title.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            if title_ascii_run_is_unsupported(&current) {
                has_unsupported = true;
                break;
            }
            current.clear();
        }
    }
    has_unsupported
}

fn title_ascii_run_is_unsupported(run: &str) -> bool {
    let token = run.trim_matches(|ch: char| ch == '-' || ch == '_');
    if token.is_empty() {
        return false;
    }
    let has_letter = token.chars().any(|ch| ch.is_ascii_alphabetic());
    if !has_letter {
        return false;
    }
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let code_like = has_digit
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && token.chars().filter(|ch| ch.is_ascii_alphabetic()).count() <= 3
        && token.chars().count() <= 8;
    !code_like
}

pub(super) fn chinese_title_control_surface_issue(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Some("title is empty".to_string());
    }
    let lowered = trimmed.to_ascii_lowercase();
    let control_markers = [
        "original user request",
        "delegated task",
        "workflow",
        "continuity",
        "entities",
        "project setup",
        "artifact",
    ];
    let matches = control_markers
        .iter()
        .filter(|marker| lowered.contains(**marker))
        .count();
    (matches >= 2
        || lowered.contains("original user request")
        || lowered.contains("delegated task")
        || lowered.contains("project setup"))
    .then(|| "title appears to contain workflow/control text".to_string())
}

pub(super) fn chapter_heading_issues(chapter: &ChapterRecord, content: &str) -> Vec<String> {
    let matching_headings = content
        .lines()
        .filter(|line| leading_line_looks_like_same_chapter_heading(line.trim(), &chapter.title))
        .count();
    if matching_headings > 1 {
        return vec![format!(
            "chapter body repeats chapter heading {matching_headings} times near: {}",
            chapter.title
        )];
    }
    Vec::new()
}

pub(super) fn language_script_issues(
    manifest: &NovelProjectManifest,
    content: &str,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    if let Some(issue) = chinese_body_language_contract_issue(content) {
        issues.push(issue);
    }
    if content.contains('\u{fffd}') {
        issues.push("Chinese-language chapter contains replacement character U+FFFD".to_string());
    }
    if let Some(fragment) = embedded_lowercase_latin_fragment_in_cjk(content) {
        issues.push(format!(
            "Chinese-language chapter contains embedded Latin fragment inside CJK text: {fragment}"
        ));
    }
    if content.contains("\\n") {
        issues.push(
            "Chinese-language chapter contains literal escaped newline marker: \\n".to_string(),
        );
    } else if let Some(fragment) = literal_newline_escape_residue_in_cjk(content) {
        issues.push(format!(
            "Chinese-language chapter contains likely escaped newline residue: {fragment}"
        ));
    }
    let unexpected = content
        .chars()
        .filter(|ch| is_unexpected_script_for_chinese(*ch))
        .take(8)
        .collect::<Vec<_>>();
    if !unexpected.is_empty() {
        issues.push(format!(
            "Chinese-language chapter contains unexpected non-CJK script fragments: {}",
            unexpected.into_iter().collect::<String>()
        ));
    }
    issues
}

pub(super) fn chinese_body_language_contract_issue(content: &str) -> Option<String> {
    let mut cjk = 0usize;
    let mut latin = 0usize;
    let mut latin_words = 0usize;
    let mut in_latin_word = false;
    for ch in content.chars() {
        if is_cjk_unified(ch) {
            cjk += 1;
            in_latin_word = false;
        } else if ch.is_ascii_alphabetic() {
            latin += 1;
            if !in_latin_word {
                latin_words += 1;
                in_latin_word = true;
            }
        } else {
            in_latin_word = false;
        }
    }
    if cjk == 0 {
        return Some("Chinese-language chapter body contains no Chinese prose".to_string());
    }
    if latin_words >= 12 && latin.saturating_mul(2) > cjk {
        return Some(format!(
            "Chinese-language chapter body contains too much English prose: {latin_words} Latin word runs, {latin} Latin letters, {cjk} Chinese characters"
        ));
    }
    None
}

pub(super) fn cjk_layout_issues(manifest: &NovelProjectManifest, content: &str) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    let mut issues = Vec::new();
    for line in content.lines() {
        if line_allows_cjk_inner_spaces(line) {
            continue;
        }
        let chars = line.chars().collect::<Vec<_>>();
        for window in chars.windows(3) {
            if !(is_cjk_unified(window[0]) && window[1] == ' ' && is_cjk_unified(window[2])) {
                continue;
            }
            issues.push(format!(
                "Chinese-language chapter contains unexpected whitespace inside CJK phrase: {} {}",
                window[0], window[2]
            ));
            return issues;
        }
    }
    issues
}

pub(super) fn cjk_malformed_structural_phrase_issues(content: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let compact = line
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        if let Some(fragment) = cjk_dangling_connector_fragment(&compact) {
            issues.push(format!(
                "Chinese chapter body contains dangling connector phrase: {fragment}"
            ));
        }
        if let Some(fragment) = cjk_orphan_particle_after_boundary_fragment(&compact) {
            issues.push(format!(
                "Chinese chapter body contains orphan particle phrase: {fragment}"
            ));
        }
        if let Some(fragment) = cjk_connector_particle_fragment(&compact) {
            issues.push(format!(
                "Chinese chapter body contains malformed connector-particle phrase: {fragment}"
            ));
        }
        if let Some(fragment) = cjk_lexical_glue_fragment(&compact) {
            issues.push(format!(
                "Chinese chapter body contains malformed lexical glue phrase: {fragment}"
            ));
        }
        if issues.len() >= 3 {
            break;
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

fn cjk_dangling_connector_fragment(line: &str) -> Option<String> {
    ["以及，", "以及。", "以及；", "以及？", "以及！"]
        .iter()
        .find_map(|needle| {
            line.find(needle).and_then(|index| {
                if !cjk_connector_is_dangling_at(line, index) {
                    return None;
                }
                Some(preview_chars(&line[index..], 24))
            })
        })
}

fn cjk_connector_is_dangling_at(line: &str, byte_index: usize) -> bool {
    let prefix = &line[..byte_index];
    let Some(prev) = prefix.chars().rev().find(|ch| !ch.is_whitespace()) else {
        return true;
    };
    matches!(
        prev,
        '。' | '；' | ';' | '！' | '？' | '!' | '?' | '：' | ':' | '（' | '(' | '“' | '‘' | '《'
    )
}

fn cjk_orphan_particle_after_boundary_fragment(line: &str) -> Option<String> {
    let chars = line.chars().collect::<Vec<_>>();
    for (index, window) in chars.windows(3).enumerate() {
        if !cjk_sentence_punctuation(window[0]) || window[1] != '的' {
            continue;
        }
        if window[2] == '确' || !is_cjk_unified(window[2]) {
            continue;
        }
        return Some(cjk_fragment_window(&chars, index, 8));
    }
    None
}

fn cjk_connector_particle_fragment(line: &str) -> Option<String> {
    for needle in ["还是的", "或者的", "以及的"] {
        if let Some(index) = line.find(needle) {
            return Some(preview_chars(&line[index..], 24));
        }
    }
    let chars = line.chars().collect::<Vec<_>>();
    for (index, window) in chars.windows(2).enumerate() {
        if !matches!((window[0], window[1]), ('和' | '与' | '及', '的')) {
            continue;
        }
        let previous = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        if previous.is_none_or(|ch| !is_cjk_unified(ch) || cjk_sentence_punctuation(ch)) {
            return Some(cjk_fragment_window(&chars, index, 8));
        }
    }
    None
}

fn cjk_lexical_glue_fragment(line: &str) -> Option<String> {
    for needle in ["材质地", "香烟雾"] {
        if let Some(index) = line.find(needle) {
            return Some(preview_chars(&line[index..], 24));
        }
    }
    None
}

fn cjk_sentence_punctuation(ch: char) -> bool {
    matches!(ch, '，' | '。' | '；' | '！' | '？' | '、' | ':' | '：')
}

fn cjk_fragment_window(chars: &[char], start: usize, max_chars: usize) -> String {
    let left = start.saturating_sub(4);
    let right = (start + max_chars).min(chars.len());
    chars[left..right].iter().collect()
}

pub(super) fn line_allows_cjk_inner_spaces(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    if markdown_heading_text(trimmed).is_some() {
        return true;
    }
    line_looks_like_chapter_heading(trimmed)
}

pub(super) fn line_looks_like_chapter_heading(line: &str) -> bool {
    let trimmed = line.trim().trim_matches(['"', '\'', '“', '”']);
    if trimmed.chars().count() > 80 {
        return false;
    }
    if !trimmed.starts_with('第') || !trimmed.contains('章') {
        return false;
    }
    let Some((chapter_index, chapter_marker)) = trimmed.char_indices().find(|(_, ch)| *ch == '章')
    else {
        return false;
    };
    let end = chapter_index + chapter_marker.len_utf8();
    trimmed[..end].chars().count() <= 8
}

pub(super) fn embedded_lowercase_latin_fragment_in_cjk(content: &str) -> Option<String> {
    let chars = content.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < chars.len() {
        if !chars[index].is_ascii_alphabetic() {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_alphabetic() {
            index += 1;
        }
        let end = index;
        let run = chars[start..end].iter().collect::<String>();
        let prev = start.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(end).copied();
        if prev.is_some_and(is_cjk_unified)
            && next.is_some_and(is_cjk_unified)
            && run.chars().any(|ch| ch.is_ascii_lowercase())
            && !looks_like_preserved_ascii_acronym(&run)
        {
            return Some(run);
        }
    }
    None
}

pub(super) fn literal_newline_escape_residue_in_cjk(content: &str) -> Option<String> {
    let chars = content.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != 'n' {
            continue;
        }
        let prev = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(index + 1).copied();
        if prev.is_some_and(is_chinese_sentence_punctuation)
            && next.is_none_or(|next| next.is_whitespace())
        {
            return Some(format!("{}n", prev.unwrap_or_default()));
        }
    }
    None
}

pub(super) fn is_chinese_sentence_punctuation(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '；' | '：')
}

pub(super) fn is_unexpected_script_for_chinese(ch: char) -> bool {
    if ch.is_ascii() || is_cjk_or_chinese_text_compatible(ch) {
        return false;
    }
    ch.is_alphabetic()
}

pub(super) fn is_cjk_or_chinese_text_compatible(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0x20000..=0x2ebef
            | 0x3000..=0x303f
            | 0xff00..=0xffef
    )
}

pub(super) fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

pub(super) fn looks_like_preserved_ascii_acronym(run: &str) -> bool {
    let len = run.chars().count();
    len >= 2 && len <= 6 && run.chars().all(|ch| ch.is_ascii_uppercase())
}

pub(super) fn stable_manifest_anchor_present(
    manifest: &NovelProjectManifest,
    content: &str,
) -> bool {
    let Some(contract) = &manifest.contract else {
        return true;
    };
    let mut anchors = manifest_character_anchors(manifest);
    anchors.extend(
        contract
            .world_rules
            .iter()
            .filter_map(|value| stable_anchor_token(value))
            .map(ToString::to_string),
    );
    if is_chinese_language(&manifest.language) {
        anchors.retain(|anchor| {
            stable_character_anchor_name(anchor).is_some() || anchor.chars().count() <= 12
        });
    } else {
        anchors.retain(|anchor| {
            let trimmed = anchor.trim();
            !trimmed.is_empty() && trimmed.chars().count() <= 80
        });
    }
    anchors.sort();
    anchors.dedup();
    if anchors.is_empty() {
        return !contract.premise.trim().is_empty() || !contract.outline.trim().is_empty();
    }
    anchors
        .iter()
        .any(|anchor| content.contains(anchor.as_str()))
}

#[cfg(test)]
pub(super) fn stable_contract_anchor_present(contract: &StoryContract, content: &str) -> bool {
    let anchors = contract
        .characters
        .iter()
        .chain(contract.world_rules.iter())
        .filter_map(|value| stable_anchor_token(value))
        .collect::<Vec<_>>();
    if anchors.is_empty() {
        return !contract.premise.trim().is_empty() || !contract.outline.trim().is_empty();
    }
    anchors.iter().any(|anchor| content.contains(anchor))
}

#[cfg(test)]
mod local_quality_tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn repeated_cjk_paragraph_opening_is_detected() {
        let content = "少年站在雨夜旧桥下听见远处钟声之后又看见灵火浮起。\n少年站在雨夜旧桥下听见远处钟声之后又看见旧城裂开。";

        let issue = repeated_cjk_paragraph_opening(content);

        assert!(issue.is_some());
    }

    #[test]
    fn generic_life_change_is_not_specific_progression() {
        let text = "他的命运已经彻底改变，新的道路将引领他继续前行。";

        assert!(!contains_specific_state_change_signal(text, "zh-CN"));
    }

    #[test]
    fn concrete_object_and_consequence_count_as_progression() {
        let text = "主角获得玉简并暴露身份，宗门因此下达追杀令。";

        assert!(contains_specific_state_change_signal(text, "zh-CN"));
    }

    #[test]
    fn concrete_event_verbs_count_as_state_change() {
        let text = "辛曜白斩杀妖兽并获得第一滴剑血，剑血初凝引发天域异动，守护者随之现身。";

        assert!(contains_state_change_signal(text, "zh-CN"));
        assert!(contains_specific_state_change_signal(text, "zh-CN"));
    }

    #[test]
    fn progression_gate_reads_chapter_tail_for_durable_change() {
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "灵脉枯竭".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "异界修仙".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            delivery_advisory_windows: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: None,
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let chapter = ChapterRecord {
            number: 2,
            title: "塔火夜鸣".to_string(),
            path: "chapters/0002.md".to_string(),
            summary: "晏照珩救下老赵叔，天枢塔警戒被触发，枯毒反噬加深。".to_string(),
            unit_count: 2_900,
            status: "draft".to_string(),
            key_facts: vec!["晏照珩救下老赵叔，天枢塔警戒被触发。".to_string()],
            continuity_updates: vec!["枯毒反噬加深，下一章必须处理追捕与伤势。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            volume_id: "volume-0001".to_string(),
            volume_title: "枯荣初醒".to_string(),
        };
        let mut content = "晏照珩在青石城里反复观察天枢塔，听见街巷间传来低低议论。".repeat(160);
        content.push_str(
            "夜色最深时，老赵叔被祭灵使拖向塔基。晏照珩斩断灵火锁链，把人从阵眼前救下；塔顶铜铃随即炸响，天枢塔警戒被触发，枯毒也在他掌心反噬加深。",
        );

        let issues = chapter_progression_contract_issues(&manifest, &chapter, &content);

        assert!(
            !issues.iter().any(|issue| issue.contains("durable state")),
            "tail state change should satisfy progression gate: {issues:?}"
        );
        assert!(
            !issues.iter().any(|issue| issue.contains("too generic")),
            "tail concrete consequence should satisfy specificity gate: {issues:?}"
        );
    }

    #[test]
    fn repeated_named_concept_without_progression_is_detected() {
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "局中法则".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "玄幻".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            delivery_advisory_windows: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: Some(StoryContract {
                premise: "唐曜珩和孟岚隅追查局中法则。".to_string(),
                themes: vec!["规则背后的代价".to_string()],
                characters: vec![
                    "name: 唐曜珩; role: 主角; desire: 破解局中法则".to_string(),
                    "name: 孟岚隅; role: 同伴; desire: 查清山巅异象".to_string(),
                ],
                world_rules: vec!["局中法则会映照修行者的选择。".to_string()],
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: "主角逐步看清局中法则的代价。".to_string(),
                structured_contract_v2: NovelContractV2::default(),
                authority_contract: None,
                updated_at: Utc::now().to_rfc3339(),
            }),
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let mut content = String::new();
        for _ in 0..120 {
            content.push_str("唐曜珩解释‘局’的法则并非力量，而是规则本质的映照。");
        }
        content.push_str("孟岚隅站在山巅听完这些话，仍然只是点头。");

        let issue = overused_cjk_named_concept(&manifest, &content);

        assert!(
            issue.is_some(),
            "repeated concept exposition should be caught"
        );
    }

    #[test]
    fn malformed_structural_phrases_are_detected() {
        let content = "那是赵铁柱的车吗？还是的人？\n顾栖川确认主要对手赵铁柱。以及，古玉持续发热。\n洛栖舟的深不可测，的神秘，都会汇聚。";
        let issues = cjk_malformed_structural_phrase_issues(content);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("connector-particle")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("dangling connector")),
            "{issues:?}"
        );
        assert!(
            issues.iter().any(|issue| issue.contains("orphan particle")),
            "{issues:?}"
        );
    }

    #[test]
    fn malformed_structural_phrases_allow_in_sentence_connector_lists() {
        let content = "“你的寿元，以及，梁澈川的命。”苏婉淡淡地说道。";
        let issues = cjk_malformed_structural_phrase_issues(content);

        assert!(
            issues
                .iter()
                .all(|issue| !issue.contains("dangling connector")),
            "{issues:?}"
        );
    }

    #[test]
    fn malformed_anchor_phrase_allows_one_person_usage() {
        let content = "这场博弈不再是晏照珩一个人的独角戏，而是多方势力的绞肉机。";

        assert_eq!(malformed_anchor_phrase(content, "晏照珩"), None);
    }

    #[test]
    fn malformed_phrase_issues_detect_action_object_part_boundary() {
        let content = "晏照珩缓缓收剑尖滴落淡金色血珠。他握紧手中的枯荣剑身上的黑色纹路亮起。";
        let repaired =
            crate::tool::writing::surface_sanitizer::repair_cjk_action_object_part_boundaries(
                content,
            );
        let issues = cjk_malformed_phrase_issues(&repaired);

        assert!(
            repaired.contains("收剑，剑尖滴落")
                && repaired.contains("枯荣剑，剑身上的黑色纹路亮起"),
            "{repaired}"
        );
        assert!(
            issues
                .iter()
                .all(|issue| !issue.contains("action-object-part boundary")),
            "{issues:?}; repaired={repaired}"
        );
    }

    #[test]
    fn malformed_structural_phrases_do_not_flag_normal_adjectives() {
        let content =
            "古玉光芒微闪，一股柔和的力量在肩头化开。\n她用温和的语气解释规则，顾栖川没有打断。\n洛栖舟声音温和，却透着一股不容置疑的威严。";
        let issues = cjk_malformed_structural_phrase_issues(content);

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn contract_premise_must_not_leak_as_prose_clause() {
        let premise = "豪门千金被迫与底层程序员结婚，身份错位中重审爱情与婚姻边界。";
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "旧证入场".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "都市言情".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            delivery_advisory_windows: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: Some(StoryContract {
                premise: premise.to_string(),
                themes: vec!["平等关系".to_string()],
                characters: vec!["name: 梁栖安; role: 主角; desire: 夺回选择权".to_string()],
                world_rules: Vec::new(),
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: "婚礼入口暴露交易真相，终局公开证据并重建关系。".to_string(),
                structured_contract_v2: NovelContractV2::default(),
                authority_contract: None,
                updated_at: Utc::now().to_rfc3339(),
            }),
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let content = "梁栖安突然听见规则在耳畔低语：豪门千金被迫与底层程序员结婚，身份错位中重审爱情与婚姻边界。她攥紧戒指，终于看向台下的司砚晚。";

        let report = contract_governance_leakage_report(&manifest, content);

        assert!(
            report
                .warnings
                .iter()
                .any(|issue| issue.contains("contract/governance clause")),
            "{report:?}"
        );
        assert!(
            report.blocking.is_empty(),
            "dramatized premise prose should warn instead of blocking: {report:?}"
        );

        let dramatized =
            "梁栖安站在婚礼后台，听见母亲压低声音谈判彩礼和股权。她没有再看那枚戒指，只把司砚晚发来的旧邮件一点点点开，第一次意识到这场婚姻不是救命绳，而是另一张网。";
        let dramatized_report = contract_governance_leakage_report(&manifest, dramatized);

        assert!(
            dramatized_report.blocking.is_empty() && dramatized_report.warnings.is_empty(),
            "dramatized premise should not be treated as copied contract text: {dramatized_report:?}"
        );

        let outline_meta =
            "本章大纲：婚礼入口暴露交易真相，终局公开证据并重建关系。梁栖安随后走上台阶。";
        let outline_meta_report = contract_governance_leakage_report(&manifest, outline_meta);
        assert!(
            outline_meta_report
                .blocking
                .iter()
                .any(|issue| issue.contains("contract/governance clause")),
            "outline/meta sentence should be treated as contract leakage: {outline_meta_report:?}"
        );

        let theme_meta = "他想起项目约束里提到的平等关系，决定按照这个主题继续行动。";
        let theme_meta_report = contract_governance_leakage_report(&manifest, theme_meta);
        assert!(
            theme_meta_report
                .blocking
                .iter()
                .any(|issue| issue.contains("contract/governance clause")),
            "explicit project-constraint theme leakage should block: {theme_meta_report:?}"
        );
    }

    #[test]
    fn contract_premise_similarity_warns_instead_of_blocking_narrative_prose() {
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "剑镇九州".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "异界修仙".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: None,
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: vec![ChapterRecord {
                number: 1,
                title: "握住剑柄".to_string(),
                path: "chapters/0001.md".to_string(),
                summary: "姜闻遥在黑风林击杀铁背獒，断剑吸收妖血并亮起符文，他意识到自己获得了斩开底层枷锁的入口。".to_string(),
                unit_count: 2_800,
                status: "draft".to_string(),
                key_facts: vec![
                    "姜闻遥击杀铁背獒并保住白岚隅。".to_string(),
                    "断剑吸收妖血后亮起第一枚符文。".to_string(),
                ],
                continuity_updates: vec![
                    "姜闻遥确认断剑能靠妖血复苏，下一章将尝试修补断剑。".to_string(),
                ],
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
                volume_id: "volume-0001".to_string(),
                volume_title: "入局见证".to_string(),
            }],
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            delivery_advisory_windows: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: Some(StoryContract {
                premise: "在这个灵气逐渐枯竭、宗门垄断资源的末法前夜，没有灵根的姜闻遥被世界遗忘，必须用断剑斩开底层枷锁。".to_string(),
                themes: vec!["底层逆袭".to_string()],
                characters: vec!["name: 姜闻遥; role: 主角; desire: 修复断剑".to_string()],
                world_rules: Vec::new(),
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: "姜闻遥在黑风林杀死铁背獒，发现断剑复苏，踏入修仙阶层争夺。".to_string(),
                structured_contract_v2: NovelContractV2::default(),
                authority_contract: None,
                updated_at: Utc::now().to_rfc3339(),
            }),
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let content = "在这个灵气逐渐枯竭、宗门垄断资源的末法前夜，没有灵根的姜闻遥，就像是一粒被风扬起的尘埃，注定要落入底层。黑风林里，铁背獒扑向白岚隅时，他没有再退，双手握住断剑刺进妖兽颈侧。妖血沿着锈迹渗入剑身，第一枚暗红符文在月色里亮起。姜闻遥扶着白岚隅站稳，终于明白这把废剑不是遗物，而是他斩开底层枷锁的入口。";
        let chapter = manifest.chapters[0].clone();
        let gate = chapter_quality_gate(&manifest, &chapter, content, &[]);

        assert!(
            gate.passed,
            "narrative premise similarity should not force body revision: {gate:?}"
        );
        assert!(
            gate.warnings
                .iter()
                .any(|issue| issue.contains("contract/governance clause")),
            "premise-like narration should still be visible as a warning: {gate:?}"
        );
    }
}
