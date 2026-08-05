use super::super::*;
use chrono::Utc;

fn complete_cjk_test_body(seed: &str) -> String {
    let mut body = seed.to_string();
    let openings = [
        "晨雾散开时",
        "钟声停下后",
        "潮声逼近前",
        "暮色落入长廊时",
        "风向突然改变后",
        "远处灯火熄灭前",
        "守夜即将结束时",
        "旧门再次震动后",
    ];
    let actions = [
        "主角重新核对现场留下的每一道痕迹",
        "同行者把刚才的选择逐项记入记录",
        "众人沿着真实线索确认下一步方向",
        "队伍避开未经证实的猜测继续前行",
        "观察者从细节中找出因果变化",
        "行动者用眼前证据校准原有判断",
        "守望者确认承诺仍与此前一致",
        "在场的人把风险与代价说得清楚",
    ];
    let consequences = [
        "因此没有遗失先前建立的事实",
        "于是新的决定拥有可以追溯的依据",
        "这让冲突沿着既定因果继续推进",
        "从而避免把未知结果当成既成事实",
        "随后各人的位置和目标都保持清晰",
        "并让尚未解决的问题继续留在视野中",
        "最终当前场景获得了明确而有限的结果",
        "同时后续行动仍受既有规则约束",
    ];
    let endings = [
        "他们随后走向下一个可验证的节点。",
        "现场的变化也被完整保留下来。",
        "没有人用一句结论跳过必要过程。",
        "这个选择将成为下一场行动的起点。",
        "所有人都清楚代价尚未消失。",
        "未完成的承诺仍等待后续兑现。",
        "新的局面由此自然接续旧的局面。",
        "故事只向前推进了当前允许的一步。",
    ];
    let mut index = 0usize;
    while body.chars().count() < 2_600 {
        let opening = openings[index % openings.len()];
        let action = actions[(index / openings.len()) % actions.len()];
        let consequence =
            consequences[(index / (openings.len() * actions.len())) % consequences.len()];
        let ending = endings
            [(index / (openings.len() * actions.len() * consequences.len())) % endings.len()];
        body.push_str(opening);
        body.push_str(action);
        body.push_str(consequence);
        body.push_str(ending);
        index += 1;
    }
    body
}

fn complete_english_test_body(seed: &str) -> String {
    let mut body = seed.to_string();
    for index in 0..2_600 {
        body.push_str(&format!(" routeword{index:04}"));
    }
    body
}

async fn seal_test_chapter_authority(
    tool: &NovelStudioTool,
    project_path: &str,
    chapter_number: usize,
) {
    let mut typed_manifest = tool
        .read_manifest(std::path::Path::new(project_path))
        .await
        .expect("test manifest");
    if typed_manifest.target_units.is_none() {
        typed_manifest.target_units = Some(100_000);
    }
    if typed_manifest.chapter_unit_target.is_none() {
        typed_manifest.chapter_unit_target = Some(2_500);
    }
    tool.write_manifest(std::path::Path::new(project_path), &typed_manifest)
        .await
        .expect("persist complete test scope");
    let manifest: serde_json::Value =
        serde_json::to_value(&typed_manifest).expect("test manifest json");
    let contract = manifest.get("contract").cloned().unwrap_or_default();
    let premise = contract
        .get("premise")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("测试主角依照既定因果推进本章目标。")
        .to_string();
    let characters = contract
        .get("characters")
        .and_then(serde_json::Value::as_array)
        .filter(|items| !items.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            vec![serde_json::Value::String(
                "name: 测试主角; role: 主角; desire: 完成本章目标; fear: 失去关键线索; bottom_line: 不伤害无辜者"
                    .to_string(),
            )]
        });
    let world_rules = contract
        .get("world_rules")
        .and_then(serde_json::Value::as_array)
        .filter(|items| !items.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            vec![serde_json::Value::String(
                "关键能力的使用必须付出可见代价".to_string(),
            )]
        });
    let outline = contract
        .get("outline")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(
            "主角沿既定因果线索推进冲突，在付出代价后取得阶段结果，并保留下一章边界。",
        )
        .to_string();
    let reader_promise = contract
        .pointer("/structured_contract_v2/reader_promise")
        .filter(|value| {
            value
                .get("core_hook")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hook| !hook.trim().is_empty())
        })
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "core_hook": "主角如何在既定规则与代价下完成核心目标"
            })
        });
    let target_units = manifest
        .get("target_units")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(100_000);
    let chapter_unit_target = manifest
        .get("chapter_unit_target")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| matches!(*value, 2500 | 5000))
        .unwrap_or(2500);
    let contract_result = tool.call(
        &serde_json::json!({
            "action": "set_contract",
            "project_path": project_path,
            "premise": premise,
            "characters": characters,
            "world_rules": world_rules,
            "outline": outline,
            "reader_promise": reader_promise,
            "target_units": target_units,
            "chapter_unit_target": chapter_unit_target
        })
        .to_string(),
    )
    .await
    .expect("complete test authority contract");
    let contract_result: serde_json::Value =
        serde_json::from_str(&contract_result).expect("test contract result json");
    assert_eq!(contract_result["success"], true, "{contract_result}");
    let context_result = tool.call(
        &serde_json::json!({
            "action": "compose_context",
            "project_path": project_path,
            "chapter_number": chapter_number
        })
        .to_string(),
    )
    .await
    .expect("compose test context");
    let context_result: serde_json::Value =
        serde_json::from_str(&context_result).expect("test context result json");
    assert_eq!(context_result["success"], true, "{context_result}");
    let seal_result = tool.call(
        &serde_json::json!({
            "action": "persist_execution_package",
            "project_path": project_path,
            "chapter_number": chapter_number,
            "chapter_title": format!("测试章节{chapter_number}"),
            "plan": format!("第{chapter_number}章只完成当前合同允许的目标。"),
            "content": "场景顺序：进入、冲突、选择、结果。结尾保留后续边界。"
        })
        .to_string(),
    )
    .await
    .expect("seal test chapter authority");
    let seal_result: serde_json::Value =
        serde_json::from_str(&seal_result).expect("test seal result json");
    assert_eq!(seal_result["success"], true, "{seal_result}");
}

async fn persist_test_best_candidate(project_path: &str, chapter_number: usize) {
    let project_dir = std::path::Path::new(project_path);
    let manifest: NovelProjectManifest = serde_json::from_str(
        &tokio::fs::read_to_string(project_dir.join("project.json"))
            .await
            .expect("test manifest"),
    )
    .expect("test manifest json");
    let chapter = manifest
        .chapters
        .iter()
        .find(|chapter| chapter.number == chapter_number)
        .expect("test chapter");
    let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path))
        .await
        .expect("test chapter body");
    let content = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
    let authority = read_sealed_chapter_authority(project_dir, &manifest, chapter_number)
        .await
        .expect("test sealed authority");
    let draft = crate::tool::writing::novel_runner::DraftOutput {
        title: chapter.title.clone(),
        content: content.clone(),
        summary: chapter.summary.clone(),
        key_facts: chapter.key_facts.clone(),
        continuity_updates: chapter.continuity_updates.clone(),
        degraded: false,
        degraded_reason: String::new(),
    };
    let metadata_fingerprint = governance::authority_fingerprint(&serde_json::json!({
        "title": &draft.title,
        "summary": &draft.summary,
        "key_facts": &draft.key_facts,
        "continuity_updates": &draft.continuity_updates
    }));
    let record = governance::DraftCandidateRecord {
        candidate_id: format!("test-best-{chapter_number}"),
        parent_candidate_id: None,
        authority_fingerprint: authority.authority_root_fingerprint,
        body_fingerprint: chapter_quality::chapter_body_fingerprint(&content),
        metadata_fingerprint,
        draft,
        findings: Vec::new(),
        quality_vector: governance::RevisionQualityVector::default(),
        provenance: governance::CandidateProvenance::InitialDraft,
        accepted_as_best: true,
    };
    let path = project_dir
        .join("reviews/candidates")
        .join(format!("chapter-{chapter_number:04}.best.json"));
    tokio::fs::create_dir_all(path.parent().expect("best candidate parent"))
        .await
        .expect("candidate directory");
    tokio::fs::write(path, serde_json::to_vec_pretty(&record).expect("candidate json"))
        .await
        .expect("best candidate");
}

#[tokio::test]
async fn novel_studio_definition_schema_exposes_curated_public_actions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let definition = tool.definition().await;
    let actions = definition
        .parameters
        .pointer("/properties/action/enum")
        .and_then(|value| value.as_array())
        .expect("action enum");
    let contains_action = |action: &str| actions.iter().any(|value| value == action);

    assert!(contains_action("persist_execution_package"));
    for hidden in [
        "compose_chapter",
        "plan_chapter",
        "architect_chapter",
        "add_chapter_plan",
        "repair_latest_chapter_metadata",
    ] {
        assert!(
            !contains_action(hidden),
            "{hidden} should remain internal compatibility surface"
        );
    }
    assert!(definition
        .parameters
        .pointer("/properties/chapter_unit_target/enum")
        .is_some());
    for field in [
        "ending_direction",
        "protagonist_arc",
        "world_imagery",
        "main_causal_spine",
        "title_rationale",
    ] {
        assert!(
            definition
                .parameters
                .pointer(&format!("/properties/{field}"))
                .is_some(),
            "{field} should be part of the public contract schema"
        );
    }
}

#[tokio::test]
async fn novel_studio_missing_action_guidance_matches_public_schema() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let definition = tool.definition().await;
    let schema_actions = definition
        .parameters
        .pointer("/properties/action/enum")
        .and_then(|value| value.as_array())
        .expect("action enum")
        .clone();

    let result: serde_json::Value = serde_json::from_str(
        &tool
            .call("{}")
            .await
            .expect("missing action should be recoverable json"),
    )
    .expect("missing action json");
    assert_eq!(
        result
            .get("available_actions")
            .and_then(|value| value.as_array())
            .expect("available actions"),
        &schema_actions
    );
}

#[test]
fn novel_studio_internal_compat_actions_have_public_hints() {
    for action in super::super::tool_schema::INTERNAL_COMPAT_ACTIONS {
        assert!(
            !super::super::tool_schema::PUBLIC_ACTIONS.contains(action),
            "{action} should remain hidden from the public tool surface"
        );
        assert!(
            super::super::tool_schema::internal_compat_action_hint(action).is_some(),
            "{action} should have a canonical public-action hint"
        );
    }
}

#[tokio::test]
async fn persist_execution_package_writes_plan_and_architecture_without_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let init: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "init_project",
                        "title": "折光城试炼",
                        "language": "zh-cn",
                        "genre": "都市玄幻",
                        "brief": "底层学生在折光城考试体系中逆袭。",
                        "target_units": 50_000,
                        "chapter_unit_target": 2_500,
                        "contract": "主角秦澈在折光城学院考试中发现灵能评分被操纵，最终打破城市晋级制度。",
                        "characters": ["name: 秦澈; role: 主角; desire: 改写考试命运"],
                        "outline": "从入学考试失败开始，逐步揭开评分制度背后的灵能垄断。"
                    })
                    .to_string(),
                )
                .await
                .expect("init output"),
        )
        .expect("init json");
    let project_path = init
        .get("project_path")
        .and_then(|value| value.as_str())
        .expect("project path");
    tool.call(
        &serde_json::json!({
            "action": "set_contract",
            "project_path": project_path,
            "premise": "主角秦澈在折光城学院考试中发现灵能评分被操纵，最终打破城市晋级制度。",
            "characters": ["name: 秦澈; role: 主角; desire: 改写考试命运"],
            "world_rules": ["折光城的晋级资格由可审计的灵能评分决定"],
            "outline": "从入学考试失败开始，逐步揭开评分制度背后的灵能垄断。",
            "reader_promise": {
                "core_hook": "被压低的考试评分如何成为推翻城市晋级制度的证据"
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

    let unresolved_error = tool
        .call(
            &serde_json::json!({
                "action": "persist_execution_package",
                "project_path": project_path,
                "chapter_number": 1,
                "chapter_title": "折光入场",
                "plan": "第1章目标：秦澈在入学测试中发现折光城评分被人为压低。",
                "content": "场景顺序：入场、测试失败、发现评分异常、决定追查。",
                "new_character_requests": [{
                    "request_id": "chapter-examiner",
                    "role": "",
                    "narrative_purpose": ""
                }]
            })
            .to_string(),
        )
        .await
        .expect_err("incomplete character request must fail before sealing");
    assert!(
        unresolved_error
            .to_string()
            .contains("unresolved character requests"),
        "{unresolved_error}"
    );
    let manifest_before_valid_persist: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::path::Path::new(project_path).join("project.json"))
            .expect("manifest before valid persist"),
    )
    .expect("manifest json before valid persist");
    assert_eq!(
        manifest_before_valid_persist["chapter_plans"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        manifest_before_valid_persist["context_packages"][0]["sealed"].as_bool(),
        Some(false)
    );
    assert!(
        !std::path::Path::new(project_path)
            .join("plans/authorities/chapter-0001.authority.json")
            .exists(),
        "an unresolved request must not leave a reusable sealed authority"
    );

    let persisted: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "persist_execution_package",
                        "project_path": project_path,
                        "chapter_number": 1,
                        "chapter_title": "折光入场",
                        "plan": "第1章目标：秦澈在入学测试中发现折光城评分被人为压低。",
                        "content": "场景顺序：入场、测试失败、发现评分异常、决定追查。冲突：制度判定与个人感知相反。结尾钩子：隐藏评分日志露出陌生签名。",
                        "future_chapters": [
                            {
                                "number": 2,
                                "goal": "秦澈核对原始评分日志并锁定第一次人为改写的时间戳。",
                                "expected_turn": "秦澈取得可供复核的异常时间戳。"
                            },
                            {
                                "number": 3,
                                "goal": "秦澈根据时间戳追查评分改写权限的实际持有人。",
                                "expected_turn": "调查对象从评分设备转向拥有改写权限的人。"
                            },
                            {
                                "number": 4,
                                "goal": "秦澈用权限记录迫使学院监察员回应评分操纵证据。",
                                "expected_turn": "学院监察线被正式卷入评分争议。"
                            }
                        ]
                    })
                    .to_string(),
                )
                .await
                .expect("persist output"),
        )
        .expect("persist json");

    assert_eq!(
        persisted.get("success").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        persisted.get("sealed").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        persisted
            .pointer("/protected_coverage/complete")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        persisted
            .get("role_projection_fingerprints")
            .and_then(|value| value.as_object())
            .map(serde_json::Map::len),
        Some(4)
    );
    let root_fingerprint = persisted
        .get("authority_root_fingerprint")
        .and_then(|value| value.as_str())
        .expect("authority root fingerprint");
    assert!(!root_fingerprint.is_empty());
    let sealed_authority = persisted
        .get("sealed_authority")
        .expect("sealed authority");
    assert_eq!(
        sealed_authority
            .get("authority_root_fingerprint")
            .and_then(|value| value.as_str()),
        Some(root_fingerprint)
    );
    assert!(sealed_authority
        .pointer("/role_projections/writer/payload/authority/working_context/story_bible")
        .is_some());
    assert!(sealed_authority
        .pointer(
            "/role_projections/writer/payload/authority/working_context/story_bible/structured_contract_v2"
        )
        .is_none());
    let protected_decisions = sealed_authority
        .pointer("/trace/selection_decisions")
        .and_then(serde_json::Value::as_array)
        .expect("authority selection trace")
        .iter()
        .filter(|decision| decision["layer"] == "protected")
        .collect::<Vec<_>>();
    assert!(!protected_decisions.is_empty());
    assert!(protected_decisions.iter().all(|decision| {
        decision["truncated"] == false
            && decision["selected_chars"] == decision["original_chars"]
    }));
    let manifest_raw =
        std::fs::read_to_string(std::path::Path::new(project_path).join("project.json"))
            .expect("manifest");
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).expect("manifest json");
    assert_eq!(
        manifest
            .get("chapter_plans")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        manifest
            .get("chapter_architectures")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        manifest
            .get("chapters")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        manifest
            .pointer("/story_bible/narrative_graph/chapter_goals")
            .and_then(serde_json::Value::as_array)
            .map(|goals| {
                goals
                    .iter()
                    .filter_map(|goal| goal["chapter_number"].as_u64())
                    .collect::<Vec<_>>()
            }),
        Some(vec![2, 3, 4])
    );
    assert_eq!(
        manifest
            .pointer("/story_bible/narrative_graph/chapter_goals")
            .and_then(serde_json::Value::as_array)
            .and_then(|goals| goals.iter().find(|goal| goal["chapter_number"] == 4))
            .and_then(|goal| goal.get("goal")),
        Some(&json!("秦澈用权限记录迫使学院监察员回应评分操纵证据。"))
    );
    let chapter_three_context: serde_json::Value = serde_json::from_str(
        &tool
            .call(
                &serde_json::json!({
                    "action": "compose_context",
                    "project_path": project_path,
                    "chapter_number": 3
                })
                .to_string(),
            )
            .await
            .expect("chapter three context"),
    )
    .expect("chapter three context json");
    assert_eq!(
        chapter_three_context.pointer("/context/next_chapter_boundary/0/number"),
        Some(&json!(4))
    );
    assert_eq!(
        chapter_three_context.pointer("/context/next_chapter_boundary/0/goal"),
        Some(&json!(
            "秦澈用权限记录迫使学院监察员回应评分操纵证据。"
        ))
    );
    assert_eq!(
        chapter_three_context
            .pointer("/execution_authority_context/current_chapter_goal/0/number"),
        Some(&json!(3))
    );
    assert_eq!(
        chapter_three_context
            .pointer("/execution_authority_context/current_chapter_goal/0/goal"),
        Some(&json!(
            "秦澈根据时间戳追查评分改写权限的实际持有人。"
        ))
    );
    assert_eq!(
        chapter_three_context
            .pointer("/execution_authority_context/rolling_outline_window/0/number"),
        Some(&json!(4))
    );
    let authority_record = manifest
        .get("context_packages")
        .and_then(serde_json::Value::as_array)
        .and_then(|records| records.first())
        .expect("sealed authority record");
    for key in ["path", "rules_path", "trace_path"] {
        let relative = authority_record
            .get(key)
            .and_then(serde_json::Value::as_str)
            .expect("durable authority path");
        assert!(
            relative.starts_with("plans/authorities/"),
            "{key} must survive lightweight snapshots: {relative}"
        );
        assert!(std::path::Path::new(project_path).join(relative).exists());
    }
    assert!(std::path::Path::new(project_path)
        .join("plans/0001_折光入场.md")
        .exists());
    assert!(std::path::Path::new(project_path)
        .join("plans/0001_折光入场_architecture.md")
        .exists());

    tool.call(
        &serde_json::json!({
            "action": "write_draft",
            "project_path": project_path,
            "chapter_number": 1,
            "chapter_title": "折光入场",
            "content": "秦澈站上折光城学院的测试台，校准灯依次亮起。他按教官要求完成灵能回路，屏幕却把稳定读数压成零分。秦澈没有争辩，而是记下日志闪过的陌生签名，决定在封存前核对原始评分。"
        })
        .to_string(),
    )
    .await
    .expect("legacy draft seed");
    let manifest_path = std::path::Path::new(project_path).join("project.json");
    let mut legacy_manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).expect("legacy manifest"),
    )
    .expect("legacy manifest json");
    legacy_manifest["context_packages"] = serde_json::json!([]);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&legacy_manifest).expect("legacy manifest bytes"),
    )
    .expect("remove legacy authority record");
    let repaired: serde_json::Value = serde_json::from_str(
        &tool
            .call(
                &serde_json::json!({
                    "action": "repair_project_state",
                    "project_path": project_path
                })
                .to_string(),
            )
            .await
            .expect("repair legacy unapproved chapter"),
    )
    .expect("repair result");
    assert_eq!(repaired["success"], true);
    assert_eq!(
        repaired
            .get("migrated_legacy_candidates")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let candidate_path = repaired
        .pointer("/migrated_legacy_candidates/0/candidate_path")
        .and_then(serde_json::Value::as_str)
        .expect("candidate record path");
    let candidate_record: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::path::Path::new(project_path).join(candidate_path))
            .expect("candidate record"),
    )
    .expect("candidate record json");
    assert_eq!(
        candidate_record
            .get("provenance")
            .and_then(serde_json::Value::as_str),
        Some("legacy_candidate")
    );
    assert!(candidate_record
        .pointer("/draft/content")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|body| body.contains("秦澈站上折光城学院")));
    let migrated_manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).expect("migrated manifest"),
    )
    .expect("migrated manifest json");
    assert_eq!(
        migrated_manifest.pointer("/chapters/0/status"),
        Some(&json!("needs_revision"))
    );

    tool.call(
        &serde_json::json!({
            "action": "set_contract",
            "project_path": project_path,
            "premise": "主角秦澈在折光城学院考试中发现灵能评分被操纵，最终打破城市晋级制度。",
            "characters": ["name: 秦澈; role: 主角; desire: 改写考试命运"],
            "outline": "从入学考试失败开始，逐步揭开评分制度背后的灵能垄断。",
            "reader_promise": {
                "core_hook": "更换后的核心承诺不能静默解释已经封存的第一章"
            }
        })
        .to_string(),
    )
    .await
    .expect("updated contract");
    let refreshed = tool
        .call(
            &serde_json::json!({
                "action": "compose_context",
                "project_path": project_path,
                "chapter_number": 1
            })
            .to_string(),
        )
        .await
        .expect("changed contract must allow fresh context after invalidation");
    let refreshed: serde_json::Value =
        serde_json::from_str(&refreshed).expect("refreshed context json");
    assert_eq!(refreshed["success"], true, "{refreshed}");
    assert_eq!(refreshed["sealed"], false, "{refreshed}");
}

