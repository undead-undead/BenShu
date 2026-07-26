    #[test]
    fn planning_dialogue_overrides_complete_book_execution_terms() {
        let message = "跟我多轮自然语言对话，定制小说大纲，写一篇异世界重生玄幻小说，要求草根逆袭，2500字每章，写5万字。请规划清楚结局、章节名字、卷宗名字、书名，然后写完一部完整的。";

        assert!(super::super::creation_draft_planning_dialogue_requested(
            message
        ));
        assert!(super::super::creation_draft_framework_requested(
            message, "fiction"
        ));
        assert!(!super::super::creation_draft_execution_requested(
            message, "fiction"
        ));
        assert!(!super::super::creation_draft_approval_requested(message));
    }

    #[test]
    fn existing_project_status_query_is_read_only_not_continuation() {
        let message = "请检查当前这本《碎灵余烬》是否已经完成。不要新开书，不要贴正文全文，只返回：是否完成、总字数、章节数、最后一章标题、TXT导出路径。";

        assert!(super::super::intent_requests_read_only_existing_artifact_answer(message));
        assert!(!super::super::intent_requests_existing_work_continuation(
            message
        ));
        assert!(!super::super::creation_draft_execution_requested(
            message, "fiction"
        ));
        assert!(!super::super::creation_draft_approval_requested(message));
    }

    #[test]
    fn existing_project_finish_request_is_continuation_not_read_only() {
        let message = "继续这本《碎灵余烬》，如果还没有真正完整结尾，就从当前进度接着写到完整结尾。不要新开书，不要贴正文全文，聊天里只告诉我进度、章节号、字数、文件路径、简短摘要和审查状态。";
        let lowered = message.to_ascii_lowercase();

        assert!(
            super::super::creation_draft_message_requests_continuation_generation(
                message, &lowered
            )
        );
        assert!(!super::super::intent_requests_read_only_existing_artifact_answer(message));
    }

    #[test]
    fn approved_creation_draft_detects_novel_content_crud() {
        assert_eq!(
            super::super::creation_draft_content_operation("总结一下第一章内容。", "fiction"),
            Some(super::super::NovelContentOperation::Read)
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "给第一章结尾增加一次短暂重逢。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Add)
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "删掉第一章里关于旧档案的线索。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Delete)
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "把第一章里女主职业改成咖啡店店长。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Modify)
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "把第一章里主角第一次参加考试时的紧张感改得更轻松幽默一点，不要改主角名字。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Modify)
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "先修复第1章中混入正文的模型说明和输出限制说明，然后继续后续章节。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Modify)
        );
        assert_eq!(
                super::super::creation_draft_content_operation(
                    "继续当前项目的后续章节写作；如果存在 needs_revision 的章节先补足，目标仍是10万字、每章5000字档位。",
                    "fiction"
                ),
                None
            );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "继续当前《长歌记》项目，从第6章继续处理；已通过的章节不要重写。",
                "fiction"
            ),
            None
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "请继续《问道纪》第五章。如果第五章已有未通过草稿，请在同一项目里修正并补足为合格章节。",
                "fiction"
            ),
            None
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "继续刚才的《问道纪》项目，第20章上一版未通过，请接着修好，通过审查后批准为最终章节并导出TXT。",
                "fiction"
            ),
            None
        );
        let recovery_message = "第二章没有通过，重新写第二章";
        let recovery_lowered = recovery_message.to_ascii_lowercase();
        assert!(
            super::super::creation_draft_message_requests_continuation_generation(
                recovery_message,
                &recovery_lowered
            )
        );
        assert_eq!(
            super::super::creation_draft_content_operation(recovery_message, "fiction"),
            None
        );
        assert!(!super::super::intent_requests_read_only_existing_artifact_answer(
                "继续当前《长歌记》项目，从第6章继续处理；已通过的章节不要重写。完成后只返回章节号、字数、路径和简短摘要。"
            ));
        assert!(super::super::referenced_artifact_segment_numbers(
            "目标仍是10万字、每章5000字档位。"
        )
        .is_empty());
        assert_eq!(
            super::super::referenced_artifact_segment_numbers("请修改第10章结尾。"),
            vec![10]
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "把第一章里女主职业改成咖啡店店长。",
                "report"
            ),
            None
        );
    }

    #[test]
    fn concrete_fiction_request_still_requires_contract_confirmation() {
        let message = "写都市玄幻小说，每章2500字，至少5万字。你自动定大纲、结局、书名和角色。";
        let draft = super::super::build_initial_creation_draft("session-a", "fiction", message)
            .expect("draft");

        assert!(!super::super::creation_draft_approval_requested(message));
        assert!(!super::super::creation_draft_execution_requested(
            message, "fiction"
        ));

        let prompt = super::super::final_prompt_from_creation_framework_request(&draft, message);
        assert!(prompt.contains("故事蓝图初始阶段"));
        assert!(!prompt.contains("合同确认阶段"));
        assert!(prompt.contains("不要写正文"));
        assert!(!prompt.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
    }

    #[tokio::test]
    async fn fresh_explicit_write_request_still_requires_contract_generation() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome = super::super::handle_creation_draft_chat(
            &mut runtime,
            "session-a",
            "直接写一部都市玄幻小说，每章2500字，至少5万字。",
        )
        .await
        .expect("handled")
        .expect("outcome");
        let super::super::CreationDraftTurnOutcome::ContinueWithMessage(prompt) = outcome else {
            panic!("fresh explicit write request should still generate a contract first");
        };

        assert!(prompt.contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert!(prompt.contains("初始合同字段包"));
        assert!(prompt.contains("不要把任务理解成法律文书"));
        assert!(!prompt.contains("小说创作合同字段包"));
        assert!(!prompt.contains("甲方"));
        assert!(!prompt.contains("乙方"));
        assert!(prompt.contains("不要写正文"));
        assert!(!prompt.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        assert_eq!(runtime.approved, 0);
    }

    #[tokio::test]
    async fn detailed_fresh_fiction_request_enters_contract_flow_before_chat_generation() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };
        let message = "我想从零写一部长篇玄幻小说，总字数10万字，每章2500字，预计约40章。题材是山河地脉与宗门秩序：一名负责修补凡城地脉裂隙的年轻女地师发现，名门以镇压地灾为名长期抽走偏远城镇的地脉灵息，制造可控灵潮供核心弟子破境，代价是凡城井水枯竭、庄稼石化。她与一名被逐出宗门的阵图抄录师合作，只能依靠地脉测绘、旧阵图、灵石流向和公开宗门问责试炼建立证据。修炼必须有资源、身体和时间代价，不能突然获得万能血脉、神明赐力或无敌法宝。终局必须由可复验的地脉回流阵和公开账册使抽取制度被废止，受损城镇恢复自主养脉；开篇八章不能解决主冲突。现在先建立合同，不要写第一章。";

        let outcome = super::super::handle_creation_draft_chat(
            &mut runtime,
            "fresh-detailed-fiction",
            message,
        )
        .await
        .expect("handled")
        .expect("fiction contract flow");

        let super::super::CreationDraftTurnOutcome::ContinueWithMessage(prompt) = outcome else {
            panic!("detailed fiction request must create a contract prompt");
        };
        assert!(prompt.contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert_eq!(runtime.draft.as_ref().and_then(|draft| draft.target_units), Some(100000));
        assert_eq!(runtime.draft.as_ref().and_then(|draft| draft.chapter_unit_target), Some(2500));
        assert_eq!(runtime.approved, 0);
    }

    #[tokio::test]
    async fn autonomous_genre_request_with_contract_first_enters_contract_flow() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };
        let message = "请从零策划并创作一部现代言情小说，总字数10万字，每章2500字。先生成完整小说合同给我确认，不要开始写正文。故事、书名和角色由你决定。";

        let outcome = super::super::handle_creation_draft_chat(
            &mut runtime,
            "fresh-autonomous-fiction",
            message,
        )
        .await
        .expect("handled")
        .expect("fiction contract flow");

        let super::super::CreationDraftTurnOutcome::ContinueWithMessage(prompt) = outcome else {
            panic!("autonomous fiction request must generate a contract prompt");
        };
        assert!(prompt.contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert_eq!(runtime.draft.as_ref().and_then(|draft| draft.target_units), Some(100000));
        assert_eq!(runtime.draft.as_ref().and_then(|draft| draft.chapter_unit_target), Some(2500));
        assert_eq!(runtime.approved, 0);
    }

    #[test]
    fn fiction_contract_title_gate_uses_story_basis_not_marketing_score() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "标准小说合同草案\n\
书名：霓虹余烬\n\
题材：都市玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角在灵考终局公开校盟剥夺草根灵籍的真相，重建公平晋级规则。\n\
主角弧线：主角从只想自保的旁听生，变成愿意承担代价的规则重写者。\n\
世界观意象：灵考、校盟、借灵证、旧操场下的灵籍碑。\n\
总主线因果链：旁听生被迫参加灵考，发现校盟夺走草根灵籍，最终用灵籍碑反证规则漏洞。\n\
命名理由：体现主角成长。\n\
角色权威表：主角姓名：许闻，欲望：通过灵考，恐惧：再次被校盟抹去身份，底线：不牺牲同学换取晋级。\n\
故事合同：核心矛盾是草根学生与校盟资源垄断；结局承诺是公开真相并重建晋级规则。\n\
结构合同：第一卷《灵考旁听》：进入灵考；第二卷《旧碑作证》：揭开校盟规则；终局：灵籍碑公开真相。\n\
近期章节包：第01章《旁听生》：本章目标：许闻被迫进入灵考。\n\
质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(!issues.iter().any(|issue| issue.contains("书名")), "{issues:?}");
    }

    #[test]
    fn fiction_contract_allows_explained_abstract_title() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "标准小说合同草案\n\
书名：感知静默\n\
题材：都市玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角献祭自身感知能力来封印失控的维度裂缝，换取城市秩序回归。\n\
主角弧线：主角从追逐感知觉醒的人变成愿意承受感知枯竭的守护者。\n\
世界观意象：感知剥夺、频率共振、感知阈值、维度噪音。\n\
总主线因果链：觉醒异能导致感知过载，过载引发维度坍塌，终局必须关闭感知阈值。\n\
命名理由：感知对应主角能力，静默对应终局代价。\n\
角色权威表：主角姓名：姜栖声，欲望：掌控异常频率，恐惧：失去感官，底线：不牺牲无辜者。\n\
故事合同：核心矛盾是感知能力与城市秩序；结局承诺是主角关闭阈值。\n\
结构合同：一句话全书大纲：主角追查频率过载并在终局关闭城市阈值。\n\
近期章节包：第01章《噪音突袭》：本章目标：主角第一次听见城市异常频率。\n\
质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(!issues.iter().any(|issue| issue.contains("书名")), "{issues:?}");
    }

    #[test]
    fn fiction_contract_does_not_apply_a_marketing_hook_threshold() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异世界重生玄幻小说，草根逆袭，2500字每章，总字数5万字。",
        )
        .expect("draft");
        let contract = "标准小说合同草案\n\
书名：碎骨余烬\n\
题材：异世界重生玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角燃烧自身灵骨重塑规则，打破阶级固化。\n\
主角弧线：主角从卑微矿工变成愿意承担代价的规则重写者。\n\
世界观意象：骨骼强度决定修为，寒铁矿区，血脉阶级。\n\
总主线因果链：矿工重生，收集残缺骨髓，发现血脉真相，终局重塑规则。\n\
命名理由：书名寓意主角在破碎中重生的代价与力量。\n\
角色权威表：主角姓名：陆沉，欲望：掌握生存自主权，恐惧：再次沦为阶级附庸，底线：不伤害无辜弱小。\n\
故事合同：核心矛盾是底层矿工与血脉统治；结局承诺是粉碎旧秩序。\n\
结构合同：第一卷《寒铁矿区》：生存挣扎；第二卷《血脉真相》：揭开统治规则；终局：重塑秩序。\n\
近期章节包：第01章《灰烬中的呼吸》：本章目标：主角重生在矿井底层。\n\
质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(!issues.iter().any(|issue| issue.contains("读者钩子")), "{issues:?}");
        assert!(
            !issues.iter().any(|issue| issue.contains("角色命名依据")),
            "{issues:?}"
        );
    }

    #[test]
    fn creation_contract_parser_does_not_treat_contract_sections_as_chapters() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异世界重生玄幻小说，草根逆袭，2500字每章，总字数5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：书名《烬余余烬》，语言：zh-CN，题材：异世界重生玄幻，总字数=50000，每章档位=2500，预计章节数=20，导出格式：txt。\n\
