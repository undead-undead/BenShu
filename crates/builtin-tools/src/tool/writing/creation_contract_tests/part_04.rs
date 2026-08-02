fn pending_normalized_contract(
    pending_contract_candidate: Option<&serde_json::Value>,
) -> super::super::NovelCreationContract {
    let value = pending_contract_candidate
        .and_then(|candidate| candidate.get("normalized"))
        .expect("pending normalized contract");
    super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
        .expect("pending typed contract")
}

fn current_contract_text(draft: &super::super::SessionCreationDraftState) -> String {
    draft
        .current_contract
        .as_ref()
        .map(|value| value.to_string())
        .expect("current typed contract")
}

fn full_contract_view(draft: &super::super::SessionCreationDraftState) -> String {
    super::super::render_creation_draft_contract_view(draft, true)
}

#[tokio::test]
async fn viewing_current_contract_is_read_only() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-a",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "雨巷灵契".to_string();
    draft.fiction_premise = "旧城雨巷出现灵能裂缝，普通学生被迫卷入守城试炼。".to_string();

    let mut runtime = MockCreationDraftRuntime {
        draft: Some(draft),
        recovered_draft: None,
        continuation_project_path: None,
        project_path: "data/generated/novels/test-project".to_string(),
        saved: 0,
        approved: 0,
    };

    let outcome = super::super::handle_creation_draft_chat(
        &mut runtime,
        "session-a",
        "把刚才生成的小说合同草案给我看一下。",
    )
    .await
    .expect("handled")
    .expect("outcome");

    let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
        panic!("contract view should be a read-only response, not a new planning prompt");
    };
    assert_eq!(runtime.approved, 0);
    assert!(response.response.contains("当前草案"));
    assert!(!response
        .response
        .contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
}

#[test]
fn non_band_chapter_unit_is_not_promoted_to_user_authority() {
    let draft = super::super::build_initial_creation_draft(
        "session-normalized-band",
        "fiction",
        "写都市玄幻小说，每章3000字，至少5万字。",
    )
    .expect("draft");

    assert_eq!(draft.chapter_unit_target, None);
    assert!(!draft.chapter_unit_target_user_specified);
    assert_eq!(draft.chapter_unit_target_user_authority, None);
    let status = super::super::render_creation_draft_compact_status(&draft);
    assert!(!status.contains("已自动归一"), "{status}");
}

#[test]
fn compact_status_hides_temporary_novel_title_placeholder() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-temp-title-hidden",
        "fiction",
        "写近未来深海档案悬疑小说，总字数10万字，每章2500字。",
    )
    .expect("draft");
    draft.title = "未命名小说-1234abcd".to_string();

    let status = super::super::render_creation_draft_compact_status(&draft);

    assert!(!status.contains("未命名小说-"), "{status}");
    assert!(status.contains("- 标题：待定"), "{status}");
}

#[test]
fn character_fear_anchor_survives_visible_draft_rebuild() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-fear-anchor",
        "fiction",
        "写一部民国奇幻探案小说，每章2500字，一共5万字。",
    )
    .expect("draft");
    draft.title = "纸人借命".to_string();
    draft.fiction_title_rationale =
        "以纸人替身和借命阴谋作为故事独有钩子，指向终局反噬。".to_string();
    draft.fiction_premise = "民国上海滩连续富商暴毙，落魄仵作被卷入纸人借命案。".to_string();
    draft.fiction_ending_direction = "主角在葬礼高潮用纸人替身让幕后黑手承受寿元反噬。".to_string();
    draft.fiction_protagonist_arc =
        "从只想靠手艺混饭吃的落魄仵作，到守住阴阳平衡的人。".to_string();
    draft.fiction_world_imagery = "雾港、纸人、阴司当铺、寿元账册".to_string();
    draft.fiction_main_causal_spine =
        "富商暴毙->发现纸人线索->追踪阴司当铺->揭露借命阴谋->葬礼反噬".to_string();
    draft.fiction_world_rules = vec![
        "纸人替身必须以血引激活，施术者会短暂丧失对应感官。".to_string(),
        "阴煞会沿接触因果反噬，需要三日内封印。".to_string(),
    ];
    draft.fiction_characters = vec![
            "name: 温闻珩; role: 主角; desire: 靠手艺查清借命案; fear: 被权贵权势碾压; bottom_line: 不牺牲无辜换取活路; arc_start: 落魄仵作; arc_end: 阴阳平衡守护者".to_string(),
            "name: 白阙川; role: 关键同伴; desire: 找回失踪姐姐; fear: 被阴司契约束缚; bottom_line: 不背弃承诺; arc_start: 契约受害者; arc_end: 共同揭开当铺真相".to_string(),
            "name: 沈澈澜; role: 对手; desire: 借寿续命; fear: 寿元耗尽; bottom_line: 不让借命账册公开; arc_start: 幕后操盘者; arc_end: 承受反噬的人".to_string(),
        ];
    draft.fiction_outline =
            "第一卷 纸人入局：温闻珩验尸发现纸人血引，追到阴司当铺。第二卷 借命账册：他确认权贵寿元交易并逼近葬礼局。第三卷 葬礼反噬：他用替身术反转借命仪式。".to_string();

    let issues = super::super::creation_draft_contract_blocking_issues_for_scope(
        &draft,
        super::super::ContractReadinessScope::LockedAuthorityContract,
    );

    assert!(
        !issues.iter().any(|issue| issue.contains("缺少恐惧锚点")),
        "{issues:?}"
    );
}

#[test]
fn food_business_contract_keeps_world_rules_and_action_title() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-food-business-contract",
        "fiction",
        "写一部古代美食商战小说，每章2500字，一共5万字。",
    )
    .expect("draft");
    draft.title = "破盐铁令".to_string();
    draft.fiction_title_rationale =
        "以盐铁专营制度漏洞和主角打破食材定价垄断作为书名钩子，指向终局破局。".to_string();
    draft.brief =
        "落魄厨娘凭失传菜谱与绝世味觉，在盐铁垄断时代以美食定价权瓦解世家垄断。".to_string();
    draft.fiction_premise = "在盐铁专营、世家垄断食材的古代背景下，拥有绝世味觉却身负家族冤案的落魄厨娘，凭借失传菜谱进入权贵视野。".to_string();
    draft.fiction_ending_direction =
        "主角打破盐铁世家对高端食材的定价权，建立独立商号并查清家族冤案。".to_string();
    draft.fiction_protagonist_arc =
        "从只靠厨艺求生的落魄厨娘，成长为能制定行业规则的商业领袖。".to_string();
    draft.fiction_world_imagery = "市井灶台、盐引票据、御膳房、算盘珠声和香料烟火。".to_string();
    draft.fiction_main_causal_spine = "家族冤案迫使主角入局->失传菜谱引出权贵订单->盐价波动暴露食材垄断->主角重建供应链->终局打破定价权。".to_string();
    draft.fiction_characters = vec![
            "name: 沈惊澜; role: 主角; desire: 重振沈家厨业; fear: 味觉退化后失去翻案筹码; bottom_line: 绝不交出祖传菜谱核心配方; arc_start: 落魄厨娘; arc_end: 制定美食商号规则的人".to_string(),
            "name: 萧景珩; role: 关系对象; desire: 摆脱皇权联姻束缚; fear: 再次被家族当作筹码; bottom_line: 不用沈惊澜的冤案换取自身自由; arc_start: 冷眼旁观的权贵; arc_end: 与主角共同承担商战代价的人".to_string(),
            "name: 赵万金; role: 关键对手; desire: 继续垄断盐铁与高端食材; fear: 盐引账册公开; bottom_line: 不让世家垄断证据进入御前; arc_start: 掌控食材行会的人; arc_end: 被迫交出定价权的人".to_string(),
        ];
    draft.fiction_world_rules = vec![
        "绝世味觉每次用于辨价都会造成短暂味觉反噬，连续使用会让主角失去谈判判断。".to_string(),
        "盐价波动直接影响高端食材流通，错误囤货会让商号资金链断裂。".to_string(),
        "御膳房认证能抬高商号价格，但违反独供契约会失去皇家背书并赔付巨额违约金。".to_string(),
    ];
    draft.fiction_themes = vec!["技艺、制度和商业定价权的争夺。".to_string()];
    draft.fiction_style_rules = vec!["用具体交易场景推进商战，不用空泛总结代替冲突。".to_string()];
    draft.fiction_must_avoid = vec!["不要把美食能力写成无代价万能外挂。".to_string()];
    draft.fiction_outline = "第一卷 市井灶火：主角凭失传菜谱拿到第一笔权贵订单。第二卷 盐引风波：主角发现盐价和食材垄断的因果。第三卷 御膳破局：主角用御膳认证反制世家。".to_string();

    let issues = super::super::creation_draft_contract_blocking_issues_for_scope(
        &draft,
        super::super::ContractReadinessScope::LockedAuthorityContract,
    );

    assert!(
        !issues.iter().any(|issue| issue.contains("缺少世界规则")
            || issue.contains("缺少可锁定书名")
            || issue.contains("尚未形成可锁定书名")),
        "{issues:?}"
    );
}

#[test]
fn skeleton_patch_brief_replaces_prior_generated_brief_instead_of_accumulating() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-skeleton-brief-replace",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let first = r#"{
  "patch_type": "skeleton_patch",
  "genre": "都市玄幻",
  "brief": "草根青年卷入城市灵脉复苏。",
  "premise": "旧城灵脉突然复苏，主角被迫进入夜校试炼。",
  "ending": {"desired_resolution": "主角公开灵脉黑幕并重建城市修行秩序。", "final_state": "城市修行入口恢复公开。"},
  "protagonist_arc": "从旁观自保到主动承担城市代价。",
  "world_imagery": "旧城、夜校、灵脉轨道。",
  "main_causal_spine": "灵脉复苏引发夜校异常，主角追查真相并在终局重建秩序。"
}"#;
    let second = r#"{
  "patch_type": "skeleton_patch",
  "genre": "都市玄幻",
  "brief": "低阶打工人发现城市灵枢账本，反击资本化修行体系。",
  "premise": "城市灵枢被资本集团垄断，底层打工人意外拿到账本证据。",
  "ending": {"desired_resolution": "主角用灵枢账本公开垄断证据，夺回普通人的修行资格。", "final_state": "普通人的修行资格获得制度保障。"},
  "protagonist_arc": "从只想保住工作到敢公开证据的破局者。",
  "world_imagery": "地铁灵脉、旧楼账本、霓虹法阵。",
  "main_causal_spine": "账本证据引来追杀，主角层层反击，终局公开证据改写晋升规则。"
}"#;

    let _ = super::super::submit_generated_contract_candidate_to_draft(&mut draft, first);
    let _ = super::super::submit_generated_contract_candidate_to_draft(&mut draft, second);

    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(
        pending.brief,
        "低阶打工人发现城市灵枢账本，反击资本化修行体系"
    );
    assert_eq!(
        pending.premise,
        "城市灵枢被资本集团垄断，底层打工人意外拿到账本证据"
    );
    assert_eq!(
        pending.ending.desired_resolution,
        "主角用灵枢账本公开垄断证据，夺回普通人的修行资格"
    );
    assert_eq!(
        pending.main_causal_spine,
        "账本证据引来追杀，主角层层反击，终局公开证据改写晋升规则"
    );
    assert!(
        !pending.brief.contains("草根青年卷入城市灵脉复苏"),
        "{}",
        pending.brief
    );
}

#[test]
fn skeleton_patch_does_not_derive_local_character_authority_when_model_omits_character_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-character-authority",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");

    let raw = r#"{
  "patch_type": "skeleton_patch",
  "title": {
    "canonical_title": "薪火断章",
    "rationale": "书名来自主角点燃薪火传承、打破旧秩序并重塑修仙体系的终局。"
  },
  "genre": "异界修仙",
  "brief": "凡人卷入灵气枯竭后的修仙秩序重建。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "主角林渊意外获得上古薪火传承，被迫进入宗门与旧秩序的争夺。",
  "ending": {
    "desired_resolution": "主角公开薪火传承的真实代价，打破宗门垄断，重塑普通人的入道规则。",
    "final_state": "旧宗门秩序崩解，新的入道规则由凡人共同守护。"
  },
  "protagonist_arc": "从被动求生到主动承担规则重建的代价。",
  "world_imagery": "灵气枯竭的边城、薪火传承、宗门天梯。",
  "main_causal_spine": "薪火传承暴露旧宗门漏洞，引来追杀和试炼，主角逐步揭开代价并在终局重写入道规则。"
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.committed);
    assert!(draft.fiction_characters.is_empty());
    assert!(draft.current_contract.is_none());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending.characters.is_empty());
    let visible_surface = [
        draft.fiction_premise.as_str(),
        draft.fiction_ending_direction.as_str(),
        draft.fiction_protagonist_arc.as_str(),
        draft.fiction_main_causal_spine.as_str(),
        &draft.fiction_characters.join("\n"),
    ]
    .join("\n");
    assert!(
            !visible_surface.contains("林渊"),
            "stale model-default names must be removed from pending contract text without synthesizing local characters: {visible_surface}"
        );
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("角色权威表")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn skeleton_patch_does_not_replace_pronoun_primary_anchor_or_derive_world_rules() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-pronoun-character-authority",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");

    let raw = r#"{
  "patch_type": "skeleton_patch",
  "title": {
    "canonical_title": "断剑天梯",
    "rationale": "终局主角以凡铁断剑斩断天梯，废除灵根税制。"
  },
  "genre": "异界修仙",
  "brief": "无灵根少年在严苛的灵根税制下，以凡铁断剑斩碎天道阶梯，重定修仙秩序。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "异界修仙界实行灵根税，主角自己因家贫沦为税奴，偶然获得一把能吞噬灵气的断剑。",
  "ending": {
    "desired_resolution": "自己废除灵根税制，众生平等皆可修仙。",
    "final_state": "天梯崩塌，普通人获得入道资格。"
  },
  "protagonist_arc": "从逆来顺受的底层税奴，成长为敢于挑战权威的破局者。",
  "world_imagery": "灵石天梯、灵根税碑、噬灵断剑。",
  "main_causal_spine": "灵根税制导致阶层固化->主角自己因贫受辱->获得噬灵断剑->发现天梯秘密->斩断天梯->重构世界秩序"
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.committed, "{:?}", outcome.gate.actionable_issues());
    assert!(draft.fiction_characters.is_empty());
    assert!(draft.fiction_world_rules.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let visible_surface = [
        draft.fiction_premise.as_str(),
        draft.fiction_ending_direction.as_str(),
        draft.fiction_main_causal_spine.as_str(),
        &draft.fiction_characters.join("\n"),
    ]
    .join("\n");
    assert!(
            !visible_surface.contains("主角自己") && !visible_surface.contains("自己废除"),
            "pronoun primary anchor should be cleaned, not replaced by locally invented authority name: {visible_surface}"
        );
    assert!(
        pending.world_rules.is_empty(),
        "world rules must come from the model contract or typed patch, not local story inference"
    );
}

#[test]
fn jsonish_patch_with_local_key_drift_keeps_user_numeric_authority() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-jsonish-patch-drift",
        "fiction",
        "写都市轻玄幻短篇小说，每章2500字，总字数5000字。",
    )
    .expect("draft");

    let raw = r#"{
  "patchtype":"skeletonpatch",
  "title":{
    "canonicaltitle":"旧物回收站",
    "candidates":[
      {"title":"旧物回收站","hooktype":"地点事件","ration":"故事核心场景为旧物回收站。"},
      {"title："雨夜交易","hooktype":"爽点行动","rationale":"主角在暴雨之夜利用规则漏洞完成关键交易。"}
    ],
    "rationale":"旧物回收站既是故事地点，也是主角回收破碎人生的隐喻。"
  },
  "genre":"都市轻玄幻",
  "brief":"落魄青年在旧物回收站修复带灵性的旧物。",
  "targetunits":50000,
  "chapterunittarget":2500,
  "maxchaptersperturn":1,
  "premise":"主角继承濒临倒闭的旧物回收站，发现旧物中隐藏灵痕。",
  "ending":{"desiredresolution":"主角修复城市核心灵脉枢纽。","finalstate":"旧物回收站成为普通人与灵能界的桥梁。"},
  "protagonistarc":"从只想守住店铺到主动承担城市代价。",
  "worldimagery":"霓虹灯牌下的潮湿巷弄、堆满杂物的昏暗店铺、金色灵痕。",
  "maincausalspine":"修复旧物->发现灵痕价值->遭遇稽查->修复城市灵脉。"
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.committed, "{:?}", outcome.gate.actionable_issues());
    assert_eq!(draft.target_units, Some(5000));
    assert_eq!(draft.chapter_unit_target, Some(2500));
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending.premise.contains("旧物回收站"));
    assert!(pending.main_causal_spine.contains("修复城市灵脉"));
}

#[test]
fn non_primary_character_can_reference_protagonist_in_anchors() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-role-parse",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_characters = vec![
            "name: 顾衡棠; role: 主角; desire: 登临剑道极致; fear: 剑心崩坏; bottom_line: 不以无辜者铸剑".to_string(),
            "name: 司砚川; role: 重要角色; desire: 助主角登临剑道之巅; fear: 无法护住所爱之人; bottom_line: 不背弃剑誓".to_string(),
            "name: 景澈砺; role: 对手; desire: 掌控九重天界; fear: 主角剑道法则超越自身极限; bottom_line: 以阴谋和力量压制主角".to_string(),
        ];

    let issues = super::super::creation_draft_approval_readiness_issues(&draft);

    assert!(
        !issues
            .iter()
            .any(|issue| issue.contains("角色权威表缺少非主角")),
        "{issues:?}"
    );
}

#[test]
fn character_patch_preserves_story_residue_for_targeted_repair_and_canonicalizes_ledgers() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-authority-canonicalize",
        "fiction",
        "写都市言情小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type": "character_patch",
              "characters": [
                {
                  "canonical_name": "宋桥序",
                  "role": "主角",
                  "desire": "守住职业尊严",
                  "fear": "被亲密关系吞没",
                  "bottom_line": "不牺牲自己的判断",
                  "arc_start": "习惯忍让",
                  "arc_end": "独立选择"
                },
                {
                  "canonical_name": "祝岑澜",
                  "role": "关键关系对象",
                  "desire": "帮助苏晚晴看清真相",
                  "fear": "再次失去信任",
                  "bottom_line": "不利用宋桥序的脆弱",
                  "arc_start": "保持距离",
                  "arc_end": "共同承担"
                }
              ],
              "relationship_ledger": [
                {
                  "characters": ["祝岑澜", "苏晚晴"],
                  "relationship_type": "误会到信任",
                  "start_state": "互相试探",
                  "desired_end_state": "共同承担"
                }
              ],
              "emotional_state_ledger": [
                {
                  "character": "苏晚晴",
                  "current_emotion": "不安",
                  "expected_next_shift": "重新选择"
                }
              ]
            }"#,
    );

    assert!(!outcome.committed, "{:?}", outcome.gate.actionable_issues());
    let contract = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let joined_characters = contract
        .characters
        .iter()
        .map(|character| {
            format!(
                "{} {} {} {}",
                character.canonical_name, character.role, character.desire, character.bottom_line
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined_characters.contains("苏晚晴"), "{joined_characters}");
    assert!(
        joined_characters.contains("不牺牲自己的判断"),
        "self-pronoun anchors must not be rewritten into the authority name: {joined_characters}"
    );
    assert!(
        !joined_characters.contains("宋桥序的判断"),
        "bare 自己 in character anchors must remain a self-pronoun: {joined_characters}"
    );
    let authority_names = draft
        .fiction_characters
        .iter()
        .filter_map(|line| super::super::character_name_from_contract_line(line))
        .collect::<Vec<_>>();
    let authority_names = if authority_names.is_empty() {
        contract
            .characters
            .iter()
            .map(|character| character.canonical_name.clone())
            .collect::<Vec<_>>()
    } else {
        authority_names
    };
    assert!(
        authority_names.len() >= 2,
        "authority names: {authority_names:?}; {joined_characters}"
    );
    assert!(contract
        .structured
        .relationship_ledger
        .iter()
        .flat_map(|entry| entry.characters.iter())
        .any(|name| name == "苏晚晴"));
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| { issue.contains("角色权威表") && issue.contains("苏晚晴") }));
}

#[test]
fn plot_patch_preserves_pending_character_authority_across_staged_repairs() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-staged-character-plot",
        "fiction",
        "写现实体育职场悬疑小说，每章2500字，总字数10万字。",
    )
    .expect("draft");

    let skeleton = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"skeleton_patch",
              "title":{"canonical_title":"界外哨声","rationale":"哨声对应联赛资格裁决，界外对应被人为制造的违规证据。"},
              "language":"zh-CN",
              "genre":"现实体育职场悬疑",
              "brief":"年轻助教追查球队被操纵转让的证据链。",
              "target_units":100000,
              "chapter_unit_target":2500,
              "premise":"赞助商制造兴奋剂证据和训练事故，逼女子排球队失去资格后低价转让。",
              "ending":{"desired_resolution":"助教公开完整操纵证据，撤销虚假处罚，保住球队资格并迫使低价转让协议永久失效。"},
              "protagonist_arc":"从只敢记录数据的助教成长为公开对抗权力并承担球队命运的教练。",
              "world_imagery":"县城体校、老锅炉房、泛黄训练日志和空荡排球馆。",
              "main_causal_spine":"匿名举报引发资格危机，锅炉事故暴露人为痕迹，助教串联药检与维修证据，终局公开操纵链并阻止低价转让。"
            }"#,
    );
    assert!(!skeleton.is_ready());

    let characters = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "characters":[
                {"canonical_name":"裴望遥","role":"主角","desire":"查清操纵链并保住队员清白","fear":"公开质疑赞助商会让全队提前解散","bottom_line":"绝不伪造药检或拿队员安全换取资格","arc_start":"只敢在幕后记录数据","arc_end":"公开证据并承担教练责任"},
                {"canonical_name":"姜望野","role":"关键队员","desire":"洗清违规嫌疑并重返赛场","fear":"伤病和污名让职业生涯同时终结","bottom_line":"绝不服用来源不明的药物或牺牲队友换主力位置","arc_start":"用强撑掩盖恐惧","arc_end":"主动配合调查并保护新人"},
                {"canonical_name":"叶谨白","role":"对手","desire":"让球队失去资格后完成低价整体转让","fear":"药检和事故记录被串成可公开验证的证据链","bottom_line":"一旦交易成本失控就会切断与执行者的联系","arc_start":"以赞助权控制队内决定","arc_end":"操纵证据公开后失去交易资格"}
              ]
            }"#,
    );
    assert!(!characters.is_ready());
    let character_contract = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(character_contract.characters.len(), 3);

    let plot = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"plot_patch",
              "outline":{
                "volumes":[
                  {"title":"污点","objective":"匿名举报迫使助教核对药检与事故记录","ending_change":"助教确认两类证据都被同一赞助体系控制"},
                  {"title":"断链","objective":"找到制造假证据和训练事故的执行层","ending_change":"执行者交出无法撤回的原始记录"},
                  {"title":"资格","objective":"在联赛裁决前公开操纵链并阻止转让","ending_change":"虚假处罚撤销且低价转让协议永久失效"}
                ],
                "near_chapters":[
                  {"number":1,"goal":"锅炉事故后保存训练现场记录","expected_turn":"主角发现维修时间与匿名举报发送时间重合"},
                  {"number":2,"goal":"复核关键队员的药检样本交接链","expected_turn":"样本签收人属于赞助商关联机构"},
                  {"number":3,"goal":"说服受伤队员共同保全证据","expected_turn":"队员交出事故前收到的来源不明药物"}
                ],
                "raw_outline":"匿名举报和锅炉事故并发，助教从训练数据追到药检与维修记录的共同控制者，最终在联赛裁决前公开完整操纵链，撤销虚假处罚并使低价转让协议永久失效。"
              }
            }"#,
    );
    assert!(!plot.is_ready());
    let plot_contract = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(
        plot_contract.characters.len(),
        3,
        "plot repair must preserve the pending character authority: {:?}",
        plot.gate.actionable_issues()
    );
    let governed_names = plot_contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !name.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(governed_names.len(), 3);
    assert!(plot_contract
        .characters
        .iter()
        .any(|character| character.role.contains("主角")));
    assert_eq!(plot_contract.outline.volumes.len(), 3);
    assert_eq!(plot_contract.outline.near_chapters.len(), 3);

    let corrected_skeleton = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
                  "patch_type":"skeleton_patch",
                  "premise":"县女子排球队在联赛裁决前遭遇伪造药检与训练事故，年轻助教必须串联原始记录保住球队资格。",
                  "ending":{
                    "desired_resolution":"助教公开完整操纵证据，撤销虚假处罚，保住球队资格并迫使低价转让协议永久失效。",
                    "final_state":"球队资格恢复，操纵方永久失去交易资格，药检与维修记录进入公开复核制度。"
                  }
                }"#,
    );
    assert!(!corrected_skeleton.is_ready());
    let corrected_contract = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(
        corrected_contract.outline.volumes.len(),
        3,
        "later skeleton repair must not discard an accepted plot patch"
    );
    assert_eq!(
        corrected_contract.outline.near_chapters.len(),
        3,
        "later skeleton repair must not discard accepted near-chapter planning"
    );

    let corrected_character = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
                  "patch_type":"character_patch",
                  "characters":[
                    {"canonical_name":"裴望遥","role":"主角","bottom_line":"绝不伪造药检或拿队员安全换取资格"},
                    {"canonical_name":"姜望野","role":"关键队员","bottom_line":"绝不服用来源不明的药物或牺牲队友换主力位置"},
                    {"canonical_name":"叶谨白","role":"对手","bottom_line":"交易成本失控时也不得销毁原始药检记录"}
                  ]
                }"#,
    );
    assert!(!corrected_character.is_ready());
    let character_corrected_contract =
        pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(
        character_corrected_contract.outline.volumes.len(),
        3,
        "later character repair must not discard an accepted plot patch"
    );
    assert_eq!(
        character_corrected_contract.outline.near_chapters.len(),
        3,
        "later character repair must not discard accepted near-chapter planning"
    );

    let governance_without_patch_type = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
                  "themes":["程序正义与团队责任"],
                  "world_rules":[
                    "联赛资格裁决必须由药检原始链与训练事故原始记录交叉验证。",
                    "任何训练停摆都会让球队失去下一轮举证窗口。",
                    "赞助资源只能通过公开预算进入球队，私下补偿会使证据失效。"
                  ],
                  "style_rules":["用具体训练、比赛和调查场景推进"],
                  "must_avoid":["不要靠巧合获得决定性证据"]
                }"#,
    );
    assert!(!governance_without_patch_type.is_ready());
    let governed_contract = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(
        governed_contract.outline.volumes.len(),
        3,
        "a stage field pack without patch_type must preserve accepted plot fields"
    );
    assert_eq!(
        governed_contract.outline.near_chapters.len(),
        3,
        "a stage field pack without patch_type must preserve accepted near chapters"
    );
    assert_eq!(governed_contract.world_rules.len(), 3);
}

#[test]
fn patch_batch_commits_scale_but_does_not_lock_title_without_story_evidence() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-batch-partial",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"skeleton_patch",
              "title":{"canonical_title":"夺回旧街灵债","rationale":"旧街是主角第一次发现灵能借贷账册的地点，夺回旧街灵债对应终局公开债务证据并重写借贷规则的关键爽点。"},
              "target_units":50000,
              "chapter_unit_target":2500
            }"#,
    );

    assert!(!outcome.committed, "{:?}", outcome.gate.actionable_issues());
    assert_eq!(draft.target_units, Some(50000));
    assert_eq!(draft.chapter_unit_target, Some(2500));
    assert!(
        draft.title.trim().is_empty(),
        "title must not be locked when the patch carries no story evidence: {}",
        draft.title
    );
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(
        pending.title.rationale.contains("重写借贷规则"),
        "{}",
        pending.title.rationale
    );
}

