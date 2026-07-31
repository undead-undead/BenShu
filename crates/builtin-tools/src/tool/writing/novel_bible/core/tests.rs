use super::*;
use crate::tool::writing::creation_contract_model::{CharacterContract, NovelCreationContract};
use crate::tool::writing::novel_contract_v2::{
    ArtifactLedgerEntry, ChapterCharacterRegistration, PayoffMatrixEntry, ReaderPromise,
    RelationshipLedgerEntry,
};

fn contract() -> StoryContract {
    StoryContract {
        premise: "草根少年在学院晋级中发现星门真相。".to_string(),
        themes: vec!["选择与代价".to_string()],
        characters: vec!["沈砚；欲望：改变命运；恐惧：被抛弃；底线：不牺牲同伴".to_string()],
        world_rules: vec!["星火修炼需要精神负荷，越级会留下裂纹。".to_string()],
        style_rules: vec!["热血但不秒杀".to_string()],
        must_avoid: vec!["无代价升级".to_string()],
        outline: "终局要揭开星门代价，并让主角以承担代价的方式获胜。".to_string(),
        structured_contract_v2: NovelContractV2::default(),
        authority_contract: None,
        updated_at: "now".to_string(),
    }
}

fn approved_change(
    entity_id: &str,
    event_type: ChapterStateEventType,
    value: &str,
) -> ChapterStateChange {
    ChapterStateChange {
        change_id: format!("test-{entity_id}"),
        entity_id: entity_id.to_string(),
        event_type,
        value: value.to_string(),
        evidence: ChapterBodyEvidence {
            start_char: 0,
            end_char: value.chars().count(),
            excerpt: value.to_string(),
        },
        authority_path: "test.authority".to_string(),
        authority_excerpt: entity_id.to_string(),
        allowance: StateChangeAllowance::Contract,
        ..Default::default()
    }
}

#[test]
fn story_bible_carries_reverse_design_and_character_anchors() {
    let bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        "now".to_string(),
    );

    assert!(bible
        .narrative_graph
        .reverse_design_notes
        .iter()
        .any(|note| note.contains("终局") || note.contains("ending")));
    assert_eq!(bible.character_ledger[0].name, "沈砚");
    assert!(bible.character_ledger[0].desire.contains("改变命运"));
    assert_eq!(bible.genre_governance.genre_family, "fantasy");
    assert!(!bible.world_database.rules.is_empty());
}

#[test]
fn typed_authority_contract_wins_over_legacy_story_mirrors() {
    let mut legacy = contract();
    legacy.characters = vec!["旧名；欲望：旧目标；恐惧：旧恐惧；底线：旧底线".to_string()];
    legacy.structured_contract_v2 = NovelContractV2 {
        revision: 2,
        ..Default::default()
    };
    legacy.authority_contract = Some(NovelCreationContract {
        characters: vec![CharacterContract {
            character_id: "character-primary".to_string(),
            canonical_name: "秦舟".to_string(),
            role: "主角".to_string(),
            desire: "夺回选择权".to_string(),
            fear: "再次失去同伴".to_string(),
            bottom_line: "不牺牲无辜者".to_string(),
            ..Default::default()
        }],
        structured: NovelContractV2 {
            revision: 8,
            reader_promise: ReaderPromise {
                core_hook: "秦舟必须夺回选择权".to_string(),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    });

    let bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &legacy,
        "now".to_string(),
    );

    assert_eq!(bible.source_contract_revision, 8);
    assert_eq!(bible.character_ledger.len(), 1);
    assert_eq!(bible.character_ledger[0].name, "秦舟");
    assert_eq!(bible.character_ledger[0].id, "character-primary");
}

#[test]
fn story_bible_does_not_promote_chapter_goals_to_character_ledger() {
    let mut contract = contract();
    contract.characters = vec![
        "寻觅碎片".to_string(),
        "进入空寂之域".to_string(),
        "name: 沈砚; role: 主角; desire: 改变命运; fear: 被抛弃; bottom_line: 不牺牲同伴"
            .to_string(),
    ];

    let bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );

    let names = bible
        .character_ledger
        .iter()
        .map(|character| character.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["沈砚"]);
    assert_eq!(bible.character_ledger[0].role, "主角");
}

#[test]
fn story_bible_filters_user_confirmation_surface_from_contract() {
    let mut contract = contract();
    contract.characters = vec![
        "因为主角：这是用户说明，不是角色".to_string(),
        "请回复开始写第一章".to_string(),
        "陆沉；欲望：夺回自由；恐惧：再次成为资源；底线：不牺牲同伴".to_string(),
    ];
    contract.world_rules = vec![
        "质量合同：不要漂移".to_string(),
        "灵力回收必须付出记忆代价。".to_string(),
    ];
    contract.outline = "第一卷：陆沉发现灵力回收真相。\n可修改说明：如果不满意可以让我修改。\n终局：陆沉终结回收秩序。".to_string();

    let bible = build_story_bible(
        "碎灵余烬",
        "zh-CN",
        "玄幻",
        "请回复开始写第一章",
        &contract,
        "now".into(),
    );

    assert!(bible.brief.is_empty());
    assert!(bible
        .character_ledger
        .iter()
        .all(|character| character.name != "因为主角"));
    assert!(bible
        .character_ledger
        .iter()
        .any(|character| character.name == "陆沉"));
    assert!(bible
        .world_database
        .rules
        .iter()
        .all(|rule| !rule.rule.contains("质量合同")));
    assert!(!bible.narrative_graph.global_spine.contains("可修改说明"));
    assert!(!bible.ending_contract.final_state.contains("请回复"));
}