2. 命名依据合同：书名《烬余余烬》；命名理由：世界规则设定为灵力源于燃烧灵魂的余烬，主角通过献祭过往记忆换取力量，最终在灰烬中寻找微光；终局方向：主角献祭所有记忆成为守护世界规则的无名之火；主角弧线：从渴望复仇的孤狼转变为守护秩序的薪火；世界观意象：灰烬、余烬、灵魂之火；总主线因果链：夺回力量—发现代价—献祭自我—秩序重建。\n\
3. 角色权威表：主角姓名：陆离，命名依据：取“离火”之意，象征在灰烬中燃烧的微光，适合其献祭记忆的弧线，欲望：重拾记忆，恐惧：再次被遗忘，底线：不伤害无辜者；对手姓名：枯骨长老，命名依据：象征规则的冷酷化身，欲望：永恒的寂静，恐惧：变化。\n\
4. 故事合同：主题承诺：探讨代价与传承；核心矛盾：个体记忆的存续与世界秩序的平衡；世界规则：力量源于记忆的燃烧；结尾承诺：主角消失，世界规则重塑。\n\
5. 结构合同：一句话大纲：主角通过燃烧记忆获得力量，最终在世界毁灭边缘选择献祭记忆以重塑秩序。第一卷：余烬觉醒，第二卷：记忆代价，第三卷：薪火归一。关键转折：主角发现力量来源即是自身记忆。\n\
6. 近期章节包：\n\
第01章《灰烬睁眼》：本章目标：主角在废墟中苏醒，意识到力量来自记忆碎片。\n\
第02章《燃血试炼》：本章目标：主角通过献祭部分童年记忆换取初级火种。\n\
第03章《寒流袭袭》：本章目标：遭遇规则监察者，主角被迫在逃亡中加速力量演化。\n\
7. 质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            !issues.iter().any(|issue| issue.contains("稳定角色锚点")),
            "{issues:?}"
        );
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("逐章规划缺少章节目标")),
            "{issues:?}"
        );
    }

    #[test]
    fn failed_contract_names_can_be_recorded_as_forbidden_surfaces() {
        let contract = "1. 基本参数：书名《烬余余烬》，语言：zh-CN。\n\
3. 角色权威表：主角姓名：陆烬，命名依据：余烬中挣扎，欲望：生存，恐惧：熄灭，底线：不伤害无辜者；对手姓名：炽阳长老，命名依据：过度燃烧。";

        let names = super::super::generated_contract_forbidden_name_surfaces(contract);

        assert!(names.iter().any(|name| name == "烬余余烬"), "{names:?}");
        assert!(
            !names.iter().any(|name| name == "陆烬"),
            "角色名由写作工具治理，不再把 LLM 角色名作为后续禁用名：{names:?}"
        );
    }

    #[test]
    fn quality_feedback_is_not_rendered_as_story_setting() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异世界重生玄幻小说，草根逆袭，每章2500字，5万字。",
        )
        .expect("draft");
        draft.planning_notes.push(
            "上一版合同草案未通过质量门：书名没有被合同里的大纲、情节、终局或世界观意象支撑"
                .to_string(),
        );
        draft
            .planning_notes
            .push("失败合同禁用命名：烬余余烬、陆烬".to_string());

        let status = super::super::render_creation_draft_compact_status(&draft);
        let outline = super::super::creation_outline_payload(&draft);
        let prompt =
            super::super::final_prompt_from_creation_framework_request(&draft, "请重新生成合同");

        assert!(!status.contains("质量门"), "{status}");
        assert!(!outline.contains("质量门"), "{outline}");
        assert!(!prompt.contains("失败合同禁用命名：烬余余烬、陆烬"), "{prompt}");
        assert!(
            !prompt.contains("- 已记录设定：上一版合同草案未通过质量门"),
            "{prompt}"
        );
    }

    #[test]
    fn hidden_diagnostics_are_not_rendered_as_story_setting() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.diagnostics.push(
            "合同草案通过文本质量门但未通过可写检查，未写入可确认草案：ContractBlocker: 大纲含有JSON残片"
                .to_string(),
        );

        let status = super::super::render_creation_draft_compact_status(&draft);
        let outline = super::super::creation_outline_payload(&draft);
        let prompt =
            super::super::final_prompt_from_creation_framework_request(&draft, "请自动补齐合同");

        assert!(!status.contains("ContractBlocker"), "{status}");
        assert!(!outline.contains("ContractBlocker"), "{outline}");
        assert!(
            !prompt.contains("- 已记录设定：合同草案通过文本质量门"),
            "{prompt}"
        );
    }

    #[test]
    fn lifecycle_ready_is_not_confirmable_when_contract_is_incomplete() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::ContractReady);

        assert!(
            !crate::tool::writing::creation_contract::creation_contract_draft_is_confirmable(
                &draft
            )
        );
    }

    #[test]
    fn split_quality_feedback_fragments_are_not_rendered_as_story_setting() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft
            .planning_notes
            .push("必须先说明结局/过程/世界规则，再从这些内容反推书名".to_string());
        draft.planning_notes.push(
            "角色名 `陆沉` 像模型默认高频名；除非用户明确指定，否则请从当前主角弧线、世界规则、身份代价或结局选择重新命名"
                .to_string(),
        );

        let status = super::super::render_creation_draft_compact_status(&draft);
        let outline = super::super::creation_outline_payload(&draft);

        assert!(!status.contains("反推书名"), "{status}");
        assert!(!status.contains("陆沉"), "{status}");
        assert!(!outline.contains("模型默认高频名"), "{outline}");
    }

    #[test]
    fn start_confirmation_diagnostic_is_not_rendered_as_story_setting() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.planning_notes.push(
            "用户已确认开始，但合同仍缺少系统必需字段，系统将自动补齐后再进入正文：小说合同缺少世界观意象"
                .to_string(),
        );

        let status = super::super::render_creation_draft_compact_status(&draft);
        let outline = super::super::creation_outline_payload(&draft);

        assert!(!status.contains("用户已确认开始"), "{status}");
        assert!(!outline.contains("系统将自动补齐"), "{outline}");
    }

    #[test]
    fn compact_status_shows_structured_contract_summary() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.emotional_contract.emotional_promise = "草根在城市裂隙中守住尊严".to_string();
        draft.antagonist_pressure.primary_pressure = "城市资源规则持续压迫主角".to_string();

        let status = super::super::render_creation_draft_compact_status(&draft);

        assert!(status.contains("结构化合同摘要"), "{status}");
        assert!(status.contains("情感承诺"), "{status}");
        assert!(status.contains("主要压力"), "{status}");
    }

    #[test]
    fn explicit_full_contract_view_renders_structured_contract_details() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "夜校灵债".to_string();
        draft.fiction_title_rationale =
            "夜校来自主角入局地点，灵债来自终局公开灵能债务账册并改写规则。"
                .to_string();
        draft.fiction_premise = "旧城夜校通过灵能债务筛掉普通学生。".to_string();
        draft.fiction_ending_direction = "主角公开灵能债务账册，重写夜校晋级规则。".to_string();
        draft.fiction_protagonist_arc =
            "从只想保住夜校资格的学生，成长为愿意公开证据的规则改写者。"
                .to_string();
        draft.fiction_world_imagery = "旧城夜校、灵能债务、裂隙资源。".to_string();
        draft.fiction_main_causal_spine =
            "夜校异常引出灵能债务账册，追查裂隙资源，终局公开证据改写规则。"
                .to_string();
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 查清灵能债务; fear: 资格被抹除; bottom_line: 不牺牲同学; arc_start: 旁听生; arc_end: 规则改写者".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护夜校债务垄断; fear: 账册公开; bottom_line: 不让证据进入听证; arc_start: 幕后监考者; arc_end: 被证据逼到台前".to_string(),
        ];
        draft.fiction_world_rules = vec!["灵能债务只能通过公开账册解除".to_string()];
        draft.fiction_outline =
            "第一卷《夜校灵债》：主角进入夜校并发现债务账册；卷尾变化：主角拿到第一份账册副本。\n第01章《旧证入场》：本章目标：许闻发现借灵证异常。\n第02章《债务账册》：本章目标：许闻确认夜校债务被人为涂改。".to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.resource_economy.value_scale = "普通工资、灵能债务和裂隙资源形成三层尺度".to_string();
        draft.resource_economy.resource_types = vec!["工资".to_string(), "灵能债务".to_string()];
        draft.power_progression.system_name = "城市裂隙修复等级".to_string();

        let view = super::super::render_creation_draft_contract_view(&draft, true);

        assert!(super::super::intent_requests_creation_contract_view(
            "展示完整合同"
        ));
        assert!(super::super::intent_requests_full_creation_contract_view(
            "展示完整合同"
        ));
        assert!(view.contains("结构化合同完整视图"), "{view}");
        assert!(view.contains("资源类型"), "{view}");
        assert!(view.contains("成长体系"), "{view}");
    }

    #[test]
    fn fiction_contract_view_hides_machine_fields_and_naturalizes_causal_text() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "旧桥灵证".to_string();
        draft.fiction_title_rationale =
            "旧桥是主角找到证据的地点，灵证是终局公开并改写灵网规则的关键物件。"
                .to_string();
        draft.fiction_premise = "旧城灵考用借灵证篡改底层学生资格。".to_string();
        draft.fiction_ending_direction = "主角公开旧桥灵证证据，反转灵网规则。".to_string();
        draft.fiction_protagonist_arc =
            "从只想通过灵考，到愿意公开证据保护底层学生。".to_string();
        draft.fiction_world_imagery = "旧桥、借灵证、灵网账册。".to_string();
        draft.fiction_main_causal_spine =
            "借灵入局 -> 查出旧桥证据 -> 终局反转灵网规则".to_string();
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 通过灵考; fear: 被抹去身份; bottom_line: 不牺牲同学; name_source: generated_by_writing_tool_policy".to_string(),
            "name: 商砚衡; role: 关键对手; desire: 维护灵网资格垄断; fear: 旧桥证据公开; bottom_line: 不让账册进入听证".to_string(),
        ];
        draft.fiction_world_rules = vec!["借灵证会记录并转移考生资格".to_string()];
        draft.fiction_outline =
            "第一卷《旧桥灵证》：目标：许闻进入灵考并确认旧桥证据异常；卷尾变化：许闻成为旧桥证据持有人。\n\
第01章《旧桥夜考》：本章目标：许闻发现旧桥证据。\n\
第02章《借灵账册》：本章目标：许闻确认借灵证转移资格的规则。\n\
第03章《听证入口》：本章目标：许闻把旧桥证据带入公开听证。"
                .to_string();
        fill_complete_fiction_contract_v2(&mut draft);

        let view = super::super::render_creation_draft_contract_view(&draft, true);

        assert!(!view.contains("name_source"), "{view}");
        assert!(!view.contains("->"), "{view}");
        assert!(!view.contains("许闻许闻"), "{view}");
        assert!(view.contains("姓名：许闻"), "{view}");
        assert!(view.contains("借灵入局，然后查出旧桥证据"), "{view}");
        assert!(view.contains("许闻发现旧桥证据"), "{view}");
    }

    #[test]
    fn approval_sync_keeps_structured_contract_characters_authoritative() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let approved = json!({
            "draft": {
                "characters": [
                    "name: 秦知白; role: 主角; desire: 查明城市裂隙; fear: 被规则抹除; bottom_line: 不牺牲无辜",
                    "name: 司望宁; role: 重要角色; desire: 守住旧秩序; fear: 秩序被改写; bottom_line: 反对必须有清晰动机"
                ],
                "structured_contract_v2": {
                    "relationship_ledger": [
                        {
                            "characters": ["秦知白", "司望宁"],
                            "relationship_type": "主角与秩序压力源"
                        }
                    ]
                },
                "outline": "第01章《裂隙初醒》：本章目标：秦知白发现城市裂隙。"
            }
        });

        assert!(super::super::sync_creation_draft_from_approval(
            &mut draft, &approved
        ));
        let authority_names = draft
            .fiction_characters
            .iter()
            .filter_map(|line| super::super::character_name_from_contract_line(line))
            .collect::<std::collections::BTreeSet<_>>();
        let contract = draft.contract_v2();

        for relation in &contract.relationship_ledger {
            for name in &relation.characters {
                assert!(
                    authority_names.contains(name),
                    "structured relation character `{name}` was not present in authoritative characters: {authority_names:?}"
                );
            }
        }
        assert!(
            authority_names
                .iter()
                .any(|name| draft.fiction_outline.contains(name)),
            "{}",
            draft.fiction_outline
        );
    }

    #[test]
    fn natural_contract_with_minor_label_typo_stays_pending() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "\
1. 基本参数：语言：中文；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20；导出格式：txt。
2. 命名依据合同：终局方向：主角用失去记忆的代价封印裂隙。主角弧线：从追逐力量到守住秩序。世界观意意象：霓虹裂隙、记忆票据、地下灵轨。总主线因果链：觉醒能力引出记忆代价，代价逼近身份崩解，终局用遗忘换取城市安宁。
书名：《拿记忆票封城》。
命名理由：记忆票是主角支付代价并留下身份证据的关键物件，封城来自终局用遗忘换取城市安宁的爽点行动。
3. 角色权威表：主角姓名：陆离，命名依据：离散记忆，欲望：找回自我，恐惧：彻底遗忘，底线：不伤害无辜者。
4. 故事合同：主题承诺：代价与平衡。核心矛盾：力量与自我认知冲突。世界规则：能力消耗记忆。关系线：信任与记录。结尾承诺：主角遗忘身份。
5. 结构合同：一句话全书大纲：青年用记忆能力修补城市裂隙。第一卷：裂痕初现。明确结局：主角在街头与记录者擦肩。
6. 近期章节包：
第01章《霓虹幻影》：本章目标：发现能力代价。
第02章《记忆碎片》：本章目标：追查旧物。
第03章《影子猎人》：本章目标：遭遇收割者。";

        let quality_issues = super::super::generated_contract_quality_issues(&draft, contract);
        assert!(quality_issues.is_empty(), "{quality_issues:?}");
        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract,
        ));
        assert!(draft.fiction_world_imagery.is_empty());
        let readiness_issues = super::super::creation_draft_approval_readiness_issues(&draft);
        assert!(
            readiness_issues
                .iter()
                .any(|issue| issue.contains("世界观意象")),
            "{readiness_issues:?}"
        );
    }

    #[test]
    fn repair_only_message_does_not_pollute_brief_or_planning_notes() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异世界重生玄幻小说，草根逆袭，每章2500字，5万字。",
        )
        .expect("draft");
        let original_brief = draft.brief.clone();
        let original_notes = draft.planning_notes.clone();
        let original_public_notes = super::super::stable_creation_planning_notes(&draft);

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "请重新生成合同，修复刚才提示的问题。",
        );

        assert_eq!(draft.brief, original_brief);
        assert_eq!(draft.planning_notes, original_notes);

        let original_title = draft.title.clone();
        super::super::apply_message_to_creation_draft(
            &mut draft,
            "合同里对手姓名“白澈白”首尾重复且不自然，请自动修正异常角色名并同步所有引用，其他设定不要改。",
        );

        assert_eq!(draft.title, original_title);
        assert_eq!(draft.brief, original_brief);
        assert_eq!(
            super::super::stable_creation_planning_notes(&draft),
            original_public_notes
        );
        assert_eq!(
            super::super::forbidden_naming_authority(&draft).character_names,
            vec!["白澈白".to_string()]
        );

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "请对当前整份创作合同做通用质量自检并自动修复：逐项检查角色锚点、分卷目标和因果句是否存在截断残句或缺少谓语。",
        );

        assert_eq!(draft.title, original_title);
        assert_eq!(draft.brief, original_brief);
        assert_eq!(
            super::super::forbidden_naming_authority(&draft).character_names,
            vec!["白澈白".to_string()]
        );
    }

    #[test]
    fn sanitizer_removes_repair_and_quality_fragments_from_existing_brief() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异世界重生玄幻小说，草根逆袭，每章2500字，5万字。",
        )
        .expect("draft");
        draft.brief = "异世界重生玄幻；请重新生成合同，修复刚才提示的问题；上一版合同草案未通过质量门：书名没有依据"
            .to_string();

        super::super::apply_message_to_creation_draft(&mut draft, "重新生成合同");

        assert_eq!(draft.brief, "异世界重生玄幻");
    }

    #[test]
    fn fiction_contract_accepts_title_derived_from_plot_and_ending() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "标准小说合同草案\n\
书名：灵考碑证\n\
题材：都市玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角在灵考终局公开校盟剥夺草根灵籍的真相，重建公平晋级规则。\n\
主角弧线：主角从只想自保的旁听生，变成愿意承担代价的规则重写者。\n\
世界观意象：灵考、校盟、借灵证、旧操场下的灵籍碑。\n\
总主线因果链：旁听生被迫参加灵考，发现校盟夺走草根灵籍，最终用灵籍碑反证规则漏洞。\n\
命名理由：书名取自终局中灵考现场和灵籍碑作证的关键情节，指向主角用证据重写晋级规则。\n\
角色权威表：主角姓名：许闻，命名依据：许是未被校盟承认的旁听身份，闻是听见碑下证词后公开真相的弧线，欲望：通过灵考，恐惧：再次被校盟抹去身份，底线：不牺牲同学换取晋级。\n\
故事合同：核心矛盾是草根学生与校盟资源垄断；结局承诺是公开真相并重建晋级规则。\n\
结构合同：第一卷《灵考旁听》：进入灵考；第二卷《旧碑作证》：揭开校盟规则；终局：灵籍碑公开真相。\n\
近期章节包：第01章《旁听生》：本章目标：许闻被迫进入灵考。\n\
质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            !issues.iter().any(|issue| issue.contains("书名没有被合同")),
            "{issues:?}"
        );
    }

    #[test]
    fn title_repair_does_not_use_uncommitted_natural_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-title-repair",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title.clear();
        let contract = "标准小说合同草案\n\
书名：缄默刻度\n\
题材：都市玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角献祭所有异能记忆，换取城市秩序回归常态。\n\
主角弧线：主角从渴望掌控超凡力量，转为理解力量代价并主动放弃力量。\n\
世界观意象：名为“裂痕”的城市维度缝隙，通过消耗记忆来维持现实稳定。\n\
总主线因果链：感知异能，获得力量，发现记忆代价，最终选择献祭。\n\
命名理由：缄默代表主角失去记忆后的沉默，刻度代表能力消耗计量。\n\
角色权威表：姓名：秦澈，角色：主角，欲望：改变命运，恐惧：失去记忆，底线：不牺牲无辜。\n\
近期章节包：\n\
第01章《裂缝显现》：本章目标：秦澈发现裂痕。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        draft.title.clear();

        assert!(super::super::repair_creation_draft_title_metadata(
            &mut draft
        ));
        assert!(draft.title.is_empty());
        assert!(draft.fiction_title_rationale.is_empty());
        assert!(!draft
            .diagnostics
            .iter()
            .any(|note| note.contains("缄默刻度")));
        assert!(!draft
            .planning_notes
            .iter()
            .any(|note| note.contains("根据终局方向、大纲、世界观意象和主角弧线")));
    }

    #[test]
    fn natural_outline_with_old_protagonist_does_not_commit_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-outline-align",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "标准小说合同草案\n\
