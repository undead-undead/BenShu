use super::*;

pub(super) fn ensure_chapter_key_facts(
    manifest: &NovelProjectManifest,
    chapter: &mut ChapterRecord,
    content: &str,
) {
    if !chapter.key_facts.is_empty() {
        return;
    }
    let mut facts = Vec::new();
    facts.extend(chapter_truth_fallback_items(
        manifest,
        content,
        CHAPTER_FACT_LIMIT,
    ));
    for candidate in [
        chapter_summary_fallback(content, &manifest.language),
        chapter.summary.clone(),
    ] {
        if let Some(value) = non_empty(&candidate) {
            push_unique_update(&mut facts, value);
        }
        if facts.len() >= 3 {
            break;
        }
    }
    chapter.key_facts = compact_truth_items(facts, CHAPTER_FACT_LIMIT);
}

pub(super) fn ensure_chapter_continuity_updates(
    manifest: &NovelProjectManifest,
    chapter: &mut ChapterRecord,
    content: &str,
) {
    if !chapter.continuity_updates.is_empty() {
        return;
    }
    let mut updates = Vec::new();
    for fact in &chapter.key_facts {
        if let Some(value) = non_empty(fact) {
            push_unique_update(&mut updates, value);
        }
        if updates.len() >= 3 {
            break;
        }
    }
    if updates.is_empty() {
        if let Some(value) = non_empty(&chapter.summary) {
            push_unique_update(&mut updates, value);
        }
    }
    updates.extend(chapter_truth_fallback_items(
        manifest,
        content,
        CHAPTER_CONTINUITY_LIMIT,
    ));
    if updates.is_empty() {
        if let Some(value) = non_empty(&chapter_summary_fallback(content, &manifest.language)) {
            push_unique_update(&mut updates, value);
        }
    }
    chapter.continuity_updates = updates;
}

pub(super) fn normalize_chapter_metadata_against_body(
    manifest: &NovelProjectManifest,
    chapter: &mut ChapterRecord,
    content: &str,
) {
    chapter.title = repair_contract_character_name_typos(manifest, &chapter.title);
    chapter.summary = repair_contract_character_name_typos(
        manifest,
        &compact_chapter_summary(
            &sanitize_metadata_text_for_manifest(manifest, &chapter.summary),
            &manifest.language,
        ),
    );
    chapter.key_facts = supported_chapter_truth_items(
        manifest,
        chapter.key_facts.clone(),
        content,
        CHAPTER_FACT_LIMIT,
    );
    chapter.continuity_updates = supported_chapter_truth_items(
        manifest,
        chapter.continuity_updates.clone(),
        content,
        CHAPTER_CONTINUITY_LIMIT,
    );

    if chapter_summary_is_body_prefix(&chapter.summary, content, &manifest.language)
        || chapter_summary_looks_like_prose_fragment(&chapter.summary, &manifest.language)
    {
        if let Some(summary) = summary_from_truth_items(manifest, &chapter.key_facts) {
            chapter.summary = summary;
        } else if let Some(summary) =
            summary_from_truth_items(manifest, &chapter.continuity_updates)
        {
            chapter.summary = summary;
        }
    }

    ensure_chapter_key_facts(manifest, chapter, content);
    ensure_chapter_continuity_updates(manifest, chapter, content);

    if !chapter_summary_has_authority_anchor(manifest, &chapter.summary) {
        if let Some(summary) = summary_from_truth_items(manifest, &chapter.key_facts) {
            chapter.summary = summary;
        } else if let Some(summary) =
            summary_from_truth_items(manifest, &chapter.continuity_updates)
        {
            chapter.summary = summary;
        }
    }

    if !chapter_summary_supported_by_content(&chapter.summary, content, &manifest.language) {
        if let Some(summary) = summary_from_truth_items(manifest, &chapter.key_facts) {
            chapter.summary = summary;
        } else if let Some(summary) =
            summary_from_truth_items(manifest, &chapter.continuity_updates)
        {
            chapter.summary = summary;
        } else {
            chapter.summary = chapter_summary_fallback(content, &manifest.language);
        }
    }

    chapter.key_facts = supported_chapter_truth_items(
        manifest,
        chapter.key_facts.clone(),
        content,
        CHAPTER_FACT_LIMIT,
    );
    chapter.continuity_updates = supported_chapter_truth_items(
        manifest,
        chapter.continuity_updates.clone(),
        content,
        CHAPTER_CONTINUITY_LIMIT,
    );

    if chapter.key_facts.is_empty() {
        let mut fallback = vec![chapter_summary_fallback(content, &manifest.language)];
        governance::retain_truth_items_supported_by_chapter(&mut fallback, content);
        chapter.key_facts = compact_truth_items(fallback, CHAPTER_FACT_LIMIT);
    }
    if chapter.continuity_updates.is_empty() {
        let mut fallback = if chapter.key_facts.is_empty() {
            vec![chapter_summary_fallback(content, &manifest.language)]
        } else {
            chapter.key_facts.clone()
        };
        governance::retain_truth_items_supported_by_chapter(&mut fallback, content);
        chapter.continuity_updates = compact_truth_items(fallback, CHAPTER_CONTINUITY_LIMIT);
    }
    let final_title = final_chapter_title_from_body_with_metadata(
        manifest,
        chapter.number,
        &chapter.title,
        &chapter.summary,
        &chapter.key_facts,
        &chapter.continuity_updates,
        content,
    );
    chapter.title = final_title;
}