#[test]
fn skeleton_patch_with_user_scale_fields_is_not_empty_scope() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-skeleton-scale-only",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"skeleton_patch","genre":"都市爽文","brief":"都市爽文，每章2500字，至少5万字起","target_units":50000,"chapter_unit_target":2500}"#,
    );

    assert!(
        !outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("skeleton_patch 没有可合并")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
    assert_eq!(draft.genre, "都市爽文");
    assert_eq!(draft.target_units, Some(50_000));
    assert_eq!(draft.chapter_unit_target, Some(2_500));
}

#[test]
fn plot_parser_does_not_treat_chapter_text_with_juan_character_as_volume() {
    let patch = super::super::patch_normalizer::derive_plot_contract_from_outline_text(
        "第1章 本章目标：主角被卷入异能者组织；预期转折：首次接触规则制定者",
    );

    assert!(patch.volumes.is_empty(), "{:?}", patch.volumes);
    assert_eq!(patch.near_chapters.len(), 1);
    assert_eq!(patch.near_chapters[0].number, Some(1));
    assert!(patch.near_chapters[0].goal.contains("被卷入异能者组织"));
}

#[test]
fn structured_labels_do_not_become_visible_world_rules() {
    let mut structured = super::super::NovelContractV2::default();
    structured.power_progression.system_name =
        "瞳孔异能体系、都市暗面法则、视觉力量具象化".to_string();
    structured.geography_model.regions =
        vec!["瞳孔异能体系、都市暗面法则、视觉力量具象化".to_string()];
    structured.resource_economy.resource_types =
        vec!["瞳孔异能体系、都市暗面法则、视觉力量具象化".to_string()];
    structured.power_progression.advancement_costs =
        vec!["每次越级使用异瞳都会损伤现实感知。".to_string()];

    let visible = super::super::visible_governance_fields_from_contract_v2(&structured);

    assert_eq!(
        visible.world_rules,
        vec!["每次越级使用异瞳都会损伤现实感知。".to_string()]
    );
}

#[test]
fn quality_blocked_response_hides_internal_contract_blocker_labels() {
    let response =
        crate::tool::writing::session_surface::creation_contract_quality_blocked_response(&[
            "ContractBlocker: 小说合同缺少世界观意象".to_string(),
            "ContractBlocker: 小说合同缺少书名理由".to_string(),
            "ContractBlocker: 小说合同缺少关系线或关键人物关系账本".to_string(),
        ]);

    assert!(!response.contains("ContractBlocker"), "{response}");
    assert!(response.contains("书名和命名理由"), "{response}");
    assert!(response.contains("世界观规则"), "{response}");
    assert!(response.contains("角色权威表"), "{response}");
}

#[test]
fn contract_quality_repair_uses_staged_typed_patch_prompt() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-staged-repair",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_premise = "旧城夜校出现灵能补考，旁听生被迫调查考场黑幕。".to_string();
    let prompt = super::super::final_prompt_from_contract_quality_repair(
        &draft,
        &draft.brief,
        &["ContractBlocker: 小说合同缺少世界观意象".to_string()],
    );

    assert!(prompt.contains("typed patch"), "{prompt}");
    assert!(prompt.contains("patch_type"), "{prompt}");
    assert!(!prompt.contains("请自动修复这份合同草案"), "{prompt}");
}

#[test]
fn staged_repair_prioritizes_plot_before_governance_after_skeleton_and_characters() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-staged-plot-first",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_premise = "旧城区夜校用借灵证筛掉底层学生。".to_string();
    draft.fiction_ending_direction = "主角公开灵籍账册并改写夜校晋级规则。".to_string();
    draft.fiction_protagonist_arc = "从旁听生到规则改写者。".to_string();
    draft.fiction_world_imagery = "旧城区夜校、灵能准考证、地下灵轨。".to_string();
    draft.fiction_main_causal_spine = "补考异常引出黑账，追查证据，终局重启规则。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻桥; role: 主角; desire: 通过夜校补考; fear: 失去资格; bottom_line: 不牺牲同学; arc_start: 旁听生; arc_end: 规则改写者".to_string(),
            "name: 梁棠; role: 关键关系对象; desire: 查清家族夺权; fear: 相信错误的人; bottom_line: 不把普通人当筹码; arc_start: 监察生; arc_end: 共同破局者".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护考试垄断; fear: 黑幕公开; bottom_line: 不亲手毁掉考试系统; arc_start: 监考者; arc_end: 被证据逼到台前".to_string(),
        ];

    let prompt = super::super::final_prompt_from_contract_quality_repair(
        &draft,
        &draft.brief,
        &[
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string(),
        ],
    );

    assert!(prompt.contains("Plot typed patch"), "{prompt}");
    assert!(prompt.contains("\"plot_patch\""), "{prompt}");
}

#[test]
fn local_contract_repair_aligns_stale_primary_names_to_character_authority() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-primary-name-authority-repair",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "血阶破天".to_string();
    draft.fiction_title_rationale =
        "血阶来自主角踏入宗门试炼的代价，破天来自终局打破天域血祭秩序。".to_string();
    draft.fiction_premise = "主角林渊在异界宗门试炼中发现血阶会吞噬底层修士的命格。".to_string();
    draft.fiction_ending_direction =
        "主角段曜野公开血阶真相，废除以底层命格献祭天门的旧规。".to_string();
    draft.fiction_protagonist_arc =
        "主角林渊从只想保命的外门弟子成长为敢公开天门血契的人。".to_string();
    draft.fiction_world_imagery = "血阶、天门、命格玉简。".to_string();
    draft.fiction_main_causal_spine =
        "主角林渊入宗试炼发现血阶异常，追查命格玉简，终局公开血契真相。".to_string();
    draft.fiction_characters = vec![
            "name: 段曜野; role: 主角; desire: 通过宗门试炼并查清血阶真相; fear: 命格被献祭; bottom_line: 不献祭无辜修士; arc_start: 外门求生者; arc_end: 血契破局者".to_string(),
            "name: 辛衡珩; role: 关键对手; desire: 维护天门血契秩序; fear: 血契真相公开; bottom_line: 不让外门弟子越过宗门阶序; arc_start: 执令者; arc_end: 被证据逼到台前".to_string(),
        ];
    draft.fiction_world_rules = vec![
        "血阶越高，越会抽取底层修士命格。".to_string(),
        "命格玉简只能记录真实献祭痕迹。".to_string(),
    ];
    draft.fiction_outline = "主角林渊进入宗门试炼并触碰血阶；终局公开血契真相。".to_string();
    let contract = super::super::strong_novel_contract_from_creation_draft(&draft);
    let raw = serde_json::to_string(&contract).expect("contract json");

    let blocked = super::super::submit_generated_contract_candidate_to_draft(&mut draft, &raw);
    assert!(!blocked.is_ready());
    assert!(blocked
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("分卷") || issue.contains("近期章节")));
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
}

#[test]
fn title_patch_updates_title_without_touching_characters() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "旧题名".to_string();
    draft.language = "zh-CN".to_string();
    draft.genre = "都市玄幻".to_string();
    draft.brief = "旧城区旁听生卷入灵能考试黑幕。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻桥; role: 主角; desire: 通过灵能考试改变命运; fear: 失去考试资格; bottom_line: 不牺牲同学换取晋级; arc_start: 旁听生; arc_end: 规则改写者".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护灵能考试垄断; fear: 黑幕公开; bottom_line: 不亲手毁掉考试系统; arc_start: 监考者; arc_end: 被证据逼到台前".to_string(),
        ];
    draft.fiction_premise =
        "灵能考试决定城市阶层，旧城区学生发现补考会吞噬普通人的运势。".to_string();
    draft.fiction_ending_direction = "主角公开运势转移证据并重写晋级规则。".to_string();
    draft.fiction_protagonist_arc =
        "从只想保住旁听名额，成长为愿意承担代价的规则改写者。".to_string();
    draft.fiction_world_imagery = "旧城区夜校、灵能准考证、会发光的地下轨道。".to_string();
    draft.fiction_main_causal_spine =
        "补考异常引出运势黑幕，主角追查地下灵轨证据，终局公开证据改写规则。".to_string();
    draft.fiction_themes = vec!["公平晋级".to_string(), "代价与选择".to_string()];
    draft.fiction_world_rules = vec![
        "借灵证能临时借用灵脉但会抽取考生运势。".to_string(),
        "地下灵轨只能由承担代价的人接通。".to_string(),
    ];
    draft.fiction_style_rules = vec!["具体场景推进。".to_string()];
    draft.fiction_must_avoid = vec!["不要角色无解释改名。".to_string()];
    draft.fiction_outline =
            "第一卷《夜校借灵》：主角拿到借灵证并发现考场异常；卷尾变化：主角确认考试吞噬运势。\n第1章 本章目标：许闻桥被迫参加夜校补考；预期转折：许闻桥意识到童年运势被交易。".to_string();
    fill_complete_fiction_contract_v2(&mut draft);
    let before_characters = draft.fiction_characters.clone();

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"夜校借灵证","candidates":["夜校借灵证","旧证入场"],"rationale":"书名来自夜校补考入口、借灵证这个关键物件，以及终局公开账册改写晋级规则的关键爽点。"}}"#,
    );

    assert!(!outcome.committed, "{:?}", outcome.gate.actionable_issues());
    assert_eq!(draft.title, "旧题名");
    assert!(draft.current_contract.is_some());
    assert!(draft.pending_contract_candidate.is_some());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.title.canonical_title, "夜校借灵证");
    assert_eq!(draft.fiction_characters, before_characters);
}

#[test]
fn title_patch_prefers_reader_facing_candidate_over_bad_canonical() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-candidate",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_premise = "地铁站灵能袭击暴露血瞳符文，底层青年被迫追查夜枭集团。".to_string();
    draft.fiction_ending_direction = "主角公开夜枭集团的瞳术账册并改写城市力量秩序。".to_string();
    draft.fiction_protagonist_arc = "从被压迫的底层青年到掌握瞳术法则的变革者。".to_string();
    draft.fiction_world_imagery = "霓虹地铁、血瞳符文、地下灵能账册。".to_string();
    draft.fiction_main_causal_spine =
        "地铁袭击引出血瞳账册，追查夜枭垄断，终局公开账册重写秩序。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻桥; role: 主角; desire: 公开血瞳账册; fear: 再次被夜枭集团抹除证据; bottom_line: 不牺牲无辜乘客换取力量; arc_start: 被压迫的底层青年; arc_end: 掌握瞳术法则的变革者".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 垄断地下灵能账册; fear: 瞳术账册公开; bottom_line: 不允许账册进入公众视野; arc_start: 夜枭集团操盘者; arc_end: 被公开证据逼到台前".to_string(),
        ];
    draft.fiction_world_rules = vec![
        "血瞳符文只能读取被账册记录过的灵能债务。".to_string(),
        "地下灵能账册公开后会重算城市力量归属。".to_string(),
    ];
    draft.fiction_outline =
        "地铁袭击引出血瞳符文，主角追查夜枭集团的灵能账册，终局公开账册重写城市力量秩序。"
            .to_string();
    fill_complete_fiction_contract_v2(&mut draft);

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"重塑主角觉醒终极瞳术","candidates":["夺血瞳账册","借血瞳翻盘","重瞳纪元"],"rationale":"夺血瞳账册来自地铁袭击后的关键物件和终局公开账册改写城市力量秩序的爽点。"}}"#,
    );

    assert!(!outcome.is_ready());
    assert!(draft.title.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.title.canonical_title, "夺血瞳账册");
}

#[test]
fn title_patch_with_weak_canonical_can_select_stronger_declared_candidate() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-local-rationale",
        "fiction",
        "写异界言情小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_premise =
        "镜渊会吞掉被替嫁者的真实姓名，主角被迫以契约婚约进入王庭。".to_string();
    draft.fiction_ending_direction =
        "主角在终局归还镜渊吞掉的姓名，让婚约从束缚变成共同选择。".to_string();
    draft.fiction_protagonist_arc =
        "从被命运替换姓名的旁观者，成长为能主动选择爱与身份的人。".to_string();
    draft.fiction_world_imagery = "镜渊、失名婚契、王庭花阶。".to_string();
    draft.fiction_main_causal_spine =
        "替嫁入局引出失名婚契，追查镜渊吞名规则，终局归还姓名并重写婚约。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻桥; role: 主角; desire: 找回被镜渊吞掉的姓名; fear: 被婚契彻底替换身份; bottom_line: 不把他人的姓名当作交换筹码; arc_start: 被命运替换姓名的旁观者; arc_end: 主动选择爱与身份的人".to_string(),
            "name: 商砚衡; role: 关键关系对象; desire: 摆脱王庭婚约束缚; fear: 再次失去选择权; bottom_line: 不用主角的失名换取自由; arc_start: 冷眼旁观的王庭继承者; arc_end: 共同重写婚约的人".to_string(),
        ];
    draft.fiction_world_rules = vec![
        "镜渊会吞掉被替嫁者的真实姓名。".to_string(),
        "失名婚契只有双方主动归还姓名时才会解除。".to_string(),
    ];
    draft.fiction_outline =
        "替嫁入局引出失名婚契，主角追查镜渊吞名规则，终局归还姓名并重写婚约。".to_string();
    fill_complete_fiction_contract_v2(&mut draft);

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"双生镜渊","candidates":["双生镜渊","失名婚契","镜渊花阶"],"rationale":"体现故事气质和成长。"}}"#,
    );

    assert!(!outcome.is_ready());
    assert!(draft.title.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.title.canonical_title, "失名婚契");
}

#[test]
fn title_patch_falls_back_to_contract_evidence_candidates() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-local-candidates",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_premise = "地铁袭击暴露血瞳符文，底层青年被迫追查夜枭集团。".to_string();
    draft.fiction_ending_direction = "主角公开夜枭集团的血瞳账册并改写城市力量秩序。".to_string();
    draft.fiction_protagonist_arc = "从被压迫的底层青年到掌握瞳术法则的变革者。".to_string();
    draft.fiction_world_imagery = "霓虹地铁、血瞳符文、地下灵能账册。".to_string();
    draft.fiction_main_causal_spine =
        "地铁袭击引出血瞳账册，追查夜枭垄断，终局公开账册重写秩序。".to_string();

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"社交网络","candidates":["社交网络"],"rationale":"书名来自终局公开血瞳账册并改写城市力量秩序的爽点。"}}"#,
    );

    assert!(!outcome.is_ready());
    assert!(
        draft.title.trim().is_empty(),
        "invalid provided title must not be replaced with a locally invented title: {}",
        draft.title
    );
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("书名") || issue.contains("标题")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn skeleton_patch_preserves_user_specified_units() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-skeleton-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"skeleton_patch","genre":"科幻","target_units":100000,"chapter_unit_target":5000,"premise":"城市考试吞噬运势。","ending":{"desired_resolution":"主角公开证据并改写规则。","final_state":"旧城区学生获得公平入口。"},"protagonist_arc":"从旁听生到规则改写者。","world_imagery":"旧城区夜校、灵能准考证、地下灵轨。","main_causal_spine":"补考异常引出黑幕，追查证据，终局改写规则。"}"#,
    );

    assert!(!outcome.is_ready());
    assert_eq!(draft.genre, "都市玄幻");
    assert_eq!(draft.target_units, Some(50_000));
    assert_eq!(draft.chapter_unit_target, Some(2_500));
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending.premise.contains("城市考试吞噬运势"));
    assert!(pending.protagonist_arc.contains("从旁听生到规则改写者"));
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .all(|issue| !issue.contains("覆盖用户已指定")));
}

#[test]
fn metadata_patch_does_not_replace_title_with_invalid_role_field_fragment() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-title-boundary",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "夜校借灵证".to_string();
    draft.fiction_title_rationale =
        "书名来自夜校补考、借灵证和终局公开账册改写晋级规则。".to_string();
    draft.genre = "都市爽文".to_string();
    draft.brief = "旧城区底层青年借一张失效准考证进入灵能考试黑幕。".to_string();
    draft.fiction_premise = "旧城区夜校用灵能考试筛掉底层学生。".to_string();
    draft.fiction_ending_direction = "主角公开账册并改写夜校晋级规则。".to_string();
    draft.fiction_protagonist_arc = "从旁听生到规则改写者。".to_string();
    draft.fiction_world_imagery = "旧城区夜校、灵能准考证、地下灵轨。".to_string();
    draft.fiction_main_causal_spine = "补考异常引出黑幕，追查证据，终局改写规则。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻桥; role: 主角; desire: 通过夜校补考; fear: 再次失去资格; bottom_line: 不牺牲同学; arc_start: 旁听生; arc_end: 规则改写者; name_source: contract".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护灵能考试垄断; fear: 黑幕公开; bottom_line: 不亲手毁掉考试系统; arc_start: 监考者; arc_end: 被证据逼到台前; name_source: contract".to_string(),
            "name: 林知晚; role: 关键关系; desire: 查清旧账; fear: 家族牵连; bottom_line: 不伪造证据; arc_start: 协助者; arc_end: 共同见证者; name_source: contract".to_string(),
        ];
    draft.fiction_themes = vec!["公平晋级".to_string()];
    draft.fiction_world_rules = vec![
        "借灵证能临时借用灵脉但会抽取考生运势。".to_string(),
        "地下灵轨只能由承担代价的人接通。".to_string(),
    ];
    draft.fiction_style_rules = vec!["具体场景推进。".to_string()];
    draft.fiction_must_avoid = vec!["不要角色无解释改名。".to_string()];
    fill_complete_fiction_contract_v2(&mut draft);

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "metadata_patch": {
                "title": {
                  "canonical_title": "证明女性",
                  "candidates": ["证明女性"],
                  "rationale": "证明女性商业才能。"
                },
                "outline": {
                  "volumes": [
                    {"title":"夜校借灵","objective":"确认补考黑幕","ending_change":"主角拿到第一份账册证据"}
                  ],
                  "near_chapters": [
                    {"number":1,"title":"旧证入场","goal":"主角拿到失效准考证","expected_turn":"确认补考资格被暗中买卖"}
                  ]
                }
              }
            }"#,
    );

    assert_ne!(draft.title, "证明女性");
    assert_eq!(draft.title, "夜校借灵证");
    let view = full_contract_view(&draft);
    assert!(
        view.contains("夜校借灵")
            || outcome
                .gate
                .actionable_issues()
                .iter()
                .any(|issue| issue.contains("metadata")),
        "{view}"
    );
}

#[test]
fn batch_patch_commits_valid_skeleton_before_invalid_character_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-batch-partial",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "skeleton_patch": {
                "title": {"canonical_title":"夜校借灵证","rationale":"来自夜校补考、借灵证和终局改写晋级规则。"},
                "premise":"旧城区夜校用灵能考试筛掉底层学生。",
                "ending":{"desired_resolution":"主角公开账册并改写夜校晋级规则。","final_state":"旧城区学生获得公平入口。"},
                "protagonist_arc":"从旁听生到规则改写者。",
                "world_imagery":"旧城区夜校、灵能准考证、地下灵轨。",
                "main_causal_spine":"补考异常引出黑幕，追查证据，终局改写规则。"
              },
              "character_patch": {
                "characters": [
                  {"canonical_name":"许闻桥","role":"主角","desire":"通过夜校补考","fear":"再次失去资格","bottom_line":"不牺牲同学","arc_start":"旁听生","arc_end":"规则改写者"}
                ]
              }
            }"#,
    );

    assert!(!outcome.is_ready());
    assert!(!outcome.committed);
    assert!(draft.fiction_premise.is_empty());
    assert!(draft.fiction_world_imagery.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.premise, "旧城区夜校用灵能考试筛掉底层学生");
    assert_eq!(pending.world_imagery, "旧城区夜校、灵能准考证、地下灵轨");
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("非主角关键角色")));
}

#[test]
fn incomplete_full_contract_keeps_story_fields_as_pending_repair_anchor() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-full-contract-partial",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "title": {"canonical_title":"灵网借考人","rationale":"来自主角借用灵网补考资格、揭开旧城区命脉账册并在终局改写考试规则。"},
              "language":"zh-CN",
              "genre":"都市玄幻",
              "brief":"旧城区旁听生借一张失效准考证闯入灵网考试。",
              "target_units":50000,
              "chapter_unit_target":2500,
              "premise":"旧城区的升学资格被灵网账册暗中买卖，底层学生只能借失效准考证参加补考。",
              "ending":{"desired_resolution":"主角公开账册，让旧城区补考资格重新按真实成绩分配。","final_state":"夜校被迫重启规则，主角从旁听生变成新规则见证者。"},
              "protagonist_arc":"从只想保住自己资格的旁听生，成长为敢公开账册的规则改写者。",
              "world_imagery":"雨夜旧校、灵能准考证、地下灵轨、会发光的灵网账册。",
              "main_causal_spine":"失效准考证引出补考黑账，主角追查账册，终局公开证据并改写升学规则。"
            }"#,
    );

    assert!(!outcome.is_ready());
    assert!(!outcome.committed);
    assert!(draft.fiction_premise.is_empty());
    assert!(draft.fiction_ending_direction.is_empty());
    let pending = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|value| value.get("normalized"))
        .expect("pending normalized contract");
    assert_eq!(
        pending
            .get("premise")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
        "旧城区的升学资格被灵网账册暗中买卖，底层学生只能借失效准考证参加补考。"
    );
    assert!(pending
        .get("world_imagery")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .contains("灵网账册"));
}

#[test]
fn character_patch_repairs_missing_anchors_without_changing_authority_names() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-anchor-repair",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.fiction_characters = vec![
            "name: 钟望宁; role: 主角; desire: 拿回被抢走的晋升资格; fear: 再次被旧规则吞没; bottom_line: ; arc_start: 被边缘化的新人; arc_end: 公开规则的破局者".to_string(),
            "name: 宋晴声; role: 关键同伴; desire: 保住审计证据; fear: 家人被派系牵连; bottom_line: ; arc_start: 谨慎旁观者; arc_end: 主动站到台前".to_string(),
        ];
    let skeleton = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"skeleton_patch",
              "title":{"canonical_title":"暗账翻盘局","rationale":"暗账是主角夺回晋升资格的证据，翻盘局指向终局公开规则的核心爽点。"},
              "language":"zh-CN",
              "genre":"都市爽文",
              "brief":"被边缘化的职场新人追查晋升暗账。",
              "target_units":50000,
              "chapter_unit_target":2500,
              "premise":"晋升资格被派系暗中交易，主角拿到审计证据后被迫破局。",
              "ending":{"desired_resolution":"主角公开晋升暗账，夺回资格并迫使公司重建透明规则。"},
              "protagonist_arc":"从被边缘化的新人，成长为敢公开规则的破局者。",
              "world_imagery":"深夜写字楼、晋升名单、审计暗账。",
              "main_causal_spine":"资格被夺引出审计暗账，追查派系交易，终局公开证据改写晋升规则。"
            }"#,
    );
    assert!(!skeleton.is_ready());
    assert!(draft.pending_contract_candidate.is_some());
    let authority = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let primary_name = authority
        .characters
        .iter()
        .find(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.clone())
        .expect("pending primary authority");
    let companion_name = authority
        .characters
        .iter()
        .find(|character| !character.role_looks_primary())
        .map(|character| character.canonical_name.clone())
        .expect("pending companion authority");
    assert_ne!(
        primary_name, companion_name,
        "the initial authority must assign a distinct name to each role slot"
    );
    let character_patch = serde_json::json!({
        "patch_type": "character_patch",
        "characters": [
            {"canonical_name": primary_name.clone(), "role":"主角", "desire":"拿回被抢走的晋升资格", "fear":"再次被旧规则吞没", "bottom_line":"不牺牲同事换取晋升", "arc_start":"被边缘化的新人", "arc_end":"公开规则的破局者"},
            {"canonical_name": companion_name.clone(), "role":"关键同伴", "desire":"保住审计证据", "fear":"家人被派系牵连", "bottom_line":"不伪造证据", "arc_start":"谨慎旁观者", "arc_end":"主动站到台前"}
        ]
    });

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        &character_patch.to_string(),
    );

    assert!(!outcome.is_ready());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let names = pending
        .characters
        .iter()
        .map(|character| character.canonical_name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![primary_name, companion_name], "{names:?}");
    assert!(
        pending
            .characters
            .iter()
            .any(|character| { character.bottom_line.contains("不牺牲同事") }),
        "characters={:?}; outcome_issues={:?}; pending={:?}",
        pending.characters,
        outcome.gate.actionable_issues(),
        draft.pending_contract_candidate
    );
    assert!(
        pending
            .characters
            .iter()
            .any(|character| { character.bottom_line.contains("不伪造证据") }),
        "{:?}",
        pending.characters
    );
}