书名：裂痕献祭\n\
题材：都市玄幻\n\
总目标字数：50000\n\
每章目标档位：2500\n\
终局方向：主角献祭所有异能记忆，换取城市秩序回归常态。\n\
主角弧线：主角从渴望掌控超凡力量，转为理解力量代价并主动放弃力量。\n\
世界观意象：名为“裂痕”的城市维度缝隙，通过消耗记忆来维持现实稳定。\n\
总主线因果链：感知异能，获得力量，发现记忆代价，最终选择献祭。\n\
命名理由：书名《裂痕献祭》里的裂痕来自城市维度缝隙，献祭来自终局中放弃异能记忆的选择。\n\
角色权威表：姓名：秦澈，角色：主角，欲望：改变命运，恐惧：失去记忆，底线：不牺牲无辜。\n\
近期章节包：\n\
第01章《裂缝显现》：本章目标：陆沉在街角第一次感知到空间震颤。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(draft.fiction_characters.is_empty());
        assert!(!draft.fiction_outline.contains("陆沉"));
    }

    #[test]
    fn fiction_contract_does_not_block_default_protagonist_name_from_real_output() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言：zh-CN；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角公开灵网吞噬凡人气运的规则漏洞，让旧城区灵桥坍塌并重建公平晋级秩序。主角弧线：从只想自保的夜校生变成愿意承担代价的秩序修补者。世界观意象：夜校、灵网、旧城区灵桥、借灵证。总主线因果链：夜校生被迫借灵，发现灵网吞噬凡人气运，最终用灵桥旧证反转规则。书名《灵桥旧证》。命名理由：灵桥是终局坍塌并重建的地点，旧证是主角用来反转灵网规则的物件。\n\
