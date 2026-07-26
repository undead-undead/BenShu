    use serde_json::json;

    use crate::tool::writing::novel_contract_v2::PressureBeat;

    struct MockCreationDraftRuntime {
        draft: Option<super::super::SessionCreationDraftState>,
        recovered_draft: Option<super::super::SessionCreationDraftState>,
        continuation_project_path: Option<String>,
        project_path: String,
        saved: usize,
        approved: usize,
    }

    fn mock_approved_draft_payload(
        draft: &super::super::SessionCreationDraftState,
    ) -> serde_json::Value {
        json!({
            "title": draft.title,
            "language": draft.language,
            "genre": draft.genre,
            "brief": draft.brief,
            "target_units": draft.target_units,
            "chapter_unit_target": draft.chapter_unit_target,
            "max_chapters_per_turn": draft.max_chapters_per_turn,
            "export_format": draft.export_format,
            "export_when_complete": draft.export_when_complete,
            "approved_only": draft.approved_only,
            "premise": draft.fiction_premise,
            "ending_direction": draft.fiction_ending_direction,
            "protagonist_arc": draft.fiction_protagonist_arc,
            "world_imagery": draft.fiction_world_imagery,
            "main_causal_spine": draft.fiction_main_causal_spine,
            "title_rationale": draft.fiction_title_rationale,
            "themes": draft.fiction_themes,
            "characters": draft.fiction_characters,
            "world_rules": draft.fiction_world_rules,
            "style_rules": draft.fiction_style_rules,
            "must_avoid": draft.fiction_must_avoid,
            "outline": draft.fiction_outline,
            "structured_contract_v2": draft.contract_v2(),
        })
    }

    #[async_trait::async_trait]
    impl super::super::CreationDraftRuntime for MockCreationDraftRuntime {
        async fn load_draft(
            &mut self,
            _session_id: &str,
        ) -> anyhow::Result<Option<super::super::SessionCreationDraftState>> {
            Ok(self.draft.clone())
        }

        async fn save_draft(
            &mut self,
            draft: &super::super::SessionCreationDraftState,
        ) -> anyhow::Result<()> {
            self.saved += 1;
            self.draft = Some(draft.clone());
            Ok(())
        }

        async fn clear_draft(&mut self, _session_id: &str) -> anyhow::Result<()> {
            self.draft = None;
            Ok(())
        }

        async fn create_draft(
            &mut self,
            draft: &mut super::super::SessionCreationDraftState,
        ) -> anyhow::Result<()> {
            self.draft = Some(draft.clone());
            Ok(())
        }

        async fn update_draft(
            &mut self,
            draft: &super::super::SessionCreationDraftState,
        ) -> anyhow::Result<()> {
            self.draft = Some(draft.clone());
            Ok(())
        }

        async fn approve_draft(
            &mut self,
            draft: &super::super::SessionCreationDraftState,
        ) -> anyhow::Result<serde_json::Value> {
            self.approved += 1;
            self.draft = Some(draft.clone());
            Ok(json!({
                "success": true,
                "init": {"project_path": self.project_path.clone()},
                "draft": mock_approved_draft_payload(draft),
            }))
        }

        async fn approved_draft_for_existing_project(
            &mut self,
            _session_id: &str,
            draft: &mut super::super::SessionCreationDraftState,
        ) -> anyhow::Result<serde_json::Value> {
            self.approved += 1;
            let authority = self.recovered_draft.as_ref().unwrap_or(draft).clone();
            self.draft = Some(authority.clone());
            Ok(json!({
                "success": true,
                "project_path": self.project_path.clone(),
                "draft": mock_approved_draft_payload(&authority),
            }))
        }

        async fn discard_draft(
            &mut self,
            _draft: &super::super::SessionCreationDraftState,
        ) -> anyhow::Result<()> {
            self.draft = None;
            Ok(())
        }

        async fn existing_project_path(
            &mut self,
            _session_id: &str,
            draft: &super::super::SessionCreationDraftState,
        ) -> anyhow::Result<Option<String>> {
            let project_path = draft.project_path.trim();
            if !project_path.is_empty() {
                return Ok(Some(project_path.to_string()));
            }
            Ok(None)
        }

        async fn existing_project_path_for_continuation_message(
            &mut self,
            _session_id: &str,
            _message: &str,
        ) -> anyhow::Result<Option<String>> {
            Ok(self.continuation_project_path.clone())
        }

        async fn existing_project_artifact_kind(
            &mut self,
            _project_path: &str,
        ) -> anyhow::Result<String> {
            Ok("fiction".to_string())
        }
    }

    fn fill_complete_fiction_contract_v2(draft: &mut super::super::SessionCreationDraftState) {
        if draft.fiction_themes.is_empty() {
            draft.fiction_themes = vec!["选择必须承担真实代价".to_string()];
        }
        if draft.fiction_style_rules.is_empty() {
            draft.fiction_style_rules = vec!["用具体行动、场景和对话推进冲突".to_string()];
        }
        if draft.fiction_must_avoid.is_empty() {
            draft.fiction_must_avoid = vec!["不要让角色无解释改名或跳过关键因果".to_string()];
        }
        let characters = draft
            .fiction_characters
            .iter()
            .map(|line| super::super::draft_character_line_to_contract(line))
            .filter(|character| !character.canonical_name.trim().is_empty())
            .collect::<Vec<_>>();
        let primary = characters
            .iter()
            .find(|character| character.role_looks_primary())
            .or_else(|| characters.first())
            .map(|character| character.canonical_name.clone())
            .unwrap_or_else(|| "主角".to_string());
        let secondary = characters
            .iter()
            .find(|character| character.canonical_name != primary)
            .map(|character| character.canonical_name.clone())
            .unwrap_or_else(|| primary.clone());

        draft.emotional_contract.emotional_promise = "守住城市与自我选择的情感承诺".to_string();
        draft.emotional_contract.emotional_beats = vec![
            "初见异常".to_string(),
            "承担代价".to_string(),
            "终局守护".to_string(),
        ];
        draft.relationship_ledger = vec![super::super::RelationshipLedgerEntry {
            characters: vec![primary.clone(), secondary.clone()],
            relationship_type: "对抗".to_string(),
            current_state: "互相试探".to_string(),
            desired_end_state: "冲突得到兑现".to_string(),
            ..Default::default()
        }];
        draft.payoff_matrix = vec![super::super::PayoffMatrixEntry {
            promise: "雨巷灵火的代价必须在终局兑现".to_string(),
            payoff_target: "终局".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];
        draft.narration_contract.pov = "第三人称有限视角".to_string();
        draft.narration_contract.dialogue_style = "对白推动关系和信息变化".to_string();
        draft.narration_contract.narrative_distance = "贴近主角选择压力".to_string();
        draft.narration_contract.chapter_pacing = "每章有明确冲突、选择和结尾钩子".to_string();
        draft.antagonist_pressure.primary_pressure = "对手试图利用裂缝改写城市秩序".to_string();
        draft.power_progression.system_name = "灵契试炼".to_string();
        draft.power_progression.levels = vec!["感知".to_string(), "立约".to_string()];
        draft.time_model.story_start_time = "雨季开端".to_string();
        draft.scene_type_mix.action = "用行动推进夜校试炼和地下灵轨追查".to_string();
        draft.scene_type_mix.dialogue = "用对话暴露借灵证黑幕和人物立场".to_string();
        draft.scene_type_mix.balance_rule =
            "行动、对话、信息揭示和情感落点轮换出现".to_string();
        draft.character_voice_ledger = vec![super::super::CharacterVoiceProfile {
            character: primary.clone(),
            voice_style: "克制观察，关键处直接反击".to_string(),
            dialogue_rules: vec!["每次对白都推进选择或证据".to_string()],
            ..Default::default()
        }];
        draft.reader_promise.core_hook =
            "读者追看普通学生如何揭开夜校借灵证黑幕并改写晋级规则".to_string();
        draft.reader_promise.pleasure_points =
            vec!["底层学生用证据反击制度垄断".to_string()];
        draft.chapter_ending_rotation.planned_rotation =
            vec!["悬念入口".to_string(), "情绪落点".to_string(), "信息反转".to_string()];
        draft.chapter_ending_rotation.avoid_repetition_rule =
            "连续章节不能使用同一种章尾形态".to_string();
        draft.conflict_pressure_curve.global_curve =
            vec![PressureBeat {
                range: "开局到第一卷前段".to_string(),
                pressure_level: "低到中".to_string(),
                function: "确认夜校借灵证异常并建立追查压力".to_string(),
            }];
        draft.motif_ledger = vec![super::super::MotifLedgerEntry {
            motif: "地下灵轨".to_string(),
            meaning: "晋级制度背后的代价".to_string(),
            payoff_target: "终局切断灵轨并公开制度".to_string(),
            ..Default::default()
        }];
        draft.reveal_schedule = vec![super::super::RevealScheduleEntry {
            secret: "借灵证会转移学生运势".to_string(),
            reveal_window: "第一卷中段".to_string(),
            status: "planned".to_string(),
            ..Default::default()
        }];
        draft.relationship_interaction_quotas =
            vec![super::super::RelationshipInteractionQuota {
                characters: vec![primary.clone(), secondary],
                relationship: "对抗".to_string(),
                cadence: "每2-3章推进一次".to_string(),
                required_interaction: "互动必须改变证据、信任或冲突压力".to_string(),
                next_due: "第一卷前半段".to_string(),
            }];
        let mut contract = super::super::strong_novel_contract_from_visible_creation_draft(draft);
        if contract.world_rules.is_empty() {
            contract.world_rules = draft.fiction_world_rules.clone();
        }
        for chapter in &mut contract.outline.near_chapters {
            if chapter.goal.trim() == chapter.expected_turn.trim() {
                let number = chapter.number.unwrap_or(1);
                chapter.expected_turn =
                    format!("第{number}章目标完成后，{primary}失去退回上一阶段的选择");
            }
        }
        let authority_names = contract
            .characters
            .iter()
            .map(|character| character.canonical_name.clone())
            .collect::<Vec<_>>();
        for relation in &mut contract.structured.relationship_ledger {
            if relation.characters.len() <= authority_names.len() {
                relation.characters = authority_names[..relation.characters.len()].to_vec();
            }
        }
        contract.normalize();
        draft.current_contract = Some(serde_json::to_value(contract).expect("contract json"));
    }

    fn ready_fiction_draft(
        session_id: &str,
    ) -> super::super::SessionCreationDraftState {
        let mut draft = super::super::build_initial_creation_draft(
            session_id,
            "fiction",
            "都市玄幻小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.title = "夜校灵轨".to_string();
        draft.fiction_premise = "旧城夜校的补考会通过地下灵轨吞噬普通学生运势。".to_string();
        draft.fiction_ending_direction = "许闻切断夜校灵轨并公开借灵制度。".to_string();
        draft.fiction_protagonist_arc =
            "许闻从旁观者成长为守住普通学生的规则改写者。".to_string();
        draft.fiction_world_imagery = "旧城夜校、借灵证、地下灵轨。".to_string();
        draft.fiction_main_causal_spine =
            "许闻在夜校试炼中发现借灵证黑幕，最终切断地下灵轨并公开制度。"
                .to_string();
        draft.fiction_title_rationale =
            "夜校是故事入口，灵轨是考试制度背后的核心规则，终局由许闻亲手切断灵轨。"
                .to_string();
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 守住城市; fear: 失去选择权; bottom_line: 不牺牲无辜者; arc_start: 旁观者; arc_end: 规则改写者".to_string(),
            "name: 梁棠; role: 关键对手; desire: 利用裂缝重排夜校名额; fear: 借灵证账册公开; bottom_line: 不让地下灵轨脱离自己掌控; arc_start: 监考者; arc_end: 被证据逼到台前".to_string(),
        ];
        draft.fiction_themes = vec!["选择权比力量更重要".to_string()];
        draft.fiction_world_rules = vec!["地下灵轨会记录并抽取考生运势".to_string()];
        draft.fiction_style_rules = vec!["用具体场景推进，不写提纲式正文".to_string()];
        draft.fiction_must_avoid = vec!["不要让角色无解释改名".to_string()];
        draft.fiction_outline = "第一卷《雨夜入校》：许闻进入夜校并确认借灵证异常；卷尾变化：许闻成为灵轨见证者。\n第01章《雨夜补考》：本章目标：许闻第一次听见地下灵轨启动。\n第02章《借灵证》：本章目标：许闻发现借灵证会记录运势损耗。".to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.refresh_contract_status_from_validation();
        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::ContractReady,
            "{:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );
        draft
    }

    #[tokio::test]
    async fn thin_fiction_opening_only_returns_intake_response() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "帮我写小说")
                .await
                .expect("handled")
                .expect("outcome");
        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("thin fiction opening should not start background contract generation");
        };

        assert_eq!(response.chat_route, "coordinator::creation_intake");
        assert_eq!(response.tool_surface_mode, "fiction");
        assert!(response.response.contains("你来定"));
        assert!(runtime.draft.is_none());
        assert_eq!(runtime.saved, 0);
    }

    #[tokio::test]
    async fn adult_fiction_opening_requires_age_confirmation_before_draft() {
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
            "帮我写一部都市色情小说，每章2500字，5万字",
        )
        .await
        .expect("handled")
        .expect("outcome");
        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("adult fiction opening should ask for age confirmation before drafting");
        };

        assert!(response.response.contains("年满十八周岁"));
        assert_eq!(response.chat_route, "coordinator::creation_intake");
        assert!(runtime.draft.is_none());
        assert_eq!(runtime.saved, 0);
    }

    #[tokio::test]
    async fn adult_fiction_modification_requires_age_confirmation_without_mutating_draft() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "帮我写一部都市言情小说，每章2500字，5万字",
        )
        .expect("draft");
        let original_brief = draft.brief.clone();
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
            "把题材改成色情暴力血腥小说",
        )
        .await
        .expect("handled")
        .expect("outcome");
        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("adult fiction modification should ask for age confirmation");
        };

        assert!(response.response.contains("年满十八周岁"));
        let stored = runtime.draft.expect("draft remains");
        assert_eq!(stored.brief, original_brief);
        assert_eq!(runtime.saved, 0);
    }

    #[test]
    fn creation_draft_maps_old_active_status_without_rewriting_it() {
        let mut draft =
            super::super::build_initial_creation_draft("session-a", "fiction", "帮我写小说")
                .expect("draft");

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );

        draft.status = "active".to_string();

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
    }

    #[test]
    fn contract_ready_draft_is_not_open_to_background_candidate_commit() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.title = "夜校灵轨".to_string();
        draft.fiction_ending_direction = "主角切断夜校灵轨并公开借灵制度。".to_string();
        draft.fiction_protagonist_arc = "从旁观者成长为守住普通学生的规则改写者。".to_string();
        draft.fiction_world_imagery = "旧城夜校、借灵证、地下灵轨。".to_string();
        draft.fiction_main_causal_spine =
            "夜校试炼引出借灵证黑幕，终局由主角切断地下灵轨并公开制度。"
                .to_string();
        draft.fiction_title_rationale =
            "夜校是故事起点，灵轨是考试制度背后的核心规则，结局是主角切断灵轨。"
                .to_string();
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 守住城市; fear: 失去选择权; bottom_line: 不牺牲无辜者".to_string(),
            "name: 梁棠; role: 关键对手; desire: 利用裂缝重排夜校名额; fear: 借灵证账册公开; bottom_line: 不让地下灵轨脱离自己掌控".to_string(),
        ];
        draft.fiction_premise = "旧城夜校的补考会通过地下灵轨吞噬普通学生运势。".to_string();
        draft.fiction_themes = vec!["选择权比力量更重要".to_string()];
        draft.fiction_world_rules = vec!["地下灵轨会记录并抽取考生运势".to_string()];
        draft.fiction_style_rules = vec!["用具体场景推进，不写提纲式正文".to_string()];
        draft.fiction_must_avoid = vec!["不要让角色无解释改名".to_string()];
        draft.fiction_outline =
            "第一卷《雨夜入校》：主角进入夜校并确认借灵证异常；卷尾变化：主角成为灵轨见证者。\n第01章《雨夜补考》：本章目标：主角第一次听见地下灵轨启动。\n第02章《借灵证》：本章目标：主角发现借灵证会记录运势损耗。"
                .to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.refresh_contract_status_from_validation();
        let readiness_issues = super::super::creation_draft_contract_blocking_issues(&draft);

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::ContractReady,
            "{readiness_issues:?}"
        );
        let status = crate::tool::writing::session_surface::creation_contract_status_for_draft(
            Some(&draft),
            None,
        )
        .expect("ready contract task status");
        assert!(matches!(status, benshu_state::TaskStatus::Completed));
        assert!(!draft.can_accept_contract_candidate());

        super::super::apply_message_to_creation_draft(&mut draft, "把主角改得更热血一点");
        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
        assert!(draft.can_accept_contract_candidate());
    }

    #[test]
    fn contract_ready_draft_reopens_for_explicit_quality_repair() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-ready-quality-review",
            "fiction",
            "写一部现实悬疑小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::ContractReady);

        let message = "请对当前创作合同做质量自检并自动修复漂移，不要写正文。";
        assert!(super::super::creation_draft_modification_requested(message));

        super::super::apply_message_to_creation_draft(&mut draft, message);

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
        assert!(draft.can_accept_contract_candidate());
    }

    #[test]
    fn approved_contract_rejects_machine_patch_without_mutating_confirmed_fields() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-approved-contract-lock",
            "fiction",
            "写一部都市小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        draft.fiction_premise = "审计员发现旧城资源账本被系统性篡改。".to_string();
        let confirmed = super::super::strong_novel_contract_from_creation_draft(&draft);
        draft.current_contract =
            Some(serde_json::to_value(&confirmed).expect("confirmed contract"));
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::Approved);
        let before = serde_json::to_value(&draft).expect("draft snapshot");

        let outcome = super::super::submit_generated_contract_candidate_to_draft(
            &mut draft,
            r#"{"patch_type":"skeleton_patch","premise":"机器擅自把故事改成校园案件"}"#,
        );

        assert!(!outcome.committed);
        assert_eq!(serde_json::to_value(&draft).expect("draft snapshot"), before);
    }

    #[test]
    fn contract_revision_promotes_all_story_facts_into_existing_user_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-ready-authority-revision",
            "fiction",
            "写一部历史权谋小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::ContractReady);

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "修复合同，不要写正文。钟星岚统一为女性寒门官员，不称士子或书生；唐晏白第一章的身份与第五章继位节点统一；终局和标题理由统一为主动挂印辞官归隐，不是流放；总因果链以朝局稳定并主动归隐这一事件结果收束。",
        );

        for expected in [
            "用户故事核心权威：钟星岚统一为女性寒门官员，不称士子或书生",
            "用户故事核心权威：唐晏白第一章的身份与第五章继位节点统一",
            "用户故事核心权威：终局和标题理由统一为主动挂印辞官归隐，不是流放",
            "用户故事核心权威：总因果链以朝局稳定并主动归隐这一事件结果收束",
        ] {
            assert!(
                draft.planning_notes.iter().any(|note| note == expected),
                "missing authority: {expected:?}; notes={:?}",
                draft.planning_notes
            );
        }
        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
    }

    #[test]
    fn locked_contract_revision_stays_pending_until_matching_typed_patch_scope_applies() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-pending-explicit-outline-revision",
            "fiction",
            "写一部历史权谋小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        let locked_contract =
            super::super::strong_novel_contract_from_creation_draft(&draft);
        draft.current_contract =
            Some(serde_json::to_value(locked_contract).expect("locked contract value"));
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::ContractReady);

        let revision =
            "修改合同：把近期规划第5章写清楚为具名皇子继位，不要只写幼主即位。";
        assert!(draft.current_contract.is_some());
        assert!(super::super::creation_draft_modification_requested(revision));
        assert!(!super::super::fiction_concept_replacement_requested(revision));
        super::super::apply_message_to_creation_draft(&mut draft, revision);
        super::super::apply_message_to_creation_draft(
            &mut draft,
            "修订合同：第1章必须明确同一人物当时仍是皇子。",
        );

        let issue = super::super::pending_explicit_contract_revision_issue(&draft)
            .unwrap_or_else(|| panic!("pending explicit revision; notes={:?}", draft.planning_notes));
        assert!(issue.contains("近期规划第5章"));
        assert!(issue.contains("第1章必须明确"));
        let findings =
            super::super::pending_explicit_contract_revision_findings(&draft);
        assert!(findings.iter().all(|finding| {
            finding.kind
                == crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot
                && finding.code == "contract.explicit_revision"
                && finding.evidence.field == "outline"
        }));
        assert!(
            super::super::stable_creation_planning_notes(&draft)
                .iter()
                .all(|note| !note.starts_with("待应用合同字段修订："))
        );

        super::super::clear_applied_explicit_contract_revisions(
            &mut draft,
            super::super::CreationContractPatchType::Characters,
        );
        assert!(super::super::pending_explicit_contract_revision_issue(&draft).is_some());

        super::super::clear_applied_explicit_contract_revisions(
            &mut draft,
            super::super::CreationContractPatchType::Plot,
        );
        assert!(super::super::pending_explicit_contract_revision_issue(&draft).is_none());
    }

    #[test]
    fn rejected_typed_patch_preserves_explicit_revision_for_the_next_repair() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-persist-applied-explicit-revision",
            "fiction",
            "写一部历史权谋小说，每章2500字，总字数10万字。",
        )
        .expect("draft");
        let mut locked_contract =
            super::super::strong_novel_contract_from_creation_draft(&draft);
        locked_contract.outline.near_chapters =
            vec![super::super::ChapterSeedContract {
                number: Some(1),
                goal: "主角以尚未继位的皇子身份微服查案".to_string(),
                expected_turn: "皇子确认朝局暗线".to_string(),
            }];
        draft.current_contract =
            Some(serde_json::to_value(locked_contract).expect("locked contract value"));
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::ContractReady);
        super::super::apply_message_to_creation_draft(
            &mut draft,
            "修订合同：第1章明确主角仍是尚未继位的皇子。",
        );
        assert!(super::super::pending_explicit_contract_revision_issue(&draft).is_some());

        let outcome = super::super::submit_generated_contract_candidate_to_draft(
            &mut draft,
            r#"{
                "patch_type":"plot_patch",
                "outline":{"near_chapters":[
                    {"number":1,"goal":"主角以尚未继位的皇子身份微服查案","expected_turn":"皇子确认朝局暗线"}
                ]}
            }"#,
        );

        assert!(!outcome.is_ready());
        assert!(
            super::super::pending_explicit_contract_revision_issue(&draft).is_some(),
            "a rejected Plot patch must not discard the user's pending revision"
        );
        assert!(draft.pending_contract_candidate.is_some());
    }

    #[test]
    fn polluted_contract_outline_never_becomes_ready() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "都市玄幻小说，每章2500字，写5万字。",
        )
        .expect("draft");
        draft.title = "雨巷灵契".to_string();
        draft.fiction_premise = "旧城雨巷出现灵能裂缝，普通学生被迫卷入守城试炼。".to_string();
        draft.fiction_ending_direction = "主角守住城市灵能裂缝并回到平凡生活。".to_string();
        draft.fiction_protagonist_arc = "从旁观者成长为守城者。".to_string();
        draft.fiction_world_imagery = "雨巷灵火、旧楼夜校、玻璃天台裂缝。".to_string();
        draft.fiction_main_causal_spine =
            "城市异常引出夜校试炼，终局由主角重立城市灵契。".to_string();
        draft.fiction_title_rationale = "灵契来自主角终局与城市重新立约的选择。".to_string();
        draft.fiction_characters = vec![
            "name: 许闻; role: 主角; desire: 守住城市; fear: 失去选择权; bottom_line: 不牺牲无辜者".to_string(),
            "name: 梁棠; role: 关键对手; desire: 利用裂缝重排夜校名额; fear: 借灵证账册公开; bottom_line: 不让地下灵轨脱离自己掌控".to_string(),
        ];
        draft.fiction_themes = vec!["选择权比力量更重要".to_string()];
        draft.fiction_world_rules = vec!["灵契只能通过承担真实代价建立".to_string()];
        draft.fiction_style_rules = vec!["用具体场景推进，不写提纲式正文".to_string()];
        draft.fiction_must_avoid = vec!["不要让角色无解释改名".to_string()];
        draft.fiction_outline =
            "第一卷：裂缝觉醒。\n第01章《雨巷灵火》：本章目标：主角发现城市异常。\n命名理由：灵契来自终局选择。\n章节审稿/修订：每章检查伏笔。"
                .to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.refresh_contract_status_from_validation();

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft)
                .iter()
                .any(|issue| issue.contains("工作流说明"))
        );
    }

    #[test]
    fn story_grounded_summary_title_is_not_a_hard_blocker() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-summary-title",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "主角突破自身极限".to_string();
        draft.fiction_premise = "旧城区夜校用灵能考试筛掉底层学生。".to_string();
        draft.fiction_ending_direction = "主角公开借灵证账册，改写夜校晋级规则。".to_string();
        draft.fiction_protagonist_arc =
            "从只想保住补考名额的旁听生成长为规则改写者。".to_string();
        draft.fiction_world_imagery = "雨夜夜校、地下灵轨、借灵证、旧城站台。".to_string();
        draft.fiction_main_causal_spine =
            "补考异常引出借灵证，追查地下灵轨账册，终局公开证据改写规则。"
                .to_string();
        draft.fiction_title_rationale =
            "突破自身极限对应主角终局公开证据并改写规则的成长。".to_string();
        draft.fiction_characters = vec![
            "name: 秦知安; role: 主角; desire: 通过夜校考试改变生活; fear: 再次被城市边缘化; bottom_line: 不牺牲无辜同学换取晋级; arc_start: 旁听生; arc_end: 规则改写者".to_string(),
            "name: 梁棠; role: 关键对手; desire: 垄断夜校晋级名额; fear: 考试黑幕公开; bottom_line: 不让底层学生越过自己; arc_start: 监考者; arc_end: 被证据逼到台前".to_string(),
        ];
        draft.fiction_themes = vec!["公平晋级".to_string(), "代价与选择".to_string()];
        draft.fiction_world_rules = vec![
            "借灵证能临时借用灵脉但会记录并抽取考生运势。".to_string(),
            "地下灵轨只能由承担代价的人接通。".to_string(),
        ];
        draft.fiction_style_rules = vec!["具体场景推进。".to_string()];
        draft.fiction_must_avoid = vec!["不要角色无解释改名。".to_string()];
        draft.fiction_outline =
            "第一卷《雨夜入校》：主角进入夜校并确认借灵证异常；卷尾变化：主角成为灵轨见证者。\n第1章 本章目标：秦知安在雨夜补考中第一次听见地下灵轨启动；预期转折：他确认补考会吞噬运势。"
                .to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.refresh_contract_status_from_validation();

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::ContractReady
        );
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft).is_empty(),
            "{:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );
    }

    #[test]
    fn nested_volume_outline_noise_never_becomes_ready() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-nested-volume-outline",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "夜校借灵证".to_string();
        draft.fiction_title_rationale =
            "夜校是故事起点，借灵证是考试黑幕的关键物件，结局公开借灵证账册。"
                .to_string();
        draft.fiction_premise = "旧城区夜校用灵能考试筛掉底层学生。".to_string();
        draft.fiction_ending_direction = "主角公开借灵证账册，改写夜校晋级规则。".to_string();
        draft.fiction_protagonist_arc =
            "从只想保住补考名额的旁听生成长为规则改写者。".to_string();
        draft.fiction_world_imagery = "雨夜夜校、地下灵轨、借灵证、旧城站台。".to_string();
        draft.fiction_main_causal_spine =
            "补考异常引出借灵证，追查地下灵轨账册，终局公开证据改写规则。"
                .to_string();
        draft.fiction_characters = vec![
            "name: 秦知安; role: 主角; desire: 通过夜校考试改变生活; fear: 再次被城市边缘化; bottom_line: 不牺牲无辜同学换取晋级; arc_start: 旁听生; arc_end: 规则改写者".to_string(),
            "name: 梁棠; role: 关键对手; desire: 垄断夜校晋级名额; fear: 考试黑幕公开; bottom_line: 不让底层学生越过自己; arc_start: 监考者; arc_end: 被证据逼到台前".to_string(),
        ];
        draft.fiction_themes = vec!["公平晋级".to_string(), "代价与选择".to_string()];
        draft.fiction_world_rules = vec![
            "借灵证能临时借用灵脉但会记录并抽取考生运势。".to_string(),
            "地下灵轨只能由承担代价的人接通。".to_string(),
        ];
        draft.fiction_style_rules = vec!["具体场景推进。".to_string()];
        draft.fiction_must_avoid = vec!["不要角色无解释改名。".to_string()];
        draft.fiction_outline =
            "第4卷《第4卷《》：第4卷《；卷尾变化：第4卷《\n第5卷《第10章 本章目标》：本章目标：第10章 本章目标；预期转折：第10章 本章目标"
                .to_string();
        fill_complete_fiction_contract_v2(&mut draft);
        draft.refresh_contract_status_from_validation();

        assert_eq!(
            draft.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft)
                .iter()
                .any(|issue| issue.contains("结构污染")),
            "{:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );
    }

    #[tokio::test]
    async fn start_request_with_incomplete_contract_is_blocked_without_repair_prompt() {
        let draft =
            super::super::build_initial_creation_draft("session-a", "fiction", "帮我写小说")
                .expect("draft");
        let mut runtime = MockCreationDraftRuntime {
            draft: Some(draft),
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "开始写第一章")
                .await
                .expect("handled")
                .expect("outcome");
        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("incomplete contract should be blocked in chat, not converted into repair");
        };

        assert!(response.response.contains("当前写作合同还不能进入正文写作"));
        assert!(response.response.contains("需要补齐"));
        assert!(!response
            .response
            .contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert!(!response
            .response
            .contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        assert_eq!(runtime.approved, 0);
        let saved = runtime.draft.expect("saved draft");
        assert_eq!(
            saved.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::DraftingContract
        );
    }

    #[tokio::test]
    async fn approved_existing_project_continuation_routes_to_writer_not_contract_display() {
        let mut draft = ready_fiction_draft("session-a");
        draft.project_path = "data/generated/novels/test-project".to_string();
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::Approved);
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
            "请继续把当前这部小说写完整。继续写之前先检查当前小说项目状态，如果最近章节有格式污染、角色称谓不一致、标题明显不合理或未通过审查的问题，请先按写作工具自己的流程修好，再继续一章一章写到真正结尾。",
        )
        .await
        .expect("handled")
        .expect("outcome");

        let message = match outcome {
            super::super::CreationDraftTurnOutcome::ContinueWithMessage(message) => message,
            super::super::CreationDraftTurnOutcome::Respond(response) => panic!(
                "approved existing project continuation should route to writer: {}",
                response.response
            ),
        };
        assert!(
            message.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER),
            "unexpected continuation prompt:\n{}",
            message
        );
        assert!(message.contains("project_path: data/generated/novels/test-project"));
        assert!(!message.contains("我还没有开始写正文"));
    }

    #[tokio::test]
    async fn continuation_in_new_session_restores_authority_before_routing_to_writer() {
        let mut recovered = ready_fiction_draft("previous-session");
        recovered.project_path = "data/generated/novels/recovered-project".to_string();
        recovered.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::Approved);
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: Some(recovered),
            continuation_project_path: Some(
                "data/generated/novels/recovered-project".to_string(),
            ),
            project_path: "data/generated/novels/recovered-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome = super::super::handle_creation_draft_chat(
            &mut runtime,
            "new-session",
            "继续写当前小说下一章",
        )
        .await
        .expect("handled")
        .expect("outcome");

        let message = match outcome {
            super::super::CreationDraftTurnOutcome::ContinueWithMessage(message) => message,
            super::super::CreationDraftTurnOutcome::Respond(response) => panic!(
                "valid recovered authority should route to writer: {}",
                response.response
            ),
        };
        assert!(message.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        let saved = runtime.draft.expect("restored draft");
        assert_eq!(saved.title, "夜校灵轨");
        assert_eq!(
            saved.lifecycle_status(),
            super::super::CreationDraftLifecycleStatus::Approved
        );
    }

    #[tokio::test]
    async fn continuation_in_new_session_rejects_missing_project_authority() {
        let mut runtime = MockCreationDraftRuntime {
            draft: None,
            recovered_draft: None,
            continuation_project_path: Some(
                "data/generated/novels/recovered-project".to_string(),
            ),
            project_path: "data/generated/novels/recovered-project".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome = super::super::handle_creation_draft_chat(
            &mut runtime,
            "new-session",
            "继续写当前小说下一章",
        )
        .await
        .expect("handled")
        .expect("outcome");

        let super::super::CreationDraftTurnOutcome::Respond(response) = outcome else {
            panic!("missing project authority must not route to writer");
        };
        assert!(!response.response.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        assert_ne!(
            runtime
                .draft
                .as_ref()
                .map(|draft| draft.lifecycle_status()),
            Some(super::super::CreationDraftLifecycleStatus::Approved)
        );
    }

    #[test]
    fn creation_draft_extracts_fiction_parameters_across_turns() {
        let mut draft =
            super::super::build_initial_creation_draft("session-a", "fiction", "帮我写小说")
                .expect("draft");

        assert_eq!(draft.artifact_kind, "fiction");
        assert_eq!(draft.tool_name, "novel_studio");
        assert!(draft.genre.is_empty());

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "玄幻，草根逆袭，每章不少于4000字，每次1章，保存txt",
        );

        assert!(draft.genre.contains("玄幻") || draft.brief.contains("玄幻"));
        assert_eq!(draft.target_units, None);
        assert_eq!(draft.chapter_unit_target, Some(5000));
        assert_eq!(draft.max_chapters_per_turn, Some(1));
        assert_eq!(draft.export_format, "txt");

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "都市类型的言情小说，每章3000字，写10万字的短篇小说，要有情感线和完整结尾。",
        );

        assert_eq!(draft.target_units, Some(100000));
        assert_eq!(draft.chapter_unit_target, Some(2500));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "请只展示当前小说创作合同草案、分卷大纲和主要人物弧线，不要生成正文。",
        );
        assert!(!draft.genre.contains("请只展示当前"));
        assert!(!draft.planning_notes.is_empty());
    }

    #[test]
    fn approval_body_target_preserves_initial_project_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "近未来海上灯塔悬疑，总字数10万字，每章2500字。",
        )
        .expect("draft");
        assert_eq!(draft.target_units, Some(100000));
        assert_eq!(draft.chapter_unit_target, Some(2500));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "确认这个合同，开始写第一章。正文目标约2500字，写完请自动审稿并保存。",
        );

        assert_eq!(draft.target_units, Some(100000));
        assert_eq!(draft.chapter_unit_target, Some(2500));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "我确认这份合同。按这个开始，只写第一章，目标约2500字；写完自动审稿、批准保存并导出。",
        );

        assert_eq!(draft.target_units, Some(100000));
        assert_eq!(draft.chapter_unit_target, Some(2500));
    }

    #[test]
    fn replacing_fiction_concept_clears_all_previous_contract_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-reset-contract",
            "fiction",
            "写玄幻小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.title = "旧书名".to_string();
        draft.fiction_premise = "旧故事前提".to_string();
        draft.fiction_must_avoid = vec!["旧合同约束".to_string()];
        draft.current_contract = Some(json!({"title": {"canonical_title": "旧书名"}}));
        draft.pending_contract_candidate = Some(json!({
            "normalized": {"premise": "旧候选前提"},
            "issues": ["旧诊断"]
        }));
        draft.planning_notes = vec!["旧题材设定".to_string()];
        draft.diagnostics = vec!["旧合同诊断".to_string()];

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "不要这个，改成都市职场小说。",
        );

        assert!(draft.current_contract.is_none());
        assert!(draft.pending_contract_candidate.is_none());
        assert!(draft.fiction_premise.is_empty());
        assert!(draft.fiction_must_avoid.is_empty());
        assert!(draft.diagnostics.is_empty());
        assert!(!draft.planning_notes.iter().any(|note| note.contains("旧题材")));
        assert_eq!(draft.target_units, Some(50_000));
        assert_eq!(draft.chapter_unit_target, Some(2_500));
    }

    #[test]
    fn correcting_whole_contract_resets_stale_story_authority_without_genre_dependency() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-correct-contract",
            "fiction",
            "写近未来噪音悬疑小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.title = "旧静默书名".to_string();
        draft.genre = "近未来科幻悬疑".to_string();
        draft.brief = "旧噪音故事".to_string();
        draft.fiction_characters = vec![
            "name: 旧角色; role: 主角; desire: 旧欲望; fear: 旧恐惧; bottom_line: 旧底线"
                .to_string(),
        ];
        draft.fiction_outline = "旧静默场大纲".to_string();
        draft.current_contract = Some(json!({
            "title": {"canonical_title": "旧静默书名"},
            "premise": "旧噪音故事"
        }));
        draft.pending_contract_candidate = Some(json!({
            "normalized": {"premise": "旧噪音候选"},
            "issues": ["旧诊断"]
        }));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "这份合同题材错误，不能确认。请完整更正为：1980年代东南沿海老玻璃厂工业悬疑；停产熔炉复燃后，主角叫许砚秋，身为修复工程师的她调查父辈事故档案与失踪彩色玻璃配方；总字数100000字，每章2500字。删除近未来、噪音能力和旧角色等全部无关设定，不写正文。",
        );

        assert!(draft.current_contract.is_none());
        assert!(draft.pending_contract_candidate.is_none());
        assert!(draft.title.is_empty());
        assert!(draft.fiction_characters.is_empty());
        assert!(draft.fiction_outline.is_empty());
        assert_eq!(draft.target_units, Some(100_000));
        assert_eq!(draft.chapter_unit_target, Some(2_500));
        assert!(draft.brief.contains("老玻璃厂"), "{}", draft.brief);
        assert!(draft.brief.contains("彩色玻璃配方"), "{}", draft.brief);
        assert!(!draft.brief.contains("题材错误"), "{}", draft.brief);
        assert!(!draft.brief.contains("近未来"), "{}", draft.brief);
        assert!(!draft.brief.contains("旧角色"), "{}", draft.brief);
        assert!(draft
            .planning_notes
            .iter()
            .any(|note| note == "明确指定角色姓名：许砚秋"));
        let story_authority = draft
            .planning_notes
            .iter()
            .find_map(|note| note.strip_prefix("用户故事核心权威："))
            .expect("replacement story authority");
        assert!(story_authority.contains("老玻璃厂"), "{story_authority}");
        assert!(!story_authority.contains("旧噪音"), "{story_authority}");
    }

    #[test]
    fn incremental_character_change_does_not_reset_whole_contract_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-character-update",
            "fiction",
            "写现实悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.title = "玻璃余温".to_string();
        draft.fiction_premise = "修复工程师调查旧厂事故。".to_string();
        draft.current_contract = Some(json!({
            "title": {"canonical_title": "玻璃余温"},
            "premise": "修复工程师调查旧厂事故。"
        }));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "这份合同里主角改成女性，其他题材、世界观和大纲保持不变。",
        );

        assert!(draft.current_contract.is_some());
        assert_eq!(draft.title, "玻璃余温");
        assert_eq!(draft.fiction_premise, "修复工程师调查旧厂事故。");
    }

    #[test]
    fn title_only_revision_does_not_reset_fiction_contract_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-title-only-update",
            "fiction",
            "写现实悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.title = "旧厂余温".to_string();
        draft.fiction_premise = "修复工程师调查旧厂事故。".to_string();
        draft.fiction_characters = vec![
            "name: 陶照声; role: 主角; desire: 查清事故; fear: 证据被销毁; bottom_line: 不伪造记录; name_source: generated_by_writing_tool_policy".to_string(),
        ];
        draft.current_contract = Some(json!({
            "title": {"canonical_title": "旧厂余温"},
            "premise": "修复工程师调查旧厂事故。"
        }));

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "把这本小说的书名改为《玻璃余温》，其他合同内容保持不变。",
        );

        assert_eq!(draft.title, "玻璃余温");
        assert!(draft.current_contract.is_some());
        assert_eq!(draft.fiction_premise, "修复工程师调查旧厂事故。");
        assert_eq!(draft.fiction_characters.len(), 1);
    }

    #[test]
    fn creation_draft_view_request_is_read_only_unless_it_mutates() {
        assert!(super::super::creation_draft_view_only_requested(
            "请展示当前小说创作合同草案、人物弧线和预计章节数，先不要写正文。"
        ));
        assert!(super::super::creation_draft_view_only_requested(
            "当前合同是什么？"
        ));
        assert!(!super::super::creation_draft_view_only_requested(
            "请展示当前小说创作合同草案，并把每章改成5000字。"
        ));
        assert!(!super::super::creation_draft_view_only_requested(
            "重新生成一份完整合同。"
        ));
        assert!(super::super::creation_draft_framework_requested(
                "先不要写正文，请根据这些设定给我一个完整框架：书名、主角名字、核心矛盾、20章左右的章节大纲。",
                "fiction"
            ));
        assert!(super::super::creation_draft_framework_requested(
            "请重新生成合同，修复刚才提示的问题。",
            "fiction"
        ));
        assert!(super::super::creation_draft_framework_requested(
            "合同草案未通过质量门，请修订草案。",
            "fiction"
        ));
        assert!(!super::super::creation_draft_framework_requested(
            "开始写第一章",
            "fiction"
        ));
        assert!(
            !super::super::creation_draft_message_requests_continuation_generation(
                "只检查当前项目状态和角色连续性，不要继续写正文。",
                "只检查当前项目状态和角色连续性，不要继续写正文。"
            )
        );
        assert_eq!(
            super::super::creation_draft_content_operation(
                "只检查当前项目状态和角色连续性，不要继续写正文。",
                "fiction"
            ),
            Some(super::super::NovelContentOperation::Read)
        );

        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写一部5万字的短篇爱情小说，每章2500字。先和我多轮对话把框架定下来。",
        )
        .expect("draft");
        let prompt = super::super::final_prompt_from_creation_framework_request(
            &draft,
            "先不要写正文，请给我章节大纲。",
        );
        assert!(prompt.contains(super::super::CREATION_PLANNING_DIALOGUE_MARKER));
        assert!(prompt.contains("初始合同字段包"));
        assert!(prompt.contains("不要把任务理解成法律文书"));
        assert!(prompt.contains("故事蓝图初始阶段：完整 typed batch"));
        assert!(prompt.contains("中文 field-pack 兼容边界"));
        assert!(prompt.contains("不输出 JSON"));
        assert!(prompt.contains("patch_type: contract_batch"));
        assert!(prompt.contains("书名：作品名"));
        assert!(prompt.contains("角色权威表："));
        assert!(prompt.contains("核心主题："));
        assert!(prompt.contains("全书大纲："));
        assert!(prompt.contains("本地命名治理并同步改写所有故事字段"));
        let skeleton = prompt.find("故事前提：").expect("skeleton");
        let characters = prompt.find("角色权威表：").expect("characters");
        let governance = prompt.find("核心主题：").expect("governance");
        let plot = prompt.find("全书大纲：").expect("plot");
        assert!(skeleton < characters && characters < governance && governance < plot);
        assert!(prompt.contains("第三章必须留下明确后续主线债务"));
        assert!(prompt.contains("全书大纲摘要不超过 120 个中文字"));
        assert!(prompt.contains("title.rationale"));
        assert!(prompt.contains("不要写正文") || prompt.contains("不写正文"));
        assert!(prompt.contains("不附英文译名或拼音"));
        assert!(prompt.contains("不要混入韩文/日文/英文括注"));
        assert!(!prompt.contains("合同分段补齐阶段"));
        assert!(!prompt.contains("用户正在定小说创作合同"));
        assert!(!prompt.contains("合同确认阶段"));
        assert!(!prompt.contains("甲方"));
        assert!(!prompt.contains("乙方"));
        assert!(!prompt.contains("报酬"));
        assert!(!prompt.contains("违约"));
        assert!(!prompt.contains("用户可读的 8 段合同摘要"));
        assert!(!prompt.contains("JSON 后再输出以下 8 段"));
        assert!(!prompt.contains("书名意象可从"));
        assert!(!prompt.contains("尘阶 / 试炼 / 破境"));
    }

    #[tokio::test]
    async fn view_only_contract_turn_does_not_mutate_draft() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.title = "旧草案标题".to_string();
        let mut runtime = MockCreationDraftRuntime {
            draft: Some(draft),
            recovered_draft: None,
            continuation_project_path: None,
            project_path: "data/generated/novels/test".to_string(),
            saved: 0,
            approved: 0,
        };

        let outcome =
            super::super::handle_creation_draft_chat(&mut runtime, "session-a", "当前合同是什么？")
                .await
                .expect("turn")
                .expect("outcome");

        assert!(matches!(
            outcome,
            super::super::CreationDraftTurnOutcome::Respond(_)
        ));
        assert_eq!(runtime.saved, 0, "view-only turns must not save drafts");
        assert_eq!(runtime.draft.as_ref().expect("draft").title, "旧草案标题");
    }

    #[test]
    fn approved_creation_prompt_carries_character_authority_table() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-authority",
            "fiction",
            "都市玄幻小说，每章2500字，总字数5万字。",
        )
        .expect("draft");
        draft.title = "潮汐碑".to_string();
        draft.genre = "都市玄幻".to_string();
        draft.brief = "普通调查员追查城市潮汐碑权限。".to_string();
        draft.fiction_characters = vec![
            "name: 秦砚桥; role: 主角; desire: 替母亲洗清旧案; fear: 变成新的压迫者; bottom_line: 不牺牲无辜者"
                .to_string(),
            "name: 顾临川; role: 关键对手; desire: 垄断潮汐碑权限; fear: 旧秩序崩塌; bottom_line: 对手动机必须清晰"
                .to_string(),
        ];

        let prompt = super::super::final_prompt_from_approved_creation_draft(
            &draft,
            &json!({
                "success": true,
                "project_path": "/tmp/novels/tide",
                "draft": {
                    "characters": draft.fiction_characters.clone()
                }
            }),
            "开始写第一章",
        );

        assert!(prompt.contains(super::super::DIRECT_WRITER_CONTINUATION_MARKER));
        assert!(prompt.contains("角色权威表"));
        assert!(prompt.contains("秦砚桥"));
        assert!(prompt.contains("顾临川"));
        assert!(!prompt.contains("重新展示草案"));
    }

    #[test]
    fn creation_draft_separates_total_and_chapter_unit_targets() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "先不要写正文，先定大纲。每章约3000字，一共约50万字。",
        )
        .expect("draft");

        assert_eq!(draft.target_units, Some(500000));
        assert_eq!(draft.chapter_unit_target, Some(2500));

        let draft = super::super::build_initial_creation_draft(
            "session-b",
            "fiction",
            "写一部50万字小说，每章不少于4000字。",
        )
        .expect("draft");

        assert_eq!(draft.target_units, Some(500000));
        assert_eq!(draft.chapter_unit_target, Some(5000));

        let draft = super::super::build_initial_creation_draft(
            "session-c",
            "fiction",
            "异世界重生玄幻，草根逆袭，2500字每章，总字数5万字。",
        )
        .expect("draft");

        assert_eq!(draft.target_units, Some(50000));
        assert_eq!(draft.chapter_unit_target, Some(2500));
    }

    #[test]
    fn generated_contract_quality_keeps_character_names_tool_governed() {
        let weak_contract = "### 标准小说合同草案\n\
书名：断流纪元\n\
题材：都市玄幻\n\
终局方向：主角关闭异常回路，让城市回到普通生活。\n\
主角弧线：从逐利边缘人变成愿意承担代价的守护者。\n\
世界观意象：霓虹、暗流、回路符文。\n\
总主线因果链：能源异常引发觉醒，城市冲突升级，终局关闭异常回路。\n\
命名理由：书名来自终局断开异常能量流。\n\
主角：沈渡；欲望：获得自由；恐惧：被城市秩序吞没；底线：不牺牲无辜。\n\
主要情节链：发现异常回路、追查来源、终局断流。\n\
第01章《霓虹暗流》：本章目标：主角发现第一个异常回路。\n\
第02章《无声之城》：本章目标：主角确认城市异变来源。\n";

        let issues = super::super::generated_fiction_contract_planning_issues(weak_contract, true);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("角色命名依据") || issue.contains("模型默认高频名")),
            "{issues:?}"
        );

        let strong_contract = "### 标准小说合同草案\n\
书名：断流纪元\n\
题材：都市玄幻\n\
终局方向：主角关闭异常回路，让城市回到普通生活。\n\
主角弧线：从逐利边缘人变成愿意承担代价的守护者。\n\
世界观意象：霓虹、暗流、回路符文。\n\
总主线因果链：能源异常引发觉醒，城市冲突升级，终局关闭异常回路。\n\
主要情节链：发现异常回路、追查来源、终局断流。\n\
命名理由：书名来自终局断开异常能量流。\n\
角色命名依据：主角名取自穿过暗流仍选择渡人的弧线，和终局断流形成呼应。\n\
命名候选表：书名候选：断流纪元、雨城无回路、霓虹静默；主角名候选：沈渡、许砚桥、韩听潮；最终选择：书名《断流纪元》，主角沈渡。\n\
主角：沈渡；欲望：获得自由；恐惧：被城市秩序吞没；底线：不牺牲无辜。\n\
第01章《霓虹暗流》：本章目标：主角发现第一个异常回路。\n\
第02章《无声之城》：本章目标：主角确认城市异变来源。\n";

        let issues =
            super::super::generated_fiction_contract_planning_issues(strong_contract, true);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("角色命名依据") || issue.contains("命名候选表")),
            "{issues:?}"
        );
    }

    #[test]
    fn natural_language_contract_does_not_commit_before_approval() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "我想写一部小说，先定大纲，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(5000);
        draft.chapter_unit_target = Some(2500);
        draft.title.clear();
        draft.genre.clear();
        let protagonist = "陆明川".to_string();

        let changed = super::super::apply_generated_contract_to_creation_draft(
            &mut draft,
            &format!("### 标准小说合同草案\n* **书名**：重临九霄：从底层崛起的剑修\n* **题材**：异世界重生、玄幻、草根逆袭\n* **总目标字数**：5,000字\n* **每章目标档位**：2,500字 / 章\n* **主角**：{protagonist}\n* **核心矛盾**：底层修士与资源垄断者的冲突。\n* **结尾承诺**：主角打破垄断，完成自由选择。\n* **世界观意象**：九霄阶梯与寒门剑契。\n* **总主线因果链**：寒门入局引出资源垄断，旧誓成锋推动终局破局。\n* **命名理由**：书名来自主角重回九霄并打破垄断的结局。\n1. 第一章：寒门入局：本章目标：建立起点。\n2. 第二章：旧誓成锋：本章目标：完成收束。\n"),
        );

        assert!(!changed);
        assert!(draft.title.is_empty());
        assert!(draft.genre.is_empty());
        assert!(draft.fiction_characters.is_empty());
    }

    #[test]
    fn partial_strong_json_contract_does_not_clear_existing_draft_fields() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-partial-json",
            "fiction",
            "都市玄幻小说，2500字每章，总字数5000字。",
        )
        .expect("draft");
        draft.title = "夜校灵轨".to_string();
        draft.fiction_premise = "旧城夜校下方的灵脉轨道重启。".to_string();
        draft.fiction_ending_direction = "主角接通灵轨，守住城市。".to_string();
        draft.fiction_outline = "第一阶段：异常；第二阶段：追查。".to_string();

        let _ = super::super::apply_generated_contract_to_creation_draft(
            &mut draft,
            r#"{
                "title": {
                    "canonical_title": "灵轨夜校",
                    "rationale": "灵轨来自终局选择，夜校来自主角起点。"
                }
            }"#,
        );

        assert_eq!(draft.title, "夜校灵轨");
        assert_eq!(draft.fiction_premise, "旧城夜校下方的灵脉轨道重启。");
        assert_eq!(draft.fiction_ending_direction, "主角接通灵轨，守住城市。");
        assert_eq!(draft.fiction_outline, "第一阶段：异常；第二阶段：追查。");
    }

    #[test]
    fn partial_strong_json_contract_does_not_commit_natural_contract_tail() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-partial-json-tail",
            "fiction",
            "都市玄幻小说，2500字每章，总字数5000字。",
        )
        .expect("draft");

        let changed = super::super::apply_generated_contract_to_creation_draft(
            &mut draft,
            r#"{
                "title": {
                    "canonical_title": "灵轨夜校"
                }
            }
            标准小说合同草案
            书名：灵轨夜校
            题材：都市玄幻
            终局方向：主角切断夜校灵轨，守住旧城学生的普通生活。
            主角弧线：从只想逃离夜校，到愿意承担灵轨代价。
            世界观意象：旧城夜校下方的蓝色灵轨。
            总主线因果链：夜校异常引出灵轨债务，主角追查代价并在终局切断灵轨。
            命名理由：灵轨来自世界观核心规则，夜校来自主角起点和终局守护对象。
            角色权威表：姓名：秦知衡，角色：主角，欲望：逃离旧城，恐惧：被灵轨夺走记忆，底线：不让同学替自己承受代价。
            近期章节包：
            第01章《蓝轨晚自习》：本章目标：秦知衡在晚自习看到地下灵轨。
            第02章《欠债名单》：本章目标：秦知衡发现同学名字出现在灵轨债务中。
            "#,
        );

        assert!(!changed);
        assert!(draft.title.is_empty());
        assert!(draft.fiction_ending_direction.is_empty());
        assert!(draft.fiction_characters.is_empty());
        assert!(draft.fiction_outline.is_empty());
    }

    #[test]
    fn natural_character_heading_with_slash_does_not_commit_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-character-heading",
            "fiction",
            "都市玄幻小说，2500字每章，总字数5000字。",
        )
        .expect("draft");
        let contract = "### 标准小说合同草案\n\
书名：断流纪元\n\
题材：都市玄幻\n\
总目标字数：5000\n\
每章目标档位：2500\n\
角色命名依据：沈渡取自穿过暗流仍选择渡人的弧线，和终局断流形成呼应。\n\
主角/重要角色：沈渡（主角，从拾荒者转变为秩序守护者）、林薇（秩序观察员）、周枭（激进派异能者）。\n\
核心矛盾：超凡力量的扩张与现实世界秩序稳定之间的冲突。\n\
终局方向：主角献祭所有超凡回路，世界回归平庸秩序。\n\
主角弧线：从逐利边缘人转变为剥夺自身力量以守护秩序的守护者。\n\
世界观意象：霓虹灯下的暗流、电路纹路符文、电磁干扰感知。\n\
总主线因果链：能源危机引发异变，秩序崩塌，主角回收回路重建秩序。\n\
命名理由：断流取自终局断开异常能量流，纪元指世界回归寂静秩序后的新阶段。\n\
第01章《霓虹暗流》：本章目标：沈渡在废墟中发现第一个异常回路。\n\
第02章《寂城归零》：本章目标：结尾，世界回归平庸，主角归于平凡。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(draft.fiction_characters.is_empty());
        assert!(!super::super::creation_draft_approval_readiness_issues(&draft).is_empty());
    }

    #[test]
    fn natural_character_anchor_variants_do_not_commit_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-character-anchor-variants",
            "fiction",
            "都市玄幻小说，2500字每章，总字数5000字。",
        )
        .expect("draft");
        let contract = "### 标准小说合同草案\n\
书名：潮汐碑\n\
题材：都市玄幻\n\
总目标字数：5000\n\
每章目标档位：2500\n\
角色权威表：\n\
- 主角姓名：秦砚桥，欲望：替母亲洗清旧案，恐惧：获得力量后变成新压迫者，底线：不牺牲无辜。\n\
- 对手姓名：顾临川，欲望：垄断潮汐碑权限，恐惧：旧秩序崩塌。\n\
核心矛盾：普通人与超凡资源垄断者之间的冲突。\n\
终局方向：主角关闭潮汐碑的私有权限，让城市重新获得公平选择。\n\
主角弧线：从只想自保的底层调查员，成长为愿意承担公共代价的守护者。\n\
世界观意象：潮汐碑、雨夜天桥、裂光纹路。\n\
总主线因果链：旧案牵出潮汐碑，主角追查垄断权，终局打破私有权限。\n\
命名理由：书名来自终局被重新开放的潮汐碑。\n\
第01章《雨夜天桥》：本章目标：主角发现旧案与潮汐碑有关。\n\
第02章《碑下回声》：本章目标：主角确认真正对手。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(draft.fiction_characters.is_empty());
        let issues = super::super::generated_fiction_contract_planning_issues(contract, true);
        assert!(
            !issues.iter().any(|issue| issue.contains("稳定角色锚点")),
            "{issues:?}"
        );
    }

    #[test]
    fn creation_draft_does_not_treat_story_arc_labels_as_character_names() {
        let contract = "### 标准小说合同草案\n\
书名：记忆偿还\n\
题材：都市玄幻\n\
总目标字数：5000\n\
每章目标档位：2500\n\
角色权威表：主角姓名：陆离，命名依据：离散记忆，欲望：找回自我，恐惧：彻底遗忘，底线：不伤害无辜者。\n\
终局方向：主角通过献祭所有超凡记忆来封印城市裂隙，回归平凡。\n\
主角弧线：从渴望掌控力量的狂热者转变为守护秩序的守秘人。\n\
世界观意象：城市霓虹下的影子流变、折叠空间、记忆货币化。\n\
总主线因果链：觉醒异能导致记忆流失，记忆流失导致身份丧失，最终通过遗忘换取世界安宁。\n\
第01章《霓虹幻影》：本章目标：陆离在能力失控后发现记忆开始流失。";

        let characters = super::super::generated_fiction_character_lines(contract);

        assert!(
            characters.iter().any(|item| item.contains("陆离")),
            "{characters:?}"
        );
        for noise in ["回归平凡", "弧线", "终局献祭", "裂痕初现", "角色权威表"] {
            assert!(
                !characters.iter().any(|item| item.contains(noise)),
                "{noise} leaked into characters: {characters:?}",
            );
        }
    }

    #[test]
    fn creation_draft_readiness_blocks_truncated_outline_before_approval() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-truncated-outline",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.title = "天律碑重开".to_string();
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        draft.fiction_characters = vec![
            "name: 苏清月; role: 主角; desire: 夺回选择权; fear: 失去自由; bottom_line: 不牺牲无辜者"
                .to_string(),
            "name: 谢寒灯; role: 关键对手; desire: 维护血脉天律; fear: 底层修行者获得选择权; bottom_line: 不亲手毁掉学宫秩序"
                .to_string(),
        ];
        draft.fiction_outline =
            "预计章节数：20\n第20章《天律新篇》：本章目标：结局，世界规则重塑。".to_string();

        let issues = super::super::creation_draft_approval_readiness_issues(&draft);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("大纲含有用户请求参数或流程说明残片")
                    || issue.contains("近期章节包缺少第1章目标")),
            "{issues:?}"
        );
    }

    #[test]
    fn creation_draft_readiness_accepts_complete_story_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-complete-outline",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.title = "天律碑重开".to_string();
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        draft.fiction_characters = vec![
            "name: 苏清月; role: 主角; desire: 夺回选择权; fear: 失去自由; bottom_line: 不牺牲无辜者"
                .to_string(),
            "name: 谢寒灯; role: 关键对手; desire: 维护血脉天律; fear: 底层修行者获得选择权; bottom_line: 不亲手毁掉学宫秩序"
                .to_string(),
        ];
        draft.fiction_ending_direction = "苏清月打破血脉天律，让底层修行者获得选择权。".to_string();
        draft.fiction_protagonist_arc =
            "从只想自保的重生者，成长为愿意承担新秩序代价的人。".to_string();
        draft.fiction_world_imagery = "裂开的天律碑、雨夜学宫、重生灵火。".to_string();
        draft.fiction_world_rules =
            vec!["血脉天律会限制底层修行者入学资格，只有公开天律碑证据才能改写。".to_string()];
        draft.fiction_themes = vec!["选择权必须由承担代价的人争取".to_string()];
        draft.fiction_style_rules = vec!["用具体行动和抉择推进规则冲突".to_string()];
        draft.fiction_must_avoid = vec!["不要跳过证据链或让角色无解释改名".to_string()];
        draft.fiction_main_causal_spine =
            "重生发现旧规则漏洞，入学试炼取得证据，终局公开改写天律。".to_string();
        draft.fiction_title_rationale =
            "天律碑来自雨夜学宫的关键证据，重开来自终局公开改写血脉天律的爽点行动。".to_string();
        draft.fiction_outline = (1..=20)
            .map(|index| {
                if index == 20 {
                    format!("第{index:02}章《天律新篇》：本章目标：完成结局，重塑世界规则；预期转折：天律公开改写，旧规则失去恢复可能。")
                } else {
                    format!("第{index:02}章《第{index}步抉择》：本章目标：推进第 {index} 个选择、代价或转折；预期转折：第 {index} 个选择的后果关闭上一阶段退路。")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let issues = super::super::creation_draft_approval_readiness_issues(&draft);

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn creation_draft_rejects_noisy_generated_contract_before_approval() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.title.clear();

        let noisy = "### 标准小说合同草案\n* **书名**：《万劫归尘》\n* **核心矛盾**：资源垄_垄断者与底层修行者的冲突。\n* **结尾承诺**：不再受血脉等级束나限制。\n";
        let issues = super::super::generated_contract_quality_issues(&draft, noisy);

        assert!(!issues.is_empty());
        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, noisy
        ));
        assert!(draft.title.is_empty());
    }

    #[test]
    fn generated_contract_missing_world_rules_is_canonicalized_before_readiness_check() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-world-rules",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");

        let contract = json!({
            "title": {
                "canonical_title": "夜校灵轨",
                "candidates": ["夜校灵轨", "灵轨借证", "霓虹学籍"],
                "rationale": "书名来自终局中主角公开夜校灵轨账册、改写城市修行资格的选择；夜校对应底层学习入口，灵轨对应贯穿全书的晋级规则。"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "底层青年在城市夜校发现灵轨资格被垄断，通过考试、证据和选择完成逆袭。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "城市灵能夜校把修行资格绑定考试和账册，主角从无证旁听生开始追查资格被篡改的真相。",
            "ending": {
                "desired_resolution": "主角在终局公开灵轨账册，让底层修行者获得合法晋级资格，同时承担守护新规则的代价。",
                "final_state": "旧资格垄断被打破，夜校成为公开晋级入口。",
                "must_resolve": ["资格垄断真相", "主角能否合法晋级", "关键对手的账册阴谋"],
                "allowed_open_questions": []
            },
            "protagonist_arc": "从只想拿到个人资格的旁听生，成长为愿意为底层学生重建晋级规则的人。",
            "world_imagery": "雨夜夜校、发光灵轨、被涂改的资格账册。",
            "main_causal_spine": "旁听生发现资格异常，追查灵轨账册，赢下关键考试，终局公开证据并改写晋级规则。",
            "characters": [
                {
                    "canonical_name": "秦砚禾",
                    "role": "主角",
                    "desire": "获得合法修行资格并证明底层学生也能晋级",
                    "fear": "资格再次被抹除，自己和同伴永远只能旁听",
                    "bottom_line": "不牺牲无辜学生换取个人晋级",
                    "arc_start": "只想自保拿证",
                    "arc_end": "主动承担新规则的代价"
                },
                {
                    "canonical_name": "岑望舒",
                    "role": "关键同伴",
                    "desire": "查清家族账册被篡改的原因",
                    "fear": "真相牵连自己最信任的人",
                    "bottom_line": "不伪造证据",
                    "arc_start": "冷眼旁观",
                    "arc_end": "共同公开真相"
                }
            ],
            "themes": ["资格与选择", "底层逆袭", "规则代价"],
            "style_rules": ["用具体场景推进设定", "保持角色姓名稳定"],
            "must_avoid": ["不要突然秒杀核心冲突", "不要更换主角姓名"],
            "outline": {
                "volumes": [
                    {
                        "title": "雨夜旁听",
                        "objective": "建立夜校资格垄断和主角入局",
                        "ending_change": "主角拿到第一份异常账册证据"
                    }
                ],
                "near_chapters": [
                    {
                        "number": 1,
                        "goal": "主角以旁听身份进入夜校，发现自己的资格记录被抹除。",
                        "expected_turn": "他在雨夜灵轨旁拿到异常账册碎页。"
                    },
                    {
                        "number": 2,
                        "goal": "主角尝试参加基础测验，被资格系统拒绝。",
                        "expected_turn": "同伴发现拒绝记录不是系统故障，而是人为涂改。"
                    }
                ]
            }
        });

        let outcome = super::super::submit_generated_contract_candidate_to_draft(
            &mut draft,
            &contract.to_string(),
        );

        assert!(
            !outcome.is_ready(),
            "missing story world rules must be repaired by the model contract, not locally derived"
        );
        assert!(draft.fiction_world_rules.is_empty());
        assert!(draft.current_contract.is_none());
        let current = draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|value| value.get("normalized"))
            .and_then(|value| {
                super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
            })
            .expect("pending canonical candidate");
        assert!(
            current.world_rules.is_empty(),
            "pending candidate should preserve the model output instead of inventing world rules"
        );
    }

    #[test]
    fn contract_change_normalization_removes_stale_names_before_voice_generation() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-stale-name-voice",
            "fiction",
            "写都市轻玄幻小说，每章2500字，总字数5000字。",
        )
        .expect("draft");
        draft.fiction_premise = "旧物修复师能听见物品残留记忆。".to_string();
        draft.fiction_ending_direction = "主角修复怀表并找回被抹去的城市记忆。".to_string();
        draft.fiction_world_imagery = "雨夜旧巷中流动的记忆微光。".to_string();
        draft.fiction_main_causal_spine =
            "修复怀表→听见残留记忆→追查城市旧案→修复核心记忆".to_string();
        draft.fiction_characters = vec![
            "name: 陶栖序; role: 主角; desire: 修复旧物维持平静; fear: 被嘈杂记忆吞没; bottom_line: 不伪造记忆; arc_start: 回避他人故事; arc_end: 主动守住旧城记忆".to_string(),
            "name: 裴庭序; role: 关键同伴; desire: 寻找家族世代守护的“林默”; fear: 家族使命失败; bottom_line: 不牺牲无辜; arc_start: 只相信家族档案; arc_end: 与主角共同公开真相".to_string(),
            "name: 晏栖晚; role: 关键对手; desire: 抽取旧物记忆精华; fear: 情感失控; bottom_line: 为达目标不惜封存他人记忆; arc_start: 记忆组织代理人; arc_end: 被迫面对记忆回流".to_string(),
        ];

        super::super::normalize_fiction_creation_draft_after_contract_change(&mut draft);

        let contract_v2 = serde_json::to_string(&draft.contract_v2()).expect("contract v2");
        assert!(
            !contract_v2.contains("林默"),
            "stale overused names must not survive into generated voice ledger: contract_v2={contract_v2}"
        );
    }

    #[test]
    fn visible_contract_defaults_do_not_pollute_compact_structured_slots() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-no-compact-slot-pollution",
            "fiction",
            "写异界修仙小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.title = "开天破命".to_string();
        draft.fiction_title_rationale = "来自终局逆命和开天破局的核心爽点。".to_string();
        draft.fiction_premise = "主角发现自身血脉与天地法则存在隐秘关联。".to_string();
        draft.fiction_ending_direction = "主角打破天地法则桎梏实现逆命重生。".to_string();
        draft.fiction_protagonist_arc =
            "从被命运束缚的修炼者成长为打破规则的人。".to_string();
        draft.fiction_world_imagery =
            "破碎虚空的天地裂痕、血色修炼等级体系、法则锁链束缚的修炼者。"
                .to_string();
        draft.fiction_main_causal_spine =
            "血脉觉醒引发反噬，主角突破法则束缚，终局改写世界规则。".to_string();
        draft.fiction_characters = vec![
            "姓名：谢照野，角色：主角，欲望：破除天地法则桎梏，恐惧：血脉反噬，底线：不牺牲无辜，弧线起点：被束缚者，弧线终点：破局者。".to_string(),
            "姓名：岑闻遥，角色：关键对手，欲望：维护法则秩序，恐惧：秩序崩塌，底线：维护旧秩序。".to_string(),
        ];

        let contract = super::super::strong_novel_contract_from_creation_draft(&draft);

        assert!(
            contract.structured.power_progression.system_name.is_empty(),
            "题材或世界意象不应被塞进成长体系名: {:?}",
            contract.structured.power_progression
        );
        assert!(
            contract.structured.geography_model.regions.is_empty(),
            "世界意象不应被塞进地理区域: {:?}",
            contract.structured.geography_model
        );
    }

    #[test]
    fn strong_contract_application_preserves_missing_character_bottom_lines_for_gate() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-visible-character-bottom-line",
            "fiction",
            "写都市爽文小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.fiction_must_avoid = vec!["不要牺牲无辜者换取胜利".to_string()];
        let value = json!({
            "title": {
                "canonical_title": "借玉翻盘",
                "rationale": "玉佩是主角入局物件，翻盘对应终局爽点。"
            },
            "language": "zh-CN",
            "genre": "都市爽文",
            "brief": "底层青年获得玉佩能力，在都市名利场完成逆袭。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "主角获得祖传玉佩，发现行业黑幕并开始反击。",
            "ending": {
                "desired_resolution": "主角公开黑幕，守住同伴并完成翻盘。",
                "final_state": "主角掌握关键证据，建立新的资源秩序。"
            },
            "protagonist_arc": "从被动求生到主动破局。",
            "world_imagery": "玉佩、旧楼、行业黑幕、城市资源网络。",
            "main_causal_spine": "玉佩能力暴露黑幕，证据推动反击，终局公开真相。",
            "characters": [
                {
                    "canonical_name": "姜岑白",
                    "role": "主角",
                    "desire": "从底层逆袭并公开行业黑幕",
                    "fear": "再次失去选择权",
                    "bottom_line": "",
                    "arc_start": "被动求生",
                    "arc_end": "主动破局"
                },
                {
                    "canonical_name": "阮晴澜",
                    "role": "关键对手",
                    "desire": "维持资源垄断",
                    "fear": "黑幕公开",
                    "bottom_line": "",
                    "arc_start": "幕后施压",
                    "arc_end": "被证据反噬"
                }
            ],
            "themes": ["选择权", "代价", "公开真相"],
            "world_rules": [
                "玉佩只能看见价值与弱点，不能直接制造财富。",
                "证据公开前，资源垄断者会持续施压。",
                "每次使用能力都会暴露新的风险。"
            ],
            "must_avoid": ["不要牺牲无辜者换取胜利"],
            "outline": {
                "raw_outline": "主角获得玉佩后追查行业黑幕，终局公开证据完成翻盘。",
                "volumes": [
                    {
                        "title": "玉佩入局",
                        "objective": "建立能力和黑幕压力",
                        "ending_change": "主角拿到第一份关键证据"
                    }
                ],
                "near_chapters": [
                    {
                        "number": 1,
                        "goal": "主角获得玉佩并发现第一处黑幕。",
                        "expected_turn": "他决定主动追查。"
                    }
                ]
            }
        });
        let mut contract =
            super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
                .expect("contract");

        assert!(super::super::apply_strong_novel_contract_to_creation_draft(
            &mut draft,
            &mut contract,
        ));

        assert!(
            draft
                .fiction_characters
                .iter()
                .any(|line| line.contains("bottom_line: ;") || line.contains("底线：。")),
            "missing model anchors should remain visible to the gate instead of being locally invented: {:?}",
            draft.fiction_characters
        );
        assert!(
            super::super::creation_draft_contract_blocking_issues(&draft)
                .iter()
                .any(|issue| issue.contains("缺少底线锚点")),
            "{:?}",
            super::super::creation_draft_contract_blocking_issues(&draft)
        );
    }

    #[test]
    fn visible_fiction_contract_does_not_invent_governance_fields() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-derived-minimum-governance",
            "fiction",
            "写都市爽文小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.title = "借势翻盘".to_string();
        draft.fiction_title_rationale =
            "来自主角借神秘传承和商业危机翻盘的主线与终局兑现。".to_string();
        draft.fiction_premise = "底层青年获得神秘传承，卷入都市商业与武道竞争。".to_string();
        draft.fiction_ending_direction =
            "主角破解垄断规则，建立新的商业与武道秩序。".to_string();
        draft.fiction_protagonist_arc =
            "从自卑隐忍到能承担代价、公开改写规则。".to_string();
        draft.fiction_world_imagery = "霓虹楼宇、旧武馆、黑金卡片".to_string();
        draft.fiction_main_causal_spine =
            "获得传承→解决危机→引出权贵压迫→突破代价→重写秩序".to_string();
        draft.fiction_characters = vec![
            "name: 谢晴白; role: 主角; desire: 改写命运; fear: 重回底层; bottom_line: 不牺牲家人; arc_end: 建立新秩序".to_string(),
            "name: 姜桥安; role: 关键关系; desire: 保住家族事业; fear: 被权贵吞并; bottom_line: 不背叛伙伴".to_string(),
            "name: 季庭晚; role: 关键对手; desire: 垄断江城资源; fear: 权威被挑战; bottom_line: 权力优先".to_string(),
        ];
        draft.fiction_outline =
            "第1卷《旧城借势》：主角获得传承并进入核心冲突；卷尾变化：被权贵正式盯上。\n第1章 本章目标：主角获得传承；预期转折：救下关键关系。"
                .to_string();

        let contract = super::super::strong_novel_contract_from_creation_draft(&draft);

        assert!(contract.structured.antagonist_pressure.primary_pressure.is_empty());
        assert!(contract.structured.relationship_interaction_quotas.is_empty());
        assert!(contract.structured.scene_type_mix.balance_rule.is_empty());
        assert!(contract.structured.reader_promise.core_hook.is_empty());
        assert!(contract.structured.conflict_pressure_curve.global_curve.is_empty());
        assert!(contract.structured.motif_ledger.is_empty());
        assert!(contract.structured.reveal_schedule.is_empty());
    }

    #[test]
    fn normalized_fiction_draft_preserves_missing_v2_fields_for_typed_patch() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-sync-visible-v2-governance",
            "fiction",
            "写都市爽文小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.title = "借势翻盘".to_string();
        draft.fiction_title_rationale =
            "来自主角借神秘传承和商业危机翻盘的主线与终局兑现。".to_string();
        draft.fiction_premise = "底层青年获得神秘传承，卷入都市商业与武道竞争。".to_string();
        draft.fiction_ending_direction =
            "主角破解垄断规则，建立新的商业与武道秩序。".to_string();
        draft.fiction_protagonist_arc =
            "从自卑隐忍到能承担代价、公开改写规则。".to_string();
        draft.fiction_world_imagery = "霓虹楼宇、旧武馆、黑金卡片".to_string();
        draft.fiction_main_causal_spine =
            "获得传承→解决危机→引出权贵压迫→突破代价→重写秩序".to_string();
        draft.fiction_characters = vec![
            "name: 谢晴白; role: 主角; desire: 改写命运; fear: 重回底层; bottom_line: 不牺牲家人; arc_end: 建立新秩序".to_string(),
            "name: 姜桥安; role: 关键关系; desire: 保住家族事业; fear: 被权贵吞并; bottom_line: 不背叛伙伴".to_string(),
            "name: 季庭晚; role: 关键对手; desire: 垄断江城资源; fear: 权威被挑战; bottom_line: 权力优先".to_string(),
        ];

        super::super::normalize_fiction_creation_draft_after_contract_change(&mut draft);
        let contract = draft.contract_v2();

        assert!(contract.character_voice_ledger.is_empty());
        assert!(contract.reader_promise.core_hook.trim().is_empty());
        assert!(contract.relationship_interaction_quotas.is_empty());
    }

    #[test]
    fn current_contract_path_preserves_missing_fields_for_the_typed_gate() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-current-contract-minimums",
            "fiction",
            "写都市爽文小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.title = "借势翻盘".to_string();
        draft.fiction_title_rationale =
            "来自主角借神秘传承和商业危机翻盘的主线与终局兑现。".to_string();
        draft.fiction_premise = "底层青年获得神秘传承，卷入都市商业与武道竞争。".to_string();
        draft.fiction_ending_direction =
            "主角破解垄断规则，建立新的商业与武道秩序。".to_string();
        draft.fiction_protagonist_arc =
            "从自卑隐忍到能承担代价、公开改写规则。".to_string();
        draft.fiction_world_imagery = "霓虹楼宇、旧武馆、黑金卡片".to_string();
        draft.fiction_main_causal_spine =
            "获得传承→解决危机→引出权贵压迫→突破代价→重写秩序".to_string();
        draft.fiction_characters = vec![
            "name: 谢晴白; role: 主角; desire: 改写命运; fear: 重回底层; bottom_line: 不牺牲家人; arc_end: 建立新秩序".to_string(),
            "name: 姜桥安; role: 关键关系; desire: 保住家族事业; fear: 被权贵吞并; bottom_line: 不背叛伙伴".to_string(),
            "name: 季庭晚; role: 关键对手; desire: 垄断江城资源; fear: 权威被挑战; bottom_line: ".to_string(),
        ];
        draft.fiction_outline =
            "简述：底层青年获得神秘传承。\n大纲框架：完整合同需要由 LLM 补齐。"
                .to_string();

        let mut current = super::super::strong_novel_contract_from_visible_creation_draft(&draft);
        current.outline = Default::default();
        current.structured = Default::default();
        if let Some(opponent) = current
            .characters
            .iter_mut()
            .find(|character| character.role.contains("对手"))
        {
            opponent.bottom_line.clear();
        }
        draft.current_contract = Some(serde_json::to_value(&current).expect("contract json"));

        let rebuilt = super::super::strong_novel_contract_from_creation_draft(&draft);

        assert!(!rebuilt.outline.has_stage_or_near_chapter_plan());
        assert!(rebuilt.structured.relationship_interaction_quotas.is_empty());
        assert!(rebuilt.structured.scene_type_mix.balance_rule.is_empty());
        assert!(
            rebuilt
                .characters
                .iter()
                .any(|character| character.bottom_line.trim().is_empty()),
            "current_contract path must preserve missing character anchors for the gate"
        );
        let report = rebuilt.validate();
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("缺少底线锚点")),
            "{:?}",
            report.issues
        );
        assert!(
            report.issues.iter().any(|issue| issue.contains("分卷")
                || issue.contains("近期章节")
                || issue.contains("结构")),
            "{:?}",
            report.issues
        );
    }

    #[test]
    fn creation_contract_field_parser_ignores_preface_mentions() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-preface",
            "fiction",
            "帮我写小说。",
        )
        .expect("draft");
        let contract = "由于您尚未提供具体的题材、背景或故事偏好，我将先给出草案。\n\
### 标准小说合同草案\n\
* 书名：回路重构\n\
* 语言：zh-CN\n\
* 题材：近未来赛博朋克 / 意识上传\n\
* 主角：陆离\n\
* 核心矛盾：意识数据与企业垄断的冲突。\n\
* 结尾承诺：主角回归真实世界。\n\
* 世界观意象：意识回路与真实街区。\n\
* 总主线因果链：意识上传事故引出企业垄断，主角夺回身体完成回归。\n\
* 命名理由：书名来自主角重构意识回路并回到真实世界的结局。";

        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, contract
        ));
        assert!(draft.genre.is_empty());
        assert!(draft.title.is_empty());
        assert!(!matches!(
            super::super::CreationDraftLifecycleStatus::from_str(&draft.status),
            Some(super::super::CreationDraftLifecycleStatus::ContractReady)
        ));
    }

    #[test]
    fn creation_draft_replaces_genre_when_user_says_change_to() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-replace-genre",
            "fiction",
            "帮我写小说。",
        )
        .expect("draft");
        draft.genre = "近未来赛博朋克 / 意识上传".to_string();
        draft.title = "回路重构".to_string();
        draft.fiction_characters = vec!["主角：陆离".to_string()];
        draft.fiction_outline = "旧科幻大纲".to_string();

        super::super::apply_message_to_creation_draft(
            &mut draft,
            "改成异世界重生玄幻小说，要求草根逆袭，2500字每章，总共5万字。",
        );

        assert_eq!(draft.genre, "异世界重生玄幻");
        assert!(draft.title.is_empty());
        assert!(draft.fiction_characters.is_empty());
        assert!(draft.fiction_outline.is_empty());
    }

    #[test]
    fn creation_contract_rejects_degenerate_repetition() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-repetition",
            "fiction",
            "帮我写小说。",
        )
        .expect("draft");
        draft.title.clear();

        let repeated = "### 标准小说合同草案\n* 书名：重构逻辑\n* 主角：陆离\n* 核心矛盾：逻辑秩序与个体觉醒之间的冲突。\n* 结尾承诺：主角重塑世界。\n* 质量合同：人物不漂移，严格遵循角色的逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑逻辑。";
        let issues = super::super::generated_contract_quality_issues(&draft, repeated);

        assert!(
            issues.iter().any(|issue| issue.contains("连续重复退化")),
            "{issues:?}"
        );
        assert!(!super::super::apply_generated_contract_to_creation_draft(
            &mut draft, repeated
        ));
    }

    #[test]
    fn creation_draft_treats_repetitive_planned_titles_as_non_blocking() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(25000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let repetitive = format!(
            "### 标准小说合同草案\n\
* **书名**：霜井问灯\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与资源垄断者的冲突。\n\
* **结局承诺**：主角打破旧秩序。\n\
* **每章目标档位**：2500字\n\
1. 第一章：命运之门：本章目标：建立起点。\n\
2. 第二章：试炼之门：本章目标：推进冲突。\n\
3. 第三章：灵契之门：本章目标：推进冲突。\n\
4. 第四章：天火之门：本章目标：推进冲突。\n\
5. 第五章：旧城之门：本章目标：推进冲突。\n\
6. 第六章：荒塔之门：本章目标：推进冲突。\n\
7. 第七章：玄光之门：本章目标：推进冲突。\n\
8. 第八章：归墟之门：本章目标：推进冲突。\n\
9. 第九章：终局之门：本章目标：推进冲突。\n\
10. 第十章：新王之门：本章目标：完成结局。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &repetitive);

        assert!(!issues
            .iter()
            .any(|issue| issue.contains("章节标题模板过于重复")
                || issue.contains("章节标题句式过于单一")));
    }

    #[test]
    fn creation_draft_rejects_title_equal_to_protagonist_name() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(5000);
        draft.chapter_unit_target = Some(2500);
        draft.title.clear();
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：{protagonist}\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与资源垄断者的冲突。\n\
* **结局承诺**：主角重塑规则。\n\
* **每章目标档位**：2500字\n\
1. 第一章：寒门入局：本章目标：建立起点。\n\
2. 第二章：旧誓成锋：本章目标：完成收束。"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);

        assert!(issues
            .iter()
            .any(|issue| issue.contains("书名直接复用了主角名")));
    }

    #[test]
    fn creation_contract_repairs_title_equal_to_protagonist_name() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(5000);
        draft.chapter_unit_target = Some(2500);
        draft.title.clear();
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：{protagonist}\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与资源垄断者围绕旧律和命灯的冲突。\n\
* **世界规则**：每一次突破都必须付出记忆代价。\n\
* **结局承诺**：主角以凡骨重写天衡，保留人的自由。\n\
* **每章目标档位**：2500字\n\
1. 第一章《寒门入局》：本章目标：建立起点。\n\
2. 第二章《旧誓成锋》：本章目标：完成收束。"
        );

        assert!(super::super::repair_creation_contract_plan_titles(&draft, &contract).is_none());
        assert!(
            super::super::generated_contract_quality_issues(&draft, &contract)
                .iter()
                .any(|issue| issue.contains("书名直接复用了主角名"))
        );
    }

    #[test]
    fn creation_contract_repairs_malformed_chapter_number_lines() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(5000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：凡骨天衡\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与门阀规则的冲突。\n\
* **结局承诺**：主角改写旧律。\n\
* **每章目标档位**：2500字\n\
第01章《寒门入局》：本章目标：建立起点。\n\
希望：第02章《黑市交易》：本章目标：通过交易获取关键情报。\n\
第0下章《秘境争夺》：本章目标：在资源争夺战中付出代价。"
        );

        let sanitized = super::super::sanitize_generated_contract_surface(&draft, &contract);
        assert!(sanitized.contains("第02章《黑市交易》"));
        let repaired = super::super::repair_creation_contract_plan_titles(&draft, &sanitized)
            .expect("malformed plan should be repairable");

        assert!(repaired.contains("第03章《秘境争夺》"));
        assert!(!repaired.contains("第0下章"));
    }

    #[test]
    fn creation_draft_rejects_malformed_numbers_and_goal_less_chapter_plan() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        draft.target_units = Some(50000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let contract = format!(
            "### 标准小说合同草案\n\
* **书名**：碎影重鸣\n\
* **主角**：{protagonist}\n\
* **核心矛盾**：底层修士与资源垄断者的冲突。\n\
* **结局承诺**：主角重塑规则。\n\
* **每章目标档位**：250_0字\n\
1. 第1章：残响重临\n\
2. 第2章：微弱律动\n\
3. 第3章：规则边缘\n\
4. 第4章：第一裂纹\n\
5. 第5章：破晓之战\n\
6. 第6章：秩序阴影\n\
7. 第7章：法则代价\n\
8. 第8章：逆流而上\n\
9. 第9章：破碎真相\n\
10. 第10章：权力博弈\n\
11. 第11章：风暴中心\n\
12. 第12章：重塑之始\n\
13. 第13章：终极审判\n\
14. 第14章：秩序重构\n\
15. 第15章：新纪元曙光\n\
16. 第16章：余音绕梁\n\
17. 第17章：尘埃落定\n\
18. 第18章：法则高台\n\
19. 第19章：永恒回响\n\
20. 第20章：微光不再"
        );

        let issues = super::super::generated_contract_quality_issues(&draft, &contract);

        assert!(issues
            .iter()
            .any(|issue| issue.contains("合同数字格式异常")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("逐章规划缺少章节目标")));
    }

    #[test]
    fn creation_draft_sanitizes_surface_noise_before_boundary_parse() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "异世界重生玄幻，2500字每章，总字数5万字。",
        )
        .expect("draft");
        let mut draft = draft;
        draft.target_units = Some(5000);
        draft.chapter_unit_target = Some(2500);
        let protagonist = "陆明川".to_string();
        let noisy = format!("### 标准小说合同草案\n* **书名**：灵渊破契\n* **关系线**：主角守住选择 나\n* **成长线**：从微末到破局\n* **主角**：{protagonist}\n* **核心矛盾**：底层修士与资源垄断者的冲突。\n* **结尾承诺**：主角完成自由选择。\n* **世界观意象**：灵渊、寒门旧誓与自由契印。\n* **总主线因果链**：寒门起点推动试炼，旧誓逼近真相，终局完成自由选择。\n* **命名理由**：灵渊来自世界规则，自由契印来自终局选择；破契对应主角打破资源垄断旧誓的关键爽点。\n* **每章目标档位**：2500字\n1. 第一章：寒门入局：本章目标：建立起点。\n2. 第二章：旧誓成锋：本章目标：完成收束。");

        let sanitized = super::super::sanitize_generated_contract_surface(&draft, &noisy);

        assert!(!sanitized.contains('나'));
        assert!(!sanitized.contains("I*"));
        assert!(super::super::generated_contract_quality_issues(&draft, &sanitized).is_empty());
    }

    #[test]
    fn chinese_enumeration_keeps_chapter_band_separate_from_total_units() {
        let message = "全部由你补齐，书名、世界规则、核心主题、叙事风格、分卷、近期章节、关系线和结局都由你根据都市玄幻、2500字每章、至少5万字来定。";

        assert_eq!(
            super::super::requested_raw_chapter_unit_target(message),
            Some(2500)
        );
        assert_eq!(
            super::super::requested_chapter_unit_target(message),
            Some(2500)
        );
        assert_eq!(
            super::super::requested_total_unit_target(message),
            Some(50_000)
        );

        let draft = super::super::build_initial_creation_draft("session-a", "fiction", message)
            .expect("draft");
        assert_eq!(draft.chapter_unit_target, Some(2500));
        assert_eq!(draft.target_units, Some(50_000));
    }

    #[test]
    fn field_pack_character_patch_rejects_non_character_terms() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = "角色权威表：林昊、赵天雄、感知、灵石、破解符文阵法\n关系线：林昊和赵天雄围绕血脉传承对抗";

        let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw);

        assert!(
            patch.is_none(),
            "loose field-pack terms must not become character authority: {patch:?}"
        );
    }

    #[test]
    fn llm_character_patch_names_are_locally_governed_before_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-character-governance",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = r#"{
            "patch_type":"character_patch",
            "characters":[
                {"canonical_name":"林凡","role":"主角","desire":"掌握双界规则","fear":"被权贵与修士联手抹杀","bottom_line":"不向任何势力低头","arc_start":"被欺凌的普通青年","arc_end":"掌控双界规则的宗师"},
                {"canonical_name":"苏璃","role":"导师","desire":"守护家族传承","fear":"修真界规则吞噬人性","bottom_line":"坚持人性与道义的平衡","arc_start":"只在暗处提供线索","arc_end":"公开承担守护传承的责任"},
                {"canonical_name":"赵天雄","role":"关键对手","desire":"掌控都市修真体系","fear":"被更强存在取代","bottom_line":"维护自身利益","arc_start":"借旧制度压制异见","arc_end":"因拒绝改变而失去控制权"}
            ]
        }"#;

        let outcome =
            super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);

        assert!(
            !outcome.is_ready(),
            "character patch alone should not complete a full contract"
        );
        assert!(
            draft.fiction_characters.is_empty(),
            "an incomplete contract patch must not leak into the visible confirmable draft"
        );
        let pending = draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
            .and_then(|normalized| {
                super::super::NovelCreationContract::parse_json_boundary(
                    &normalized.to_string(),
                )
            })
            .expect("pending typed contract");
        let characters = pending
            .characters
            .iter()
            .map(|character| character.to_draft_line())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!characters.contains("name: 林凡"), "{characters}");
        assert!(!characters.contains("name: 苏璃"), "{characters}");
        assert!(!characters.contains("name: 赵天雄"), "{characters}");
        assert!(characters.contains("previous_names: 林凡"), "{characters}");
        assert!(characters.contains("previous_names: 苏璃"), "{characters}");
        assert!(characters.contains("previous_names: 赵天雄"), "{characters}");
        assert!(characters.contains("name_source: generated_by_writing_tool_policy"));
    }

    #[test]
    fn incomplete_fiction_draft_does_not_render_full_confirmable_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.title = "都市龙尊".to_string();
        draft.fiction_premise =
            "现代都市中隐藏着古老玄门规则，普通人被迫卷入觉醒事件。".to_string();
        draft.fiction_characters = vec![
            "name: 林昊; role: 主角; desire: 觉醒力量; fear: 失控; bottom_line: 不伤害无辜"
                .to_string(),
        ];

        let response = super::super::creation_draft_planning_response_text(
            &draft,
            "按这个开始写第一章",
        );

        assert!(!response.contains("标准小说合同草案"));
        assert!(!response.contains("核心主题：未指定"));
        assert!(!response.contains("世界规则：未指定"));
        assert!(response.contains("补齐并通过质量门"));
    }

    #[test]
    fn compact_plot_patch_keys_fill_strong_outline_fields() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = r#"{
            "patchtype":"plotpatch",
            "outline":{
                "volumes":[
                    {"title":"初觉卷","objective":"神瞳觉醒与都市势力初接触","endingchange":"林昊暴露身份并进入势力视野"}
                ],
                "nearchapters":[
                    {"number":1,"goal":"林昊完成神瞳初次觉醒","expectedturn":"能力觉醒引发首次势力关注"}
                ],
                "rawoutline":"主角在都市中觉醒神瞳并卷入势力争夺。"
            },
            "payoffmatrix":[
                {"promise":"神瞳觉醒","payofftarget":"能力觉醒引发首次势力关注","status":"planned"}
            ]
        }"#;

        let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw)
            .expect("compact plot patch");
        let super::super::CreationContractPatch::Plot(ref plot) = patch else {
            panic!("expected plot patch, got {patch:?}");
        };

        assert_eq!(plot.volumes[0].ending_change, "林昊暴露身份并进入势力视野");
        assert_eq!(plot.near_chapters[0].expected_turn, "能力觉醒引发首次势力关注");
        assert_eq!(plot.payoff_matrix[0].payoff_target, "能力觉醒引发首次势力关注");
        assert!(patch.validate_scope(&draft).ready());
    }

    #[test]
    fn plot_patch_keeps_typed_outline_when_rebuilding_strong_contract() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.set_lifecycle_status(super::super::CreationDraftLifecycleStatus::DraftingContract);
        let raw = r#"{
            "patchtype":"plotpatch",
            "outline":{
                "volumes":[
                    {"title":"迷雾初开","objective":"建立都市玄幻世界框架","endingchange":"主角首次感知到数据与灵气的共振"}
                ],
                "nearchapters":[
                    {"number":1,"goal":"主角在都市夜校觉醒异瞳","expectedturn":"异瞳首次看见灵气数据流"}
                ]
            }
        }"#;

        let outcome =
            super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
        assert!(
            !outcome.is_ready(),
            "only the plot patch was supplied, so the full contract should still need repair"
        );
        assert!(
            draft.fiction_outline.is_empty(),
            "plot patch must stay pending while the contract is incomplete: {}",
            draft.fiction_outline
        );
        let contract = draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
            .and_then(|value| {
                super::super::NovelCreationContract::parse_json_boundary(&value.to_string())
            })
            .expect("pending typed contract");
        assert!(
            !contract.outline.raw_outline.contains("第1卷"),
            "raw outline should not retain typed control blocks: {}",
            contract.outline.raw_outline
        );
        assert_eq!(contract.outline.volumes[0].title, "迷雾初开");
        assert_eq!(
            contract.outline.volumes[0].ending_change,
            "主角首次感知到数据与灵气的共振"
        );
        assert_eq!(contract.outline.near_chapters[0].number, Some(1));
        assert_eq!(
            contract.outline.near_chapters[0].expected_turn,
            "异瞳首次看见灵气数据流"
        );
    }

    #[test]
    fn plot_patch_canonicalizes_primary_role_references_to_character_authority() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-plot-authority",
            "fiction",
            "写都市爽文小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.fiction_characters = vec![
            "姓名：南晴舟；角色定位：主角；欲望：夺回旧城账户主导权；恐惧：被债务系统吞没；底线：不让普通人替自己背债；弧线起点：被动求生；弧线终点：公开规则漏洞".to_string(),
            "姓名：阮砚晚；角色定位：关键关系对象；欲望：守住底层客户；恐惧：信任再次崩塌；底线：不伪造证据；弧线起点：旁观；弧线终点：共同作证".to_string(),
            "姓名：许砚安；角色定位：关键对手；欲望：维持账户黑箱；恐惧：漏洞公开；底线：不让审计介入；弧线起点：操盘；弧线终点：被迫退场".to_string(),
        ];
        let raw = r#"{
            "patch_type":"plot_patch",
            "outline":{
                "volumes":[
                    {"title":"旧账开盘","objective":"主角陶予声统领旧城账户联盟并追查黑箱","ending_change":"陶予声公开第一份账户证据"}
                ],
                "near_chapters":[
                    {"number":1,"goal":"主角陶予声名普通的公司职员，却发现账户系统会转嫁债务","expected_turn":"陶予声第一次用证据反击"}
                ],
                "raw_outline":"主角陶予声统领旧城账户联盟，从被动背债走向公开黑箱。"
            }
        }"#;

        let outcome = super::super::submit_generated_contract_candidate_to_draft(&mut draft, raw);
        assert!(
            !outcome.is_ready(),
            "plot patch alone should not make the contract ready"
        );
        assert!(
            draft.fiction_outline.is_empty(),
            "plot patch must not visibly rewrite draft outline while the contract is not ready: {}",
            draft.fiction_outline
        );
        let pending = draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
            .expect("pending normalized contract");
        let pending_text = pending.to_string();
        assert!(
            !pending_text.contains("陶予声"),
            "pending plot patch should not retain the stale protagonist surface: {pending_text}"
        );
    }

    #[test]
    fn compact_raw_outline_is_derived_into_structured_plot_fields() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = r#"{
            "patchtype":"plotpatch",
            "outline":{
                "rawoutline":"第1卷《初入玄门》：完成主角对修真世界的初步认知；卷尾变化：主角掌握基础御剑术并结识第一位同伴第1章 本章目标：主角意外获得上古玉璧；预期转折：玉璧觉醒引发天地异象"
            }
        }"#;

        let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw)
            .expect("raw outline patch");
        let super::super::CreationContractPatch::Plot(ref plot) = patch else {
            panic!("expected plot patch, got {patch:?}");
        };

        assert_eq!(plot.volumes[0].title, "初入玄门");
        assert_eq!(plot.volumes[0].ending_change, "主角掌握基础御剑术并结识第一位同伴");
        assert_eq!(plot.near_chapters[0].number, Some(1));
        assert_eq!(plot.near_chapters[0].goal, "主角意外获得上古玉璧");
        assert_eq!(plot.near_chapters[0].expected_turn, "玉璧觉醒引发天地异象");
    }

    #[test]
    fn compact_raw_outline_cleans_volume_title_echo_from_objective() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = r#"{
            "patchtype":"plotpatch",
            "outline":{
                "rawoutline":"第1卷《卷一：浮城拾荒》：浮城拾荒》：唐砚舟获得源初核心碎片并确认灵网吞噬资源；卷尾变化：唐砚舟被迫进入灵网争夺第1章 本章目标：唐砚舟拿到第一枚碎片；预期转折：灵网开始追踪他"
            }
        }"#;

        let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw)
            .expect("raw outline patch");
        let super::super::CreationContractPatch::Plot(ref plot) = patch else {
            panic!("expected plot patch, got {patch:?}");
        };

        assert_eq!(plot.volumes[0].title, "浮城拾荒");
        assert_eq!(
            plot.volumes[0].objective,
            "唐砚舟获得源初核心碎片并确认灵网吞噬资源"
        );
        assert_eq!(plot.volumes[0].ending_change, "唐砚舟被迫进入灵网争夺");
        assert_eq!(plot.near_chapters[0].goal, "唐砚舟拿到第一枚碎片");
    }

    #[test]
    fn complete_characters_do_not_route_relationship_issue_back_to_character_stage() {
        let mut draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        draft.fiction_premise = "现代都市存在隐秘玄门制度。".to_string();
        draft.fiction_ending_direction = "主角揭开制度漏洞并建立新秩序。".to_string();
        draft.fiction_world_imagery = "霓虹楼宇下的灵脉账册。".to_string();
        draft.fiction_main_causal_spine =
            "获得账册线索→进入玄门秩序→揭开代价→重写规则".to_string();
        draft.fiction_characters = vec![
            "name: 江衡; role: 主角; desire: 查清父亲失踪; fear: 害死同伴; bottom_line: 不牺牲无辜".to_string(),
            "name: 许青栀; role: 关系对象; desire: 保住旧城; fear: 再次被抛弃; bottom_line: 不背叛旧城".to_string(),
            "name: 顾玄章; role: 关键对手; desire: 垄断灵脉账册; fear: 制度失控; bottom_line: 权力优先".to_string(),
        ];
        draft.fiction_outline =
            "第1卷《旧城借灵》：主角发现旧城灵脉账册；卷尾变化：主角被玄门通缉。"
                .to_string();

        let issues = super::super::issue::ContractIssueList::from_messages(
            "contract.governance",
            super::super::issue::ContractIssueKind::Governance,
            "governance",
            vec![
                "ContractBlocker: 小说合同缺少关系线或关键人物关系账本".to_string(),
                "ContractBlocker: 小说合同缺少世界规则".to_string(),
            ],
        );
        let prompt = super::super::final_prompt_from_staged_contract_completion(
            &draft,
            "继续补齐合同",
            &issues,
        );

        assert!(prompt.contains("Governance typed patch"), "{prompt}");
        assert!(!prompt.contains("Characters typed patch"), "{prompt}");
        let compact = prompt
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        let quota_index = compact
            .find("\"relationship_interaction_quotas\"")
            .expect("relationship quota example");
        let after_quota = &compact[quota_index..];
        assert!(
            after_quota.contains("}],\"resource_economy\""),
            "genre governance fields must stay inside structured: {prompt}"
        );
        assert!(
            !after_quota.contains("]}},\"resource_economy\""),
            "genre governance fields must not be appended after structured closes: {prompt}"
        );
    }

    #[test]
    fn governance_patch_reads_top_level_genre_structured_fields() {
        let draft = super::super::build_initial_creation_draft(
            "session-a",
            "fiction",
            "写异界修仙小说，每章2500字，至少5万字起。",
        )
        .expect("draft");
        let raw = r#"{
            "patch_type":"governance_patch",
            "themes":["代价与守护"],
            "world_rules":["灵脉借用必须抵押记忆。"],
            "resource_economy":{"currency":"灵石与记忆债","value_scale":"灵石购买术材，记忆债决定高阶法门资格","resource_types":["灵石","记忆债"],"scarcity_rules":["记忆债不可伪造"]},
            "power_progression":{"system_name":"问道阶序","levels":["引气","筑基","金丹"],"advancement_costs":["突破需偿还记忆债"],"anti_power_creep_rules":["越级使用法门会损伤神识"]},
            "social_order":{"institutions":["问道司"],"rank_system":"宗门阶序与凡籍制度并行","authority_conflicts":["问道司与宗门争夺灵脉审判权"]}
        }"#;

        let patch = super::super::normalize_creation_contract_patch_boundary(&draft, raw)
            .expect("governance patch");
        let super::super::CreationContractPatch::Governance(ref governance) = patch else {
            panic!("expected governance patch, got {patch:?}");
        };

        assert_eq!(governance.structured.resource_economy.currency, "灵石与记忆债");
        assert_eq!(governance.structured.power_progression.system_name, "问道阶序");
        assert_eq!(
            governance.structured.social_order.rank_system,
            "宗门阶序与凡籍制度并行"
        );
        assert!(patch.validate_scope(&draft).ready(), "{patch:?}");
    }
