use super::turn_scope::approval_requests_first_writing_unit;
use super::*;

pub fn requested_title(message: &str) -> Option<String> {
    if message.contains('《') && message.contains('》') {
        for quoted in quoted_segments(message) {
            let trimmed = quoted.trim();
            if requested_title_candidate_is_valid(trimmed) {
                return Some(trimmed.to_string());
            }
        }
    }
    const TITLE_ASSIGNMENT_MARKERS: &[&str] = &[
        "书名叫",
        "标题叫",
        "题目叫",
        "小说叫",
        "书名是",
        "标题是",
        "题目是",
        "书名为",
        "标题为",
        "题目为",
        "书名改为",
        "标题改为",
        "命名为",
        "title is",
        "title:",
        "title=",
    ];
    for marker in TITLE_ASSIGNMENT_MARKERS {
        let Some((_, right)) = message.split_once(marker) else {
            continue;
        };
        for quoted in quoted_segments(right) {
            let trimmed = quoted.trim();
            if requested_title_candidate_is_valid(trimmed) {
                return Some(trimmed.to_string());
            }
        }
        if let Some(value) = requested_after_marker(message, &[*marker]) {
            let value = trim_title_wrappers(&value);
            if requested_title_candidate_is_valid(value) {
                return Some(value.to_string());
            }
        }
    }

    let whole = message.trim();
    for quoted in quoted_segments(whole) {
        let candidate = quoted.trim();
        if quoted_title_is_the_whole_message(whole, candidate)
            && requested_title_candidate_is_valid(candidate)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn trim_title_wrappers(value: &str) -> &str {
    value.trim().trim_matches(|ch| {
        matches!(
            ch,
            '《' | '》' | '「' | '」' | '“' | '”' | '"' | '\'' | '：' | ':'
        )
    })
}

fn quoted_title_is_the_whole_message(message: &str, candidate: &str) -> bool {
    let compact = message.trim();
    [
        ('《', '》'),
        ('「', '」'),
        ('“', '”'),
        ('"', '"'),
        ('\'', '\''),
    ]
    .iter()
    .any(|(left, right)| compact == format!("{left}{candidate}{right}"))
}

fn requested_title_candidate_is_valid(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    let len = trimmed.chars().count();
    if !(2..=80).contains(&len) {
        return false;
    }
    !title_surface_is_meta_discussion(trimmed)
}

pub(crate) fn title_surface_is_meta_discussion(title: &str) -> bool {
    let lowered = title.to_ascii_lowercase();
    naming::title_meta_discussion_markers()
        .iter()
        .any(|marker| title.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()))
}

pub fn creation_draft_requests_generated_title_revision(message: &str) -> bool {
    if requested_title(message).is_some() {
        return false;
    }
    let lowered = message.to_ascii_lowercase();
    let mentions_title = ["书名", "标题", "题目", "命名", "title", "name"]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    if !mentions_title {
        return false;
    }
    [
        "重新取",
        "重取",
        "重新起",
        "重起",
        "换一个",
        "换个",
        "另一个",
        "不同",
        "新书名",
        "新标题",
        "不够吸引人",
        "没有吸引力",
        "没吸引力",
        "不好听",
        "太普通",
        "太抽象",
        "太模板",
        "不要用",
        "不要叫",
        "别用",
        "别叫",
        "不要沿用",
        "不要重复",
        "别沿用",
        "别重复",
        "regenerate",
        "new",
        "different",
        "another",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn requested_after_marker(message: &str, markers: &[&str]) -> Option<String> {
    for marker in markers {
        if let Some((_, right)) = message.split_once(marker) {
            let value = right
                .split(|ch| matches!(ch, '，' | '。' | ',' | ';' | '；' | '\n'))
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches(|ch| matches!(ch, '：' | ':' | '"' | '\'' | '“' | '”'));
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn requested_chapter_unit_target(message: &str) -> Option<usize> {
    requested_raw_chapter_unit_target(message).map(nearest_novel_chapter_unit_band)
}

pub fn requested_raw_chapter_unit_target(message: &str) -> Option<usize> {
    let lowered = message.to_ascii_lowercase();
    let body_is_chapter_scoped = approval_requests_first_writing_unit(message, "fiction")
        || ["本章", "上一章", "下一章", "当前章"]
            .iter()
            .any(|term| message.contains(term))
        || ["next chapter", "previous chapter", "current chapter"]
            .iter()
            .any(|term| lowered.contains(term));
    let markers = [
        "每章",
        "一章",
        "单章",
        "本章目标",
        "本章字数",
        "章节目标",
        "章节字数",
        "正文目标",
        "正文字数",
        "字档位",
        "字档",
        "chapter",
    ]
    .into_iter()
    .chain(body_is_chapter_scoped.then_some("正文"));
    for marker in markers {
        if let Some(segment) = requested_unit_segment_before_marker(message, marker) {
            if let Some(value) = requested_semantically_scoped_unit_chars(&segment) {
                return Some(value);
            }
        }
        if let Some(segment) = requested_unit_segment_after_marker(
            message,
            marker,
            &[
                "一共",
                "总共",
                "总计",
                "总目标",
                "全文",
                "全书",
                "整部",
                "整体",
                "总字数",
                "target",
                "total",
            ],
        ) {
            if let Some(value) = requested_semantically_scoped_unit_chars(&segment) {
                return Some(value);
            }
        }
    }
    None
}

pub fn nearest_novel_chapter_unit_band(requested: usize) -> usize {
    longform_policy::nearest_novel_chapter_unit_band(requested)
}

pub fn requested_total_unit_target(message: &str) -> Option<usize> {
    let total_markers = [
        "一共",
        "总共",
        "总计",
        "总目标",
        "全文",
        "全书",
        "整部",
        "整体",
        "总字数",
        "total",
    ];
    for marker in total_markers {
        if let Some(segment) = requested_unit_segment_before_marker(message, marker) {
            if let Some(value) = requested_semantically_scoped_unit_chars(&segment) {
                return Some(value);
            }
        }
        if let Some(segment) = requested_unit_segment_after_marker(
            message,
            marker,
            &[
                "每章",
                "一章",
                "单章",
                "本章",
                "章节",
                "正文",
                "每节",
                "每段",
                "每部分",
                "chapter",
                "section",
            ],
        ) {
            if !segment_starts_with_continuation(&segment) {
                if let Some(value) = requested_semantically_scoped_unit_chars(&segment) {
                    return Some(value);
                }
            }
        }
    }

    let scoped_markers = [
        "每章",
        "一章",
        "单章",
        "本章",
        "章节",
        "正文目标",
        "正文字数",
        "字档位",
        "字档",
        "每节",
        "每段",
        "每部分",
        "chapter",
        "section",
    ];
    let unscoped_message = message_without_scoped_unit_segments(message, &scoped_markers);
    if unscoped_message.trim().is_empty() {
        return None;
    }
    let unscoped_target = DelegateTool::requested_text_target_chars(&unscoped_message)?;
    if requested_raw_chapter_unit_target(message) == Some(unscoped_target) {
        return None;
    }
    Some(unscoped_target)
}

fn requested_semantically_scoped_unit_chars(segment: &str) -> Option<usize> {
    DelegateTool::requested_text_target_chars(segment).or_else(|| {
        // The surrounding marker already supplies the “字数” semantics, so
        // natural forms such as “10万总字数” and “总目标字数是100000” do not
        // need to repeat a trailing “字”. Reuse the existing quantity parser
        // after restoring only that omitted unit.
        DelegateTool::requested_text_target_chars(&format!("{}字", segment.trim()))
    })
}

fn segment_starts_with_continuation(segment: &str) -> bool {
    segment
        .trim_start_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '：' | ':' | '=' | '，' | ',' | '；' | ';')
        })
        .starts_with(['和', '与', '及', '并'])
}

pub fn message_without_scoped_unit_segments(message: &str, scoped_markers: &[&str]) -> String {
    let mut out = String::new();
    for segment in
        message.split_inclusive(|ch| matches!(ch, '，' | '。' | '、' | ',' | ';' | '；' | '\n'))
    {
        if scoped_markers.iter().any(|marker| segment.contains(marker)) {
            continue;
        }
        out.push_str(segment);
    }
    out
}

pub fn requested_unit_segment_after_marker(
    message: &str,
    marker: &str,
    semantic_stops: &[&str],
) -> Option<String> {
    let (_, right) = message.split_once(marker)?;
    let right = right.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '，' | '、' | ',' | ';' | '；')
    });
    let mut end = right.len();
    for (idx, ch) in right.char_indices() {
        if matches!(ch, '，' | '。' | '、' | ',' | ';' | '；' | '\n') {
            end = idx;
            break;
        }
    }
    for stop in semantic_stops {
        if let Some(idx) = right.find(stop) {
            end = end.min(idx);
        }
    }
    let segment = right[..end]
        .trim()
        .trim_matches(|ch| {
            matches!(
                ch,
                '：' | ':' | '约' | '大' | '概' | '左' | '右' | '、' | ',' | '，'
            )
        })
        .to_string();
    if segment.is_empty() {
        None
    } else {
        Some(segment)
    }
}