fn chapter_truth_fallback_items(
    manifest: &NovelProjectManifest,
    content: &str,
    limit: usize,
) -> Vec<String> {
    let anchors = manifest_character_anchors(manifest);
    let mut ranked = chapter_truth_fallback_candidates(content, &anchors, &manifest.language);
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    compact_truth_items(
        ranked.into_iter().map(|(_, sentence)| sentence).collect(),
        limit,
    )
}

fn chapter_truth_fallback_candidates(
    content: &str,
    anchors: &[String],
    language: &str,
) -> Vec<(usize, String)> {
    sanitize_saved_prose(content)
        .split(|ch| matches!(ch, '。' | '！' | '？' | '\n' | '.' | '!' | '?'))
        .map(str::trim)
        .filter(|sentence| {
            let len = sentence.chars().count();
            len >= 12 && len <= 160
        })
        .filter(|sentence| !chapter_truth_candidate_looks_like_prose_fragment(sentence, language))
        .filter_map(|sentence| {
            let score = chapter_truth_candidate_score(sentence, anchors, language);
            (score > 0).then(|| (score, sentence.to_string()))
        })
        .collect()
}

fn chapter_truth_candidate_looks_like_prose_fragment(sentence: &str, language: &str) -> bool {
    let trimmed = sentence.trim();
    if trimmed.is_empty() {
        return true;
    }
    if chapter_summary_looks_like_prose_fragment(trimmed, language) {
        return true;
    }
    if !is_chinese_language(language) {
        return false;
    }
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.contains("，“") || compact.contains("，”") || compact.contains("：“") {
        return true;
    }
    if compact.starts_with('他')
        || compact.starts_with('她')
        || compact.starts_with('它')
        || compact.starts_with("那人")
        || compact.starts_with("老人")
        || compact.starts_with("少年")
    {
        return true;
    }
    false
}

fn chapter_truth_candidate_score(sentence: &str, anchors: &[String], language: &str) -> usize {
    let mut score = 0usize;
    let anchor_hits = anchors
        .iter()
        .filter(|anchor| !anchor.trim().is_empty() && sentence.contains(anchor.as_str()))
        .count();
    if anchor_hits > 0 {
        score += 4 + anchor_hits.min(3);
    }
    if chapter_truth_candidate_has_event_verb(sentence, language) {
        score += 3;
    }
    if chapter_truth_candidate_has_relationship_marker(sentence, language) {
        score += 2;
    }
    if chapter_truth_candidate_is_atmospheric_opening(sentence, language) {
        score = score.saturating_sub(4);
    }
    score
}