#[tokio::test]
async fn contract_world_terms_do_not_trigger_character_drift() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let init: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "init_project",
                        "title": "旧城感知账",
                        "language": "zh-CN",
                        "genre": "都市玄幻",
                        "brief": "普通学生追查符文网络吞噬感知的真相。",
                        "target_units": 50_000,
                        "chapter_unit_target": 2_500,
                        "premise": "旧城符文网络会按账册抽取普通人的感知。",
                        "ending_direction": "主角公开感知账册并重写城市能量规则。",
                        "protagonist_arc": "从自保的学生成长为城市规则守门人。",
                        "world_imagery": "雨夜天桥、符文网络、感知账册、旧城节点。",
                        "main_causal_spine": "觉醒瞳术，看见符文网络，追查感知账册，终局公开账册。",
                        "title_rationale": "旧城和感知账都来自终局公开账册的核心事件。",
                        "characters": [
                            "name: 许闻桥; role: 主角; desire: 查清感知被夺真相; fear: 妹妹被吞噬记忆; bottom_line: 不牺牲普通人; arc_start: 边缘学生; arc_end: 城市规则守门人",
                            "name: 商砚衡; role: 关键对手; desire: 维持符文网络垄断; fear: 感知账册公开; bottom_line: 不允许底层越过旧秩序; arc_start: 垄断维护者; arc_end: 被新规则审判"
                        ],
                        "world_rules": [
                            "符文网络会按感知账册抽取普通人的感知",
                            "旧城节点决定城市能量流向"
                        ],
                        "outline": "许闻桥在雨夜天桥看见符文网络，追查感知账册，最终公开账册并改写城市规则。"
                    })
                    .to_string(),
                )
                .await
                .expect("init output"),
        )
        .expect("init json");
    let project_path = init
        .get("project_path")
        .and_then(|value| value.as_str())
        .expect("project path");
    let set_contract_args = json!({
                "action": "set_contract",
                "project_path": project_path,
                "premise": "旧城符文网络会按账册抽取普通人的感知。",
                "characters": [
                    "name: 许闻桥; role: 主角; desire: 查清感知被夺真相; fear: 妹妹被吞噬记忆; bottom_line: 不牺牲普通人; arc_start: 边缘学生; arc_end: 城市规则守门人",
                    "name: 商砚衡; role: 关键对手; desire: 维持符文网络垄断; fear: 感知账册公开; bottom_line: 不允许底层越过旧秩序; arc_start: 垄断维护者; arc_end: 被新规则审判"
                ],
                "world_rules": [
                    "符文网络会按感知账册抽取普通人的感知",
                    "旧城节点决定城市能量流向"
                ],
                "outline": "许闻桥在雨夜天桥看见符文网络，追查感知账册，最终公开账册并改写城市规则。"
            })
            .to_string();
    tool.call(&set_contract_args)
        .await
        .expect("set contract output");
    let manifest = tool
        .read_manifest(std::path::Path::new(project_path))
        .await
        .expect("manifest");
    let primary_name = manifest
        .character_ledger
        .iter()
        .find(|character| character.role.contains("主角"))
        .or_else(|| manifest.character_ledger.first())
        .map(|character| character.canonical_name.clone())
        .expect("primary character");
    let chapter = ChapterRecord {
        number: 1,
        title: "雨夜天桥".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: format!("{primary_name}看见符文网络和旧城节点。"),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["符文网络会抽取感知。".to_string()],
        continuity_updates: vec!["旧城节点首次出现。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = format!("{primary_name}站在雨夜天桥下，看见符文网络在旧城节点之间亮起。符文网络不是人，而是城市能量规则的一部分；符文网络越亮，感知账册上的名字越模糊。");

    let view = contract_term_authority_view(&manifest);
    assert!(view.character_names.contains(&primary_name));
    assert!(view.world_terms.contains("符文"));
    assert!(view.world_terms.contains("符文网络"));

    let issues = contract_character_drift_issues(&manifest, &chapter, &content);
    assert!(
        issues.is_empty(),
        "world terms should not be treated as character drift: {issues:?}"
    );
}

#[tokio::test]
async fn character_drift_does_not_block_sentence_fragments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let init: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "init_project",
                        "title": "破局之眼",
                        "language": "zh-CN",
                        "genre": "异界修仙",
                        "brief": "主角踏入古老符文局，追查封印真相。",
                        "target_units": 50_000,
                        "chapter_unit_target": 2_500,
                        "premise": "上古符文局吞噬修士灵力，主角必须找到真相。",
                        "ending_direction": "主角公开封印真相并重写修行规则。",
                        "protagonist_arc": "从被卷入局中的新人变成破局者。",
                        "world_imagery": "云海、金色符文、上古封印、残剑。",
                        "main_causal_spine": "觉醒天赋，触碰残剑，进入符文局，最终破除封印。",
                        "title_rationale": "破局之眼来自主角终局看破封印规则的核心爽点。",
                        "characters": [
                            "name: 谢阙川; role: 主角; desire: 查清封印真相; fear: 被符文吞噬自我; bottom_line: 不以无辜者献祭; arc_start: 新晋修士; arc_end: 破局者",
                            "name: 陶砚遥; role: 关键同伴; desire: 阻止封印失控; fear: 旧局重演; bottom_line: 不把谢阙川当祭品; arc_start: 警告者; arc_end: 共同破局者"
                        ],
                        "world_rules": [
                            "金色符文会抽取靠近者的灵力",
                            "上古封印必须以代价换取真相"
                        ],
                        "outline": "谢阙川在云海中看见符文似乎能回应残剑，但他已经无法回头，只能与陶砚遥一起破局。"
                    })
                    .to_string(),
                )
                .await
                .expect("init output"),
        )
        .expect("init json");
    let project_path = init
        .get("project_path")
        .and_then(|value| value.as_str())
        .expect("project path");
    let manifest = tool
        .read_manifest(std::path::Path::new(project_path))
        .await
        .expect("manifest");
    let chapter = ChapterRecord {
        number: 1,
        title: "符文闪烁".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "谢阙川触碰残剑并看见金色符文。".to_string(),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["符文似乎能回应残剑。".to_string()],
        continuity_updates: vec!["谢阙川已经无法回头。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "谢阙川握紧残剑，符文似乎能与他的灵力相通。陶砚遥提醒他别碰那些符文，但符文似乎在等待他的选择。谢阙川知道自己已经无法回头，也已经无法假装没有看见云海下的上古封印。";

    let issues = contract_character_drift_issues(&manifest, &chapter, content);
    assert!(
        issues.is_empty(),
        "sentence fragments should not be treated as unrecorded characters: {issues:?}"
    );
}

#[test]
fn approved_chapter_prose_metadata_does_not_create_character_authority() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "旧港新舵手".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在旧港招募巴克。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: vec!["新增角色：巴克，负责船队掌舵。".to_string()],
        continuity_updates: vec!["巴克正式加入主角队伍。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });

    let view = contract_term_authority_view(&manifest);

    assert!(!view.character_names.contains("巴克"));
}

#[test]
fn two_character_common_words_do_not_trigger_fuzzy_name_drift() {
    let variants = near_anchor_cjk_name_variants("炉火仍然温热，窗外只剩模糊身影。", "温朔");
    assert!(
        variants.is_empty(),
        "two-character names are too ambiguous for fuzzy prose matching: {variants:?}"
    );
}

#[test]
fn character_drift_ignores_prose_suffix_attached_to_trusted_name() {
    let manifest = test_manifest_with_primary_character();
    let trusted_name = manifest_character_anchors(&manifest)
        .into_iter()
        .next()
        .expect("trusted character");
    let chapter = ChapterRecord {
        number: 1,
        title: "旧案回潮".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: format!("新增角色：{trusted_name}的判断改变了调查方向。"),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = format!("{trusted_name}的判断改变了调查方向，并保留了关键证据。");

    let issues = contract_character_drift_issues(&manifest, &chapter, &content);

    assert!(
        issues
            .iter()
            .all(|issue| !issue.contains("chapter metadata declares unregistered character")),
        "a prose suffix attached to a trusted name must not become a new character: {issues:?}"
    );
}

#[tokio::test]
async fn character_drift_does_not_extract_name_from_cjk_sentence_middle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tool = NovelStudioTool::new(temp.path().to_path_buf(), "writer");
    let init: serde_json::Value = serde_json::from_str(
            &tool
                .call(
                    &serde_json::json!({
                        "action": "init_project",
                        "title": "破局之眼",
                        "language": "zh-CN",
                        "genre": "异界修仙",
                        "brief": "主角踏入古老符文局，追查封印真相。",
                        "target_units": 50_000,
                        "chapter_unit_target": 2_500,
                        "premise": "上古符文局吞噬修士灵力，主角必须找到真相。",
                        "ending_direction": "主角公开封印真相并重写修行规则。",
                        "protagonist_arc": "从被卷入局中的新人变成破局者。",
                        "world_imagery": "云海、金色符文、上古封印、残剑。",
                        "main_causal_spine": "觉醒天赋，触碰残剑，进入符文局，最终破除封印。",
                        "title_rationale": "破局之眼来自主角终局看破封印规则的核心爽点。",
                        "characters": [
                            "name: 谢阙川; role: 主角; desire: 查清封印真相; fear: 被符文吞噬自我; bottom_line: 不以无辜者献祭; arc_start: 新晋修士; arc_end: 破局者",
                            "name: 陶砚遥; role: 关键同伴; desire: 阻止封印失控; fear: 旧局重演; bottom_line: 不把谢阙川当祭品; arc_start: 警告者; arc_end: 共同破局者"
                        ],
                        "world_rules": [
                            "金色符文会抽取靠近者的灵力",
                            "上古封印必须以代价换取真相"
                        ],
                        "outline": "谢阙川在云海中看见符文似乎能回应残剑，但他已经无法回头，只能与陶砚遥一起破局。"
                    })
                    .to_string(),
                )
                .await
                .expect("init output"),
        )
        .expect("init json");
    let project_path = init
        .get("project_path")
        .and_then(|value| value.as_str())
        .expect("project path");
    let manifest = tool
        .read_manifest(std::path::Path::new(project_path))
        .await
        .expect("manifest");
    let chapter = ChapterRecord {
        number: 1,
        title: "符文闪烁".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "谢阙川触碰残剑并看见金色符文。".to_string(),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["符文似乎能回应残剑。".to_string()],
        continuity_updates: vec!["谢阙川已经无法回头。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = [
        "谢阙川握紧残剑，陶砚遥提醒他别碰那些符文。",
        "谢阙川知道自己已经无法回头，也已经无法假装没有看见云海下的上古封印。",
        "谢阙川抬头望向封印深处，确认自己已经无法逃开这场因果。",
        "陶砚遥低声提醒，谢阙川已经无法把这件事当成普通试炼。",
        "谢阙川最后一次回望山门，明白自己已经无法退回旧日身份。",
    ]
    .join("");

    let issues = contract_character_drift_issues(&manifest, &chapter, &content);
    assert!(
        !issues.iter().any(|issue| issue.contains("经无法")),
        "sentence-middle fragments should not be extracted as characters: {issues:?}"
    );
    assert!(
        issues.is_empty(),
        "sentence fragments should not be treated as unrecorded characters: {issues:?}"
    );
}

#[test]
fn draft_json_path_is_not_a_novel_project_path() {
    let draft_path =
        std::path::Path::new("/tmp/benshu/data/generated/novels/drafts/example.json");

    assert!(project_path_looks_like_draft_file(
        "/tmp/benshu/data/generated/novels/drafts/example.json"
    ));
    assert!(project_path_points_to_draft_file(
        "/tmp/benshu/data/generated/novels/drafts/example.json",
        draft_path,
    ));
    assert!(!project_path_looks_like_draft_file(
        "/tmp/benshu/data/generated/novels/example"
    ));
}

#[test]
fn inline_cjk_markup_noise_is_removed_from_saved_prose() {
    let raw = "这种噪音实际上是一种感知过}_过载，它在挑战边界。";

    let cleaned = sanitize_saved_prose(raw);

    assert!(cleaned.contains("感知过载"));
    assert!(!cleaned.contains("}_"));
}

#[test]
fn saved_prose_unwraps_json_string_paragraph_lines() {
    let raw = "谢砚息站在云海边。\n\n\"韩闻隅的玉简忽然发烫，提醒他不要被血祭传承吞没。\",\n\n\"季栖澜拔剑拦路，逼他承认自己已经无法回头。\",";

    let cleaned = sanitize_saved_prose(raw);

    assert!(cleaned.contains("谢砚息站在云海边"));
    assert!(cleaned.contains("韩闻隅的玉简忽然发烫"));
    assert!(cleaned.contains("季栖澜拔剑拦路"));
    assert!(!cleaned.contains("\","));
    assert!(!cleaned.contains("\n\"韩闻隅"));
    assert!(!cleaned.contains("\n\"季栖澜"));
}

#[test]
fn saved_chinese_script_cleanup_preserves_balanced_dialogue_quotes() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    let raw =
        "“顾先生，洛天雄已经到了。”\n\n“我知道。”顾栖川说。\n\n他摸到一枚没有闭合的“影卫令牌。";

    let cleaned = sanitize_chinese_script_noise(&manifest, &sanitize_saved_prose(raw));

    assert!(cleaned.contains("“顾先生，洛天雄已经到了。”"));
    assert!(cleaned.contains("“我知道。”顾栖川说。"));
    assert!(cleaned.contains("影卫令牌"));
    assert!(!cleaned.contains("没有闭合的“影卫"));
}

#[test]
fn saved_prose_collapses_adjacent_repeated_cjk_phrases() {
    let raw = "令牌背面刻着一个小小的‘影’字，字体古朴字体古朴，透着肃杀之气。";

    let cleaned = sanitize_saved_prose(raw);

    assert!(cleaned.contains("字体古朴，透着肃杀之气"));
    assert!(!cleaned.contains("字体古朴字体古朴"));
}

#[test]
fn saved_chinese_script_cleanup_preserves_dialogue_narration_particles() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 沈渡; role: 主角; desire: 寻找失踪的父亲".to_string(),
        "name: 陆清漪; role: 重要角色".to_string(),
        "name: 严廷; role: 关键对手".to_string(),
    ];
    let raw = "沈渡站在天桥的护栏边。\n\n“如果不找，我无法确定他是在这个世界，还是在‘那边’。”沈渡的声音沙哑。\n\n陆清漪察觉到了沈渡眼神中的变化。\n\n随着严廷的身影消失在街道尽头的阴影里，陆清漪转过身，看着沈渡。\n\n沈渡没有说话，他只是看向远方。";

    let cleaned = repair_contract_character_name_typos(
        &manifest,
        &sanitize_chinese_script_noise(&manifest, &sanitize_saved_prose(raw)),
    );

    assert!(cleaned.contains("沈渡站在天桥"));
    assert!(cleaned.contains("沈渡的声音沙哑"));
    assert!(cleaned.contains("陆清漪察觉到了沈渡眼神中的变化"));
    assert!(cleaned.contains("严廷的身影消失"));
    assert!(cleaned.contains("陆清漪转过身"));
    assert!(cleaned.contains("沈渡没有说话"));
    assert!(!cleaned.contains("沈渡天桥"));
    assert!(!cleaned.contains("沈渡音沙哑"));
    assert!(!cleaned.contains("陆清漪到了沈渡"));
    assert!(!cleaned.contains("严廷身影消失"));
    assert!(!cleaned.contains("陆清漪身"));
    assert!(!cleaned.contains("沈渡说话"));
}

#[test]
fn character_name_repair_preserves_legitimate_prose_after_anchors() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 姜知序; role: 主角; desire: 查清龙纹真相".to_string(),
        "name: 晏望棠; role: 关键同伴; desire: 守住城市边界".to_string(),
    ];
    let raw = "暴雨里，姜知序站在写字楼顶层。\n晏望棠的瞳孔猛地收缩。\n姜知序的声音低沉而冷峻：“为什么对我这么感兴趣？”\n晏望棠没有立刻回答。";

    let cleaned = repair_contract_character_name_typos(&manifest, raw);

    assert!(cleaned.contains("姜知序站在写字楼顶层"));
    assert!(cleaned.contains("晏望棠的瞳孔猛地收缩"));
    assert!(cleaned.contains("姜知序的声音低沉"));
    assert!(cleaned.contains("为什么对我这么感兴趣"));
    assert!(cleaned.contains("晏望棠没有立刻回答"));
    assert!(!cleaned.contains("姜知序写字楼"));
    assert!(!cleaned.contains("晏望棠孔"));
    assert!(!cleaned.contains("姜知序音"));
    assert!(!cleaned.contains("为什对我这感兴趣"));
}

#[test]
fn character_name_repair_fixes_adjacent_transposed_contract_anchor() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 姜知序; role: 主角; desire: 查清龙纹真相".to_string(),
        "name: 晏望棠; role: 关键同伴; desire: 守住城市边界".to_string(),
    ];
    let raw = "姜序知的指尖碰到雨门，晏望棠没有立刻回答。";

    let cleaned = repair_contract_character_name_typos(&manifest, raw);

    assert!(cleaned.contains("姜知序的指尖碰到雨门"), "{cleaned}");
    assert!(!cleaned.contains("姜序知"), "{cleaned}");
}

#[test]
fn character_name_repair_preserves_full_anchor_plus_action_verb() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 晏予声; role: 主角; desire: 重回资本战场".to_string(),
        "name: 闻予安; role: 关键对手; desire: 维持控制".to_string(),
    ];
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            id: "character-0001".to_string(),
            name_source: "local_governed".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            canonical_name: "晏予声".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "主角".to_string(),
            desire: "重回资本战场".to_string(),
            fear: "再次被背叛".to_string(),
            bottom_line: "不再交出主动权".to_string(),
            arc_start: "落魄回城".to_string(),
            arc_end: "夺回规则制定权".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            id: "character-0002".to_string(),
            name_source: "local_governed".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            canonical_name: "闻予安".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "关键对手".to_string(),
            desire: "维持控制".to_string(),
            fear: "失去资本地位".to_string(),
            bottom_line: "不允许旧事曝光".to_string(),
            arc_start: "掌控局面".to_string(),
            arc_end: "被反向清算".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];
    let raw =
        "晏予声站在窗前。\n晏予声笑了笑，拿起手机。\n“为什么是我？”晏予声问。\n闻予安看着他。";

    let cleaned = repair_contract_character_name_typos(&manifest, raw);

    assert!(cleaned.contains("晏予声站在窗前"), "{cleaned}");
    assert!(cleaned.contains("晏予声笑了笑"), "{cleaned}");
    assert!(cleaned.contains("晏予声问"), "{cleaned}");
    assert!(cleaned.contains("闻予安看着他"), "{cleaned}");
    assert_eq!(cleaned.matches("晏予声站").count(), 1);
    assert!(!cleaned.contains("晏予声声在窗前"), "{cleaned}");
    assert!(!cleaned.contains("晏予声声了笑"), "{cleaned}");
}

#[test]
fn near_anchor_name_variants_ignore_continuous_common_phrase() {
    let variants = near_anchor_cjk_name_variants("姜栖感知到噪音在窗外扩散。", "姜栖声");

    assert!(
        !variants.contains("姜栖感"),
        "姜栖感知 should be parsed as prose, not a drifted character name"
    );
}

#[test]
fn malformed_anchor_phrase_requires_left_name_boundary() {
    assert_eq!(
        malformed_anchor_phrase("穿过一片枯死的红树林深吸了一口气。", "林深"),
        None,
        "红树林深吸 should be parsed as prose across a normal noun boundary"
    );
    assert_eq!(
        malformed_anchor_phrase("林深吸了一口气。", "林深"),
        Some("林深吸".to_string()),
        "a real anchor-tail malformed phrase should still be reported"
    );
}

#[test]
fn malformed_anchor_phrase_allows_normal_ninan_verb_after_name() {
    assert_eq!(
        malformed_anchor_phrase("商星声呢喃着，她的眼神涣散。", "商星声"),
        None,
        "呢喃 is a normal verb after a character name, not a malformed particle fragment"
    );
}