pub fn requested_unit_segment_before_marker(message: &str, marker: &str) -> Option<String> {
    let (left, _) = message.split_once(marker)?;
    let clause = left
        .rsplit(|ch| matches!(ch, '，' | '。' | '、' | ',' | ';' | '；' | '\n'))
        .next()
        .unwrap_or(left)
        .trim()
        .trim_matches(|ch| matches!(ch, '约' | '大' | '概' | '左' | '右' | '、' | ',' | '，'));
    if clause.is_empty() {
        None
    } else {
        Some(clause.to_string())
    }
}

pub fn requested_section_unit_target(message: &str) -> Option<usize> {
    for marker in ["每节", "每段", "每部分", "section"] {
        if let Some((_, right)) = message.split_once(marker) {
            if let Some(value) = DelegateTool::requested_text_target_chars(right) {
                return Some(value);
            }
        }
    }
    None
}

pub fn requested_max_chapters_per_turn(message: &str) -> Option<usize> {
    for marker in ["每次", "每轮", "一次", "一轮"] {
        if let Some((_, right)) = message.split_once(marker) {
            let digits = right
                .chars()
                .take_while(|ch| !matches!(ch, '，' | '。' | ',' | ';' | '；' | '\n'))
                .filter(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if let Ok(value) = digits.parse::<usize>() {
                if value > 0 {
                    return Some(value);
                }
            }
        }
    }
    None
}

pub fn requested_export_format(message: &str) -> Option<String> {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("markdown") || lowered.contains(".md") || lowered.contains(" md") {
        Some("md".to_string())
    } else if lowered.contains("txt") || message.contains("文本") {
        Some("txt".to_string())
    } else {
        None
    }
}

pub fn requested_structure_items(message: &str) -> Vec<String> {
    let Some(value) = requested_after_marker(message, &["包含", "包括", "结构", "sections"])
    else {
        return Vec::new();
    };
    value
        .split(|ch| matches!(ch, '、' | '/' | '，' | ',' | ';' | '；'))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn requested_evidence_rules(message: &str) -> Vec<String> {
    let mut rules = Vec::new();
    if message.contains("引用") || message.contains("证据") || message.contains("来源") {
        rules.push("需要可核查引用或来源说明".to_string());
    }
    rules
}

pub fn requested_style_rules(message: &str) -> Vec<String> {
    requested_after_marker(message, &["风格", "语气", "style"])
        .into_iter()
        .collect()
}

pub fn infer_fiction_genre(message: &str) -> Option<String> {
    if let Some(value) = requested_after_marker(
        message,
        &[
            "题材是",
            "题材为",
            "题材：",
            "题材:",
            "类型是",
            "类型为",
            "类型：",
            "类型:",
            "genre",
        ],
    ) {
        let value = sanitize_creation_genre_value(&value);
        if !value.is_empty() {
            return Some(value);
        }
    }
    let compact = message.replace(char::is_whitespace, "");
    for surface in ["小说", "故事"] {
        if let Some((left, _)) = compact.split_once(surface) {
            let cleaned = strip_creation_prefix(left);
            let cleaned = sanitize_creation_genre_value(&cleaned);
            if !cleaned.is_empty()
                && cleaned.chars().count() <= 32
                && (longform_policy::looks_like_fiction_genre_surface(&cleaned)
                    || cleaned.contains('的')
                    || arbitrary_fiction_genre_surface_looks_usable(&cleaned))
                && !text_has_any(&cleaned, &["展示", "当前", "合同", "大纲", "章节"])
            {
                return Some(cleaned);
            }
        }
    }
    None
}

pub fn infer_followup_fiction_genre(message: &str) -> Option<String> {
    if !longform_policy::fiction_genre_signal_present(message) {
        return None;
    }
    let value = message
        .split(|ch| matches!(ch, '。' | '\n'))
        .next()
        .unwrap_or_default()
        .split("每章")
        .next()
        .unwrap_or_default()
        .split("保存")
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':' | ' '));
    if value.is_empty() || creation_draft_approval_requested(value) {
        return None;
    }
    let value = sanitize_creation_genre_value(value);
    if !value.is_empty() && value.chars().count() <= 32 {
        Some(value)
    } else {
        None
    }
}

