    #[test]
    fn manifest_character_anchors_reject_contaminated_name_extensions_without_rewriting_prose() {
        let mut manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "尘寰破劫录".to_string(),
            title_state: TitleState::default(),
            language: "中文".to_string(),
            genre: "异世界重生玄幻".to_string(),
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
                premise: "底层少年重生后逆袭。".to_string(),
                themes: Vec::new(),
                characters: vec![
                    "name: 凌衡烬; role: 主角".to_string(),
                    "name: 洛微霜; role: 同伴".to_string(),
                ],
                world_rules: Vec::new(),
                style_rules: Vec::new(),
                must_avoid: Vec::new(),
                outline: String::new(),
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
        for number in 1..=2 {
            manifest.chapters.push(ChapterRecord {
                number,
                title: format!("第{number}章"),
                volume_id: String::new(),
                volume_title: String::new(),
                path: format!("chapters/{number:04}.md"),
                summary: "凌衡烬通碎石铺吸收能量，洛微霜提醒测灵仪会检查余量。".to_string(),
                unit_count: 2500,
                status: "approved".to_string(),
                key_facts: vec!["凌衡烬通识到风险升级。".to_string()],
                continuity_updates: vec!["能量波动与余量检查带来压力。".to_string()],
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            });
        }

        let anchors = manifest_character_anchors(&manifest);

        assert!(anchors.contains(&"凌衡烬".to_string()));
        assert!(anchors.contains(&"洛微霜".to_string()));
        assert!(!anchors.contains(&"凌衡烬通".to_string()));
        assert!(!anchors.contains(&"衡烬".to_string()));
        assert!(!anchors.contains(&"余量".to_string()));
        assert!(!anchors.contains(&"能量".to_string()));
        let repaired =
            repair_contract_character_name_typos(&manifest, "凌衡烬通行证贴近测灵仪，洛微霜抬手示意。");
        assert!(repaired.contains("凌衡烬通行证"));
        assert!(repaired.contains("洛微霜抬手示意"));
    }

    #[test]
    fn character_drift_skips_common_locative_phrases() {
        let candidates = near_anchor_cjk_name_variants("林间传来风声，林墨停下脚步。", "林墨");

        assert!(!candidates.contains("林间"));
    }

    #[test]
    fn character_drift_skips_common_time_phrases() {
        assert!(stable_character_anchor_name("时存").is_none());
        assert!(stable_character_anchor_name("时刻").is_none());
        let candidates = near_anchor_cjk_name_variants("关键时刻，规则裂缝忽然静止。", "时存");

        assert!(!candidates.contains("时刻"));
    }

    #[test]
    fn chinese_script_sanitizer_removes_numeric_adjacent_foreign_residue() {
        let mut manifest = test_manifest_with_primary_character();
        manifest.language = "Chinese".to_string();
        let cleaned =
            sanitize_chinese_script_noise(&manifest, "逻辑偏差指数：0.나。她没有立刻回答。");

        assert!(cleaned.contains("逻辑偏差指数：0.。"));
        assert!(!cleaned.contains('나'));
    }

    #[tokio::test]
    async fn write_draft_repairs_contract_character_typo_before_quality_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"角色修复测试","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "唐栖晚调查城市裂缝。",
                "characters": ["name: 唐栖晚; role: 主角", "name: 秦栖宁; role: 同伴"],
                "world_rules": ["记忆碎片会改写现实"]
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
        let companion_name = manifest["character_ledger"]
            .as_array()
            .expect("character ledger")
            .iter()
            .find(|entry| entry["role"].as_str().unwrap_or_default().contains("同伴"))
            .or_else(|| {
                manifest["character_ledger"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| {
                        entry["canonical_name"].as_str().unwrap_or_default() != primary_name
                    })
            })
            .and_then(|entry| entry["canonical_name"].as_str())
            .expect("companion character")
            .to_string();
        let mut typo_chars = companion_name.chars().collect::<Vec<_>>();
        if typo_chars.len() >= 3 {
            let last = typo_chars.len() - 1;
            typo_chars.swap(last - 1, last);
        }
        let companion_typo = typo_chars.into_iter().collect::<String>();
        seal_test_chapter_authority(&tool, project_path, 1).await;
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "旧城裂隙",
                    "content": format!("{primary_name}进入旧城区，{companion_typo}站在封锁线旁提醒她不要过度感知。记忆碎片会改写现实，街道尽头的裂缝正在扩大。{primary_name}决定先记录裂缝的频率。")
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

        assert!(
            saved.contains(&companion_name),
            "draft: {draft}\nsaved chapter:\n{saved}"
        );
        assert!(!saved.contains(&companion_typo));
        assert!(!draft["quality_gate"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue.as_str().unwrap_or_default().contains(&companion_typo)));
    }

    #[tokio::test]
    async fn execution_package_registers_new_character_before_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"本地命名测试","language":"Chinese","genre":"都市玄幻","target_units":100000,"chapter_unit_target":2500}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "南棠白调查城市灵脉。",
                "characters": ["name: 南棠白; role: 主角", "name: 唐知舟; role: 同伴"],
                "world_rules": ["灵脉会在夜里改写街区秩序"],
                "reader_promise": {
                    "core_hook": "城市灵脉为何在夜里改写街区秩序"
                }
            })
            .to_string(),
        )
        .await
        .expect("contract");
        tool.call(
            &serde_json::json!({
                "action": "compose_context",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("base context");
        let package = tool
            .call(
                &serde_json::json!({
                    "action": "persist_execution_package",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "plan": "南棠白追踪夜市灵脉并得到残缺钥匙。",
                    "content": "场景一：南棠白在废弃站台遇到线索提供者；场景二：线索提供者交出钥匙。",
                    "summary": "追踪灵脉并取得钥匙",
                    "new_character_requests": [{
                        "request_id": "station-informant",
                        "role": "废弃站台的线索提供者",
                        "importance": "volume_recurring",
                        "narrative_purpose": "交出钥匙并隐瞒灵脉来源",
                        "planned_entry": "第1章",
                        "planned_exit": "本卷结束",
                        "relationship_to_existing": "暂时帮助南棠白",
                        "desire": "查清灵脉失控的真正源头",
                        "fear": "钥匙落入操控灵脉的人手中",
                        "bottom_line": "不拿无辜者换取情报",
                        "arc_start": "只肯向南棠白透露最低限度的线索",
                        "arc_end": "决定共同揭开本卷的灵脉真相",
                        "voice_style": "谨慎简短，重要信息分层透露"
                    }]
                })
                .to_string(),
            )
            .await
            .expect("execution package");
        let package: serde_json::Value = serde_json::from_str(&package).expect("package json");
        let registration = &package["character_registrations"][0];
        let declared_name = registration["canonical_name"]
            .as_str()
            .expect("declared name");
        assert!(!declared_name.trim().is_empty());
        assert_ne!(declared_name, "station-informant");
        assert_eq!(registration["request_id"], "station-informant");

        let manifest = tokio::fs::read_to_string(std::path::Path::new(project_path).join("project.json"))
            .await
            .expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("manifest json");
        let ledger = manifest["character_ledger"]
            .as_array()
            .expect("character ledger");
        let pending = ledger
            .iter()
            .find(|entry| entry["canonical_name"] == declared_name)
            .expect("pending structured character");
        assert_eq!(pending["status"], "pending:chapter-1");
        assert_eq!(pending["name_source"], "local_character_allocator");
        assert_eq!(pending["aliases"], serde_json::json!([]));
        assert!(pending["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("character-chapter-")));
    }

    #[tokio::test]
    async fn write_draft_preserves_normal_chinese_dialogue_when_saving_chapter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"保存清洗测试","language":"Chinese"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "沈渡调查城市裂隙。",
                "characters": [
                    "name: 沈渡; role: 主角",
                    "name: 陆清漪; role: 重要角色",
                    "name: 严廷; role: 关键对手"
                ],
                "world_rules": ["灵力碎片会导致现实扭曲"]
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let content = "沈渡站在天桥的护栏边，指尖摩挲着那张已经泛黄的寻人启事。\n\n“如果不找，我无法确定他是在这个世界，还是在‘那边’。”沈渡的声音沙哑，他看向远处的大厦。\n\n“你要去拿它？”陆清漪察觉到了沈渡眼神中的变化，她的声音里透出一丝警告。\n\n随着严廷的身影消失在街道尽头的阴影里，陆清漪转过身，看着沈渡，眼神复杂。\n\n沈渡没有说话，他只是看向远方。";
        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "write_draft",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "霓虹裂隙与白光碎片",
                    "content": content,
                    "summary": "沈渡遭遇城市裂隙。",
                    "key_facts": ["沈渡看到裂隙。"],
                    "continuity_updates": ["沈渡开始调查裂隙。"]
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

        assert!(saved.contains("沈渡站在天桥"), "{saved}");
        assert!(saved.contains("沈渡的声音沙哑"), "{saved}");
        assert!(saved.contains("陆清漪察觉到了沈渡眼神中的变化"), "{saved}");
        assert!(saved.contains("严廷的身影消失"), "{saved}");
        assert!(saved.contains("陆清漪转过身"), "{saved}");
        assert!(saved.contains("沈渡没有说话"), "{saved}");
        assert!(!saved.contains("沈渡天桥"), "{saved}");
        assert!(!saved.contains("沈渡音沙哑"), "{saved}");
        assert!(!saved.contains("陆清漪到了沈渡"), "{saved}");
        assert!(!saved.contains("严廷身影消失"), "{saved}");
        assert!(!saved.contains("陆清漪身"), "{saved}");
        assert!(!saved.contains("沈渡说话"), "{saved}");
    }

    #[test]
    fn character_drift_skips_ability_and_modal_phrases() {
        for phrase in ["能力", "权力", "战力", "修为", "灵能", "能用", "能地"] {
            assert!(
                stable_character_anchor_name(phrase).is_none(),
                "{phrase} should not become a character anchor"
            );
        }
        let candidates = near_anchor_cjk_name_variants("他终于能用这份能力站稳脚跟。", "能力");

        assert!(!candidates.contains("能用"));
    }

    #[test]
    fn character_drift_skips_same_prefix_verb_phrases() {
        let candidates = near_anchor_cjk_name_variants(
            "识到危机已经逼近，识拉住门闩，却没有看见识尘留下的标记。",
            "识尘",
        );

        assert!(!candidates.contains("识到"));
        assert!(!candidates.contains("识拉"));
    }

    #[test]
    fn chinese_title_language_rejects_workflow_surface_words() {
        for title in [
            "第3章：继承",
            "第4章：推进",
            "阶段转折",
            "第23章：一章入口",
            "第23章：随着世界",
            "第23章：着世界秩",
            "第2章：墨辞在废",
            "第3章：墨辞在平",
            "第24章：异世界重生玄幻",
            "第24章：段展开",
            "到一阵",
            "墨辞觉到",
            "时那股令",
        ] {
            let issue = chinese_title_language_issues(title).expect("title issue");
            assert!(
                issue.contains("workflow/control")
                    || issue.contains("sentence fragment")
                    || issue.contains("sensory fragment")
                    || issue.contains("demonstrative fragment")
                    || issue.contains("genre/category"),
                "{title}: {issue}"
            );
        }
        assert!(chinese_title_language_issues("裂痕中的微光").is_none());
        assert!(chinese_title_language_issues("天枢中心B-7区域的契约与反噬").is_none());
        assert!(
            chinese_title_language_issues("Project X的契约").is_some(),
            "English word fragments should still be rejected"
        );
    }

    #[test]
    fn chapter_title_quality_requires_story_evidence() {
        let mut manifest = test_manifest_with_primary_character();
        manifest.chapter_unit_target = None;
        let chapter = ChapterRecord {
            number: 5,
            title: "霜河旧钟".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0005.md".to_string(),
            summary: "黎启洄在黑炉前公开拒绝牺牲同伴，并夺回被扣押的试炼牌。".to_string(),
            unit_count: 2500,
            status: "draft".to_string(),
            key_facts: vec!["黎启洄夺回试炼牌。".to_string()],
            continuity_updates: vec!["黑炉试炼规则被迫公开。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        let content = "黎启洄站到黑炉前，夺回试炼牌，也让旁听生第一次获得公开申诉的资格。";

        let quality_gate = chapter_quality_gate(&manifest, &chapter, content, &[]);
        let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

        assert!(quality_gate.passed);
        assert!(metadata_gate.repairable.iter().any(|issue| {
            issue.contains("章节标题没有被本章摘要")
                && issue.contains("not grounded in chapter evidence")
        }));
        assert!(metadata_gate.warnings.is_empty());
    }

    #[test]
    fn chapter_metadata_gate_repairs_bare_abstract_concept_stack_title() {
        let mut manifest = test_manifest_with_primary_character();
        manifest.chapter_unit_target = None;
        let chapter = ChapterRecord {
            number: 2,
            title: "频率过载".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0002.md".to_string(),
            summary: "黎启洄在旧站救下同伴，并确认异常频率会撕裂站台下的封印。".to_string(),
            unit_count: 2500,
            status: "draft".to_string(),
            key_facts: vec!["黎启洄在旧站救下同伴。".to_string()],
            continuity_updates: vec!["旧站封印出现裂纹。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        let content = "黎启洄冲进旧站月台，拖住即将坠入裂缝的同伴，亲眼看见封印石上的频纹被撕开。";

        let quality_gate = chapter_quality_gate(&manifest, &chapter, content, &[]);
        let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

        assert!(quality_gate.passed);
        assert!(
            metadata_gate.needs_repair(),
            "abstract title must require metadata repair: {metadata_gate:?}"
        );
    }

    #[test]
    fn chapter_metadata_gate_repairs_body_fragment_title() {
        let manifest = test_manifest_with_primary_character();
        let chapter = ChapterRecord {
            number: 6,
            title: "手中长剑化".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0006.md".to_string(),
            summary: "黎启洄在旧站月台截住敌人的突袭，并迫使封印裂纹暴露。".to_string(),
            unit_count: 2500,
            status: "draft".to_string(),
            key_facts: vec!["黎启洄在旧站月台挡下突袭。".to_string()],
            continuity_updates: vec!["旧站封印裂纹暴露。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        let content =
            "敌人身形暴起，手中长剑化作一道流光，直劈黎启洄头顶。黎启洄反手扣住试炼牌，逼得封印裂纹在月台下显形。";

        let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

        assert!(
            metadata_gate
                .repairable
                .iter()
                .any(|issue| issue.contains("prose fragment")),
            "body-fragment chapter titles must be repaired as metadata: {:?}",
            metadata_gate
        );
    }

    #[test]
    fn chapter_record_normalizes_literal_escaped_newlines() {
        let normalized = normalize_chapter_body_for_record("第一段\\n第二段", "第一章");

        assert_eq!(normalized, "第一段\n第二段");
    }

    #[test]
    fn chapter_record_strips_model_generated_leading_chapter_heading() {
        let normalized = normalize_chapter_body_for_record(
            "# 第一章：律令的裂痕与感官的剥夺\n\n陆离站在晋升广场上。",
            "悬浮钟塔",
        );

        assert_eq!(normalized, "陆离站在晋升广场上。");
    }

    #[test]
    fn chinese_surface_punctuation_cleanup_removes_unbalanced_quote_noise() {
        let cleaned =
            normalize_chinese_surface_punctuation("陆远听见‘节点坍塌，机械臂``发出低鸣。");

        assert!(cleaned.contains("陆远听见节点坍塌，机械臂发出低鸣。"));
        assert!(!cleaned.contains('‘'));
        assert!(!cleaned.contains('`'));
    }

    #[test]
    fn chinese_surface_cleanup_removes_structured_tail_and_repetition_noise() {
        let raw = format!(
            "{}{}\n    \"summary\": \"wrong tail\"",
            "陆远穿过节点，看见世界的底层频率。".repeat(10),
            "虚".repeat(32)
        );
        let cleaned = strip_embedded_structured_field_residue_from_chinese_prose(
            &collapse_excessive_repeated_cjk_chars(&raw),
        );

        assert!(cleaned.contains("陆远穿过节点"));
        assert!(!cleaned.contains("\"summary\""));
        assert!(!cleaned.contains(&"虚".repeat(8)));
    }

    #[test]
    fn chinese_surface_cleanup_removes_foreign_runs_between_cjk_with_spaces() {
        let cleaned = strip_adjacent_foreign_alpha_runs_from_chinese_text("电 going 电磁弧光");

        assert_eq!(cleaned, "电  电磁弧光");
    }

    #[test]
    fn chapter_record_removes_artifact_receipt_surface_before_body() {
        let record = ChapterRecord {
            number: 1,
            title: "第一章：裂痕与微光".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: String::new(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            status: "revised".to_string(),
            unit_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let rendered = render_chapter_file(
            &record,
            "# 第一章：裂痕与微光\n\n第1章：字数：3420左右，文件路径：/tmp/novel/第1章.txt，修改摘要：删除元文本。修改状态：已完成。\n\n# 第一章：裂痕与微光\n\n林舒推开会议室的门。",
        );

        assert!(!rendered.contains("修改摘要"));
        assert!(!rendered.contains("文件路径"));
        assert_eq!(rendered.matches("# 第一章：裂痕与微光").count(), 1);
        assert!(!rendered.contains("\n第一章：裂痕与微光\n"));
        assert!(rendered.contains("林舒推开会议室的门。"));
    }

    #[test]
    fn chapter_record_removes_stacked_leading_title_metadata() {
        let record = ChapterRecord {
            number: 1,
            title: "雨夜天桥".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: String::new(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            status: "revised".to_string(),
            unit_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let rendered = render_chapter_file(
            &record,
            "# 雨水顺\n\n# 雨水顺着\n\n# 庭川站\n\n# 晏庭川站\n\n夜幕下的城市开始发亮。",
        );

        assert_eq!(rendered.matches("# 雨夜天桥").count(), 1);
        assert!(!rendered.contains("# 雨水顺"));
        assert!(!rendered.contains("# 雨水顺着"));
        assert!(!rendered.contains("# 庭川站"));
        assert!(!rendered.contains("# 晏庭川站"));
        assert!(rendered.contains("夜幕下的城市开始发亮。"));
    }

    #[test]
    fn prompt_context_keeps_full_context_out_of_stage_prompts() {
        let long_truth = "真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10);
        let long_source = "素材".repeat(PROMPT_SOURCE_EXCERPT_CHARS + 10);
        let context = json!({
            "project": {
                "title": "测试",
                    "chapter_unit_target": 5000
            },
            "contract": {
                "premise": "前提".repeat(PROMPT_CONTRACT_TEXT_CHARS + 10),
                "outline": "大纲".repeat(PROMPT_CONTRACT_TEXT_CHARS + 10),
                "characters": ["name: 陆沉; identity: 主角".repeat(PROMPT_CONTRACT_ITEM_CHARS + 10)],
                "structured_contract_v2": {
                    "summary": ["节奏控制".repeat(PROMPT_CONTRACT_ITEM_CHARS + 10)]
                }
            },
            "story_bible": {
                "narrative_graph": {
                    "global_spine": "全书主线".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                    "chapter_goals": [
                        "章节目标1".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标2".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标3".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标4".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标5".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标6".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标7".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标8".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10),
                        "章节目标9".repeat(PROMPT_STORY_BIBLE_TEXT_CHARS + 10)
                    ]
                }
            },
            "truth_files": [{
                "section": "continuity",
                "content": long_truth
            }],
            "sources": [{
                "title": "source",
                "excerpt": long_source
            }]
        });

        let prompt_context = build_prompt_context_payload(&context);

        assert_eq!(
            prompt_context["project"]["chapter_unit_target"].as_i64(),
            Some(5000)
        );
        assert_eq!(
            prompt_context["contract"]["characters"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert!(
            prompt_context["contract"]["premise"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= PROMPT_CONTRACT_TEXT_CHARS + 3
        );
        assert!(
            prompt_context["contract"]["characters"][0]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= PROMPT_CONTRACT_ITEM_CHARS + 3
        );
        assert!(
            prompt_context["story_bible"]["narrative_graph"]["global_spine"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= PROMPT_STORY_BIBLE_TEXT_CHARS + 3
        );
        assert_eq!(
            prompt_context["story_bible"]["narrative_graph"]["chapter_goals"]
                .as_array()
                .map(Vec::len),
            Some(PROMPT_STORY_BIBLE_ARRAY_ITEMS)
        );
        assert_eq!(
            prompt_context["prompt_packaging"]["schema"].as_str(),
            Some("compact_prompt_context.v1")
        );
        assert_eq!(
            prompt_context["truth_files"][0]["content_truncated"].as_bool(),
            Some(true)
        );
        assert_eq!(
            prompt_context["sources"][0]["excerpt_truncated"].as_bool(),
            Some(true)
        );
        assert!(
            prompt_context["truth_files"][0]["content"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= PROMPT_TRUTH_FILE_CHARS + 3
        );
        assert!(
            prompt_context["sources"][0]["excerpt"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= PROMPT_SOURCE_EXCERPT_CHARS + 3
        );
    }

    #[test]
    fn prompt_context_caps_truth_total_budget() {
        let long_truth = "真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10);
        let context = json!({
            "truth_files": [
                { "section": "one", "content": long_truth },
                { "section": "two", "content": "后续真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10) },
                { "section": "three", "content": "归档真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10) },
                { "section": "four", "content": "索引真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10) },
                { "section": "five", "content": "人物真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10) },
                { "section": "six", "content": "伏笔真相".repeat(PROMPT_TRUTH_FILE_CHARS + 10) }
            ]
        });

        let prompt_context = build_prompt_context_payload(&context);
        let total = prompt_context["truth_files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["content"].as_str())
            .map(|content| content.chars().count())
            .sum::<usize>();

        assert!(total <= PROMPT_TRUTH_TOTAL_CHARS + 3);
    }

    #[test]
    fn truth_file_body_strips_wrapper_heading_once() {
        let raw = "# chapter_summaries\n\n第1章：主角出发。\n";

        assert_eq!(
            truth_file_body("chapter_summaries", raw),
            "第1章：主角出发。"
        );
    }

    #[test]
    fn chapter_summary_truth_normalization_removes_recursive_headers() {
        let raw = "# chapter_summaries\n\n# chapter_summaries\n\n第1章：主角出发。\n第1章：主角出发。\n第2章：主角抵达城门并发现异常。".to_string();

        let normalized = normalize_truth_section_content("chapter_summaries", &raw, "zh");

        assert!(!normalized.contains("# chapter_summaries"));
        assert_eq!(normalized.matches("第1章：主角出发。").count(), 1);
        assert!(normalized.contains("第2章：主角抵达城门并发现异常。"));
    }

    #[test]
    fn structured_current_state_remains_complete_valid_json() {
        let raw = json!({
            "source": "approved_typed_state_changes",
            "last_approved_chapter": 9,
            "characters": [
                {
                    "id": "character-0001",
                    "name": "叶清序",
                    "current_state": "Established by project contract; update only from approved chapters."
                },
                {
                    "id": "character-0002",
                    "name": "宋望岚",
                    "current_state": "她已从技术认可转为战术信任。".repeat(80)
                }
            ]
        })
        .to_string();

        let normalized = normalize_truth_section_content("current_state", &raw, "zh");
        let reparsed: serde_json::Value =
            serde_json::from_str(&normalized).expect("current state must remain valid JSON");

        assert_eq!(reparsed["last_approved_chapter"], 9);
        assert_eq!(reparsed["characters"].as_array().unwrap().len(), 2);
        assert!(
            normalized.contains("战术信任"),
            "structured typed state must not be routed through prose sentence compaction"
        );
    }

    #[test]
    fn chapter_metadata_compaction_bounds_summary_and_facts() {
        let summary =
            "第一句发生了很长的事情。第二句继续推进。第三句留下伏笔。第四句不应该进入短摘要。";
        let compact = compact_chapter_summary(summary, "zh");
        let items = (0..20)
            .map(|index| format!("第{index}条事实{}", "很长".repeat(200)))
            .collect::<Vec<_>>();

        assert!(!compact.contains("第四句"));
        assert!(compact.chars().count() <= CHAPTER_SUMMARY_MAX_CHARS);
        let compact_items = compact_truth_items(items, CHAPTER_FACT_LIMIT);
        assert_eq!(compact_items.len(), CHAPTER_FACT_LIMIT);
        assert!(compact_items
            .iter()
            .all(|item| item.chars().count() <= CHAPTER_FACT_MAX_CHARS));
    }

    #[test]
    fn compact_truth_items_drops_bare_character_name_fragments() {
        let items = vec![
            "季曜隅".to_string(),
            "季曜隅在石碑前确认血脉代价。".to_string(),
            "洛夙阙".to_string(),
            "血脉觉醒".to_string(),
        ];

        let compact_items = compact_truth_items(items, CHAPTER_FACT_LIMIT);

        assert!(!compact_items.iter().any(|item| item == "季曜隅"));
        assert!(!compact_items.iter().any(|item| item == "洛夙阙"));
        assert!(compact_items
            .iter()
            .any(|item| item.contains("季曜隅在石碑前确认血脉代价")));
        assert!(compact_items.iter().any(|item| item == "血脉觉醒"));
    }

    #[tokio::test]
    async fn compact_longform_state_archives_old_summaries_and_keeps_context_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_dir = dir.path();
        tokio::fs::create_dir_all(project_dir.join("archives"))
            .await
            .expect("archives");
        tokio::fs::create_dir_all(project_dir.join("truth"))
            .await
            .expect("truth");
        let mut manifest = NovelProjectManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            title: "长篇测试".to_string(),
            title_state: TitleState::default(),
            language: "zh".to_string(),
            genre: "玄幻".to_string(),
            brief: "测试长篇治理".to_string(),
            target_units: Some(5_000_000),
            chapter_unit_target: Some(8_000),
            max_chapters_per_turn: None,
            export_format: None,
            export_when_complete: false,
            approved_only: false,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            sources: Vec::new(),
            chapter_plans: Vec::new(),
            chapter_contracts: Vec::new(),
            context_packages: Vec::new(),
            chapter_architectures: Vec::new(),
            chapters: (1..=30)
                .map(|number| ChapterRecord {
                    number,
                    title: format!("第{number}章"),
                    volume_id: String::new(),
                    volume_title: String::new(),
                    path: format!("chapters/{number:04}.md"),
                    summary: format!("第{number}章摘要：主角完成阶段推进。"),
                    unit_count: 4000,
                    status: "approved".to_string(),
                    key_facts: vec![format!("第{number}章关键事实")],
                    continuity_updates: vec![format!("第{number}章连续性更新")],
                    created_at: "now".to_string(),
                    updated_at: "now".to_string(),
                })
                .collect(),
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

        compact_longform_state(project_dir, &mut manifest)
            .await
            .expect("compact");
        write_approved_settlement(
            project_dir,
            30,
            &SettlementOutput {
                chapter_fingerprint: String::new(),
                body_fingerprint: String::new(),
                authority_fingerprint: String::new(),
                state_changes: Vec::new(),
                degraded_reason: String::new(),
                current_state: "主角留在钟楼，异常仍在持续，手中的钥匙已经断裂。".to_string(),
                pending_hooks: "钟楼下方仍有未开启的暗门。".to_string(),
                chapter_summary: "主角抵达钟楼并以钥匙断裂为代价确认暗门。".to_string(),
                continuity_updates: vec!["钥匙已经断裂。".to_string()],
                resolved_hooks: Vec::new(),
            },
        )
        .await
        .expect("approved settlement");
        let context = build_context_payload(project_dir, &manifest, 31)
            .await
            .expect("context");
        let recent = context["recent_chapters"].as_array().unwrap();
        let archives = context["archives"].as_array().unwrap();
        let continuity = tokio::fs::read_to_string(project_dir.join("truth/continuity-index.md"))
            .await
            .expect("continuity");

        assert_eq!(recent.len(), CONTEXT_RECENT_CHAPTER_LIMIT);
        assert_eq!(recent[0]["number"], 30);
        assert_eq!(recent[0]["source"], "approved_final_body_settlement");
        assert!(recent[0]["current_state"]
            .as_str()
            .is_some_and(|value| value.contains("钥匙已经断裂")));
        assert!(recent[0].get("key_facts").is_none());
        assert!(!archives.is_empty());
        assert!(manifest
            .archives
            .iter()
            .any(|archive| archive.kind == "arc"));
        assert!(continuity.contains("Active Approved Chapters"));
        assert!(continuity.contains("第30章"));
        assert!(!continuity.contains("### Chapter 1: 第1章"));
    }

    #[test]
    fn chapter_path_slug_keeps_chinese_title_compact() {
        assert_eq!(slugify("第 1 章：秩序的裂痕"), "第1章秩序的裂痕");
        assert_eq!(
            slugify("Chapter One: Broken Gate"),
            "chapter-one-broken-gate"
        );
    }

    #[test]
    fn chapter_continuity_updates_fall_back_to_key_facts() {
        let mut chapter = ChapterRecord {
            number: 1,
            title: "第1章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "林墨发现稳定频率。".to_string(),
            unit_count: 0,
            status: "drafted".to_string(),
            key_facts: vec!["林墨在失控中感知到稳定频率。".to_string()],
            continuity_updates: Vec::new(),
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        let manifest = test_manifest_with_primary_character();
        ensure_chapter_continuity_updates(&manifest, &mut chapter, "正文");
        assert_eq!(
            chapter.continuity_updates,
            vec!["林墨在失控中感知到稳定频率。".to_string()]
        );
    }

    #[test]
    fn settlement_validation_allows_paraphrased_chinese_state() {
        let body = "陆沉在虚空裂隙边缘发现上古遗物，触碰后看见正在断裂的法则线条，并意识到世界崩塌并非自然衰减。";
        let settlement = SettlementOutput {
            chapter_fingerprint: String::new(),
            body_fingerprint: String::new(),
            authority_fingerprint: String::new(),
            state_changes: Vec::new(),
            degraded_reason: String::new(),
            current_state: "陆沉获得遗物并从拾荒者转向法则感知者。".to_string(),
            pending_hooks: String::new(),
            chapter_summary: "陆沉在裂隙边缘获得上古遗物，觉醒法则感知。".to_string(),
            continuity_updates: vec!["陆沉在虚空裂隙边缘发现并获得了一件上古遗物。".to_string()],
            resolved_hooks: Vec::new(),
        };
        let validation = deterministic_state_validation(body, &settlement);
        assert!(validation.passed, "{:?}", validation.warnings);
    }

    #[tokio::test]
    async fn novel_studio_creates_project_adds_chapter_and_exports() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(
                r#"{"action":"init_project","title":"Test Novel","language":"en"}"#,
            )
            .await
            .expect("init");
        let init_value: serde_json::Value = serde_json::from_str(&init).expect("json");
        let project_path = init_value["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "add_source",
                "project_path": project_path,
                "source_title": "seed",
                "content": "A source fragment with a city, a promise, and a broken gate."
            })
            .to_string(),
        )
        .await
        .expect("source");

        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "A courier repairs a city's memory.",
                "themes": ["Memory must be chosen, not owned."],
                "characters": [
                    "name: Mara; role: courier; desire: restore the city memory; fear: becoming another stored ghost; bottom_line: will not erase living people; character_id: character-mara; name_source: user",
                    "name: Iven; role: map keeper; desire: protect the glass archive; fear: losing the last route home; bottom_line: will not trade names for power; character_id: character-iven; name_source: user"
                ],
                "world_rules": ["Memory can be stored in glass."],
                "style_rules": ["Scene-first literary fantasy prose."],
                "outline": "Opening, failure, recovery."
            })
            .to_string(),
        )
        .await
        .expect("contract");

        seal_test_chapter_authority(&tool, project_path, 1).await;
        let chapter_body = complete_english_test_body("Mara reached the broken gate before dawn, carrying the brass message tube under her coat while rain moved through the ruined market. Iven waited beside the glass map and showed her three streets that had vanished from the city's memory. Mara chose the dangerous route through the archive furnace, because the missing streets still held living names, not dead records. She repaired the first hinge with wire from her courier badge, traded her safe passage for the map keeper's trust, and promised that no stored ghost would be erased for convenience. By sunrise, the gate opened just enough for both of them to hear the city remember its own bells.");
        let chapter = tool
            .call(&serde_json::json!({
                "action": "write_draft",
                "project_path": project_path,
                "chapter_title": "The Gate",
                "summary": "Mara reaches the broken gate, earns Iven's trust, and opens a path into the city's missing memory.",
                "content": &chapter_body,
                "key_facts": [
                    "Mara reaches the broken gate before dawn.",
                    "Iven shows Mara a glass map with three vanished streets.",
                    "Mara trades safe passage for Iven's trust and repairs the first hinge."
                ],
                "continuity_updates": [
                    "Mara and Iven can now enter the archive furnace route.",
                    "The city begins to remember its bells after the gate opens."
                ]
            }).to_string())
            .await
            .expect("chapter");
        let chapter_value: serde_json::Value = serde_json::from_str(&chapter).expect("json");
        assert_eq!(chapter_value["quality_gate"]["passed"], true);
        assert_eq!(
            chapter_value["outcome_status"],
            "accepted",
            "{chapter_value}"
        );
        let txt_path = chapter_value["txt_artifact_path"]
            .as_str()
            .expect("txt artifact path");
        let txt_collection_path = chapter_value["txt_collection_path"]
            .as_str()
            .expect("txt collection path");
        assert!(txt_path.ends_with("exports/current.txt"));
        assert!(txt_collection_path.ends_with("exports/章节合集.txt"));
        let synced_txt = tokio::fs::read_to_string(txt_path).await.expect("read txt");
        assert!(synced_txt.contains("Test Novel"));
        assert!(synced_txt.contains("The Gate"));
        assert!(!synced_txt.contains("---\nnumber:"));
        assert_eq!(
            chapter_value["preferred_artifact_path"],
            chapter_value["txt_artifact_path"]
        );
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
        .expect("audit chapter");
        tool.call(
            &serde_json::json!({
                "action": "review_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "verdict": "pass",
                "feedback": "The chapter preserves the contract and its visible state changes."
            })
            .to_string(),
        )
        .await
        .expect("review chapter");
        tool.call(
            &serde_json::json!({
                "action": "settle_chapter_state",
                "project_path": project_path,
                "chapter_number": 1,
                "content": serde_json::json!({
                    "state_changes": [],
                    "current_state": "Mara and Iven can enter the archive furnace route.",
                    "pending_hooks": "The city has begun to remember its bells.",
                    "chapter_summary": "Mara reaches the broken gate, earns Iven's trust, and opens a path into the city's missing memory.",
                    "continuity_updates": [
                        "Mara and Iven can now enter the archive furnace route.",
                        "The city begins to remember its bells after the gate opens."
                    ]
                }).to_string()
            })
            .to_string(),
        )
        .await
        .expect("settle chapter");
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
        let approval: serde_json::Value = serde_json::from_str(&approval).expect("approval json");
        assert_eq!(approval["success"], true, "{approval}");

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
            .expect("export");
        let export_value: serde_json::Value = serde_json::from_str(&export).expect("json");
        let output_path = export_value["output_path"].as_str().expect("output");
        assert_eq!(export_value["runtime_effect"], "artifact.written");
        assert_eq!(export_value["format"], "txt");
        assert_eq!(export_value["artifact_path"], output_path);
        assert!(export_value["runtime_effects"]
            .as_array()
            .expect("runtime effects")
            .iter()
            .any(|effect| effect == "artifact.txt"));
        let exported = tokio::fs::read_to_string(output_path).await.expect("read");
        assert!(exported.contains("Test Novel"));
        assert!(exported.contains("The Gate"));
    }

    #[tokio::test]
    async fn readable_current_txt_includes_latest_draft_even_when_collection_is_approved_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_dir = dir.path();
        tokio::fs::create_dir_all(project_dir.join("chapters"))
            .await
            .expect("chapters dir");
        tokio::fs::write(
            project_dir.join("chapters/0001.md"),
            "---\nnumber: 1\ntitle: 第一章\n---\n# 第一章\n\n草稿正文已经写出，等待审稿修订。\n",
        )
        .await
        .expect("chapter");

        let mut manifest = test_manifest_with_primary_character();
        manifest.title = "可读草稿测试".to_string();
        manifest.language = "zh-CN".to_string();
        manifest.approved_only = true;
        manifest.chapters = vec![ChapterRecord {
            number: 1,
            title: "第一章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "草稿已写出但尚未批准。".to_string(),
            unit_count: 24,
            status: "needs_revision".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }];

        let export = export::sync_readable_txt_export(project_dir, &manifest)
            .await
            .expect("sync readable txt");
        let current = tokio::fs::read_to_string(&export.current_path)
            .await
            .expect("current");
        let collection = tokio::fs::read_to_string(&export.collection_path)
            .await
            .expect("collection");

        assert!(current.contains("草稿正文已经写出"), "{current}");
        assert!(
            !collection.contains("草稿正文已经写出"),
            "approved-only collection should not include unapproved draft: {collection}"
        );
    }

    #[tokio::test]
    async fn novel_studio_lists_projects_by_recent_update() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        tool.call(r#"{"action":"init_project","title":"Older","language":"en"}"#)
            .await
            .expect("older init");
        let newer = tool
            .call(r#"{"action":"init_project","title":"Newer","language":"en"}"#)
            .await
            .expect("newer init");
        let newer: serde_json::Value = serde_json::from_str(&newer).expect("newer json");
        let newer_path = newer["project_path"].as_str().expect("newer path");

        let listed = tool
            .call(r#"{"action":"list_projects"}"#)
            .await
            .expect("list");
        let listed: serde_json::Value = serde_json::from_str(&listed).expect("list json");
        let first_path = listed["projects"][0]["path"].as_str().expect("first path");

        assert_eq!(first_path, newer_path);
    }

    #[tokio::test]
    async fn novel_studio_read_missing_chapter_returns_alternatives() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let empty = tool
            .call(r#"{"action":"init_project","title":"Empty","language":"en"}"#)
            .await
            .expect("empty init");
        let empty: serde_json::Value = serde_json::from_str(&empty).expect("empty json");
        let empty_path = empty["project_path"].as_str().expect("empty path");

        let filled = tool
            .call(r#"{"action":"init_project","title":"Filled","language":"en"}"#)
            .await
            .expect("filled init");
        let filled: serde_json::Value = serde_json::from_str(&filled).expect("filled json");
        let filled_path = filled["project_path"].as_str().expect("filled path");
        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": filled_path,
                "chapter_number": 1,
                "chapter_title": "Only Chapter",
                "content": "A stable chapter body with enough content to persist."
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        let missing = tool
            .call(
                &serde_json::json!({
                    "action": "read_chapter",
                    "project_path": empty_path,
                    "chapter_number": 1
                })
                .to_string(),
            )
            .await
            .expect("read missing");
        let missing: serde_json::Value = serde_json::from_str(&missing).expect("missing json");

        assert_eq!(missing["success"], false);
        assert_eq!(missing["recoverable"], true);
        assert_eq!(missing["error_kind"], "chapter_not_found");
        assert_eq!(
            missing["alternative_projects"][0]["path"]
                .as_str()
                .expect("alternative path"),
            filled_path
        );
    }

    #[tokio::test]
    async fn novel_studio_revise_chapter_allows_metadata_only_write_receipt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Metadata Revision","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Opening",
                "content": "The old body remains in place while metadata is revised."
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        let revised = tool
            .call(
                &serde_json::json!({
                    "action": "revise_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "summary": "Metadata-only summary update.",
                    "key_facts": ["The old body remains."],
                    "continuity_updates": ["Metadata-only revision preserves body continuity."]
                })
                .to_string(),
            )
            .await
            .expect("revise");
        let revised: serde_json::Value = serde_json::from_str(&revised).expect("revised json");

        assert_eq!(revised["success"], true);
        assert_eq!(revised["runtime_effect"], "artifact.written");
        assert_eq!(revised["metadata_only"], true);
        assert_eq!(
            revised["chapter"]["summary"],
            "Metadata-only summary update."
        );
    }

    #[tokio::test]
    async fn novel_studio_repair_chapter_metadata_preserves_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Metadata Gate","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        tool.call(
            &serde_json::json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "Mara keeps a rescue engine running through a storm.",
                "characters": ["name: Mara; role: protagonist; desire: save the stranded crew; fear: losing the engine; bottom_line: will not abandon survivors"],
                "world_rules": ["The rescue engine can cross the storm only while its keeper remains aboard."],
                "outline": "Mara chooses the stranded crew over the easy route home.",
                "reader_promise": {"core_hook": "Whether Mara can preserve both the engine and the stranded crew"}
            })
            .to_string(),
        )
        .await
        .expect("contract");
        seal_test_chapter_authority(&tool, project_path, 1).await;
        let body = complete_english_test_body("Mara keeps the engine running through the storm and chooses to save the stranded crew instead of taking the easy route home.");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Chapter 1",
                "content": &body
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        tool.call(
            &serde_json::json!({
                "action": "settle_chapter_state",
                "project_path": project_path,
                "chapter_number": 1,
                "content": serde_json::json!({
                    "state_changes": [],
                    "current_state": "Mara keeps the rescue engine running through the storm.",
                    "pending_hooks": "The stranded crew still needs a safe route home.",
                    "chapter_summary": "Mara keeps the rescue engine running and chooses the stranded crew over the easy route home.",
                    "continuity_updates": [
                        "Mara chooses the stranded crew over the easy route home."
                    ]
                }).to_string()
            })
            .to_string(),
        )
        .await
        .expect("settle chapter before metadata repair");

        let settlement_path = pending_settlement_path(std::path::Path::new(project_path), 1);
        let settlement_before: SettlementOutput = serde_json::from_str(
            &tokio::fs::read_to_string(&settlement_path)
                .await
                .expect("pending settlement before metadata repair"),
        )
        .expect("pending settlement json before metadata repair");

        let chapter_path = std::path::Path::new(project_path).join("chapters/0001.md");
        let saved_before_candidate = tokio::fs::read_to_string(&chapter_path)
            .await
            .expect("chapter file before candidate metadata repair");
        let candidate = tool
            .call(
                &serde_json::json!({
                    "action": "repair_chapter_metadata",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Candidate Storm Engine",
                    "summary": "Mara keeps the engine running and saves the stranded crew.",
                    "key_facts": ["Mara keeps the engine running through the storm."],
                    "continuity_updates": ["Mara chooses the stranded crew over the easy route home."],
                    "candidate_only": true
                })
                .to_string(),
            )
            .await
            .expect("candidate metadata repair");
        let candidate: serde_json::Value =
            serde_json::from_str(&candidate).expect("candidate repair json");
        assert_eq!(candidate["success"], true);
        assert_eq!(candidate["candidate_only"], true);
        assert_eq!(candidate["read_only"], true);
        assert_eq!(candidate["chapter"]["title"], "Candidate Storm Engine");
        assert_eq!(candidate["candidate_body"], body);
        assert!(
            !candidate["candidate_body"]
                .as_str()
                .unwrap_or_default()
                .starts_with("# "),
            "candidate metadata repair must preserve canonical body identity"
        );
        assert_eq!(
            tokio::fs::read_to_string(&chapter_path)
                .await
                .expect("chapter file after candidate metadata repair"),
            saved_before_candidate
        );

        let repaired = tool
            .call(
                &serde_json::json!({
                    "action": "repair_chapter_metadata",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "Storm Engine",
                    "summary": "Mara keeps the engine running and saves the stranded crew.",
                    "key_facts": ["Mara keeps the engine running through the storm."],
                    "continuity_updates": ["Mara chooses the stranded crew over the easy route home."]
                })
                .to_string(),
            )
            .await
            .expect("repair metadata");
        let repaired: serde_json::Value = serde_json::from_str(&repaired).expect("repair json");
        assert_eq!(repaired["success"], true);
        assert_eq!(repaired["repaired_chapters"][0]["title"], "Storm Engine");
        assert!(repaired.get("quality_gate").is_some());
        assert!(repaired.get("metadata_gate").is_some());
        assert!(repaired.get("truth_validation").is_some());

        let saved = tokio::fs::read_to_string(chapter_path)
            .await
            .expect("chapter file");
        assert!(saved.contains(&body));
        assert_eq!(saved.matches(&body).count(), 1);
        assert!(saved.contains("# Storm Engine"));

        let settlement_after: SettlementOutput = serde_json::from_str(
            &tokio::fs::read_to_string(&settlement_path)
                .await
                .expect("pending settlement after metadata repair"),
        )
        .expect("pending settlement json after metadata repair");
        assert_eq!(settlement_after.body_fingerprint, settlement_before.body_fingerprint);
        assert_eq!(
            settlement_after.authority_fingerprint,
            settlement_before.authority_fingerprint
        );
        assert_eq!(
            serde_json::to_value(&settlement_after.state_changes).expect("state changes after"),
            serde_json::to_value(&settlement_before.state_changes).expect("state changes before")
        );
        assert_eq!(settlement_after.current_state, settlement_before.current_state);
        assert_eq!(settlement_after.pending_hooks, settlement_before.pending_hooks);
        assert_eq!(settlement_after.resolved_hooks, settlement_before.resolved_hooks);
    }

    #[tokio::test]
    async fn novel_studio_repair_chapter_metadata_preserves_explicit_chinese_title() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"灵脉独尊","language":"zh-CN"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");
        let body = "青云宗后山，闻朔砺被判为弃徒。他在祖碑底部发现幽蓝裂纹，看见凡骨古法。韩曜砺提醒他宋澈川正在寻找祖碑线索，闻朔砺赶往杂役处，当夜第一次引气入脉。";

        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "他站起身",
                "content": body
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        let rejected_candidate = tool
            .call(
                &serde_json::json!({
                    "action": "repair_chapter_metadata",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "祖碑深处",
                    "summary": "闻朔砺被判为弃徒后，在祖碑底部发现残纹和凡骨古法，并于杂役处第一次引气入脉。",
                    "key_facts": ["闻朔砺被判为弃徒。", "祖碑底部出现残纹和凡骨古法。"],
                    "continuity_updates": ["闻朔砺获得凡骨古法线索。"],
                    "candidate_only": true
                })
                .to_string(),
            )
            .await
            .expect("evaluate rejected metadata candidate");
        let rejected_candidate: serde_json::Value = serde_json::from_str(&rejected_candidate)
            .expect("rejected metadata candidate json");
        assert_eq!(rejected_candidate["candidate_only"], true);
        assert_eq!(rejected_candidate["chapter"]["title"], "祖碑深处");
        assert_ne!(rejected_candidate["chapter"]["title"], "第1章");
        assert_eq!(
            rejected_candidate["metadata_gate"]["repairable"]
                .as_array()
                .is_some_and(|issues| !issues.is_empty()),
            true,
            "the real rejected candidate must reach the metadata gate so the next retry receives useful feedback"
        );

        let repaired = tool
            .call(
                &serde_json::json!({
                    "action": "repair_chapter_metadata",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "chapter_title": "断碑残纹与凡骨古法",
                    "summary": "闻朔砺被判为弃徒后，在祖碑底部发现残纹和凡骨古法，并于杂役处第一次引气入脉。",
                    "key_facts": ["闻朔砺被判为弃徒。", "祖碑底部出现残纹和凡骨古法。"],
                    "continuity_updates": ["闻朔砺获得凡骨古法线索。"]
                })
                .to_string(),
            )
            .await
            .expect("repair metadata");
        let repaired: serde_json::Value = serde_json::from_str(&repaired).expect("repair json");
        assert_eq!(
            repaired["repaired_chapters"][0]["title"],
            "断碑残纹与凡骨古法"
        );

        let chapter_path = std::path::Path::new(project_path).join("chapters/0001.md");
        let saved = tokio::fs::read_to_string(chapter_path)
            .await
            .expect("chapter file");
        assert!(saved.contains(body));
        assert!(saved.contains("# 断碑残纹与凡骨古法"));
        assert!(!saved.contains("# 去外门杂"));
    }

    #[tokio::test]
    async fn novel_studio_rewriting_same_chapter_keeps_stable_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Stable Chapter Paths","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        let first = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter",
                    "project_path": project_path,
                    "chapter_number": 2,
                    "chapter_title": "Chapter 2",
                    "content": "The first stable chapter body has enough real content to persist."
                })
                .to_string(),
            )
            .await
            .expect("first chapter");
        let first: serde_json::Value = serde_json::from_str(&first).expect("first json");

        let second = tool
            .call(
                &serde_json::json!({
                    "action": "add_chapter",
                    "project_path": project_path,
                    "chapter_number": 2,
                    "chapter_title": "A Better Title",
                    "content": "The replacement chapter body keeps the same stable chapter path."
                })
                .to_string(),
            )
            .await
            .expect("second chapter");
        let second: serde_json::Value = serde_json::from_str(&second).expect("second json");

        assert_eq!(first["chapter"]["path"], "chapters/0002.md");
        assert_eq!(second["chapter"]["path"], "chapters/0002.md");
        assert_eq!(second["chapter"]["title"], "A Better Title");
        assert!(std::path::Path::new(project_path)
            .join("chapters/.revisions")
            .exists());
        assert!(!std::path::Path::new(project_path)
            .join("chapters/0002_Chapter2.md")
            .exists());
    }

    #[tokio::test]
    async fn novel_studio_normalizes_null_list_fields_before_deserialize() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let init = tool
            .call(r#"{"action":"init_project","title":"Null Lists","language":"en"}"#)
            .await
            .expect("init");
        let init: serde_json::Value = serde_json::from_str(&init).expect("init json");
        let project_path = init["project_path"].as_str().expect("project path");

        tool.call(
            &serde_json::json!({
                "action": "add_chapter",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "Opening",
                "content": "A chapter exists so metadata-only revision can update safely."
            })
            .to_string(),
        )
        .await
        .expect("add chapter");

        let revised = tool
            .call(
                &serde_json::json!({
                    "action": "revise_chapter",
                    "project_path": project_path,
                    "chapter_number": 1,
                    "summary": "Metadata survives nullable list fields.",
                    "key_facts": null,
                    "continuity_updates": null,
                    "issues": null
                })
                .to_string(),
            )
            .await
            .expect("revise");
        let revised: serde_json::Value = serde_json::from_str(&revised).expect("revised json");

        assert_eq!(revised["success"], true);
        assert_eq!(revised["runtime_effect"], "artifact.written");
        assert_eq!(revised["metadata_only"], true);
        assert_eq!(
            revised["chapter"]["summary"],
            "Metadata survives nullable list fields."
        );
    }

    #[tokio::test]
    async fn novel_studio_missing_action_returns_recoverable_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let result = tool.call("{}").await.expect("guidance");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert!(value["available_actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action == "init_project"));
    }

    #[tokio::test]
    async fn novel_studio_path_boundary_returns_recoverable_guidance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");
        let outside = dir
            .path()
            .parent()
            .expect("parent")
            .join("outside-novel-project");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Boundary Test",
                    "project_path": outside
                })
                .to_string(),
            )
            .await
            .expect("guidance");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], false);
        assert_eq!(value["recoverable"], true);
        assert_eq!(value["error_kind"], "path_outside_workspace");
        assert_eq!(value["safe_output_root"], "data/generated/novels");
    }

    #[tokio::test]
    async fn novel_studio_avoids_nested_data_root_when_workspace_is_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join("data");
        tokio::fs::create_dir_all(&data_dir)
            .await
            .expect("create data dir");
        let tool = NovelStudioTool::new(data_dir.clone(), "tester");

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "init_project",
                    "title": "Storage Root",
                    "language": "en"
                })
                .to_string(),
            )
            .await
            .expect("init");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        let project_path = value["project_path"].as_str().expect("project path");
        assert!(project_path.contains("/data/generated/novels/"));
        assert!(!project_path.contains("/data/data/generated/novels/"));

        let result = tool
            .call(
                &serde_json::json!({
                    "action": "status",
                    "project_path": "data/generated/novels/Storage Root"
                })
                .to_string(),
            )
            .await
            .expect("status");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(value["success"], true);
    }

    #[tokio::test]
    async fn novel_studio_approve_draft_ignores_draft_file_as_project_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tool = NovelStudioTool::new(dir.path().to_path_buf(), "tester");

        let draft = tool
            .call(
                &serde_json::json!({
                    "action": "draft_project",
                    "language": "zh-CN",
                    "genre": "都市玄幻",
                    "brief": "底层学生在都市灵脉学院逆袭。",
                    "premise": "沈知衡作为旁听生进入都市灵脉学院，追查灵脉污染并争取选择权。",
                    "title": "灵脉灯塔",
                    "ending_direction": "主角公开灵脉真相并守住城市。",
                    "protagonist_arc": "从自卑旁听生变成愿意承担代价的守护者。",
                    "world_imagery": "雨夜灯塔、地下灵脉、旧校徽。",
                    "main_causal_spine": "考试失利引出灵脉污染，主角追查并反制幕后操控。",
                    "title_rationale": "书名来自终局里主角点亮灯塔、公开城市灵脉真相的核心事件。",
                    "characters": ["沈知衡：主角，旁听生，渴望证明自己但害怕拖累亲人。"],
                    "themes": ["底层学生争取选择权"],
                    "world_rules": ["地下灵脉会改变城市考试与资源分配。"],
                    "style_rules": ["都市节奏，冲突清楚。"],
                    "must_avoid": ["不要复用旧项目人物名。"],
                    "outline": "沈知衡从旁听生进入灵脉学院，调查污染真相，最终守住城市。"
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
                    "draft_path": draft_path,
                    "project_path": draft_path
                })
                .to_string(),
            )
            .await
            .expect("approve draft");
        let approved: serde_json::Value = serde_json::from_str(&approved).expect("approved json");
        assert_eq!(approved["success"], true, "{approved}");
        let project_path = approved["project_path"].as_str().expect("project path");
        assert!(!project_path.ends_with(".json"));
        assert!(std::path::Path::new(project_path)
            .join("project.json")
            .is_file());
    }