fn chapter_truth_candidate_has_event_verb(sentence: &str, language: &str) -> bool {
    if is_chinese_language(language) {
        let terms = [
            "发现", "决定", "拒绝", "接受", "获得", "失去", "交换", "合作", "调查", "追查", "命令",
            "威胁", "揭开", "证明", "购买", "卖给", "进入", "离开", "背叛", "抛弃", "觉醒", "确认",
            "暴露", "建立", "达成", "击败", "保护", "救下",
        ];
        return terms
            .iter()
            .any(|term| cjk_event_term_supported(sentence, term));
    }
    let lowered = sentence.to_ascii_lowercase();
    [
        "decided",
        "discovered",
        "accepted",
        "refused",
        "gained",
        "lost",
        "traded",
        "investigated",
        "threatened",
        "revealed",
        "proved",
        "bought",
        "sold",
        "entered",
        "left",
        "betrayed",
        "allied",
    ]
    .iter()
    .any(|term| lowered.contains(term))
}

fn cjk_event_term_supported(sentence: &str, term: &str) -> bool {
    sentence.match_indices(term).any(|(idx, _)| {
        let suffix = sentence[idx + term.len()..].chars().next();
        !matches!(
            suffix,
            Some('力' | '性' | '权' | '额' | '率' | '价' | '单' | '者' | '层' | '感')
        )
    })
}

fn chapter_truth_candidate_has_relationship_marker(sentence: &str, language: &str) -> bool {
    if is_chinese_language(language) {
        let terms = [
            "未婚妻",
            "朋友",
            "学长",
            "盟友",
            "对手",
            "敌人",
            "家族",
            "合作",
            "关系",
            "背叛",
            "抛弃",
            "牵线",
            "佣金",
            "条件",
            "信任",
            "怀疑",
        ];
        return terms.iter().any(|term| sentence.contains(term));
    }
    let lowered = sentence.to_ascii_lowercase();
    [
        "fiancee",
        "friend",
        "ally",
        "rival",
        "enemy",
        "family",
        "partner",
        "relationship",
        "betrayed",
        "trust",
    ]
    .iter()
    .any(|term| lowered.contains(term))
}

fn chapter_truth_candidate_is_atmospheric_opening(sentence: &str, language: &str) -> bool {
    if is_chinese_language(language) {
        let terms = ["雨", "夜色", "灯火", "天空", "风", "月", "空气", "街道"];
        return terms.iter().any(|term| sentence.contains(term))
            && !chapter_truth_candidate_has_event_verb(sentence, language);
    }
    false
}

pub(super) fn apply_explicit_chapter_metadata_args(
    manifest: &NovelProjectManifest,
    chapter: &mut ChapterRecord,
    args: &NovelStudioArgs,
) {
    if !args.chapter_title.trim().is_empty() {
        chapter.title = args.chapter_title.trim().to_string();
    }
    if !args.summary.trim().is_empty() {
        let summary = compact_chapter_summary(args.summary.trim(), &manifest.language);
        chapter.summary = repair_contract_character_name_typos(manifest, &summary);
    }
    let key_facts = clean_list(&args.key_facts);
    if !key_facts.is_empty() {
        chapter.key_facts = clean_contract_character_name_typos(
            manifest,
            compact_truth_items(key_facts, CHAPTER_FACT_LIMIT),
        );
    }
    let continuity_updates = clean_list(&args.continuity_updates);
    if !continuity_updates.is_empty() {
        chapter.continuity_updates = clean_contract_character_name_typos(
            manifest,
            compact_truth_items(continuity_updates, CHAPTER_CONTINUITY_LIMIT),
        );
    }
}

pub(super) fn chapter_summary_supported_by_content(
    summary: &str,
    content: &str,
    language: &str,
) -> bool {
    let summary = summary.trim();
    if summary.is_empty() || content.trim().is_empty() {
        return false;
    }
    if chapter_summary_is_body_prefix(summary, content, language) {
        return false;
    }
    let clauses = split_summary_support_clauses(summary, language);
    if clauses.is_empty() {
        let mut probe = vec![summary.to_string()];
        return governance::retain_truth_items_supported_by_chapter(&mut probe, content).is_empty();
    }
    clauses.into_iter().all(|clause| {
        if is_chinese_language(language) {
            return cjk_summary_clause_supported_by_content(&clause, content);
        }
        let mut probe = vec![clause];
        governance::retain_truth_items_supported_by_chapter(&mut probe, content).is_empty()
    })
}