#[test]
fn targeted_character_patch_replaces_truncated_anchor_in_pending_contract() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-pending-character-fear-repair",
        "fiction",
        "写现实主义海岛悬疑小说，每章2500字，一共10万字。",
    )
    .expect("draft");
    draft.pending_contract_candidate = Some(serde_json::json!({
        "normalized": {
            "title": {
                "canonical_title": "雾中报码",
                "rationale": "书名来自浓雾夜出现的异常报码及终局公开原始记录的行动。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "现实主义海岛悬疑",
            "brief": "无线电检修员追查灯塔废弃频段的异常报码。",
            "target_units": 100000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "无线电检修员在灯塔撤编前追查异常报码与旧沉船记录的矛盾。",
            "ending": {
                "desired_resolution": "主角公开原始记录，修正岛民对旧事故的记忆。",
                "final_state": "灯塔完成交接，事故记录改为可公开核验。",
                "must_resolve": []
            },
            "protagonist_arc": "从只信任设备数据成长为能同时理解证据与人情的检修员。",
            "world_imagery": "浓雾、灯塔光束、老式电报机与锈蚀机柜。",
            "main_causal_spine": "异常报码引出记录偏差，旧值班习惯暴露人为发送，原始记录最终被公开。",
            "characters": [
                {
                    "canonical_name": "商听声",
                    "role": "主角",
                    "desire": "查明异常报码来源",
                    "fear": "盲目信任技术数据导致判断失误",
                    "bottom_line": "绝不篡改原始电报记录",
                    "arc_start": "只相信设备数据",
                    "arc_end": "理解证据背后的人情选择",
                    "name_source": "generated_by_writing_tool_policy"
                },
                {
                    "canonical_name": "闻泊川",
                    "role": "导师",
                    "desire": "守住旧事故的秘密",
                    "fear": "新自动化设备取代人工后",
                    "bottom_line": "绝不修改原始记录板",
                    "arc_start": "沉默隐瞒旧事",
                    "arc_end": "坦然承认当年选择",
                    "name_source": "generated_by_writing_tool_policy"
                },
                {
                    "canonical_name": "温启遥",
                    "role": "对手",
                    "desire": "维持岛民对救援英雄的记忆",
                    "fear": "原始记录揭露旧事故责任",
                    "bottom_line": "在交接前不允许公开英雄履历的污点",
                    "arc_start": "维持体面叙事",
                    "arc_end": "面对公开证据",
                    "name_source": "generated_by_writing_tool_policy"
                }
            ],
            "themes": [],
            "world_rules": [],
            "style_rules": ["现实主义叙事"],
            "must_avoid": [],
            "outline": {
                "volumes": [{
                    "title": "雾锁孤岛",
                    "objective": "确认异常报码不是设备故障",
                    "ending_change": "主角确认信号由人工发送"
                }],
                "near_chapters": [
                    {
                        "number": 1,
                        "goal": "主角抵达灯塔并完成报到",
                        "expected_turn": "她对老守塔员的疏离产生不安"
                    },
                    {
                        "number": 2,
                        "goal": "主角检查无线电设备",
                        "expected_turn": "她记录到废弃频段的微弱信号"
                    },
                    {
                        "number": 3,
                        "goal": "主角夜间捕获清晰报码",
                        "expected_turn": "报码数据与旧沉船记录不一致"
                    }
                ],
                "raw_outline": "主角从设备排查进入旧事故调查，并在灯塔交接前公开原始记录。"
            }
        },
        "issues": [
            "ContractBlocker: 小说合同缺少世界规则",
            "ContractBlocker: 小说合同缺少必须避免",
            "ContractBlocker: 小说合同缺少核心主题",
            "ContractBlocker: 角色 `闻泊川`（导师）的恐惧锚点像全书主线、截断残句或流程说明，必须改成短的角色级锚点"
        ]
    }));
    let mut visible_contract =
        pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    super::super::apply_strong_novel_contract_to_creation_draft(&mut draft, &mut visible_contract);
    let mentor_name = visible_contract
        .characters
        .iter()
        .find(|character| character.role == "导师")
        .map(|character| character.canonical_name.clone())
        .expect("governed mentor name");
    draft.pending_contract_candidate = Some(serde_json::json!({
        "normalized": visible_contract,
        "issues": [
            "ContractBlocker: 小说合同缺少世界规则",
            "ContractBlocker: 小说合同缺少必须避免",
            "ContractBlocker: 小说合同缺少核心主题",
            format!(
                "ContractBlocker: 角色 `{mentor_name}`（导师）的恐惧锚点像全书主线、截断残句或流程说明，必须改成短的角色级锚点"
            )
        ]
    }));

    let raw_patch = serde_json::json!({
        "patch_type": "character_patch",
        "characters": [{
            "canonical_name": mentor_name,
            "fear": "亲手调试的新设备会让自己失去守塔职责"
        }]
    })
    .to_string();
    let mut effective = super::super::creation_draft_with_pending_contract_applied(&draft);
    let patch = super::super::normalize_creation_contract_patch_boundary(&effective, &raw_patch)
        .expect("character patch");
    patch.apply_to_draft(&mut effective);
    let repaired_visible =
        super::super::strong_novel_contract_from_visible_creation_draft(&effective);
    let visible_mentor = repaired_visible
        .characters
        .iter()
        .find(|character| character.role == "导师")
        .unwrap_or_else(|| {
            panic!(
                "visible mentor missing; characters={:?}; lines={:?}",
                repaired_visible.characters, effective.fiction_characters
            )
        });
    assert_eq!(visible_mentor.fear, "亲手调试的新设备会让自己失去守塔职责");

    let outcome =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, &raw_patch);

    assert!(!outcome.is_ready());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let mentor = pending
        .characters
        .iter()
        .find(|character| character.role == "导师")
        .expect("mentor");
    assert_eq!(mentor.fear, "亲手调试的新设备会让自己失去守塔职责");
    assert!(
        !outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("导师") && issue.contains("恐惧锚点")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn targeted_bottom_line_patch_updates_pending_authority_without_visible_character_copy() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-pending-character-bottom-line-repair",
        "fiction",
        "写玄幻小说，每章2500字，一共10万字。",
    )
    .expect("draft");
    let contract_text = r#"{
              "title":{"canonical_title":"凡骨登天录","rationale":"书名来自主角以凡骨点燃天阶的终局行动。","source":"llm_contract"},
              "language":"zh-CN","genre":"玄幻","brief":"凡骨弃子挑战天阶旧秩序。",
              "target_units":100000,"chapter_unit_target":2500,"max_chapters_per_turn":1,
              "premise":"韩谨野在天阶垄断的世界发现凡骨也能承载神火。",
              "ending":{"desired_resolution":"韩谨野点燃天阶并终结血脉垄断。","final_state":"凡人获得公开登阶资格。"},
              "protagonist_arc":"韩谨野从只求苟活的弃子成长为承担新秩序代价的守护者。",
              "world_imagery":"断裂天阶、凡骨神火与悬空仙城。",
              "main_causal_spine":"神火异变引出天阶秘密，追杀迫使韩谨野集结凡人，终局改写登阶规则。",
              "characters":[
                {"canonical_name":"韩谨野","role":"主角","desire":"以凡骨点燃天阶","fear":"因妥协而失去专业底线","bottom_line":"","arc_start":"卑微苟活的弃子","arc_end":"承担新秩序代价的守护者","name_source":"generated_by_writing_tool_policy"},
                {"canonical_name":"钟云安","role":"同伴","desire":"守护韩谨野","fear":"韩谨野燃尽后世界重回黑暗","bottom_line":"无论韩谨野变成何种形态","arc_start":"冷漠旁观的守城者","arc_end":"新秩序的维护者","name_source":"generated_by_writing_tool_policy"},
                {"canonical_name":"顾维遥","role":"对手","desire":"维持旧天阶秩序","fear":"凡人崛起导致阶层崩塌","bottom_line":"绝不允许无灵根者染指核心天阶","arc_start":"天阶守护者","arc_end":"失去旧秩序权威","name_source":"generated_by_writing_tool_policy"}
              ],
              "themes":["力量资格不应由血脉垄断。"],
              "world_rules":["凡骨每承载一次神火都会永久失去一段寿命。"],
              "style_rules":["用行动和代价表现力量成长。"],
              "must_avoid":["不得用隐藏血脉绕过凡骨代价。"],
              "outline":{"raw_outline":"韩谨野从神火异变追到天阶核心，并在终局改写登阶规则。","volumes":[
                {"title":"凡火初燃","objective":"韩谨野查明神火来源","ending_change":"顾维遥封锁凡城"},
                {"title":"天阶重铸","objective":"韩谨野点燃天阶并终结血脉垄断","ending_change":"凡人获得公开登阶资格"}
              ],"near_chapters":[
                {"number":1,"goal":"韩谨野发现凡骨承载神火","expected_turn":"顾维遥派人封锁矿场"},
                {"number":2,"goal":"韩谨野保存神火证据","expected_turn":"钟云安决定协助韩谨野"},
                {"number":3,"goal":"韩谨野逃离矿场","expected_turn":"两人取得前往凡城的路线"}
              ]}
            }"#;
    let initial =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, contract_text);
    assert!(!initial.is_ready());
    assert!(draft.fiction_characters.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let companion_name = pending
        .characters
        .iter()
        .find(|character| character.role == "同伴")
        .map(|character| character.canonical_name.clone())
        .expect("pending companion authority");
    let protagonist_name = pending
        .characters
        .iter()
        .find(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.clone())
        .expect("pending protagonist authority");
    let improved_bottom_line = format!("无论{protagonist_name}变成何种形态，必守其身后一步");

    let raw_patch = serde_json::json!({
        "patch_type": "character_patch",
        "characters": [{
            "canonical_name": companion_name,
            "bottom_line": improved_bottom_line
        }]
    })
    .to_string();
    let mut effective = super::super::creation_draft_with_pending_contract_applied(&draft);
    let patch = super::super::normalize_creation_contract_patch_boundary(&effective, &raw_patch)
        .expect("character patch");
    patch.apply_to_draft(&mut effective);
    let effective_contract =
        super::super::strong_novel_contract_from_visible_creation_draft(&effective);
    let effective_companion = effective_contract
        .characters
        .iter()
        .find(|character| character.canonical_name == companion_name)
        .expect("effective companion authority");
    assert_eq!(
        effective_companion.bottom_line,
        improved_bottom_line
    );
    let _ = super::super::submit_generated_contract_candidate_to_draft(&mut draft, &raw_patch);

    let repaired = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let companion = repaired
        .characters
        .iter()
        .find(|character| character.canonical_name == companion_name)
        .expect("companion authority");
    assert_eq!(companion.bottom_line, improved_bottom_line);

    let protagonist_bottom_line = "绝不签署与实际证据不符的验收报告";
    let protagonist_patch = serde_json::json!({
        "patch_type": "character_patch",
        "characters": [{
            "canonical_name": protagonist_name,
            "bottom_line": protagonist_bottom_line
        }]
    })
    .to_string();
    let _ =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, &protagonist_patch);
    let repaired = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    let protagonist = repaired
        .characters
        .iter()
        .find(|character| character.role_looks_primary())
        .expect("protagonist authority");
    assert_eq!(protagonist.bottom_line, protagonist_bottom_line);
}

#[test]
fn cjk_compact_character_patch_normalizes_names_and_secondary_primary_roles() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-cjk-character-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"character_patch",
              "characters":[
                {"canonicalName":"秦知安","role":"主角","desire":"守住妹妹的入学资格","fear":"被灵能阶层彻底抹除","bottom_line":"不牺牲无辜学生","arc_start":"旧城区旁听生","arc_end":"公开灵网账册的破局者"},
                {"canonical_name":"岑予晚","role":"女主","desire":"查清家族被夺权真相","fear":"亲手相信错误的人","bottom_line":"不把普通人当筹码","arc_start":"冷静的监察生","arc_end":"选择和主角共同改写规则"},
                {"canonical_name":"觉醒传承","role":"角色","desire":"觉醒","fear":"失败","bottom_line":"无","arc_start":"能力","arc_end":"能力"}
              ],
              "relationshipLedger":[{"characters":["秦知安","岑予晚"],"relationship_type":"互相试探到共同破局","start_state":"互不信任","desired_end_state":"并肩改写规则","conflicts":["阶层身份不同"]}]
            }"#,
    );

    assert!(!outcome.is_ready());
    assert!(!outcome.committed);
    assert!(draft.fiction_characters.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending
        .characters
        .iter()
        .any(|character| character.role.contains("主角")));
    assert!(!pending
        .characters
        .iter()
        .any(|character| character.canonical_name.contains("觉醒传承")));
    assert!(pending
        .structured
        .relationship_ledger
        .iter()
        .all(|entry| entry.characters.iter().all(|name| pending
            .characters
            .iter()
            .any(|character| character.canonical_name.as_str() == name.as_str()))));
}

#[test]
fn skeleton_patch_accepts_common_world_imagery_misspelling() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-cjk-world-imagery",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"skeleton_patch","premise":"旧城区学生误入灵网补考。","ending_direction":"主角公开灵网黑账并让补考制度重启。","protagonist_arc":"从底层旁听生到规则改写者。","world_imaginery":"雨夜旧校、灵能准考证、地下灵轨。","main_causal_spine":"补考异常引出黑账，追查证据，终局重启规则。"}"#,
    );

    assert!(!outcome.is_ready());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.world_imagery, "雨夜旧校、灵能准考证、地下灵轨");
}

#[test]
fn skeleton_patch_title_uses_new_story_evidence_before_replacing_genre_placeholder() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-cjk-title-order",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "都市玄幻".to_string();
    assert_eq!(draft.title, "都市玄幻");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patchtype":"skeletonpatch",
              "title":{"canonicaltitle":"夜校借灵证","candidates":["夜校借灵证","旧证入场"],"rationale":"书名来自夜校补考入口、借灵证这个关键物件，以及终局公开账册改写晋级规则的爽点。"},
              "premise":"旧城区夜校用借灵证筛掉底层学生。",
              "ending":{"desiredresolution":"主角公开灵籍账册并改写夜校晋级规则。","finalstate":"旧城区学生获得公平入口。"},
              "protagonistarc":"从旁听生到规则改写者。",
              "worldimagery":"旧城区夜校、灵能准考证、地下灵轨。",
              "maincausalspine":"补考异常引出黑账，追查证据，终局重启规则。"
            }"#,
    );

    assert!(!outcome.is_ready());
    assert!(draft.title.is_empty() || draft.title == "都市玄幻");
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert_eq!(pending.title.canonical_title, "夜校借灵证");
    assert!(pending.title.rationale.contains("借灵证这个关键物件"));
}

#[test]
fn skeleton_patch_does_not_commit_placeholder_story_fields() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-cjk-placeholder-skeleton",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patchtype":"skeletonpatch",
              "title":{"canonicaltitle":"未定","candidates":["灵压镇魂","都市劫变"],"rationale":"书名将围绕终局的秩序重建、主线的因果冲突、世界观中的法则意象或核心爽点进行演化。"},
              "genre":"都市玄幻",
              "brief":"在现代都市秩序下，隐藏的超凡力量通过资源争夺与阶层博弈，重塑社会法则。",
              "targetunits":50000,
              "chapterunittarget":2500,
              "maxchaptersperturn":1,
              "premise":"待补",
              "ending":{"desiredresolution":"待补","finalstate":"待补"},
              "protagonistarc":"待补",
              "worldimagery":"待补",
              "maincausalspine":"待补"
            }"#,
    );

    assert!(!outcome.is_ready());
    assert_ne!(draft.fiction_premise, "待补");
    assert_ne!(draft.fiction_ending_direction, "待补");
    assert_ne!(draft.fiction_protagonist_arc, "待补");
    assert_ne!(draft.fiction_world_imagery, "待补");
    assert_ne!(draft.fiction_main_causal_spine, "待补");
    let issues = outcome.gate.actionable_issues().join("；");
    assert!(
        issues.contains("故事前提")
            || issues.contains("终局方向")
            || issues.contains("主角弧线")
            || issues.contains("世界观意象")
            || issues.contains("总主线因果链"),
        "{issues}"
    );
}

#[test]
fn cjk_title_field_pack_becomes_pending_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-field-pack",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
            &mut draft,
            "书名：夜校灵轨\n书名理由：来自夜校补考、地下灵轨证据和终局重写晋级规则。\n书名候选：夜校灵轨、旧证入场、借灵补考",
        );

    assert!(!outcome.is_ready());
    assert!(draft.current_contract.is_none());
    let pending = draft
        .pending_contract_candidate
        .as_ref()
        .expect("pending")
        .to_string();
    assert!(pending.contains("夜校灵轨"), "{pending}");
    assert!(draft
        .diagnostics
        .iter()
        .any(|item| item.contains("合同候选未进入可确认草案")));
}

#[test]
fn title_patch_can_infer_canonical_title_from_rationale() {
    assert_eq!(
        super::super::infer_book_title_from_rationale_text(
            "最终书名云阙司命录源自异界司命规则、云阙审判和终局改命。"
        ),
        "云阙司命录"
    );

    let mut draft = super::super::build_initial_creation_draft(
        "session-title-rationale-only",
        "fiction",
        "写异界言情小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"","candidates":[],"rationale":"最终书名云阙司命录源自异界司命规则、云阙审判和终局改命。艰难取舍和爱情线都落在改命代价上。"}}"#,
    );

    assert!(!outcome.is_ready());
    let pending = draft
        .pending_contract_candidate
        .as_ref()
        .expect("pending")
        .to_string();
    assert!(pending.contains("云阙司命录"), "{pending}");
}

#[test]
fn title_patch_does_not_clip_origin_explanation_as_book_title() {
    assert_eq!(
        super::super::infer_book_title_from_rationale_text(
            "书名取自关键灵枢物件与终局断代反转，体现以残缺破圆满的核心主线。"
        ),
        ""
    );

    let mut draft = super::super::build_initial_creation_draft(
        "session-title-origin-explanation",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"title_patch","title":{"canonical_title":"","candidates":[],"rationale":"书名取自关键灵枢物件与终局断代反转，体现以残缺破圆满的核心主线。"}}"#,
    );

    assert!(!outcome.is_ready());
    assert!(!outcome.committed);
}

#[test]
fn character_patch_normalizes_multiple_primary_slots_before_gate() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"character_patch","characters":[{"canonical_name":"许闻桥","role":"主角","desire":"通过考试","fear":"失去资格","bottom_line":"不牺牲同学","arc_start":"旁听生","arc_end":"规则改写者"},{"canonical_name":"梁棠","role":"主角","desire":"守住妹妹","fear":"再次失去选择权","bottom_line":"不伤害无辜","arc_start":"被动卷入","arc_end":"主动承担"}]}"#,
    );

    assert!(!outcome.is_ready());
    assert!(!outcome.committed);
    assert!(draft.fiction_characters.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending
        .characters
        .iter()
        .any(|character| character.role.contains("主角")));
    assert_eq!(
        pending
            .characters
            .iter()
            .filter(|character| character.role.contains("主角"))
            .count(),
        1
    );
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .all(|issue| !issue.contains("恰好 1 个主角槽位")));
}

#[test]
fn character_patch_normalizes_external_anchor_names_before_gate() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-anchor-normalize",
        "fiction",
        "写都市言情小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{
              "patch_type":"character_patch",
              "characters":[
                {"canonical_name":"钟望晚","role":"主角","desire":"在职场与情感中找回自我","fear":"再次被关系和旧伤束缚","bottom_line":"不牺牲人格尊严","arc_start":"被动忍让的新人","arc_end":"能主动选择关系与事业的人"},
                {"canonical_name":"裴桥澜","role":"关键关系对象","desire":"帮南栖安突破自我设限","fear":"失去她时无法挽回","bottom_line":"在原则与情感之间保持平衡","arc_start":"旁观的导师","arc_end":"并肩承担选择的人"},
                {"canonical_name":"晏岑安","role":"关键对手","desire":"维护自己的商业地位","fear":"旧事被公开","bottom_line":"维护表面光鲜","arc_start":"体面的阻力","arc_end":"被迫面对代价的人"}
              ],
              "relationship_ledger":[{"characters":["钟望晚","裴桥澜"],"relationship_type":"从互相试探到共同面对选择","start_state":"保持距离","desired_end_state":"互相信任","conflicts":["职场边界与旧伤"]}]
            }"#,
    );

    let joined_characters = draft.fiction_characters.join("\n");
    let joined_issues = outcome.gate.actionable_issues().join("；");
    assert!(!joined_characters.contains("南栖安"), "{joined_characters}");
    assert!(
        !joined_issues.contains("权威表外角色 `南栖安`"),
        "{joined_issues}"
    );
}

#[test]
fn generic_character_fallback_blocks_contract_confirmation() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-a",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    draft.title = "雨巷灵契".to_string();
    draft.genre = "都市玄幻".to_string();
    draft.language = "zh-CN".to_string();
    draft.brief = "旧城雨巷出现灵能裂缝，普通学生被迫卷入守城试炼。".to_string();
    draft.fiction_premise = draft.brief.clone();
    draft.fiction_ending_direction = "主角守住城市灵能裂缝并回到平凡生活。".to_string();
    draft.fiction_protagonist_arc = "从旁观者成长为守城者。".to_string();
    draft.fiction_world_imagery = "雨巷灵火、旧楼夜校、玻璃天台裂缝。".to_string();
    draft.fiction_main_causal_spine = "城市异常引出夜校试炼，终局由主角重立城市灵契。".to_string();
    draft.fiction_title_rationale = "灵契来自主角终局与城市重新立约的选择。".to_string();
    draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 完成故事合同中的核心目标并改变自身命运; fear: 在力量或秩序面前再次失去选择权; bottom_line: 不违背合同确立的核心价值和人物承诺".to_string(),
            "name: 梁棠; role: 对手; desire: 利用裂缝改写城市秩序; fear: 秩序回归平凡; bottom_line: 动机必须清晰".to_string(),
        ];
    draft.fiction_themes = vec!["选择权比力量更重要".to_string()];
    draft.fiction_world_rules = vec!["灵契只能通过承担真实代价建立".to_string()];
    draft.fiction_style_rules = vec!["用具体场景推进，不写提纲式正文".to_string()];
    draft.fiction_must_avoid = vec!["不要让角色无解释改名".to_string()];
    draft.fiction_outline =
            "第一卷：裂缝觉醒。\n第01章《雨巷灵火》：本章目标：主角发现城市异常。\n第02章《旧楼试炼》：本章目标：主角进入夜校试炼。\n结局：主角守住城市灵能裂缝。"
                .to_string();
    fill_complete_fiction_contract_v2(&mut draft);

    let issues = super::super::creation_draft_contract_blocking_issues(&draft);

    assert!(
        issues.iter().any(|issue| issue.contains("通用兜底动机"))
            || issues.iter().any(|issue| issue.contains("缺少欲望锚点")
                || issue.contains("缺少恐惧锚点")
                || issue.contains("缺少底线锚点")),
        "{issues:?}"
    );
}

#[test]
fn story_grounded_title_is_not_rejected_by_aesthetic_scoring() {
    let draft = super::super::build_initial_creation_draft(
        "session-a",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");
    let contract = "书名《律令重构》\n\
命名理由：律令指代城市强制规则，重构指主角终局打破规则并重新建立自由秩序。\n\
题材：都市玄幻\n\
语言：中文\n\
故事前提：城市由律令维持，普通学生发现雨巷里的灵火异常。\n\
终局方向：主角在旧楼夜校公开证据，打破绝对律令并守住普通人的选择权。\n\
主角弧线：从被规则驱赶的旁观者成长为愿意承担代价的守城者。\n\
世界观意象：雨巷灵火、旧楼夜校、玻璃天台裂缝。\n\
总主线因果链：发现裂缝——进入夜校——付出感官代价——终局重立城市契约。\n\
角色权威表：姓名：许闻，角色：主角，欲望：守住妹妹和自己的普通生活，恐惧：代价夺走记忆，底线：不牺牲无辜者。\n\
姓名：梁棠，角色：对手，欲望：利用裂缝改写城市秩序，恐惧：真相公开，底线：不隐藏核心动机。\n\
核心主题：选择权比力量更重要。\n\
世界规则：灵契只能通过承担真实代价建立。\n\
叙事风格：具体场景推进。\n\
必须避免：角色无解释改名。\n\
近期章节包：第01章《雨巷灵火》：本章目标：主角发现城市异常。\n第02章《旧楼试炼》：本章目标：主角进入夜校试炼。\n第03章《玻璃天台》：本章目标：主角确认灵契代价。";

    let gate = super::super::generated_contract_gate_result(&draft, contract, true);

    assert!(
        !gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("抽象概念") || issue.contains("读者入口")),
        "{:?}",
        gate.actionable_issues()
    );
}

#[test]
fn initial_creation_draft_strips_control_phrases_from_story_fields() {
    let draft = super::super::build_initial_creation_draft(
            "session-control",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起；你来定，自动补齐合同。书名、角色名字、世界观、大纲、结局和前三章章节名都由你根据都市玄幻题材自己生成，补齐后给我确认。",
        )
        .expect("draft");

    assert_eq!(draft.genre, "都市玄幻");
    for value in [&draft.genre, &draft.brief, &draft.fiction_premise] {
        assert!(!value.contains("你来定"), "{value}");
        assert!(!value.contains("自动补齐"), "{value}");
        assert!(!value.contains("给我确认"), "{value}");
        assert!(!value.contains("每章"), "{value}");
        assert!(!value.contains("5万"), "{value}");
    }
}

#[test]
fn initial_creation_draft_preserves_user_story_core_as_stable_authority() {
    let draft = super::super::build_initial_creation_draft(
            "session-story-authority",
            "fiction",
            "写一部现实悬疑小说，每章2500字，总字数10万字。2006年的重庆老城，一家社区火锅店接连遭遇灶台回火、账本失踪和租户提前搬走，店主怀疑有人借燃气安全事故制造整栋楼低价清退，必须查清并阻止产权被吞并。书名和人物姓名由你生成。请先自动生成并修复完整创作合同，合同通过后给我确认，现在不要写正文。",
        )
        .expect("draft");

    assert!(!draft.brief.is_empty(), "{:?}", draft.planning_notes);
    let authority = draft
        .planning_notes
        .iter()
        .find_map(|note| note.strip_prefix("用户故事核心权威："))
        .unwrap_or_else(|| panic!("stable user story authority: {:?}", draft.planning_notes));

    assert!(authority.contains("燃气安全事故"), "{authority}");
    assert!(authority.contains("整栋楼低价清退"), "{authority}");
    assert!(authority.contains("产权被吞并"), "{authority}");
    assert!(!authority.contains("每章2500字"), "{authority}");
    assert!(!authority.contains("书名和人物姓名由你生成"), "{authority}");
    assert!(!authority.contains("先给合同确认"), "{authority}");
    assert!(
        super::super::stable_creation_planning_notes(&draft)
            .iter()
            .all(|note| !note.starts_with("用户故事核心权威：")),
        "internal authority note must not leak to the user surface"
    );
}

#[test]
fn initial_creation_draft_strips_system_generation_workflow_from_story_authority() {
    let draft = super::super::build_initial_creation_draft(
            "session-system-generated-authority",
            "fiction",
            "请新建一部2013年西南山区广播剧团背景的现实职场悬疑长篇，总字数10万字，每章2500字。核心故事：年轻录音师发现承包商调包方言母带并制造设备短路，必须在年度直播前恢复原始母带、公开证据链并保住公共播出时段。书名和人物姓名由系统生成并接受本地治理。请先自动生成并修复完整创作合同，合同通过后给我确认，现在不要写正文。",
        )
        .expect("draft");

    assert!(!draft.brief.starts_with("请新建"), "{}", draft.brief);
    assert!(draft.brief.contains("方言母带"), "{}", draft.brief);
    let authority = draft
        .planning_notes
        .iter()
        .find_map(|note| note.strip_prefix("用户故事核心权威："))
        .unwrap_or_else(|| panic!("stable user story authority: {:?}", draft.planning_notes));
    assert!(authority.contains("方言母带"), "{authority}");
    assert!(!authority.contains("由系统生成"), "{authority}");
    assert!(!authority.contains("本地治理"), "{authority}");
    assert!(draft
        .planning_notes
        .iter()
        .all(|note| !note.contains("由系统生成")));
}

#[test]
fn initial_creation_draft_strips_process_control_from_story_fields() {
    let draft = super::super::build_initial_creation_draft(
        "session-process-control",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。请先给我合同草案和大纲，然后等我确认。",
    )
    .expect("draft");

    assert_eq!(draft.genre, "异界修仙");
    for value in [&draft.brief, &draft.fiction_premise] {
        assert!(!value.contains("合同草案"), "{value}");
        assert!(!value.contains("大纲"), "{value}");
        assert!(!value.contains("然后等我"), "{value}");
        assert!(!value.contains("等我确认"), "{value}");
        assert!(!value.contains("请先给我"), "{value}");
    }
    assert_eq!(draft.brief, "异界修仙");
    assert!(
        draft.audience.is_empty(),
        "contract display request must not be parsed as audience: {}",
        draft.audience
    );
    assert!(
        draft
            .planning_notes
            .iter()
            .all(|note| !note.contains("合同草案") && !note.contains("等我确认")),
        "{:?}",
        draft.planning_notes
    );

    let draft = super::super::build_initial_creation_draft(
        "session-process-control-confirm-after",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。先给我完整合同草案，我确认后再开始写。",
    )
    .expect("draft");
    assert_eq!(draft.genre, "都市玄幻");
    assert_eq!(draft.brief, "都市玄幻");
    for value in [&draft.brief, &draft.fiction_premise] {
        assert!(!value.contains("我后再开始"), "{value}");
        assert!(!value.contains("确认后"), "{value}");
        assert!(!value.contains("合同草案"), "{value}");
    }
}

#[test]
fn creation_draft_sanitizer_strips_contract_template_noise_from_lists() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-list-control",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_themes = vec![
        "草根逆袭与规则代价".to_string(),
        "创作周期1.乙方应在本合同签订后日内开始创作".to_string(),
        "版权与署名1.本作品的著作权归甲方所有".to_string(),
    ];
    draft.fiction_world_rules = vec![
        "商业资源必须通过证据链、人脉交换或代价获取。".to_string(),
        "乙方应按照甲方的建议和意见进行修改".to_string(),
    ];

    super::super::sanitize_creation_draft_control_noise(&mut draft);

    assert_eq!(draft.fiction_themes, vec!["草根逆袭与规则代价"]);
    assert_eq!(
        draft.fiction_world_rules,
        vec!["商业资源必须通过证据链、人脉交换或代价获取。"]
    );
}

#[test]
fn incomplete_contract_candidate_stays_pending_without_polluting_current_contract() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-pending",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"title":{"canonical_title":"夜校灵轨","rationale":"夜校来自起点，灵轨来自终局。"},"genre":"都市玄幻"}"#,
    );

    assert!(!outcome.is_ready());
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(draft.title.is_empty());
    assert!(draft.fiction_characters.is_empty());
    assert!(draft.fiction_ending_direction.is_empty());
    assert!(draft
        .diagnostics
        .iter()
        .any(|item| item.contains("合同候选未进入可确认草案")));
}