#[test]
fn structured_character_request_allocates_pending_local_name() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 韩岑安; role: 主角; desire: 在豪门夹缝里活下来".to_string(),
        "name: 梁予禾; role: 关键对手; desire: 维持名媛身份".to_string(),
    ];
    ensure_character_authority_ledger(&mut manifest);
    let registrations = register_chapter_character_requests(
        &mut manifest,
        1,
        &[ChapterCharacterRequest {
            request_id: "su-family-steward".to_string(),
            role: "苏家管家".to_string(),
            importance: "chapter_temporary".to_string(),
            narrative_purpose: "引导主角进入宴会并传递规矩".to_string(),
            planned_entry: "第1章".to_string(),
            planned_exit: "第1章结束".to_string(),
            relationship_to_existing: "暂时服从梁予禾".to_string(),
            ..ChapterCharacterRequest::default()
        }],
    );

    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].request_id, "su-family-steward");
    assert!(
        naming::audit_character_name_candidate(&registrations[0].canonical_name, "zh-CN").accepted
    );
    assert_ne!(registrations[0].canonical_name, "韩岑安");
    assert_ne!(registrations[0].canonical_name, "梁予禾");
    let pending = manifest
        .character_ledger
        .iter()
        .find(|record| record.id == registrations[0].character_id)
        .expect("pending registration");
    assert_eq!(pending.status, "pending:chapter-1");
    assert_eq!(pending.name_source, "local_character_allocator");
    assert!(pending.aliases.is_empty());
}

#[test]
fn recurring_character_request_requires_long_term_identity_anchors() {
    let mut manifest = test_manifest_with_primary_character();
    ensure_character_authority_ledger(&mut manifest);

    let registrations = register_chapter_character_requests(
        &mut manifest,
        2,
        &[ChapterCharacterRequest {
            request_id: "academy-rival".to_string(),
            role: "长期竞争者".to_string(),
            importance: "volume_recurring".to_string(),
            narrative_purpose: "在本卷持续挑战主角选择".to_string(),
            planned_entry: "第2章".to_string(),
            relationship_to_existing: "与黎启洄竞争".to_string(),
            ..ChapterCharacterRequest::default()
        }],
    );

    assert!(registrations.is_empty());
    assert!(manifest.character_ledger.iter().all(|record| !record
        .identity_markers
        .iter()
        .any(|marker| marker == "request_id:academy-rival")));
}

#[test]
fn approved_recurring_character_updates_existing_relationship_and_voice_ledgers() {
    let mut manifest = test_manifest_with_primary_character();
    ensure_character_authority_ledger(&mut manifest);
    let registrations = register_chapter_character_requests(
        &mut manifest,
        2,
        &[ChapterCharacterRequest {
            request_id: "academy-rival".to_string(),
            role: "长期竞争者".to_string(),
            importance: "volume_recurring".to_string(),
            narrative_purpose: "在本卷持续挑战主角选择".to_string(),
            planned_entry: "第2章".to_string(),
            planned_exit: "本卷结束".to_string(),
            relationship_to_existing: "与黎启洄从竞争走向有限合作".to_string(),
            desire: "证明自己的判断比家族安排更可靠".to_string(),
            fear: "再次因错误选择失去同伴".to_string(),
            bottom_line: "不以无辜者作为交换筹码".to_string(),
            arc_start: "把黎启洄视为必须击败的对手".to_string(),
            arc_end: "承认共同目标并保留原则分歧".to_string(),
            voice_style: "克制直接，习惯先质疑证据".to_string(),
        }],
    );
    let registered_name = registrations[0].canonical_name.clone();

    promote_chapter_character_registrations(&mut manifest, 2);

    let character = manifest
        .character_ledger
        .iter()
        .find(|record| record.canonical_name == registered_name)
        .expect("promoted recurring character");
    assert_eq!(character.status, "active");
    assert_eq!(character.desire, "证明自己的判断比家族安排更可靠");
    assert!(manifest
        .structured_contract_v2
        .character_voice_ledger
        .iter()
        .any(|profile| profile.character == registered_name));
    assert!(manifest
        .structured_contract_v2
        .relationship_ledger
        .iter()
        .any(|relationship| {
            relationship.characters.iter().any(|name| name == "黎启洄")
                && relationship
                    .characters
                    .iter()
                    .any(|name| name == &registered_name)
        }));
}

#[test]
fn approved_chapter_promotes_only_its_pending_character_records() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            id: "character-chapter-a".to_string(),
            canonical_name: "钟照宁".to_string(),
            name_source: "generated_by_writing_tool_policy".to_string(),
            planned_entry: "第2章".to_string(),
            planned_exit: String::new(),
            aliases: vec!["临时名甲".to_string()],
            identity_markers: Vec::new(),
            role: "章节关键角色".to_string(),
            desire: String::new(),
            fear: String::new(),
            bottom_line: String::new(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "pending:chapter-2".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            id: "character-chapter-b".to_string(),
            canonical_name: "闻砚安".to_string(),
            name_source: "generated_by_writing_tool_policy".to_string(),
            planned_entry: "第3章".to_string(),
            planned_exit: String::new(),
            aliases: vec!["临时名乙".to_string()],
            identity_markers: Vec::new(),
            role: "章节关键角色".to_string(),
            desire: String::new(),
            fear: String::new(),
            bottom_line: String::new(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "pending:chapter-3".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];

    promote_chapter_character_registrations(&mut manifest, 2);

    assert_eq!(manifest.character_ledger[0].status, "active");
    assert_eq!(manifest.character_ledger[1].status, "pending:chapter-3");
}

