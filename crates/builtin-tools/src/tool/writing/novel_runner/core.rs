//! Novel runner implementation facade.

mod draft;
mod jsonish;
mod memo;
mod model;
mod phase_contract;
mod prompts;
mod protocol;

pub(crate) use draft::*;
pub(crate) use jsonish::*;
pub(crate) use memo::*;
pub(crate) use model::*;
pub(crate) use phase_contract::*;
pub(crate) use prompts::*;
pub(crate) use protocol::*;

#[cfg(test)]
mod tests {
    use super::model::ZH_MEMO_SECTIONS;
    use super::*;

    fn test_authority(protagonist: &str, names: &[&str]) -> CharacterAuthority {
        CharacterAuthority::from_names(
            (!protagonist.is_empty()).then(|| protagonist.to_string()),
            names.iter().map(|name| (*name).to_string()).collect(),
        )
    }

    #[test]
    fn parses_required_chinese_memo_sections() {
        let raw = r#"goal: 主角在试炼中确认新的代价

## 当前任务
推进试炼。
## 本章目标
让试炼产生可追踪的新状态。
## 该兑现
兑现上一章的门槛。
## 暂不掀
不揭开终局。
## 日常过渡功能
用短暂休整展示关系变化。
## 关键抉择三连问
为什么做、是否符合人设、读者是否突兀。
## 章尾必须发生的改变
主角获得新的代价。
## 不要做
不要改名。
"#;
        let memo = parse_memo(raw, "zh").expect("memo");
        assert_eq!(memo.goal, "主角在试炼中确认新的代价");
        assert_eq!(memo.sections.len(), ZH_MEMO_SECTIONS.len());
    }

    #[test]
    fn rejects_incomplete_memo_instead_of_inventing_sections() {
        let raw = "goal: x\n\n## 当前任务\n只写一段";
        let error = parse_memo(raw, "zh").expect_err("incomplete memo");
        assert!(error.contains("missing required sections"));
        assert!(error.contains("不要做"));
    }

    #[test]
    fn normalizes_decision_check_memo_as_internal_control_not_prose_seed() {
        let raw = r#"目标：主角主动入局

## 当前任务
写出主角第一次选择。
## 本章目标
让选择通过行动和代价落地。
## 该兑现
兑现上一章的行动后果。
## 暂不掀
保留终局秘密。
## 日常过渡功能
通过具体日常行动承接上一章。
## 关键抉择三连问
本章关键选择必须回答：为什么做、是否符合人设、读者是否突兀。
## 章尾必须发生的改变
主角承担选择造成的代价。
## 不要做
不要改名或输出作者说明。
"#;
        let memo = parse_memo(raw, "zh").expect("memo");

        assert!(memo.body.contains("作者内部检查"));
        assert!(!memo.body.contains("为什么做"));
        assert!(!memo.body.contains("是否符合人设"));
        assert!(!memo.body.contains("读者是否突兀"));
    }

    #[test]
    fn rejects_freeform_memo_without_required_sections() {
        let raw = "本章让主角意识到旧代价正在反噬。\n\n她必须做出选择。";
        let error = parse_memo(raw, "zh").expect_err("freeform memo");
        assert!(error.contains("missing required sections"));
    }