#[test]
fn skeleton_patch_commits_stable_anchor_fields_before_character_stage() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-skeleton-anchor",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");

    let outcome = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        r#"{"patch_type":"skeleton_patch","genre":"都市玄幻","premise":"现代都市中隐藏的上古玄门势力，通过争夺城市能量命脉决定世界格局。","ending":{"desired_resolution":"主角摧毁旧秩序，建立新的能量分配体系。","final_state":"底层觉醒者获得公开修行入口。"},"protagonist_arc":"从底层觉醒者成长为规则制定者。","world_imagery":"摩天大楼是能量节点，地铁系统是灵脉通道，霓虹灯是结界屏障。","main_causal_spine":"主角发现都市能量失衡真相，遭遇势力阻挠，追查灵脉证据，终局重建秩序。"}"#,
    );

    assert!(!outcome.is_ready());
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(draft.fiction_premise.is_empty());
    let pending = pending_normalized_contract(draft.pending_contract_candidate.as_ref());
    assert!(pending.premise.contains("上古玄门势力"));
    assert!(pending
        .ending
        .desired_resolution
        .contains("新的能量分配体系"));
    assert_eq!(pending.ending.final_state, "底层觉醒者获得公开修行入口");
    assert!(pending.world_imagery.contains("地铁系统是灵脉通道"));
    let prompt = super::super::final_prompt_from_contract_quality_repair(
        &draft,
        "继续补齐合同",
        &outcome.gate.actionable_issues(),
    );
    assert!(
        prompt.contains("Characters typed patch") || prompt.contains("Governance typed patch"),
        "{prompt}"
    );
    assert!(prompt.contains("上古玄门势力"), "{prompt}");
    if prompt.contains("Characters typed patch") {
        assert!(prompt.contains("role 只能填写一个"), "{prompt}");
        assert!(prompt.contains("\"role\":\"关键同伴\""), "{prompt}");
        assert!(
            !prompt.contains("\"role\":\"关系对象/盟友/导师\""),
            "{prompt}"
        );
    }
    assert!(!prompt.contains("arc_start: ;"), "{prompt}");
    assert!(!prompt.contains("arc_end: ；"), "{prompt}");
    assert!(!prompt.contains("arc_end: \n"), "{prompt}");
}

#[test]
fn contract_validation_response_hides_internal_blocker_labels() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-validation-surface",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_premise = "城市夜校里藏着灵脉入学考试。".to_string();
    let report = super::super::ContractValidationReport::for_draft(&draft);
    let response = report.user_response(&draft, "开始写第一章").response;

    assert!(!response.contains("ContractBlocker"), "{response}");
    assert!(response.contains("需要补齐："), "{response}");
}

#[test]
fn contract_repair_keeps_best_pending_candidate_as_anchor() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-best-pending",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let mostly_complete_with_numeric_turns = r#"{
  "title":{"canonical_title":"夜校灵轨","candidates":["夜校灵轨","旧证入场","借灵补考"],"rationale":"夜校来自主角进入旧城区补考的地点，灵轨来自终局公开运势转移轨迹并改写晋级规则的关键证据。","source":"llm_contract"},
  "language":"zh-CN",
  "genre":"都市玄幻",
  "brief":"旧城区旁听生卷入城市灵能考试黑幕。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"灵能考试决定城市阶层，旧城区学生发现补考会吞噬普通人的运势。",
  "ending":{"desired_resolution":"主角公开运势转移证据并重写晋级规则。","final_state":"旧城区学生获得公平考试入口。","must_resolve":["灵能考试黑幕","运势转移证据"],"allowed_open_questions":[]},
  "protagonist_arc":"从只想保住旁听名额，成长为愿意承担代价的规则改写者。",
  "world_imagery":"旧城区夜校、灵能准考证、会发光的地下轨道。",
  "main_causal_spine":"补考异常引出运势黑幕，主角追查地下灵轨证据，终局公开证据改写规则。",
  "characters":[
    {"canonical_name":"许闻桥","role":"主角","desire":"通过灵能考试改变命运","fear":"再次失去考试资格","bottom_line":"不牺牲同学换取晋级","arc_start":"旁听生","arc_end":"规则改写者"},
    {"canonical_name":"商砚衡","role":"关键对手","desire":"维护灵能考试垄断","fear":"黑幕公开","bottom_line":"不亲手毁掉考试系统","arc_start":"监考者","arc_end":"被证据逼到台前"}
  ],
  "themes":["公平晋级","代价与选择"],
  "world_rules":["灵能考试会转移考生运势","旧城区考生必须借灵入场"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "structured":{"narration_contract":{"pov":"第三人称有限视角"}},
  "outline":{
    "volumes":[{"title":"夜校借灵","objective":"主角拿到借灵证并发现考场异常","ending_change":"主角确认考试吞噬运势"}],
    "near_chapters":[
      {"number":1,"goal":"许闻桥被迫参加夜校补考","expected_turn":"1"},
      {"number":2,"goal":"主角找到旧城区考试证据","expected_turn":"2"},
      {"number":3,"goal":"主角第一次反向利用借灵规则","expected_turn":"3"}
    ],
    "raw_outline":"旁听生借灵入场，追查考试吞噬运势，终局公开旧城区证据重写晋级规则。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        mostly_complete_with_numeric_turns,
    );
    assert!(!first.is_ready());
    let first_pending = draft
        .pending_contract_candidate
        .as_ref()
        .expect("pending candidate")
        .to_string();
    assert!(first_pending.contains("夜校灵轨"), "{first_pending}");

    let worse_candidate =
        r#"{"title":{"canonical_title":"旧街计划","rationale":"体现计划。"},"genre":"都市玄幻"}"#;
    let second =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, worse_candidate);
    assert!(!second.is_ready());
    let kept_pending = draft
        .pending_contract_candidate
        .as_ref()
        .expect("kept pending candidate")
        .to_string();
    assert!(kept_pending.contains("夜校灵轨"), "{kept_pending}");
    assert!(!kept_pending.contains("旧街计划"), "{kept_pending}");

    let repair_prompt = super::super::final_prompt_from_contract_quality_repair(
        &draft,
        "请自动修复这份合同草案",
        &second.gate.actionable_issues(),
    );
    assert!(
        !repair_prompt.contains("候选书名：夜校灵轨"),
        "{repair_prompt}"
    );
    assert!(
        !repair_prompt.contains("角色锚点：许闻桥"),
        "{repair_prompt}"
    );
    assert!(
        repair_prompt.contains("角色表必须恰好 1 个主角")
            && repair_prompt.contains("关键对手/反派/压力源"),
        "{repair_prompt}"
    );
    assert!(
        !repair_prompt.contains("角色表只保留一个明确主角"),
        "{repair_prompt}"
    );
    assert!(
        repair_prompt.contains("故事蓝图补齐阶段：Characters")
            && !repair_prompt.contains("近期章节包保留 3 到 8 章"),
        "{repair_prompt}"
    );
    assert!(!repair_prompt.contains("合同分段补齐阶段"));
    assert!(!repair_prompt.contains("用户正在定小说创作合同"));
    assert!(!repair_prompt.contains("合同确认阶段"));
}

#[test]
fn pending_contract_metadata_patch_repairs_numeric_chapter_turns_without_full_rewrite() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let mostly_complete_with_numeric_turns = r#"{
  "title":{"canonical_title":"夜校灵轨","candidates":["夜校灵轨","旧证入场","借灵补考"],"rationale":"夜校来自主角进入旧城区补考的地点，灵轨来自终局公开运势转移轨迹并改写晋级规则的关键证据。","source":"llm_contract"},
  "language":"zh-CN",
  "genre":"都市玄幻",
  "brief":"旧城区旁听生卷入城市灵能考试黑幕。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"灵能考试决定城市阶层，旧城区学生发现补考会吞噬普通人的运势。",
  "ending":{"desired_resolution":"主角公开运势转移证据并重写晋级规则。","final_state":"旧城区学生获得公平考试入口。","must_resolve":["灵能考试黑幕","运势转移证据"],"allowed_open_questions":[]},
  "protagonist_arc":"从只想保住旁听名额，成长为愿意承担代价的规则改写者。",
  "world_imagery":"旧城区夜校、灵能准考证、会发光的地下轨道。",
  "main_causal_spine":"补考异常引出运势黑幕，主角追查地下灵轨证据，终局公开证据改写规则。",
  "characters":[
    {"canonical_name":"许闻桥","role":"主角","desire":"通过灵能考试改变命运","fear":"再次失去考试资格","bottom_line":"不牺牲同学换取晋级","arc_start":"旁听生","arc_end":"规则改写者"},
    {"canonical_name":"商砚衡","role":"关键对手","desire":"维护灵能考试垄断","fear":"黑幕公开","bottom_line":"不亲手毁掉考试系统","arc_start":"监考者","arc_end":"被证据逼到台前"}
  ],
  "themes":["公平晋级","代价与选择"],
  "world_rules":["灵能考试会转移考生运势","旧城区考生必须借灵入场"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "structured":{"narration_contract":{"pov":"第三人称有限视角"}},
  "outline":{
    "volumes":[{"title":"夜校借灵","objective":"主角拿到借灵证并发现考场异常","ending_change":"主角确认考试吞噬运势"}],
    "near_chapters":[
      {"number":1,"goal":"许闻桥被迫参加夜校补考","expected_turn":"1"},
      {"number":2,"goal":"主角找到旧城区考试证据","expected_turn":"2"},
      {"number":3,"goal":"主角第一次反向利用借灵规则","expected_turn":"3"}
    ],
    "raw_outline":"旁听生借灵入场，追查考试吞噬运势，终局公开旧城区证据重写晋级规则。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        mostly_complete_with_numeric_turns,
    );
    assert!(!first.is_ready());
    assert!(
        super::super::creation_contract_issues_are_contract_metadata_only(
            &first.gate.actionable_issues()
        )
    );

    let patch = r#"{
  "outline": {
    "near_chapters": [
      {"number":1,"goal":"许闻桥被迫参加夜校补考","expected_turn":"他发现借灵证会记录运势损耗，并确认补考不是公平机会"},
      {"number":2,"goal":"许闻桥找到旧城区考试证据","expected_turn":"证据指向监考系统，商砚衡第一次锁定他"},
      {"number":3,"goal":"许闻桥第一次反向利用借灵规则","expected_turn":"他救下同学但暴露身份，从旁听生变成被追查者"}
    ]
  }
}"#;
    let repaired = super::super::submit_pending_contract_metadata_repair(&mut draft, patch)
        .expect("metadata repair outcome");

    assert!(
        repaired.is_ready(),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    assert!(draft.current_contract.is_some());
    assert_eq!(draft.title, "夜校灵轨");
    assert!(current_contract_text(&draft).contains("他救下同学但暴露身份"));
}

#[test]
fn mixed_title_and_outline_issues_are_metadata_repairable() {
    let issues = vec![
        "ContractBlocker: 书名缺少读者钩子，虽然可能能解释，但不像会让人想点开的作品名".to_string(),
        "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string(),
        "小说合同尚未形成逐章规划或分卷/阶段大纲".to_string(),
    ];

    assert!(
        super::super::creation_contract_issues_are_contract_metadata_only(&issues),
        "{issues:?}"
    );
}

#[test]
fn metadata_patch_accepts_nested_outline_with_title_and_volumes() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-title-outline",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let contract_without_good_title_or_outline = r#"{
  "title": {
    "canonical_title": "感知静默",
    "rationale": "体现主角成长和世界变化。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"异界修仙",
  "brief":"异界修仙，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"凡人少年在九霄大陆发现天道残卷，被迫卷入宗门与旧天道的争夺。",
  "ending":{"desired_resolution":"主角公开残卷中的修行漏洞，让凡人也能获得入道资格。","final_state":"旧天道垄断被打破，九霄大陆出现新的修行入口。"},
  "protagonist_arc":"从只求自保的凡人少年，到愿意公开残卷代价的开道者。",
  "world_imagery":"九霄云阙、断裂残卷、灵脉天梯、被封锁的凡人城。",
  "main_causal_spine":"残卷现世引出宗门追杀，主角追查天道漏洞，终局公开残卷改写修行入口。",
  "characters":[
    {"canonical_name":"南照野","role":"主角","desire":"为凡人争取入道资格","fear":"残卷代价害死同伴","bottom_line":"不献祭无辜者","arc_start":"凡人自保","arc_end":"公开修行入口"},
    {"canonical_name":"段岚澜","role":"同伴","desire":"守住凡人城","fear":"宗门清算家人","bottom_line":"不背叛凡人城","arc_start":"谨慎相助","arc_end":"并肩公开证据"},
    {"canonical_name":"祁无涯","role":"关键对手","desire":"维持宗门对天门的垄断","fear":"凡人入道动摇宗门秩序","bottom_line":"不允许残卷公开","arc_start":"执法长老","arc_end":"被迫面对旧天道漏洞"}
  ],
  "themes":["凡人开道","代价与公开"],
  "world_rules":["残卷记录修行入口的漏洞，但每次使用都会引来宗门追踪。","天道垄断会封锁凡人城的灵脉入口。","凡人借灵脉修行必须付出记忆、寿元或关系代价。"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要用摘要替代正文"],
  "structured":{"narration_contract":{"pov":"第三人称有限视角"}},
  "outline":{"volumes":[],"near_chapters":[],"raw_outline":""}
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        contract_without_good_title_or_outline,
    );
    assert!(!first.is_ready());
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("书名") || issue.contains("读者钩子")),
        "{:?}",
        first.gate.actionable_issues()
    );
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("分卷") || issue.contains("大纲")),
        "{:?}",
        first.gate.actionable_issues()
    );

    let patch = r#"{
  "title": {
    "canonical_title": "凡骨开天门",
    "candidates": ["凡骨开天门", "残卷照凡城", "九霄凡门令"],
    "rationale": "凡骨来自主角凡人身份，开天门对应终局公开残卷漏洞、让凡人获得入道资格的核心爽点。",
    "source": "llm_contract"
  },
  "outline": {
    "volumes": [
      {"title":"残卷入城","objective":"南照野得到残卷并确认凡人城被封锁的真相","ending_change":"宗门追杀让他无法再躲回凡人身份"},
      {"title":"灵脉借道","objective":"主角找到凡人借灵脉入道的代价和证据","ending_change":"同伴关系被代价撕开，残卷秘密被对手锁定"},
      {"title":"天门公审","objective":"主角把残卷漏洞公开到九霄云阙之前","ending_change":"凡人城获得第一条公开入道路径"}
    ],
    "near_chapters": [
      {"number":1,"goal":"南照野在凡人城夜市发现断裂残卷","expected_turn":"残卷显出被封锁的天门坐标，宗门追兵第一次出现"},
      {"number":2,"goal":"南照野带着段岚澜逃入废灵脉","expected_turn":"他们确认残卷每次开启都会夺走一段记忆"},
      {"number":3,"goal":"祁无涯派人封城搜卷","expected_turn":"南照野选择公开第一条证据，凡人城从旁观变成被卷入者"}
    ]
  }
}"#;
    let repaired = super::super::submit_pending_contract_metadata_repair(&mut draft, patch)
        .expect("metadata repair outcome");

    assert!(
        repaired.is_ready(),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    assert_eq!(draft.title, "凡骨开天门");
    let view = full_contract_view(&draft);
    assert!(view.contains("残卷入城"), "{view}");
    assert!(view.contains("凡人城从旁观变成被卷入者"), "{view}");
}

#[test]
fn metadata_patch_applies_world_rules_with_title_and_outline() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-world-rules",
        "fiction",
        "写都市轻玄幻短篇小说，每章2500字，总字数5000字。",
    )
    .expect("draft");
    let incomplete = r#"{
  "title": {
    "canonical_title": "旧物回收站",
    "rationale": "书名来自主角经营旧货店和回收灵物的设定。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"都市轻玄幻",
  "brief":"都市轻玄幻短篇",
  "target_units":5000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"温栖安经营一家能回收被遗忘之物的旧货店。",
  "ending":{"desired_resolution":"温栖安用旧怀表修复城市遗忘裂痕。","final_state":"旧城居民重新记起被抹掉的名字。"},
  "protagonist_arc":"从逃避过去到主动修补城市裂痕。",
  "world_imagery":"雨夜旧货店、因果线、会哭的旧怀表。",
  "main_causal_spine":"旧怀表异动引出恒宇集团收购，温栖安追查遗忘裂痕，终局修复城市记忆。",
  "characters":[
    {"canonical_name":"温栖安","role":"主角","desire":"找回自己的名字","fear":"被城市彻底遗忘","bottom_line":"不把起源怀表交给恒宇集团","arc_start":"逃避过去","arc_end":"主动修补裂痕"},
    {"canonical_name":"白闻安","role":"同伴","desire":"记录都市传说","fear":"真相无人相信","bottom_line":"不伪造证据","arc_start":"旁观记录者","arc_end":"并肩守住旧城"},
    {"canonical_name":"辛知澜","role":"对手","desire":"垄断遗忘之物","fear":"恒宇集团失去控制","bottom_line":"不允许旧城记忆公开","arc_start":"收购代理人","arc_end":"被迫面对裂痕代价"}
  ],
  "themes":["记忆与代价"],
  "world_rules":[],
  "style_rules":["保持中文场景推进"],
  "must_avoid":["不要角色改名"],
  "structured":{"narration_contract":{"pov":"第三人称有限视角"}},
  "outline":{"volumes":[],"near_chapters":[]}
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(&mut draft, incomplete);
    assert!(!first.is_ready());
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("世界规则")),
        "{:?}",
        first.gate.actionable_issues()
    );

    let patch = r#"{
  "title": {
    "canonical_title": "雨夜旧表借名",
    "candidates": ["雨夜旧表借名", "雨夜借名人", "旧城补名师"],
    "rationale": "雨夜是因果线显现的时刻，旧表是关键物件，借名对应主角修复城市遗忘裂痕并让居民重新取回名字的终局爽点。",
    "source": "llm_contract"
  },
  "world_rules": [
    "被遗忘之物会吞掉持有者的一段真实记忆，回收时必须用等价记忆抵偿。",
    "旧货店只能在雨夜看见因果线，白天错误回收会让裂痕扩大。",
    "恒宇集团能格式化无主旧物，但格式化会永久抹掉关联者的名字。"
  ],
  "outline": {
    "volumes": [
      {"title":"雨夜借名","objective":"温栖安接到会哭的旧怀表并确认遗忘裂痕存在","ending_change":"恒宇集团发现旧货店能修补裂痕"}
    ],
    "near_chapters": [
      {"number":1,"goal":"温栖安在雨夜回收会哭的旧怀表","expected_turn":"怀表叫出一个被城市抹掉的名字"},
      {"number":2,"goal":"恒宇集团代理人上门收购旧怀表","expected_turn":"温栖安拒绝交易并被标记为异常回收者"},
      {"number":3,"goal":"温栖安追查名字消失的源头","expected_turn":"他发现自己的名字也在裂痕名单上"}
    ]
  }
}"#;
    let repaired = super::super::submit_pending_contract_metadata_repair(&mut draft, patch)
        .expect("metadata repair outcome");

    assert!(
        repaired.is_ready(),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    assert!(
        !draft.title.trim().is_empty(),
        "metadata repair should leave a usable title"
    );
    assert_ne!(draft.title, "未命名小说");
    assert_eq!(draft.fiction_world_rules.len(), 3);
    assert!(full_contract_view(&draft).contains("雨夜借名"));
}

#[test]
fn title_with_world_rules_is_metadata_patch_without_outline() {
    let draft = super::super::build_initial_creation_draft(
        "session-title-world-rules-metadata",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "凡骨开天门",
    "candidates": ["凡骨开天门", "残卷照凡城", "九霄凡门令"],
    "rationale": "凡骨来自主角凡人身份，开天门对应终局公开残卷漏洞、让凡人获得入道资格的核心爽点。",
    "source": "llm_contract"
  },
  "world_rules": [
    "残卷记录修行入口的漏洞，但每次使用都会引来宗门追踪。",
    "天道垄断会封锁凡人城的灵脉入口。",
    "凡人借灵脉修行必须付出记忆、寿元或关系代价。"
  ]
}"#;

    let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw)
        .expect("metadata patch");
    let super::super::CreationContractPatch::Metadata(metadata) = patch else {
        panic!("title plus world_rules must stay metadata patch, got {patch:?}");
    };

    assert_eq!(metadata.title.canonical_title, "凡骨开天门");
    assert_eq!(metadata.world_rules.len(), 3);
}

#[test]
fn title_patch_accepts_candidate_only_payload_for_quality_selection() {
    let draft = super::super::build_initial_creation_draft(
        "session-title-candidates-only",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "candidates": [
      {
        "title": "夺账本翻盘",
        "hook_type": "爽点行动",
        "rationale": "账本是主角公开垄断证据的关键物，翻盘对应底层主角终局改写晋升规则的爽点。"
      },
      {
        "title": "旧楼开局令",
        "hook_type": "关键物件",
        "rationale": "旧楼是主角获得线索的起点，开局令指向他获得反击资格。"
      }
    ]
  }
}"#;

    let patch =
        super::super::normalize_creation_contract_patch_boundary(&draft, raw).expect("title patch");
    assert!(patch.validate_scope(&draft).ready());
    let super::super::CreationContractPatch::Title(title) = patch else {
        panic!("candidate-only title payload should normalize as title patch: {patch:?}");
    };

    assert_eq!(title.canonical_title, "夺账本翻盘");
    assert!(title
        .candidate_rationales
        .get("夺账本翻盘")
        .is_some_and(|value| value.contains("账本")));
}

#[test]
fn metadata_repair_accepts_cjk_field_pack_world_rules() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-world-rules-field-pack",
        "fiction",
        "写都市轻玄幻短篇小说，每章2500字，总字数5000字。",
    )
    .expect("draft");
    let incomplete = r#"{
  "title": {
    "canonical_title": "旧表借名夜",
    "candidates": ["旧表借名夜", "雨夜借名人", "旧城补名师"],
    "rationale": "旧表来自关键物件，借名对应主角修复城市遗忘裂痕并让居民重新取回名字的终局爽点。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"都市轻玄幻",
  "brief":"都市轻玄幻短篇",
  "target_units":5000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"温栖安经营一家能回收被遗忘之物的旧货店。",
  "ending":{"desired_resolution":"温栖安用旧怀表修复城市遗忘裂痕。","final_state":"旧城居民重新记起被抹掉的名字。"},
  "protagonist_arc":"从逃避过去到主动修补城市裂痕。",
  "world_imagery":"雨夜旧货店、因果线、会哭的旧怀表。",
  "main_causal_spine":"旧怀表异动引出恒宇集团收购，温栖安追查遗忘裂痕，终局修复城市记忆。",
  "characters":[
    {"canonical_name":"温栖安","role":"主角","desire":"找回自己的名字","fear":"被城市彻底遗忘","bottom_line":"不把起源怀表交给恒宇集团","arc_start":"逃避过去","arc_end":"主动修补裂痕"},
    {"canonical_name":"白闻安","role":"同伴","desire":"记录都市传说","fear":"真相无人相信","bottom_line":"不伪造证据","arc_start":"旁观记录者","arc_end":"并肩守住旧城"},
    {"canonical_name":"辛知澜","role":"对手","desire":"垄断遗忘之物","fear":"恒宇集团失去控制","bottom_line":"不允许旧城记忆公开","arc_start":"收购代理人","arc_end":"被迫面对裂痕代价"}
  ],
  "themes":["记忆与代价"],
  "world_rules":[],
  "style_rules":["保持中文场景推进"],
  "must_avoid":["不要角色改名"],
  "outline":{
    "volumes":[{"title":"雨夜借名","objective":"温栖安接到会哭的旧怀表并确认遗忘裂痕存在","ending_change":"恒宇集团发现旧货店能修补裂痕"}],
    "near_chapters":[
      {"number":1,"goal":"温栖安在雨夜回收会哭的旧怀表","expected_turn":"怀表叫出一个被城市抹掉的名字"},
      {"number":2,"goal":"恒宇集团代理人上门收购旧怀表","expected_turn":"温栖安拒绝交易并被标记为异常回收者"},
      {"number":3,"goal":"温栖安追查名字消失的源头","expected_turn":"他发现自己的名字也在裂痕名单上"}
    ]
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(&mut draft, incomplete);
    assert!(!first.is_ready());
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("世界规则")),
        "{:?}",
        first.gate.actionable_issues()
    );

    let repaired_text = "世界规则：被遗忘之物会吞掉持有者的一段真实记忆，回收时必须用等价记忆抵偿；旧货店只能在雨夜看见因果线，白天错误回收会让裂痕扩大；恒宇集团能格式化无主旧物，但格式化会永久抹掉关联者的名字。";
    let repaired = super::super::submit_pending_contract_metadata_repair(&mut draft, repaired_text)
        .expect("metadata repair outcome");

    assert!(
        !repaired
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("缺少世界规则")),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    let contract = draft
        .current_contract
        .as_ref()
        .or_else(|| {
            draft
                .pending_contract_candidate
                .as_ref()
                .and_then(|candidate| candidate.get("normalized"))
        })
        .and_then(|value| {
            serde_json::from_value::<super::super::NovelCreationContract>(value.clone()).ok()
        })
        .expect("current or pending contract");
    assert!(
        contract.world_rules.len() >= 3,
        "{:?}",
        contract.world_rules
    );
    assert!(
        contract
            .world_rules
            .iter()
            .any(|rule| rule.contains("旧货店只能在雨夜看见因果线")),
        "{:?}",
        contract.world_rules
    );
}

#[test]
fn pending_contract_metadata_local_repair_does_not_invent_chapter_turns() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-metadata-local-patch",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let mostly_complete_with_numeric_turns = r#"{
  "title":{"canonical_title":"夜校灵轨","candidates":["夜校灵轨","旧证入场","借灵补考"],"rationale":"夜校来自主角进入旧城区补考的地点，灵轨来自终局公开运势转移轨迹并改写晋级规则的关键证据。","source":"llm_contract"},
  "language":"zh-CN",
  "genre":"都市玄幻",
  "brief":"旧城区旁听生卷入城市灵能考试黑幕。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"灵能考试决定城市阶层，旧城区学生发现补考会吞噬普通人的运势。",
  "ending":{"desired_resolution":"主角公开运势转移证据并重写晋级规则。","final_state":"旧城区学生获得公平考试入口。","must_resolve":["灵能考试黑幕","运势转移证据"],"allowed_open_questions":[]},
  "protagonist_arc":"从只想保住旁听名额，成长为愿意承担代价的规则改写者。",
  "world_imagery":"旧城区夜校、灵能准考证、会发光的地下轨道。",
  "main_causal_spine":"补考异常引出运势黑幕，主角追查地下灵轨证据，终局公开证据改写规则。",
  "characters":[
    {"canonical_name":"许闻桥","role":"主角","desire":"通过灵能考试改变命运","fear":"再次失去考试资格","bottom_line":"不牺牲同学换取晋级","arc_start":"旁听生","arc_end":"规则改写者"},
    {"canonical_name":"商砚衡","role":"关键对手","desire":"维护灵能考试垄断","fear":"黑幕公开","bottom_line":"不亲手毁掉考试系统","arc_start":"监考者","arc_end":"被证据逼到台前"}
  ],
  "themes":["公平晋级","代价与选择"],
  "world_rules":["灵能考试会转移考生运势","旧城区考生必须借灵入场"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "outline":{
    "volumes":[{"title":"夜校借灵","objective":"主角拿到借灵证并发现考场异常","ending_change":"主角确认考试吞噬运势"}],
    "near_chapters":[
      {"number":1,"goal":"许闻桥被迫参加夜校补考","expected_turn":"1"},
      {"number":2,"goal":"主角找到旧城区考试证据","expected_turn":"2"},
      {"number":3,"goal":"主角第一次反向利用借灵规则","expected_turn":"3"}
    ],
    "raw_outline":"旁听生借灵入场，追查考试吞噬运势，终局公开旧城区证据重写晋级规则。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        mostly_complete_with_numeric_turns,
    );
    assert!(!first.is_ready());

    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("预期转折") && issue.contains("数字占位")),
        "{:?}",
        first.gate.actionable_issues()
    );
    assert!(super::super::repair_pending_contract_metadata_locally(&mut draft).is_none());
    assert!(draft.current_contract.is_none());
    let pending = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|candidate| candidate.get("normalized"))
        .expect("pending normalized contract");
    assert!(pending.to_string().contains("\"expected_turn\":\"1\""));
}

