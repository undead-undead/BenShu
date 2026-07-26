//! Work-title and chapter-title naming authority adapters.
//!
//! Title quality callers should go through this module instead of reaching into
//! writing policy directly.

use super::title_lexicon;
use super::title_policy;

#[derive(Debug, Clone)]
pub(crate) struct BookTitleEvidence {
    pub(crate) target: String,
    pub(crate) story_evidence: String,
}

impl BookTitleEvidence {
    pub(crate) fn new(target: impl Into<String>, story_evidence: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            story_evidence: story_evidence.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BookTitleCandidate {
    pub(crate) title: String,
    pub(crate) rationale: String,
}

impl BookTitleCandidate {
    pub(crate) fn new(title: impl Into<String>, rationale: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rationale: rationale.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BookTitleDecision {
    pub(crate) accepted: bool,
    pub(crate) selected: Option<BookTitleCandidate>,
    pub(crate) reasons: Vec<String>,
}

impl BookTitleDecision {
    #[cfg(test)]
    pub(crate) fn selected_title(&self) -> Option<&str> {
        self.selected
            .as_ref()
            .map(|candidate| candidate.title.as_str())
    }
}

pub(crate) fn title_contract_basis_issue(
    title: &str,
    target: &str,
    rationale: &str,
    story_evidence: &str,
) -> Option<String> {
    title_policy::title_contract_basis_issue(title, target, rationale, story_evidence)
}

pub(crate) fn title_formality_issue(title: &str, target: &str) -> Option<String> {
    title_policy::title_formality_issue(title, target)
}

pub(crate) fn title_anchor_tokens(title: &str) -> Vec<String> {
    title_policy::title_anchor_tokens(title)
}

pub(crate) fn title_rationale_is_concrete(rationale: &str, title: &str) -> bool {
    title_policy::title_rationale_is_concrete(rationale, title)
}

pub(crate) fn select_book_title_candidate_decision(
    candidates: impl IntoIterator<Item = BookTitleCandidate>,
    evidence: &BookTitleEvidence,
) -> BookTitleDecision {
    let mut rejected_reasons = Vec::new();
    let mut selected: Option<(BookTitleCandidate, u16)> = None;

    for candidate in candidates {
        let title = candidate.title.trim();
        if title.is_empty() {
            continue;
        }
        if let Some(issue) = title_formality_issue(title, &evidence.target).or_else(|| {
            title_contract_basis_issue(
                title,
                &evidence.target,
                &candidate.rationale,
                &evidence.story_evidence,
            )
        }) {
            rejected_reasons.push(format!("{title}: {issue}"));
            continue;
        }
        let score = book_title_candidate_score(&candidate, &evidence.story_evidence);
        if selected
            .as_ref()
            .is_none_or(|(_, selected_score)| score > *selected_score)
        {
            selected = Some((candidate, score));
        }
    }

    if let Some((selected, _score)) = selected {
        return BookTitleDecision {
            accepted: true,
            selected: Some(BookTitleCandidate {
                title: selected.title.trim().to_string(),
                rationale: selected.rationale.trim().to_string(),
            }),
            reasons: Vec::new(),
        };
    }

    BookTitleDecision {
        accepted: false,
        selected: None,
        reasons: rejected_reasons,
    }
}

/// Ranks already-valid titles by how directly their words are supported by the
/// current story. This score has no pass threshold and never blocks a contract.
fn book_title_candidate_score(candidate: &BookTitleCandidate, story_evidence: &str) -> u16 {
    let title = candidate.title.trim();
    let rationale = candidate.rationale.trim();
    let anchors = title_anchor_tokens(title);
    let evidence_hits = anchors
        .iter()
        .filter(|anchor| story_evidence.contains(anchor.as_str()))
        .count() as u16;
    let rationale_hits = anchors
        .iter()
        .filter(|anchor| rationale.contains(anchor.as_str()))
        .count() as u16;

    evidence_hits * 3 + rationale_hits * 2 + u16::from(rationale.contains(title))
}

pub(crate) fn declared_book_title_candidates_from_contract_evidence(
    story_evidence: &str,
) -> Vec<BookTitleCandidate> {
    let story_evidence = story_evidence.trim();
    if story_evidence.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for title in declared_book_title_candidates(story_evidence) {
        push_story_title_candidate(&mut candidates, &title, story_evidence);
    }
    candidates.truncate(8);
    candidates
}

fn declared_book_title_candidates(story_evidence: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in story_evidence.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((_, value)) = trimmed.split_once(['：', ':']) {
            let label = trimmed
                .split(['：', ':'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if label.contains("书名候选")
                || label.contains("标题候选")
                || label.contains("候选书名")
                || label.contains("title_candidates")
                || label.contains("title candidates")
                || label == "书名"
                || label == "标题"
                || label == "title"
            {
                for candidate in split_declared_title_candidates(value) {
                    if !out.iter().any(|known| known == &candidate) {
                        out.push(candidate);
                    }
                }
            }
        }
    }
    out
}

fn split_declared_title_candidates(value: &str) -> Vec<String> {
    value
        .split(['；', ';', '、', ',', '，', '/', '|'])
        .filter_map(|candidate| {
            let candidate = candidate
                .trim()
                .trim_matches(|ch| {
                    matches!(
                        ch,
                        '《' | '》'
                            | '"'
                            | '\''
                            | '“'
                            | '”'
                            | '['
                            | ']'
                            | '【'
                            | '】'
                            | '-'
                            | '*'
                            | ' '
                    )
                })
                .trim();
            if candidate.is_empty() {
                None
            } else {
                Some(candidate.to_string())
            }
        })
        .collect()
}

fn push_story_title_candidate(
    out: &mut Vec<BookTitleCandidate>,
    candidate: &str,
    story_evidence: &str,
) {
    push_story_title_candidate_with_rationale(
        out,
        candidate,
        contract_evidence_title_rationale(candidate, story_evidence),
    );
}

fn push_story_title_candidate_with_rationale(
    out: &mut Vec<BookTitleCandidate>,
    candidate: &str,
    rationale: String,
) {
    let title = candidate.trim();
    let len = title.chars().count();
    if !(3..=14).contains(&len)
        || out.iter().any(|known| known.title == title)
        || title_formality_issue(title, "书名").is_some()
    {
        return;
    }
    out.push(BookTitleCandidate::new(title, rationale));
}

fn contract_evidence_title_rationale(title: &str, story_evidence: &str) -> String {
    if rationale_token_is_occupation_payoff_fragment(title) {
        return "当前候选像职业/身份标签和泛化爽点词的机械拼接，需要改用合同里的具体物件、地点、制度漏洞、关键事件或结局反转来命名。".to_string();
    }
    let anchors = title_rationale_anchor_tokens(title);
    let anchor_text = if anchors.is_empty() {
        title.to_string()
    } else {
        anchors.join("、")
    };
    let basis = story_evidence
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && anchors.iter().any(|token| line.contains(token.as_str())))
        .unwrap_or(story_evidence);
    let basis = compact_title_rationale_story_basis(basis);
    format!(
        "{anchor_text}来自合同证据“{basis}”；书名《{title}》以这个具体锚点作为读者入口，并指向主线代价、关键行动或终局兑现。"
    )
}

fn compact_title_rationale_story_basis(basis: &str) -> String {
    let compact = basis
        .trim()
        .replace(['\n', '\r', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = compact.chars().take(80).collect::<String>();
    if compact.chars().count() > out.chars().count() {
        out.push('…');
    }
    out
}

fn title_rationale_anchor_tokens(title: &str) -> Vec<String> {
    let title = title.trim();
    if rationale_token_is_occupation_payoff_fragment(title) {
        return vec![title.to_string()];
    }
    let mut anchors = title_anchor_tokens(title)
        .into_iter()
        .filter(|token| title_rationale_token_looks_useful(token))
        .collect::<Vec<_>>();
    remove_title_rationale_subfragments(&mut anchors);
    if anchors_look_like_overlapping_title_ngrams(title, &anchors) {
        anchors.clear();
        anchors.push(title.to_string());
    }
    if anchors.len() > 3 {
        anchors.clear();
        anchors.push(title.to_string());
    }
    anchors
}

fn anchors_look_like_overlapping_title_ngrams(title: &str, anchors: &[String]) -> bool {
    let title_len = title.chars().count();
    if title_len > 10 || anchors.len() < 2 {
        return false;
    }
    let title_chars = title.chars().collect::<Vec<_>>();
    anchors.iter().all(|anchor| {
        let anchor_chars = anchor.chars().collect::<Vec<_>>();
        !anchor_chars.is_empty()
            && anchor_chars.len() < title_chars.len()
            && title_chars
                .windows(anchor_chars.len())
                .any(|window| window == anchor_chars.as_slice())
    })
}

fn title_rationale_token_looks_useful(token: &str) -> bool {
    let token = token.trim();
    let len = token.chars().count();
    if !(2..=6).contains(&len) {
        return false;
    }
    if token
        .chars()
        .any(|ch| matches!(ch, '是' | '本' | '为' | '把' | '将' | '让'))
    {
        return false;
    }
    if rationale_token_is_occupation_payoff_fragment(token) {
        return false;
    }
    true
}

fn rationale_token_is_occupation_payoff_fragment(token: &str) -> bool {
    let payoff_terms = [
        "破局", "逆袭", "翻盘", "崛起", "登顶", "觉醒", "打脸", "巅峰",
    ];
    let Some(payoff) = payoff_terms.iter().find(|payoff| token.ends_with(**payoff)) else {
        return false;
    };
    let head = token.trim_end_matches(*payoff);
    [
        "外卖",
        "职员",
        "员工",
        "打工",
        "社畜",
        "赘婿",
        "保安",
        "司机",
        "店员",
        "实习生",
        "底层",
        "草根",
        "凡人",
        "废柴",
        "小人物",
        "普通人",
    ]
    .iter()
    .any(|term| head.contains(*term))
}

fn remove_title_rationale_subfragments(tokens: &mut Vec<String>) {
    let originals = tokens.clone();
    tokens.retain(|token| {
        !originals.iter().any(|other| {
            other != token
                && other.chars().count() > token.chars().count()
                && other.contains(token.as_str())
        })
    });
}

pub(crate) fn book_title_candidate_rationale_from_story_evidence(
    title: &str,
    story_evidence: &str,
) -> String {
    contract_evidence_title_rationale(title, story_evidence)
}

pub(crate) fn generated_project_title_looks_stale_for_task(task: &str, title: &str) -> bool {
    title_language_mismatch(task, title)
        || title_looks_like_workflow_instruction(title)
        || title_looks_like_control_surface(title)
}

pub(crate) fn title_language_mismatch(task: &str, title: &str) -> bool {
    prefers_chinese_output(task) && !has_cjk(title)
}

pub(crate) fn title_looks_like_control_surface(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let markers = [
        "original user request",
        "delegated task",
        "workflow",
        "artifact",
        "entities",
        "continuity",
        "rules",
        "contract",
        "characters",
        "chapter",
        "project setup",
        "title",
        "language",
        "重新生成",
        "生成一个",
        "随机",
        "不要",
        "不能",
        "例如",
        "比如",
        "风格",
        "重复",
        "新书名",
        "更有",
    ];
    markers
        .iter()
        .any(|marker| lowered.contains(marker) || trimmed.contains(marker))
        || title_lexicon::title_meta_discussion_markers()
            .iter()
            .any(|marker| {
                trimmed.contains(marker) || lowered.contains(&marker.to_ascii_lowercase())
            })
}

pub(crate) fn prefers_chinese_output(task: &str) -> bool {
    if explicitly_requests_english(task) {
        return false;
    }
    let lowered = task.to_ascii_lowercase();
    has_cjk(task)
        || lowered.contains("in chinese")
        || lowered.contains("write chinese")
        || lowered.contains("chinese language")
        || lowered.contains("language: chinese")
        || lowered.contains("language is chinese")
}

fn explicitly_requests_english(task: &str) -> bool {
    let lowered = task.to_ascii_lowercase();
    let negated_english = [
        "不要英文",
        "不能英文",
        "不要使用英文",
        "不使用英文",
        "禁止英文",
        "严禁英文",
        "不要英语",
        "no english",
        "not english",
        "do not use english",
    ];
    if negated_english
        .iter()
        .any(|marker| task.contains(marker) || lowered.contains(marker))
    {
        return false;
    }
    let explicit_english = [
        "用英文",
        "使用英文",
        "英文写",
        "英语写",
        "英文小说",
        "英语小说",
        "in english",
        "write english",
        "english language",
        "language: english",
        "language is english",
    ];
    explicit_english
        .iter()
        .any(|marker| task.contains(marker) || lowered.contains(marker))
}

fn title_looks_like_workflow_instruction(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    lowered.contains("json")
        || lowered.contains("schema")
        || lowered.contains("output")
        || lowered.contains("field")
        || lowered.contains("prompt")
        || lowered.contains("workflow")
        || lowered.contains("contract")
        || trimmed.contains("用户")
        || trimmed.contains("任务")
        || trimmed.contains("合同")
        || trimmed.contains("字段")
        || trimmed.contains("输出")
}

fn has_cjk(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_title_decision_prefers_better_supported_valid_candidate() {
        let evidence = BookTitleEvidence::new(
            "书名",
            "都市玄幻。霓虹是城市灵能的载体，城市核心控制居民感知；主角最终关闭城市核心，街区只剩余烬。书名候选包括《城市核心》。",
        );
        let decision = select_book_title_candidate_decision(
            [
                BookTitleCandidate::new(
                    "霓虹余烬",
                    "霓虹来自城市灵能载体，余烬对应结局中关闭城市核心后的残留景象。",
                ),
                BookTitleCandidate::new(
                    "城市核心",
                    "城市核心是控制居民感知的关键设施，也是在结局中被主角关闭的具体对象。",
                ),
            ],
            &evidence,
        );

        assert!(
            decision.accepted,
            "decision should accept a supported title: {decision:?}"
        );
        assert_eq!(decision.selected_title(), Some("城市核心"));
    }

    #[test]
    fn declared_book_title_candidates_do_not_invent_compounded_fragments() {
        let evidence =
            "主角被诬陷遭通缉，凭借前世记忆和当代科技手段逆袭重生，最终掌握城市核心资源。";
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();

        assert!(
            !candidates.iter().any(|candidate| candidate == "握城借前世记"),
            "declared candidates must not glue unrelated story tokens into a synthetic book title: {candidates:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_only_reuse_declared_candidates() {
        let evidence = "未来学院用星环暗榜垄断晋级，主角追查被夺走的天穹校印，终局公开暗榜证据并夺回校印。\n书名候选：夺回天穹校印；星环暗榜；校印见光";
        let decision = select_book_title_candidate_decision(
            declared_book_title_candidates_from_contract_evidence(evidence),
            &BookTitleEvidence::new("书名", evidence),
        );

        assert!(
            decision.accepted,
            "declared title candidates should select from declared LLM/contract candidates: {decision:?}"
        );
        let selected = decision.selected_title().unwrap_or_default();
        assert!(
            ["夺回天穹校印", "星环暗榜", "校印见光"].contains(&selected),
            "title should come from declared candidates only: {selected}"
        );
    }

    #[test]
    fn declared_book_title_candidates_reject_mechanical_action_chain_from_contract_sentence() {
        let evidence = "主角因意外获得源契传承，从此以命运换修为，最终公开源核漏洞并打破宗门垄断。";
        let decision = select_book_title_candidate_decision(
            [BookTitleCandidate::new(
                "运换修破局",
                "《运换修破局》中的运换修来自命运换修为，破局来自终局打破困局。",
            )],
            &BookTitleEvidence::new("书名", evidence),
        );

        assert!(
            !decision.accepted,
            "mechanical action-chain titles should be rejected: {decision:?}"
        );
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.contains("运换修")),
            "declared candidates must not keep sentence-clipped action chains: {candidates:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_do_not_clip_genre_selling_point_fragments() {
        let evidence = "题材：都市爽文。主角宋庭晚出身贫寒，因一次偶然机遇获得关键资源，从此在危机四伏的都市中步步为营，最终登顶城市权力之巅。";
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();

        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.contains("市爽")),
            "declared title candidates must not clip genre/selling-point words into anchors: {candidates:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_do_not_clip_connector_tail_fragments() {
        let evidence = "追查阶段围绕主线因果推进证据、资源与关系代价；终局阶段兑现结局承诺，让主角夺回公司控制权并公开行业黑幕。";
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();

        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.contains("进证据")),
            "declared title candidates must not clip narrative connector tails into anchors: {candidates:?}"
        );
        let decision = select_book_title_candidate_decision(
            [BookTitleCandidate::new(
                "进证据破局",
                "来自推进证据和破局爽点。",
            )],
            &BookTitleEvidence::new("书名", evidence),
        );
        assert!(
            !decision.accepted,
            "clipped narrative connector payoff titles should be rejected: {decision:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_do_not_create_title_from_contract_evidence() {
        let evidence = "题材：都市爽文。主角段桥晚因车祸失忆，沦为城市边缘的普通职员，却意外激活祖传玉佩中的记忆碎片，发现自己是百年前商业帝国唯一继承人。从此，他以小博大，利用信息差与天赋，在商战、情场与权谋中屡战屡胜，从月薪三千到富可敌国。主线：失忆车祸->激活玉佩->初露锋芒->遭遇强敌->联手破局->身份曝光->家族内斗->清除异己->登顶巅峰->真相大白。结局：主角彻底整合家族势力，清除所有敌对势力，成为都市隐形之王，并找回全部记忆。";
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();
        assert!(
            candidates.is_empty(),
            "declared candidates must not invent a book title from story evidence: {candidates:?}"
        );
    }

    #[test]
    fn book_title_candidates_do_not_accept_clipped_behind_phrase_fragments() {
        let evidence = "在永不停歇的蒸汽都市中，一名机械义体修理工通过修复关键齿轮，揭开城市动力源背后的谋杀真相。";
        let decision = select_book_title_candidate_decision(
            [BookTitleCandidate::new(
                "公开城市动力源背",
                book_title_candidate_rationale_from_story_evidence("公开城市动力源背", evidence),
            )],
            &BookTitleEvidence::new("书名", evidence),
        );
        assert!(
            !decision.accepted,
            "sentence fragments cut from 背后 clauses must not become confirmable book titles: {decision:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_do_not_prefer_abstract_resource_payoff() {
        let evidence = "都市爽文。主角激活祖传玉佩，发现行业黑幕，最终掌控城市核心资源并完成翻盘。";
        let candidates = declared_book_title_candidates_from_contract_evidence(evidence)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect::<Vec<_>>();

        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.contains("核心资源破局")
                    || candidate.contains("城市核心资源")),
            "declared title candidates should avoid abstract resource payoff labels: {candidates:?}"
        );
    }

    #[test]
    fn declared_book_title_candidates_reject_occupation_payoff_and_hide_ngram_rationale_noise() {
        let evidence = "都市爽文。主角沈棠白本是外卖员，意外获得财富之眼系统，从二手市场起步，终局摆脱系统束缚并建立商业帝国。";
        let decision = select_book_title_candidate_decision(
            [BookTitleCandidate::new(
                "本是外卖破局",
                book_title_candidate_rationale_from_story_evidence("本是外卖破局", evidence),
            )],
            &BookTitleEvidence::new("书名", evidence),
        );

        assert!(
            !decision.accepted,
            "occupation/identity payoff title should not be accepted: {decision:?}"
        );

        let rationale =
            book_title_candidate_rationale_from_story_evidence("本是外卖破局", evidence);
        assert!(
            !rationale.contains("是外卖破") && !rationale.contains("外卖破局、本是外"),
            "rationale must not expose internal ngram fragments: {rationale}"
        );
    }

    #[test]
    fn title_rationale_uses_whole_short_title_instead_of_overlapping_ngrams() {
        let evidence =
            "都市爽文。主角背负巨债，获得天机录后利用信息差翻盘，终局公开行业黑幕并改写规则。";
        let rationale =
            book_title_candidate_rationale_from_story_evidence("背负巨债破局", evidence);

        let lead = rationale.split("来自合同证据").next().unwrap_or("");
        assert!(
            lead.contains("背负巨债破局")
                && !lead.contains("负巨债破、")
                && !lead.contains("、负巨债破")
                && !lead.contains("巨债破局、")
                && !lead.contains("、巨债破局"),
            "rationale should not expose overlapping ngram fragments: {rationale}"
        );
    }

    #[test]
    fn connector_book_title_with_story_backed_components_is_allowed() {
        let evidence = BookTitleEvidence::new(
            "书名",
            "拾荒者在戴森球残骸里找到旧引擎，旧引擎记录能源垄断契约；终局中主角公开契约，让殖民地脱离戴森球核心控制。",
        );
        let decision = select_book_title_candidate_decision(
            [BookTitleCandidate::new(
                "拾荒者的旧引擎",
                "拾荒者是主角入口，旧引擎是推动能源垄断契约公开的关键物件，书名对应终局反转。",
            )],
            &evidence,
        );

        assert!(
            decision.accepted,
            "story-backed connector title should not be rejected as a weak connector template: {decision:?}"
        );
        assert_eq!(decision.selected_title(), Some("拾荒者的旧引擎"));
    }
}
