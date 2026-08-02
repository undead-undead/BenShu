    #[test]
    fn cjk_layout_allows_chapter_heading_spacing() {
        let manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "测试小说".to_string(),
            title_state: TitleState::default(),
            language: "zh-CN".to_string(),
            genre: "玄幻".to_string(),
            brief: String::new(),
            target_units: None,
            chapter_unit_target: None,
            max_chapters_per_turn: None,
            export_format: None,
            export_when_complete: false,
            approved_only: false,
            created_at: now_iso(),
            updated_at: now_iso(),
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
            delivery_advisory_windows: Vec::new(),
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
        assert!(cjk_layout_issues(&manifest, "# 第一章 灰烬之城的裂痕\n\n正文开始。").is_empty());
        assert!(cjk_layout_issues(&manifest, "第一章 灰烬之城的裂痕\n\n正文开始。").is_empty());
        assert!(!cjk_layout_issues(&manifest, "林霄发现灵渊吞 量异常。").is_empty());
    }

    #[test]
    fn novel_studio_sanitizes_contract_cjk_separator_noise() {
        assert_eq!(
            sanitize_contract_text("玄_幻世界需要守恒"),
            "玄幻世界需要守恒"
        );
        assert_eq!(sanitize_contract_text("法^则崩毁"), "法则崩毁");
        assert_eq!(sanitize_contract_text("A_B 测试"), "A_B 测试");
        assert_eq!(
            sanitize_contract_text("季景白确认阿离是关键。；屋内铜镜破裂。"),
            "季景白确认阿离是关键。屋内铜镜破裂。"
        );
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_recovery_placeholder_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                r#"{"action":"init_project","title":"Recovery Placeholder","language":"Chinese"}"#,
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "characters": ["陆弦: 主角"],
                "world_rules": ["天体余音可以被听律者感知"]
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "第一章",
                    "content": "陆弦站在星轨之下。此处应为第一章的具体正文内容。由于当前处于恢复阶段，请根据项目设定生成符合要求的正文。"
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");

        assert_eq!(draft["runtime_effect"], "artifact.needs_revision");
        assert_eq!(draft["quality_gate"]["passed"], false);
        assert!(draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("placeholder or omission marker")));
    }

    #[tokio::test]
    async fn novel_studio_init_project_explicit_existing_project_is_reusable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Reusable",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let reused = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Reusable",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("reuse");
        let reused: serde_json::Value = serde_json::from_str(&reused).expect("reuse json");
        assert_eq!(reused["success"], true);
        assert_eq!(reused["reused_existing"], true);
        assert_eq!(reused["project_path"], project_path);
    }

    #[tokio::test]
    async fn novel_studio_recovers_title_style_project_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "《岁律劫：时律之主》",
                    "language": "zh"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let written = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Opening",
                    "content": "A first chapter exists."
                })
                .to_string(),
            )
            .await
            .expect("write");
        let written: serde_json::Value = serde_json::from_str(&written).expect("write json");
        assert_eq!(written["success"], true);

        let read = tool
            .call(
                &serde_json::json!({
                    "action": "read_chapter",
                    "project_path": "岁律劫_时律之主",
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("read");
        let read: serde_json::Value = serde_json::from_str(&read).expect("read json");
        assert_eq!(read["success"], true);
        assert_eq!(read["project_path"], project_path);
        assert_eq!(read["runtime_effect"], "artifact.verified");
        assert!(read["artifact_path"]
            .as_str()
            .expect("artifact path")
            .ends_with(".md"));
        assert!(read["content"]
            .as_str()
            .expect("content")
            .contains("A first chapter exists."));
    }

    #[tokio::test]
    async fn novel_studio_run_next_chapter_returns_writer_packet_not_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Continuity Packet",
                    "language": "zh",
                    "target_units": 10000,
                    "chapter_unit_target": 1000
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
                "premise": "少年发现城中刻印正在吞噬记忆。",
                "themes": ["记忆、选择与代价"],
                "characters": [
                    "name: 陆沉; role: 主角; desire: 查清刻印真相; fear: 失去自己的记忆; bottom_line: 不牺牲无辜者换取答案",
                    "name: 苏薇; role: 同伴; desire: 守住家族秘密; fear: 被议会利用; bottom_line: 不背叛真正救过她的人"
                ],
                "world_rules": ["刻印会保存也会篡改记忆。"],
                "style_rules": ["以场景和行动推进，不用设定说明替代正文。"],
                "outline": "发现异兆、追查源头、面对议会。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        let packet = tool
            .call(
                &serde_json::json!({
                    "action": "run_next_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("packet");
        let packet: serde_json::Value = serde_json::from_str(&packet).expect("packet json");
        assert_eq!(packet["success"], true);
        assert_eq!(packet["stage"], "draft");
        assert_eq!(packet["next_action"], "write_draft");
        assert_eq!(packet["runtime_effect"], "artifact.checkpointed");
        assert!(packet.get("error_kind").is_none());
        assert_eq!(
            packet["writing_phase"]["content_submission"]["tool"],
            "novel_studio"
        );
        assert_eq!(
            packet["writing_phase"]["content_submission"]["args"]["action"],
            "write_draft"
        );
        assert!(
            packet["writing_phase"]["content_submission"]["required_fields"]
                .as_array()
                .expect("required fields")
                .iter()
                .any(|value| value == "content")
        );
    }

    #[tokio::test]
    async fn novel_studio_recovers_managed_project_from_damaged_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Cloud Trial",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        let project_name = Path::new(project_path)
            .file_name()
            .and_then(|name| name.to_str())
            .expect("project name");

        let damaged_path = format!("/outside/workspace/data/generated/novels/{project_name}");
        seal_test_chapter_authority(&tool, project_path, 1).await;
        let written = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": damaged_path,
                    "chapter_number": 1,
                    "chapter_title": "Opening",
                    "content": "A draft survives a damaged absolute project path."
                })
                .to_string(),
            )
            .await
            .expect("write");
        let written: serde_json::Value = serde_json::from_str(&written).expect("write json");
        assert_eq!(written["success"], true);
        assert_eq!(written["project_path"], project_path);
    }

    #[tokio::test]
    async fn novel_studio_recovers_damaged_project_path_with_nullable_lists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Path Recovery Nullable Lists",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        let project_name = Path::new(project_path)
            .file_name()
            .and_then(|name| name.to_str())
            .expect("project name");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Opening",
                "content": "A chapter exists before a later call carries a damaged path."
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        let damaged_path =
            format!("/outside/workspace/garbled한글/data/generated/novels/{project_name}");
        let revised = tool
            .call(
                &serde_json::json!({
                    "action": "revise_chapter",
                    "project_path": damaged_path,
                    "chapter_number": 1,
                    "summary": "Recovered through managed project lookup.",
                    "key_facts": null,
                    "continuity_updates": null
                })
                .to_string(),
            )
            .await
            .expect("revise with damaged path");
        let revised: serde_json::Value = serde_json::from_str(&revised).expect("revised json");

        assert_eq!(revised["success"], true);
        assert_eq!(revised["project_path"], project_path);
        assert_eq!(
            revised["chapter"]["summary"],
            "Recovered through managed project lookup."
        );
    }

    #[tokio::test]
    async fn novel_studio_recovers_project_from_output_root_and_repairs_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Output Root Recovery",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tokio::fs::remove_dir_all(Path::new(project_path).join("plans"))
            .await
            .expect("remove plans");

        let plan = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter_plan",
                    "output_root": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Opening",
                    "plan": "The protagonist commits to the journey."
                })
                .to_string(),
            )
            .await
            .expect("plan via output_root");
        let plan: serde_json::Value = serde_json::from_str(&plan).expect("plan json");
        assert_eq!(plan["success"], true);
        assert_eq!(plan["project_path"], project_path);
        assert!(Path::new(project_path).join("plans").is_dir());
    }

    #[tokio::test]
    async fn novel_studio_architect_chapter_can_use_existing_plan() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Plan Fallback","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter_plan",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Opening",
                "plan": "Scene one establishes the conflict. Scene two forces a choice."
            })
            .to_string(),
        )
        .await
        .expect("plan");

        let architecture = tool
            .call(
                &serde_json::json!({
                    "action": "architect_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("architecture");
        let architecture: serde_json::Value =
            serde_json::from_str(&architecture).expect("architecture json");
        assert_eq!(architecture["success"], true);
        assert_eq!(architecture["stage"], "chapter_execution_package");
        assert_eq!(
            architecture["writing_phase"]["content_submission"]["args"]["action"],
            "write_draft"
        );
        assert_eq!(architecture["writing_phase"]["phase"], "draft");
        assert!(architecture["progress_report_contract"]
            ["chat_should_not_include"]
            .as_array()
            .expect("chat hidden fields")
            .iter()
            .any(|value| value == "complete long-form body"));
        assert!(architecture["chapter_architecture"]["architecture"]
            .as_str()
            .expect("architecture text")
            .contains("Scene one"));
    }

    #[tokio::test]
    async fn novel_studio_architect_chapter_defaults_to_first_unarchitected_plan() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Plan Continuation","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter_plan",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Opening",
                "plan": "The opening plan should be used without repeating chapter_number."
            })
            .to_string(),
        )
        .await
        .expect("plan");

        let context = tool
            .call(
                &serde_json::json!({
                    "action": "compose_context",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("context");
        let context: serde_json::Value = serde_json::from_str(&context).expect("context json");
        assert_eq!(context["chapter_number"], 1);
        assert_eq!(context["next_action"], "generate_chapter_execution_package");

        let architecture = tool
            .call(
                &serde_json::json!({
                    "action": "architect_chapter",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("architecture");
        let architecture: serde_json::Value =
            serde_json::from_str(&architecture).expect("architecture json");
        assert_eq!(architecture["success"], true);
        assert_eq!(architecture["chapter_number"], 1);
        assert_eq!(architecture["next_action"], "write_draft");
        assert_eq!(
            architecture["writing_phase"]["content_submission"]["args"]["chapter_number"],
            1
        );
        assert!(architecture["chapter_architecture"]["architecture"]
            .as_str()
            .expect("architecture text")
            .contains("opening plan"));
    }

    #[tokio::test]
    async fn compose_context_excludes_current_and_unapproved_chapter_drafts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Context Isolation","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let requested_approved: serde_json::Value = serde_json::from_str(
            &tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Approved Prior",
                "content": "Approved anchor chapter text with enough concrete continuity.",
                "summary": "Approved prior summary",
                "status": "approved"
            })
            .to_string(),
        )
        .await
        .expect("requested prior chapter"),
        )
        .expect("requested prior chapter json");
        assert_ne!(requested_approved["chapter"]["status"], "approved");
        let mut manifest = tool
            .read_manifest(std::path::Path::new(project_path))
            .await
            .expect("manifest");
        manifest
            .chapters
            .iter_mut()
            .find(|chapter| chapter.number == 1)
            .expect("fixture chapter")
            .status = "approved".to_string();
        tool.write_manifest(std::path::Path::new(project_path), &manifest)
            .await
            .expect("write approved fixture");
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "Rejected Current",
                "content": "Rejected current draft should never be fed back as context.",
                "summary": "Rejected current summary",
                "status": "needs_revision"
            })
            .to_string(),
        )
        .await
        .expect("current needs revision chapter");

        let context = tool
            .call(
                &serde_json::json!({
                    "action": "compose_context",
                    "project_path": project_path,
                    "chapter_number": 2
                })
                .to_string(),
            )
            .await
            .expect("context");
        let context: serde_json::Value = serde_json::from_str(&context).expect("context json");
        let recent = context["context"]["recent_chapters"]
            .as_array()
            .expect("recent chapters");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0]["number"], 1);
        let selected_sources = context["trace"]["selected_sources"]
            .as_array()
            .expect("selected sources")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert!(selected_sources.contains(&"chapters/0001.md"));
        assert!(!selected_sources.contains(&"chapters/0002.md"));
    }

    #[tokio::test]
    async fn low_level_add_revise_and_import_cannot_bypass_canonical_approval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init: serde_json::Value = serde_json::from_str(
            &tool
                .call(r#"{"action":"init_project","title":"Approval Boundary","language":"en"}"#)
                .await
                .expect("init"),
        )
        .expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let added: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "add_chapter",
                        "project_path": project_path,
                        "chapter_number": 1,
                        "chapter_title": "Boundary",
                        "content": "A concrete draft remains unapproved until canonical approval.",
                        "summary": "A draft tests the approval boundary.",
                        "status": "approved"
                    })
                    .to_string(),
                )
                .await
                .expect("add"),
        )
        .expect("add json");
        assert_ne!(added["chapter"]["status"], "approved");

        let revised: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "revise_chapter",
                        "project_path": project_path,
                        "chapter_number": 1,
                        "content": "The revised concrete draft still requires canonical approval.",
                        "summary": "The revision preserves the approval boundary.",
                        "status": "approved"
                    })
                    .to_string(),
                )
                .await
                .expect("revise"),
        )
        .expect("revise json");
        assert_ne!(revised["chapter"]["status"], "approved");

        let imported: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "import_chapters",
                        "project_path": project_path,
                        "content": "Chapter 2 Imported\\nImported prose remains outside durable truth.",
                        "status": "approved"
                    })
                    .to_string(),
                )
                .await
                .expect("import"),
        )
        .expect("import json");
        assert!(imported["imported_chapters"]
            .as_array()
            .expect("imported chapters")
            .iter()
            .all(|chapter| chapter["status"] == "imported_unverified"));
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_duplicate_chapter_surface() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "重复检测项目",
                    "language": "zh",
                    "target_units": 5000
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
                "premise": "闻庭安在城市废墟中寻找失落信标。",
                "characters": ["name: 闻庭安; role: 主角; desire: 找回信标; fear: 失去同伴; bottom_line: 不牺牲无辜者; 用户指定"],
                "world_rules": ["信标会改变城市能源流向。"],
                "style_rules": ["用具体场景推动剧情。"],
                "outline": "发现信标、追索真相、完成选择。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        let content = [
            "暴雨把废弃车站冲成一条暗河，闻庭安蹲在售票亭残墙后，听见失落信标在铁轨深处发出细小的回声。",
            "他没有立刻靠近，而是先把三枚旧电池接入检测环，让蓝色指针沿着裂缝一点点爬向地下配电室。",
            "同伴阿洛压低声音提醒追兵已经进站，闻庭安却指了指天花板坠落的广告牌，示意她把反光面转向北侧。",
            "第一束探灯扫过站台时，广告牌反射出一片假影，追兵朝空荡候车区开火，真正的入口因此露出半秒空隙。",
            "闻庭安滑进检修井，掌心被锈铁划出血线，血滴落在线缆上，信标的脉冲忽然变得像心跳一样清晰。",
            "他意识到这东西不是单纯的能源核心，而是在改变整座废城的供电记忆，谁掌握它，谁就能重写城市路线。",
            "阿洛从上方递下绳索，语气里第一次带了迟疑：如果信标真能重写路线，失踪的人也许不是死了，而是被城市藏起来了。",
            "闻庭安想起三年前消失在南环隧道的妹妹，胸口像被冷水灌满，却仍然逼自己先记录频率，不让情绪盖过判断。",
            "追兵的脚步声压到井口，领头人报出他的名字，准确到连他少年时用过的假姓都没有漏掉。",
            "这说明对方不是临时追踪，而是早就把他放进了某张名单；信标只是把那张名单提前照亮。",
            "闻庭安拆下配电盒外壳，把检测环改成短路诱饵，随后用一段铜丝把脉冲引向废弃扶梯。",
            "扶梯轰然启动，停摆多年的台阶像铁浪一样卷起，追兵被迫后退，整座车站也随之亮起断断续续的白灯。",
            "灯光照见墙上的旧地图，地图上南环隧道的位置被人用红漆圈出，旁边还有一串只有他妹妹会写的简码。",
            "阿洛看见他停住，伸手按住他的肩，提醒他信标的回声正在变强，如果再拖延，地下主线会彻底过载。",
            "闻庭安把简码拍进腕表，没有解释，只把主线保险拔掉一半，给信标留下可控的残余电流。",
            "这一刀让车站陷入半暗，也让城市能源流出现短暂断层，追兵的定位器同时失效，骂声在黑暗里乱成一团。",
            "他趁乱带着阿洛穿过货运通道，通道尽头有一扇被焊死的门，门缝里透出和信标同频的微光。",
            "阿洛用爆破钉切开门锁，门后不是设备间，而是一间保存完整的临时教室，桌上还摆着十几本学生档案。",
            "闻庭安翻开第一本，发现里面记录的不是成绩，而是每个人对城市脉冲的承受极限和记忆偏移程度。",
            "他的妹妹排在第三页，状态栏写着“未回收”，日期正是南环隧道事故后的第七天。",
            "那一瞬间，信标在他背包里震动，像是回应档案里的名字，整间教室的灯管也依次亮起。",
            "墙角扬声器传出早已失真的女声，通知所有实验对象返回原位，违抗者将被城市防卫系统标记。",
            "闻庭安没有被恐惧压住，他反而更冷静，开始把档案编号和地图节点对应起来，确认这不是鬼故事，而是一套仍在运行的系统。",
            "追兵重新逼近时，他把三本关键档案塞给阿洛，让她先从通风管离开，自己留下处理信标过载。",
            "阿洛不肯走，问他是不是又打算一个人扛下所有后果，闻庭安第一次没有逞强，只说需要她把证据带出去。",
            "这句话让阿洛沉默两秒，随后她点头，把自己的备用通讯扣在他袖口，约定十分钟后在北闸门汇合。",
            "闻庭安回到配电台前，把信标嵌入老旧接口，屏幕跳出一行问题：是否恢复南环路线。",
            "他知道只要按下确认，妹妹可能出现，追兵也可能顺着恢复路线找到所有失踪者；这是诱惑，也是陷阱。",
            "于是他没有选择恢复，而是输入反向查询，让系统列出最近一次被手动隐藏的路线修改记录。",
            "屏幕停顿片刻，吐出一个熟悉的徽记：城防署第九实验组，负责人景望川。",
            "景望川正是追兵领头人的声音来源，也是三年前亲自宣布南环事故无人生还的人。",
            "闻庭安把记录复制进通讯扣，随即拔掉信标，主控台发出尖锐警报，整座车站开始进入封锁倒计时。",
            "他冲向北闸门时，身后铁门一扇扇落下，像城市终于发现有人从它胃里偷走了一块真相。",
            "阿洛在闸门外接住他，两人滚进雨水里，远处废城的灯区依次熄灭，却有一条通往南环的细线亮了起来。",
            "闻庭安攥紧信标，明白自己今晚没有救回妹妹，却第一次拿到了能逼近她的方向。",
            "他们没有急着离开，而是藏进车站外侧的排水廊，那里还残留着旧商铺的招牌和被水泡烂的儿童画。",
            "阿洛检查通讯扣里的文件，发现复制记录并不完整，中间有三段被系统主动抹去，只剩下异常整齐的空白。",
            "闻庭安把信标贴近空白处，微弱的电流顺着屏幕爬开，隐约补出一组坐标和一个陌生的时间戳。",
            "时间戳属于明天凌晨四点，也就是说第九实验组不是在隐藏过去，而是在准备一次即将发生的转移。",
            "排水廊外忽然传来广播，城防署宣布废城北区进入临时净化，所有无证人员必须在十五分钟内撤离。",
            "阿洛冷笑说净化从来不是清理污染，而是清理看见污染的人；她把湿透的头发拧干，眼神比雨夜还冷。",
            "闻庭安没有接话，他正在比对南环地图和新坐标，发现两条路线的终点都指向同一座废弃学校。",
            "那所学校曾经是妹妹参加城市能源测试的地方，也是事故后第一批被封存的公共建筑。",
            "信标再次震动，这一次不是脉冲，而是一段极短的录音：哥哥，如果你听见这个，不要相信蓝灯。",
            "声音太轻，却像一根针扎进闻庭安的骨缝，他几乎立刻想起车站里依次亮起的白灯和南环线上那条蓝色细线。",
            "阿洛看出他的脸色变了，主动把路线投到墙面，让他把情绪交给地图，而不是交给恐慌。",
            "两人决定分头准备：阿洛去黑市换一枚干净通行芯片，闻庭安回旧仓库取妹妹留下的机械钥匙。",
            "离开前，闻庭安把三本档案藏进排水廊夹层，又用废弃螺栓做了一个只有他和阿洛看得懂的标记。",
            "雨势渐小，废城上空的巡逻无人机却变得更多，红色扫描线像一张缓慢收紧的网。",
            "闻庭安穿过倒塌的步行桥，桥下积水映出他的影子，也映出远处一辆无声跟随的黑色维护车。",
            "他没有回头，故意绕进旧影院，从员工通道翻到后街，再把一只坏掉的投影仪接上备用电源。",
            "投影亮起时，整面外墙出现一群奔跑的人影，维护车果然被假目标牵走，轮胎压碎了满地玻璃。",
            "闻庭安趁机钻进仓库，打开藏在地板下的铁盒，里面除了机械钥匙，还有妹妹写给他的半张纸条。",
            "纸条上没有求救，只有一行歪斜的字：如果城市开始说谎，就让它自己作证。",
            "他终于明白妹妹当年不是被动卷入实验，她很可能在失踪前已经发现了第九实验组的漏洞。",
            "仓库门外传来轻轻的敲击声，三长两短，是阿洛约定的暗号，却比预定时间早了整整七分钟。",
            "闻庭安把信标压进袖套，隔着门问她黑市是不是出事了，门外却传来阿洛故意压低的回答：蓝灯来了。",
            "下一秒，仓库街区所有路灯同时变成冷蓝色，信标在袖中剧烈发烫，像要把他拖回那条被恢复的南环路线。",
            "闻庭安终于确认，真正的敌人不是追兵，而是这座会伪装成秩序的城市本身。"
        ]
        .join("\n");
        let first = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "信标脉冲",
                    "content": content,
                    "summary": "闻庭安在排水廊补出南环坐标，并确认蓝灯会把他拖回南环路线。",
                    "key_facts": ["闻庭安用信标补出南环坐标和陌生时间戳。"],
                    "continuity_updates": ["蓝灯成为追踪闻庭安的明确危险信号。"]
                })
                .to_string(),
            )
            .await
            .expect("first chapter");
        let first: serde_json::Value = serde_json::from_str(&first).expect("first json");
        assert_eq!(
            first["quality_gate"]["passed"],
            true,
            "first chapter should pass before duplicate replay check: {}",
            serde_json::to_string_pretty(&first).unwrap()
        );
        let project_dir = std::path::Path::new(project_path);
        let mut manifest = tool.read_manifest(project_dir).await.expect("manifest");
        manifest
            .chapters
            .iter_mut()
            .find(|chapter| chapter.number == 1)
            .expect("chapter one")
            .status = "approved".to_string();
        tool.write_manifest(project_dir, &manifest)
            .await
            .expect("mark fixture chapter approved");

        let second = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter",
                    "project_path": project_path,
                    "chapter_number": 2,
                    "chapter_title": "信标脉冲",
                    "content": content,
                    "summary": "闻庭安在排水廊补出南环坐标，并确认蓝灯会把他拖回南环路线。",
                    "key_facts": ["闻庭安用信标补出南环坐标和陌生时间戳。"],
                    "continuity_updates": ["蓝灯成为追踪闻庭安的明确危险信号。"]
                })
                .to_string(),
            )
            .await
            .expect("second chapter");
        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");
        assert_eq!(second["quality_gate"]["passed"], false);
        assert_ne!(second["chapter"]["title"], "信标脉冲");
        let issues = second["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert!(issues
            .iter()
            .any(|issue| issue.contains("body is identical to chapter 1")));
        assert_eq!(second["metadata_gate"]["blocking"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn audit_routes_duplicate_chapter_title_to_metadata_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(r#"{"action":"init_project","title":"标题归属测试","language":"zh-CN"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let chapter_one = complete_cjk_test_body(&(0..18)
            .map(|idx| {
                format!(
                    "白塔下第{idx}道风从南面吹来，顾衡在旧钟楼前发现铜色徽记的不同刻痕。它记录的是学院废弃前夜的逃亡名单，也揭开了校门地下仍在运转的灵阵。顾衡没有立刻追逐力量，而是把线索藏进袖口，决定先确认名单上的幸存者是否还活着。"
                )
            })
            .collect::<String>());
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "旧钟楼",
                "content": chapter_one,
                "summary": "顾衡在旧钟楼前发现铜色徽记和逃亡名单。",
                "key_facts": ["顾衡发现铜色徽记。"],
                "continuity_updates": ["旧钟楼和地下灵阵进入调查线。"]
            })
            .to_string(),
        )
        .await
        .expect("chapter one");

        let chapter_two = complete_cjk_test_body(&[
            "北境雨夜里，顾衡跟随另一条线索抵达断桥驿站。",
            "驿站没有钟楼，也没有徽记，只有被雨水泡开的军令和一盏不灭的蓝灯。",
            "他先把军令上的时间戳摊在灯下，再对照学院名单里缺失的三个人名。",
            "蓝灯的火芯忽然偏向西北，照出边境守军失踪前留下的泥印。",
            "顾衡意识到旧钟楼只是入口，真正的阴谋已经从学院延伸到关外。",
            "驿卒留下的半枚木牌证明有人故意调换了巡防路线。",
            "他把木牌、军令和名单并排摆好，终于看见三条线索指向同一支暗军。",
            "雨声压住马蹄声时，顾衡把蓝灯收进油纸，决定连夜追查边境缺口。",
            "这一章的变化不在钟楼，而在他第一次确认学院事件背后还有更大的军事阴影。",
        ]
        .join(""));
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "断桥驿站",
                "content": chapter_two,
                "summary": "顾衡在断桥驿站发现军令和蓝灯，确认边境阴谋。",
                "key_facts": ["顾衡发现边境守军失踪时间差。"],
                "continuity_updates": ["断桥驿站把学院线索扩展到边境阴谋。"]
            })
            .to_string(),
        )
        .await
        .expect("chapter two");
        tool.call(
            &serde_json::json!({
                "action": "revise_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "旧钟楼",
                "summary": "顾衡在断桥驿站发现军令和蓝灯，确认边境阴谋。",
                "key_facts": ["顾衡发现边境守军失踪时间差。"],
                "continuity_updates": ["断桥驿站把学院线索扩展到边境阴谋。"]
            })
            .to_string(),
        )
        .await
        .expect("metadata title revision");
        seal_test_chapter_authority(&tool, project_path, 2).await;

        let audit = tool
            .call(
                &serde_json::json!({
                    "action": "audit_chapter",
                    "project_path": project_path,
                    "chapter_number": 2
                })
                .to_string(),
            )
            .await
            .expect("audit chapter");
        let audit: serde_json::Value = serde_json::from_str(&audit).expect("audit json");
        let quality_issues = audit["quality_gate"]["issues"]
            .as_array()
            .expect("quality issues")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert!(
            !quality_issues
                .iter()
                .any(|issue| issue.contains("chapter title duplicates")),
            "title metadata issue leaked into body quality gate: {quality_issues:?}"
        );
        assert_ne!(
            audit["chapter"]["title"].as_str(),
            Some("旧钟楼"),
            "duplicate title should be repaired before audit persists metadata: {}",
            serde_json::to_string_pretty(&audit).unwrap()
        );
        assert!(
            audit["metadata_gate"]["blocking"]
                .as_array()
                .expect("metadata blocking")
                .is_empty(),
            "{}",
            serde_json::to_string_pretty(&audit).unwrap()
        );
        assert_eq!(
            audit["next_action"].as_str(),
            Some("repair_chapter_metadata"),
            "{}",
            serde_json::to_string_pretty(&audit).unwrap()
        );
    }

    #[tokio::test]
    async fn approve_requires_settlement_before_metadata_projection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(r#"{"action":"init_project","title":"批准标题测试","language":"zh-CN"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let chapter_one = complete_cjk_test_body(&(0..18)
            .map(|idx| {
                format!(
                    "旧钟楼第{idx}道钟声落进雨里，顾衡沿着石阶找到被封存的学院名单。他确认名单背后藏着一次集体逃亡，也确认地下灵阵仍在缓慢运转。"
                )
            })
            .collect::<String>());
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "旧钟楼",
                "content": chapter_one,
                "summary": "顾衡在旧钟楼找到学院名单和地下灵阵线索。",
                "key_facts": ["顾衡找到学院名单。"],
                "continuity_updates": ["旧钟楼线索进入主线。"]
            })
            .to_string(),
        )
        .await
        .expect("chapter one");

        let chapter_two = complete_cjk_test_body(&[
            "断桥驿站的蓝灯在雨幕中亮起，顾衡读出边境军令里的时间差。",
            "他没有再寻找钟楼线索，而是把学院名单和守军换防册放在同一张桌上。",
            "驿站掌柜留下的茶盏还温着，说明传令人离开不到半个时辰。",
            "蓝灯照出军令背面的暗印，暗印属于一支早该撤编的边境队伍。",
            "顾衡由此确认学院事件连接着边境守军失踪，也确认有人在两地同时灭口。",
            "他把断桥下的马蹄印拓进纸里，准备把这条证据带回给同伴。",
            "离开驿站前，他在灯芯里找到一截黑线，黑线和旧钟楼铜徽的纹路完全相同。",
            "这一章让调查从学院内部推进到边境军线，顾衡必须面对更危险的幕后势力。",
        ]
        .join(""));
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "断桥驿站",
                "content": chapter_two,
                "summary": "顾衡在断桥驿站发现边境军令和蓝灯线索。",
                "key_facts": ["顾衡发现边境军令时间差。"],
                "continuity_updates": ["断桥驿站把学院线索扩展到边境阴谋。"]
            })
            .to_string(),
        )
        .await
        .expect("chapter two");
        tool.call(
            &serde_json::json!({
                "action": "revise_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "chapter_title": "旧钟楼",
                "summary": "顾衡在断桥驿站发现边境军令和蓝灯线索。",
                "key_facts": ["顾衡发现边境军令时间差。"],
                "continuity_updates": ["断桥驿站把学院线索扩展到边境阴谋。"]
            })
            .to_string(),
        )
        .await
        .expect("metadata title revision");
        seal_test_chapter_authority(&tool, project_path, 2).await;
        persist_test_best_candidate(project_path, 2).await;
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 2,
                "verdict": "passed"
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
                    "chapter_number": 2
                })
                .to_string(),
            )
            .await
            .expect("approve");
        let approve: serde_json::Value = serde_json::from_str(&approve).expect("approve json");
        assert_eq!(
            approve["success"].as_bool(),
            Some(false),
            "{}",
            serde_json::to_string_pretty(&approve).unwrap()
        );
        assert_eq!(
            approve["error_kind"].as_str(),
            Some("approval_requires_state_settlement"),
            "{}",
            serde_json::to_string_pretty(&approve).unwrap()
        );
        assert_eq!(approve["next_action"].as_str(), Some("settle_chapter_state"));
    }

    #[tokio::test]
    async fn approval_settlement_precedes_display_metadata_projection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(r#"{"action":"init_project","title":"元数据分层测试","language":"zh-CN"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "黎启洄追查旧站封印裂纹，并在救人和揭露异常频率之间寻找平衡。",
                "characters": ["name: 黎启洄; role: 主角; desire: 救出同伴; fear: 封印失控; bottom_line: 不牺牲无辜者。"],
                "world_rules": ["旧站频纹会撕裂封印。"],
                "outline": "旧站救人、确认频纹、追查封印。"
            })
            .to_string(),
        )
        .await
        .expect("contract");
        let manifest_raw =
            tokio::fs::read_to_string(std::path::Path::new(project_path).join("project.json"))
                .await
                .expect("manifest");
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest_raw).expect("manifest json");
        let primary_name = manifest["character_ledger"]
            .as_array()
            .expect("character ledger")
            .iter()
            .find(|entry| entry["role"].as_str().unwrap_or_default().contains("主角"))
            .or_else(|| manifest["character_ledger"].as_array().unwrap().first())
            .and_then(|entry| entry["canonical_name"].as_str())
            .expect("primary character")
            .to_string();
        seal_test_chapter_authority(&tool, project_path, 1).await;

        let content = complete_cjk_test_body(&[
            &format!("旧站雨声把月台敲得发亮，{primary_name}听见轨道深处传来细密的低鸣。"),
            "同伴半只脚已经踩进裂缝，他先扔掉手里的频率探针，扑过去扣住对方腕骨。",
            "封印石上的纹路一寸寸亮起，像有人在黑暗里拨动看不见的弦。",
            &format!("{primary_name}没有追逐那股突然涌来的力量，而是用断线钳剪开月台下方的失控线路。"),
            "蓝白火花炸开时，他把同伴拖回灯下，自己肩上却被频纹烫出一圈暗红痕迹。",
            "站务室里的旧钟停在二十三点十七分，钟面背后夹着一张潮湿的巡检单。",
            "巡检单写着三天前就有人报告封印异常，但报告被划掉，签名处只剩一个残缺印章。",
            &format!("{primary_name}把巡检单塞进内袋，转身看见裂缝里浮起一枚碎裂的铜环。"),
            "铜环内侧刻着同伴家族的徽记，这让他意识到事故不是偶然。",
            "他没有把真相说出口，只让同伴先离开站台，自己留下记录频纹变化。",
            "雨水从破顶棚落下，打在封印石上，低鸣忽然变成短促的求救声。",
            &format!("{primary_name}伸手按住石面，看见一段被封住的旧影：有人曾在这里交换过一整列失踪乘客。"),
            "他收回手时掌心全是血，却第一次确认频纹不是灾害，而是一封迟到多年的证词。",
            "远处的应急灯逐盏熄灭，站台出口传来守夜人的脚步。",
            &format!("{primary_name}把铜环藏进袖口，扶起同伴，决定天亮前查清被划掉的报告来自谁。"),
            "离开前，他回头看了一眼裂缝，低声承诺不会让旧站继续吞人。",
        ]
        .join(""));
        let add = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "频率过载",
                    "content": content,
                    "summary": format!("{primary_name}在旧站救下同伴，并确认异常频率会撕裂站台封印。"),
                    "key_facts": [format!("{primary_name}在旧站救下同伴。")],
                    "continuity_updates": ["旧站封印出现裂纹。"]
                })
                .to_string(),
            )
            .await
            .expect("add chapter");
        let add: serde_json::Value = serde_json::from_str(&add).expect("add json");
        persist_test_best_candidate(project_path, 1).await;
        assert_eq!(add["quality_gate"]["passed"], true, "{add}");
        assert!(
            add["metadata_gate"]["blocking"]
                .as_array()
                .expect("blocking")
                .is_empty(),
            "{add}"
        );
        assert!(
            !add["metadata_gate"]["repairable"]
                .as_array()
                .expect("repairable")
                .is_empty(),
            "{add}"
        );

        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "passed"
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
            approve["success"].as_bool(),
            Some(false),
            "{}",
            serde_json::to_string_pretty(&approve).unwrap()
        );
        assert_eq!(
            approve["error_kind"].as_str(),
            Some("approval_requires_state_settlement"),
            "{}",
            serde_json::to_string_pretty(&approve).unwrap()
        );
    }

    #[tokio::test]
    async fn novel_studio_runs_full_project_lifecycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "长篇项目",
                    "language": "zh"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "import_chapters",
                "project_path": project_path,
                "content": "第一章 初火\n宁衡声在雪中醒来。\n第二章 夜桥\n他遇见了守桥人。"
            })
            .to_string(),
        )
        .await
        .expect("import");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "宁衡声追寻钟声来源，并逐步理解自己的来历。",
                "characters": [
                    "name: 宁衡声; role: 主角; 用户指定",
                    "name: 守桥人; role: 同伴; 用户指定"
                ],
                "world_rules": ["钟声会牵引记忆。"],
                "outline": "醒来、过桥、面对风暴。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        tool.call(
            &serde_json::json!({
                "action": "plan_chapter",
                "project_path": project_path,
                "chapter_number": 3,
                "chapter_title": "风暴",
                "plan": "延续前两章事实，推进主角第一次主动选择。"
            })
            .to_string(),
        )
        .await
        .expect("plan");

        let content_plan = tool
            .call(
                &serde_json::json!({
                    "action": "plan_chapter",
                    "project_path": project_path,
                    "chapter_number": 4,
                    "chapter_title": "余波",
                    "content": "承接上一章的代价，安排主角发现新的连续性线索。"
                })
                .to_string(),
            )
            .await
            .expect("content plan");
        let content_plan_value: serde_json::Value =
            serde_json::from_str(&content_plan).expect("content plan json");
        assert_eq!(
            content_plan_value["chapter_plan"]["plan"],
            "承接上一章的代价，安排主角发现新的连续性线索。"
        );

        tool.call(
            &serde_json::json!({
                "action": "architect_chapter",
                "project_path": project_path,
                "chapter_number": 3,
                "chapter_title": "风暴",
                "content": "场景一：钟声逼近。场景二：主角选择过桥。场景三：代价显现。"
            })
            .to_string(),
        )
        .await
        .expect("architect");

        tool.call(
            &serde_json::json!({
                "action": "update_truth",
                "project_path": project_path,
                "section": "characters",
                "content": "宁衡声在雪中醒来，仍不知道自己的来历。",
                "administrative_override": true,
                "notes": "测试显式管理更正的来源记录。"
            })
            .to_string(),
        )
        .await
        .expect("truth");

        tool.call(
            &serde_json::json!({
                "action": "update_style",
                "project_path": project_path,
                "title": "叙事风格",
                "content": "短句优先，避免角色口吻混淆。"
            })
            .to_string(),
        )
        .await
        .expect("style");

        let context = tool
            .call(
                &serde_json::json!({
                    "action": "compose_context",
                    "project_path": project_path,
                    "chapter_number": 3
                })
                .to_string(),
            )
            .await
            .expect("context");
        assert!(context.contains("context_path"));
        seal_test_chapter_authority(&tool, project_path, 1).await;

        let review = tool
            .call(
                &serde_json::json!({
                    "action": "audit_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "issues": ["缺少明确目标"],
                    "feedback": "补充主角的短期目标。"
                })
                .to_string(),
            )
            .await
            .expect("review");
        let review_value: serde_json::Value = serde_json::from_str(&review).expect("json");
        assert_eq!(review_value["runtime_effect"], "artifact.reviewed");
        assert!(review_value["artifact_path"]
            .as_str()
            .unwrap()
            .contains("/reviews/"));
        assert_eq!(review_value["review"]["verdict"], "needs_revision");

        let revised_body = complete_cjk_test_body("宁衡声在雪中醒来。守桥人提醒他：钟声会牵引记忆。宁衡声听见记忆钟声，决定找到钟声的源头；记忆钟声成为下一章线索。");
        tool.call(
            &serde_json::json!({
                "action": "revise_draft",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "记忆钟声",
                "summary": "宁衡声和守桥人在雪中听见记忆钟声，并决定寻找钟声来源。",
                "content": revised_body,
                "key_facts": ["宁衡声和守桥人听见记忆钟声", "钟声会牵引记忆"],
                "continuity_updates": ["记忆钟声成为下一章线索"]
            })
            .to_string(),
        )
        .await
        .expect("revise");
        persist_test_best_candidate(project_path, 1).await;

        let blocked_approval = tool
            .call(
                &serde_json::json!({
                    "action": "approve_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("approval gate guidance");
        let blocked_approval_json: serde_json::Value =
            serde_json::from_str(&blocked_approval).expect("json");
        assert_eq!(blocked_approval_json["success"], false);
        assert_eq!(blocked_approval_json["recoverable"], true);
        assert_eq!(
            blocked_approval_json["error_kind"],
            "invalid_chapter_lifecycle_transition"
        );
        assert_eq!(blocked_approval_json["next_action"], "audit_chapter");

        let pass_review = tool
            .call(
                &serde_json::json!({
                    "action": "audit_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("pass review");
        let pass_review_value: serde_json::Value =
            serde_json::from_str(&pass_review).expect("json");
        assert_eq!(pass_review_value["runtime_effect"], "artifact.reviewed");
        assert!(pass_review_value["artifact_path"]
            .as_str()
            .unwrap()
            .contains("/reviews/"));

        tool.call(
            &serde_json::json!({
                "action": "settle_chapter_state",
                "project_path": project_path,
                "chapter_number": 1,
                "content": serde_json::json!({
                    "state_changes": [],
                    "current_state": "宁衡声决定找到记忆钟声的源头。",
                    "pending_hooks": "记忆钟声成为下一章线索。",
                    "chapter_summary": "宁衡声和守桥人在雪中听见记忆钟声，并决定寻找钟声来源。",
                    "continuity_updates": ["记忆钟声成为下一章线索"]
                }).to_string()
            })
            .to_string(),
        )
        .await
        .expect("settle revised chapter");

        tool.call(
            &serde_json::json!({
                "action": "approve_chapter",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .map(|result| {
            let value: serde_json::Value = serde_json::from_str(&result).expect("json");
            assert_eq!(value["runtime_effect"], "artifact.approved");
            assert!(value["artifact_path"]
                .as_str()
                .unwrap()
                .ends_with("project.json"));
        })
        .expect("approve");

        let readable_collection =
            tokio::fs::read_to_string(PathBuf::from(project_path).join("exports/章节合集.txt"))
                .await
                .expect("readable collection export");
        assert!(
            readable_collection.contains("记忆钟声"),
            "approved chapter should refresh readable txt collection: {readable_collection}"
        );

        tool.call(
            &serde_json::json!({
                "action": "snapshot",
                "project_path": project_path,
                "snapshot_id": "after-first-approval",
                "notes": "stable first chapter"
            })
            .to_string(),
        )
        .await
        .expect("snapshot");

        let analytics = tool
            .call(
                &serde_json::json!({
                    "action": "analytics",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("analytics");
        assert!(analytics.contains("status_counts"));

        let truth = tool
            .call(
                &serde_json::json!({
                    "action": "read_truth",
                    "project_path": project_path,
                    "section": "characters"
                })
                .to_string(),
            )
            .await
            .expect("truth read");
        assert!(truth.contains("宁衡声在雪中醒来"));

        let export = tool
            .call(
                &serde_json::json!({
                    "action": "export",
                    "project_path": project_path,
                    "format": "txt",
                    "approved_only": true
                })
                .to_string(),
            )
            .await
            .expect("export");
        let export_value: serde_json::Value = serde_json::from_str(&export).expect("json");
        let output_path = export_value["output_path"].as_str().expect("output");
        let exported = tokio::fs::read_to_string(output_path).await.expect("read");
        assert!(exported.contains("钟声"));
        assert!(!exported.contains("她遇见了守桥人"));
    }