#[test]
fn typed_ending_obligations_block_completion_until_paid_off() {
    let bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );

    let blockers = story_bible_completion_blockers(Some(&bible));

    assert!(blockers
        .iter()
        .any(|item| item.contains("unresolved debts")));
}

#[test]
fn story_bible_derives_volume_arcs_from_contract_outline() {
    let mut contract = contract();
    contract.outline = "第一卷《尘校试炼》（第1-8章）：进入学院考试，建立草根逆袭的第一层代价。\n第二卷《星门裂潮》（第9-18章）：星门代价扩大，主角必须在晋级和守护同伴之间选择。\n终局：主角承担星门代价并击败幕后者。".to_string();

    let bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".into(),
    );

    assert_eq!(bible.narrative_graph.volume_arcs.len(), 2);
    assert!(bible.narrative_graph.volume_arcs[0]
        .title
        .contains("尘校试炼"));
    assert_eq!(bible.narrative_graph.volume_arcs[0].start_chapter, Some(1));
    assert_eq!(bible.narrative_graph.volume_arcs[0].end_chapter, Some(8));
    assert_eq!(bible.narrative_graph.volume_arcs[1].start_chapter, Some(9));
}

#[test]
fn story_bible_seeds_contract_payoffs_as_completion_debts() {
    let mut contract = contract();
    contract.structured_contract_v2.payoff_matrix = vec![PayoffMatrixEntry {
        promise: "揭开潮汐引擎为何抬升海平面".to_string(),
        payoff_target: "主角重置引擎并建立海洋共生秩序".to_string(),
        status: "planned".to_string(),
        ..Default::default()
    }];

    let bible = build_story_bible(
        "海平面之下",
        "zh-CN",
        "科幻",
        "brief",
        &contract,
        "now".into(),
    );

    assert!(bible.hook_ledger.iter().any(|hook| {
        hook.title.contains("潮汐引擎")
            && matches!(hook.status, HookStatus::Open)
            && hook.evidence.iter().any(|item| item.contains("海洋共生"))
    }));
}

#[test]
fn rolling_execution_package_upserts_chapter_goal() {
    let mut bible = build_story_bible(
        "海平面之下",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".into(),
    );

    upsert_planned_chapter_goal(
        &mut bible,
        9,
        "梁隼白进入潮汐引擎控制层",
        "联盟封锁协议被永久公开",
        "把调查阶段推进到公开对抗阶段",
    );

    let goal = bible
        .narrative_graph
        .chapter_goals
        .iter()
        .find(|goal| goal.chapter_number == 9)
        .expect("rolling chapter goal");
    assert!(goal.goal.contains("潮汐引擎"));
    assert!(goal.moves_toward_ending.contains("永久公开"));
    assert_eq!(goal.depends_on, vec![8]);
}

#[test]
fn rolling_execution_package_does_not_rewrap_existing_chapter_goal() {
    let mut bible = build_story_bible(
        "海平面之下",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".into(),
    );
    bible.narrative_graph.chapter_goals.push(ChapterGoal {
        chapter_number: 1,
        goal: "主角确认潮汐异常并作出第一次不可逆选择".to_string(),
        depends_on: Vec::new(),
        moves_toward_ending: "建立终局调查入口".to_string(),
    });

    for wrapped in [
        "完成《海平面之下》第 1 章：主角确认潮汐异常并作出第一次不可逆选择",
        "完成《海平面之下》第 1 章：完成《海平面之下》第 1 章：主角确认潮汐异常并作出第一次不可逆选择",
    ] {
        upsert_planned_chapter_goal(&mut bible, 1, wrapped, "", "");
    }

    let goal = bible
        .narrative_graph
        .chapter_goals
        .iter()
        .find(|goal| goal.chapter_number == 1)
        .expect("existing chapter goal");
    assert_eq!(goal.goal, "主角确认潮汐异常并作出第一次不可逆选择");
    assert_eq!(goal.moves_toward_ending, "建立终局调查入口");
}

