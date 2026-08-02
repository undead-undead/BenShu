use super::*;
use crate::tool::writing::typed_contract_gate;
use std::collections::{BTreeMap, BTreeSet};

mod issue_classification;
mod revision_prompt;

pub(super) use issue_classification::*;
pub(super) use revision_prompt::*;

pub(super) fn chapter_completes_delivery_review_window(chapter_number: usize) -> bool {
    chapter_number > 0 && chapter_number % 5 == 0
}

pub(super) fn chapter_requires_llm_quality_audit(chapter_number: usize) -> bool {
    chapter_number > 0 && chapter_number <= 2
}

pub(super) fn delivery_advisory_window_prompt(language: &str, window: &Value) -> String {
    let window = serde_json::to_string(window).unwrap_or_else(|_| "{}".to_string());
    if language_looks_cjk(language) {
        return format!(
            "你是小说交付表现观察员。下面是五个连续、已批准章节的受控只读样本与最终正文状态结算。只比较这五章的交付表现，不改写正文，不新增剧情事实，不修改人物身份、世界规则、伏笔状态、终局或合同。\n\
             观察开篇和章尾是否重复、主要人物对白是否同质、场景类型是否偏科、句段节奏是否长期单一，以及读者承诺是否持续得到可见兑现。因果、关系、情绪或伏笔问题只能转写为下一阶段的表达/交付建议，不能宣称新的故事事实。\n\
             只返回 JSON：{{\"advisories\":[{{\"category\":\"opening|ending|dialogue|scene_mix|rhythm|reader_promise\",\"message\":\"简短、可执行且不改变故事事实的建议\"}}],\"score\":0-100}}。最多 6 条；没有可靠建议时返回空数组。不要输出 verdict、finding、blocker、next_action、Markdown 或解释。score 仅用于观测。\n\n五章窗口：\n{window}"
        );
    }
    format!(
        "You observe delivery patterns across five contiguous approved fiction chapters. The bounded read-only samples and final-body settlements follow. Compare delivery only: opening and ending repetition, homogeneous character voices, scene mix, sentence/paragraph rhythm, and visible fulfillment of the reader promise. Do not rewrite prose or invent/alter story facts, identities, world rules, hook state, ending, or contract. Any causal, relationship, emotional, or hook concern must be phrased only as a delivery suggestion for the next window.\n\
         Return JSON only: {{\"advisories\":[{{\"category\":\"opening|ending|dialogue|scene_mix|rhythm|reader_promise\",\"message\":\"short actionable advice that changes no story fact\"}}],\"score\":0-100}}. At most 6 items; use an empty array when evidence is insufficient. Never output verdict, finding, blocker, next_action, Markdown, or explanation. Score is telemetry only.\n\nFive-chapter window:\n{window}"
    )
}

pub(super) fn parse_delivery_advisory_window_output(
    raw: &str,
) -> Option<RawDeliveryAdvisoryWindow> {
    const ALLOWED: [&str; 6] = [
        "opening",
        "ending",
        "dialogue",
        "scene_mix",
        "rhythm",
        "reader_promise",
    ];
    let cleaned = clean_model_output(raw);
    let json = novel_runner::extract_json(&cleaned)?;
    let mut output = serde_json::from_str::<RawDeliveryAdvisoryWindow>(&json).ok()?;
    output.advisories = output
        .advisories
        .into_iter()
        .filter_map(|mut advisory| {
            advisory.category = advisory.category.trim().to_ascii_lowercase();
            advisory.message = advisory.message.trim().to_string();
            (ALLOWED.contains(&advisory.category.as_str()) && !advisory.message.is_empty())
                .then_some(advisory)
        })
        .collect();
    output.advisories.sort_by(|left, right| {
        (left.category.as_str(), left.message.as_str())
            .cmp(&(right.category.as_str(), right.message.as_str()))
    });
    output.advisories.dedup_by(|left, right| left == right);
    output.advisories.truncate(6);
    output.score = output.score.map(|score| score.min(100));
    Some(output)
}

pub(super) async fn read_chapter_body_from_write_result(value: &Value) -> Option<String> {
    if let Some(body) = value
        .get("candidate_body")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|body| !body.is_empty())
    {
        return Some(body.to_string());
    }
    let path = value
        .get("artifact_path")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/chapter/path").and_then(Value::as_str))?;
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    Some(surface_sanitizer::strip_markdown_frontmatter(&raw))
}

pub(super) fn collect_string_array_to_vec(value: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    collect_string_array(value, &mut out);
    out
}

pub(super) fn llm_quality_audit_prompt(
    language: &str,
    chapter_number: usize,
    title: &str,
    deterministic: &[String],
    authority_context: &str,
    content: &str,
) -> String {
    let deterministic = if deterministic.is_empty() {
        "none".to_string()
    } else {
        deterministic.join("\n- ")
    };
    let authority_context = if authority_context.trim().is_empty() {
        "none".to_string()
    } else {
        authority_context.to_string()
    };
    if language_looks_cjk(language) {
        return format!(
            "你是小说章节的质量审稿人，只审稿，不改写。请检查第 {chapter_number} 章《{title}》是否适合进入正式章节。\n\
             重点检查：中文是否通顺；是否有乱码、外文残片、公式残片、JSON/工具回执；是否有明显错字残字、词语拼接错误、重复插入字符；是否不符合已给人物和情节合同；是否像摘要/大纲而不是正文；是否复述本章前文或上一段而没有新事件；是否只围绕设定加字、没有具体行动/代价/关系变化；新势力、新设定或关键帮助是否缺少铺垫和风险。必须单独检查正文最后 3 段：如果本章已经自然收束，后面却又追加一个没有完成动作闭环、因果后果或新收束的短动作段，属于正文截断/拼接残片，必须重写，不能因最后一句有句号就放行。\n\
             必须以“项目与大纲权威”、“当前章节合同”和“上一批准状态”为权威：检查正文是否完成本章目标、是否提前消费未来章节事件、是否让角色弧线提前完成、是否改变既定时间线。还必须逐项核对正文新声明的主角身份、过去经历、知识或能力来源是否已存在于合同或批准状态；凭空增加的背景或知识来源会改变故事前提，属于必须重写的合同漂移。逐项核对同一关键物件在本章内的来源、持有者、位置、状态和首次获得事件；不能先写角色已经持有某物，后面又把同名或同描述物件当作首次获得，除非正文明确区分为两个物件且合同允许。章尾必须让下一大纲节点仍能自然发生；若地点、任务、人物状态或因果方向被改成与下一节点冲突，属于必须重写的合同漂移。未经大纲或既有伏笔支持的新主线、新谜团、新关键物件，若取代本章目标或把故事引向另一任务，也属于硬错误。若合同明确了时代或技术边界，还要检查正文用语、器物和人物认知是否明显越界。\n\
             “下一大纲节点”只表示未来禁区：本章没有完成或提及它是正确状态。绝不能因为下一章事件尚未发生而判错，包括人物、相遇、决定、揭示、冲突或转折尚未出现；也绝不能在 issues 或 feedback 中建议提前补入，若正文已经提前写入，才应指出并要求删除。\n\
             人物/术语是否漂移以确定性检查问题为准；如果确定性检查没有指出人物漂移，不要因为正文里同时出现多个角色名就判定主角改名。\n\
             只把有精确权威引用和正文引用的合同/连续性冲突写入 authority_conflicts。每项必须包含 kind、authority_path（JSON Pointer）、authority_excerpt、body_excerpt、message。章节标题、摘要、关键事实、连续性记录、审美偏好、措辞、节奏和主观评分只能写入 advisories。请在 advisories 中补充观察：前两三段是否建立本章具体问题，主要人物对白是否同质化或只在解释设定，关键行动/代价/关系变化是否被总结取代，修饰语是否遮蔽主体行动，句段节奏是否长期单一，以及近章开头/章尾形态是否重复。这些表现问题即使明显也不得写入 authority_conflicts，不得要求重写。\n\
             只返回 JSON，不要 Markdown，不要解释。JSON 字段：authority_conflicts(array object), advisories(array string), score(0-100)。score 只用于观测，不能决定是否重写。\n\n\
             确定性检查问题：\n- {deterministic}\n\n合同与连续性权威：\n{authority_context}\n\n正文：\n{content}"
        );
    }
    format!(
        "You are a fiction chapter quality auditor. Audit chapter {chapter_number}, \"{title}\", without rewriting it.\n\
         Check prose fluency, mojibake/foreign-script/math/JSON/tool receipt residue, obvious typo fragments or spliced words, contract drift, whether the text is actual prose rather than outline/summary, whether it repeats earlier prose without a new event, whether it only expands setting terms without concrete action/cost/relationship change, and whether new factions/rules/help arrive without setup or risk. Inspect the final three paragraphs separately: if a natural chapter landing is followed by a short action paragraph that does not complete its action-cause-consequence beat or reach a new landing, treat it as a truncated/spliced body fragment even when its final sentence has terminal punctuation.\n\
         Treat the project/outline authority, current chapter contract, and previous approved state as authority. Check whether the chapter fulfills its own goal without consuming future chapter events, prematurely completing a character arc, changing the established timeline, or violating an explicit period/technology boundary. Explicitly compare every newly asserted protagonist identity, prior history, knowledge source, or ability source against that authority; an unsupported source changes the story premise and is hard contract drift. Track each key object's origin, holder, location, state, and first-acquisition event within this chapter. Do not accept prose that says a character already holds an object and later presents an identically named or described object as a first acquisition unless the prose clearly distinguishes two objects and the contract permits both. The ending must leave the next outline node naturally reachable; a location, task, character state, or causal direction that conflicts with that node is hard contract drift. An unplanned main branch, mystery, or key object that replaces the chapter goal or diverts the story into another task is also a hard error.\n\
         The next outline node is a future exclusion boundary. Its absence and non-mention in the current chapter are correct. Never fail the chapter for not completing or mentioning that future node, and never recommend moving its character, meeting, decision, reveal, conflict, or turn into the current chapter. Only fail when prose has already consumed it early, and then require removal.\n\
         Treat character/proper-noun drift as authoritative only when the deterministic issues list reports it; do not infer protagonist renaming merely because multiple character names appear in the prose.\n\
         Put only contract or continuity conflicts with exact authority and body citations in authority_conflicts. Every item must contain kind, authority_path (JSON Pointer), authority_excerpt, body_excerpt, and message. Titles, summaries, key facts, continuity metadata, aesthetics, wording, pacing, and subjective scores belong only in advisories. Use advisories to note whether the first few paragraphs establish a concrete chapter question; whether major voices are becoming homogeneous or dialogue merely explains the setting; whether summary replaces key action, cost, or relationship movement; whether modifiers obscure action; whether sentence/paragraph rhythm stays monotonous; and whether recent opening or ending forms repeat. These delivery observations must never enter authority_conflicts or demand a rewrite.\n\
         Return JSON only with authority_conflicts(array object), advisories(array string), and score(0-100). Score is telemetry and never decides revision.\n\n\
         Deterministic issues:\n- {deterministic}\n\nContract and continuity authority:\n{authority_context}\n\nProse:\n{content}"
    )
}

pub(super) fn parse_llm_quality_audit_output(raw: &str) -> Option<RawChapterQualityAudit> {
    let cleaned = clean_model_output(raw);
    let json = novel_runner::extract_json(&cleaned)?;
    let mut audit = serde_json::from_str::<RawChapterQualityAudit>(&json).ok()?;
    audit.authority_conflicts.retain(|conflict| {
        !conflict.kind.trim().is_empty()
            && !conflict.authority_path.trim().is_empty()
            && !conflict.authority_excerpt.trim().is_empty()
            && !conflict.body_excerpt.trim().is_empty()
            && !conflict.message.trim().is_empty()
    });
    audit.advisories = chapter_quality::finalize_issues(
        audit
            .advisories
            .into_iter()
            .map(|advisory| advisory.trim().to_string())
            .filter(|advisory| !advisory.is_empty())
            .collect(),
    );
    Some(audit)
}

pub(super) fn local_hard_findings(write_result: &Value) -> Vec<chapter_quality::ChapterFinding> {
    typed_findings_in_value(write_result)
        .into_iter()
        .filter(chapter_quality::ChapterFinding::hard_blocking)
        .collect()
}

pub(super) fn validate_llm_authority_conflict(
    conflict: &RawAuthorityConflict,
    locally_confirmed_findings: &[chapter_quality::ChapterFinding],
    authority_context: &str,
    body: &str,
) -> Option<chapter_quality::ChapterFinding> {
    let code = conflict.kind.trim();
    let local = locally_confirmed_findings
        .iter()
        .find(|finding| finding.code == code && finding.hard_blocking())?;
    let authority: Value = serde_json::from_str(authority_context).ok()?;
    let authority_value = authority.pointer(conflict.authority_path.trim())?;
    let authority_serialized = match authority_value {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).ok()?,
    };
    let authority_excerpt = conflict.authority_excerpt.trim();
    let body_excerpt = conflict.body_excerpt.trim();
    if authority_excerpt.is_empty()
        || body_excerpt.is_empty()
        || !authority_serialized.contains(authority_excerpt)
        || !body.contains(body_excerpt)
    {
        return None;
    }
    let start = body.find(body_excerpt)?;
    let authority_root = authority.get("authority")?;
    Some(chapter_quality::ChapterFinding {
        code: code.to_string(),
        class: local.class,
        disposition: local.disposition,
        evidence_grade: chapter_quality::FindingEvidenceGrade::EvidenceBackedSemantic,
        source: "llm_audit_validated_by_local_finding".to_string(),
        message: conflict.message.trim().to_string(),
        authority_evidence: vec![chapter_quality::AuthorityEvidenceRef {
            path: conflict.authority_path.trim().to_string(),
            excerpt: authority_excerpt.to_string(),
        }],
        body_evidence: vec![chapter_quality::BodyEvidenceSpan {
            start,
            end: start + body_excerpt.len(),
            excerpt: body_excerpt.to_string(),
        }],
        authority_fingerprint: super::super::novel_governance::authority_fingerprint(
            authority_root,
        ),
        body_fingerprint: chapter_quality::chapter_body_fingerprint(body),
    })
}

pub(super) fn json_array_is_empty(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .map(|items| items.is_empty())
        .unwrap_or(true)
}

pub(super) fn audit_status_label(value: &Value) -> String {
    value
        .pointer("/review/verdict")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

pub(super) fn apply_local_revision_suggestions(content: &str, issues: &[String]) -> String {
    let mut repaired = strip_intrusive_ascii_quotes_inside_cjk(content);
    repaired = collapse_duplicate_cjk_before_open_quote(&repaired);
    repaired = repair_line_start_missing_open_bracket_timestamps(&repaired);
    let repair_boundary_punctuation = local_revision_issues_request_boundary_punctuation(issues);
    if repair_boundary_punctuation {
        repaired = repair_cjk_quote_and_action_boundary_punctuation(&repaired);
    }
    if local_revision_issues_request_paragraph_breaks(issues) {
        repaired = repair_overlong_cjk_paragraphs(&repaired);
    }
    for _ in 0..2 {
        let before = text_fingerprint(&repaired);
        for issue in issues {
            if let Some(locally_repaired) = reduce_overused_cjk_story_descriptor(&repaired, issue) {
                repaired = locally_repaired;
            }
            if let Some(locally_repaired) = reduce_overused_cjk_rhetorical_marker(&repaired, issue)
            {
                repaired = locally_repaired;
            }
            let repair_pairs = local_text_repair_pairs(issue);
            for (source, target) in repair_pairs {
                repaired = apply_local_text_repair_pair(&repaired, &source, &target);
            }
        }
        if repair_boundary_punctuation {
            repaired = repair_cjk_quote_and_action_boundary_punctuation(&repaired);
        }
        repaired = repair_line_start_missing_open_bracket_timestamps(&repaired);
        if local_revision_issues_request_paragraph_breaks(issues) {
            repaired = repair_overlong_cjk_paragraphs(&repaired);
        }
        if text_fingerprint(&repaired) == before {
            break;
        }
    }
    repaired
}

fn repair_line_start_missing_open_bracket_timestamps(content: &str) -> String {
    let mut changed = false;
    let mut out = String::with_capacity(content.len());
    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        let indent_len = line
            .char_indices()
            .find(|(_, ch)| !ch.is_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(line.len());
        let (indent, rest) = line.split_at(indent_len);
        if line_start_looks_like_missing_open_bracket_timestamp(rest) {
            out.push_str(indent);
            out.push('[');
            out.push_str(rest);
            changed = true;
        } else {
            out.push_str(line);
        }
        out.push_str(newline);
    }
    if changed {
        out
    } else {
        content.to_string()
    }
}

fn line_start_looks_like_missing_open_bracket_timestamp(rest: &str) -> bool {
    if rest.starts_with('[') {
        return false;
    }
    let mut chars = rest.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(':'))
        && matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(':'))
        && matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
        && matches!(chars.next(), Some(']'))
}

fn local_revision_issues_request_boundary_punctuation(issues: &[String]) -> bool {
    issues.iter().any(|issue| {
        let compact = issue
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        [
            "标点",
            "句号",
            "缺少句",
            "断句",
            "拼接",
            "黏连",
            "粘连",
            "punctuation",
            "sentenceboundary",
            "missingperiod",
            "malformedphrasenearstablecharacteranchor",
            "malformedstructuralphrase",
            "danglingconnectorphrase",
            "action-object-partboundary",
        ]
        .iter()
        .any(|marker| compact.to_ascii_lowercase().contains(marker) || compact.contains(marker))
    })
}