3. 角色权威表：主角姓名：陆沉，命名依据：取沉入城市暗流又浮出真相之意，欲望：摆脱借灵身份，恐惧：被灵网抹去，底线：不牺牲同伴；对手姓名：祁执，命名依据：执守旧规则。\n\
4. 故事合同：核心矛盾是夜校草根与灵网规则垄断；结尾承诺是公开旧证并重建晋级秩序。\n\
5. 结构合同：一句话全书大纲：夜校生追查借灵证，最终以灵桥旧证推翻规则。\n\
6. 近期章节包：第01章《夜校借灵》：本章目标：主角获得借灵证并付出代价。\n\
7. 质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            !issues.iter().any(|issue| issue.contains("模型默认高频名")),
            "{issues:?}"
        );
    }

    #[test]
    fn fiction_contract_does_not_block_ungrounded_character_naming_rationale() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言：zh-CN；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角在旧城灵考终局公开校盟夺取草根灵籍的真相。主角弧线：从旁听生变成规则重写者。世界观意象：灵考、旧操场、借灵证、灵籍碑。总主线因果链：旁听生取得借灵证，发现校盟篡改灵籍，最终用灵籍碑反证规则。书名《旧操场灵证》。命名理由：旧操场是终局公开证据的地点，灵证是反转规则的物件。\n\