#[test]
fn volume_arcs_do_not_swallow_chapter_plans_or_generic_headers() {
    let mut contract = contract();
    contract.outline = "第一卷《微光破晓》（阶段目标：完成生存积累）\n第二卷：裂痕扩张（阶段目标：身份暴露）\n第三卷：余烬重燃（阶段目标：直面反派）\n逐章规划：\n第01章《黑石村醒》：本章目标：主角重生。\n第12章《风暴中心》：本章目标：卷入两大势力争夺。\n全书推进依据：\n书名：碎灵余烬\n书名理由：来自终局代价。\n终局方向：主角终结回收秩序。\n质量合同\n导出规范：仅在章节获批后导出。".to_string();

    let bible = build_story_bible(
        "碎灵余烬",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".into(),
    );

    assert_eq!(bible.narrative_graph.volume_arcs.len(), 3);
    assert_eq!(bible.narrative_graph.volume_arcs[0].title, "微光破晓");
    assert_eq!(bible.narrative_graph.volume_arcs[1].title, "裂痕扩张");
    assert_eq!(bible.narrative_graph.volume_arcs[2].title, "余烬重燃");
    assert!(!bible
        .narrative_graph
        .volume_arcs
        .iter()
        .any(|arc| { arc.title.contains("第12章") || arc.goal.contains("质量合同") }));
    assert!(!bible.narrative_graph.volume_arcs.iter().any(|arc| {
        arc.goal.contains("全书推进依据")
            || arc.goal.contains("书名理由")
            || arc.goal.contains("终局方向")
    }));
}

#[test]
fn story_bible_promotes_near_chapter_goals_to_narrative_graph() {
    let mut contract = contract();
    contract.outline = "第一卷《旧城借势》：主角获得传承并进入核心冲突；卷尾变化：被权贵正式盯上。\n第1章 本章目标：主角获得传承并救下关键关系；预期转折：确认传承必须付出记忆代价。\n第2章 本章目标：主角第一次主动反击；预期转折：制度压力公开压向主角。".to_string();

    let bible = build_story_bible(
        "旧城借势",
        "zh-CN",
        "都市爽文",
        "brief",
        &contract,
        "now".into(),
    );

    assert_eq!(bible.narrative_graph.chapter_goals.len(), 2);
    assert_eq!(bible.narrative_graph.chapter_goals[0].chapter_number, 1);
    assert!(bible.narrative_graph.chapter_goals[0]
        .goal
        .contains("获得传承"));
    assert!(bible.narrative_graph.chapter_goals[0]
        .moves_toward_ending
        .contains("记忆代价"));
    assert!(!bible
        .ending_contract
        .desired_resolution
        .contains("第1章 本章目标"));
}

#[test]
fn story_bible_extracts_clean_ending_and_spine_without_outline_pollution() {
    let mut contract = contract();
    contract.premise = "白望禾在蒸汽铁律城调查无声病，发现城市能源来自被剥夺的声音。".to_string();
    contract.outline = "全书推进依据：内部展示说明，不应进入权威状态。\n\
总主线因果链：底层检修员追查无声病 -> 揭开声纹炉真相 -> 迫使铁律城公开代价。\n\
第1章 本章目标：主角发现黄铜管道里的异常低鸣。\n\
第2章 本章目标：主角确认失声者并非病人。\n\
终局方向：白望禾关闭声纹炉，让城市以公开代价重启。\n\
质量合同：不要漂移；导出规范：仅章节获批后导出。"
        .to_string();

    let bible = build_story_bible(
        "铁律城无声者",
        "zh-CN",
        "蒸汽朋克悬疑",
        "brief",
        &contract,
        "now".into(),
    );

    assert_eq!(
        bible.ending_contract.desired_resolution,
        "白望禾关闭声纹炉，让城市以公开代价重启"
    );
    assert_eq!(
        bible.narrative_graph.global_spine,
        "底层检修员追查无声病 -> 揭开声纹炉真相 -> 迫使铁律城公开代价"
    );
    assert!(!bible
        .ending_contract
        .desired_resolution
        .contains("第1章 本章目标"));
    assert!(!bible.narrative_graph.global_spine.contains("质量合同"));
}

#[test]
fn default_volume_arc_uses_clean_localized_fallback() {
    let mut contract = contract();
    contract.outline =
        "全书主线：主角进入铁律城调查失声事件。\n终局：主角公开能源代价。".to_string();

    let bible = build_story_bible(
        "铁律城无声者",
        "zh-CN",
        "蒸汽朋克悬疑",
        "brief",
        &contract,
        "now".into(),
    );

    assert_eq!(bible.narrative_graph.volume_arcs.len(), 1);
    assert_eq!(bible.narrative_graph.volume_arcs[0].title, "开局卷");
    assert!(!bible.narrative_graph.volume_arcs[0].goal.contains("第1章"));
    assert!(!bible.narrative_graph.volume_arcs[0]
        .title
        .contains("Opening movement"));
}

#[test]
fn story_contract_requires_core_character_anchor() {
    let mut weak = contract();
    weak.characters = vec!["name: 沈砚; role: 主角".to_string()];

    let blockers = story_contract_blockers(&weak);

    assert!(blockers
        .iter()
        .any(|item| item.contains("desire/fear/bottom-line")));
}

