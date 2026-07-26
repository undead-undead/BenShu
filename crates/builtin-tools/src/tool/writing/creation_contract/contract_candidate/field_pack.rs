use super::*;

pub(crate) fn novel_creation_contract_from_field_pack(
    draft: &SessionCreationDraftState,
    text: &str,
) -> Option<NovelCreationContract> {
    if draft.artifact_kind != "fiction" {
        return None;
    }
    let normalized_field_pack = normalize_generated_contract_field_pack_lines(text);
    let text = normalized_field_pack.as_str();
    if !text_looks_like_contract_field_pack(text) {
        return None;
    }

    let mut contract = super::strong_novel_contract_from_creation_draft(draft);
    let mut changed = false;

    if let Some(title) = field_pack_string(text, &["书名", "标题", "Title"]) {
        contract.title.canonical_title = title;
        contract.title.source = TitleSource::LlmContract;
        changed = true;
    }
    if let Some(rationale) = field_pack_string(
        text,
        &["命名理由", "书名理由", "标题理由", "Title Rationale"],
    ) {
        contract.title.rationale = rationale;
        changed = true;
    }
    if let Some(candidates) = field_pack_list(text, &["书名候选", "标题候选", "title_candidates"])
    {
        contract.title.candidates = candidates;
        changed = true;
    }
    if contract.title.candidates.is_empty()
        && !value_missing(&contract.title.canonical_title)
        && !value_missing(&contract.title.rationale)
    {
        contract.title.candidates = vec![contract.title.canonical_title.clone()];
    }
    if let Some(language) = field_pack_string(text, &["语言", "language"]) {
        contract.language = language;
        changed = true;
    }
    if let Some(genre) = field_pack_string(text, &["题材", "类型", "genre"]) {
        contract.genre = genre;
        changed = true;
    }
    if let Some(brief) = field_pack_string(text, &["简述", "创作简述", "brief"]) {
        contract.brief = brief;
        changed = true;
    }
    if let Some(target) = field_pack_usize(
        text,
        &["总字数", "目标字数", "总目标字数", "target_units"],
        requested_total_unit_target,
    ) {
        contract.target_units = Some(target);
        changed = true;
    }
    if let Some(target) = field_pack_usize(
        text,
        &[
            "每章档位",
            "每章字数",
            "每章目标字数",
            "chapter_unit_target",
        ],
        requested_chapter_unit_target,
    ) {
        contract.chapter_unit_target = Some(target);
        changed = true;
    }
    if let Some(target) = field_pack_usize(
        text,
        &["每轮最多章节", "max_chapters_per_turn"],
        |value| requested_max_chapters_per_turn(value),
    ) {
        contract.max_chapters_per_turn = Some(target);
        changed = true;
    }
    if let Some(premise) = field_pack_string(text, &["故事前提", "前提", "premise"]) {
        contract.premise = premise;
        changed = true;
    }
    if value_missing(&contract.premise) {
        if let Some(premise) = field_pack_string(text, &["核心矛盾", "核心冲突"]) {
            contract.premise = premise;
            changed = true;
        }
    }
    if let Some(ending) = field_pack_string(
        text,
        &["终局方向", "结局方向", "结尾承诺", "ending_direction"],
    ) {
        contract.ending.desired_resolution = ending;
        changed = true;
    }
    if let Some(final_state) = field_pack_string(text, &["终局状态", "最终状态", "final_state"])
    {
        contract.ending.final_state = final_state;
        changed = true;
    }
    if let Some(arc) =
        field_pack_string(text, &["主角弧线", "主角弧光", "成长线", "protagonist_arc"])
    {
        contract.protagonist_arc = arc;
        changed = true;
    }
    if let Some(imagery) = field_pack_string(
        text,
        &["世界观意象", "世界意象", "核心意象", "world_imagery"],
    ) {
        contract.world_imagery = imagery;
        changed = true;
    }
    if let Some(spine) = field_pack_string(
        text,
        &[
            "总主线因果链",
            "主线因果链",
            "主线因果",
            "main_causal_spine",
        ],
    ) {
        contract.main_causal_spine = spine;
        changed = true;
    }
    if let Some(themes) = field_pack_list(text, &["核心主题", "主题", "themes"]) {
        contract.themes = themes;
        changed = true;
    } else if let Some(themes) = field_pack_list(text, &["主题承诺"]) {
        contract.themes = themes;
        changed = true;
    }
    let characters = generated_fiction_character_lines(text);
    if !characters.is_empty() {
        contract.characters = characters
            .iter()
            .map(|line| super::draft_character_line_to_contract(line))
            .collect();
        changed = true;
    }
    if let Some(world_rules) = field_pack_world_rules(text) {
        contract.world_rules = world_rules;
        changed = true;
    }
    if let Some(style_rules) = field_pack_list(text, &["叙事风格", "风格", "style_rules"]) {
        contract.style_rules = style_rules;
        changed = true;
    }
    if let Some(must_avoid) = field_pack_list(text, &["必须避免", "禁区", "must_avoid"]) {
        contract.must_avoid = must_avoid;
        changed = true;
    }
    if let Some(quality) = field_pack_list(text, &["质量合同"]) {
        if contract.must_avoid.is_empty() {
            contract.must_avoid = quality
                .iter()
                .filter(|item| item.contains("不要") || item.contains("禁止"))
                .cloned()
                .collect();
            changed |= !contract.must_avoid.is_empty();
        }
        if contract.style_rules.is_empty() {
            let style_rules = quality
                .iter()
                .filter(|item| !item.contains("不要") && !item.contains("禁止"))
                .cloned()
                .collect::<Vec<_>>();
            if style_rules.is_empty() {
                contract
                    .style_rules
                    .push("用具体场景、行动和对话推进正文，不用摘要替代正文".to_string());
            } else {
                contract.style_rules = style_rules;
            }
            changed = true;
        }
    }
    let outline = generated_fiction_outline(text);
    if !outline.trim().is_empty() {
        contract.outline.raw_outline = normalize_field_pack_raw_outline(&outline, &contract);
        if contract.outline.near_chapters.is_empty() {
            contract.outline.near_chapters = collect_explicit_chapter_plan_titles(&outline)
                .into_iter()
                .enumerate()
                .map(|(index, goal)| ChapterSeedContract {
                    number: Some(index + 1),
                    expected_turn: String::new(),
                    goal,
                })
                .collect();
        }
        changed = true;
    }
    if contract.outline.volumes.is_empty() {
        let volumes = loose_volume_contracts_from_field_pack(text);
        if !volumes.is_empty() {
            contract.outline.volumes = volumes;
            changed = true;
        }
    }
    fill_primary_character_arc_from_contract(&mut contract);

    contract.normalize();
    changed.then_some(contract)
}