3. 角色权威表：主角姓名：沈砚，命名依据：象征主角成长和主题，欲望：通过灵考，恐惧：被抹去身份，底线：不牺牲同学；对手姓名：祁衡，命名依据：衡量旧规则的冷硬秩序。\n\
4. 故事合同：核心矛盾是草根学生与校盟垄断；结尾承诺是公开真相并改写晋级规则。\n\
5. 结构合同：一句话全书大纲：旁听生用旧操场灵证推翻校盟规则。\n\
6. 近期章节包：第01章《旁听入场》：本章目标：主角被迫进入灵考。\n\
第02章《借灵旧证》：本章目标：主角找到借灵证缺口。\n\
第03章《碑前证词》：本章目标：主角第一次听见灵籍碑证词。\n\
7. 质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("命名依据没有被合同")),
            "{issues:?}"
        );
    }

    #[test]
    fn fiction_contract_blocks_repetitive_chapter_title_templates() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言：zh-CN；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角在灵考终局公开校盟夺取草根灵籍的真相。主角弧线：从旁听生变成规则重写者。世界观意象：灵考、旧操场、借灵证、灵籍碑。总主线因果链：旁听生取得借灵证，发现校盟篡改灵籍，最终用灵籍碑反证规则。书名《旧操场灵证》。命名理由：旧操场是终局公开证据的地点，灵证是反转规则的物件。\n\