#[test]
fn labeled_character_name_is_not_parsed_as_name_literal() {
    let mut labeled = contract();
    labeled.characters = vec![
        "name: 沈砚; role: 主角; desire: 改变命运; fear: 被抛弃; bottom_line: 不牺牲同伴"
            .to_string(),
    ];

    let bible = build_story_bible("星门试炼", "zh-CN", "玄幻", "brief", &labeled, "now".into());

    assert_eq!(bible.character_ledger[0].name, "沈砚");
    assert!(story_bible_audit(Some(&bible)).0.is_empty());
}

#[test]
fn approved_chapter_metadata_updates_display_history_without_mutating_durable_state() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    let character_state_before = bible.character_ledger[0].current_state.clone();
    let timeline_before = serde_json::to_value(&bible.timeline).expect("timeline json");
    let chapter = ApprovedChapterDelta {
        number: 1,
        title: "入学试".to_string(),
        summary: "沈砚通过入学试，但发现一条未解线索。".to_string(),
        unit_count: 2500,
        key_facts: vec!["沈砚保住同伴。".to_string()],
        continuity_updates: vec!["伏笔：星门裂纹只有沈砚看见。".to_string()],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    assert_eq!(bible.chapter_summaries.len(), 1);
    assert!(
        bible
            .hook_ledger
            .iter()
            .all(|hook| !hook.title.contains("星门裂纹")),
        "chapter metadata must not bypass the explicit settlement hook lifecycle"
    );
    assert_eq!(
        bible.character_ledger[0].current_state, character_state_before,
        "display metadata must not mutate durable character truth"
    );
    assert_eq!(
        serde_json::to_value(&bible.timeline).expect("timeline json after update"),
        timeline_before,
        "display summary/key facts must not synthesize durable timeline entries"
    );
}

#[test]
fn approved_chapter_updates_each_character_state_from_own_evidence() {
    let mut contract = contract();
    contract.characters = vec![
        "name: 沈砚; role: 主角; desire: 改变命运; fear: 失去同伴; bottom_line: 不牺牲无辜者"
            .to_string(),
        "name: 季澜; role: 同盟; desire: 查明家族真相; fear: 被迫背叛; bottom_line: 不向黑炉低头"
            .to_string(),
    ];
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let chapter = ApprovedChapterDelta {
        number: 1,
        title: "入学试".to_string(),
        summary: "沈砚通过入学试，季澜在旁观中改变判断。".to_string(),
        unit_count: 2500,
        key_facts: vec![
            "沈砚保住同伴并发现星门裂纹。".to_string(),
            "季澜确认黑炉规训存在漏洞。".to_string(),
        ],
        continuity_updates: vec![
            "沈砚决定追查星门裂纹。".to_string(),
            "季澜开始怀疑家族命令。".to_string(),
        ],
        state_changes: vec![
            approved_change(
                "沈砚",
                ChapterStateEventType::Character,
                "沈砚决定追查星门裂纹。",
            ),
            approved_change(
                "季澜",
                ChapterStateEventType::Character,
                "季澜开始怀疑家族命令。",
            ),
        ],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    let shen_state = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "沈砚")
        .map(|character| character.current_state.as_str())
        .unwrap_or("");
    let ji_state = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "季澜")
        .map(|character| character.current_state.as_str())
        .unwrap_or("");
    assert!(shen_state.contains("沈砚"));
    assert!(!shen_state.contains("季澜开始怀疑"));
    assert!(ji_state.contains("季澜"));
    assert!(!ji_state.contains("沈砚决定追查"));
}