fn text_looks_like_contract_field_pack(text: &str) -> bool {
    let has_contract_surface = [
        "合同字段包",
        "标准小说合同",
        "小说创作合同",
        "故事合同",
        "角色权威表",
        "近期章节包",
    ]
    .iter()
    .any(|term| text.contains(term));
    let groups = [
        &["书名", "标题", "Title"][..],
        &["故事前提", "前提", "premise"][..],
        &["终局方向", "结局方向", "ending_direction"][..],
        &["主角弧线", "成长线", "protagonist_arc"][..],
        &["世界观意象", "世界意象", "world_imagery"][..],
        &["总主线因果链", "主线因果链", "main_causal_spine"][..],
        &["角色权威表", "人物权威表", "characters"][..],
        &["世界规则", "world_rules"][..],
        &["大纲", "分卷规划", "近期章节包", "outline"][..],
    ];
    let matched_groups = groups
        .iter()
        .filter(|labels| {
            labels
                .iter()
                .any(|label| generated_contract_field(text, &[*label]).is_some())
        })
        .count();
    has_contract_surface && matched_groups >= 3 || matched_groups >= 5
}

fn field_pack_string(text: &str, labels: &[&str]) -> Option<String> {
    generated_contract_field(text, labels)
        .map(|value| sanitize_generated_contract_scalar(&value))
        .filter(|value| !value_missing(value))
}

fn field_pack_list(text: &str, labels: &[&str]) -> Option<Vec<String>> {
    let value = field_pack_string(text, labels)?;
    let values = split_field_pack_list(&value);
    (!values.is_empty()).then_some(values)
}

