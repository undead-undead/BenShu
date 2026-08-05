    #[test]
    fn warning_only_quality_gate_does_not_require_revision() {
        let warning_only = json!({
            "recoverable": false,
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": [],
                "warnings": ["章节标题没有被本章摘要、关键事实或正文事件支撑"]
            },
            "review": {
                "verdict": "passed"
            }
        });

        assert!(!needs_revision(&warning_only));
        assert!(write_result_is_clean_for_rule_audit(&warning_only));
    }

    #[test]
fn chapter_loop_accepts_clean_audited_chapter() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": [],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "passed",
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn chapter_loop_stops_after_single_topup_for_pure_length_shortfall() {
        let write_result = json!({
            "unit_count": 2499,
            "quality_gate": {
                "passed": false,
                "findings": [chapter_quality::ChapterFinding::local(
                    "length_below_target",
                    chapter_quality::ChapterFindingClass::Length,
                    chapter_quality::ChapterFindingDisposition::DeterministicRepair,
                    chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
                    "chapter_length",
                    "chapter length is below the soft target: 2499 of 2500 units",
                    "authority",
                    "body",
                )],
                "issues": [],
                "repairable": ["chapter length is below the soft target: 2499 of 2500 units"],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": ["chapter length is below the soft target: 2499 of 2500 units"]
            }
        });

        let first = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 7,
            last_cleanup_fingerprint: Some(7),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });
        let second = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 7,
            last_cleanup_fingerprint: Some(7),
            attempted_tail_completion: false,
            attempted_length_topup: true,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

    assert_eq!(first, ChapterLoopDecision::LengthTopup);
    assert_eq!(second, ChapterLoopDecision::StopForFinalCleanup);
}

#[test]
fn exhausted_local_cleanup_routes_to_bounded_semantic_revision() {
    let finding = chapter_quality::ChapterFinding::local(
        "body_surface_contamination",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::DeterministicRepair,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "anchor_malformed_predicate",
        "chapter body contains malformed phrase near a stable character anchor",
        "authority",
        "body",
    );
    let write_result = json!({
        "quality_gate": {
            "passed": false,
            "findings": [finding],
            "issues": [],
            "repairable": ["malformed phrase"],
            "warnings": []
        },
        "truth_validation": {"issues": []}
    });
    let audit = json!({
        "review": {"verdict": "passed", "locally_validated": true, "findings": []}
    });

    let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
        write_result: &write_result,
        audit: &audit,
        body_fingerprint: 41,
        last_cleanup_fingerprint: Some(41),
        attempted_tail_completion: false,
        attempted_length_topup: false,
        chapter_unit_target: Some(2500),
        language: "zh-CN",
    });

    assert_eq!(decision, ChapterLoopDecision::LlmRevision);
}

#[test]
fn free_text_audit_length_shortfall_cannot_trigger_topup() {
    let write_result = json!({
        "quality_gate": {
            "passed": true,
            "issues": [],
            "repairable": [],
            "warnings": []
        },
        "truth_validation": {
            "issues": []
        }
    });
    let audit = json!({
        "chapter_number": 8,
        "issues": [
            "quality gate: chapter length is below minimum target: 2498 of 2500 units"
        ],
        "next_action": "revise_draft",
        "verdict": "needs_revision"
    });

    let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
        write_result: &write_result,
        audit: &audit,
        body_fingerprint: 8,
        last_cleanup_fingerprint: Some(8),
        attempted_tail_completion: false,
        attempted_length_topup: false,
        chapter_unit_target: Some(2500),
        language: "zh-CN",
    });

    assert_eq!(decision, ChapterLoopDecision::Accept);
}

#[test]
fn persisted_rejected_length_topup_still_exhausts_the_one_shot_budget() {
    let mut budget = RevisionBudget::default();

    super::super::chapter_runtime::restore_recovered_attempt_budget(
        &mut budget,
        CandidateProvenance::LengthTopup,
    );

    assert!(budget.length_topup_attempted);
}

#[test]
fn persisted_rejected_semantic_revision_restores_the_shared_five_attempt_budget() {
    let mut budget = RevisionBudget::default();

    for expected in 1..=MAX_LLM_REVISION_ATTEMPTS {
        super::super::chapter_runtime::restore_recovered_attempt_budget(
            &mut budget,
            CandidateProvenance::SemanticRevision,
        );
        assert_eq!(budget.semantic_attempts, expected);
        assert_eq!(
            budget.can_attempt_semantic_revision(),
            expected < MAX_LLM_REVISION_ATTEMPTS
        );
    }
    assert!(!budget.can_attempt_semantic_revision());
}

