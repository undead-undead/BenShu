//! Model-assisted semantic review for structured writing contracts.
//!
//! The reviewer may classify meaning, but it never mutates contract authority.
//! Any accepted repair is converted back into an existing typed patch and must
//! pass the normal contract gate.

use super::creation_contract_model::{value_missing, NovelCreationContract};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticReviewVerdict {
    Equivalent,
    Conflict,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticReviewFinding {
    pub(crate) verdict: SemanticReviewVerdict,
    pub(crate) rationale: String,
    pub(crate) evidence: Option<SemanticConflictEvidence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SemanticConflictEvidence {
    #[serde(default)]
    pub(crate) authority_field: String,
    #[serde(default)]
    pub(crate) authority_quote: String,
    #[serde(default)]
    pub(crate) candidate_field: String,
    #[serde(default)]
    pub(crate) candidate_quote: String,
}

impl SemanticConflictEvidence {
    pub(crate) fn is_exact(&self) -> bool {
        [
            self.authority_field.as_str(),
            self.authority_quote.as_str(),
            self.candidate_field.as_str(),
            self.candidate_quote.as_str(),
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
    }

    fn is_grounded_in(&self, authority_sources: &[&str], candidate_sources: &[&str]) -> bool {
        self.is_exact()
            && sources_contain_exact_quote(authority_sources, &self.authority_quote)
            && sources_contain_exact_quote(candidate_sources, &self.candidate_quote)
    }

    fn is_grounded_user_authority_omission(
        &self,
        authority_sources: &[&str],
        candidate_sources: &[&str],
    ) -> bool {
        self.is_exact()
            && self.candidate_quote.trim() == "<missing>"
            && sources_contain_exact_quote(authority_sources, &self.authority_quote)
            && !sources_contain_exact_quote(candidate_sources, &self.authority_quote)
            && user_authority_quote_is_explicit_requirement(&self.authority_quote)
            && user_story_candidate_field_is_known(&self.candidate_field)
    }
}

impl SemanticReviewFinding {
    fn require_grounded_conflict(
        mut self,
        authority_sources: &[&str],
        candidate_sources: &[&str],
    ) -> Self {
        if self.verdict == SemanticReviewVerdict::Conflict
            && (semantic_rationale_denies_conflict(&self.rationale)
                || !self.evidence.as_ref().is_some_and(|evidence| {
                    evidence.is_grounded_in(authority_sources, candidate_sources)
                }))
        {
            self.verdict = SemanticReviewVerdict::Uncertain;
            self.evidence = None;
        }
        self
    }

    fn require_grounded_user_authority_conflict(
        mut self,
        authority_sources: &[&str],
        candidate_sources: &[&str],
    ) -> Self {
        if self.verdict == SemanticReviewVerdict::Conflict
            && (semantic_rationale_denies_conflict(&self.rationale)
                || !self.evidence.as_ref().is_some_and(|evidence| {
                    evidence.is_grounded_in(authority_sources, candidate_sources)
                        || evidence.is_grounded_user_authority_omission(
                            authority_sources,
                            candidate_sources,
                        )
                }))
        {
            self.verdict = SemanticReviewVerdict::Uncertain;
            self.evidence = None;
        }
        self
    }
}

fn user_authority_quote_is_explicit_requirement(quote: &str) -> bool {
    let compact = quote.replace(char::is_whitespace, "");
    [
        "必须",
        "不能",
        "不得",
        "只能",
        "不可",
        "不要",
        "避免",
        "禁止",
        "务必",
        "绝不",
        "总字数",
        "每章",
        "主角",
        "男主",
        "女主",
        "对手",
        "同伴",
        "导师",
        "关系对象",
        "终局",
        "结局",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn user_story_candidate_field_is_known(field: &str) -> bool {
    let compact = field.replace(char::is_whitespace, "").to_ascii_lowercase();
    [
        "角色权威",
        "角色表",
        "人物表",
        "character",
        "role",
        "故事简述",
        "brief",
        "故事前提",
        "premise",
        "主角弧线",
        "protagonist_arc",
        "主线",
        "因果",
        "causal",
        "终局",
        "结局",
        "ending",
        "书名理由",
        "title_rationale",
        "主题",
        "世界规则",
        "必须避免",
        "governance",
        "大纲",
        "分卷",
        "章节",
        "outline",
        "volume",
        "chapter",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn semantic_rationale_denies_conflict(rationale: &str) -> bool {
    let compact = rationale
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "未触犯",
        "没有触犯",
        "未违背",
        "没有违背",
        "不构成冲突",
        "并不冲突",
        "不矛盾",
        "相容",
        "兼容",
        "保持一致",
        "doesnotconflict",
        "notaconflict",
        "iscompatible",
        "remainsconsistent",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndingEquivalenceReviewRequest {
    pub(crate) canonical_ending: String,
    pub(crate) outline_ending: String,
    pub(crate) raw_outline: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserStoryAuthorityReviewRequest {
    pub(crate) authority: String,
    pub(crate) character_authority: String,
    pub(crate) brief: String,
    pub(crate) premise: String,
    pub(crate) protagonist_arc: String,
    pub(crate) causal_spine: String,
    pub(crate) ending: String,
    pub(crate) title_rationale: String,
    pub(crate) governance: String,
    pub(crate) outline: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutlineCharacterAuthorityReviewRequest {
    pub(crate) character_authority: String,
    pub(crate) story_authority: String,
    pub(crate) contract_fields: String,
    pub(crate) outline: String,
    pub(crate) payoff_matrix: String,
}

impl UserStoryAuthorityReviewRequest {
    pub(crate) fn fingerprint(&self) -> String {
        format!(
            "user_story_authority\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            compact_clause(&self.authority),
            compact_clause(&self.character_authority),
            compact_clause(&self.brief),
            compact_clause(&self.premise),
            compact_clause(&self.protagonist_arc),
            compact_clause(&self.causal_spine),
            compact_clause(&self.ending),
            compact_clause(&self.title_rationale),
            compact_clause(&self.governance),
            compact_clause(&self.outline)
        )
    }

    pub(crate) fn prompt(&self) -> String {
        format!(
            "你是小说合同语义与语言质量裁判，判断生成合同是否保留用户最初明确指定及后续明确修订的故事核心，并检查核心字段是否可直接作为稳定写作权威。\n\
             用户后续明确修订的事实高于更早的方向和模型生成字段；若二者冲突，必须以后续明确修订为准。\n\
             必须逐条核对“后续明确修订”里的具名人物、亲属或身份关系、事件先后和因果：这些事实必须直接出现在对应故事字段或指定章节中，不能靠泛称、隐含推断或题材常识补足。把明确亲属关系降级为无法确认归属的职位泛称、把具名人物降级为无姓名身份、遗漏修订要求的前因或结果，都必须判 conflict。\n\
             允许模型原创书名、人物姓名、支线、场景和技术细节，但不得改变用户指定的核心行为主体、对手目的、作案或冲突机制、主要因果以及终局必须解决的对象。\n\
             如果合同把用户指定的核心阴谋/目标换成另一种阴谋/目标，即使题材相近，也必须判 conflict。\n\
             用户把两个事件写成相关、伴随、掩盖或偶然相交时，合同不得擅自升级为一方主动制造、控制另一方；新增细节不得把相关性改写成用户未指定的直接因果，否则必须判 conflict。\n\
             必须核对角色权威表与故事前提、主线、终局、大纲中的身份和叙事功能；同一姓名被同时写成不同人物、死者与对手互换、同伴与对手身份错置，或角色权威表和故事字段互相矛盾时，必须判 conflict。\n\
             角色权威表是模型生成的候选合同，不高于用户故事核心权威。若用户指定的男主、女主、关系对象、同伴或对手在候选角色表中被标成互斥职能，必须判 conflict；candidate_field 必须指向角色权威表并引用错误角色行，不得把本来符合用户要求的故事前提当成冲突证据。\n\
             必须检查核心证据链是否自洽；如果结论依赖明显无法由所述证据推出的跳跃，也必须判 conflict。\n\
             不得凭空扩大人物、工具或制度的能力边界；如果分卷或近期章节依赖故事前提、世界规则和因果链从未建立的关键能力，也必须判 conflict。\n\
             必须检查分卷与近期章节是否按因果和时间顺序持续推进：主冲突或终局已经完成后又重启同一主线、追加新的“最终反派/最终解决”，单卷拼入多套卷目标或重复卷尾，近期章节目标存在缺谓语、词序破损或事件编号错位，都必须判 conflict。\n\
             主题、世界规则和“必须避免”只是约束，不能替代对冲突故事字段的实际改写；若“必须避免”已经说明某种身份、时间线或终局写法不允许，但角色权威、故事字段或大纲仍保留该写法，必须判 conflict，并在 rationale 中点明仍冲突的具体字段。\n\
             近期章节里的行为主体、承受对象和实体类型必须自洽；地点、房号、人物、设备、机构不能互相错当成另一类实体来执行或承受动作，否则必须判 conflict。\n\
             同时检查故事前提、总主线因果、终局和大纲是否含明显错字、词序错乱、截断或不可读拼接；只要存在这些语言损坏，即使大意接近，也必须判 conflict。\n\
             用户权威中含“必须、不能、不得、只能、避免”等明确约束时，必须在合同故事字段、世界规则、必须避免或大纲中逐项落实；整项遗漏必须判 conflict，不能因为候选没有相反句就判 equivalent。\n\
             不得改写合同，不得输出补丁。\n\
             用户故事核心权威：{}\n\
             候选合同角色权威表（含人物弧线）：{}\n\
             合同故事简述：{}\n\
             合同故事前提：{}\n\
             合同主角弧线：{}\n\
             合同总主线因果：{}\n\
             合同终局方向：{}\n\
             合同书名理由：{}\n\
             合同主题、世界规则与必须避免：{}\n\
             合同大纲：{}\n\
             conflict 必须逐字引用一处用户权威和一处候选合同，并标出两边字段。若用户明确要求被候选完全遗漏，authority_quote 仍须逐字引用用户权威，candidate_field 填应承载该要求的候选字段，candidate_quote 固定填 `<missing>`；其他无法提供双侧精确引用的情况必须输出 uncertain。\n\
             只输出一个 JSON 对象：\n\
             {{\"verdict\":\"equivalent|conflict|uncertain\",\"rationale\":\"一句简短理由\",\"evidence\":{{\"authority_field\":\"用户权威字段\",\"authority_quote\":\"逐字短引文\",\"candidate_field\":\"候选合同字段\",\"candidate_quote\":\"逐字短引文\"}}}}",
            self.authority,
            self.character_authority,
            self.brief,
            self.premise,
            self.protagonist_arc,
            self.causal_spine,
            self.ending,
            self.title_rationale,
            self.governance,
            self.outline
        )
    }

    pub(crate) fn ground_finding(&self, finding: SemanticReviewFinding) -> SemanticReviewFinding {
        finding.require_grounded_user_authority_conflict(
            &[&self.authority],
            &[
                &self.character_authority,
                &self.brief,
                &self.premise,
                &self.protagonist_arc,
                &self.causal_spine,
                &self.ending,
                &self.title_rationale,
                &self.governance,
                &self.outline,
            ],
        )
    }
}

pub(crate) fn user_story_authority_review_request(
    authority: &str,
    contract: &NovelCreationContract,
) -> Option<UserStoryAuthorityReviewRequest> {
    let authority = authority.trim();
    if authority.chars().count() < 16
        || value_missing(&contract.premise)
        || value_missing(&contract.main_causal_spine)
        || value_missing(&contract.ending.desired_resolution)
    {
        return None;
    }
    Some(UserStoryAuthorityReviewRequest {
        authority: authority.to_string(),
        character_authority: contract
            .characters
            .iter()
            .map(|character| {
                format!(
                    "姓名：{}；角色：{}；欲望：{}；恐惧：{}；底线：{}；弧线起点：{}；弧线终点：{}",
                    character.canonical_name,
                    character.role,
                    character.desire,
                    character.fear,
                    character.bottom_line,
                    character.arc_start,
                    character.arc_end
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        brief: contract.brief.clone(),
        premise: contract.premise.clone(),
        protagonist_arc: contract.protagonist_arc.clone(),
        causal_spine: contract.main_causal_spine.clone(),
        ending: contract.ending.desired_resolution.clone(),
        title_rationale: contract.title.rationale.clone(),
        governance: format!(
            "主题：{}\n世界规则：{}\n必须避免：{}",
            contract.themes.join("；"),
            contract.world_rules.join("；"),
            contract.must_avoid.join("；")
        ),
        outline: super::creation_contract::strong_contract_outline_text(contract),
    })
}

impl OutlineCharacterAuthorityReviewRequest {
    pub(crate) fn fingerprint(&self) -> String {
        format!(
            "outline_character_authority\n{}\n{}\n{}\n{}\n{}",
            compact_clause(&self.character_authority),
            compact_clause(&self.story_authority),
            compact_clause(&self.contract_fields),
            compact_clause(&self.outline),
            compact_clause(&self.payoff_matrix)
        )
    }

    pub(crate) fn prompt(&self) -> String {
        format!(
            "你是小说合同内部一致性裁判，只判断大纲和伏笔兑现是否服从已经形成的角色与故事权威。\n\
             不得改写合同，不得输出补丁，也不要因为措辞不同就判冲突。\n\
             必须逐一核对具名角色的身份、职责、欲望、恐惧、底线、弧线、身体特征、能力边界、关系和终局命运。\n\
             角色权威标明男主、女主或其他明确性别身份时，大纲和兑现矩阵中的性别称谓、代词、亲属身份与年龄身份必须一致；把女主写成男性代词或男性年龄身份、把男主写成女性代词或女性年龄身份，且合同没有建立身份变化时，必须判 conflict。\n\
             如果大纲或兑现矩阵把一名角色的身体特征、特殊能力、弱点、身份、关系或命运明确转移给另一名角色，必须判 conflict。\n\
             角色权威中的“同伴”“盟友”“导师”“关键关系对象”是广义叙事职能；大纲中的朋友、恋人、亲属、旧识、合作者等可以是兼容的具体关系。只有候选文本明确把同一人写成互斥的另一职能、或把已锁定关系归给另一人时才能判 conflict；不得用更具体的关系称谓反推权威表缺失了性别或另一职能。\n\
             角色欲望和恐惧是推动选择与弧线的动机锚点，不是保证欲望必然实现、恐惧必然成真的命运预言。终局克服或避免恐惧、没有实现初始欲望、或者从弧线起点成长到弧线终点，本身都不构成 conflict；只有候选把欲望、恐惧或弧线归给错误角色，或明确否定合同已经锁定的角色身份、能力、关系、选择与终局结果时才可判 conflict。\n\
             角色底线是对具体行为和具体代价对象的禁止边界；只有候选文本实际执行了该禁止行为才能判 conflict。改由本人承担代价、保全被禁止伤害的对象、或付出不同类型的代价，是遵守底线，不是违反底线。\n\
             角色底线约束角色主动做出的选择，不是保证角色永远不会被外力击败、杀死、剥夺资源或失去控制权的命运预言。候选只写角色被击败、被杀、被迫失去某物，不能据此判定其主动放弃了底线；只有候选明确写出该角色选择、决定、同意或亲手实施了被底线禁止的行为，才可引用底线形成 conflict。\n\
             必须核对伤病、残疾、能力损失、康复和再次受伤的先后顺序；角色不能在没有恢复过程或世界规则依据时绕过既定身体代价，重复受伤也不能被当成无后果的能力升级，否则必须判 conflict。\n\
             如果大纲让角色依赖故事前提、世界规则和角色权威从未建立的关键能力完成阶段目标，必须判 conflict。\n\
             必须逐条核对世界规则中的触发条件、代价、限制和失败后果与近期章节结果；不能把规则明确声明的失败后果写成成功突破、奖励或稳定新状态，也不能用近义词替换来规避该冲突。\n\
             必须核对书名、书名理由与合同核心字段中的具名道具、协议、法则、装置、药物、术法、能力和地点。已经由书名或书名理由锁定的核心专名，在故事前提、主线、世界规则、终局、大纲和兑现矩阵中必须保持原字一致；只改一个字、近音字或形近字也属于名称漂移，除非合同明确声明两者是不同实体或正式别名。\n\
             对故事前提、世界规则和终局中反复出现的每个具名协议、法则、装置、药物、术法或能力，必须先锁定它已建立的触发条件和直接效果，再核对主线、大纲与兑现矩阵中的最终用途。同一具名机制如果原本会伤害、清除、封锁或摧毁目标，终局却直接“触发/启动/执行”它来拯救、恢复、解锁或保护同一目标，而合同没有在终局行动之前明确建立可执行的改写、反转、重定向、目标替换及其代价，必须判 conflict。\n\
             仅把另一个物件称为“密钥、病毒、后门、解药、核心”不能自动解释作用反转；合同必须写清它如何改变原机制、改变哪个目标、在何时完成，以及失败代价。只在兑现矩阵声称结果已经改变、但主线或大纲没有先建立转换步骤，也必须判 conflict。\n\
             必须逐条检查每卷目标、卷尾变化、近期章节目标和预期转折是否为语法完整、主谓关系清楚的事件句；缺谓语、缺必要介词或宾语、词序破损、句尾截断、只确认人物姓名却没有事件结果，均必须判 conflict，并在 rationale 中指出具体卷或章节。\n\
             必须把终局方向与兑现矩阵中同一人物、制度或核心冲突的不可逆结果逐项核对。终局写明死亡、摧毁、终结或失去控制，而兑现矩阵明确写存活、保留、继续运行或继续掌控，或者反向出现这些结果，必须判 conflict；不得用角色底线替相反的终局结果开脱。\n\
             同一条因果链中先发生的阶段性胜负、失能或失去手段，可以继续发展为后续的死亡、同化、囚禁、退出或制度性终结；只要后续结果没有明确否定前一结果，就不是互斥状态，不得判 conflict。卷目标写终局行动、同卷卷尾变化写该行动造成的不可逆结果，是目标到结果的正常顺序；只有候选明确让结果先于必要行动、让非末卷提前完成终局、或同时保留两个不可并存的最终状态，才可判 conflict。\n\
             必须区分渐进变化与最终完成：破损、削弱、局部关闭、暂时失控等阶段事件，可以在后续发展为彻底摧毁、永久关闭或最终失去控制；不得仅因动词或对象重复就推断事件已经提前完成。\n\
             合理的新事件细节、未改变权威归属的同义改写、角色利用另一角色的弱点或规则，不算冲突。\n\
             角色权威表：{}\n\
             故事权威：{}\n\
             合同核心字段：{}\n\
             大纲：{}\n\
             伏笔兑现矩阵：{}\n\
             conflict 必须逐字引用一处角色/故事权威和一处大纲/兑现候选，并标出两边字段；无法提供双侧精确引用时必须输出 uncertain。\n\
             只输出一个 JSON 对象：\n\
             {{\"verdict\":\"equivalent|conflict|uncertain\",\"rationale\":\"一句简短理由\",\"evidence\":{{\"authority_field\":\"权威字段\",\"authority_quote\":\"逐字短引文\",\"candidate_field\":\"候选字段\",\"candidate_quote\":\"逐字短引文\"}}}}",
            self.character_authority,
            self.story_authority,
            self.contract_fields,
            self.outline,
            self.payoff_matrix
        )
    }

    pub(crate) fn ground_finding(&self, finding: SemanticReviewFinding) -> SemanticReviewFinding {
        let mut finding = finding.require_grounded_conflict(
            &[&self.character_authority, &self.story_authority],
            &[&self.contract_fields, &self.outline, &self.payoff_matrix],
        );
        if finding.verdict == SemanticReviewVerdict::Conflict
            && finding.evidence.as_ref().is_some_and(|evidence| {
                evidence_quote_is_character_bottom_line(
                    &self.character_authority,
                    &evidence.authority_quote,
                ) && !candidate_explicitly_executes_forbidden_choice(
                    &evidence.authority_quote,
                    &evidence.candidate_quote,
                )
            })
        {
            finding.verdict = SemanticReviewVerdict::Uncertain;
            finding.evidence = None;
        }
        if finding.verdict == SemanticReviewVerdict::Conflict
            && finding.evidence.as_ref().is_some_and(|evidence| {
                evidence_quote_is_character_motivational_anchor(
                    &self.character_authority,
                    &evidence.authority_quote,
                )
            })
        {
            finding.verdict = SemanticReviewVerdict::Uncertain;
            finding.evidence = None;
        }
        finding
    }
}

fn evidence_quote_is_character_motivational_anchor(character_authority: &str, quote: &str) -> bool {
    evidence_quote_matches_character_field(character_authority, quote, &["欲望：", "恐惧："])
}

fn evidence_quote_is_character_bottom_line(character_authority: &str, quote: &str) -> bool {
    evidence_quote_matches_character_field(character_authority, quote, &["底线："])
}

fn evidence_quote_matches_character_field(
    character_authority: &str,
    quote: &str,
    prefixes: &[&str],
) -> bool {
    let quote = quote.trim();
    if quote.is_empty() {
        return false;
    }
    character_authority.lines().any(|line| {
        line.split('；')
            .filter_map(|field| {
                let field = field.trim();
                prefixes
                    .iter()
                    .find_map(|prefix| field.strip_prefix(prefix))
            })
            .any(|authority_value| {
                let authority_value = authority_value.trim();
                !authority_value.is_empty()
                    && (authority_value.contains(quote) || quote.contains(authority_value))
            })
    })
}

fn candidate_explicitly_executes_forbidden_choice(authority: &str, candidate: &str) -> bool {
    let compact = candidate
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let has_voluntary_choice = voluntary_choice_markers()
        .iter()
        .any(|marker| contains_unnegated_voluntary_marker(&compact, marker));
    let has_explicit_action = forbidden_action_markers()
        .iter()
        .any(|marker| contains_unnegated_voluntary_marker(&compact, marker));
    if !has_voluntary_choice || !has_explicit_action {
        return false;
    }

    let anchors = forbidden_choice_authority_anchors(authority);
    !anchors.is_empty()
        && anchors
            .iter()
            .all(|anchor| compact_clause(&compact).contains(anchor))
}

fn voluntary_choice_markers() -> &'static [&'static str] {
    &["主动", "选择", "决定", "同意", "允许", "亲手", "自愿"]
}

fn forbidden_action_markers() -> &'static [&'static str] {
    &[
        "换取", "交换", "牺牲", "出卖", "背叛", "放弃", "交出", "销毁", "舍弃", "抛弃", "转让",
        "泄露", "伤害", "杀害", "杀死",
    ]
}

fn forbidden_choice_authority_anchors(authority: &str) -> Vec<String> {
    let mut value = compact_clause(authority);
    let mut separators = Vec::new();
    separators.extend_from_slice(&[
        "绝不", "不得", "不能", "不可", "不准", "禁止", "不许", "不要", "不以", "不",
    ]);
    separators.extend_from_slice(voluntary_choice_markers());
    separators.extend_from_slice(forbidden_action_markers());
    separators.sort_unstable_by_key(|marker| std::cmp::Reverse(marker.chars().count()));
    for marker in separators {
        value = value.replace(marker, "|");
    }
    for connector in [
        "以此", "从而", "因此", "为了", "用来", "对", "以", "把", "将", "的",
    ] {
        value = value.replace(connector, "");
    }
    value
        .split(|ch: char| {
            ch == '|' || ch.is_ascii_punctuation() || matches!(ch, '，' | '。' | '；' | '：' | '、')
        })
        .map(compact_clause)
        .filter(|anchor| anchor.chars().count() >= 2)
        .fold(Vec::new(), |mut anchors, anchor| {
            if !anchors.iter().any(|known| known == &anchor) {
                anchors.push(anchor);
            }
            anchors
        })
}

fn contains_unnegated_voluntary_marker(text: &str, marker: &str) -> bool {
    text.match_indices(marker).any(|(index, _)| {
        let prefix = &text[..index];
        let prefix = prefix
            .char_indices()
            .rev()
            .nth(5)
            .map(|(start, _)| &prefix[start..])
            .unwrap_or(prefix);
        ![
            "不", "未", "没", "无", "拒绝", "绝不", "并未", "从未", "不得", "不能", "被迫", "迫于",
        ]
        .iter()
        .any(|negation| prefix.contains(negation))
    })
}

pub(crate) fn outline_character_authority_review_request(
    contract: &NovelCreationContract,
) -> Option<OutlineCharacterAuthorityReviewRequest> {
    if contract.characters.is_empty()
        || value_missing(&contract.premise)
        || value_missing(&contract.ending.desired_resolution)
        || !contract.outline.has_stage_or_near_chapter_plan()
    {
        return None;
    }
    let character_authority = contract
        .characters
        .iter()
        .filter(|character| {
            !value_missing(&character.canonical_name) && !value_missing(&character.role)
        })
        .map(|character| {
            format!(
                "姓名：{}；角色：{}；欲望：{}；恐惧：{}；底线：{}；弧线起点：{}；弧线终点：{}",
                character.canonical_name,
                character.role,
                character.desire,
                character.fear,
                character.bottom_line,
                character.arc_start,
                character.arc_end
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if character_authority.is_empty() {
        return None;
    }
    let story_authority = format!(
        "书名：{}\n书名理由：{}\n故事前提：{}\n主角弧线：{}\n终局方向：{}\n终局状态：{}\n世界规则：{}",
        contract.title.canonical_title,
        contract.title.rationale,
        contract.premise,
        contract.protagonist_arc,
        contract.ending.desired_resolution,
        contract.ending.final_state,
        contract.world_rules.join("；")
    );
    let contract_fields = format!(
        "故事简述：{}\n故事前提：{}\n总主线因果：{}\n终局方向：{}\n终局状态：{}\n世界规则：{}",
        contract.brief,
        contract.premise,
        contract.main_causal_spine,
        contract.ending.desired_resolution,
        contract.ending.final_state,
        contract.world_rules.join("；")
    );
    let payoff_matrix = contract
        .structured
        .payoff_matrix
        .iter()
        .map(|entry| {
            format!(
                "承诺：{}；兑现：{}；状态：{}",
                entry.promise, entry.payoff_target, entry.status
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(OutlineCharacterAuthorityReviewRequest {
        character_authority,
        story_authority,
        contract_fields,
        outline: super::creation_contract::strong_contract_outline_text(contract),
        payoff_matrix,
    })
}

impl EndingEquivalenceReviewRequest {
    pub(crate) fn fingerprint(&self) -> String {
        format!(
            "ending_equivalence\n{}\n{}",
            compact_clause(&self.canonical_ending),
            compact_clause(&self.outline_ending)
        )
    }

    pub(crate) fn prompt(&self) -> String {
        format!(
            "你是小说合同语义裁判，只判断两段结局是否表达同一个不可逆终局。\n\
             不得改写书名、角色、结局权威或大纲，不得输出合同补丁。\n\
             权威终局：{}\n\
             大纲显式结局：{}\n\
             conflict 必须逐字引用权威终局和大纲终局；无法提供双侧精确引用时必须输出 uncertain。\n\
             只输出一个 JSON 对象：\n\
             {{\"verdict\":\"equivalent|conflict|uncertain\",\"rationale\":\"一句简短理由\",\"evidence\":{{\"authority_field\":\"ending.desired_resolution\",\"authority_quote\":\"权威终局逐字短引文\",\"candidate_field\":\"outline.ending\",\"candidate_quote\":\"大纲终局逐字短引文\"}}}}",
            self.canonical_ending, self.outline_ending
        )
    }

    pub(crate) fn canonicalizing_plot_patch(&self) -> String {
        serde_json::json!({
            "patch_type": "plot_patch",
            "outline": {
                "raw_outline": replace_labeled_clause(
                    &self.raw_outline,
                    "结局",
                    &self.canonical_ending,
                )
            }
        })
        .to_string()
    }

    pub(crate) fn ground_finding(&self, finding: SemanticReviewFinding) -> SemanticReviewFinding {
        finding.require_grounded_conflict(&[&self.canonical_ending], &[&self.outline_ending])
    }
}

pub(crate) fn ending_equivalence_review_request(
    contract: &NovelCreationContract,
) -> Option<EndingEquivalenceReviewRequest> {
    let canonical = contract.ending.desired_resolution.trim();
    if value_missing(canonical) {
        return None;
    }
    let outline_ending = labeled_clause(&contract.outline.raw_outline, "结局")?;
    if clauses_lexically_match(canonical, &outline_ending) {
        return None;
    }
    Some(EndingEquivalenceReviewRequest {
        canonical_ending: canonical.to_string(),
        outline_ending,
        raw_outline: contract.outline.raw_outline.clone(),
    })
}

pub(crate) fn parse_semantic_review_finding(raw: &str) -> SemanticReviewFinding {
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .or_else(|| {
            let start = raw.find('{')?;
            let end = raw.rfind('}')?;
            (start < end)
                .then(|| serde_json::from_str::<serde_json::Value>(&raw[start..=end]).ok())
                .flatten()
        });
    let verdict_text = value
        .as_ref()
        .and_then(|value| value.get("verdict"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let normalized = verdict_text
        .trim_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
        .replace([' ', '\t', '\r', '\n'], "");
    let mut verdict = if matches!(
        normalized.as_str(),
        "equivalent" | "same" | "一致" | "等价" | "相同" | "不冲突" | "无冲突"
    ) || normalized.contains("noconflict")
    {
        SemanticReviewVerdict::Equivalent
    } else if matches!(
        normalized.as_str(),
        "conflict" | "different" | "冲突" | "不一致" | "不等价" | "不同"
    ) {
        SemanticReviewVerdict::Conflict
    } else {
        SemanticReviewVerdict::Uncertain
    };
    let rationale = value
        .as_ref()
        .and_then(|value| value.get("rationale"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let evidence = value
        .as_ref()
        .and_then(|value| value.get("evidence"))
        .and_then(|evidence| {
            serde_json::from_value::<SemanticConflictEvidence>(evidence.clone()).ok()
        })
        .filter(SemanticConflictEvidence::is_exact);
    if verdict == SemanticReviewVerdict::Conflict && evidence.is_none() {
        verdict = SemanticReviewVerdict::Uncertain;
    }
    SemanticReviewFinding {
        verdict,
        rationale,
        evidence,
    }
}

fn clauses_lexically_match(left: &str, right: &str) -> bool {
    let left = compact_clause(left);
    let right = compact_clause(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    let shorter_chars = left.chars().count().min(right.chars().count());
    shorter_chars >= 8 && (left.contains(&right) || right.contains(&left))
}

fn compact_clause(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | '.' | '；' | ';'))
        .collect()
}

fn sources_contain_exact_quote(sources: &[&str], quote: &str) -> bool {
    let quote = quote.trim();
    !quote.is_empty() && sources.iter().any(|source| source.contains(quote))
}

fn labeled_clause(text: &str, label: &str) -> Option<String> {
    let marker = format!("{label}：");
    let ascii_marker = format!("{label}:");
    let start = text
        .find(&marker)
        .map(|index| index + marker.len())
        .or_else(|| {
            text.find(&ascii_marker)
                .map(|index| index + ascii_marker.len())
        })?;
    let tail = &text[start..];
    let end = tail
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '。' | '\n' | '\r').then_some(index))
        .unwrap_or(tail.len());
    let value = tail[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn replace_labeled_clause(text: &str, label: &str, replacement: &str) -> String {
    let marker = format!("{label}：");
    let ascii_marker = format!("{label}:");
    let Some((marker_start, value_start)) = text
        .find(&marker)
        .map(|index| (index, index + marker.len()))
        .or_else(|| {
            text.find(&ascii_marker)
                .map(|index| (index, index + ascii_marker.len()))
        })
    else {
        return text.to_string();
    };
    let tail = &text[value_start..];
    let value_end = tail
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '。' | '\n' | '\r').then_some(value_start + index))
        .unwrap_or(text.len());
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..marker_start]);
    out.push_str(&marker);
    out.push_str(replacement.trim());
    out.push_str(&text[value_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_verdict_parser_accepts_json_and_rejects_conflict() {
        assert_eq!(
            parse_semantic_review_finding(r#"{"verdict":"equivalent","rationale":"same ending"}"#)
                .verdict,
            SemanticReviewVerdict::Equivalent
        );
        assert_eq!(
            parse_semantic_review_finding("结论：不一致，终局对象不同").verdict,
            SemanticReviewVerdict::Uncertain
        );
        assert_eq!(
            parse_semantic_review_finding("no conflict").verdict,
            SemanticReviewVerdict::Uncertain
        );
        assert_eq!(
            parse_semantic_review_finding("可能 equivalent，但证据不足").verdict,
            SemanticReviewVerdict::Uncertain
        );
        let finding = parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"character arc_start still says 士子"}"#,
        );
        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert_eq!(finding.rationale, "character arc_start still says 士子");
        let finding = parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"身份冲突","evidence":{"authority_field":"characters[0].role","authority_quote":"女主","candidate_field":"outline.near_chapters[0].goal","candidate_quote":"他独自进入矿井"}}"#,
        );
        assert_eq!(finding.verdict, SemanticReviewVerdict::Conflict);
        assert!(finding.evidence.is_some_and(|evidence| evidence.is_exact()));
    }

    #[test]
    fn semantic_conflict_requires_quotes_grounded_on_both_sides() {
        let request = EndingEquivalenceReviewRequest {
            canonical_ending: "主角公开账册并终结垄断".to_string(),
            outline_ending: "账册沉入海底，垄断继续".to_string(),
            raw_outline: String::new(),
        };
        let grounded = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"终局相反","evidence":{"authority_field":"ending.desired_resolution","authority_quote":"终结垄断","candidate_field":"outline.ending","candidate_quote":"垄断继续"}}"#,
        ));
        assert_eq!(grounded.verdict, SemanticReviewVerdict::Conflict);

        let fabricated = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"终局相反","evidence":{"authority_field":"ending.desired_resolution","authority_quote":"终结垄断","candidate_field":"outline.ending","candidate_quote":"建立新帝国"}}"#,
        ));
        assert_eq!(fabricated.verdict, SemanticReviewVerdict::Uncertain);
        assert!(fabricated.evidence.is_none());
    }

    #[test]
    fn user_story_authority_conflict_accepts_grounded_missing_field_evidence_only() {
        let request = UserStoryAuthorityReviewRequest {
            authority: "重生优势必须随时间推移失效".to_string(),
            character_authority: "姓名：祝照澜；角色：主角".to_string(),
            brief: "采购经理重生后追查供应链造假".to_string(),
            premise: "采购经理回到上市前一年".to_string(),
            protagonist_arc: "从背锅者成长为透明供应体系的建立者".to_string(),
            causal_spine: "发现造假并建立新供应链".to_string(),
            ending: "公司以透明供应链完成上市".to_string(),
            title_rationale: "书名来自终局行动".to_string(),
            governance: "世界规则：合同违约需要赔偿".to_string(),
            outline: "第一卷追查造假；第二卷完成上市".to_string(),
        };
        let grounded = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"明确约束被完全遗漏","evidence":{"authority_field":"用户故事核心权威","authority_quote":"重生优势必须随时间推移失效","candidate_field":"世界规则与大纲","candidate_quote":"<missing>"}}"#,
        ));
        assert_eq!(grounded.verdict, SemanticReviewVerdict::Conflict);

        let fabricated = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"明确约束被完全遗漏","evidence":{"authority_field":"用户故事核心权威","authority_quote":"主角必须预知所有股价","candidate_field":"世界规则与大纲","candidate_quote":"<missing>"}}"#,
        ));
        assert_eq!(fabricated.verdict, SemanticReviewVerdict::Uncertain);

        let fabricated_field = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"明确约束被完全遗漏","evidence":{"authority_field":"用户故事核心权威","authority_quote":"重生优势必须随时间推移失效","candidate_field":"模型随意声明的字段","candidate_quote":"<missing>"}}"#,
        ));
        assert_eq!(fabricated_field.verdict, SemanticReviewVerdict::Uncertain);

        let non_requirement = UserStoryAuthorityReviewRequest {
            authority: "故事发生在一座沿海城市".to_string(),
            ..request.clone()
        }
        .ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"背景被遗漏","evidence":{"authority_field":"用户故事核心权威","authority_quote":"故事发生在一座沿海城市","candidate_field":"故事前提","candidate_quote":"<missing>"}}"#,
        ));
        assert_eq!(non_requirement.verdict, SemanticReviewVerdict::Uncertain);

        let already_present = UserStoryAuthorityReviewRequest {
            premise: "重生优势必须随时间推移失效".to_string(),
            ..request.clone()
        }
        .ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"错误声称遗漏","evidence":{"authority_field":"用户故事核心权威","authority_quote":"重生优势必须随时间推移失效","candidate_field":"故事前提","candidate_quote":"<missing>"}}"#,
        ));
        assert_eq!(already_present.verdict, SemanticReviewVerdict::Uncertain);

        let ending_request = EndingEquivalenceReviewRequest {
            canonical_ending: "主角公开账册并终结垄断".to_string(),
            outline_ending: "主角回到故乡".to_string(),
            raw_outline: String::new(),
        };
        let unsupported_elsewhere = ending_request.ground_finding(
            parse_semantic_review_finding(
                r#"{"verdict":"conflict","rationale":"终局遗漏","evidence":{"authority_field":"ending","authority_quote":"终结垄断","candidate_field":"outline","candidate_quote":"<missing>"}}"#,
            ),
        );
        assert_eq!(
            unsupported_elsewhere.verdict,
            SemanticReviewVerdict::Uncertain
        );
    }

    #[test]
    fn user_story_authority_treats_generated_character_roles_as_candidate_contract() {
        let request = UserStoryAuthorityReviewRequest {
            authority: "女主经营香药铺，男主是追查贡香账册失窃案的年轻官员".to_string(),
            character_authority: "姓名：陶泊衡；角色：对手；欲望：查明贡香失窃真相".to_string(),
            brief: "女掌柜与年轻官员合作查案".to_string(),
            premise: "女掌柜卷入陶泊衡的查案计划".to_string(),
            protagonist_arc: "从独行者成长为合作者".to_string(),
            causal_spine: "账册失窃引发调查".to_string(),
            ending: "两人追回账册并定情".to_string(),
            title_rationale: "以契约为证".to_string(),
            governance: "担保人承担真实债务".to_string(),
            outline: "陶泊衡与女掌柜联手破案".to_string(),
        };
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"男主在角色表被标成对手","evidence":{"authority_field":"用户故事核心权威","authority_quote":"男主是追查贡香账册失窃案的年轻官员","candidate_field":"候选合同角色权威表","candidate_quote":"姓名：陶泊衡；角色：对手；欲望：查明贡香失窃真相"}}"#,
        ));
        assert_eq!(finding.verdict, SemanticReviewVerdict::Conflict);

        let wrong_side = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"错把角色表当用户权威","evidence":{"authority_field":"角色权威表","authority_quote":"姓名：陶泊衡；角色：对手","candidate_field":"故事前提","candidate_quote":"女掌柜卷入陶泊衡的查案计划"}}"#,
        ));
        assert_eq!(wrong_side.verdict, SemanticReviewVerdict::Uncertain);
    }

    #[test]
    fn self_contradictory_conflict_rationale_cannot_hard_block() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "宋云声".to_string(),
            role: "主角".to_string(),
            bottom_line: "不以挨爱之人的性命换取突破瓶颈的机会".to_string(),
            ..Default::default()
        }];
        contract.premise = "宋云声守护天纪。".to_string();
        contract.ending.desired_resolution = "宋云声弥合天裂。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(1),
                goal: "宋云声承担代价".to_string(),
                expected_turn: "顾承宁失明但存活".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"顾承宁失明但未死亡，未触犯性命底线，故违背底线","evidence":{"authority_field":"底线","authority_quote":"不以挨爱之人的性命换取突破瓶颈的机会","candidate_field":"近期章节","candidate_quote":"顾承宁失明但存活"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert!(finding.evidence.is_none());
    }

    #[test]
    fn involuntary_character_fate_does_not_violate_behavioral_bottom_line() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "阮星安".to_string(),
            role: "对手".to_string(),
            bottom_line: "绝不放弃对听雪楼的控制权".to_string(),
            ..Default::default()
        }];
        contract.premise = "阮星安控制听雪楼，主角必须终结其垄断。".to_string();
        contract.ending.desired_resolution = "主角斩杀阮星安并终结听雪楼垄断。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(40),
                goal: "主角攻入听雪楼中枢".to_string(),
                expected_turn: "主角斩杀阮星安，听雪楼失去旧主".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"阮星安死亡后会失去控制权，违反其底线","evidence":{"authority_field":"阮星安_底线","authority_quote":"绝不放弃对听雪楼的控制权","candidate_field":"第40章_预期转折","candidate_quote":"主角斩杀阮星安，听雪楼失去旧主"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert!(finding.evidence.is_none());
    }

    #[test]
    fn resolved_character_fear_does_not_conflict_with_terminal_state() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "叶予序".to_string(),
            role: "主角".to_string(),
            fear: "孤独终老且被世人遗忘".to_string(),
            arc_start: "害怕留下任何牵挂的散修".to_string(),
            arc_end: "承担宗门传承的守剑人".to_string(),
            ..Default::default()
        }];
        contract.premise = "叶予序持断剑追查灵脉真相。".to_string();
        contract.ending.desired_resolution = "叶予序斩断伪灵脉并成为传说。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(1),
                goal: "叶予序进入废弃剑冢".to_string(),
                expected_turn: "断剑首次回应叶予序".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"成为传说与害怕被遗忘相反","evidence":{"authority_field":"叶予序_恐惧","authority_quote":"孤独终老且被世人遗忘","candidate_field":"终局方向","candidate_quote":"叶予序斩断伪灵脉并成为传说"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert!(finding.evidence.is_none());
    }

    #[test]
    fn unfulfilled_character_desire_alone_is_not_authority_conflict() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "沈照川".to_string(),
            role: "对手".to_string(),
            desire: "永远掌控渡口商会".to_string(),
            ..Default::default()
        }];
        contract.premise = "沈照川掌控渡口商会，主角追查沉船旧案。".to_string();
        contract.ending.desired_resolution = "主角公开账册，沈照川失去商会控制权。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(1),
                goal: "主角取得第一张货单".to_string(),
                expected_turn: "沈照川发现账目已被复制".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"结局没有实现角色欲望","evidence":{"authority_field":"沈照川_欲望","authority_quote":"永远掌控渡口商会","candidate_field":"终局方向","candidate_quote":"主角公开账册，沈照川失去商会控制权"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert!(finding.evidence.is_none());
    }

    #[test]
    fn voluntary_forbidden_choice_remains_a_grounded_bottom_line_conflict() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "阮星安".to_string(),
            role: "对手".to_string(),
            bottom_line: "绝不放弃对听雪楼的控制权".to_string(),
            ..Default::default()
        }];
        contract.premise = "阮星安控制听雪楼。".to_string();
        contract.ending.desired_resolution = "听雪楼控制权完成交接。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(40),
                goal: "阮星安面对最终选择".to_string(),
                expected_turn: "阮星安主动放弃并交出听雪楼控制权".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"角色主动执行了底线禁止的行为","evidence":{"authority_field":"阮星安_底线","authority_quote":"绝不放弃对听雪楼的控制权","candidate_field":"第40章_预期转折","candidate_quote":"阮星安主动放弃并交出听雪楼控制权"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Conflict);
        assert!(finding.evidence.is_some());
    }

    #[test]
    fn different_self_sacrifice_does_not_inherit_an_unrelated_bottom_line_action() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "姜照舟".to_string(),
            role: "主角".to_string(),
            bottom_line: "不以心核换取安逸生活".to_string(),
            ..Default::default()
        }];
        contract.premise = "姜照舟携带心核寻找骨海灯塔。".to_string();
        contract.ending.desired_resolution = "姜照舟逆转潮汐。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(40),
                goal: "姜照舟抵达灯塔核心".to_string(),
                expected_turn: "姜照舟牺牲自己嵌入心核，逆转潮汐".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"牺牲自己等于用心核换安逸","evidence":{"authority_field":"姜照舟_底线","authority_quote":"不以心核换取安逸生活","candidate_field":"第40章_预期转折","candidate_quote":"姜照舟牺牲自己嵌入心核，逆转潮汐"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Uncertain);
        assert!(finding.evidence.is_none());
    }

    #[test]
    fn synonymous_voluntary_action_still_violates_the_same_bottom_line() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "裴望川".to_string(),
            role: "主角".to_string(),
            bottom_line: "不牺牲同伴换取通行权".to_string(),
            ..Default::default()
        }];
        contract.premise = "裴望川带领同伴穿越封锁区。".to_string();
        contract.ending.desired_resolution = "众人抵达安全区。".to_string();
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(12),
                goal: "裴望川面对守门人的交易".to_string(),
                expected_turn: "裴望川决定主动交出同伴，以此获得通行权".to_string(),
            }];
        let request = outline_character_authority_review_request(&contract).expect("review");
        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"主角主动执行了底线禁止的交易","evidence":{"authority_field":"裴望川_底线","authority_quote":"不牺牲同伴换取通行权","candidate_field":"第12章_预期转折","candidate_quote":"裴望川决定主动交出同伴，以此获得通行权"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Conflict);
        assert!(finding.evidence.is_some());
    }

    #[test]
    fn forced_or_negated_bottom_line_action_is_not_a_voluntary_choice() {
        assert!(!candidate_explicitly_executes_forbidden_choice(
            "绝不交出听雪楼控制权",
            "阮星安被迫交出听雪楼控制权"
        ));
        assert!(!candidate_explicitly_executes_forbidden_choice(
            "绝不放弃听雪楼控制权",
            "阮星安拒绝放弃听雪楼控制权"
        ));
        assert!(candidate_explicitly_executes_forbidden_choice(
            "绝不交出听雪楼控制权",
            "阮星安决定主动交出听雪楼控制权"
        ));
        assert!(candidate_explicitly_executes_forbidden_choice(
            "不牺牲同伴换取通行权",
            "裴望川决定主动交出同伴，以此获得通行权"
        ));
        assert!(!candidate_explicitly_executes_forbidden_choice(
            "不以心核换取安逸生活",
            "姜照舟牺牲自己嵌入心核，逆转潮汐"
        ));
    }

    #[test]
    fn short_shared_clause_does_not_bypass_semantic_review() {
        assert!(!clauses_lexically_match("主角胜利", "胜利"));
        assert!(clauses_lexically_match(
            "主角公开账册并终结垄断",
            "最终主角公开账册并终结垄断"
        ));
    }

    #[test]
    fn canonicalizing_patch_changes_only_explicit_outline_ending() {
        let request = EndingEquivalenceReviewRequest {
            canonical_ending: "主角公开账册并终结垄断".to_string(),
            outline_ending: "账册被公开，旧秩序倒塌".to_string(),
            raw_outline: "开局：主角得到残页。结局：账册被公开，旧秩序倒塌。尾声保留余波。"
                .to_string(),
        };
        let patch: serde_json::Value =
            serde_json::from_str(&request.canonicalizing_plot_patch()).expect("patch");
        let outline = patch
            .pointer("/outline/raw_outline")
            .and_then(serde_json::Value::as_str)
            .expect("outline");
        assert_eq!(
            outline,
            "开局：主角得到残页。结局：主角公开账册并终结垄断。尾声保留余波。"
        );
    }

    #[test]
    fn user_story_authority_review_carries_core_and_generated_contract_fields() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![
            super::super::creation_contract_model::CharacterContract {
                canonical_name: "韩知朔".to_string(),
                role: "同伴".to_string(),
                desire: "让旧工程死者被正式登记".to_string(),
                fear: "名册再次被篡改".to_string(),
                bottom_line: "绝不冒用死者身份".to_string(),
                ..Default::default()
            },
            super::super::creation_contract_model::CharacterContract {
                canonical_name: "闻望言".to_string(),
                role: "对手".to_string(),
                desire: "掩盖赔偿账目".to_string(),
                fear: "审计发现身份置换".to_string(),
                bottom_line: "绝不交出旧名册".to_string(),
                ..Default::default()
            },
        ];
        contract.premise = "店主发现施工方使用劣质管材牟利。".to_string();
        contract.main_causal_spine = "劣质管材渗漏，然后店主追查。".to_string();
        contract.ending.desired_resolution = "施工方回购管材。".to_string();
        contract.outline.raw_outline = "店主修复暗管。".to_string();
        contract.outline.volumes = vec![super::super::creation_contract_model::VolumeContract {
            title: "失控阀门".to_string(),
            objective: "店主突然获得控制整座城市燃气管网的能力".to_string(),
            ending_change: "全城阀门被远程关闭".to_string(),
        }];
        contract.outline.near_chapters =
            vec![super::super::creation_contract_model::ChapterSeedContract {
                number: Some(1),
                goal: "检查事故楼的暗管".to_string(),
                expected_turn: "发现阀门编号被调换".to_string(),
            }];

        let request = user_story_authority_review_request(
            "有人借燃气事故制造整栋楼低价清退，店主必须阻止产权被吞并。",
            &contract,
        )
        .expect("review");
        let prompt = request.prompt();

        assert!(prompt.contains("整栋楼低价清退"));
        assert!(prompt.contains("劣质管材牟利"));
        assert!(prompt.contains("施工方回购管材"));
        assert!(prompt.contains("失控阀门"));
        assert!(prompt.contains("控制整座城市燃气管网"));
        assert!(prompt.contains("发现阀门编号被调换"));
        assert!(prompt.contains("韩知朔"));
        assert!(prompt.contains("闻望言"));
        assert!(prompt.contains("同伴与对手身份错置"));
        assert!(prompt.contains("证据推出的跳跃"));
        assert!(prompt.contains("能力边界"));
        assert!(prompt.contains("从未建立的关键能力"));
        assert!(prompt.contains("相关性"));
        assert!(prompt.contains("主动制造、控制"));
        assert!(prompt.contains("实体类型必须自洽"));
        assert!(prompt.contains("主冲突或终局已经完成后又重启同一主线"));
        assert!(prompt.contains("单卷拼入多套卷目标或重复卷尾"));
        assert!(prompt.contains("近期章节目标存在缺谓语"));
        assert!(prompt.contains("必须判 conflict"));
        assert!(prompt.contains("词序错乱"));
        assert!(prompt.contains("不可读拼接"));
        assert!(prompt.contains("后续明确修订"));
        assert!(prompt.contains("不能靠泛称、隐含推断或题材常识补足"));
        assert!(prompt.contains("明确亲属关系降级为无法确认归属的职位泛称"));
    }

    #[test]
    fn outline_character_authority_review_runs_without_explicit_user_story_core() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![
            super::super::creation_contract_model::CharacterContract {
                canonical_name: "岑知声".to_string(),
                role: "女主".to_string(),
                desire: "还原档案颜色真相".to_string(),
                fear: "视觉缺陷使证词无人相信".to_string(),
                bottom_line: "绝不销毁纸质证物".to_string(),
                arc_start: "怀疑自己的色彩感知".to_string(),
                arc_end: "信任自身感知并主动破局".to_string(),
                ..Default::default()
            },
            super::super::creation_contract_model::CharacterContract {
                canonical_name: "秦承朔".to_string(),
                role: "对手".to_string(),
                desire: "掩盖地下管网旧案".to_string(),
                fear: "视觉陷阱被岑知声识破".to_string(),
                bottom_line: "绝不留下纸质证据".to_string(),
                arc_start: "利用灯光布置陷阱".to_string(),
                arc_end: "灯光陷阱被反向利用后落败".to_string(),
                ..Default::default()
            },
        ];
        contract.premise = "患有间歇性色盲的岑知声追查被篡改的旧案档案。".to_string();
        contract.protagonist_arc = "岑知声从怀疑感官转为信任自身判断。".to_string();
        contract.ending.desired_resolution = "岑知声利用色盲特质识破秦承朔的灯光陷阱。".to_string();
        contract.world_rules = vec!["岑知声在红光下对红色物体的感知较弱。".to_string()];
        contract.outline.volumes = vec![super::super::creation_contract_model::VolumeContract {
            title: "盲点终局".to_string(),
            objective: "岑知声反向利用灯光规则锁定秦承朔".to_string(),
            ending_change: "秦承朔因色盲特性无法分辨路径而坠落".to_string(),
        }];

        let request =
            outline_character_authority_review_request(&contract).expect("internal review");
        let prompt = request.prompt();

        assert!(prompt.contains("岑知声"));
        assert!(prompt.contains("秦承朔"));
        assert!(prompt.contains("患有间歇性色盲"));
        assert!(prompt.contains("秦承朔因色盲特性"));
        assert!(prompt.contains("身体特征"));
        assert!(prompt.contains("明确转移给另一名角色"));
        assert!(prompt.contains("广义叙事职能"));
        assert!(prompt.contains("更具体的关系称谓"));
        assert!(prompt.contains("禁止边界"));
        assert!(prompt.contains("是遵守底线，不是违反底线"));
        assert!(prompt.contains("不是保证角色永远不会被外力击败"));
        assert!(prompt.contains("只有候选明确写出该角色选择、决定、同意或亲手实施"));
        assert!(prompt.contains("性别称谓、代词"));
        assert!(prompt.contains("伤病、残疾、能力损失、康复和再次受伤"));
        assert!(prompt.contains("失败后果写成成功突破"));
        assert!(prompt.contains("书名、书名理由与合同核心字段"));
        assert!(prompt.contains("只改一个字、近音字或形近字"));
        assert!(prompt.contains("正式别名"));
        assert!(prompt.contains("每个具名协议、法则、装置、药物、术法或能力"));
        assert!(prompt.contains("改写、反转、重定向、目标替换及其代价"));
        assert!(prompt.contains("密钥、病毒、后门、解药、核心"));
        assert!(prompt.contains("主线或大纲没有先建立转换步骤"));
        assert!(prompt.contains("每卷目标、卷尾变化、近期章节目标和预期转折"));
        assert!(prompt.contains("缺谓语、缺必要介词或宾语"));
        assert!(prompt.contains("只确认人物姓名却没有事件结果"));
        assert!(prompt.contains("终局方向与兑现矩阵"));
        assert!(prompt.contains("不得用角色底线替相反的终局结果开脱"));
        assert!(prompt.contains("阶段性胜负、失能或失去手段"));
        assert!(prompt.contains("卷目标写终局行动、同卷卷尾变化写该行动造成的不可逆结果"));
        assert!(prompt.contains("必须区分渐进变化与最终完成"));
        assert!(request
            .fingerprint()
            .starts_with("outline_character_authority\n"));
    }

    #[test]
    fn named_core_object_spelling_conflict_is_grounded_across_contract_fields() {
        let mut contract = NovelCreationContract::default();
        contract.title.canonical_title = "骨荒界：噬骨罗盘".to_string();
        contract.title.rationale = "噬骨罗盘是贯穿全书的核心道具。".to_string();
        contract.characters = vec![super::super::creation_contract_model::CharacterContract {
            canonical_name: "姜怀言".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        }];
        contract.brief = "姜怀言依靠噬骨罗盘寻找世界之心。".to_string();
        contract.premise = "姜怀言获得上古神器蚀骨罗盘并深入骨荒界。".to_string();
        contract.main_causal_spine = "姜怀言启动噬骨罗盘并抵达世界之心。".to_string();
        contract.ending.desired_resolution = "姜怀言修复世界之心。".to_string();
        contract.outline.volumes = vec![super::super::creation_contract_model::VolumeContract {
            title: "罗盘初醒".to_string(),
            objective: "姜怀言掌握噬骨罗盘".to_string(),
            ending_change: "噬骨罗盘完成第一次导航".to_string(),
        }];

        let request = outline_character_authority_review_request(&contract).expect("review");
        assert!(request.story_authority.contains("骨荒界：噬骨罗盘"));
        assert!(request.contract_fields.contains("上古神器蚀骨罗盘"));

        let finding = request.ground_finding(parse_semantic_review_finding(
            r#"{"verdict":"conflict","rationale":"核心道具名称发生一字漂移","evidence":{"authority_field":"书名","authority_quote":"噬骨罗盘","candidate_field":"故事前提","candidate_quote":"蚀骨罗盘"}}"#,
        ));

        assert_eq!(finding.verdict, SemanticReviewVerdict::Conflict);
        assert!(finding.evidence.is_some());
    }
}