#[test]
fn local_metadata_repair_does_not_infer_missing_world_rules_from_story_semantics() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-world-rules",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let missing_world_rules = r#"{
  "title": {
    "canonical_title": "旧城借灵证",
    "rationale": "旧城是主角入口，借灵证是撬开晋级制度的关键物件，终局会用它公开规则黑账。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"都市爽文",
  "brief":"都市爽文，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"旧城区打工者发现城市晋级系统会把失败者资源转卖给上层。",
  "ending":{"desired_resolution":"主角公开晋级黑账，击败垄断资源的财团，建立透明晋级规则。"},
  "protagonist_arc":"从被压榨的底层打工者到重写晋级规则的城市掌舵人。",
  "world_imagery":"旧城区夜校、借灵证、财团账本、霓虹办公楼",
  "main_causal_spine":"拿到借灵证→发现资源黑账→反制财团陷阱→公开晋级规则→重写城市分配制度",
  "characters":[
    {"canonical_name":"温岑禾","role":"主角","desire":"拿到晋级资格","fear":"再次被制度吞掉成果","bottom_line":"不牺牲同伴换晋升","arc_start":"旧城区打工者","arc_end":"城市掌舵人"},
    {"canonical_name":"顾棠禾","role":"关键对手","desire":"维护财团资源垄断","fear":"黑账公开","bottom_line":"不放弃上层特权","arc_start":"财团继承人","arc_end":"被公开证据逼到台前"}
  ],
  "themes":["公平晋级","代价与选择"],
  "world_rules":[],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "outline":{
    "volumes":[{"title":"旧城入场","objective":"主角拿到借灵证并发现资源异常","ending_change":"主角确认晋级黑账存在"}],
    "near_chapters":[
      {"number":1,"goal":"温岑禾被迫参加夜校补考","expected_turn":"他拿到第一张借灵证并发现账本编号"},
      {"number":2,"goal":"主角找到资源转卖证据","expected_turn":"财团第一次派人试探主角底线"},
      {"number":3,"goal":"主角反向利用借灵规则","expected_turn":"他赢下第一场公开反杀"}
    ],
    "raw_outline":"打工者借灵入场，追查晋级黑账，终局公开财团证据重写规则。"
  }
}"#;

    let first =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, missing_world_rules);
    if first.is_ready() {
        assert!(
            draft.fiction_world_rules.len() >= 3,
            "{:?}",
            draft.fiction_world_rules
        );
        return;
    }
    assert!(first
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("世界规则")));

    let repaired = super::super::repair_pending_contract_metadata_locally(&mut draft);
    if let Some(outcome) = repaired {
        assert!(
                !outcome.is_ready(),
                "local metadata repair may preserve deterministic fixes, but missing semantic world rules must not become ready"
            );
    }
    assert!(
        draft.fiction_world_rules.is_empty(),
        "{:?}",
        draft.fiction_world_rules
    );
    assert!(draft.current_contract.is_none());
}

#[test]
fn local_metadata_repair_syncs_existing_structured_world_rules() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-structured-world-rules",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let structured_world_rules = r#"{
  "title": {
    "canonical_title": "旧城借灵证",
    "rationale": "旧城是主角入口，借灵证是撬开晋级制度的关键物件，终局会用它公开规则黑账。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"都市爽文",
  "brief":"都市爽文，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"旧城区打工者发现城市晋级系统会把失败者资源转卖给上层。",
  "ending":{"desired_resolution":"主角公开晋级黑账，击败垄断资源的财团，建立透明晋级规则。"},
  "protagonist_arc":"从被压榨的底层打工者到重写晋级规则的城市掌舵人。",
  "world_imagery":"旧城区夜校、借灵证、财团账本、霓虹办公楼",
  "main_causal_spine":"拿到借灵证→发现资源黑账→反制财团陷阱→公开晋级规则→重写城市分配制度",
  "characters":[
    {"canonical_name":"温岑禾","role":"主角","desire":"拿到晋级资格","fear":"再次被制度吞掉成果","bottom_line":"不牺牲同伴换晋升","arc_start":"旧城区打工者","arc_end":"城市掌舵人"},
    {"canonical_name":"顾棠禾","role":"关键对手","desire":"维护财团资源垄断","fear":"黑账公开","bottom_line":"不放弃上层特权","arc_start":"财团继承人","arc_end":"被公开证据逼到台前"}
  ],
  "themes":["公平晋级","代价与选择"],
  "world_rules":[],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "resource_economy":{
    "cost_examples":["借灵证每使用一次都会抽走持证人下一场晋级资格。"],
    "scarcity_rules":["旧城区每月只能发三张借灵证，逾期必须重新排队。"],
    "trade_rules":["财团只能通过公开账本编号交易借灵证，否则交易会触发审计记录。"]
  },
  "outline":{
    "volumes":[{"title":"旧城入场","objective":"主角拿到借灵证并发现资源异常","ending_change":"主角确认晋级黑账存在"}],
    "near_chapters":[
      {"number":1,"goal":"温岑禾被迫参加夜校补考","expected_turn":"他拿到第一张借灵证并发现账本编号"},
      {"number":2,"goal":"主角找到资源转卖证据","expected_turn":"财团第一次派人试探主角底线"},
      {"number":3,"goal":"主角反向利用借灵规则","expected_turn":"他赢下第一场公开反杀"}
    ],
    "raw_outline":"打工者借灵入场，追查晋级黑账，终局公开财团证据重写规则。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        structured_world_rules,
    );
    if first.is_ready() {
        assert!(
            draft
                .fiction_world_rules
                .iter()
                .any(|rule| rule.contains("借灵证每使用一次")),
            "{:?}",
            draft.fiction_world_rules
        );
        return;
    }
    assert!(first
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("世界规则")));

    let repaired = super::super::repair_pending_contract_metadata_locally(&mut draft)
        .expect("structured world rules should repair locally");
    assert!(
        repaired.is_ready(),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    assert!(draft
        .fiction_world_rules
        .iter()
        .any(|rule| rule.contains("借灵证每使用一次")));
    assert!(draft.current_contract.is_some());
}

#[test]
fn local_metadata_repair_replaces_phrase_like_world_rules_with_actionable_rules() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-world-rule-phrases",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let phrase_like_world_rules = r#"{
  "title": {
    "canonical_title": "逆脉破天门",
    "rationale": "逆脉是主角破局能力，天门是终局要打破的垄断入口。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"异界修仙",
  "brief":"凡人以逆脉法打破宗门资源垄断。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"在灵气枯竭、宗门垄断资源的异界，祝阙棠发现上古逆脉修炼法，通过吞噬异种灵力打破阶层壁垒。",
  "ending":{"desired_resolution":"祝阙棠公开灵脉真相，斩断仙庭收割链，重定修行秩序。"},
  "protagonist_arc":"从隐忍求生的底层弃子，成长为敢于承担代价并改写秩序的人。",
  "world_imagery":"枯竭灵脉、逆脉残卷、宗门矿坑、仙庭税印",
  "main_causal_spine":"灵脉枯竭导致资源垄断->祝阙棠发现逆脉法->宗门打压->仙庭降临收割->祝阙棠斩断因果链->世界重生",
  "characters":[
    {"canonical_name":"祝阙棠","role":"主角","desire":"打破宗门资源垄断","fear":"自己也成为收割者","bottom_line":"不以无辜者为修行耗材","arc_start":"底层弃子","arc_end":"重定秩序者"},
    {"canonical_name":"南砚舟","role":"关键对手","desire":"维持仙庭收割链","fear":"凡人知道灵脉真相","bottom_line":"众生都可作为税印耗材","arc_start":"仙庭代行者","arc_end":"失去收割权柄"}
  ],
  "themes":["代价与自由"],
  "world_rules":["宗门垄断资源的异界","通过吞噬异种灵力打破阶层壁垒","灵脉枯竭导致资源垄断"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名"],
  "outline":{
    "volumes":[{"title":"逆脉初醒","objective":"确认逆脉法和宗门垄断之间的关系","ending_change":"祝阙棠被迫成为宗门追捕目标"}],
    "near_chapters":[
      {"number":1,"goal":"祝阙棠在矿坑发现逆脉残卷","expected_turn":"他确认自己能吞噬异种灵力但会被税印追踪"},
      {"number":2,"goal":"宗门发现逆脉异常","expected_turn":"祝阙棠被迫逃出矿坑"},
      {"number":3,"goal":"祝阙棠第一次反向利用税印","expected_turn":"他拿到宗门垄断灵脉的证据"}
    ],
    "raw_outline":"矿坑发现逆脉，追查宗门垄断，终局斩断仙庭收割链。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        phrase_like_world_rules,
    );
    assert!(!first.is_ready());
    assert!(super::super::repair_pending_contract_metadata_locally(&mut draft).is_none());
    assert!(
        draft
            .fiction_world_rules
            .iter()
            .all(|rule| !rule.contains("通过吞噬异种灵力打破阶层壁垒")),
        "{:?}",
        draft.fiction_world_rules
    );
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("世界规则")),
        "{:?}",
        first.gate.actionable_issues()
    );
    assert!(draft.current_contract.is_none());
}

#[test]
fn local_metadata_repair_does_not_commit_bare_role_label_book_title() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-title-role-label",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let role_label_title = r#"{
  "title": {
    "canonical_title": "破局者",
    "rationale": "主角打破旧有利益阶层固化，成为重构城市规则的关键破局人物。"
  },
  "language":"zh-CN",
  "genre":"都市爽文",
  "brief":"底层销售在城市资源垄断里翻身，公开财阀合同漏洞。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"裴岑澜从被裁销售入局，发现财阀用合同漏洞垄断城市资源。",
  "ending":{"desired_resolution":"主角公开财阀合同漏洞，打破旧有利益阶层固化。","final_state":"城市资源分配规则被重构。","must_resolve":["合同漏洞","财阀垄断"],"allowed_open_questions":[]},
  "protagonist_arc":"从被规则压着走的人，成长为重构城市规则的关键破局人物。",
  "world_imagery":"雨夜写字楼、废弃合同档案、城市资源交易大厅。",
  "main_causal_spine":"被裁入局->发现合同漏洞->追查资源垄断->公开证据->重构城市规则。",
  "characters":[
    {"canonical_name":"裴岑澜","role":"主角","desire":"夺回被规则吞掉的职业尊严","fear":"再次被财阀规则碾碎","bottom_line":"不牺牲普通员工换取胜利","arc_start":"被裁销售","arc_end":"规则重构者"},
    {"canonical_name":"韩闻棠","role":"关键对手","desire":"维持财阀资源垄断","fear":"合同漏洞公开","bottom_line":"不放弃核心资源控制权","arc_start":"掌握规则优势","arc_end":"被公开证据逼到台前"}
  ],
  "themes":["尊严","规则与代价"],
  "world_rules":[],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名"],
  "outline":{
    "volumes":[{"title":"雨夜入局","objective":"主角发现财阀合同漏洞","ending_change":"主角无法再置身事外"}],
    "near_chapters":[
      {"number":1,"goal":"裴岑澜在雨夜写字楼拿到废弃合同档案","expected_turn":"他发现财阀资源垄断的第一个漏洞"},
      {"number":2,"goal":"韩闻棠派人回收合同档案","expected_turn":"裴岑澜确认自己已经入局"},
      {"number":3,"goal":"主角利用旧客户关系验证漏洞","expected_turn":"他拿到第一份可公开证据"}
    ],
    "raw_outline":"裴岑澜从雨夜写字楼拿到废弃合同档案，追查财阀资源垄断，终局公开合同漏洞并重构城市规则。"
  }
}"#;

    let first =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, role_label_title);
    assert!(!first.is_ready(), "{:?}", first.gate.actionable_issues());

    if let Some(repaired) = super::super::repair_pending_contract_metadata_locally(&mut draft) {
        assert!(
                !repaired.is_ready(),
                "local metadata repair must not commit a bare role-label title or infer missing semantics"
            );
        assert!(
            repaired
                .gate
                .actionable_issues()
                .iter()
                .any(|issue| issue.contains("书名") || issue.contains("世界规则")),
            "{:?}",
            repaired.gate.actionable_issues()
        );
    }
    assert!(draft.current_contract.is_none());
}

#[test]
fn local_metadata_repair_reconciles_outline_book_title_authority() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-outline-title-authority",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let title_mismatch = r#"{
  "title": {
    "canonical_title": "凡骨借天门",
    "rationale": "凡骨是主角被宗门轻视的身份，借天门是他借公开残卷漏洞为凡人打开修行入口的终局爽点。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"异界修仙",
  "brief":"凡人少年发现宗门垄断修行入口的残卷漏洞。",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"凡人城少年拿到残卷，发现宗门用天门令垄断修行资格。",
  "ending":{"desired_resolution":"主角公开残卷漏洞，击败守门宗门，让凡人获得入道资格。","final_state":"凡人城获得公开试炼入口。","must_resolve":["残卷漏洞","天门令垄断"],"allowed_open_questions":[]},
  "protagonist_arc":"从只想保住家人和摊位的凡人少年，成长为敢公开修行入口的破局者。",
  "world_imagery":"凡人城夜市、断裂残卷、天门令、宗门石阶。",
  "main_causal_spine":"残卷现世引来追杀，主角追查天门令垄断证据，终局公开漏洞改写入道规则。",
  "characters":[
    {"canonical_name":"顾岚珩","role":"主角","desire":"拿到公开入道资格","fear":"家人被宗门牵连","bottom_line":"不以凡人城性命换取修行捷径","arc_start":"凡人城少年","arc_end":"天门破局者"},
    {"canonical_name":"谢夙棠","role":"关键对手","desire":"维护宗门入道垄断","fear":"残卷漏洞公开","bottom_line":"不承认凡人有资格登天门","arc_start":"宗门执令者","arc_end":"被公开证据逼到台前"}
  ],
  "themes":["公平入道","代价与选择"],
  "world_rules":["天门令决定凡人能否入道","残卷能暴露天门令的资格漏洞","宗门追杀会牵连持卷者所在城镇"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要抽象总结式书名"],
  "outline":{
    "volumes":[{"title":"残卷入城","objective":"主角拿到残卷并确认天门令漏洞","ending_change":"宗门追杀让他无法留在事外"}],
    "near_chapters":[
      {"number":1,"goal":"主角在凡人城夜市发现断裂残卷","expected_turn":"残卷引来宗门追杀，主角无法再留在凡人城"},
      {"number":2,"goal":"主角查到天门令资格被宗门改写","expected_turn":"他确认凡人入道失败不是天命，而是被制度夺走"},
      {"number":3,"goal":"对手奉命夺卷并封锁凡人城","expected_turn":"主角第一次公开反击宗门执令者"}
    ],
    "raw_outline":"《浊天诀》从残卷现世起势，追查天门令垄断证据，终局公开漏洞让凡人获得入道资格。"
  }
}"#;

    let direct_repaired = super::super::canonicalize_outline_book_title_quotes(
        "《浊天诀》从残卷现世起势，追查天门令垄断证据，终局公开漏洞让凡人获得入道资格。",
        "凡骨借天门",
        &[],
    )
    .expect("direct outline title quote repair");
    assert!(
        direct_repaired.contains("《凡骨借天门》"),
        "{direct_repaired}"
    );

    let first =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, title_mismatch);
    assert!(!first.is_ready(), "{:?}", first.gate.actionable_issues());
    assert!(first
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("权威书名不一致")));

    let repaired = super::super::repair_pending_contract_metadata_locally(&mut draft)
        .expect("local metadata repair should reconcile outline title authority");
    assert!(
        !repaired
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("权威书名不一致")),
        "{:?}",
        repaired.gate.actionable_issues()
    );
    let normalized = draft
        .current_contract
        .as_ref()
        .or_else(|| {
            draft
                .pending_contract_candidate
                .as_ref()
                .and_then(|candidate| candidate.get("normalized"))
        })
        .expect("current or pending normalized contract");
    let raw_outline = normalized
        .pointer("/outline/raw_outline")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert!(raw_outline.contains("《凡骨借天门》"), "{raw_outline}");
    assert!(!raw_outline.contains("《浊天诀》"), "{raw_outline}");
}

#[test]
fn local_metadata_repair_requires_typed_patch_for_missing_outline() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-outline-repair",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let missing_outline = r#"{
  "title": {
    "canonical_title": "残卷开天门",
    "rationale": "残卷是主角发现修行入口被垄断的关键证据，开天门对应终局公开漏洞让凡人获得入道资格。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"异界修仙",
  "brief":"异界修仙，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"凡人少年在九霄大陆发现天道残卷，被迫卷入宗门与旧天道的争夺。",
  "ending":{"desired_resolution":"主角公开残卷中的修行漏洞，让凡人也能获得入道资格。","final_state":"旧天道垄断被打破，九霄大陆出现新的修行入口。"},
  "protagonist_arc":"从只求自保的凡人少年，到愿意公开残卷代价的开道者。",
  "world_imagery":"九霄云阙、断裂残卷、灵脉天梯、被封锁的凡人城。",
  "main_causal_spine":"残卷现世引出宗门追杀，主角追查天道漏洞，终局公开残卷改写修行入口。",
  "characters":[
    {"canonical_name":"南照野","role":"主角","desire":"为凡人争取入道资格","fear":"残卷代价害死同伴","bottom_line":"不献祭无辜者","arc_start":"凡人自保","arc_end":"公开修行入口"},
    {"canonical_name":"段岚澜","role":"同伴","desire":"守住凡人城","fear":"宗门清算家人","bottom_line":"不背叛凡人城","arc_start":"谨慎相助","arc_end":"并肩公开证据"}
  ],
  "themes":["凡人开道","代价与公开"],
  "world_rules":["残卷记录修行入口的漏洞，但每次使用都会引来宗门追踪。","天道垄断会封锁凡人城的灵脉入口。"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要用摘要替代正文"],
  "outline":{"volumes":[],"near_chapters":[],"raw_outline":""}
}"#;

    let first =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, missing_outline);
    assert!(!first.is_ready());
    assert!(super::super::repair_pending_contract_metadata_locally(&mut draft).is_none());
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("分卷") || issue.contains("近期章节")),
        "{:?}",
        first.gate.actionable_issues()
    );
    let contract = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|candidate| candidate.get("normalized"))
        .and_then(|value| {
            super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
        })
        .expect("pending typed contract");
    assert!(contract.outline.volumes.is_empty());
    assert!(contract.outline.near_chapters.is_empty());
    assert!(contract.outline.raw_outline.is_empty());
}

#[test]
fn local_metadata_repair_keeps_missing_outline_for_typed_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-outline-partial-commit",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let missing_outline_and_bad_title = r#"{
  "title": {
    "canonical_title": "南照野",
    "rationale": "书名暂时错误地等于主角名。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"异界修仙",
  "brief":"异界修仙，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"凡人少年在九霄大陆发现天道残卷，被迫卷入宗门与旧天道的争夺。",
  "ending":{"desired_resolution":"主角公开残卷中的修行漏洞，让凡人也能获得入道资格。","final_state":"旧天道垄断被打破，九霄大陆出现新的修行入口。"},
  "protagonist_arc":"从只求自保的凡人少年，到愿意公开残卷代价的开道者。",
  "world_imagery":"九霄云阙、断裂残卷、灵脉天梯、被封锁的凡人城。",
  "main_causal_spine":"残卷现世引出宗门追杀，主角追查天道漏洞，终局公开残卷改写修行入口。",
  "characters":[
    {"canonical_name":"南照野","role":"主角","desire":"为凡人争取入道资格","fear":"残卷代价害死同伴","bottom_line":"不献祭无辜者","arc_start":"凡人自保","arc_end":"公开修行入口"},
    {"canonical_name":"段岚澜","role":"同伴","desire":"守住凡人城","fear":"宗门清算家人","bottom_line":"不背叛凡人城","arc_start":"谨慎相助","arc_end":"并肩公开证据"}
  ],
  "themes":["凡人开道","代价与公开"],
  "world_rules":["残卷记录修行入口的漏洞，但每次使用都会引来宗门追踪。","天道垄断会封锁凡人城的灵脉入口。"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要用摘要替代正文"],
  "outline":{"volumes":[],"near_chapters":[],"raw_outline":""}
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(
        &mut draft,
        missing_outline_and_bad_title,
    );
    assert!(!first.is_ready());

    assert!(super::super::repair_pending_contract_metadata_locally(&mut draft).is_none());
    assert!(
        first
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("分卷") || issue.contains("近期章节")),
        "{:?}",
        first.gate.actionable_issues()
    );
    let normalized_after_repair = draft
        .current_contract
        .as_ref()
        .or_else(|| {
            draft
                .pending_contract_candidate
                .as_ref()
                .and_then(|value| value.get("normalized"))
        })
        .expect("current or pending normalized contract");
    let pending = super::super::NovelCreationContract::parse_json_boundary(
        &normalized_after_repair.to_string(),
    )
    .expect("normalized contract");
    assert!(pending.outline.volumes.is_empty());
    assert!(pending.outline.near_chapters.is_empty());
    assert!(pending.outline.raw_outline.is_empty());
}

#[test]
fn local_metadata_repair_does_not_invent_genre_specific_outline() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-romance-outline-repair",
        "fiction",
        "写都市言情小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let missing_outline = r#"{
  "title": {
    "canonical_title": "雨夜退婚局",
    "rationale": "雨夜是婚礼事故发生的入口地点，退婚局对应主角公开证据并重建事业与亲密关系的终局反转。",
    "source": "llm_contract"
  },
  "language":"zh-CN",
  "genre":"都市言情",
  "brief":"都市言情，每章2500字，至少5万字起",
  "target_units":50000,
  "chapter_unit_target":2500,
  "max_chapters_per_turn":1,
  "premise":"投行女总裁在婚礼前夜发现未婚夫背叛，却在追查中发现这场婚变牵连一桩财务造假案。",
  "ending":{"desired_resolution":"主角公开证据守住事业边界，并与真正支持她的人完成双向选择。","final_state":"婚变真相公开，主角重建事业和亲密关系。"},
  "protagonist_arc":"从把完美婚姻当成安全感，到敢于公开真相并选择真正尊重自己的关系。",
  "world_imagery":"雨夜酒店、投行会议室、婚戒、审计底稿、深夜便利店。",
  "main_causal_spine":"婚变触发调查，审计底稿揭开财务造假，公开证据迫使主角在事业声誉和亲密关系中作出选择。",
  "characters":[
    {"canonical_name":"许闻禾","role":"主角","desire":"公开真相并守住事业尊严","fear":"被婚姻和职场共同吞没","bottom_line":"不让无辜同事替人背锅","arc_start":"压抑自保","arc_end":"公开选择"},
    {"canonical_name":"宋庭川","role":"关系对象","desire":"帮助许闻禾守住证据链","fear":"自己的沉默再次伤害重要之人","bottom_line":"不替权势掩盖真相","arc_start":"克制旁观","arc_end":"并肩承担"}
  ],
  "themes":["自我选择","现实关系与声誉代价"],
  "world_rules":["投行项目的审计证据会影响职业声誉和婚礼舆论。","家庭、客户和公司制度会同时压迫主角的亲密选择。"],
  "style_rules":["具体场景推进","保持中文"],
  "must_avoid":["不要角色改名","不要加入超自然规则"],
  "outline":{"volumes":[],"near_chapters":[],"raw_outline":""}
}"#;

    let first =
        super::super::submit_generated_contract_candidate_to_draft(&mut draft, missing_outline);
    assert!(!first.is_ready());
    assert!(super::super::repair_pending_contract_metadata_locally(&mut draft).is_none());
    let contract = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|candidate| candidate.get("normalized"))
        .and_then(|value| {
            super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
        })
        .expect("pending typed contract");
    assert!(contract.outline.volumes.is_empty());
    assert!(contract.outline.near_chapters.is_empty());
    assert!(contract.outline.raw_outline.is_empty());
}