pub(super) fn chapter_summary_looks_like_prose_fragment(summary: &str, language: &str) -> bool {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(
        first,
        '"' | '\'' | '“' | '”' | '‘' | '’' | '，' | '。' | '、' | ',' | '.'
    ) {
        return true;
    }
    if !is_chinese_language(language) {
        return false;
    }
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .take(16)
        .collect::<String>();
    let semantic_compact = cjk_summary_without_transition_prefix(&compact);
    ["我", "我们", "你", "你们", "他", "她", "他们", "她们"]
        .iter()
        .any(|prefix| semantic_compact.starts_with(prefix))
        || trimmed.contains('：')
        || trimmed.contains(':')
        || semantic_compact.starts_with("他说")
        || semantic_compact.starts_with("她说")
        || semantic_compact.starts_with("它说")
        || semantic_compact.starts_with("他问")
        || semantic_compact.starts_with("她问")
        || semantic_compact.starts_with("他答")
        || semantic_compact.starts_with("她答")
        || semantic_compact.starts_with("如实回答")
        || semantic_compact.starts_with("低声")
        || semantic_compact.starts_with("冷声")
        || cjk_summary_starts_with_pronoun_body_action(semantic_compact)
        || cjk_summary_contains_dialogue_tail_fragment(trimmed)
        || cjk_summary_starts_like_scene_fragment(semantic_compact)
        || cjk_summary_looks_like_descriptive_body_excerpt(semantic_compact)
}

fn cjk_summary_without_transition_prefix(compact: &str) -> &str {
    let mut current = compact;
    for _ in 0..2 {
        let Some(rest) = [
            "然而",
            "不过",
            "但是",
            "可是",
            "于是",
            "因此",
            "随后",
            "接着",
            "同时",
            "此时",
            "此刻",
            "这时",
            "片刻后",
            "紧接着",
        ]
        .iter()
        .find_map(|prefix| current.strip_prefix(prefix)) else {
            break;
        };
        current = rest.trim_start_matches(['，', ',', '；', ';', '：', ':']);
    }
    current
}

fn cjk_summary_starts_with_pronoun_body_action(compact: &str) -> bool {
    let Some(rest) = ["他", "她", "它", "他们", "她们", "它们"]
        .iter()
        .find_map(|prefix| compact.strip_prefix(prefix))
    else {
        return false;
    };
    let possessive_body_fragments = [
        "的动作",
        "的手",
        "的脚",
        "的眼",
        "的目光",
        "的声音",
        "的脸",
        "的肩",
        "的背",
        "的心",
    ];
    if possessive_body_fragments
        .iter()
        .any(|prefix| rest.starts_with(prefix))
    {
        return true;
    }
    let body_action_prefixes = [
        "咬紧",
        "抬起",
        "低下",
        "转身",
        "回头",
        "深吸",
        "握紧",
        "闭上",
        "睁开",
        "停下",
        "迈步",
        "站起",
        "蹲下",
        "伸手",
        "举起",
        "看向",
        "望向",
        "听见",
        "感到",
        "意识到",
    ];
    body_action_prefixes
        .iter()
        .any(|prefix| rest.starts_with(prefix))
}

