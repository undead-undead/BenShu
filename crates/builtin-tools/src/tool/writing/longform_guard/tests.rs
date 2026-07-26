use benshu_brain::runtime::continuous_task::{
    ContinuousStepRequest, ContinuousStepResult, ContinuousTaskAnchor, ContinuousTaskContract,
    ContinuousTaskStep,
};

use super::core::LongformArtifactGuard;

fn request(index: usize, anchors: &[(&str, &str)]) -> ContinuousStepRequest {
    ContinuousStepRequest {
        task_id: uuid::Uuid::new_v4(),
        objective: "continue a checkpointed artifact".to_string(),
        worker_role: "writer".to_string(),
        step: ContinuousTaskStep {
            index,
            label: format!("step-{index}"),
            instruction: "continue".to_string(),
            expected_output: None,
            depends_on: Vec::new(),
            action: Default::default(),
        },
        previous_summary: None,
        recent_checkpoint_summaries: Vec::new(),
        attempt: 0,
        previous_error: None,
        contract: (!anchors.is_empty()).then(|| ContinuousTaskContract {
            anchors: anchors
                .iter()
                .map(|(name, value)| ContinuousTaskAnchor {
                    name: (*name).to_string(),
                    value: (*value).to_string(),
                })
                .collect(),
            ..ContinuousTaskContract::default()
        }),
    }
}

fn result(output: &str) -> ContinuousStepResult {
    ContinuousStepResult {
        output: output.to_string(),
        summary: String::new(),
        artifact_uri: None,
    }
}

fn guard(total: usize) -> LongformArtifactGuard {
    LongformArtifactGuard::new(total)
}

#[test]
fn first_step_establishes_and_locks_document_title() {
    let mut guard = guard(3);
    guard
        .validate(
            &request(1, &[]),
            &result("书名：《潮汐回声》\n\n### 第一章\n正文。"),
        )
        .expect("first title should establish identity");

    let error = guard
        .validate(
            &request(2, &[]),
            &result("书名：《另一部书》\n\n### 第二章\n正文。"),
        )
        .expect_err("a later step cannot rename the artifact");
    assert!(error.to_string().contains("attempted to rename title"));
}

#[test]
fn contract_title_is_authoritative_without_requiring_repeated_metadata() {
    let mut guard = guard(3);
    guard
        .validate(
            &request(2, &[("locked_title", "潮汐回声")]),
            &result("### 第二章\n正文继续。"),
        )
        .expect("locked identity need not be repeated in every checkpoint");
}

#[test]
fn explicit_primary_anchor_rejects_conflicting_declared_metadata() {
    let mut guard = guard(3);
    let error = guard
        .validate(
            &request(
                1,
                &[
                    ("locked_title", "潮汐回声"),
                    ("locked_primary_anchor", "梁知远"),
                ],
            ),
            &result("主角：周岚\n\n### 第一章\n正文。"),
        )
        .expect_err("declared metadata must not conflict with the contract");
    assert!(error.to_string().contains("declared primary subject"));
}

#[test]
fn prose_names_are_not_guessed_or_rewritten() {
    let guard = guard(3);
    let mut output = result("### 第二章\n安宁看见施工困难，毕生都没有改名。梁知远与周岚同时在场。");
    let original = output.output.clone();

    guard.repair(
        &request(
            2,
            &[
                ("locked_title", "潮汐回声"),
                ("locked_primary_anchor", "梁知远"),
            ],
        ),
        &mut output,
    );

    assert_eq!(output.output, original);
}

#[test]
fn progress_total_is_repaired_from_runtime_plan() {
    let guard = guard(20);
    let mut output = result("书名：《潮汐回声》\n当前进度：1/12\n\n### 第一章\n正文。");

    guard.repair(&request(1, &[]), &mut output);

    assert!(output.output.contains("当前进度：1/20"));
}

#[test]
fn current_progress_must_match_checkpoint_index() {
    let mut managed_guard = guard(20);
    let error = managed_guard
        .validate(
            &request(3, &[("locked_title", "潮汐回声")]),
            &result("当前进度：2/20\n\n### 第三章\n正文。"),
        )
        .expect_err("checkpoint index is runtime authority");
    assert!(error.to_string().contains("declared current progress 2"));
}

#[test]
fn content_heading_cannot_jump_to_another_checkpoint() {
    let mut guard = guard(20);
    let error = guard
        .validate(
            &request(3, &[("locked_title", "潮汐回声")]),
            &result("### 第四章\n正文。"),
        )
        .expect_err("a step cannot emit another checkpoint heading");
    assert!(error.to_string().contains("content heading for step 4"));
}

#[test]
fn repeated_next_hook_is_rejected() {
    let mut guard = guard(3);
    guard
        .validate(
            &request(1, &[]),
            &result(
                "书名：《潮汐回声》\n\n### 第一章\n正文。\n\n连续性记录：状态变化。\n下一步钩子：旧门将在午夜打开。",
            ),
        )
        .expect("first hook should be stored");

    let error = guard
        .validate(
            &request(2, &[]),
            &result(
                "### 第二章\n正文继续。\n\n连续性记录：状态继续变化。\n下一步钩子：旧门将在午夜打开。",
            ),
        )
        .expect_err("identical hooks indicate no checkpoint progress");
    assert!(error.to_string().contains("repeated the prior next hook"));
}

#[test]
fn malformed_surface_is_blocking_but_semantic_style_is_not() {
    let mut guard = guard(3);
    let error = guard
        .validate(
            &request(1, &[("locked_title", "潮汐回声")]),
            &result("### 第一章\n正文包含替换字符�。"),
        )
        .expect_err("provider corruption must be rejected");
    assert!(error.to_string().contains("malformed text surface"));
}

#[test]
fn body_minimum_and_continuity_tail_apply_only_to_managed_longform_contracts() {
    let mut managed_guard = guard(20);
    let error = managed_guard
        .validate(
            &request(
                1,
                &[("locked_title", "潮汐回声"), ("planned_total_steps", "20")],
            ),
            &result("### 第一章\n太短。"),
        )
        .expect_err("managed longform output must not silently truncate");
    assert!(error.to_string().contains("too little body content"));

    let mut lightweight_guard = guard(20);
    lightweight_guard
        .validate(
            &request(1, &[("locked_title", "潮汐回声")]),
            &result("### 第一章\n短摘要。"),
        )
        .expect("non-longform checkpoints do not inherit the body-size contract");
}
