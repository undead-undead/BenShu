    #[tokio::test]
    async fn novel_studio_persists_governance_loop_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Governed Loop",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara protects a living archive.",
                "themes": ["memory has a cost"],
                "characters": ["name: Mara; role: protagonist; desire: protect a living archive; fear: becoming only another stored memory; bottom_line: will not sacrifice a living person to preserve a record; character_id: character-mara; name_source: user"],
                "world_rules": ["Archives remember vows."],
                "style_rules": ["Advance through concrete scenes and choices rather than outline prose."],
                "outline": "Mara enters the archive and accepts a vow."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        let plan = tool
            .call(
                &serde_json::json!({
                    "action": "plan_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "The Archive",
                    "plan": "Mara enters the archive and accepts the vow.",
                    "key_facts": ["Mara accepts the vow"]
                })
                .to_string(),
            )
            .await
            .expect("plan");
        let plan: serde_json::Value = serde_json::from_str(&plan).expect("plan json");
        assert!(plan["chapter_contract"]["path"]
            .as_str()
            .expect("contract path")
            .ends_with(".contract.json"));

        let context = tool
            .call(
                &serde_json::json!({
                    "action": "compose_context",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("context");
        let context: serde_json::Value = serde_json::from_str(&context).expect("context json");
        assert!(context["context_package"]["selected_context"]
            .as_array()
            .expect("selected context")
            .iter()
            .any(|entry| entry["source"]
                .as_str()
                .unwrap_or("")
                .contains(".contract.md")));
        assert!(context["rule_stack"]["diagnostic"]
            .as_array()
            .expect("diagnostic")
            .iter()
            .any(|item| item == "truth_validation"));

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let chapter_body = complete_english_test_body("Mara enters the living archive and accepts the vow. The shelves answer with quiet light while the archive records her name. Mara repeats the vow and chooses to guard every remembered door. Archives remember vows.");
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "The Archive",
                    "summary": "Mara enters the archive.",
                    "content": chapter_body,
                    "key_facts": ["Mara accepts the vow"],
                    "continuity_updates": ["Archives remember vows"]
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        assert_eq!(draft["truth_validation"]["verdict"], "passed");
        assert!(draft["hook_debt"]["path"]
            .as_str()
            .expect("hook debt path")
            .contains("hook_debt"));

        let audit = tool
            .call(
                &serde_json::json!({
                    "action": "audit_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("audit");
        let audit: serde_json::Value = serde_json::from_str(&audit).expect("audit json");
        assert_eq!(audit["review_cycle"]["iteration"], 1);
        assert_eq!(
            audit["review_cycle"]["next_action"], "approve_chapter",
            "{audit}"
        );

        let status = tool
            .call(
                &serde_json::json!({
                    "action": "status",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("status");
        let status: serde_json::Value = serde_json::from_str(&status).expect("status json");
        assert_eq!(status["state"]["chapter_contracts"], 1);
        assert_eq!(status["state"]["context_packages"], 1);
        assert_eq!(status["state"]["truth_validations"], 1);
        assert_eq!(status["state"]["review_cycles"], 1);
        assert_eq!(status["state"]["hook_debt_reports"], 1);
        assert_eq!(status["state"]["first_unapproved_chapter"], 1);
    }

    #[tokio::test]
    async fn novel_studio_commits_settlement_truth_only_after_approval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Approval Settlement",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara protects a living archive.",
                "characters": ["name: Mara; role: protagonist; character_id: character-mara; name_source: user"],
                "world_rules": ["Archives remember vows."],
                "outline": "Mara enters the archive and accepts a vow."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let content = complete_english_test_body("Mara enters the living archive at dusk and hears the shelves answer with quiet light. The oldest door opens only after she repeats the vow in her own words, naming every remembered corridor she will protect. When the archive records her name, Mara chooses to guard the remembered doors instead of leaving with the other apprentices. The choice changes the archive's current state: Mara accepts the vow, the archive records her name, and the living shelves begin answering to her watch. Archives remember vows.");
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Mara Opens the Living Archive Door",
                "summary": "Mara enters the living archive, accepts the vow, and becomes the guard of its remembered doors.",
                    "content": &content,
                "key_facts": ["Mara accepts the vow"],
                "continuity_updates": ["Archives remember vows"]
            })
            .to_string(),
        )
        .await
        .expect("draft");
        persist_test_best_candidate(project_path, 1).await;

        let settlement_content = serde_json::json!({
            "current_state": "Mara accepts the vow and guards the living archive.",
            "pending_hooks": "The archive records her name.",
            "chapter_summary": "Mara enters the living archive and accepts the vow.",
            "continuity_updates": ["Archives remember vows"]
        })
        .to_string();
        let settlement = tool
            .call(
                &serde_json::json!({
                    "action": "settle_chapter_state",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "content": settlement_content
                })
                .to_string(),
            )
            .await
            .expect("settle");
        let settlement: serde_json::Value = serde_json::from_str(&settlement).expect("settle json");
        assert_eq!(
            settlement["commit_policy"],
            "pending_until_chapter_approval"
        );
        assert_eq!(settlement["truth_updates"].as_array().unwrap().len(), 0);

        let pre_approval_truth = tool
            .call(
                &serde_json::json!({
                    "action": "read_truth",
                    "project_path": project_path,
                    "section": "current_state"
                })
                .to_string(),
            )
            .await;
        assert!(pre_approval_truth.is_err());

        tool.call(
            &serde_json::json!({
                "action": "audit_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("audit");
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "pass",
                "feedback": "校准事实和连续性清楚。"
            })
            .to_string(),
        )
        .await
        .expect("review");

        let approve = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approve: serde_json::Value = serde_json::from_str(&approve).expect("approve json");
        assert_eq!(
            approve["success"],
            true,
            "{}",
            serde_json::to_string_pretty(&approve).unwrap()
        );
        assert_eq!(approve["truth_updates"].as_array().unwrap().len(), 3);
        assert_eq!(approve["state"]["snapshots"].as_i64(), Some(1));

        let post_approval_truth = tool
            .call(
                &serde_json::json!({
                    "action": "read_truth",
                    "project_path": project_path,
                    "section": "current_state"
                })
                .to_string(),
            )
            .await
            .expect("truth");
        let post_approval_truth: serde_json::Value =
            serde_json::from_str(&post_approval_truth).expect("truth result json");
        let current_state = post_approval_truth["content"]
            .as_str()
            .expect("current state content");
        assert!(current_state.contains("Mara"), "{current_state}");
    }

    #[tokio::test]
    async fn novel_studio_repair_project_state_blocks_corrupted_approved_chain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "状态修复",
                    "language": "zh-CN"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "黎启洄保护一艘会思考的星舰。",
                "characters": ["name: 黎启洄; role: 主角; character_id: character-li-qihui; name_source: user"],
                "world_rules": ["星舰会响应逻辑记录。"],
                "outline": "黎启洄夺回航向权限。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let body = complete_cjk_test_body(
            "黎启洄在脉冲核心过载时保持清醒，靠手动校准把频率重新压回安全区。星舰会响应逻辑记录。",
        );
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "核心频率",
                "summary": "黎启洄在脉冲核心过载时保持清醒，并用手动校准把频率压回安全区。",
                "content": &body,
                "key_facts": ["黎启洄在脉冲核心里稳定核心频率"],
                "continuity_updates": ["星舰会响应逻辑记录。"]
            })
            .to_string(),
        )
        .await
        .expect("draft");
        persist_test_best_candidate(project_path, 1).await;
        tool.call(
            &serde_json::json!({
                "action": "audit_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("audit");
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "pass",
                "feedback": "章节事实、标题和连续性均可进入批准。"
            })
            .to_string(),
        )
        .await
        .expect("pass review");
        tool.call(
            &serde_json::json!({
                "action": "settle_chapter_state",
                "project_path": project_path,
                "chapter_number": 1,
                "content": serde_json::json!({
                    "current_state": "黎启洄完成危险校准。",
                    "pending_hooks": "星舰会响应逻辑记录。",
                    "chapter_summary": "黎启洄在脉冲核心过载时保持清醒，并用手动校准把频率压回安全区，验证星舰会响应逻辑记录。",
                    "continuity_updates": ["星舰会响应逻辑记录。"]
                }).to_string()
            })
            .to_string(),
        )
        .await
        .expect("settle");

        let approved = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approved json");
        assert_eq!(approved["success"], true, "{approved}");

        let manifest_path = format!("{project_path}/project.json");
        let mut manifest_json: serde_json::Value = serde_json::from_str(
            &tokio::fs::read_to_string(&manifest_path)
                .await
                .expect("manifest before corruption"),
        )
        .expect("manifest json");
        let chapters = manifest_json["chapters"]
            .as_array_mut()
            .expect("chapters array");
        let chapter = chapters
            .iter_mut()
            .find(|chapter| chapter["number"].as_u64() == Some(1))
            .expect("chapter");
        chapter["key_facts"] = serde_json::json!([
            "黎启洄成功通过手动过载将脉冲核心的频率锁定在44.나122 kHz。",
            "星舰会响应逻辑记录。"
        ]);
        chapter["continuity_updates"] = serde_json::json!(["林墨通过手动过载成功锁定了频率。"]);
        tokio::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest_json).expect("serialize manifest"),
        )
        .await
        .expect("write corrupted manifest");
        tokio::fs::write(
            format!("{project_path}/truth/continuity-index.md"),
            "# continuity_index\n\n- 林墨通过手动过载成功锁定了频率。\n",
        )
        .await
        .expect("write corrupted continuity");
        let corrupted_manifest = tokio::fs::read_to_string(&manifest_path)
            .await
            .expect("corrupted manifest");
        let continuity_path = format!("{project_path}/truth/continuity-index.md");
        let corrupted_continuity = tokio::fs::read_to_string(&continuity_path)
            .await
            .expect("corrupted continuity");

        let repair = tool
            .call(
                &serde_json::json!({
                    "action": "repair_project_state",
                    "project_path": project_path,
                    "feedback": "黎启洄是男主，请同步项目角色权威。"
                })
                .to_string(),
            )
            .await
            .expect("repair");
        let repair: serde_json::Value = serde_json::from_str(&repair).expect("repair json");
        assert_eq!(repair["success"], false, "{repair}");
        assert_eq!(repair["runtime_effect"], "artifact.repair_blocked");
        assert_eq!(repair["old_truth_preserved"], true);
        assert_eq!(repair["next_action"], "migrate_or_repair_approval_dependencies");
        assert!(!repair["integrity_blockers"]
            .as_array()
            .expect("integrity blockers")
            .is_empty());
        let manifest = tokio::fs::read_to_string(format!("{project_path}/project.json"))
            .await
            .expect("manifest");
        let continuity = tokio::fs::read_to_string(&continuity_path)
            .await
            .expect("continuity");
        assert_eq!(manifest, corrupted_manifest);
        assert_eq!(continuity, corrupted_continuity);
    }

    #[tokio::test]
    async fn novel_studio_default_settlement_uses_verifiable_body_when_summary_is_abstract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Verifiable Settlement",
                    "language": "en",
                    "chapter_unit_target": 30
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara protects a living archive.",
                "characters": ["Mara"],
                "world_rules": ["Archives remember vows."],
                "outline": "Mara enters the archive and accepts a vow."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let body = complete_english_test_body("Mara enters the living archive and accepts the vow. The shelves answer with quiet light while the archive records her name. Mara repeats the vow and chooses to guard every remembered door. Archives remember vows.");
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "The Archive",
                "summary": "A decisive transformation begins after duty reshapes the heroine's future.",
                "content": &body,
                "key_facts": ["Mara accepts the vow"],
                "continuity_updates": ["Archives remember vows"]
            })
            .to_string(),
        )
        .await
        .expect("draft");

        let settlement = tool
            .call(
                &serde_json::json!({
                    "action": "settle_chapter_state",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("settle");
        let settlement: serde_json::Value = serde_json::from_str(&settlement).expect("settle json");
        assert_eq!(settlement["validation"]["passed"], true);
        assert_eq!(settlement["settlement_source"], "observer_degraded");
        assert_eq!(settlement["chapter_status"], "state_ready");
        assert_eq!(settlement["next_action"], "approve_chapter");
    }

    #[tokio::test]
    async fn novel_studio_run_next_chapter_returns_unapproved_chapter_before_advancing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Unapproved First",
                    "language": "en",
                    "chapter_unit_target": 30
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara protects a living archive.",
                "characters": ["Mara"],
                "world_rules": ["Archives remember vows."],
                "outline": "Mara enters the archive and accepts a vow."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "The Archive",
                "summary": "Mara enters the archive.",
                "content": "Mara enters the living archive and accepts the vow. The shelves answer with quiet light while the archive records her name. Mara repeats the vow and chooses to guard every remembered door. Archives remember vows.",
                "key_facts": ["Mara accepts the vow"],
                "continuity_updates": ["Archives remember vows"]
            })
            .to_string(),
        )
        .await
        .expect("draft");

        let packet = tool
            .call(
                &serde_json::json!({
                    "action": "run_next_chapter",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("packet");
        let packet: serde_json::Value = serde_json::from_str(&packet).expect("packet json");
        assert_eq!(packet["chapter_number"], 1);
    }

    #[tokio::test]
    async fn novel_studio_blocks_export_after_failed_drift_audit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Guarded Novel",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "A courier protects a city's ledger.",
                "characters": ["Mara"],
                "world_rules": ["The ledger cannot be rewritten by force."],
                "must_avoid": ["forbidden phrase"],
                "outline": "Mara finds the ledger and protects it."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        tool.call(
            &serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_title": "The Ledger",
                "summary": "Mara finds the ledger.",
                "content": "Mara finds the ledger, but the forbidden phrase appears in the draft.",
                "key_facts": ["Mara finds the ledger."],
                "continuity_updates": ["The ledger remains protected."]
            })
            .to_string(),
        )
        .await
        .expect("draft");

        let audit = tool
            .call(
                &serde_json::json!({
                    "action": "audit_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("audit");
        let audit_value: serde_json::Value = serde_json::from_str(&audit).expect("json");
        assert_eq!(audit_value["review"]["verdict"], "needs_revision");

        let export_err = tool
            .call(
                &serde_json::json!({
                    "action": "export",
                    "project_path": project_path,
                    "format": "txt"
                })
                .to_string(),
            )
            .await
            .expect_err("export gate");
        assert!(export_err.to_string().contains("need attention"));
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_chinese_task_english_title_and_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "青霄问道录",
                    "language": "Chinese"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "陆青在灵潮边境寻找失落道印。",
                "characters": ["name: 陆青; role: 主角"],
                "world_rules": ["灵潮会改变修士的记忆。"],
                "outline": "陆青发现异兆并逐步接近真相。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let mut english_body = "Lu Qing enters the border shrine, finds the missing seal beneath a cracked altar, and decides to follow the changing tide toward its source.".to_string();
        for index in 0..240 {
            english_body.push_str(&format!(" routeword{index:03}"));
        }
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Chapter 1: The Broken Contract",
                    "summary": "陆青发现道印。",
                    "content": english_body,
                    "key_facts": ["陆青发现道印"],
                    "continuity_updates": ["陆青继续追查灵潮"]
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        assert_eq!(draft["success"], true);
        assert_eq!(draft["recoverable"], true);
        let issues = draft["quality_gate"]["findings"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(|value| value["message"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let repaired_title = draft["chapter"]["title"].as_str().unwrap_or_default();
        assert!(repaired_title.chars().any(is_cjk_unified));
        assert!(!repaired_title.contains("Chapter"));
        assert!(
            issues.contains("no Chinese prose") || issues.contains("too much English prose"),
            "{draft}"
        );
    }

    #[tokio::test]
    async fn novel_studio_blocks_paths_outside_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let result = tool
            .call(r#"{"action":"init_project","title":"x","project_path":"../outside"}"#)
            .await
            .expect("outside path guidance");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "path_traversal");
    }

    #[tokio::test]
    async fn novel_studio_draft_approval_commits_real_project_parameters() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "writer");

        let draft = tool
            .call(
                &json!({
                    "action": "draft_project",
                    "title": "星尘渡口",
                    "language": "Chinese",
                    "genre": "玄幻",
                    "brief": "草根少年在星潮中逆袭。",
                    "target_units": 500000,
                    "chapter_unit_target": 5000,
                    "max_chapters_per_turn": 1,
                    "format": "txt",
                    "export_when_complete": true,
                    "approved_only": true,
                    "premise": "沈砚从渡口杂役开始修行。",
                    "ending_direction": "沈砚最终让星潮成为普通人也能承受的修行潮汐。",
                    "protagonist_arc": "沈砚从只想活下去的杂役，成长为愿意承担星潮代价的修行者。",
                    "world_imagery": "渡口、星潮、灵脉潮汐与夜航灯。",
                    "main_causal_spine": "星潮异变迫使沈砚追查灵脉源头，逐步触及旧秩序的代价。",
                    "title_rationale": "书名来自星潮渡口和主角跨过阶层边界的结局。",
                    "characters": [
                        "name: 沈砚; role: 主角",
                        "name: 祁岸; role: 对手"
                    ],
                    "themes": ["草根逆袭与代价选择"],
                    "world_rules": ["星潮会改变灵脉。"],
                    "style_rules": ["节奏明快。"],
                    "must_avoid": ["不要复用旧书名。"],
                    "outline": "沈砚发现星潮秘密并逐步改变命运。",
                    "emotional_contract": {
                        "primary_emotion": "压抑后的昂扬",
                        "emotional_promise": "草根角色用代价换来尊严和选择权"
                    },
                    "relationship_ledger": [{
                        "characters": ["沈砚", "祁岸"],
                        "relationship_type": "阶层压迫到正面对抗",
                        "current_state": "尚未公开冲突"
                    }],
                    "payoff_matrix": [{
                        "promise": "星潮代价会在结局被重新定义",
                        "payoff_target": "终局",
                        "status": "open"
                    }],
                    "narration_contract": {
                        "pov": "有限第三人称",
                        "chapter_pacing": "每章有目标、冲突、选择和代价"
                    }
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let draft_path = draft["draft_path"].as_str().expect("draft path");

        let approved = tool
            .call(
                &json!({
                    "action": "approve_draft",
                    "draft_path": draft_path
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approve json");
        assert_eq!(approved["success"], true, "{approved}");
        let project_path = approved["project_path"].as_str().expect("project path");
        let raw = tokio::fs::read_to_string(PathBuf::from(project_path).join("project.json"))
            .await
            .expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest json");

        assert_eq!(manifest["target_units"], 500000);
        assert_eq!(manifest["chapter_unit_target"], 5000);
        assert_eq!(manifest["max_chapters_per_turn"], 1);
        assert_eq!(manifest["export_format"], "txt");
        assert_eq!(manifest["export_when_complete"], true);
        assert_eq!(manifest["approved_only"], true);
        assert_eq!(manifest["title_state"]["locked"], true);
        assert_eq!(manifest["title_state"]["source"], "llm_contract");
        assert_eq!(
            manifest["title_state"]["rationale"],
            "书名来自星潮渡口和主角跨过阶层边界的结局。"
        );
        assert_eq!(
            manifest["structured_contract_v2"]["emotional_contract"]["emotional_promise"],
            "草根角色用代价换来尊严和选择权"
        );
        assert_eq!(
            manifest["story_bible"]["structured_contract_v2"]["payoff_matrix"][0]["promise"],
            "星潮代价会在结局被重新定义"
        );
        let characters = manifest["contract"]["characters"]
            .as_array()
            .expect("characters");
        assert!(
            characters.iter().any(|character| character
                .as_str()
                .unwrap_or_default()
                .contains("role: 主角")),
            "{characters:?}"
        );
        assert!(
            characters.iter().any(|character| {
                let character = character.as_str().unwrap_or_default();
                character.contains("role: 主角")
                    && character.contains("name_source: contract_authority")
                    && character.contains("name: 沈砚")
            }),
            "{characters:?}"
        );
        let protagonist = characters
            .iter()
            .find_map(|character| {
                let line = character.as_str()?;
                line.contains("role: 主角").then(|| {
                    line.split_once("name:")
                        .and_then(|(_, tail)| tail.split(';').next())
                        .map(str::trim)
                        .unwrap_or("")
                        .to_string()
                })
            })
            .expect("governed protagonist");
        assert!(
            serde_json::to_string(&manifest["structured_contract_v2"])
                .expect("structured")
                .contains(&protagonist),
            "{}",
            manifest["structured_contract_v2"]
        );
    }

    #[tokio::test]
    async fn novel_studio_auto_title_conflict_recovers_on_draft_approval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "writer");
        let brief = "写一部现代都市爱情小说，女主是独立插画师，男主是急诊医生。";

        let draft = tool
            .call(
                &json!({
                    "action": "draft_project",
                    "language": "Chinese",
                    "genre": "现代都市爱情",
                    "brief": brief,
                    "target_units": 50000,
                    "chapter_unit_target": 2500
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let draft_path = draft["draft_path"].as_str().expect("draft path");
        let generated_title = draft["draft"]["title"].as_str().expect("title");

        tool.call(
            &json!({
                "action": "init_project",
                "title": generated_title,
                "language": "Chinese"
            })
            .to_string(),
        )
        .await
        .expect("seed conflict");

        let approved = tool
            .call(
                &json!({
                    "action": "approve_draft",
                    "draft_path": draft_path
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approve json");

        assert_eq!(approved["success"], false);
        assert_eq!(approved["error_kind"], "draft_requires_contract_revision");
        assert!(approved["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("title must be generated")));
    }

    #[test]
    fn chapter_unit_target_is_minimum_not_upper_band() {
        let now = Utc::now().to_rfc3339();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "测试小说".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "玄幻".to_string(),
            brief: "测试".to_string(),
            target_units: None,
            chapter_unit_target: Some(300),
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
        let chapter = ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "主角出场。".to_string(),
            unit_count: 585,
            status: "draft".to_string(),
            key_facts: vec!["主角出场。".to_string()],
            continuity_updates: vec!["主角开始行动。".to_string()],
            created_at: now.clone(),
            updated_at: now,
        };

        let issues = mechanical_chapter_issues(&manifest, &chapter, "主角出场并开始行动。");
        assert!(
            issues.iter().all(|issue| !issue.contains("length")),
            "{issues:?}"
        );
    }

    #[test]
    fn chapter_unit_target_marks_one_unit_shortfall_for_bounded_repair() {
        let now = Utc::now().to_rfc3339();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "测试小说".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "玄幻".to_string(),
            brief: "测试".to_string(),
            target_units: None,
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
        let chapter = ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "主角出场。".to_string(),
            unit_count: 0,
            status: "draft".to_string(),
            key_facts: vec!["主角出场。".to_string()],
            continuity_updates: vec!["主角开始行动。".to_string()],
            created_at: now.clone(),
            updated_at: now,
        };

        let content = "章".repeat(2_499);
        let issues = mechanical_chapter_issues(&manifest, &chapter, &content);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("chapter length is below the soft target: 2499 of 2500")),
            "{issues:?}"
        );
    }

    #[test]
    fn chapter_unit_tiers_allow_double_target_but_block_one_unit_above_it() {
        let now = Utc::now().to_rfc3339();
        let mut manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "测试小说".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "玄幻".to_string(),
            brief: "测试".to_string(),
            target_units: None,
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
        let chapter = ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "主角出场。".to_string(),
            unit_count: 5_000,
            status: "draft".to_string(),
            key_facts: vec!["主角出场。".to_string()],
            continuity_updates: vec!["主角开始行动。".to_string()],
            created_at: now.clone(),
            updated_at: now,
        };
        let content = "章".repeat(5_000);

        let at_2500_cap = mechanical_chapter_issues(&manifest, &chapter, &content);
        assert!(at_2500_cap
            .iter()
            .all(|issue| !issue.contains("exceeds maximum")));
        let content = "章".repeat(5_001);
        let above_2500_cap = mechanical_chapter_issues(&manifest, &chapter, &content);
        assert!(above_2500_cap
            .iter()
            .any(|issue| issue.contains("2500-unit chapters may not exceed 5000 units")));

        manifest.chapter_unit_target = Some(5_000);
        let content = "章".repeat(10_000);
        let at_5000_cap = mechanical_chapter_issues(&manifest, &chapter, &content);
        assert!(at_5000_cap
            .iter()
            .all(|issue| !issue.contains("exceeds maximum")));
        let content = "章".repeat(10_001);
        let above_5000_cap = mechanical_chapter_issues(&manifest, &chapter, &content);
        assert!(above_5000_cap
            .iter()
            .any(|issue| issue.contains("5000-unit chapters may not exceed 10000 units")));
    }

    #[test]
    fn chapter_metadata_fallback_derives_key_facts_from_body() {
        let mut chapter = ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: String::new(),
            unit_count: 120,
            status: "draft".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        let content = "陶见微在矿井深处捡到一枚会发热的铜钱，他第一次意识到自己能看见灵脉。随后他把铜钱藏进鞋底，避开管事搜查。";

        let manifest = test_manifest_with_primary_character();
        ensure_chapter_key_facts(&manifest, &mut chapter, content);
        ensure_chapter_continuity_updates(&manifest, &mut chapter, content);

        assert!(!chapter.key_facts.is_empty());
        assert!(!chapter.continuity_updates.is_empty());
    }

    #[test]
    fn metadata_normalization_drops_unsupported_foreign_script_truth() {
        let now = now_iso();
        let manifest = NovelProjectManifest {
            schema_version: "1".to_string(),
            title: "裂痕证词".to_string(),
            title_state: TitleState::default(),
            language: "Chinese".to_string(),
            genre: "都市玄幻".to_string(),
            brief: "城市现实规则偏移。".to_string(),
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
        let content = "钟知禾走过旧书店时，看见电线杆的影子像墨水一样流动。她确认现实逻辑正在偏移，余烬气息留在空气里。";
        let mut chapter = ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "钟知禾确认现实逻辑正在偏移。".to_string(),
            unit_count: 120,
            status: "draft".to_string(),
            key_facts: vec!["现实逻辑的偏移与“余나”能量有关。".to_string()],
            continuity_updates: vec!["现实逻辑的偏移与“余나”能量有关。".to_string()],
            created_at: now.clone(),
            updated_at: now,
        };

        normalize_chapter_metadata_against_body(&manifest, &mut chapter, content);

        assert!(chapter
            .key_facts
            .iter()
            .chain(chapter.continuity_updates.iter())
            .all(|item| !item.contains('나')));
        let validation = governance::validate_truth_against_chapter(
            chapter.number,
            content,
            &chapter.key_facts,
            &chapter.continuity_updates,
            now_iso(),
        );
        assert_eq!(validation.verdict, "passed", "{:?}", validation.issues);
    }

    #[test]
    fn unit_count_ignores_omission_placeholder_lines() {
        let content = "真实正文一。\n（此处省略约3800字，后续剧情展开。）\n真实正文二。";

        let units = count_units(content, "Chinese");
        let expected = count_units("真实正文一。\n真实正文二。", "Chinese");

        assert_eq!(units, expected);
        assert!(units < 30);
    }

    #[test]
    fn chinese_script_noise_sanitizer_removes_isolated_foreign_marks() {
        let content = "林晚把合同合上 ี，她没有立刻回答。";

        let cleaned = strip_isolated_unexpected_scripts_from_chinese_text(content);

        assert_eq!(cleaned, "林晚把合同合上 ，她没有立刻回答。");
        assert!(!cleaned.contains('ี'));
    }

    #[test]
    fn chinese_markup_residue_sanitizer_removes_latex_arrow_and_escape_fragments() {
        let content =
            "ightarrow$ 硬件温度升高 $\n$ $\n\\ l她知道风险正在扩大。\n\\来看，她已经没有退路。\n她没有回头路了。\\ l";

        let cleaned = strip_chinese_markup_residue_lines(content);

        assert!(cleaned.contains("硬件温度升高"));
        assert!(cleaned.contains("她知道风险正在扩大。"));
        assert!(cleaned.contains("来看，她已经没有退路。"));
        assert!(cleaned.contains("她没有回头路了。"));
        assert!(!cleaned.contains("ightarrow"));
        assert!(!cleaned.contains('$'));
        assert!(!cleaned.contains("\\ l"));
        assert!(!cleaned.contains("\\来看"));
    }

    #[test]
    fn chapter_title_rejects_bare_body_action_phrase() {
        assert!(cjk_title_core_has_prose_grammar_fragment("站起身"));
        assert!(cjk_title_core_has_prose_grammar_fragment("抬起头"));
        assert!(cjk_title_core_has_prose_grammar_fragment("仿佛连星"));
        assert!(cjk_title_core_has_prose_grammar_fragment("似乎有光"));
        assert!(cjk_title_core_has_prose_grammar_fragment("城市如同"));
        assert!(cjk_title_core_has_prose_grammar_fragment("古剑身泛"));
        assert!(cjk_title_core_has_prose_grammar_fragment("灯盏身亮"));
        assert!(!cjk_title_core_has_prose_grammar_fragment("镇灵大阵"));
        assert!(!cjk_title_core_has_prose_grammar_fragment("断桥驿站"));
    }

    #[test]
    fn readable_export_sanitizer_repairs_single_cjk_stutter_without_breaking_boundaries() {
        let cleaned =
            sanitize_readable_chapter_body("纯白折射光构构成裂缝，而几何结构构成实体。", "Chinese");

        assert!(cleaned.contains("纯白折射光构成裂缝"));
        assert!(cleaned.contains("几何结构构成实体"));
        assert!(!cleaned.contains("光构构成"));
    }

    #[test]
    fn readable_export_sanitizer_preserves_normal_cjk_reduplication_words() {
        let cleaned = sanitize_readable_chapter_body(
            "他跌跌撞撞地跑进雨中，喃喃自语，密密麻麻的金色纹路在皮肤下浮现。",
            "Chinese",
        );

        assert!(cleaned.contains("跌跌撞撞地跑进雨中"), "{cleaned}");
        assert!(cleaned.contains("喃喃自语"), "{cleaned}");
        assert!(cleaned.contains("密密麻麻"), "{cleaned}");
    }

    #[test]
    fn prose_surface_gate_flags_markup_math_residue() {
        let issues = prose_surface_contamination_issues("ightarrow$ 硬件温度升高 $");

        assert!(issues
            .iter()
            .any(|issue| issue.contains("markup/math residue")));
        let issues = prose_surface_contamination_issues("\\来看，她已经没有退路。");
        assert!(issues
            .iter()
            .any(|issue| issue.contains("markup/math residue")));
        let issues = prose_surface_contamination_issues("她没有回头路了。\\ l");
        assert!(issues
            .iter()
            .any(|issue| issue.contains("markup/math residue")));
    }

    #[test]
    fn prose_surface_gate_flags_high_confidence_missing_character_fragments() {
        let issues =
            prose_surface_contamination_issues("陶棠澜觉得那条线索隐藏着什的直觉，像雨水悄蔓延。");

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("missing-character fragment")),
            "{issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_allows_natural_tactile_feel_wording() {
        let issues = prose_surface_contamination_issues(
            "指尖传来的触感温热、柔软且富有弹性，像某种仍在呼吸的深海组织。",
        );

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("missing 感觉/感到 wording")),
            "natural tactile noun phrase should not be treated as malformed: {issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_preserves_normal_cjk_missing_fragment_targets() {
        let issues = prose_surface_contamination_issues(
            "钟望宁猛地回头，发现暗潮正在悄悄蔓延，心脏也跟着突突直跳。",
        );

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("missing-character fragment")),
            "{issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_flags_unfinished_final_line() {
        let issues = prose_surface_contamination_issues(
            "钟望宁终于看清城市里的符文。\n「保护这座城市，」阮砚晚回答，",
        );

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("appears unfinished")),
            "{issues:?}"
        );

        let half_sentence_issues =
            prose_surface_contamination_issues("韩砚舟终于踏入天阙深处。\n因为从这一刻起，他");
        assert!(
            half_sentence_issues
                .iter()
                .any(|issue| issue.contains("no terminal punctuation")),
            "{half_sentence_issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_accepts_terminal_punctuation_before_ascii_closing_quote() {
        let issues = prose_surface_contamination_issues(
            "叶清序抬手示意，目光锁定在前方的警示灯上，\"谢启宁的人就在那边布控。\"",
        );

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("no terminal punctuation")),
            "{issues:?}"
        );
    }

    #[test]
    fn prose_surface_gate_flags_anchor_missing_character_fragments() {
        let issues = prose_surface_contamination_issues(
            "钟望宁孔剧烈收缩。阮砚晚光坚定。钟望宁吸变得急促，他终于明白为什会被追杀。",
        );

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("missing-character fragment")),
            "{issues:?}"
        );
    }

    #[test]
    fn cjk_structural_gate_flags_high_confidence_lexical_glue() {
        let issues = cjk_malformed_structural_phrase_issues(
            "印章是青玉材质地温润。她手里夹着一支细长的香烟雾缭绕中。",
        );

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("malformed lexical glue")),
            "{issues:?}"
        );
    }

    #[test]
    fn cjk_layout_does_not_treat_character_possessive_as_story_term() {
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "炼古剑诀".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "异界修仙".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
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
            contract: Some(StoryContract {
                premise: "闻澈川追查古剑封印。".to_string(),
                themes: vec!["代价与选择".to_string()],
                characters: vec![
                    "name: 闻澈川; role: 主角; desire: 查清古剑真相".to_string(),
                    "name: 辛澈砺; role: 对手; desire: 守住封印".to_string(),
                ],
                world_rules: vec!["古剑封印会回应剑修道心。".to_string()],
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: "闻澈川在遗迹中逐步看清古剑封印的真相。".to_string(),
                structured_contract_v2: NovelContractV2::default(),
                authority_contract: None,
                updated_at: Utc::now().to_rfc3339(),
            }),
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let content = [
            "闻澈川的手指按在古剑纹路上，辛澈砺的剑锋停在石阶前。",
            "闻澈川的呼吸慢慢稳住，他把符纹的变化记在心里。",
            "闻澈川的判断不再只靠蛮力，而是从封印回响里寻找破绽。",
            "闻澈川的选择让辛澈砺沉默，也让遗迹深处出现新的裂光。",
            "闻澈川的剑没有立刻斩出，他先逼自己看清代价。",
            "闻澈川的目光越过断壁，终于确认古剑并非普通传承。",
            "闻澈川的掌心渗血，却没有再后退。",
            "闻澈川的心神被封印拉扯，但他仍然守住了底线。",
        ]
        .join("");

        let issues = narrative_substance_issues(&manifest, &content);
        assert!(
            !issues.iter().any(|issue| issue.contains("闻澈川的")),
            "possessive grammar fragments should not be overused story terms: {issues:?}"
        );
    }

    #[test]
    fn cjk_layout_flags_repeated_concept_without_counting_character_name_parts() {
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "炼古剑诀".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "异界修仙".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
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
            contract: Some(StoryContract {
                premise: "闻澈川追查古剑封印。".to_string(),
                themes: vec!["代价与选择".to_string()],
                characters: vec![
                    "name: 闻澈川; role: 主角; desire: 查清古剑真相".to_string(),
                    "name: 辛澈砺; role: 对手; desire: 守住封印".to_string(),
                ],
                world_rules: vec!["古剑封印会回应剑修道心。".to_string()],
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: "闻澈川在遗迹中逐步看清古剑封印的真相。".to_string(),
                structured_contract_v2: NovelContractV2::default(),
                authority_contract: None,
                updated_at: Utc::now().to_rfc3339(),
            }),
            snapshots: Vec::new(),
            style_profiles: Vec::new(),
            volumes: Vec::new(),
            volume_summaries: Vec::new(),
            character_ledger: Vec::new(),
            story_bible: None,
            structured_contract_v2: NovelContractV2::default(),
        };
        let repeated = "闻澈川调整呼吸，剑意沿着古剑纹路震动，他没有急着出手，而是观察石壁回声。";
        let content = (0..24)
            .map(|_| repeated)
            .collect::<Vec<_>>()
            .join("");
        let issues = narrative_substance_issues(&manifest, &content);

        assert!(
            issues.iter().any(|issue| issue.contains("剑意")),
            "repeated non-character concept should be flagged: {issues:?}"
        );
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("named concept") && issue.contains("闻澈")),
            "character name substrings should not be flagged as concepts: {issues:?}"
        );
    }

    #[test]
    fn cjk_layout_flags_repeated_rhetorical_marker() {
        let mut manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "雨城之约".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "原创小说".to_string(),
            brief: String::new(),
            target_units: Some(50_000),
            chapter_unit_target: Some(2_500),
            max_chapters_per_turn: Some(1),
            export_format: Some("txt".to_string()),
            export_when_complete: true,
            approved_only: true,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
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
        manifest.contract = Some(StoryContract {
            premise: "主角在雨城寻找失踪的旧友。".to_string(),
            themes: vec!["记忆与选择".to_string()],
            characters: vec!["name: 岑雨棠; role: 主角".to_string()],
            world_rules: Vec::new(),
            style_rules: Vec::new(),
            must_avoid: Vec::new(),
            outline: "岑雨棠在雨城逐步接近旧友失踪真相。".to_string(),
            structured_contract_v2: NovelContractV2::default(),
            authority_contract: None,
            updated_at: Utc::now().to_rfc3339(),
        });
        let repeated = "岑雨棠穿过街口，仿佛听见旧友在雨声里回应，她停下脚步，重新确认那枚钥匙的重量。";
        let content = (0..18)
            .map(|_| repeated)
            .collect::<Vec<_>>()
            .join("");
        let issues = narrative_substance_issues(&manifest, &content);

        assert!(
            issues.iter().any(|issue| issue.contains("仿佛")),
            "repeated rhetorical marker should be flagged: {issues:?}"
        );
    }