#[test]
fn rejected_chapter_discards_only_its_pending_character_records() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            id: "character-chapter-a".to_string(),
            canonical_name: "钟照宁".to_string(),
            name_source: "generated_by_writing_tool_policy".to_string(),
            planned_entry: "第2章".to_string(),
            planned_exit: String::new(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "章节关键角色".to_string(),
            desire: String::new(),
            fear: String::new(),
            bottom_line: String::new(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "pending:chapter-2".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            id: "character-chapter-b".to_string(),
            canonical_name: "闻砚安".to_string(),
            name_source: "generated_by_writing_tool_policy".to_string(),
            planned_entry: "第3章".to_string(),
            planned_exit: String::new(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "章节关键角色".to_string(),
            desire: String::new(),
            fear: String::new(),
            bottom_line: String::new(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "pending:chapter-3".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];

    discard_chapter_character_registrations(&mut manifest, 2);

    assert_eq!(manifest.character_ledger.len(), 1);
    assert_eq!(manifest.character_ledger[0].id, "character-chapter-b");
    assert_eq!(manifest.character_ledger[0].status, "pending:chapter-3");
}

#[test]
fn project_action_rejects_draft_json_as_project_path_recoverably() {
    let args: NovelStudioArgs = serde_json::from_value(serde_json::json!({
        "action": "run_project",
        "project_path": "/tmp/benshu/data/generated/novels/drafts/example.json"
    }))
    .expect("args");

    let result = invalid_draft_path_as_project_path_result(&args).expect("recoverable result");

    assert_eq!(
        result.get("error_kind").and_then(|value| value.as_str()),
        Some("project_path_is_creation_draft_file")
    );
    assert_eq!(
        result.get("draft_path").and_then(|value| value.as_str()),
        Some("/tmp/benshu/data/generated/novels/drafts/example.json")
    );
}

#[test]
fn settlement_display_wrong_actor_is_advisory_not_durable_state() {
    let body = "黎启洄在脉冲核心过载时保持清醒，靠手动校准把频率重新压回安全区。";
    let settlement = SettlementOutput {
        chapter_fingerprint: String::new(),
        body_fingerprint: String::new(),
        authority_fingerprint: String::new(),
        state_changes: Vec::new(),
        degraded_reason: String::new(),
        current_state: "林墨通过手动过载成功锁定了频率。".to_string(),
        pending_hooks: String::new(),
        chapter_summary: "黎启洄完成一次危险校准。".to_string(),
        continuity_updates: vec!["林墨通过手动过载成功锁定了频率。".to_string()],
        resolved_hooks: Vec::new(),
    };

    let validation = deterministic_state_validation(body, &settlement);

    assert!(validation.passed);
    assert!(validation
        .advisories
        .iter()
        .any(|warning| warning.contains("林墨")));
}

#[test]
fn settlement_display_script_residue_is_advisory() {
    let body = "黎启洄把核心频率稳定在安全阈值附近，所有人都听见了低沉的回响。";
    let settlement = SettlementOutput {
        chapter_fingerprint: String::new(),
        body_fingerprint: String::new(),
        authority_fingerprint: String::new(),
        state_changes: Vec::new(),
        degraded_reason: String::new(),
        current_state: "核心频率稳定在44.나122 kHz。".to_string(),
        pending_hooks: String::new(),
        chapter_summary: "黎启洄完成一次危险校准。".to_string(),
        continuity_updates: Vec::new(),
        resolved_hooks: Vec::new(),
    };

    let validation = deterministic_state_validation(body, &settlement);

    assert!(validation.passed);
    assert!(!validation.advisories.is_empty());
}

#[test]
fn nonempty_final_body_degrades_empty_display_state_without_polluting_truth() {
    let settlement = SettlementOutput {
        chapter_fingerprint: String::new(),
        body_fingerprint: String::new(),
        authority_fingerprint: String::new(),
        state_changes: Vec::new(),
        degraded_reason: String::new(),
        current_state: String::new(),
        pending_hooks: String::new(),
        chapter_summary: String::new(),
        continuity_updates: Vec::new(),
        resolved_hooks: Vec::new(),
    };

    let validation = deterministic_state_validation("沈砚离开旧城。", &settlement);

    assert!(validation.passed);
    assert_eq!(
        validation.disposition,
        StateSettlementDisposition::DisplayMetadataDegraded
    );
    assert!(validation
        .warnings
        .iter()
        .any(|warning| warning.contains("current_state")));
    assert!(validation
        .warnings
        .iter()
        .any(|warning| warning.contains("chapter_summary")));
}

#[test]
fn settlement_parser_accepts_fenced_json_without_falling_back() {
    let raw = r#"```json
{
  "current_state": "沈砚取得试炼资格。",
  "pending_hooks": "导师隐瞒了试炼真相。",
  "chapter_summary": "沈砚承担代价并通过试炼。",
  "continuity_updates": ["沈砚取得试炼资格。"],
  "resolved_hooks": ["入门资格"]
}
```"#;

    let settlement = parse_settlement_output(raw, "不会用于回退的正文");

    assert_eq!(settlement.current_state, "沈砚取得试炼资格。");
    assert_eq!(settlement.resolved_hooks, ["入门资格"]);
}

#[test]
fn settlement_display_copied_prose_fragment_is_advisory() {
    let body = "白知白坐在甲板角落的木箱上，左臂的袖管空荡荡地垂着。那是三天前在一次锅炉爆炸事故中失去的左臂，也是他作为黑铁号首席机械师唯一值得炫耀的遗产——虽然这只义肢粗糙得像是铁匠铺里随手打废的半成品。艾拉向白知白提出合作，帮助黑铁号穿过回声兽群。";
    let settlement = SettlementOutput {
            chapter_fingerprint: String::new(),
            body_fingerprint: String::new(),
            authority_fingerprint: String::new(),
            state_changes: Vec::new(),
            degraded_reason: String::new(),
            current_state: "那是三天前在一次锅炉爆炸事故中失去的左臂，也是他作为黑铁号首席机械师唯一值得炫耀的遗产——虽然这只义肢粗糙得像是铁匠铺里随手打废的半成品。".to_string(),
            pending_hooks: String::new(),
            chapter_summary: "那是三天前在一次锅炉爆炸事故中失去的左臂，也是他作为黑铁号首席机械师唯一值得炫耀的遗产——虽然这只义肢粗糙得像是铁匠铺里随手打废的半成品。".to_string(),
            continuity_updates: vec!["艾拉向白知白提出合作，帮助黑铁号穿过回声兽群。".to_string()],
            resolved_hooks: Vec::new(),
        };

    let validation = deterministic_state_validation(body, &settlement);

    assert!(validation.passed);
    assert!(validation
        .advisories
        .iter()
        .any(|warning| warning.contains("copied prose fragment")));
}

#[test]
fn contract_character_anchor_extracts_labeled_names() {
    let contract = StoryContract {
        premise: "测试".to_string(),
        themes: Vec::new(),
        characters: vec![
            "name: 陆沉; identity: 落魄继承人".to_string(),
            "name: 苏清月; identity: 圣女".to_string(),
        ],
        world_rules: Vec::new(),
        style_rules: Vec::new(),
        must_avoid: Vec::new(),
        outline: String::new(),
        structured_contract_v2: NovelContractV2::default(),
        authority_contract: None,
        updated_at: String::new(),
    };

    assert_eq!(
        contract_character_anchors(&contract),
        vec!["苏清月".to_string(), "陆沉".to_string()]
    );
    assert!(stable_contract_anchor_present(
        &contract,
        "陆沉在裂缝下收起玉简。"
    ));
}

#[test]
fn contract_character_anchor_extracts_chinese_role_name_labels() {
    let contract = StoryContract {
            premise: "测试".to_string(),
            themes: Vec::new(),
            characters: vec![
                "主角姓名：陆离，命名依据：离散记忆，欲望：找回自我，恐惧：彻底遗忘，底线：不伤害无辜者。".to_string(),
                "关键配角姓名：苏清，命名依据：清醒观测者。".to_string(),
            ],
            world_rules: Vec::new(),
            style_rules: Vec::new(),
            must_avoid: Vec::new(),
            outline: String::new(),
            structured_contract_v2: NovelContractV2::default(),
            authority_contract: None,
            updated_at: String::new(),
        };

    assert_eq!(
        contract_character_anchors(&contract),
        vec!["苏清".to_string(), "陆离".to_string()]
    );
    assert!(stable_contract_anchor_present(
        &contract,
        "陆离在低频区寻找苏清留下的锚点。"
    ));
}

fn test_manifest_with_primary_character() -> NovelProjectManifest {
    NovelProjectManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        title: "测试小说".to_string(),
        title_state: TitleState::default(),
        language: "zh-cn".to_string(),
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
        delivery_advisory_windows: Vec::new(),
        truth_files: Vec::new(),
        archives: Vec::new(),
        contract: Some(StoryContract {
            premise: "草根逆袭。".to_string(),
            themes: vec!["成长".to_string()],
            characters: vec![
                "name: 黎启洄; role: 主角; desire: 改变命运".to_string(),
                "name: 沈青萝; role: 同伴".to_string(),
            ],
            world_rules: Vec::new(),
            style_rules: Vec::new(),
            must_avoid: Vec::new(),
            outline: "黎启洄从底层学院开始逐步逆袭。".to_string(),
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
    }
}

#[test]
fn writing_governance_report_exposes_ten_axes() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
                "name: 黎启洄; role: 主角; desire: 改变命运; fear: 再次被学院淘汰; bottom_line: 不牺牲同伴"
                    .to_string(),
            ];
        contract.world_rules =
            vec!["学院晋级必须通过公开考试，越级突破会留下灵脉裂纹。".to_string()];
        contract.outline = "第一卷《裂纹入学》（第1-5章）：主角进入学院并通过第一次晋级考试。\n终局：主角以公开考试击败压迫者，完成草根逆袭。".to_string();
    }
    ensure_story_bible_from_manifest(&mut manifest);
    ensure_volume_records_from_story_bible(&mut manifest);

    let report = writing_governance_report(&manifest);
    let axes = report["axes"].as_array().expect("axes");

    assert_eq!(axes.len(), 10);
    assert!(axes.iter().any(|axis| axis["id"] == "story_contract"));
    assert!(axes.iter().any(|axis| axis["id"] == "volume_graph"));
    assert!(axes
        .iter()
        .any(|axis| axis["id"] == "naming_governance"));
}

#[test]
fn project_governance_migrates_title_volume_character_and_chapter_authority() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.title = "星门逆旅".to_string();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
                "name: 沈砚; role: 主角; desire: 通过学院考试; fear: 被旧阶层重新吞没; bottom_line: 不牺牲同伴"
                    .to_string(),
                "name: 许照白; role: 反派; desire: 垄断星门名额; fear: 失去血统特权; bottom_line: 只承认胜利"
                    .to_string(),
            ];
        contract.world_rules = vec!["星门晋级必须公开考试，越级会留下灵核裂纹。".to_string()];
        contract.outline = "第一卷《寒鸦鸣》（第1-3章）：沈砚进入学院底层考试并夺回第一枚星门令牌。\n终局：沈砚公开击败许照白，完成草根逆袭。".to_string();
    }
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "第一章".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "沈砚在公开考试中夺回第一枚星门令牌。".to_string(),
        unit_count: 2600,
        status: "approved".to_string(),
        key_facts: vec!["沈砚夺回星门令牌。".to_string()],
        continuity_updates: vec!["伏笔：星门令牌出现裂纹。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });

    ensure_project_governance(&mut manifest);

    assert_eq!(manifest.title_state.canonical_title, "星门逆旅");
    assert!(manifest.title_state.locked);
    assert!(!manifest.volumes.is_empty());
    assert_eq!(manifest.chapters[0].volume_id, "volume-0001");
    assert_eq!(manifest.chapters[0].volume_title, "寒鸦鸣");
    assert!(manifest
        .character_ledger
        .iter()
        .any(|character| character.canonical_name == "沈砚"));
    assert!(manifest.character_ledger.iter().all(|character| {
        !character.id.trim().is_empty() && !character.name_source.trim().is_empty()
    }));
    assert!(manifest
        .contract
        .as_ref()
        .expect("contract")
        .characters
        .iter()
        .all(|line| line.contains("character_id:") && line.contains("name_source:")));
    assert!(manifest
        .volume_summaries
        .iter()
        .any(|summary| summary.volume_id == "volume-0001"));
}

#[test]
fn post_body_title_repair_defers_invalid_title_to_metadata_gate() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.outline =
            "第一卷《星门试炼》（第1-3章）：沈砚进入学院考试。\n终局：沈砚公开击败垄断者。"
                .to_string();
    }
    ensure_project_governance(&mut manifest);
    let title = final_chapter_title_from_body(
            &manifest,
            1,
            "第1章",
            "沈砚在寒雨考场夺回星门令牌，并发现令牌裂纹会吞噬记忆。",
            "沈砚在寒雨考场夺回星门令牌，并发现令牌裂纹会吞噬记忆。他没有把同伴推出去抵债，而是在众目睽睽下改写了考场规则。",
        );

    assert_eq!(normalized_title_key(&title), normalized_title_key("第1章"));
    let chapter = ChapterRecord {
        number: 1,
        title,
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "沈砚夺回星门令牌，并在众目睽睽下改写考场规则。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["沈砚改写了考场规则。".to_string()],
        continuity_updates: vec!["星门令牌裂纹开始吞噬记忆。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let gate = chapter_metadata_gate(
        &manifest,
        &chapter,
        "沈砚在寒雨考场夺回星门令牌，并在众目睽睽下改写考场规则。",
    );
    assert!(
        gate.blocking.is_empty(),
        "body anchored title should not be blocked: {:?}",
        gate.blocking
    );
    assert!(gate.needs_repair(), "{gate:?}");
}

#[test]
fn post_body_title_repair_rejects_clipped_comparative_prose_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.outline =
                "第一卷《暗涌入局》（第1-3章）：主角在都市权力竞聘中觉醒能力。\n终局：主角公开击败垄断者。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary =
        "闻予晚在内部竞聘中获得晋升机会，随后觉醒需要付出生命能量的特殊能力，并被洛闻序盯上。";
    let content = "暗涌之城的夜色总是浓得化不开，仿佛连星光都被这座城市吞噬。然而，对于闻予晚来说，今晚的黑暗却带来了前所未有的转机。闻予晚抓住内部竞聘机会，获得晋升名额，却在竞聘结束后觉醒了需要付出生命能量的能力。洛闻序发现了他的异常，警告他这份力量会把他推向毁灭。";
    let title = final_chapter_title_from_body(&manifest, 1, "第1章", summary, content);

    assert_ne!(title, "仿佛连星");
    let chapter = ChapterRecord {
        number: 1,
        title: "仿佛连星".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: summary.to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["闻予晚觉醒特殊能力。".to_string()],
        continuity_updates: vec!["洛闻序开始关注闻予晚。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let gate = chapter_metadata_gate(&manifest, &chapter, content);
    assert!(
        gate.needs_repair(),
        "clipped prose fragment should require metadata repair"
    );
}

#[test]
fn post_body_title_preserves_supported_setting_phrase() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 许岑安; role: 主角; desire: 拆开地下市场的供应链垄断".to_string(),
            "name: 唐予声; role: 对手; desire: 维持建材供应链垄断".to_string(),
        ];
        contract.outline =
                "第一卷《地下入局》（第1-3章）：许岑安从地下市场发现建材供应链垄断。\n终局：许岑安公开账册并改写城市商业规则。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "许岑安在地下市场发现唐予声集团对建材供应链的垄断，并抓住水泥厂偷工减料的证据。";
    let content = "许岑安站在钢筋水泥交织的地下市场入口。这座城市从不缺灯火，也不缺吞人的账本。陈老板把水泥厂偷工减料的单据压在桌角，建材供应链垄断的第一道裂缝终于露出来。许岑安没有退让，他把商业计划书推到众人面前，决定从水泥厂账册切开唐予声的封锁。";
    let title = final_chapter_title_from_body(&manifest, 1, "这座城市", summary, content);

    assert_eq!(title, "这座城市");
}

#[test]
fn post_body_title_repair_rejects_character_action_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.outline =
                "第一卷《星门试炼》（第1-3章）：黎启洄在寒雨考场夺回星门令牌。\n终局：黎启洄公开击败垄断者。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "黎启洄在寒雨考场夺回星门令牌，并发现令牌裂纹会吞噬记忆。";
    let content = "黎启洄站在寒雨考场边缘，雨水顺着破旧校服落下。他夺回星门令牌的那一刻，考场规则第一次被底层学生公开改写。星门令牌裂出细纹，吞噬记忆的代价也随之浮现。";
    let title = final_chapter_title_from_body(&manifest, 1, "黎启洄站", summary, content);

    assert_eq!(title, "第1章");
}

#[test]
fn post_body_title_repair_rejects_character_tail_action_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 晏庭川; role: 主角; desire: 夺回被白家控制的命运".to_string(),
            "name: 沈棠遥; role: 同伴; desire: 追查天罡诀真相".to_string(),
        ];
        contract.outline =
                "第一卷《雨夜天罡》（第1-3章）：晏庭川在雨夜工厂觉醒天罡诀，并面对白家追猎。\n终局：晏庭川公开击败白家，夺回城市灵能秩序。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "晏庭川在雨夜工厂觉醒天罡诀，并与沈棠遥确认白家追猎将至。";
    let content = "晏庭川站在废弃工厂的屋顶，雨水顺着他的黑色风衣滑落。沈棠遥说出天罡诀的名字后，白家的警笛声逼近，他决定不再逃避。";
    let title = final_chapter_title_from_body(&manifest, 1, "庭川站", summary, content);

    assert_ne!(title, "庭川站");
}

#[test]
fn post_body_title_repair_rejects_incomplete_prose_edge_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 晏庭川; role: 主角; desire: 夺回被白家控制的命运".to_string(),
            "name: 沈棠遥; role: 同伴; desire: 追查天罡诀真相".to_string(),
        ];
        contract.outline =
                "第一卷《雨夜天罡》（第1-3章）：晏庭川在雨夜工厂觉醒天罡诀，并面对白家追猎。\n终局：晏庭川公开击败白家，夺回城市灵能秩序。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "晏庭川在雨夜工厂觉醒天罡诀，并与沈棠遥确认白家追猎将至。";
    let content = "晏庭川站在废弃工厂的屋顶，雨水顺着他的黑色风衣滑落。沈棠遥说出天罡诀的名字后，白家的警笛声逼近，他决定不再逃避。";
    let title = final_chapter_title_from_body(&manifest, 1, "雨水顺着", summary, content);

    assert_ne!(title, "雨水顺着");
}

#[test]
fn post_body_title_repair_rejects_incomplete_relation_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 晏庭川; role: 主角; desire: 夺回被白家控制的命运".to_string(),
            "name: 沈棠遥; role: 同伴; desire: 追查天罡诀真相".to_string(),
        ];
        contract.outline =
                "第一卷《雨夜天罡》（第1-3章）：晏庭川在雨夜工厂觉醒天罡诀，并面对白家追猎。\n终局：晏庭川公开击败白家，夺回城市灵能秩序。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "晏庭川在雨夜工厂觉醒天罡诀，并与沈棠遥确认白家追猎将至。";
    let content = "晏庭川站在废弃工厂的屋顶，雨水顺着他的黑色风衣滑落。沈棠遥说出天罡诀的名字后，白家的警笛声逼近，他决定不再逃避。";
    let title = final_chapter_title_from_body(&manifest, 1, "雨水顺", summary, content);

    assert_ne!(title, "雨水顺");
}

#[test]
fn post_body_title_repair_rejects_clipped_comparison_tail_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 岑曜舟; role: 主角; desire: 突破天地桎梏".to_string(),
            "name: 辛曜白; role: 导师; desire: 守护修行传承".to_string(),
        ];
        contract.outline =
                "第一卷《破界卷》（第1-3章）：岑曜舟觉醒灵脉并引来天机阁。\n终局：岑曜舟建立新的修行秩序。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "岑曜舟在秘境觉醒灵脉，天机阁执法使现身，神秘黑影袭来并暴露黑焰之力。";
    let content = "岑曜舟在秘境觉醒灵脉，天机阁执法使现身，神秘黑影袭来。那黑影似乎并未被击退，反而让黑焰之力与灵脉产生共鸣。岑曜舟意识到这不是普通袭击，而是旧修行秩序对觉醒者的第一次追猎。";
    let title = final_chapter_title_from_body(&manifest, 1, "那黑影似", summary, content);

    assert_ne!(title, "那黑影似");
    assert!(
        !cjk_title_candidate_has_sentence_fragment_edge(&title),
        "{title}"
    );
}

#[test]
fn post_body_title_repair_rejects_aspect_particle_prose_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 辛曜白; role: 主角; desire: 追索剑血本源".to_string(),
            "name: 梁衡遥; role: 导师; desire: 守住剑道传承".to_string(),
        ];
        contract.outline =
            "第一卷《苍穹之始》：辛曜白追索剑血本源并解读古碑。\n终局：辛曜白以剑血斩开九天桎梏。"
                .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "辛曜白触碰古碑，发现剑血本源与上古剑道宗师有关。";
    let content = "辛曜白站在九重天域中心，周身环绕着淡淡剑气。他触碰古碑后，碑文回应剑血，显露上古剑道宗师留下的本源线索。";
    let title = final_chapter_title_from_body(&manifest, 6, "环绕着淡", summary, content);

    assert_ne!(title, "环绕着淡");
    assert!(
        !cjk_title_core_has_prose_grammar_fragment(&title),
        "{title}"
    );
}

#[test]
fn post_body_title_repair_rejects_comparative_quantity_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 顾庭白; role: 主角; desire: 摆脱负债".to_string(),
            "name: 辛知序; role: 同盟; desire: 寻找商业新星".to_string(),
        ];
        contract.outline =
                "第一卷《代价追索》：顾庭白在暴雨夜觉醒趋势洞察，并接到辛知序的邀约。\n终局：顾庭白建立稳固事业后不再依赖异能。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "顾庭白在暴雨夜确认债务压力，觉醒趋势洞察能力，并收到辛知序留下的名片邀约。";
    let content = "暴雨像无数条鞭子，抽打着江城老城区的屋檐。顾庭白推开生锈铁门，回到城中村出租屋，面对三百二十万债务与南予川的催逼。辛知序深夜来访，留下明早十点见面的名片，顾庭白决定带着答案赴约。";
    let title = final_chapter_title_from_body(&manifest, 1, "暴雨像无数", summary, content);

    assert_ne!(title, "暴雨像无数");
    assert!(
        !cjk_title_core_has_prose_grammar_fragment(&title),
        "{title}"
    );
}

#[test]
fn post_body_title_repair_rejects_adverbial_predicate_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 顾棠澜; role: 主角; desire: 查清失踪案".to_string(),
            "name: 段栖安; role: 同伴; desire: 保护证据链".to_string(),
        ];
        contract.outline =
                "第一卷《盲区证词》：顾棠澜在雨夜电梯盲区发现失踪案证据。\n终局：顾棠澜用盲区证据锁定真凶。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary =
        "顾棠澜在雨夜案发楼栋发现电梯盲区和碎屏手机，将失踪案与旧搭档留下的暗门线索连在一起。";
    let content = "雨水像无数根冰冷的钢针，密集地扎进这座城市的肌理中。顾棠澜走进案发楼栋，发现电梯监控存在固定盲区，血迹旁只剩一部碎屏手机。段栖安确认盲区并非设备故障，而是有人提前改过线路。";
    let title = final_chapter_title_from_body(&manifest, 1, "密集地扎", summary, content);

    assert_eq!(title, "第1章");
}

#[test]
fn post_body_title_repair_rejects_object_body_state_fragment() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 钟望宁; role: 主角; desire: 找到古剑真相".to_string(),
            "name: 许砚安; role: 同伴; desire: 守住门派遗物".to_string(),
        ];
        contract.outline =
                "第一卷《古剑入局》：钟望宁发现古剑会牵出旧宗门的灭门真相。\n终局：钟望宁用古剑证据击破旧秩序。"
                    .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "钟望宁发现古剑泛出幽蓝光芒，并意识到旧宗门灭门证据仍藏在剑纹里。";
    let content = "钟望宁握着那把古剑，剑身泛着幽蓝光芒。许砚安认出剑纹来自旧宗门，提醒他这不是普通法器，而是灭门真相留下的证物。";
    let title = final_chapter_title_from_body(&manifest, 1, "第1章", summary, content);

    assert_ne!(title, "古剑身泛");
    assert!(
        !cjk_title_core_has_prose_grammar_fragment(&title),
        "{title}"
    );
}

#[test]
fn post_body_title_repair_rejects_predicate_fragment_cut_from_prose() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 裴岚野; role: 主角; desire: 悟剑道本源".to_string(),
            "name: 秦珩隅; role: 导师; desire: 寻得剑道真谛".to_string(),
        ];
        contract.world_rules = vec!["九重天阙试炼会暴露异常规则。".to_string()];
        contract.outline =
            "第一卷《入局见证》：裴岚野在九重天阙发现异常规则。\n终局：裴岚野斩破世界桎梏。"
                .to_string();
    }
    ensure_project_governance(&mut manifest);
    let summary = "裴岚野在九重天阙发现第一条异常规则，确认这场试炼和他所知的剑道完全不同。";
    let content = "裴岚野踏入九重天阙的剑塔，第一条异常规则在石壁上亮起。他意识到这场试炼和他所知的剑道完全不同，也第一次看见剑塔深处的黑色门环。秦珩隅没有解释，只让他记住门环上的裂纹。";
    let title = final_chapter_title_from_body(&manifest, 1, "第1章", summary, content);

    assert_eq!(title, "第1章");
}

#[test]
fn chapter_metadata_summary_prefers_supported_facts_over_body_opening() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 岑曜舟; role: 主角; desire: 突破天地桎梏".to_string(),
            "name: 辛曜白; role: 导师; desire: 守护修行传承".to_string(),
        ];
    }
    ensure_project_governance(&mut manifest);
    let content = "残阳如血，将天际染成一片猩红。岑曜舟跪在青石台阶上，掌心贴着地面，感受着脚下大地传来的震颤。岑曜舟体内灵脉觉醒，天机阁执法使现身，神秘黑影以黑焰之力试探他的传承。";
    let mut chapter = ChapterRecord {
            number: 1,
            title: "第1章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "残阳如血，将天际染成一片猩红。岑曜舟跪在青石台阶上，掌心贴着地面，感受着脚下大地传来的震颤。".to_string(),
            unit_count: 2500,
            status: "draft".to_string(),
            key_facts: vec![
                "岑曜舟体内灵脉觉醒。".to_string(),
                "天机阁执法使现身。".to_string(),
            ],
            continuity_updates: vec!["神秘黑影以黑焰之力试探岑曜舟的传承。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };

    normalize_chapter_metadata_against_body(&manifest, &mut chapter, content);

    assert!(
        !chapter.summary.starts_with("残阳如血"),
        "{}",
        chapter.summary
    );
    assert!(
        chapter.summary.contains("灵脉觉醒") || chapter.summary.contains("天机阁"),
        "{}",
        chapter.summary
    );
}

#[test]
fn chapter_metadata_rebuilds_identity_conflicting_fields_from_final_body() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 商予真; role: 主角; desire: 延续寿元".to_string(),
            "name: 阮承澜; role: 同伴; desire: 找到矿脉".to_string(),
        ];
    }
    ensure_project_governance(&mut manifest);
    manifest
        .character_ledger
        .iter_mut()
        .find(|character| character.canonical_name == "商予真")
        .expect("primary character ledger")
        .identity_markers = vec!["inferred_pronoun_profile:feminine".to_string()];
    let body = "商予真发现乱石后的矿脉入口，她带着阮承澜进入矿道。商予真主动引导青灯释放灵气，她也因此承受了寿元流失的代价。";
    let mut chapter = ChapterRecord {
        number: 4,
        title: "矿脉入口".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: "商予真发现了矿脉入口，他的视线越过乱石。".to_string(),
        unit_count: 2500,
        status: "state_ready".to_string(),
        key_facts: vec![
            "商予真发现乱石后的矿脉入口，他带着阮承澜进入矿道。".to_string(),
        ],
        continuity_updates: vec![
            "商予真主动引导青灯释放灵气，他也因此承受了寿元流失的代价。".to_string(),
        ],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    normalize_chapter_metadata_against_body(&manifest, &mut chapter, body);

    let governed = format!(
        "{}\n{}\n{}",
        chapter.summary,
        chapter.key_facts.join("\n"),
        chapter.continuity_updates.join("\n")
    );
    assert!(!governed.contains("他的"), "{governed}");
    assert!(!chapter.key_facts.is_empty(), "{chapter:?}");
    assert!(!chapter.continuity_updates.is_empty(), "{chapter:?}");
    assert!(governed.contains("商予真"), "{governed}");
}

#[test]
fn chapter_metadata_summary_rejects_prior_chapter_fact_reuse() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 辛曜白; role: 主角; desire: 追索剑血本源".to_string(),
            "name: 梁衡遥; role: 导师; desire: 守住剑道传承".to_string(),
        ];
    }
    ensure_project_governance(&mut manifest);
    let content = "辛曜白站在九重天域中心，周身环绕着淡淡剑气。辛曜白触碰古碑，发现剑血本源线索。碑文回应剑血，显露上古剑道宗师留下的传承因果。梁衡遥提醒辛曜白必须稳住剑心。";
    let mut chapter = ChapterRecord {
            number: 6,
            title: "第6章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0006.md".to_string(),
            summary: "辛曜白在九重天域中心遭遇强大妖兽，成功斩杀并获得第一滴剑血。；剑血初凝引发天域异动，天域守护者现身。".to_string(),
            unit_count: 2500,
            status: "draft".to_string(),
            key_facts: vec!["辛曜白触碰古碑，发现剑血本源线索。".to_string()],
            continuity_updates: vec!["梁衡遥提醒辛曜白必须稳住剑心。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };

    assert!(!chapter_summary_supported_by_content(
        &chapter.summary,
        content,
        &manifest.language
    ));

    normalize_chapter_metadata_against_body(&manifest, &mut chapter, content);

    assert!(
        !chapter.summary.contains("妖兽") && !chapter.summary.contains("守护者"),
        "{}",
        chapter.summary
    );
    assert!(chapter_summary_supported_by_content(
        &chapter.summary,
        content,
        &manifest.language
    ));
}

#[test]
fn chapter_metadata_does_not_rebuild_summary_from_loosely_supported_stale_facts() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 陆栖宁; role: 主角; desire: 修复灰阶公寓".to_string(),
            "name: 梁栖言; role: 对手; desire: 提高旧城项目效率".to_string(),
        ];
    }
    ensure_project_governance(&mut manifest);
    let content = "陆栖宁在灰阶公寓里向梁栖言展示保留旧墙的修复方案。梁栖言坚持商业效率，要求拆除东翼。陆栖宁拒绝退让，并与梁栖言约定用一个月的租金回报证明方案价值。";
    let stale_summary = "陆栖宁调整呼吸后进入屋内；梁栖言的气息像一道墙隔绝了她熟悉的天地";
    let mut chapter = ChapterRecord {
        number: 2,
        title: "侵入者".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: stale_summary.to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec![
            "陆栖宁调整呼吸后进入屋内".to_string(),
            "梁栖言的气息像一道墙隔绝了陆栖宁熟悉的天地".to_string(),
        ],
        continuity_updates: vec!["陆栖宁与梁栖言之间形成无形隔墙".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    assert!(!chapter_summary_supported_by_content(
        stale_summary,
        content,
        &manifest.language
    ));

    normalize_chapter_metadata_against_body(&manifest, &mut chapter, content);

    assert_ne!(chapter.summary, stale_summary);
    assert!(chapter_summary_supported_by_content(
        &chapter.summary,
        content,
        &manifest.language
    ));
}

#[test]
fn chapter_metadata_title_uses_validated_fact_evidence() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 段知遥; role: 主角; desire: 守住球队".to_string(),
            "name: 韩澈川; role: 教练; desire: 完成训练模型".to_string(),
        ];
    }
    ensure_project_governance(&mut manifest);
    let body = "段知遥在训练后对着空旷球场呐喊。韩澈川认可她身上无法被数据模型模拟的特质，队友随后邀请她去吃烧烤。";
    let mut chapter = ChapterRecord {
        number: 9,
        title: "野性回响".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0009.md".to_string(),
        summary: "段知遥在训练后释放压力，并接受队友的烧烤邀请。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["韩澈川认可段知遥呐喊中无法由数据模拟的野性。".to_string()],
        continuity_updates: vec!["段知遥与队友的关系进一步拉近。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    normalize_chapter_metadata_against_body(&manifest, &mut chapter, body);

    assert_eq!(chapter.title, "野性回响");
}

#[test]
fn chapter_metadata_summary_accepts_validated_truth_items_spread_across_body() {
    let content = "晏照序查看三号发电机房的燃料曲线，发现实际消耗比正常值高出百分之十五。这个异常让他确认，站内还有一个没有进入静默状态的人。随后，他播放林远留下的录音，背景里持续出现低频嗡嗡声。沿着线路追查后，韩知野确认三号机正在为备用天线供电。";
    let summary = "晏照序发现三号发电机房燃料消耗多出15%，推断存在未处于静默状态的第四人。；林远录音与线路证据表明三号机正在为备用天线供电，背景音中有低频嗡嗡声。";
    let chapter = ChapterRecord {
        number: 2,
        title: "极夜下的第四人".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: summary.to_string(),
        unit_count: 2500,
        status: "reviewed_passed".to_string(),
        key_facts: vec![
            "晏照序发现三号发电机房燃料消耗多出15%，推断存在未处于静默状态的第四人。".to_string(),
            "林远录音与线路证据表明三号机正在为备用天线供电，背景音中有低频嗡嗡声。".to_string(),
        ],
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    assert!(chapter_summary_supported_by_content(
        summary, content, "zh-CN"
    ));
    assert!(chapter_summary_supported_by_truth_items(&chapter, "zh-CN"));
}

#[test]
fn chapter_metadata_summary_accepts_concise_paraphrase_across_multiple_truth_items() {
    let chapter = ChapterRecord {
        number: 2,
        title: "资本的博弈与信任的交托".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "南谨弦利用重生者的先知视角，精准洞察了沈氏集团面临的资金缺口，并提出通过期货市场杠杆对冲风险的激进策略，成功赢得了秦泊序的信任与调度权的交托。".to_string(),
        unit_count: 2500,
        status: "reviewed_passed".to_string(),
        key_facts: vec![
            "沈氏集团目前面临生产线预付款增加与原材料价格波动的双重压力".to_string(),
            "南谨弦提议利用第一桶金在期货市场进行空头对冲和多头布局，以时间换空间".to_string(),
            "秦泊序决定打破守旧的习惯，将生产线的调度权暂时交给南谨弦".to_string(),
        ],
        continuity_updates: vec![
            "南谨弦提出利用期货市场波动填补缺口的战略计划".to_string(),
            "秦泊序将生产线的调度权交托给南谨弦".to_string(),
        ],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    assert!(chapter_summary_supported_by_truth_items(&chapter, "zh-CN"));
}

#[test]
fn chapter_metadata_summary_rejects_pronoun_and_dialogue_fragments() {
    assert!(chapter_summary_looks_like_prose_fragment(
        "他在看你能利用这个节点走多远",
        "zh-CN"
    ));
    assert!(chapter_summary_looks_like_prose_fragment(
        "阮星轨转过头说：白，这可能就是关键",
        "zh-CN"
    ));
}

#[test]
fn chapter_metadata_gate_rejects_fully_reused_prior_truth_items() {
    let mut manifest = test_manifest_with_primary_character();
    let shared_facts = vec![
        "黎启洄发现旧考场规则正在吞噬旁听生记忆。".to_string(),
        "梁衡遥决定公开试炼牌的真实来源。".to_string(),
    ];
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "旧考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄发现旧考场规则并与梁衡遥决定公开证据。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: shared_facts.clone(),
        continuity_updates: shared_facts.clone(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });
    let current = ChapterRecord {
        number: 2,
        title: "雨夜取证".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "黎启洄进入档案室并取得新的账册证据。".to_string(),
        unit_count: 2500,
        status: "reviewed_passed".to_string(),
        key_facts: shared_facts.clone(),
        continuity_updates: shared_facts,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let issues = chapter_metadata_gate(&manifest, &current, "黎启洄进入档案室并取得新的账册证据。")
        .repairable
        .join("\n");
    assert!(issues.contains("fully reused"), "{issues}");
}

#[test]
fn title_without_story_evidence_requires_metadata_repair() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.chapter_unit_target = None;
    let chapter = ChapterRecord {
        number: 1,
        title: "弦音初颤".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在寒雨考场夺回试炼牌，并发现旧规则正在吞噬旁听生的记忆。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄夺回试炼牌。".to_string()],
        continuity_updates: vec!["旧考场规则开始吞噬旁听生记忆。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "黎启洄站在寒雨考场中央，夺回被扣押的试炼牌，也让旁听生第一次获得公开申诉的资格。他没有把同伴推出去抵债，而是在众目睽睽下改写了考场规则。";

    let gate = chapter_quality_gate(&manifest, &chapter, content, &[]);
    let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

    assert!(gate.passed);
    assert!(gate.issues.is_empty());
    assert!(gate.repairable.is_empty());
    assert!(gate.warnings.is_empty());
    assert!(metadata_gate.repairable.iter().any(|issue| {
        issue.contains("章节标题没有被本章摘要")
            && issue.contains("not grounded in chapter evidence")
    }));
    assert!(metadata_gate.warnings.is_empty());
}

#[test]
fn summary_body_excerpt_without_chapter_outcome_requires_metadata_repair() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 4,
        title: "旧档暗账".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: "黎启洄确认走廊无人后，推开档案室的木门进入房间。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄在旧账册中发现学院篡改考试名次的暗账。".to_string()],
        continuity_updates: vec!["黎启洄取得暗账原件，下一章将与沈青萝核对经手人。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "夜雨敲打学院西廊，守夜人的灯影刚从楼梯口消失。黎启洄确认走廊无人后，推开档案室的木门进入房间。他逐页核对旧账册，最终发现学院篡改考试名次的暗账，并把原件藏入衣袋，决定与沈青萝核对经手人。";

    let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

    assert!(
        metadata_gate.repairable.iter().any(|issue| {
            issue.contains("does not cover the chapter's key facts or continuity change")
        }),
        "issues: {:?}",
        metadata_gate.repairable
    );
}

#[test]
fn summary_support_warning_does_not_block_usable_chapter_body() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "寒雨考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在旧城雨巷发现一枚会改变誓约规则的黑色钟芯。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄夺回试炼牌。".to_string()],
        continuity_updates: vec!["寒雨考场规则被迫公开。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "黎启洄站在寒雨考场中央，夺回被扣押的试炼牌，也让旁听生第一次获得公开申诉的资格。他没有把同伴推出去抵债，而是在众目睽睽下改写了考场规则。";

    let metadata_gate = chapter_metadata_gate(&manifest, &chapter, content);

    assert!(!metadata_gate.passed);
    assert!(metadata_gate.blocking.is_empty());
    assert!(metadata_gate
        .repairable
        .iter()
        .any(|issue| issue.contains("chapter summary is not supported")));
    assert!(metadata_gate.warnings.is_empty());
}

#[test]
fn quality_gate_blocks_primary_character_replacement() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 16,
        title: "第十六章：裂纹下的微光".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0016.md".to_string(),
        summary: "林墨在废墟中与苏禾争执，并试图关闭稳定器。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["林墨发现结晶正在吞噬现实。".to_string()],
        continuity_updates: vec!["林墨决定阻止苏禾。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let content =
        "林墨站在废墟边缘，林墨听见苏禾说出新的秩序。林墨抬起稳定器，决定阻止结晶继续扩张。";
    let issues = contract_character_anchor_issues(&manifest, &chapter, content);
    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(issues
        .iter()
        .any(|issue| issue.contains("primary character anchor")));
    assert!(drift
        .iter()
        .any(|issue| issue.contains("possible protagonist replacement")));
}

#[test]
fn quality_gate_warns_on_degenerate_repeated_scene_fragments() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 2,
        title: "第二章：试炼钟声".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "黎启洄在试炼中承受压力并找到突破口。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现试炼阵纹的缺口。".to_string()],
        continuity_updates: vec!["黎启洄开始理解符文网络的代价。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let repeated = "黎启洄没有理会对方的嘲讽，他的注意力全部集中在阵纹上。";
    let content = [
        "黎启洄站在试炼台前，听见钟声从雾里滚过。",
        repeated,
        "他深吸一口气，试图让灵息沿着旧伤重新流动。",
        repeated,
        "台下的人群开始低语，导师也皱起眉头。",
        repeated,
        "他终于看见阵纹里缺失的那一点光。",
    ]
    .join("");

    let gate = chapter_quality_gate(&manifest, &chapter, &content, &[]);

    assert!(gate
        .warnings
        .iter()
        .any(|issue| issue.contains("repeats the same scene fragment")));
}

#[test]
fn quality_gate_warns_on_repeated_large_scene_blocks() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 2,
        title: "第二章：账册暗流".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "黎启洄在档案室找到责任账册并确认旧城暗线。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现责任账册被人刻意调换。".to_string()],
        continuity_updates: vec!["黎启洄决定追查地下档案室的旧城暗线。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let block = "\
黎启洄沿着地下二层的走廊往前走，潮湿的墙皮在灯下泛出灰白色的光。
他在档案柜背面找到一枚被折断的铜扣，铜扣内侧刻着旧城账册的编号。
门外传来巡查员的脚步声，他把铜扣压进掌心，意识到这不是普通遗失物。";
    let content = format!(
            "黎启洄推开档案室的门，先确认窗外没有人影。\n{block}\n他把线索记进随身本，准备离开。\n{block}\n门锁忽然轻响，他知道有人正在靠近。"
        );

    let gate = chapter_quality_gate(&manifest, &chapter, &content, &[]);

    assert!(gate
        .warnings
        .iter()
        .any(|issue| issue.contains("repeats the same scene block")));
}

#[test]
fn quality_gate_does_not_treat_authority_character_name_frequency_as_story_term_overuse() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
                "name: 艾利克斯; role: 主角; desire: 送达密钥信; fear: 被机械体制吞没; bottom_line: 不背弃同伴".to_string(),
                "name: 铁腕; role: 同伴; desire: 夺取中央塔控制权".to_string(),
            ];
        contract.outline = "艾利克斯背负密钥信穿越新伦敦，最终抵达中央塔。".to_string();
    }
    let chapter = ChapterRecord {
        number: 1,
        title: "中央塔的信".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "艾利克斯在新伦敦底层误拿密钥信，并被迫奔向中央塔。".to_string(),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["艾利克斯确认密钥信必须送达中央塔。".to_string()],
        continuity_updates: vec!["艾利克斯与铁腕建立临时合作。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let actions = [
        ("雨棚塌落前", "把密钥信塞进防水内袋，踩过冒泡的煤泥"),
        ("铜桥升起时", "抓住邮差缆索滑向对岸，避开巡逻灯束"),
        ("雾钟敲响后", "听见信封里齿轮轻响，意识到坐标正在变化"),
        ("集市熄灯前", "帮卖花老妇推开失控货箱，换来一枚冷却针"),
        ("窄巷回声里", "看见黑风衣追兵分成两队，立刻改走排烟井"),
        ("旧站台边缘", "和铁腕交换暗号，把假邮袋抛进货车底盘"),
        ("玻璃升降舱内", "发现贵族区地图被人故意撕去中央塔入口"),
        ("管道爆裂瞬间", "用铜哨震开锁扣，让热雾遮住追踪镜片"),
        ("废弃钟楼下", "拆下义肢侧盖，把过热阀门拧回安全刻度"),
        ("悬空轨道尽头", "望见中央塔白光压低云层，终于确认送达期限"),
        ("机库警报声中", "沿着飞艇骨架爬行，躲开视界集团的探照眼"),
        ("冷却池旁", "把手伸进冰水里压住颤抖，听铁腕讲出反抗军暗线"),
        ("上层宴会厅外", "看见丝绸贵族谈笑，脚下却流过底层抽来的蒸汽"),
        ("红色信号灯亮起", "判断维克多已经封锁主桥，只能走旧检修索"),
        ("风压撕开外套时", "把信封咬在齿间，借货运钩翻上另一节车厢"),
        ("报废机器人醒来前", "用邮差编号骗过门禁，却被要求留下血样"),
        (
            "塔影吞没街区时",
            "第一次怀疑这封信不是启动钥匙，而是审判名单",
        ),
        ("灰鸽群飞散后", "在羽毛和煤灰之间找到收件人的第二行隐形字"),
        ("地下餐车晃动中", "把冷掉的合成汤分给铁腕，两人短暂笑了一声"),
        ("终端蓝光闪烁时", "读出中央塔顶层坐标，也读出自己的通缉编号"),
        ("蒸汽闸门合拢前", "冲进只够一人通过的缝隙，把追兵关在身后"),
        (
            "旧邮局招牌下",
            "摸到墙里藏着的备用路线图，发现前任邮差留下刻痕",
        ),
        ("贵族飞艇降落时", "混进搬运工队伍，用煤灰遮住邮差徽章"),
        ("义肢冷却针断裂后", "咬牙继续奔跑，把疼痛压成短促呼吸"),
        ("中央塔影贴近时", "听见整座城市的管道像巨兽一样吸气"),
        ("升降井坠落中", "抓住铁腕抛来的链钩，两人撞上维护平台"),
        ("白雾散开以后", "发现追猎者首领没有开枪，只递来一枚旧邮戳"),
        ("能源脉冲扫过街面", "看见每个人胸口都短暂亮起编号"),
        ("齿轮广场尽头", "把密钥信对准塔门，信纸却先烫伤了掌心"),
        ("最后一班轨道车驶离时", "选择跳下站台，沿着供热管继续向上"),
    ];
    let paragraphs = actions
        .iter()
        .map(|(opening, action)| format!("{opening}，艾利克斯{action}。"))
        .collect::<Vec<_>>();
    let content = paragraphs.join("\n");

    let gate = chapter_quality_gate(&manifest, &chapter, &content, &[]);

    assert!(
        !gate
            .issues
            .iter()
            .any(|issue| issue.contains("overuses the same story term")),
        "{:?}",
        gate.issues
    );
}

#[test]
fn quality_gate_does_not_treat_world_concepts_as_character_replacement() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "第一章：玻璃幕墙".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄发现城市符文网络。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄在幕墙上发现符文密码。".to_string()],
        continuity_updates: vec!["黎启洄开始研究符文网络。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let content = "黎启洄站在玻璃幕墙前，黎启洄看到符文闪烁。符文沿着幕墙蔓延，符文像密码一样嵌入城市网络。符文不是人物，而是世界规则的一部分。黎启洄记录符文、符号、阵法和节点，决定继续追查。";
    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
        !drift
            .iter()
            .any(|issue| issue.contains("possible protagonist replacement")),
        "{drift:?}"
    );
}

#[test]
fn quality_gate_blocks_recurring_supporting_character_without_registration() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "寒雨考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在寒雨考场发现试炼牌异常。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现试炼牌异常。".to_string()],
        continuity_updates: vec!["寒雨考场规则开始松动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "“黎师弟，你看。”林婉儿快步追上黎启洄。林婉儿压低声音，把试炼牌递给他。黎启洄看见牌面裂纹，林婉儿又提醒他不要惊动监考。离开前，林婉儿收回试炼牌。";

    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
        drift
            .iter()
            .any(|issue| issue.contains("unregistered character `林婉儿`")),
        "recurring named supporting characters must be registered before prose: {drift:?}"
    );
    assert!(
        !drift.iter().any(|issue| issue.contains("黎师弟")),
        "address forms for known characters should not become new characters: {drift:?}"
    );
}

#[test]
fn quality_gate_blocks_truncated_single_character_identities() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "灰区试跑".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄进入灰区训练馆。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄完成第一次试跑。".to_string()],
        continuity_updates: vec!["黎启洄获得临时训练资格。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "黎启洄交出凭证。说话的是地下裁判兼债主，老。老嗤笑一声，指向跑道。随后队长指着远处的选手说：“看见那个叫阿的家伙了吗？”";

    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
        drift
            .iter()
            .any(|issue| issue.contains("single-character identity `老`")),
        "a role apposition must not let a truncated name pass as a valid character: {drift:?}"
    );
    assert!(
        drift
            .iter()
            .any(|issue| issue.contains("single-character identity `阿`")),
        "an explicit naming phrase must not let a truncated name pass as a valid character: {drift:?}"
    );
}

#[test]
fn quality_gate_does_not_split_complete_names_or_role_labels_into_single_characters() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "灰区试跑".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄进入灰区训练馆。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄完成第一次试跑。".to_string()],
        continuity_updates: vec!["黎启洄获得临时训练资格。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content =
        "黎启洄遇见一个名叫程景朔的队长。裁判宣布开始，债主站在门外，老人转身离开。教练把这一现象叫风的方向偏转。";

    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
        !drift
            .iter()
            .any(|issue| issue.contains("single-character identity")),
        "complete names and unnamed role labels must not be split into one-character identities: {drift:?}"
    );
}

#[test]
fn quality_gate_blocks_unrecorded_character_that_replaces_protagonist() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "寒雨考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在寒雨考场发现试炼牌异常。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现试炼牌异常。".to_string()],
        continuity_updates: vec!["寒雨考场规则开始松动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "林婉儿走进寒雨考场。林婉儿拿起试炼牌。林婉儿发现裂纹。林婉儿决定追查。林婉儿避开监考。林婉儿把证据藏好。黎启洄只在旁边看了一眼。";

    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
            drift
                .iter()
                .any(|issue| issue.contains("possible protagonist replacement")),
            "dominant unrecorded character should still be blocked as protagonist replacement: {drift:?}"
        );
}

#[test]
fn quality_gate_blocks_named_character_not_registered_by_execution_contract() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "旧井线索".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄接手旧店，并从记忆碎片里看到失踪亲人的线索。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["新增角色：温望舒，作为失踪亲人线索。".to_string()],
        continuity_updates: vec!["黎启洄决定追查旧井。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let drift = unregistered_character_candidate_issues(&manifest, &chapter);

    assert!(
        drift
            .iter()
            .any(|issue| issue.contains("unregistered character `温望舒`")),
        "named characters must be registered before prose generation: {drift:?}"
    );
}

#[test]
fn quality_gate_does_not_treat_role_possessive_phrase_as_character_declaration() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "第一剑鸣".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄挡住对手的攻击并保住凭证。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec![
            "流云手暗含灵力震荡，专门用来震碎对手的兵器或护盾。".to_string(),
        ],
        continuity_updates: vec!["黎启洄开始调查导师的旧日记录。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let drift = unregistered_character_candidate_issues(&manifest, &chapter);

    assert!(
        drift.is_empty(),
        "role possessives and conjunction phrases are grammar, not character declarations: {drift:?}"
    );
}

#[test]
fn quality_gate_ignores_recurring_scene_terms_as_characters() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 1,
        title: "九重天阶初现".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄在九重天阶遭遇异常规则。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现九重天阶规则异常。".to_string()],
        continuity_updates: vec!["黎启洄开始追查规则裂痕。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "黎启洄站在云海前，云海翻滚，云海映出九重天阶。黎启洄站在石阶边，看见符文沿着掌心亮起，符文不是人物，而是世界规则的痕迹。山门前响起钟声，山门前的石阶裂开，山谷尽头传来轰鸣，山谷石壁裂开，山谷深处露出光痕。他终于明白，明白这不是幻象，也明白自己已被规则选中。每一步都像是踩在旧誓言上，每一步都像是把退路压碎。从他体内涌出的金光挡住黑影，从他掌心延伸出的纹路指向远方。";

    let drift = contract_character_drift_issues(&manifest, &chapter, content);

    assert!(
        !drift.iter().any(|issue| issue.contains("云海")
            || issue.contains("山谷")
            || issue.contains("明白")
            || issue.contains("从他")
            || issue.contains("黎启洄站")
            || issue.contains("符文")
            || issue.contains("终于")
            || issue.contains("山门前")
            || issue.contains("步都像是")),
        "scene/state/connective fragments must not become character drift issues: {drift:?}"
    );
}

#[test]
fn quality_gate_warns_when_character_bottom_line_appears_as_inner_vow() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger.push(CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "黎启洄".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "主角".to_string(),
        desire: "改变命运".to_string(),
        fear: "牵连同伴".to_string(),
        bottom_line: "不无解释地背叛核心关系，也不牺牲无辜来换取胜利。".to_string(),
        arc_start: "底层学生".to_string(),
        arc_end: "公开改写规则的人".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    });
    let chapter = ChapterRecord {
        number: 1,
        title: "寒雨考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄夺回试炼牌。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄夺回试炼牌。".to_string()],
        continuity_updates: vec!["寒雨考场规则开始松动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "黎启洄握紧试炼牌，在心里默念：“我不会无谓地背叛核心关系，也不会牺牲无辜来换取胜利。”随后他走向考场中央。";

    let gate = chapter_quality_gate(&manifest, &chapter, content, &[]);

    assert!(
        !gate
            .issues
            .iter()
            .any(|issue| issue.contains("contract/governance clause")),
        "character bottom-line inner vows should not block approval: {:?}",
        gate.issues
    );
    assert!(
        gate.warnings
            .iter()
            .any(|issue| issue.contains("contract/governance clause")),
        "near-exact bottom-line prose should still be visible as warning: {:?}",
        gate.warnings
    );
}

#[test]
fn quality_gate_blocks_must_avoid_governance_clause_leaking_into_prose() {
    let mut manifest = test_manifest_with_primary_character();
    let contract = manifest.contract.as_mut().expect("contract");
    contract.must_avoid = vec!["不要把工具日志或 JSON 外壳写进正文".to_string()];
    let chapter = ChapterRecord {
        number: 1,
        title: "寒雨考场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄夺回试炼牌。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄夺回试炼牌。".to_string()],
        continuity_updates: vec!["寒雨考场规则开始松动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content =
        "黎启洄站在考场中央，忽然想到：不要把工具日志或 JSON 外壳写进正文。随后他继续前进。";

    let gate = chapter_quality_gate(&manifest, &chapter, content, &[]);

    assert!(
        gate.issues
            .iter()
            .any(|issue| issue.contains("contract/governance clause")),
        "must_avoid governance leakage should still block approval: {:?}",
        gate.issues
    );
}

#[test]
fn local_repair_restores_reduplicated_primary_anchor_when_model_shortens_it() {
    let mut manifest = test_manifest_with_primary_character();
    let contract = manifest.contract.as_mut().expect("contract");
    contract.characters = vec![
        "姓名：南棠棠；角色：主角；欲望：找回父亲；恐惧：城市秩序崩塌；底线：不伤害无辜市民"
            .to_string(),
    ];
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "南棠棠".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "主角".to_string(),
        desire: "找回父亲".to_string(),
        fear: "城市秩序崩塌".to_string(),
        bottom_line: "不伤害无辜市民".to_string(),
        arc_start: "守住旧街".to_string(),
        arc_end: "成为城市守灯人".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 1,
        title: "第一章".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "南棠进入旧街裂缝。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["南棠触碰灵纹。".to_string()],
        continuity_updates: vec!["南棠决定追查父亲失踪。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content =
        "南棠站在旧街灯下，南棠听见裂缝里传出父亲的声音。南棠抬手触碰灵纹，决定守住这座城。";

    let repaired = repair_contract_character_name_typos(&manifest, content);

    assert!(repaired.contains("南棠棠站在旧街灯下"));
    assert_eq!(repaired.matches("南棠棠").count(), 3);
    assert!(
        contract_character_anchor_issues(&manifest, &chapter, &repaired).is_empty(),
        "repaired body should preserve canonical primary anchor"
    );
    assert!(
        contract_character_drift_issues(&manifest, &chapter, &repaired).is_empty(),
        "repaired body should not be treated as protagonist replacement"
    );
}

#[test]
fn quality_gate_blocks_established_character_pronoun_drift() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "陶栖序".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["pronoun_profile:feminine".to_string()],
        role: "重要角色".to_string(),
        desire: "解开家族封印".to_string(),
        fear: "家族彻底没落".to_string(),
        bottom_line: "不把源核交给垄断者".to_string(),
        arc_start: "最后的守门人".to_string(),
        arc_end: "参与新秩序".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 4,
        title: "核心室".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "陶栖序站在门前，他把玉简嵌入凹槽。陶栖序没有回头，他只说源核已经醒了。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("pronoun/appellation drift")),
        "pronoun drift should be a blocking identity issue: {issues:?}"
    );
}

#[test]
fn contract_arc_start_seeds_supporting_character_identity_before_first_appearance() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.contract.as_mut().unwrap().characters = vec![
        "name: 南云澜; role: 主角; arc_start: 恪守命令的军人".to_string(),
        "name: 祝启安; role: 关键关系对象; arc_start: 柔弱的贵族少女; arc_end: 坚韧的军中军师"
            .to_string(),
    ];
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        id: "character-0002".to_string(),
        canonical_name: "祝启安".to_string(),
        name_source: "approved_chapter".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["inferred_pronoun_profile:masculine".to_string()],
        role: "关键关系对象".to_string(),
        desire: String::new(),
        fear: String::new(),
        bottom_line: String::new(),
        arc_start: "柔弱的贵族少女".to_string(),
        arc_end: "坚韧的军中军师".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];

    ensure_character_authority_ledger(&mut manifest);

    let character = manifest
        .character_ledger
        .iter()
        .find(|record| record.canonical_name == "祝启安")
        .expect("supporting character authority");
    assert_eq!(
        character.identity_markers,
        vec!["pronoun_profile:feminine".to_string()],
        "explicit contract identity must replace a contradictory body-derived profile"
    );

    let chapter = ChapterRecord {
        number: 2,
        title: "荒原上的规矩".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let issues = contract_character_pronoun_drift_issues(
        &manifest,
        &chapter,
        "祝启安走进营帐，他向南云澜行礼。祝启安展开账册，他开始清点粮草。",
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("pronoun/appellation drift")),
        "the existing deterministic quality gate must consume the seeded contract identity: {issues:?}"
    );
}

#[test]
fn quality_gate_allows_established_character_pronoun_consistency() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "陶栖序".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["pronoun_profile:feminine".to_string()],
        role: "重要角色".to_string(),
        desire: "解开家族封印".to_string(),
        fear: "家族彻底没落".to_string(),
        bottom_line: "不把源核交给垄断者".to_string(),
        arc_start: "最后的守门人".to_string(),
        arc_end: "参与新秩序".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 4,
        title: "核心室".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "陶栖序站在门前，她把玉简嵌入凹槽。陶栖序没有回头，她只说源核已经醒了。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
        issues.is_empty(),
        "consistent pronoun use should not be blocked: {issues:?}"
    );
}

#[test]
fn quality_gate_does_not_multiply_one_ambiguous_pronoun_across_overlapping_name_windows() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0001".to_string(),
            canonical_name: "陆启朔".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:masculine".to_string()],
            role: "主角".to_string(),
            desire: "追查真相".to_string(),
            fear: "失去记忆".to_string(),
            bottom_line: "不伤害无辜".to_string(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0002".to_string(),
            canonical_name: "季砚岚".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:feminine".to_string()],
            role: "对手".to_string(),
            desire: "维持秩序".to_string(),
            fear: "城市失控".to_string(),
            bottom_line: "不放弃中枢".to_string(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];
    let chapter = ChapterRecord {
        number: 2,
        title: "逃亡".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "季砚岚站在数据洪流中，她抬手关闭接口。陆启朔逃进贫民窟，那里也许是他唯一的庇护所。季砚岚回到中枢，她命令特工继续追查。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
        issues.is_empty(),
        "one pronoun attached to another established character must not be multiplied by overlapping windows: {issues:?}"
    );
}

#[test]
fn quality_gate_does_not_attach_nearby_female_role_pronouns_to_male_character() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "裴予舟".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["pronoun_profile:masculine".to_string()],
        role: "主角".to_string(),
        desire: "查明残契真相".to_string(),
        fear: "奇术反噬".to_string(),
        bottom_line: "不主动献祭至亲".to_string(),
        arc_start: "落魄术士".to_string(),
        arc_end: "主动承担代价".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 2,
        title: "暗室".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "裴予舟看着李夫人，她抬起手，示意老仆打开暗室。他没有后退，只把残契压在掌心。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
            issues.is_empty(),
            "female role pronouns near a male character mention should not be attached to him: {issues:?}"
        );
}

#[test]
fn quality_gate_does_not_attach_other_character_pronouns_across_names() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0001".to_string(),
            canonical_name: "闻星穹".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:masculine".to_string()],
            role: "主角".to_string(),
            desire: "建立生物共生秩序".to_string(),
            fear: "机械文明彻底崩溃".to_string(),
            bottom_line: "不牺牲核心种群换取短期生存".to_string(),
            arc_start: "工程师".to_string(),
            arc_end: "共生节点".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0002".to_string(),
            canonical_name: "阿珊瑚".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:feminine".to_string()],
            role: "同伴".to_string(),
            desire: "完成生态净化".to_string(),
            fear: "失去可塑性".to_string(),
            bottom_line: "保持变异可塑性".to_string(),
            arc_start: "生态异变体".to_string(),
            arc_end: "共生守护者".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];
    let chapter = ChapterRecord {
        number: 8,
        title: "生物电场".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0008.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "阿珊瑚点了点头，眼中闪过一丝赞许。闻星穹知道，将生物电场注入基座可能过载，但他没有时间犹豫。阿珊瑚站在他身后，双手张开，她像是在指挥一场无声的交响乐。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
            issues.is_empty(),
            "pronouns belonging to another named character should not be attached across character names: {issues:?}"
        );
}

#[test]
fn quality_gate_does_not_attach_repeated_object_pronouns_to_the_named_subject() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0001".to_string(),
            canonical_name: "宋晏川".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:masculine".to_string()],
            role: "主角".to_string(),
            desire: "完成旧城设计".to_string(),
            fear: "失去自我".to_string(),
            bottom_line: "不牺牲职业尊严".to_string(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0002".to_string(),
            canonical_name: "祝星岚".to_string(),
            aliases: Vec::new(),
            identity_markers: vec!["pronoun_profile:feminine".to_string()],
            role: "关键关系对象".to_string(),
            desire: "守住选择".to_string(),
            fear: "再次分离".to_string(),
            bottom_line: "不放弃职业".to_string(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];
    let chapter = ChapterRecord {
        number: 5,
        title: "审核".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0005.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "宋晏川接过杯子，他看着她，问她是否要走。宋晏川看向祝星岚，她没有回答。宋晏川重新坐下，他拿起笔。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
        issues.is_empty(),
        "object pronouns must not be attributed back to the named male subject: {issues:?}"
    );
}

#[test]
fn inferred_pronoun_profile_blocks_repeated_future_contradiction() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "陶栖序".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["inferred_pronoun_profile:feminine".to_string()],
        role: "重要角色".to_string(),
        desire: "解开家族封印".to_string(),
        fear: "家族彻底没落".to_string(),
        bottom_line: "不把源核交给垄断者".to_string(),
        arc_start: "最后的守门人".to_string(),
        arc_end: "参与新秩序".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 4,
        title: "核心室".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "陶栖序站在门前，他把玉简嵌入凹槽。陶栖序没有回头，他只说源核已经醒了。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
            issues
                .iter()
                .any(|issue| issue.contains("pronoun/appellation drift")),
            "stable identity inferred from approved prose must block repeated contradictory references: {issues:?}"
        );
}

#[test]
fn approved_final_body_locks_first_stable_pronoun_profile_without_later_overwrite() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "陶栖序".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "主角".to_string(),
        desire: "解开家族封印".to_string(),
        fear: "家族彻底没落".to_string(),
        bottom_line: "不把源核交给垄断者".to_string(),
        arc_start: "最后的守门人".to_string(),
        arc_end: "参与新秩序".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "陶栖序站在门前，他把玉简嵌入凹槽。陶栖序没有回头，他只说源核已经醒了。",
    );
    assert!(manifest.character_ledger[0]
        .identity_markers
        .iter()
        .any(|marker| marker == "inferred_pronoun_profile:masculine"));

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "陶栖序站在门前，她把玉简嵌入凹槽。陶栖序没有回头，她只说源核已经醒了。",
    );
    assert_eq!(
        manifest.character_ledger[0]
            .identity_markers
            .iter()
            .filter(|marker| marker.starts_with("inferred_pronoun_profile:"))
            .count(),
        1
    );
    assert!(manifest.character_ledger[0]
        .identity_markers
        .iter()
        .any(|marker| marker == "inferred_pronoun_profile:masculine"));

    let chapter = ChapterRecord {
        number: 2,
        title: "核心室".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let issues = contract_character_pronoun_drift_issues(
        &manifest,
        &chapter,
        "陶栖序站在门前，她把玉简嵌入凹槽。陶栖序没有回头，她只说源核已经醒了。",
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("pronoun/appellation drift")),
        "the first stable approved-body profile must become future continuity authority: {issues:?}"
    );
}

#[test]
fn approved_final_body_locks_sole_named_primary_from_stable_narrative_pronouns() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "许观澜".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "主角".to_string(),
        desire: "找回遗失的记忆".to_string(),
        fear: "失去感知能力".to_string(),
        bottom_line: "不牺牲他人".to_string(),
        arc_start: "孤独的修复者".to_string(),
        arc_end: "坚定的守护者".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "许观澜低头校准刻度，随后她抬起放大镜检查齿轮。齿轮重新转动时，她屏住呼吸。她没有移开视线，而是把异常频率记进账本。门外的访客停在阴影里，他始终没有报出姓名。",
    );

    assert_eq!(
        manifest.character_ledger[0].identity_markers,
        vec!["inferred_pronoun_profile:feminine"]
    );
}

#[test]
fn approved_final_body_does_not_assign_unnamed_secondary_pronouns_to_sole_named_primary() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "许观澜".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "主人公".to_string(),
        desire: "找回遗失的记忆".to_string(),
        fear: "失去感知能力".to_string(),
        bottom_line: "不牺牲他人".to_string(),
        arc_start: "孤独的修复者".to_string(),
        arc_end: "坚定的守护者".to_string(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "许观澜整理好他的工具，把记录放回抽屉。门外的女孩走进工作间，她放下雨伞。她检查窗锁，又把她带来的旧表放在桌上。",
    );

    assert!(manifest.character_ledger[0].identity_markers.is_empty());
}

#[test]
fn approved_final_body_does_not_use_global_pronouns_when_another_named_character_appears() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0001".to_string(),
            canonical_name: "许观澜".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "主角".to_string(),
            desire: "找回遗失的记忆".to_string(),
            fear: "失去感知能力".to_string(),
            bottom_line: "不牺牲他人".to_string(),
            arc_start: "孤独的修复者".to_string(),
            arc_end: "坚定的守护者".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0002".to_string(),
            canonical_name: "顾临川".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "关键关系对象".to_string(),
            desire: "守住档案".to_string(),
            fear: "秘密暴露".to_string(),
            bottom_line: "不伤害无辜".to_string(),
            arc_start: String::new(),
            arc_end: String::new(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "许观澜看向顾临川。她把档案收进抽屉，她没有解释自己的来意。",
    );

    assert!(manifest
        .character_ledger
        .iter()
        .all(|character| character.identity_markers.is_empty()));
}

#[test]
fn approved_final_body_locks_repeated_cross_sentence_profiles_for_separate_named_characters() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0001".to_string(),
            canonical_name: "梁知遥".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "主角".to_string(),
            desire: "修复裂痕".to_string(),
            fear: "城市崩塌".to_string(),
            bottom_line: "不牺牲他人".to_string(),
            arc_start: "谨慎的维修工".to_string(),
            arc_end: "城市守护者".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
        CharacterAuthorityRecord {
            name_source: "contract".to_string(),
            planned_entry: String::new(),
            planned_exit: String::new(),
            id: "character-0002".to_string(),
            canonical_name: "阮启序".to_string(),
            aliases: Vec::new(),
            identity_markers: Vec::new(),
            role: "关键关系对象".to_string(),
            desire: "记录空间异常".to_string(),
            fear: "观测失真".to_string(),
            bottom_line: "不伪造数据".to_string(),
            arc_start: "严谨的观察员".to_string(),
            arc_end: "共同重建城市".to_string(),
            forbidden_renames: Vec::new(),
            status: "active".to_string(),
            updated_at: Utc::now().to_rfc3339(),
        },
    ];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "梁知遥放下工具箱。她用指尖检查墙缝。梁知遥收回抹刀。她没有离开现场。\n\n阮启序站在观测塔顶。他记录重力参数。阮启序放下望远镜。他重新核对读数。",
    );

    let primary = manifest
        .character_ledger
        .iter()
        .find(|character| character.canonical_name == "梁知遥")
        .expect("primary character");
    assert_eq!(
        primary.identity_markers,
        vec!["inferred_pronoun_profile:feminine"]
    );
    let observer = manifest
        .character_ledger
        .iter()
        .find(|character| character.canonical_name == "阮启序")
        .expect("observer character");
    assert_eq!(
        observer.identity_markers,
        vec!["inferred_pronoun_profile:masculine"]
    );
}

#[test]
fn approved_final_body_does_not_turn_repeated_object_pronouns_into_identity_authority() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0002".to_string(),
        canonical_name: "陆栖真".to_string(),
        aliases: Vec::new(),
        identity_markers: Vec::new(),
        role: "关键关系对象".to_string(),
        desire: "守护岑启川".to_string(),
        fear: "失去岑启川".to_string(),
        bottom_line: "必须守住岑启川的安全".to_string(),
        arc_start: String::new(),
        arc_end: String::new(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];

    promote_approved_chapter_character_identity_markers(
        &mut manifest,
        "陆栖真曾是他最信任的盾，也是最后为他挡下致命一击的利刃。陆栖真放下水盆，有些嗔怪地看着他，随后注意到他的目光。",
    );

    assert!(
        manifest.character_ledger[0]
            .identity_markers
            .iter()
            .all(|marker| !marker.starts_with("inferred_pronoun_profile:")),
        "object pronouns must not become final-body identity authority"
    );
}

#[test]
fn quality_gate_blocks_mixed_identity_even_when_matching_references_are_more_frequent() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.character_ledger = vec![CharacterAuthorityRecord {
        name_source: "contract".to_string(),
        planned_entry: String::new(),
        planned_exit: String::new(),
        id: "character-0001".to_string(),
        canonical_name: "陆栖真".to_string(),
        aliases: Vec::new(),
        identity_markers: vec!["pronoun_profile:masculine".to_string()],
        role: "关键关系对象".to_string(),
        desire: "守住领地".to_string(),
        fear: "任务失败".to_string(),
        bottom_line: "不伤害无辜".to_string(),
        arc_start: String::new(),
        arc_end: String::new(),
        forbidden_renames: Vec::new(),
        status: "active".to_string(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    let chapter = ChapterRecord {
        number: 2,
        title: "残镜余温".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: String::new(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content = "陆栖真走到床边，他放下毛巾。陆栖真没有回头，他关上窗。陆栖真守在门口，他握紧刀柄。陆栖真走到镜前，她收起残镜。陆栖真回到床边，她低声告警。";

    let issues = contract_character_pronoun_drift_issues(&manifest, &chapter, content);

    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("pronoun/appellation drift")),
        "repeated opposite identity evidence must not be hidden by majority matching references: {issues:?}"
    );
}

#[test]
fn quality_gate_does_not_require_every_secondary_character_each_chapter() {
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 3,
        title: "第三章：雾城试炼".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0003.md".to_string(),
        summary: "黎启洄独自进入试炼场，寻找改变命运的机会。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄完成试炼前的准备。".to_string()],
        continuity_updates: vec!["黎启洄暂时与同伴分头行动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let content = "黎启洄推开试炼场的铁门，独自面对第一轮考核。";
    let issues = contract_character_anchor_issues(&manifest, &chapter, content);

    assert!(
        issues.is_empty(),
        "secondary characters should not be required in every chapter: {issues:?}"
    );
}

#[test]
fn title_fatigue_blocks_repeated_cjk_title_template() {
    let mut manifest = test_manifest_with_primary_character();
    for (number, title) in [
        (1, "第一章：裂痕中的余烬"),
        (2, "第二章：灰雾下的试炼"),
        (3, "第三章：钟声下的赌局"),
        (4, "第四章：星火下的追索"),
    ] {
        manifest.chapters.push(ChapterRecord {
            number,
            title: title.to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: format!("chapters/{number:04}.md"),
            summary: "黎启洄推进了一个明确变化。".to_string(),
            unit_count: 2500,
            status: "approved".to_string(),
            key_facts: vec!["黎启洄完成阶段推进。".to_string()],
            continuity_updates: vec!["黎启洄保持主线连续。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }
    let chapter = ChapterRecord {
        number: 5,
        title: "第五章：暗潮下的回声".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0005.md".to_string(),
        summary: "黎启洄继续行动。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现新证据。".to_string()],
        continuity_updates: vec!["黎启洄进入下一阶段。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let issues = chapter_title_fatigue_issues(&manifest, &chapter);

    assert!(issues
        .iter()
        .any(|issue| issue.contains("syntactic template")));
}

#[test]
fn title_fatigue_allows_one_prior_matching_cjk_template() {
    let issues = crate::tool::writing::naming::chapter_title_fatigue_issues(
        "zh-CN",
        2,
        "第二章：齿轮中的蓝图",
        [(1, "第一章：裂隙中的残影".to_string())],
    );

    assert!(
        !issues
            .iter()
            .any(|issue| issue.contains("recent syntactic template")),
        "one ordinary neighbouring template must not trigger fatigue: {issues:?}"
    );
}

#[test]
fn chapter_metadata_gate_keeps_repeated_title_fatigue_advisory() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "雾起无声，潮汐有信".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "温望遥醒在雾隐岛，确认潮汐和声音代价。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: vec!["温望遥确认声音会消耗声量。".to_string()],
        continuity_updates: vec!["雾隐岛的潮汐契约启动。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });
    let chapter = ChapterRecord {
        number: 2,
        title: "鹤鸣无声，契约有声".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "温望遥和沈照澜前往灯塔废墟，进一步确认声量规则。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: vec!["沈照澜展示银质哨子。".to_string()],
        continuity_updates: vec!["灯塔废墟成为下一处调查地点。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let direct_fatigue = crate::tool::writing::naming::chapter_title_fatigue_issues(
        &manifest.language,
        chapter.number,
        &chapter.title,
        manifest
            .chapters
            .iter()
            .map(|other| (other.number, other.title.clone())),
    );
    assert!(
        direct_fatigue
            .iter()
            .any(|issue| issue.contains("repeats a recent")),
        "naming fatigue should detect repeated title rhythm: {:?}",
        direct_fatigue
    );
    let studio_fatigue = chapter_title_fatigue_issues(&manifest, &chapter);
    assert!(
        studio_fatigue
            .iter()
            .any(|issue| issue.contains("repeats a recent")),
        "studio fatigue should pass through repeated title rhythm: {:?}",
        studio_fatigue
    );

    let gate = chapter_metadata_gate(
        &manifest,
        &chapter,
        "沈照澜展示银质哨子，温望遥确认声量规则后前往灯塔废墟。",
    );

    assert!(
        gate.blocking.is_empty(),
        "title fatigue is metadata-only, not a body blocker: {:?}",
        gate.blocking
    );
    assert!(
        !gate
            .repairable
            .iter()
            .any(|issue| issue.contains("repeats a recent")),
        "subjective title rhythm must not enter the repair queue: {:?}",
        gate.repairable
    );
    assert!(
        gate.warnings
            .iter()
            .any(|issue| issue.contains("repeats a recent")),
        "repeated title rhythm should remain visible as advisory telemetry: {:?}",
        gate.warnings
    );
}

#[test]
fn generic_stage_chapter_title_is_repaired_by_metadata_gate_not_surface_gate() {
    let manifest = test_manifest_with_primary_character();

    assert!(!title_needs_post_body_repair(&manifest, 3, "第三章：抉择"));
    assert!(!title_needs_post_body_repair(&manifest, 4, "第四章：裂痕"));
    assert!(title_needs_post_body_repair(&manifest, 1, "钟桥序没"));
    assert!(!title_needs_post_body_repair(
        &manifest,
        5,
        "第五章：黑炉试血"
    ));
}

#[test]
fn cjk_malformed_phrase_gate_preserves_normal_question_words() {
    let content =
        "姜桥晚问：为什么偏偏是我？钟桥序没有回答。他低声喃喃自语，想知道这到底是什么东西。";

    let issues = cjk_malformed_phrase_issues(content);

    assert!(
        !issues
            .iter()
            .any(|issue| issue.contains("为什") || issue.contains("喃自语")),
        "{issues:?}"
    );
}

#[test]
fn completion_gate_requires_promised_relationship_and_antagonist_payoff() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.target_units = Some(5_000);
    if let Some(contract) = manifest.contract.as_mut() {
        contract.premise = "草根少年完成逆袭，并兑现情感线。".to_string();
        contract.characters = vec![
                "name: 黎启洄; role: 主角; desire: 改变命运; fear: 被抛弃; bottom_line: 不牺牲同伴"
                    .to_string(),
                "name: 沈青萝; role: 女主; desire: 获得自由; fear: 被家族控制; bottom_line: 不背叛真心"
                    .to_string(),
                "name: 许照白; role: 反派; desire: 垄断晋级名额; fear: 失去特权; bottom_line: 不承认底层"
                    .to_string(),
            ];
        contract.outline = "终局：黎启洄打败坏人并抱得美人归。".to_string();
    }
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "第一章：旧校门".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄进入学院。".to_string(),
        unit_count: 3_000,
        status: "approved".to_string(),
        key_facts: vec!["黎启洄进入学院。".to_string()],
        continuity_updates: vec!["黎启洄准备考试。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });
    ensure_story_bible_from_manifest(&mut manifest);
    let chapter = ChapterRecord {
        number: 2,
        title: "第二章：终局".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "黎启洄完成终局，世界恢复平静。".to_string(),
        unit_count: 2_500,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄完成终局。".to_string()],
        continuity_updates: vec!["终局收束。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let content = "黎启洄站在校门前，听见钟声落定。旧制度被新的规则替代，所有人都说故事终于结束。";
    let issues = chapter_completion_mode_issues(&manifest, &chapter, content);

    assert!(issues
        .iter()
        .any(|issue| issue.contains("relationship/emotional")));
    assert!(issues
        .iter()
        .any(|issue| issue.contains("antagonist/opposition")));
}

#[test]
fn completion_gate_respects_planned_final_chapter_before_target_units() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.target_units = Some(50_000);
    manifest.volumes = vec![VolumeRecord {
        id: "volume-0001".to_string(),
        title: "第一卷".to_string(),
        start_chapter: 1,
        end_chapter: Some(20),
        objective: "完成整本书主线。".to_string(),
        key_results: Vec::new(),
        emotional_curve: String::new(),
        must_open: Vec::new(),
        must_payoff: Vec::new(),
        ending_change: "第20章收束主线。".to_string(),
        status: "active".to_string(),
        summary: None,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    for number in 1..15 {
        manifest.chapters.push(ChapterRecord {
            number,
            title: format!("第{number}章"),
            volume_id: "volume-0001".to_string(),
            volume_title: "第一卷".to_string(),
            path: format!("chapters/{number:04}.md"),
            summary: "前序章节推进。".to_string(),
            unit_count: 4_000,
            status: "approved".to_string(),
            key_facts: vec!["前序章节推进。".to_string()],
            continuity_updates: vec!["前序状态更新。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }
    let chapter = ChapterRecord {
        number: 15,
        title: "中央岛入口".to_string(),
        volume_id: "volume-0001".to_string(),
        volume_title: "第一卷".to_string(),
        path: "chapters/0015.md".to_string(),
        summary: "主角取得中央岛通行权，准备进入下一阶段。".to_string(),
        unit_count: 3_000,
        status: "draft".to_string(),
        key_facts: vec!["主角取得中央岛通行权。".to_string()],
        continuity_updates: vec!["下一章进入中央岛。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let content =
        "主角取得通行权，远处中央岛的灯塔亮起。新的入口打开，他知道真正的对抗还没有结束。";

    let issues = chapter_completion_mode_issues(&manifest, &chapter, content);

    assert!(
            issues.is_empty(),
            "mid-story chapter should not be forced into completion mode before planned final chapter: {issues:?}"
        );
}

#[test]
fn quality_gate_blocks_malformed_anchor_predicate_fragments() {
    let manifest = test_manifest_with_primary_character();
    let content = "黎启洄识到旧规则并没有真正消失，沈青萝静地站在门外。";

    let issues = anchor_malformed_predicate_issues(&manifest, content);

    assert!(issues
        .iter()
        .any(|issue| issue.contains("malformed phrase")));
}

#[test]
fn strong_character_context_requires_identity_syntax_not_nearby_role_word() {
    assert!(!cjk_candidate_has_strong_person_identity_context(
        "钟声停下后主角重新核对现场。",
        "钟声"
    ));
    assert!(cjk_candidate_has_strong_person_identity_context(
        "新来的同伴名叫林墨。林墨说他会守住入口。",
        "林墨"
    ));
}

#[tokio::test]
async fn legacy_approved_context_preserves_manifest_facts_and_continuity() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest = test_manifest_with_primary_character();
    let chapter = ChapterRecord {
        number: 2,
        title: "旧站回声".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "黎启洄确认旧站仍在运转。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: vec!["黎启洄取得旧站通行证。".to_string()],
        continuity_updates: vec!["通行证仍由黎启洄持有。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let view = approved_chapter_context_view(temp.path(), &manifest, &chapter)
        .await
        .expect("legacy context view");

    assert_eq!(
        view.pointer("/key_facts/0").and_then(|value| value.as_str()),
        Some("黎启洄取得旧站通行证。")
    );
    assert_eq!(
        view.pointer("/continuity_updates/0")
            .and_then(|value| value.as_str()),
        Some("通行证仍由黎启洄持有。")
    );
}

#[test]
fn quality_gate_blocks_anchor_followed_by_sensory_object() {
    let manifest = test_manifest_with_primary_character();
    let content = "黎启洄一种前所未有的剧痛。";

    let issues = anchor_malformed_predicate_issues(&manifest, content);

    assert!(
        issues.iter().any(|issue| issue.contains("黎启洄一种")),
        "{issues:?}"
    );
}

#[test]
fn quality_gate_allows_plural_demonstrative_after_character_anchor() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec!["name: 陆启宁; role: 对手; desire: 找到青灯".to_string()];
    }
    ensure_project_governance(&mut manifest);

    let issues = anchor_malformed_predicate_issues(
        &manifest,
        "一旦释放灵气，陆启宁那些人的感应会比之前敏锐十倍。",
    );

    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn quality_gate_does_not_scan_later_predicate_for_demonstrative_locative() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec!["name: 陆启宁; role: 对手; desire: 找到青灯".to_string()];
    }
    ensure_project_governance(&mut manifest);

    for content in [
        "陆启宁那个人的感应会比之前更强。",
        "陆启宁那个对手后来改变了计划。",
        "陆启宁那座前哨站仍然亮着灯。",
    ] {
        let issues = anchor_malformed_predicate_issues(&manifest, content);
        assert!(issues.is_empty(), "{content}: {issues:?}");
    }
}

#[test]
fn quality_gate_blocks_cjk_anchor_surface_fragments() {
    let manifest = test_manifest_with_primary_character();
    let content = "黎启洄神一凛，黎启洄头一震。黎启洄脏猛地收缩，黎启洄睛看向远处。黎启洄沈青萝即将离开前开口。";

    let issues = anchor_malformed_predicate_issues(&manifest, content);

    assert!(
        issues.iter().any(|issue| issue.contains("黎启洄神一凛")),
        "{issues:?}"
    );
    assert!(
        issues.iter().any(|issue| issue.contains("黎启洄沈青萝")),
        "{issues:?}"
    );
}

#[test]
fn quality_gate_does_not_treat_action_phrase_as_adjacent_character_anchor() {
    let mut manifest = test_manifest_with_primary_character();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec!["name: 韩照野; role: 主角; desire: 护住灵脉".to_string()];
    }
    for number in 1..=2 {
        manifest.chapters.push(ChapterRecord {
            number,
            title: format!("第{number}章"),
            volume_id: String::new(),
            volume_title: String::new(),
            path: format!("chapters/{number:04}.md"),
            summary: "韩照野从怀中掏出旧木牌，确认灵脉枯竭的源头。".to_string(),
            unit_count: 2500,
            status: "approved".to_string(),
            key_facts: vec!["韩照野从怀中掏出旧木牌。".to_string()],
            continuity_updates: vec!["韩照野继续追查灵脉枯竭。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }

    let issues = anchor_malformed_predicate_issues(&manifest, "韩照野从怀中掏出旧木牌。");

    assert!(
        !issues.iter().any(|issue| issue.contains("从怀中掏")),
        "{issues:?}"
    );
}

#[test]
fn contract_character_name_repair_removes_inserted_cjk_in_anchor() {
    let manifest = test_manifest_with_primary_character();
    let repaired =
        repair_contract_character_name_typos(&manifest, "黎启洄走进风里，黎启落洄没有回头。");

    assert!(repaired.contains("黎启洄没有回头"), "{repaired}");
    assert!(!repaired.contains("黎启落洄"), "{repaired}");
}

#[test]
fn title_fatigue_uses_unapproved_prior_titles_as_references() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    for (number, title, status) in [
        (1, "第一章：裂痕下的余烬", "needs_revision"),
        (2, "第二章：灰雾下的试炼", "draft"),
    ] {
        manifest.chapters.push(ChapterRecord {
            number,
            title: title.to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: format!("chapters/{number:04}.md"),
            summary: "上一章已有标题形态。".to_string(),
            unit_count: 2500,
            status: status.to_string(),
            key_facts: Vec::new(),
            continuity_updates: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }
    let chapter = ChapterRecord {
        number: 3,
        title: "第三章：暗潮下的回声".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0003.md".to_string(),
        summary: "本章不应复用同一标题句式。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let issues = chapter_title_fatigue_issues(&manifest, &chapter);

    assert!(issues
        .iter()
        .any(|issue| issue.contains("recent syntactic template")));
}

#[test]
fn title_fatigue_allows_single_different_connector_neighbor() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "第一章：灰烬之城的裂痕".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "上一章已有标题形态。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });
    let chapter = ChapterRecord {
        number: 2,
        title: "第二章：秩序的余温".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "偶发连接词标题不应被过严拦截。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let issues = chapter_title_fatigue_issues(&manifest, &chapter);

    assert!(!issues.iter().any(|issue| issue.contains("connector-heavy")));
}

#[test]
fn title_fatigue_allows_connector_after_plain_event_title() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "第一章：寒鸦鸣".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "上一章是事件型短标题。".to_string(),
        unit_count: 2500,
        status: "approved".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });
    let chapter = ChapterRecord {
        number: 2,
        title: "第二章：秩序的余温".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0002.md".to_string(),
        summary: "前一章不是连接词模板时，偶发连接词标题可以保留。".to_string(),
        unit_count: 2500,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let issues = chapter_title_fatigue_issues(&manifest, &chapter);

    assert!(
        issues.is_empty(),
        "plain neighbor should allow this: {issues:?}"
    );
}

#[test]
fn stable_contract_anchor_allows_unseeded_character_contracts() {
    let contract = StoryContract {
        premise: "都市言情，主角由 writer 自行生成，感情线慢热。".to_string(),
        themes: vec![
            "原创".to_string(),
            "人物稳定".to_string(),
            "情节递进".to_string(),
        ],
        characters: Vec::new(),
        world_rules: Vec::new(),
        style_rules: Vec::new(),
        must_avoid: Vec::new(),
        outline: "普通人在大城市成长。".to_string(),
        structured_contract_v2: NovelContractV2::default(),
        authority_contract: None,
        updated_at: String::new(),
    };

    assert!(stable_contract_anchor_present(
        &contract,
        "林晓在城市规划院的清晨发现了第一处异常。"
    ));
}

#[test]
fn manifest_character_anchors_do_not_promote_unregistered_chapter_metadata() {
    let mut manifest = NovelProjectManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        title: "问道纪".to_string(),
        title_state: TitleState::default(),
        language: "中文".to_string(),
        genre: "赛博朋克玄幻".to_string(),
        brief: String::new(),
        target_units: Some(100_000),
        chapter_unit_target: Some(5_000),
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
        delivery_advisory_windows: Vec::new(),
        truth_files: Vec::new(),
        archives: Vec::new(),
        contract: Some(StoryContract {
            premise: "一部10万字赛博朋克玄幻".to_string(),
            themes: vec!["原创".to_string(), "人物稳定".to_string()],
            characters: Vec::new(),
            world_rules: Vec::new(),
            style_rules: Vec::new(),
            must_avoid: Vec::new(),
            outline: "陆远在下城区觉醒灵觉义体。".to_string(),
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
    for number in 1..=3 {
        manifest.chapters.push(ChapterRecord {
            number,
            title: format!("第{number}章"),
            volume_id: String::new(),
            volume_title: String::new(),
            path: format!("chapters/{number:04}.md"),
            summary: "陆远与少女在下城区追查灵脉异常。".to_string(),
            unit_count: 5000,
            status: "approved".to_string(),
            key_facts: vec!["陆远的左臂义体持续过载。".to_string()],
            continuity_updates: vec!["陆远与少女继续协作。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }
    manifest.chapters.push(ChapterRecord {
        number: 4,
        title: "第4章".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0004.md".to_string(),
        summary: "苏哲在实验室里观察逻辑实体。".to_string(),
        unit_count: 5000,
        status: "needs_revision".to_string(),
        key_facts: vec!["苏哲启动实验。".to_string()],
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });

    let anchors = manifest_character_anchors(&manifest);

    assert!(!anchors.contains(&"陆远".to_string()));
    assert!(!anchors.contains(&"左臂".to_string()));
    assert!(!anchors.contains(&"关系变化".to_string()));
    assert!(!anchors.contains(&"线索进展".to_string()));
    assert!(!anchors.contains(&"苏哲".to_string()));
}

#[test]
fn inferred_character_anchors_ignore_world_terms_and_common_phrases() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 陆沉; role: 主角; desire: 改变命运; fear: 失去选择权; bottom_line: 不牺牲无辜"
                .to_string(),
        ];
    }
    for number in 1..=2 {
        manifest.chapters.push(ChapterRecord {
            number,
            title: format!("第{number}章"),
            volume_id: String::new(),
            volume_title: String::new(),
            path: format!("chapters/{number:04}.md"),
            summary: "陆沉发现识海中的余烬微光仍在燃烧。".to_string(),
            unit_count: 2600,
            status: "approved".to_string(),
            key_facts: vec![
                "陆沉意识到余烬不是灵力，而是危险的世界规则裂痕。".to_string(),
                "巡逻队留下的余光照过矿道。".to_string(),
            ],
            continuity_updates: vec!["陆沉 (主角): 仍在黑石矿区寻找生路。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }

    let anchors = manifest_character_anchors(&manifest);

    assert!(anchors.contains(&"陆沉".to_string()));
    assert!(!anchors.contains(&"余烬".to_string()));
    assert!(!anchors.contains(&"余光".to_string()));
}

#[test]
fn manifest_character_anchors_keep_primary_character_first() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 司桥遥; role: 重要角色; desire: 垄断资源".to_string(),
            "name: 谢栖遥; role: 主角; desire: 摆脱底层命运".to_string(),
            "name: 姜庭白; role: 关键关系角色; desire: 查明真相".to_string(),
        ];
    }

    let anchors = manifest_character_anchors(&manifest);

    assert_eq!(anchors.first().map(String::as_str), Some("谢栖遥"));
    assert!(anchors.contains(&"司桥遥".to_string()));
    assert!(anchors.contains(&"姜庭白".to_string()));
}

#[test]
fn chapter_quality_advises_on_supporting_character_dominance_without_hard_evidence() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.language = "zh-CN".to_string();
    manifest.chapter_unit_target = None;
    if let Some(contract) = manifest.contract.as_mut() {
        contract.characters = vec![
            "name: 谢栖遥; role: 主角; desire: 摆脱底层命运".to_string(),
            "name: 司桥遥; role: 重要角色; desire: 垄断资源".to_string(),
        ];
    }
    let chapter = ChapterRecord {
        number: 1,
        title: "折叠币".to_string(),
        volume_id: "volume-0001".to_string(),
        volume_title: "第一卷".to_string(),
        path: "chapters/0001.md".to_string(),
        summary: "谢栖遥接触折叠币。".to_string(),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["谢栖遥确认折叠币存在。".to_string()],
        continuity_updates: vec!["谢栖遥仍是主角。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let body = "谢栖遥听见折叠币响了一声。司桥遥走进旧楼，司桥遥拿起银行卡，司桥遥决定下注，司桥遥联系买家，司桥遥完成交易，司桥遥站到窗前。";

    let gate = chapter_quality_gate(&manifest, &chapter, body, &[]);

    assert!(
        gate.warnings
            .iter()
            .any(|issue| issue.contains("supporting character")),
        "warnings: {:?}",
        gate.warnings
    );
    assert!(gate.issues.is_empty());
}

#[test]
fn chapter_quality_leaves_semantic_future_boundary_decisions_to_sealed_observer() {
    use crate::tool::writing::creation_contract_model::ChapterSeedContract;

    let mut manifest = test_manifest_with_primary_character();
    let contract = manifest.contract.as_mut().expect("contract");
    let mut authority = NovelCreationContract::default();
    authority.outline.near_chapters = vec![
        ChapterSeedContract {
            number: Some(1),
            goal: "黎启洄发现被覆盖的原始共同记忆".to_string(),
            expected_turn: "私自保留记忆胶囊".to_string(),
        },
        ChapterSeedContract {
            number: Some(2),
            goal: "黎启洄前往黑市鉴定记忆胶囊来源".to_string(),
            expected_turn: "确认其源自上层区主脑核心的原始备份".to_string(),
        },
    ];
    contract.authority_contract = Some(authority);
    let chapter = ChapterRecord {
        number: 1,
        title: "蓝色胶囊".to_string(),
        volume_id: "volume-0001".to_string(),
        volume_title: "第一卷".to_string(),
        path: "chapters/0001.md".to_string(),
        summary: "黎启洄保留了记忆胶囊。".to_string(),
        unit_count: 2600,
        status: "draft".to_string(),
        key_facts: vec!["黎启洄发现原始共同记忆。".to_string()],
        continuity_updates: vec!["记忆胶囊仍未鉴定。".to_string()],
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let intent = chapter_quality_gate(
        &manifest,
        &chapter,
        "黎启洄收好胶囊，决定明天前往黑市。",
        &[],
    );
    assert!(!intent
        .findings
        .iter()
        .any(|finding| finding.code == "future_chapter_consumed"));

    let lexical_overlap = chapter_quality_gate(
        &manifest,
        &chapter,
        "黎启洄检查胶囊，发现其中的源头直指上层区的主脑核心。",
        &[],
    );
    assert!(!lexical_overlap
        .findings
        .iter()
        .any(|finding| finding.code == "future_chapter_consumed"),
        "the generic studio gate must not recreate the sealed final-body observer with lexical overlap"
    );
}

#[test]
fn manifest_character_anchors_prefer_story_bible_ledger() {
    let manifest = NovelProjectManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        title: "问道纪".to_string(),
        title_state: TitleState::default(),
        language: "中文".to_string(),
        genre: "赛博朋克玄幻".to_string(),
        brief: String::new(),
        target_units: Some(100_000),
        chapter_unit_target: Some(5_000),
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
        chapters: vec![ChapterRecord {
            number: 1,
            title: "第1章".to_string(),
            volume_id: String::new(),
            volume_title: String::new(),
            path: "chapters/0001.md".to_string(),
            summary: "陆远通过左臂感知到线索进展。".to_string(),
            unit_count: 5000,
            status: "approved".to_string(),
            key_facts: vec!["关系变化：陆远与少女继续协作。".to_string()],
            continuity_updates: vec!["陆远意识到左臂需要降温。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }],
        reviews: Vec::new(),
        review_cycles: Vec::new(),
        truth_validations: Vec::new(),
        hook_debt_reports: Vec::new(),
        delivery_advisory_windows: Vec::new(),
        truth_files: Vec::new(),
        archives: Vec::new(),
        contract: Some(StoryContract {
            premise: "一部10万字赛博朋克玄幻".to_string(),
            themes: vec!["原创".to_string()],
            characters: Vec::new(),
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
        story_bible: Some(novel_bible::StoryBible {
            character_ledger: vec![
                novel_bible::CharacterAnchor {
                    name: "陆远".to_string(),
                    ..Default::default()
                },
                novel_bible::CharacterAnchor {
                    name: "主角".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        structured_contract_v2: NovelContractV2::default(),
    };

    assert_eq!(
        manifest_character_anchors(&manifest),
        vec!["陆远".to_string()]
    );
}

#[test]
fn manifest_character_anchors_use_registered_contract_authority_only() {
    let mut manifest = NovelProjectManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        title: "尘寰逆鳞".to_string(),
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
        delivery_advisory_windows: Vec::new(),
        truth_files: Vec::new(),
        archives: Vec::new(),
        contract: Some(StoryContract {
            premise: "陆沉在异世界底层逆袭。".to_string(),
            themes: vec!["草根逆袭".to_string()],
            characters: vec![
                "name: 前世身份; role: 同伴".to_string(),
                "name: 南晏野; role: 对手".to_string(),
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
            summary: "陆沉在矿场中寻找灵屑。".to_string(),
            unit_count: 2500,
            status: "approved".to_string(),
            key_facts: vec!["陆沉确认矿道阵纹存在。".to_string()],
            continuity_updates: vec!["陆沉继续隐藏自己的感知能力。".to_string()],
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }

    let anchors = manifest_character_anchors(&manifest);

    assert!(!anchors.contains(&"陆沉".to_string()));
    assert!(anchors.contains(&"南晏野".to_string()));
    assert!(!anchors.contains(&"前世身份".to_string()));
}

#[test]
fn volume_ranges_distribute_generated_one_chapter_ranges_across_project_scale() {
    let now = Utc::now().to_rfc3339();
    let mut volumes = (1..=3)
        .map(|index| VolumeRecord {
            id: format!("volume-{index:04}"),
            title: format!("第{index}卷"),
            start_chapter: index,
            end_chapter: None,
            objective: String::new(),
            key_results: Vec::new(),
            emotional_curve: String::new(),
            must_open: Vec::new(),
            must_payoff: Vec::new(),
            ending_change: String::new(),
            status: "planned".to_string(),
            summary: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        })
        .collect::<Vec<_>>();

    project_governance::normalize_volume_ranges(&mut volumes, Some(20));

    assert_eq!(volumes[0].start_chapter, 1);
    assert_eq!(volumes[0].end_chapter, Some(7));
    assert_eq!(volumes[1].start_chapter, 8);
    assert_eq!(volumes[1].end_chapter, Some(14));
    assert_eq!(volumes[2].start_chapter, 15);
    assert_eq!(volumes[2].end_chapter, Some(20));
}

#[test]
fn volume_ranges_redistribute_stale_single_volume_project_end() {
    let now = Utc::now().to_rfc3339();
    let mut volumes = (1..=5)
        .map(|index| VolumeRecord {
            id: format!("volume-{index:04}"),
            title: format!("第{index}卷"),
            start_chapter: index,
            end_chapter: (index == 1).then_some(40),
            objective: String::new(),
            key_results: Vec::new(),
            emotional_curve: String::new(),
            must_open: Vec::new(),
            must_payoff: Vec::new(),
            ending_change: String::new(),
            status: "planned".to_string(),
            summary: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        })
        .collect::<Vec<_>>();

    project_governance::normalize_volume_ranges(&mut volumes, Some(40));

    assert_eq!(
        volumes
            .iter()
            .map(|volume| (volume.start_chapter, volume.end_chapter))
            .collect::<Vec<_>>(),
        vec![
            (1, Some(8)),
            (9, Some(16)),
            (17, Some(24)),
            (25, Some(32)),
            (33, Some(40)),
        ]
    );
}

#[test]
fn volume_ranges_remove_overlaps_from_model_supplied_ranges() {
    let now = Utc::now().to_rfc3339();
    let mut volumes = [
        (1, Some(4)),
        (2, Some(8)),
        (3, Some(12)),
        (4, Some(16)),
        (5, Some(20)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| VolumeRecord {
        id: format!("volume-{:04}", index + 1),
        title: format!("第{}卷", index + 1),
        start_chapter: start,
        end_chapter: end,
        objective: String::new(),
        key_results: Vec::new(),
        emotional_curve: String::new(),
        must_open: Vec::new(),
        must_payoff: Vec::new(),
        ending_change: String::new(),
        status: "planned".to_string(),
        summary: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    })
    .collect::<Vec<_>>();

    project_governance::normalize_volume_ranges(&mut volumes, Some(20));

    assert_eq!(
        volumes
            .iter()
            .map(|volume| (volume.start_chapter, volume.end_chapter))
            .collect::<Vec<_>>(),
        vec![
            (1, Some(4)),
            (5, Some(8)),
            (9, Some(12)),
            (13, Some(16)),
            (17, Some(20))
        ]
    );
}

#[test]
fn legacy_single_volume_is_bounded_and_receives_contract_debts() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.target_units = Some(50_000);
    manifest.chapter_unit_target = Some(2_500);
    manifest.story_bible = None;
    manifest.volumes.clear();
    manifest.structured_contract_v2.payoff_matrix = vec![PayoffMatrixEntry {
        promise: "查清旧校名册被篡改的原因".to_string(),
        payoff_target: "公开证据并重建晋级规则".to_string(),
        status: "planned".to_string(),
        ..Default::default()
    }];

    project_governance::ensure_volume_records_from_story_bible(&mut manifest);

    assert_eq!(manifest.volumes.len(), 1);
    assert_eq!(manifest.volumes[0].end_chapter, Some(20));
    assert!(manifest.volumes[0]
        .must_open
        .iter()
        .any(|item| item.contains("名册")));
    assert!(manifest.volumes[0]
        .must_payoff
        .iter()
        .any(|item| item.contains("重建晋级规则")));
}

#[test]
fn normalized_manifest_volume_ranges_are_synchronized_back_to_story_bible() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.target_units = Some(100_000);
    manifest.chapter_unit_target = Some(2_500);
    manifest.volumes.clear();
    let mut bible = novel_bible::StoryBible::default();
    bible.narrative_graph.volume_arcs = (1..=3)
        .map(|index| novel_bible::NarrativeArc {
            id: if index == 1 {
                String::new()
            } else {
                format!("volume-{index:04}")
            },
            title: format!("第{index}卷"),
            goal: format!("第{index}卷目标"),
            start_chapter: Some(index),
            end_chapter: None,
            resolves_toward: format!("第{index}卷变化"),
        })
        .collect();
    manifest.story_bible = Some(bible);

    project_governance::ensure_volume_records_from_story_bible(&mut manifest);

    let expected = vec![(1, Some(14)), (15, Some(28)), (29, Some(40))];
    assert_eq!(
        manifest
            .volumes
            .iter()
            .map(|volume| (volume.start_chapter, volume.end_chapter))
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        manifest
            .story_bible
            .as_ref()
            .expect("story bible")
            .narrative_graph
            .volume_arcs
            .iter()
            .map(|arc| (arc.start_chapter.expect("start"), arc.end_chapter))
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        manifest
            .story_bible
            .as_ref()
            .expect("story bible")
            .narrative_graph
            .volume_arcs
            .iter()
            .map(|arc| arc.id.as_str())
            .collect::<Vec<_>>(),
        vec!["volume-0001", "volume-0002", "volume-0003"]
    );
}

#[test]
fn narrative_progress_restricts_expansion_during_convergence() {
    let mut manifest = test_manifest_with_primary_character();
    manifest.target_units = Some(10_000);
    manifest.chapter_unit_target = Some(2_500);
    manifest.chapters.push(ChapterRecord {
        number: 1,
        title: "旧账入夜".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: "主角取得旧账证据。".to_string(),
        unit_count: 7_500,
        status: "approved".to_string(),
        key_facts: vec!["旧账证据已经取得。".to_string()],
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });

    let progress = context_packaging::narrative_progress_contract(&manifest, 4);

    assert_eq!(progress["phase"], "convergence");
    assert_eq!(progress["expansion_policy"], "restricted");
    assert_eq!(progress["progress_percent"], 75);
}

#[test]
fn contract_previous_names_become_forbidden_authority_and_are_repaired() {
    use crate::tool::writing::creation_contract_model::CharacterContract;

    let mut manifest = test_manifest_with_primary_character();
    let primary = CharacterContract {
        character_id: "protagonist".to_string(),
        canonical_name: "谢知原".to_string(),
        previous_names: vec!["沈算盘".to_string(), "岑怀川".to_string()],
        role: "女主角".to_string(),
        desire: "查清官镖失踪真相".to_string(),
        fear: "证据再次被权势淹没".to_string(),
        bottom_line: "不伪造账目".to_string(),
        name_source: "contract_authority".to_string(),
        ..CharacterContract::default()
    };
    let contract = manifest.contract.as_mut().expect("contract");
    contract.characters = vec![primary.to_draft_line()];
    let mut authority = NovelCreationContract::default();
    authority.characters = vec![primary];
    contract.authority_contract = Some(authority);

    ensure_character_authority_ledger(&mut manifest);

    let ledger = manifest
        .character_ledger
        .iter()
        .find(|character| character.canonical_name == "谢知原")
        .expect("canonical character ledger");
    assert_eq!(
        ledger.forbidden_renames,
        vec!["岑怀川".to_string(), "沈算盘".to_string()]
    );
    assert!(ledger
        .identity_markers
        .contains(&"pronoun_profile:feminine".to_string()));
    let chapter = ChapterRecord {
        number: 1,
        title: "旧账".to_string(),
        volume_id: String::new(),
        volume_title: String::new(),
        path: "chapters/0001.md".to_string(),
        summary: String::new(),
        unit_count: 0,
        status: "draft".to_string(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let raw = "沈算盘翻开盐引账册。";
    assert!(contract_character_drift_issues(&manifest, &chapter, raw)
        .iter()
        .any(|issue| issue.contains("沈算盘") && issue.contains("谢知原")));
    assert_eq!(
        repair_contract_character_name_typos(&manifest, raw),
        "谢知原翻开盐引账册。"
    );
    let protected_location = "沈算盘馆保存着盐引旧档。";
    assert!(
        contract_character_drift_issues(&manifest, &chapter, protected_location).is_empty(),
        "a location containing an old name surface must not be treated as the old character"
    );
    assert_eq!(
        repair_contract_character_name_typos(&manifest, protected_location),
        protected_location
    );

    let ledger = manifest
        .character_ledger
        .iter_mut()
        .find(|character| character.canonical_name == "谢知原")
        .expect("canonical character ledger");
    ledger.forbidden_renames.push("骸骨".to_string());
    ledger.forbidden_renames.sort();
    ledger.forbidden_renames.dedup();

    let common_noun = "扭曲的钢筋像一具巨大的骸骨，直指灰暗的天空。";
    assert!(
        contract_character_drift_issues(&manifest, &chapter, common_noun).is_empty(),
        "an ambiguous two-character former candidate used as a common noun must not be treated as a character"
    );
    assert_eq!(
        repair_contract_character_name_typos(&manifest, common_noun),
        common_noun
    );

    let explicit_character = "对手骸骨正在操控异兽。";
    assert!(contract_character_drift_issues(&manifest, &chapter, explicit_character)
        .iter()
        .any(|issue| issue.contains("骸骨") && issue.contains("谢知原")));
    assert_eq!(
        repair_contract_character_name_typos(&manifest, explicit_character),
        "对手谢知原正在操控异兽。"
    );
}

#[test]
fn prompt_context_excludes_next_chapter_people_and_superseded_names() {
    use crate::tool::writing::creation_contract_model::{ChapterSeedContract, CharacterContract};

    let mut manifest = test_manifest_with_primary_character();
    let primary = CharacterContract {
        character_id: "protagonist".to_string(),
        canonical_name: "谢知原".to_string(),
        previous_names: vec!["沈算盘".to_string()],
        role: "女主角".to_string(),
        planned_entry: "第1章".to_string(),
        ..CharacterContract::default()
    };
    let future = CharacterContract {
        character_id: "companion".to_string(),
        canonical_name: "钟怀野".to_string(),
        previous_names: vec!["陆断骨".to_string()],
        role: "搭档".to_string(),
        planned_entry: "第2章".to_string(),
        ..CharacterContract::default()
    };
    let contract = manifest.contract.as_mut().expect("contract");
    contract.characters = vec![primary.to_draft_line(), future.to_draft_line()];
    contract.outline = "第二章谢知原才与钟怀野相遇。".to_string();
    let mut authority = NovelCreationContract::default();
    authority.characters = vec![primary, future];
    authority.outline.near_chapters = vec![
        ChapterSeedContract {
            number: Some(1),
            goal: "谢知原发现账册异常".to_string(),
            expected_turn: "决定独自追查".to_string(),
        },
        ChapterSeedContract {
            number: Some(2),
            goal: "谢知原遇到钟怀野".to_string(),
            expected_turn: "看见铁尺伤痕".to_string(),
        },
    ];
    authority.outline.raw_outline = "第二章钟怀野出场。".to_string();
    contract.authority_contract = Some(authority);
    manifest.chapter_plans = vec![ChapterPlanRecord {
        number: 1,
        title: "旧账留痕".to_string(),
        path: "plans/0001.md".to_string(),
        plan: "谢知原核对路线与盐引账目。\n## 下一章边界\n钟怀野携铁尺出场。".to_string(),
        status: "approved".to_string(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    ensure_character_authority_ledger(&mut manifest);
    manifest.structured_contract_v2.relationship_ledger =
        vec![RelationshipLedgerEntry {
            characters: vec!["谢知原".to_string(), "钟怀野".to_string()],
            relationship_type: "未来搭档".to_string(),
            start_state: "尚未相遇".to_string(),
            desired_end_state: "共同查清盐引案".to_string(),
            ..Default::default()
        }];

    let payload = build_minimal_context_payload(&manifest, 1);
    let serialized = serde_json::to_string(&payload).expect("prompt payload");
    let prompt_characters = payload
        .get("character_ledger")
        .and_then(serde_json::Value::as_array)
        .expect("character ledger");

    assert!(serialized.contains("谢知原"));
    assert!(!serialized.contains("沈算盘"));
    assert!(!serialized.contains("陆断骨"));
    assert!(!serialized.contains("下一章边界"));
    assert!(serialized.contains("next_chapter_boundary"));
    assert!(serialized.contains("钟怀野"));
    assert!(prompt_characters.iter().all(|character| {
        character
            .get("canonical_name")
            .and_then(serde_json::Value::as_str)
            != Some("钟怀野")
    }));

}

#[test]
fn prompt_story_bible_keeps_only_the_current_chapter_goal() {
    let value = serde_json::json!({
        "hook_ledger": [
            {"introduced_chapter": 1, "title": "本章已引入的异常记录"},
            {"introduced_chapter": null, "title": "第五章才揭示的证人"}
        ],
        "narrative_graph": {
            "chapter_goals": [
                {"chapter_number": 1, "goal": "核对异常记录"},
                {"chapter_number": 2, "goal": "指派监察员"},
                {"chapter_number": 3, "goal": "与维修师相遇"}
            ],
            "volume_arcs": [
                {"id": "volume-0001", "goal": "第一卷未来总变化"},
                {"id": "volume-0002", "goal": "第二卷未来总变化"}
            ]
        }
    });

    let filtered = context_packaging::relevant_story_bible_view(
        value,
        &NovelContractV2::default(),
        &context_packaging::ChapterRelevanceSelection {
            chapter_number: 1,
            names: BTreeSet::new(),
            ids: BTreeSet::new(),
            evidence: "核对异常记录".to_string(),
            current_volume_id: String::new(),
            current_volume_title: String::new(),
        },
    );
    let serialized = serde_json::to_string(&filtered).expect("story bible prompt view");

    assert!(serialized.contains("核对异常记录"));
    assert!(serialized.contains("本章已引入的异常记录"));
    assert!(!serialized.contains("指派监察员"));
    assert!(!serialized.contains("与维修师相遇"));
    assert!(!serialized.contains("第五章才揭示的证人"));
    assert!(!serialized.contains("未来总变化"));
}

#[test]
fn current_chapter_projection_keeps_relevant_tail_entries_in_both_work_views() {
    use crate::tool::writing::creation_contract_model::{
        ChapterSeedContract, CharacterContract,
    };
    use crate::tool::writing::novel_contract_v2::{
        AgeProgressionState, ArtifactLedgerEntry, CharacterProgressionState,
        CharacterVoiceProfile, EmotionalStateLedgerEntry, PayoffMatrixEntry,
        RelationshipInteractionQuota, RevealScheduleEntry,
    };

    let mut manifest = test_manifest_with_primary_character();
    let mut authority = NovelCreationContract::default();
    authority.characters = (1..=12)
        .map(|index| CharacterContract {
            character_id: format!("character-{index}"),
            canonical_name: format!("角色{index}"),
            role: if index == 1 {
                "主角".to_string()
            } else {
                "配角".to_string()
            },
            planned_entry: if index == 10 {
                "第1章".to_string()
            } else {
                "第20章".to_string()
            },
            ..CharacterContract::default()
        })
        .collect();
    authority.outline.near_chapters = vec![ChapterSeedContract {
        number: Some(1),
        goal: "角色10携带物件10核对秘密10".to_string(),
        expected_turn: "角色10决定当面作证".to_string(),
    }];
    let mut structured = NovelContractV2::default();
    for index in 1..=12 {
        let name = format!("角色{index}");
        structured.character_voice_ledger.push(CharacterVoiceProfile {
            character: name.clone(),
            voice_style: format!("声音{index}"),
            ..Default::default()
        });
        structured
            .emotional_state_ledger
            .push(EmotionalStateLedgerEntry {
                character: name.clone(),
                current_emotion: format!("情绪{index}"),
                ..Default::default()
            });
        structured
            .power_progression
            .character_current_levels
            .push(CharacterProgressionState {
                character: name.clone(),
                level: format!("层级{index}"),
                ..Default::default()
            });
        structured.time_model.age_progression.push(AgeProgressionState {
            character: name.clone(),
            current_age: index.to_string(),
            ..Default::default()
        });
        structured
            .relationship_interaction_quotas
            .push(RelationshipInteractionQuota {
                relationship: format!("关系{index}"),
                characters: vec![name],
                ..Default::default()
            });
        structured.artifact_ledger.push(ArtifactLedgerEntry {
            name: format!("物件{index}"),
            owner: format!("角色{index}"),
            status: "active".to_string(),
            ..Default::default()
        });
        structured.payoff_matrix.push(PayoffMatrixEntry {
            promise: format!("承诺{index}"),
            introduced_chapter: (index == 10).then_some(1),
            payoff_chapter: (index == 10).then_some(1),
            ..Default::default()
        });
        structured.reveal_schedule.push(RevealScheduleEntry {
            secret: format!("秘密{index}"),
            reveal_window: if index == 10 {
                "第1章".to_string()
            } else {
                "第20章".to_string()
            },
            status: "planned".to_string(),
            ..Default::default()
        });
    }
    let contract = manifest.contract.as_mut().expect("contract");
    contract.characters = authority
        .characters
        .iter()
        .map(CharacterContract::to_draft_line)
        .collect();
    contract.authority_contract = Some(authority);
    contract.structured_contract_v2 = structured.clone();
    manifest.structured_contract_v2 = structured;
    ensure_character_authority_ledger(&mut manifest);
    ensure_story_bible_from_manifest(&mut manifest);

    let payload = build_minimal_context_payload(&manifest, 1);
    let serialized = serde_json::to_string(&payload).expect("payload");
    assert!(serialized.contains("声音10"));
    assert!(serialized.contains("情绪10"));
    assert!(serialized.contains("层级10"));
    assert!(serialized.contains("物件10"));
    assert!(serialized.contains("秘密10"));
    assert!(!serialized.contains("声音9"));
    assert!(!serialized.contains("秘密9"));
    assert!(payload.pointer("/contract/structured_contract_v2/character_voice_ledger").is_some());
    assert!(payload.pointer("/story_bible/structured_contract_v2/character_voice_ledger").is_some());
}

#[test]
fn compact_prompt_context_hides_future_volume_payload() {
    let context = serde_json::json!({
        "project": {
            "title": "海雾港",
            "current_volume": {
                "title": "旧港卷",
                "objective": "本卷核对失踪船单",
                "ending_change": "未来才与监察员见面"
            },
            "volumes": [{"title": "第二卷", "objective": "未来攻入海塔"}],
            "volume_summaries": ["未来总结"]
        },
        "contract": {
            "authority_contract": {
                "outline": {
                    "volumes": [{"title": "终局卷", "ending_change": "揭示全部真相"}]
                }
            }
        }
    });

    let prompt = context_packaging::build_prompt_context_payload(&context);
    let serialized = serde_json::to_string(&prompt).expect("prompt context");

    assert!(serialized.contains("本卷核对失踪船单"));
    assert!(!serialized.contains("未来才与监察员见面"));
    assert!(!serialized.contains("未来攻入海塔"));
    assert!(!serialized.contains("揭示全部真相"));
}

#[tokio::test]
async fn chapter_control_contract_uses_only_relevant_canonical_identities() {
    use crate::tool::writing::creation_contract_model::CharacterContract;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut manifest = test_manifest_with_primary_character();
    let primary = CharacterContract {
        character_id: "protagonist".to_string(),
        canonical_name: "谢知原".to_string(),
        previous_names: vec!["沈算盘".to_string()],
        role: "女主角".to_string(),
        planned_entry: "第1章".to_string(),
        ..CharacterContract::default()
    };
    let future = CharacterContract {
        character_id: "companion".to_string(),
        canonical_name: "钟怀野".to_string(),
        role: "搭档".to_string(),
        planned_entry: "第2章".to_string(),
        ..CharacterContract::default()
    };
    let current_secondary = CharacterContract {
        character_id: "current-companion".to_string(),
        canonical_name: "裴朔".to_string(),
        role: "关键配角".to_string(),
        ..CharacterContract::default()
    };
    manifest.contract.as_mut().expect("contract").characters =
        vec![
            primary.to_draft_line(),
            current_secondary.to_draft_line(),
            future.to_draft_line(),
        ];
    manifest.story_bible = Some(novel_bible::StoryBible {
        narrative_graph: novel_bible::NarrativeGraph {
            chapter_goals: vec![novel_bible::ChapterGoal {
                chapter_number: 1,
                goal: "谢知原与裴朔核对账册。".to_string(),
                moves_toward_ending: "两人确认旧账上的同一处缺口。".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    });
    manifest.chapter_plans = vec![ChapterPlanRecord {
        number: 1,
        title: "旧账留痕".to_string(),
        path: "plans/0001.md".to_string(),
        plan: "谢知原核对账册。\n## 下一章边界\n钟怀野出场。".to_string(),
        status: "approved".to_string(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    }];
    ensure_character_authority_ledger(&mut manifest);
    manifest
        .character_ledger
        .iter_mut()
        .find(|character| character.canonical_name == "裴朔")
        .expect("current secondary authority")
        .identity_markers
        .push("pronoun_profile:masculine".to_string());

    write_chapter_control_contract(
        dir.path(),
        &manifest,
        1,
        "旧账留痕",
        "谢知原核对账册。",
        "",
        &[],
        ChapterExecutionContractV2::default(),
    )
    .await
    .expect("chapter control contract");
    let raw = tokio::fs::read_to_string(dir.path().join("runtime/chapter-0001.contract.json"))
        .await
        .expect("control contract file");

    assert!(raw.contains("canonical_identity_only: 谢知原"));
    assert!(raw.contains("pronoun/gender authority: feminine; use 她"));
    assert!(raw.contains("canonical_identity_only: 裴朔"));
    assert!(raw.contains("pronoun/gender authority: masculine; use 他"));
    assert!(!raw.contains("沈算盘"));
    assert!(!raw.contains("钟怀野"));
    assert!(!raw.contains("previous_names"));
}