pub fn strip_creation_prefix(value: &str) -> String {
    let mut out = value.trim().to_string();
    loop {
        let Some(prefix) = [
            "我想要",
            "我想",
            "从零开始",
            "麻烦",
            "请",
            "从零",
            "帮我",
            "替我",
            "给我",
            "为我",
            "重新",
            "新建",
            "创建",
            "生成",
            "就这样",
            "可以",
            "策划并创作",
            "策划与创作",
            "设计并创作",
            "设计与创作",
            "策划并写",
            "设计并写",
            "并自动创作",
            "并自动写",
            "自动创作",
            "自动写",
            "并创作",
            "并写",
            "策划",
            "设计",
            "写完",
            "创作",
            "开始",
            "写",
            "用",
            "改成",
            "换成",
            "改为",
            "换为",
            "一个",
            "一部",
            "一本",
            "的",
        ]
        .into_iter()
        .find(|prefix| out.starts_with(prefix) && out.len() > prefix.len()) else {
            break;
        };
        out = out[prefix.len()..]
            .trim_start_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':' | ' '))
            .to_string();
    }
    out.trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':'))
        .to_string()
}

fn arbitrary_fiction_genre_surface_looks_usable(value: &str) -> bool {
    let text = value.trim();
    let count = text.chars().count();
    if !(2..=24).contains(&count) {
        return false;
    }
    if text_has_any(
        text,
        &[
            "完整", "长篇", "短篇", "中篇", "原创", "新的", "好看", "有趣", "精彩", "一本", "一部",
            "一个",
        ],
    ) {
        return false;
    }
    text.chars()
        .any(|ch| !ch.is_ascii() && !ch.is_ascii_punctuation())
}

pub fn creation_brief(message: &str, artifact_kind: &str) -> String {
    let mut brief = sanitize_creation_brief_value(message.trim());
    brief = strip_creation_prefix(&brief);
    if artifact_kind == "fiction" {
        brief = brief.replace("小说", "").replace("故事", "");
    }
    sanitize_creation_brief_value(
        &brief
            .trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':' | ' '))
            .to_string(),
    )
}

pub(crate) fn sanitize_creation_brief_value(value: &str) -> String {
    value
        .split(|ch| matches!(ch, '；' | ';' | '\n'))
        .map(str::trim)
        .map(strip_creation_control_phrases)
        .map(strip_creation_parameter_control_clauses)
        .map(strip_creation_process_control_clauses)
        .filter(|part| !part.is_empty() && !creation_control_only_fragment(part))
        .collect::<Vec<_>>()
        .join("；")
}

pub(crate) fn sanitize_creation_genre_value(value: &str) -> String {
    value
        .split(|ch| matches!(ch, '；' | ';' | '\n' | '。' | '.'))
        .map(str::trim)
        .find(|part| !part.is_empty() && !creation_control_only_fragment(part))
        .map(strip_creation_control_phrases)
        .map(strip_creation_genre_list_marker)
        .map(strip_creation_genre_tail_marker)
        .unwrap_or_default()
        .trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':' | ' '))
        .to_string()
}

fn strip_creation_genre_list_marker(value: &str) -> &str {
    value
        .trim_end_matches(|ch: char| ch.is_ascii_digit())
        .trim_end_matches(|ch| matches!(ch, '、' | ')' | '）' | '：' | ':' | ' '))
        .trim()
}