fn local_revision_issues_request_paragraph_breaks(issues: &[String]) -> bool {
    issues.iter().any(|issue| {
        let compact = issue
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        [
            "段落结构异常",
            "缺失换行",
            "缺少换行",
            "换行缺失",
            "段落粘连",
            "段落黏连",
            "阅读体验割裂",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
    })
}

fn repair_overlong_cjk_paragraphs(content: &str) -> String {
    const TARGET_PARAGRAPH_CHARS: usize = 180;
    const MIN_SPLIT_CHARS: usize = 120;
    content
        .split('\n')
        .map(|line| {
            let total_chars = line.chars().count();
            if line.trim().is_empty() || total_chars <= TARGET_PARAGRAPH_CHARS {
                return line.to_string();
            }
            let mut out = String::with_capacity(line.len() + line.len() / 80);
            let mut current_len = 0usize;
            let mut consumed = 0usize;
            for ch in line.chars() {
                out.push(ch);
                current_len += 1;
                consumed += 1;
                if current_len >= MIN_SPLIT_CHARS
                    && cjk_sentence_boundary(ch)
                    && total_chars.saturating_sub(consumed) > MIN_SPLIT_CHARS / 2
                {
                    out.push('\n');
                    current_len = 0;
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cjk_sentence_boundary(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '；' | '!' | '?' | ';')
}

fn repair_cjk_quote_and_action_boundary_punctuation(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let line = repair_cjk_dialogue_quote_terminal(line);
            repair_cjk_action_subject_boundary(&line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn repair_cjk_dialogue_quote_terminal(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.len() < 4 {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '”'
            && index > 0
            && index + 1 < chars.len()
            && is_cjk_char(chars[index - 1])
            && !cjk_dialogue_terminal(chars[index - 1])
            && cjk_quote_followed_by_speaker_attribution(&chars[index + 1..])
        {
            out.push('。');
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn cjk_quote_followed_by_speaker_attribution(rest: &[char]) -> bool {
    let compact = rest
        .iter()
        .copied()
        .filter(|ch| !ch.is_whitespace())
        .take(12)
        .collect::<String>();
    if compact.chars().filter(|ch| is_cjk_char(*ch)).count() < 4 {
        return false;
    }
    cjk_speech_or_reaction_markers()
        .iter()
        .any(|marker| compact.contains(marker))
}

fn repair_cjk_action_subject_boundary(line: &str) -> String {
    let mut repaired = line.to_string();
    for marker in cjk_action_subject_boundary_markers() {
        repaired = repair_cjk_action_subject_boundary_for_marker(&repaired, marker);
    }
    crate::tool::writing::surface_sanitizer::repair_cjk_action_object_part_boundaries(&repaired)
}

fn repair_cjk_action_subject_boundary_for_marker(line: &str, marker: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let marker_chars = marker.chars().collect::<Vec<_>>();
    if chars.len() < marker_chars.len() + 4 {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < chars.len() {
        if slice_starts_with(&chars[index..], &marker_chars) {
            if let Some(name_start) = cjk_subject_name_start_before_marker(&chars, index) {
                if name_start > 0
                    && is_cjk_char(chars[name_start - 1])
                    && !cjk_dialogue_terminal(chars[name_start - 1])
                {
                    out.extend(chars[..name_start].iter());
                    out.push('。');
                    out.extend(chars[name_start..].iter());
                    return out;
                }
            }
        }
        index += 1;
    }
    line.to_string()
}

fn cjk_subject_name_start_before_marker(chars: &[char], marker_start: usize) -> Option<usize> {
    for len in (2..=4).rev() {
        if marker_start < len {
            continue;
        }
        let start = marker_start - len;
        if !chars[start..marker_start].iter().copied().all(is_cjk_char) {
            continue;
        }
        if start > 0 && cjk_predicate_before_subject_boundary(chars[start - 1]) {
            return Some(start);
        }
    }
    None
}

fn cjk_predicate_before_subject_boundary(ch: char) -> bool {
    matches!(
        ch,
        '走' | '去'
            | '来'
            | '回'
            | '出'
            | '入'
            | '进'
            | '退'
            | '停'
            | '立'
            | '坐'
            | '落'
            | '下'
            | '开'
    )
}

fn slice_starts_with(slice: &[char], prefix: &[char]) -> bool {
    slice.len() >= prefix.len() && slice[..prefix.len()] == *prefix
}

fn cjk_dialogue_terminal(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '；' | '…')
}

fn cjk_speech_or_reaction_markers() -> &'static [&'static str] {
    &[
        "说道", "说", "问道", "问", "低声", "沉声", "轻声", "冷笑", "点头", "摇头", "抬头", "转身",
    ]
}

fn cjk_action_subject_boundary_markers() -> &'static [&'static str] {
    &[
        "紧随其后",
        "抬起头",
        "转过身",
        "转身",
        "点了点头",
        "摇了摇头",
        "低声说道",
        "沉声说道",
        "轻声说道",
    ]
}

fn reduce_overused_cjk_rhetorical_marker(content: &str, issue: &str) -> Option<String> {
    let lowered = issue.to_ascii_lowercase();
    if !lowered.contains("overuses the same rhetorical marker")
        && !issue.contains("频繁重复使用")
        && !issue.contains("修辞")
    {
        return None;
    }
    let marker = quoted_segments(issue)
        .into_iter()
        .find(|term| cjk_rhetorical_marker(term))?;
    let occurrences = content.match_indices(&marker).collect::<Vec<_>>();
    if occurrences.len() < 8 {
        return None;
    }

    let replacements = cjk_rhetorical_marker_replacements(&marker);
    let keep_first = 4usize;
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    let mut replaced = 0usize;
    for (index, (start, _)) in occurrences.iter().enumerate() {
        out.push_str(&content[cursor..*start]);
        if index < keep_first {
            out.push_str(&marker);
        } else {
            let replacement = replacements[(index - keep_first) % replacements.len()];
            out.push_str(replacement);
            replaced += 1;
        }
        cursor = start + marker.len();
    }
    out.push_str(&content[cursor..]);
    (replaced > 0 && out != content).then_some(out)
}

fn cjk_rhetorical_marker(value: &str) -> bool {
    matches!(value.trim(), "仿佛" | "似乎" | "好像" | "宛如" | "像是")
}

fn cjk_rhetorical_marker_replacements(marker: &str) -> &'static [&'static str] {
    match marker {
        "仿佛" => &["好似", "犹如", "几乎", "近乎", "隐约"],
        "似乎" => &["好似", "仿若", "隐约", "几乎", "近乎"],
        "好像" => &["仿若", "犹如", "似有", "近乎", "隐约"],
        "宛如" => &["好似", "仿若", "犹如", "近乎", "隐约"],
        "像是" => &["仿若", "好似", "犹如", "近乎", "隐约"],
        _ => &["好似", "仿若", "近乎"],
    }
}

fn reduce_overused_cjk_story_descriptor(content: &str, issue: &str) -> Option<String> {
    let lowered = issue.to_ascii_lowercase();
    if !lowered.contains("overuses the same story term") {
        return None;
    }
    let term = quoted_segments(issue).into_iter().find(|term| {
        let len = term.chars().count();
        (2..=8).contains(&len) && term.chars().all(is_cjk_char)
    })?;
    let replacements = cjk_story_descriptor_replacements(&term)?;
    let occurrences = content.match_indices(&term).collect::<Vec<_>>();
    if occurrences.len() < 8 {
        return None;
    }

    let keep_first = 4usize;
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    let mut replaced = 0usize;
    for (index, (start, _)) in occurrences.iter().enumerate() {
        out.push_str(&content[cursor..*start]);
        if index < keep_first {
            out.push_str(&term);
        } else {
            let replacement = replacements[(index - keep_first) % replacements.len()];
            out.push_str(replacement);
            replaced += 1;
        }
        cursor = start + term.len();
    }
    out.push_str(&content[cursor..]);
    (replaced > 0 && out != content).then_some(out)
}

fn cjk_story_descriptor_replacements(term: &str) -> Option<&'static [&'static str]> {
    const MALE_OR_NEUTRAL: &[&str] = &["他", "对方", "那人", "那道身影"];
    const FEMALE: &[&str] = &["她", "对方", "那人", "那名女子"];
    const NEUTRAL_ENTITY: &[&str] = &["它", "这股力量", "这份线索", "这个存在"];

    if term.ends_with("女子")
        || term.ends_with("少女")
        || term.ends_with("女人")
        || term.ends_with("女修")
        || term.ends_with("姑娘")
        || term.ends_with("女郎")
        || term.ends_with("女弟子")
    {
        return Some(FEMALE);
    }
    if term.ends_with("男子")
        || term.ends_with("少年")
        || term.ends_with("男人")
        || term.ends_with("修士")
        || term.ends_with("剑客")
        || term.ends_with("老者")
        || term.ends_with("老人")
        || term.ends_with("道人")
        || term.ends_with("身影")
        || term.ends_with("黑影")
    {
        return Some(MALE_OR_NEUTRAL);
    }
    if term.ends_with("力量")
        || term.ends_with("线索")
        || term.ends_with("证据")
        || term.ends_with("意志")
        || term.ends_with("气息")
        || term.ends_with("规则")
    {
        return Some(NEUTRAL_ENTITY);
    }
    None
}

pub(super) fn deterministic_cleanup_issues_are_stale_after_local_repair(
    content: &str,
    issues: &[String],
) -> bool {
    if issues.is_empty() {
        return false;
    }
    let mut saw_repair_pair = false;
    for issue in issues {
        for (source, target) in local_text_repair_pairs(issue) {
            saw_repair_pair = true;
            if apply_local_text_repair_pair(content, &source, &target) != content {
                return false;
            }
        }
    }
    saw_repair_pair
}

pub(super) fn local_text_repair_pairs(issue: &str) -> Vec<(String, String)> {
    if let Some(pair) = local_cjk_orthography_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = local_cjk_surface_noise_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = local_malformed_lexical_glue_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = local_adjacent_stable_anchor_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = local_malformed_anchor_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = local_missing_character_fragment_repair_pair(issue) {
        return vec![pair];
    }
    if issue.contains('；') || issue.contains(';') {
        let pairs = issue
            .split(['；', ';'])
            .flat_map(local_text_repair_pairs)
            .collect::<Vec<_>>();
        if !pairs.is_empty() {
            return pairs;
        }
    }
    if !(issue_contains_local_repair_marker(issue)
        || issue.contains("误写")
        || issue.contains("漏掉")
        || issue.contains("错字")
        || issue.contains("错别字")
        || issue.contains("多了一个")
        || issue.contains("多出一个")
        || issue.contains("多余")
        || issue.contains("词语拼接")
        || issue_contains_cjk_orthography_marker(issue)
        || issue.contains("角色名称漂移")
        || issue.contains("角色名漂移")
        || issue.contains("角色名一致性")
        || issue.contains("角色名称一致性")
        || issue.contains("人物名称漂移")
        || (issue.contains("人物名字") && issue.contains("混用"))
        || issue.contains("人物名称不一致")
        || issue.contains("名称不一致")
        || issue.contains("与合同")
        || issue.contains("不符")
        || issue.to_ascii_lowercase().contains("character name drift")
        || issue
            .to_ascii_lowercase()
            .contains("stable contract character"))
    {
        return Vec::new();
    }
    let quoted = quoted_segments(issue);
    if let Some(pair) = duplicated_cjk_character_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    if let Some(pair) = extra_cjk_character_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    if let Some(pair) = punctuated_bridge_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    if let Some(pair) = single_cjk_character_in_phrase_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    if let Some(pair) = embedded_token_should_be_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    if let Some(pair) = overlapping_clause_glue_repair_pair(issue) {
        return vec![pair];
    }
    if let Some(pair) = explicit_should_be_repair_pair(issue) {
        return vec![pair];
    }
    if quoted.len() < 2 {
        return Vec::new();
    }
    if let Some(pair) = local_character_drift_repair_pair(issue, &quoted) {
        return vec![pair];
    }
    let source = quoted[0].trim();
    if let Some(pair) = local_missing_suffix_repair_pair(issue, source, &quoted) {
        return vec![pair];
    }
    let targets_after_marker = split_once_local_repair_marker(issue)
        .map(|(_, after)| repair_target_segments_after_marker(after))
        .unwrap_or_default();
    let fallback_targets = quoted.iter().skip(1).cloned().collect::<Vec<_>>();
    let target_candidates = if targets_after_marker.is_empty() {
        fallback_targets
    } else {
        targets_after_marker
    };
    let mut pairs = Vec::new();
    for target in target_candidates {
        let target = target.trim();
        if !local_repair_term_is_safe(source) || !local_repair_term_is_safe(target) {
            continue;
        }
        if let Some(source_token) = local_short_token_repair_source(source, target) {
            pairs.push((source_token, target.to_string()));
            break;
        }
        if local_repair_term_is_safe(source) && local_repair_term_is_safe(target) {
            pairs.push((source.to_string(), target.to_string()));
            break;
        }
    }
    pairs
}

fn overlapping_clause_glue_repair_pair(issue: &str) -> Option<(String, String)> {
    if !(issue.contains("句式杂糅") || issue.contains("语义重复") || issue.contains("词语拼接"))
    {
        return None;
    }
    let (before_marker, after_marker) = split_once_local_repair_marker(issue)?;
    let source = quoted_segments(before_marker).into_iter().next()?;
    let targets = repair_target_segments_after_marker(after_marker)
        .into_iter()
        .filter_map(|target| normalize_local_repair_target(&source, &target))
        .collect::<Vec<_>>();

    for left in &targets {
        for right in &targets {
            if left == right {
                continue;
            }
            let left_chars = left.chars().collect::<Vec<_>>();
            let right_chars = right.chars().collect::<Vec<_>>();
            let max_overlap = left_chars.len().min(right_chars.len()).min(4);
            for overlap in (1..=max_overlap).rev() {
                if left_chars[left_chars.len() - overlap..] != right_chars[..overlap] {
                    continue;
                }
                let glued = left_chars
                    .iter()
                    .chain(right_chars[overlap..].iter())
                    .collect::<String>();
                if !source.contains(&glued) {
                    continue;
                }
                let repaired = format!("{left}，{right}");
                if local_repair_term_is_safe(&glued)
                    && local_repair_term_is_safe(&repaired)
                    && glued != repaired
                {
                    return Some((glued, repaired));
                }
            }
        }
    }
    None
}

fn local_cjk_orthography_repair_pair(issue: &str) -> Option<(String, String)> {
    if !issue_contains_cjk_orthography_marker(issue) {
        return None;
    }
    quoted_segments(issue)
        .into_iter()
        .filter(|segment| segment.chars().count() > 1)
        .find_map(|source| {
            let target = normalize_common_cjk_orthography(&source);
            (target != source
                && local_repair_term_is_safe(&source)
                && local_repair_term_is_safe(&target))
            .then_some((source, target))
        })
        .or_else(|| {
            quoted_segments(issue)
                .into_iter()
                .filter(|segment| segment.chars().count() == 1)
                .find_map(|source| {
                    let target = normalize_common_cjk_orthography(&source);
                    (target != source
                        && local_repair_term_is_safe(&source)
                        && local_repair_term_is_safe(&target))
                    .then_some((source, target))
                })
        })
}

fn issue_contains_cjk_orthography_marker(issue: &str) -> bool {
    issue.contains("繁体")
        || issue.contains("繁體")
        || issue.contains("简繁")
        || issue.contains("簡繁")
        || issue.contains("繁简")
        || issue.contains("繁簡")
}

fn normalize_common_cjk_orthography(value: &str) -> String {
    value
        .chars()
        .map(common_traditional_to_simplified_char)
        .collect()
}

fn common_traditional_to_simplified_char(ch: char) -> char {
    match ch {
        '區' => '区',
        '開' => '开',
        '關' => '关',
        '門' => '门',
        '體' => '体',
        '靈' => '灵',
        '線' => '线',
        '網' => '网',
        '風' => '风',
        '雲' => '云',
        '龍' => '龙',
        '無' => '无',
        '為' => '为',
        '來' => '来',
        '對' => '对',
        '過' => '过',
        '還' => '还',
        '說' => '说',
        '時' => '时',
        '點' => '点',
        '聲' => '声',
        '會' => '会',
        '認' => '认',
        '見' => '见',
        '戰' => '战',
        '畫' => '画',
        _ => ch,
    }
}

fn local_short_token_repair_source(source: &str, target: &str) -> Option<String> {
    let target_chars = target.chars().collect::<Vec<_>>();
    let target_len = target_chars.len();
    if !(2..=6).contains(&target_len) || source.chars().count() <= target_len {
        return None;
    }
    if source.contains(target) {
        return None;
    }
    let source_chars = source.chars().collect::<Vec<_>>();
    if target_len > 2 {
        let inserted = source_chars
            .windows(target_len - 1)
            .map(|window| window.iter().collect::<String>())
            .find(|candidate| target_single_char_insertion_matches(candidate, target))
            .filter(|candidate| candidate != target);
        if inserted.is_some() {
            return inserted;
        }
    }
    let replacement = source_chars
        .windows(target_len)
        .map(|window| {
            let distance = window
                .iter()
                .zip(target_chars.iter())
                .filter(|(left, right)| left != right)
                .count();
            let prefix_match = window.first() == target_chars.first();
            (window.iter().collect::<String>(), distance, prefix_match)
        })
        .filter(|(_, distance, prefix_match)| *distance <= 1 || (*prefix_match && *distance <= 2))
        .min_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0))
        })
        .map(|(candidate, _, _)| candidate)
        .filter(|candidate| candidate != target);
    if replacement.is_some() {
        return replacement;
    }
    None
}

fn target_single_char_insertion_matches(source: &str, target: &str) -> bool {
    let source_chars = source.chars().collect::<Vec<_>>();
    let target_chars = target.chars().collect::<Vec<_>>();
    if target_chars.len() != source_chars.len() + 1 {
        return false;
    }
    (0..target_chars.len()).any(|skip| {
        target_chars
            .iter()
            .enumerate()
            .filter_map(|(index, ch)| (index != skip).then_some(*ch))
            .eq(source_chars.iter().copied())
    })
}

fn extra_cjk_character_repair_pair(issue: &str, quoted: &[String]) -> Option<(String, String)> {
    if !(issue.contains("多出一个") || issue.contains("多了一个") || issue.contains("多余"))
    {
        return None;
    }
    let extra = quoted
        .iter()
        .find_map(|segment| single_cjk_char_segment(segment))
        .or_else(|| cjk_char_after_extra_marker(issue))?;
    quoted
        .iter()
        .filter(|segment| segment.chars().count() > 1)
        .find_map(|source| remove_one_adjacent_duplicate_char(source, extra))
        .map(|target| {
            let source = quoted
                .iter()
                .find(|segment| {
                    segment.chars().count() > 1
                        && remove_one_adjacent_duplicate_char(segment, extra).as_ref()
                            == Some(&target)
                })
                .cloned()
                .unwrap_or_default();
            (source, target)
        })
        .filter(|(source, target)| {
            !source.is_empty()
                && source != target
                && local_repair_term_is_safe(source)
                && local_repair_term_is_safe(target)
        })
}

fn single_cjk_char_segment(segment: &str) -> Option<char> {
    let mut chars = segment.trim().chars();
    let ch = chars.next()?;
    if chars.next().is_none() && is_cjk_char(ch) {
        Some(ch)
    } else {
        None
    }
}

fn cjk_char_after_extra_marker(issue: &str) -> Option<char> {
    ["多出一个", "多了一个", "多余"].iter().find_map(|marker| {
        let (_, after) = issue.split_once(marker)?;
        after.chars().find(|ch| is_cjk_char(*ch))
    })
}

fn remove_one_adjacent_duplicate_char(source: &str, extra: char) -> Option<String> {
    let chars = source.chars().collect::<Vec<_>>();
    let duplicate_index = chars
        .windows(2)
        .position(|window| window[0] == extra && window[1] == extra)?;
    let mut out = String::with_capacity(source.len());
    for (index, ch) in chars.into_iter().enumerate() {
        if index == duplicate_index {
            continue;
        }
        out.push(ch);
    }
    Some(out)
}

fn local_cjk_surface_noise_repair_pair(issue: &str) -> Option<(String, String)> {
    if let Some(pair) = explicit_cjk_surface_noise_repair_pair(issue) {
        return Some(pair);
    }
    let source = issue
        .rsplit_once("repeatedcharacterinsertion:")
        .map(|(_, source)| source.trim())
        .or_else(|| {
            issue
                .rsplit_once("repeated character insertion:")
                .map(|(_, source)| source.trim())
        })?;
    local_cjk_surface_noise_repair_target(source).map(|target| (source.to_string(), target))
}

fn local_malformed_lexical_glue_repair_pair(issue: &str) -> Option<(String, String)> {
    let lowered = issue.to_ascii_lowercase();
    let looks_like_lexical_glue = lowered.contains("malformed lexical glue phrase")
        || issue.contains("词汇粘")
        || issue.contains("词语粘");
    if !looks_like_lexical_glue {
        return None;
    }
    let source = quoted_segments(issue)
        .into_iter()
        .chain([issue.to_string()])
        .find_map(|segment| {
            let compact = segment
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            lexical_glue_source_fragment(&compact)
        })?;
    lexical_glue_repair_target(&source).map(|target| (source, target))
}

fn lexical_glue_source_fragment(value: &str) -> Option<String> {
    ["香烟雾", "材质地"]
        .iter()
        .find(|needle| value.contains(**needle))
        .map(|needle| (*needle).to_string())
}

fn lexical_glue_repair_target(source: &str) -> Option<String> {
    match source {
        "香烟雾" => Some("香烟，烟雾".to_string()),
        "材质地" => Some("材质，地".to_string()),
        _ => None,
    }
}

fn explicit_cjk_surface_noise_repair_pair(issue: &str) -> Option<(String, String)> {
    let issue_compact = issue
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let has_surface_marker = [
        "明显错字",
        "错别字",
        "漏字",
        "缺字",
        "乱码",
        "残片",
        "ocr",
        "mojibake",
    ]
    .iter()
    .any(|marker| {
        issue_compact.to_ascii_lowercase().contains(marker) || issue_compact.contains(marker)
    });
    if !has_surface_marker || !issue_contains_local_repair_marker(issue) {
        return None;
    }
    let quoted = quoted_segments(issue);
    let source = quoted
        .iter()
        .find(|segment| cjk_surface_noise_source_is_safe(segment))?;
    let (_, after_marker) = split_once_local_repair_marker(issue)?;
    let target = repair_target_segments_after_marker(after_marker)
        .into_iter()
        .find(|target| cjk_surface_noise_target_is_safe(source, target))?;
    let (source, target) = normalized_local_repair_pair(source, &target)?;
    let source = local_short_token_repair_source(&source, &target).unwrap_or(source);
    Some((source, target))
}

fn cjk_surface_noise_source_is_safe(value: &str) -> bool {
    let len = value.chars().count();
    (2..=16).contains(&len)
        && value.chars().any(is_cjk_char)
        && value.chars().any(|ch| ch.is_ascii_alphanumeric())
        && value.chars().all(|ch| {
            is_cjk_char(ch) || ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' ')
        })
}

fn cjk_surface_noise_target_is_safe(source: &str, target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() || !target.chars().any(is_cjk_char) {
        return false;
    }
    let source_len = source.chars().count();
    let target_len = target.chars().count();
    target_len <= source_len.saturating_add(4)
        && target_len >= source_len.saturating_sub(4)
        && target.chars().all(|ch| {
            is_cjk_char(ch) || ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ' ')
        })
}

fn local_cjk_surface_noise_repair_target(source: &str) -> Option<String> {
    let source = source.trim();
    if !local_repair_term_is_safe(source) {
        return None;
    }
    let collapsed = surface_sanitizer::collapse_excessive_repeated_cjk_chars(source);
    if collapsed != source {
        return Some(collapsed);
    }
    let target = if let Some(prefix) = source.strip_suffix("有直接回答") {
        format!("{prefix}没有直接回答")
    } else if let Some(prefix) = source.strip_suffix("有回答") {
        format!("{prefix}没有回答")
    } else if let Some(prefix) = source.strip_suffix("声说道") {
        format!("{prefix}低声说道")
    } else if let Some(prefix) = source.strip_suffix("声说") {
        format!("{prefix}低声说")
    } else if let Some(prefix) = source.strip_suffix("了点头") {
        format!("{prefix}点了点头")
    } else {
        return None;
    };
    if target == source || !local_repair_term_is_safe(&target) {
        return None;
    }
    Some(target)
}

fn local_missing_character_fragment_repair_pair(issue: &str) -> Option<(String, String)> {
    let marker = local_missing_character_fragment_marker(issue)?;
    let marker = marker
        .trim_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
        .trim_matches(['"', '\'', '“', '”', '‘', '’', '。', '，', '：', ':']);
    let target = match marker {
        "为什" => "为什么".to_string(),
        "什都" => "什么都".to_string(),
        "什东西" => "什么东西".to_string(),
        "什代价" => "什么代价".to_string(),
        "什的" => "什么的".to_string(),
        "什地" => "什么地".to_string(),
        "正静地" => "正静静地".to_string(),
        "正静的" => "正静静的".to_string(),
        "悄蔓延" => "悄然蔓延".to_string(),
        "悄扩散" => "悄然扩散".to_string(),
        "悄靠近" => "悄然靠近".to_string(),
        "突直跳" => "突突直跳".to_string(),
        "喃自语" => "喃喃自语".to_string(),
        "地回头" => "猛地回头".to_string(),
        "地甩头" => "猛地甩头".to_string(),
        _ if cjk_standalone_shen_fragment(marker) => {
            format!("什么{}", &marker['什'.len_utf8()..])
        }
        _ => return None,
    };
    Some((marker.to_string(), target))
}

fn local_missing_character_fragment_marker(issue: &str) -> Option<&str> {
    let normalized = issue.to_ascii_lowercase();
    let markers = [
        "missing-character fragment:",
        "missing-characterfragment:",
        "missingcharacterfragment:",
    ];
    for marker in markers {
        if let Some(index) = normalized.find(marker) {
            let start = index + marker.len();
            return Some(issue.get(start..)?.trim());
        }
    }
    issue
        .split_once("缺字残片：")
        .map(|(_, marker)| marker.trim())
}

fn cjk_standalone_shen_fragment(marker: &str) -> bool {
    let mut chars = marker.chars();
    if chars.next() != Some('什') {
        return false;
    }
    let Some(next) = chars.next() else {
        return false;
    };
    !matches!(next, '么' | '錦' | '锦') && is_cjk_char(next)
}

pub(super) fn embedded_token_should_be_repair_pair(
    issue: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    if !(issue.contains("中的") && issue.contains("应为")) || quoted.len() < 3 {
        return None;
    }
    let source = quoted.first()?.trim();
    let wrong = quoted.get(1)?.trim();
    let mut best: Option<(usize, String)> = None;
    for target in quoted.iter().skip(2) {
        let Some((_, target)) = normalized_local_repair_pair(wrong, target) else {
            continue;
        };
        if target == wrong || target.contains('/') || target.contains('／') {
            continue;
        }
        let repaired = if source.contains(wrong) {
            source.replacen(wrong, &target, 1)
        } else {
            continue;
        };
        if repaired == source || !local_repair_term_is_safe(&repaired) {
            continue;
        }
        let score = local_repair_target_score(source, &repaired);
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, repaired));
        }
    }
    let (_, target) = best?;
    if local_repair_term_is_safe(source) {
        Some((source.to_string(), target))
    } else {
        None
    }
}

pub(super) fn explicit_should_be_repair_pair(issue: &str) -> Option<(String, String)> {
    let (before_marker, after_marker) = split_once_local_repair_marker(issue)?;
    let source = quoted_segments(before_marker).into_iter().next()?;
    let mut best: Option<(usize, String, String)> = None;
    for target in repair_target_segments_after_marker(after_marker) {
        let target = normalize_local_repair_target(&source, &target)?;
        let source = local_repair_source_near_target(&source, &target)
            .unwrap_or_else(|| source.trim().to_string());
        let Some((source, target)) = normalized_local_repair_pair(&source, &target) else {
            continue;
        };
        let source = local_short_token_repair_source(&source, &target).unwrap_or(source);
        let score = local_repair_target_score(&source, &target);
        if best
            .as_ref()
            .is_none_or(|(best_score, _, _)| score > *best_score)
        {
            best = Some((score, source, target));
        }
    }
    let (_, source, target) = best?;
    Some((source, target))
}

fn local_repair_source_near_target(source: &str, target: &str) -> Option<String> {
    if local_repair_term_is_safe(source) {
        return Some(source.trim().to_string());
    }
    let target_first = target.chars().find(|ch| is_cjk_char(*ch))?;
    source
        .split(['。', '！', '？', '；', ';', '!', '?', '\n'])
        .map(str::trim)
        .filter(|candidate| local_repair_term_is_safe(candidate))
        .filter(|candidate| candidate.chars().find(|ch| is_cjk_char(*ch)) == Some(target_first))
        .max_by_key(|candidate| local_repair_target_score(candidate, target))
        .map(ToString::to_string)
}

fn issue_contains_local_repair_marker(issue: &str) -> bool {
    local_repair_markers()
        .iter()
        .any(|marker| issue.contains(marker))
}

fn split_once_local_repair_marker(issue: &str) -> Option<(&str, &str)> {
    local_repair_markers()
        .iter()
        .find_map(|marker| issue.split_once(marker))
}

fn local_repair_markers() -> [&'static str; 5] {
    ["应为", "应改为", "可改为", "建议改为", "建议调整为"]
}

fn single_cjk_character_in_phrase_repair_pair(
    issue: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    if !issue.contains("字应为") && !issue.contains("字应改为") {
        return None;
    }
    let (before_marker, after_marker) = issue
        .split_once("应为")
        .or_else(|| issue.split_once("应改为"))?;
    let source_char = quoted_segments(before_marker)
        .into_iter()
        .rev()
        .find(|candidate| candidate.chars().count() == 1 && candidate.chars().all(is_cjk_char))?;
    let target_char = repair_target_segments_after_marker(after_marker)
        .into_iter()
        .find(|candidate| candidate.chars().count() == 1 && candidate.chars().all(is_cjk_char))?;
    let source_phrase = quoted
        .iter()
        .find(|candidate| candidate.chars().count() > 1 && candidate.contains(&source_char))?;
    let target_phrase = source_phrase.replacen(&source_char, &target_char, 1);
    if target_phrase == *source_phrase
        || !local_repair_term_is_safe(source_phrase)
        || !local_repair_term_is_safe(&target_phrase)
    {
        return None;
    }
    Some((source_phrase.clone(), target_phrase))
}

fn duplicated_cjk_character_repair_pair(
    issue: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    let compact = issue
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if !(compact.contains("错别字")
        || compact.contains("笔误")
        || compact.contains("重复字符")
        || compact.contains("重复字")
        || compact.contains("多了一个")
        || compact.contains("多出一个")
        || compact.contains("多余"))
    {
        return None;
    }
    let source = quoted
        .iter()
        .find(|candidate| candidate.chars().count() >= 2 && candidate.chars().any(is_cjk_char))?;
    let chars = source.chars().collect::<Vec<_>>();
    let mut best = None;
    for index in 1..chars.len() {
        if chars[index] != chars[index - 1] || !is_cjk_char(chars[index]) {
            continue;
        }
        let mut repaired = String::with_capacity(source.len());
        for (pos, ch) in chars.iter().enumerate() {
            if pos != index {
                repaired.push(*ch);
            }
        }
        if repaired != *source
            && local_repair_term_is_safe(source)
            && local_repair_term_is_safe(&repaired)
        {
            best = Some((source.clone(), repaired));
            break;
        }
    }
    best
}

pub(super) fn local_repair_target_score(source: &str, target: &str) -> usize {
    let source_len = source.chars().count();
    let target_len = target.chars().count();
    let source_first = source.chars().find(|ch| is_cjk_char(*ch));
    let same_start = source_first.is_some_and(|ch| target.starts_with(ch));
    usize::from(same_start) * 100 + target_len.min(source_len)
}

pub(super) fn normalized_local_repair_pair(source: &str, target: &str) -> Option<(String, String)> {
    let source = source.trim();
    let target = normalize_local_repair_target(source, target)?;
    if source == target || !local_repair_term_is_safe(source) || !local_repair_term_is_safe(&target)
    {
        return None;
    }
    Some((source.to_string(), target))
}