#[test]
fn generic_title_rationale_requires_a_typed_repair() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-weak-title",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "霓虹余烬",
    "rationale": "霓虹和余烬体现都市玄幻气质。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "草根学生卷入城市灵能考试黑幕。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "灵能考试决定城市阶层，旁听生发现考试会吞噬普通人的运势。",
  "ending": {
    "desired_resolution": "主角公开灵能考试黑幕并重写城市晋级规则。",
    "final_state": "旧城区学生获得公平考试入口。",
    "must_resolve": ["灵能考试黑幕", "旧城区被吞噬的运势"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只想保住旁听名额，成长为愿意承担代价的规则改写者。",
  "world_imagery": "霓虹校门、灵能考场、旧城区余烬。",
  "main_causal_spine": "失败补考引出晋级黑幕，主角追查运势吞噬证据，终局公开证据改写规则。",
  "characters": [
    {"canonical_name":"许闻桥","role":"主角","desire":"通过灵能考试改变命运","fear":"再次失去考试资格","bottom_line":"不牺牲同学换取晋级","arc_start":"旁听生","arc_end":"规则改写者"},
    {"canonical_name":"商砚衡","role":"关键对手","desire":"维护灵能考试垄断","fear":"黑幕公开","bottom_line":"不亲手毁掉考试系统","arc_start":"监考者","arc_end":"被迫面对旧城区证据"}
  ],
  "themes": ["公平晋级", "代价与选择"],
  "world_rules": ["灵能考试会转移考生运势", "旧城区考生必须借灵入场"],
  "style_rules": ["具体场景推进", "保持中文"],
  "must_avoid": ["不要角色改名", "不要抽象总结式书名"],
  "outline": {
    "volumes": [{"title":"夜校借灵","objective":"主角拿到借灵证并发现考场异常","ending_change":"主角确认考试吞噬运势"}],
    "near_chapters": [
      {"number":1,"title":"夜校借灵","goal":"许闻桥被迫参加夜校补考","expected_turn":"他发现借灵证会记录运势损耗"},
      {"number":2,"title":"旧证入场","goal":"主角找到旧城区考试证据","expected_turn":"证据指向监考系统"},
      {"number":3,"title":"考场逆光","goal":"主角第一次反向利用借灵规则","expected_turn":"他救下同学但暴露身份"}
    ],
    "raw_outline":"旁听生借灵入场，追查考试吞噬运势，终局公开旧城区证据重写晋级规则。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(
        !outcome.is_ready(),
        "generic title rationale should require title repair before contract approval"
    );
    assert!(!outcome.committed);
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("命名理由")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn title_metadata_repair_prompt_uses_pending_contract_anchor() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-pending-anchor",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "都市问道录",
    "rationale": "书名来自终局方向的问道结局、主线因果链的修真体系、世界观意象的都市玄门",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "都市玄幻，每章2500字，至少5万字起",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "现代都市中隐藏的修真世家与科技文明的碰撞",
  "ending": {
    "desired_resolution": "主角打破都市与修真的界限，建立新秩序",
    "final_state": "城市获得科技修真共治的新规则",
    "must_resolve": ["都市修真资源垄断"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从普通程序员成长为都市修真体系的缔造者",
  "world_imagery": "霓虹灯下的古建筑群，数据流与灵气交织的都市生态",
  "main_causal_spine": "主角发现都市中隐藏的修真传承，为保护普通人而与各方势力对抗，最终建立融合科技与修真的新文明",
  "characters": [
    {"canonical_name":"秦澈棠","role":"主角","desire":"揭开都市修真秘密","fear":"力量失控伤及无辜","bottom_line":"不牺牲普通人","arc_start":"普通程序员","arc_end":"都市修真体系的缔造者"},
    {"canonical_name":"陆天启","role":"关键对手","desire":"掌控都市力量","fear":"被时代淘汰","bottom_line":"维护既得利益","arc_start":"既得利益者","arc_end":"被新规则审判"}
  ],
  "themes": ["力量与责任", "科技与修真秩序"],
  "world_rules": ["灵气会被数据协议记录", "修真资源必须通过城市节点流转"],
  "style_rules": ["具体场景推进", "保持中文"],
  "must_avoid": ["不要角色改名", "不要抽象总结式书名"],
  "outline": {
    "volumes": [{"title":"暗流涌动","objective":"揭示都市修真体系存在","ending_change":"主角获得初步修行感知"}],
    "near_chapters": [
      {"number":1,"goal":"发现异常数据波动","expected_turn":"主角觉醒特殊感知能力"},
      {"number":2,"goal":"追踪灵气数据源","expected_turn":"线索指向古建筑群"}
    ],
    "raw_outline":"主角调试程序触发上古秘术，追查都市修真世家，终局建立科技与修真共治新秩序。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.is_ready());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(draft.fiction_premise.is_empty());
    let prompt = super::super::final_prompt_from_title_metadata_repair(
        &draft,
        &outcome.gate.actionable_issues(),
    )
    .expect("title metadata repair prompt");
    assert!(prompt.contains("上一版候选中可复用的结构化字段"));
    assert!(prompt.contains("现代都市中隐藏的修真世家"));
    assert!(prompt.contains("普通程序员"));
    assert!(prompt.contains("已锁定角色：姓名："), "{prompt}");
    assert!(!prompt.contains("已锁定角色：本轮重新生成具体姓名和角色锚点"));
    assert!(prompt.contains("已锁定角色名是唯一可使用的人名"));
    assert!(prompt.contains("不得新造角色名充当书名锚点"));
    assert!(prompt.contains("文字必须完整、自然"));
    assert!(
        prompt.contains("科技修真共治")
            || prompt.contains("科技与修真共治")
            || prompt.contains("融合科技与修真")
    );
    assert!(prompt.contains("书名：中文作品名"));
    assert!(prompt.contains("书名候选：候选1；候选2；候选3"));
    assert!(
        prompt.contains("书名理由：用一句具体中文说明书名如何来自终局、主线、世界规则或关键事件")
    );
}

#[test]
fn title_metadata_repair_refreshes_pending_normalized_contract() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-refreshes-pending-normalized",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "瞳力觉醒",
    "candidates": ["瞳力觉醒"],
    "rationale": "书名来自主角觉醒瞳术。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "都市玄幻，每章2500字，至少5万字起",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "现代都市中隐藏的符文网络正在吞噬普通人的感知。",
  "ending": {
    "desired_resolution": "主角在终局公开符文网络的吞噬账册，重写城市能量分配规则。",
    "final_state": "普通人获得不被符文网络掠夺的城市新秩序。",
    "must_resolve": ["符文网络吞噬感知的真相"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只想自保的边缘学生成长为敢公开城市账册的守门人。",
  "world_imagery": "雨夜天桥、符文网络、感知账册、旧城节点。",
  "main_causal_spine": "觉醒瞳术看见符文网络，追查感知账册，终局公开账册并改写城市规则。",
  "characters": [
    {"canonical_name":"许闻桥","role":"主角","desire":"查清感知被夺的真相","fear":"妹妹也被符文网络吞噬记忆","bottom_line":"不牺牲普通人换取力量","arc_start":"边缘学生","arc_end":"城市规则守门人"},
    {"canonical_name":"商砚衡","role":"关键对手","desire":"维持符文网络垄断","fear":"感知账册公开","bottom_line":"不允许底层越过旧秩序","arc_start":"垄断维护者","arc_end":"被新规则审判"}
  ],
  "themes": ["力量垄断与普通人的选择权"],
  "world_rules": ["符文网络会按账册抽取感知", "旧城节点决定能量流向"],
  "style_rules": ["用具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"旧城账册","objective":"揭示符文网络吞噬感知","ending_change":"主角拿到第一份感知账册"}],
    "near_chapters": [
      {"number":1,"goal":"许闻桥在雨夜天桥第一次看见符文网络","expected_turn":"主角确认感知被抽取不是幻觉"},
      {"number":2,"goal":"追查旧城节点的能量记录","expected_turn":"线索指向感知账册"}
    ],
    "raw_outline":"主角看见符文网络，追查感知账册，终局公开账册并重写城市规则。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(!outcome.is_ready(), "weak title should stay pending first");
    assert!(draft.pending_contract_candidate.is_some());

    let repaired = r#"{
	  "title": {
	    "canonical_title": "温氏掌权者",
	    "candidates": ["温氏掌权者", "旧城感知账", "天桥见账人"],
	    "rationale": "其他候选包括温氏掌权者和天桥见账人；旧城来自世界观意象和终局地点，感知账来自符文网络抽取感知的核心物证，书名指向主角终局公开账册改写城市规则。",
	    "source": "llm_contract"
	  }
	}"#;
    let repaired_outcome =
        super::super::submit_pending_contract_title_metadata_repair(&mut draft, repaired)
            .expect("title repair outcome");
    assert!(
        repaired_outcome.committed || draft.pending_contract_candidate.is_some(),
        "title repair should commit a ready contract or keep a repaired pending candidate"
    );

    let authoritative_title = if repaired_outcome.is_ready() {
        draft
            .current_contract
            .as_ref()
            .and_then(|value| value.pointer("/title/canonical_title"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    } else {
        draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|value| value.pointer("/normalized/title/canonical_title"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    };
    assert_ne!(authoritative_title, "温氏掌权者");
    let authoritative_contract = if repaired_outcome.is_ready() {
        draft.current_contract.as_ref()
    } else {
        draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|value| value.get("normalized"))
    }
    .expect("authoritative contract should be retained");
    let contract = super::super::NovelCreationContract::parse_json_boundary(
        &authoritative_contract.to_string(),
    )
    .expect("contract should still parse");
    assert_eq!(contract.title.canonical_title, authoritative_title);
    assert!(
        super::super::naming::title_anchor_tokens(authoritative_title)
            .iter()
            .any(|token| contract.title.rationale.contains(token)),
        "{}",
        contract.title.rationale
    );
    assert!(
        !contract.title.rationale.contains("其他候选"),
        "{}",
        contract.title.rationale
    );
    assert!(
        contract
            .world_rules
            .iter()
            .any(|rule| rule.contains("符文网络")),
        "{:?}",
        contract.world_rules
    );
    assert_eq!(
        contract
            .characters
            .iter()
            .filter(|character| character.role_looks_primary())
            .count(),
        1,
        "{:?}",
        contract.characters
    );
    assert!(
        contract
            .characters
            .iter()
            .any(|character| !character.role_looks_primary()),
        "{:?}",
        contract.characters
    );
    assert!(
        contract.outline.has_stage_or_near_chapter_plan(),
        "{:?}",
        contract.outline
    );
}

#[test]
fn title_metadata_repair_accepts_field_pack_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-field-pack",
        "fiction",
        "写都市职场小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "职场逆袭",
    "candidates": ["职场逆袭"],
    "rationale": "书名来自职场逆袭。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市职场",
  "brief": "都市职场，每章2500字，至少5万字起",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "底层新人发现玻璃塔晋升名单被暗箱操控。",
  "ending": {
    "desired_resolution": "主角在终局公开升迁账册，打破暗箱晋升机制。",
    "final_state": "团队获得透明晋升通道。",
    "must_resolve": ["暗箱晋升名单"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从被动背锅的职场新人，成长为敢公开升迁账册的破局者。",
  "world_imagery": "玻璃塔、升迁账册、午夜会议室、灰色审批链。",
  "main_causal_spine": "背锅事件引出升迁账册，主角追查灰色审批链，终局公开账册打破暗箱晋升机制。",
  "characters": [
    {"canonical_name":"祝岑棠","role":"主角","desire":"公开升迁账册","fear":"母亲医药费被对手拿捏","bottom_line":"不牺牲同事换取晋升","arc_start":"被动背锅的新人","arc_end":"公开规则的破局者"},
    {"canonical_name":"温岑序","role":"关键对手","desire":"维持暗箱晋升机制","fear":"升迁账册公开","bottom_line":"不允许新人越级查账","arc_start":"审批链掌控者","arc_end":"被账册逼到台前"}
  ],
  "themes": ["透明规则与职场尊严"],
  "world_rules": ["升迁资格由灰色审批链控制", "升迁账册记录每次暗箱换名"],
  "style_rules": ["用具体职场场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"玻璃塔账册","objective":"主角找到第一份升迁账册","ending_change":"确认背锅事件不是偶然"}],
    "near_chapters": [
      {"number":1,"goal":"祝岑棠在午夜会议室被迫背锅","expected_turn":"主角发现升迁账册异常"},
      {"number":2,"goal":"追查灰色审批链","expected_turn":"线索指向玻璃塔高层"}
    ],
    "raw_outline":"主角从背锅事件追查升迁账册，终局公开账册并改写晋升规则。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(
        !outcome.is_ready(),
        "weak title should require repair first"
    );

    let repaired = "书名：玻璃塔升迁账\n书名候选：玻璃塔升迁账；午夜审批链；背锅人的升迁名单\n书名理由：玻璃塔来自核心职场地点，升迁账来自主线物证，书名指向主角终局公开账册打破暗箱晋升机制。";
    let repaired_outcome =
        super::super::submit_pending_contract_title_metadata_repair(&mut draft, repaired)
            .expect("field-pack title repair outcome");
    assert!(
            repaired_outcome.committed || draft.pending_contract_candidate.is_some(),
            "field-pack title repair should reuse typed patch normalizer and keep pending candidate if not ready"
        );
    let authoritative_title = if repaired_outcome.is_ready() {
        draft
            .current_contract
            .as_ref()
            .and_then(|value| value.pointer("/title/canonical_title"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    } else {
        draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|value| value.pointer("/normalized/title/canonical_title"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    };
    assert!(
        ["玻璃塔升迁账", "午夜审批链", "背锅人的升迁名单"].contains(&authoritative_title),
        "{authoritative_title}"
    );
}

#[test]
fn title_repair_rejects_generic_rationale_without_story_specific_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-uses-contract-evidence",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "问道录",
    "candidates": ["问道录"],
    "rationale": "书名体现主角成长。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "异界修仙",
  "brief": "异界修仙，每章2500字，至少5万字起",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "凡人修士在灵气复苏的异界，通过悟道、炼体、结丹、渡劫等阶段逐步突破。",
  "ending": {
    "desired_resolution": "主角突破九重天桎梏，公开天道残卷里的规则漏洞，建立自己的修行体系。",
    "final_state": "旧天道垄断被打破，底层修士获得新的问道入口。",
    "must_resolve": ["上古传承残卷的来源", "天道法则被篡改的真相"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从懵懂学徒到掌握本源法则的问道者。",
  "world_imagery": "九霄云阙、灵墟、上古传承残卷、天道法则。",
  "main_causal_spine": "主角发现传承残卷，卷入法则争夺，终局公开天道残卷并改写修行入口。",
  "characters": [
    {"canonical_name":"南朔珩","role":"主角","desire":"参悟上古问道传承","fear":"道心崩坏堕入魔道","bottom_line":"不为力量牺牲本心","arc_start":"懵懂学徒","arc_end":"本源法则问道者"},
    {"canonical_name":"闻澈遥","role":"关键对手","desire":"吞噬天道法则之力","fear":"被本源法则反噬","bottom_line":"不逾越修行界底线","arc_start":"法则垄断者","arc_end":"被残卷真相反噬"}
  ],
  "themes": ["底层修士夺回问道入口"],
  "world_rules": ["灵气化形为修行资源，但过度汲取会导致灵脉枯竭与天地反噬", "天道法则具象化为修行规则，违反者将遭本源之力反噬", "传承残卷记录旧天道垄断的规则漏洞"],
  "style_rules": ["用具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"灵墟启源","objective":"建立主角成长坐标","ending_change":"南朔珩觉醒传承残卷印记"}],
    "near_chapters": [
      {"number":1,"goal":"南朔珩在偏远宗门发现上古传承残卷","expected_turn":"主角确认残卷能感应天道漏洞"},
      {"number":2,"goal":"追查残卷记录的第一处灵墟节点","expected_turn":"线索指向旧天道垄断"}
    ],
    "raw_outline":"主角发现传承残卷，卷入法则争夺，终局公开残卷并改写修行入口。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(
        !outcome.is_ready(),
        "generic title should require repair first"
    );

    let repaired = r#"{
  "title": {
    "canonical_title": "九霄问道录",
    "candidates": ["九霄问道录", "问道九重天", "天道问道录"],
    "rationale": "九霄来自九霄云阙，问道来自主角终局建立修行体系，天道来自被篡改的世界规则。",
    "source": "llm_contract"
  }
}"#;
    let repaired_outcome =
        super::super::submit_pending_contract_title_metadata_repair(&mut draft, repaired);

    let repaired_outcome = repaired_outcome.expect("repairable title patch outcome");
    assert!(repaired_outcome.is_ready());
    assert!(repaired_outcome.committed);
    assert!(draft.pending_contract_candidate.is_none());
    let retained = draft
        .current_contract
        .as_ref()
        .and_then(|value| {
            super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
        })
        .expect("ready title repair must retain the complete contract");
    assert_eq!(retained.characters.len(), 2);
    assert_eq!(retained.world_rules.len(), 3);
    assert!(!retained.outline.volumes.is_empty());
    assert!(!retained.outline.near_chapters.is_empty());
}

#[test]
fn generic_title_metadata_repair_does_not_commit_without_concrete_rationale() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-rejects-generic",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "都市传奇",
    "candidates": ["都市传奇"],
    "rationale": "书名来自主角在都市中逆袭的终局方向，结合都市爽文的核心爽点和世界观意象",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市爽文",
  "brief": "底层青年在都市暗面中靠智慧和关键证据完成逆袭。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "城市核心资源被少数人掌控，主角从底层入口查到账册黑幕。",
  "ending": {
    "desired_resolution": "主角公开资源账册，打破暗面垄断并建立新的分配秩序。",
    "final_state": "普通人获得进入核心资源系统的公开通道。",
    "must_resolve": ["资源账册黑幕"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从底层自保者成长为敢公开账册的破局者。",
  "world_imagery": "雨夜天桥、资源账册、旧城会所、核心通道。",
  "main_causal_spine": "底层入口引出资源账册黑幕，主角追查证据，终局公开账册并改写分配规则。",
  "characters": [
    {"canonical_name":"程栖棠","role":"主角","desire":"打破资源垄断","fear":"家人被暗面势力牵连","bottom_line":"不牺牲普通人换取上位","arc_start":"底层自保者","arc_end":"公开规则的破局者"},
    {"canonical_name":"钟砚澜","role":"关键对手","desire":"维持资源垄断","fear":"账册公开","bottom_line":"不允许底层越过旧秩序","arc_start":"垄断维护者","arc_end":"被新规则审判"}
  ],
  "themes": ["底层选择权", "资源垄断与公开规则"],
  "world_rules": ["资源通道必须经由账册授权", "旧城会所掌控隐性评级"],
  "style_rules": ["用具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"旧城账册","objective":"主角找到第一份资源账册","ending_change":"确认暗面评级存在"}],
    "near_chapters": [
      {"number":1,"goal":"程栖棠在雨夜天桥拿到第一份资源账册","expected_turn":"确认底层失败不是偶然"},
      {"number":2,"goal":"追查旧城会所的隐性评级","expected_turn":"线索指向核心通道"}
    ],
    "raw_outline":"主角从旧城入口追查资源账册，终局公开账册并改写城市资源规则。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(!outcome.is_ready());
    let repaired = r#"{
  "title": {
    "canonical_title": "都市传奇",
    "candidates": ["都市传奇", "都市逆袭", "巅峰人生"],
    "rationale": "书名来自主角在都市中逆袭的终局方向，结合都市爽文的核心爽点和世界观意象",
    "source": "llm_contract"
  }
}"#;

    let repaired_outcome =
        super::super::submit_pending_contract_title_metadata_repair(&mut draft, repaired);
    let repaired_outcome = repaired_outcome.expect("repairable title patch outcome");
    assert!(
        !repaired_outcome.committed,
        "{:?}",
        repaired_outcome.gate.actionable_issues()
    );
    assert!(
        repaired_outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("书名") && issue.contains("命名理由")),
        "{:?}",
        repaired_outcome.gate.actionable_issues()
    );
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
}

#[test]
fn degenerate_title_metadata_repair_does_not_replace_pending_contract() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-title-repair-rejects-degenerate",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.title = "灵墟借剑帖".to_string();
    draft.fiction_title_rationale =
        "灵墟来自第一卷入口，借剑帖是主角撬开宗门血阶的关键物件，终局用它公开旧规漏洞。"
            .to_string();
    draft.fiction_premise =
        "外门弟子在灵墟试炼中拿到借剑帖，发现宗门血阶正在吞掉底层命格。".to_string();
    draft.fiction_ending_direction = "主角公开血阶账册并改写外门晋升规则。".to_string();
    draft.fiction_protagonist_arc = "从外门求生者成长为敢撬开旧规的破局者。".to_string();
    draft.fiction_world_imagery = "灵墟、借剑帖、血阶、命格玉简。".to_string();
    draft.fiction_main_causal_spine =
        "灵墟试炼引出血阶异常，追查命格玉简，终局公开账册改写规则。".to_string();
    draft.fiction_characters = vec![
            "name: 段曜野; role: 主角; desire: 查清血阶真相; fear: 命格被献祭; bottom_line: 不献祭无辜修士; arc_start: 外门求生者; arc_end: 破局者".to_string(),
            "name: 辛衡珩; role: 关键对手; desire: 维护血阶旧规; fear: 血阶真相公开; bottom_line: 不让外门越过宗门阶序; arc_start: 执令者; arc_end: 被证据逼到台前".to_string(),
        ];
    draft.fiction_themes = vec!["底层修士夺回晋升入口".to_string()];
    draft.fiction_world_rules = vec![
        "血阶越高，越会抽取底层修士命格。".to_string(),
        "命格玉简只能记录真实献祭痕迹。".to_string(),
    ];
    draft.fiction_outline =
            "第一卷《灵墟借剑》：主角拿到借剑帖并发现血阶异常；卷尾变化：确认命格玉简记录献祭痕迹。\n第1章 本章目标：段曜野进入灵墟试炼；预期转折：借剑帖响应血阶裂纹。"
                .to_string();
    fill_complete_fiction_contract_v2(&mut draft);
    let mut contract = super::super::strong_novel_contract_from_creation_draft(&draft);
    contract.normalize();
    draft.pending_contract_candidate = Some(serde_json::json!({
        "raw_preview": "pending good contract",
        "normalized": serde_json::to_value(&contract).expect("contract value"),
        "issues": ["书名 metadata 需要修复"],
        "created_at": chrono::Utc::now().to_rfc3339()
    }));
    let pending_before = draft.pending_contract_candidate.clone();

    let repaired = r#"{
  "title": {
    "canonical_title": "祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝",
    "candidates": ["祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝"],
    "rationale": "祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝祝",
    "source": "llm_contract"
  }
}"#;

    let outcome = super::super::submit_pending_contract_title_metadata_repair(&mut draft, repaired)
        .expect("boundary rejection outcome");
    assert!(!outcome.committed);
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("连续重复退化")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
    assert_eq!(draft.pending_contract_candidate, pending_before);
}

#[test]
fn creation_contract_status_reports_latest_pending_issues() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-status-latest-pending-issues",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.pending_contract_candidate = Some(serde_json::json!({
        "issues": [
            "ContractBlocker: 书名缺少读者钩子，虽然可能能解释，但不像会让人想点开的作品名"
        ]
    }));

    let status = crate::tool::writing::session_surface::creation_contract_status_for_draft(
        Some(&draft),
        None,
    )
    .expect("status");
    let benshu_state::TaskStatus::Blocked { reason } = status else {
        panic!("drafting contract should report blocked status");
    };
    assert!(reason.contains("书名和命名理由"), "{reason}");
    assert!(
            !reason.contains("角色权威表"),
            "status should not report stale outer draft gaps when pending candidate has latest issues: {reason}"
        );
}

#[test]
fn local_title_metadata_repair_does_not_commit_creative_title_without_llm_patch() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-local-title-repair",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
	  "title": {
	    "canonical_title": "霓虹灯下的旧咒",
	    "rationale": "旧咒指代世界观中被现代文明掩盖的古老规则，霓虹灯象征都市繁华与超自然力量的视觉冲突。",
	    "source": "llm_contract"
	  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "城市规划员发现建筑布局是一套封印阵法。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "城市规划本身是巨大封印阵法，扩张触碰地脉节点后古老咒力失控。",
  "ending": {
    "desired_resolution": "主角重新排列城市核心节点，把失控咒力转成稳定能源。",
    "final_state": "暗处流动的能量被纳入有序管理，旧咒碑\\n记录守门人的代价。",
    "must_resolve": ["城市地脉暴走", "古老咒力对现代秩序的冲击"],
    "allowed_open_questions": ["其他城市是否也有隐秘规则"]
  },
  "protagonist_arc": "从普通城市规划员成长为平衡地脉与现代秩序的守门人。",
  "world_imagery": "雨夜街道、地脉节点、旧咒碑、余烬街区、城市核心阵眼。",
  "main_causal_spine": "发现规划异常，追查地脉失控，阻止破坏者，终局重排城市阵眼。",
  "characters": [
    {"canonical_name":"陆沉","role":"主角","desire":"保护城市生活秩序","fear":"日常生活破碎","bottom_line":"不牺牲无辜市民","arc_start":"迷茫规划员","arc_end":"城市守门人"},
    {"canonical_name":"苏清月","role":"导师/盟友","desire":"传承旧咒知识","fear":"知识流失导致秩序崩塌","bottom_line":"不主动牺牲凡人","arc_start":"知识传承者","arc_end":"合作者"},
    {"canonical_name":"雷厉","role":"对手/压力源","desire":"利用咒力建立新阶级","fear":"被旧规则束缚","bottom_line":"可以牺牲秩序","arc_start":"破坏者","arc_end":"失败者"}
  ],
  "themes": ["秩序与混乱", "现代文明与古老规则"],
  "world_rules": ["城市建筑几何决定能量流向", "地标是能量节点", "旧咒碑可记录阵眼变化"],
  "style_rules": ["具体场景推进", "保持中文"],
  "must_avoid": ["不要科幻芯片矩阵", "不要角色改名"],
  "outline": {
    "volumes": [{"title":"旧碑入城","objective":"主角发现旧咒碑与城市阵眼","ending_change":"确认地脉暴走并非偶然"}],
    "near_chapters": [
      {"number":1,"goal":"陆沉在雨夜工地发现旧咒碑","expected_turn":"他确认规划异常会触发地脉失控"},
      {"number":2,"goal":"苏清月解释旧咒碑记录的城市阵眼","expected_turn":"陆沉获得守门人线索"},
      {"number":3,"goal":"雷厉破坏第一个地脉节点","expected_turn":"陆沉被迫公开介入"}
    ],
    "raw_outline": "发现旧咒碑，追查地脉节点，终局重排城市阵眼。"
  }
}"#;

    let first = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(!first.is_ready(), "{:?}", first.gate.actionable_issues());
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());

    let repaired = super::super::repair_pending_contract_metadata_locally(&mut draft);
    if let Some(outcome) = repaired {
        assert!(
                !outcome.is_ready(),
                "local metadata repair must not invent a creative book title without a valid LLM-provided title candidate"
            );
    }
    assert!(
            draft.current_contract.is_none(),
            "local metadata repair must leave the contract pending when the only blocker is a missing or invalid creative title"
        );
    assert!(draft.pending_contract_candidate.is_some());
}

#[test]
fn plan_like_title_candidate_stays_pending_for_typed_repair() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-plan-title",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "旧街灵脉重塑计划",
    "candidates": ["旧街灵脉重塑计划", "旧街灵脉井", "雨灯灵脉", "秩序仪黑幕"],
    "rationale": "书名《旧街灵脉重塑计划》取自当前故事的关键地点、物件、制度、事件或终局选择，并连接到主线/终局：主角打破秩序仪，让旧街灵脉重新流动。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "旧街青年发现城市灵脉被秩序仪垄断。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "秩序仪把城市灵力锁在中心区，旧街居民被迫承担枯竭代价。",
  "ending": {"desired_resolution":"主角拆解秩序仪并让旧街灵脉恢复自然流动。","final_state":"旧街获得新的灵力入口。","must_resolve":["秩序仪垄断"],"allowed_open_questions":[]},
  "protagonist_arc": "从只想自保的旧街青年，成长为愿意公开真相的守街人。",
  "world_imagery": "旧街雨灯、灵脉井、秩序仪。",
  "main_causal_spine": "旧街灵脉枯竭引出秩序仪黑幕，主角追查证据，终局拆解垄断。",
  "characters": [
    {"canonical_name":"顾知遥","role":"主角","desire":"恢复旧街灵脉","fear":"旧街彻底枯竭","bottom_line":"不牺牲居民换取力量","arc_start":"自保者","arc_end":"守街人"},
    {"canonical_name":"沈知晚","role":"关键对手","desire":"守住秩序仪权限","fear":"垄断真相公开","bottom_line":"不允许旧街证据进入中心区","arc_start":"秩序维护者","arc_end":"被迫面对证据"}
  ],
  "themes": ["公平分配", "代价与选择"],
  "world_rules": ["灵脉必须经由秩序仪登记才能进入中心区。"],
  "style_rules": ["具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"旧街借灯","objective":"主角找到灵脉井证据","ending_change":"确认秩序仪垄断旧街灵力"}],
    "near_chapters": [
      {"number":1,"goal":"顾知遥在雨灯下发现灵脉井异常","expected_turn":"旧街灵力被抽走"},
      {"number":2,"goal":"顾知遥借符灯进入中心区边界","expected_turn":"发现秩序仪记录"},
      {"number":3,"goal":"顾知遥与沈知晚第一次正面交锋","expected_turn":"证据被夺走但真相扩大"}
    ],
    "raw_outline":"旧街青年追查灵脉枯竭，终局拆解秩序仪垄断。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(
        !outcome.is_ready(),
        "{:?}",
        outcome.gate.actionable_issues()
    );
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("计划") || issue.contains("书名")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn generic_character_and_relationship_contract_stays_pending() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-generic-contract",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"{
  "title": {
    "canonical_title": "夜校借灵",
    "candidates": ["夜校借灵", "雨巷符灯", "旧证入场"],
    "rationale": "夜校来自主角被迫参加补考的起点，借灵来自终局公开考试吞噬运势的规则，标题指向读者期待的考试爽点和规则反转。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "旧城区旁听生卷入灵能考试黑幕。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "灵能考试决定城市阶层，旁听生发现考试会吞噬普通人的运势。",
  "ending": {"desired_resolution":"主角公开灵能考试黑幕并重写城市晋级规则。","final_state":"旧城区学生获得公平考试入口。","must_resolve":["灵能考试黑幕"],"allowed_open_questions":[]},
  "protagonist_arc": "从只想保住旁听名额，成长为愿意承担代价的规则改写者。",
  "world_imagery": "夜校考场、借灵证、旧城区雨巷。",
  "main_causal_spine": "失败补考引出晋级黑幕，主角追查运势吞噬证据，终局公开证据改写规则。",
  "characters": [
    {"canonical_name":"许闻桥","role":"主角","desire":"通过灵能考试改变命运","fear":"再次失去考试资格","bottom_line":"不牺牲同学换取晋级","arc_start":"旁听生","arc_end":"规则改写者"},
    {"canonical_name":"商砚衡","role":"重要角色","desire":"维护与主角目标冲突的秩序或利益","fear":"自身秩序被主角选择改写","bottom_line":"反对必须由清晰动机推动","arc_start":"监考者","arc_end":"被迫面对旧城区证据"}
  ],
  "themes": ["公平晋级", "代价与选择"],
  "world_rules": ["灵能考试会转移考生运势"],
  "style_rules": ["具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "outline": {
    "volumes": [{"title":"夜校借灵","objective":"主角拿到借灵证并发现考场异常","ending_change":"主角确认考试吞噬运势"}],
    "near_chapters": [
      {"number":1,"goal":"许闻桥被迫参加夜校补考","expected_turn":"发现借灵证会记录运势损耗"},
      {"number":2,"goal":"主角找到旧城区考试证据","expected_turn":"证据指向监考系统"},
      {"number":3,"goal":"主角第一次反向利用借灵规则","expected_turn":"救下同学但暴露身份"}
    ],
    "raw_outline":"旁听生借灵入场，追查考试吞噬运势，终局公开旧城区证据重写晋级规则。"
  },
  "structured": {
    "relationship_ledger": [{
      "characters": ["许闻桥"],
      "arc_type": "relationship",
      "relationship_type": "主角核心关系",
      "stage": "建立关系",
      "next_expected_stage": "产生变化"
    }]
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.is_ready());
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("通用兜底动机")
                || issue.contains("缺少欲望锚点")
                || issue.contains("缺少底线锚点")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn valid_fenced_json_contract_commits_as_current_contract() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-ready",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"```json
{
  "title": {
    "canonical_title": "夜校灵轨",
    "rationale": "夜校对应主角被迫进入的城市考试入口，灵轨对应终局中他接通城市灵脉轨道、守住普通人选择权的关键行动。",
    "source": "llm_contract"
  },
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "普通学生卷入城市灵脉复苏和夜校晋级考试。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "premise": "旧城夜校下方的灵脉轨道重启，普通学生必须在考试晋级和城市危机之间做选择。",
  "ending": {
    "desired_resolution": "秦知安接通灵轨，公开夜校真相，守住城市也守住自己的普通生活。",
    "final_state": "夜校成为普通人也能进入的守城入口。",
    "must_resolve": ["灵轨失控原因", "夜校考试黑幕"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只想通过考试的旁观者，成长为愿意承担城市代价的守门人。",
  "world_imagery": "雨夜夜校、地下灵轨、旧城站台",
  "main_causal_spine": "灵轨复苏引发考试异常，主角追查夜校黑幕，最终以个人选择修复城市秩序。",
  "characters": [
    {
      "canonical_name": "秦知安",
      "role": "主角",
      "desire": "通过夜校考试改变生活",
      "fear": "再次被城市边缘化",
      "bottom_line": "不牺牲无辜同学换取晋级",
      "arc_start": "旁观自保",
      "arc_end": "主动守城"
    },
    {
      "canonical_name": "梁棠",
      "role": "对手",
      "desire": "垄断夜校晋级名额",
      "fear": "灵轨真相公开",
      "bottom_line": "不让底层学生越过自己",
      "arc_start": "操控考试",
      "arc_end": "被迫面对公开审查"
    }
  ],
  "themes": ["普通人的选择权", "晋级制度的代价"],
  "world_rules": ["灵轨只能由承担代价的人接通", "夜校考试会放大考生最害怕失去的东西"],
  "style_rules": ["用具体场景推进", "考试和城市异常交替推进"],
  "must_avoid": ["不要让角色无解释改名", "不要用摘要替代正文"],
  "structured": {"narration_contract":{"pov":"第三人称有限视角"}},
  "outline": {
    "volumes": [
      {"title":"雨夜入校","objective":"主角进入夜校并发现灵轨异常","ending_change":"主角被迫成为灵轨见证者"}
    ],
    "near_chapters": [
      {"number":1,"goal":"主角在雨夜补考中听见地下灵轨启动","expected_turn":"他发现考试题目会改写现实"},
      {"number":2,"goal":"主角进入旧城夜校并遇见梁棠","expected_turn":"梁棠展示晋级名额的垄断规则"},
      {"number":3,"goal":"主角第一次接触灵轨代价","expected_turn":"他必须选择救同学还是保住成绩"}
    ],
    "raw_outline":"第一卷：雨夜入校；第二卷：灵轨追查；第三卷：公开夜校真相。"
  }
}
```"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    let normalized =
        super::super::creation_contract_normalizer::normalize_creation_contract_boundary(raw)
            .expect("normalized contract");
    let parsed = super::super::NovelCreationContract::parse_json_boundary(&normalized.json)
        .expect("parsed contract");
    assert_eq!(parsed.title.canonical_title, "夜校灵轨");
    assert_eq!(
        parsed.protagonist_arc,
        "从只想通过考试的旁观者，成长为愿意承担城市代价的守门人。"
    );
    assert!(parsed
        .characters
        .iter()
        .any(|character| character.canonical_name == "秦知安" && character.role == "主角"));
    assert!(
        parsed.validate().is_ready(),
        "{:?}",
        parsed.validate().issues
    );
    assert!(
        super::super::contract_boundary_quality_issues(&draft, &normalized.json).is_empty(),
        "{:?}",
        super::super::contract_boundary_quality_issues(&draft, &normalized.json)
    );

    assert!(outcome.is_ready(), "{:?}", outcome.gate.actionable_issues());
    assert!(draft.current_contract.is_some());
    assert!(draft.pending_contract_candidate.is_none());
    assert_eq!(draft.title, "夜校灵轨");
    assert!(draft
        .fiction_characters
        .iter()
        .any(|line| line.contains("role: 主角") || line.contains("role:主角")));
    let governed_characters = draft.fiction_characters.join("\n");
    assert!(
        governed_characters.contains("name:")
            && !governed_characters.contains("name: 林凡")
            && !governed_characters.contains("name: 陆沉"),
        "{governed_characters}"
    );
    assert!(
        governed_characters.contains("role: 主角")
            && (governed_characters.contains("role: 对手")
                || governed_characters.contains("role: 关键对手")),
        "{governed_characters}"
    );
    assert!(
            !draft.fiction_outline.contains("本章目标")
                && !draft.fiction_outline.contains("预期转折")
                && !draft.fiction_outline.contains("卷尾变化"),
            "ready draft outline should stay a prose summary; structured chapter/volume data belongs to the typed contract view: {}",
            draft.fiction_outline
        );
    let contract_view = super::super::render_creation_draft_contract_view(&draft, true);
    assert!(
        contract_view.contains("分卷规划")
            && contract_view.contains("近期章节包")
            && contract_view.contains("雨夜入校")
            && contract_view.contains("第1章 本章目标"),
        "typed outline data should still be visible in the full contract view: {contract_view}"
    );
}

#[test]
fn natural_language_field_pack_is_preserved_for_structured_governance_repair() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-ready-natural",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = "\
1. 基本参数：语言：中文；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20；导出格式：txt。
2. 命名依据合同：终局方向：主角在旧城夜校终局接通地下灵轨，公开晋级考试黑幕，让普通学生拥有入场资格。主角弧线：从只想保住补考名额的旁听生，成长为愿意承担灵轨代价的守门人。世界观意象：雨夜夜校、地下灵轨、借灵证、旧城站台。总主线因果链：补考异常引出借灵证，借灵证暴露运势吞噬规则，终局在地下灵轨公开证据并改写晋级制度。
书名：《夜校借灵证》。
命名理由：夜校来自开局雨夜补考入口，借灵证会记录运势损耗，并在终局公开考试黑幕时成为证据。
3. 角色权威表：主角姓名：秦知安，命名依据：知安对应他从求自保到守住旧城安稳的弧线，欲望：通过夜校考试改变生活，恐惧：再次被城市边缘化，底线：不牺牲无辜同学换取晋级；对手姓名：梁棠，命名依据：棠对应其表面温和、实则维护垄断的形象，欲望：垄断夜校晋级名额，恐惧：考试黑幕公开，底线：不让底层学生越过自己。
4. 世界规则：借灵证能临时借用灵脉，但会记录并抽取考生运势；地下灵轨只能由承担代价的人接通；夜校账册只有公开证据链完整时才会重算资格。
5. 结构合同：一句话全书大纲：旧城旁听生在夜校补考中发现借灵证会吞噬运势，追查地下灵轨并最终公开考试黑幕。第一卷：雨夜入校；目标：主角进入夜校并确认借灵证异常；卷尾变化：主角成为灵轨见证者。第二卷：灵轨追查；目标：主角追查运势账册；卷尾变化：主角暴露身份。第三卷：旧城开门；目标：公开证据并改写考试入口；卷尾变化：夜校入口向普通学生开放。
6. 近期章节包：第01章《雨夜补考》：本章目标：秦知安在雨夜补考中第一次听见地下灵轨启动。
第02章《借灵证》：本章目标：秦知安发现借灵证会记录运势损耗。
第03章《旧站名单》：本章目标：秦知安在旧城站台找到被吞噬运势的名单。
7. 质量合同：核心主题：普通人争取选择权必须承担真实代价；叙事风格：保持近距离第三人称，以调查行动和人物选择推进；必须避免：人物漂移；英文名；用摘要替代正文；章节标题脱离本章事件。";

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.is_ready());
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("缺少可执行的结构化治理内容")));
    assert!(draft.pending_contract_candidate.is_some());
    let (draft, contract) =
        super::super::creation_draft_and_contract_with_pending_applied(&draft);
    assert_eq!(draft.title, "夜校借灵证");
    assert!(draft.pending_contract_candidate.is_none());
    assert_eq!(draft.target_units, Some(50000));
    assert_eq!(draft.chapter_unit_target, Some(2500));
    assert!(draft
        .fiction_characters
        .iter()
        .any(|line| line.contains("role: 主角") || line.contains("role:主角")));
    let governed_characters = draft.fiction_characters.join("\n");
    assert!(
        governed_characters.contains("name:")
            && !governed_characters.contains("name: 林凡")
            && !governed_characters.contains("name: 陆沉"),
        "{governed_characters}"
    );
    assert!(
        governed_characters.contains("role: 主角")
            && (governed_characters.contains("role: 对手")
                || governed_characters.contains("role: 关键对手")),
        "{governed_characters}"
    );
    assert!(
        contract.outline.near_chapters.len() >= 3,
        "{:?}",
        contract.outline.near_chapters
    );
}

