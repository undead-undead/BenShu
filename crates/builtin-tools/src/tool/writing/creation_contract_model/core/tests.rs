use crate::tool::writing::novel_contract_v2::{CharacterVoiceProfile, PowerProgression};

use super::*;

#[test]
fn parses_and_validates_strong_fiction_contract_json() {
    let raw = r#"{
            "title": {
                "canonical_title": "夜校灵轨",
                "rationale": "夜校来自主角起点，灵轨来自终局中城市灵脉轨道被重新接通的不可逆选择。",
                "source": "llm_contract"
            },
            "language": "中文",
            "genre": "都市玄幻",
            "brief": "普通学生卷入城市灵脉复苏。",
            "premise": "旧城夜校下方的灵脉轨道重启，普通学生必须在考试和城市危机之间选择。",
            "ending": {
                "desired_resolution": "秦知安接通灵轨，守住城市也守住自己的普通生活。",
                "final_state": "夜校成为新的守城入口。"
            },
            "protagonist_arc": "从只想通过考试的旁观者，成长为愿意承担城市代价的守门人。",
            "world_imagery": "夜校、灵脉轨道、雨夜站台",
            "main_causal_spine": "灵轨复苏引发考试异常，主角追查真相，最终以个人选择修复城市秩序。",
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
                    "canonical_name": "岑望舒",
                    "role": "关键同伴",
                    "desire": "查清夜校资格异常",
                    "fear": "真相牵连家人",
                    "bottom_line": "不伪造证据",
                    "arc_start": "谨慎旁观",
                    "arc_end": "共同公开证据"
                }
            ],
            "world_rules": ["灵轨只能由承担代价的人接通"],
            "themes": ["普通人对公共责任的承担"],
            "style_rules": ["保持近距离第三人称视角，线索通过行动逐步显现"],
            "must_avoid": ["不用偶然获得的新能力解决终局危机"],
            "structured": {"narration_contract":{"pov":"第三人称有限视角"}},
            "outline": {
                "raw_outline": "第一阶段：夜校异常；第二阶段：灵轨追查；第三阶段：终局接通。",
                "volumes": [
                    {"title": "夜校异常", "objective": "确认考试异常来自灵轨", "ending_change": "秦知安取得被篡改的灵籍证据"},
                    {"title": "灵轨重启", "objective": "秦知安公开证据并接通灵轨守住城市", "ending_change": "秦知安接通灵轨，夜校成为不可逆的新守城入口"}
                ],
                "near_chapters": [
                    {"number": 1, "title": "雨夜补考", "goal": "秦知安进入夜校并发现考试读数异常", "expected_turn": "异常读数指向地下灵轨"},
                    {"number": 2, "title": "旧站回声", "goal": "秦知安追查地下灵轨入口", "expected_turn": "岑望舒交出被删改的资格记录"},
                    {"number": 3, "title": "灵籍缺页", "goal": "两人核对资格记录与灵籍账册", "expected_turn": "缺页证明校盟篡改资格并留下追查债务"}
                ]
            }
        }"#;
    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let report = contract.validate();
    assert!(report.is_ready(), "{:#?}", report.issues);
    assert_eq!(contract.chapter_unit_target, None);
}

#[test]
fn normalize_world_rules_drops_numbered_heading_segments() {
    let raw = r#"{
            "title": {
                "canonical_title": "沉岛倒计时",
                "rationale": "沉岛倒计时来自三天内查明沉没原因并以引爆基站托起孤岛的终局压力。"
            },
            "language": "中文",
            "genre": "近未来海岛灾难悬疑",
            "brief": "孤岛潮汐系统失灵后，工程师必须在沉没前查清真相。",
            "premise": "近未来孤岛依赖潮汐能供电，系统故障让海水异常上涨。",
            "ending": {"desired_resolution": "林溯引爆海底基站托起孤岛，但永久切断与大陆的联系。"},
            "protagonist_arc": "从迷信数据的工程师，转变为能在混沌中承担代价的领袖。",
            "world_imagery": "潮汐能塔、幽蓝迷雾、海底遗迹",
            "main_causal_spine": "潮汐预测失灵->海底遗迹干扰->岛屿地基坍塌->林溯引爆基站托起孤岛",
            "characters": [
                {"canonical_name": "林溯", "role": "主角", "desire": "证明潮汐系统仍能救岛", "fear": "未知变量让所有人葬身海底", "bottom_line": "不隐瞒会危及居民生存的数据", "arc_start": "只相信模型", "arc_end": "承担失去大陆联系的代价"},
                {"canonical_name": "陈伯", "role": "关键同伴", "desire": "守住老岛民的生路", "fear": "岛民被新系统抛弃", "bottom_line": "不放弃任何被困居民", "arc_start": "抵触新技术", "arc_end": "把经验交给林溯"}
            ],
            "world_rules": [
                "规则1：潮汐能塔的能量输出与海底遗迹的呼吸频率严格同步",
                "当遗迹能量爆发导致潮汐预测失灵时，基站若强行超频供电，会加速海底空洞坍塌，代价是岛屿地基在48小时内出现结构性裂纹",
                "规则2：暴雨与迷雾具有导电性",
                "当雾色转为幽蓝时，海水温度骤降，淹没区的残骸会因静电吸附形成漂浮屏障，但会阻碍船只靠岸"
            ],
            "outline": {"raw_outline": "第一阶段停电查因；第二阶段海底遗迹真相；第三阶段引爆基站托起孤岛。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.world_rules.len(), 2);
    assert!(contract
        .world_rules
        .iter()
        .all(|rule| !rule.starts_with("规则")));
    assert!(contract
        .world_rules
        .iter()
        .any(|rule| rule.contains("48小时")));
}

#[test]
fn normalize_world_rules_drops_unstructured_heading_segments() {
    let mut rules = vec![
        "热能守恒代价。遗迹生物必须持续消耗热源维持体温。声波共振限制。任何超过阈值的机械噪音都会引发守卫攻击。".to_string(),
    ];

    normalize_world_rules_vec(&mut rules);

    assert_eq!(rules.len(), 2, "{rules:?}");
    assert!(rules.iter().all(|rule| rule.chars().count() > 8));
}

#[test]
fn normalize_world_rules_rejoins_dependent_cost_and_consequence_clauses() {
    let mut rules = vec![
        "义体改造需定期注入冷却液；否则神经会烧毁".to_string(),
        "商业信誉一旦破产；将永久失去大型竞标资格".to_string(),
        "天罡劲修炼越深；越需寒玉压制".to_string(),
    ];

    normalize_world_rules_vec(&mut rules);

    assert_eq!(rules.len(), 3, "{rules:?}");
    assert_eq!(rules[0], "义体改造需定期注入冷却液；否则神经会烧毁");
    assert_eq!(rules[1], "商业信誉一旦破产；将永久失去大型竞标资格");
    assert_eq!(rules[2], "天罡劲修炼越深；越需寒玉压制");
}

#[test]
fn normalize_world_rules_rejoins_dependent_array_entries() {
    let mut rules = vec![
        "情报传递必须经过两名中间人".to_string(),
        "否则坐标会暴露".to_string(),
    ];

    normalize_world_rules_vec(&mut rules);

    assert_eq!(rules, ["情报传递必须经过两名中间人；否则坐标会暴露"]);
}

#[test]
fn normalize_world_rules_preserves_complete_rules_beginning_with_yue_or_jiang() {
    let mut rules = vec![
        "越级挑战必须登记并支付十枚灵石".to_string(),
        "将军不得私自调动守城军".to_string(),
    ];

    normalize_world_rules_vec(&mut rules);

    assert_eq!(
        rules,
        ["越级挑战必须登记并支付十枚灵石", "将军不得私自调动守城军"]
    );
}

#[test]
fn normalize_world_rules_rejoins_split_conditional_consequences() {
    let mut rules = vec![
        "开发商若隐瞒地基缺陷".to_string(),
        "需承担双倍修复成本".to_string(),
        "社区书店享有优先承租权".to_string(),
    ];

    normalize_world_rules_vec(&mut rules);

    assert_eq!(
        rules,
        [
            "开发商若隐瞒地基缺陷；需承担双倍修复成本",
            "社区书店享有优先承租权"
        ]
    );
}

#[test]
fn contract_gate_rejects_unresolved_character_voice_placeholders() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校来自主角起点，灵轨来自终局中城市灵脉轨道被重新接通的不可逆选择。"},
            "language": "中文",
            "genre": "都市玄幻",
            "brief": "普通学生卷入城市灵脉复苏。",
            "premise": "旧城夜校下方的灵脉轨道重启，普通学生必须在考试和城市危机之间选择。",
            "ending": {"desired_resolution": "主角接通灵轨，守住城市也守住自己的普通生活。"},
            "protagonist_arc": "从只想通过考试的旁观者，成长为愿意承担城市代价的守门人。",
            "world_imagery": "夜校、灵脉轨道、雨夜站台",
            "main_causal_spine": "灵轨复苏引发考试异常，主角追查真相，最终以个人选择修复城市秩序。",
            "characters": [
                {"canonical_name": "秦知安", "role": "主角", "desire": "通过夜校考试改变生活", "fear": "再次被城市边缘化", "bottom_line": "不牺牲无辜同学换取晋级", "arc_start": "旁观自保", "arc_end": "主动守城"},
                {"canonical_name": "岑望舒", "role": "关键同伴", "desire": "查清夜校资格异常", "fear": "真相牵连家人", "bottom_line": "不伪造证据", "arc_start": "谨慎旁观", "arc_end": "共同公开证据"}
            ],
            "world_rules": ["灵轨只能由承担代价的人接通"],
            "outline": {"raw_outline": "第一阶段：夜校异常；第二阶段：灵轨追查；第三阶段：终局接通。"}
        }"#;
    let mut contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    contract.structured.character_voice_ledger = vec![CharacterVoiceProfile {
        character: "秦知安".to_string(),
        voice_style: "说话先克制观察，再在关键处露出锋芒。".to_string(),
        dialogue_rules: vec![
            "对白必须体现 `秦知安` 的欲望 `未明欲望`、恐惧 `未明恐惧` 或底线 `未明底线`。"
                .to_string(),
        ],
        ..Default::default()
    }];

    let issues = contract.validate().issues;

    assert!(
        issues.iter().any(|issue| issue.contains("角色声音表")),
        "{issues:?}"
    );
}