#[test]
fn persisted_metadata_repairs_restore_the_exact_attempt_count() {
    let mut budget = RevisionBudget::default();

    for _ in 0..4 {
        super::super::chapter_runtime::restore_recovered_attempt_budget(
            &mut budget,
            CandidateProvenance::MetadataRepair,
        );
    }

    assert_eq!(budget.metadata_repair_attempts, 4);
}

    #[test]
    fn tiny_length_shortfall_uses_tiny_topup_segment() {
        let segment = chapter_expansion_segment_target(2500, 22);

        assert!(
            segment <= 80,
            "tiny shortfalls should not request a full expansion segment: {segment}"
        );
        assert!(
            chapter_minimum_addition_units(segment) <= 30,
            "tiny shortfalls should accept a short natural closing addition"
        );
    }

    #[test]
    fn moderate_length_shortfall_does_not_force_a_full_scene() {
        let segment = chapter_expansion_segment_target(2500, 335);

        assert_eq!(segment, 436);
        assert!(
            segment < 800,
            "a bounded shortfall must not force an unrelated full-scene expansion"
        );
    }


    #[test]
    fn llm_audit_keeps_subjective_output_as_telemetry_only() {
        let audit = parse_llm_quality_audit_output(
            r#"{"score":0,"authority_conflicts":[],"advisories":["节奏偏慢"]}"#,
        )
        .expect("audit should parse");

        assert_eq!(audit.score, Some(0));
        assert!(audit.authority_conflicts.is_empty());
        assert_eq!(audit.advisories, vec!["节奏偏慢".to_string()]);
    }

    #[test]
    fn llm_conflict_requires_local_confirmation_and_exact_citations() {
        let authority = json!({"authority": {"chapter_contract": {"goal": "查明失踪案"}}});
        let conflict = RawAuthorityConflict {
            kind: "character_identity_conflict".to_string(),
            authority_path: "/authority/chapter_contract/goal".to_string(),
            authority_excerpt: "查明失踪案".to_string(),
            body_excerpt: "查明失踪案".to_string(),
            message: "目标被替换".to_string(),
        };

        let grounded = RawAuthorityConflict {
            body_excerpt: "主角转而追查军火案".to_string(),
            ..conflict.clone()
        };
        assert!(validate_llm_authority_conflict(
            &grounded,
            &[],
            &authority.to_string(),
            "主角转而追查军火案。",
        )
        .is_none());

        let locally_confirmed = vec![chapter_quality::ChapterFinding::local(
            "character_identity_conflict",
            chapter_quality::ChapterFindingClass::Contract,
            chapter_quality::ChapterFindingDisposition::HardBlock,
            chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
            "local_test",
            "locally confirmed conflict",
            "authority",
            "主角转而追查军火案。",
        )];
        let finding = validate_llm_authority_conflict(
            &grounded,
            &locally_confirmed,
            &authority.to_string(),
            "主角转而追查军火案。",
        )
        .expect("exact authority and body citations should validate");
        assert!(finding.hard_blocking());

        let invalid = RawAuthorityConflict {
            authority_excerpt: "不存在的合同目标".to_string(),
            ..grounded.clone()
        };
        assert!(validate_llm_authority_conflict(
            &invalid,
            &locally_confirmed,
            &authority.to_string(),
            "主角转而追查军火案。",
        )
        .is_none());

        let subjective = RawAuthorityConflict {
            kind: "pacing_is_slow".to_string(),
            ..grounded
        };
        assert!(validate_llm_authority_conflict(
            &subjective,
            &locally_confirmed,
            &authority.to_string(),
            "主角转而追查军火案。",
        )
        .is_none());
    }

    #[test]
    fn future_boundary_audit_blocks_only_after_local_confirmation() {
        let authority = json!({
            "authority": {
                "next_chapter_boundary": "第2章才与监察员会面"
            }
        });
        let conflict = RawAuthorityConflict {
            kind: "future_chapter_consumed".to_string(),
            authority_path: "/authority/next_chapter_boundary".to_string(),
            authority_excerpt: "第2章才与监察员会面".to_string(),
            body_excerpt: "与监察员会面".to_string(),
            message: "疑似提前消费下一章事件".to_string(),
        };

        let locally_confirmed = vec![chapter_quality::ChapterFinding::local(
            "future_chapter_consumed",
            chapter_quality::ChapterFindingClass::Continuity,
            chapter_quality::ChapterFindingDisposition::HardBlock,
            chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
            "local_test",
            "locally confirmed future boundary",
            "authority",
            "主角在本章末尾与监察员会面。",
        )];
        let finding = validate_llm_authority_conflict(
            &conflict,
            &locally_confirmed,
            &authority.to_string(),
            "主角在本章末尾与监察员会面。",
        )
        .expect("an exact future-boundary conflict is a deterministic hard finding");
        assert!(finding.hard_blocking());
    }

    #[test]
    fn small_length_shortfall_uses_topup_instead_of_full_revision() {
        let write_result = json!({
            "unit_count": 4446,
            "quality_gate": {
                "passed": false,
                "findings": [chapter_quality::ChapterFinding::local(
                    "length_below_target",
                    chapter_quality::ChapterFindingClass::Length,
                    chapter_quality::ChapterFindingDisposition::DeterministicRepair,
                    chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
                    "chapter_length",
                    "chapter length is below the soft target: 4446 of 5000 units",
                    "authority",
                    "chapter body",
                )],
                "issues": [],
                "repairable": ["chapter length is below the soft target: 4446 of 5000 units"],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": ["chapter length is below the soft target: 4446 of 5000 units"]
            }
        });

        assert!(only_small_length_shortfall(
            &write_result,
            &audit,
            Some(5000),
            "Chinese"
        ));
    }

    #[test]
    fn cjk_summary_like_stagnation_text_does_not_create_a_blocker() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": ["章节像摘要/大纲而不是正文"],
                "warnings": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "章节只围绕设定加字、没有具体行动/代价/关系变化",
                    "章节复述本章前文或上一段而没有新事件"
                ]
            }
        });

        assert!(!body_revision_required_after_audit(&write_result, &audit));
    }

    #[test]
    fn overused_story_term_uses_revision_not_fresh_generation() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": ["Chinese chapter body overuses the same story term without enough concrete progression: `道的真意` appears 22 times"],
                "warnings": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            }
        });

        assert!(!body_revision_required_after_audit(&write_result, &audit));
        assert!(!only_deterministic_cleanup_issues(&write_result, &audit));
    }

    #[test]
    fn local_revision_reduces_overused_cjk_story_descriptor() {
        let sentence = "黑衣男子站在石门前。闻澈川看着黑衣男子，黑衣男子没有退让。";
        let content = sentence.repeat(8);
        let issues = vec![
            "Chinese chapter body overuses the same story term without enough concrete progression: `黑衣男子` appears 24 times"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(&content, &issues);

        assert!(repaired.matches("黑衣男子").count() <= 4, "{repaired}");
        assert!(repaired.contains("对方") || repaired.contains("那人"), "{repaired}");
        assert_ne!(repaired, content);
    }

    #[test]
    fn large_implicit_length_shortfall_does_not_use_small_topup() {
        let write_result = json!({
            "unit_count": 3072,
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "passed",
                "issues": []
            }
        });

        assert!(!only_small_length_shortfall(
            &write_result,
            &audit,
            Some(5000),
            "Chinese"
        ));
    }

    #[test]
    fn local_revision_repairs_cjk_fragment_regressions() {
        let content = "钟岑禾到自己的身体被撕裂。他想，无论付出什代价。";
        let issues = vec![
            "chapter body contains malformed phrase near stable character anchor `钟岑禾`: 钟岑禾到自己的身".to_string(),
            "chapter body contains likely missing-character fragment: 什代价".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert!(repaired.contains("钟岑禾感到自己的身体被撕裂"));
        assert!(repaired.contains("无论付出什么代价"));
        assert!(!repaired.contains("钟岑禾到自己的身"));
        assert!(!repaired.contains("付出什代价"));
    }

    #[test]
    fn local_revision_repairs_cjk_missing_character_surface_noise() {
        let content = "宋知舟吸一口气，终于明白为什都变了。韩晴舟声说：别动。韩晴舟有回答，只是看着符咒。宋知舟了点头。";
        let issues = vec![
            "chapter body contains malformed phrase near stable character anchor `宋知舟`: 宋知舟吸".to_string(),
            "chapter body contains likely missing-character fragment: 为什".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:什都".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:什东西".to_string(),
            "chapterbodycontainsrepeatedcharacterinsertion:韩晴舟声说".to_string(),
            "chapterbodycontainsrepeatedcharacterinsertion:韩晴舟有回答".to_string(),
            "chapterbodycontainsrepeatedcharacterinsertion:宋知舟了点头".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert!(repaired.contains("宋知舟深吸一口气"), "{repaired}");
        assert!(repaired.contains("为什么都变了"), "{repaired}");
        assert!(repaired.contains("韩晴舟低声说"), "{repaired}");
        assert!(repaired.contains("韩晴舟没有回答"), "{repaired}");
        assert!(repaired.contains("宋知舟点了点头"), "{repaired}");
        assert!(!repaired.contains("宋知舟吸一口气"), "{repaired}");
        assert!(!repaired.contains("为什都"), "{repaired}");
        assert!(!repaired.contains("韩晴舟声说"), "{repaired}");
        assert!(!repaired.contains("韩晴舟有回答"), "{repaired}");
        assert!(!repaired.contains("宋知舟了点头"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_cjk_missing_character_surface_noise_variants() {
        let content = "景晴禾地回头，太阳穴突直跳。他从未听说过什炼虚真气，只能喃自语：为什总这样？景晴禾地甩头。景晴禾孔剧烈收缩，阮砚晚光坚定。";
        let issues = vec![
            "chapter body contains likely missing-character fragment: 地回头".to_string(),
            "chapter body contains likely missing-character fragment: 突直跳".to_string(),
            "chapter body contains likely missing-character fragment: 什炼虚".to_string(),
            "chapter body contains likely missing-character fragment: 喃自语".to_string(),
            "chapter body contains likely missing-character fragment: 为什".to_string(),
            "chapter body contains likely missing-character fragment: 地甩头".to_string(),
            "chapter body contains malformed phrase near stable character anchor `景晴禾`: 景晴禾孔".to_string(),
            "chapter body contains malformed phrase near stable character anchor `阮砚晚`: 阮砚晚光".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert!(repaired.contains("景晴禾猛地回头"), "{repaired}");
        assert!(repaired.contains("太阳穴突突直跳"), "{repaired}");
        assert!(repaired.contains("什么炼虚真气"), "{repaired}");
        assert!(repaired.contains("喃喃自语"), "{repaired}");
        assert!(repaired.contains("为什么总这样"), "{repaired}");
        assert!(repaired.contains("景晴禾猛地甩头"), "{repaired}");
        assert!(repaired.contains("景晴禾瞳孔剧烈收缩"), "{repaired}");
        assert!(repaired.contains("阮砚晚目光坚定"), "{repaired}");
        assert!(!repaired.contains("什炼虚"), "{repaired}");
        assert!(!repaired.contains("只能喃自语"), "{repaired}");
        assert!(!repaired.contains("太阳穴突直跳"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_cjk_missing_character_surface_noise_from_review_cycle() {
        let content = "许栖川喃自语。他猛地回头，只见段知白匆跑来，似乎有什重要的事情要告诉他。段知白音低沉。许栖川海中闪过画面，为什他会做那些梦，为什偏偏是我？";
        let issues = vec![
            "chapter body contains likely missing-character fragment: 为什".to_string(),
            "chapter body contains likely missing-character fragment: 什他会做那".to_string(),
            "chapter body contains likely missing-character fragment: 什偏偏是我".to_string(),
            "chapter body contains likely missing-character fragment: 什重要的事".to_string(),
            "chapter body contains likely missing-character fragment: 喃自语".to_string(),
            "chapter body contains likely missing-character fragment: 地回头".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:为什".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:什他会做那".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:什偏偏是我".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:什重要的事".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:喃自语".to_string(),
            "chapterbodycontainslikelymissing-characterfragment:地回头".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert!(repaired.contains("许栖川喃喃自语"), "{repaired}");
        assert!(repaired.contains("他猛地回头"), "{repaired}");
        assert!(!repaired.contains("猛猛地回头"), "{repaired}");
        assert!(repaired.contains("有什么重要的事情"), "{repaired}");
        assert!(repaired.contains("为什么他会做那些梦"), "{repaired}");
        assert!(repaired.contains("为什么偏偏是我"), "{repaired}");
        assert!(!repaired.contains("许栖川喃自语"), "{repaired}");
        assert!(!repaired.contains("为什他"), "{repaired}");
        assert!(!repaired.contains("为什偏偏"), "{repaired}");
        assert!(!repaired.contains("有什重要"), "{repaired}");
    }

    #[test]
    fn local_revision_repairs_explicit_repeated_cjk_character_issue() {
        let content = "老赵含糊地说：“你那个频谱仪是不是该该准了？”";
        let issues =
            vec!["明显错别字：第11段中“你那个频谱仪是不是该该准了？”出现重复字符“该该”。".to_string()];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert!(repaired.contains("你那个频谱仪是不是该准了？"), "{repaired}");
        assert!(!repaired.contains("该该准"), "{repaired}");
    }

    #[test]
    fn stale_missing_character_cleanup_issue_requests_reaudit_without_double_repair() {
        let content = "许栖川猛地回头，看见雨幕里的信号灯。";
        let issues =
            vec!["chapter body contains likely missing-character fragment: 地回头".to_string()];

        let repaired = apply_local_revision_suggestions(content, &issues);

        assert_eq!(repaired, content);
        assert!(deterministic_cleanup_issues_are_stale_after_local_repair(
            &repaired, &issues
        ));
    }

    #[test]
    fn free_text_non_length_issue_cannot_require_full_revision() {
        let write_result = json!({
            "unit_count": 4944,
            "quality_gate": {
                "passed": false,
                "findings": [chapter_quality::ChapterFinding::local(
                    "length_below_target",
                    chapter_quality::ChapterFindingClass::Length,
                    chapter_quality::ChapterFindingDisposition::DeterministicRepair,
                    chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
                    "chapter_length",
                    "chapter length is below the soft target: 4944 of 5000 units",
                    "authority",
                    "body",
                )],
                "issues": ["chapter body contains placeholder or omission marker: placeholder"],
                "repairable": ["chapter length is below the soft target: 4944 of 5000 units"],
                "warnings": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "chapter length is below minimum target: 4944 of 5000 units",
                    "quality gate: chapter body contains placeholder or omission marker: placeholder"
                ]
            }
        });

        assert!(only_small_length_shortfall(
            &write_result,
            &audit,
            Some(5000),
            "Chinese"
        ));
    }

    #[test]
    fn chapter_expansion_appends_segment_without_replacing_existing_prose() {
        let mut draft = novel_runner::DraftOutput {
            title: "青灯渡劫".to_string(),
            content: "第一段已有正文。".to_string(),
            summary: "主角入山。".to_string(),
            key_facts: vec!["主角入山".to_string()],
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        };
        let addition = parse_chapter_expansion_output(
            r#"{"addition":"第二段新正文。","summary_delta":"遇见古碑。","key_facts":["主角看见古碑"],"continuity_updates":["古碑待解"]}"#,
            "Chinese",
        );

        append_chapter_addition(&mut draft, addition);

        assert!(draft.content.contains("第一段已有正文。"));
        assert!(draft.content.contains("第二段新正文。"));
        assert!(draft.summary.contains("遇见古碑"));
        assert!(draft.key_facts.iter().any(|fact| fact.contains("古碑")));
    }

    #[test]
    fn chapter_expansion_parses_jsonish_addition_without_leaking_metadata() {
        let raw = r#""addition": "第二段新正文。残卷再次发光。", "summary_delta": "残卷发光。", "key_facts": ["残卷再次发光"], "continuity_updates": ["残卷仍未解释"]"#;
        let addition = parse_chapter_expansion_output(raw, "Chinese");

        assert_eq!(addition.addition, "第二段新正文。残卷再次发光。");
        assert!(!addition.addition.contains("summary_delta"));
        assert_eq!(addition.summary_delta.as_deref(), Some("残卷发光。"));
        assert_eq!(addition.key_facts, vec!["残卷再次发光"]);
    }

    #[test]
    fn chapter_expansion_allows_scene_or_cast_growth_for_later_quality_gates() {
        let existing = "陆远抬头看向精炼站深处，左臂上的蓝光正在震颤。";
        let addition = "林墨站在实验室中央，苏琳把报告递给他。";

        assert!(chapter_expansion_rejection_reason(existing, addition, "Chinese").is_none());
    }

    #[test]
    fn chapter_expansion_rejects_high_overlap_repeated_segment() {
        let existing = "晏栖序跌跌撞撞地跑进雨中，金色的影子在积水里晃动，仿佛某种古老图腾在水面浮现。他试图回忆刚才发生的一切，却发现脑海中那道疤痕的痛感正在消退，取而代之的是某种陌生的灼热感。街道两旁的霓虹灯牌开始闪烁，雨水在空中凝结成细小的光点，仿佛整个世界都被某种力量重新编织。他忽然意识到，自己已经无法分辨这是现实还是幻觉。".repeat(3);
        let addition = "晏栖序跌跌撞撞地跑进雨中，金色的影子在积水里晃动，仿佛某种古老图腾在水面浮现。他试图回忆刚才发生的一切，却发现脑海中那道疤痕的痛感正在消退，取而代之的是某种陌生的灼热感。街道两旁的霓虹灯牌开始闪烁，雨水在空中凝结成细小的光点，仿佛整个世界都被某种力量重新编织。他忽然意识到，自己已经无法分辨这是现实还是幻觉。";

        let reason = chapter_expansion_rejection_reason(&existing, addition, "Chinese");

        assert!(reason.is_some(), "duplicate addition was accepted");
    }

    #[test]
    fn sanitize_chapter_body_preserves_phrase_duplication_for_review() {
        let cleaned = sanitize_chapter_body(
            "陆远看见逻辑逻辑溢出，这种这种震荡正在扩散。",
            "第5章",
            "Chinese",
        );

        assert!(cleaned.contains("逻辑逻辑溢出"));
        assert!(cleaned.contains("这种这种震荡"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_suspicious_cjk_stutter_for_review() {
        let cleaned = sanitize_chapter_body(
            "那是一道由高频白噪声和纯白折射光构构成的裂缝。几何结构构成的实体仍然稳定。",
            "第13章",
            "Chinese",
        );

        assert!(cleaned.contains("折射光构构成的裂缝"));
        assert!(cleaned.contains("几何结构构成的实体"));
    }

    #[test]
    fn sanitize_chapter_body_preserves_normal_cjk_reduplication_words() {
        let cleaned = sanitize_chapter_body(
            "他跌跌撞撞地跑进雨中，喃喃自语，密密麻麻的金色纹路在皮肤下浮现。",
            "第1章",
            "Chinese",
        );

        assert!(cleaned.contains("跌跌撞撞地跑进雨中"), "{cleaned}");
        assert!(cleaned.contains("喃喃自语"), "{cleaned}");
        assert!(cleaned.contains("密密麻麻"), "{cleaned}");
    }

    #[test]
    fn sanitize_chapter_body_preserves_common_chinese_question_words() {
        let cleaned = sanitize_chapter_body(
            "这种东西怎么会在旧货摊上？仿佛有什么东西正在苏醒。他踩到了什么东西。\n他低声问：为什么你会在这里？然后喃喃自语。",
            "第1章",
            "Chinese",
        );

        assert!(cleaned.contains("怎么会在旧货摊上"), "{cleaned}");
        assert!(cleaned.contains("有什么东西正在苏醒"), "{cleaned}");
        assert!(cleaned.contains("踩到了什么东西"), "{cleaned}");
        assert!(cleaned.contains("为什么你会在这里"), "{cleaned}");
        assert!(cleaned.contains("喃喃自语"), "{cleaned}");
        assert!(!cleaned.contains("怎会"), "{cleaned}");
        assert!(!cleaned.contains("有什东西"), "{cleaned}");
        assert!(!cleaned.contains("为什你会"), "{cleaned}");
        assert!(!cleaned.contains("然后喃自语"), "{cleaned}");
    }

    #[test]
    fn local_revision_repairs_cjk_missing_character_fragments() {
        let cleaned = apply_local_revision_suggestions(
            "他终于明白为什会被追杀，悄蔓延的灵压已经逼近。",
            &[
                "chapter body contains likely missing-character fragment: 为什".to_string(),
                "chapter body contains likely missing-character fragment: 悄蔓延".to_string(),
            ],
        );

        assert!(cleaned.contains("为什么会被追杀"));
        assert!(cleaned.contains("悄然蔓延的灵压"));
        assert!(!cleaned.contains("为什会"));
        assert!(!cleaned.contains("悄蔓延"));
    }

    #[test]
    fn sanitizer_preserves_adjacent_foreign_script_for_review() {
        let raw = "大地像在呻나。\\나陆沉握紧玉简，听见SQL阵列启动。";
        let cleaned = sanitize_chapter_body(raw, "第一章：裂隙", "Chinese");

        assert!(cleaned.contains('나'));
        assert!(cleaned.contains("陆沉握紧玉简"));
        assert!(cleaned.contains("SQL阵列"));
    }

    #[test]
    fn sanitize_chapter_body_removes_json_residue_and_meta_preface() {
        let raw = "（此处为修订后的完整章节内容，已剔除所有非CJK脚本碎片。）\n\n闻庭安踏进雨夜，掌心旧印微微发烫。\",\"summary_delta\":\"本章总结\",\"key_facts\":[\"x\"]";
        let cleaned = sanitize_chapter_body(raw, "第一章", "Chinese");

        assert!(cleaned.starts_with("闻庭安踏进雨夜"));
        assert!(!cleaned.contains("此处为修订后的完整章节内容"));
        assert!(!cleaned.contains("summary_delta"));
        assert!(!cleaned.contains("key_facts"));
    }

    #[test]
    fn sanitize_chapter_body_unwraps_json_string_paragraph_lines() {
        let raw = "谢砚息踏进血色云海，第一次听见天域法则低鸣。\n\n\"韩闻隅抛来玉简，提醒他血祭传承必须付出代价。\",\n\n\"季栖澜的剑光落下，逼得谢砚息在众目睽睽下做出选择。\",\n\n\"谢砚息按住伤口，没有退后。";
        let cleaned = sanitize_chapter_body(raw, "血色符文", "zh-CN");

        assert!(cleaned.contains("谢砚息踏进血色云海"));
        assert!(cleaned.contains("韩闻隅抛来玉简"));
        assert!(cleaned.contains("季栖澜的剑光落下"));
        assert!(cleaned.contains("谢砚息按住伤口"));
        assert!(!cleaned.contains("\","));
        assert!(!cleaned.contains("\n\"韩闻隅"));
        assert!(!cleaned.contains("\n\"季栖澜"));
    }

    #[test]
    fn ambiguous_cjk_spacing_requires_contextual_revision() {
        let write_result = json!({
            "quality_gate": {
                "issues": [
                    "Chinese-language chapter contains unexpected whitespace inside CJK phrase: 吞 量"
                ]
            }
        });
        let audit = json!({
            "review": {
                "issues": [
                    "quality gate: Chinese-language chapter contains unexpected whitespace inside CJK phrase: 吞 量"
                ]
            }
        });

        assert!(!only_deterministic_cleanup_issues(&write_result, &audit));
    }

    #[test]
    fn deterministic_cleanup_reduces_repeated_cjk_rhetorical_marker() {
        let issue =
            "Chinese chapter body overuses the same rhetorical marker instead of varying prose movement: `仿佛` appears 18 times";
        let content = (0..18)
            .map(|index| format!("第{index}次交锋时，剑光仿佛压低了整座石厅。"))
            .collect::<Vec<_>>()
            .join("\n");

        let repaired = apply_local_revision_suggestions(&content, &[issue.to_string()]);

        assert!(repaired.matches("仿佛").count() <= 4, "{repaired}");
        assert!(repaired.contains("好似") || repaired.contains("犹如"), "{repaired}");
        assert!(repaired.contains("剑光"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_malformed_anchor_predicate() {
        let issue =
            "chapter body contains malformed phrase near stable character anchor `程闻川`: 程闻川觉到";
        let repaired = apply_local_revision_suggestions(
            "门外传来机械音，程闻川觉到胃部一阵痉挛。",
            &[issue.to_string()],
        );

        assert!(repaired.contains("程闻川感觉到胃部一阵痉挛"), "{repaired}");
        assert!(!repaired.contains("程闻川觉到"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_cjk_dialogue_and_action_boundary_punctuation() {
        let issues = vec![
            "第15段末尾缺少句号：'走吧'顾岚珩说道".to_string(),
            "第16段存在文字拼接错误：'向断崖下的迷雾走顾岚珩紧随其后'".to_string(),
        ];
        let repaired = apply_local_revision_suggestions(
            "“走吧”顾岚珩说道，声音不再颤抖。\n季澈澜点了点头，转身向断崖下的迷雾走顾岚珩紧随其后。",
            &issues,
        );

        assert!(repaired.contains("“走吧。”顾岚珩说道"), "{repaired}");
        assert!(
            repaired.contains("向断崖下的迷雾走。顾岚珩紧随其后"),
            "{repaired}"
        );
        assert!(!repaired.contains("“走吧”顾岚珩说道"), "{repaired}");
        assert!(!repaired.contains("走顾岚珩紧随其后"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_handles_paragraph_break_surface_issue() {
        let issue = "段落结构异常：场景转换处缺失换行，导致动作描写粘连，阅读体验割裂。";
        let content = [
            "雨，下得更大了。司衡息握紧季砚阙的手，沿着祠堂外的长廊向前走去。长廊两侧挂满褪色的红纸灯笼，每一盏灯笼里都像藏着一只闭合的眼睛。风穿过瓦缝时发出细碎的啸声，像有人贴着墙根低语。季砚阙停下脚步，指尖轻轻触碰柱子上剥落的朱漆。那一瞬间，柱身深处传来纸页翻动般的声响，仿佛整座祠堂都在醒来。司衡息意识到这里不是普通的祭祀场所，而是一座被旧契约维系的牢笼。老僧留下的香灰仍在地面缓慢移动，拼成一个又一个残缺的字。季砚阙想要辨认，却被一阵突如其来的寒意逼得后退半步。司衡息扶住她，低声提醒她不要再看。可是那些香灰已经聚成祖父的姓氏，像是在催促他们继续往祠堂深处走。",
            "门后传来一声轻响。"
        ]
        .join("\n");

        let repaired = apply_local_revision_suggestions(&content, &[issue.to_string()]);

        assert!(repaired.lines().count() > content.lines().count(), "{repaired}");
        assert!(repaired.contains("门后传来一声轻响。"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_cjk_action_object_part_boundary() {
        let issues = vec![
            "quality gate: chapter body contains malformed phrase near stable character anchor `晏照珩`: 晏照珩一个".to_string(),
            "第1段存在文字拼接错误：动作宾语和物件部位缺少句读".to_string(),
        ];
        let repaired = apply_local_revision_suggestions(
            "晏照珩缓缓收剑尖滴落淡金色血珠。老人手中握着一根枯木杖头镶嵌着一颗晶石。“我接受。”晏照珩握紧手中的枯荣剑身上的黑色纹路仿佛活了过来。",
            &issues,
        );

        assert!(repaired.contains("收剑，剑尖滴落"), "{repaired}");
        assert!(repaired.contains("枯木杖，杖头镶嵌"), "{repaired}");
        assert!(repaired.contains("枯荣剑，剑身上的黑色纹路"), "{repaired}");
        assert!(!repaired.contains("收剑尖滴落"), "{repaired}");
        assert!(!repaired.contains("枯荣剑身上的"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_quality_gate_action_object_part_boundary_issue() {
        let issue = "quality gate: chapter body contains likely malformed CJK action-object-part boundary; missing punctuation or duplicated object near: 照珩握紧了手中的剑尖滴落的并非鲜";
        let repaired = apply_local_revision_suggestions(
            "晏照珩握紧了手中的剑尖滴落的并非鲜血，而是梁澈川灵力溃散后凝结的淡金色血珠。",
            &[issue.to_string()],
        );

        assert!(
            repaired.contains("晏照珩握紧了手中的剑，剑尖滴落的并非鲜血"),
            "{repaired}"
        );
        assert!(!repaired.contains("手中的剑尖滴落"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_modified_object_part_boundary_issue() {
        let issue = "quality gate: chapter body contains likely malformed CJK action-object-part boundary; missing punctuation or duplicated object near: 握着一柄断裂的长剑尖滴落的不是血";
        let repaired = apply_local_revision_suggestions(
            "那人握着一柄断裂的长剑尖滴落的不是血，而是漆黑的雾气。",
            &[issue.to_string()],
        );

        assert!(
            repaired.contains("那人握着一柄断裂的长剑，剑尖滴落的不是血"),
            "{repaired}"
        );
        assert!(!repaired.contains("长剑尖滴落"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_does_not_split_object_part_without_action_context() {
        let issues = vec!["第1段存在文字拼接错误：动作宾语和物件部位缺少句读".to_string()];
        let content = "老人手中握着一根枯木杖安静站着。";
        let repaired = apply_local_revision_suggestions(content, &issues);

        assert_eq!(repaired, content);
    }

    #[test]
    fn deterministic_cleanup_does_not_repair_punctuation_without_related_issue() {
        let content =
            "“走吧”顾岚珩说道，声音不再颤抖。\n季澈澜点了点头，转身向断崖下的迷雾走顾岚珩紧随其后。";
        let repaired = apply_local_revision_suggestions(
            content,
            &["部分段落存在总结性陈述过多、具体动作较少的问题".to_string()],
        );

        assert_eq!(repaired, content);
    }

    #[test]
    fn deterministic_cleanup_repairs_anchor_followed_by_sensory_object() {
        let issue =
            "chapter body contains malformed phrase near stable character anchor `陆离`: 陆离一种";
        let repaired = apply_local_revision_suggestions(
            "幽刹击中中枢。陆离一种前所未闻的剧痛。",
            &[issue.to_string()],
        );

        assert!(
            repaired.contains("陆离感到一种前所未闻的剧痛"),
            "{repaired}"
        );
        assert!(!repaired.contains("陆离一种前所未闻"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_adjacent_stable_character_anchors() {
        let issue =
            "chapter body contains adjacent stable character anchors without syntax boundary `秦棠川` + `白庭白`: 秦棠川白庭白";
        let repaired = apply_local_revision_suggestions(
            "秦棠川白庭白赶到时，夜色已经压低。",
            &[issue.to_string()],
        );

        assert!(repaired.contains("秦棠川看向白庭白赶到时"), "{repaired}");
        assert!(!repaired.contains("秦棠川白庭白"), "{repaired}");
    }

    #[test]
    fn semantic_foreign_narration_requires_contextual_revision() {
        let issues = vec![
            "存在外文残片：倒数第二段末尾夹杂英文'unawarethatsomewhereinthecity,ahundredpaperfiguresarewakingup,theirpaperbonescreakingintherain,waitingfortheirmasterscommand.'".to_string(),
            "语体风格不统一：结尾处突然插入英文叙述，与全文中文语境割裂，像未翻译的草稿或残留的元数据。".to_string(),
        ];
        let repaired = apply_local_revision_suggestions(
            "沈墨抬头，看见雨幕里所有纸人同时转身。unawarethatsomewhereinthecity,ahundredpaperfiguresarewakingup,theirpaperbonescreakingintherain,waitingfortheirmasterscommand.",
            &issues,
        );

        assert!(repaired.contains("沈墨抬头，看见雨幕里所有纸人同时转身。"));
        assert!(repaired.contains("unawarethat"), "{repaired}");
        assert!(repaired.contains("paperfigures"), "{repaired}");
    }

    #[test]
    fn repeated_person_name_aliases_are_not_globally_rewritten() {
        let issue = "人物名字中‘庭白’与‘白庭白’混用，导致人物名称不一致。";
        let adjacent =
            "chapter body contains adjacent stable character anchors without syntax boundary `秦棠川` + `白庭白`: 秦棠川白庭白";
        let repaired = apply_local_revision_suggestions(
            "庭白握紧玉佩，秦棠川庭白声音压低。",
            &[issue.to_string(), adjacent.to_string()],
        );

        assert_eq!(repaired, "庭白握紧玉佩，秦棠川庭白声音压低。");
    }

    #[test]
    fn deterministic_cleanup_repairs_embedded_token_copyedits() {
        let raw = "苏璃有回答，只是看着段予序的纹路。段予序到那名黑衣人的法杖表面浮现出裂痕。苏璃然挡在段予序身前。";
        let issues = vec![
            "存在明显错字残字：'苏璃有回答'中的'有'应为'没'或'未'".to_string(),
            "存在明显错字残字：'到那名黑衣人的法杖表面浮现出裂痕'中的'到'应为'看'"
                .to_string(),
            "存在明显错字残字：'然挡在段予序身前'中的'然'应为'突然'或'猛地'"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("苏璃没回答"), "{repaired}");
        assert!(repaired.contains("段予序看那名黑衣人的法杖表面浮现出裂痕"), "{repaired}");
        assert!(repaired.contains("苏璃突然挡在段予序身前"), "{repaired}");
        assert!(!repaired.contains("苏璃有回答"), "{repaired}");
        assert!(!repaired.contains("段予序到那名"), "{repaired}");
        assert!(!repaired.contains("苏璃然挡"), "{repaired}");
    }

    #[test]
    fn deterministic_cleanup_repairs_explicit_extra_cjk_character() {
        let raw = "就在他距离溶洞入口还有五米米处，脚下的碎石突然松动。";
        let issues = vec![
            "正文中存在明显乱码/输入错误：'距离溶洞入口还有五米米处'，多出一个'米'字。"
                .to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("距离溶洞入口还有五米处"), "{repaired}");
        assert!(!repaired.contains("五米米处"), "{repaired}");
    }

    #[test]
    fn tail_completion_trims_replayed_tail_prefix_before_append() {
        let mut draft = novel_runner::DraftOutput {
            title: "第一章".to_string(),
            content: "他点开终端，屏幕亮起，显示着账户余额的变动。一万信用点。他看着那串数字，嘴角再次扬起一抹".to_string(),
            summary: String::new(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        };
        let addition = trim_overlapping_chapter_tail_completion(
            &draft.content,
            ChapterExpansionOutput {
                addition: "屏幕亮起，显示着账户余额的变动。一万信用点。他看着那串数字，嘴角再次扬起一抹笑意。雨声仍在窗外敲打，但他的第一笔本钱已经落袋。".to_string(),
                summary_delta: None,
                key_facts: Vec::new(),
                continuity_updates: Vec::new(),
            },
            "zh-CN",
        );

        append_chapter_tail_completion(&mut draft, addition);

        assert!(draft.content.ends_with("笑意。雨声仍在窗外敲打，但他的第一笔本钱已经落袋。"), "{}", draft.content);
        assert_eq!(draft.content.matches("屏幕亮起").count(), 1, "{}", draft.content);
        assert_eq!(draft.content.matches("账户余额").count(), 1, "{}", draft.content);
    }

    #[test]
    fn deterministic_cleanup_can_run_again_after_new_surface_issue() {
        let first = apply_local_revision_suggestions(
            "陆离到自己的身体正在被规则拖拽。",
            &[
                "chapter body contains malformed phrase near stable character anchor `陆离`: 陆离到自己的身"
                    .to_string(),
            ],
        );
        assert!(
            first.contains("陆离感到自己的身体正在被规则拖拽"),
            "{first}"
        );
        assert!(!first.contains("陆离到自己的身"), "{first}");

        let second = apply_local_revision_suggestions(
            "陆离觉到废墟深处的风正在改写方向。",
            &[
                "chapter body contains malformed phrase near stable character anchor `陆离`: 陆离觉到"
                    .to_string(),
            ],
        );
        assert!(
            second.contains("陆离感觉到废墟深处的风正在改写方向"),
            "{second}"
        );
        assert!(!second.contains("陆离觉到"), "{second}");
    }

    #[test]
    fn deterministic_cleanup_repairs_common_anchor_surface_fragments() {
        let raw = "陆沉舟神一凛。陆沉舟头一震。陆沉舟脏猛地收缩。陆沉舟睛望向风沙。陆沉舟原地，终于开口。陆沉舟白芷宁即将离开前开口。陆沉舟那座半透明的晶质遗迹旁，心里一沉。";
        let issues = vec![
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟神一凛".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟头一震".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟脏猛".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟睛".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟原地".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟白芷宁".to_string(),
            "chapter body contains malformed phrase near stable character anchor `陆沉舟`: 陆沉舟那座半透明的晶质遗迹旁".to_string(),
        ];

        let repaired = apply_local_revision_suggestions(raw, &issues);

        assert!(repaired.contains("陆沉舟神色一凛"), "{repaired}");
        assert!(repaired.contains("陆沉舟心头一震"), "{repaired}");
        assert!(repaired.contains("陆沉舟心脏猛地收缩"), "{repaired}");
        assert!(repaired.contains("陆沉舟眼睛望向风沙"), "{repaired}");
        assert!(repaired.contains("陆沉舟站在原地"), "{repaired}");
        assert!(
            repaired.contains("陆沉舟在白芷宁即将离开前开口"),
            "{repaired}"
        );
        assert!(repaired.contains("陆沉舟站在晶质遗迹旁"), "{repaired}");
    }

    #[test]
    fn novel_continuous_plan_keeps_bounded_transient_step_retries() {
        let plan = build_novel_continuous_plan(
            "写小说",
            "writer",
            "/tmp/novel",
            Some(50_000),
            Some(2_500),
            1,
            18,
        );

        assert_eq!(plan.policy.max_retries_per_step, 10);
    }


    #[test]
    fn wrapped_studio_context_exposes_character_authority_to_chapter_runner() {
        let packet = json!({
            "success": true,
            "context": {
                "continuity_anchors": {
                    "primary_characters": ["姓名：宋昙川；角色：主角"],
                    "characters": ["宋昙川", "纪澹弦"]
                }
            }
        });

        let authority =
            novel_runner::CharacterAuthority::from_context(context_payload(&packet));

        assert_eq!(authority.protagonist.as_deref(), Some("宋昙川"));
        assert_eq!(authority.canonical_names, ["宋昙川", "纪澹弦"]);
    }


    #[test]
    fn draft_summary_with_json_title_residue_is_repaired_from_body() {
        let mut draft = novel_runner::DraftOutput {
            title: "第18章".to_string(),
            content: "宋昙川站在逻辑门前，纪澹弦握住他的手，两人决定关闭紫色光芒。".to_string(),
            summary: "\"title\": \"第18章\", 林墨在实验室继续研究。".to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            degraded: false,
            degraded_reason: String::new(),
        };

        repair_draft_summary_after_body_cleanup(&mut draft, "Chinese");

        assert!(draft.summary.contains("宋昙川"));
        assert!(!draft.summary.contains("\"title\""));
        assert!(!draft.summary.contains("林墨"));
    }

    #[test]
    fn reusable_unapproved_chapter_requires_substantive_length() {
        let long = novel_runner::DraftOutput {
            title: "第16章：权力的余震".to_string(),
            content: "林舒".repeat(2_500),
            summary: "既有草稿".to_string(),
            key_facts: vec![],
            continuity_updates: vec![],
            degraded: false,
            degraded_reason: String::new(),
        };
        let short = novel_runner::DraftOutput {
            title: "第16章".to_string(),
            content: "林舒".repeat(600),
            summary: "短稿".to_string(),
            key_facts: vec![],
            continuity_updates: vec![],
            degraded: false,
            degraded_reason: String::new(),
        };

        assert!(existing_unapproved_chapter_is_reusable(
            &long,
            Some(5000),
            "Chinese"
        ));
        assert!(!existing_unapproved_chapter_is_reusable(
            &short,
            Some(5000),
            "Chinese"
        ));
    }

    #[test]
    fn reusable_unapproved_chapter_rejects_degenerate_scene_loops() {
        let repeated = novel_runner::DraftOutput {
            title: "第2章：宗门阴影".to_string(),
            content: "季澈白握住剑柄，洛闻澜冷笑着逼近。你根本不是修剑的料还是赶紧滚出宗门吧。".repeat(80),
            summary: "退化循环草稿".to_string(),
            key_facts: vec![],
            continuity_updates: vec![],
            degraded: false,
            degraded_reason: String::new(),
        };

        assert!(chapter_body_has_degenerate_repetition(
            &repeated.content,
            "Chinese"
        ));
        assert!(!existing_unapproved_chapter_is_reusable(
            &repeated,
            Some(2500),
            "Chinese"
        ));
    }

    #[test]
    fn severe_body_issues_request_fresh_generation() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "findings": [{
                    "code": "body_truncated",
                    "class": "body_integrity",
                    "disposition": "hard_block",
                    "evidence_grade": "deterministic_invariant",
                    "source": "local_test",
                    "message": "正文未完成，结尾被截断",
                    "authority_fingerprint": "authority",
                    "body_fingerprint": "body"
                }],
                "issues": ["正文未完成，结尾被截断"],
                "repairable": [],
                "warnings": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": ["正文未完成，结尾被截断"]
            }
        });

        assert!(body_revision_required_after_audit(&write_result, &audit));
        assert!(!only_deterministic_cleanup_issues(&write_result, &audit));
    }

    #[test]
    fn repeated_paragraph_wording_does_not_create_a_blocker() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": ["Chinese chapter body repeats the same paragraph opening instead of advancing the scene: `谢谢你阮栖晚她轻声说道语气中带着一丝` repeated 2 times"],
                "repairable": [],
                "warnings": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            }
        });

        assert!(!body_revision_required_after_audit(&write_result, &audit));
        assert!(!only_deterministic_cleanup_issues(&write_result, &audit));
    }

    #[test]
    fn missing_chapter_read_result_is_not_approved_even_with_alternatives() {
        let read = json!({
            "success": false,
            "recoverable": true,
            "error_kind": "chapter_not_found",
            "requested_chapter": 8,
            "project_path": "/tmp/current-project",
            "alternative_projects": [
                {
                    "path": "/tmp/other-project",
                    "chapter": {
                        "number": 8,
                        "status": "approved"
                    }
                }
            ]
        });

        assert!(!read_chapter_result_is_approved(&read));
    }

    #[test]
    fn approved_chapter_read_result_is_approved() {
        let read = json!({
            "success": true,
            "chapter": {
                "number": 8,
                "status": "approved"
            }
        });

        assert!(read_chapter_result_is_approved(&read));
    }

    #[test]
    fn audit_passed_chapter_read_result_is_not_approved() {
        let read = json!({
            "success": true,
            "chapter": {
                "number": 8,
                "status": "audit_passed"
            }
        });

        assert!(!read_chapter_result_is_approved(&read));
    }

    #[test]
    fn chapter_llm_quality_audit_scope_keeps_bounded_sampling() {
        assert!(chapter_requires_llm_quality_audit(1));
        assert!(chapter_requires_llm_quality_audit(2));
        assert!(!chapter_requires_llm_quality_audit(3));
        assert!(!chapter_requires_llm_quality_audit(5));
        assert!(!chapter_requires_llm_quality_audit(200));
        assert!(!chapter_requires_llm_quality_audit(0));
    }

    #[test]
    fn delivery_window_output_accepts_only_non_authoritative_categories() {
        let parsed = parse_delivery_advisory_window_output(
            r#"{"advisories":[{"category":"dialogue","message":"区分说话节奏"},{"category":"hard_finding","message":"改掉主角身份"}],"score":188,"verdict":"blocked"}"#,
        )
        .expect("delivery advisory json");

        assert_eq!(parsed.score, Some(100));
        assert_eq!(parsed.advisories.len(), 1);
        assert_eq!(parsed.advisories[0].category, "dialogue");
    }

    #[test]
    fn parses_llm_quality_audit_json() {
        let parsed = parse_llm_quality_audit_output(
            r#"{"score":72,"authority_conflicts":[],"advisories":["节奏偏慢"]}"#,
        )
        .expect("audit json");

        assert_eq!(parsed.score, Some(72));
        assert!(parsed.authority_conflicts.is_empty());
        assert_eq!(parsed.advisories, vec!["节奏偏慢"]);
    }

    #[test]
    fn chapter_expansion_accepts_declared_cast_growth() {
        let existing = "洛玄川站在矿道尽头，意识到记忆献祭才是这片世界真正的代价。";
        let addition = "墨辞低声道出新的线索，指出黑市深处藏着第一块完整记忆碎片。";

        assert!(chapter_expansion_rejection_reason(existing, addition, "Chinese").is_none());
    }

    #[test]
    fn chapter_expansion_accepts_normal_yizhong_phrase() {
        let raw =
            r#"{"addition":"灵力在洛玄川掌心产生的一种细微回声，让他终于确认矿场深处并非死地。"}"#;

        assert!(raw_chapter_expansion_rejection_reason(raw, "Chinese").is_none());
    }

    #[test]
    fn chapter_tail_fragment_allows_quote_closed_by_existing_context() {
        let raw = r#"{"addition":"底。”他说完后，风声终于停了。"}"#;
        let existing = "他说：“断魂崖前，一座吊桥横跨在深渊";
        let addition = "底。”他说完后，风声终于停了。";

        assert!(raw_chapter_expansion_rejection_reason(raw, "Chinese").is_none());
        assert!(chapter_expansion_rejection_reason(existing, addition, "Chinese").is_none());
    }

    #[test]
    fn chapter_tail_fragment_still_rejects_unbalanced_combined_body() {
        let existing = "断魂崖前，一座吊桥横跨在深渊";
        let addition = "底。”他说完后，风声终于停了。";

        assert_eq!(
            chapter_expansion_rejection_reason(existing, addition, "Chinese").as_deref(),
            Some(
                "正文表面污染：unbalanced Chinese double quotes: expected matching “” pairs, found 0 openings and 1 closings"
            )
        );
    }

    #[test]
    fn chapter_expansion_rejects_prefix_that_replays_recent_tail() {
        let existing = "窗外的雨还在下，但景砚白的心里，却燃起了一团火焰。他知道，这场资本游戏才刚刚开始，而他，已经做好了准备。";
        let addition = "窗外的雨还在下，但景砚白的心里，却燃起了一团火焰。他知道，这场资本游戏才刚刚开始，而他，已经做好了准备。随后他拨通新的号码，继续追查那张银行卡背后的金鼎资本。";

        assert_eq!(
            chapter_expansion_rejection_reason(existing, addition, "Chinese").as_deref(),
            Some("扩写片段开头复述既有正文尾部")
        );
    }

    #[test]
    fn chapter_expansion_rejects_replayed_earlier_paragraph() {
        let existing = "晏闻宁站在落地窗前，望着脚下川流不息的车流，手中握着的咖啡早已凉透。手机在会议桌上的震动声突兀地划破了办公室的寂静，她瞥了眼来电显示，是谢澈舟的号码。\n\n苏晚晴把文件推到晏闻宁面前，笑意像一枚冷针。晏闻宁没有立刻接话，只把那份合同翻到第三页，发现签名栏旁边多出了一行她从未见过的附加条款。";
        let addition = "晏闻宁站在办公室的落地窗前，目光落在楼下川流不息的车流上。她手中的咖啡早已凉透，而她的思绪却比这杯冷掉的咖啡还要沉重。她知道，这场谈判不仅仅关乎她的职业地位。";

        assert_eq!(
            chapter_expansion_rejection_reason(existing, addition, "Chinese").as_deref(),
            Some("扩写片段复述既有正文段落")
        );
    }

    #[test]
    fn chapter_expansion_rejects_replayed_paragraph_after_new_material() {
        let existing = "孟望野和段岑安穿过废料堆，沿着隐蔽小路向海边走去。孟望野回头看见逐渐远去的城区，知道许照白的阴影仍笼罩着黑石港，但海鸥号即将扬帆。";
        let addition = "老锚地的木栈道在晨雾中显出轮廓，巴克率先踩上腐朽木板，提醒两人避开铁爪帮留下的暗哨，三人因此改变了原定路线。\n\n孟望野和段岑安穿过废料堆，沿着隐蔽小路向海边走去。孟望野回头看见逐渐远去的城区，知道许照白的阴影仍笼罩着黑石港，但海鸥号即将扬帆。";

        assert_eq!(
            chapter_expansion_rejection_reason(existing, addition, "Chinese").as_deref(),
            Some("扩写片段复述既有正文段落")
        );
    }

    #[test]
    fn chapter_expansion_context_keeps_opening_and_recent_tail() {
        let content = format!(
            "开头已完成封存数据。{}结尾停在等待复核。",
            "中段调查记录。".repeat(900)
        );
        let prompt = chapter_expansion_prompt(
            4,
            "风眼校准",
            "Chinese",
            2500,
            2500,
            2129,
            800,
            1,
            None,
            "主角已经完成初步校准。",
            &content,
            "本章只处理风眼窗口。",
        );

        assert!(prompt.contains("开头已完成封存数据"));
        assert!(prompt.contains("结尾停在等待复核"));
        assert!(prompt.contains("已写正文中段省略"));
        assert!(prompt.contains("当前正文末尾"));
    }

    #[test]
    fn novel_content_mutation_result_blocks_failed_quality_gate() {
        let revision = json!({
            "project_path": "/tmp/novel",
            "artifact_path": "/tmp/novel/chapters/0005.md",
            "txt_artifact_path": "/tmp/novel/exports/current.txt",
            "chapter": {
                "unit_count": 3752
            },
            "quality_gate": {
                "passed": false,
                "findings": [{
                    "code": "body_truncated",
                    "class": "body_integrity",
                    "disposition": "hard_block",
                    "evidence_grade": "deterministic_invariant",
                    "source": "local_test",
                    "message": "chapter body is truncated",
                    "authority_fingerprint": "authority",
                    "body_fingerprint": "body"
                }],
                "issues": ["chapter body is truncated"]
            }
        });
        let audit = json!({
            "success": true,
            "review": {
                "verdict": "passed",
                "locally_validated": true,
                "issues": []
            }
        });

        let result = format_novel_content_mutation_result("modify", 5, &revision, &audit);

        assert!(result.starts_with("status: blocked"));
        assert!(result.contains("runtime_effect: artifact.needs_revision"));
        assert!(result.contains("quality_gate_passed: false"));
        assert!(result.contains("audit_passed: true"));
        assert!(result.contains("blockers: chapter quality gate did not pass"));
    }

    #[test]
    fn chapter_audit_prompt_reuses_compose_context_authority() {
        let context = serde_json::to_string(&json!({
            "project": {
                "brief": "轨道保险调查必须沿既定证据链推进。"
            },
            "contract": {
                "premise": "顾闻舟调查轨道事故保险记录。",
                "main_causal_spine": "保险残片 -> 轨道碎片 -> 责任结论"
            },
            "plan": {
                "number": 4,
                "goal": "只核对轨道碎片的保险证据，不提前揭晓幕后操作者。",
                "expected_turn": "确认保单时间戳"
            },
            "recent_chapters": [
                {
                    "number": 3,
                    "summary": "顾闻舟尚未确认事故是人为制造。",
                    "status": "approved"
                }
            ],
            "narrative_progress": {
                "next_chapter_boundary": "第5章带着时间戳前往听证会，证人首次出席。"
            }
        }))
        .expect("compose context json");
        let prompt = llm_quality_audit_prompt(
            "zh-CN",
            4,
            "险单残片",
            &[],
            &context,
            "顾闻舟开始核对证据。",
        );

        assert!(context.contains("不提前揭晓幕后操作者"));
        assert!(context.contains("尚未确认事故是人为制造"));
        assert!(context.contains("核对轨道碎片"));
        assert!(context.contains("前往听证会"));
        assert!(context.contains("\"status\":\"approved\""));
        assert!(prompt.contains("合同与连续性权威"));
        assert!(prompt.contains("不提前揭晓幕后操作者"));
        assert!(prompt.contains("绝不能因为下一章事件尚未发生而判错"));
        assert!(prompt.contains("必须单独检查正文最后 3 段"));
        assert!(prompt.contains("短动作段"));
        assert!(prompt.contains("同一关键物件在本章内的来源、持有者、位置、状态和首次获得事件"));
        assert!(prompt.contains("主要人物对白是否同质化"));
        assert!(prompt.contains("这些表现问题即使明显也不得写入 authority_conflicts"));
        assert!(!prompt.contains("需要重写的跨章重复"));
    }

    #[test]
    fn audit_projection_preserves_next_chapter_exclusion() {
        let authority = serde_json::to_string(&json!({
            "schema_version": "benshu.sealed_chapter_authority.v1",
            "role": "auditor",
            "authority": {
                "working_context": {
                    "chapter_number": 1,
                    "contract": {
                        "outline": {
                            "near_chapters": [{
                                "number": 1,
                                "goal": "只在账阁发现金色丝线",
                                "expected_turn": "记下被抹去的编号"
                            }]
                        }
                    }
                },
                "context_package": {
                    "selected_context": [{
                        "source": "contract.md",
                        "excerpt": "第1章 只在账阁发现金色丝线\n第2章 才与看守人闻维棠在矿脉深处相遇并确认荒骨灵力"
                    }]
                }
            }
        }))
        .expect("audit authority");
        let prompt = llm_quality_audit_prompt(
            "zh-CN",
            1,
            "金线残账",
            &[],
            &authority,
            "闻维棠已在账阁向主角解释荒骨灵力。",
        );

        assert!(!prompt.contains("下一章边界（只作为禁区，不得在本章完成）"));
        assert!(prompt.contains("才与看守人闻维棠在矿脉深处相遇"));
        assert!(prompt.contains("只在账阁发现金色丝线"));
    }

    #[test]
    fn chapter_authority_focus_survives_large_context_prefix() {
        let context = format!(
            r#"{{"padding":"{}","contract":{{"outline":{{"near_chapters":[{{"number":1,"goal":"只发现枯草与玉简碑相连","expected_turn":"观察到寿元数字减少"}},{{"number":2,"goal":"回村进行第一次药炉提纯","expected_turn":"发现药渣能够延缓衰老"}}]}}}}}}"#,
            "x".repeat(16_000)
        );
        let audit = llm_quality_audit_prompt(
            "zh-CN",
            1,
            "枯草连碑",
            &[],
            &context,
            "闻栖原发现枯草与玉简碑相连。",
        );
        let expansion = chapter_expansion_prompt(
            1,
            "枯草连碑",
            "zh-CN",
            2500,
            2200,
            1900,
            400,
            1,
            None,
            "闻栖原发现了异常。",
            "闻栖原站在枯草旁。",
            &context,
        );
        let tail = chapter_tail_completion_prompt(
            1,
            "枯草连碑",
            "zh-CN",
            300,
            "闻栖原发现了异常。",
            "闻栖原站在枯草旁，抬头看见",
            &["chapter body ends with an incomplete sentence".to_string()],
            &context,
        );

        for prompt in [&audit, &expansion, &tail] {
            assert!(prompt.contains("只发现枯草与玉简碑相连"));
            assert!(prompt.contains("回村进行第一次药炉提纯"));
            assert!(prompt.contains("发现药渣能够延缓衰老"));
            assert!(!prompt.contains("下一章边界（只作为禁区，不得在本章完成）"));
        }
    }

    #[test]
    fn supplemental_generation_prompts_reuse_chapter_authority_and_future_boundary() {
        let authority = "[CURRENT CHAPTER CONTRACT]\n本章只核对保险残片。\n\
                         NEXT OUTLINE NODE (future boundary, not for completion in this chapter): 前往听证会";
        let expansion = chapter_expansion_prompt(
            4,
            "险单残片",
            "zh-CN",
            2500,
            2250,
            2100,
            400,
            1,
            None,
            "顾闻舟正在核对保险残片。",
            "他将残片放到灯下。",
            authority,
        );
        let tail = chapter_tail_completion_prompt(
            4,
            "险单残片",
            "zh-CN",
            600,
            "顾闻舟正在核对保险残片。",
            "他将残片放到灯下，时间戳显示",
            &["chapter body ends with an incomplete sentence".to_string()],
            authority,
        );

        for prompt in [&expansion, &tail] {
            assert!(prompt.contains("本章只核对保险残片"));
            assert!(prompt.contains("前往听证会"));
            assert!(prompt.contains("下一章节点只作为禁区"));
            assert!(prompt.contains("不得发明合同外的新谜团"));
        }
    }

    #[test]
    fn rejected_expansion_retry_carries_feedback_and_changes_attempt_identity() {
        let prompt = chapter_expansion_prompt(
            1,
            "险单残片",
            "zh-CN",
            2500,
            2250,
            2100,
            400,
            2,
            Some("扩写片段复述既有正文段落"),
            "顾闻舟正在核对保险残片。",
            "他将残片放到灯下。",
            "本章只核对保险残片。",
        );

        assert!(prompt.contains("第 2 次扩写尝试"));
        assert!(prompt.contains("上一扩写尝试被拒绝"));
        assert!(prompt.contains("扩写片段复述既有正文段落"));
        assert!(prompt.contains("不能再次生成相同片段"));
    }

    #[test]
    fn revision_guidance_keeps_memo_authority_above_conflicting_review_advice() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "下一章边界要求证人出席，但本章尚未让证人出席。"
                ]
            }
        });

        let mode = revision_mode_for_results(&write_result, &audit);
        let guidance = revision_guidance(5, &write_result, &audit, "zh-CN", mode);

        assert!(guidance.contains("审稿意见只是待核对的问题"));
        assert!(guidance.contains("不得覆盖章节 memo 和架构"));
        assert!(guidance.contains("下一章边界"));
        assert!(guidance.contains("保持未发生"));
    }

    #[test]
    fn future_boundary_revision_is_not_mislabeled_as_a_finale() {
        let write_result = json!({
            "quality_gate": {
                "findings": [{
                    "code": "future_chapter_consumed",
                    "class": "continuity",
                    "disposition": "hard_block",
                    "evidence_grade": "evidence_backed_semantic",
                    "source": "local_test",
                    "message": "chapter 1 consumes the sealed chapter 2 boundary early",
                    "authority_evidence": [{
                        "path": "/authority/next_chapter_boundary",
                        "excerpt": "叶知宁测绘局部地形"
                    }],
                    "body_evidence": [{
                        "start": 0,
                        "end": 18,
                        "excerpt": "他完成了这片局部区域的基础测绘。"
                    }],
                    "authority_fingerprint": "authority",
                    "body_fingerprint": "body"
                }]
            }
        });
        let audit = json!({});

        let mode = revision_mode_for_results(&write_result, &audit);
        let guidance = revision_guidance(1, &write_result, &audit, "zh-CN", mode);

        assert!(guidance.contains("章节边界"));
        assert!(guidance.contains("不代表全书进入终局或尾声"));
        assert!(guidance.contains("下一章事件可以被预示或准备"));
        assert!(guidance.contains("叶知宁测绘局部地形"));
        assert!(guidance.contains("他完成了这片局部区域的基础测绘。"));
        assert!(!guidance.contains("这是终局/尾声修订"));
        assert_eq!(mode, novel_runner::RevisionMode::LocalRepair);
        assert!(guidance.contains("以当前正文为底稿做局部修补"));
        assert!(!guidance.contains("从头生成一版完整正文"));
    }