fn cjk_summary_looks_like_descriptive_body_excerpt(compact: &str) -> bool {
    if compact.contains("——") {
        return true;
    }
    if compact.starts_with("那是")
        || compact.starts_with("这是")
        || compact.starts_with("这就是")
        || compact.starts_with("那就是")
    {
        return true;
    }
    [
        "也是他",
        "也是她",
        "唯一值得",
        "少数几个",
        "像是",
        "仿佛",
        "似乎",
        "粗糙得",
        "微微",
        "轻轻",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn cjk_summary_contains_dialogue_tail_fragment(summary: &str) -> bool {
    let compact = summary
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if !(compact.contains('“')
        || compact.contains('”')
        || compact.contains('‘')
        || compact.contains('’'))
    {
        return false;
    }
    [
        "”他说",
        "”她说",
        "”他问",
        "”她问",
        "”他低声",
        "”她低声",
        "”他顿",
        "”她顿",
        "”他看",
        "”她看",
        "’他说",
        "’她说",
        "’他问",
        "’她问",
        "’他低声",
        "’她低声",
        "’他顿",
        "’她顿",
        "’他看",
        "’她看",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn cjk_summary_starts_like_scene_fragment(compact: &str) -> bool {
    let scene_openers = [
        "就在",
        "当他",
        "当她",
        "当它",
        "当众人",
        "正当",
        "随着",
        "这时",
        "此时",
        "忽然",
        "突然",
        "片刻后",
        "下一刻",
    ];
    scene_openers
        .iter()
        .any(|opener| compact.starts_with(opener))
}

pub(super) fn chapter_summary_has_authority_anchor(
    manifest: &NovelProjectManifest,
    summary: &str,
) -> bool {
    let summary = summary.trim();
    if summary.is_empty() {
        return false;
    }
    let anchors = manifest_character_anchors(manifest);
    anchors
        .iter()
        .any(|anchor| !anchor.trim().is_empty() && summary.contains(anchor.as_str()))
}

fn cjk_summary_clause_supported_by_content(clause: &str, content: &str) -> bool {
    let tokens = cjk_summary_support_tokens(clause);
    if tokens.is_empty() {
        let mut probe = vec![clause.to_string()];
        return governance::retain_truth_items_supported_by_chapter(&mut probe, content).is_empty();
    }
    let supported = tokens
        .iter()
        .filter(|token| content.contains(token.as_str()))
        .count();
    let short_tokens = tokens.iter().filter(|token| token.chars().count() == 2);
    let short_total = short_tokens.clone().count();
    let short_supported = short_tokens
        .filter(|token| content.contains(token.as_str()))
        .count();
    supported >= 4
        && supported * 100 >= tokens.len() * 25
        && short_supported >= 2
        && short_supported * 100 >= short_total.max(1) * 25
}

pub(super) fn chapter_summary_supported_by_truth_items(
    chapter: &ChapterRecord,
    language: &str,
) -> bool {
    let clauses = split_summary_support_clauses(&chapter.summary, language);
    if clauses.is_empty() {
        return false;
    }
    let truth_items = chapter
        .key_facts
        .iter()
        .chain(chapter.continuity_updates.iter())
        .map(|item| normalized_metadata_evidence(item))
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    clauses.into_iter().all(|clause| {
        let clause = normalized_metadata_evidence(&clause);
        truth_items.iter().any(|item| item == &clause)
    })
}

fn normalized_metadata_evidence(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'))
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn cjk_summary_support_tokens(value: &str) -> Vec<String> {
    let chars = value
        .chars()
        .filter(|ch| is_cjk_unified(*ch))
        .collect::<Vec<_>>();
    let mut tokens = Vec::new();
    for len in [4usize, 3, 2] {
        if chars.len() < len {
            continue;
        }
        for window in chars.windows(len) {
            let token = window.iter().collect::<String>();
            if cjk_summary_support_token_is_useful(&token)
                && !tokens.iter().any(|known| known == &token)
            {
                tokens.push(token);
            }
        }
    }
    tokens
}

fn cjk_summary_support_token_is_useful(token: &str) -> bool {
    let generic = [
        "主角", "本章", "章节", "中心", "力量", "强大", "真正", "开始", "继续",
    ];
    !generic.iter().any(|item| token.contains(item))
}

fn split_summary_support_clauses(summary: &str, language: &str) -> Vec<String> {
    let min_chars = if is_chinese_language(language) {
        10
    } else {
        20
    };
    summary
        .split(|ch| matches!(ch, '。' | '！' | '？' | ';' | '；' | '\n'))
        .map(str::trim)
        .filter(|part| part.chars().count() >= min_chars)
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn supported_chapter_truth_items(
    manifest: &NovelProjectManifest,
    items: Vec<String>,
    content: &str,
    limit: usize,
) -> Vec<String> {
    let cleaned = items
        .into_iter()
        .map(|item| sanitize_metadata_text_for_manifest(manifest, &item))
        .map(|item| repair_contract_character_name_typos(manifest, &item))
        .collect::<Vec<_>>();
    let mut cleaned = compact_truth_items(clean_list(&cleaned), limit);
    governance::retain_truth_items_supported_by_chapter(&mut cleaned, content);
    cleaned
}

pub(super) fn sanitize_metadata_text_for_manifest(
    manifest: &NovelProjectManifest,
    value: &str,
) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if is_chinese_language(&manifest.language) {
        sanitize_chinese_script_noise(manifest, trimmed)
    } else {
        trimmed.to_string()
    }
}

pub(super) fn push_unique_update(updates: &mut Vec<String>, value: String) {
    let normalized = value.trim();
    if normalized.is_empty() {
        return;
    }
    if !updates.iter().any(|existing| existing == normalized) {
        updates.push(normalized.to_string());
    }
}

pub(super) fn chapter_summary_fallback(content: &str, language: &str) -> String {
    let trimmed = chapter_metadata_body_without_leading_heading(content);
    if trimmed.is_empty() {
        return String::new();
    }
    let max_chars = if is_chinese_language(language) {
        120
    } else {
        180
    };

    if let Some(sentence) = trimmed
        .split(|ch| matches!(ch, '。' | '！' | '？' | '\n' | '.' | '!' | '?'))
        .map(str::trim)
        .filter(|sentence| {
            let len = sentence.chars().count();
            len >= 12 && len <= max_chars
        })
        .find(|sentence| {
            chapter_truth_candidate_has_event_verb(sentence, language)
                && !chapter_truth_candidate_is_atmospheric_opening(sentence, language)
                && !chapter_summary_looks_like_prose_fragment(sentence, language)
        })
    {
        return sentence.chars().take(max_chars).collect();
    }

    let sentence_end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| {
            matches!(ch, '。' | '！' | '？' | '.' | '!' | '?').then_some(idx + ch.len_utf8())
        })
        .unwrap_or_else(|| trimmed.len().min(max_chars));
    trimmed[..sentence_end.min(trimmed.len())]
        .chars()
        .take(max_chars)
        .collect()
}

fn chapter_metadata_body_without_leading_heading(content: &str) -> String {
    let cleaned = sanitize_saved_prose(content);
    let mut lines = cleaned.lines().peekable();
    while lines.peek().is_some_and(|line| line.trim().is_empty()) {
        lines.next();
    }
    if lines.peek().is_some_and(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.starts_with("### ")
    }) {
        lines.next();
        while lines.peek().is_some_and(|line| line.trim().is_empty()) {
            lines.next();
        }
    }
    lines.collect::<Vec<_>>().join("\n").trim().to_string()
}

fn chapter_summary_is_body_prefix(summary: &str, content: &str, language: &str) -> bool {
    let summary = summary.trim();
    if summary.is_empty() || content.trim().is_empty() {
        return false;
    }
    let body = sanitize_saved_prose(content);
    let summary_key = compact_for_prefix_match(summary);
    let body_key = compact_for_prefix_match(&body);
    let min_chars = if is_chinese_language(language) {
        24
    } else {
        48
    };
    summary_key.chars().count() >= min_chars && body_key.starts_with(&summary_key)
}

fn summary_from_truth_items(manifest: &NovelProjectManifest, items: &[String]) -> Option<String> {
    let anchors = manifest_character_anchors(manifest);
    let joined = items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .filter(|item| {
            !chapter_summary_looks_like_prose_fragment(item, &manifest.language)
                && (chapter_summary_has_authority_anchor(manifest, item)
                    || anchors
                        .iter()
                        .any(|anchor| !anchor.trim().is_empty() && item.contains(anchor.as_str())))
        })
        .take(2)
        .collect::<Vec<_>>()
        .join(if is_chinese_language(&manifest.language) {
            "；"
        } else {
            "; "
        });
    non_empty(&compact_chapter_summary(&joined, &manifest.language))
}

fn compact_for_prefix_match(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            !matches!(
                ch,
                ' ' | '\t' | '\n' | '\r' | '#' | '*' | '`' | '"' | '\'' | '“' | '”' | '《' | '》'
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_summary_support_accepts_grounded_paraphrase() {
        let body = "洛照舟在塔克拉玛干沙漠进入地下遗址，找到许砚安留下的笔记。他用血激活青铜星盘和立体星图。秦栖棠随后赶到，两人共同压住星盘外环，应对吞噬影子的影蚀现象。";
        let summary = "考古学家洛照舟在沙漠深处发现导师许砚安失踪的地下遗址，他通过滴血激活青铜星盘与立体星图，并与秦栖棠共同应对星图带来的影蚀现象。";

        assert!(chapter_summary_supported_by_content(summary, body, "zh-CN"));
    }

    #[test]
    fn chapter_summary_fallback_skips_heading_and_prefers_event_sentence() {
        let content = "# 金融界\n\n江城的深秋，雨丝挂在写字楼玻璃上。祝澈宁确认恒达科技盘口被资金恶意压盘，并用第一笔做空验证自己的市场预判。";

        let summary = chapter_summary_fallback(content, "zh-CN");

        assert!(!summary.starts_with('#'), "{summary}");
        assert!(!summary.contains("金融界"), "{summary}");
        assert!(summary.contains("确认恒达科技"), "{summary}");
        assert!(summary.contains("做空"), "{summary}");
    }

    #[test]
    fn chapter_summary_detects_dialogue_fragment_surface() {
        assert!(chapter_summary_looks_like_prose_fragment(
            "”祝珩阙如实回答，“晚辈发现，这里的浊气中，蕴含着一种独特的韵律",
            "zh-CN"
        ));
        assert!(chapter_summary_looks_like_prose_fragment(
            "如实回答，晚辈发现这里的浊气中蕴含着韵律",
            "zh-CN"
        ));
        assert!(chapter_summary_looks_like_prose_fragment(
            "如果你能跑出标准成绩，系统会标记你的‘隐藏潜能’；如果跑不出……”她顿了顿，目光投向远处高耸入云的精英塔。",
            "zh-CN"
        ));
        assert!(!chapter_summary_looks_like_prose_fragment(
            "祝珩阙发现浊灵石回应他的灵脉，并决定换取外门试炼资格。",
            "zh-CN"
        ));
    }

    #[test]
    fn chapter_summary_detects_scene_transition_fragment_surface() {
        assert!(chapter_summary_looks_like_prose_fragment(
            "就在他转身准备离开时，一阵轻微的脚步声从巷口传来",
            "zh-CN"
        ));
        assert!(chapter_summary_looks_like_prose_fragment(
            "他咬紧牙关，强行压制住心跳的频率，让身体进入一种近乎停滞的状态",
            "zh-CN"
        ));
        assert!(chapter_summary_looks_like_prose_fragment(
            "他的动作迟缓而精准，尽管舌尖已经失去了对咸淡酸甜的感知",
            "zh-CN"
        ));
        assert!(chapter_summary_looks_like_prose_fragment(
            "然而，她的依赖是克制的，目光始终没有离开终端上的参数",
            "zh-CN"
        ));
    }

    #[test]
    fn chapter_summary_detects_descriptive_body_excerpt_surface() {
        assert!(chapter_summary_looks_like_prose_fragment(
            "那是三天前在一次锅炉爆炸事故中失去的左臂，也是他作为黑铁号首席机械师唯一值得炫耀的遗产——虽然这只义肢粗糙得像是铁匠铺里随手打废的半成品",
            "zh-CN"
        ));
        assert!(!chapter_summary_looks_like_prose_fragment(
            "白知白与艾拉达成合作，决定驾驶黑铁号穿过回声兽群前往云栖岛。",
            "zh-CN"
        ));
    }

    #[test]
    fn chapter_truth_fallback_skips_dialogue_and_pronoun_fragments() {
        let content = "”裴曜阙冷冷打断，“这石头是我先发现的，谁敢碰，废其灵根。”温朔砺没有给他反应的机会，他转身一把抓起地上的黑石，在裴曜阙重新稳住身形之前，拉着姜衡阙向矿坑深处狂奔而去。温朔砺获得骨血黑石，并决定带姜衡阙逃入矿坑深处。";
        let anchors = vec!["温朔砺".to_string(), "姜衡阙".to_string()];

        let candidates = chapter_truth_fallback_candidates(content, &anchors, "zh-CN")
            .into_iter()
            .map(|(_, sentence)| sentence)
            .collect::<Vec<_>>();

        assert!(
            candidates
                .iter()
                .any(|sentence| sentence.contains("获得骨血黑石")),
            "{candidates:?}"
        );
        assert!(
            !candidates
                .iter()
                .any(|sentence| sentence.contains("谁敢碰")),
            "{candidates:?}"
        );
        assert!(
            !candidates.iter().any(|sentence| sentence.starts_with('他')),
            "{candidates:?}"
        );
    }
}