pub(super) fn normalize_local_repair_target(source: &str, target: &str) -> Option<String> {
    let trimmed = target.trim().trim_matches(['。', '，', ',', '.']);
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('/') || trimmed.contains('／') {
        return None;
    }
    if trimmed.contains("后接") || trimmed.contains("补足") || trimmed.contains("加上") {
        return None;
    }
    let source_first = source.chars().find(|ch| is_cjk_char(*ch));
    let source_len = source.chars().count();
    let candidates = trimmed
        .split('或')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty() && candidate.chars().any(is_cjk_char))
        .filter_map(normalize_local_repair_candidate_surface)
        .collect::<Vec<_>>();
    let primary = candidates
        .iter()
        .copied()
        .find(|candidate| {
            let candidate_len = candidate.chars().count();
            source_first.is_some_and(|ch| candidate.starts_with(ch))
                && (candidate_len + 4 >= source_len || source_len <= 8)
        })
        .or_else(|| candidates.first().copied())?;
    let primary = primary
        .trim_matches(['(', ')', '（', '）', '[', ']', '【', '】'])
        .trim();
    if primary.is_empty() {
        None
    } else {
        Some(primary.to_string())
    }
}

fn normalize_local_repair_candidate_surface(candidate: &str) -> Option<&str> {
    let candidate = candidate
        .trim()
        .trim_matches(['。', '，', ',', '.', '：', ':', ';', '；']);
    let candidate = candidate
        .split("...")
        .next()
        .unwrap_or(candidate)
        .split('…')
        .next()
        .unwrap_or(candidate)
        .trim();
    if candidate.is_empty() || !candidate.chars().any(is_cjk_char) {
        None
    } else {
        Some(candidate)
    }
}

pub(super) fn punctuated_bridge_repair_pair(
    issue: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    if !(issue.contains("应为")
        || issue.contains("缺少标点")
        || issue.contains("缺少逗号")
        || issue.contains("句式杂糅")
        || issue.contains("词语拼接")
        || issue.contains("粘连")
        || issue.contains("黏连"))
    {
        return None;
    }
    let source = quoted.first()?.trim();
    if !local_repair_term_is_safe(source) {
        return None;
    }
    if source.contains('，') || source.contains(',') {
        return None;
    }
    let candidate = quoted
        .iter()
        .skip(1)
        .filter_map(|candidate| normalize_local_repair_candidate_surface(candidate))
        .find(|candidate| candidate.contains('，') || candidate.contains(','))?;
    let delimiter = if candidate.contains('，') {
        '，'
    } else {
        ','
    };
    let (left, right) = candidate.split_once(delimiter)?;
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let collapsed = collapsed_cjk_bridge(left, right)?;
    let source_tail = source
        .split_once(&collapsed)
        .map(|(_, tail)| leading_cjk_tail(tail, 4))
        .unwrap_or_default();
    let target = if source_tail.is_empty() || candidate.ends_with(&source_tail) {
        candidate.to_string()
    } else {
        format!("{candidate}{source_tail}")
    };
    if !local_repair_term_is_safe(&collapsed) || !local_repair_term_is_safe(&target) {
        return None;
    }
    if let Some(last_tail) = source_tail.chars().last() {
        let observed_truncated = format!("{collapsed}{last_tail}");
        if observed_truncated != collapsed && local_repair_term_is_safe(&observed_truncated) {
            return Some((observed_truncated, target));
        }
    }
    Some((collapsed, target))
}

fn collapsed_cjk_bridge(left: &str, right: &str) -> Option<String> {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let left_last = left.chars().last();
    let mut right_chars = right.chars();
    let right_first = right_chars.next();
    let collapsed = if left_last.is_some() && left_last == right_first {
        format!("{left}{}", right_chars.collect::<String>())
    } else {
        format!("{left}{right}")
    };
    if collapsed.chars().all(is_cjk_char) {
        Some(collapsed)
    } else {
        None
    }
}

fn leading_cjk_tail(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in value.chars().take_while(|ch| is_cjk_char(*ch)) {
        if out.chars().count() >= max_chars {
            break;
        }
        out.push(ch);
        if matches!(ch, '着' | '了' | '过') {
            break;
        }
    }
    out
}

pub(super) fn local_missing_suffix_repair_pair(
    issue: &str,
    source: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    if !issue.contains("漏掉") || !local_repair_term_is_safe(source) {
        return None;
    }
    let source_len = source.chars().count();
    if source_len > 6 {
        return None;
    }
    let target = quoted.iter().skip(1).find_map(|candidate| {
        let candidate = candidate.trim();
        if candidate.is_empty() || !local_repair_term_is_safe(candidate) {
            return None;
        }
        let repaired = if candidate.starts_with(source) {
            candidate.to_string()
        } else {
            format!("{source}{candidate}")
        };
        let repaired_len = repaired.chars().count();
        if repaired_len <= source_len || repaired_len > 12 {
            return None;
        }
        Some(repaired)
    })?;
    Some((source.to_string(), target))
}

pub(super) fn local_malformed_anchor_repair_pair(issue: &str) -> Option<(String, String)> {
    let lowered = issue.to_ascii_lowercase();
    if !lowered.contains("malformed phrase near stable character anchor") {
        return None;
    }
    let source = issue.rsplit_once(':')?.1.trim();
    if !local_repair_term_is_safe(source) {
        return None;
    }
    let target = if let Some(target) = local_malformed_anchor_sensory_verb_target(source) {
        target
    } else if let Some(prefix) = source.strip_suffix("觉到") {
        format!("{prefix}感觉到")
    } else if let Some(prefix) = source.strip_suffix("识到") {
        format!("{prefix}意识到")
    } else if let Some(prefix) = source.strip_suffix("神一凛") {
        format!("{prefix}神色一凛")
    } else if let Some(prefix) = source.strip_suffix("头一震") {
        format!("{prefix}心头一震")
    } else if let Some(prefix) = source.strip_suffix("脏猛") {
        format!("{prefix}心脏猛")
    } else if let Some(prefix) = source.strip_suffix("睛") {
        format!("{prefix}眼睛")
    } else if let Some(prefix) = source.strip_suffix("孔") {
        format!("{prefix}瞳孔")
    } else if let Some(prefix) = source.strip_suffix("吸") {
        format!("{prefix}深吸")
    } else if let Some(prefix) = source.strip_suffix("光") {
        format!("{prefix}目光")
    } else if let Some(prefix) = source.strip_suffix("冷地") {
        format!("{prefix}冷冷地")
    } else if let Some(prefix) = source.strip_suffix("原地") {
        format!("{prefix}站在原地")
    } else if let Some(prefix) = source.strip_suffix("到，") {
        format!("{prefix}感到，")
    } else if let Some(prefix) = source.strip_suffix("到。") {
        format!("{prefix}感到。")
    } else if let Some(prefix) = source.strip_suffix("静地") {
        format!("{prefix}静静地")
    } else if let Some(prefix) = source.strip_suffix("呼吸，") {
        format!("{prefix}呼吸着，")
    } else if let Some(prefix) = source.strip_suffix("一种") {
        format!("{prefix}感到一种")
    } else if let Some(prefix) = source.strip_suffix("一阵") {
        format!("{prefix}感到一阵")
    } else if let Some(prefix) = source.strip_suffix("一个") {
        format!("{prefix}意识到一个")
    } else if let Some(target) = local_adjacent_anchor_repair_target(source) {
        target
    } else if let Some(target) = local_demonstrative_anchor_repair_target(source) {
        target
    } else {
        return None;
    };
    if target == source || !local_repair_term_is_safe(&target) {
        return None;
    }
    Some((source.to_string(), target))
}

pub(super) fn local_malformed_anchor_sensory_verb_target(source: &str) -> Option<String> {
    if !source.chars().any(is_cjk_char) || source.chars().count() > 24 {
        return None;
    }
    if source.contains("觉到") {
        let repaired = source.replacen("觉到", "感觉到", 1);
        if repaired != source {
            return Some(repaired);
        }
    }
    local_malformed_anchor_missing_feel_target(source)
}

pub(super) fn local_malformed_anchor_missing_feel_target(source: &str) -> Option<String> {
    let chars = source.chars().collect::<Vec<_>>();
    let index = chars.iter().position(|ch| *ch == '到')?;
    if index < 2 || index + 1 >= chars.len() {
        return None;
    }
    let next = chars[index + 1];
    if matches!(next, '了' | '达' | '底' | '处' | '站' | '场' | '口')
        || !is_cjk_char(next)
        || !chars[..index].iter().copied().all(is_cjk_char)
    {
        return None;
    }
    let mut target = chars[..index].iter().collect::<String>();
    target.push_str("感到");
    target.extend(chars[index + 1..].iter());
    Some(target)
}

pub(super) fn local_adjacent_anchor_repair_target(source: &str) -> Option<String> {
    let chars = source.chars().collect::<Vec<_>>();
    if chars.len() < 4 || !chars.iter().all(|ch| is_cjk_char(*ch)) {
        return None;
    }
    let split = chars.len() / 2;
    if !(2..=4).contains(&split) || !(2..=4).contains(&(chars.len() - split)) {
        return None;
    }
    let left = chars[..split].iter().collect::<String>();
    let right = chars[split..].iter().collect::<String>();
    Some(format!("{left}在{right}"))
}

pub(super) fn local_demonstrative_anchor_repair_target(source: &str) -> Option<String> {
    let (prefix, rest) = source.split_once('那')?;
    if prefix.chars().count() < 2 || rest.chars().count() < 3 {
        return None;
    }
    let rest = rest.strip_prefix('座').unwrap_or(rest);
    let object = rest
        .rsplit_once('的')
        .map(|(_, tail)| tail)
        .unwrap_or(rest)
        .trim();
    if object.is_empty() || !object.chars().any(is_cjk_char) {
        return None;
    }
    Some(format!("{prefix}站在{object}"))
}

pub(super) fn quoted_segments(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        let closing = match ch {
            '‘' => '’',
            '“' => '”',
            '\'' => '\'',
            '"' => '"',
            '`' => '`',
            _ => continue,
        };
        let mut segment = String::new();
        for next in chars.by_ref() {
            if next == closing {
                break;
            }
            segment.push(next);
        }
        let segment = segment.trim();
        if !segment.is_empty() {
            out.push(segment.to_string());
        }
    }
    out
}

fn repair_target_segments_after_marker(value: &str) -> Vec<String> {
    let quoted = quoted_segments(value);
    if !quoted.is_empty() {
        return quoted;
    }
    let candidate = value
        .trim()
        .trim_start_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '：' | '(' | ')' | '（' | '）' | '[' | ']' | '【' | '】' | '"' | '\''
                )
        })
        .chars()
        .take_while(|ch| is_cjk_char(*ch))
        .take(12)
        .collect::<String>();
    if candidate.is_empty() {
        Vec::new()
    } else {
        vec![candidate]
    }
}

pub(super) fn local_character_drift_repair_pair(
    issue: &str,
    quoted: &[String],
) -> Option<(String, String)> {
    let lowered = issue.to_ascii_lowercase();
    let looks_like_character_drift = issue.contains("角色名称漂移")
        || issue.contains("角色名漂移")
        || issue.contains("人物名称漂移")
        || lowered.contains("character name drift")
        || lowered.contains("stable contract character")
        || lowered.contains("possible character name drift")
        || (issue.contains("人物名字") && issue.contains("混用"))
        || issue.contains("人物名称不一致")
        || issue.contains("名称不一致");
    if !looks_like_character_drift {
        return None;
    }
    let source = quoted.first()?.trim();
    let target = quoted.iter().skip(1).rev().find_map(|candidate| {
        let candidate = candidate.trim();
        if local_repair_term_is_safe(candidate) && plausible_cjk_name_near_miss(source, candidate) {
            Some(candidate.to_string())
        } else {
            None
        }
    })?;
    if local_repair_term_is_safe(source) {
        Some((source.to_string(), target))
    } else {
        None
    }
}

pub(super) fn local_adjacent_stable_anchor_repair_pair(issue: &str) -> Option<(String, String)> {
    let lowered = issue.to_ascii_lowercase();
    if !lowered.contains("adjacent stable character anchors")
        && !issue.contains("相邻稳定角色")
        && !issue.contains("角色名粘连")
    {
        return None;
    }
    let (left, right) = stable_anchor_pair_from_issue(issue)?;
    let joined = issue
        .rsplit_once(':')
        .map(|(_, tail)| tail.trim())
        .filter(|tail| tail.contains(&left) && tail.contains(&right))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{left}{right}"));
    if !local_repair_term_is_safe(&joined) {
        return None;
    }
    let raw_joined = format!("{left}{right}");
    let replacement = format!("{left}看向{right}");
    let target = if joined == raw_joined {
        replacement
    } else {
        joined.replacen(&raw_joined, &replacement, 1)
    };
    if target == joined || !local_repair_term_is_safe(&target) {
        return None;
    }
    Some((joined, target))
}

pub(super) fn stable_anchor_pair_from_issue(issue: &str) -> Option<(String, String)> {
    let quoted = quoted_segments(issue);
    if quoted.len() >= 2 {
        let left = quoted[0].trim();
        let right = quoted[1].trim();
        if local_repair_term_is_safe(left) && local_repair_term_is_safe(right) {
            return Some((left.to_string(), right.to_string()));
        }
    }
    let mut backticked = Vec::new();
    let mut chars = issue.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '`' {
            continue;
        }
        let mut segment = String::new();
        for next in chars.by_ref() {
            if next == '`' {
                break;
            }
            segment.push(next);
        }
        let segment = segment.trim();
        if !segment.is_empty() {
            backticked.push(segment.to_string());
        }
    }
    if backticked.len() >= 2 {
        let left = backticked[0].trim();
        let right = backticked[1].trim();
        if local_repair_term_is_safe(left) && local_repair_term_is_safe(right) {
            return Some((left.to_string(), right.to_string()));
        }
    }
    None
}

pub(super) fn plausible_cjk_name_near_miss(source: &str, target: &str) -> bool {
    let source_chars = source.chars().collect::<Vec<_>>();
    let target_chars = target.chars().collect::<Vec<_>>();
    if source_chars.is_empty()
        || target_chars.is_empty()
        || source_chars.len() > 6
        || target_chars.len() > 6
    {
        return false;
    }
    if source == target {
        return false;
    }
    if source_chars.first() == target_chars.first()
        && source_chars.len().abs_diff(target_chars.len()) <= 1
    {
        return true;
    }
    levenshtein_distance_chars(&source_chars, &target_chars) <= 1
}