#[test]
fn normalization_preserves_explicit_voice_rules_for_typed_patch_authority() {
    let raw = r#"{
            "title": {"canonical_title": "海鸥号沉银录", "rationale": "海鸥号是主角承接的破旧帆船，沉银录指向寻找沉银航线并付出船员代价的终局。"},
            "language": "中文",
            "genre": "历史航海冒险",
            "brief": "落魄青年接手破船寻找沉银航线。",
            "premise": "债务迫使主角带领船员穿越危险海域。",
            "ending": {"desired_resolution": "主角失去大部分财富，却赢得船员忠诚并重建家族航线。"},
            "protagonist_arc": "从依赖家族背景到愿意承担船员命运。",
            "world_imagery": "破旧帆船、沉银残骸、浓雾海峡",
            "main_causal_spine": "债务迫使出海，沉银线索引发争夺，主角最终选择船员而非独占财富。",
            "characters": [
                {"canonical_name": "陶照声", "role": "主角", "desire": "还清家族债务", "fear": "因无能失去所有船员", "bottom_line": "绝不牺牲船员换取短期利益", "arc_start": "傲慢的继承人", "arc_end": "承担责任的船长"},
                {"canonical_name": "阮予宁", "role": "导师", "desire": "平安返乡", "fear": "死在无人知晓的海域", "bottom_line": "绝不交出备用舵轮", "arc_start": "只顾自保的老水手", "arc_end": "托付航线的领航者"}
            ],
            "world_rules": ["每次逆风转向都会加速船体损耗。"],
            "outline": {"raw_outline": "出海、争夺沉银线索、风暴抉择、归港重建航线。"},
            "structured": {
                "character_voice_ledger": [{
                    "character": "阮予宁",
                    "voice_style": "符合 `历史航海冒险` 的人物位置，但每次对白必须带出个人目标、信息或关系变化。",
                    "dialogue_rules": ["对白必须体现 `阮予宁` 的欲望 `平安返乡`、恐惧 `葬身海底` 或底线 `无论风向如何`。"]
                }]
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let voice = contract
        .structured
        .character_voice_ledger
        .iter()
        .find(|voice| voice.character == "阮予宁")
        .expect("voice");
    let rendered = voice.dialogue_rules.join(" ");

    assert!(rendered.contains("无论风向如何"), "{voice:#?}");
    assert!(
        !rendered.contains("绝不交出备用舵轮"),
        "normalize must not synthesize voice rules from character anchors: {voice:#?}"
    );
}

#[test]
fn contract_gate_rejects_authority_name_glued_to_person_fragment() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校来自主角起点，灵轨来自终局中城市灵脉轨道被重新接通的不可逆选择。"},
            "language": "中文",
            "genre": "都市玄幻",
            "brief": "普通学生卷入城市灵脉复苏。",
            "premise": "旧城夜校下方的灵脉轨道重启，普通学生必须在考试和城市危机之间选择。",
            "ending": {"desired_resolution": "主角接通灵轨，守住城市也守住自己的普通生活。"},
            "protagonist_arc": "从只想通过考试的旁观者，成长为愿意承担城市代价的守门人。",
            "world_imagery": "夜校、灵脉轨道、雨夜站台",
            "main_causal_spine": "灵轨复苏引发考试异常，主角追查真相，最终以个人选择修复城市秩序。",
            "characters": [
                {"canonical_name": "秦知安", "role": "主角", "desire": "通过夜校考试改变生活", "fear": "再次被城市边缘化", "bottom_line": "不牺牲无辜同学换取晋级", "arc_start": "旁观自保", "arc_end": "主动守城"},
                {"canonical_name": "岑望舒", "role": "关键同伴", "desire": "查清夜校资格异常", "fear": "真相牵连家人", "bottom_line": "不伪造证据", "arc_start": "谨慎旁观", "arc_end": "共同公开证据"}
            ],
            "world_rules": ["灵轨只能由承担代价的人接通"],
            "outline": {"raw_outline": "第一阶段：夜校异常；第二阶段：灵轨追查；第三阶段：终局接通。"}
        }"#;
    let mut contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    contract.structured.antagonist_pressure.primary_pressure = "秦知安闻野片".to_string();

    let issues = contract.validate().issues;

    assert!(
        issues.iter().any(|issue| issue.contains("角色名拼接污染")),
        "{issues:?}"
    );
}

#[test]
fn normalize_keeps_missing_title_for_model_repair() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: String::new(),
            candidates: Vec::new(),
            rationale: String::new(),
            source: TitleSource::LlmContract,
        },
        language: "zh-CN".to_string(),
        genre: "异界修仙".to_string(),
        brief: "异界修仙，每章2500字，至少5万字起。".to_string(),
        premise: "凡人世界资质卓绝者意外陨落，转世异界觉醒前世记忆，踏上问道之路。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角突破天道桎梏，于九重天界建立全新修炼体系。".to_string(),
            final_state: "九重天界的命格秩序被改写。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从懵懂少年到逆天强者，最终承担重立修炼秩序的代价。".to_string(),
        world_imagery: "九重天界层层递进，功法典籍化作天象，修炼者以命格为根基构建社会秩序。"
            .to_string(),
        main_causal_spine: "主角因前世因果被灭门，转世后逐步揭开天地大劫真相，最终打破轮回桎梏。"
            .to_string(),
        outline: OutlineContract {
            volumes: vec![VolumeContract {
                title: "九重天阶".to_string(),
                objective: "主角跨过第一重天阶，确认命格秩序的代价。".to_string(),
                ending_change: "第一重天阶崩裂，旧命格开始松动。".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "主角在异界醒来，发现命格残缺并被迫踏入天阶试炼。".to_string(),
                expected_turn: "天阶试炼暴露前世因果，主角不能再退回凡俗身份。".to_string(),
            }],
            raw_outline: "第一卷跨过九重天阶，终局打破轮回桎梏。".to_string(),
        },
        ..Default::default()
    };

    contract.normalize();

    assert!(
        value_missing(&contract.title.canonical_title),
        "normalize must not invent a book title from story basis"
    );
    assert!(
        contract
            .validate()
            .issues
            .iter()
            .any(|issue| issue.contains("缺少可锁定书名")),
        "{:?}",
        contract.validate().issues
    );
}

#[test]
fn normalize_keeps_weak_existing_title_for_title_patch_repair() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "尘封道标".to_string(),
            candidates: Vec::new(),
            rationale: "《尘封道标》里的道标来自终局水晶道标，尘封指主角揭开被封存的城市灵气真相。"
                .to_string(),
            source: TitleSource::LlmContract,
        },
        language: "zh-CN".to_string(),
        genre: "都市玄幻".to_string(),
        brief: "都市玄幻，每章2500字，至少5万字。".to_string(),
        premise: "旧城区夜校地下的灵脉轨道被财阀封锁，底层青年必须夺回借灵证。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角在旧桥终局公开借灵证造假的证据，重建公平晋级秩序。"
                .to_string(),
            final_state: "夜校借灵制度被改写，草根学生获得真实考试资格。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从只想保住夜校名额的旁听生，变成愿意承担代价的秩序修补者。".to_string(),
        world_imagery: "旧桥、夜校、借灵证、灵网账册、水晶道标。".to_string(),
        main_causal_spine: "旁听生取得借灵证，发现校盟篡改灵籍，最终用旧桥账册反证规则。"
            .to_string(),
        outline: OutlineContract {
            volumes: vec![VolumeContract {
                title: "旧桥账册".to_string(),
                objective: "查明借灵证造假的第一条证据链。".to_string(),
                ending_change: "主角拿到旧桥账册副本。".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "主角在夜校补考前拿到异常借灵证。".to_string(),
                expected_turn: "异常借灵证暴露灵网规则漏洞。".to_string(),
            }],
            raw_outline: "主角从借灵证异常查到旧桥账册，终局公开证据重建秩序。".to_string(),
        },
        ..Default::default()
    };

    contract.normalize();

    assert_eq!(contract.title.canonical_title, "尘封道标");
    assert!(matches!(contract.title.source, TitleSource::LlmContract));
}

#[test]
fn normalize_does_not_replace_primary_authority_from_story_subject() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "豪门换脸游戏".to_string(),
            rationale: "换脸来自替身身份，游戏来自主角在豪门规则里反向布局的爽点。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "都市爽文".to_string(),
        brief: "普通女孩韩岑安被豪门选中成为弃妇梁予禾的完美替身。".to_string(),
        premise: "韩岑安因长相酷似豪门弃妇梁予禾，被苏家老爷子选中作为梁予禾的替身，进入苏家生活。"
            .to_string(),
        ending: EndingContract {
            desired_resolution: "主角季棠白取代梁予禾，成为豪门主母。".to_string(),
            final_state: "季棠白获得独立地位与财富。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "季棠白从依附者成长为掌控者。".to_string(),
        world_imagery: "苏家老宅、替身契约、订婚宴".to_string(),
        main_causal_spine: "季棠白被选中入局，逐步揭开豪门规则，最终在订婚宴反向破局。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "季棠白".to_string(),
                role: "主角".to_string(),
                desire: "摆脱卑微出身".to_string(),
                fear: "被识破替身身份后再次被抛弃".to_string(),
                bottom_line: "即便伪装也不伤害无辜者".to_string(),
                arc_start: "依附者".to_string(),
                arc_end: "掌控者".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "韩岑安".to_string(),
                role: "重要角色".to_string(),
                desire: "在豪门夹缝里活下来".to_string(),
                fear: "被当作影子抹掉".to_string(),
                bottom_line: "不再交出自我名字".to_string(),
                arc_start: "被迫入局".to_string(),
                arc_end: "主动破局".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "梁予禾".to_string(),
                role: "关键对手".to_string(),
                desire: "维持名媛身份".to_string(),
                fear: "被替身取代".to_string(),
                bottom_line: "不公开自己的弱点".to_string(),
                arc_start: "高位压迫".to_string(),
                arc_end: "公开失势".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["豪门身份必须通过公开场合验证。".to_string()],
        outline: OutlineContract {
            raw_outline: "季棠白入局后逐步取代梁予禾。".to_string(),
            volumes: vec![VolumeContract {
                title: "替身入局".to_string(),
                objective: "季棠白确认替身代价。".to_string(),
                ending_change: "季棠白无法退回局外。".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "季棠白进入苏家。".to_string(),
                expected_turn: "替身身份暴露第一层代价。".to_string(),
            }],
        },
        ..Default::default()
    };

    contract.normalize();

    let primary = contract
        .characters
        .iter()
        .find(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.as_str());
    assert_eq!(primary, Some("季棠白"));
    assert!(contract.protagonist_arc.contains("季棠白"));
    assert!(contract.ending.desired_resolution.contains("季棠白"));
    assert!(contract.outline.raw_outline.contains("季棠白入局"));
}