    #[test]
    fn parses_chapter_execution_package_json() {
        let raw = r#"```json
{
  "memo_markdown": "目标：主角通过入门试炼\n\n## 当前任务\n完成试炼。\n\n## 本章目标\n取得入门资格。\n\n## 该兑现\n兑现学院压力。\n\n## 暂不掀\n不揭开幕后。\n\n## 日常过渡功能\n展示课堂。\n\n## 关键抉择三连问\n为何行动；代价；因果。\n\n## 章尾必须发生的改变\n获得资格。\n\n## 不要做\n不要改名。",
  "architecture": "1. 试炼集合：目标、压力、行动、事实、钩子。\n2. 考场异动：目标、压力、行动、事实、钩子。",
  "scene_goal": "沈砚通过入门试炼",
  "conflict": "资格规则与考场异动同时施压",
  "choice": "沈砚选择承担反噬完成试炼",
  "cost": "旧伤加重",
  "reveal": "考场异动来自旧阵",
  "emotional_beat": "从退缩转为坚定",
  "chapter_function": "完成入门阶段并建立旧阵债务",
  "irreversible_event": "沈砚取得入门资格",
  "new_state_after_chapter": "沈砚成为正式弟子",
  "world_change": "",
  "character_change": "沈砚从逃避试炼转为主动承担代价",
  "relationship_change": "沈砚与导师从戒备转为有限合作",
  "power_delta": "沈砚掌握一次受限的旧阵感知",
  "resource_delta": "沈砚获得入门资格",
  "hook_opened": ["旧阵为何干预试炼"],
  "hook_paid_off": [],
  "title_basis": "取得入门资格",
  "future_chapters": [],
  "new_character_requests": []
}
```"#;
        let package = parse_chapter_execution_package(raw, "zh").expect("package");
        assert_eq!(package.memo.goal, "主角通过入门试炼");
        assert!(package.memo.body.contains("## 当前任务"));
        assert!(package.architecture.contains("考场异动"));
        assert!(package.character_change.contains("主动承担"));
        assert!(package.relationship_change.contains("有限合作"));
        assert_eq!(package.scene_goal, "沈砚通过入门试炼");
        assert!(package.world_change.is_empty());
        assert_eq!(package.power_delta, "沈砚掌握一次受限的旧阵感知");
        assert_eq!(package.resource_delta, "沈砚获得入门资格");
        assert_eq!(package.hook_opened, vec!["旧阵为何干预试炼"]);
        assert!(!package.architecture.contains("scene_goal:"));
        assert!(render_execution_contract_header(&package).contains("scene_goal: 沈砚通过入门试炼"));
    }

    #[test]
    fn hook_paid_off_is_the_single_canonical_debt_field() {
        let raw = r#"{
  "memo_markdown": "目标：兑现旧债\n\n## 当前任务\n推进。\n\n## 本章目标\n兑现。\n\n## 该兑现\n旧债。\n\n## 暂不掀\n保留。\n\n## 日常过渡功能\n承接。\n\n## 关键抉择三连问\n行动；代价；因果。\n\n## 章尾必须发生的改变\n旧债已清。\n\n## 不要做\n不要改名。",
  "architecture": "1. 主角找到旧信并承担代价。",
  "scene_goal": "兑现旧债",
  "conflict": "",
  "choice": "",
  "cost": "",
  "reveal": "",
  "emotional_beat": "",
  "chapter_function": "关闭旧债",
  "irreversible_event": "",
  "new_state_after_chapter": "旧债已清",
  "world_change": "",
  "character_change": "",
  "relationship_change": "",
  "power_delta": "",
  "resource_delta": "",
  "hook_opened": [],
  "hook_paid_off": ["旧信来历", "失踪证人"],
  "title_basis": "旧信来历",
  "future_chapters": [],
  "new_character_requests": []
}"#;

        let package = parse_chapter_execution_package(raw, "zh").expect("package");

        assert_eq!(package.hook_paid_off, vec!["失踪证人", "旧信来历"]);
        assert!(!package.architecture.contains("hook_paid_off:"));
        assert_eq!(
            render_execution_contract_header(&package)
                .matches("hook_paid_off:")
                .count(),
            1
        );
    }

    #[test]
    fn parses_structured_execution_package_arrays() {
        let raw = r#"{
  "memo_markdown": {
    "goal": "主角在都市困局中做出真诚选择",
    "sections": [
      {"heading": "当前任务", "content": ["建立情感冲突", "推进事业压力"]},
      {"heading": "本章目标", "content": "两人决定共同面对现实压力。"},
      {"heading": "该兑现", "content": "兑现上一章的误会。"},
      {"heading": "暂不掀", "content": "不揭开最终旧事。"},
      {"heading": "日常过渡功能", "content": "用晚班地铁承接关系变化。"},
      {"heading": "关键抉择三连问", "content": "为什么行动；是否符合人设；读者是否突兀。"},
      {"heading": "章尾必须发生的改变", "content": "两人决定共同面对。"},
      {"heading": "不要做", "content": "不要写露骨内容。"}
    ]
  },
  "architecture": [
    {"scene": "地铁重逢", "pressure": "误会未解", "hook": "旧信出现"},
    {"scene": "雨夜选择", "pressure": "事业取舍", "hook": "关系推进"}
  ],
  "scene_goal": "两人决定共同面对现实压力",
  "conflict": "误会与事业压力",
  "choice": "共同面对",
  "cost": "",
  "reveal": "",
  "emotional_beat": "从防备转为合作",
  "chapter_function": "推进关系",
  "irreversible_event": "两人形成共同决定",
  "new_state_after_chapter": "关系进入合作",
  "world_change": "",
  "character_change": "",
  "relationship_change": "从误会转为合作",
  "power_delta": "",
  "resource_delta": "",
  "hook_opened": ["旧信出现"],
  "hook_paid_off": [],
  "title_basis": "雨夜选择",
  "future_chapters": [
    {"number": 2, "goal": "两人核对旧信来源", "expected_turn": "旧信来源指向共同旧事"},
    {"number": 3, "goal": "两人面对共同旧事的证人", "expected_turn": "证人同意公开作证"}
  ],
  "new_character_requests": []
}"#;
        let package = parse_chapter_execution_package(raw, "zh").expect("package");
        assert_eq!(package.memo.goal, "主角在都市困局中做出真诚选择");
        assert!(package.memo.body.contains("## 当前任务"));
        assert!(package.architecture.contains("地铁重逢"));
        assert!(package.architecture.contains("旧信出现"));
        assert_eq!(package.future_chapters.len(), 2);
        assert_eq!(package.future_chapters[0].number, Some(2));
        assert_eq!(package.future_chapters[1].number, Some(3));
    }

    #[test]
    fn rejects_malformed_execution_package_instead_of_using_json_brace_as_goal() {
        let raw = r##"{ "memo_markdown": "# 第 23 章：章节执行备忘录\n\n**目标：** 推进主角逃离", "architecture": "# 场景" "##;

        let error = parse_chapter_execution_package(raw, "zh").expect_err("malformed package");

        assert!(error.contains("JSON"));
    }

    #[test]
    fn rejects_nested_execution_package_as_architecture() {
        let raw = r##"{
  "memo_markdown": "目标：主角确认新的生存代价\n\n## 当前任务\n推进。\n\n## 本章目标\n确认代价。\n\n## 该兑现\n兑现。\n\n## 暂不掀\n保留。\n\n## 日常过渡功能\n承接。\n\n## 关键抉择三连问\n行动；代价；因果。\n\n## 章尾必须发生的改变\n改变。\n\n## 不要做\n不要改名。",
  "architecture": "{ \"memo_markdown\": \"# 第 23 章\", \"architecture\": \"1. 场景\" }",
  "scene_goal": "", "conflict": "", "choice": "", "cost": "", "reveal": "",
  "emotional_beat": "", "chapter_function": "", "irreversible_event": "",
  "new_state_after_chapter": "", "world_change": "", "character_change": "", "relationship_change": "",
  "power_delta": "", "resource_delta": "", "hook_opened": [], "hook_paid_off": [],
  "title_basis": "", "future_chapters": [], "new_character_requests": []
}"##;

        let error = parse_chapter_execution_package(raw, "zh").expect_err("nested package");

        assert!(error.contains("malformed"));
    }

    #[test]
    fn rejects_execution_package_with_silently_omitted_typed_fields() {
        let raw = r#"{
  "memo_markdown": "目标：推进本章\n\n## 当前任务\n推进。\n\n## 本章目标\n推进。\n\n## 该兑现\n兑现。\n\n## 暂不掀\n保留。\n\n## 日常过渡功能\n承接。\n\n## 关键抉择三连问\n行动；代价；因果。\n\n## 章尾必须发生的改变\n改变。\n\n## 不要做\n不要改名。",
  "architecture": []
}"#;

        let error = parse_chapter_execution_package(raw, "zh")
            .expect_err("missing typed fields must trigger the bounded package retry");

        assert!(error.contains("missing required fields"));
        assert!(error.contains("scene_goal"));
        assert!(error.contains("future_chapters"));
        assert!(error.contains("new_character_requests"));
    }

    #[test]
    fn normalizes_structural_execution_package_drift_without_losing_valid_fields() {
        let raw = r#"{
  "memo_markdown": "目标：推进线索\n\n## 当前任务\n推进。\n\n## 本章目标\n取得线索。\n\n## 该兑现\n兑现。\n\n## 暂不掀\n保留。\n\n## 日常过渡功能\n承接。\n\n## 关键抉择三连问\n行动；代价；因果。\n\n## 章尾必须发生的改变\n取得线索。\n\n## 不要做\n不要改名。",
  "architecture": [{"scene":"旧站追踪","result":"取得线索"}],
  "scene_goal": {"action":"取得线索"},
  "conflict": ["时间压力", "守卫阻拦"],
  "choice": "继续追踪",
  "cost": null,
  "reveal": "旧站仍在运作",
  "emotional_beat": "从迟疑转为坚定",
  "chapter_function": "推进主线",
  "irreversible_event": "取得站内记录",
  "new_state_after_chapter": "掌握新线索",
  "world_change": "",
  "character_change": "主角决定继续追踪",
  "relationship_change": "",
  "power_delta": "",
  "resource_delta": "取得站内记录",
  "hook_opened": "旧站为何仍在运作",
  "hook_paid_off": null,
  "title_basis": "旧站记录",
  "future_chapters": [],
  "new_character_requests": ["req-guard-001", {"request_id":"req-guide-001","role":"向导","narrative_purpose":"提供旧站入口","planned_exit":"本章结束"}]
}"#;

        let package = parse_chapter_execution_package(raw, "zh").expect("package");

        assert!(package.scene_goal.contains("取得线索"));
        assert!(package.conflict.contains("时间压力"));
        assert_eq!(package.hook_opened, ["旧站为何仍在运作"]);
        assert_eq!(package.new_character_requests.len(), 1);
        assert_eq!(
            package.new_character_requests[0].request_id,
            "req-guide-001"
        );
    }

    #[test]
    fn parses_json_draft_output() {
        let raw = r#"```json
{"title":"风起","content":"林衡推开雾门。灵脉回应誓言。","summary":"林衡入门。","key_facts":["林衡推开雾门"],"continuity_updates":["灵脉回应誓言"]}
```"#;
        let draft = parse_draft_output(raw, 1, "zh");
        assert_eq!(draft.title, "风起");
        assert_eq!(draft.key_facts, vec!["林衡推开雾门"]);
    }

    #[test]
    fn parses_final_chapter_observation_without_writer_metadata() {
        let raw = r#"```json
{"current_state":"闻庭安带着铜钥匙离开旧站","pending_hooks":"铜钥匙对应的门仍未找到","chapter_summary":"闻庭安从旧站取走铜钥匙并避开巡守","continuity_updates":["闻庭安持有铜钥匙"],"resolved_hooks":["旧站储物柜的开启方法已揭示"]}
```"#;
        let observation =
            parse_final_chapter_observation(raw, "闻庭安从旧站取走铜钥匙。").expect("observation");
        assert_eq!(observation.continuity_updates, ["闻庭安持有铜钥匙"]);
        assert_eq!(observation.resolved_hooks.len(), 1);
    }

    #[test]
    fn final_chapter_observation_normalizes_structured_summary_fields() {
        let raw = r#"{
  "current_state": {"character": "闻庭安带着铜钥匙离开旧站", "location": "北门"},
  "pending_hooks": ["铜钥匙对应的门仍未找到"],
  "chapter_summary": {"summary": "闻庭安从旧站取走铜钥匙并避开巡守"},
  "continuity_updates": [{"fact": "闻庭安持有铜钥匙"}],
  "resolved_hooks": [],
  "state_changes": []
}"#;

        let observation =
            parse_final_chapter_observation(raw, "闻庭安从旧站取走铜钥匙。").expect("observation");

        assert!(observation
            .current_state
            .contains("character：闻庭安带着铜钥匙离开旧站"));
        assert!(observation.current_state.contains("location：北门"));
        assert_eq!(observation.pending_hooks, "铜钥匙对应的门仍未找到");
        assert_eq!(
            observation.chapter_summary,
            "summary：闻庭安从旧站取走铜钥匙并避开巡守"
        );
        assert_eq!(observation.continuity_updates, ["fact：闻庭安持有铜钥匙"]);
    }

    #[test]
    fn final_chapter_observation_resolves_sentence_ids_to_exact_local_evidence() {
        let body = "闻庭安进入旧站。闻庭安带着铜钥匙离开旧站。";
        let raw = r#"{
  "current_state": "闻庭安带着铜钥匙离开旧站",
  "pending_hooks": "",
  "chapter_summary": "闻庭安从旧站取走铜钥匙",
  "continuity_updates": [],
  "resolved_hooks": [],
  "state_changes": [{
    "entity_id": "character-0001",
    "event_type": "resource",
    "authority_path": "chapter_contract.resource_delta",
    "evidence_sentence_ids": [2]
  }]
}"#;

        let observation = parse_final_chapter_observation(raw, body).expect("observation");

        assert_eq!(observation.state_changes[0].evidence.start_char, 0);
        assert_eq!(observation.state_changes[0].evidence.end_char, 0);
        assert_eq!(
            observation.state_changes[0].evidence.excerpt,
            "闻庭安带着铜钥匙离开旧站。"
        );
        assert_eq!(
            observation.state_changes[0].value,
            observation.state_changes[0].evidence.excerpt
        );
        assert!(observation.state_changes[0].authority_excerpt.is_empty());
    }

    #[test]
    fn final_chapter_observation_rejects_non_contiguous_sentence_ids() {
        let body = "闻庭安进入旧站。闻庭安打开铜柜。闻庭安带着铜钥匙离开。";
        let raw = r#"{
  "current_state": "闻庭安离开旧站",
  "chapter_summary": "闻庭安取得铜钥匙",
  "state_changes": [{
    "entity_id": "character-0001",
    "event_type": "resource",
    "authority_path": "chapter_contract.resource_delta",
    "evidence_sentence_ids": [1, 3]
  }]
}"#;

        let observation = parse_final_chapter_observation(raw, body).expect("observation");

        assert!(observation.state_changes.is_empty());
    }

    #[test]
    fn final_chapter_observation_rejects_evidence_across_single_newline_paragraphs() {
        let body = "闻庭安打开铜柜。\n闻庭安带着铜钥匙离开。";
        let raw = r#"{
  "current_state": "闻庭安离开旧站",
  "chapter_summary": "闻庭安取得铜钥匙",
  "state_changes": [{
    "entity_id": "character-0001",
    "event_type": "resource",
    "authority_path": "chapter_contract.resource_delta",
    "evidence_sentence_ids": [1, 2]
  }]
}"#;

        let observation = parse_final_chapter_observation(raw, body).expect("observation");

        assert!(observation.state_changes.is_empty());
        assert!(render_final_body_sentence_index(body).contains("。\n\n[S0002]"));
    }

    #[test]
    fn final_body_sentence_index_keeps_decimal_measurements_in_one_sentence() {
        let body = "裂缝宽度稳定在3.14毫米。闻庭安记下读数。";
        let rendered = render_final_body_sentence_index(body);

        assert!(rendered.contains("[S0001] 裂缝宽度稳定在3.14毫米。"));
        assert!(rendered.contains("[S0002] 闻庭安记下读数。"));
        assert_eq!(rendered.matches("[S").count(), 2);
    }

    #[test]
    fn final_body_evidence_spans_reuse_indexed_same_paragraph_windows() {
        let body = "顾景真感到意识脱离肉体。他进入非物质状态。\n程昭禾仍在等待。";
        let spans = final_body_evidence_spans(body);

        assert!(spans
            .iter()
            .any(|span| span.excerpt == "顾景真感到意识脱离肉体。他进入非物质状态。"));
        assert!(!spans
            .iter()
            .any(|span| span.excerpt.contains("非物质状态。\n程昭禾")));
        assert!(spans.iter().all(|span| {
            body.chars()
                .skip(span.start_char)
                .take(span.end_char - span.start_char)
                .collect::<String>()
                == span.excerpt
        }));
    }

    #[test]
    fn final_chapter_observer_delegates_offsets_to_local_validator() {
        let prompt = final_chapter_observer_prompt(
            "zh-CN",
            3,
            r#"{"authority":{"chapter_contract":{}}}"#,
            "闻庭安带着铜钥匙离开旧站。",
            None,
        );

        assert!(prompt.contains("本地验证器会从不可变最终正文按句子编号恢复精确原文"));
        assert!(prompt.contains("[S0001] 闻庭安带着铜钥匙离开旧站。"));
    }

    #[test]
    fn final_chapter_observer_allows_bounded_adjacent_sentence_evidence() {
        let prompt = final_chapter_observer_prompt(
            "zh-CN",
            3,
            r#"{"authority":{"chapter_contract":{"new_state_after_chapter":"旧引擎发出非自然信号"}}}"#,
            "旧引擎突然震动。那不是自然噪声。信号仍在持续。",
            None,
        );

        assert!(prompt.contains("1 至 3 个同段、连续、正序的整数编号"));
        assert!(prompt.contains("不得超过 320 字"));
        assert!(prompt.contains("authority_path=chapter_contract.new_state_after_chapter"));
        assert!(prompt.contains("不要求逐字同措辞"));
        assert!(prompt.contains("entity_id 使用稳定 ID"));
        assert!(prompt.contains("\"authority_path\":\"chapter_contract.new_state_after_chapter\""));
        assert!(prompt.contains("\"evidence_sentence_ids\":[12,13]"));
        assert!(!prompt.contains("\"authority_excerpt\""));
        assert!(!prompt.contains("\"value\":\"正文"));
    }

    #[test]
    fn parses_jsonish_draft_with_unescaped_multiline_content() {
        let raw = r#"{
  "title": "第一章：裂纹中的微光",
  "content": "清晨，青羽阁的雾气里透着一股子滞涩的冷。

陆羽盘坐在后山灵植园。",
  "summary": "陆羽发现灵气异变。",
  "key_facts": [
    "陆羽是青羽阁杂役",
    "灵气出现异变"
  ],
  "continuity_updates": [
    "陆羽对灵气极度敏感"
  ]
}"#;
        let draft = parse_draft_output(raw, 1, "zh");

        assert_eq!(draft.title, "第一章：裂纹中的微光");
        assert!(draft.content.contains("陆羽盘坐"));
        assert_eq!(draft.key_facts.len(), 2);
        assert_eq!(draft.continuity_updates, vec!["陆羽对灵气极度敏感"]);
    }

    #[test]
    fn parses_jsonish_draft_with_unclosed_long_content_field() {
        let raw = r#"{
  "title": "第一章：符文醒来",
  "content": "医院走廊的消毒水气味在凌晨三点格外刺鼻。

辛砚遥盯着门上用血画出的古老符文，忽然意识到自己已经无法回头。

\"你看见什么了？\"岑知安问。

辛砚遥没有回答，只是抬起手，看见掌心有金色纹路一点点亮起。"#;

        let draft = parse_draft_output(raw, 1, "zh");

        assert!(!draft.degraded);
        assert_eq!(draft.title, "第一章：符文醒来");
        assert!(draft.content.contains("辛砚遥盯着门上"));
        assert!(draft.content.contains("金色纹路一点点亮起"));
    }

    #[test]
    fn jsonish_array_renders_object_items_with_labels() {
        let raw = r#"{
  "characters": [
    {"name": "林枫", "role": "主角", "identity": "青风宗弟子"},
    {"name": "苏颜", "role": "对手"}
  ]
}"#;
        let items = jsonish_string_array_field(raw, "characters");

        assert_eq!(
            items,
            vec![
                "name: 林枫; role: 主角; identity: 青风宗弟子",
                "name: 苏颜; role: 对手"
            ]
        );
    }

    #[test]
    fn writer_prompt_surfaces_exact_contract_names() {
        let authority = test_authority("闻庭安", &["闻庭安", "楚辞尘"]);
        let memo = ChapterMemo {
            goal: "推进第一章".to_string(),
            body: "## 当前任务\n写第一章".to_string(),
            sections: Vec::new(),
        };
        let prompt = writer_prompt(
            "Chinese",
            "苍陌渡厄传",
            1,
            Some(4000),
            &memo,
            "架构",
            "{}",
            &authority,
        );

        assert!(prompt.contains("闻庭安、楚辞尘"));
        assert!(prompt.contains("禁止音译"));
        assert!(prompt.contains("权威主角：闻庭安"));
        assert!(prompt.contains("整章必须至少出现一个稳定角色名"));
        assert!(prompt.contains("不得自行给新人物起名"));
        assert!(prompt.contains("临时功能人物只能使用身份称谓"));
        assert!(prompt.contains("其他长期合同角色"));
        assert!(prompt.contains("属于未来"));
        assert!(prompt.contains("标题和正文都必须使用中文"));
        assert!(prompt.contains("---BODY---"));
        assert!(prompt.contains("---END BODY---"));
        assert!(prompt.contains("正文不得嵌入 JSON 字符串"));
        assert!(prompt.contains("不得少于 4000"));
        assert!(prompt.contains("约 4400 字"));
        assert!(prompt.contains("面板/worker 的章节最低目标"));
        assert!(prompt.contains("同一关键物件的来源、持有者、位置、状态和首次获得事件"));
        assert!(prompt.contains("new_state_after_chapter"));
        assert!(prompt.contains("点名受影响的既有权威实体"));
        assert!(prompt.contains("不超过 320 个字符"));
        assert!(!prompt.contains("本章参考字数"));
    }

    #[test]
    fn writer_prompt_uses_panel_target_as_first_draft_output_contract() {
        let memo = ChapterMemo {
            goal: "推进第一章".to_string(),
            body: "## 当前任务\n写第一章".to_string(),
            sections: Vec::new(),
        };
        let authority = test_authority("", &[]);
        let prompt = writer_prompt(
            "zh-CN",
            "照夜录",
            1,
            Some(8123),
            &memo,
            "架构",
            "{}",
            &authority,
        );

        assert!(prompt.contains("输出合同"));
        assert!(prompt.contains("8123"));
        assert!(prompt.contains("8936"));
        assert!(prompt.contains("首次草稿"));
        assert!(prompt.contains("低于该值视为本章未完成"));
        assert!(!prompt.contains("4000"));
    }

    #[test]
    fn reviser_prompt_rewrites_same_chapter_for_structural_body_degradation() {
        let memo = ChapterMemo {
            goal: "完成第一章觉醒".to_string(),
            body: "## 当前任务\n写第一章".to_string(),
            sections: Vec::new(),
        };
        let issues = vec![
            "Chinese chapter body repeats the same scene fragment too many times".to_string(),
            "章节存在明显行动缺失：大量描写停留在内心独白和场景描写".to_string(),
        ];
        let authority = test_authority("闻庭安", &["闻庭安"]);
        let prompt = reviser_prompt(
            "zh-CN",
            "阙试血",
            1,
            Some(2500),
            &memo,
            "场景架构",
            "{}",
            "旧正文",
            &issues,
            RevisionMode::FullRewrite,
            &authority,
        );

        assert!(prompt.contains("按同一章合同从头生成"));
        assert!(prompt.contains("这不是另开新章"));
        assert!(prompt.contains("不要复用上一版正文"));
        assert!(prompt.contains("不得少于 2500"));
        assert!(prompt.contains("修订后的 content"));
        assert!(!prompt.contains("旧正文"));
        assert!(!prompt.contains("以当前正文为底稿做局部修补"));
    }

    #[test]
    fn reviser_prompt_rewrites_same_chapter_for_identity_and_timeline_contradictions() {
        let memo = ChapterMemo {
            goal: "修复第一章核心场景".to_string(),
            body: "## 当前任务\n重写第一章".to_string(),
            sections: Vec::new(),
        };
        let authority = test_authority("秦闻禾", &["秦闻禾", "白闻白"]);
        let issues = vec![
            "人物设定矛盾：同一角色身份前后矛盾。".to_string(),
            "逻辑时间跳跃：时间线未闭环。".to_string(),
        ];
        let prompt = reviser_prompt(
            "zh-CN",
            "都市破局者",
            1,
            Some(2500),
            &memo,
            "场景架构",
            "{}",
            "旧正文",
            &issues,
            RevisionMode::FullRewrite,
            &authority,
        );

        assert!(prompt.contains("按同一章合同从头生成"));
        assert!(prompt.contains("这不是另开新章"));
        assert!(prompt.contains("秦闻禾、白闻白"));
        assert!(prompt.contains("不得少于 2500"));
        assert!(!prompt.contains("旧正文"));
        assert!(!prompt.contains("以当前正文为底稿做局部修补"));
    }

    #[test]
    fn reviser_prompt_rewrites_same_chapter_for_outline_prose_and_unseeded_setup() {
        let memo = ChapterMemo {
            goal: "写出第一章入局".to_string(),
            body: "## 当前任务\n写第一章".to_string(),
            sections: Vec::new(),
        };
        let issues = vec![
            "存在明显的摘要/大纲式叙述，而非纯正文展示。".to_string(),
            "存在未铺垫的新设定/关键帮助，缺乏前文铺垫，逻辑链条不完整。".to_string(),
        ];
        let authority = test_authority("闻庭安", &["闻庭安"]);
        let prompt = reviser_prompt(
            "zh-CN",
            "雨夜老码头",
            1,
            Some(2500),
            &memo,
            "场景架构",
            "{}",
            "旧正文",
            &issues,
            RevisionMode::FullRewrite,
            &authority,
        );

        assert!(prompt.contains("按同一章合同从头生成"));
        assert!(prompt.contains("不要复用上一版正文"));
        assert!(!prompt.contains("旧正文"));
    }

    #[test]
    fn reviser_prompt_rewrites_same_chapter_for_generic_progression_gate() {
        let memo = ChapterMemo {
            goal: "完成第三章灵脉争夺".to_string(),
            body: "## 当前任务\n写第三章".to_string(),
            sections: Vec::new(),
        };
        let issues = vec![
            "quality gate: chapter does not show a durable state change or irreversible event"
                .to_string(),
            "quality gate: chapter progression is too generic; keyfacts/continuityupdates must name a concrete action, object, relationship, place, or consequence".to_string(),
        ];
        let authority = test_authority("闻庭安", &["闻庭安"]);
        let prompt = reviser_prompt(
            "zh-CN",
            "破残卷",
            3,
            Some(2500),
            &memo,
            "场景架构",
            "{}",
            "旧正文",
            &issues,
            RevisionMode::FullRewrite,
            &authority,
        );

        assert!(prompt.contains("按同一章合同从头生成"));
        assert!(prompt.contains("不得少于 2500"));
        assert!(!prompt.contains("旧正文"));
        assert!(!prompt.contains("以当前正文为底稿做局部修补"));
    }

    #[test]
    fn reviser_prompt_locally_repairs_overused_story_term() {
        let memo = ChapterMemo {
            goal: "完成第四章悟道转折".to_string(),
            body: "## 当前任务\n写第四章".to_string(),
            sections: Vec::new(),
        };
        let issues = vec![
            "Chinese chapter body overuses the same story term without enough concrete progression: `道的真意` appears 22 times".to_string(),
        ];
        let authority = test_authority("闻庭安", &["闻庭安"]);
        let prompt = reviser_prompt(
            "zh-CN",
            "破残卷",
            4,
            Some(2500),
            &memo,
            "场景架构",
            "{}",
            "旧正文",
            &issues,
            RevisionMode::LocalRepair,
            &authority,
        );

        assert!(prompt.contains("修订《破残卷》第 4 章"));
        assert!(prompt.contains("只修复列出问题，不要重写成另一章"));
        assert!(!prompt.contains("按同一章合同从头生成"));
    }
}
