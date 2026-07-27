    use super::super::*;

    #[test]
    fn clean_model_output_strips_channel_tags_and_outer_fence() {
        let raw = "<|channel>thought\n<channel|>```markdown\n# 第一章\n\n正文\n```";

        assert_eq!(clean_model_output(raw), "# 第一章\n\n正文");
    }

    #[test]
    fn clean_provider_prompt_strips_channel_markers_before_next_call() {
        let raw =
            "请继续。\n<|channel>thought\n<channel|>```json\n{\"content\":\"正文\"}<|eot_id|>";

        let cleaned = clean_provider_prompt(raw);

        assert!(cleaned.contains("请继续。"));
        assert!(!cleaned.contains("<|channel>"));
        assert!(!cleaned.contains("<channel|>"));
        assert!(!cleaned.contains("<|eot_id|>"));
    }

    #[test]
    fn clean_stream_progress_text_strips_provider_channel_tags() {
        let raw = "<|channel>thought\n<channel|>```json\n{\"addition\":\"正文\"}<|eot_id|>";

        let cleaned = clean_stream_progress_text(raw);

        assert!(!cleaned.contains("<|channel>"));
        assert!(!cleaned.contains("<channel|>"));
        assert!(!cleaned.contains("<|eot_id|>"));
        assert!(cleaned.contains("正文"));
    }

    #[test]
    fn sanitize_chapter_body_removes_inline_cjk_markup_noise() {
        let raw = "他意识到，这种噪音实际上是一种感知过}_过载。";

        let cleaned = sanitize_chapter_body(raw, "噪音突袭", "zh-CN");

        assert!(!cleaned.contains("}_"));
        assert!(cleaned.contains("感知过载"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_unclassified_script_fragments_for_review() {
        let raw = "辛岑宁顺着他的视线看去，发现地나地的逻辑关联和痕 आ迹都在闪烁。";

        let cleaned = sanitize_chapter_body(raw, "街角失声", "zh-CN");

        assert!(cleaned.contains('나'));
        assert!(cleaned.contains('आ'));
    }

    #[test]
    fn sanitize_chapter_body_does_not_guess_object_body_state_clause() {
        let raw = "他的手中紧握着那把古剑身泛着幽蓝的光芒，旧宗门的纹路随之亮起。";

        let cleaned = sanitize_chapter_body(raw, "剑纹证物", "zh-CN");

        assert!(cleaned.contains("古剑身泛着幽蓝的光芒"), "{cleaned}");
    }

    #[test]
    fn sanitize_chapter_body_preserves_normal_chinese_dialogue_and_narration() {
        let raw = "沈渡站在天桥的护栏边。\n\n“如果不找，我无法确定他是在这个世界，还是在‘那边’。”沈渡的声音沙哑。\n\n陆清漪察觉到了沈渡眼神中的变化。\n\n沈渡没有说话，他只是看向远方。";

        let cleaned = sanitize_chapter_body(raw, "霓虹裂隙与白光碎片", "zh-CN");

        assert!(cleaned.contains("沈渡站在天桥"));
        assert!(cleaned.contains("沈渡的声音沙哑"));
        assert!(cleaned.contains("陆清漪察觉到了沈渡眼神中的变化"));
        assert!(cleaned.contains("沈渡没有说话"));
        assert!(!cleaned.contains("沈渡天桥"));
        assert!(!cleaned.contains("沈渡音沙哑"));
        assert!(!cleaned.contains("陆清漪到了沈渡"));
        assert!(!cleaned.contains("沈渡说话"));
    }

    #[test]
    fn sanitize_chapter_body_removes_unmatched_trailing_ascii_bracket_in_cjk() {
        let raw = "系统提示：检测到关键决策节点。是否启用‘断点芯片’回溯？]\n系统日志：第1次回溯完成。当前时间线分支：A-01。]";

        let cleaned = sanitize_chapter_body(raw, "暴雨如注", "zh-CN");

        assert!(cleaned.contains("是否启用‘断点芯片’回溯？"));
        assert!(cleaned.contains("时间线分支：A-01。"));
        assert!(!cleaned.contains("？]"));
        assert!(!cleaned.contains("。]"));
    }

    #[test]
    fn target_chapter_parser_ignores_unspecified_instruction_text() {
        let task = "目标章节：未明确章节号；如果用户请求项目状态、连续性、漂移、所有章节检查，则必须先查询项目状态；如果只是读取正文片段且仍不确定章节，先查询项目状态再决定。";

        assert_eq!(extract_target_chapter_number(task), None);
    }

    #[test]
    fn target_chapter_parser_uses_only_explicit_target_segment() {
        let task = "目标章节：第16章；如果不确定不要猜第一章。";

        assert_eq!(extract_target_chapter_number(task), Some(16));
    }

    #[test]
    fn parse_chapter_expansion_output_strips_protocol_and_json_surface_residue() {
        let raw = "```json\n  \"addition\": \"第一段真实正文。<|channel>\\n\\n第二段真实正文。\", \"summary_delta\": \"推进。\", \"key_facts\": [\"真实正文发生\"], \"continuity_updates\": [\"线索推进\"]\n```";

        let parsed = parse_chapter_expansion_output(raw, "Chinese");

        assert!(parsed.addition.contains("第一段真实正文"));
        assert!(parsed.addition.contains("第二段真实正文"));
        assert!(!parsed.addition.contains("<|channel>"));
        assert!(!parsed.addition.contains("\"addition\""));
        assert!(!parsed.addition.contains("```"));
    }

    #[test]
    fn sanitize_chapter_body_strips_trailing_markdown_metadata_block() {
        let raw = "司桥宁走出会议室，知道真正的战斗才刚刚开始。**SummaryDelta:**司桥宁决定继续追查。\n**KeyFacts:**1. 他通知老赵。\n**ContinuityUpdates:**下一章进入复盘。";

        let cleaned = sanitize_chapter_body(raw, "会议室之后", "Chinese");

        assert!(cleaned.contains("真正的战斗才刚刚开始。"));
        assert!(!cleaned.contains("SummaryDelta"));
        assert!(!cleaned.contains("KeyFacts"));
        assert!(!cleaned.contains("ContinuityUpdates"));
        assert!(!cleaned.contains("他通知老赵"));
    }

    #[test]
    fn raw_chapter_expansion_allows_clean_json_wrapper() {
        let raw = r#"{"addition":"陆远握紧左臂，听见灵脉深处传来稳定的回声。","summary_delta":"陆远稳定灵脉。","key_facts":["陆远稳定灵脉"],"continuity_updates":["灵脉暂时稳定"]}"#;

        let reason = raw_chapter_expansion_rejection_reason(raw, "Chinese");

        assert!(
            reason.is_none(),
            "clean JSON wrapper should pass: {reason:?}"
        );
    }

    #[test]
    fn raw_chapter_expansion_rejects_foreign_script_fragment_for_revision() {
        let raw = r#"{"addition":"陆远没有取而나代之，而是把左臂贴在灵脉边缘。","summary_delta":"陆远选择稳定灵脉。"}"#;

        let reason = raw_chapter_expansion_rejection_reason(raw, "Chinese");

        assert!(reason.is_some(), "foreign script residue must reach revision");
    }

    #[test]
    fn raw_chapter_expansion_allows_repairable_invalid_escape_before_cjk() {
        let raw = "\"addition\":\"陆远继续向前。\\并没有立刻冲进核心。\"";

        let reason = raw_chapter_expansion_rejection_reason(raw, "Chinese");

        assert!(
            reason.is_none(),
            "repairable escape residue should be cleaned before rejection: {reason:?}"
        );
    }

    #[test]
    fn raw_chapter_expansion_allows_repairable_unclosed_quote() {
        let raw = "陆远听见‘核心门正在打开，他没有回头。";

        let reason = raw_chapter_expansion_rejection_reason(raw, "Chinese");

        assert!(
            reason.is_none(),
            "single unclosed CJK quote should be repaired before rejection: {reason:?}"
        );
    }

    #[test]
    fn raw_chapter_expansion_allows_common_chinese_words_with_modal_chars() {
        let raw = "陆远穿过旧城酒吧的霓虹门廊，听见灵脉在地下回响。酒馆外的雨仍在落。";

        let reason = raw_chapter_expansion_rejection_reason(raw, "Chinese");

        assert!(
            reason.is_none(),
            "common words such as 酒吧 should not trip the particle typo gate: {reason:?}"
        );
    }

    #[test]
    fn sanitize_chapter_body_preserves_ambiguous_ascii_quotes_for_review() {
        let raw = "泵站主干道的红光褪去，取而'代'之的是惨白晨曦。";

        let sanitized = sanitize_chapter_body(raw, "第二十章", "Chinese");

        assert!(sanitized.contains("取而'代'之"));
    }

    #[test]
    fn sanitize_chapter_body_removes_stale_leading_heading_stack() {
        let raw = "# 灵力火光\n\n# 站起身\n# 规则坍塌：秩序的崩解\n议事厅的灯火在寒风中摇曳。";

        let sanitized = sanitize_chapter_body(raw, "灵力火光", "Chinese");

        assert!(sanitized.starts_with("# 灵力火光\n\n议事厅的灯火"));
        assert!(!sanitized.contains("# 站起身"));
        assert!(!sanitized.contains("# 规则坍塌"));
    }

    #[test]
    fn local_revision_suggestions_patch_typo_windows_without_rewriting_body() {
        let body = "所有的生命冗惶被清理干净之前，蓝光构注成森林，真相往就在节点附近。";
        let issues = vec![
            "存在错字/词语拼接错误：'所有的生命冗惶被清理干净之前'（应为'冗余'）"
                .to_string(),
            "存在错字/词语拼接错误：'由蓝光构注成的'（应为'构筑'）".to_string(),
            "存在错字/词语拼接错误：'真相往就在最危险的逻辑节点附近'（应为'真相就在'或'真相往往就在'）"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(repaired.contains("生命冗余被清理"));
        assert!(repaired.contains("蓝光构筑成森林"));
        assert!(repaired.contains("真相就在节点附近") || repaired.contains("真相往往就在节点附近"));
        assert!(!repaired.contains("冗惶"));
        assert!(!repaired.contains("构注"));
        assert!(!repaired.contains("真相往就在"));
    }

    #[test]
    fn local_revision_suggestions_patch_missing_suffix_copyedit() {
        let body = "一旦靠近遗迹，命火可能会引发更剧烈的坍。";
        let issues = vec!["‘坍’字后面似乎漏掉了‘塌’或‘塌陷’".to_string()];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(repaired.contains("更剧烈的坍塌"), "{repaired}");
        assert!(!repaired.contains("更剧烈的坍。"), "{repaired}");
    }

    #[test]
    fn local_revision_suggestions_patch_near_miss_character_drift() {
        let body = "辛岑来向辛岑宁，眼神中充满了前所未有的危机感。";
        let issues = vec![
            "possible character name drift: `辛岑来` is close to stable contract character `辛岑宁` but is not recorded in the story contract or truth ledger"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(repaired.contains("辛岑宁向辛岑宁"));
        assert!(!repaired.contains("辛岑来"));
    }

    #[test]
    fn audit_next_action_blocked_requires_typed_hard_finding() {
        let audit = serde_json::json!({
            "next_action": "revise_draft",
            "review_cycle": {
                "next_action": "blocked"
            }
        });

        assert!(!audit_next_action_blocked(&audit));
    }

    #[test]
    fn status_packet_with_unapproved_chapter_blocks_outer_completion() {
        let status = serde_json::json!({
            "success": true,
            "state": {
                "chapters": 3,
                "approved_chapters": 2,
                "first_unapproved_chapter": 3
            }
        });

        assert!(status_packet_reports_unapproved_chapters(&status));

        let clean = serde_json::json!({
            "state": {
                "chapters": 3,
                "approved_chapters": 3,
                "first_unapproved_chapter": null
            }
        });

        assert!(!status_packet_reports_unapproved_chapters(&clean));
    }

    #[test]
    fn local_revision_suggestions_patch_short_near_miss_windows() {
        let body = "天花风垂下无数细长纤维，排物排斥这两个异物。";
        let issues = vec![
            "存在错字/词语拼接错误：'天风垂下' 应为 '天花板垂下'；'排物排斥' 应为 '排斥'。"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(repaired.contains("天花板垂下"));
        assert!(repaired.contains("排斥这两个异物"));
        assert!(!repaired.contains("天花风垂下"));
        assert!(!repaired.contains("排物排斥"));
    }

    #[test]
    fn local_revision_suggestions_patch_real_reuse_existing_blockers() {
        let body = "实验室的地风都似乎发出了沉闷的回响。那句话直接在陶闻遥识中响起，陶闻遥识到自己已经没有退路。他的指尖即将触碰到开关的刹刹，廉█的义体光泽忽然暗了下去。";
        let issues = vec![
            "chapter body contains malformed phrase near stable character anchor `陶闻遥`: 陶闻遥识到".to_string(),
            "错字/词语拼接错误：'实验室的地风' 应为 '实验室的地板' 或 '实验室的风' (语意不明)".to_string(),
            "错字/词语拼接错误：'指尖即将触碰到开关的刹刹' 应为 '刹那'".to_string(),
            "错字/词语拼接错误：'直接在陶闻遥识中响起' 应为 '在陶闻遥意识中响起'".to_string(),
            "错字/词语拼接错误：'陶闻遥识到' 应为 '陶闻遥意识到' 或 '陶闻遥感知到'".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(
            repaired.contains("实验室的地板都似乎发出了沉闷的回响"),
            "{repaired}"
        );
        assert!(repaired.contains("在陶闻遥意识中响起"), "{repaired}");
        assert!(
            repaired.contains("陶闻遥意识到自己已经没有退路"),
            "{repaired}"
        );
        assert!(repaired.contains("开关的刹那"), "{repaired}");
        assert!(!repaired.contains("地风"), "{repaired}");
        assert!(!repaired.contains("陶闻遥识"), "{repaired}");
        assert!(!repaired.contains("刹刹"), "{repaired}");
    }

    #[test]
    fn local_revision_suggestions_patch_reusable_chapter_copyedit_blockers() {
        let body = "枯叶川得意地笑道：“灵火乃天道所化。”韩照野递给他一碗热气腾腾的药汤：“喝了笑，能缓解枯毒。”";
        let issues = vec![
            "人物名称漂移：枯叶卫在对话中称呼主角斩断灵火的成就者为'枯叶川'（'枯叶川得意地笑道'），与之前的'枯叶卫'不一致，且前文未提及'枯叶川'这一名字。".to_string(),
            "对话错别字：韩照野递药汤时说'喝了笑，能缓解枯毒'，'笑'字应为'药'或'后'，属于明显错字。".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(body, &issues);

        assert!(repaired.contains("枯叶卫得意地笑道"), "{repaired}");
        assert!(
            repaired.contains("喝了药，能缓解枯毒") || repaired.contains("喝了后，能缓解枯毒"),
            "{repaired}"
        );
        assert!(!repaired.contains("枯叶川"), "{repaired}");
        assert!(!repaired.contains("喝了笑"), "{repaired}");
    }

    #[test]
    fn sanitize_chapter_body_removes_omission_placeholder_lines() {
        let raw = "第一段真实正文。\n\n（此处省略约3800字，后续剧情展开。）\n\n第二段真实正文。";

        let sanitized = sanitize_chapter_body(raw, "第一章", "Chinese");

        assert!(sanitized.contains("第一段真实正文"));
        assert!(sanitized.contains("第二段真实正文"));
        assert!(!sanitized.contains("此处省略"));
        assert!(!sanitized.contains("后续剧情"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_ambiguous_cjk_spacing_for_review() {
        let raw = "季棠 安握住长剑。\n秦照 澜后退半步。";

        let sanitized = sanitize_chapter_body(raw, "第一章", "Chinese");

        assert!(sanitized.contains("季棠 安握住长剑"));
        assert!(sanitized.contains("秦照 澜后退半步"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_suspected_cjk_typos_for_review() {
        let raw = "天花啦板上的灯光摇晃，空间扭吗曲成一道暗纹，陆远扶住墙壁。";

        let sanitized = sanitize_chapter_body(raw, "第15章", "Chinese");

        assert!(sanitized.contains("天花啦板上的灯光"));
        assert!(sanitized.contains("空间扭吗曲成"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_standalone_foreign_script_for_review() {
        let raw = "第一段真实正文。\n\\나\n第二段继续推进。";

        let sanitized = sanitize_chapter_body(raw, "第15章", "Chinese");

        assert!(sanitized.contains("第一段真实正文"));
        assert!(sanitized.contains("第二段继续推进"));
        assert!(sanitized.contains('나'));
    }

    #[test]
    fn sanitize_chapter_body_preserves_trailing_ascii_prose_for_review() {
        let raw = "沈墨抬头，看见雨幕里所有纸人同时转身。unawarethatsomewhereinthecity,ahundredpaperfiguresarewakingup,theirpaperbonescreakingintherain,waitingfortheirmasterscommand.";

        let sanitized = sanitize_chapter_body(raw, "雨夜纸人", "zh-CN");

        assert!(sanitized.contains("沈墨抬头，看见雨幕里所有纸人同时转身。"));
        assert!(sanitized.contains("unawarethat"), "{sanitized}");
        assert!(sanitized.contains("paperfigures"), "{sanitized}");
    }

    #[test]
    fn sanitize_chapter_body_removes_standalone_generation_number_residue() {
        let raw = "第一段真实正文。\n1\n第二段继续推进。";

        let sanitized = sanitize_chapter_body(raw, "第1章", "Chinese");

        assert!(sanitized.contains("第一段真实正文"));
        assert!(sanitized.contains("第二段继续推进"));
        assert!(!sanitized.lines().any(|line| line.trim() == "1"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_prefix_fragments_for_contextual_revision() {
        let raw = "这种静谧并非来自外界的消\n这种静谧并非来自外界的消减，而更像是一种内在的坍缩。\n他开始尝试将自己的心跳，与那股宏大的脉冲进行错位式的对\n他开始尝试将自己的心跳，与那股宏大的脉冲进行错位式的对齐。";

        let sanitized = sanitize_chapter_body(raw, "第10章", "Chinese");

        assert!(sanitized.contains("这种静谧并非来自外界的消减"));
        assert!(sanitized.contains("错位式的对齐"));
        assert!(sanitized.contains("消\n这种静谧"));
        assert!(sanitized.contains("对\n他开始尝试"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_ambiguous_line_boundaries_for_review() {
        let raw = "设备仿佛在视\n视线中扭曲成线。\n裂口通向不可名\n名状的维度。";

        let sanitized = sanitize_chapter_body(raw, "第1章", "Chinese");

        assert!(sanitized.contains("在视\n视线"));
        assert!(sanitized.contains("不可名\n名状"));
    }

    #[test]
    fn sanitize_chapter_body_removes_standalone_chapter_end_marker() {
        let raw = "雨还在下。\n第一章，完。\n他推开门，发现旧收音机亮了起来。";

        let sanitized = sanitize_chapter_body(raw, "巷子深处", "Chinese");

        assert!(sanitized.contains("雨还在下。"));
        assert!(sanitized.contains("他推开门"));
        assert!(!sanitized.contains("第一章，完。"));
    }

    #[test]
    fn local_revision_repairs_realistic_fragment_issue_wording() {
        let raw = "林克的眼睛里闪烁着廉█的义体光泽。起许，那只是一阵干扰。陶闻遥到一阵眩晕。";
        let issues = vec![
            "文本中存在明显的字符/文字残片：'廉█的义体光泽'（可能是'廉价'的错字或屏蔽导致的残片）"
                .to_string(),
            "存在逻辑/输入错误：'起许，那只是一阵...'（应为'起初'）".to_string(),
            "存在词语重复/拼接错误：'陶闻遥到'（应为'陶闻遥感到'）".to_string(),
        ];

        let repaired = sanitize_chapter_body(
            &apply_local_revision_suggestions(raw, &issues),
            "第1章",
            "Chinese",
        );

        assert!(repaired.contains("廉价的义体光泽"), "{repaired}");
        assert!(repaired.contains("起初，那只是一阵干扰"), "{repaired}");
        assert!(repaired.contains("陶闻遥感到一阵眩晕"), "{repaired}");
        assert!(!repaired.contains('█'), "{repaired}");
        assert!(!repaired.contains("起许"), "{repaired}");
        assert!(!repaired.contains("陶闻遥到"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_audit_suggested_copyedits_without_rewriting_body() {
        let raw = "许澈禾到一阵莫名的心悸。他把灵压将增降到最低，却像一拳撞风胸口。许澈禾到耳膜一阵紧缩。许澈禾觉到自己的丹田在震动。";
        let issues = vec![
            "存在局部错词：'将增降到最低' 应为 '将增益降到最低' 或 '将增益调至最低'"
                .to_string(),
            "存在词语误写：'撞风' 应为 '撞击'".to_string(),
            "chapter body contains malformed phrase near stable character anchor `许澈禾`: 许澈禾到一阵"
                .to_string(),
            "chapter body contains malformed phrase near stable character anchor `许澈禾`: 许澈禾到耳膜"
                .to_string(),
            "chapter body contains malformed phrase near stable character anchor `许澈禾`: 许澈禾觉到自己的丹"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("许澈禾感到一阵莫名的心悸"), "{repaired}");
        assert!(
            repaired.contains("将增益降到最低") || repaired.contains("将增益调至最低"),
            "{repaired}"
        );
        assert!(repaired.contains("一拳撞击胸口"), "{repaired}");
        assert!(repaired.contains("许澈禾感到耳膜一阵紧缩"), "{repaired}");
        assert!(
            repaired.contains("许澈禾感觉到自己的丹田在震动"),
            "{repaired}"
        );
        assert!(!repaired.contains("许澈禾到一阵"), "{repaired}");
        assert!(!repaired.contains("许澈禾到耳膜"), "{repaired}");
        assert!(!repaired.contains("许澈禾觉到"), "{repaired}");
        assert!(!repaired.contains("撞风"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_blocked_audit_copyedit_suggestions() {
        let raw = "他冷声说：动手腕，洛衡隅。并那名流浪者并没有求饶。洛衡并平静且毫无波澜，高强改度的灵力剥离开始了。";
        let issues = vec![
            "存在逻辑/词语拼接错误：'动手腕，洛衡隅'（明显错字，应为'动手吧，洛衡隅'）"
                .to_string(),
            "存在逻辑/词语拼接错误：'并那名流浪者并没有求饶'（句首多余的'并'字，应为'那名流浪者并没有求饶'）"
                .to_string(),
            "存在逻辑/词语拼接错误：'洛衡并平静且毫无波澜'（应为'洛衡隅表现得平静'）"
                .to_string(),
            "存在逻辑/词语拼接错误：'高强改度的灵力剥离'（应为'高强度的灵力剥离'）"
                .to_string(),
        ];

        assert!(issues
            .iter()
            .all(|issue| !local_text_repair_pairs(issue).is_empty()));

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("动手吧，洛衡隅"), "{repaired}");
        assert!(repaired.contains("那名流浪者并没有求饶"), "{repaired}");
        assert!(repaired.contains("洛衡隅表现得平静"), "{repaired}");
        assert!(repaired.contains("高强度的灵力剥离"), "{repaired}");
        assert!(!repaired.contains("动手腕"), "{repaired}");
        assert!(!repaired.contains("高强改度"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_multi_clause_chinese_copyedits_without_rewrite() {
        let raw = "陆沉舟原地，感受着体内那股躁动。陆沉舟白芷宁即将隐入风沙前开口。真相往隐藏在裂缝里。她没有给出方向，让陆沉舟到一丝挫败。随着她的声音远去，陆沉舟到周围的空气重新变得沉重起来。陆沉舟那座半透明的晶质遗迹旁，心里一沉。";
        let issues = vec![
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟到一丝挫败"
                .to_string(),
            "存在明显的错字/词语拼接错误：'陆沉舟到一丝挫败'应为'陆沉舟感到一丝挫败'；'陆沉舟原地'应为'陆沉舟站在原地'；'陆沉舟白芷宁即将隐入'应为'陆沉舟在白芷宁即将隐入'；'真相往隐藏'应为'真相往往隐藏'。"
                .to_string(),
            "存在逻辑/语句残缺：'陆沉舟到周围的空气重新变得沉重起来'应为'陆沉'后接动词或'陆沉舟周围的空气'。"
                .to_string(),
            "存在重复/逻辑混乱：'陆沉舟那座半透明的晶质遗迹旁'，逻辑语义不明，应为'在遗迹旁'或'陆沉舟站在遗迹旁'。"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("陆沉舟站在原地"), "{repaired}");
        assert!(
            repaired.contains("陆沉舟在白芷宁即将隐入风沙前开口"),
            "{repaired}"
        );
        assert!(repaired.contains("真相往往隐藏在裂缝里"), "{repaired}");
        assert!(repaired.contains("陆沉舟感到一丝挫败"), "{repaired}");
        assert!(
            repaired.contains("陆沉舟感到周围的空气重新变得沉重起来")
                || repaired.contains("陆沉舟周围的空气重新变得沉重起来"),
            "{repaired}"
        );
        assert!(
            repaired.contains("陆沉舟站在遗迹旁，心里一沉"),
            "{repaired}"
        );
        assert!(!repaired.contains("陆沉舟原地"), "{repaired}");
        assert!(!repaired.contains("陆沉舟白芷宁"), "{repaired}");
        assert!(!repaired.contains("真相往隐藏"), "{repaired}");
        assert!(!repaired.contains("陆沉舟到一丝"), "{repaired}");
        assert!(
            !repaired.contains("陆沉舟那座半透明的晶质遗迹旁"),
            "{repaired}"
        );
    }

    #[test]
    fn local_revision_repairs_duplicate_cjk_before_open_quote() {
        let raw = "他们终于理解了共“共振”不是口号，而是一种代价。";
        let issues = vec![
            "quality gate: chapter body contains likely malformed CJK prose: duplicated CJK quote fragment: 共“共振”"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("理解了“共振”不是口号"));
        assert!(!repaired.contains("共“共振”"));
    }

    #[test]
    fn sanitize_chapter_body_removes_markup_math_residue_from_chinese_prose() {
        let raw =
            "ightarrow$ 硬件温度升高 $\n$ $\n\\ ^{她意识到风险正在扩大。\n\\ l她知道风险正在扩大。\n\\来看，她已经没有退路。\n她没有回头路了。\\ l";

        let sanitized = sanitize_chapter_body(raw, "第一章", "Chinese");

        assert!(sanitized.contains("硬件温度升高"));
        assert!(sanitized.contains("她意识到风险正在扩大。"));
        assert!(sanitized.contains("她知道风险正在扩大。"));
        assert!(sanitized.contains("来看，她已经没有退路。"));
        assert!(sanitized.contains("她没有回头路了。"));
        assert!(!sanitized.contains("ightarrow"));
        assert!(!sanitized.contains('$'));
        assert!(!sanitized.contains("\\ ^{"));
        assert!(!sanitized.contains("\\ l"));
        assert!(!sanitized.contains("\\来看"));
    }

    #[test]
    fn sanitize_chapter_body_removes_markup_wrapper_before_chinese_quote() {
        let raw = "\\ ^{“墨羽，你所谓的秩序，不过是建立在沙基上的囚笼！”";

        let sanitized = sanitize_chapter_body(raw, "虚空袭城", "zh-CN");

        assert!(sanitized.contains("墨羽，你所谓的秩序"));
        assert!(!sanitized.contains("\\ ^{"));
        assert!(!sanitized.contains('^'));
        assert!(!sanitized.contains('{'));
    }

    #[test]
    fn sanitize_chapter_body_removes_inline_markdown_emphasis_markers() {
        let raw = "她打开最终**报表**，发现底层**资料**和归档记录不一致。";

        let sanitized = sanitize_chapter_body(raw, "边缘局的博弈", "zh-CN");

        assert!(sanitized.contains("最终报表"));
        assert!(sanitized.contains("底层资料"));
        assert!(!sanitized.contains("**"));
    }

    #[test]
    fn content_cleanup_detection_finds_markup_math_residue() {
        assert!(content_contains_surface_cleanup_target(
            "ightarrow$ 硬件温度升高 $"
        ));
        assert!(content_contains_surface_cleanup_target(
            "\\ l她知道风险正在扩大。"
        ));
        assert!(content_contains_surface_cleanup_target(
            "\\来看，她已经没有退路。"
        ));
        assert!(content_contains_surface_cleanup_target(
            "她没有回头路了。\\ l"
        ));
        assert!(!content_contains_surface_cleanup_target(
            "她知道风险正在扩大。"
        ));
    }

    #[test]
    fn content_cleanup_detection_finds_inline_markdown_emphasis_residue() {
        assert!(content_contains_surface_cleanup_target(
            "她打开最终**报表**，发现底层**资料**和归档记录不一致。"
        ));
    }

    #[test]
    fn continuation_output_boundaries_do_not_trigger_project_cleanup() {
        let task = "继续当前项目写完整5万字。正文保存成txt，不要把长正文塞进聊天框，只返回进度、章节、字数、文件路径、简短摘要和审查状态。";

        assert!(!task_requests_novel_surface_cleanup(task));
    }

    #[test]
    fn revision_then_continuation_does_not_trigger_project_cleanup() {
        let task = "继续刚才这本书，不要新开项目。第15章现在是未通过草稿，请先清理和修订第15章，审查通过后批准并写入文件；然后继续第16章。聊天里只返回进度、章节号、字数、审查状态和txt文件路径。";

        assert!(!task_requests_novel_surface_cleanup(task));
    }

    #[test]
    fn explicit_surface_cleanup_still_routes_to_project_cleanup() {
        let task = "请清理项目里的转义残片、模型说明和 LaTeX 残片，然后重新导出 txt。";

        assert!(task_requests_novel_surface_cleanup(task));
    }

    #[test]
    fn finale_instruction_allows_exceeding_target_for_natural_ending() {
        let gate = ProjectCompletionGateDecision {
            target_reached: true,
            narrative_closed: false,
            complete: false,
            needs_finale: true,
            reason: "字数够了但缺结尾".to_string(),
            finale_brief: Some("让主角与核心关系线完成选择。".to_string()),
            debt_ids: vec!["ending-final-state".to_string()],
        };

        let task = append_finale_instruction("写一部10万字言情小说", &gate, "Chinese");

        assert!(task.contains("允许超过目标字数"));
        assert!(task.contains("类型化合同债务"));
        assert!(task.contains("不开启新主线"));
        assert!(task.contains("字数够了但缺结尾"));
        assert!(task.contains("最终正文 observer"));
    }

    #[test]
    fn complete_narrative_request_allows_elastic_finale() {
        assert!(task_requests_complete_narrative(
            "继续这本书，写到真正结尾，不要新开书。"
        ));
        assert!(!task_requests_complete_narrative(
            "继续这本书，先写下一章。"
        ));
    }

    #[test]
    fn unsuccessful_tool_result_can_be_classified_by_error_code() {
        let title_conflict = json!({
            "success": false,
            "error": "title_conflict"
        });
        let other_error = json!({
            "success": false,
            "error": "content_required"
        });

        assert!(novel_studio_error_is(&title_conflict, "title_conflict"));
        assert!(!novel_studio_error_is(&other_error, "title_conflict"));
    }

    #[test]
    fn user_facing_task_brief_prefers_original_request_body() {
        let brief = user_facing_task_brief(
            "Original user request:\n请写前两章\n\nDelegated task:\nCreate a detailed setting",
        );

        assert_eq!(brief, "请写前两章");
    }

    #[test]
    fn chinese_task_rejects_english_title_candidate() {
        assert!(title_language_mismatch(
            "请写一部玄幻小说",
            "The Verdant Resonance"
        ));
        assert!(title_language_mismatch(
            "Write a fantasy novel in Chinese",
            "The Verdant Resonance"
        ));
        assert!(!title_language_mismatch("请写一部玄幻小说", "九脉灵劫"));
    }

    #[test]
    fn chapter_generation_limits_bind_target_to_runtime_char_cap() {
        let limits = chapter_generation_limits(Some(5000), "Chinese");
        let initial_limits = initial_chapter_generation_limits(Some(5000), "Chinese");
        let segment_limits = chapter_segment_generation_limits(800, "Chinese");

        assert_eq!(limits.max_tokens, Some(12_000));
        assert_eq!(limits.target_chars, Some(5000));
        assert_eq!(limits.hard_max_chars, Some(10_000));
        assert_eq!(initial_limits.max_tokens, Some(13_000));
        assert_eq!(initial_limits.target_chars, Some(5500));
        assert_eq!(initial_limits.hard_max_chars, Some(10_000));
        assert_eq!(segment_limits.max_tokens, Some(3600));
        assert_eq!(segment_limits.hard_max_chars, Some(3600));
        assert_eq!(minimum_chapter_units(5000), 4000);
        assert_eq!(required_chapter_units(5000), 5000);
        assert_eq!(required_chapter_units(2500), 2500);
        assert_eq!(chapter_step_duration_secs(Some(5000), Some(500_000)), 1554);
        assert_eq!(chapter_expansion_round_budget(5000, 3600), 1);
        assert_eq!(chapter_expansion_segment_target(5000, 1400), 1820);
        assert_eq!(chapter_expansion_round_budget(2500, 2499), 1);
    }

    #[test]
    fn draft_summary_is_repaired_after_jsonish_fallback_cleanup() {
        let mut draft = novel_runner::DraftOutput {
            title: "第6章".to_string(),
            content: "第一段真实正文。第二段继续推进人物选择。".to_string(),
            summary: "{ \"title\": \"第6章\", \"content\": \"第一段真实正文。".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        };

        repair_draft_summary_after_body_cleanup(&mut draft, "Chinese");

        assert_eq!(draft.summary, "第一段真实正文。");
    }

    #[test]
    fn appended_summary_deltas_are_compacted_after_expansion() {
        let mut draft = novel_runner::DraftOutput {
            title: "第6章".to_string(),
            content: "闻庭安踏进雨夜，发现旧印正在发烫。他沿着旧街追查，终于看见灯塔下的裂缝。裂缝里传来同伴留下的求救声。".to_string(),
            summary: "闻庭安发现旧印发烫。 闻庭安追查旧街裂缝。 闻庭安听见同伴求救。".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        };

        repair_draft_summary_after_body_cleanup(&mut draft, "Chinese");

        assert_eq!(draft.summary, "闻庭安踏进雨夜，发现旧印正在发烫。");
    }


    #[test]
    fn execution_package_fallback_preserves_required_memo_shape() {
        let package = fallback_chapter_execution_package(
            "Chinese",
            "长歌记",
            6,
            r#"{"truth":"上一章状态"}"#,
            false,
            None,
        );

        assert!(package.memo.body.contains("## 当前任务"));
        assert!(package.memo.body.contains("## 不要做"));
        assert!(package.architecture.contains("章尾落点"));
        assert!(!package.degraded);
        assert!(package.degraded_reason.is_empty());
    }

    #[test]
    fn execution_package_fallback_does_not_embed_full_context_or_old_architecture() {
        let context = json!({
            "project": {
                "title": "问道纪",
                "genre": "赛博朋克玄幻",
                "brief": "主角在高噪音城市中求生。"
            },
            "continuity_anchors": {
                "characters": ["陆远", "少女"]
            },
            "recent_chapters": [{
                "number": 19,
                "title": "第19章：热量的余烬",
                "summary": "陆远与少女转入泵站主干道，左臂刺痛加重。"
            }],
            "architecture": {
                "architecture": "旧的递归架构内容不应该被再次嵌入。"
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "问道纪", 20, &context, false, None);

        assert!(package.architecture.contains("稳定角色锚点：陆远、少女"));
        assert!(package.architecture.contains("第 19 章"));
        assert!(!package.architecture.contains("旧的递归架构内容"));
        assert!(!package.architecture.contains("\"architecture\""));
    }

    #[test]
    fn execution_package_fallback_uses_chapter_goal_from_context_outline() {
        let context = json!({
            "project_context": {
                "contract": {
                    "outline": "第01章《灰度街道》：本章目标：主角首次发现逻辑裂纹并付出记忆代价\n第02章《记忆留白》：本章目标：主角发现能力带来的记忆缺失"
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "裂纹之城", 1, &context, false, None);

        assert!(package.memo.goal.contains("首次发现逻辑裂纹"));
        assert!(package.memo.body.contains("## 本章目标"));
        assert!(!package.memo.body.contains("## 本章合同"));
        assert!(package.memo.body.contains("不要把上面的目标"));
        assert!(package.memo.body.contains("付出记忆代价"));
        assert!(package.memo.body.contains("本章目标素材"));
        assert!(package.memo.body.contains("禁止原句进入正文"));
    }

    #[test]
    fn execution_package_fallback_uses_presealed_canonical_contract_goal() {
        let context = json!({
            "schema_version": "benshu.presealed_execution_authority.v1",
            "chapter_number": 1,
            "canonical_contract": {
                "premise": "测绘师在折叠城市发现重力异常。",
                "characters": [{"canonical_name": "谢星衡", "role": "主角"}],
                "outline": {
                    "near_chapters": [
                        {
                            "number": 1,
                            "goal": "谢星衡在例行校准中发现核心重力数据异常",
                            "expected_turn": "章末确认异常来自人为操作"
                        },
                        {
                            "number": 2,
                            "goal": "谢星衡前往旧巷追查异常源头",
                            "expected_turn": "章末遇到关键关系对象"
                        }
                    ]
                }
            },
            "next_chapter_boundary": [{
                "number": 2,
                "goal": "谢星衡前往旧巷追查异常源头",
                "expected_turn": "章末遇到关键关系对象"
            }]
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "折叠重力", 1, &context, false, None);

        assert!(package.memo.goal.contains("例行校准"));
        assert!(package.memo.goal.contains("人为操作"));
        assert!(package.title_basis.contains("核心重力数据异常"));
        assert!(package.memo.body.contains("下一章边界"));
        assert!(package.memo.body.contains("前往旧巷"));
        assert!(!package.architecture.contains("继承合同并完成一个可验证变化"));
    }

    #[test]
    fn execution_package_fallback_preserves_future_boundary_without_echoing_next_node() {
        let context = json!({
            "project_context": {
                "contract": {
                    "outline": {
                        "near_chapters": [
                            {
                                "number": 3,
                                "goal": "队伍穿过风浪并失去远程通讯",
                                "expected_turn": "船长稳住设备，工程师开始信任他的经验"
                            },
                            {
                                "number": 4,
                                "goal": "队伍抵达断点海域开始勘测",
                                "expected_turn": "抓钩首次成功抓取海缆"
                            }
                        ]
                    }
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "风暴线", 3, &context, false, None);

        assert!(
            package
                .memo
                .body
                .contains("下一章边界（只作为禁区，不得在本章完成）")
        );
        assert!(package.memo.body.contains("抵达断点海域"));
        assert!(package.memo.body.contains("抓钩首次成功抓取海缆"));
        assert!(package.architecture.contains("只围绕"));
        assert!(package.architecture.contains("下一章边界"));
        assert!(package.architecture.contains("抵达断点海域"));
        assert!(package.architecture.contains("抓钩首次成功抓取海缆"));
    }

    #[test]
    fn execution_package_fallback_uses_story_bible_chapter_goal() {
        let context = json!({
            "project_context": {
                "story_bible": {
                    "narrative_graph": {
                        "chapter_goals": [{
                            "chapter_number": 1,
                            "goal": "主角在雨夜签下第一份灵契并付出听觉代价",
                            "moves_toward_ending": "他确认灵契不是馈赠而是债务"
                        }]
                    }
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "灵契账本", 1, &context, false, None);

        assert!(package.memo.goal.contains("签下第一份灵契"));
        assert!(package.memo.body.contains("听觉代价"));
        assert!(package.memo.body.contains("灵契不是馈赠"));
        assert_eq!(package.memo.sections.len(), 8);
    }

    #[test]
    fn execution_package_fallback_derives_opening_goal_when_near_chapter_one_missing() {
        let context = json!({
            "project_context": {
                "contract": {
                    "premise": "主角段予宁在人生最低谷时激活断点芯片，获得回溯关键决策的能力。",
                    "characters": [{
                        "name": "段予宁",
                        "role": "主角"
                    }],
                    "outline": {
                        "near_chapters": [{
                            "number": 2,
                            "goal": "段予宁第一次用断点芯片反击公司审查。",
                            "expected_turn": "反击让芯片代价开始显形。"
                        }]
                    }
                }
            }
        })
        .to_string();

        let package =
            fallback_chapter_execution_package("Chinese", "夺开芯片", 1, &context, false, None);

        assert!(!package.memo.goal.contains("开局章只负责"));
        assert!(package.memo.goal.contains("主角段予宁"));
        assert!(package.memo.goal.contains("第一次不可逆选择"));
        assert!(!package.memo.goal.contains("不要提前兑现后续阶段"));
        assert!(!package.memo.goal.contains("推进《夺开芯片》第 1 章"));
        assert!(package.memo.body.contains("不要把上面的目标"));
    }

    #[test]
    fn finale_fallback_package_closes_without_next_entry() {
        let gate = ProjectCompletionGateDecision {
            target_reached: true,
            narrative_closed: false,
            complete: false,
            needs_finale: true,
            reason: "核心冲突尚未解决；主角与盟友的关系没有落点".to_string(),
            finale_brief: Some("让主角关闭深海核心，并与盟友完成最终选择".to_string()),
            debt_ids: vec![
                "ending-desired-resolution".to_string(),
                "ending-final-state".to_string(),
            ],
        };
        let package = fallback_chapter_execution_package(
            "Chinese",
            "碎灵余烬",
            25,
            r#"{"truth":"主冲突尚未自然收束"}"#,
            true,
            Some(&gate),
        );

        assert!(package.memo.goal.contains("终局/尾声"));
        assert!(package.architecture.contains("关闭深海核心"));
        assert!(package.irreversible_event.contains("关闭深海核心"));
        assert_eq!(package.hook_paid_off[0], "ending-desired-resolution");
        assert!(package.architecture.contains("不再开启新敌人"));
        assert!(package.architecture.contains("不留下下一章入口"));
        assert_eq!(package.memo.sections.len(), 8);
    }

    #[test]
    fn execution_package_llm_is_enabled_by_default_and_can_opt_out() {
        std::env::remove_var("BENSHU_NOVEL_EXECUTION_PACKAGE_LLM");
        assert!(chapter_execution_package_llm_enabled());
        std::env::set_var("BENSHU_NOVEL_EXECUTION_PACKAGE_LLM", "false");
        assert!(!chapter_execution_package_llm_enabled());
        std::env::remove_var("BENSHU_NOVEL_EXECUTION_PACKAGE_LLM");
    }

    #[test]
    fn existing_project_target_update_ignores_chapter_sized_target_shrink() {
        assert_eq!(
            sanitize_existing_project_target_update(Some(4000), Some(500_000)),
            None
        );
        assert_eq!(
            sanitize_existing_project_target_update(Some(750_000), Some(500_000)),
            Some(750_000)
        );
        assert_eq!(
            sanitize_existing_project_target_update(Some(4000), None),
            Some(4000)
        );
    }

    #[test]
    fn chapter_target_preserves_project_band_before_deriving_from_remaining_steps() {
        assert_eq!(
            chapter_unit_target_from_total_and_steps(Some(500_000), 125),
            Some(5_000)
        );
        assert_eq!(
            resolve_chapter_unit_target(None, Some(2_500), Some(50_000), 6),
            Some(2_500)
        );
        assert_eq!(
            resolve_chapter_unit_target(None, Some(6_250), Some(500_000), 125),
            Some(5_000)
        );
        assert_eq!(
            longform_policy::normalize_chapter_unit_target(Some(4_000), Some(500_000)),
            Some(5_000)
        );
        assert_eq!(
            longform_policy::normalize_chapter_unit_target(Some(800), Some(500_000)),
            Some(2_500)
        );
        assert_eq!(
            resolve_chapter_unit_target(Some(8_000), Some(6_250), Some(500_000), 125),
            Some(5_000)
        );
    }

    #[test]
    fn existing_project_turn_count_stops_at_remaining_target() {
        assert_eq!(
            existing_project_turn_chapter_count(
                25,
                96_158,
                Some(100_000),
                Some(5_000),
                true,
                false,
                true
            ),
            1
        );
        assert_eq!(
            existing_project_turn_chapter_count(
                25,
                50_000,
                Some(100_000),
                Some(5_000),
                false,
                false,
                true
            ),
            10
        );
        assert_eq!(
            existing_project_turn_chapter_count(
                1,
                50_000,
                Some(100_000),
                Some(5_000),
                false,
                false,
                true
            ),
            10
        );
        assert_eq!(
            existing_project_turn_chapter_count(
                25,
                50_000,
                Some(100_000),
                Some(5_000),
                false,
                true,
                true
            ),
            1
        );
        assert_eq!(
            existing_project_turn_chapter_count(
                25,
                50_000,
                Some(100_000),
                Some(5_000),
                false,
                false,
                false
            ),
            1
        );
    }

    #[test]
    fn explicit_target_chapter_expands_turn_to_cover_pending_prefix() {
        assert_eq!(expand_chapter_count_to_explicit_target(1, 1, Some(2)), 2);
        assert_eq!(expand_chapter_count_to_explicit_target(1, 1, Some(3)), 3);
        assert_eq!(expand_chapter_count_to_explicit_target(2, 1, Some(2)), 1);
        assert_eq!(expand_chapter_count_to_explicit_target(3, 2, Some(4)), 2);
        assert_eq!(expand_chapter_count_to_explicit_target(3, 1, None), 1);
    }

    #[test]
    fn project_scale_generation_ignores_negated_scope_guidance() {
        let task = "用户已经确认合同。\n\
用户最新要求：按这个开始写第一章。\n\
本轮范围：用户本轮只要求先写第一章/下一章；不要因为总目标字数存在而连续生成全书，完成本章后返回进度。";

        assert!(!task_requests_project_scale_generation(task));
        assert!(task_requests_single_chapter_turn(task));
        assert_eq!(
            existing_project_turn_chapter_count(1, 0, Some(50_000), Some(2_500), false, true, false),
            1
        );
    }

    #[test]
    fn project_scale_generation_does_not_treat_chapter_completion_status_as_whole_book() {
        let task = "用户已经确认合同。\n\
总目标字数：1000000\n\
每章目标字数档位：5000\n\
用户最新要求：按这个开始，先写第一章。请不要展示 JSON、内部路径或工具参数；正文写完后告诉我保存和审稿状态。\n\
本轮范围：用户本轮只要求先写第一章/下一章；不要因为总目标字数存在而连续生成全书，完成本章后返回进度。";

        assert!(!task_requests_project_scale_generation(task));
        assert!(task_requests_single_chapter_turn(task));
        assert_eq!(
            existing_project_turn_chapter_count(
                1,
                0,
                Some(1_000_000),
                Some(5_000),
                false,
                true,
                false,
            ),
            1
        );
    }

    #[test]
    fn explicit_next_chapter_request_with_body_target_is_single_chapter() {
        let task = "用户已经确认合同。\n\
总目标字数：100000\n\
每章目标字数档位：2500\n\
用户最新要求：继续这本《残碑镇魂录》，只写下一章，也就是第4章；沿用已批准设定，不要新开项目，不要重写前文。正文目标约2500字，写完后审稿、批准保存并导出。\n\
本轮范围：用户本轮只要求先写第一章/下一章；不要因为总目标字数存在而连续生成全书，完成本章后返回进度。";

        assert!(!task_requests_project_scale_generation(task));
        assert!(task_requests_single_chapter_turn(task));
        assert_eq!(
            existing_project_turn_chapter_count(
                1,
                10_063,
                Some(100_000),
                Some(2_500),
                false,
                true,
                false,
            ),
            1
        );
    }

    #[test]
    fn project_scale_generation_uses_latest_user_request() {
        let task = "合同摘要：全书大纲已经确认。\n\
用户最新要求：继续写到结尾，完成整本。\n\
本轮范围：如果用户明确要求写完整本，可以连续推进。";

        assert!(task_requests_project_scale_generation(task));
        assert!(!task_requests_single_chapter_turn(task));
        assert_eq!(
            existing_project_turn_chapter_count(1, 0, Some(50_000), Some(2_500), false, false, true),
            20
        );
    }

    #[test]
    fn routed_full_book_scope_remains_project_scale_authority() {
        let task = "用户要求继续当前写作项目。不要重新规划合同，不要新开项目。\n\
USER REQUEST\n\
继续自动处理当前阻塞，并按已确认合同连续完成、审稿和保存整本小说，直到全书完结。\n\
本轮范围：用户要求完成全书；按当前合同推进全部剩余章节到目标规模和结局完成门；每章通过质量门后继续，直到目标达成、叙事闭合或出现明确 blocker。";

        assert!(task_requests_project_scale_generation(task));
        assert!(!task_requests_single_chapter_turn(task));
        assert_eq!(
            existing_project_turn_chapter_count(
                1,
                8_575,
                Some(100_000),
                Some(2_500),
                true,
                false,
                true,
            ),
            37
        );
    }

    #[test]
    fn target_gate_uses_approved_units_not_draft_units() {
        let state = json!({
            "approved_units": 96_000,
            "units": 101_000,
            "target_units": 100_000
        });
        assert!(!state_target_reached_by_approved_units(&state));
        let state = json!({
            "approved_units": 100_000,
            "units": 101_000,
            "target_units": 100_000
        });
        assert!(state_target_reached_by_approved_units(&state));
    }

    #[test]
    fn rule_first_audit_requires_clean_quality_and_truth_receipts() {
        let clean = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let warning = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": ["mechanical warning"]
            },
            "truth_validation": {
                "issues": []
            }
        });
        let truth_issue = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": []
            },
            "truth_validation": {
                "issues": ["truth drift"]
            }
        });

        assert!(write_result_is_clean_for_rule_audit(&clean));
        assert!(write_result_is_clean_for_rule_audit(&warning));
        assert!(!write_result_is_clean_for_rule_audit(&truth_issue));
        assert!(!chapter_requires_periodic_full_audit(4));
        assert!(chapter_requires_periodic_full_audit(5));
    }
