    #[tokio::test]
    async fn novel_studio_approve_draft_returns_canonical_manifest_contract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "zh-CN",
                    "genre": "都市玄幻",
                    "brief": "基层维修员在城市灵脉系统里查清事故真相。",
                    "premise": "秦知安从设备维修员起步，发现灵脉系统事故背后有权力交易。",
                    "title": "雨塔回声",
                    "ending_direction": "秦知安公开事故证据，守住城市下层人的选择权。",
                    "protagonist_arc": "从只想保住饭碗，到愿意承担代价公开真相。",
                    "world_imagery": "雨塔、灵脉井、失真的城市广播。",
                    "main_causal_spine": "一次维修事故引出证据，逼迫主角在自保和公开真相之间选择。",
                    "title_rationale": "雨塔是终局公开证据的地点，回声对应被压下的事故真相重新传遍城市。",
                    "characters": [
                        "name: 秦知安; role: 主角; desire: 保住家人和工作; fear: 被城市系统吞没; bottom_line: 不牺牲无辜者",
                        "name: 梁栖川; role: 关键对手; desire: 维持灵脉交易秩序; fear: 权力网络崩塌; bottom_line: 对手动机必须清楚"
                    ],
                    "themes": ["基层角色争取选择权"],
                    "world_rules": ["灵脉系统控制城市能源和考试权限。"],
                    "style_rules": ["都市节奏，冲突推进明确。"],
                    "must_avoid": ["不要替换权威角色名。"],
                    "outline": "秦知安从维修事故追到灵脉交易，终局在雨塔公开证据。"
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let draft_path = draft["draft_path"].as_str().expect("draft path");

        let approved = tool
            .call(
                &serde_json::json!({
                    "action": "approve_draft",
                    "draft_path": draft_path
                })
                .to_string(),
            )
            .await
            .expect("approve draft");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approved json");
        assert_eq!(approved["success"], true, "{approved}");
        assert!(
            !approved.to_string().contains(".novel-approve-"),
            "approval response must not leak its internal staging path: {approved}"
        );
        let project_path = approved["project_path"].as_str().expect("project path");
        let project_parent = PathBuf::from(project_path)
            .parent()
            .expect("project parent")
            .to_path_buf();
        let mut entries = tokio::fs::read_dir(&project_parent)
            .await
            .expect("project parent entries");
        while let Some(entry) = entries.next_entry().await.expect("next project entry") {
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".novel-approve-"),
                "successful approval must atomically consume its staging directory"
            );
        }
        let raw = tokio::fs::read_to_string(PathBuf::from(project_path).join("project.json"))
            .await
            .expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest json");
        assert_eq!(
            approved["draft"]["characters"], manifest["contract"]["characters"],
            "approved draft must be the same canonical contract that writer/reviewer will read"
        );
        let character_lines = approved["draft"]["characters"]
            .as_array()
            .expect("characters");
        assert_eq!(character_lines.len(), 2);
        let character_text = character_lines
            .iter()
            .filter_map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(character_text.contains("name: 秦知安"), "{character_text}");
        assert!(character_text.contains("name: 梁栖川"), "{character_text}");
        assert!(
            character_lines.iter().all(|line| {
                let line = line.as_str().unwrap_or_default();
                line.contains("name_source: contract_authority")
                    && line.contains("character_id: character-")
            }),
            "{character_text}"
        );
    }

    #[tokio::test]
    async fn novel_studio_plan_chapter_can_scaffold_from_contract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Contract Scaffold",
                    "language": "zh-cn",
                    "brief": "一部长篇原创故事",
                    "target_units": 500000,
                    "chapter_unit_target": 2000
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
                "premise": "主角在长期冲突中逐步确认自己的道路。",
                "outline": "开端建立目标，中段承受代价，终局完成选择。"
            })
            .to_string(),
        )
        .await
        .expect("contract");

        let planned = tool
            .call(
                &serde_json::json!({
                    "action": "plan_chapter",
                    "project_path": project_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("plan");
        let value: serde_json::Value = serde_json::from_str(&planned).expect("json");
        assert_eq!(value["success"], true);
        assert_eq!(value["plan_generated_from_contract"], true);
        assert_eq!(value["chapter_plan"]["title"], "第1章");
        assert!(value["chapter_plan"]["plan"]
            .as_str()
            .expect("plan")
            .contains("连续性要求"));
    }

    #[tokio::test]
    async fn novel_studio_compose_chapter_with_content_saves_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Compose Saves Draft",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "compose_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Opening",
                    "content": "A stable draft is written when compose_chapter carries body content."
                })
                .to_string(),
            )
            .await
            .expect("compose");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], true);
        assert_eq!(value["runtime_effect"], "artifact.written");
        assert_eq!(value["next_action"], "repair_chapter_metadata");
        assert_eq!(value["chapter"]["number"], 1);
    }

    #[tokio::test]
    async fn novel_studio_infers_chapter_target_from_project_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Scale Target",
                    "language": "zh-CN",
                    "target_units": 500000
                })
                .to_string(),
            )
            .await
            .expect("init");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        let chapter_target = value["state"]["chapter_unit_target"]
            .as_u64()
            .expect("chapter target");
        assert!(chapter_target > 0);
        assert!(chapter_target <= 8000);
    }

    #[tokio::test]
    async fn novel_studio_write_draft_without_content_is_recoverable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": dir.path().join("project"),
                    "chapter_number": 2
                })
                .to_string(),
            )
            .await
            .expect("recoverable");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "missing_required_content");
        assert!(value["next_step_hint"]
            .as_str()
            .expect("hint")
            .contains("Generate the actual body text first"));
        assert!(value["next_step_hint"]
            .as_str()
            .expect("hint")
            .contains("runtime can attach it"));
    }

    #[tokio::test]
    async fn novel_studio_rejects_locator_only_source_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Source Guard",
                    "language": "zh-CN"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");
        let url = "https://example.test/source.txt";

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "add_source",
                    "project_path": project_path,
                    "source_title": "Only URL",
                    "source_url": url,
                    "content": url
                })
                .to_string(),
            )
            .await
            .expect("recoverable");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "invalid_source_content");
        assert!(value["next_step_hint"]
            .as_str()
            .expect("hint")
            .contains("retrieval/search/read tool"));
    }

    #[tokio::test]
    async fn novel_studio_accepts_short_source_excerpt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Source Excerpt",
                    "language": "zh-CN"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "add_source",
                    "project_path": project_path,
                    "source_title": "Excerpt",
                    "source_url": "https://example.test/source.txt",
                    "content": "素材摘要：主角在濒临崩塌的边境城市中发现古老契约，城市、誓言和失落门扉构成主要灵感。"
                })
                .to_string(),
            )
            .await
            .expect("source");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], true);
        assert_eq!(value["source"]["unit_count"].as_u64().unwrap() > 10, true);

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
        let excerpt = context["context"]["sources"][0]["excerpt"]
            .as_str()
            .expect("source excerpt");
        assert!(excerpt.contains("古老契约"));
        let context_sources = context["context_package"]["selected_context"]
            .as_array()
            .expect("selected context");
        assert!(context_sources.iter().any(|entry| entry["excerpt"]
            .as_str()
            .unwrap_or_default()
            .contains("失落门扉")));
    }

    #[tokio::test]
    async fn novel_studio_merges_json_input_wrapper_arguments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Wrapped Input",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter_plan",
                    "input": serde_json::json!({
                        "project_path": project_path,
                        "chapter_number": 1,
                        "chapter_title": "Opening",
                        "plan": "Plan text carried inside a generic input wrapper."
                    }).to_string()
                })
                .to_string(),
            )
            .await
            .expect("wrapped input");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], true);
        assert_eq!(value["chapter_plan"]["number"], 1);
        assert_eq!(value["chapter_plan"]["title"], "Opening");
    }

    #[tokio::test]
    async fn novel_studio_treats_plain_input_string_as_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Plain Input",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter_plan",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "input": "Plain plan text carried in a generic input field."
                })
                .to_string(),
            )
            .await
            .expect("plain input");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], true);
        assert_eq!(
            value["chapter_plan"]["plan"],
            "Plain plan text carried in a generic input field."
        );
    }

    #[tokio::test]
    async fn novel_studio_init_project_title_collision_blocks_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let first = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Same Title",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("first init");
        let second = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Same Title",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("second init");

        let first: serde_json::Value = serde_json::from_str(&first).expect("first json");
        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");
        assert_eq!(first["success"], true);
        assert_eq!(second["success"], false);
        assert_eq!(second["recoverable"], true);
        assert_eq!(second["title_conflicts"][0]["title"], "Same Title");
        assert_eq!(
            second["title_conflict_policy"],
            "blocked_by_default_for_new_project"
        );
    }

    #[tokio::test]
    async fn novel_studio_draft_project_without_title_generates_fresh_title() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let brief = "我们先通过多轮对话定下一部小说的大纲和创作合同。我要一部草根逆袭的科幻玄幻小说，每章约3000字，一共约50万字。";

        let output = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "Chinese",
                    "genre": "科幻玄幻",
                    "brief": brief
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        let title = value["draft"]["title"].as_str().expect("title");

        assert_eq!(value["success"], true);
        assert!(!title.trim().is_empty());
        assert_ne!(title, brief);
        assert!(title.chars().any(is_cjk_unified));
        assert!(!title.contains("多轮对话"));
        assert!(!title.contains("创作合同"));
    }

    #[tokio::test]
    async fn novel_studio_draft_project_governs_names_and_approval_canonicalizes_contract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let output = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "title": "霓虹缄默",
                    "language": "zh-CN",
                    "genre": "都市玄幻",
                    "brief": "都市玄幻，每章2500字，至少5万字起。",
                    "target_units": 50000,
                    "chapter_unit_target": 2500,
                    "premise": "白澈声围绕城市裂隙引发核心冲突。",
                    "ending_direction": "白澈声以遗忘换取城市秩序稳定。",
                    "protagonist_arc": "白澈声从追逐力量转为守护平凡。",
                    "world_imagery": "霓虹灯下的裂隙与记忆碎片。",
                    "main_causal_spine": "白澈声发现力量代价并最终封印裂隙。",
                    "title_rationale": "书名来自终局里城市裂隙被封住后，霓虹仍保存失忆者缄默回声的代价。",
                    "themes": ["力量代价与普通人的选择权"],
                    "world_rules": ["城市裂隙会以记忆作为力量代价。"],
                    "style_rules": ["都市现场感，冲突和代价明确。"],
                    "must_avoid": ["不要替换已确认角色名。"],
                    "characters": [
                        "姓名：白澈声，角色：主角，欲望：掌控裂隙，恐惧：失去记忆，底线：不牺牲无辜。",
                        "姓名：沈庭声，角色：对手，欲望：维护旧秩序，恐惧：秩序崩塌，底线：不公开真相。"
                    ],
                    "outline": "第01章《裂痕中的微光》：本章目标：白澈声发现能力觉醒，并意识到记忆流失的征兆。\n第02章《记忆的租借期》：本章目标：白澈声尝试通过获取更多力量来解决危机。",
                    "relationship_ledger": [{
                        "characters": ["白澈声", "沈庭声"],
                        "relationship_type": "主角与关键压力源",
                        "current_state": "白澈声与沈庭声围绕裂隙治理权发生冲突。"
                    }]
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        assert_eq!(value["success"], true, "{value}");

        let draft = &value["draft"];
        let text = serde_json::to_string(draft).expect("draft text");
        assert!(text.contains("白澈声"), "{text}");
        assert!(text.contains("沈庭声"), "{text}");
        assert!(
            text.contains("name_source: contract_authority")
                && text.contains("character_id: character-"),
            "the visible draft must preserve model names and add stable authority metadata: {text}"
        );

        let approved = tool
            .call(
                &serde_json::json!({
                    "action": "approve_draft",
                    "draft_path": value["draft_path"].as_str().expect("draft path")
                })
                .to_string(),
            )
            .await
            .expect("approve draft");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approved json");
        assert_eq!(approved["success"], true, "{approved}");
        let project_path = approved["project_path"].as_str().expect("project path");
        let raw = tokio::fs::read_to_string(PathBuf::from(project_path).join("project.json"))
            .await
            .expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest json");
        assert_eq!(
            approved["draft"]["characters"], manifest["contract"]["characters"],
            "approved draft and manifest must expose one canonical role table"
        );
        let canonical_text =
            serde_json::to_string(&manifest["contract"]).expect("canonical contract");
        assert!(
            canonical_text.contains("白澈声")
                && canonical_text.contains("沈庭声")
                && canonical_text.contains("name_source: contract_authority"),
            "approved contract must preserve model-authored names as contract authority: {canonical_text}"
        );
        let character_names = manifest["contract"]["characters"]
            .as_array()
            .expect("characters")
            .iter()
            .filter_map(|line| line.as_str())
            .map(|line| {
                crate::tool::writing::creation_contract::draft_character_line_to_contract(line)
                    .canonical_name
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        let character_ids = manifest["contract"]["characters"]
            .as_array()
            .expect("characters")
            .iter()
            .filter_map(|line| line.as_str())
            .map(|line| {
                crate::tool::writing::creation_contract::draft_character_line_to_contract(line)
                    .character_id
            })
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let relationship_names = manifest["contract"]["structured_contract_v2"]
            ["relationship_ledger"][0]["characters"]
            .as_array()
            .expect("relationship characters")
            .iter()
            .filter_map(|name| name.as_str().map(ToString::to_string))
            .collect::<Vec<_>>();
        assert_eq!(
            relationship_names, character_names,
            "structured relationship ledger must use the same governed character authority table"
        );
        let relationship_ids = manifest["contract"]["structured_contract_v2"]
            ["relationship_ledger"][0]["character_ids"]
            .as_array()
            .expect("relationship character ids")
            .iter()
            .filter_map(|id| id.as_str().map(ToString::to_string))
            .collect::<Vec<_>>();
        assert_eq!(
            relationship_ids, character_ids,
            "relationship graph must reference the canonical character ids"
        );
    }

    #[tokio::test]
    async fn novel_studio_keeps_title_temporary_until_model_supplies_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "zh-CN",
                    "brief": "帮我写小说"
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let draft_path = draft["draft_path"].as_str().expect("draft path");
        let old_title = draft["draft"]["title"].as_str().expect("old title");

        let updated = tool
            .call(
                &serde_json::json!({
                    "action": "update_draft",
                    "draft_path": draft_path,
                    "language": "zh-CN",
                    "genre": "星际科幻",
                    "brief": "写一部星际科幻类小说，总字数5万字，每章2500字，剧情完整，有清晰结尾。",
                    "target_units": 50000,
                    "chapter_unit_target": 2500
                })
                .to_string(),
            )
            .await
            .expect("update draft");
        let updated: serde_json::Value = serde_json::from_str(&updated).expect("updated json");
        let new_title = updated["draft"]["title"].as_str().expect("new title");

        assert!(
            project_title_is_temporary_placeholder(old_title)
                && project_title_is_temporary_placeholder(new_title),
            "draft without LLM-authored title should stay temporary: {old_title} -> {new_title}"
        );
        assert!(
            !new_title.contains("星际") && !new_title.contains("深空"),
            "tool should not invent a story-derived formal title: {new_title}"
        );
    }

    #[tokio::test]
    async fn novel_studio_auto_title_uses_outline_and_ending_seed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let output = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "zh-CN",
                    "genre": "赛博朋克玄幻",
                    "brief": "底层学生在霓虹学院通过考试晋级。",
                    "premise": "主角破解义体灵脉背后的算法秩序。",
                    "world_rules": ["义体灵脉由神经符阵驱动。"],
                    "outline": "终局重写灵脉算法，让普通人也能修行。"
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        let title = value["draft"]["title"].as_str().expect("title");

        assert!(
            project_title_is_temporary_placeholder(title),
            "tool must not invent formal title before LLM contract confirmation: {title}"
        );
    }

    #[tokio::test]
    async fn novel_studio_draft_project_without_brief_still_honors_chinese_language() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let output = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "zh-CN"
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        let title = value["draft"]["title"].as_str().expect("title");

        assert_eq!(value["success"], true);
        assert!(title.chars().any(is_cjk_unified), "title was {title}");
    }

    #[tokio::test]
    async fn novel_studio_update_draft_recovers_auto_english_title_for_chinese_contract() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let output = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("draft project");
        let value: serde_json::Value = serde_json::from_str(&output).expect("json");
        let draft_path = value["draft_path"].as_str().expect("draft_path");
        let original_title = value["draft"]["title"].as_str().expect("title");
        assert!(!original_title.chars().any(is_cjk_unified));

        let updated = tool
            .call(
                &serde_json::json!({
                    "action": "update_draft",
                    "draft_path": draft_path,
                    "language": "zh-CN",
                    "genre": "草根逆袭玄幻",
                    "brief": "写一个草根逆袭的玄幻小说。"
                })
                .to_string(),
            )
            .await
            .expect("update draft");
        let updated: serde_json::Value = serde_json::from_str(&updated).expect("json");
        let title = updated["draft"]["title"].as_str().expect("title");

        assert_ne!(title, original_title);
        assert!(title.chars().any(is_cjk_unified), "title was {title}");
    }

    #[tokio::test]
    async fn novel_studio_init_project_allows_explicit_title_conflict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let first = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Same Title",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("first init");
        let second = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Same Title",
                    "language": "en",
                    "allow_title_conflict": true
                })
                .to_string(),
            )
            .await
            .expect("second init");

        let first: serde_json::Value = serde_json::from_str(&first).expect("first json");
        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");
        assert_eq!(first["success"], true);
        assert_eq!(second["success"], true);
        assert_ne!(first["project_path"], second["project_path"]);
        assert_eq!(second["title_conflicts"][0]["title"], "Same Title");
        assert_eq!(
            second["title_conflict_policy"],
            "explicit_allow_unique_project_path_allocated"
        );
    }

    #[tokio::test]
    async fn novel_studio_init_project_blocks_wrapped_prior_title_conflict_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        tool.call(
            &serde_json::json!({
                "action": "init_project",
                "title": "《万象归墟》 (Provisional Title)",
                "language": "Chinese"
            })
            .to_string(),
        )
        .await
        .expect("first init");
        let second = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "万象归墟",
                    "language": "Chinese"
                })
                .to_string(),
            )
            .await
            .expect("second init");

        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");
        assert_eq!(second["success"], false);
        assert_eq!(second["recoverable"], true);
        assert_eq!(
            second["title_conflicts"][0]["title"],
            "《万象归墟》 (Provisional Title)"
        );
    }

    #[tokio::test]
    async fn novel_studio_init_project_blocks_reused_cjk_title_core_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        tool.call(
            &serde_json::json!({
                "action": "init_project",
                "title": "万象归墟：旧章",
                "language": "Chinese"
            })
            .to_string(),
        )
        .await
        .expect("first init");
        let second = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "万象归墟 Eternal Return",
                    "language": "Chinese"
                })
                .to_string(),
            )
            .await
            .expect("second init");

        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");
        assert_eq!(second["success"], false);
        assert_eq!(second["recoverable"], true);
        assert_eq!(second["title_conflicts"][0]["title"], "万象归墟：旧章");
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_placeholder_and_character_drift_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Quality Gate","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "少年林霄守住灵渊。",
                "characters": ["林霄: 主角", "苏瑶: 同伴"],
                "world_rules": ["灵渊会吞噬失衡世界"]
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
                    "content": "林枫走入灵渊。此处省略后续剧情。"
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");

        assert_eq!(
            draft["runtime_effect"], "artifact.needs_revision",
            "{draft}"
        );
        assert_eq!(draft["chapter"]["status"], "needs_revision");
        assert_eq!(draft["quality_gate"]["passed"], false);
        assert!(draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("placeholder or omission marker")));
        let artifact_path = draft["artifact_path"].as_str().expect("artifact path");
        let saved = tokio::fs::read_to_string(artifact_path)
            .await
            .expect("saved chapter");
        assert!(saved.contains("林枫走入灵渊"));
        assert!(tool
            .call(
                &serde_json::json!({
                    "action": "export",
                    "project_path": project_path,
                    "format": "txt"
                })
                .to_string()
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn novel_studio_sanitizes_removable_protocol_and_json_surface_residue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Surface Gate","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "云岚进入灵墟。",
                "characters": ["云岚: 主角"],
                "world_rules": ["灵墟的力量必须付出代价"]
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
                    "chapter_title": "第1章：灵墟",
                    "content": "云岚走入灵墟。\n```json\n  \"addition\": \"云岚听见石门低鸣。\"\n```\n云岚握紧残灯。"
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let issues = draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(|issue| issue.as_str())
            .collect::<Vec<_>>();

        assert!(!issues
            .iter()
            .any(|issue| issue.contains("code fence/control block")));
        assert!(!issues
            .iter()
            .any(|issue| issue.contains("JSON field/control surface")));
        let artifact_path = draft["artifact_path"].as_str().expect("artifact path");
        let saved = std::fs::read_to_string(artifact_path).expect("saved chapter");
        assert!(!saved.contains("```json"));
        assert!(!saved.contains("\"addition\""));
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_generation_meta_disclaimer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Meta Gate","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "云岚进入灵墟。",
                "characters": ["云岚: 主角"],
                "world_rules": ["灵墟的力量必须付出代价"]
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
                    "chapter_title": "第1章：灵墟",
                    "content": "云岚走入灵墟，听见石门后的潮声。\n[Note: Due to character limit constraints in the output, the content ends at the point of decision. In a production environment, the text would continue to complete the chapter.]\n云岚没有回头，她把残灯举到胸前。"
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let issues = draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(|issue| issue.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            draft["runtime_effect"], "artifact.needs_revision",
            "{draft}"
        );
        assert!(issues
            .iter()
            .any(|issue| issue.contains("model/output-limit meta commentary")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("placeholder or omission marker")));
    }

    #[tokio::test]
    async fn novel_studio_repairs_unambiguous_near_character_name_typos_before_audit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Drift Gate","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "林墨与苏清月守住灵枢。",
                "characters": [
                    "name: 林墨; role: 主角; 用户指定",
                    "name: 苏清月; role: 同伴; 用户指定"
                ],
                "world_rules": ["灵枢失衡会引发灵灾"]
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let typo_body = complete_cjk_test_body("# 第1章：裂痕\n# 第1章：裂痕\n“退后！林浩，快退回阵内！”苏清过握住符文，林墨看见灵灾逼近。苏清月月低声提醒：苏清，别让灵灾靠近。");
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "第1章：裂痕",
                    "summary": "林墨和苏清月月遭遇灵灾。",
                    "content": typo_body,
                    "key_facts": ["林墨看见灵灾逼近"],
                    "continuity_updates": ["苏清月参与灵灾现场"]
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");
        let issues = draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(|issue| issue.as_str())
            .collect::<Vec<_>>();

        assert_eq!(draft["runtime_effect"], "artifact.needs_revision");
        let artifact_path = draft["artifact_path"].as_str().expect("artifact path");
        let saved = tokio::fs::read_to_string(artifact_path)
            .await
            .expect("saved chapter");
        assert!(saved.contains("林浩，快退回阵内"));
        assert!(!saved.contains("苏清过握住符文"));
        assert!(!saved.contains("苏清月月低声提醒"));
        assert!(saved.contains("苏清月握住符文"));
        assert!(saved.contains("苏清月低声提醒"));
        let heading_count = saved
            .lines()
            .filter(|line| markdown_heading_text(line).is_some())
            .count();
        assert_eq!(
            heading_count, 1,
            "writer output should persist one canonical chapter heading"
        );
        assert!(issues.iter().all(|issue| !issue.contains("苏清月月")));
    }

    #[tokio::test]
    async fn novel_studio_quality_gate_blocks_unexpected_script_noise() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Script Noise","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "characters": ["name: 林霄; role: 主角; 用户指定"],
                "world_rules": ["灵渊会吞噬失衡世界"]
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let script_noise_body = complete_cjk_test_body("林霄走入雪夜，忽然听见 الليل 中传来剑鸣。");
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "content": script_noise_body
                })
                .to_string(),
            )
            .await
            .expect("draft");
        let draft: serde_json::Value = serde_json::from_str(&draft).expect("draft json");

        let artifact_path = draft["artifact_path"].as_str().expect("artifact path");
        let saved = tokio::fs::read_to_string(artifact_path)
            .await
            .expect("saved chapter");
        assert!(!saved.contains("الليل"));
        assert!(draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .all(|issue| !issue
                .as_str()
                .unwrap_or_default()
                .contains("unexpected non-CJK script")));

        seal_test_chapter_authority(&tool, project_path, 2).await;
        let embedded_body = complete_cjk_test_body("林霄走入雪夜，忽然听见林as的声音从门后传来。");
        let embedded = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 2,
                    "chapter_title": "门后声音",
                    "content": embedded_body
                })
                .to_string(),
            )
            .await
            .expect("embedded Latin draft");
        let embedded: serde_json::Value =
            serde_json::from_str(&embedded).expect("embedded draft json");
        let embedded_path = embedded["artifact_path"].as_str().expect("artifact path");
        let embedded_saved = tokio::fs::read_to_string(embedded_path)
            .await
            .expect("embedded chapter");
        assert!(!embedded_saved.contains("林as的"));
        assert!(!embedded["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("embedded Latin fragment")));

        seal_test_chapter_authority(&tool, project_path, 3).await;
        let escaped_body = complete_cjk_test_body("林霄走入雪夜，忽然听见门后传来剑鸣。n\n他停住脚步。");
        let escaped_newline = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 3,
                    "content": escaped_body
                })
                .to_string(),
            )
            .await
            .expect("escaped newline draft");
        let escaped_newline: serde_json::Value =
            serde_json::from_str(&escaped_newline).expect("escaped newline draft json");
        let escaped_path = escaped_newline["artifact_path"]
            .as_str()
            .expect("escaped artifact path");
        let escaped_saved = tokio::fs::read_to_string(escaped_path)
            .await
            .expect("escaped saved chapter");
        assert!(!escaped_saved.contains("。n\n"));
        assert!(escaped_newline["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .all(|issue| !issue
                .as_str()
                .unwrap_or_default()
                .contains("escaped newline residue")));

        seal_test_chapter_authority(&tool, project_path, 4).await;
        let adjacent_body = complete_cjk_test_body("林霄听见大地呻나。\\나他握紧断剑，沿着灵渊继续前行。");
        let adjacent_noise = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 4,
                    "chapter_title": "断剑灵渊",
                    "content": adjacent_body
                })
                .to_string(),
            )
            .await
            .expect("adjacent script noise draft");
        let adjacent_noise: serde_json::Value =
            serde_json::from_str(&adjacent_noise).expect("adjacent noise draft json");
        let artifact_path = adjacent_noise["artifact_path"]
            .as_str()
            .expect("artifact path");
        let saved = tokio::fs::read_to_string(artifact_path)
            .await
            .expect("saved chapter");
        assert!(!saved.contains('나'));
        assert!(!saved.contains("\\他"));
        assert!(saved.contains("。他握紧断剑"));
        assert!(!adjacent_noise["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("unexpected non-CJK script")));

        seal_test_chapter_authority(&tool, project_path, 5).await;
        let cjk_space_body = complete_cjk_test_body("林霄发现灵渊吞 量异常，立刻让阵列降频。");
        let cjk_space_noise = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 5,
                    "chapter_title": "阵列降频",
                    "content": cjk_space_body
                })
                .to_string(),
            )
            .await
            .expect("cjk space noise draft");
        let cjk_space_noise: serde_json::Value =
            serde_json::from_str(&cjk_space_noise).expect("cjk space noise draft json");
        assert_eq!(cjk_space_noise["runtime_effect"], "artifact.checkpointed");
        let space_artifact_path = cjk_space_noise["artifact_path"]
            .as_str()
            .expect("artifact path");
        let space_saved = tokio::fs::read_to_string(space_artifact_path)
            .await
            .expect("saved chapter");
        assert!(!space_saved.contains("吞 量"));
        assert!(space_saved.contains("吞量"));
        assert!(!cjk_space_noise["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap_or_default()
                .contains("unexpected whitespace inside CJK phrase")));
    }