3. 角色权威表：主角姓名：许闻，命名依据：许来自被校盟拒绝承认的旁听身份，闻来自终局听见灵籍碑证词后公开真相，欲望：通过灵考，恐惧：被抹去身份，底线：不牺牲同学；对手姓名：祁衡，命名依据：衡来自校盟衡量灵籍的旧规则。\n\
4. 故事合同：核心矛盾是草根学生与校盟垄断；结尾承诺是公开真相并改写晋级规则。\n\
5. 结构合同：一句话全书大纲：旁听生用旧操场灵证推翻校盟规则。\n\
6. 近期章节包：第01章《命运的裂缝》：本章目标：许闻被迫进入灵考。\n\
第02章《灵证的回响》：本章目标：许闻找到借灵证缺口。\n\
第03章《旧桥的真相》：本章目标：许闻发现校盟篡改灵籍。\n\
7. 质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("章节标题句式过于单一")),
            "contract completion must not be blocked by chapter-title diversity advice: {issues:?}"
        );
        let advisory = super::super::generated_contract_advisory_issues(&draft, contract);

        assert!(
            advisory
                .iter()
                .any(|issue| issue.contains("章节标题句式过于单一")),
            "advisory={advisory:?}; count={}; titles={:?}",
            super::super::count_explicit_chapter_plan_lines(contract),
            super::super::collect_explicit_chapter_plan_titles(contract)
        );
    }

    #[test]
    fn fiction_contract_blocks_repetitive_numbered_chapter_package_items() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言：zh-CN；题材：都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角在灵考终局公开校盟夺取草根灵籍的真相。主角弧线：从旁听生变成规则重写者。世界观意象：灵考、旧操场、借灵证、灵籍碑。总主线因果链：旁听生取得借灵证，发现校盟篡改灵籍，最终用灵籍碑反证规则。书名《旧操场灵证》。命名理由：旧操场是终局公开证据的地点，灵证是反转规则的物件。\n\
3. 角色权威表：主角姓名：许闻，命名依据：许来自被校盟拒绝承认的旁听身份，闻来自终局听见灵籍碑证词后公开真相，欲望：通过灵考，恐惧：被抹去身份，底线：不牺牲同学。\n\
4. 故事合同：核心矛盾是草根学生与校盟垄断；结尾承诺是公开真相并改写晋级规则。\n\
5. 近期章节包：\n\
1. 命运的裂缝：本章目标：许闻被迫进入灵考。\n\
2. 灵证的回响：本章目标：许闻找到借灵证缺口。\n\
3. 旧桥的真相：本章目标：许闻发现校盟篡改灵籍。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("章节标题句式过于单一")),
            "contract completion must not be blocked by chapter-title diversity advice: {issues:?}"
        );
        let advisory = super::super::generated_contract_advisory_issues(&draft, contract);

        assert!(
            advisory
                .iter()
                .any(|issue| issue.contains("章节标题句式过于单一")),
            "advisory={advisory:?}; count={}; titles={:?}",
            super::super::count_explicit_chapter_plan_lines(contract),
            super::super::collect_explicit_chapter_plan_titles(contract)
        );
    }

    #[test]
    fn fiction_contract_accepts_multiline_character_naming_rationale() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角在灵籍碑前公开校盟夺取草根灵籍的证据。主角弧线：从旁听生变成规则重写者。世界观意象：灵考、旧操场、借灵证、灵籍碑。总主线因果链：旁听生取得借灵证，发现校盟篡改灵籍，最终用灵籍碑反证规则。书名《旧操场灵证》。命名理由：旧操场是终局公开证据的地点，灵证是反转规则的物件。\n\
3. 角色权威表：\n\
主角姓名：许闻\n\
命名依据：许来自校盟拒绝承认的旁听身份，闻来自终局听见灵籍碑证词后公开真相\n\
欲望：通过灵考；恐惧：被抹去身份；底线：不牺牲同学。\n\
4. 故事合同：核心矛盾是草根学生与校盟垄断；结尾承诺是公开真相并改写晋级规则。\n\
5. 结构合同：一句话全书大纲：旁听生用旧操场灵证推翻校盟规则。\n\
6. 近期章节包：第01章《旁听入场》：本章目标：主角被迫进入灵考。\n\
第02章《借灵旧证》：本章目标：主角找到借灵证缺口。\n\
第03章《碑前证词》：本章目标：主角第一次听见灵籍碑证词。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(
            !issues.iter().any(|issue| issue.contains("角色名 `许闻`")),
            "{issues:?}"
        );
    }

    #[test]
    fn fiction_contract_accepts_grounded_title_with_candidate_line() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20；导出格式=txt。\n\
2. 命名依据合同：终局方向：主角关闭城市灵频阈值，牺牲感知换回秩序。主角弧线：从追逐灵频力量的人变成守住城市静默边界的人。世界观意象：霓虹、灵频阈值、静默塔。总主线因果链：灵频觉醒导致过载，静默塔暴露真相，主角在终局关闭阈值。书名候选：灵频阈值 / 霓虹静默 / 静默塔下。书名《灵频阈值》。命名理由：灵频对应城市力量来源，阈值对应终局关闭静默塔前必须跨过的规则边界。\n\
3. 角色权威表：主角姓名：许阈，命名依据：阈来自终局关闭灵频阈值的选择，许来自他承诺守住静默塔边界，欲望：掌控灵频，恐惧：感知过载，底线：不牺牲无辜者；对手姓名：商弦，命名依据：弦来自其操纵城市频率的规则。\n\
4. 故事合同：核心矛盾是灵频力量与城市秩序；结尾承诺是主角关闭阈值保护城市。\n\
5. 结构合同：一句话全书大纲：主角追查灵频过载并在静默塔终局关闭阈值。\n\
6. 近期章节包：第01章《灵频初醒》：本章目标：主角第一次听见城市灵频。\n\
第02章《旧站失声》：本章目标：主角在废弃地铁站发现静默塔线索。\n\
第03章《阈值试验》：本章目标：主角为救人第一次触碰灵频边界。\n\
7. 质量合同：人物不漂移。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn creation_contract_prompt_requires_title_after_plot_basis() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");

        let prompt =
            super::super::final_prompt_from_creation_framework_request(&draft, "先定大纲和结局");

        assert!(prompt.contains("完整 typed batch"), "{prompt}");
        assert!(prompt.contains("title.rationale"), "{prompt}");
        assert!(
            prompt.contains("故事物件、地点、制度、事件、关系或终局变化"),
            "{prompt}"
        );
        assert!(prompt.contains("终局方向：主角终局行动与直接结果"), "{prompt}");
        assert!(prompt.contains("总主线因果链：连续因果短句"), "{prompt}");
        assert!(!prompt.contains("本段不要先定书名"), "{prompt}");
    }

    #[test]
    fn natural_contract_does_not_derive_missing_main_causal_spine() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20；导出格式=txt。\n\