pub(super) fn levenshtein_distance_chars(left: &[char], right: &[char]) -> usize {
    let mut prev = (0..=right.len()).collect::<Vec<_>>();
    let mut curr = vec![0usize; right.len() + 1];
    for (i, left_ch) in left.iter().enumerate() {
        curr[0] = i + 1;
        for (j, right_ch) in right.iter().enumerate() {
            let cost = usize::from(left_ch != right_ch);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[right.len()]
}

pub(super) fn local_repair_term_is_safe(value: &str) -> bool {
    let len = value.chars().count();
    len > 0 && len <= 40 && value.chars().any(is_cjk_char)
}

pub(super) fn apply_local_text_repair_pair(content: &str, source: &str, target: &str) -> String {
    if source == target || source.trim().is_empty() || target.trim().is_empty() {
        return content.to_string();
    }
    if content.contains(source) {
        if let Some(replacement) = local_phrase_replacement(source, target) {
            return replace_local_source(content, source, &replacement);
        }
        if let Some((wrong, right)) = best_local_window_replacement(source, target) {
            let repaired_source = replace_local_source(source, &wrong, &right);
            if repaired_source != source {
                return replace_local_source(content, source, &repaired_source);
            }
        }
    }
    if let Some((wrong, right)) = best_local_window_replacement(source, target) {
        if content.contains(&wrong) {
            return replace_local_source(content, &wrong, &right);
        }
    }
    if let Some((wrong, right)) = best_content_near_miss_replacement(content, source, target) {
        return replace_local_source(content, &wrong, &right);
    }
    content.to_string()
}

pub(super) fn replace_local_source(content: &str, source: &str, target: &str) -> String {
    if source.is_empty() || source == target {
        return content.to_string();
    }
    // Audit-derived lexical repairs are only deterministic when the source has
    // one unambiguous location. Repeated terms need contextual model revision;
    // replacing every occurrence can corrupt valid names and story concepts.
    if content.match_indices(source).take(2).count() != 1 {
        return content.to_string();
    }
    if !target.contains(source) {
        return content.replacen(source, target, 1);
    }
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (index, _) in content.match_indices(source) {
        if index < cursor {
            continue;
        }
        if occurrence_is_inside_existing_target(content, index, source, target) {
            out.push_str(&content[cursor..index + source.len()]);
            cursor = index + source.len();
            continue;
        }
        out.push_str(&content[cursor..index]);
        out.push_str(target);
        cursor = index + source.len();
    }
    out.push_str(&content[cursor..]);
    out
}

pub(super) fn occurrence_is_inside_existing_target(
    content: &str,
    source_index: usize,
    source: &str,
    target: &str,
) -> bool {
    if source.is_empty() || !target.contains(source) {
        return false;
    }
    for (target_source_index, _) in target.match_indices(source) {
        if source_index < target_source_index {
            continue;
        }
        let target_start = source_index - target_source_index;
        let target_end = target_start + target.len();
        if content
            .get(target_start..target_end)
            .is_some_and(|slice| slice == target)
        {
            return true;
        }
    }
    false
}

pub(super) fn local_phrase_replacement(source: &str, target: &str) -> Option<String> {
    let source_len = source.chars().count();
    let target_len = target.chars().count();
    if source_len <= target_len + 4 {
        return Some(target.to_string());
    }
    if target
        .chars()
        .next()
        .is_some_and(|first| source.starts_with(first))
        && target_len.saturating_mul(2) >= source_len
    {
        return Some(target.to_string());
    }
    None
}

pub(super) fn best_local_window_replacement(
    source: &str,
    target: &str,
) -> Option<(String, String)> {
    let target_chars = target.chars().collect::<Vec<_>>();
    let target_len = target_chars.len();
    if target_len == 0 || target_len > 8 {
        return None;
    }
    let source_chars = source.chars().collect::<Vec<_>>();
    if source_chars.len() < target_len {
        return None;
    }
    if target_len > 1 {
        let window_len = target_len - 1;
        for start in 0..=source_chars.len() - window_len {
            let window = &source_chars[start..start + window_len];
            if target_matches_window_with_one_inserted_char(&target_chars, window) {
                return Some((window.iter().collect::<String>(), target.to_string()));
            }
        }
    }
    for extra in 1..=2 {
        let window_len = target_len + extra;
        if source_chars.len() < window_len {
            continue;
        }
        for start in 0..=source_chars.len() - window_len {
            let window = &source_chars[start..start + window_len];
            if target_is_subsequence_of_window(&target_chars, window) {
                return Some((window.iter().collect::<String>(), target.to_string()));
            }
        }
    }
    let mut best: Option<(usize, usize, String)> = None;
    for start in 0..=source_chars.len() - target_len {
        let window = &source_chars[start..start + target_len];
        let score = window
            .iter()
            .zip(target_chars.iter())
            .filter(|(left, right)| left == right)
            .count();
        if score == target_len {
            continue;
        }
        if score == 0 && !window.iter().any(|ch| target_chars.contains(ch)) {
            continue;
        }
        let window_string = window.iter().collect::<String>();
        if best.as_ref().is_none_or(|(best_score, best_start, _)| {
            score > *best_score || (score == *best_score && start > *best_start)
        }) {
            best = Some((score, start, window_string));
        }
    }
    let (score, _, wrong) = best?;
    if score == 0 || wrong == target {
        return None;
    }
    Some((wrong, target.to_string()))
}

fn target_matches_window_with_one_inserted_char(target: &[char], window: &[char]) -> bool {
    if target.len() != window.len() + 1 || target.is_empty() {
        return false;
    }
    for skip in 0..target.len() {
        if target
            .iter()
            .enumerate()
            .filter_map(|(index, ch)| (index != skip).then_some(ch))
            .copied()
            .eq(window.iter().copied())
        {
            return true;
        }
    }
    false
}

pub(super) fn best_content_near_miss_replacement(
    content: &str,
    source: &str,
    target: &str,
) -> Option<(String, String)> {
    let target_chars = target.chars().collect::<Vec<_>>();
    let target_len = target_chars.len();
    if !(2..=12).contains(&target_len) || !target_chars.iter().copied().all(is_cjk_char) {
        return None;
    }
    if target_len <= source.chars().count() && content.contains(source) {
        return None;
    }

    let chars = content.chars().collect::<Vec<_>>();
    if chars.len() < target_len {
        return None;
    }
    let min_score = target_len
        .saturating_sub(1)
        .max(target_len.saturating_mul(3).div_ceil(4));
    let mut best: Option<(usize, usize, usize, String)> = None;
    for window_len in target_len.saturating_sub(1).max(1)..=target_len + 1 {
        if window_len > chars.len() {
            continue;
        }
        for start in 0..=chars.len() - window_len {
            let window = &chars[start..start + window_len];
            if !window.iter().copied().all(is_cjk_char) {
                continue;
            }
            let wrong = window.iter().collect::<String>();
            if wrong == target {
                continue;
            }
            if window.first() != target_chars.first() || window.last() != target_chars.last() {
                continue;
            }
            let score = ordered_char_overlap_score(window, &target_chars);
            if score < min_score {
                continue;
            }
            let source_overlap = if source.is_empty() {
                0
            } else {
                source.chars().filter(|ch| wrong.contains(*ch)).count()
            };
            if best
                .as_ref()
                .is_none_or(|(best_score, best_overlap, best_start, _)| {
                    score > *best_score
                        || (score == *best_score && source_overlap > *best_overlap)
                        || (score == *best_score
                            && source_overlap == *best_overlap
                            && start > *best_start)
                })
            {
                best = Some((score, source_overlap, start, wrong));
            }
        }
    }
    let (_, _, _, wrong) = best?;
    Some((wrong, target.to_string()))
}

pub(super) fn ordered_char_overlap_score(left: &[char], right: &[char]) -> usize {
    let mut score = 0usize;
    let mut right_index = 0usize;
    for ch in left {
        while right_index < right.len() && right[right_index] != *ch {
            right_index += 1;
        }
        if right_index < right.len() {
            score += 1;
            right_index += 1;
        }
    }
    score.max(positional_char_overlap_score(left, right))
}

pub(super) fn positional_char_overlap_score(left: &[char], right: &[char]) -> usize {
    left.iter()
        .zip(right.iter())
        .filter(|(left, right)| left == right)
        .count()
}

pub(super) fn target_is_subsequence_of_window(target: &[char], window: &[char]) -> bool {
    let mut index = 0usize;
    for ch in window {
        if index < target.len() && *ch == target[index] {
            index += 1;
        }
    }
    index == target.len()
}

pub(super) fn strip_intrusive_ascii_quotes_inside_cjk(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(content.len());
    for (index, ch) in chars.iter().copied().enumerate() {
        if (ch == '\'' || ch == '"')
            && index > 0
            && index + 1 < chars.len()
            && is_cjk_char(chars[index - 1])
            && is_cjk_char(chars[index + 1])
        {
            continue;
        }
        out.push(ch);
    }
    out
}

pub(super) fn collapse_duplicate_cjk_before_open_quote(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if is_cjk_char(ch)
            && matches!(chars.get(index + 1), Some('“' | '「' | '『'))
            && chars.get(index + 2).is_some_and(|next| *next == ch)
        {
            index += 1;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

pub(super) fn only_small_length_shortfall(
    write_result: &Value,
    audit: &Value,
    chapter_unit_target: Option<usize>,
    language: &str,
) -> bool {
    let (Some(current), Some(target)) =
        chapter_length_current_and_target(write_result, audit, chapter_unit_target)
    else {
        return false;
    };
    if current >= target {
        return false;
    }
    let findings = typed_findings_in_value(write_result)
        .into_iter()
        .chain(typed_findings_in_value(audit));
    let mut hard_codes = BTreeSet::new();
    let mut has_length_finding = false;
    let mut repair_codes = BTreeSet::new();
    for finding in findings {
        if matches!(
            finding.code.as_str(),
            "length_below_target" | "length_below_usable_floor"
        ) {
            has_length_finding = true;
        }
        if finding.hard_blocking() {
            hard_codes.insert(finding.code);
        } else if finding.disposition
            == chapter_quality::ChapterFindingDisposition::DeterministicRepair
            && finding.class != chapter_quality::ChapterFindingClass::Metadata
        {
            repair_codes.insert(finding.code);
        }
    }
    // The usable-floor finding is a deterministic length blocker, but it is
    // still recoverable by the existing bounded length-top-up route when the
    // remaining gap is within that route's limit. Treating it as an arbitrary
    // semantic blocker sent a complete-but-short draft through repeated LLM
    // rewrites until the revision budget was exhausted.
    if hard_codes
        .iter()
        .any(|code| code != "length_below_usable_floor")
        || !has_length_finding
        || repair_codes
            .iter()
            .any(|code| code != "length_below_target")
        || !json_array_is_empty(write_result.pointer("/truth_validation/issues"))
        || !json_array_is_empty(audit.pointer("/truth_validation/issues"))
    {
        return false;
    }

    let shortfall = target.saturating_sub(current);
    shortfall <= length_topup_shortfall_limit(target, language)
}

pub(super) fn chapter_length_current_and_target(
    write_result: &Value,
    _audit: &Value,
    chapter_unit_target: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    let current = write_result
        .get("unit_count")
        .or_else(|| write_result.pointer("/chapter/unit_count"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let target = chapter_unit_target.filter(|value| *value > 0);
    (current, target)
}

pub(super) fn length_topup_shortfall_limit(target: usize, language: &str) -> usize {
    let percent = target.saturating_mul(50).div_ceil(100);
    let floor = if language_looks_cjk(language) {
        600
    } else {
        250
    };
    percent.max(floor).min(target.saturating_div(2).max(1))
}

pub(super) fn collect_string_array(value: Option<&Value>, out: &mut Vec<String>) {
    let Some(value) = value else {
        return;
    };
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(text) = item.as_str().map(str::trim).filter(|text| !text.is_empty()) {
                    out.push(text.to_string());
                }
            }
        }
        Value::String(text) if !text.trim().is_empty() => out.push(text.trim().to_string()),
        _ => {}
    }
}

pub(super) fn chapter_generation_limits(
    chapter_unit_target: Option<usize>,
    language: &str,
) -> TextGenerationLimits {
    let target = chapter_unit_target
        .filter(|value| *value > 0)
        .unwrap_or_else(longform_policy::step_target_chars);
    TextGenerationLimits {
        max_tokens: Some(chapter_output_token_budget(target, language)),
        target_chars: Some(target),
        hard_max_chars: Some(chapter_hard_char_limit(target, language)),
    }
}

pub(super) fn chapter_execution_package_llm_enabled() -> bool {
    std::env::var("BENSHU_NOVEL_EXECUTION_PACKAGE_LLM")
        .ok()
        .is_none_or(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
}

pub(super) fn govern_generated_execution_package(
    mut package: novel_runner::ChapterExecutionPackage,
    language: &str,
    title: &str,
    chapter_number: usize,
    context_json: &str,
    finale_mode: bool,
    completion_gate: Option<&ProjectCompletionGateDecision>,
) -> novel_runner::ChapterExecutionPackage {
    let canonical = fallback_chapter_execution_package(
        language,
        title,
        chapter_number,
        context_json,
        finale_mode,
        completion_gate,
    );
    let parsed_context = serde_json::from_str::<Value>(context_json).ok();
    let current_seed = parsed_context.as_ref().and_then(|value| {
        fallback_chapter_seed_from_near_chapters(value, chapter_number).or_else(|| {
            fallback_opening_chapter_seed_from_story_contract(
                value,
                chapter_number,
                language_looks_cjk(language),
            )
        })
    });
    package.future_chapters = parsed_context
        .as_ref()
        .map(|value| {
            govern_rolling_future_chapters(
                value,
                chapter_number,
                std::mem::take(&mut package.future_chapters),
            )
        })
        .unwrap_or_default();
    let future_seeds = package
        .future_chapters
        .iter()
        .map(|seed| {
            [seed.goal.as_str(), seed.expected_turn.as_str()]
                .into_iter()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("；")
        })
        .filter(|seed| !seed.is_empty())
        .collect::<Vec<_>>();
    if !future_seeds.is_empty() {
        let cjk = language_looks_cjk(language);
        let current_seed = current_seed.as_deref().unwrap_or_default();
        for field in [
            &mut package.architecture,
            &mut package.conflict,
            &mut package.choice,
            &mut package.cost,
            &mut package.reveal,
            &mut package.emotional_beat,
            &mut package.chapter_function,
            &mut package.irreversible_event,
            &mut package.new_state_after_chapter,
            &mut package.world_change,
            &mut package.character_change,
            &mut package.relationship_change,
            &mut package.power_delta,
            &mut package.resource_delta,
        ] {
            if future_seeds.iter().any(|future_seed| {
                governance::text_consumes_future_chapter(field, current_seed, future_seed, cjk)
            }) {
                field.clear();
            }
        }
        package.hook_opened.retain(|field| {
            future_seeds.iter().all(|future_seed| {
                !governance::text_consumes_future_chapter(field, current_seed, future_seed, cjk)
            })
        });
        package.hook_paid_off.retain(|field| {
            future_seeds.iter().all(|future_seed| {
                !governance::text_consumes_future_chapter(field, current_seed, future_seed, cjk)
            })
        });
        package.new_character_requests.retain(|request| {
            serde_json::to_string(request).ok().is_none_or(|field| {
                future_seeds.iter().all(|future_seed| {
                    !governance::text_consumes_future_chapter(
                        &field,
                        current_seed,
                        future_seed,
                        cjk,
                    )
                })
            })
        });
    }
    if let Some(current_seed) = current_seed.as_deref() {
        let cjk = language_looks_cjk(language);
        // These fields become sealed reveal/state authority. Model-generated
        // scene detail can remain creative, but an ungrounded invention must
        // not become a durable transition that the final body is forced to
        // realize.
        for field in [
            &mut package.reveal,
            &mut package.irreversible_event,
            &mut package.new_state_after_chapter,
            &mut package.world_change,
            &mut package.character_change,
            &mut package.relationship_change,
            &mut package.power_delta,
            &mut package.resource_delta,
        ] {
            if !field.trim().is_empty()
                && !governance::event_text_is_grounded_in_current_chapter(field, current_seed, cjk)
            {
                field.clear();
            }
        }
        package.hook_opened.retain(|field| {
            governance::event_text_is_grounded_in_current_chapter(field, current_seed, cjk)
        });
    }

    // The canonical contract owns the immutable chapter goal and future
    // exclusion boundary. The model may refine scenes and typed transitions,
    // but it cannot replace those two authority fields before sealing.
    let generated_architecture = package.architecture.trim().to_string();
    package.memo = canonical.memo;
    package.scene_goal = canonical.scene_goal;
    package.title_basis = canonical.title_basis;
    // Keep the sealed chapter contract's required outcome as the sole state
    // authority. Atomic evidence selection belongs to the existing settlement
    // recovery path after the final body is available; execution-package
    // governance must not derive a second pre-body state authority.
    if !canonical.new_state_after_chapter.trim().is_empty() {
        package.new_state_after_chapter = canonical.new_state_after_chapter;
    }
    let architecture = if generated_architecture.is_empty() {
        canonical.architecture
    } else if language_looks_cjk(language) {
        format!(
            "{}\n\n## 场景细化（不得覆盖上方合同边界）\n{}",
            canonical.architecture, generated_architecture
        )
    } else {
        format!(
            "{}\n\n## Scene refinement (must not override the contract boundary above)\n{}",
            canonical.architecture, generated_architecture
        )
    };
    package.architecture = novel_runner::render_execution_contract_header(&package) + &architecture;
    package
}

pub(super) fn fallback_chapter_execution_package(
    language: &str,
    title: &str,
    chapter_number: usize,
    context_json: &str,
    finale_mode: bool,
    completion_gate: Option<&ProjectCompletionGateDecision>,
) -> novel_runner::ChapterExecutionPackage {
    let cjk = language_looks_cjk(language);
    let chapter_seed = fallback_chapter_seed_goal(context_json, chapter_number, cjk);
    let chapter_end_state = serde_json::from_str::<Value>(context_json)
        .ok()
        .and_then(|value| fallback_chapter_end_state_from_near_chapters(&value, chapter_number));
    let next_chapter_seed = serde_json::from_str::<Value>(context_json)
        .ok()
        .and_then(|value| {
            chapter_number
                .checked_add(1)
                .and_then(|number| fallback_chapter_seed_from_near_chapters(&value, number))
        });
    let goal = match (cjk, finale_mode, chapter_seed.as_deref()) {
        (true, true, Some(seed)) => format!("完成《{title}》第 {chapter_number} 章作为终局/尾声：{seed}；同时收束主冲突、人物弧线和主要伏笔，给出自然结尾，不再开启新阶段。"),
        (true, false, Some(seed)) => format!("完成《{title}》第 {chapter_number} 章：{seed}"),
        (true, true, None) => format!("完成《{title}》第 {chapter_number} 章作为终局/尾声，收束主冲突、人物弧线和主要伏笔，给出自然结尾，不再开启新阶段。"),
        (true, false, None) => format!("推进《{title}》第 {chapter_number} 章，继承上下文并完成本章可验证变化。"),
        (false, true, Some(seed)) => format!("Complete chapter {chapter_number} of \"{title}\" as a finale/epilogue: {seed}; close the main conflict, character arc, and key hooks without opening a new phase."),
        (false, false, Some(seed)) => {
            format!("Complete chapter {chapter_number} of \"{title}\": {seed}")
        }
        (false, true, None) => format!(
            "Complete chapter {chapter_number} of \"{title}\" as a finale/epilogue: close the main conflict, character arc, and key hooks without opening a new phase."
        ),
        (false, false, None) => format!(
            "Advance chapter {chapter_number} of \"{title}\" with continuity and a verifiable chapter change."
        ),
    };
    let completion_directive = completion_gate
        .map(|gate| finale_execution_directive(gate, language))
        .unwrap_or_default();
    let completion_directive_section = if completion_directive.is_empty() {
        String::new()
    } else if cjk {
        format!(
            "\n\n## 本轮完成门权威指令\n{}",
            preview_text(&completion_directive, 3200)
        )
    } else {
        format!(
            "\n\n## Authoritative Completion Directive\n{}",
            preview_text(&completion_directive, 3200)
        )
    };
    let context_hint = fallback_chapter_context_hint(context_json, cjk);
    let memo_markdown = fallback_chapter_memo_markdown(
        cjk,
        finale_mode,
        chapter_number,
        &goal,
        chapter_seed.as_deref(),
        next_chapter_seed.as_deref(),
        &completion_directive,
        &context_hint,
    );
    let memo = novel_runner::parse_memo(&memo_markdown, language)
        .expect("deterministic fallback memo must satisfy the canonical memo schema");
    let architecture = if cjk && finale_mode {
        format!(
            "1. 承接上一章状态：确认主角、核心关系、主要冲突和剩余伏笔的位置。\n2. 终局选择：让主角做出不可逆选择，解决主冲突的核心矛盾。\n3. 兑现债务：用具体事件关闭主要伏笔和关系线，不再开启新敌人或新阶段。\n4. 结局状态：写出世界、人物关系和主角内在状态的稳定结果。\n5. 终章画面：以完成感收束，不留下下一章入口。{completion_directive_section}\n\n上下文摘要参考：{context_hint}"
        )
    } else if cjk {
        format!(
            "1. 承接上一章状态：用具体场景确认人物位置、关系状态和未解决事项。\n2. 锁定本章目标：只围绕“{chapter_goal}”组织核心行动，不用计划外谜团或新任务替换它。\n3. 中段展开：通过行动、对话、观察或发现完成本章目标中的事件与关系变化，不改变既有角色名。\n4. 形成可记录变化：让正文明确证明本章目标和预期转折已经发生。\n5. 章尾落点：保持下一章节点仍能自然发生，不提前完成它，也不把地点、任务或人物状态转向与它冲突的方向。{next_boundary}\n\n上下文摘要参考：{context_hint}",
            chapter_goal = chapter_seed.as_deref().unwrap_or("继承合同并完成一个可验证变化"),
            next_boundary = next_chapter_seed
                .as_deref()
                .map(|seed| format!("\n下一章边界：{}", preview_text(seed, 260)))
                .unwrap_or_default()
        )
    } else if finale_mode {
        format!(
            "1. Re-anchor the protagonist, core relationship, main conflict, and remaining hooks.\n2. Final choice: make the protagonist take an irreversible action that resolves the central contradiction.\n3. Pay debts through concrete events; do not open a new enemy, phase, or next-chapter entry.\n4. Record the stable ending state for the world, relationships, and inner arc.\n5. End on a closing image with no next-chapter hook.{completion_directive_section}\n\nContext hint: {context_hint}"
        )
    } else {
        format!(
            "1. Re-anchor continuity with a concrete scene, character position, relationship state, and unresolved item.\n2. Lock the chapter goal to: {chapter_goal}. Do not replace it with an unplanned mystery or task.\n3. Develop through action, dialogue, observation, or discovery while preserving established names.\n4. Make the chapter goal and expected turn visibly occur in prose.\n5. Leave the next outline node naturally reachable; do not complete it early or redirect the location, task, or character state against it.{next_boundary}\n\nContext hint: {context_hint}",
            chapter_goal = chapter_seed.as_deref().unwrap_or("inherit the contract and create one verifiable change"),
            next_boundary = next_chapter_seed
                .as_deref()
                .map(|seed| format!("\nNext chapter boundary: {}", preview_text(seed, 260)))
                .unwrap_or_default()
        )
    };
    let hook_paid_off = completion_gate
        .map(|gate| gate.debt_ids.clone())
        .unwrap_or_default();
    let finale_brief = completion_gate
        .and_then(|gate| gate.finale_brief.clone())
        .unwrap_or_default();
    novel_runner::ChapterExecutionPackage {
        memo,
        architecture,
        scene_goal: goal,
        conflict: String::new(),
        choice: String::new(),
        cost: String::new(),
        reveal: String::new(),
        emotional_beat: String::new(),
        chapter_function: if finale_mode {
            if cjk {
                "终局收束".to_string()
            } else {
                "finale closure".to_string()
            }
        } else {
            String::new()
        },
        irreversible_event: finale_brief,
        // Keep the outline's expected turn verbatim as the required end-state
        // authority. The final observer may prove it with a bounded contiguous
        // multi-sentence excerpt; execution must not derive a second, weakened
        // authority by splitting or paraphrasing this field.
        new_state_after_chapter: chapter_end_state.unwrap_or_default(),
        world_change: String::new(),
        character_change: String::new(),
        relationship_change: String::new(),
        power_delta: String::new(),
        resource_delta: String::new(),
        hook_opened: Vec::new(),
        hook_paid_off,
        title_basis: chapter_seed.unwrap_or_default(),
        future_chapters: Vec::new(),
        new_character_requests: Vec::new(),
        degraded: false,
        degraded_reason: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn fallback_chapter_memo_markdown(
    cjk: bool,
    finale_mode: bool,
    chapter_number: usize,
    goal: &str,
    chapter_seed: Option<&str>,
    next_chapter_seed: Option<&str>,
    completion_directive: &str,
    context_hint: &str,
) -> String {
    if cjk {
        let chapter_material = chapter_seed
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|seed| {
                format!(
                    "\n本章目标素材（只供改写成场景，禁止原句进入正文）：{}",
                    preview_text(seed, 220)
                )
            })
            .unwrap_or_default();
        let boundary = if finale_mode {
            (!completion_directive.trim().is_empty())
                .then(|| {
                    format!(
                        "\n本轮完成门权威指令：{}",
                        preview_text(completion_directive, 3200)
                    )
                })
                .unwrap_or_default()
        } else {
            next_chapter_seed
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|seed| {
                    format!(
                        "\n下一章边界（只作为禁区，不得在本章完成）：{}\n本章结尾必须让这个节点仍能自然发生；不得提前完成它，也不得把地点、任务、人物状态或因果方向改到与它冲突。",
                        preview_text(seed, 360)
                    )
                })
                .unwrap_or_default()
        };
        let chapter_goal = if finale_mode {
            "把主冲突、人物最终选择、关系落点、到期伏笔和世界状态收束成正文中的实际事件。"
        } else {
            "根据本章权威目标完成一个可由正文证明的具体变化，不用计划外谜团替换它。"
        };
        let payoff = if finale_mode {
            "兑现主线结果、人物弧线、核心关系和已到期伏笔；不得只写准备或逼近。"
        } else {
            "兑现当前大纲节点和到期伏笔；未到期的信息只推进，不提前揭开。"
        };
        let reserved = if finale_mode {
            "不再开启新阶段、新敌人、新主线或下一章入口。"
        } else {
            "保留后续章节边界，不提前完成结局、下一节点或尚未到期的伏笔。"
        };
        let ending = if finale_mode {
            "主要冲突有结果，核心关系有落点，主角弧线有归宿，世界进入可记录的结局状态。"
        } else {
            "留下可记录的事实、关系、线索或阶段变化，并使下一章节点仍可自然发生。"
        };
        return format!(
            "目标：{goal}\n\n\
## 当前任务\n继承合同、truth、最近批准章节和只读章节权威，写出完整第 {chapter_number} 章。上下文摘要：{context_hint}\n\n\
## 本章目标\n{chapter_goal}{chapter_material}\n\n\
## 该兑现\n{payoff}\n\n\
## 暂不掀\n{reserved}{boundary}\n\n\
## 日常过渡功能\n按题材和压力曲线加入必要的关系、观察或呼吸段落，但必须服务本章变化。\n\n\
## 关键抉择三连问\n人物行动、付出代价和造成结果必须形成可追踪因果，并符合既有角色状态。\n\n\
## 章尾必须发生的改变\n{ending}\n\n\
## 不要做\n不要改名、复制上下文、输出流程说明或改写已批准章节；不要把上面的目标、权威摘要或约束原句塞进正文。"
        );
    }

    let chapter_material = chapter_seed
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|seed| {
            format!(
                "\nChapter goal material (rewrite into scenes; do not copy this wording into prose): {}",
                preview_text(seed, 220)
            )
        })
        .unwrap_or_default();
    let boundary = if finale_mode {
        (!completion_directive.trim().is_empty())
            .then(|| {
                format!(
                    "\nAuthoritative completion directive: {}",
                    preview_text(completion_directive, 3200)
                )
            })
            .unwrap_or_default()
    } else {
        next_chapter_seed
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|seed| {
                format!(
                    "\nNext chapter boundary (constraint only; do not complete now): {}\nThe ending must leave this node naturally reachable. Do not complete it early or redirect location, task, character state, or causality against it.",
                    preview_text(seed, 360)
                )
            })
            .unwrap_or_default()
    };
    let chapter_goal = if finale_mode {
        "Resolve the main conflict, final choice, relationship landing, due hooks, and world state through events in the prose."
    } else {
        "Complete one concrete, text-provable change from the authoritative chapter goal without replacing it with an unplanned mystery."
    };
    let payoff = if finale_mode {
        "Pay off the main-line outcome, character arc, core relationships, and all due hooks; do not merely prepare or approach."
    } else {
        "Pay off the current outline node and due hooks; only advance information whose reveal is not yet due."
    };
    let reserved = if finale_mode {
        "Do not open a new phase, enemy, main line, or next-chapter entry."
    } else {
        "Preserve later chapter boundaries and do not complete the ending, next node, or not-yet-due hooks early."
    };
    let ending = if finale_mode {
        "The main conflict has an outcome, core relationships land, the protagonist arc resolves, and the world reaches a recordable ending state."
    } else {
        "Leave a recordable fact, relationship, clue, or phase change while keeping the next chapter naturally reachable."
    };
    format!(
        "goal: {goal}\n\n\
## Current Task\nInherit the contract, truth, approved chapters, and read-only chapter authority; write complete chapter {chapter_number}. Context hint: {context_hint}\n\n\
## Chapter Goal\n{chapter_goal}{chapter_material}\n\n\
## Pay Off\n{payoff}\n\n\
## Do Not Reveal Yet\n{reserved}{boundary}\n\n\
## Everyday Transition Function\nUse only genre-appropriate relationship, observation, or relief beats that serve this chapter's change.\n\n\
## Decision Checks\nCharacter action, paid cost, and resulting consequence must form traceable causality consistent with established state.\n\n\
## Required End-State Change\n{ending}\n\n\
## Do Not\nDo not rename characters, copy context, emit workflow notes, rewrite approved chapters, or paste authority summaries into prose."
    )
}

pub(super) fn fallback_chapter_seed_goal(
    context_json: &str,
    chapter_number: usize,
    cjk: bool,
) -> Option<String> {
    let value = serde_json::from_str::<Value>(context_json).ok()?;
    fallback_chapter_seed_from_near_chapters(&value, chapter_number)
        .or_else(|| fallback_chapter_seed_from_outline_texts(&value, chapter_number, cjk))
        .or_else(|| fallback_opening_chapter_seed_from_story_contract(&value, chapter_number, cjk))
        .map(|value| preview_text(value.trim(), 360))
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn fallback_opening_chapter_seed_from_story_contract(
    value: &Value,
    chapter_number: usize,
    cjk: bool,
) -> Option<String> {
    if chapter_number != 1 {
        return None;
    }
    let premise = json_pointer_string(
        value,
        &[
            "/canonical_contract/premise",
            "/authority/canonical_contract/premise",
            "/project_context/contract/premise",
            "/contract/premise",
            "/creation_contract/premise",
            "/project_context/project/brief",
            "/project/brief",
        ],
    )?;
    let protagonist = primary_character_anchor_from_context(value);
    if cjk {
        let subject = protagonist
            .as_deref()
            .map(|name| format!("主角{name}"))
            .unwrap_or_else(|| "主角".to_string());
        Some(format!(
            "建立{subject}的初始处境、故事前提入口和第一次不可逆选择：{premise}"
        ))
    } else {
        let subject = protagonist.as_deref().unwrap_or("the protagonist");
        Some(format!(
            "Establish {subject}'s starting situation, the premise entry, and one first irreversible choice: {premise}."
        ))
    }
}

fn json_pointer_string(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
    })
}

fn primary_character_anchor_from_context(value: &Value) -> Option<String> {
    for pointer in [
        "/canonical_contract/characters",
        "/authority/canonical_contract/characters",
        "/project_context/contract/characters",
        "/contract/characters",
        "/project_context/story_bible/characters",
        "/story_bible/characters",
    ] {
        let Some(items) = value.pointer(pointer).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let role = item
                .get("role")
                .or_else(|| item.get("function"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !role.contains("主角") && !role.to_ascii_lowercase().contains("protagonist") {
                continue;
            }
            if let Some(name) = item
                .get("name")
                .or_else(|| item.get("canonical_name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

pub(super) fn fallback_chapter_seed_from_near_chapters(
    value: &Value,
    chapter_number: usize,
) -> Option<String> {
    let item = chapter_seed_item_from_context(value, chapter_number, true, false)?;
    let parts = [
        "title",
        "goal",
        "expected_turn",
        "moves_toward_ending",
        "objective",
        "summary",
    ]
    .into_iter()
    .filter_map(|key| item.get(key).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("；"))
}

fn fallback_chapter_end_state_from_near_chapters(
    value: &Value,
    chapter_number: usize,
) -> Option<String> {
    let item = chapter_seed_item_from_context(value, chapter_number, true, false)?;
    ["expected_turn", "moves_toward_ending"]
        .into_iter()
        .find_map(|key| item.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
}

const CHAPTER_SEED_CONTEXT_POINTERS: &[&str] = &[
    "/canonical_contract/outline/near_chapters",
    "/authority/canonical_contract/outline/near_chapters",
    "/current_chapter_goal",
    "/authority/current_chapter_goal",
    "/truth_as_of_chapter/story_state/narrative_graph/chapter_goals",
    "/authority/truth_as_of_chapter/story_state/narrative_graph/chapter_goals",
    "/project_context/story_bible/narrative_graph/chapter_goals",
    "/story_bible/narrative_graph/chapter_goals",
    "/narrative_graph/chapter_goals",
    "/project_context/next_chapter_boundary",
    "/next_chapter_boundary",
    "/authority/working_context/next_chapter_boundary",
    "/rolling_outline_window",
    "/authority/rolling_outline_window",
    "/project_context/contract/outline/near_chapters",
    "/contract/outline/near_chapters",
    "/outline/near_chapters",
    "/creation_contract/outline/near_chapters",
];

fn chapter_seed_item_from_context<'a>(
    value: &'a Value,
    chapter_number: usize,
    allow_unnumbered: bool,
    require_complete: bool,
) -> Option<&'a Value> {
    for pointer in CHAPTER_SEED_CONTEXT_POINTERS {
        let Some(items) = value.pointer(pointer).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let number = item
                .get("number")
                .or_else(|| item.get("chapter_number"))
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            if number.is_some_and(|number| number != chapter_number)
                || number.is_none() && !allow_unnumbered
            {
                continue;
            }
            let goal = ["goal", "objective", "summary"]
                .into_iter()
                .find_map(|key| item.get(key).and_then(Value::as_str))
                .is_some_and(|value| !value.trim().is_empty());
            let expected_turn = ["expected_turn", "moves_toward_ending"]
                .into_iter()
                .find_map(|key| item.get(key).and_then(Value::as_str))
                .is_some_and(|value| !value.trim().is_empty());
            let any_seed_text = goal
                || expected_turn
                || item
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
            if (require_complete && goal && expected_turn) || (!require_complete && any_seed_text) {
                return Some(item);
            }
        }
    }
    None
}

fn expected_chapters_from_execution_context(value: &Value) -> Option<usize> {
    for pointer in [
        "/narrative_progress/expected_chapters",
        "/project_context/narrative_progress/expected_chapters",
        "/authority/working_context/narrative_progress/expected_chapters",
    ] {
        if let Some(expected) = value.pointer(pointer).and_then(Value::as_u64) {
            return usize::try_from(expected)
                .ok()
                .filter(|expected| *expected > 0);
        }
    }
    let target = [
        "/canonical_contract/target_units",
        "/authority/canonical_contract/target_units",
        "/project_context/project/target_units",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))?;
    let per_chapter = [
        "/canonical_contract/chapter_unit_target",
        "/authority/canonical_contract/chapter_unit_target",
        "/project_context/project/chapter_unit_target",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))?;
    let target = usize::try_from(target).ok()?;
    let per_chapter = usize::try_from(per_chapter).ok()?;
    longform_policy::expected_chapter_count(target, per_chapter)
}

fn chapter_seed_contract_from_context(
    value: &Value,
    chapter_number: usize,
) -> Option<crate::tool::writing::creation_contract_model::ChapterSeedContract> {
    let item = chapter_seed_item_from_context(value, chapter_number, false, true)?;
    let goal = ["goal", "objective", "summary"]
        .into_iter()
        .find_map(|key| item.get(key).and_then(Value::as_str))?
        .trim();
    let expected_turn = ["expected_turn", "moves_toward_ending"]
        .into_iter()
        .find_map(|key| item.get(key).and_then(Value::as_str))?
        .trim();
    Some(
        crate::tool::writing::creation_contract_model::ChapterSeedContract {
            number: Some(chapter_number),
            goal: goal.to_string(),
            expected_turn: expected_turn.to_string(),
        },
    )
}

fn govern_rolling_future_chapters(
    context: &Value,
    chapter_number: usize,
    generated: Vec<crate::tool::writing::creation_contract_model::ChapterSeedContract>,
) -> Vec<crate::tool::writing::creation_contract_model::ChapterSeedContract> {
    let expected_chapters = expected_chapters_from_execution_context(context);
    let last_allowed = chapter_number
        .saturating_add(governance::ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS)
        .min(expected_chapters.unwrap_or(usize::MAX));
    let mut generated = generated
        .into_iter()
        .filter_map(|mut seed| {
            let number = seed.number?;
            seed.goal = seed.goal.trim().to_string();
            seed.expected_turn = seed.expected_turn.trim().to_string();
            (number > chapter_number
                && number <= last_allowed
                && !seed.goal.is_empty()
                && !seed.expected_turn.is_empty())
            .then_some((number, seed))
        })
        .collect::<BTreeMap<_, _>>();
    let mut governed = Vec::new();
    for number in chapter_number.saturating_add(1)..=last_allowed {
        let seed = chapter_seed_contract_from_context(context, number)
            .or_else(|| generated.remove(&number));
        let Some(seed) = seed else {
            continue;
        };
        if typed_contract_gate::contract_outline_plan_text_is_placeholder(&seed.goal)
            || typed_contract_gate::contract_outline_plan_text_is_placeholder(&seed.expected_turn)
            || rolling_seed_replays_current_transition(context, chapter_number, &seed)
        {
            break;
        }
        if !rolling_seed_stays_within_volume_scope(context, &seed) {
            continue;
        }
        let fingerprint = format!(
            "{}|{}",
            normalize_repetition_segment(&seed.goal),
            normalize_repetition_segment(&seed.expected_turn)
        );
        if governed.iter().any(
            |existing: &crate::tool::writing::creation_contract_model::ChapterSeedContract| {
                format!(
                    "{}|{}",
                    normalize_repetition_segment(&existing.goal),
                    normalize_repetition_segment(&existing.expected_turn)
                ) == fingerprint
            },
        ) {
            continue;
        }
        governed.push(seed);
    }
    governed
}

fn rolling_seed_replays_current_transition(
    context: &Value,
    chapter_number: usize,
    seed: &crate::tool::writing::creation_contract_model::ChapterSeedContract,
) -> bool {
    let Some(current) = chapter_seed_contract_from_context(context, chapter_number) else {
        return false;
    };
    let current_turn = normalize_repetition_segment(&current.expected_turn);
    let future_goal = normalize_repetition_segment(&seed.goal);
    if current_turn.is_empty() || future_goal.is_empty() {
        return false;
    }
    if current_turn == future_goal {
        return true;
    }
    let cjk = current_turn.chars().any(is_cjk_char) || future_goal.chars().any(is_cjk_char);
    if !cjk {
        let current_turn = current_turn.to_ascii_lowercase();
        let future_goal = future_goal.to_ascii_lowercase();
        return current_turn.split_whitespace().count() >= 4 && future_goal.contains(&current_turn);
    }
    current_turn.chars().count() >= 8
        && future_goal.chars().count() >= 8
        && chapter_quality::shared_distinctive_bigram_count(&current_turn, &future_goal) >= 8
}

fn rolling_seed_stays_within_volume_scope(
    context: &Value,
    seed: &crate::tool::writing::creation_contract_model::ChapterSeedContract,
) -> bool {
    let Some(chapter_number) = seed.number else {
        return false;
    };
    let volumes = [
        "/authority/working_context/project/volumes",
        "/project_context/project/volumes",
        "/project/volumes",
    ]
    .into_iter()
    .find_map(|pointer| context.pointer(pointer).and_then(Value::as_array));
    let Some(volumes) = volumes else {
        return true;
    };
    let scoped_index = volumes.iter().position(|volume| {
        let start = volume
            .get("start_chapter")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(1);
        let end = volume
            .get("end_chapter")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        chapter_number >= start && end.is_none_or(|end| chapter_number <= end)
    });
    let Some(scoped_index) = scoped_index else {
        return false;
    };
    let seed_event = format!("{} {}", seed.goal.trim(), seed.expected_turn.trim());
    let cjk = seed_event.chars().any(is_cjk_char);
    let seed_goal = normalize_repetition_segment(&seed.goal);
    let seed_turn = normalize_repetition_segment(&seed.expected_turn);
    for (index, volume) in volumes.iter().enumerate() {
        for (field, only_at_volume_end) in [("objective", false), ("ending_change", true)] {
            let Some(contract_field) = volume.get(field).and_then(Value::as_str) else {
                continue;
            };
            let normalized_contract_field = normalize_repetition_segment(contract_field);
            if normalized_contract_field.is_empty() {
                continue;
            }
            let exact_copy =
                seed_goal == normalized_contract_field || seed_turn == normalized_contract_field;
            let consumes_volume_event =
                governance::text_consumes_future_chapter(&seed_event, "", contract_field, cjk);
            if index != scoped_index && (exact_copy || consumes_volume_event) {
                return false;
            }
            if index == scoped_index && !only_at_volume_end && exact_copy {
                return false;
            }
            if index == scoped_index && only_at_volume_end && (exact_copy || consumes_volume_event)
            {
                let end = volume
                    .get("end_chapter")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok());
                if end != Some(chapter_number) {
                    return false;
                }
            }
        }
    }
    true
}

pub(super) fn fallback_chapter_seed_from_outline_texts(
    value: &Value,
    chapter_number: usize,
    cjk: bool,
) -> Option<String> {
    let mut texts = Vec::new();
    for pointer in [
        "/canonical_contract/outline/raw_outline",
        "/authority/canonical_contract/outline/raw_outline",
        "/project_context/contract/outline",
        "/project_context/narrative_graph/global_spine",
        "/project_context/story_bible/narrative_graph/global_spine",
        "/contract/outline",
        "/outline/raw_outline",
    ] {
        if let Some(text) = value.pointer(pointer).and_then(Value::as_str) {
            texts.push(text);
        }
    }
    if let Some(items) = value
        .pointer("/context_package/selected_context")
        .and_then(Value::as_array)
    {
        texts.extend(
            items
                .iter()
                .filter_map(|item| item.get("excerpt").and_then(Value::as_str)),
        );
    }
    texts
        .into_iter()
        .find_map(|text| fallback_chapter_seed_from_outline_text(text, chapter_number, cjk))
}

pub(super) fn fallback_chapter_seed_from_outline_text(
    text: &str,
    chapter_number: usize,
    cjk: bool,
) -> Option<String> {
    let markers = if cjk {
        vec![
            format!("第{chapter_number:02}章"),
            format!("第{chapter_number}章"),
            format!("第 {chapter_number} 章"),
        ]
    } else {
        vec![
            format!("chapter {chapter_number}"),
            format!("chapter-{chapter_number}"),
            format!("chapter_{chapter_number}"),
        ]
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| {
            let lowered = line.to_ascii_lowercase();
            markers.iter().any(|marker| {
                if cjk {
                    line.contains(marker)
                } else {
                    lowered.contains(&marker.to_ascii_lowercase())
                }
            })
        })
        .map(|line| {
            line.split_once("本章目标")
                .map(|(_, tail)| tail.trim_start_matches(['：', ':', ' ', '-']).trim())
                .unwrap_or(line)
                .to_string()
        })
        .filter(|line| !line.is_empty())
}

pub(super) fn fallback_chapter_context_hint(context_json: &str, cjk: bool) -> String {
    let Ok(value) = serde_json::from_str::<Value>(context_json) else {
        return preview_text(context_json, 480);
    };
    let mut lines = Vec::new();
    if let Some(project) = value.get("project") {
        let title = project.get("title").and_then(Value::as_str).unwrap_or("");
        let genre = project.get("genre").and_then(Value::as_str).unwrap_or("");
        let brief = project.get("brief").and_then(Value::as_str).unwrap_or("");
        if !title.trim().is_empty() {
            lines.push(if cjk {
                format!("项目：{title}")
            } else {
                format!("Project: {title}")
            });
        }
        if !genre.trim().is_empty() {
            lines.push(if cjk {
                format!("题材：{}", preview_text(genre, 80))
            } else {
                format!("Genre: {}", preview_text(genre, 80))
            });
        }
        if !brief.trim().is_empty() {
            lines.push(if cjk {
                format!("简述：{}", preview_text(brief, 160))
            } else {
                format!("Brief: {}", preview_text(brief, 160))
            });
        }
    }
    if let Some(characters) = value
        .pointer("/continuity_anchors/characters")
        .and_then(Value::as_array)
    {
        let anchors = characters
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .take(8)
            .collect::<Vec<_>>();
        if !anchors.is_empty() {
            lines.push(if cjk {
                format!("稳定角色锚点：{}", anchors.join("、"))
            } else {
                format!("Stable character anchors: {}", anchors.join(", "))
            });
        }
    }
    if let Some(primary) = value
        .pointer("/continuity_anchors/primary_characters")
        .and_then(Value::as_array)
    {
        let anchors = primary
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .take(3)
            .collect::<Vec<_>>();
        if !anchors.is_empty() {
            lines.push(if cjk {
                format!(
                    "主角权威锚点：{}。本章正文必须保留这些主角作为叙事中心；非主角不能无解释接管主角行动线。",
                    anchors.join("、")
                )
            } else {
                format!(
                    "Primary character authority: {}. The chapter must preserve these protagonists as focal anchors; supporting characters must not replace the protagonist arc without explicit chapter contract.",
                    anchors.join(", ")
                )
            });
        }
    }
    if let Some(recent) = value.get("recent_chapters").and_then(Value::as_array) {
        let recent_lines = recent
            .iter()
            .take(3)
            .filter_map(|chapter| {
                let number = chapter.get("number").and_then(Value::as_u64)?;
                let title = chapter.get("title").and_then(Value::as_str).unwrap_or("");
                let summary = chapter.get("summary").and_then(Value::as_str).unwrap_or("");
                Some(if cjk {
                    format!(
                        "第 {number} 章《{}》：{}",
                        preview_text(title, 48),
                        preview_text(summary, 180)
                    )
                } else {
                    format!(
                        "Chapter {number} {}: {}",
                        preview_text(title, 48),
                        preview_text(summary, 180)
                    )
                })
            })
            .collect::<Vec<_>>();
        if !recent_lines.is_empty() {
            lines.push(if cjk {
                format!("最近已批准章节：{}", recent_lines.join(" / "))
            } else {
                format!("Recent approved chapters: {}", recent_lines.join(" / "))
            });
        }
    }
    if let Some(plan) = value.pointer("/plan/plan").and_then(Value::as_str) {
        if !plan.trim().is_empty() {
            lines.push(if cjk {
                format!("本章计划：{}", preview_text(plan, 240))
            } else {
                format!("Chapter plan: {}", preview_text(plan, 240))
            });
        }
    }
    if lines.is_empty() {
        if cjk {
            "项目权威上下文已加载；只按本章合同推进，并保持后续具体事件未发生。".to_string()
        } else {
            "Project authority context is loaded. Advance only this chapter contract and keep specific later events unperformed.".to_string()
        }
    } else {
        lines.join("\n")
    }
}

pub(super) fn initial_chapter_generation_limits(
    chapter_unit_target: Option<usize>,
    language: &str,
) -> TextGenerationLimits {
    let target = chapter_unit_target
        .filter(|value| *value > 0)
        .unwrap_or_else(longform_policy::step_target_chars);
    let first_pass_target = target.saturating_mul(110).div_ceil(100).max(target);
    TextGenerationLimits {
        max_tokens: Some(chapter_output_token_budget(first_pass_target, language)),
        target_chars: Some(first_pass_target),
        hard_max_chars: Some(chapter_hard_char_limit(target, language)),
    }
}

pub(super) fn chapter_segment_generation_limits(
    target: usize,
    language: &str,
) -> TextGenerationLimits {
    let max_tokens = chapter_output_token_budget(target, language);
    TextGenerationLimits {
        max_tokens: Some(max_tokens),
        target_chars: Some(target),
        // Segment output is a structured envelope around prose. Reusing the prose-only
        // character cap can cut the JSON string before its closing quote.
        hard_max_chars: Some(chapter_hard_char_limit(target, language).max(max_tokens as usize)),
    }
}

pub(super) fn minimum_chapter_units(target: usize) -> usize {
    longform_policy::minimum_usable_chapter_units(target)
}

pub(super) fn required_chapter_units(target: usize) -> usize {
    target.max(1)
}

pub(super) fn chapter_step_duration_secs(
    chapter_unit_target: Option<usize>,
    project_target_units: Option<usize>,
) -> u64 {
    let target = chapter_unit_target
        .filter(|value| *value > 0)
        .unwrap_or_else(|| longform_policy::dynamic_chapter_unit_target(project_target_units));
    let scaled = 720u64.saturating_add((target as u64).div_ceil(6));
    scaled.clamp(720, 2_400)
}

pub(super) fn chapter_expansion_round_budget(target: usize, current: usize) -> usize {
    let required = required_chapter_units(target);
    if current >= required {
        return 0;
    }
    let missing = required.saturating_sub(current);
    let segment_target = chapter_expansion_segment_target(target, missing).max(1);
    let max_rounds = if target <= longform_policy::step_target_chars() {
        2
    } else {
        3
    };
    missing
        .div_ceil(segment_target)
        .saturating_add(1)
        .clamp(1, max_rounds)
}

pub(super) fn chapter_expansion_segment_target(target: usize, remaining: usize) -> usize {
    if remaining <= 160 {
        return remaining.saturating_mul(2).clamp(40, 220);
    }
    let preferred = target.div_ceil(2).max(1200);
    let requested = remaining.saturating_mul(130).div_ceil(100);
    requested.min(preferred).max(40)
}

pub(super) fn chapter_minimum_addition_units(segment_target: usize) -> usize {
    if segment_target <= 220 {
        return (segment_target / 3).max(16);
    }
    (segment_target / 5).max(120)
}

pub(super) fn count_chapter_units(content: &str, language: &str) -> usize {
    if language_looks_cjk(language) {
        content.chars().filter(|ch| !ch.is_whitespace()).count()
    } else {
        content.split_whitespace().count()
    }
}

pub(super) fn existing_unapproved_chapter_is_reusable(
    draft: &novel_runner::DraftOutput,
    chapter_unit_target: Option<usize>,
    language: &str,
) -> bool {
    let units = count_chapter_units(&draft.content, language);
    if units == 0 {
        return false;
    }
    let Some(target) = chapter_unit_target.filter(|value| *value > 0) else {
        return units
            >= longform_policy::step_target_chars()
                .saturating_div(2)
                .max(1000)
            && !chapter_body_has_degenerate_repetition(&draft.content, language);
    };
    units >= minimum_chapter_units(target)
        && !chapter_body_has_degenerate_repetition(&draft.content, language)
}

pub(super) fn chapter_body_has_degenerate_repetition(content: &str, language: &str) -> bool {
    if !language_looks_cjk(language) {
        return repeated_normalized_line_or_sentence(content, 10, 4);
    }
    repeated_normalized_line_or_sentence(content, 12, 3)
        || repeated_long_normalized_line_or_sentence(content, 36, 2)
        || repeated_cjk_abstract_progression_markers(content)
}

fn repeated_long_normalized_line_or_sentence(
    content: &str,
    min_chars: usize,
    max_repeats: usize,
) -> bool {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for segment in content
        .split(|ch| matches!(ch, '\n' | '。' | '！' | '？' | '!' | '?' | ';' | '；'))
        .map(normalize_repetition_segment)
        .filter(|segment| segment.chars().count() >= min_chars)
    {
        let count = seen.entry(segment).or_insert(0);
        *count += 1;
        if *count >= max_repeats {
            return true;
        }
    }
    false
}

fn repeated_normalized_line_or_sentence(
    content: &str,
    min_chars: usize,
    max_repeats: usize,
) -> bool {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for segment in content
        .split(|ch| matches!(ch, '\n' | '。' | '！' | '？' | '!' | '?' | ';' | '；'))
        .map(normalize_repetition_segment)
        .filter(|segment| segment.chars().count() >= min_chars)
    {
        let count = seen.entry(segment).or_insert(0);
        *count += 1;
        if *count >= max_repeats {
            return true;
        }
    }
    false
}

fn normalize_repetition_segment(segment: &str) -> String {
    segment
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '，' | ','
                        | '。'
                        | '.'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                        | '“'
                        | '”'
                        | '"'
                        | '\''
                        | '‘'
                        | '’'
                        | '：'
                        | ':'
                        | '；'
                        | ';'
                )
        })
        .collect::<String>()
}