pub(super) fn field_pack_world_rules(text: &str) -> Option<Vec<String>> {
    if let Some(value) = field_pack_string(text, &["世界规则", "规则", "world_rules"]) {
        let semicolon_values = split_field_pack_semicolon_list(&value);
        if semicolon_values.len() >= 2 {
            return Some(semicolon_values);
        }
        let values = split_field_pack_list(&value);
        if !values.is_empty() {
            return Some(values);
        }
    }
    let values = numbered_world_rule_lines(text);
    (!values.is_empty()).then_some(values)
}

fn field_pack_usize(
    text: &str,
    labels: &[&str],
    parser: impl Fn(&str) -> Option<usize>,
) -> Option<usize> {
    field_pack_string(text, labels).and_then(|value| parser(&value))
}

fn split_field_pack_list(value: &str) -> Vec<String> {
    value
        .lines()
        .flat_map(|line| line.split(['；', ';', '，', ',', '、']))
        .map(|item| {
            item.trim()
                .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'))
                .trim()
                .to_string()
        })
        .filter(|item| !value_missing(item))
        .take(12)
        .collect()
}

fn split_field_pack_semicolon_list(value: &str) -> Vec<String> {
    value
        .lines()
        .flat_map(|line| line.split(['；', ';']))
        .map(|item| {
            item.trim()
                .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'))
                .trim()
                .to_string()
        })
        .filter(|item| !value_missing(item))
        .take(12)
        .collect()
}

fn numbered_world_rule_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .filter_map(numbered_world_rule_line)
        .take(12)
        .collect()
}

fn numbered_world_rule_line(line: &str) -> Option<String> {
    let trimmed = line
        .trim()
        .trim_start_matches(|ch| matches!(ch, '-' | '*' | '+' | ' ' | '\t'));
    let tail = trimmed
        .strip_prefix("规则")
        .and_then(|value| {
            let digit_len = value
                .chars()
                .take_while(|ch| {
                    ch.is_ascii_digit() || matches!(ch, '一' | '二' | '三' | '四' | '五')
                })
                .map(char::len_utf8)
                .sum::<usize>();
            (digit_len > 0).then_some(&value[digit_len..])
        })
        .or_else(|| {
            let digit_len = trimmed
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .map(char::len_utf8)
                .sum::<usize>();
            (digit_len > 0).then_some(&trimmed[digit_len..])
        })?
        .trim_start_matches(|ch| matches!(ch, ':' | '：' | '.' | '、' | ')' | '）' | ' ' | '\t'))
        .trim();
    let value = sanitize_generated_contract_scalar(tail);
    (!value_missing(&value)
        && !crate::tool::writing::typed_contract_gate::world_rule_looks_truncated_or_not_actionable(
            &value,
        ))
    .then_some(value)
}

fn normalize_field_pack_raw_outline(outline: &str, contract: &NovelCreationContract) -> String {
    let compact = outline.replace(char::is_whitespace, "");
    if outline_has_conflicting_book_title(outline, contract) {
        return outline.to_string();
    }
    let marker_count = ["第", "卷", "章", "本章目标", "卷尾变化"]
        .iter()
        .filter(|marker| compact.contains(**marker))
        .count();
    let reference_count = compact.matches('章').count() + compact.matches('卷').count();
    if compact.chars().count() >= 120 && marker_count >= 3 && reference_count >= 4 {
        return first_non_missing_contract_summary(contract);
    }
    outline.to_string()
}

fn outline_has_conflicting_book_title(outline: &str, contract: &NovelCreationContract) -> bool {
    let canonical = contract.title.canonical_title.trim();
    if value_missing(canonical) {
        return false;
    }
    let chapter_titles = collect_explicit_chapter_plan_titles(outline);
    let mut rest = outline;
    while let Some(start) = rest.find('《') {
        let after_start = &rest[start + '《'.len_utf8()..];
        let Some(end) = after_start.find('》') else {
            break;
        };
        let quoted = after_start[..end].trim();
        if quoted_looks_like_book_title(quoted)
            && quoted != canonical
            && !chapter_titles.iter().any(|title| title.trim() == quoted)
            && !quoted_segment_is_explicit_chapter_title(outline, quoted)
            && !contract
                .outline
                .volumes
                .iter()
                .any(|volume| volume.title.trim() == quoted)
        {
            return true;
        }
        rest = &after_start[end + '》'.len_utf8()..];
    }
    false
}