2. 命名依据合同：终局方向：主角通过献祭记忆换取城市秩序的平衡，回归平凡。主角弧线：从追求超凡力量的人转变为守护规则的人。世界观意象：霓虹灯下的灵力潮汐，以记忆作为媒介的能量交换。书名《记忆偿付》。命名理由：偿付指代主角支付个人记忆来平衡城市能量的规则代价。\n\
3. 角色权威表：姓名：季栖声，角色：主角，欲望：掌握灵力，恐惧：失去自我，底线：不牺牲无辜者。\n\
6. 近期章节包：第01章《霓虹下的裂缝》：本章目标：季栖声初次感知灵力，并发现记忆碎片化消失。\n\
第02章《代价初现》：本章目标：通过一次能力释放，主角意识到能力的来源是记忆。\n\
第03章《监管者注视》：本章目标：关键人物出现，主角意识到自己已被纳入监管名单。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(draft.fiction_main_causal_spine.is_empty());
        let issues = super::super::creation_draft_contract_blocking_issues(&draft);
        assert!(
            issues.iter().any(|issue| issue.contains("总主线因果链")),
            "{issues:?}"
        );
    }

    #[test]
    fn natural_contract_does_not_commit_inline_naming_basis_and_character_table() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20；导出格式=txt。\n\
2. 命名依据合同：终局方向：主角在旧城灵桥终局公开灵网吞噬凡人气运的证据，让灵桥坍塌并重建公平晋级秩序。主角弧线：从只想保住夜校名额的旁听生，变成愿意承担代价的秩序修补者。世界观意象：夜校、灵网、旧城区灵桥、借灵证。总主线因果链：夜校生被迫借灵，发现灵网吞噬凡人气运，最终用灵桥旧证反转规则。书名《灵桥旧证》。命名理由：灵桥是终局坍塌并重建的地点，旧证是主角用来反转灵网规则的物件。\n\
3. 角色权威表：姓名：许闻桥，角色：主角，欲望：通过夜校灵考并保住身份，恐惧：被灵网再次抹去名额，底线：不牺牲同学换取晋级；姓名：商砚衡，角色：关键对手，欲望：维护灵网资源垄断，恐惧：旧证公开。\n\
4. 故事合同：核心矛盾是草根学生与灵网资源垄断；结尾承诺是公开真相并改写晋级规则。\n\
5. 结构合同：一句话全书大纲：旁听生用旧城区灵桥旧证推翻灵网垄断。第一卷《夜校借灵》：取得借灵证；第二卷《灵桥旧证》：追查证据；第三卷《桥塌之后》：重建规则。\n\
6. 近期章节包：第01章《夜校借灵》：本章目标：许闻桥被迫进入夜校灵考。\n\
第02章《桥下旧证》：本章目标：许闻桥发现借灵证缺口。\n\
第03章《灵网账簿》：本章目标：许闻桥第一次看见灵网吞噬气运的记录。\n\
7. 质量合同：人物不漂移。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));

        assert!(draft.title.is_empty());
        assert!(draft.fiction_ending_direction.is_empty());
        assert!(draft.fiction_protagonist_arc.is_empty());
        assert!(draft.fiction_world_imagery.is_empty());
        assert!(draft.fiction_main_causal_spine.is_empty());
        assert!(draft.fiction_title_rationale.is_empty());
        assert!(draft.fiction_characters.is_empty());
        assert!(draft.fiction_outline.is_empty());
    }

    #[test]
    fn explained_contract_title_is_not_blocked_by_aesthetic_scoring() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = "1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。\n\
2. 命名依据合同：终局方向：主角在静默塔关闭城市灵频阈值，牺牲感知换回秩序。主角弧线：从追逐灵频力量的人变成守住城市边界的人。世界观意象：霓虹、灵频阈值、静默塔。总主线因果链：灵频觉醒导致过载，静默塔暴露真相，主角在终局关闭阈值。书名《感知静默》。命名理由：感知对应主角能力，静默对应终局代价。\n\
3. 角色权威表：姓名：姜栖声，角色：主角，欲望：掌控异常灵频，恐惧：感知过载，底线：不牺牲无辜者。\n\
4. 故事合同：核心矛盾是灵频力量与城市秩序；结局承诺是主角关闭阈值。\n\
5. 结构合同：一句话全书大纲：主角追查灵频过载并在静默塔终局关闭阈值。\n\
6. 近期章节包：第01章《灵频初醒》：本章目标：主角第一次听见城市灵频。\n\
第02章《旧站失声》：本章目标：主角在废弃地铁站发现静默塔线索。\n\
第03章《阈值试验》：本章目标：主角为救人第一次触碰灵频边界。";

        let issues = super::super::generated_contract_completion_quality_issues(&draft, contract);
        assert!(issues.is_empty(), "{issues:?}");
        let gate = super::super::generated_contract_gate_result(&draft, contract, true);
        assert!(gate.is_ready(), "{:?}", gate.actionable_issues());
    }

    #[test]
    fn novel_content_crud_prompt_requires_read_then_revise_same_project() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市言情小说，每章3000字，写10万字。",
        )
        .expect("draft");
        draft.status = "approved".to_string();
        let prompt = super::super::final_prompt_from_novel_content_operation(
            &draft,
            &json!({"success": true, "init": {"project_path": "data/generated/novels/test-project"}}),
            "删掉第一章里关于旧档案的线索。",
            super::super::NovelContentOperation::Delete,
        );

        assert!(prompt.contains("project_path: data/generated/novels/test-project"));
        assert!(prompt.contains("不要新建小说项目"));
        assert!(prompt.contains("先 read_chapter，再 revise_chapter"));
        assert!(prompt.contains("第1章"));
    }

    #[test]
    fn approved_tool_draft_syncs_back_to_session_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "临时标题".to_string();
        draft.fiction_characters.clear();

        let approved = json!({
            "success": true,
            "project_path": "data/generated/novels/记忆熔断",
            "draft": {
                "title": "记忆熔断",
                "language": "zh-CN",
                "genre": "都市玄幻",
                "brief": "城市记忆流失危机。",
                "target_units": 50000,
                "chapter_unit_target": 2500,
                "export_format": "txt",
                "export_when_complete": true,
                "approved_only": true,
                "premise": "感知者调查城市记忆熔断。",
                "ending_direction": "主角切断超凡与城市记忆的连接。",
                "protagonist_arc": "从追逐力量到守护平庸秩序。",
                "world_imagery": "记忆提取仪、逻辑锁、静默街区。",
                "main_causal_spine": "力量越强，城市记忆流失越快。",
                "title_rationale": "记忆对应代价，熔断对应终局选择。",
                "themes": ["代价与平衡"],
                "characters": ["name: 辛岑宁; role: 主角", "name: 白知白; role: 重要角色"],
                "world_rules": ["每次施展异能都会消耗集体记忆。"],
                "style_rules": ["具体场景推进。"],
                "must_avoid": ["角色不漂移。"],
                "outline": "第01章《逻辑锁启动》：主角第一次感知代价。"
            }
        });

        assert!(super::super::sync_creation_draft_from_approval(
            &mut draft, &approved
        ));

        assert_eq!(draft.title, "记忆熔断");
        assert_eq!(draft.fiction_characters.len(), 2);
        assert!(draft.fiction_ending_direction.contains("切断超凡"));
        assert!(draft.fiction_outline.contains("逻辑锁启动"));
    }

    #[test]
    fn structured_contract_world_fields_fill_visible_world_rules() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");

        let approved = json!({
            "success": true,
            "project_path": "data/generated/novels/弦音捕手",
            "draft": {
                "title": "弦音捕手",
                "language": "zh-CN",
                "genre": "都市玄幻",
                "brief": "都市玄幻，每章2500字，至少5万字。",
                "target_units": 50000,
                "chapter_unit_target": 2500,
                "export_format": "txt",
                "export_when_complete": true,
                "approved_only": true,
                "premise": "主角调查城市脉动异常。",
                "ending_direction": "主角以感官代价稳住城市秩序。",
                "protagonist_arc": "从追逐力量到守护凡人秩序。",
                "world_imagery": "城市脉络中的律动纹章。",
                "main_causal_spine": "感知觉醒推动主角发现规则漏洞并走向终局选择。",
                "title_rationale": "弦音对应城市脉动，捕手对应主角最终承担守护职责。",
                "themes": ["代价与秩序"],
                "characters": ["name: 谢砚宁; role: 主角", "name: 裴闻声; role: 重要角色"],
                "world_rules": [],
                "style_rules": ["具体场景推进。"],
                "must_avoid": ["角色不漂移。"],
                "outline": "第01章《频率偏移》：本章目标：谢砚宁第一次感知城市异常。\n第02章《代价初显》：本章目标：谢砚宁发现能力代价。\n第03章《秩序裂痕》：本章目标：谢砚宁遇见裴闻声并确认规则漏洞。",
                "structured_contract_v2": {
                    "power_progression": {
                        "system_name": "律动纹章",
                        "levels": ["频率偏移", "代价初显", "秩序裂痕"]
                    },
                    "resource_economy": {
                        "value_scale": "感官越敏锐，代价越接近永久剥离"
                    },
                    "geography_model": {
                        "regions": ["城市脉络"]
                    },
                    "conflict_pressure_curve": {
                        "global_curve": [
                            {"range":"第一卷","pressure_level":"升压","function":"主角误以为能力可以解决校园声誉危机"}
                        ],
                        "release_strategy": "每次升压后用社团日常缓冲。",
                        "peak_policy": "卷尾必须用公开选择兑现压力。"
                    },
                    "reveal_schedule": [
                        {"secret":"异常频率来自校内旧广播室","reader_knows":"知道异常有源头","protagonist_knows":"只知道能力会失控","antagonist_knows":"知道广播室位置","reveal_window":"第一卷","status":"planned"}
                    ]
                }
            }
        });

        assert!(super::super::sync_creation_draft_from_approval(
            &mut draft, &approved
        ));

        assert!(
            draft.fiction_world_rules.is_empty(),
            "structured execution fields must not be silently copied into visible world_rules"
        );
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft)
                .iter()
                .any(|issue| issue.contains("世界规则")),
            "{:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );
    }

    #[test]
    fn natural_contract_cleanup_preserves_candidate_without_inventing_outline_names() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        let contract = r#"1. 基本参数：语言=zh-CN；题材=都市玄幻；总字数=50000；每章档位=2500；预计章节数=20。