fn strip_creation_genre_tail_marker(value: &str) -> &str {
    let mut trimmed = value.trim();
    if let Some((left, _)) = trimmed.split_once("题材") {
        trimmed = left.trim();
    }
    if let Some((left, _)) = trimmed.split_once("类型") {
        trimmed = left.trim();
    }
    for suffix in ["长篇", "中篇", "短篇"] {
        if let Some(left) = trimmed.strip_suffix(suffix) {
            trimmed = left.trim();
            break;
        }
    }
    trimmed
}

fn strip_creation_control_phrases(value: &str) -> &str {
    let mut trimmed = value.trim();
    loop {
        let next = trimmed
            .strip_prefix("请用")
            .or_else(|| trimmed.strip_prefix("用"))
            .or_else(|| trimmed.strip_prefix("你来定"))
            .or_else(|| trimmed.strip_prefix("你决定"))
            .or_else(|| trimmed.strip_prefix("自动补齐"))
            .or_else(|| trimmed.strip_prefix("自动补全"))
            .or_else(|| trimmed.strip_prefix("补齐后给我确认"))
            .or_else(|| trimmed.strip_prefix("给我确认"))
            .or_else(|| trimmed.strip_prefix("先定合同"))
            .or_else(|| trimmed.strip_prefix("不要写正文"))
            .map(|tail| {
                tail.trim_start_matches(|ch| {
                    matches!(ch, '，' | ',' | '。' | '；' | ';' | '：' | ':' | ' ' | '\t')
                })
            });
        let Some(next) = next else {
            return trimmed;
        };
        trimmed = next;
    }
}