#[test]
fn normalize_preserves_unknown_relationship_characters_for_typed_gate() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校灵轨".to_string(),
            rationale: "夜校来自主角起点，灵轨来自终局中城市灵脉轨道被重新接通的不可逆选择。"
                .to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "都市玄幻".to_string(),
        brief: "都市玄幻，每章2500字，至少5万字。".to_string(),
        premise: "旧城夜校下方的灵脉轨道重启。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角接通灵轨，守住城市也守住自己的普通生活。".to_string(),
            final_state: "夜校成为新的守城入口。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从旁观自保到主动守城。".to_string(),
        world_imagery: "夜校、灵脉轨道、雨夜站台".to_string(),
        main_causal_spine: "灵轨复苏引发考试异常，主角追查真相，最终修复城市秩序。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "秦知安".to_string(),
                role: "主角".to_string(),
                desire: "通过夜校考试改变生活".to_string(),
                fear: "再次被城市边缘化".to_string(),
                bottom_line: "不牺牲无辜同学换取晋级".to_string(),
                arc_start: "旁观自保".to_string(),
                arc_end: "主动守城".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "岑望舒".to_string(),
                role: "关键同伴".to_string(),
                desire: "查清夜校资格异常".to_string(),
                fear: "真相牵连家人".to_string(),
                bottom_line: "不伪造证据".to_string(),
                arc_start: "谨慎旁观".to_string(),
                arc_end: "共同公开证据".to_string(),
                ..Default::default()
            },
        ],
        structured: NovelContractV2 {
            relationship_ledger: vec![RelationshipLedgerEntry {
                characters: vec![
                    "秦知安".to_string(),
                    "岑望舒".to_string(),
                    "司晴".to_string(),
                ],
                relationship_type: "司晴共同追查夜校灵轨异常".to_string(),
                start_state: "互不信任".to_string(),
                desired_end_state: "共同公开证据".to_string(),
                conflicts: vec!["司晴掌握一份会误导灵轨调查的证词".to_string()],
                secrets: vec!["总督并试图把夜校事故嫁祸给学生".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    };

    contract.normalize();

    let characters = &contract.structured.relationship_ledger[0].characters;
    assert_eq!(characters.len(), 3);
    assert!(characters.iter().any(|name| name == "司晴"));
    let relation = &contract.structured.relationship_ledger[0];
    assert!(relation.relationship_type.contains("司晴"));
    assert!(relation
        .conflicts
        .iter()
        .any(|value| value.contains("司晴")));
    let report = contract.validate();
    assert!(
        report.issues.iter().any(|issue| {
            issue.contains("关系账本") && issue.contains("角色权威表之外") && issue.contains("司晴")
        }),
        "typed validation must preserve and report unknown relationship participants: {:?}",
        report.issues
    );
}

#[test]
fn normalize_repairs_authority_name_with_single_cjk_tail_noise() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "荒驿鬼车".to_string(),
            rationale: "鬼车是关键物件，荒驿是终局归宿。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "民国公路奇幻".to_string(),
        brief: "民国公路奇幻，每章2500字，一共5万字。".to_string(),
        premise: "陆沉舟舟继承一辆能驶入阴阳夹缝的鬼车。".to_string(),
        ending: EndingContract {
            desired_resolution: "陆沉舟舟牺牲鬼车核心，成为新的守驿人。".to_string(),
            final_state: "陆沉舟舟留在荒驿，守住精魂归途。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "陆沉舟舟从旁观自保到主动守护。".to_string(),
        world_imagery: "鬼车、荒驿、废弃铁路".to_string(),
        main_causal_spine: "陆沉舟舟启动鬼车，然后沿途追查家族诅咒。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "陆沉舟".to_string(),
                role: "主角".to_string(),
                desire: "寻找失踪未婚妻".to_string(),
                fear: "继承父亲失败命运".to_string(),
                bottom_line: "不牺牲无辜精魂".to_string(),
                arc_start: "冷漠自保".to_string(),
                arc_end: "主动守护".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "苏婉".to_string(),
                role: "关键同伴".to_string(),
                desire: "重返阳间".to_string(),
                fear: "精魂消散".to_string(),
                bottom_line: "不化厉鬼伤人".to_string(),
                arc_start: "隐瞒真相".to_string(),
                arc_end: "共同渡魂".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["鬼车每次穿梭都要消耗魂火，否则乘客会困在夹缝中。".to_string()],
        structured: NovelContractV2 {
            relationship_ledger: vec![RelationshipLedgerEntry {
                characters: vec!["陆沉舟".to_string(), "苏婉".to_string()],
                relationship_type: "苏婉舟牺牲自身魂火帮助陆沉舟。".to_string(),
                current_state: "陆沉舟舟仍然怀疑苏婉。".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    };

    contract.normalize();

    let serialized = serde_json::to_string(&contract).expect("contract json");
    assert!(!serialized.contains("陆沉舟舟"), "{serialized}");
    assert!(!serialized.contains("苏婉舟"), "{serialized}");
    assert!(serialized.contains("陆沉舟"), "{serialized}");
    assert!(serialized.contains("苏婉"), "{serialized}");
}

#[test]
fn normalize_repairs_secondary_authority_tail_noise_across_story_surfaces() {
    let mut contract = NovelCreationContract {
        brief: "顾青禾与同伴沈砚舟岚携手查清旧案。".to_string(),
        premise: "沈砚舟岚的证词让顾青禾找到突破口。".to_string(),
        ending: EndingContract {
            desired_resolution: "沈砚舟岚平定风波，并与顾青禾达成和解。".to_string(),
            must_resolve: vec!["顾青禾发现线索->沈砚舟岚遭遇伏击->两人公开真相".to_string()],
            ..Default::default()
        },
        main_causal_spine: "顾青禾接案->沈砚舟岚调查账册->沈砚舟岚功高受忌->旧案昭雪".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "顾青禾".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "沈砚舟".to_string(),
                role: "同伴".to_string(),
                bottom_line: "不因权势放弃证据".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["沈砚舟岚若隐瞒证据，案卷效力就会失效。".to_string()],
        ..Default::default()
    };

    contract.normalize();

    let serialized = serde_json::to_string(&contract).expect("contract json");
    assert!(!serialized.contains("沈砚舟岚"), "{serialized}");
    assert!(serialized.contains("沈砚舟携手"), "{serialized}");
    assert!(serialized.contains("沈砚舟的证词"), "{serialized}");
    assert!(serialized.contains("沈砚舟若隐瞒"), "{serialized}");
}

#[test]
fn normalize_preserves_valid_word_start_after_authority_name() {
    let mut contract = NovelCreationContract {
        premise: "顾青禾会为真相承担代价，也正在调查旧案。".to_string(),
        characters: vec![CharacterContract {
            canonical_name: "顾青禾".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };

    contract.normalize();

    assert_eq!(contract.premise, "顾青禾会为真相承担代价，也正在调查旧案。");
}

#[test]
fn primary_name_authority_ignores_protagonist_concept_phrases() {
    let names = explicit_primary_names_in_contract_text(
        "主角成长由旁观自保走向主动承担，主角弧线围绕守住选择展开。",
    );
    assert!(
        names.is_empty(),
        "concept words such as 成长/弧线 must not be treated as protagonist names: {names:?}"
    );

    let names =
        explicit_primary_names_in_contract_text("主角为突破境界不断寻找机缘，最终改写修行规则。");
    assert!(
        names.is_empty(),
        "goal phrases after 主角为 must not be treated as protagonist names: {names:?}"
    );

    let names = explicit_primary_names_in_contract_text("主角：季庭序在终局承担镜渊代价。");
    assert_eq!(names, vec!["季庭序"]);

    let names = explicit_primary_names_in_contract_text("主角名为季庭序，在终局承担镜渊代价。");
    assert_eq!(names, vec!["季庭序"]);

    let names = explicit_primary_names_in_contract_text(
        "结局：主角秦闻遥成为隐形的城市之王，所有曾经欺辱过他的人要么臣服。",
    );
    assert_eq!(names, vec!["秦闻遥"]);

    let names = explicit_primary_names_in_contract_text(
        "故事前提：主角辛岑遥原本只是城市边缘的普通人，后来被卷入核心冲突。",
    );
    assert_eq!(names, vec!["辛岑遥"]);

    let names =
        explicit_primary_names_in_contract_text("分卷目标：主角陶予声统领旧城账户联盟并公开证据。");
    assert_eq!(names, vec!["陶予声"]);

    let names = explicit_primary_names_in_contract_text("前提：主角祝栖序名普通的线路检修员。");
    assert_eq!(names, vec!["祝栖序"]);

    let names = explicit_primary_names_in_contract_text(
        "故事前提：主角林默因父债被赶出豪门，意外获得能显示世间万物真实价值的云端账本。",
    );
    assert_eq!(
        names,
        vec!["林默"],
        "artifact/world terms after the protagonist action must not be treated as stale names"
    );
    assert!(!stale_primary_name_candidate_looks_like_person("云端账本"));
}

#[test]
fn normalizes_compound_world_rules_into_actionable_items() {
    let raw = r#"{
            "title": {
                "canonical_title": "账本翻盘局",
                "rationale": "账本来自关键物件，翻盘局来自主角用代价规则反杀金融围剿的终局爽点。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "都市爽文",
            "brief": "落魄青年获得云端账本后反制金融围剿。",
            "premise": "林默因父债被赶出豪门，意外获得能显示世间万物真实价值的云端账本。",
            "ending": {
                "desired_resolution": "林默公开账本代价规则，反杀幕后黑手并重建交易秩序。",
                "final_state": "新交易规则保护普通人不再被资本围猎。"
            },
            "protagonist_arc": "从只想还债的自保者，成长为愿意公开代价规则的秩序修补者。",
            "world_imagery": "云端账本、霓虹金融城、破旧出租屋",
            "main_causal_spine": "父债逼迫主角入局，云端账本揭示价值弱点，主角利用代价规则反制围剿，终局公开账本真相。",
            "characters": [
                {"canonical_name": "林默", "role": "主角", "desire": "还清父债并掌控自己的命运", "fear": "再次被资本规则吞噬", "bottom_line": "不牺牲无辜者换取翻盘", "arc_start": "负债自保", "arc_end": "公开规则"},
                {"canonical_name": "苏清歌", "role": "关键同伴", "desire": "重塑家族公司声誉", "fear": "被旧资本规则吞掉", "bottom_line": "不做假账", "arc_start": "谨慎观望", "arc_end": "共同公开证据"}
            ],
            "world_rules": ["万物皆有标价，洞察弱点必须支付记忆或寿命代价；超额收益必须在72小时内用因果闭环偿还，否则资产自动归零；云端账本长期闲置会吞噬持有者潜意识"],
            "outline": {"raw_outline": "第一卷还债入局；第二卷金融围剿；终局公开账本规则。"}
        }"#;
    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    assert_eq!(contract.world_rules.len(), 3, "{:?}", contract.world_rules);
    assert!(contract
        .world_rules
        .iter()
        .any(|rule| rule.contains("洞察弱点必须支付")));
    assert!(contract
        .world_rules
        .iter()
        .all(|rule| !crate::tool::writing::typed_contract_gate::world_rule_looks_truncated_or_not_actionable(rule)),
        "{:?}",
        contract.world_rules);
}

#[test]
fn parses_common_model_json_shape_drift_without_losing_contract_fields() {
    let raw = r#"{
            "title": {
                "canonical_title": "旧桥灵证",
                "candidates": ["旧桥灵证", "夜校借灵证"],
                "rationale": "旧桥是终局公开证据并重建秩序的地点，灵证是主角反转夜校借灵规则的关键物件。",
                "source": "contract_generation"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "都市玄幻，每章2500字，至少5万字起。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "旧城区夜校用借灵证筛选草根学生，主角发现规则背后正在吞噬凡人气运。",
            "ending": {
                "desired_resolution": "主角在旧桥终局公开借灵证造假的证据，重建公平晋级秩序。",
                "final_state": "夜校借灵制度被改写，草根学生获得真实考试资格。",
                "must_resolve": "借灵证造假；旧桥灵网漏洞",
                "allowed_open_questions": "新制度是否仍会产生新的灰色交易"
            },
            "protagonist_arc": "从只想保住夜校名额的旁听生，变成愿意承担代价的秩序修补者。",
            "world_imagery": "旧桥、夜校、借灵证、灵网账册。",
            "main_ca_spine": "旁听生取得借灵证，发现校盟篡改灵籍，最终用旧桥账册反证规则。",
            "characters": [{
                "canonical_name": "许照桥",
                "role": "主角",
                "desire": "保住夜校资格并查清父亲旧案",
                "fear": "再次被规则抹掉姓名",
                "bottom_line": "不把同学当成晋级垫脚石",
                "arc_start": "只想自保的旁听生",
                "arc_end": "公开证据的规则修补者"
            }, {
                "canonical_name": "商砚衡",
                "role": "关键对手",
                "desire": "维护校盟对夜校考试的垄断",
                "fear": "灵籍账册被公开",
                "bottom_line": "不亲手毁掉考试系统",
                "arc_start": "幕后监考者",
                "arc_end": "被证据逼到台前"
            }],
            "themes": ["公平晋级", "规则代价"],
            "world_rules": "借灵证可以临时借用灵网资格；每次借用都会留下账册痕迹。",
            "style_rules": "节奏紧凑；场景具体；避免空泛口号。",
            "must_avoid": ["万能升级", "反派降智"],
            "outline": {
                "volumes": [{
                    "title": "旧桥账册",
                    "objective": "查明借灵证造假的第一条证据链。",
                    "ending_change": "主角拿到旧桥账册副本。"
                }],
                "near_chapters": [{
                    "number": 1,
                    "goal": "许照桥在夜校补考前拿到异常借灵证。",
                    "expected_turn": 1
                }, {
                    "number": 2,
                    "goal": "旧桥账册第一次显示父亲旧案编号。",
                    "expected_turn": 2
                }, {
                    "number": 3,
                    "goal": "校盟开始追查泄密者，主角被迫选择公开或隐藏证据。",
                    "expected_turn": 3
                }],
                "raw_outline": "第一卷围绕旧桥账册追证，第二卷公开校盟夺籍真相，终局重写夜校晋级规则。"
            }
        }"#;
    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    assert_eq!(
        contract.main_causal_spine,
        "旁听生取得借灵证，发现校盟篡改灵籍，最终用旧桥账册反证规则。"
    );
    assert_eq!(contract.outline.near_chapters.len(), 3);
    assert_eq!(contract.outline.near_chapters[0].expected_turn, "1");
    assert_eq!(contract.world_rules.len(), 2);
    assert!(
        !contract.validate().is_ready(),
        "{:?}",
        contract.validate().issues
    );
    assert!(contract
        .validate()
        .issues
        .iter()
        .any(|issue| issue.contains("数字占位")));
}

#[test]
fn parses_string_form_character_authority_rows() {
    let raw = r#"{
            "title": {
                "canonical_title": "旧桥灵证",
                "rationale": "旧桥是终局公开证据的地点，灵证是主角反转夜校规则的关键物。"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                "name: 许照桥; role: 主角; desire: 通过夜校考试并查清父亲旧案; fear: 再次被规则抹掉姓名; bottom_line: 不把同学当成晋级垫脚石; arc_start: 只想自保的旁听生; arc_end: 公开证据的规则修补者",
                "name: 商砚衡; role: 角色; desire: 维护校盟对夜校考试的垄断; fear: 灵籍账册被公开; bottom_line: 不亲手毁掉考试系统; arc_start: 幕后监考者; arc_end: 被证据逼到台前"
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "outline": {"near_chapters": [{"number": 1, "goal": "许照桥发现灵轨账册异常。", "expected_turn": "第一章"}], "raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.characters.len(), 2);
    assert_eq!(contract.characters[0].canonical_name, "许照桥");
    assert!(contract.characters[0].role_looks_primary());
    assert_eq!(contract.characters[1].canonical_name, "商砚衡");
    assert_eq!(contract.characters[1].role, "关键对手");
    assert_eq!(contract.characters[1].arc_start, "幕后监考者");
}

#[test]
fn rejects_chapter_label_as_expected_turn() {
    let raw = r#"{
            "title": {"canonical_title": "旧桥灵证", "rationale": "旧桥是终局公开证据的地点，灵证是主角反转夜校规则的关键物。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "通过夜校考试并查清父亲旧案", "fear": "再次被规则抹掉姓名", "bottom_line": "不把同学当成晋级垫脚石", "arc_start": "只想自保的旁听生", "arc_end": "公开证据的规则修补者"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护校盟垄断", "fear": "账册被公开", "bottom_line": "不亲手毁掉考试系统", "arc_start": "幕后监考者", "arc_end": "被证据逼到台前"}
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "outline": {"near_chapters": [{"number": 1, "goal": "许照桥发现灵轨账册异常。", "expected_turn": "第一章"}], "raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let issues = contract.validate().issues;

    assert!(issues.iter().any(|issue| issue.contains("只是章节标签")));
}

#[test]
fn allows_natural_primary_name_repetition_in_near_chapter_goal() {
    let raw = r#"{
            "title": {"canonical_title": "旧桥灵证", "rationale": "旧桥是终局公开证据的地点，灵证是主角反转夜校规则的关键物。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "通过夜校考试并查清父亲旧案", "fear": "再次被规则抹掉姓名", "bottom_line": "不把同学当成晋级垫脚石", "arc_start": "只想自保的旁听生", "arc_end": "公开证据的规则修补者"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护校盟垄断", "fear": "账册被公开", "bottom_line": "不亲手毁掉考试系统", "arc_start": "幕后监考者", "arc_end": "被证据逼到台前"}
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "outline": {"near_chapters": [{"number": 1, "goal": "许照桥目睹导师许照桥，确认灵轨账册异常。", "expected_turn": "主角确认账册被人篡改并失去退路。"}], "raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let issues = contract.validate().issues;

    assert!(!issues.iter().any(|issue| issue.contains("重复使用主角名")));
}

#[test]
fn rejects_glued_outline_control_blocks() {
    let raw = r#"{
            "title": {"canonical_title": "剥骨令", "rationale": "剥骨令是修士晋阶制度中的关键命令，终局主角公开再生法门并终结剥骨晋阶制度。"},
            "language": "zh-CN",
            "genre": "异界玄幻",
            "brief": "修士通过脊骨刻印换取灵力，主角要终结献祭制度。",
            "premise": "灵力必须通过脊骨刻印流转，普通修士被迫献祭身体换取晋阶机会。",
            "ending": {"desired_resolution": "主角公开脊骨再生法门，切断献祭制度对修士的控制。"},
            "protagonist_arc": "从只想保住自身脊骨的低阶修士，变成愿意公开法门的秩序重写者。",
            "world_imagery": "脊骨刻印、灵脉祭台、剥骨令、极夜矿镇。",
            "main_causal_spine": "低阶修士被迫刻印，发现剥骨令背后的垄断，终局公开再生法门终结献祭制度。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "保住自身脊骨并找到替代晋阶法门", "fear": "被剥骨令夺走身体和姓名", "bottom_line": "不献祭无辜修士换取力量", "arc_start": "低阶自保", "arc_end": "公开法门"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护剥骨令垄断", "fear": "再生法门被公开", "bottom_line": "不让低阶修士越过矿镇秩序", "arc_start": "矿镇执令者", "arc_end": "被制度反噬"}
            ],
            "world_rules": ["脊骨刻印越深，灵力越强，但肉身会持续衰败。"],
            "outline": {
                "volumes": [{"title":"剥骨矿镇","objective":"查出剥骨令的第一条证据。","ending_change":"主角得到再生法门残页。"}],
                "near_chapters": [{"number": 1, "goal": "许照桥被迫接受第一次刻印。", "expected_turn": "他发现刻印会吞掉身份印记。"}],
                "raw_outline": "第一阶段：许照桥在极夜矿镇被迫刻印。第二阶段：寻找再生法门并与商砚衡对抗。第三阶段：公开法门打破剥骨令第1卷《剥骨矿镇》：展示残酷规则；卷尾变化：拿到残页第2卷《灵脉逆证》：追查制度漏洞；卷尾变化：公开证据第1章 本章目标：第一次刻印；预期转折：开篇第2章 本章目标：刻印反噬；预期转折：冲突爆发第3章 本章目标：进入矿镇执令堂；预期转折：背上新债。"
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let issues = contract.validate().issues;

    assert!(
        issues.iter().any(|issue| issue.contains("胶合")),
        "{issues:?}"
    );
}

#[test]
fn normalizes_total_chapter_estimate_out_of_turn_count() {
    let raw = r#"{
            "title": {
                "canonical_title": "旧桥借灵证",
                "rationale": "旧桥是终局公开证据并重建秩序的地点，借灵证是主角反转夜校借灵规则的关键物件。"
            },
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 20,
            "premise": "旧城区夜校用借灵证筛选草根学生。",
            "ending": {"desired_resolution": "主角在旧桥终局公开借灵证造假的证据，重建公平晋级秩序。"},
            "protagonist_arc": "从只想保住夜校名额的旁听生，变成愿意承担代价的秩序修补者。",
            "world_imagery": "旧桥、夜校、借灵证、灵网账册。",
            "main_causal_spine": "旁听生取得借灵证，发现校盟篡改灵籍，最终用旧桥账册反证规则。",
            "characters": [{
                "canonical_name": "许照桥",
                "role": "主角",
                "desire": "保住夜校资格",
                "fear": "再次被规则抹掉姓名",
                "bottom_line": "不把同学当成晋级垫脚石",
                "arc_start": "只想自保的旁听生",
                "arc_end": "公开证据的规则修补者"
            }, {
                "canonical_name": "商砚衡",
                "role": "关键对手",
                "desire": "维护校盟对借灵证的垄断",
                "fear": "旧桥账册证据被公开",
                "bottom_line": "不亲手毁掉夜校秩序",
                "arc_start": "幕后监考者",
                "arc_end": "被证据逼到台前"
            }],
            "world_rules": ["借灵证可以临时借用灵网资格；每次借用都会留下账册痕迹。"],
            "outline": {
                "volumes": [{
                    "title": "旧桥账册",
                    "objective": "追查借灵证造假的第一组证据",
                    "ending_change": "许照桥确认夜校资格被校盟账册篡改"
                }],
                "near_chapters": [
                    {"number":1,"goal":"许照桥在旧桥拿到借灵证并发现账册痕迹","expected_turn":"确认借灵证不是普通资格凭证"},
                    {"number":2,"goal":"追查夜校名额被篡改的证据","expected_turn":"线索指向校盟监考系统"}
                ],
                "raw_outline": "第一卷围绕旧桥账册追证，第二卷公开校盟夺籍真相，终局重写夜校晋级规则。"
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.max_chapters_per_turn, Some(1));
}

#[test]
fn blocks_contract_without_primary_character() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校灵轨".to_string(),
            rationale: "夜校来自主角起点，灵轨来自终局选择。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        ending: EndingContract {
            desired_resolution: "主角守住夜校灵轨。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从旁观到承担。".to_string(),
        world_imagery: "夜校灵轨".to_string(),
        main_causal_spine: "灵轨异常推动考试和守城冲突。".to_string(),
        world_rules: vec!["灵轨接通需要代价。".to_string()],
        outline: OutlineContract {
            raw_outline: "第一阶段：异常；第二阶段：追查。".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();
    assert!(contract
        .validate()
        .issues
        .iter()
        .any(|issue| issue.contains("缺少明确主角")));
}

#[test]
fn blocks_contract_with_only_primary_character() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校灵轨".to_string(),
            rationale: "夜校来自主角起点，灵轨来自终局选择。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        ending: EndingContract {
            desired_resolution: "主角切断夜校灵轨，公开借灵证并守住普通学生。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从只想自保到愿意公开证据。".to_string(),
        world_imagery: "夜校、灵轨、借灵证、旧桥账册。".to_string(),
        main_causal_spine: "旁听生取得借灵证，追查账册，终局公开证据。".to_string(),
        characters: vec![CharacterContract {
            canonical_name: "许照桥".to_string(),
            role: "主角".to_string(),
            desire: "保住夜校资格".to_string(),
            fear: "再次被规则抹掉姓名".to_string(),
            bottom_line: "不把同学当成晋级垫脚石".to_string(),
            arc_start: "只想自保的旁听生".to_string(),
            arc_end: "公开证据的规则修补者".to_string(),
            aliases: Vec::new(),
            ..Default::default()
        }],
        world_rules: vec!["借灵证可以临时借用灵网资格。".to_string()],
        outline: OutlineContract {
            raw_outline: "第一阶段：异常；第二阶段：追查。".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();
    assert!(contract
        .validate()
        .issues
        .iter()
        .any(|issue| { issue.contains("缺少非主角关键角色") }));
}

#[test]
fn blocks_contract_with_json_or_markup_surface_pollution() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校灵轨".to_string(),
            rationale: "夜校来自主角起点，灵轨来自终局选择。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "中文".to_string(),
        ending: EndingContract {
            desired_resolution: "主角守住夜校灵轨。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从旁观到承担。".to_string(),
        world_imagery: "夜校灵轨".to_string(),
        main_causal_spine: "灵轨异常推动考试和守城冲突。".to_string(),
        characters: vec![CharacterContract {
            canonical_name: "秦知安".to_string(),
            role: "主角".to_string(),
            desire: "通过考试".to_string(),
            fear: "失败".to_string(),
            bottom_line: "不牺牲同学".to_string(),
            arc_start: "旁观".to_string(),
            arc_end: "守城".to_string(),
            ..Default::default()
        }],
        world_rules: vec!["灵轨接通需要代价。".to_string()],
        outline: OutlineContract {
            raw_outline: "第01章：异常。\\rightarrow$ 第02章：追查。".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();

    let report = contract.validate();

    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("LaTeX/转义/数学格式残片")));
}

#[test]
fn normalize_clears_structured_legal_contract_residue_before_validation() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校借灵证".to_string(),
            rationale: "夜校来自主角起点，借灵证来自终局公开规则黑账的关键物件。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "中文".to_string(),
        genre: "都市玄幻".to_string(),
        premise: "旁听生发现夜校借灵证会转移失败者运势。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角公开借灵证黑账，改写夜校晋级规则。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从被排斥的旁听生到公开黑账的守约人。".to_string(),
        world_imagery: "雨夜夜校、借灵证、旧城灵轨".to_string(),
        main_causal_spine: "旁听入场→发现借灵证→追查黑账→公开规则漏洞".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "秦知安".to_string(),
                role: "主角".to_string(),
                desire: "拿回考试资格".to_string(),
                fear: "再次被制度抹去".to_string(),
                bottom_line: "不牺牲同伴".to_string(),
                arc_start: "被排斥的旁听生".to_string(),
                arc_end: "公开黑账的守约人".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "沈青萝".to_string(),
                role: "关键同伴".to_string(),
                desire: "证明父亲案卷被篡改".to_string(),
                fear: "真相被买断".to_string(),
                bottom_line: "不伪造证据".to_string(),
                arc_start: "不信任秦知安".to_string(),
                arc_end: "共同公开黑账".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["借灵证每次借用都会留下账册痕迹。".to_string()],
        outline: OutlineContract {
            raw_outline: "秦知安进入夜校，发现借灵证黑账，最终公开账册改写晋级规则。".to_string(),
            volumes: vec![VolumeContract {
                title: "夜校入场".to_string(),
                objective: "拿到旁听资格并发现借灵证异常。".to_string(),
                ending_change: "秦知安确认黑账存在。".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "秦知安进入夜校考场，发现借灵证记录异常。".to_string(),
                expected_turn: "他拿到第一条被删改的账册编号。".to_string(),
            }],
        },
        structured: NovelContractV2 {
            power_progression: PowerProgression {
                system_name: "异界言情小说。第二条字数要求。乙方应于合同签订后完成初稿。"
                    .to_string(),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    contract.normalize();

    assert!(!contract
        .structured
        .power_progression
        .system_name
        .contains("乙方"));
    assert!(contract
        .validate()
        .issues
        .iter()
        .all(|issue| { !issue.contains("成长体系含有合同条款或交付协议残片") }));
}

#[test]
fn blocks_mechanical_causal_chain_and_numeric_chapter_turns() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夜校借灵".to_string(),
            rationale: "夜校来自补考起点，借灵来自终局公开考试吞噬运势的规则反转。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "都市玄幻".to_string(),
        brief: "旧城区旁听生卷入灵能考试黑幕。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角公开灵能考试黑幕并重写城市晋级规则。".to_string(),
            final_state: "旧城区学生获得公平考试入口。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从只想保住旁听名额，成长为愿意承担代价的规则改写者。".to_string(),
        world_imagery: "夜校考场、借灵证、旧城区雨巷。".to_string(),
        main_causal_spine: "失败补考引出晋级黑幕，然后追查证据，然后公开证据，然后改写规则。"
            .to_string(),
        characters: vec![CharacterContract {
            canonical_name: "许闻桥".to_string(),
            role: "主角".to_string(),
            desire: "通过灵能考试改变命运".to_string(),
            fear: "再次失去考试资格".to_string(),
            bottom_line: "不牺牲同学换取晋级".to_string(),
            arc_start: "旁听生".to_string(),
            arc_end: "规则改写者".to_string(),
            ..Default::default()
        }],
        world_rules: vec!["灵能考试会转移考生运势。".to_string()],
        outline: OutlineContract {
            raw_outline: "第01章：夜校补考。".to_string(),
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "许闻桥被迫参加夜校补考".to_string(),
                expected_turn: "1".to_string(),
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();

    let report = contract.validate();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("机械连接词链")),
        "{:?}",
        report.issues
    );
    assert!(
        report.issues.iter().any(|issue| issue.contains("数字占位")),
        "{:?}",
        report.issues
    );
}

#[test]
fn grounded_role_label_title_is_not_rewritten_during_normalization() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "裂纹观测者".to_string(),
            rationale: "裂纹来自终局中主角缝合城市逻辑裂纹的不可逆选择，观测者来自主角早期能力。"
                .to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "中文".to_string(),
        ending: EndingContract {
            desired_resolution: "主角放弃观测能力，缝合城市裂纹。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从追逐力量到承担城市代价。".to_string(),
        world_imagery: "城市裂纹、雨夜站台".to_string(),
        main_causal_spine: "观测裂纹引发记忆代价，最终以选择修补秩序。".to_string(),
        characters: vec![CharacterContract {
            canonical_name: "秦知安".to_string(),
            role: "主角".to_string(),
            desire: "获得观测能力".to_string(),
            fear: "失去记忆".to_string(),
            bottom_line: "不牺牲无辜者".to_string(),
            arc_start: "追逐力量".to_string(),
            arc_end: "承担代价".to_string(),
            ..Default::default()
        }],
        world_rules: vec!["观测裂纹必须支付记忆。".to_string()],
        outline: OutlineContract {
            raw_outline: "第一阶段：发现裂纹；第二阶段：代价扩大；第三阶段：终局缝合。".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();

    let report = contract.validate();

    assert_eq!(contract.title.canonical_title, "裂纹观测者");
    assert!(
        !report.issues.iter().any(|issue| issue.contains("书名")),
        "{:?}",
        report.issues
    );
}

#[test]
fn story_grounded_abstract_title_is_not_an_automatic_blocker() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "感知静默".to_string(),
            rationale: "《感知静默》中的感知对应主角能力，静默对应终局关闭静默塔后失去感知的代价。"
                .to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "中文".to_string(),
        ending: EndingContract {
            desired_resolution: "主角在静默塔关闭城市灵频阈值，牺牲感知换回秩序。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从追逐灵频力量的人变成守住城市边界的人。".to_string(),
        world_imagery: "霓虹、灵频阈值、静默塔。".to_string(),
        main_causal_spine: "灵频觉醒导致过载，静默塔暴露真相，主角在终局关闭阈值。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "秦知安".to_string(),
                role: "主角".to_string(),
                desire: "获得灵频感知能力".to_string(),
                fear: "失去普通生活".to_string(),
                bottom_line: "不牺牲无辜者".to_string(),
                arc_start: "追逐力量".to_string(),
                arc_end: "承担代价".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "沈砚棠".to_string(),
                role: "关键对手".to_string(),
                desire: "维持灵频阈值权限".to_string(),
                fear: "静默塔真相公开".to_string(),
                bottom_line: "不允许普通人越过城市边界".to_string(),
                arc_start: "阈值守门人".to_string(),
                arc_end: "被迫面对静默代价".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["灵频阈值每次开启都会吞噬一段记忆。".to_string()],
        outline: OutlineContract {
            raw_outline: "第一卷：发现灵频阈值；第二卷：追查静默塔；第三卷：关闭阈值。".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    contract.normalize();

    let report = contract.validate();

    assert_eq!(contract.title.canonical_title, "感知静默");
    assert!(
        !report.issues.iter().any(|issue| issue.contains("书名")),
        "{:?}",
        report.issues
    );
}

#[test]
fn normalization_does_not_replace_canonical_title_from_candidates() {
    let raw = r#"{
            "title": {
                "canonical_title": "霓虹下的裂纹",
                "candidates": ["霓虹下的裂纹", "旧桥借灵证", "夜雨余烬"],
                "rationale": "霓虹隐喻都市表象，裂纹指向终局选择。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "旧城区夜校通过借灵证筛选学生。",
            "premise": "旧城区夜校用借灵证筛选草根学生，借灵证背后藏着灵网账册。",
            "ending": {
                "desired_resolution": "主角在旧桥终局公开借灵证造假的证据，重建公平晋级秩序。"
            },
            "protagonist_arc": "从只想保住夜校名额的旁听生，变成愿意承担代价的秩序修补者。",
            "world_imagery": "旧桥、夜校、借灵证、灵网账册。",
            "main_causal_spine": "旁听生取得借灵证，发现校盟篡改灵籍，最终用旧桥账册反证规则。",
            "characters": [{
                "canonical_name": "许照桥",
                "role": "主角",
                "desire": "保住夜校资格并查清父亲旧案",
                "fear": "再次被规则抹掉姓名",
                "bottom_line": "不把同学当成晋级垫脚石",
                "arc_start": "只想自保的旁听生",
                "arc_end": "公开证据的规则修补者"
            }, {
                "canonical_name": "商砚衡",
                "role": "关键对手",
                "desire": "维护校盟对夜校考试的垄断",
                "fear": "灵籍账册被公开",
                "bottom_line": "不亲手毁掉考试系统",
                "arc_start": "幕后监考者",
                "arc_end": "被证据逼到台前"
            }],
            "world_rules": ["借灵证可以临时借用灵网资格；每次借用都会留下账册痕迹。"],
            "outline": {
                "volumes": [{
                    "title": "旧桥账册",
                    "objective": "追查借灵证造假的第一组证据",
                    "ending_change": "许照桥确认夜校资格被校盟账册篡改"
                }],
                "near_chapters": [
                    {"number":1,"goal":"许照桥在旧桥拿到借灵证并发现账册痕迹","expected_turn":"确认借灵证不是普通资格凭证"},
                    {"number":2,"goal":"追查夜校名额被篡改的证据","expected_turn":"线索指向校盟监考系统"}
                ],
                "raw_outline": "第一卷围绕旧桥账册追证，第二卷公开校盟夺籍真相，终局重写夜校晋级规则。"
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.title.canonical_title, "霓虹下的裂纹");
    assert!(matches!(contract.title.source, TitleSource::LlmContract));
    assert_eq!(contract.title.candidates[1], "旧桥借灵证");
}

#[test]
fn normalization_does_not_rewrite_quoted_outline_title() {
    let raw = r#"{
            "title": {
                "canonical_title": "都市神瞳",
                "candidates": ["古老符文", "异能秩序", "都市灵瞳"],
                "rationale": "书名来自世界观意象中的瞳孔异能体系和都市玄幻题材的结合。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "草根青年觉醒异瞳，在都市异能网络中追查符文秩序。",
            "premise": "古老符文寄生在都市瞳术体系中，主角通过异瞳看见权力集团隐藏的灵能账册。",
            "ending": {
                "desired_resolution": "主角摧毁都市异能统治体系，公开古老符文的代价，建立新的秩序平衡。",
                "final_state": "普通人重新获得选择权，主角守住新秩序。"
            },
            "protagonist_arc": "从只想自保的底层青年，变成敢公开符文真相的秩序重建者。",
            "world_imagery": "异瞳、古老符文、都市灵能账册、霓虹裂缝。",
            "main_causal_spine": "异瞳觉醒发现符文账册，追查异能集团，终局公开符文代价并改写都市秩序。",
            "characters": [{
                "canonical_name": "顾岑序",
                "role": "主角",
                "desire": "掌握异瞳能力并摆脱底层命运",
                "fear": "自己也成为吞噬普通人的异能集团一员",
                "bottom_line": "不牺牲无辜者换取力量",
                "arc_start": "被动求生",
                "arc_end": "主动重建秩序"
            }, {
                "canonical_name": "宁砚澜",
                "role": "关键配角",
                "desire": "查明家族符文账册真相",
                "fear": "真相会毁掉亲人",
                "bottom_line": "不伪造证据",
                "arc_start": "谨慎旁观",
                "arc_end": "公开站队"
            }],
            "world_rules": ["异瞳每次读取古老符文都会付出记忆代价。"],
            "outline": {
                "volumes": [
                    {"title":"符文初醒","objective":"揭示异瞳觉醒机制与都市异能网络初现","ending_change":"主角首次感知到符文世界的存在"}
                ],
                "near_chapters": [
                    {"number":1,"goal":"主角在地铁事故中看见古老符文并觉醒异瞳。","expected_turn":"主角意识到自己的视野能看见普通人看不见的灵能账册。"},
                    {"number":2,"goal":"主角追查账册痕迹并遇见宁砚澜。","expected_turn":"主角发现都市异能网络的隐蔽存在。"},
                    {"number":3,"goal":"异能集团派人试探主角能力。","expected_turn":"主角第一次主动使用异瞳反制追捕。"}
                ],
                "raw_outline": "《都市神瞳》以顾岑序觉醒异瞳能力为起点，逐步揭示现代都市中隐藏的玄幻势力网络。终局主角摧毁旧秩序后，建立符合自身价值观的新平衡。"
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.title.canonical_title, "都市神瞳");
    assert!(
        contract.outline.raw_outline.contains("《都市神瞳》"),
        "{}",
        contract.outline.raw_outline
    );
}

#[test]
fn weak_title_candidates_are_preserved_for_repair_coordinator() {
    let raw = r#"{
            "title": {
                "canonical_title": "裂痕中的余烬",
                "candidates": ["裂痕中的余烬", "止息之潮", "劫后余晖"],
                "rationale": "裂痕对应规则崩塌，余烬对应终局代价。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "异界玄幻",
            "brief": "主角在资源被神性法则垄断的异界发现规则漏洞。",
            "premise": "世界规则由‘灵脉律令’掌控，凡人只能通过分配额度获取生存资源，主角发现律令存在逻辑漏洞。",
            "ending": {
                "desired_resolution": "主角粉碎旧有的资源分配机制，建立新的平衡。",
                "final_state": "世界秩序重塑，主角成为新秩序的守望者。"
            },
            "protagonist_arc": "从卑微采集者变成挑战规则的秩序挑战者。",
            "world_imagery": "破碎的浮空岛屿、流动的灵力裂痕、枯竭的灵脉、带有刻印的生存凭证。",
            "main_causal_spine": "发现规则漏洞 -> 资源积累 -> 触碰秩序底线 -> 终局粉碎旧机制。",
            "characters": [{
                "canonical_name": "钟阙隅",
                "role": "主角",
                "desire": "获得生存资源并保护族群",
                "fear": "再次陷入资源匮乏的绝境",
                "bottom_line": "绝不向剥夺生存权的统治阶层低头",
                "arc_start": "卑微的拾荒者",
                "arc_end": "规则的重塑者"
            }, {
                "canonical_name": "司徒墨",
                "role": "反派",
                "desire": "维持资源垄断",
                "fear": "秩序失控",
                "bottom_line": "为了秩序牺牲一切",
                "arc_start": "秩序执行者",
                "arc_end": "旧秩序守护者"
            }],
            "themes": ["生存权利", "规则与自由"],
            "world_rules": ["灵脉资源通过‘律令’分配，过度使用会导致空间裂痕。"],
            "outline": {
                "volumes": [{"title":"裂痕初现","objective":"发现规则漏洞","ending_change":"主角脱离贫困线"}],
                "near_chapters": [
                    {"number":1,"goal":"主角在采集任务中发现裂痕，第一次获得超额灵力。","expected_turn":"主角通过漏洞获取资源，脱离贫困线。"},
                    {"number":2,"goal":"监管者发现资源波动。","expected_turn":"主角被迫伪装，接触更高阶规则知识。"},
                    {"number":3,"goal":"主角与盟友面对资源分配危机。","expected_turn":"主角意识到漏洞也是世界崩塌征兆。"}
                ],
                "raw_outline": "第一卷侧重生存与发现；第二卷侧重对抗与规则冲突。"
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.title.canonical_title, "裂痕中的余烬");
    assert_eq!(contract.title.candidates.len(), 3);
    assert!(matches!(contract.title.source, TitleSource::LlmContract));
}

#[test]
fn candidate_ranking_does_not_mutate_normalized_contract() {
    let raw = r#"{
            "title": {
                "canonical_title": "霓虹下的裂纹",
                "candidates": ["断裂的灵能回路", "夜校灵轨", "霓虹余烬"],
                "rationale": "霓虹隐喻都市表象，裂纹指向终局选择。",
                "source": "llm_contract"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {
                "desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"
            },
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [{
                "canonical_name": "许照桥",
                "role": "主角",
                "desire": "通过夜校考试并查清父亲旧案",
                "fear": "再次被规则抹掉姓名",
                "bottom_line": "不把同学当成晋级垫脚石",
                "arc_start": "只想自保的旁听生",
                "arc_end": "公开证据的规则修补者"
            }, {
                "canonical_name": "商砚衡",
                "role": "关键对手",
                "desire": "维护校盟对夜校考试的垄断",
                "fear": "灵籍账册被公开",
                "bottom_line": "不亲手毁掉考试系统",
                "arc_start": "幕后监考者",
                "arc_end": "被证据逼到台前"
            }],
            "world_rules": ["夜校灵轨会记录每次考试借力；灵籍账册可以证明资格是否被篡改。"],
            "outline": {"raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.title.canonical_title, "霓虹下的裂纹");
    assert_eq!(contract.title.candidates[1], "夜校灵轨");
    assert!(matches!(contract.title.source, TitleSource::LlmContract));
}

#[test]
fn normalize_clears_numbered_task_spec_residue_in_structured_fields() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校和灵轨都来自终局公开证据的核心场景。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "通过夜校考试并查清父亲旧案", "fear": "再次被规则抹掉姓名", "bottom_line": "不把同学当成晋级垫脚石", "arc_start": "只想自保的旁听生", "arc_end": "公开证据的规则修补者"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护校盟对夜校考试的垄断", "fear": "灵籍账册被公开", "bottom_line": "不亲手毁掉考试系统", "arc_start": "幕后监考者", "arc_end": "被证据逼到台前"}
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "themes": ["教育资格与公平秩序"],
            "style_rules": ["保持第三人称限制视角，用调查行动推进线索"],
            "must_avoid": ["不用无代价的临时能力解决考试与灵轨危机"],
            "outline": {
                "raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。",
                "volumes": [
                    {"title": "夜校追查", "objective": "取得灵籍账册异常证据", "ending_change": "主角确认校盟篡改资格"},
                    {"title": "账册公开", "objective": "主角公开灵轨账册并改写规则", "ending_change": "主角切断校盟对夜校考试的垄断"}
                ],
                "near_chapters": [
                    {"number": 1, "title": "夜考异响", "goal": "许照桥参加夜考并发现灵轨异常", "expected_turn": "异常记录指向被删改的灵籍"},
                    {"number": 2, "title": "账册缺名", "goal": "许照桥核对灵籍与考试记录", "expected_turn": "自己的姓名也曾被规则抹除"},
                    {"number": 3, "title": "监考暗门", "goal": "许照桥追查监考室的数据入口", "expected_turn": "商砚衡现身封锁入口并留下后续债务"}
                ]
            },
            "structured": {
                "narration_contract": {"pov":"第三人称有限视角"},
                "power_progression": {
                    "system_name": "都市玄幻2.作品字数：总字数不少于50,000字3.章节数量：不少于20章4.",
                    "current_ceiling": "",
                    "breakthrough_cost": ""
                }
            }
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let system_name = contract.structured.power_progression.system_name.as_str();

    assert!(!system_name.contains("作品字数"), "{system_name}");
    assert!(!system_name.contains("章节数量"), "{system_name}");
    assert!(
        contract.validate().is_ready(),
        "{:?}",
        contract.validate().issues
    );
}

#[test]
fn typed_gate_blocks_orphan_numbered_spec_fragments_in_primary_lists() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校和灵轨都来自终局公开证据的核心场景。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "通过夜校考试并查清父亲旧案", "fear": "再次被规则抹掉姓名", "bottom_line": "不把同学当成晋级垫脚石", "arc_start": "只想自保的旁听生", "arc_end": "公开证据的规则修补者"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护校盟对夜校考试的垄断", "fear": "灵籍账册被公开", "bottom_line": "不亲手毁掉考试系统", "arc_start": "幕后监考者", "arc_end": "被证据逼到台前"}
            ],
            "themes": [
                "公平晋级不能建立在记忆剥削上",
                "最终成为一代强者。三",
                "以吸引读者持续阅读。3.作品需具备较强的可读性与逻辑性"
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "outline": {"raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert!(
        contract
            .themes
            .iter()
            .all(|theme| !theme.contains("最终成为") && !theme.contains("作品需")),
        "{:?}",
        contract.themes
    );
}

#[test]
fn structured_character_array_uses_explicit_roles() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校与灵轨都来自故事终局。"},
            "premise": "普通学生卷入城市灵轨复苏。",
            "ending": {"desired_resolution": "主角守住夜校灵轨。"},
            "protagonist_arc": "从旁观到承担。",
            "world_imagery": "夜校灵轨",
            "main_causal_spine": "灵轨异常推动考试和守城冲突。",
            "characters": [
                {"canonical_name": "秦知安", "role": "主角", "desire": "通过考试", "fear": "失败", "bottom_line": "不牺牲同学", "arc_start": "旁观", "arc_end": "守城"},
                {"canonical_name": "梁棠", "role": "关键配角", "desire": "帮助主角确认异常", "fear": "被旧案牵连", "bottom_line": "不伪造证据", "arc_start": "防备", "arc_end": "信任"}
            ],
            "world_rules": ["灵轨接通需要代价。"],
            "outline": {"raw_outline": "第一阶段：异常；第二阶段：追查。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(
        contract
            .characters
            .iter()
            .filter(|character| character.role_looks_primary())
            .count(),
        1
    );
    assert_eq!(contract.characters[1].role, "关键配角");
}

#[test]
fn contract_language_normalization_does_not_preserve_garbled_suffixes() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校与灵轨都来自故事终局。"},
            "language": "zh-cn；zh-并",
            "premise": "普通学生卷入城市灵轨复苏。",
            "ending": {"desired_resolution": "主角守住夜校灵轨。"},
            "protagonist_arc": "从旁观到承担。",
            "world_imagery": "夜校灵轨",
            "main_causal_spine": "灵轨异常推动考试和守城冲突。",
            "characters": [
                {"canonical_name": "秦知安", "role": "主角", "desire": "通过考试", "fear": "失败", "bottom_line": "不牺牲同学", "arc_start": "旁观", "arc_end": "守城"}
            ],
            "world_rules": ["灵轨接通需要代价。"],
            "outline": {"raw_outline": "第一阶段：异常；第二阶段：追查。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

    assert_eq!(contract.language, "zh-CN");
}

#[test]
fn normalize_clears_json_residue_from_character_anchor_fields() {
    let raw = r#"{
            "title": {"canonical_title": "夜校灵轨", "rationale": "夜校与灵轨都来自故事终局。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨。",
            "premise": "普通学生卷入城市灵轨复苏。",
            "ending": {"desired_resolution": "主角公开灵轨账册，切断校盟垄断。"},
            "protagonist_arc": "从旁观到承担。",
            "world_imagery": "夜校灵轨",
            "main_causal_spine": "灵轨异常推动考试和守城冲突。",
            "characters": [
                {
                    "canonical_name": "秦知安",
                    "role": "主角",
                    "desire": "\"desired_resolution\": \"守住夜校灵轨\"",
                    "fear": "{\"fear\":\"被旧案吞没\"}",
                    "bottom_line": "不牺牲同学",
                    "arc_start": "旁观",
                    "arc_end": "守城"
                },
                {
                    "canonical_name": "梁棠",
                    "role": "关键配角",
                    "desire": "帮助主角确认异常",
                    "fear": "被旧案牵连",
                    "bottom_line": "不伪造证据",
                    "arc_start": "防备",
                    "arc_end": "信任"
                }
            ],
            "world_rules": ["灵轨接通需要代价。"],
            "outline": {"raw_outline": "第一阶段：异常；第二阶段：追查。"}
        }"#;

    let contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");
    let protagonist = contract
        .characters
        .iter()
        .find(|character| character.role_looks_primary())
        .expect("primary character");

    assert!(protagonist.desire.is_empty(), "{:?}", protagonist.desire);
    assert!(protagonist.fear.is_empty(), "{:?}", protagonist.fear);
    let issues = contract.validate().issues.join("；");
    assert!(issues.contains("主角缺少欲望锚点"), "{issues}");
    assert!(issues.contains("主角缺少恐惧锚点"), "{issues}");
    assert!(!issues.contains("JSON 字段或结构残片"), "{issues}");
}

#[test]
fn normalize_preserves_ambiguous_external_character_anchor_references() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "夺回旧城证言".to_string(),
            rationale: "旧城来自主角必须守住的具体地点，夺回旧城证言来自终局公开关系真相并完成选择的关键行动。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "都市言情".to_string(),
        brief: "关系选择和城市真相交织的长篇故事。".to_string(),
        target_units: Some(50_000),
        chapter_unit_target: Some(2500),
        premise: "旧城关系网络里隐藏着一份会改变人物选择的证言。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角公开证言，完成情感选择并守住自己的判断。".to_string(),
            final_state: "旧城关系真相公开，主角获得自主选择权。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从习惯退让，到主动公开证言并承担选择后果。".to_string(),
        world_imagery: "旧城雨巷、证言录音、深夜天台。".to_string(),
        main_causal_spine: "证言出现引发关系裂缝，主角追查旧城真相，终局公开证言完成选择。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "钟望宁".to_string(),
                role: "主角".to_string(),
                desire: "只信任林晚晴一人看清真相".to_string(),
                fear: "失去自己".to_string(),
                bottom_line: "不背叛自己的判断".to_string(),
                arc_start: "习惯退让".to_string(),
                arc_end: "主动选择".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "白望棠".to_string(),
                role: "关键关系对象".to_string(),
                desire: "只信任钟栖晚一人看清真相".to_string(),
                fear: "再次失去信任".to_string(),
                bottom_line: "不利用钟望宁的脆弱".to_string(),
                arc_start: "保持距离".to_string(),
                arc_end: "共同承担".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["证言只有当事人共同确认后才能公开并改变关系格局。".to_string()],
        outline: OutlineContract {
            volumes: vec![VolumeContract {
                title: "旧城证言".to_string(),
                objective: "主角确认证言真实性并进入关系压力中心".to_string(),
                ending_change: "主角无法再回避旧城真相".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "钟望宁在旧城雨巷听到证言录音".to_string(),
                expected_turn: "她发现关系真相与自己判断直接相关".to_string(),
            }],
            raw_outline: "证言出现，关系追查，终局公开真相。".to_string(),
        },
        ..Default::default()
    };

    contract.normalize();
    let joined = contract
        .characters
        .iter()
        .map(|character| {
            format!(
                "{} {} {} {}",
                character.canonical_name, character.desire, character.fear, character.bottom_line
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("林晚晴"), "{joined}");
    assert!(joined.contains("钟栖晚"), "{joined}");
    let issues = contract.validate().issues.join("；");
    assert!(!issues.contains("权威表外角色 `林晚晴`"), "{issues}");
    assert!(!issues.contains("权威表外角色 `钟栖晚`"), "{issues}");
}

#[test]
fn normalize_preserves_ambiguous_bare_character_anchor_without_local_verdict() {
    let mut contract = NovelCreationContract {
        characters: vec![
            CharacterContract {
                canonical_name: "司望安".to_string(),
                role: "主角".to_string(),
                desire: "建立开放式灵气交易所".to_string(),
                fear: "被视为无用的废人".to_string(),
                bottom_line: "在权力巅峰保持本心".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "陶澈遥".to_string(),
                role: "关键对手".to_string(),
                desire: "巩固财阀对灵气市场的垄断".to_string(),
                fear: "灵气期货波动暴露自身弱点".to_string(),
                bottom_line: "林渊".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    contract.normalize();
    let antagonist = contract
        .characters
        .iter()
        .find(|character| character.canonical_name == "陶澈遥")
        .expect("antagonist");

    assert_eq!(antagonist.bottom_line, "林渊");
    let issues = contract.validate().issues.join("；");
    assert!(!issues.contains("权威表外角色 `林渊`"), "{issues}");
}

#[test]
fn normalize_preserves_action_prefixed_ambiguous_character_references() {
    let mut contract = NovelCreationContract {
        title: TitleContract {
            canonical_title: "递上黑箱证据".to_string(),
            rationale: "黑箱证据来自事业规则里的关键物件，递上黑箱证据对应终局公开证据并完成情感选择的动作爽点。".to_string(),
            source: TitleSource::LlmContract,
            ..Default::default()
        },
        language: "zh-CN".to_string(),
        genre: "都市言情".to_string(),
        brief: "事业线和关系压力交织的都市言情。".to_string(),
        target_units: Some(50_000),
        chapter_unit_target: Some(2500),
        premise: "城市律所黑箱影响人物关系和事业选择。".to_string(),
        ending: EndingContract {
            desired_resolution: "主角把黑箱证据递上法庭，守住事业选择并完成情感关系决断。".to_string(),
            final_state: "黑箱被公开，主角保住自我价值。".to_string(),
            ..Default::default()
        },
        protagonist_arc: "从被关系和利益裹挟，到主动公开证据并守住自我价值。".to_string(),
        world_imagery: "玻璃幕墙、深夜法庭、黑箱证据。".to_string(),
        main_causal_spine: "黑箱证据出现，关系压力升级，终局递上法庭完成公开反转。".to_string(),
        characters: vec![
            CharacterContract {
                canonical_name: "南栖禾".to_string(),
                role: "主角".to_string(),
                desire: "守住自己的事业选择".to_string(),
                fear: "被利益关系吞没".to_string(),
                bottom_line: "不牺牲自我价值".to_string(),
                arc_start: "被动防守".to_string(),
                arc_end: "主动公开证据".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "段晴晚".to_string(),
                role: "关键对手".to_string(),
                desire: "夺取苏晚晴的婚姻地位".to_string(),
                fear: "被顾沉渊超越的威胁".to_string(),
                bottom_line: "守护沈清歌不被黑道侵蚀".to_string(),
                arc_start: "暗中施压".to_string(),
                arc_end: "被证据逼到台前".to_string(),
                ..Default::default()
            },
        ],
        world_rules: vec!["黑箱证据必须形成完整链条才能进入法庭。".to_string()],
        outline: OutlineContract {
            volumes: vec![VolumeContract {
                title: "黑箱入庭".to_string(),
                objective: "主角拿到第一份黑箱证据并进入法庭压力".to_string(),
                ending_change: "主角失去退回安全位置的可能".to_string(),
            }],
            near_chapters: vec![ChapterSeedContract {
                number: Some(1),
                goal: "南栖禾在深夜律所发现黑箱证据".to_string(),
                expected_turn: "她决定保留证据并面对关系压力".to_string(),
            }],
            raw_outline: "黑箱出现，关系施压，终局递上法庭。".to_string(),
        },
        ..Default::default()
    };

    contract.normalize();
    let antagonist = contract
        .characters
        .iter()
        .find(|character| character.canonical_name == "段晴晚")
        .expect("antagonist");

    assert!(antagonist.desire.contains("苏晚晴"), "{antagonist:?}");
    assert!(antagonist.fear.contains("顾沉渊"), "{antagonist:?}");
    assert!(antagonist.bottom_line.contains("沈清歌"), "{antagonist:?}");
    let issues = contract.validate().issues.join("；");
    assert!(!issues.contains("权威表外角色 `苏晚晴`"), "{issues}");
    assert!(!issues.contains("权威表外角色 `顾沉渊`"), "{issues}");
    assert!(!issues.contains("权威表外角色 `沈清歌`"), "{issues}");
}

#[test]
fn normalize_does_not_invent_reveal_schedule_from_top_level_fields() {
    let mut contract = NovelCreationContract {
        genre: "都市言情".to_string(),
        world_imagery: "玻璃幕墙、深夜茶水间、被隐藏的合同条款".to_string(),
        ..Default::default()
    };
    contract.title.canonical_title = "她把黑箱递上法庭".to_string();
    contract.ending.desired_resolution =
        "主角公开合同黑箱，保住事业独立并完成情感选择。".to_string();

    contract.normalize();

    assert!(contract.structured.reveal_schedule.is_empty());
}

#[test]
fn outline_plan_labels_are_not_slot_placeholder_pollution() {
    let outline = "第1章 本章目标：闻衡隅被卷入薪火道统争夺；预期转折：他确认燃烧寿元是获得力量的代价\n第2章 本章目标：闻衡隅第一次主动交换寿元破局；预期转折：仙门执事盯上薪火残印";

    assert_eq!(contract_text_surface_issue(outline, true), None);
}

#[test]
fn quoted_contract_slot_labels_are_still_blocked() {
    let value = "读者追看主角如何让 `总主线因果链` 付出代价";

    assert_eq!(
        contract_text_surface_issue(value, true),
        Some("合同槽位名占位")
    );
}

#[test]
fn embedded_contract_field_labels_are_surface_pollution() {
    let value = "找到传说中未被污染的“纯净蒸汽源，恐惧：真相被掩盖，底线：信任一旦给予";

    assert_eq!(
        contract_text_surface_issue(value, true),
        Some("其他合同字段标签残片")
    );
}

#[test]
fn unbalanced_contract_delimiters_are_surface_pollution() {
    let value = "找到传说中未被污染的“纯净蒸汽源";

    assert_eq!(
        contract_text_surface_issue(value, true),
        Some("未闭合引号或书名号")
    );
}

#[test]
fn contract_scalar_normalization_closes_trailing_cjk_delimiter() {
    assert_eq!(
        normalize_contract_scalar("信号源正在从“静止”转为“缓慢移动"),
        "信号源正在从“静止”转为“缓慢移动”"
    );
    assert_eq!(
        normalize_contract_scalar("》错误闭合后仍有文本"),
        "》错误闭合后仍有文本",
        "ambiguous closing corruption must remain visible to the quality gate"
    );
}

#[test]
fn normalize_removes_current_authority_name_from_identity_history() {
    let mut contract = NovelCreationContract {
        premise: "温屿桥修补沉钟并找回自我存在。".to_string(),
        characters: vec![CharacterContract {
            canonical_name: "温屿桥".to_string(),
            aliases: vec!["温屿桥".to_string(), "无声者".to_string()],
            previous_names: vec!["温屿桥".to_string(), "宋照岚".to_string()],
            role: "女主".to_string(),
            desire: "修补沉钟".to_string(),
            fear: "彻底遗忘自我".to_string(),
            bottom_line: "绝不吞噬他人记忆".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };

    contract.normalize();

    let character = &contract.characters[0];
    assert_eq!(character.aliases, vec!["无声者"]);
    assert_eq!(character.previous_names, vec!["宋照岚"]);
    assert!(
        !contract
            .validate()
            .issues
            .iter()
            .any(|issue| issue.contains("已废弃角色名 `温屿桥`")),
        "current authority name must never be audited as superseded"
    );
}