2. 命名依据合同：终局方向：主角关闭霓虹裂痕并永久失去超常感知。主角弧线：从追逐力量到守护平凡秩序。世界观意象：城市高楼间的霓虹裂痕。总主线因果链：发现裂痕——误用力量——公开证据——关闭裂痕。
书名：霓虹代价
书名候选：霓虹代价（裂痕与终局感知代价）；失焦街灯（开篇事件与关键物件）；裂痕观测簿（调查制度与关键证据）
书名理由：霓虹对应城市裂痕，代价对应永久失去超常感知的终局选择。
3. 角色权威表：姓名：陆沉，角色：主角，欲望：掌控裂痕力量，恐惧：失去感知，底线：绝不牺牲无辜市民，弧线起点：追逐力量，弧线终点：守护平凡秩序。
姓名：顾听澜，角色：关键关系对象，欲望：保全裂痕调查记录，恐惧：证据被毁，底线：必须守住原始观测档案，弧线起点：独自调查，弧线终点：公开证据。
姓名：沈薇，角色：关键对手，欲望：通过裂痕获取永生，恐惧：平庸死亡，底线：绝不交出裂痕控制权，弧线起点：暗中操控裂痕，弧线终点：失去控制权。
4. 世界规则：裂痕力量每次使用都会永久削弱一种感官；只有完整观测记录能定位控制室；关闭裂痕会永久剥夺操作者的超常感知。
5. 全书大纲：主角调查裂痕并在终局公开证据、关闭城市裂隙。
第1卷《裂痕追踪》：本卷目标：取得裂痕异常证据；卷尾变化：确认裂痕由人为扩大。
第2卷《霓虹封印》：本卷目标：公开证据并封印裂痕；卷尾变化：陆沉关闭霓虹裂痕并永久失去超常感知。
6. 近期章节包：第01章《失焦的街灯》：本章目标：陆沉第一次触碰裂痕导致视觉模糊；预期转折：模糊视野中出现人为刻写的坐标。
第02章《噪音诱捕》：本章目标：陆沉按坐标追查并遭遇沈薇试探；预期转折：顾听澜取得被删改的观测记录。
第03章《代价清单》：本章目标：陆沉核对记录并确认力量与感官等价交换；预期转折：记录指向裂痕控制室并留下追查债务。
第04章《沈辞的馈赠》：本章目标：对手出现并展示通过记忆构建的幻象；预期转折：幻象暴露控制室入口。
7. 核心主题：力量选择必须承担感官代价；叙事风格：用城市行动场景和人物选择推进；必须避免：角色无解释改名；跳过力量代价。"#;

        let outcome =
            super::super::submit_generated_contract_candidate_to_draft(&mut draft, contract);
        assert!(!outcome.is_ready());
        assert!(outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("缺少可执行的结构化治理内容")));
        let draft = super::super::creation_draft_with_pending_contract_applied(&draft);

        assert!(!draft.fiction_outline.is_empty());
        assert!(!draft.fiction_characters.is_empty());
        assert!(draft
            .fiction_characters
            .iter()
            .all(|line| !line.contains("沈辞")));
        assert!(
            draft
                .fiction_must_avoid
                .iter()
                .all(|item| !item.contains("沈薇") && !item.contains("欲望")),
            "{:?}",
            draft.fiction_must_avoid
        );
        assert!(
            draft
                .fiction_world_rules
                .iter()
                .all(|item| !item.contains("书名")),
            "{:?}",
            draft.fiction_world_rules
        );
    }

    #[test]
    fn placeholder_name_check_ignores_placeholder_words_in_character_story_fields() {
        let governed = "name: 南屿真; role: 对手; desire: 隐瞒旧案; fear: 村落被开发; bottom_line: 不让外人看到遗址中那具未命名的尸体; name_source: generated_by_writing_tool_policy";
        let placeholder = "name: 待命名; role: 对手; desire: 隐瞒旧案; fear: 村落被开发; bottom_line: 不销毁原始证据";

        assert!(!super::super::fiction_character_line_has_placeholder_name(
            governed
        ));
        assert!(super::super::fiction_character_line_has_placeholder_name(
            placeholder
        ));
    }