#[test]
fn natural_language_field_pack_rejects_conflicting_outline_book_title() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-conflicting-title",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = "\
书名：夜校借灵证
书名理由：夜校来自主角进入规则的第一地点，借灵证是考试黑幕的关键物件，终局公开借灵证账册后才让普通学生获得入场资格。
语言：中文
题材：都市玄幻
简述：旧城区旁听生卷入灵能考试黑幕，追查借灵证吞噬运势的真相。
故事前提：灵能考试决定城市阶层，旧城区学生发现夜校补考会吞噬普通人的运势。
终局方向：主角公开借灵证账册，改写夜校晋级规则，让普通学生获得公平入口。
终局状态：夜校入口向普通学生开放，旧城区不再被借灵证吞噬运势。
主角弧线：从只想保住补考名额的旁听生成长为愿意承担灵轨代价的规则改写者。
世界观意象：雨夜夜校、地下灵轨、借灵证、旧城站台。
总主线因果链：补考异常引出借灵证，借灵证暴露运势吞噬规则，主角追查地下灵轨账册并在终局公开证据改写晋级制度。
角色权威表：
姓名：秦知安，角色：主角，欲望：通过夜校考试改变生活，恐惧：再次被城市边缘化，底线：不牺牲无辜同学换取晋级，弧线起点：只想保住补考名额，弧线终点：公开证据改写规则。
姓名：梁棠，角色：对手，欲望：垄断夜校晋级名额，恐惧：考试黑幕公开，底线：维护制度但不亲手毁掉考试系统。
姓名：宋晚照，角色：同伴，欲望：救回被借灵证夺走运势的哥哥，恐惧：自己也被夜校抹除，底线：不伪造证据。
核心主题：公平晋级；代价与选择
世界规则：借灵证能临时借用灵脉但会记录并抽取考生运势；地下灵轨只能由承担代价的人接通。
叙事风格：具体场景推进；保持中文人物对话；每章有明确冲突。
必须避免：不要角色无解释改名；不要英文名；不要用摘要替代正文。
大纲：《都市妖瞳》以秦知安在夜校补考中发现借灵证异常为起点，最终公开账册改写晋级制度。
近期章节包：
第01章《雨夜补考》：本章目标：秦知安在雨夜补考中第一次听见地下灵轨启动。
第02章《借灵证》：本章目标：秦知安发现借灵证会记录运势损耗。
第03章《旧站名单》：本章目标：秦知安在旧城站台找到被吞噬运势的名单。
情感承诺：主角在被边缘化的恐惧中学会承担代价。
关系线：秦知安与宋晚照从互相试探到共同守住旧城入口。
资源体系：借灵证与运势账册。
社会秩序：夜校考试垄断晋级资格。
叙事口径：第三人称有限视角。
兑现矩阵：借灵证账册在终局公开；地下灵轨成为普通学生入口。";

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.is_ready());
    assert!(
        outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("不一致的标题《都市妖瞳》")),
        "{:?}",
        outcome.gate.actionable_issues()
    );
}

#[test]
fn complete_visible_field_pack_still_requires_structured_governance() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-ready-field-pack",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = "\
标准小说合同
书名：夜校借灵证
书名候选：夜校借灵证；旧站补考；雨夜入场券
书名理由：夜校来自主角进入规则的第一地点，借灵证是考试黑幕的关键物件，终局公开借灵证账册后才让普通学生获得入场资格。
语言：中文
题材：都市玄幻
简述：旧城区旁听生卷入灵能考试黑幕，追查借灵证吞噬运势的真相。
故事前提：灵能考试决定城市阶层，旧城区学生发现夜校补考会吞噬普通人的运势。
终局方向：主角公开借灵证账册，改写夜校晋级规则，让普通学生获得公平入口。
终局状态：夜校入口向普通学生开放，旧城区不再被借灵证吞噬运势。
主角弧线：从只想保住补考名额的旁听生成长为愿意承担灵轨代价的规则改写者。
世界观意象：雨夜夜校、地下灵轨、借灵证、旧城站台。
总主线因果链：补考异常引出借灵证，借灵证暴露运势吞噬规则，主角追查地下灵轨账册并在终局公开证据改写晋级制度。
角色权威表：
姓名：秦知安，角色：主角，欲望：通过夜校考试改变生活，恐惧：再次被城市边缘化，底线：不牺牲无辜同学换取晋级，弧线起点：只想保住补考名额，弧线终点：公开证据改写规则。
姓名：梁棠，角色：对手，欲望：垄断夜校晋级名额，恐惧：考试黑幕公开，底线：维护制度但不亲手毁掉所有学生。
姓名：宋晚照，角色：同伴，欲望：救回被借灵证夺走运势的哥哥，恐惧：自己也被夜校抹除，底线：不伪造证据。
核心主题：公平晋级；代价与选择
世界规则：借灵证能临时借用灵脉但会记录并抽取考生运势；地下灵轨只能由承担代价的人接通。
叙事风格：具体场景推进；保持中文人物对话；每章有明确冲突。
必须避免：不要角色无解释改名；不要英文名；不要用摘要替代正文。
近期章节包：
第01章《雨夜补考》：本章目标：秦知安在雨夜补考中第一次听见地下灵轨启动。
第02章《借灵证》：本章目标：秦知安发现借灵证会记录运势损耗。
第03章《旧站名单》：本章目标：秦知安在旧城站台找到被吞噬运势的名单。";

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(!outcome.is_ready());
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("缺少可执行的结构化治理内容")));
    assert!(draft.pending_contract_candidate.is_some());
    let (draft, contract) =
        super::super::creation_draft_and_contract_with_pending_applied(&draft);
    assert_eq!(draft.title, "夜校借灵证");
    assert!(draft.pending_contract_candidate.is_none());
    assert!(draft
        .fiction_characters
        .iter()
        .any(|line| line.contains("role: 主角") || line.contains("role:主角")));
    let governed_characters = draft.fiction_characters.join("\n");
    assert!(
        governed_characters.contains("name:")
            && !governed_characters.contains("name: 林凡")
            && !governed_characters.contains("name: 陆沉"),
        "{governed_characters}"
    );
    assert!(
        governed_characters.contains("role: 主角")
            && (governed_characters.contains("role: 对手")
                || governed_characters.contains("role: 关键对手")),
        "{governed_characters}"
    );
    assert!(
        draft
            .fiction_world_rules
            .iter()
            .any(|rule| rule.contains("借灵证")),
        "{:?}",
        draft.fiction_world_rules
    );
    assert!(
        draft
            .fiction_themes
            .iter()
            .any(|theme| theme.contains("公平晋级")),
        "{:?}",
        draft.fiction_themes
    );
    assert!(
        contract.outline.near_chapters.len() >= 3,
        "{:?}",
        contract.outline.near_chapters
    );
    assert!(
        contract
            .structured
            .emotional_contract
            .emotional_beats
            .is_empty()
            || !contract
                .structured
                .emotional_contract
                .primary_emotion
                .is_empty()
    );
    assert!(
        contract.structured.relationship_ledger.is_empty(),
        "normalization must not invent relationship governance: {:?}",
        contract.structured.relationship_ledger
    );
    assert!(
        contract.structured.emotional_state_ledger.is_empty(),
        "normalization must not invent emotional governance: {:?}",
        contract.structured.emotional_state_ledger
    );
}

#[test]
fn chinese_key_json_contract_commits_through_typed_normalizer() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-chinese-json-contract",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"下面是合同草案：
{
  "书名": "雨站借灵簿",
  "书名理由": "雨站来自主角第一次发现运势账册的地点，借灵簿来自黑幕证据，终局用它公开考试规则的爽点。",
  "书名候选": ["雨站借灵簿", "旧城补考簿", "灵轨入场券"],
  "语言": "中文",
  "题材": "都市玄幻",
  "简述": "旧城区学生在补考夜发现借灵簿吞噬运势，并追查地下灵轨真相。",
  "总字数": 50000,
  "每章字数": 2500,
  "故事前提": "灵能考试决定城市阶层，补考系统把普通学生的运势写进借灵簿。",
  "终局方向": "主角公开借灵簿和灵轨账册，迫使夜校向普通学生开放公平入口。",
  "终局状态": "旧城区学生拥有公开补考资格，借灵簿被封存为证据。",
  "主角弧线": "从只想保住补考资格的旁听生成长为敢公开黑幕的规则改写者。",
  "世界观意象": "雨夜站台、借灵簿、地下灵轨、旧城夜校。",
  "总主线因果链": "补考异常引出借灵簿，借灵簿指向地下灵轨账册，终局公开证据改写晋级规则。",
  "角色权威表": [
    {"姓名":"秦知安","角色":"主角","欲望":"通过夜校补考改变生活","恐惧":"再次被城市边缘化","底线":"不牺牲无辜同学换取晋级","弧线起点":"只想保住补考名额","弧线终点":"公开证据改写规则"},
    {"姓名":"梁棠","角色":"关键对手","欲望":"垄断夜校晋级名额","恐惧":"考试黑幕公开","底线":"维护制度但不亲手毁掉所有学生"},
    {"姓名":"宋晚照","角色":"同伴","欲望":"救回被借灵簿夺走运势的哥哥","恐惧":"自己也被夜校抹除","底线":"不伪造证据"}
  ],
  "核心主题": ["公平晋级", "代价与选择"],
  "世界规则": ["借灵簿能临时借用灵脉但会记录并抽取考生运势", "地下灵轨只能由承担代价的人接通"],
  "叙事风格": ["具体场景推进", "保持中文人物对话"],
  "必须避免": ["不要角色无解释改名", "不要英文名", "不要用摘要替代正文"],
  "structured": {"narration_contract":{"pov":"第三人称有限视角"}},
  "全书大纲": "秦知安进入夜校补考，追查借灵簿和地下灵轨，终局公开证据改写晋级规则。",
  "分卷": [
    {"卷名":"雨夜入校","阶段目标":"主角进入夜校并确认借灵簿异常","卷尾变化":"主角成为灵轨见证者"}
  ],
  "近期章节包": [
    {"number":1,"本章目标":"秦知安在雨夜补考中第一次听见地下灵轨启动","预期转折":"秦知安意识到借灵簿在记录自己的运势"},
    {"number":2,"本章目标":"秦知安发现借灵簿会记录运势损耗","预期转折":"宋晚照交出哥哥失踪前的借灵页"},
    {"number":3,"本章目标":"秦知安在旧城站台找到被吞噬运势的名单","预期转折":"梁棠第一次出手封锁站台"}
  ]
}
以上请确认。"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(outcome.is_ready(), "{:?}", outcome.gate.actionable_issues());
    assert_eq!(draft.title, "雨站借灵簿");
    assert!(draft.current_contract.is_some());
    assert!(draft
        .fiction_characters
        .iter()
        .any(|line| line.contains("role: 主角") || line.contains("role:主角")));
    let governed_characters = draft.fiction_characters.join("\n");
    assert!(
        governed_characters.contains("name:")
            && !governed_characters.contains("name: 林凡")
            && !governed_characters.contains("name: 陆沉"),
        "{governed_characters}"
    );
    assert!(
        governed_characters.contains("role: 主角")
            && (governed_characters.contains("role: 对手")
                || governed_characters.contains("role: 关键对手")),
        "{governed_characters}"
    );
    let contract = draft
        .current_contract
        .as_ref()
        .and_then(|value| {
            super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
        })
        .expect("current typed contract");
    assert_eq!(contract.outline.near_chapters.len(), 3);
    assert_eq!(contract.outline.volumes.len(), 1);
    assert!(
        contract
            .world_rules
            .iter()
            .any(|rule| rule.contains("借灵簿")),
        "{:?}",
        contract.world_rules
    );
}

#[test]
fn patch_scope_diagnostic_does_not_drag_stage_back_to_characters() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-stage-scope-diagnostic",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_premise = "旧城灵脉考试决定阶层，旁听生追查命格账本。".to_string();
    draft.fiction_ending_direction = "主角公开命格账本，改写晋级制度。".to_string();
    draft.fiction_world_imagery = "雨夜高架、地下灵脉、旧校钟楼。".to_string();
    draft.fiction_main_causal_spine =
        "补考异常引出命格账本，账本暴露阶层吞噬规则，终局公开证据改写制度。".to_string();
    draft.fiction_characters = vec![
            "姓名：秦知安，角色：主角，欲望：通过灵脉考试，恐惧：被城市抹除，底线：不牺牲无辜，弧线起点：旁听生，弧线终点：规则改写者。".to_string(),
            "姓名：梁棠，角色：对手，欲望：垄断晋级名额，恐惧：黑幕公开，底线：维护制度。".to_string(),
            "姓名：宋晚照，角色：同伴，欲望：救回哥哥，恐惧：自己被抹除，底线：不伪造证据。".to_string(),
        ];
    draft.fiction_outline =
        "第一卷《旧校钟》：补考异常暴露命格账本；卷尾变化：主角拿到第一份证据。".to_string();

    let mut issues = super::super::issue::ContractIssueList::from_messages(
        "contract.patch_scope",
        super::super::issue::ContractIssueKind::Other,
        "typed_patch",
        vec![
            "typed patch 作用域校验未通过：character_patch 至少需要 1 个非主角关键角色"
                .to_string(),
            "character_patch 角色 :\"陆玄辰 缺少欲望/恐惧/底线/弧线字段".to_string(),
        ],
    );
    issues.set_disposition(super::super::issue::ContractIssueDisposition::Diagnostic);
    issues.push_issue(super::super::issue::ContractIssue::new(
        "contract.world_rules",
        super::super::issue::ContractIssueKind::Governance,
        super::super::issue::ContractIssueDisposition::Repairable,
        super::super::issue::ContractIssueEvidence::new("world_rules", "missing"),
        "ContractBlocker: 小说合同缺少世界规则",
    ));
    let stage = super::super::staged_prompts::select_contract_completion_stage(&draft, &issues);

    assert_eq!(
        stage,
        super::super::staged_prompts::ContractCompletionStage::Governance
    );
}

#[test]
fn missing_outline_takes_precedence_over_character_quality_issue_when_cast_exists() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-stage-plot-before-character-quality",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_premise =
        "草根青年拿到城市资源账本，发现上层家族用规则吞掉普通人的机会。".to_string();
    draft.fiction_ending_direction = "主角公开资源账本，夺回被垄断的晋升入口。".to_string();
    draft.fiction_world_imagery = "旧城天台、商务楼暗账、午夜地铁、资源牌照。".to_string();
    draft.fiction_main_causal_spine =
        "偶得账本引出资源垄断，主角逐层反击，终局让隐藏规则公开失效。".to_string();
    draft.fiction_characters = vec![
            "姓名：程岑宁，角色：主角，欲望：夺回晋升入口，恐惧：再次被规则吞掉，底线：不牺牲普通人机会，弧线起点：底层执行员，弧线终点：规则改写者。".to_string(),
            "姓名：裴知序，角色：同伴，欲望：保住家人诊所，恐惧：证据被毁，底线：不伪造账本。".to_string(),
            "姓名：沈晴澜，角色：对手，欲望：维护家族牌照垄断，恐惧：暗账公开，底线：维护旧秩序。".to_string(),
        ];
    draft.fiction_outline.clear();
    let mut issues = super::super::issue::ContractIssueList::from_messages(
        "contract.outline",
        super::super::issue::ContractIssueKind::Plot,
        "outline",
        vec![
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string(),
            "小说合同尚未形成逐章规划或分卷/阶段大纲".to_string(),
        ],
    );
    issues.push_issue(super::super::issue::ContractIssue::new(
        "contract.character_authority",
        super::super::issue::ContractIssueKind::Characters,
        super::super::issue::ContractIssueDisposition::Repairable,
        super::super::issue::ContractIssueEvidence::new("characters", "supporting missing"),
        "小说合同角色权威表缺少非主角关键角色、关系对象或对手，不能支撑冲突和关系线",
    ));

    let stage = super::super::staged_prompts::select_contract_completion_stage(&draft, &issues);

    assert_eq!(
        stage,
        super::super::staged_prompts::ContractCompletionStage::Plot
    );
}

#[test]
fn missing_governance_with_existing_outline_does_not_loop_back_to_plot() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-stage-governance-before-plot-loop",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_premise = "都市旧区隐藏瞳术账册，草根青年被卷入资源争夺。".to_string();
    draft.fiction_ending_direction = "主角公开瞳术账册代价，改写地下资源秩序。".to_string();
    draft.fiction_world_imagery = "雨夜天桥、瞳术账册、地下诊所、霓虹符纹。".to_string();
    draft.fiction_main_causal_spine =
        "异瞳觉醒引出账册代价，主角追查地下势力，终局公开证据改写秩序。".to_string();
    draft.fiction_characters = vec![
            "姓名：宁望禾，角色：主角，欲望：掌控瞳术并查清账册真相，恐惧：被瞳术反噬，底线：不牺牲同伴，弧线起点：被动觉醒者，弧线终点：规则改写者。".to_string(),
            "姓名：宋岑序，角色：导师，欲望：破解瞳术传承，恐惧：资料被毁，底线：不让传承被滥用。".to_string(),
            "姓名：闻棠澜，角色：关键对手，欲望：维持地下势力秩序，恐惧：账册公开，底线：维护旧秩序。".to_string(),
        ];
    draft.fiction_outline =
        "第一卷《雨桥见瞳》：完成异瞳觉醒与账册线索；卷尾变化：主角确认瞳术有记忆代价。"
            .to_string();
    let mut issues = super::super::issue::ContractIssueList::single(
        "contract.world_rules",
        super::super::issue::ContractIssueKind::Governance,
        "world_rules",
        "ContractBlocker: 小说合同缺少世界规则",
    );
    issues.extend_findings([
        super::super::issue::ContractIssue::new(
            "contract.outline.plan",
            super::super::issue::ContractIssueKind::Plot,
            super::super::issue::ContractIssueDisposition::Repairable,
            super::super::issue::ContractIssueEvidence::new("outline", "missing stage plan"),
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
        ),
        super::super::issue::ContractIssue::new(
            "contract.outline.plan",
            super::super::issue::ContractIssueKind::Plot,
            super::super::issue::ContractIssueDisposition::Repairable,
            super::super::issue::ContractIssueEvidence::new("outline", "missing outline"),
            "小说合同尚未形成逐章规划或分卷/阶段大纲",
        ),
    ]);

    let stage = super::super::staged_prompts::select_contract_completion_stage(&draft, &issues);

    assert_eq!(
        stage,
        super::super::staged_prompts::ContractCompletionStage::Governance
    );
}