#[test]
fn required_character_end_state_persists_authority_outcome_not_prose_evidence() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    let protagonist = bible.character_ledger[0].clone();
    let evidence = "冰冷从沈砚的骨缝里渗出来，他扶墙站稳。";
    let required_state = "沈砚已适应重生后的身体状态并初步稳住军心";
    let chapter = ApprovedChapterDelta {
        number: 1,
        state_changes: vec![ChapterStateChange {
            entity_id: protagonist.id,
            event_type: ChapterStateEventType::Character,
            value: evidence.to_string(),
            evidence: ChapterBodyEvidence {
                start_char: 0,
                end_char: evidence.chars().count(),
                excerpt: evidence.to_string(),
            },
            authority_path: "chapter_contract.new_state_after_chapter".to_string(),
            authority_excerpt: required_state.to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    assert_eq!(bible.character_ledger[0].current_state, required_state);
}

#[test]
fn approved_chapter_updates_structured_contract_v2_evidence() {
    let mut contract = contract();
    contract
        .structured_contract_v2
        .emotional_contract
        .emotional_promise = "草根承担代价后获得尊严".to_string();
    contract
        .structured_contract_v2
        .relationship_ledger
        .push(RelationshipLedgerEntry {
            characters: vec!["沈砚".to_string(), "季澜".to_string()],
            relationship_type: "同盟逐步互信".to_string(),
            start_state: "互相试探".to_string(),
            current_state: "互相试探".to_string(),
            desired_end_state: "并肩作战".to_string(),
            conflicts: Vec::new(),
            secrets: Vec::new(),
            turning_points: Vec::new(),
            last_changed_chapter: None,
            ..Default::default()
        });
    contract
        .structured_contract_v2
        .artifact_ledger
        .push(ArtifactLedgerEntry {
            name: "星门钥片".to_string(),
            owner: "沈砚".to_string(),
            origin: "旧城遗物".to_string(),
            ability: "打开星门裂隙".to_string(),
            cost_or_limit: "每次使用都会消耗记忆".to_string(),
            last_seen_chapter: None,
            status: String::new(),
        });
    contract
        .structured_contract_v2
        .payoff_matrix
        .push(PayoffMatrixEntry {
            promise: "星门钥片的真实代价".to_string(),
            introduced_chapter: None,
            payoff_target: "终局前解决星门钥片代价".to_string(),
            payoff_chapter: None,
            status: "open".to_string(),
            evidence: Vec::new(),
        });
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let chapter = ApprovedChapterDelta {
        number: 3,
        title: "旧城钥片".to_string(),
        summary: "沈砚与季澜在旧城争夺星门钥片，第一次知道使用它必须付出记忆代价。".to_string(),
        unit_count: 2500,
        key_facts: vec!["星门钥片会消耗记忆。".to_string()],
        continuity_updates: vec![
            "关系更新：沈砚与季澜因共同承担旧城风险而建立有限信任。".to_string(),
            "线索进展：星门钥片的真实代价已经被沈砚发现。".to_string(),
        ],
        state_changes: vec![
            approved_change(
                "沈砚",
                ChapterStateEventType::Relationship,
                "因共同承担旧城风险而建立有限信任",
            ),
            approved_change(
                "星门钥片",
                ChapterStateEventType::Resource,
                "沈砚已确认使用它会消耗记忆",
            ),
            approved_change(
                "星门钥片的真实代价",
                ChapterStateEventType::HookAdvance,
                "真实代价已由沈砚发现",
            ),
        ],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    assert_eq!(
        bible.structured_contract_v2.relationship_ledger[0].last_changed_chapter,
        Some(3)
    );
    let relation = &bible.structured_contract_v2.relationship_ledger[0];
    assert!(relation.current_state.contains("共同承担旧城风险"));
    assert!(relation.stage.is_empty());
    assert!(relation.transition_history.is_empty());
    assert_eq!(
        bible.structured_contract_v2.artifact_ledger[0].last_seen_chapter,
        Some(3)
    );
    assert!(!bible.structured_contract_v2.payoff_matrix[0]
        .evidence
        .is_empty());
    assert!(bible
        .structured_contract_v2
        .emotional_contract
        .emotional_beats
        .is_empty());
}

#[test]
fn approved_power_delta_resolves_character_id_and_upserts_missing_progression_state() {
    let mut contract = contract();
    contract
        .structured_contract_v2
        .power_progression
        .character_current_levels
        .clear();
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let protagonist = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "沈砚")
        .expect("沈砚 character anchor")
        .clone();
    let chapter = ApprovedChapterDelta {
        number: 2,
        state_changes: vec![ChapterStateChange {
            entity_id: protagonist.id,
            event_type: ChapterStateEventType::Power,
            value: "沈砚将星门剑意稳定在第二阶。".to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    let states = &bible
        .structured_contract_v2
        .power_progression
        .character_current_levels;
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].character, "沈砚");
    assert!(states[0].level.contains("第二阶"));
    assert!(states[0].evidence.contains("chapter 2"));
}

#[test]
fn approved_relationship_delta_resolves_stable_character_id() {
    let mut contract = contract();
    contract
        .structured_contract_v2
        .relationship_ledger
        .push(RelationshipLedgerEntry {
            characters: vec!["沈砚".to_string(), "季澜".to_string()],
            current_state: "互相试探".to_string(),
            ..Default::default()
        });
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let protagonist = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "沈砚")
        .expect("沈砚 character anchor")
        .clone();
    let chapter = ApprovedChapterDelta {
        number: 2,
        state_changes: vec![ChapterStateChange {
            entity_id: protagonist.id,
            event_type: ChapterStateEventType::Relationship,
            value: "沈砚与季澜在共担风险后建立了有限信任。".to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    let relation = &bible.structured_contract_v2.relationship_ledger[0];
    assert!(relation.current_state.contains("有限信任"));
    assert_eq!(relation.last_changed_chapter, Some(2));
}

#[test]
fn approved_relationship_delta_updates_only_the_named_counterparty_relation() {
    let mut contract = contract();
    contract.structured_contract_v2.relationship_ledger = vec![
        RelationshipLedgerEntry {
            character_ids: vec![
                "character-shen-yan".to_string(),
                "character-ji-lan".to_string(),
            ],
            characters: vec!["沈砚".to_string(), "季澜".to_string()],
            current_state: "互相试探".to_string(),
            ..Default::default()
        },
        RelationshipLedgerEntry {
            character_ids: vec![
                "character-shen-yan".to_string(),
                "character-lu-zhou".to_string(),
            ],
            characters: vec!["沈砚".to_string(), "陆舟".to_string()],
            current_state: "彼此戒备".to_string(),
            ..Default::default()
        },
    ];
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let protagonist = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "沈砚")
        .expect("沈砚 character anchor")
        .clone();
    let chapter = ApprovedChapterDelta {
        number: 2,
        state_changes: vec![ChapterStateChange {
            entity_id: protagonist.id,
            event_type: ChapterStateEventType::Relationship,
            authority_excerpt: "沈砚与陆舟因争夺星盘而公开决裂。".to_string(),
            value: "沈砚与陆舟因争夺星盘而公开决裂。".to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    let relationships = &bible.structured_contract_v2.relationship_ledger;
    assert_eq!(relationships[0].current_state, "互相试探");
    assert_eq!(relationships[0].last_changed_chapter, None);
    assert!(relationships[1].current_state.contains("公开决裂"));
    assert_eq!(relationships[1].last_changed_chapter, Some(2));
}

#[test]
fn ambiguous_relationship_delta_does_not_modify_an_arbitrary_relation() {
    let mut contract = contract();
    contract.structured_contract_v2.relationship_ledger = vec![
        RelationshipLedgerEntry {
            character_ids: vec![
                "character-shen-yan".to_string(),
                "character-ji-lan".to_string(),
            ],
            characters: vec!["沈砚".to_string(), "季澜".to_string()],
            current_state: "互相试探".to_string(),
            ..Default::default()
        },
        RelationshipLedgerEntry {
            character_ids: vec![
                "character-shen-yan".to_string(),
                "character-lu-zhou".to_string(),
            ],
            characters: vec!["沈砚".to_string(), "陆舟".to_string()],
            current_state: "彼此戒备".to_string(),
            ..Default::default()
        },
    ];
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract,
        "now".to_string(),
    );
    let protagonist = bible
        .character_ledger
        .iter()
        .find(|character| character.name == "沈砚")
        .expect("沈砚 character anchor")
        .clone();
    let chapter = ApprovedChapterDelta {
        number: 2,
        state_changes: vec![ChapterStateChange {
            entity_id: protagonist.id,
            event_type: ChapterStateEventType::Relationship,
            authority_excerpt: "沈砚与同伴的关系发生改变。".to_string(),
            value: "沈砚不再信任同行者。".to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    let relationships = &bible.structured_contract_v2.relationship_ledger;
    assert_eq!(relationships[0].current_state, "互相试探");
    assert_eq!(relationships[0].last_changed_chapter, None);
    assert_eq!(relationships[1].current_state, "彼此戒备");
    assert_eq!(relationships[1].last_changed_chapter, None);
}

#[test]
fn approved_chapter_registers_only_explicit_characters_and_does_not_infer_world_facts() {
    let mut bible = build_story_bible(
        "试验书",
        "zh-cn",
        "赛博朋克玄幻",
        "测试",
        &StoryContract {
            premise: String::new(),
            themes: Vec::new(),
            characters: Vec::new(),
            world_rules: Vec::new(),
            style_rules: Vec::new(),
            must_avoid: Vec::new(),
            outline: String::new(),
            structured_contract_v2: NovelContractV2::default(),
            authority_contract: None,
            updated_at: String::new(),
        },
        "now".to_string(),
    );
    let chapter = ApprovedChapterDelta {
        number: 1,
        title: "第1章".to_string(),
        summary: "陆远与神秘少女在废弃工业区遭遇追捕。".to_string(),
        unit_count: 3200,
        key_facts: vec![
            "世界设定：灵气通过义体转化，过载会导致灵毒。".to_string(),
            "主角现状：陆远，下城区拾荒者。".to_string(),
            "少女解释了她可以感知频率。".to_string(),
        ],
        continuity_updates: Vec::new(),
        state_changes: Vec::new(),
        character_registrations: vec![ChapterCharacterRegistration {
            canonical_name: "陆远".to_string(),
            role: "主角".to_string(),
            narrative_purpose: "第一章正式登场".to_string(),
            ..Default::default()
        }],
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    assert!(bible
        .character_ledger
        .iter()
        .any(|character| character.name == "陆远"));
    assert!(!bible
        .character_ledger
        .iter()
        .any(|character| character.name == "少女"));
    assert!(!bible
        .character_ledger
        .iter()
        .any(|character| character.name.contains("赛博朋克")));
    assert!(
        bible.world_database.rules.is_empty(),
        "display key facts must not create durable world rules"
    );
}

#[test]
fn completion_gate_reports_unpaid_key_hooks() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    bible.hook_ledger.push(HookLedgerEntry {
        id: "hook-extra".to_string(),
        title: "关键秘密：星门真相".to_string(),
        introduced_chapter: Some(2),
        introduced_when: "chapter-0002".to_string(),
        knowers: vec!["沈砚".to_string()],
        reader_knows: "星门背后有关键秘密".to_string(),
        planned_payoff_window: "终局前".to_string(),
        planned_payoff_chapter: None,
        payoff_chapter: None,
        last_advanced_chapter: None,
        deferred_until_chapter: None,
        emotional_effect: "悬念".to_string(),
        status: HookStatus::Seeded,
        evidence: vec!["关键秘密：星门真相".to_string()],
    });

    let blockers = story_bible_completion_blockers(Some(&bible));

    assert!(blockers
        .iter()
        .any(|item| item.contains("unresolved debts")));
}

#[test]
fn progress_clue_does_not_create_completion_debt() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    let chapter = ApprovedChapterDelta {
        number: 4,
        title: "信号校准".to_string(),
        summary: "沈砚确认外部信号与飞船动力脉冲同步。".to_string(),
        unit_count: 2500,
        key_facts: vec!["线索进展：确认了外部信号与飞船动力脉冲同步。".to_string()],
        continuity_updates: Vec::new(),
        ..Default::default()
    };

    apply_approved_chapter_delta(&mut bible, &chapter, "later".to_string());

    assert!(bible
        .hook_ledger
        .iter()
        .all(|hook| hook.id.starts_with("ending-")));
    assert!(bible
        .hook_ledger
        .iter()
        .all(|hook| !hook.title.contains("外部信号")));
}

#[test]
fn unresolved_clue_remains_completion_debt() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    bible.hook_ledger.push(HookLedgerEntry {
        id: "hook-extra".to_string(),
        title: "线索进展：核心航向权限尚未夺回。".to_string(),
        introduced_chapter: Some(8),
        introduced_when: "chapter-0008".to_string(),
        knowers: vec!["沈砚".to_string()],
        reader_knows: "核心航向权限尚未夺回。".to_string(),
        planned_payoff_window: "终局前".to_string(),
        planned_payoff_chapter: None,
        payoff_chapter: None,
        last_advanced_chapter: None,
        deferred_until_chapter: None,
        emotional_effect: "主线压力".to_string(),
        status: HookStatus::Seeded,
        evidence: vec!["核心航向权限尚未夺回。".to_string()],
    });

    let blockers = story_bible_completion_blockers(Some(&bible));

    assert!(blockers
        .iter()
        .any(|item| item.contains("unresolved debts")));
}

#[test]
fn rebuilding_from_the_same_approved_history_is_idempotent() {
    let chapter = ApprovedChapterDelta {
        number: 1,
        title: "入学试".to_string(),
        summary: "沈砚通过入学试并发现星门裂纹。".to_string(),
        unit_count: 2500,
        key_facts: vec!["沈砚保住同伴。".to_string()],
        continuity_updates: vec!["伏笔：星门裂纹只有沈砚看见。".to_string()],
        ..Default::default()
    };
    let first = rebuild_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        std::slice::from_ref(&chapter),
        "same-time".to_string(),
    );
    let second = rebuild_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        &[chapter],
        "same-time".to_string(),
    );

    assert_eq!(
        serde_json::to_value(&first).expect("serialize first bible"),
        serde_json::to_value(&second).expect("serialize second bible")
    );
}

#[test]
fn rebuilding_contains_only_contract_and_approved_history() {
    let approved = ApprovedChapterDelta {
        number: 1,
        title: "入学试".to_string(),
        summary: "第一章已批准。".to_string(),
        unit_count: 2500,
        ..Default::default()
    };

    let rebuilt = rebuild_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        &[approved],
        "after".to_string(),
    );

    assert!(rebuilt.narrative_graph.chapter_goals.is_empty());
    assert_eq!(rebuilt.last_rebuilt_chapter, Some(1));
}

#[test]
fn display_theme_mentions_do_not_mutate_durable_theme_state() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "玄幻",
        "brief",
        &contract(),
        "before".to_string(),
    );
    let unrelated = ApprovedChapterDelta {
        number: 1,
        title: "入学试".to_string(),
        summary: "沈砚通过考试。".to_string(),
        unit_count: 2500,
        ..Default::default()
    };
    apply_approved_chapter_delta(&mut bible, &unrelated, "one".to_string());
    assert_eq!(bible.theme_ledger[0].last_touched_chapter, None);

    let related = ApprovedChapterDelta {
        number: 2,
        title: "代价".to_string(),
        summary: "沈砚面对选择与代价。".to_string(),
        unit_count: 2500,
        ..Default::default()
    };
    apply_approved_chapter_delta(&mut bible, &related, "two".to_string());
    assert_eq!(bible.theme_ledger[0].last_touched_chapter, None);
}

#[test]
fn legacy_story_bible_defaults_migrate_to_current_schema() {
    let mut bible = StoryBible {
        schema_version: "benshu.story_bible.v1".to_string(),
        structured_contract_v2: NovelContractV2 {
            revision: 7,
            ..Default::default()
        },
        ..Default::default()
    };

    ensure_bible_defaults(&mut bible);

    assert_eq!(bible.schema_version, STORY_BIBLE_VERSION);
    assert_eq!(bible.source_contract_revision, 7);
    assert!(!bible.narrative_graph.reverse_design_notes.is_empty());
}

#[test]
fn explicit_hook_lifecycle_advances_defers_overdues_and_pays_off() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    bible.hook_ledger.clear();
    bible.hook_ledger.push(HookLedgerEntry {
        id: "hook-core".to_string(),
        title: "失踪的航向密钥".to_string(),
        planned_payoff_chapter: Some(5),
        status: HookStatus::Seeded,
        ..Default::default()
    });

    let hook_delta = |number, event_type, value: &str, defer_until_chapter| ApprovedChapterDelta {
        number,
        state_changes: vec![ChapterStateChange {
            entity_id: "hook-core".to_string(),
            event_type,
            value: value.to_string(),
            allowance: StateChangeAllowance::Contract,
            defer_until_chapter,
            ..Default::default()
        }],
        ..Default::default()
    };
    apply_approved_chapter_delta(
        &mut bible,
        &hook_delta(
            3,
            ChapterStateEventType::HookAdvance,
            "沈砚确认航向密钥曾在旧舰桥出现。",
            None,
        ),
        "chapter-3".to_string(),
    );
    assert!(matches!(bible.hook_ledger[0].status, HookStatus::Advancing));
    assert_eq!(bible.hook_ledger[0].last_advanced_chapter, Some(3));

    apply_approved_chapter_delta(
        &mut bible,
        &hook_delta(
            4,
            ChapterStateEventType::HookDefer,
            "舰桥坍塌迫使搜寻延后。",
            Some(7),
        ),
        "chapter-4".to_string(),
    );
    assert!(matches!(bible.hook_ledger[0].status, HookStatus::Deferred));
    apply_approved_chapter_delta(
        &mut bible,
        &ApprovedChapterDelta {
            number: 8,
            ..Default::default()
        },
        "chapter-8".to_string(),
    );
    assert!(matches!(bible.hook_ledger[0].status, HookStatus::Overdue));

    apply_approved_chapter_delta(
        &mut bible,
        &hook_delta(
            9,
            ChapterStateEventType::HookPayOff,
            "沈砚从坍塌舰桥取回航向密钥。",
            None,
        ),
        "chapter-9".to_string(),
    );
    assert!(matches!(bible.hook_ledger[0].status, HookStatus::PaidOff));
    assert_eq!(bible.hook_ledger[0].payoff_chapter, Some(9));
}

#[test]
fn approved_typed_hook_seed_reuses_stable_id_instead_of_creating_a_duplicate() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    bible.hook_ledger.clear();
    let seed = ApprovedChapterDelta {
        number: 2,
        state_changes: vec![ChapterStateChange {
            entity_id: "hook-bridge".to_string(),
            event_type: ChapterStateEventType::HookSeed,
            value: "旧舰桥印记".to_string(),
            allowance: StateChangeAllowance::Contract,
            ..Default::default()
        }],
        ..Default::default()
    };
    apply_approved_chapter_delta(&mut bible, &seed, "first".to_string());
    apply_approved_chapter_delta(&mut bible, &seed, "replay".to_string());

    assert_eq!(bible.hook_ledger.len(), 1);
}