fn repeated_cjk_abstract_progression_markers(content: &str) -> bool {
    let compact = normalize_repetition_segment(content);
    if compact.chars().count() < 180 {
        return false;
    }
    let marker_groups: &[&[&str]] = &[
        &["证明", "认可", "偏见"],
        &["第一步", "开始", "未来的路"],
        &["命运", "转折"],
        &["强者", "巅峰"],
        &["挑战", "准备"],
        &["改变", "秩序"],
    ];
    let repeated_groups = marker_groups
        .iter()
        .filter(|group| {
            let repeated_in_group = group
                .iter()
                .filter_map(|marker| {
                    let count = compact.matches(*marker).count();
                    if count >= 2 {
                        Some(count)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            repeated_in_group.len() >= 2
        })
        .count();
    repeated_groups >= 2
}

pub(super) fn json_string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(super) fn repair_draft_summary_after_body_cleanup(
    draft: &mut novel_runner::DraftOutput,
    language: &str,
) {
    let summary = draft.summary.trim();
    if !summary_needs_repair(summary, &draft.content, language) {
        draft.summary = summary.to_string();
        return;
    }
    draft.summary = compact_chapter_summary_from_content(&draft.content, language);
}

pub(super) fn summary_needs_repair(summary: &str, content: &str, language: &str) -> bool {
    if summary.is_empty() {
        return true;
    }
    let lowered = summary.to_ascii_lowercase();
    let summary_chars = summary.chars().count();
    let cjk = language_looks_cjk(language);
    summary.starts_with('{')
        || summary.starts_with("```")
        || summary.contains("\"title\"")
        || summary.contains("\"content\"")
        || summary.contains("\"summary\"")
        || summary.contains("\"key_facts\"")
        || summary.contains("```json")
        || lowered.contains("return only json")
        || lowered.contains("output contract")
        || lowered.contains("workflow")
        || (cjk && content_has_cjk(content) && !content_has_cjk(summary))
        || (cjk && summary_chars < 40 && content.chars().count() >= 800)
        || (cjk && summary_chars > 180)
        || summary_looks_like_appended_deltas(summary, language)
}

fn summary_looks_like_appended_deltas(summary: &str, language: &str) -> bool {
    if !language_looks_cjk(language) {
        return false;
    }
    let normalized = normalize_repetition_segment(summary);
    let common_starts = ["主角", "他", "她"];
    let sentences = summary
        .split(|ch| matches!(ch, '。' | '！' | '？' | ';' | '；'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if sentences.len() >= 3 && repeated_summary_sentence_lead(&sentences) {
        return true;
    }
    common_starts
        .iter()
        .any(|prefix| summary.matches(prefix).count() >= 3)
        || (normalized.chars().count() >= 80 && sentences.len() >= 3)
}

fn repeated_summary_sentence_lead(sentences: &[&str]) -> bool {
    let mut counts = BTreeMap::<String, usize>::new();
    for sentence in sentences {
        let lead = sentence
            .chars()
            .take_while(|ch| is_cjk_char(*ch))
            .take(3)
            .collect::<String>();
        if lead.chars().count() < 2 {
            continue;
        }
        *counts.entry(lead).or_default() += 1;
    }
    counts.values().any(|count| *count >= 3)
}

pub(super) fn compact_chapter_summary_from_content(content: &str, language: &str) -> String {
    let cleaned = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        return String::new();
    }
    let cjk = language_looks_cjk(language);
    let max_chars = if cjk { 160 } else { 260 };
    let min_chars = if cjk { 80 } else { 120 };
    let sentence_min_chars = if cjk { 8 } else { 32 };
    let mut out = String::new();
    for ch in cleaned.chars() {
        out.push(ch);
        let chars = out.chars().count();
        if chars >= max_chars
            || (chars >= sentence_min_chars && matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'))
            || (chars >= min_chars && matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'))
        {
            break;
        }
    }
    out.trim().to_string()
}

pub(super) fn chapter_body_completion_issue_list(content: &str) -> Vec<String> {
    crate::tool::writing::novel_studio::chapter_body_completion_issues(content)
}

pub(super) fn content_has_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

#[derive(Default)]
pub(super) struct ChapterExpansionOutput {
    pub(super) addition: String,
    pub(super) summary_delta: Option<String>,
    pub(super) key_facts: Vec<String>,
    pub(super) continuity_updates: Vec<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct RawChapterExpansionOutput {
    add: Option<String>,
    addition: Option<String>,
    content: Option<String>,
    text: Option<String>,
    summary_delta: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    key_facts: Vec<String>,
    #[serde(default)]
    continuity_updates: Vec<String>,
}

pub(super) fn parse_chapter_expansion_output(raw: &str, language: &str) -> ChapterExpansionOutput {
    if let Some(json) = novel_runner::extract_json(raw) {
        if let Ok(parsed) = serde_json::from_str::<RawChapterExpansionOutput>(&json) {
            let addition = parsed
                .add
                .or(parsed.addition)
                .or(parsed.content)
                .or(parsed.text)
                .unwrap_or_default();
            let addition = sanitize_chapter_body(&addition, "", language);
            return ChapterExpansionOutput {
                addition,
                summary_delta: parsed.summary_delta.or(parsed.summary),
                key_facts: parsed.key_facts,
                continuity_updates: parsed.continuity_updates,
            };
        }
    }
    if let Some(addition) = novel_runner::jsonish_string_field(
        raw,
        "addition",
        &[
            "summary_delta",
            "summary",
            "key_facts",
            "continuity_updates",
        ],
    ) {
        return ChapterExpansionOutput {
            addition: sanitize_chapter_body(&addition, "", language),
            summary_delta: novel_runner::jsonish_string_field(
                raw,
                "summary_delta",
                &["summary", "key_facts", "continuity_updates"],
            )
            .or_else(|| {
                novel_runner::jsonish_string_field(
                    raw,
                    "summary",
                    &["key_facts", "continuity_updates"],
                )
            }),
            key_facts: novel_runner::jsonish_string_array_field(raw, "key_facts"),
            continuity_updates: novel_runner::jsonish_string_array_field(raw, "continuity_updates"),
        };
    }
    if let Some(addition) = novel_runner::jsonish_string_field(
        raw,
        "add",
        &[
            "addition",
            "summary_delta",
            "summary",
            "key_facts",
            "continuity_updates",
        ],
    ) {
        return ChapterExpansionOutput {
            addition: sanitize_chapter_body(&addition, "", language),
            summary_delta: novel_runner::jsonish_string_field(
                raw,
                "summary_delta",
                &["summary", "key_facts", "continuity_updates"],
            )
            .or_else(|| {
                novel_runner::jsonish_string_field(
                    raw,
                    "summary",
                    &["key_facts", "continuity_updates"],
                )
            }),
            key_facts: novel_runner::jsonish_string_array_field(raw, "key_facts"),
            continuity_updates: novel_runner::jsonish_string_array_field(raw, "continuity_updates"),
        };
    }
    if raw.trim_start().starts_with('{') {
        return ChapterExpansionOutput::default();
    }

    ChapterExpansionOutput {
        addition: sanitize_chapter_body(raw, "", language),
        summary_delta: None,
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
    }
}

pub(super) fn raw_chapter_expansion_rejection_reason(raw: &str, language: &str) -> Option<String> {
    if !language_looks_cjk(language) {
        return None;
    }
    let addition = raw_expansion_addition_text(raw).unwrap_or_else(|| raw.trim().to_string());
    let addition = addition.trim();
    if addition.is_empty() {
        return None;
    }
    let cleaned = sanitize_chapter_body(addition, "", language);
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Some("扩写片段清洗后为空".to_string());
    }
    if raw_has_invalid_escape_before_cjk(raw) && raw_has_invalid_escape_before_cjk(cleaned) {
        return Some("扩写片段包含异常转义残片".to_string());
    }
    if contains_unexpected_script_for_chinese(cleaned) {
        return Some("扩写片段包含非中文脚本残片".to_string());
    }
    if let Some(reason) = surface_sanitizer::high_confidence_surface_issue(cleaned) {
        if !high_confidence_noise_reason_is_fragment_boundary(&reason) {
            return Some(format!("正文表面污染：{reason}"));
        }
    }
    if cleaned
        .lines()
        .any(|line| line_looks_like_json_artifact_residue(line))
    {
        return Some("扩写片段包含结构化字段残片".to_string());
    }
    None
}

pub(super) fn raw_expansion_addition_text(raw: &str) -> Option<String> {
    if let Some(json) = novel_runner::extract_json(raw) {
        if let Ok(parsed) = serde_json::from_str::<RawChapterExpansionOutput>(&json) {
            return parsed.addition.or(parsed.content).or(parsed.text);
        }
    }
    novel_runner::jsonish_string_field(
        raw,
        "addition",
        &[
            "summary_delta",
            "summary",
            "key_facts",
            "continuity_updates",
        ],
    )
    .or_else(|| {
        novel_runner::jsonish_string_field(
            raw,
            "content",
            &[
                "summary_delta",
                "summary",
                "key_facts",
                "continuity_updates",
            ],
        )
    })
    .or_else(|| {
        novel_runner::jsonish_string_field(
            raw,
            "text",
            &[
                "summary_delta",
                "summary",
                "key_facts",
                "continuity_updates",
            ],
        )
    })
}

pub(super) fn raw_has_invalid_escape_before_cjk(raw: &str) -> bool {
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        let Some(next) = chars.peek().copied() else {
            return true;
        };
        if is_cjk_char(next) {
            return true;
        }
    }
    false
}

pub(super) fn contains_unexpected_script_for_chinese(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x0370..=0x03FF
                | 0x0400..=0x052F
                | 0x0590..=0x05FF
                | 0x0600..=0x06FF
                | 0x0900..=0x097F
                | 0x0E00..=0x0E7F
                | 0x3040..=0x30FF
                | 0xAC00..=0xD7AF
        )
    })
}

pub(super) fn chapter_expansion_rejection_reason(
    existing_content: &str,
    addition: &str,
    language: &str,
) -> Option<String> {
    let addition = addition.trim();
    if addition.is_empty() {
        return Some("扩写片段为空".to_string());
    }
    if expansion_addition_repeats_existing_content(existing_content, addition, language) {
        return Some("扩写片段与既有正文高度重复".to_string());
    }
    if expansion_addition_replays_existing_paragraph(existing_content, addition, language) {
        return Some("扩写片段复述既有正文段落".to_string());
    }
    if expansion_addition_repeats_recent_tail(existing_content, addition, language) {
        return Some("扩写片段开头复述既有正文尾部".to_string());
    }
    if language_looks_cjk(language) {
        if let Some(reason) = surface_sanitizer::high_confidence_surface_issue(addition) {
            if high_confidence_noise_reason_is_fragment_boundary(&reason) {
                let combined = if existing_content.trim_end().is_empty() {
                    addition.to_string()
                } else {
                    format!("{}{}", existing_content.trim_end(), addition)
                };
                if surface_sanitizer::high_confidence_surface_issue(&combined).is_none() {
                    return None;
                }
            }
            return Some(format!("正文表面污染：{reason}"));
        }
    }
    None
}

pub(super) fn chapter_tail_completion_rejection_reason(
    existing_content: &str,
    addition: &str,
    language: &str,
) -> Option<String> {
    let addition = addition.trim();
    if addition.is_empty() {
        return Some("补尾片段为空".to_string());
    }
    if chapter_body_completion_issue_list(existing_content).is_empty() {
        return chapter_expansion_rejection_reason(existing_content, addition, language);
    }
    if expansion_addition_repeats_existing_content(existing_content, addition, language) {
        return Some("补尾片段与既有正文高度重复".to_string());
    }
    if expansion_addition_replays_existing_paragraph(existing_content, addition, language)
        || expansion_addition_repeats_recent_tail(existing_content, addition, language)
    {
        return Some("补尾片段疑似复述既有正文".to_string());
    }
    if language_looks_cjk(language) {
        if let Some(reason) = surface_sanitizer::high_confidence_surface_issue(addition) {
            if high_confidence_noise_reason_is_fragment_boundary(&reason) {
                let combined = format!("{}{}", existing_content.trim_end(), addition);
                if surface_sanitizer::high_confidence_surface_issue(&combined).is_none() {
                    return None;
                }
            }
            return Some(format!("正文表面污染：{reason}"));
        }
    }

    let mut probe = novel_runner::DraftOutput {
        title: String::new(),
        content: existing_content.to_string(),
        summary: String::new(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        degraded: false,
        degraded_reason: String::new(),
    };
    append_chapter_tail_completion(
        &mut probe,
        ChapterExpansionOutput {
            addition: addition.to_string(),
            summary_delta: None,
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
        },
    );
    if chapter_body_completion_issue_list(&probe.content).is_empty() {
        return None;
    }
    Some("补尾片段未能修复截断".to_string())
}

fn high_confidence_noise_reason_is_fragment_boundary(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    lowered.contains("unbalanced chinese double quotes")
        || lowered.contains("unbalanced chinese single quotes")
        || lowered.contains("unbalanced cjk quotes")
}

pub(super) fn expansion_addition_repeats_existing_content(
    existing_content: &str,
    addition: &str,
    language: &str,
) -> bool {
    let existing = normalize_expansion_overlap_text(existing_content, language);
    let addition = normalize_expansion_overlap_text(addition, language);
    if existing.is_empty() || addition.chars().count() < 120 {
        return false;
    }
    if existing.contains(&addition) {
        return true;
    }
    let addition_bigrams = chapter_quality::adjacent_bigrams(&addition);
    if addition_bigrams.len() < 80 {
        return false;
    }
    let existing_bigrams = chapter_quality::adjacent_bigrams(&existing)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if existing_bigrams.is_empty() {
        return false;
    }
    let overlap = addition_bigrams
        .iter()
        .filter(|bigram| existing_bigrams.contains(*bigram))
        .count();
    overlap.saturating_mul(100) / addition_bigrams.len() >= 72
}

fn expansion_addition_repeats_recent_tail(
    existing_content: &str,
    addition: &str,
    language: &str,
) -> bool {
    let existing = normalize_expansion_overlap_text(existing_content, language);
    let addition = normalize_expansion_overlap_text(addition, language);
    if existing.chars().count() < 40 || addition.chars().count() < 40 {
        return false;
    }
    let tail = last_n_chars(&existing, 700);
    let prefix = first_n_chars(&addition, 120);
    if tail.contains(&prefix) {
        return true;
    }
    let prefix_bigrams = chapter_quality::adjacent_bigrams(&prefix);
    if prefix_bigrams.len() < 30 {
        return false;
    }
    let tail_bigrams = chapter_quality::adjacent_bigrams(&tail)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if tail_bigrams.is_empty() {
        return false;
    }
    let overlap = prefix_bigrams
        .iter()
        .filter(|bigram| tail_bigrams.contains(*bigram))
        .count();
    overlap.saturating_mul(100) / prefix_bigrams.len() >= 48
}

fn expansion_addition_replays_existing_paragraph(
    existing_content: &str,
    addition: &str,
    language: &str,
) -> bool {
    let existing_paragraphs = existing_content
        .split('\n')
        .map(|paragraph| normalize_expansion_overlap_text(paragraph, language))
        .filter(|paragraph| paragraph.chars().count() >= 55)
        .filter_map(|paragraph| {
            let paragraph_bigrams = chapter_quality::adjacent_bigrams(&paragraph);
            if paragraph_bigrams.len() < 45 {
                return None;
            }
            Some((
                paragraph,
                paragraph_bigrams.into_iter().collect::<BTreeSet<_>>(),
            ))
        })
        .collect::<Vec<_>>();
    if existing_paragraphs.is_empty() {
        return false;
    }

    addition
        .split('\n')
        .map(str::trim)
        .map(|paragraph| normalize_expansion_overlap_text(paragraph, language))
        .filter(|paragraph| paragraph.chars().count() >= 55)
        .any(|addition_paragraph| {
            let addition_bigrams = chapter_quality::adjacent_bigrams(&addition_paragraph);
            if addition_bigrams.len() < 45 {
                return false;
            }
            let addition_bigrams = addition_bigrams.into_iter().collect::<BTreeSet<_>>();
            existing_paragraphs
                .iter()
                .any(|(paragraph, paragraph_bigrams)| {
                    let overlap = paragraph_bigrams
                        .iter()
                        .filter(|bigram| addition_bigrams.contains(*bigram))
                        .count();
                    let basis = paragraph_bigrams.len().min(addition_bigrams.len());
                    overlap.saturating_mul(100) / basis >= 56
                        || (overlap.saturating_mul(100) / basis >= 34
                            && cjk_unique_char_overlap_ratio(paragraph, &addition_paragraph) >= 50)
                        || expansion_addition_replays_paragraph_by_ordered_core(
                            paragraph,
                            &addition_paragraph,
                        )
                })
        })
}

fn expansion_addition_replays_paragraph_by_ordered_core(
    paragraph: &str,
    addition_intro: &str,
) -> bool {
    let paragraph_chars = paragraph.chars().collect::<Vec<_>>();
    let addition_chars = addition_intro.chars().collect::<Vec<_>>();
    let min_len = paragraph_chars.len().min(addition_chars.len());
    if min_len < 55 {
        return false;
    }
    let ordered_overlap = ordered_char_overlap_score(&addition_chars, &paragraph_chars);
    let ordered_ratio = ordered_overlap.saturating_mul(100) / min_len;
    if ordered_ratio < 42 {
        return false;
    }
    cjk_unique_char_overlap_ratio(paragraph, addition_intro) >= 58
}

fn cjk_unique_char_overlap_ratio(left: &str, right: &str) -> usize {
    let left_chars = left
        .chars()
        .filter(|ch| is_cjk_char(*ch))
        .collect::<BTreeSet<_>>();
    let right_chars = right
        .chars()
        .filter(|ch| is_cjk_char(*ch))
        .collect::<BTreeSet<_>>();
    let min_len = left_chars.len().min(right_chars.len());
    if min_len == 0 {
        return 0;
    }
    let overlap = left_chars
        .iter()
        .filter(|ch| right_chars.contains(ch))
        .count();
    overlap.saturating_mul(100) / min_len
}

fn first_n_chars(value: &str, n: usize) -> String {
    value.chars().take(n).collect()
}

fn last_n_chars(value: &str, n: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(n);
    chars[start..].iter().collect()
}

fn normalize_expansion_overlap_text(value: &str, language: &str) -> String {
    if language_looks_cjk(language) {
        value
            .chars()
            .filter(|ch| is_cjk_char(*ch))
            .collect::<String>()
    } else {
        value
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(super) fn append_chapter_addition(
    draft: &mut novel_runner::DraftOutput,
    addition: ChapterExpansionOutput,
) {
    let addition_text = addition.addition.trim();
    if addition_text.is_empty() {
        return;
    }
    let base = draft.content.trim_end();
    draft.content = if base.is_empty() {
        addition_text.to_string()
    } else {
        format!("{base}\n\n{addition_text}")
    };
    if let Some(summary) = addition
        .summary_delta
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        if draft.summary.trim().is_empty() {
            draft.summary = summary;
        } else {
            draft.summary = format!("{} {}", draft.summary.trim(), summary);
        }
    }
    draft.key_facts.extend(
        addition
            .key_facts
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
    draft.continuity_updates.extend(
        addition
            .continuity_updates
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
}

pub(super) fn append_chapter_tail_completion(
    draft: &mut novel_runner::DraftOutput,
    addition: ChapterExpansionOutput,
) {
    let addition_text = addition.addition.trim();
    if addition_text.is_empty() {
        return;
    }
    let base = draft.content.trim_end();
    draft.content = if base.is_empty() {
        addition_text.to_string()
    } else if !chapter_body_completion_issue_list(base).is_empty() {
        format!("{base}{addition_text}")
    } else {
        format!("{base}\n\n{addition_text}")
    };
    if let Some(summary) = addition
        .summary_delta
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        if draft.summary.trim().is_empty() {
            draft.summary = summary;
        } else {
            draft.summary = format!("{} {}", draft.summary.trim(), summary);
        }
    }
    draft.key_facts.extend(
        addition
            .key_facts
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
    draft.continuity_updates.extend(
        addition
            .continuity_updates
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
}

pub(super) fn trim_overlapping_chapter_expansion(
    existing_content: &str,
    mut addition: ChapterExpansionOutput,
    language: &str,
) -> ChapterExpansionOutput {
    let mut trimmed = addition.addition.trim().to_string();
    for _ in 0..8 {
        let next =
            trim_overlapping_tail_addition_text(existing_content.trim_end(), &trimmed, language)
                .trim_start()
                .to_string();
        if next == trimmed {
            break;
        }
        trimmed = next;
        if trimmed.is_empty() {
            break;
        }
    }
    if trimmed != addition.addition.trim() {
        addition.addition = trimmed;
    }
    addition
}

pub(super) fn trim_overlapping_chapter_tail_completion(
    existing_content: &str,
    mut addition: ChapterExpansionOutput,
    language: &str,
) -> ChapterExpansionOutput {
    let trimmed = trim_overlapping_tail_addition_text(
        existing_content.trim_end(),
        addition.addition.trim(),
        language,
    );
    if trimmed != addition.addition.trim() {
        addition.addition = trimmed;
    }
    addition
}

fn trim_overlapping_tail_addition_text(existing: &str, addition: &str, language: &str) -> String {
    if existing.is_empty() || addition.is_empty() {
        return addition.to_string();
    }
    let min_overlap = if language_looks_cjk(language) { 6 } else { 16 };
    let max_overlap = if language_looks_cjk(language) {
        96
    } else {
        240
    };
    let existing_chars = existing.chars().collect::<Vec<_>>();
    let addition_chars = addition.chars().collect::<Vec<_>>();
    let max = existing_chars
        .len()
        .min(addition_chars.len())
        .min(max_overlap);
    for overlap in (min_overlap..=max).rev() {
        let existing_tail = existing_chars[existing_chars.len() - overlap..]
            .iter()
            .collect::<String>();
        let addition_head = addition_chars[..overlap].iter().collect::<String>();
        if normalize_repetition_segment(&existing_tail)
            == normalize_repetition_segment(&addition_head)
        {
            return addition_chars[overlap..].iter().collect::<String>();
        }
    }
    trim_leading_replayed_sentence(existing, addition, language)
        .unwrap_or_else(|| addition.to_string())
}

fn trim_leading_replayed_sentence(
    existing: &str,
    addition: &str,
    language: &str,
) -> Option<String> {
    if !language_looks_cjk(language) {
        return None;
    }
    let sentence_end = addition.char_indices().find_map(|(index, ch)| {
        matches!(ch, '。' | '！' | '？' | '；').then_some(index + ch.len_utf8())
    })?;
    let leading = addition[..sentence_end].trim();
    if leading.chars().count() < 8 {
        return None;
    }
    let existing_normalized = normalize_repetition_segment(existing);
    let leading_normalized = normalize_repetition_segment(leading);
    if !leading_normalized.is_empty() && existing_normalized.contains(&leading_normalized) {
        let rest = addition[sentence_end..].trim_start();
        if rest.chars().count() >= 4 {
            return Some(rest.to_string());
        }
    }
    None
}

pub(super) fn chapter_expansion_prompt(
    chapter_number: usize,
    title: &str,
    language: &str,
    target: usize,
    minimum: usize,
    current: usize,
    segment_target: usize,
    attempt: usize,
    previous_rejection: Option<&str>,
    summary: &str,
    content: &str,
    authority_context: &str,
) -> String {
    let existing_context = chapter_expansion_existing_context(content, language);
    let authority_context = authority_context.to_string();
    let retry_feedback = previous_rejection
        .filter(|reason| !reason.trim().is_empty())
        .map(|reason| {
            if language_looks_cjk(language) {
                format!(
                    "\n\n上一扩写尝试被拒绝，原因：{reason}。本次必须换一个尚未发生的具体行动或后果推进，不能再次生成相同片段。"
                )
            } else {
                format!(
                    "\n\nThe previous expansion attempt was rejected because: {reason}. Use a different concrete action or consequence that has not happened yet; do not generate the same segment again."
                )
            }
        })
        .unwrap_or_default();
    if language_looks_cjk(language) {
        format!(
            "继续扩写《{title}》第 {chapter_number} 章，这是第 {attempt} 次扩写尝试。当前正文约 {current} 字，最低要求 {minimum} 字，目标约 {target} 字；本次请追加约 {segment_target} 字的新正文。{retry_feedback}\n\n要求：只输出 JSON 对象，字段为 addition, summary_delta, key_facts, continuity_updates。addition 必须直接接在下方正文末尾之后，从新的动作或反应开始，不能改写或复述末尾内容；必须推进本章当前目标内尚未发生的新行动、新决定、新代价或新发现；不要重写整章，不要改变标题、主角、已发生事实和结局方向；不得发明合同外的新谜团、新超常现象、新关键物件或新任务；下一章节点只作为禁区，不能在扩写片段中提前完成。如果当前正文已经有自然收束，追加内容必须形成一个完整的“动作—反应/后果—新收束”小节，不能只在收束段后另起一个短动作或准备动作就戛然而止。若摘要、正文末尾或其他文字与下方章节权威冲突，以下方章节权威为准。严禁使用“此处省略”“略去”“待补充”“后续剧情”“未完待续”等占位、摘要式替代或省略说明；key_facts 和 continuity_updates 只能写 addition 中明确发生的新事实，不能只写人物名。\n\n章节与连续性权威：\n{authority_context}\n\n本章摘要（已发生事件的压缩记录，禁止复述）：\n{summary}\n\n当前正文末尾（addition 必须从其后开始，禁止重放）：\n{existing_context}"
        )
    } else {
        format!(
            "Continue expanding chapter {chapter_number} of \"{title}\". This is expansion attempt {attempt}. The current body is about {current} units, below the minimum {minimum}, with a target around {target}; add about {segment_target} units of new prose.{retry_feedback}\n\nReturn only JSON with fields: addition, summary_delta, key_facts, continuity_updates. addition must continue directly after the prose tail below, begin with a new action or reaction, and never rewrite or paraphrase that tail. It must advance an action, decision, cost, or discovery still available inside the current chapter goal. Do not rewrite the whole chapter, change the title, rename protagonists, alter established facts, or change the ending direction. Do not invent an uncontracted mystery, supernatural event, key object, or task. The next outline node is an exclusion boundary and must not be completed in this expansion. If the existing body already has a natural landing, the addition must form a complete action-reaction/consequence-new-landing mini-section; never append one short setup action after the landing and stop. If the summary or prose tail conflicts with the chapter authority below, the authority wins. Never use omission markers, placeholder notes, summary substitutes, \"omitted\", \"placeholder\", \"to be continued\", or similar text. key_facts and continuity_updates must be visibly supported by the addition and must not be only a character name.\n\nChapter and continuity authority:\n{authority_context}\n\nCurrent chapter summary (compressed record of completed events; do not replay it):\n{summary}\n\nCurrent prose tail (addition must begin after it; never replay it):\n{existing_context}"
        )
    }
}

fn chapter_expansion_existing_context(content: &str, language: &str) -> String {
    let limit = if language_looks_cjk(language) {
        6_000
    } else {
        10_000
    };
    if content.chars().count() <= limit {
        return content.to_string();
    }
    let head = first_n_chars(content, limit / 3);
    let tail = last_n_chars(content, limit.saturating_sub(limit / 3));
    format!("{head}\n\n[...已写正文中段省略...]\n\n{tail}")
}

pub(super) fn chapter_tail_completion_prompt(
    chapter_number: usize,
    title: &str,
    language: &str,
    segment_target: usize,
    summary: &str,
    content: &str,
    issues: &[String],
    authority_context: &str,
) -> String {
    let tail = chapter_tail_context(content, language);
    let authority_context = authority_context.to_string();
    let issue_text = if issues.is_empty() {
        "none".to_string()
    } else {
        issues.join("\n- ")
    };
    if language_looks_cjk(language) {
        format!(
            "补完《{title}》第 {chapter_number} 章的末尾截断。这不是重写整章，只是接在当前正文最后一句后面，补完未完成句并自然收束本章。\n\n\
             要求：只输出 JSON 对象，字段为 addition, summary_delta, key_facts, continuity_updates。addition 必须从当前正文最后半句的后续文字开始，先补完截断句，再追加约 {segment_target} 字以内的自然收束正文；不要重复当前正文，不要另开新章，不要改变标题、主角、已发生事实和结局方向；不得发明合同外的新谜团、新超常现象、新关键物件或新任务；下一章节点只作为禁区，不得在补尾中提前完成。若摘要、正文尾部或问题描述与下方章节权威冲突，以下方章节权威为准；严禁使用“未完待续”“此处省略”“后续剧情”等占位。\n\n\
             章节与连续性权威：\n{authority_context}\n\n\
             检测到的问题：\n- {issue_text}\n\n本章摘要：\n{summary}\n\n当前正文末尾：\n{tail}"
        )
    } else {
        format!(
            "Complete the truncated ending of chapter {chapter_number}, \"{title}\". This is not a full rewrite: continue directly after the current final incomplete sentence, finish it, and close the chapter naturally.\n\n\
             Return only JSON with fields: addition, summary_delta, key_facts, continuity_updates. addition must begin as the continuation of the current unfinished final sentence, then add up to about {segment_target} units of natural closing prose. Do not repeat current prose, start a new chapter, change the title, rename protagonists, or alter established facts. Do not invent an uncontracted mystery, supernatural event, key object, or task. The next outline node is an exclusion boundary and must not be completed in this tail. If the summary, prose tail, or issue text conflicts with the chapter authority below, the authority wins. Do not use placeholders such as omitted/to be continued.\n\n\
             Chapter and continuity authority:\n{authority_context}\n\n\
             Detected issues:\n- {issue_text}\n\nCurrent chapter summary:\n{summary}\n\nTail of current prose:\n{tail}"
        )
    }
}

pub(super) fn chapter_tail_context(content: &str, language: &str) -> String {
    let limit = if language_looks_cjk(language) {
        700
    } else {
        1200
    };
    let chars = content.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(limit);
    chars[start..].iter().collect::<String>()
}

pub(super) fn chapter_output_token_budget(target: usize, language: &str) -> u64 {
    let cjk = language_looks_cjk(language);
    let budget = if cjk {
        (target as u64).saturating_mul(2).saturating_add(2000)
    } else {
        (target as u64).saturating_mul(2) + 800
    };
    budget.clamp(1200, 16_384)
}

pub(super) fn chapter_hard_char_limit(target: usize, language: &str) -> usize {
    let maximum_units = longform_policy::chapter_tier_max_units(target);
    if language_looks_cjk(language) {
        maximum_units.max(512)
    } else {
        // The generation API caps characters while non-CJK quality gates count
        // words. Use the same unit ceiling with a conservative character
        // conversion; the saved-body gate remains the exact final authority.
        maximum_units.saturating_mul(8).max(2400)
    }
}

pub(super) fn language_looks_cjk(language: &str) -> bool {
    let language = language.trim().to_ascii_lowercase();
    language.contains("zh")
        || language.contains("cn")
        || language.contains("chinese")
        || language.contains("中文")
        || language.contains("汉")
        || language.contains("漢")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(content: &str) -> novel_runner::DraftOutput {
        novel_runner::DraftOutput {
            title: "第一章".to_string(),
            content: content.to_string(),
            summary: String::new(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        }
    }

    #[test]
    fn tail_completion_appends_directly_to_unfinished_final_sentence() {
        let mut draft = draft("女人走到他面前，蹲下身，捡起地上的一");
        append_chapter_tail_completion(
            &mut draft,
            ChapterExpansionOutput {
                addition: "枚黑色碎片，轻轻放回他的掌心。她说：“这是你刚刚付出的代价，也是下一道门的钥匙。”".to_string(),
                summary_delta: None,
                key_facts: vec![],
                continuity_updates: vec![],
            },
        );

        assert!(draft.content.contains("捡起地上的一枚黑色碎片"));
        assert!(!draft.content.contains("一\n\n枚黑色碎片"));
        assert!(chapter_body_completion_issue_list(&draft.content).is_empty());
    }

    #[test]
    fn tail_completion_accepts_direct_suffix_that_closes_truncated_cjk_sentence() {
        let existing = "片刻后，一股暖流从胃部升起，迅速扩散至全身。姜闻川感到自己的视力似乎变得更加清晰，原本昏暗的矿洞在他眼中仿佛笼罩了一层淡淡的白光，物体的轮廓变得更加分明。“好机会。”姜闻川握紧手中的断木，眼中闪烁着兴";
        let addition = "奋的光芒。他没有继续莽撞深入，而是把铁背鼠的尸体拖到岩壁边，借着刚获得的感知力辨认洞中的气流。等到远处爪声散去，他才收起白色气膜，确认这片矿洞里还藏着更深的灵脉裂隙。";

        assert_eq!(
            chapter_tail_completion_rejection_reason(existing, addition, "zh-CN"),
            None
        );
    }

    #[test]
    fn expansion_trims_replayed_prefix_before_duplicate_rejection() {
        let existing = "许澜握着备用钥匙，站在灯塔主控台前。雨声敲打玻璃，她听见服务器深处传来一声低低的呼吸。";
        let addition = ChapterExpansionOutput {
            addition: "许澜握着备用钥匙，站在灯塔主控台前。雨声忽然停了一拍，屏幕右下角弹出一行新字：第二次记忆闭环即将开始。她抬头看向窗外，发现海面上的雾灯正按许铮旧日的求救节奏依次亮起。".to_string(),
            summary_delta: None,
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
        };

        let trimmed = trim_overlapping_chapter_expansion(existing, addition, "zh-CN");

        assert!(!trimmed.addition.starts_with("许澜握着备用钥匙"));
        assert!(trimmed.addition.contains("第二次记忆闭环即将开始"));
        assert!(
            chapter_expansion_rejection_reason(existing, &trimmed.addition, "zh-CN").is_none(),
            "{}",
            trimmed.addition
        );
    }

    #[test]
    fn expansion_parser_accepts_add_alias_without_leaking_json_surface() {
        let parsed = parse_chapter_expansion_output(
            r#"{"add":"他推开控制室的大门，雨声被抛在身后。","summary_delta":"进入控制室","key_facts":["进入控制室"],"continuity_updates":["位置变化"]}"#,
            "zh-CN",
        );

        assert_eq!(parsed.addition, "他推开控制室的大门，雨声被抛在身后。");
        assert!(!parsed.addition.contains("\"add\""));
        assert_eq!(parsed.key_facts, vec!["进入控制室".to_string()]);
    }

    #[test]
    fn expansion_parser_rejects_unrecognized_json_object_as_prose() {
        let parsed =
            parse_chapter_expansion_output(r#"{"unknown":"这不应该被当成正文追加。"}"#, "zh-CN");

        assert!(parsed.addition.is_empty());
    }

    #[test]
    fn cjk_repeated_abstract_progression_markers_are_degenerate() {
        let content = "\
少年站在演武场中央，所有人都在等待他的选择。他知道这只是开始，未来的路还很长。
他必须证明自己，也必须证明普通出身的人并非只能仰望强者。
风从石阶间穿过，他再次握紧拳头，命运的转折点已经来到。
旁人议论纷纷，他仍然明白这只是开始，未来的路还很长。
他要证明自己，也要证明那些轻视他的人错了。
长老沉默地看着他，像在等待一个命运的转折点。
夜色落下，少年仍想着成为强者，踏上巅峰。
第二天清晨，他再次告诉自己，这只是开始，未来的路还很长。
他必须继续证明自己，直到所有人承认他的强者之路。
山门之外云雾翻涌，命运的转折点仿佛还在前方等待。";

        assert!(chapter_body_has_degenerate_repetition(content, "zh-CN"));
    }

    #[test]
    fn ordinary_cjk_progression_words_do_not_make_prose_degenerate() {
        let content = "\
谢星衡开始核对校准表，第一组重力读数比昨日低了零点七。商听桥准备拆开传感器外壳，先确认线路有没有改变。值班员按秩序疏散围观者，走廊很快安静下来。
第二次复测开始后，屏幕上的曲线忽然反向抬升。谢星衡没有急着下结论，而是把旧记录调出来逐项比对。商听桥准备接入备用探头，却发现铅封位置已经改变。
雨水敲着玻璃，楼下的交通仍按秩序流动。谢星衡开始追查昨夜的访问日志，从一条被删除的设备编号找到仓库入口。两人下楼时，管理员正在改变货架排列，试图遮住墙后的暗门。
暗门开启，失重感让灯绳缓慢飘起。商听桥让谢星衡先拍下现场，再按取证秩序封存控制盒。返回实验室后，新一轮校准开始，三组数据都指向同一个人为写入的参数。";

        assert!(!chapter_body_has_degenerate_repetition(content, "zh-CN"));
    }

    #[test]
    fn cjk_repeated_long_scene_fragment_twice_is_degenerate() {
        let repeated =
            "辛澈砺本能地后退却见闻澈川的剑光在半空中凝成一道虚影仿佛有一把无形之剑与他交锋。";
        let content = format!(
            "闻澈川踏入遗迹，青光沿着石壁一寸寸亮起。{repeated}他意识到剑诀正在回应自己的选择。{repeated}两人之间的气息骤然冷下去。"
        );

        assert!(chapter_body_has_degenerate_repetition(&content, "zh-CN"));
    }

    #[test]
    fn passed_audit_overrides_stale_recoverable_write_state_for_body_revision() {
        let write_result = serde_json::json!({
            "recoverable": true,
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            },
            "review": {
                "verdict": "needs_revision"
            }
        });
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "issues": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn local_literary_suggestion_does_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "部分比喻略显堆砌，如前后两个意象有轻微重复。"
            ],
            "feedback": "正文通顺，情节连贯，无乱码或外文残片，符合正式章节标准。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "soft literary notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn local_dialogue_tag_suggestion_does_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "部分段落对话引导语略显重复（如多次使用同一角色作为主语）。"
            ],
            "feedback": "正文通顺，情节连贯，无乱码或外文残片，符合正式章节标准。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "local prose-style notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn local_weakness_and_wording_notes_do_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "人物行为逻辑矛盾：局部行动连贯性略弱，紧张感断层。",
                "细节重复：文中多次使用相同形容，用词略显单一。"
            ],
            "feedback": "正文通顺，情节连贯，无乱码或外文残片，符合正式章节标准。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "local weakness/style notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn minor_pacing_and_setup_notes_do_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "节奏稍快：主角从接受建议到直接谈判，中间仅隔一夜，铺垫略显仓促。"
            ],
            "feedback": "整体文笔流畅，氛围营造到位。唯一小瑕疵在于转场略显仓促，除此之外适合进入正式章节。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "minor pacing/setup notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn minor_timeline_anchor_notes_do_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "时间线微小混乱：开头强调“清晨”，结尾仍回到“清晨终于迎来了一丝不一样的色彩”，中间登梯过程未体现时间流逝，但整体节奏尚可。"
            ],
            "feedback": "第一章整体通顺，设定引入清晰，主角人设鲜明，适合进入正式章节。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "minor timeline anchor notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn local_scene_bridge_feedback_does_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "情节跳跃/复述：结尾处主角打电话说‘我有个发现’，紧接着‘电话挂断后的寂静中’，中间缺失了对方的反应和对话细节，导致‘电话挂断’这一动作略显突兀。"
            ],
            "feedback": "整体通顺，情节推进清晰，但存在几处小瑕疵：建议补充余额变化的解释，并细化电话沟通的细节。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "local bridge notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn soft_literary_and_title_relation_notes_do_not_block_quality_audit() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "人物逻辑矛盾：前后描述略显冗余且存在微冲突。",
                "情节推进过快：主角行动过于顺遂，建议增加一个小阻力。",
                "标题与内容关联度弱：标题暗示口才，但正文只在电话谈判中体现。"
            ],
            "feedback": "本章作为开篇，基础设定清晰，重生+系统+商战套路完整。建议后续强化波折。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "soft literary/title relation notes should be warnings, not blockers: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn title_metadata_and_minor_physical_detail_notes_do_not_rewrite_chapter_body() {
        let raw = r#"{
            "passed": false,
            "score": 82,
            "issues": [
                "标题《银鳞号与第一滴血》与内容严重不符，正文中死亡发生在沉船内部，标题未涵盖核心冲突。",
                "细节描写瑕疵：海底船体碎片边缘锋利，但长期浸泡后通常较脆，稍有不符。"
            ],
            "feedback": "整体叙事流畅，氛围营造较好；标题可在 metadata 阶段调整，物理细节可作为后续润色建议。"
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert!(
            audit.authority_conflicts.is_empty(),
            "title metadata and minor realism notes must not trigger body revision: {:?}",
            audit.authority_conflicts
        );
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.score, Some(82));
    }

    #[test]
    fn stored_soft_review_cycle_does_not_force_body_revision() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "chapter_number": 2,
            "issues": [
                "感官描述重复：前文已描述指尖透明、骨节可见，后文再次强调'透明指尖'和'骨节纹路'，存在轻微重复。",
                "节奏冗余：结尾处连续两段均为进入控制台前的铺垫，节奏拖沓，建议合并。"
            ],
            "next_action": "blocked",
            "review": {"verdict": "passed", "locally_validated": true}
        });

        assert!(audit_passed(&audit));
        assert!(!audit_next_action_blocked(&audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn ending_repetition_review_note_does_not_block_approved_body() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "chapter_number": 1,
            "issues": [
                "描写重复：结尾处“垄断帝国，将从这里开始裂开第一道缝隙”与前文“垄断链条，就从这里开始断裂缝”意思高度重复，显得啰嗦。"
            ],
            "next_action": "blocked",
            "review": {"verdict": "passed", "locally_validated": true}
        });

        assert!(audit_passed(&audit));
        assert!(!audit_next_action_blocked(&audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn soft_setting_surface_review_note_does_not_block_approved_body() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "chapter_number": 1,
            "issues": [
                "琉璃契颜色描述存在轻微不一致：前文描述为暗红色，后文描述为青色纹路和幽微蓝光。建议统一初始色调或明确颜色随魔力状态变化的逻辑。"
            ],
            "next_action": "blocked",
            "review": {"verdict": "passed", "locally_validated": true}
        });

        assert!(audit_passed(&audit));
        assert!(!audit_next_action_blocked(&audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn scene_pacing_review_notes_do_not_block_approved_body() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            },
            "review": {
                "verdict": "needs_revision"
            }
        });
        let audit = serde_json::json!({
            "chapter_number": 1,
            "issues": [
                "场景转换突兀：第17段结尾主角走出档案局大门，第18段开头他已在地下商业街，中间缺少走出大楼、进入街道的过渡描写。",
                "段落间存在明显的复述与逻辑跳跃：第10段描述主角在工位上分析数据并得出结论，第11段再次描述他拿起卷宗、戴手套、核对数据，仿佛回到了分析前。",
                "节奏问题：第10-12段在极短篇幅内重复了“发现异常-确认异常-行动”的过程，导致叙事拖沓。"
            ],
            "next_action": "blocked",
            "review": {"verdict": "passed", "locally_validated": true}
        });

        assert!(audit_passed(&audit));
        assert!(!audit_next_action_blocked(&audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn truth_support_metadata_issue_does_not_force_body_revision() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "metadata_gate": {
                "blocking": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": [
                    "truth item lacks visible support in chapter body: 冲突从单线追踪升级为多方混战，局势更加复杂。"
                ]
            }
        });
        let audit = serde_json::json!({
            "chapter_number": 11,
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": []
            },
            "truth_validation": {
                "issues": [
                    "truth item lacks visible support in chapter body: 冲突从单线追踪升级为多方混战，局势更加复杂。"
                ]
            }
        });

        assert!(metadata_gate_needs_repair(&write_result));
        assert!(metadata_repair_allowed_with_audit(&write_result, &audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn local_cleanup_repairs_handheld_object_part_boundary() {
        let content =
            "晏照珩握紧了手中的剑尖滴落的并非鲜血，而是梁澈川灵力溃散后凝结的淡金色血珠。";
        let issue = "chapter body contains likely malformed CJK action-object-part boundary; missing punctuation or duplicated object near: 照珩握紧了手中的剑尖滴落的并非鲜";

        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert_eq!(
            repaired,
            "晏照珩握紧了手中的剑，剑尖滴落的并非鲜血，而是梁澈川灵力溃散后凝结的淡金色血珠。"
        );
    }

    #[test]
    fn local_cleanup_repairs_line_start_missing_open_bracket_timestamps() {
        let content = "03:14:00]状态：接收中。\n  03:14:05]信号源确认。\n[03:14:10]已正常。";
        let issue =
            "日志时间戳缺少左方括号，如'03:14:00]状态：接收中。'，应为'[03:14:00]状态：接收中。'。";

        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert!(repaired.contains("[03:14:00]状态：接收中"), "{repaired}");
        assert!(repaired.contains("  [03:14:05]信号源确认。"), "{repaired}");
        assert_eq!(repaired.matches("[03:14:10]已正常。").count(), 1);
    }

    #[test]
    fn stored_hard_review_cycle_still_blocks_body_revision() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "issues": [
                "结尾部分严重冗余，最后五段像大纲总结而非正文。"
            ],
            "findings": [{
                "code": "body_is_outline_summary",
                "class": "body_integrity",
                "disposition": "hard_block",
                "evidence_grade": "deterministic_invariant",
                "source": "local_test",
                "message": "body is outline summary",
                "authority_fingerprint": "authority",
                "body_fingerprint": "body"
            }],
            "next_action": "blocked",
            "verdict": "needs_revision"
        });

        assert!(!audit_passed(&audit));
        assert!(audit_next_action_blocked(&audit));
        assert!(body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn punctuated_bridge_repair_extracts_ellipsis_target() {
        let issue = "第3段存在明显语病/缺字：'矗立着一根粗壮的黑色金属柱身上缠绕着发光的蓝色符文管线'，'柱身'后缺少标点或动词，应为'金属柱，柱身上...'或'金属柱，其柱身...'。";
        let pairs = local_text_repair_pairs(issue);

        assert!(
            pairs.iter().any(
                |(source, target)| source == "金属柱身上着" && target == "金属柱，柱身上缠绕着"
            ),
            "expected punctuated bridge repair pair, got {pairs:?}"
        );
    }

    #[test]
    fn punctuated_bridge_repair_fixes_truncated_location_particle() {
        let issue = "第3段存在明显语病/缺字：'矗立着一根粗壮的黑色金属柱身上缠绕着发光的蓝色符文管线'，'柱身'后缺少标点或动词，应为'金属柱，柱身上...'或'金属柱，其柱身...'。";
        let content =
            "大厅中央矗立着一根粗壮的黑色金属柱身上着发光的蓝色符文管线，像血管一样搏动着。";
        let repaired = local_text_repair_pairs(issue)
            .into_iter()
            .fold(content.to_string(), |content, (source, target)| {
                apply_local_text_repair_pair(&content, &source, &target)
            });

        assert!(repaired.contains("黑色金属柱，柱身上缠绕着发光的蓝色符文管线"));
    }

    #[test]
    fn local_window_repair_prefers_inserted_character_over_suffix_consumption() {
        let issue = "明显错字/词语拼接错误：'黑色金属柱身上着发光的蓝色符文管线'，缺少动词，应为'柱身上附着'或'柱身连着'。";
        let content = "大厅中央矗立着一根粗壮的黑色金属柱身上着发光的蓝色符文管线。";
        let repaired = local_text_repair_pairs(issue)
            .into_iter()
            .fold(content.to_string(), |content, (source, target)| {
                apply_local_text_repair_pair(&content, &source, &target)
            });

        assert!(
            repaired.contains("黑色金属柱身上附着发光的蓝色符文管线"),
            "repair should insert the missing character without swallowing suffix text: {repaired}"
        );
    }

    #[test]
    fn local_window_repair_stays_inside_quoted_source_fragment() {
        let issue = "明显错字/词语拼接错误：'黑色金属柱身上着发光的蓝色符文管线'，缺少动词，应为'柱身上附着'或'柱身连着'。";
        let content = "大厅中央矗立着一根粗壮的黑色金属柱身上着发光的蓝色符文管线。他看见一个老者从柱子后走出。";
        let repaired = local_text_repair_pairs(issue)
            .into_iter()
            .fold(content.to_string(), |content, (source, target)| {
                apply_local_text_repair_pair(&content, &source, &target)
            });

        assert!(repaired.contains("黑色金属柱身上附着发光的蓝色符文管线"));
        assert!(
            repaired.contains("从柱子后走出"),
            "repair must not rewrite unrelated repeated words outside the quoted source: {repaired}"
        );
    }

    #[test]
    fn malformed_lexical_glue_uses_local_cleanup_pair() {
        let issue =
            "Chinese chapter body contains malformed lexical glue phrase: 香烟雾缭绕间，那张精致却缺乏温度的脸显得格外冷漠...";
        let content = "她手里夹着一支细长的女士香烟雾缭绕间，那张精致却缺乏温度的脸显得格外冷漠。";
        let repaired = local_text_repair_pairs(issue)
            .into_iter()
            .fold(content.to_string(), |content, (source, target)| {
                apply_local_text_repair_pair(&content, &source, &target)
            });

        assert!(repaired.contains("女士香烟，烟雾缭绕间"));
    }

    #[test]
    fn local_revision_preserves_cjk_perception_boundary_for_contextual_revision() {
        let issue = "明显错字/漏字：'侧过头看了他一眼神中闪过一丝不易察觉的复杂情绪'，应为'看了他一眼，眼神中闪过'。";
        let content = "女人停下脚步，侧过头看了他一眼神中闪过一丝不易察觉的复杂情绪。";
        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert_eq!(repaired, content);
    }

    #[test]
    fn repeated_cjk_surface_noise_is_preserved_for_contextual_revision() {
        let issue =
            "正文中存在明显的乱码/OCR残留字符：'皱巴巴的4纸'（出现三次），应为'A4纸'或'纸张'。";
        let content = "钟知棠把皱巴巴的4纸摊开，又看见另一张皱巴巴的4纸。";
        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert_eq!(repaired, content);
        assert!(
            !local_text_repair_pairs(issue).is_empty(),
            "the audit still needs to expose a contextual revision suggestion"
        );
    }

    #[test]
    fn local_revision_repairs_explicit_cjk_orthography_mix() {
        let issue = "正文中'核心开发區'混用了繁体字'區'，与其余简体中文语境不一致";
        let content = "林知远沿着核心开发區的旧围栏走了一圈。";
        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert!(repaired.contains("核心开发区"), "{repaired}");
        assert!(!repaired.contains("核心开发區"), "{repaired}");
        assert!(
            !local_text_repair_pairs(issue).is_empty(),
            "explicit CJK orthography issue should expose a local repair path"
        );
    }

    #[test]
    fn local_revision_repairs_cjk_spliced_adverb_verb_phrase() {
        let issue = "第13段存在明显的词语拼接错误和漏字：'并没有完全被晏照珩深吸收'，应为'被晏照珩吸收'或'被晏照珩深深吸收'。";
        let content =
            "那股喷涌而出的灵气洪流并没有完全被晏照珩深吸收，反而有一部分倒灌入他的体内。";
        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert!(
            !repaired.contains("深吸收"),
            "repair should remove the spliced CJK adverb/verb fragment: {repaired}"
        );
        assert!(
            repaired.contains("被晏照珩吸收") || repaired.contains("被晏照珩深深吸收"),
            "repair should keep a fluent local target phrase: {repaired}"
        );
    }

    #[test]
    fn local_revision_repairs_explicit_should_change_sentence_pair() {
        let issue = "倒数第三段'远处的仙山悬浮在半空，由巨大的锁链连接着地面，那是仙门垄断灵脉的标志。而在仙山之下，凡人聚居的城市如同蝼蚁般渺小，依赖着仙门施舍的有限灵气生存。段朔白抬头望向那座由凡铁铸就的通天塔顶闪烁着微弱的光芒'，最后一句缺少谓语动词，应改为'段朔白抬头望向那座由凡铁铸就的通天塔，塔顶闪烁着微弱的光芒'。";
        let content = "远处的仙山悬浮在半空，由巨大的锁链连接着地面，那是仙门垄断灵脉的标志。而在仙山之下，凡人聚居的城市如同蝼蚁般渺小，依赖着仙门施舍的有限灵气生存。段朔白抬头望向那座由凡铁铸就的通天塔顶闪烁着微弱的光芒，那是灵脉修补阵法的核心。";
        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert!(
            repaired.contains("段朔白抬头望向那座由凡铁铸就的通天塔，塔顶闪烁着微弱的光芒"),
            "应改为 pair should be applied locally instead of forcing a full rewrite: {repaired}"
        );
        assert!(
            only_local_cleanup_issues(
                &serde_json::json!({
                    "status": "needs_revision",
                    "quality_gate": {
                        "findings": [{
                            "code": "body_surface_cleanup",
                            "class": "body_integrity",
                            "disposition": "deterministic_repair",
                            "evidence_grade": "deterministic_invariant",
                            "source": "local_test",
                            "message": "body surface cleanup",
                            "authority_fingerprint": "authority",
                            "body_fingerprint": "body"
                        }]
                    }
                }),
                &serde_json::json!({
                    "review": {
                        "verdict": "needs_revision",
                        "issues": [issue]
                    },
                    "truth_validation": {"issues": []}
                })
            ),
            "explicit should-change pair must stay on the local cleanup path"
        );
    }

    #[test]
    fn local_revision_preserves_malformed_sentence_for_contextual_revision() {
        let issue = "第12段存在明显的句子拼接错误和重复插入字符：'只要拿到灵髓，便有了与这妖孽蝎的利爪擦着他的后背划过，带起一串血珠。'前半句'便有了'未完成，后半句突然插入'蝎的利爪'，导致语义断裂且逻辑混乱。";
        let content = "阮岚白咬紧牙关向前扑去。他深知，只要拿到灵髓，便有了与这妖孽蝎的利爪擦着他的后背划过，带起一串血珠。黑石灵髓在裂隙里亮了一下，他终于看清了阵纹的缺口。";

        let repaired = apply_local_revision_suggestions(content, &[issue.to_string()]);

        assert_eq!(repaired, content);
        assert!(
            !only_local_cleanup_issues(
                &serde_json::json!({"status": "needs_revision", "quality_gate": {"passed": true, "issues": [], "repairable": []}}),
                &serde_json::json!({
                    "review": {
                        "verdict": "needs_revision",
                        "issues": [issue]
                    },
                    "truth_validation": {"issues": []}
                })
            ),
            "a malformed sentence without a safe replacement must use contextual revision"
        );
    }

    #[test]
    fn local_minor_surface_issue_with_overall_pass_is_non_blocking() {
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "第20段‘手中夹着一根烟雾缭绕中’存在语病，疑似漏字，应改为‘手中夹着一根烟，烟雾缭绕中’。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(
            audit_passed(&audit),
            "a single suspected typo/grammar issue should not block when the chapter is otherwise acceptable"
        );
    }

    #[test]
    fn local_minor_layout_wording_issue_is_non_blocking() {
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "标点/排版小误：倒数第6段“老莫浑浊的眼珠微微转动，视线在那枚废石上停留了片刻，嘴角扯出一抹似笑非笑的弧度。他伸出枯瘦的手指尖夹住灵石”，“手指尖”应为“指尖”。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(
            audit_passed(&audit),
            "a single local layout/wording issue should not block an otherwise acceptable chapter"
        );
        assert!(
            !audit_next_action_blocked(&serde_json::json!({
                "next_action": "blocked",
                "issues": [
                    "标点/排版小误：倒数第6段“老莫浑浊的眼珠微微转动，视线在那枚废石上停留了片刻，嘴角扯出一抹似笑非笑的弧度。他伸出枯瘦的手指尖夹住灵石”，“手指尖”应为“指尖”。"
                ],
                "truth_validation": {
                    "issues": []
                }
            })),
            "blocked next_action should be ignored when only non-actionable surface notes remain"
        );
    }

    #[test]
    fn duplicated_cjk_typo_issue_is_local_repairable_and_non_blocking() {
        let issue = "错别字：'语气中带着一一丝玩味'，多了一个'一'字。";
        let pairs = local_text_repair_pairs(issue);
        assert_eq!(
            pairs,
            vec![(
                "语气中带着一一丝玩味".to_string(),
                "语气中带着一丝玩味".to_string()
            )]
        );
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [issue]
            },
            "truth_validation": {
                "issues": []
            }
        });
        assert!(
            audit_passed(&audit),
            "a single duplicated-character typo should not keep an otherwise acceptable chapter blocked"
        );
    }

    #[test]
    fn local_revision_reuses_shared_excessive_cjk_run_cleanup() {
        let content = "警报声变成虚虚虚虚虚的长鸣。";
        let issue = "quality gate: chapter body contains likely malformed CJK prose: repeated character insertion: 虚虚虚虚";

        assert_eq!(
            apply_local_revision_suggestions(content, &[issue.to_string()]),
            "警报声变成虚虚虚的长鸣。"
        );
    }

    #[test]
    fn local_revision_repairs_embedded_lexical_glue_without_rewriting_chapter() {
        let issue = "存在明显的文本重复与冗余插入：'惊雷！秦望澜大喝一声音穿透雨幕。'中'大喝'与'声音'语义重复且句式杂糅，应为'大喝一声'或'声音穿透雨幕'。";
        let content = "雨水砸在戏台上。惊雷！秦望澜大喝一声音穿透雨幕。台下骤然安静。";
        let write_result = json!({"quality_gate": {
            "passed": false,
            "issues": [],
            "findings": [{
                "code": "body_surface_cleanup",
                "class": "body_integrity",
                "disposition": "deterministic_repair",
                "evidence_grade": "deterministic_invariant",
                "source": "local_test",
                "message": "body surface cleanup",
                "authority_fingerprint": "authority",
                "body_fingerprint": "body"
            }]
        }});
        let audit = json!({"review": {"verdict": "needs_revision", "issues": [issue]}});

        assert!(
            only_local_cleanup_issues(&write_result, &audit),
            "a bounded lexical glue repair must not route to a full chapter rewrite"
        );

        let repaired = local_text_repair_pairs(issue)
            .into_iter()
            .fold(content.to_string(), |current, (source, target)| {
                apply_local_text_repair_pair(&current, &source, &target)
            });

        assert_ne!(repaired, content, "the local repair pair was not applied");
        assert!(
            repaired.contains("秦望澜大喝一声，声音穿透雨幕")
                || repaired.contains("秦望澜大喝一声。台下"),
            "unexpected local repair: {repaired}"
        );
        assert!(
            repaired.contains("雨水砸在戏台上") && repaired.contains("台下骤然安静"),
            "local repair must preserve surrounding body text: {repaired}"
        );
    }

    #[test]
    fn local_minor_exposition_observation_with_no_serious_drag_is_non_blocking() {
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "部分段落存在轻微的“设定解说”倾向，但通过主角回忆和搜索动作进行了情节化处理，未造成严重拖沓。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(
            audit_passed(&audit),
            "minor exposition observations that explicitly do not harm the chapter should not trigger a rewrite"
        );
    }

    #[test]
    fn functional_core_term_repetition_observation_is_non_blocking() {
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "术语重复：核心标的在正文中出现频率较高，但上下文中有具体波动、事件和对话支撑，重复具有功能性，未造成阅读疲劳。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(
            audit_passed(&audit),
            "functional repetition that is explicitly justified by the audit should not force a rewrite"
        );
    }

    #[test]
    fn passed_audit_overrides_overused_story_term_quality_issue() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "Chinese chapter body overuses the same story term without enough concrete progression: `江城地产` appears 25 times"
                ],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "术语重复：作为核心标的出现频率较高，但上下文中有具体股价波动、事件和对话支撑，重复具有功能性，未造成阅读疲劳。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(audit_passed(&audit));
        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn soft_word_repetition_and_term_detail_notes_do_not_block_chapter() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = serde_json::json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": [
                    "词语重复：第27段与第30段之间，情绪转换较快，且主角作为主语在短篇幅内出现频率过高，可适当省略主语以增强流畅度。",
                    "逻辑/细节瑕疵：一账双印制度与火漆检查的动作描写略有脱节，此处术语与动作描写可细化。"
                ],
                "feedback": "本章整体叙事流畅，悬念铺设合理，情节推进逻辑基本通顺，但部分转场稍显急促。建议微调细节。"
            },
            "truth_validation": {
                "issues": []
            }
        });

        assert!(
            audit_passed(&audit),
            "soft wording and minor term/detail notes should not force full-body revision"
        );
        assert!(
            !body_revision_required_after_audit(&write_result, &audit),
            "a clean body should be approvable when the audit only contains non-blocking notes"
        );
        assert!(
            !audit_next_action_blocked(&serde_json::json!({
                "next_action": "blocked",
                "issues": [
                    "词语重复：主语在短篇幅内出现频率过高，可适当省略主语以增强流畅度。",
                    "逻辑/细节瑕疵：术语与动作描写略有脱节，可细化。"
                ],
                "truth_validation": {
                    "issues": []
                }
            })),
            "a blocked next_action should not override only soft audit notes"
        );
    }

    #[test]
    fn local_ending_redundancy_and_pacing_notes_do_not_force_full_revision() {
        let write_result = serde_json::json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let soft_issues = [
            "部分描写略有冗余，如连续三个动作短语，稍显刻意。",
            "结尾部分存在明显的叙事倒退与场景复述，导致动作状态在已出门和准备出发之间摇摆。",
            "结构重复冗余：结尾处人物离开地点的动作与前文高度重合，造成叙事停滞。",
            "结尾金句稍显直白，建议精简主角离开过程，使章节结尾更紧凑有力。",
        ];

        for issue in soft_issues {
            let audit = serde_json::json!({
                "review": {
                    "verdict": "passed",
                    "locally_validated": true,
                    "issues": [issue],
                    "feedback": "正文整体通顺，人物性格和设定符合预期，情节推进清晰；该问题属于局部润色建议。"
                },
                "truth_validation": {
                    "issues": []
                }
            });

            assert!(
                audit_passed(&audit),
                "soft local pacing note should not block chapter approval: {issue}"
            );
            assert!(
                !body_revision_required_after_audit(&write_result, &audit),
                "soft local pacing note should not trigger full-body revision: {issue}"
            );
        }
    }

    #[test]
    fn subjective_score_and_advice_do_not_create_a_blocking_finding() {
        let raw = r#"{
            "score": 70,
            "authority_conflicts": [],
            "advisories": ["节奏偏慢，可考虑压缩重复事件。"]
        }"#;

        let audit = parse_llm_quality_audit_output(raw).expect("audit parses");

        assert_eq!(audit.score, Some(70));
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.advisories.len(), 1);
    }

    #[test]
    fn fallback_execution_package_labels_the_next_chapter_event_as_an_exclusion_boundary() {
        let context = serde_json::json!({
            "project": {"title": "测试书", "genre": "悬疑"},
            "story_bible": {
                "narrative_graph": {
                    "chapter_goals": [
                        {"chapter_number": 1, "goal": "主角封存异常数据"},
                        {"chapter_number": 2, "goal": "上级指派两名监察员"}
                    ]
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("zh-CN", "测试书", 1, &context, false, None);

        assert!(package.memo.body.contains("主角封存异常数据"));
        assert!(package.memo.body.contains("下一章边界（只作为禁区"));
        assert!(package.memo.body.contains("上级指派两名监察员"));
        assert!(package.architecture.contains("下一章边界"));
        assert!(package.architecture.contains("上级指派两名监察员"));
    }

    #[test]
    fn fallback_execution_package_reads_current_truth_goal_and_rolling_future_boundary() {
        let context = serde_json::json!({
            "truth_as_of_chapter": {
                "story_state": {
                    "narrative_graph": {
                        "chapter_goals": [{
                            "chapter_number": 7,
                            "goal": "商队因灵石配额排斥岑星澜",
                            "moves_toward_ending": "岑星澜被迫选择新的同行者"
                        }]
                    }
                }
            },
            "rolling_outline_window": [{
                "number": 8,
                "goal": "众人进入荒野寻找失落驿站",
                "expected_turn": "发现驿站曾被人为抹去"
            }]
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("zh-CN", "灵脉行旅", 7, &context, false, None);

        assert!(package.memo.body.contains("商队因灵石配额排斥岑星澜"));
        assert!(package.memo.body.contains("岑星澜被迫选择新的同行者"));
        assert!(package.memo.body.contains("下一章边界（只作为禁区"));
        assert!(package.memo.body.contains("众人进入荒野寻找失落驿站"));
        assert!(package.architecture.contains("众人进入荒野寻找失落驿站"));
    }

    #[test]
    fn fallback_execution_package_reads_explicit_current_goal_when_truth_excludes_planning() {
        let context = serde_json::json!({
            "truth_as_of_chapter": {
                "story_state": {
                    "character_ledger": [{"name": "闻雪渡", "role": "主角"}]
                }
            },
            "current_chapter_goal": [{
                "number": 4,
                "goal": "闻雪渡进入封山矿洞寻找失踪的勘探队",
                "expected_turn": "闻雪渡在矿壁内发现仍在运转的古代升降机"
            }],
            "rolling_outline_window": [{
                "number": 5,
                "goal": "闻雪渡沿升降机进入地下城",
                "expected_turn": "地下城的照明系统因她到来而重启"
            }]
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("zh-CN", "封山旧井", 4, &context, false, None);

        assert!(package.memo.body.contains("进入封山矿洞寻找失踪的勘探队"));
        assert!(package.memo.body.contains("发现仍在运转的古代升降机"));
        assert!(package.memo.body.contains("下一章边界（只作为禁区"));
        assert!(package.memo.body.contains("沿升降机进入地下城"));
        assert_eq!(
            package.new_state_after_chapter,
            "闻雪渡在矿壁内发现仍在运转的古代升降机"
        );
        assert!(!package
            .new_state_after_chapter
            .contains("进入封山矿洞寻找失踪的勘探队"));
    }

    #[test]
    fn fallback_execution_package_preserves_the_authoritative_expected_turn() {
        let context = serde_json::json!({
            "canonical_contract": {
                "characters": [{
                    "canonical_name": "叶承白",
                    "role": "主角"
                }],
                "outline": {
                    "near_chapters": [{
                        "number": 1,
                        "goal": "叶承白从旧钟表提取温度记忆",
                        "expected_turn": "叶承白第一次感受到强烈悲伤，感知能力发生质变"
                    }]
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("zh-CN", "齿轮余温", 1, &context, false, None);

        assert_eq!(
            package.new_state_after_chapter,
            "叶承白第一次感受到强烈悲伤，感知能力发生质变"
        );
    }

    #[test]
    fn generated_execution_package_preserves_model_state_when_context_is_unavailable() {
        let mut package = fallback_chapter_execution_package(
            "zh-CN",
            "断线档案",
            1,
            "not valid json",
            false,
            None,
        );
        package.new_state_after_chapter = "主角保住了唯一的证据".to_string();

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "断线档案",
            1,
            "not valid json",
            false,
            None,
        );

        assert_eq!(
            governed.new_state_after_chapter, "主角保住了唯一的证据",
            "a failed context parse must not erase a model state that has no canonical replacement"
        );
    }

    #[test]
    fn generated_execution_package_cannot_promote_the_next_chapter_into_current_authority() {
        let context = serde_json::json!({
            "canonical_contract": {
                "premise": "岑维声重生后发现一间只在深夜出现的时间当铺。",
                "outline": {
                    "near_chapters": [
                        {
                            "number": 1,
                            "goal": "岑维声确认重生并发现阁楼深夜当铺",
                            "expected_turn": "意识到自己回到过去"
                        },
                        {
                            "number": 2,
                            "goal": "测试规则，典当乏味童年记忆换取第一笔启动资金",
                            "expected_turn": "完成第一次记忆交易"
                        }
                    ]
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "灰塔当铺", 1, &context, false, None);
        package.memo.goal = "确认重生、发现当铺并完成第一次记忆典当换取启动资金".to_string();
        package.scene_goal = package.memo.goal.clone();
        package.irreversible_event = "完成第一次记忆交易".to_string();

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "灰塔当铺",
            1,
            &context,
            false,
            None,
        );

        assert!(governed.memo.goal.contains("确认重生"));
        assert!(!governed.memo.goal.contains("第一次记忆典当"));
        assert!(governed.irreversible_event.is_empty());
    }

    #[test]
    fn generated_execution_package_keeps_model_scenes_but_uses_canonical_goal_boundary() {
        let context = serde_json::json!({
            "canonical_contract": {
                "premise": "一名修士在灵脉枯竭后寻找旧宗门遗址。",
                "outline": {
                    "near_chapters": [
                        {
                            "number": 3,
                            "goal": "主角进入废弃山门并确认灵脉残响",
                            "expected_turn": "找到尚未熄灭的阵眼"
                        },
                        {
                            "number": 4,
                            "goal": "沿阵眼追查失踪守脉人的去向",
                            "expected_turn": "发现守脉人留下的求救刻痕"
                        }
                    ]
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "枯脉山门", 3, &context, false, None);
        package.memo.goal = "在雨夜搜索废弃山门".to_string();
        package.scene_goal = "用五个场景探索山门".to_string();
        package.architecture = "模型给出的五个具体场景".to_string();
        package.new_state_after_chapter = "主角进入废弃山门后开始谨慎观察周围".to_string();

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "枯脉山门",
            3,
            &context,
            false,
            None,
        );

        assert!(governed.memo.goal.contains("进入废弃山门"));
        assert!(governed.scene_goal.contains("进入废弃山门"));
        assert!(governed.architecture.contains("下一章边界"));
        assert!(governed.architecture.contains("模型给出的五个具体场景"));
        assert_eq!(governed.new_state_after_chapter, "找到尚未熄灭的阵眼");
    }

    #[test]
    fn generated_execution_package_removes_events_from_later_rolling_boundaries() {
        let context = serde_json::json!({
            "canonical_contract": {
                "premise": "修仙界的灵气由众生因果凝结。",
                "outline": {
                    "near_chapters": [
                        {
                            "number": 1,
                            "goal": "谢栖禾发现灵气消耗异象",
                            "expected_turn": "谢栖禾吸收第一缕因果灵气"
                        },
                        {
                            "number": 2,
                            "goal": "秦星朔发现宗门灵泉枯竭",
                            "expected_turn": "秦星朔强行抽取灵气后修为跌落"
                        },
                        {
                            "number": 3,
                            "goal": "钟予原降临寒潭镇",
                            "expected_turn": "谢栖禾发现体内因果债正加速增长"
                        }
                    ]
                }
            },
            "next_chapter_boundary": [{
                "number": 2,
                "goal": "秦星朔发现宗门灵泉枯竭",
                "expected_turn": "秦星朔强行抽取灵气后修为跌落"
            }],
            "rolling_outline_window": [
                {
                    "number": 2,
                    "goal": "秦星朔发现宗门灵泉枯竭",
                    "expected_turn": "秦星朔强行抽取灵气后修为跌落"
                },
                {
                    "number": 3,
                    "goal": "钟予原降临寒潭镇",
                    "expected_turn": "谢栖禾发现体内因果债正加速增长"
                }
            ]
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "重塑灵气法则", 1, &context, false, None);
        package.architecture =
            "谢栖禾意识到体内因果债正加速增长，并把这一变化记录下来。".to_string();
        package.character_change = "谢栖禾发现体内因果债正加速增长".to_string();

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "重塑灵气法则",
            1,
            &context,
            false,
            None,
        );

        assert!(!governed.architecture.contains("因果债正加速增长"));
        assert!(governed.character_change.is_empty());
        assert!(governed.architecture.contains("下一章边界"));
    }

    #[test]
    fn rolling_outline_preserves_existing_next_boundary_and_only_adds_missing_nodes() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 100_000,
                "chapter_unit_target": 2_500,
                "outline": {
                    "near_chapters": [
                        {
                            "number": 3,
                            "goal": "主角确认账本被替换",
                            "expected_turn": "取得伪造页的压痕证据"
                        },
                        {
                            "number": 4,
                            "goal": "沿压痕寻找印刷作坊",
                            "expected_turn": "锁定作坊夜班经手人"
                        }
                    ]
                }
            },
            "next_chapter_boundary": [
                {
                    "number": 4,
                    "goal": "沿压痕寻找印刷作坊",
                    "expected_turn": "锁定作坊夜班经手人"
                }
            ],
            "rolling_outline_window": [
                {
                    "number": 4,
                    "goal": "沿压痕寻找印刷作坊",
                    "expected_turn": "锁定作坊夜班经手人"
                },
                {
                    "number": 5,
                    "goal": "跟踪夜班经手人的交货路线",
                    "expected_turn": "发现货物被送入市政档案馆"
                },
                {
                    "number": 6,
                    "goal": "潜入档案馆核对原始登记",
                    "expected_turn": "确认换页命令来自清退专案组"
                }
            ]
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "旧账夜印", 3, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(4),
                goal: "覆盖旧权威的错误目标".to_string(),
                expected_turn: "覆盖旧权威的错误变化".to_string(),
            },
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(5),
                goal: "覆盖第五章旧权威的错误目标".to_string(),
                expected_turn: "覆盖第五章旧权威的错误变化".to_string(),
            },
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(6),
                goal: "覆盖第六章旧权威的错误目标".to_string(),
                expected_turn: "覆盖第六章旧权威的错误变化".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "旧账夜印",
            3,
            &context,
            false,
            None,
        );

        assert_eq!(governed.future_chapters.len(), 3);
        assert_eq!(governed.future_chapters[0].number, Some(4));
        assert_eq!(governed.future_chapters[0].goal, "沿压痕寻找印刷作坊");
        assert_eq!(governed.future_chapters[1].number, Some(5));
        assert_eq!(governed.future_chapters[1].goal, "跟踪夜班经手人的交货路线");
        assert_eq!(governed.future_chapters[2].number, Some(6));
        assert_eq!(governed.future_chapters[2].goal, "潜入档案馆核对原始登记");
    }

    #[test]
    fn rolling_outline_stops_at_expected_book_length_and_drops_duplicate_nodes() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 10_000,
                "chapter_unit_target": 2_500,
                "outline": {"near_chapters": []}
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "四章短篇", 3, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(4),
                goal: "公开账本原件".to_string(),
                expected_turn: "产权清退被永久叫停".to_string(),
            },
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(5),
                goal: "不应超出全书长度".to_string(),
                expected_turn: "不应保存".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "四章短篇",
            3,
            &context,
            false,
            None,
        );

        assert_eq!(governed.future_chapters.len(), 1);
        assert_eq!(governed.future_chapters[0].number, Some(4));
    }

    #[test]
    fn rolling_outline_drops_a_future_goal_that_replays_the_current_required_turn() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 100_000,
                "chapter_unit_target": 2_500,
                "outline": {
                    "near_chapters": [{
                        "number": 5,
                        "goal": "遭遇巡逻艇的近距离火力试探",
                        "expected_turn": "双方在云层中展开第一次实质性的机动对抗"
                    }]
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "云端勘探", 5, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(6),
                goal: "在能量波动区遭遇城邦武装的第一次实质性机动对抗".to_string(),
                expected_turn: "勘探队被迫改变原定航线".to_string(),
            },
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(7),
                goal: "勘探队沿新航线寻找安全着陆点".to_string(),
                expected_turn: "叶维遥发现废弃补给塔仍有能源反应".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "云端勘探",
            5,
            &context,
            false,
            None,
        );

        assert!(governed.future_chapters.is_empty());
    }

    #[test]
    fn rolling_outline_drops_future_seed_with_existing_outline_placeholder() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 100_000,
                "chapter_unit_target": 2_500,
                "outline": {
                    "near_chapters": [{
                        "number": 5,
                        "goal": "勘探队进入异常云层",
                        "expected_turn": "叶维遥确认云层会干扰高度读数"
                    }]
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "云端勘探", 5, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(6),
                goal: "追查高度读数的异常来源".to_string(),
                expected_turn: "本章末不可逆变化".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "云端勘探",
            5,
            &context,
            false,
            None,
        );

        assert!(governed.future_chapters.is_empty());
    }

    #[test]
    fn rolling_outline_rejects_later_volume_contract_copied_into_current_volume() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 100_000,
                "chapter_unit_target": 2_500,
                "outline": {"near_chapters": []}
            },
            "authority": {
                "working_context": {
                    "project": {
                        "volumes": [
                            {
                                "id": "volume-0001",
                                "start_chapter": 1,
                                "end_chapter": 14,
                                "objective": "在废墟中收集核心组件并建立初步生存据点",
                                "ending_change": "敌方察觉核心并向废墟边缘进发"
                            },
                            {
                                "id": "volume-0002",
                                "start_chapter": 15,
                                "end_chapter": 28,
                                "objective": "建立防御体系并对抗敌方多次突袭",
                                "ending_change": "核心修复达到临界点并引来更深异变"
                            }
                        ]
                    }
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "灰雾核心", 3, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(4),
                goal: "建立防御体系并对抗敌方多次突袭".to_string(),
                expected_turn: "核心修复达到临界点并引来更深异变".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "灰雾核心",
            3,
            &context,
            false,
            None,
        );

        assert!(governed.future_chapters.is_empty());
    }

    #[test]
    fn rolling_outline_does_not_collapse_the_current_volume_objective_into_one_chapter() {
        let context = serde_json::json!({
            "project": {
                "volumes": [{
                    "id": "volume-0001",
                    "start_chapter": 1,
                    "end_chapter": 8,
                    "objective": "收集证据并建立完整生存据点",
                    "ending_change": "敌方确认据点坐标并发动围攻"
                }]
            }
        });
        let seed = crate::tool::writing::creation_contract_model::ChapterSeedContract {
            number: Some(4),
            goal: "收集证据并建立完整生存据点".to_string(),
            expected_turn: "主角找到下一份账册".to_string(),
        };

        assert!(!rolling_seed_stays_within_volume_scope(&context, &seed));
    }

    #[test]
    fn rolling_outline_rejects_a_chapter_outside_all_declared_volume_ranges() {
        let context = serde_json::json!({
            "project": {
                "volumes": [
                    {"id": "volume-0001", "start_chapter": 1, "end_chapter": 4},
                    {"id": "volume-0002", "start_chapter": 6, "end_chapter": 10}
                ]
            }
        });
        let seed = crate::tool::writing::creation_contract_model::ChapterSeedContract {
            number: Some(5),
            goal: "调查失踪证人".to_string(),
            expected_turn: "主角取得证人的旧录音".to_string(),
        };

        assert!(!rolling_seed_stays_within_volume_scope(&context, &seed));
    }

    #[test]
    fn rolling_outline_allows_current_volume_ending_only_at_its_last_chapter() {
        let context = serde_json::json!({
            "canonical_contract": {
                "target_units": 40_000,
                "chapter_unit_target": 2_500,
                "outline": {"near_chapters": []}
            },
            "project": {
                "volumes": [
                    {
                        "id": "volume-0001",
                        "start_chapter": 1,
                        "end_chapter": 8,
                        "objective": "收集证据并建立据点",
                        "ending_change": "敌方确认据点坐标并发动围攻"
                    },
                    {
                        "id": "volume-0002",
                        "start_chapter": 9,
                        "end_chapter": 16,
                        "objective": "守住据点并公开证据",
                        "ending_change": "旧秩序失去资源垄断"
                    }
                ]
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "废墟证据", 7, &context, false, None);
        package.future_chapters = vec![
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(8),
                goal: "主角把最后一份坐标痕迹带回据点".to_string(),
                expected_turn: "敌方确认据点坐标并发动围攻".to_string(),
            },
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "废墟证据",
            7,
            &context,
            false,
            None,
        );

        assert_eq!(governed.future_chapters.len(), 1);
        assert_eq!(governed.future_chapters[0].number, Some(8));
    }

    #[test]
    fn generated_state_authority_drops_ungrounded_and_future_chapter_inventions() {
        let context = serde_json::json!({
            "canonical_contract": {
                "premise": "唐承原受托寻找失踪的谢知序。",
                "outline": {
                    "near_chapters": [
                        {
                            "number": 1,
                            "goal": "唐承原接受谢知序失踪委托，初次接触南承声",
                            "expected_turn": "南承声暗示谢知序背叛了约定"
                        },
                        {
                            "number": 2,
                            "goal": "唐承原调查谢知序最后踪迹，发现服务器异常",
                            "expected_turn": "服务器数据被远程清空，只留一个时间戳"
                        }
                    ]
                }
            }
        })
        .to_string();
        let mut package =
            fallback_chapter_execution_package("zh-CN", "危机顾问", 1, &context, false, None);
        package.reveal = "电脑有远程操作痕迹，暗示失踪是计划内的".to_string();
        package.relationship_change = "唐承原与谢知序建立调查者关系".to_string();
        package.resource_delta = "唐承原获得U盘中的时间戳线索".to_string();
        package.new_state_after_chapter = "唐承原进入高度戒备状态".to_string();
        package.hook_opened = vec![
            "谢知序失踪的真正原因".to_string(),
            "U盘中的时间戳含义".to_string(),
            "南承声为何如此急切".to_string(),
        ];

        let governed = govern_generated_execution_package(
            package,
            "zh-CN",
            "危机顾问",
            1,
            &context,
            false,
            None,
        );

        assert!(governed.reveal.is_empty());
        assert!(governed.relationship_change.is_empty());
        assert!(governed.resource_delta.is_empty());
        assert_eq!(
            governed.new_state_after_chapter,
            "南承声暗示谢知序背叛了约定"
        );
        assert_eq!(
            governed.hook_opened,
            vec!["谢知序失踪的真正原因".to_string()]
        );
    }

    #[test]
    fn english_common_bigrams_do_not_erase_a_current_chapter_field() {
        assert!(!governance::text_consumes_future_chapter(
            "The investigator reviews the insurance timestamp.",
            "The investigator reviews the insurance timestamp in the current chapter.",
            "The investigator brings the timestamp to the public hearing.",
            false,
        ));
        assert!(governance::text_consumes_future_chapter(
            "The investigator brings the timestamp to the public hearing.",
            "The investigator reviews the insurance timestamp in the current chapter.",
            "The investigator brings the timestamp to the public hearing.",
            false,
        ));
    }
}
#[test]
fn expansion_context_includes_already_completed_opening_events() {
    let content = format!(
        "开头已完成封存数据。{}结尾停在等待复核。",
        "中段调查记录。".repeat(180)
    );
    let context = chapter_expansion_existing_context(&content, "zh-CN");

    assert!(context.contains("开头已完成封存数据"));
    assert!(context.contains("结尾停在等待复核"));
    assert!(context.chars().count() <= 6_100);
}