fn strip_creation_parameter_control_clauses(value: &str) -> String {
    let parts = value
        .split(|ch| matches!(ch, '，' | ',' | '。' | '.'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let retained = parts
        .iter()
        .copied()
        .filter(|part| !creation_parameter_control_clause(part))
        .collect::<Vec<_>>();
    if retained.len() == parts.len() {
        value.trim().to_string()
    } else {
        retained.join("，")
    }
}

fn strip_creation_process_control_clauses(value: String) -> String {
    let parts = value
        .split(|ch| matches!(ch, '，' | ',' | '。' | '.'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let retained = parts
        .iter()
        .copied()
        .filter(|part| !creation_process_control_clause(part))
        .collect::<Vec<_>>();
    if retained.len() == parts.len() {
        value.trim().to_string()
    } else {
        retained.join("，")
    }
}

fn creation_parameter_control_clause(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let unit_marker = compact.contains('字')
        || compact.contains("章")
        || compact.contains("万")
        || compact.chars().any(|ch| ch.is_ascii_digit());
    unit_marker
        && [
            "每章",
            "总字数",
            "目标字数",
            "至少",
            "起步",
            "起",
            "不少于",
            "大概",
            "约",
            "章节",
            "每次",
            "自动写",
            "字数",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

fn creation_process_control_clause(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    if creation_wait_control_clause(&compact) {
        return true;
    }
    if creation_confirm_then_write_control_clause(&compact) {
        return true;
    }
    let workflow_negation = ["不要", "别", "禁止", "不复用", "无需"]
        .iter()
        .any(|term| compact.contains(term));
    let workflow_target = [
        "正文",
        "旧项目",
        "历史项目",
        "已有项目",
        "内部路径",
        "工具参数",
        "伪成功",
    ]
    .iter()
    .any(|term| compact.contains(term));
    if workflow_negation && workflow_target {
        return true;
    }
    let removes_previous_contract_authority = ["删除", "清除", "移除", "去掉"]
        .iter()
        .any(|term| compact.contains(term))
        && [
            "旧设定",
            "原设定",
            "无关设定",
            "旧角色",
            "原角色",
            "旧书名",
            "原书名",
            "旧大纲",
            "原大纲",
            "旧合同",
            "原合同",
        ]
        .iter()
        .any(|term| compact.contains(term));
    if removes_previous_contract_authority {
        return true;
    }
    let language_directive = ["全程", "必须", "保持", "只用", "使用"]
        .iter()
        .any(|term| compact.contains(term))
        && ["中文", "英文", "日文", "韩文"]
            .iter()
            .any(|term| compact.contains(term));
    if language_directive && compact.chars().count() <= 24 {
        return true;
    }
    let delegates_generated_contract_content = [
        "书名",
        "标题",
        "人物姓名",
        "角色姓名",
        "人物名字",
        "角色名字",
        "世界观",
        "大纲",
        "结局",
    ]
    .iter()
    .any(|term| compact.contains(term))
        && [
            "由你",
            "你来",
            "自动",
            "由系统",
            "系统来",
            "由工具",
            "工具来",
            "由模型",
            "模型来",
        ]
        .iter()
        .any(|term| compact.contains(term))
        && ["生成", "原创", "决定", "设定", "补齐", "补全"]
            .iter()
            .any(|term| compact.contains(term));
    if delegates_generated_contract_content {
        return true;
    }
    if compact.contains("不要暴露")
        || compact.contains("内部路径")
        || compact.contains("工具参数")
        || compact.contains("伪成功")
        || compact.to_ascii_lowercase().contains("json")
    {
        return true;
    }
    let asks_contract_or_outline = [
        "合同草案",
        "创作合同",
        "完整合同",
        "合同",
        "大纲",
        "框架",
        "草案",
    ]
    .iter()
    .any(|term| compact.contains(term));
    if !asks_contract_or_outline {
        return false;
    }
    [
        "请先整理",
        "先整理",
        "请先建立",
        "先建立",
        "整理成",
        "可确认",
        "请先给我",
        "先给我",
        "先给合同",
        "给合同确认",
        "合同确认",
        "自动生成",
        "自动补齐",
        "自动补全",
        "自动修复",
        "生成并修复",
        "修复完整",
        "给我",
        "展示",
        "等我确认",
        "我确认",
        "确认后",
        "确认再",
        "不要写正文",
        "先不要写",
        "再开始",
    ]
    .iter()
    .any(|term| compact.contains(term))
}

fn creation_confirm_then_write_control_clause(compact: &str) -> bool {
    (compact.contains("确认后") || compact.contains("确认再") || compact.contains("确认了再"))
        && ["开始写", "再开始写", "再写", "正式写", "开始正文", "写正文"]
            .iter()
            .any(|term| compact.contains(term))
}

fn creation_wait_control_clause(compact: &str) -> bool {
    matches!(
        compact,
        "然后等我"
            | "等我"
            | "等我确认"
            | "等待我确认"
            | "等我看完"
            | "等我确认后"
            | "之后等我确认"
            | "然后等我确认"
    )
}

fn creation_control_only_fragment(value: &str) -> bool {
    let normalized = value
        .trim()
        .trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '.' | '！' | '!' | '：' | ':' | ' '))
        .to_ascii_lowercase();
    let raw = value.trim();
    if creation_planning_note_is_quality_feedback(raw) || creation_contract_repair_only_message(raw)
    {
        return true;
    }
    if creation_process_control_clause(raw) {
        return true;
    }
    matches!(
        normalized.as_str(),
        "你来定"
            | "你决定"
            | "自动补齐"
            | "自动补全"
            | "补齐合同"
            | "补全合同"
            | "补齐后给我确认"
            | "给我确认"
            | "先定合同"
            | "不要写正文"
            | "停止当前任务"
            | "停止任务"
            | "停下当前任务"
            | "停下任务"
            | "暂停当前任务"
            | "暂停任务"
            | "取消当前任务"
            | "取消任务"
            | "先停一下"
            | "停一下"
            | "先暂停"
            | "pause"
            | "stop"
            | "cancel"
    )
}

pub fn creation_planning_notes(message: &str, artifact_kind: &str) -> Vec<String> {
    let fiction_contract_revision = artifact_kind == "fiction"
        && text_has_any(message, &["合同", "草案", "创作蓝图", "story contract"])
        && text_has_any(
            message,
            &[
                "修复", "修订", "修改", "更新", "改成", "改为", "统一", "同步", "纠正", "更正",
            ],
        );
    let markers = if artifact_kind == "fiction" {
        &[
            "主角",
            "女主",
            "男主",
            "反派",
            "学校",
            "学院",
            "考试",
            "晋级",
            "升级",
            "体系",
            "世界观",
            "结尾",
            "感情",
            "大纲",
            "分卷",
            "人物",
            "目标",
        ][..]
    } else {
        &[
            "主题", "论点", "读者", "结构", "证据", "引用", "目的", "用途", "风格", "章节", "部分",
        ][..]
    };
    message
        .split(|ch| matches!(ch, '。' | '；' | ';' | '\n'))
        .map(str::trim)
        .map(strip_creation_process_prefix_from_mixed_clause)
        .filter(|part| {
            !part.is_empty()
                && (text_has_any(part, markers) || fiction_contract_revision)
                && !creation_control_only_fragment(part)
                && !creation_process_control_clause(part)
        })
        .map(strip_creation_prefix)
        .map(|part| {
            part.trim_matches(|ch| matches!(ch, '，' | ',' | '。' | '：' | ':' | ' '))
                .to_string()
        })
        .filter(|part| !part.is_empty())
        .take(12)
        .collect()
}

fn strip_creation_process_prefix_from_mixed_clause(value: &str) -> &str {
    for separator in ['：', ':'] {
        let Some((prefix, payload)) = value.split_once(separator) else {
            continue;
        };
        let payload = payload.trim();
        if !payload.is_empty()
            && (creation_process_control_clause(prefix) || creation_control_only_fragment(prefix))
        {
            return payload;
        }
    }
    value
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCharacterNameDeclaration {
    pub role: String,
    pub name: String,
}

pub fn explicit_user_character_name_declarations(
    message: &str,
) -> Vec<UserCharacterNameDeclaration> {
    let mut declarations = Vec::new();
    for clause in message.split(|ch| matches!(ch, '，' | ',' | '。' | '；' | ';' | '\n')) {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        collect_cjk_character_name_declarations(clause, &mut declarations);
        collect_latin_character_name_declarations(clause, &mut declarations);
    }
    declarations.sort_by(|left, right| {
        left.role
            .cmp(&right.role)
            .then_with(|| left.name.cmp(&right.name))
    });
    declarations.dedup();
    declarations
}

pub fn explicit_user_character_name_notes(message: &str) -> Vec<String> {
    explicit_user_character_name_declarations(message)
        .into_iter()
        .map(|declaration| format!("明确指定角色姓名：{}", declaration.name))
        .collect()
}

fn collect_cjk_character_name_declarations(
    clause: &str,
    declarations: &mut Vec<UserCharacterNameDeclaration>,
) {
    const ROLE_ALIASES: &[(&str, &str)] = &[
        ("主人公", "主角"),
        ("男主人公", "主角"),
        ("女主人公", "主角"),
        ("主角", "主角"),
        ("男主", "主角"),
        ("女主", "主角"),
        ("反派", "反派"),
        ("对手", "对手"),
        ("导师", "导师"),
        ("角色", "角色"),
        ("人物", "角色"),
    ];
    const CONNECTORS: &[&str] = &[
        "姓名设定为",
        "名字设定为",
        "姓名改成",
        "名字改成",
        "姓名改为",
        "名字改为",
        "姓名是",
        "名字是",
        "设定为",
        "名叫",
        "名为",
        "改成",
        "改为",
        "叫",
        "是",
    ];

    for (alias, role) in ROLE_ALIASES {
        let mut rest = clause;
        while let Some(role_index) = rest.find(alias) {
            let after_role = &rest[role_index + alias.len()..];
            let Some((connector_index, connector)) = CONNECTORS
                .iter()
                .filter_map(|connector| after_role.find(connector).map(|index| (index, *connector)))
                .min_by_key(|(index, connector)| (*index, usize::MAX - connector.len()))
            else {
                rest = &after_role[after_role
                    .char_indices()
                    .nth(1)
                    .map(|(index, _)| index)
                    .unwrap_or(after_role.len())..];
                continue;
            };
            let prefix = after_role[..connector_index]
                .trim()
                .trim_matches(|ch| matches!(ch, '：' | ':' | '的' | ' ' | '\t'));
            if prefix.chars().count() > 6
                || ["不", "别", "不要", "无需"]
                    .iter()
                    .any(|term| prefix.contains(term))
            {
                rest = &after_role[connector_index + connector.len()..];
                continue;
            }
            let name_tail = &after_role[connector_index + connector.len()..];
            if let Some(name) = cjk_declared_character_name(name_tail, connector == "是") {
                declarations.push(UserCharacterNameDeclaration {
                    role: (*role).to_string(),
                    name,
                });
            }
            rest = name_tail;
        }
    }
}

fn cjk_declared_character_name(tail: &str, bare_copula: bool) -> Option<String> {
    let tail = tail.trim_start_matches(|ch| {
        matches!(
            ch,
            '：' | ':' | ' ' | '\t' | '《' | '「' | '“' | '"' | '\'' | '为'
        )
    });
    if bare_copula
        && ["一名", "一个", "一位", "一种", "负责", "来自", "属于"]
            .iter()
            .any(|prefix| tail.starts_with(prefix))
    {
        return None;
    }
    let mut candidate = String::new();
    for (index, ch) in tail.char_indices() {
        let remaining = &tail[index..];
        if !candidate.is_empty()
            && (remaining.starts_with('的')
                || remaining.starts_with("担任")
                || remaining.starts_with("作为")
                || remaining.starts_with("负责")
                || remaining.starts_with("是一")
                || remaining.starts_with("将会")
                || remaining.starts_with("会在"))
        {
            break;
        }
        if ch == '·' || ch == '•' || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            candidate.push(ch);
            if candidate.chars().count() >= 12 {
                break;
            }
        } else {
            break;
        }
    }
    let candidate = candidate.trim();
    if bare_copula
        && tail
            .strip_prefix(candidate)
            .is_some_and(|rest| rest.starts_with('的'))
    {
        return None;
    }
    if bare_copula && candidate.chars().count() > 4 {
        return None;
    }
    if [
        "女性",
        "男性",
        "女孩",
        "男孩",
        "医生",
        "警察",
        "学生",
        "老师",
        "工程师",
        "调查员",
    ]
    .contains(&candidate)
    {
        return None;
    }
    naming::audit_character_name_candidate(candidate, "zh-CN")
        .accepted
        .then(|| candidate.to_string())
}

fn collect_latin_character_name_declarations(
    clause: &str,
    declarations: &mut Vec<UserCharacterNameDeclaration>,
) {
    const ROLE_ALIASES: &[(&str, &str)] = &[
        ("main character", "protagonist"),
        ("protagonist", "protagonist"),
        ("heroine", "protagonist"),
        ("hero", "protagonist"),
        ("antagonist", "antagonist"),
        ("villain", "antagonist"),
        ("mentor", "mentor"),
        ("character", "character"),
    ];
    const CONNECTORS: &[&str] = &[" named ", " called ", " will be ", " is "];
    let lowered = clause.to_ascii_lowercase();
    for (alias, role) in ROLE_ALIASES {
        let mut search_start = 0;
        while let Some(relative_index) = lowered[search_start..].find(alias) {
            let role_end = search_start + relative_index + alias.len();
            let after_role = &lowered[role_end..];
            let Some((connector_index, connector)) = CONNECTORS
                .iter()
                .filter_map(|connector| after_role.find(connector).map(|index| (index, *connector)))
                .min_by_key(|(index, connector)| (*index, usize::MAX - connector.len()))
            else {
                break;
            };
            if after_role[..connector_index]
                .trim()
                .split_whitespace()
                .count()
                > 3
            {
                search_start = role_end;
                continue;
            }
            let name_start = role_end + connector_index + connector.len();
            if let Some(name) = latin_declared_character_name(&clause[name_start..]) {
                declarations.push(UserCharacterNameDeclaration {
                    role: (*role).to_string(),
                    name,
                });
            }
            search_start = name_start;
        }
    }
}

fn latin_declared_character_name(tail: &str) -> Option<String> {
    let mut words = Vec::new();
    for raw in tail
        .trim_start_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, ':' | '"' | '\'' | '“' | '”')
        })
        .split_whitespace()
    {
        let word = raw.trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | '.' | ';' | ':' | '!' | '?' | '"' | '\'' | '“' | '”'
            )
        });
        let Some(first) = word.chars().next() else {
            break;
        };
        if !first.is_uppercase()
            || !word
                .chars()
                .all(|ch| ch.is_alphabetic() || matches!(ch, '-' | '\'' | '’' | '.'))
        {
            break;
        }
        words.push(word);
        if words.len() >= 4 {
            break;
        }
    }
    let candidate = words.join(" ");
    naming::audit_character_name_candidate(&candidate, "en")
        .accepted
        .then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_fiction_genre_uses_first_story_surface_before_process_clauses() {
        let genre = infer_fiction_genre(
            "帮我写一部都市轻玄幻成长小说，每章2500字，至少5万字。请先给出完整创作合同草案，合同确认后自动写完整部小说并导出 TXT。",
        );

        assert_eq!(genre.as_deref(), Some("都市轻玄幻成长"));
    }

    #[test]
    fn extracts_structured_user_character_name_declarations() {
        let cases = [
            ("主角叫林远。", "林远"),
            ("主角名叫林远。", "林远"),
            ("主人公是林远。", "林远"),
            ("男主我想叫林远。", "林远"),
            ("反派设定为顾寒。", "顾寒"),
            ("The protagonist is Alice.", "Alice"),
            ("A character named Alice serves as the mentor.", "Alice"),
        ];
        for (message, expected) in cases {
            let declarations = explicit_user_character_name_declarations(message);
            assert!(
                declarations
                    .iter()
                    .any(|declaration| declaration.name == expected),
                "{message}: {declarations:?}"
            );
        }
    }

    #[test]
    fn ordinary_character_mentions_do_not_become_user_name_authority() {
        for message in [
            "林远走进档案室，开始检查旧记录。",
            "不要改主角名字。",
            "主角是一名负责修复档案的工程师。",
            "女主是边州驿站的女译官。",
            "男主是监察院的年轻监察官。",
        ] {
            assert!(
                explicit_user_character_name_declarations(message).is_empty(),
                "{message}"
            );
        }
    }

    #[test]
    fn infer_fiction_genre_accepts_arbitrary_story_surface() {
        let genre = infer_fiction_genre("帮我写一部蒸汽朋克侦探小说，每章2500字，一共5万字。");

        assert_eq!(genre.as_deref(), Some("蒸汽朋克侦探"));
    }

    #[test]
    fn infer_fiction_genre_strips_composed_creation_command_without_damaging_genre() {
        assert_eq!(
            infer_fiction_genre("请从零为我写完一部修仙小说，总字数10万字。").as_deref(),
            Some("修仙")
        );
        assert_eq!(
            infer_fiction_genre("帮我写一部书写疗愈小说，总字数10万字。").as_deref(),
            Some("书写疗愈")
        );
        assert_eq!(
            infer_fiction_genre("我想从零创作一本修仙长篇小说，总字数10万字，每章固定2500字。")
                .as_deref(),
            Some("修仙")
        );
        assert_eq!(
            infer_fiction_genre("请从零策划并创作一部历史权谋小说，总字数10万字。").as_deref(),
            Some("历史权谋")
        );
        assert_eq!(
            infer_fiction_genre("请从零设计一部现代言情小说，总字数10万字。").as_deref(),
            Some("现代言情")
        );
        assert_eq!(
            creation_brief(
                "请从零策划并创作一部历史权谋小说，总字数10万字。",
                "fiction"
            ),
            "历史权谋"
        );
        assert_eq!(
            infer_fiction_genre(
                "请从零创建并自动写一部赛博朋克长篇小说，总字数10万字，使用2500字每章档。"
            )
            .as_deref(),
            Some("赛博朋克")
        );
        assert_eq!(
            creation_brief(
                "请从零创建并自动写一部赛博朋克长篇小说，总字数10万字，使用2500字每章档。",
                "fiction"
            ),
            "赛博朋克长篇"
        );
    }

    #[test]
    fn infer_fiction_genre_does_not_treat_generic_size_as_genre() {
        let genre = infer_fiction_genre("帮我写一部完整小说，每章2500字，一共5万字。");

        assert_eq!(genre, None);
    }

    #[test]
    fn infer_fiction_genre_prefers_explicit_marker_over_project_setup_words() {
        let genre =
            infer_fiction_genre("新开一个全新小说项目，题材为东方奇幻，总字数37万字，每章2500字。");

        assert_eq!(genre.as_deref(), Some("东方奇幻"));
    }

    #[test]
    fn infer_fiction_genre_trims_use_topic_request_surface() {
        let genre =
            infer_fiction_genre("请用都市异能悬疑题材，创建一本全新长篇小说，总字数100万字。");

        assert_eq!(genre.as_deref(), Some("都市异能悬疑"));
    }

    #[test]
    fn infer_fiction_genre_strips_new_project_command_prefix() {
        let genre = infer_fiction_genre(
            "请新建一部2008年中国东南沿海背景的现实主义职场悬疑长篇小说，总字数10万字。",
        );

        assert_eq!(
            genre.as_deref(),
            Some("2008年中国东南沿海背景的现实主义职场悬疑")
        );
    }

    #[test]
    fn creation_planning_notes_strip_new_project_command_prefix() {
        let notes = creation_planning_notes(
            "请新建一部现实主义医疗冷链职场悬疑小说：主角发现承包商伪造温度记录。主角必须找回纸质台账。",
            "fiction",
        );

        assert_eq!(
            notes,
            vec![
                "现实主义医疗冷链职场悬疑小说：主角发现承包商伪造温度记录",
                "主角必须找回纸质台账"
            ]
        );
    }

    #[test]
    fn contract_revision_keeps_all_story_fact_clauses_without_field_keyword_whitelist() {
        let notes = creation_planning_notes(
            "修复合同，不要写正文。钟星岚统一为女性寒门官员，不称士子或书生；唐晏白第一章的身份与第五章继位节点统一；终局和标题理由统一为主动挂印辞官归隐，不是流放；总因果链以朝局稳定并主动归隐这一事件结果收束。",
            "fiction",
        );

        assert_eq!(
            notes,
            vec![
                "钟星岚统一为女性寒门官员，不称士子或书生",
                "唐晏白第一章的身份与第五章继位节点统一",
                "终局和标题理由统一为主动挂印辞官归隐，不是流放",
                "总因果链以朝局稳定并主动归隐这一事件结果收束",
            ]
        );
    }

    #[test]
    fn contract_revision_keeps_story_payload_after_do_not_write_process_prefix() {
        let notes = creation_planning_notes(
            "合同还需一处明确修改，不要写正文：把近期规划第5章直接写清楚为‘父皇驾崩，原为皇子的同一人物继位成为幼主’，并保持第1章是微服出行的皇子。只更新相关大纲字段，等待确认。",
            "fiction",
        );

        assert!(notes.iter().any(|note| {
            note.contains("父皇驾崩") && note.contains("第1章") && note.contains("第5章")
        }));
    }

    #[test]
    fn arbitrary_total_targets_remain_exact_while_chapter_size_uses_supported_bands() {
        for (message, chapter_units, total_units) in [
            ("每章2500字，总字数7万字", 2_500, 70_000),
            ("每章2500字，总字数37万字", 2_500, 370_000),
            ("每章5000字，总字数100万字", 5_000, 1_000_000),
            ("每章5000字，总字数250万字", 5_000, 2_500_000),
        ] {
            assert_eq!(requested_chapter_unit_target(message), Some(chapter_units));
            assert_eq!(requested_total_unit_target(message), Some(total_units));
        }
        assert_eq!(
            requested_total_unit_target("保持原来的末世题材、10万字总字数和2500字档位"),
            Some(100_000)
        );
        assert_eq!(
            requested_total_unit_target("保持原来的题材、10万总字数和2500字档不变"),
            Some(100_000)
        );
        assert_eq!(
            requested_total_unit_target(
                "用户权威保持不变：小说总目标字数是100000，每章档位是2500，预计约40章"
            ),
            Some(100_000)
        );
        assert_eq!(
            requested_total_unit_target("保持总字数和2500字档不变"),
            None
        );
    }

    #[test]
    fn body_or_chapter_target_does_not_override_project_total() {
        for message in [
            "确认这个合同，开始写第一章。正文目标约2500字，写完请自动审稿并保存。",
            "确认，开始写第一章，本章目标约2500字。",
            "确认，先写第一章，章节目标5000字。",
            "我确认这份合同。按这个开始，只写第一章，目标约2500字；写完自动审稿、批准保存并导出。",
            "合同已确认，先写下一章，目标5000字，然后审稿保存。",
            "按这个合同开始写。先只完成第一章：自动生成章节合同、写作、审稿、必要修订并保存；正文达到2500字档位。",
        ] {
            assert_eq!(requested_total_unit_target(message), None, "{message}");
        }
        assert_eq!(
            requested_raw_chapter_unit_target(
                "确认这个合同，开始写第一章。正文目标约2500字，写完请自动审稿并保存。"
            ),
            Some(2500)
        );
        assert_eq!(
            requested_raw_chapter_unit_target("确认，先写第一章，章节目标5000字。"),
            Some(5000)
        );
        assert_eq!(
            requested_raw_chapter_unit_target(
                "按这个合同开始写。先只完成第一章：自动生成章节合同、写作、审稿、必要修订并保存；正文达到2500字档位。"
            ),
            Some(2500)
        );
        assert_eq!(
            requested_total_unit_target("每章2500字，写10万字。"),
            Some(100000)
        );
        assert_eq!(
            requested_total_unit_target("请写一部长篇小说，正文10万字。"),
            Some(100000)
        );
        assert_eq!(
            requested_total_unit_target("全书2500字，每章2500字。"),
            Some(2500)
        );
    }

    #[test]
    fn creation_brief_excludes_runtime_and_language_control_clauses() {
        let brief = creation_brief(
            "请创建一本全新的近未来深海考古悬疑小说，题材为近未来深海考古悬疑，总字数10万字，每章2500字。先生成完整创作合同供我确认，现在不要写正文。书名和角色由你原创，全程中文，不复用任何旧项目。",
            "fiction",
        );

        assert!(brief.contains("近未来深海考古悬疑"));
        assert!(!brief.contains("不要写正文"));
        assert!(!brief.contains("全程中文"));
        assert!(!brief.contains("不复用任何旧项目"));
    }

    #[test]
    fn creation_brief_excludes_contract_establishment_workflow_clause() {
        let brief = creation_brief(
            "请写一本发生在1998年西北高原天文台的科学悬疑长篇：一次流星雨观测后，光学技师发现底片编号被篡改。总字数10万字，每章2500字。先建立完整创作合同，合同确认后再写完整本书。",
            "fiction",
        );

        assert!(brief.contains("1998年西北高原天文台"));
        assert!(!brief.contains("先建立"));
        assert!(!brief.contains("创作合同"));
        assert!(!brief.contains("合同确认后"));
    }

    #[test]
    fn explicit_initial_project_total_is_extracted() {
        for message in [
            "新建一本整本总字数20万字的小说，每章2500字。",
            "创建长篇小说，全文目标30万字，每章5000字。",
            "全书一共100万字，每章5000字。",
        ] {
            assert!(requested_total_unit_target(message).is_some(), "{message}");
        }
        assert_eq!(
            requested_total_unit_target("新建一本整本总字数20万字的小说，每章2500字。"),
            Some(200000)
        );
    }
}