#[test]
fn typed_ending_debt_ids_are_stable_and_only_typed_payoff_clears_them() {
    let mut bible = build_story_bible(
        "星门试炼",
        "zh-CN",
        "科幻",
        "brief",
        &contract(),
        "now".to_string(),
    );
    let ending_debts = story_bible_completion_debts(Some(&bible))
        .into_iter()
        .filter(|debt| debt.id.starts_with("ending-"))
        .collect::<Vec<_>>();
    assert!(!ending_debts.is_empty());
    assert!(ending_debts
        .iter()
        .all(|debt| bible.hook_ledger.iter().any(|hook| hook.id == debt.id)));

    for debt in &ending_debts {
        apply_approved_chapter_delta(
            &mut bible,
            &ApprovedChapterDelta {
                number: 12,
                state_changes: vec![ChapterStateChange {
                    entity_id: debt.id.clone(),
                    event_type: ChapterStateEventType::HookPayOff,
                    value: format!("正文已兑现：{}", debt.title),
                    allowance: StateChangeAllowance::Contract,
                    ..Default::default()
                }],
                ..Default::default()
            },
            "chapter-12".to_string(),
        );
    }

    let remaining_ids = story_bible_completion_debts(Some(&bible))
        .into_iter()
        .map(|debt| debt.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(ending_debts
        .iter()
        .all(|debt| !remaining_ids.contains(&debt.id)));
}