fn quoted_segment_is_explicit_chapter_title(text: &str, quoted: &str) -> bool {
    let needle = format!("《{quoted}》");
    text.lines().any(|line| {
        let Some(index) = line.find(&needle) else {
            return false;
        };
        let prefix = &line[..index];
        prefix.contains('第') && prefix.contains('章')
    })
}

fn quoted_looks_like_book_title(value: &str) -> bool {
    let len = value.chars().count();
    if !(2..=16).contains(&len) {
        return false;
    }
    !value.contains("第")
        && !value.contains("章")
        && !value.contains("卷")
        && value
            .chars()
            .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch) || ch == '·')
}

fn first_non_missing_contract_summary(contract: &NovelCreationContract) -> String {
    [
        contract.main_causal_spine.as_str(),
        contract.premise.as_str(),
        contract.brief.as_str(),
        contract.ending.desired_resolution.as_str(),
    ]
    .into_iter()
    .find(|value| !value_missing(value))
    .unwrap_or("围绕本次小说合同推进主线、人物弧线和终局兑现。")
    .to_string()
}

fn fill_primary_character_arc_from_contract(contract: &mut NovelCreationContract) {
    let Some(primary) = contract
        .characters
        .iter_mut()
        .find(|character| character.role_looks_primary())
    else {
        return;
    };
    let arc = contract.protagonist_arc.trim();
    if arc.is_empty() {
        return;
    }
    if value_missing(&primary.arc_start) {
        primary.arc_start = super::project_arc_parts(arc, &contract.ending.desired_resolution).0;
    }
    if value_missing(&primary.arc_end) {
        primary.arc_end = super::project_arc_parts(arc, &contract.ending.desired_resolution).1;
    }
}

fn loose_volume_contracts_from_field_pack(text: &str) -> Vec<VolumeContract> {
    let mut volumes = Vec::new();
    let mut current = None::<VolumeContract>;
    for segment in text
        .lines()
        .flat_map(|line| line.split(['；', ';']))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        if let Some(title) = loose_volume_title(segment) {
            if let Some(volume) = current.take() {
                volumes.push(volume);
            }
            current = Some(VolumeContract {
                title,
                ..Default::default()
            });
            continue;
        }
        if let Some(volume) = current.as_mut() {
            if value_missing(&volume.objective) {
                if let Some(value) = loose_labeled_tail(segment, &["目标"]) {
                    volume.objective = value;
                    continue;
                }
            }
            if value_missing(&volume.ending_change) {
                if let Some(value) = loose_labeled_tail(segment, &["卷尾变化", "阶段变化"])
                {
                    volume.ending_change = value;
                }
            }
        }
    }
    if let Some(volume) = current {
        volumes.push(volume);
    }
    volumes
        .into_iter()
        .filter(|volume| !value_missing(&volume.title))
        .take(8)
        .collect()
}

fn loose_volume_title(segment: &str) -> Option<String> {
    let trimmed = segment
        .trim()
        .trim_start_matches(|ch: char| ch.is_ascii_digit() || matches!(ch, '.' | '、' | ')' | ' '));
    for marker in ["第一卷", "第二卷", "第三卷", "第四卷", "第五卷", "第六卷"] {
        let Some(rest) = trimmed.strip_prefix(marker) else {
            continue;
        };
        let title = rest
            .trim_start_matches(['：', ':', '-', ' '])
            .trim()
            .trim_matches(['《', '》', '"', '“', '”']);
        if !value_missing(title) {
            return Some(title.to_string());
        }
    }
    None
}

fn loose_labeled_tail(segment: &str, labels: &[&str]) -> Option<String> {
    labels.iter().find_map(|label| {
        let (_, tail) = segment.split_once(label)?;
        let tail = tail
            .trim_start_matches(['：', ':', '-', ' '])
            .trim()
            .trim_end_matches(['。', '；', ';']);
        (!value_missing(tail)).then(|| tail.to_string())
    })
}
