    #[test]
    fn prose_surface_gate_allows_natural_repeated_verb_particles() {
        let issues = prose_surface_contamination_issues(
            "此时此刻，陆远揉了揉太阳穴，又不得不应对窗外逐渐变密的雨。",
        );

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("malformed CJK prose")),
            "natural CJK prose should not be treated as malformed: {issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_blocks_add_json_alias_leakage() {
        let issues =
            prose_surface_contamination_issues("他推开控制室的大门。\n{\"add\":\"新的正文\"}");

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("JSON field/control surface")),
            "JSON add aliases should be treated as prose contamination: {issues:?}"
        );
    }

    #[test]
    fn post_body_title_preserves_supported_x_de_y_template() {
        let now = now_iso();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "灵考碑证".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "都市玄幻".to_string(),
            brief: "草根学生通过灵考揭开校盟垄断。".to_string(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: now.clone(),
            updated_at: now.clone(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: None,
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let summary = "许闻在旧操场发现灵籍碑，把旁听生身份和校盟暗账连在一起。";
        let content = "许闻把借灵证压在掌心，走进旧操场。雨水顺着看台缝隙落下，灵籍碑在泥水里亮起一行被抹去的名字。校盟老师要他离开，他却当众读出暗账编号，让旁听生第一次拥有了证词。";

        let title =
            final_chapter_title_from_body(&manifest, 1, "第1章：命运的裂缝", summary, content);

        assert_eq!(
            normalized_title_key(&title),
            normalized_title_key("命运的裂缝")
        );
    }

    #[test]
    fn post_body_title_persists_core_without_chapter_prefix() {
        let now = now_iso();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "第一桶金掠夺战".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "都市爽文".to_string(),
            brief: "重生都市金融爽文。".to_string(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: now.clone(),
            updated_at: now.clone(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: None,
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let summary = "南栖晚重回2014年，确认华创科技和云图网络是她夺回第一桶金的廉价筹码。";
        let content = "南栖晚在2014年的出租屋醒来，电脑屏幕上还停着华创科技和云图网络的旧行情。她记得未来的资本风暴，也记得秦闻遥和许岑安如何在酒会里低估她。她把仅有的钱压进廉价筹码，决定先夺回自己的第一桶金。";

        let title = final_chapter_title_from_body(
            &manifest,
            1,
            "第一章坠落与重启：2014年的廉价筹码",
            summary,
            content,
        );

        assert!(!title.starts_with("第一章"), "{title}");
        assert!(!title.starts_with("第1章"), "{title}");
        assert!(
            title.contains("2014") || title.contains("廉价筹码") || title.contains("第一桶金"),
            "{title}"
        );
    }

    #[test]
    fn chapter_summary_replaces_supported_setting_quote_with_story_fact() {
        let now = now_iso();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "灵脉浮岛主".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "异界修仙".to_string(),
            brief: "无灵根矿奴通过断根法打破灵根垄断。".to_string(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: now.clone(),
            updated_at: now.clone(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: Vec::new(),
            reviews: Vec::new(),
            review_cycles: Vec::new(),
            truth_validations: Vec::new(),
            hook_debt_reports: Vec::new(),
            truth_files: Vec::new(),
            archives: Vec::new(),
            contract: None,
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: vec![CharacterAuthorityRecord {
                id: "char-0001".to_string(),
                name_source: "contract".to_string(),
                planned_entry: String::new(),
                planned_exit: String::new(),
                canonical_name: "段照棠".to_string(),
                aliases: Vec::new(),
                identity_markers: Vec::new(),
                role: "主角".to_string(),
                desire: "以凡人之躯斩断灵根垄断".to_string(),
                fear: "一生困死矿区".to_string(),
                bottom_line: "不牺牲矿奴同伴换取修行捷径".to_string(),
                arc_start: "绝脉矿奴".to_string(),
                arc_end: "新道统开创者".to_string(),
                forbidden_renames: Vec::new(),
                status: "active".to_string(),
                updated_at: now.clone(),
            }],
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let mut chapter = ChapterRecord {
            number: 1,
            title: "纯度灵石".to_string(),
            path: "chapters/0001.md".to_string(),
            status: "draft".to_string(),
            unit_count: 0,
            summary: "在苍岚浮岛，铜钱代表购买力，而真正的灵石，则是通往更高阶层的钥匙"
                .to_string(),
            key_facts: vec![
                "段照棠天生绝脉，在苍岚浮岛外环矿区做矿奴。".to_string(),
                "内环灵脉节点暴动，高纯度灵石坠向外环，段照棠捡到一块八成纯度灵石并引发经脉异动。"
                    .to_string(),
            ],
            continuity_updates: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            volume_id: String::new(),
            volume_title: String::new(),
        };
        let content = "苍岚浮岛外环矿区里，铜钱代表购买力，而真正的灵石，则是通往更高阶层的钥匙。段照棠天生绝脉，在苍岚浮岛外环矿区做矿奴。内环灵脉节点暴动，高纯度灵石坠向外环，段照棠捡到一块八成纯度灵石并引发经脉异动。";

        normalize_chapter_metadata_against_body(&manifest, &mut chapter, content);

        assert!(
            chapter.summary.contains("段照棠") && chapter.summary.contains("经脉异动"),
            "{}",
            chapter.summary
        );
        assert!(!chapter.summary.starts_with("在苍岚浮岛"), "{}", chapter.summary);
    }

    #[test]
    fn structured_contract_sync_prefers_confirmed_contract_over_derived_revision() {
        let mut manifest = test_manifest_with_primary_character();
        manifest.structured_contract_v2.revision = 2;
        manifest.structured_contract_v2.reader_promise.core_hook =
            "旧项目级承诺".to_string();
        manifest
            .contract
            .as_mut()
            .expect("story contract")
            .structured_contract_v2 = NovelContractV2 {
            revision: 4,
            reader_promise: crate::tool::writing::novel_contract_v2::ReaderPromise {
                core_hook: "较新的合同承诺".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        ensure_story_bible_from_manifest(&mut manifest);
        manifest
            .story_bible
            .as_mut()
            .expect("story bible")
            .structured_contract_v2 = NovelContractV2 {
            revision: 99,
            reader_promise: crate::tool::writing::novel_contract_v2::ReaderPromise {
                core_hook: "派生运行态不应反向覆盖合同".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        super::super::project_governance::ensure_project_governance(&mut manifest);

        assert_eq!(manifest.structured_contract_v2.revision, 4);
        assert_eq!(
            manifest.structured_contract_v2.reader_promise.core_hook,
            "较新的合同承诺"
        );
        assert_eq!(
            manifest
                .story_bible
                .as_ref()
                .expect("story bible")
                .structured_contract_v2
                .revision,
            4
        );
    }

    #[test]
    fn story_bible_rebuild_reprojects_unapproved_chapter_contract_goals() {
        let mut manifest = test_manifest_with_primary_character();
        let now = now_iso();
        manifest.chapter_contracts.push(ChapterContractRecord {
            number: 2,
            title: "第二章".to_string(),
            path: "contracts/0002.json".to_string(),
            markdown_path: "contracts/0002.md".to_string(),
            goal: "沈砚查明星门令牌裂纹的来源。".to_string(),
            scene_goal: String::new(),
            conflict: String::new(),
            choice: String::new(),
            cost: String::new(),
            reveal: "裂纹来自被篡改的学院考核阵列。".to_string(),
            emotional_beat: String::new(),
            relationship_delta: String::new(),
            power_delta: String::new(),
            resource_delta: String::new(),
            hook_opened: Vec::new(),
            hook_paid_off: Vec::new(),
            character_change: String::new(),
            world_change: String::new(),
            payoff_target: "让第二章的调查推动主线终局。".to_string(),
            new_character_requests: Vec::new(),
            character_registrations: Vec::new(),
            status: "planned".to_string(),
            created_at: now.clone(),
            updated_at: now,
        });

        super::super::project_governance::ensure_project_governance(&mut manifest);

        let goals = &manifest
            .story_bible
            .as_ref()
            .expect("story bible")
            .narrative_graph
            .chapter_goals;
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].chapter_number, 2);
        assert_eq!(goals[0].goal, "沈砚查明星门令牌裂纹的来源。");
    }

    #[test]
    fn structured_contract_sync_recovers_authored_legacy_copy() {
        let mut manifest = test_manifest_with_primary_character();
        manifest.structured_contract_v2 = NovelContractV2::default();
        manifest
            .contract
            .as_mut()
            .expect("story contract")
            .structured_contract_v2 = NovelContractV2 {
            reader_promise: crate::tool::writing::novel_contract_v2::ReaderPromise {
                core_hook: "旧项目保留下来的读者承诺".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        super::super::project_governance::ensure_structured_contract_v2(&mut manifest);

        assert_eq!(
            manifest.structured_contract_v2.reader_promise.core_hook,
            "旧项目保留下来的读者承诺"
        );
    }

    async fn create_reviewable_test_chapter(
        tool: &NovelStudioTool,
        title: &str,
    ) -> String {
        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": title,
                    "language": "en",
                    "genre": "urban fantasy",
                    "brief": "A courier and a map keeper restore a city's damaged memory routes.",
                    "target_units": 50_000,
                    "chapter_unit_target": 2_500,
                    "contract": "Mara repairs a city memory gate and restores the first route without erasing living names.",
                    "characters": [
                        "name: Mara; role: courier; desire: restore the city memory",
                        "name: Iven; role: map keeper; desire: protect the archive"
                    ],
                    "outline": "Mara opens the damaged gate and restores the first route."
                })
                .to_string(),
            )
            .await
            .expect("init project");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"]
            .as_str()
            .expect("project path")
            .to_string();
        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara repairs a city memory gate.",
                "characters": [
                    "name: Mara; role: courier; desire: restore the city memory; fear: becoming a stored ghost; bottom_line: will not erase living people; character_id: character-mara; name_source: user",
                    "name: Iven; role: map keeper; desire: protect the archive; fear: losing the route home; bottom_line: will not trade names for power; character_id: character-iven; name_source: user"
                ],
                "world_rules": ["Memory can be stored in glass."],
                "outline": "Mara opens the damaged gate and restores the first route.",
                "reader_promise": {
                    "core_hook": "Each restored route reveals which living names the city tried to forget."
                }
            })
            .to_string(),
        )
        .await
        .expect("set contract");
        tool.call(
            &serde_json::json!({
                "action": "compose_context",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("compose context");
        tool.call(
            &serde_json::json!({
                "action": "persist_execution_package",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "The First Gate",
                "plan": "Mara repairs the first memory-gate hinge while Iven preserves the vanished names.",
                "content": "Mara reaches the gate, chooses the archive furnace route, repairs the hinge, and restores the first bell route."
            })
            .to_string(),
        )
        .await
        .expect("seal authority");
        let chapter_body = complete_english_test_body("Mara reached the broken gate before dawn, carrying the brass message tube while rain moved through the ruined market. Iven waited beside the glass map and showed her three streets that had vanished from the city's memory. Mara chose the dangerous route through the archive furnace because the missing streets still held living names. She repaired the first hinge with wire from her courier badge, earned the map keeper's trust, and promised that no stored ghost would be erased for convenience. By sunrise, the gate opened enough for both of them to hear the city remember its bells. Together they recorded the restored route before the morning market opened.");
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "The First Gate",
                "summary": "Mara and Iven reopen the first route into the city's missing memory.",
                "content": chapter_body,
                "key_facts": [
                    "Mara reaches the broken gate before dawn.",
                    "Iven shows Mara a glass map with three vanished streets.",
                    "Mara earns the map keeper's trust and repairs the first hinge."
                ],
                "continuity_updates": [
                    "Mara and Iven can enter the archive furnace route.",
                    "The city begins to remember its bells after the gate opens."
                ]
            })
            .to_string(),
        )
        .await
        .expect("write chapter");
        persist_test_best_candidate(&project_path, 1).await;
        project_path
    }

    #[tokio::test]
    async fn candidate_only_revision_does_not_replace_formal_chapter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Candidate Isolation").await;
        let chapter_path = std::path::Path::new(&project_path).join("chapters/0001.md");
        let before = std::fs::read_to_string(&chapter_path).expect("formal chapter");

        let evaluated = tool
            .call(
                &serde_json::json!({
                    "action": "revise_draft",
                    "candidate_only": true,
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "A Rejected Candidate",
                    "content": "Mara abandons every established goal without cause.",
                    "summary": "A candidate that must remain read-only."
                })
                .to_string(),
            )
            .await
            .expect("evaluate candidate");
        let evaluated: serde_json::Value =
            serde_json::from_str(&evaluated).expect("candidate json");

        assert_eq!(evaluated["candidate_only"], true);
        assert_eq!(evaluated["read_only"], true);
        assert_eq!(
            std::fs::read_to_string(&chapter_path).expect("formal chapter after evaluation"),
            before
        );
        assert!(!std::path::Path::new(&project_path).join("archives/chapters").exists());
    }

    #[tokio::test]
    async fn approval_requires_a_settlement_for_the_current_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Settlement Required").await;
        tool.call(
            &serde_json::json!({
                "action": "audit_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("audit chapter");
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "pass",
                "feedback": "The current revision is ready for settlement."
            })
            .to_string(),
        )
        .await
        .expect("review chapter");

        let approval = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approval response");
        let approval: serde_json::Value = serde_json::from_str(&approval).expect("approval json");
        assert_eq!(approval["success"], false);
        assert_eq!(
            approval["error_kind"],
            "approval_requires_state_settlement",
            "{approval}"
        );
    }

    async fn approve_reviewable_test_chapter(
        tool: &NovelStudioTool,
        project_path: &str,
    ) -> serde_json::Value {
        tool.call(
            &serde_json::json!({
                "action": "audit_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("audit chapter");
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "pass",
                "feedback": "The final body and authority dependencies are ready."
            })
            .to_string(),
        )
        .await
        .expect("review chapter");
        let settlement = tool
            .call(
                &serde_json::json!({
                    "action": "settle_chapter_state",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "content": serde_json::json!({
                        "state_changes": [],
                        "current_state": "Mara and Iven restore the first memory route.",
                        "pending_hooks": "The remaining vanished streets still need investigation.",
                        "chapter_summary": "Mara repairs the first hinge while Iven preserves the vanished names.",
                        "continuity_updates": [
                            "Mara and Iven can continue through the archive furnace route."
                        ]
                    }).to_string()
                })
                .to_string(),
            )
            .await
            .expect("settle chapter");
        let settlement: serde_json::Value =
            serde_json::from_str(&settlement).expect("settlement json");
        assert_eq!(settlement["validation"]["passed"], true, "{settlement}");

        let approval = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approve chapter");
        serde_json::from_str(&approval).expect("approval json")
    }

    #[tokio::test]
    async fn prepared_approval_before_manifest_commit_rolls_back_to_complete_before_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Prepared Rollback").await;
        let project_dir = std::path::Path::new(&project_path);
        let manifest = tool
            .read_manifest(project_dir)
            .await
            .expect("manifest before prepared transaction");
        let chapter = manifest
            .chapters
            .iter()
            .find(|chapter| chapter.number == 1)
            .expect("chapter one");
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path))
            .await
            .expect("chapter body");
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let authority = read_sealed_chapter_authority(project_dir, &manifest, 1)
            .await
            .expect("sealed authority");

        let marker = project_dir.join("runtime/approval-rollback-marker.txt");
        tokio::fs::write(&marker, "before")
            .await
            .expect("write before marker");
        let transaction_id = "prepared-before-manifest";
        let backup_path = format!(".approval-transactions/{transaction_id}/before");
        snapshot::copy_project_state_for_snapshot(project_dir, &project_dir.join(&backup_path))
            .await
            .expect("write before image");
        write_approval_journal(
            project_dir,
            &ApprovalJournal {
                transaction_id: transaction_id.to_string(),
                chapter_number: 1,
                state: ApprovalJournalState::Prepared,
                body_fingerprint: chapter_quality::chapter_body_fingerprint(&body),
                authority_fingerprint: authority.authority_root_fingerprint,
                prepared_at: now_iso(),
                committed_at: String::new(),
                receipt_path: String::new(),
                backup_path,
            },
        )
        .await
        .expect("write prepared journal");
        tokio::fs::write(&marker, "interrupted mutation")
            .await
            .expect("mutate marker");

        let recovered = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("recover prepared approval");
        let recovered: serde_json::Value =
            serde_json::from_str(&recovered).expect("recovery json");
        assert_eq!(recovered["success"], false, "{recovered}");
        assert_eq!(recovered["recovered_prepared_transaction"], true);
        assert_eq!(recovered["runtime_effect"], "artifact.rolled_back");
        assert_eq!(
            tokio::fs::read_to_string(&marker)
                .await
                .expect("restored marker"),
            "before"
        );
        assert!(read_approval_journal(project_dir, 1)
            .await
            .expect("journal after rollback")
            .is_none());
        assert!(read_approval_receipt(project_dir, 1)
            .await
            .expect("receipt after rollback")
            .is_none());
        let restored = tool
            .read_manifest(project_dir)
            .await
            .expect("manifest after rollback");
        assert!(!chapter_is_approved(
            restored
                .chapters
                .iter()
                .find(|chapter| chapter.number == 1)
                .expect("restored chapter")
        ));
    }

    #[tokio::test]
    async fn prepared_approval_after_manifest_commit_finishes_receipt_without_reapplying_truth() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Prepared Completion").await;
        let approved = approve_reviewable_test_chapter(&tool, &project_path).await;
        assert_eq!(approved["success"], true, "{approved}");

        let project_dir = std::path::Path::new(&project_path);
        let mut journal = read_approval_journal(project_dir, 1)
            .await
            .expect("read committed journal")
            .expect("committed journal");
        let transaction_id = journal.transaction_id.clone();
        let truth_before = super::super::approval_transaction::approval_truth_fingerprint(
            &tool
                .read_manifest(project_dir)
                .await
                .expect("manifest before replay"),
        );
        journal.state = ApprovalJournalState::Prepared;
        journal.committed_at.clear();
        journal.receipt_path.clear();
        write_approval_journal(project_dir, &journal)
            .await
            .expect("restore prepared journal state");
        tokio::fs::remove_file(approval_receipt_path(project_dir, 1))
            .await
            .expect("remove receipt to simulate crash");

        let recovered = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("finish prepared approval");
        let recovered: serde_json::Value =
            serde_json::from_str(&recovered).expect("recovered approval json");
        assert_eq!(recovered["success"], true, "{recovered}");
        assert_eq!(recovered["recovered_prepared_transaction"], true);
        assert_eq!(recovered["runtime_effect"], "artifact.recovered");
        assert_eq!(
            recovered["approval_receipt"]["transaction_id"],
            transaction_id
        );
        let truth_after = super::super::approval_transaction::approval_truth_fingerprint(
            &tool
                .read_manifest(project_dir)
                .await
                .expect("manifest after replay"),
        );
        assert_eq!(truth_after, truth_before);

        let mut receipt_written_journal_prepared = read_approval_journal(project_dir, 1)
            .await
            .expect("journal after recovered receipt")
            .expect("journal after recovered receipt");
        receipt_written_journal_prepared.state = ApprovalJournalState::Prepared;
        receipt_written_journal_prepared.committed_at.clear();
        receipt_written_journal_prepared.receipt_path.clear();
        write_approval_journal(project_dir, &receipt_written_journal_prepared)
            .await
            .expect("simulate receipt-before-journal crash");

        let replay = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("idempotent approval replay");
        let replay: serde_json::Value = serde_json::from_str(&replay).expect("replay json");
        assert_eq!(replay["success"], true, "{replay}");
        assert_eq!(replay["idempotent_replay"], true);
        assert_eq!(
            replay["approval_receipt"]["transaction_id"],
            transaction_id
        );
        let closed_journal = read_approval_journal(project_dir, 1)
            .await
            .expect("journal after receipt replay")
            .expect("committed journal after receipt replay");
        assert_eq!(closed_journal.state, ApprovalJournalState::Committed);
        assert_eq!(closed_journal.transaction_id, transaction_id);
        assert!(!closed_journal.committed_at.is_empty());
        assert!(!closed_journal.receipt_path.is_empty());
    }

    #[tokio::test]
    async fn chapter_revision_invalidates_prior_review_and_settlement() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Revision Binding").await;
        tool.call(
            &serde_json::json!({
                "action": "audit_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("audit chapter");
        tool.call(
            &serde_json::json!({
                "action": "settle_chapter_state",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("settle chapter");
        tool.call(
            &serde_json::json!({
                "action": "revise_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "The Bell Route",
                "summary": "Mara and Iven reopen the bell route and expose a second sealed gate.",
                "content": "Mara reached the broken gate before dawn, carrying the brass message tube while rain moved through the ruined market. Iven waited beside the glass map and showed her three streets that had vanished from the city's memory. Mara chose the dangerous route through the archive furnace because the missing streets still held living names. She repaired the first hinge with wire from her courier badge, earned the map keeper's trust, and promised that no stored ghost would be erased for convenience. When the bells returned, their echo exposed a second sealed gate beneath the market, changing the route they would take next.",
                "key_facts": ["Mara repairs the first hinge", "The bells expose a second gate"],
                "continuity_updates": ["Mara and Iven must investigate the second gate"]
            })
            .to_string(),
        )
        .await
        .expect("revise chapter");

        let approval = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approval response");
        let approval: serde_json::Value = serde_json::from_str(&approval).expect("approval json");
        assert_eq!(approval["success"], false);
        assert_eq!(approval["error_kind"], "invalid_chapter_lifecycle_transition");
    }

    #[tokio::test]
    async fn restoring_snapshot_removes_state_created_after_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Snapshot Replacement").await;
        tool.call(
            &serde_json::json!({
                "action": "snapshot",
                "project_path": project_path,
                "snapshot_id": "chapter-one"
            })
            .to_string(),
        )
        .await
        .expect("snapshot");
        seal_test_chapter_authority(&tool, &project_path, 2).await;
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "The Second Gate",
                "summary": "Mara and Iven enter the second gate.",
                "content": "Mara and Iven entered the second gate after the bells marked a safe route through the market. The glass map changed beneath Iven's hand, revealing a corridor that the city had deliberately forgotten. Mara secured the passage, recorded every living name they found, and refused the archive's offer to erase the dangerous route. Their choice preserved the corridor and gave the missing residents a path home before nightfall.",
                "key_facts": ["Mara and Iven enter the second gate"],
                "continuity_updates": ["The second corridor remains open"]
            })
            .to_string(),
        )
        .await
        .expect("second chapter");
        assert!(std::path::Path::new(&project_path)
            .join("chapters/0002.md")
            .exists());

        tool.call(
            &serde_json::json!({
                "action": "restore_snapshot",
                "project_path": project_path,
                "snapshot_id": "chapter-one"
            })
            .to_string(),
        )
        .await
        .expect("restore snapshot");

        assert!(!std::path::Path::new(&project_path)
            .join("chapters/0002.md")
            .exists());
        let manifest: serde_json::Value = serde_json::from_str(
            &tokio::fs::read_to_string(std::path::Path::new(&project_path).join("project.json"))
                .await
                .expect("manifest"),
        )
        .expect("manifest json");
        assert_eq!(manifest["chapters"].as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn project_approved_only_policy_applies_to_direct_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let project_path = create_reviewable_test_chapter(&tool, "Approved Export Policy").await;
        tool.call(
            &serde_json::json!({
                "action": "update_project",
                "project_path": project_path,
                "approved_only": true
            })
            .to_string(),
        )
        .await
        .expect("enable approved-only policy");

        let export = tool
            .call(
                &serde_json::json!({
                    "action": "export",
                    "project_path": project_path,
                    "format": "txt"
                })
                .to_string(),
            )
            .await
            .expect("export response");
        let export: serde_json::Value = serde_json::from_str(&export).expect("export json");
        assert_eq!(export["approved_only"], true);
        let content = tokio::fs::read_to_string(export["output_path"].as_str().expect("output"))
            .await
            .expect("export file");
        assert!(!content.contains("Mara reached the broken gate"));
    }

    #[tokio::test]
    async fn project_recovery_never_uses_partial_title_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        create_reviewable_test_chapter(&tool, "North Archive").await;

        let recovered = tool
            .recover_project_path_by_title("North", "data/generated/novels")
            .expect("recovery result");
        assert!(recovered.is_none());
    }

    #[tokio::test]
    async fn durable_progress_stops_at_the_first_disk_gap() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::create_dir_all(dir.path().join("chapters"))
            .await
            .expect("chapter dir");
        tokio::fs::write(
            dir.path().join("chapters/0001.md"),
            "# 第一章\n\n第一章的真实正文。",
        )
        .await
        .expect("chapter one");
        tokio::fs::write(
            dir.path().join("chapters/0003.md"),
            "# 第三章\n\n第三章不应越过缺口建立进度。",
        )
        .await
        .expect("chapter three");

        let mut manifest = test_manifest_with_primary_character();
        manifest.target_units = Some(1_000);
        manifest.chapters = [1usize, 3usize]
            .into_iter()
            .map(|number| ChapterRecord {
                number,
                title: format!("第{number}章"),
                volume_id: String::new(),
                volume_title: String::new(),
                path: format!("chapters/{number:04}.md"),
                summary: String::new(),
                unit_count: 999,
                status: "approved".to_string(),
                key_facts: Vec::new(),
                continuity_updates: Vec::new(),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .collect();

        let progress = durable_chapter_progress(dir.path(), &manifest).await;
        assert_eq!(progress.approved_prefix_chapters, 1);
        assert_eq!(progress.next_chapter, 2);
        assert_eq!(progress.first_unapproved_chapter, Some(2));
        assert!(progress.approved_prefix_units < 999);
        assert!(progress
            .blockers
            .iter()
            .any(|blocker| blocker.contains("missing from the manifest")));
        assert!(
            !durable_project_target_reached(&manifest, &progress),
            "later manifest units must not jump across a disk gap and satisfy the project target"
        );
        assert!(durable_project_completion_blockers(&manifest, &progress)
            .iter()
            .any(|blocker| blocker.contains("missing from the manifest")));
        let audit = apply_durable_progress_to_audit(
            json!({"passed": true, "blockers": [], "warnings": []}),
            &manifest,
            &progress,
        );
        assert_eq!(audit["passed"], false);
        assert!(audit["blockers"]
            .as_array()
            .is_some_and(|blockers| blockers.iter().any(|blocker| blocker
                .as_str()
                .is_some_and(|value| value.contains("missing from the manifest")))));
    }

    #[tokio::test]
    async fn durable_progress_rejects_an_approved_record_without_a_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manifest = test_manifest_with_primary_character();
        manifest.target_units = Some(1);
        manifest.chapters.push(ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: String::new(),
            unit_count: 999,
            status: "approved".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });

        let progress = durable_chapter_progress(dir.path(), &manifest).await;
        assert_eq!(progress.approved_prefix_chapters, 0);
        assert_eq!(progress.next_chapter, 1);
        assert_eq!(progress.first_unapproved_chapter, Some(1));
        assert!(!progress.blockers.is_empty());
        assert!(!durable_project_target_reached(&manifest, &progress));
    }

    #[test]
    fn prompt_context_labels_protected_and_compressible_layers() {
        let context = json!({
            "project": {"title": "测试"},
            "contract": {"premise": "必须保留的故事权威"},
            "character_ledger": [{"canonical_name": "沈砚"}],
            "continuity_anchors": {"characters": ["沈砚"]},
            "story_bible": {"hook_ledger": [{"id": "hook-0001", "title": "旧钥匙"}]},
            "truth_files": [{"section": "current_state", "content": "沈砚持有旧钥匙"}],
            "recent_chapters": [{"summary": "近期章节".repeat(500)}],
            "archives": [{"excerpt": "历史压缩层".repeat(500)}],
            "sources": [{"excerpt": "参考资料".repeat(500)}]
        });

        let prompt = build_prompt_context_payload(&context);

        assert_eq!(
            prompt
                .pointer("/contract/premise")
                .and_then(serde_json::Value::as_str),
            Some("必须保留的故事权威")
        );
        assert_eq!(
            prompt
                .pointer("/context_layers/protected/policy")
                .and_then(serde_json::Value::as_str),
            Some("reserved authority; never removed to admit reference material")
        );
        assert!(prompt
            .pointer("/context_layers/compressible/paths")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|paths| paths
                .iter()
                .any(|path| path.as_str() == Some("/sources"))));
    }