#[test]
fn authority_external_character_issue_routes_back_to_character_stage() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-stage-external-character-anchor",
        "fiction",
        "写都市言情小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    draft.fiction_premise = "职场新人发现旧项目事故背后隐藏情感和商业真相。".to_string();
    draft.fiction_ending_direction =
        "主角公开证据，修复自我边界，并与关键关系对象共同选择未来。".to_string();
    draft.fiction_world_imagery = "雨夜写字楼、旧合同、玻璃会议室、江边路灯。".to_string();
    draft.fiction_main_causal_spine =
        "旧项目事故引出证据链，关系误会加深，主角查清真相并完成选择。".to_string();
    draft.fiction_characters = vec![
            "姓名：钟望晚，角色：主角，欲望：找回事业与情感主动权，恐惧：再次被旧伤束缚，底线：不牺牲人格尊严，弧线起点：被动忍让，弧线终点：主动选择。".to_string(),
            "姓名：裴桥澜，角色：关键关系对象，欲望：帮南栖安突破自我设限，恐惧：失去她，底线：不越过原则。".to_string(),
            "姓名：晏岑安，角色：关键对手，欲望：维护商业地位，恐惧：旧事公开，底线：保住体面。".to_string(),
        ];
    draft.fiction_outline =
        "第一卷《旧合同》：旧项目事故重启；卷尾变化：主角拿到第一份证据。".to_string();
    let issues = super::super::issue::ContractIssueList::single(
        "contract.character_authority",
        super::super::issue::ContractIssueKind::Characters,
        "characters",
        "ContractBlocker: 角色 `裴桥澜` 的欲望锚点引用了权威表外角色 `南栖安`",
    );

    let stage = super::super::staged_prompts::select_contract_completion_stage(&draft, &issues);

    assert_eq!(
        stage,
        super::super::staged_prompts::ContractCompletionStage::Characters
    );
}

#[test]
fn local_protagonist_name_is_projected_into_outline_text() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-governed-outline",
        "fiction",
        "写都市玄幻小说，每章2500字，至少5万字起。",
    )
    .expect("draft");
    let raw = r#"```json
{
  "title": {"canonical_title": "雨站破忆票", "rationale": "雨站来自终局公开运行的地点，忆票来自追查童年交易的关键物件，破对应主角打破记忆交易规则的爽点行动。"},
  "language": "zh-CN",
  "genre": "都市玄幻",
  "brief": "陆沉在城市记忆站寻找童年真相。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "premise": "记忆能被交易，也能被城市吞掉。",
  "ending": {"desired_resolution": "陆沉放弃个人执念，把被交易的记忆还给普通人。", "final_state": "城市记忆站公开运行。"},
  "protagonist_arc": "陆沉从执念调查者成长为愿意守住普通人记忆的人。",
  "world_imagery": "雨夜站台、记忆票据、霓虹档案柜",
  "main_causal_spine": "陆沉追查失踪记忆，发现交易规则，最终改写记忆站。",
  "characters": [
    {"canonical_name":"陆沉","role":"主角","desire":"找回童年记忆","fear":"失去自我","bottom_line":"不出卖他人记忆","arc_start":"执念调查者","arc_end":"记忆守门人"},
    {"canonical_name":"苏曼","role":"对手","desire":"控制记忆交易","fear":"秩序失控","bottom_line":"绝不删除无辜者的原始记忆来维持交易秩序","arc_start":"交易维护者","arc_end":"被迫承认代价"}
  ],
  "themes": ["记忆与身份"],
  "world_rules": ["记忆可以交易", "交易越多越接近空壳"],
  "style_rules": ["具体场景推进"],
  "must_avoid": ["不要角色改名"],
  "structured": {"narration_contract":{"pov":"第三人称有限视角"}},
  "outline": {
    "volumes": [{"title":"雨站开闸","objective":"陆沉进入记忆交易链","ending_change":"陆沉发现自己也是交易样本"}],
    "near_chapters": [{"number":1,"goal":"陆沉在雨夜站台发现第一张记忆票据","expected_turn":"陆沉意识到童年被交易"}],
    "raw_outline":"陆沉进入记忆站，陆沉发现交易规则，陆沉最终改写记忆站。"
  }
}
```"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

    assert!(outcome.is_ready(), "{:?}", outcome.gate.actionable_issues());
    let current_contract = draft.current_contract.as_ref().expect("current contract");
    let governed_primary = current_contract
        .pointer("/characters/0/canonical_name")
        .and_then(|value| value.as_str())
        .expect("governed primary name");
    assert_ne!(governed_primary, "陆沉");
    assert!(
        current_contract
            .pointer("/outline/raw_outline")
            .and_then(|value| value.as_str())
            .is_some_and(|outline| {
                outline.contains(governed_primary) && !outline.contains("陆沉")
            }),
        "story prose must remain available for the targeted plot repair: {current_contract}"
    );
}

#[test]
fn contract_normalization_blocks_relationship_participants_outside_character_authority() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-relationship-authority",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let raw = r#"{
  "title": {
    "canonical_title": "公开九霄残卷",
    "rationale": "九霄来自终局飞升压力，公开九霄残卷对应主角把旧天道漏洞带到宗门审判台并改写修行入口的关键行动。"
  },
  "language": "zh-CN",
  "genre": "异界修仙",
  "brief": "异界修仙，每章2500字，至少5万字。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "premise": "凡人少年在九霄大陆发现天道残卷，被迫卷入宗门与旧天道的争夺。",
  "ending": {
    "desired_resolution": "主角公开残卷中的修行漏洞，让凡人也能获得入道资格。",
    "final_state": "旧天道垄断被打破，九霄大陆出现新的修行入口。"
  },
  "protagonist_arc": "从只求自保的凡人少年，到愿意公开残卷代价的开道者。",
  "world_imagery": "九霄云阙、断裂残卷、灵脉天梯、被封锁的凡人城。",
  "main_causal_spine": "残卷现世引出宗门追杀，主角追查天道漏洞，终局公开残卷改写修行入口。",
  "characters": [
    {"canonical_name":"南照野","role":"主角","desire":"为凡人争取入道资格","fear":"残卷代价害死同伴","bottom_line":"不献祭无辜者","arc_start":"凡人自保","arc_end":"公开修行入口"},
    {"canonical_name":"段岚澜","role":"同伴","desire":"守住凡人城","fear":"宗门清算家人","bottom_line":"不背叛凡人城","arc_start":"谨慎相助","arc_end":"并肩公开证据"}
  ],
  "world_rules": ["残卷会暴露持有者气息并引来宗门追杀", "凡人城的入道资格必须由公开残卷漏洞后才能重算", "宗门天梯只能承认被残卷记录过的修行代价"],
  "outline": {
    "volumes": [{"title":"残卷入城","objective":"主角发现残卷并进入宗门视野","ending_change":"凡人城被卷入宗门追杀"}],
    "near_chapters": [{"number":1,"goal":"南照野在凡人城夜市发现断裂残卷","expected_turn":"残卷引来宗门追杀，南照野无法再留在凡人城"}],
    "raw_outline": "残卷现世，宗门追杀，终局公开修行入口。"
  },
  "structured": {
    "relationship_ledger": [{
      "characters": ["南照野", "裴阙阙"],
      "arc_type": "rivalry",
      "relationship_type": "生死敌对",
      "stage": "压迫/对抗",
      "next_expected_stage": "相互试探",
      "start_state": "裴阙阙奉宗门命令追杀持卷者",
      "desired_end_state": "南照野以公开残卷漏洞击败裴阙阙背后的垄断",
      "conflicts": ["宗门追杀", "修行入口垄断"]
    }]
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(!outcome.is_ready());
    assert!(outcome.gate.actionable_issues().iter().any(|issue| {
        issue.contains("关系") && issue.contains("角色权威表") && issue.contains("裴阙阙")
    }));
    let normalized = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|candidate| candidate.get("normalized"))
        .expect("pending normalized contract");
    let character_names = normalized
        .get("characters")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|character| {
            character
                .get("canonical_name")
                .and_then(|value| value.as_str())
        })
        .collect::<Vec<_>>();
    assert!(!character_names.contains(&"裴阙阙"), "{normalized}");
    let serialized = normalized.to_string();
    assert!(serialized.contains("裴阙阙"), "{serialized}");
    assert!(
        normalized
            .get("world_rules")
            .and_then(|value| value.as_array())
            .is_some_and(|items| items.len() >= 3),
        "{normalized}"
    );
}

#[test]
fn contract_authority_aligns_stale_primary_names_inside_character_fields() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-field-stale-primary",
        "fiction",
        "写异界修仙小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let raw = r#"{
  "title": {
    "canonical_title": "夺回残卷令",
    "candidates": ["夺回残卷令", "残卷开道", "凡城夺令"],
    "rationale": "残卷令是宗门垄断凡人入道的关键物件，夺回对应终局公开规则漏洞并让凡人获得入道资格。"
  },
  "language": "zh-CN",
  "genre": "异界修仙",
  "brief": "异界修仙，每章2500字，至少5万字。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "凡人少年在凡城夜市拿到残卷令，被宗门追杀后发现旧天道垄断入道入口。",
  "ending": {
    "desired_resolution": "主角公开残卷令里的规则漏洞，让凡人也能获得入道资格。",
    "final_state": "旧天道垄断被打破，凡城出现新的修行入口。",
    "must_resolve": ["残卷令来源", "宗门垄断入道入口的真相"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只求自保的凡人少年，到愿意公开残卷令代价的开道者。",
  "world_imagery": "凡城夜市、残卷令、灵脉天梯、九霄云阙。",
  "main_causal_spine": "残卷令现世引出宗门追杀，主角追查旧天道漏洞，终局公开残卷令改写入道入口。",
  "characters": [
    {"canonical_name":"景朔棠","role":"主角","desire":"为凡人争取入道资格","fear":"残卷令代价害死同伴","bottom_line":"不献祭无辜者","arc_start":"凡人自保","arc_end":"公开修行入口"},
    {"canonical_name":"钟砚澜","role":"关键对手","desire":"维持入道垄断","fear":"残卷令公开","bottom_line":"不得干涉对手：林烬突破","arc_start":"垄断维护者","arc_end":"被公开规则反噬"}
  ],
  "themes": ["底层修士夺回入道入口"],
  "world_rules": ["残卷令只能揭示规则漏洞，不能直接提升修为", "宗门入道名额由灵脉天梯分配，篡改名额会引发天道反噬", "凡人使用残卷令必须付出被宗门追踪的代价"],
  "style_rules": ["用具体场景推进", "每章必须有不可逆变化"],
  "must_avoid": ["不要角色改名"],
  "structured_contract_v2": {
    "character_voice_ledger": [
      {"character":"景朔棠","voice_style":"说话先克制观察，再在关键处露出锋芒","dialogue_rules":["对白必须体现景朔棠害怕林烬突破后的代价。"]}
    ],
    "relationship_ledger": [
      {"characters":["景朔棠","钟砚澜"],"relationship_type":"对抗","stage":"追杀","start_state":"林烬被宗门逼入凡城","current_state":"林烬追查残卷令","desired_end_state":"林烬公开规则漏洞","conflicts":["林烬突破会触发宗门清算"]}
    ],
    "emotional_state_ledger": [
      {"character":"景朔棠","current_emotion":"林烬仍在恐惧代价","pressure":"宗门追杀林烬","desire":"守住残卷令","fear":"林烬突破失败","expected_next_shift":"林烬主动公开证据","payoff_target":"林烬成为开道者"}
    ],
    "conflict_pressure_curve": {
      "global_curve": [
        {"range":"第一卷","pressure_level":"中","function":"林烬被迫逃离凡城并确认残卷令代价"}
      ],
      "release_strategy":"林烬每次突破后都要付出关系代价",
      "peak_policy":"林烬不能无代价升级"
    },
    "reveal_schedule": [
      {"secret":"林烬身上的残卷令不是偶然","reader_knows":"林烬被盯上","protagonist_knows":"林烬只知道追杀","antagonist_knows":"宗门知道林烬的位置","reveal_window":"第二卷","status":"planned"}
    ]
  },
  "outline": {
    "volumes": [{"title":"凡城夺令","objective":"景朔棠发现残卷令并进入宗门视野","ending_change":"凡城被卷入宗门追杀"}],
    "near_chapters": [
      {"number":1,"goal":"景朔棠在凡城夜市发现残卷令","expected_turn":"残卷令引来宗门追杀，景朔棠无法再留在凡城"},
      {"number":2,"goal":"景朔棠追查残卷令指向的第一处灵脉天梯","expected_turn":"线索证明入道入口被宗门人为垄断"}
    ],
    "raw_outline":"残卷令现世，宗门追杀，终局公开修行入口。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    assert!(!outcome.is_ready());
    assert!(outcome
        .gate
        .actionable_issues()
        .iter()
        .any(|issue| issue.contains("林烬")));
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
}

#[test]
fn bare_outcome_summary_title_is_not_accepted_as_work_title() {
    let evidence = "都市爽文。主角从普通职员卷入资本暗战，终局夺回公司控制权并公开行业黑幕。";

    let issue = crate::tool::writing::naming::title_contract_basis_issue(
        "财富机遇",
        "书名",
        "《财富机遇》体现主角获得财富和机遇。",
        evidence,
    );

    assert!(
        issue.is_some(),
        "bare outcome-summary title should be rejected"
    );
}

#[test]
fn field_label_volume_title_is_replaced_before_contract_confirmation() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-field-label-volume-title",
        "fiction",
        "写都市爽文小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let raw = r#"{
  "title": {
    "canonical_title": "夺回旧楼暗账",
    "candidates": ["夺回旧楼暗账", "灰巷夺盘", "暗账开城"],
    "rationale": "旧楼是暗账硬盘第一次现身的地点，夺回旧楼暗账指主角把硬盘证据带上公开听证席后反转公司控制权归属。"
  },
  "language": "zh-CN",
  "genre": "都市爽文",
  "brief": "都市爽文，每章2500字，至少5万字。",
  "target_units": 50000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "普通职员在旧楼档案室发现暗账，被资本派系追杀后决定公开行业黑幕。",
  "ending": {
    "desired_resolution": "主角用旧楼暗账公开行业黑幕，夺回公司控制权并保护同伴。",
    "final_state": "旧楼从被遗忘的档案室变成行业公开听证的证据起点。",
    "must_resolve": ["旧楼暗账来源", "公司控制权归属"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只求保住工作的普通职员，到敢把暗账公开给所有人的破局者。",
  "world_imagery": "旧楼档案室、雨夜霓虹、暗账硬盘、公开听证席。",
  "main_causal_spine": "旧楼暗账引来追杀，主角追查资本派系，终局公开证据夺回公司控制权。",
  "characters": [
    {"canonical_name":"沈栖序","role":"主角","desire":"查清旧楼暗账并保住同伴","fear":"证据公开前同伴被牵连","bottom_line":"不牺牲无辜同事换取胜利","arc_start":"谨慎自保","arc_end":"公开破局"},
    {"canonical_name":"许闻晚","role":"同伴","desire":"帮主角整理证据","fear":"家人被资本派系报复","bottom_line":"不伪造证据","arc_start":"旁观协助","arc_end":"主动站上听证席"},
    {"canonical_name":"洛予安","role":"关键对手","desire":"销毁旧楼暗账并维持控制权","fear":"暗账公开导致派系崩塌","bottom_line":"不让证据进入听证流程","arc_start":"幕后施压","arc_end":"被证据链反噬"}
  ],
  "themes": ["普通人用证据夺回选择权"],
  "world_rules": ["旧楼暗账必须形成完整证据链才能公开", "资本派系只能通过合法外衣施压，不能凭空解决证据", "公司控制权必须由董事会和听证证据共同决定"],
  "style_rules": ["每章必须有具体场景推进", "爽点来自证据反击和身份翻转"],
  "must_avoid": ["不要角色改名", "不要让主角无代价秒赢"],
  "outline": {
    "volumes": [{"title":"卷尾变化","objective":"主角发现旧楼暗账并进入资本派系视野","ending_change":"主角无法再回到普通职员身份"}],
    "near_chapters": [
      {"number":1,"goal":"沈栖序在旧楼档案室发现暗账硬盘","expected_turn":"硬盘触发追查，沈栖序被迫选择保留证据"},
      {"number":2,"goal":"许闻晚帮助沈栖序核对第一组流水","expected_turn":"证据指向洛予安控制的空壳公司"}
    ],
    "raw_outline":"旧楼暗账现世，主角追查资本派系，终局公开证据夺回公司控制权。"
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    let issues = outcome.gate.actionable_issues().join("；");

    assert!(!outcome.is_ready(), "{issues}");
    assert!(issues.contains("分卷规划"), "{issues}");
    assert!(draft.current_contract.is_none());
    assert!(draft.pending_contract_candidate.is_some());
}

#[test]
fn legal_agreement_surface_is_rejected_before_pending_contract_pollution() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-legal-contract-residue",
        "fiction",
        "写都市轻玄幻小说，每章2500字，至少5万字。",
    )
    .expect("draft");

    let raw = "都市轻玄幻小说写作委托合同草案\n\
合同编号：[年份]-WR-[序号]\n\
甲方（委托方）：某某\n\
乙方（受托方/作者）：某某\n\
第三条 稿酬及支付方式：甲方应向乙方支付稿酬。\n\
第四条 知识产权：著作权归属由双方另行约定。\n\
第六条 违约责任：逾期交付需支付违约金。";

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    let issues = outcome.gate.actionable_issues().join("；");

    assert!(
        !outcome.is_ready(),
        "legal agreement surface must not be accepted as a fiction contract"
    );
    assert!(
        issues.contains("法律合同") || issues.contains("委托协议"),
        "expected legal-contract boundary issue, got: {issues}"
    );
    assert!(
        draft.current_contract.is_none(),
        "legal agreement output must not become current contract"
    );
}

#[test]
fn local_character_authority_rewrites_structured_voice_references() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-character-voice-local-governance",
        "fiction",
        "写都市轻玄幻小说，每章2500字，总字数5000字。",
    )
    .expect("draft");

    let raw = r#"{
  "title": {
    "canonical_title": "旧物开灵局",
    "candidates": ["旧物开灵局", "雨巷修灵人", "回收站见灵"],
    "rationale": "旧物回收站是主角第一次读取记忆的地点，开灵局指他用旧物线索反制灵气垄断。"
  },
  "language": "zh-CN",
  "genre": "都市轻玄幻",
  "brief": "都市轻玄幻短篇，每章2500字，总字数5000字。",
  "target_units": 5000,
  "chapter_unit_target": 2500,
  "max_chapters_per_turn": 1,
  "premise": "旧物回收站青年触碰古物后获得灵视，能读取旧物残留记忆，并追查城市灵气被垄断的真相。",
  "ending": {
    "desired_resolution": "主角用旧物线索公开灵气垄断证据，把回收站变成新的公共灵气节点。",
    "final_state": "城市灵气重新流动，回收站成为普通人也能求助的入口。",
    "must_resolve": ["父亲债务来源", "灵气垄断证据"],
    "allowed_open_questions": []
  },
  "protagonist_arc": "从只想还债的自保者，变成愿意守住公共灵气秩序的承担者。",
  "world_imagery": "雨夜旧物回收站、发光裂纹瓷碗、霓虹下的灵气尘埃。",
  "main_causal_spine": "触碰古物觉醒灵视->修复旧物取得线索->发现灵气节点被垄断->公开证据重建灵气节点。",
  "characters": [
    {"canonical_name":"林默","role":"主角","desire":"还清父亲债务并保住回收站","fear":"被城市彻底抛弃","bottom_line":"不卖掉旧物里的亡者记忆","arc_start":"只看价格","arc_end":"守住公共灵气节点"},
    {"canonical_name":"苏青","role":"关系对象","desire":"找到失传引灵阵","fear":"家族被遗忘","bottom_line":"绝不背弃与林默共同守护回收站的契约","arc_start":"高傲疏离","arc_end":"共同守护新节点"},
    {"canonical_name":"赵天穹","role":"关键对手","desire":"垄断城市灵气节点","fear":"垄断证据公开","bottom_line":"不让证据进入公众视野","arc_start":"幕后施压","arc_end":"被证据链反噬"}
  ],
  "themes": ["普通人用旧物记忆夺回城市灵气"],
  "world_rules": ["灵视读取旧物记忆必须消耗持有者情绪余温", "灵气节点被财阀垄断后普通人只能通过旧物残响借用灵气", "公开垄断证据必须同时满足物证记忆和现实证词"],
  "style_rules": ["每章必须有具体旧物或场景推进", "轻松缓冲来自旧物记忆反差和市井误会"],
  "must_avoid": ["不要角色改名", "不要无代价解决冲突"],
  "outline": {
    "volumes": [{"title":"旧物见灵","objective":"主角觉醒灵视并确认旧物能留下记忆证据","ending_change":"主角无法再把回收站当普通生意"}],
    "near_chapters": [
      {"number":1,"goal":"林默在雨夜回收站读取瓷碗残留记忆","expected_turn":"他发现债务和灵气垄断有关"},
      {"number":2,"goal":"苏青带来失传引灵阵线索","expected_turn":"两人结成临时契约"}
    ],
    "raw_outline":"旧物见灵，主角追查灵气垄断，终局公开证据重建节点。"
  },
  "structured": {
    "field_requirements": {"character_voice_ledger":"required"},
    "character_voice_ledger": [
      {"character":"苏青","voice_style":"语气冷静但信守与林默的契约","catchphrases":[],"forbidden_expressions":["不要替换角色姓名"],"dialogue_rules":["对白必须体现苏青信守与林默的契约"]}
    ]
  }
}"#;

    let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
    let issues = outcome.gate.actionable_issues().join("；");
    let current_contract = draft
        .current_contract
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    assert!(
        outcome.is_ready(),
        "issues={issues}; current_contract={current_contract}"
    );
    let current_value = draft.current_contract.as_ref().expect("current contract");
    let authority_names = current_value
        .get("characters")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|character| {
            character
                .get("canonical_name")
                .and_then(|value| value.as_str())
        })
        .collect::<Vec<_>>();
    let voice_character = current_value
        .pointer("/structured/character_voice_ledger/0/character")
        .and_then(|value| value.as_str())
        .expect("voice character");
    assert!(authority_names.contains(&voice_character));
    let structured = current_value
        .get("structured")
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(!structured.contains("林默") && !structured.contains("苏青"));
    assert!(structured.contains(voice_character));
    assert!(structured.contains(authority_names[0]));
    assert!(
            current_contract.contains("旧物回收站")
                && current_contract.contains("灵视")
                && current_contract.contains("旧物残留记忆"),
            "character authority projection must preserve non-character story anchors: {current_contract}"
        );
}

#[test]
fn approval_sync_does_not_hide_stale_story_names_from_typed_gate() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-approval-authority-sync",
        "fiction",
        "写奇幻冒险小说，每章2500字，一共5万字。",
    )
    .expect("draft");
    draft.current_contract = Some(serde_json::json!({
        "title": {"canonical_title": "星核守夜人", "rationale": "根据星核与永夜守护使命命名"},
        "genre": "奇幻冒险",
        "language": "zh-CN",
        "brief": "星语者寻找星核终结永夜",
        "target_units": 50000,
        "chapter_unit_target": 2500,
        "max_chapters_per_turn": 1,
        "premise": "唐栖阙听见星核低语并踏上冒险",
        "ending": {"desired_resolution": "唐栖阙牺牲听觉天赋终结永夜"},
        "protagonist_arc": "唐栖阙从逃避低语成长为守护光明的人",
        "world_imagery": "星核、永夜、深渊遗迹",
        "main_causal_spine": "唐栖阙获得星核碎片->对抗守夜人->终结永夜",
        "characters": [
            {"canonical_name":"唐栖阙","role":"主角","desire":"听见完整旋律","fear":"被低语撕裂","bottom_line":"不背叛真相","arc_start":"逃避低语","arc_end":"守护光明"},
            {"canonical_name":"祝栖川","role":"关键对手","desire":"证明家族价值","fear":"知识被垄断","bottom_line":"真相高于安全","arc_start":"观察者","arc_end":"同行者"},
            {"canonical_name":"顾闻砺","role":"关键对手","desire":"维持永夜秩序","fear":"权力消散","bottom_line":"阻止星核重聚","arc_start":"压迫者","arc_end":"失败者"}
        ],
        "themes": ["倾听真相"],
        "world_rules": ["唐栖阙倾听星核会承受精神负荷"],
        "style_rules": ["具体场景推进"],
        "must_avoid": ["不要角色改名"],
        "outline": {
            "raw_outline": "唐栖阙获得星核碎片，最终终结永夜。",
            "near_chapters": [{"number":1,"goal":"唐栖阙获得星核碎片","expected_turn":"确认冒险不可回头"}]
        }
    }));

    let approved = serde_json::json!({
        "draft": {
            "title": "星核守夜人",
            "language": "zh-CN",
            "genre": "奇幻冒险",
            "brief": "星语者寻找星核终结永夜",
            "export_format": "txt",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "export_when_complete": true,
            "approved_only": true,
            "premise": "温照珩听见星核低语并踏上冒险",
            "ending_direction": "温照珩牺牲听觉天赋终结永夜",
            "protagonist_arc": "温照珩从逃避低语成长为守护光明的人",
            "world_imagery": "星核、永夜、深渊遗迹",
            "main_causal_spine": "唐栖阙获得星核碎片->对抗守夜人->终结永夜",
            "title_rationale": "根据星核与永夜守护使命命名",
            "themes": ["倾听真相"],
            "characters": [
                "name: 温照珩; role: 主角; desire: 听见完整旋律; fear: 被唐栖阙的低语撕裂; bottom_line: 不背叛真相; name_source: generated_by_writing_tool_policy",
                "name: 司衡舟; role: 关键对手; desire: 证明家族价值; fear: 被祝栖川识破; bottom_line: 真相高于安全; name_source: generated_by_writing_tool_policy",
                "name: 沈阙澜; role: 关键对手; desire: 维持永夜秩序; fear: 权力消散; bottom_line: 阻止星核重聚; name_source: generated_by_writing_tool_policy"
            ],
            "world_rules": ["温照珩倾听星核会承受精神负荷"],
            "style_rules": ["具体场景推进"],
            "must_avoid": ["不要角色改名"],
            "outline": "唐栖阙获得星核碎片，最终终结永夜。",
            "structured_contract_v2": {
                "reader_promise": {
                    "core_hook": "唐栖阙在永夜中听见星核真相",
                    "curiosity_engine": "祝栖川究竟为何隐瞒星核代价",
                    "payoff_style": "唐栖阙以听觉天赋换来黎明",
                    "pleasure_points": ["唐栖阙逐步听懂星核"]
                }
            }
        }
    });

    assert!(super::super::sync_creation_draft_from_approval(
        &mut draft, &approved
    ));
    let current_contract = current_contract_text(&draft);
    assert!(current_contract.contains("温照珩"), "{current_contract}");
    assert!(current_contract.contains("司衡舟"), "{current_contract}");
    assert!(current_contract.contains("沈阙澜"), "{current_contract}");
    assert!(current_contract.contains("唐栖阙"), "{current_contract}");
    assert!(
        !super::super::creation_draft_contract_blocking_issues(&draft).is_empty(),
        "stale free-form contract prose must not be silently promoted to ready authority"
    );
}

#[test]
fn creation_draft_round_trips_structured_contract_authority_metadata() {
    let mut draft = super::super::build_initial_creation_draft(
        "session-contract-v2-round-trip",
        "fiction",
        "写一部原创小说，每章2500字，总字数5万字。",
    )
    .expect("draft");
    let contract = crate::tool::writing::novel_contract_v2::NovelContractV2 {
        schema_version: crate::tool::writing::novel_contract_v2::NOVEL_CONTRACT_V2_SCHEMA_VERSION
            .to_string(),
        revision: 9,
        reader_promise: crate::tool::writing::novel_contract_v2::ReaderPromise {
            core_hook: "主角必须在失去旧身份后重建新的社会位置".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    draft.set_contract_v2(contract);
    let round_trip = draft.contract_v2();

    assert_eq!(
        round_trip.schema_version,
        crate::tool::writing::novel_contract_v2::NOVEL_CONTRACT_V2_SCHEMA_VERSION
    );
    assert_eq!(round_trip.revision, 9);
    assert_eq!(
        round_trip.reader_promise.core_hook,
        "主角必须在失去旧身份后重建新的社会位置"
    );
}
