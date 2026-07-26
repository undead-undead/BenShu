use super::*;
use crate::tool::writing::longform_guard::{LongformArtifactGuard, LongformArtifactGuardedRunner};
use crate::tool::writing::longform_policy::{self, LongformContinuationSeed};
use async_trait::async_trait;

pub(crate) struct DelegateContinuousActionHandler {
    pub(crate) coordinator: Weak<Coordinator>,
    pub(crate) artifact_uri: String,
}

enum DelegateContinuousCheckpointSink {
    File(FileAppendCheckpointSink),
    Persistent(PersistentTaskCheckpointSink<FileAppendCheckpointSink>),
}

impl DelegateContinuousCheckpointSink {
    async fn initialize(&self, content: impl AsRef<str>) -> anyhow::Result<()> {
        match self {
            Self::File(sink) => sink.initialize(content).await,
            Self::Persistent(sink) => sink.inner_ref().initialize(content).await,
        }
    }
}

#[async_trait]
impl benshu_brain::runtime::continuous_task::ContinuousCheckpointSink
    for DelegateContinuousCheckpointSink
{
    async fn record_step(
        &mut self,
        request: &ContinuousStepRequest,
        result: &ContinuousStepResult,
    ) -> anyhow::Result<Option<String>> {
        let public_result = ContinuousStepResult {
            output: DelegateTool::longform_public_artifact_output(&result.output),
            summary: result.summary.clone(),
            artifact_uri: result.artifact_uri.clone(),
        };
        match self {
            Self::File(sink) => sink.record_step(request, &public_result).await,
            Self::Persistent(sink) => sink.record_step(request, &public_result).await,
        }
    }

    async fn record_step_attempt(
        &mut self,
        request: &ContinuousStepRequest,
        status: &str,
        reason: &str,
        output_preview: Option<&str>,
    ) -> anyhow::Result<()> {
        match self {
            Self::File(sink) => {
                sink.record_step_attempt(request, status, reason, output_preview)
                    .await
            }
            Self::Persistent(sink) => {
                sink.record_step_attempt(request, status, reason, output_preview)
                    .await
            }
        }
    }
}

#[async_trait]
impl ContinuousActionHandler for DelegateContinuousActionHandler {
    async fn execute_action(
        &mut self,
        action: ContinuousStepAction,
        request: ContinuousStepRequest,
    ) -> anyhow::Result<ContinuousStepResult> {
        match action {
            ContinuousStepAction::Delegate { role, task } => {
                let output = self.run_worker_step(&role, &task, &request).await?;
                Ok(ContinuousStepResult {
                    summary: Self::summarize_step_output(&output, &request.step.label),
                    output,
                    artifact_uri: Some(self.artifact_uri.clone()),
                })
            }
            ContinuousStepAction::Model { prompt } => {
                let role = request.worker_role.clone();
                let output = self.run_worker_step(&role, &prompt, &request).await?;
                Ok(ContinuousStepResult {
                    summary: Self::summarize_step_output(&output, &request.step.label),
                    output,
                    artifact_uri: Some(self.artifact_uri.clone()),
                })
            }
            ContinuousStepAction::Custom { action, .. } if action == "longform_chapter" => {
                anyhow::bail!(
                    "legacy hardcoded longform_chapter action is disabled; use Delegate or Model steps so the worker generates content from the live task context"
                )
            }
            ContinuousStepAction::Custom { action, .. } => {
                anyhow::bail!("unsupported delegate continuous custom action: {action}")
            }
            other => anyhow::bail!("unsupported delegate continuous action: {other:?}"),
        }
    }
}

impl DelegateContinuousActionHandler {
    async fn run_worker_step(
        &self,
        role: &str,
        task: &str,
        request: &ContinuousStepRequest,
    ) -> anyhow::Result<String> {
        let coordinator = self
            .coordinator
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Coordinator has been dropped"))?;
        let target_role = DelegateTool::resolve_target_role_for_task(&coordinator, role, task);
        let agent = coordinator
            .get_or_spawn(&target_role)
            .await
            .ok_or_else(|| anyhow::anyhow!("No agent registered for role: {:?}", target_role))?;
        let prompt = Self::build_step_prompt(task, request);
        let step_max_tokens = Self::continuous_step_output_token_budget(request);
        let output = agent
            .generate_text_only_with_max_tokens(&prompt, Some(step_max_tokens))
            .await?;
        if !output.trim().is_empty() {
            return Ok(output);
        }

        let recovery_prompt = Self::build_empty_step_recovery_prompt(task, request);
        let recovered = agent
            .generate_text_only_with_max_tokens(&recovery_prompt, Some(512))
            .await?;
        if recovered.trim().is_empty() {
            anyhow::bail!(
                "worker {} returned an empty continuous step output",
                target_role.name()
            );
        }
        Ok(recovered)
    }

    pub(crate) fn continuous_step_output_token_budget(request: &ContinuousStepRequest) -> u64 {
        longform_policy::continuous_step_output_token_budget(request)
    }

    pub(crate) fn build_step_prompt(task: &str, request: &ContinuousStepRequest) -> String {
        longform_policy::build_continuous_step_prompt(task, request)
    }

    pub(crate) fn build_empty_step_recovery_prompt(
        task: &str,
        request: &ContinuousStepRequest,
    ) -> String {
        longform_policy::build_empty_step_recovery_prompt(task, request)
    }

    pub(crate) fn summarize_step_output(output: &str, fallback_label: &str) -> String {
        longform_policy::summarize_step_output(output, fallback_label)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RequestedChapterScope {
    pub(crate) start_chapter: Option<usize>,
    pub(crate) chapter_count: Option<usize>,
}

impl DelegateTool {
    pub(crate) fn requested_chapter_scope(task: &str) -> RequestedChapterScope {
        let user_task = Self::user_request_slice_for_phase_boundary(task);
        let mut scope = Self::requested_chapter_range(user_task)
            .or_else(|| Self::requested_single_chapter(user_task))
            .or_else(|| Self::requested_chapter_quantity(user_task))
            .unwrap_or_default();
        if scope.chapter_count.is_none() {
            scope = Self::requested_chapter_range(task)
                .or_else(|| Self::requested_single_chapter(task))
                .or_else(|| Self::requested_chapter_quantity(task))
                .unwrap_or(scope);
        }
        scope
    }

    pub(crate) fn requested_chapter_count(task: &str) -> usize {
        Self::requested_chapter_count_with_step_target(task, Self::longform_step_target_chars())
    }

    pub(crate) fn requested_chapter_count_with_step_target(
        task: &str,
        longform_step_target_chars: usize,
    ) -> usize {
        let mut best = 0usize;
        if let Some(target_chars) = Self::requested_total_text_target_chars(task) {
            let step_target = Self::requested_chapter_unit_target_chars(task)
                .unwrap_or(longform_step_target_chars)
                .max(1);
            let estimated_steps = target_chars.div_ceil(step_target).max(1);
            best = best.max(estimated_steps);
        }
        if let Some(count) = Self::requested_chapter_scope(task).chapter_count {
            if count > 1 || best == 0 || Self::single_chapter_scope_explicitly_limits_task(task) {
                return count.clamp(1, 10_000);
            }
        }
        if best == 0 && Self::requests_single_initial_longform_step(task) {
            return 1;
        }
        if best == 0
            && Self::requested_total_text_target_chars(task).is_none()
            && (Self::task_requests_local_file_continuation(task)
                || Self::task_requests_local_writing_context(task))
        {
            return 1;
        }
        if best == 0 {
            Self::default_unspecified_longform_checkpoints()
        } else {
            best.clamp(1, 10_000)
        }
    }

    fn single_chapter_scope_explicitly_limits_task(task: &str) -> bool {
        let compact: String = task.chars().filter(|ch| !ch.is_whitespace()).collect();
        if [
            "只写第一章",
            "仅写第一章",
            "只完成第一章",
            "仅完成第一章",
            "本轮写第一章",
            "这次写第一章",
            "暂时写第一章",
            "先写第一章",
            "先完成第一章",
            "每次只写1章",
            "每轮只写1章",
            "每次写1章",
            "每轮写1章",
        ]
        .iter()
        .any(|marker| {
            compact.find(marker).is_some_and(|index| {
                !Self::single_step_marker_is_conditional_fallback(&compact, index)
            })
        }) {
            return true;
        }

        let lowered = task.to_ascii_lowercase();
        [
            "only write chapter 1",
            "only write the first chapter",
            "just write chapter 1",
            "write only chapter 1",
            "first chapter only",
            "one chapter per turn",
            "one chapter this turn",
        ]
        .iter()
        .any(|marker| lowered.contains(marker))
    }

    fn single_step_marker_is_conditional_fallback(compact: &str, marker_start: usize) -> bool {
        let prefix = compact[..marker_start]
            .chars()
            .rev()
            .take(18)
            .collect::<String>();
        let prefix: String = prefix.chars().rev().collect();
        [
            "否则",
            "未指定",
            "没有指定",
            "没指定",
            "如未",
            "如果未",
            "如果没有",
            "withoutatotal",
            "ifnototal",
            "ifnotarget",
        ]
        .iter()
        .any(|marker| prefix.contains(marker))
    }

    pub(crate) fn requested_start_chapter(task: &str) -> Option<usize> {
        Self::requested_chapter_scope(task).start_chapter
    }

    fn requested_chapter_range(task: &str) -> Option<RequestedChapterScope> {
        let quantity = r"(\d{1,4}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖佰仟拾]+)";
        let patterns = [
            format!(
                r"第\s*{quantity}\s*(?:章|章节)\s*(?:到|至|－|-|—|~|～)\s*第?\s*{quantity}\s*(?:章|章节)?"
            ),
            format!(r"(?i)chapters?\s*{quantity}\s*(?:to|through|thru|－|-|—|~)\s*{quantity}"),
        ];
        for pattern in patterns {
            let regex = Regex::new(&pattern).expect("valid chapter range regex");
            for capture in regex.captures_iter(task) {
                let Some(start) = capture
                    .get(1)
                    .and_then(|m| Self::parse_chapter_number(m.as_str()))
                else {
                    continue;
                };
                let Some(end) = capture
                    .get(2)
                    .and_then(|m| Self::parse_chapter_number(m.as_str()))
                else {
                    continue;
                };
                if end >= start {
                    return Some(RequestedChapterScope {
                        start_chapter: Some(start),
                        chapter_count: Some(end - start + 1),
                    });
                }
            }
        }
        None
    }

    fn requested_single_chapter(task: &str) -> Option<RequestedChapterScope> {
        let quantity = r"(\d{1,4}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖佰仟拾]+)";
        let patterns = [
            format!(r"第\s*{quantity}\s*(?:章|章节)"),
            format!(r"(?i)chapters?\s*{quantity}\b"),
        ];
        for pattern in patterns {
            let regex = Regex::new(&pattern).expect("valid single chapter regex");
            for capture in regex.captures_iter(task) {
                if let Some(matched) = capture.get(0) {
                    let tail = task[matched.end()..].trim_start();
                    if Self::chapter_mention_is_completed_context(tail) {
                        continue;
                    }
                }
                if let Some(start) = capture
                    .get(1)
                    .and_then(|m| Self::parse_chapter_number(m.as_str()))
                {
                    return Some(RequestedChapterScope {
                        start_chapter: Some(start),
                        chapter_count: Some(1),
                    });
                }
            }
        }
        if Self::requests_single_initial_longform_step(task)
            || Self::requests_next_longform_step(task)
        {
            return Some(RequestedChapterScope {
                start_chapter: None,
                chapter_count: Some(1),
            });
        }
        None
    }

    fn chapter_mention_is_completed_context(tail: &str) -> bool {
        let compact: String = tail
            .chars()
            .take(16)
            .filter(|ch| !ch.is_whitespace())
            .collect();
        [
            "完成后",
            "完成以后",
            "写完后",
            "写完以后",
            "结束后",
            "结束以后",
            "已完成",
            "已经完成",
            "完成了",
            "写完了",
            "之后",
            "以后",
        ]
        .iter()
        .any(|marker| compact.starts_with(marker))
    }

    fn requested_chapter_quantity(task: &str) -> Option<RequestedChapterScope> {
        let quantity = r"(\d{1,4}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖佰仟拾]+)";
        let patterns = [
            format!(
                r"(?:至少|最少|不少于|完成|写|生成|创作|续写|继续写|先写|再写|连写|输出|跑)\s*{quantity}\s*(?:个)?(?:章|章节)"
            ),
            format!(r"前\s*{quantity}\s*(?:章|章节)"),
            format!(
                r"(?i)(?:at least|minimum|write|create|generate|draft|complete|continue|first)\s*{quantity}\s*chapters?"
            ),
            format!(r"(?i)\b{quantity}\s*chapters?\b"),
        ];
        let mut best = None;
        for pattern in patterns {
            let regex = Regex::new(&pattern).expect("valid chapter quantity regex");
            for capture in regex.captures_iter(task) {
                let Some(count) = capture
                    .get(1)
                    .and_then(|m| Self::parse_chapter_number(m.as_str()))
                else {
                    continue;
                };
                best = Some(best.unwrap_or(0usize).max(count));
            }
        }
        best.map(|count| RequestedChapterScope {
            start_chapter: None,
            chapter_count: Some(count),
        })
    }

    fn requests_next_longform_step(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let compact: String = task.chars().filter(|ch| !ch.is_whitespace()).collect();
        [
            "下一章",
            "下章",
            "继续下一章",
            "接着写",
            "继续写",
            "再写一章",
            "续写一章",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
            || [
                "next chapter",
                "one more chapter",
                "continue with the next chapter",
                "write the next chapter",
            ]
            .iter()
            .any(|marker| lowered.contains(marker))
    }

    fn parse_chapter_number(raw: &str) -> Option<usize> {
        let value = if raw.chars().any(|ch| ch.is_ascii_digit()) {
            Self::parse_ascii_quantity(raw)?
        } else {
            Self::parse_chinese_quantity(raw)?
        };
        if value.is_finite() && value >= 1.0 {
            Some(value.round() as usize)
        } else {
            None
        }
    }

    pub(crate) fn requests_single_initial_longform_step(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        let compact: String = task.chars().filter(|ch| !ch.is_whitespace()).collect();
        let cjk_markers = [
            "先写第一章",
            "只写第一章",
            "仅写第一章",
            "写第一章",
            "第一章",
            "首章",
            "开篇",
            "第一节",
            "第一部分",
            "第一段",
        ];
        if cjk_markers.iter().any(|marker| compact.contains(marker)) {
            return true;
        }
        let ascii_markers = [
            "first chapter",
            "opening chapter",
            "chapter one",
            "chapter 1",
            "first section",
            "opening section",
            "first part",
            "part one",
            "part 1",
            "first installment",
        ];
        ascii_markers.iter().any(|marker| lowered.contains(marker))
    }

    pub(crate) fn default_unspecified_longform_checkpoints() -> usize {
        std::env::var("BENSHU_LONGFORM_DEFAULT_CHECKPOINTS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| (1..=200).contains(value))
            .unwrap_or(1)
    }

    pub(crate) fn build_longform_continuation_plan(task: &str, path: &str) -> ContinuousTaskPlan {
        Self::build_longform_continuation_plan_with_seed(task, path, None)
    }

    pub(crate) fn longform_plan_objective(task: &str) -> &str {
        let objective = Self::artifact_source_goal_text(task).trim();
        if objective.is_empty() {
            task.trim()
        } else {
            objective
        }
    }

    pub(crate) fn build_longform_continuation_plan_with_seed(
        task: &str,
        path: &str,
        seed: Option<LongformContinuationSeed>,
    ) -> ContinuousTaskPlan {
        let objective = Self::longform_plan_objective(task);
        let chapter_count = Self::requested_chapter_count(objective);
        let seeded_identity = seed
            .as_ref()
            .map(LongformContinuationSeed::has_identity)
            .unwrap_or(false);
        let steps = (1..=chapter_count)
            .map(|index| ContinuousTaskStep {
                index,
                label: if index == 1 && !seeded_identity {
                    "artifact-identity-and-chapter-1".to_string()
                } else {
                    format!("chapter-draft-{index}")
                },
                instruction: format!(
                    "Continue the longform artifact by producing continuity-safe text draft step {index}."
                ),
                expected_output: Some(if index == 1 && !seeded_identity {
                    "A document identity block with a fresh non-hardcoded title and primary subject/core object, followed by the first complete draft section/chapter with continuity note and next hook."
                        .to_string()
                } else {
                    "One continuity-safe draft section/chapter with heading, prose, continuity note, and next hook."
                        .to_string()
                }),
                depends_on: if index > 1 {
                    vec![index - 1]
                } else {
                    Vec::new()
                },
                action: ContinuousStepAction::Delegate {
                    role: "writer".to_string(),
                    task: Self::build_longform_chapter_model_prompt(
                        index,
                        chapter_count,
                        seeded_identity,
                    ),
                },
            })
            .collect::<Vec<_>>();

        ContinuousTaskPlan::new(objective, "writer")
            .with_steps(steps)
            .with_policy(ContinuousTaskPolicy {
                max_steps: chapter_count,
                max_retries_per_step: 3,
                stop_on_exact_repeat: true,
                max_step_duration_secs: Some(300),
                max_step_total_duration_secs: None,
            })
            .with_contract(Self::build_longform_continuity_contract(
                objective,
                chapter_count,
                seed,
            ))
            .with_artifact_target(ContinuousArtifactTarget {
                uri: path.to_string(),
                kind: "longform_text".to_string(),
                media_type: Some("text/plain".to_string()),
            })
    }

    pub(crate) fn build_longform_continuity_contract(
        task: &str,
        planned_total_steps: usize,
        seed: Option<LongformContinuationSeed>,
    ) -> ContinuousTaskContract {
        let mut anchors = vec![
            ContinuousTaskAnchor {
                name: "objective".to_string(),
                value: ellipsize(task.trim(), 360),
            },
            ContinuousTaskAnchor {
                name: "planned_total_steps".to_string(),
                value: planned_total_steps.to_string(),
            },
            ContinuousTaskAnchor {
                name: "step_target_chars".to_string(),
                value: Self::longform_step_target_chars().to_string(),
            },
        ];
        if let Some(seed) = seed {
            if let Some(title) = seed.title.filter(|value| !value.trim().is_empty()) {
                anchors.push(ContinuousTaskAnchor {
                    name: "locked_title".to_string(),
                    value: title,
                });
            }
            if let Some(anchor) = seed
                .primary_anchor
                .and_then(|value| LongformArtifactGuard::normalize_primary_anchor(&value))
            {
                anchors.push(ContinuousTaskAnchor {
                    name: "locked_primary_anchor".to_string(),
                    value: anchor,
                });
            }
            if let Some(next_hook) = seed.last_next_hook.filter(|value| !value.trim().is_empty()) {
                anchors.push(ContinuousTaskAnchor {
                    name: "last_next_hook".to_string(),
                    value: ellipsize(&next_hook, 240),
                });
            }
            if let Some(context) = seed.context.filter(|value| !value.trim().is_empty()) {
                anchors.push(ContinuousTaskAnchor {
                    name: "seed_context".to_string(),
                    value: ellipsize(&context, 900),
                });
            }
        }
        ContinuousTaskContract {
            invariants: vec![
                "始终以用户原始目标为最高约束，不把中间计划当作最终产物".to_string(),
                "第一步建立的标题、主角/主体/核心对象、来源使用边界、核心规则和语气不得漂移"
                    .to_string(),
                "每一步只能推进当前 bounded step，不能跳到最终总结或停止说明".to_string(),
                "如果校验反馈指出漂移、重命名、重复或质量问题，下一次重试必须修正该问题"
                    .to_string(),
            ],
            anchors,
            completion_criteria: vec![format!(
                "完成全部 {planned_total_steps} 个 checkpoint，或返回明确 blocker，不得假完成"
            )],
            required_events: vec!["continuous.step.checkpointed".to_string()],
            completion_event: None,
        }
    }

    pub(crate) fn build_longform_chapter_model_prompt(
        index: usize,
        total: usize,
        seeded_identity: bool,
    ) -> String {
        longform_policy::build_chapter_model_prompt(index, total, seeded_identity)
    }

    pub const fn longform_step_target_chars() -> usize {
        longform_policy::step_target_chars()
    }

    pub(crate) fn previous_error_requests_smaller_step(error: &str) -> bool {
        longform_policy::previous_error_requests_smaller_step(error)
    }

    pub(crate) fn render_longform_continuation_artifact(
        _task: &str,
        existing: &str,
        chapter_blocks: &[String],
    ) -> String {
        let mut output = Self::longform_continuation_prefix(existing);

        for block in chapter_blocks {
            output.push_str(block);
        }

        output
    }

    pub(crate) fn longform_continuation_prefix(existing: &str) -> String {
        let base_existing = Self::strip_previous_longform_continuation(existing);
        let mut output = String::new();
        if !base_existing.trim().is_empty() {
            output.push_str(base_existing.trim_end());
            output.push_str("\n\n---\n\n");
        }
        output
    }

    pub fn requested_text_target_chars(task: &str) -> Option<usize> {
        let mut best = None;
        let ascii_regex = Regex::new(
            r"(?i)(\d[\d,]*(?:\.\d+)?)\s*(万|億|亿|w|k|m)?\s*(?:-|–|—)?\s*(?:字|词|words?|characters?)",
        )
        .expect("valid numeric target regex");
        for capture in ascii_regex.captures_iter(task) {
            let Some(raw_number) = capture.get(1).map(|m| m.as_str()) else {
                continue;
            };
            let Some(mut value) = Self::parse_ascii_quantity(raw_number) else {
                continue;
            };
            if let Some(unit) = capture.get(2).map(|m| m.as_str()) {
                value *= Self::quantity_scale(unit).unwrap_or(1.0);
            }
            if value.is_finite() && value >= 1.0 {
                let chars = value.round() as usize;
                best = Some(best.unwrap_or(0usize).max(chars));
            }
        }

        let chinese_regex = Regex::new(
            r"([零〇一二两三四五六七八九十百千万亿壹贰叁肆伍陆柒捌玖佰仟拾萬億]+)\s*(?:字|词|words?|characters?)",
        )
        .expect("valid Chinese numeric target regex");
        for capture in chinese_regex.captures_iter(task) {
            let Some(raw_number) = capture.get(1).map(|m| m.as_str()) else {
                continue;
            };
            if let Some(value) = Self::parse_chinese_quantity(raw_number) {
                if value.is_finite() && value >= 1.0 {
                    let chars = value.round() as usize;
                    best = Some(best.unwrap_or(0usize).max(chars));
                }
            }
        }

        best
    }

    pub(crate) fn requested_chapter_unit_target_chars(task: &str) -> Option<usize> {
        let user_task = Self::user_request_slice_for_phase_boundary(task);
        let target = Self::requested_structured_unit_target_chars(user_task)
            .or_else(|| Self::requested_scoped_unit_target_chars(user_task))
            .or_else(|| Self::requested_structured_unit_target_chars(task))
            .or_else(|| Self::requested_scoped_unit_target_chars(task));
        longform_policy::normalize_user_chapter_unit_target(target)
    }

    pub(crate) fn requested_total_text_target_chars(task: &str) -> Option<usize> {
        let user_task = Self::user_request_slice_for_phase_boundary(task);
        let from_user_task = Self::requested_structured_total_target_chars(user_task)
            .or_else(|| Self::requested_total_text_target_chars_inner(user_task));
        if from_user_task.is_some() || user_task.trim() != task.trim() {
            return from_user_task;
        }
        Self::requested_structured_total_target_chars(task)
            .or_else(|| Self::requested_total_text_target_chars_inner(task))
    }

    fn requested_structured_unit_target_chars(task: &str) -> Option<usize> {
        Self::requested_structured_target_chars(
            task,
            &[
                "chapter_unit_target",
                "section_unit_target",
                "chapter_target",
                "section_target",
                "每章目标字数档位",
                "每章目标字数",
                "每章目标",
                "单章目标",
                "章节目标",
            ],
        )
    }

    fn requested_structured_total_target_chars(task: &str) -> Option<usize> {
        Self::requested_structured_target_chars(
            task,
            &[
                "target_units",
                "total_units",
                "total_target",
                "目标规模",
                "目标字数",
                "总目标字数",
                "总字数",
            ],
        )
    }

    fn requested_structured_target_chars(task: &str, keys: &[&str]) -> Option<usize> {
        let quantity = r"(\d[\d,]*(?:\.\d+)?|[零〇一二两三四五六七八九十百千万亿壹贰叁肆伍陆柒捌玖佰仟拾萬億]+)";
        let unit = r"(万|萬|億|亿|w|k|m)?";
        let mut best = None;
        for key in keys {
            let pattern = format!(
                r"(?i){}\s*(?:[:：=]|是|为)?\s*{}\s*{}\s*(?:字|词|words?|characters?)?",
                regex::escape(key),
                quantity,
                unit
            );
            let Ok(regex) = Regex::new(&pattern) else {
                continue;
            };
            for capture in regex.captures_iter(task) {
                if let Some(chars) = Self::capture_quantity_chars(&capture, 1, 2) {
                    best = Some(best.unwrap_or(0usize).max(chars));
                }
            }
        }
        best
    }

    fn requested_scoped_unit_target_chars(task: &str) -> Option<usize> {
        let lowered = task.to_ascii_lowercase();
        let ordinal_scope = Regex::new(
            r"(?i)(?:第\s*(?:\d+|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾百千]+)\s*(?:章|节|section|chapter)|chapter\s*\d+|section\s*\d+)",
        )
        .expect("valid ordinal chapter/section scope regex");
        let mut best = None;
        for matched in ordinal_scope.find_iter(task) {
            let index = matched.start();
            let slice = &task[index..];
            let end = slice
                .char_indices()
                .find_map(|(idx, ch)| {
                    (idx > matched.as_str().len()
                        && matches!(
                            ch,
                            '。' | '，' | '；' | ';' | ',' | '\n' | '\r' | '.' | '!' | '?'
                        ))
                    .then_some(idx)
                })
                .unwrap_or_else(|| slice.len().min(96));
            if let Some(chars) = Self::requested_text_target_chars(&slice[..end]) {
                best = Some(best.unwrap_or(0usize).max(chars));
            }
        }

        let markers = [
            "每章",
            "每一章",
            "单章",
            "每节",
            "每一节",
            "单节",
            "each chapter",
            "per chapter",
            "each section",
            "per section",
        ];
        for marker in markers {
            let mut start = 0usize;
            while let Some(relative) = lowered[start..].find(marker) {
                let index = start + relative;
                let slice = &task[index..];
                let end = slice
                    .char_indices()
                    .find_map(|(idx, ch)| {
                        (idx > marker.len()
                            && matches!(
                                ch,
                                '。' | '，' | '；' | ';' | ',' | '\n' | '\r' | '.' | '!' | '?'
                            ))
                        .then_some(idx)
                    })
                    .unwrap_or_else(|| slice.len().min(96));
                if let Some(chars) = Self::requested_text_target_chars(&slice[..end]) {
                    best = Some(best.unwrap_or(0usize).max(chars));
                }
                start = index + marker.len();
                if start >= lowered.len() {
                    break;
                }
            }
        }
        best
    }

    fn requested_total_text_target_chars_inner(task: &str) -> Option<usize> {
        let scoped_markers = [
            "每章",
            "每一章",
            "单章",
            "每节",
            "每一节",
            "单节",
            "each chapter",
            "per chapter",
            "each section",
            "per section",
        ];
        let ordinal_scope = Regex::new(
            r"(?i)(?:第\s*(?:\d+|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾百千]+)\s*(?:章|节|section|chapter)|chapter\s*\d+|section\s*\d+)",
        )
        .expect("valid ordinal chapter/section scope regex");
        let mut retained = Vec::new();
        for clause in task.split(|ch| matches!(ch, '。' | '，' | '；' | ';' | ',' | '\n' | '\r'))
        {
            let lowered = clause.to_ascii_lowercase();
            if ordinal_scope.is_match(clause)
                || scoped_markers
                    .iter()
                    .any(|marker| clause.contains(marker) || lowered.contains(marker))
            {
                continue;
            }
            retained.push(clause);
        }
        let cleaned = retained.join("\n");
        Self::requested_text_target_chars(&cleaned).filter(|chars| *chars >= 100)
    }

    pub(crate) fn requested_text_max_chars(task: &str) -> Option<usize> {
        let mut candidates = Vec::new();
        let quantity = r"(\d[\d,]*(?:\.\d+)?|[零〇一二两三四五六七八九十百千万亿壹贰叁肆伍陆柒捌玖佰仟拾萬億]+)";
        let unit = r"(万|萬|億|亿|w|k|m)?";
        let target_unit = r"(?:字|词|words?|characters?)";
        let before_limit = Regex::new(&format!(
            r"(?i)(?:no more than|not more than|at most|up to|under|less than|within|maximum|max|<=|<|≤|不超过|不超|最多|至多|不多于|少于|低于|控制在|限制在)\s*{quantity}\s*{unit}\s*(?:-|–|—)?\s*{target_unit}"
        ))
        .expect("valid upper-bound target regex");
        for capture in before_limit.captures_iter(task) {
            if let Some(chars) = Self::capture_quantity_chars(&capture, 1, 2) {
                candidates.push(chars);
            }
        }

        let after_limit = Regex::new(&format!(
            r"(?i){quantity}\s*{unit}\s*(?:-|–|—)?\s*{target_unit}\s*(?:or less|or fewer|以内|之内|以下|以内完成)"
        ))
        .expect("valid postfixed upper-bound target regex");
        for capture in after_limit.captures_iter(task) {
            if let Some(chars) = Self::capture_quantity_chars(&capture, 1, 2) {
                candidates.push(chars);
            }
        }

        candidates.into_iter().min().or_else(|| {
            if Self::text_target_is_upper_bound(task) {
                Self::requested_text_target_chars(task)
            } else {
                None
            }
        })
    }

    pub(crate) fn text_target_is_upper_bound(task: &str) -> bool {
        let lowered = task.to_lowercase();
        let ascii_limit_terms = [
            "no more than",
            "not more than",
            "at most",
            "up to",
            "under",
            "less than",
            "within",
            "maximum",
            "max ",
            "<=",
            "≤",
        ];
        ascii_limit_terms.iter().any(|term| lowered.contains(term))
            || [
                "不超过",
                "不超",
                "以内",
                "之内",
                "最多",
                "至多",
                "不多于",
                "少于",
                "低于",
                "控制在",
                "限制在",
            ]
            .iter()
            .any(|term| task.contains(term))
    }

    fn capture_quantity_chars(
        capture: &regex::Captures<'_>,
        number_index: usize,
        unit_index: usize,
    ) -> Option<usize> {
        let raw_number = capture.get(number_index)?.as_str();
        let mut value = if raw_number.chars().any(|ch| ch.is_ascii_digit()) {
            Self::parse_ascii_quantity(raw_number)?
        } else {
            Self::parse_chinese_quantity(raw_number)?
        };
        if let Some(unit) = capture.get(unit_index).map(|m| m.as_str()) {
            value *= Self::quantity_scale(unit).unwrap_or(1.0);
        }
        if value.is_finite() && value >= 1.0 {
            Some(value.round() as usize)
        } else {
            None
        }
    }

    pub(crate) fn parse_ascii_quantity(raw: &str) -> Option<f64> {
        raw.replace(',', "").parse::<f64>().ok()
    }

    pub(crate) fn quantity_scale(unit: &str) -> Option<f64> {
        match unit.to_ascii_lowercase().as_str() {
            "k" => Some(1_000.0),
            "m" => Some(1_000_000.0),
            "w" => Some(10_000.0),
            _ if unit == "万" || unit == "萬" => Some(10_000.0),
            _ if unit == "亿" || unit == "億" => Some(100_000_000.0),
            _ => None,
        }
    }

    pub(crate) fn parse_chinese_quantity(raw: &str) -> Option<f64> {
        let normalized = raw
            .chars()
            .map(Self::normalize_chinese_quantity_char)
            .collect::<Option<String>>()?;
        if normalized.trim().is_empty() {
            return None;
        }

        let mut total = 0usize;
        let mut rest = normalized.as_str();
        if let Some((left, right)) = rest.split_once('亿') {
            total = total.checked_add(Self::parse_chinese_quantity_section(left)? * 100_000_000)?;
            rest = right;
        }
        if let Some((left, right)) = rest.split_once('万') {
            total = total.checked_add(Self::parse_chinese_quantity_section(left)? * 10_000)?;
            rest = right;
        }
        if !rest.is_empty() {
            total = total.checked_add(Self::parse_chinese_quantity_section(rest)?)?;
        }
        Some(total as f64)
    }

    pub(crate) fn normalize_chinese_quantity_char(ch: char) -> Option<char> {
        Some(match ch {
            '零' | '〇' => '零',
            '一' | '壹' => '一',
            '二' | '贰' | '两' => '二',
            '三' | '叁' => '三',
            '四' | '肆' => '四',
            '五' | '伍' => '五',
            '六' | '陆' => '六',
            '七' | '柒' => '七',
            '八' | '捌' => '八',
            '九' | '玖' => '九',
            '十' | '拾' => '十',
            '百' | '佰' => '百',
            '千' | '仟' => '千',
            '万' | '萬' => '万',
            '亿' | '億' => '亿',
            _ => return None,
        })
    }

    pub(crate) fn parse_chinese_quantity_section(section: &str) -> Option<usize> {
        if section.is_empty() {
            return Some(1);
        }
        let mut total = 0usize;
        let mut number = 0usize;
        for ch in section.chars() {
            match ch {
                '零' => number = 0,
                '一' => number = 1,
                '二' => number = 2,
                '三' => number = 3,
                '四' => number = 4,
                '五' => number = 5,
                '六' => number = 6,
                '七' => number = 7,
                '八' => number = 8,
                '九' => number = 9,
                '十' => {
                    total = total.checked_add(number.max(1) * 10)?;
                    number = 0;
                }
                '百' => {
                    total = total.checked_add(number.max(1) * 100)?;
                    number = 0;
                }
                '千' => {
                    total = total.checked_add(number.max(1) * 1_000)?;
                    number = 0;
                }
                _ => return None,
            }
        }
        total.checked_add(number)
    }

    pub(crate) fn build_longform_continuation_artifact(task: &str, existing: &str) -> String {
        let plan = Self::build_longform_continuation_plan(task, "memory://longform-preview");
        let chapter_blocks = plan
            .steps
            .iter()
            .map(|step| {
                format!(
                    "### 第{}个续写步骤\n\n本步骤必须由 writer worker 根据当前文件内容、上一步摘要和用户请求实时生成，不能使用代码内置剧情模板。",
                    step.index
                )
            })
            .collect::<Vec<_>>();
        Self::render_longform_continuation_artifact(task, existing, &chapter_blocks)
    }

    pub(crate) fn strip_previous_longform_continuation(existing: &str) -> String {
        const CONTINUATION_MARKER: &str = "\n\n---\n\n# BenShu 长文档连续生成批次";
        if let Some(index) = existing.find(CONTINUATION_MARKER) {
            return existing[..index].trim_end().to_string();
        }
        if let Some(index) = existing.find("\n\n---\n\n# BenShu 长篇续写批次") {
            return existing[..index].trim_end().to_string();
        }
        if let Some(index) = existing.find("# BenShu 长文档连续生成批次") {
            return existing[..index].trim_end().to_string();
        }
        if let Some(index) = existing.find("# BenShu 长篇续写批次") {
            return existing[..index].trim_end().to_string();
        }
        existing.trim_end().to_string()
    }

    pub(crate) fn task_requests_txt_output(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        lowered.contains(".txt") || lowered.contains("txt") || task.contains("文本")
    }

    pub(crate) fn managed_novel_project_dir_from_path(path: &Path) -> Option<PathBuf> {
        let mut current = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()?.to_path_buf()
        };
        loop {
            if current.join("project.json").is_file()
                && current
                    .components()
                    .any(|component| component.as_os_str() == "novels")
            {
                return Some(current);
            }
            if !current.pop() {
                return None;
            }
        }
    }

    pub(crate) fn managed_project_txt_output_path(project_dir: &Path) -> Option<PathBuf> {
        let project_name = project_dir.file_name()?.to_string_lossy();
        Some(
            project_dir
                .join("exports")
                .join(format!("{}.txt", Self::sanitize_file_stem(&project_name))),
        )
    }

    pub(crate) fn sanitize_file_stem(value: &str) -> String {
        let stem = value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric()
                    || ch == '-'
                    || ch == '_'
                    || ('\u{4e00}'..='\u{9fff}').contains(&ch)
                {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_matches('_')
            .to_string();
        if stem.is_empty() {
            "artifact".to_string()
        } else {
            stem
        }
    }

    pub(crate) fn managed_longform_seed_from_path(path: &Path) -> Option<LongformContinuationSeed> {
        let project_dir = Self::managed_novel_project_dir_from_path(path)?;
        let raw = std::fs::read_to_string(project_dir.join("project.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let title = value
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let primary_anchor = value
            .pointer("/contract/characters")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find_map(|item| {
                    let text = item.as_str()?.trim();
                    let anchor = text
                        .split([' ', '：', ':', '（', '('])
                        .next()
                        .unwrap_or(text)
                        .trim();
                    LongformArtifactGuard::normalize_primary_anchor(anchor)
                })
            });
        let mut context_lines = Vec::new();
        if let Some(title) = title.as_deref() {
            context_lines.push(format!("标题：{title}"));
        }
        if let Some(premise) = value
            .pointer("/contract/premise")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        {
            context_lines.push(format!("前提：{}", premise.trim()));
        }
        if let Some(characters) = value
            .pointer("/contract/characters")
            .and_then(|value| value.as_array())
        {
            let rendered = characters
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(8)
                .collect::<Vec<_>>()
                .join("；");
            if !rendered.is_empty() {
                context_lines.push(format!("角色：{rendered}"));
            }
        }
        if let Some(world_rules) = value
            .pointer("/contract/world_rules")
            .and_then(|value| value.as_array())
        {
            let rendered = world_rules
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(8)
                .collect::<Vec<_>>()
                .join("；");
            if !rendered.is_empty() {
                context_lines.push(format!("世界规则：{rendered}"));
            }
        }
        if let Some(chapters) = value.get("chapters").and_then(|value| value.as_array()) {
            for chapter in chapters.iter().rev().take(3).rev() {
                let number = chapter.get("number").and_then(|value| value.as_u64());
                let chapter_title = chapter.get("title").and_then(|value| value.as_str());
                let summary = chapter.get("summary").and_then(|value| value.as_str());
                let continuity = chapter
                    .get("continuity_updates")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|value| value.as_str())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .collect::<Vec<_>>()
                            .join("；")
                    })
                    .unwrap_or_default();
                let mut line = String::new();
                if let Some(number) = number {
                    line.push_str(&format!("第{number}章"));
                }
                if let Some(chapter_title) = chapter_title {
                    line.push_str(&format!("《{}》", chapter_title.trim()));
                }
                if let Some(summary) = summary.filter(|value| !value.trim().is_empty()) {
                    line.push_str(&format!(" 摘要：{}", summary.trim()));
                }
                if !continuity.is_empty() {
                    line.push_str(&format!(" 连续性：{continuity}"));
                }
                if !line.trim().is_empty() {
                    context_lines.push(line);
                }
            }
        }
        Some(LongformContinuationSeed {
            title,
            primary_anchor,
            last_next_hook: None,
            context: (!context_lines.is_empty()).then(|| context_lines.join("\n")),
        })
    }

    pub(crate) fn longform_seed_from_existing_text(
        existing: &str,
    ) -> Option<LongformContinuationSeed> {
        let title = LongformArtifactGuard::extract_document_title(existing);
        let primary_anchor = LongformArtifactGuard::extract_labeled_primary_anchor(existing);
        let last_next_hook = LongformArtifactGuard::extract_next_hook_text(existing);
        if title.is_none() && primary_anchor.is_none() && last_next_hook.is_none() {
            return None;
        }
        Some(LongformContinuationSeed {
            title,
            primary_anchor,
            last_next_hook,
            context: Some(ellipsize(existing.trim(), 900)),
        })
    }

    pub(crate) async fn write_longform_continuation_for_delegate(
        &self,
        task: &str,
        worker_label: &str,
    ) -> anyhow::Result<Option<String>> {
        if !Self::task_requests_local_file_continuation(task) {
            return Ok(None);
        }
        let Some(mut path) = Self::select_longform_artifact_path(task) else {
            return Ok(None);
        };
        if path.to_ascii_lowercase().ends_with(".pdf") {
            return Ok(None);
        }
        let current_dir = std::env::current_dir()?;
        let mut safe_path = if Path::new(&path).is_absolute() {
            PathBuf::from(&path)
        } else {
            current_dir.join(&path)
        };
        let project_seed = Self::managed_longform_seed_from_path(&safe_path);
        if Self::task_requests_txt_output(task) {
            if let Some(project_dir) = Self::managed_novel_project_dir_from_path(&safe_path) {
                if let Some(target) = Self::managed_project_txt_output_path(&project_dir) {
                    safe_path = target;
                    path = safe_path.to_string_lossy().to_string();
                }
            }
        }
        if safe_path.exists() && !Self::path_is_inside(&current_dir, &safe_path) {
            return Ok(Some(Self::workspace_boundary_blocker(
                worker_label,
                "write_file",
                &safe_path,
                &current_dir,
            )));
        }
        let existing = if safe_path.exists() {
            std::fs::read_to_string(&safe_path)?
        } else {
            String::new()
        };
        let seed = project_seed.or_else(|| Self::longform_seed_from_existing_text(&existing));
        let initial_content = if existing.trim().is_empty() {
            String::new()
        } else {
            Self::longform_continuation_prefix(&existing)
        };
        let objective = Self::longform_plan_objective(task).to_string();
        let plan = Self::build_longform_continuation_plan_with_seed(task, &path, seed.clone());
        let runner = ContinuousActionRunner::new(DelegateContinuousActionHandler {
            coordinator: self.coordinator.clone(),
            artifact_uri: path.clone(),
        });
        let mut runner =
            LongformArtifactGuardedRunner::new(runner, Self::requested_chapter_count(&objective));
        let file_sink = FileAppendCheckpointSink::new(&safe_path).with_separator("\n\n");
        let (context_task_id, context_session_id) = Self::current_runtime_task_refs();
        let supervisor_task_id = if let Some(task_manager) = self.task_manager.as_ref() {
            Self::resolve_delegate_checkpoint_task_id(
                task_manager,
                context_task_id,
                context_session_id.as_deref(),
            )
            .await
        } else {
            context_task_id
        };
        let mut sink = if let (Some(task_manager), Some(task_id)) =
            (self.task_manager.clone(), supervisor_task_id)
        {
            let persistent = PersistentTaskCheckpointSink::new(task_manager, task_id, file_sink);
            let persistent = if let Some(event_manager) = self.runtime_event_manager.clone() {
                persistent.with_event_manager(event_manager)
            } else {
                persistent
            };
            DelegateContinuousCheckpointSink::Persistent(persistent)
        } else {
            DelegateContinuousCheckpointSink::File(file_sink)
        };
        sink.initialize(initial_content).await?;
        let run: ContinuousTaskRun = ContinuousTaskExecutor
            .run_with_checkpoint_sink(plan.clone(), &mut runner, &mut sink)
            .await?;
        let gate_decision = continuous_completion_gate_decision(
            &plan,
            &run,
            &Self::runtime_events_from_continuous_run(&run),
        );
        let bytes = std::fs::metadata(&safe_path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        let status = match &gate_decision {
            ContinuousCompletionGateDecision::Complete => {
                Self::continuous_run_result_status(&run.status)
            }
            ContinuousCompletionGateDecision::Blocked { .. } => "blocked",
        };
        let blockers = match &gate_decision {
            ContinuousCompletionGateDecision::Complete => {
                Self::continuous_run_blockers(&run.status)
            }
            ContinuousCompletionGateDecision::Blocked { reason } => Some(reason.as_str()),
        }
        .map(|reason| format!("\nblockers: {reason}"))
        .unwrap_or_default();
        Ok(Some(format!(
            "status: {}\nworker: {worker_label}\nexecuted_tool: write_file\ncontinuous_task_id: {}\ncontinuous_task_status: {:?}\npath: {}\nruntime_effect: artifact.written{}\nmedia_type: text/plain\nsteps_completed: {}\nsteps_planned: {}{}\nresult:\n{}",
            status,
            plan.id,
            run.status,
            path,
            Self::artifact_format_runtime_effect_line(&path),
            run.completed_steps,
            Self::requested_chapter_count(task),
            blockers,
            format!(
                "Checkpointed {} steps and wrote {} bytes to {}",
                run.completed_steps, bytes, path
            )
        )))
    }

    pub(crate) fn select_longform_artifact_path(task: &str) -> Option<String> {
        if let Some(path) = Self::extract_write_target_path(task) {
            return Some(path);
        }
        let intent = Self::artifact_intent_surface(task);
        let surface = intent.trim();
        let surface = if surface.is_empty() {
            task.trim()
        } else {
            surface
        };
        Self::extract_write_target_path(surface)
            .or_else(|| {
                if Self::task_requests_existing_artifact_revision(surface) {
                    Self::extract_local_path(surface).map(|path| path.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .or_else(|| Self::default_generated_artifact_path(surface))
    }

    pub(crate) fn artifact_format_runtime_effect_line(path: &str) -> String {
        let lowered = path.to_ascii_lowercase();
        if lowered.ends_with(".txt") {
            "\nruntime_effect: artifact.txt".to_string()
        } else if lowered.ends_with(".md") {
            "\nruntime_effect: artifact.md".to_string()
        } else if lowered.ends_with(".pdf") {
            "\nruntime_effect: artifact.pdf".to_string()
        } else {
            String::new()
        }
    }

    pub(crate) fn longform_public_artifact_output(output: &str) -> String {
        LongformArtifactGuard::body_before_continuity_tail(output)
            .trim_end()
            .to_string()
    }

    pub(crate) fn continuous_run_result_status(status: &ContinuousTaskStatus) -> &'static str {
        match status {
            ContinuousTaskStatus::Completed => "completed",
            ContinuousTaskStatus::Paused { .. } => "paused",
            ContinuousTaskStatus::Blocked { .. } => "blocked",
            ContinuousTaskStatus::Failed { .. } => "failed",
        }
    }

    pub(crate) fn continuous_run_blockers(status: &ContinuousTaskStatus) -> Option<&str> {
        match status {
            ContinuousTaskStatus::Completed => None,
            ContinuousTaskStatus::Paused { reason }
            | ContinuousTaskStatus::Blocked { reason }
            | ContinuousTaskStatus::Failed { reason } => Some(reason.as_str()),
        }
    }

    pub(crate) fn runtime_events_from_continuous_run(
        run: &ContinuousTaskRun,
    ) -> Vec<benshu_state::RuntimeEventRecord> {
        let mut events = Vec::new();
        for checkpoint in &run.checkpoints {
            events.push(
                benshu_state::RuntimeEventRecord::new("continuous.step.checkpointed")
                    .with_task(run.task_id)
                    .with_actor("continuous_worker")
                    .with_payload(serde_json::json!({
                        "step": checkpoint.step,
                        "label": checkpoint.label,
                        "summary": checkpoint.summary,
                        "artifact_uri": checkpoint.artifact_uri,
                    })),
            );
            if let Some(uri) = &checkpoint.artifact_uri {
                events.push(
                    benshu_state::RuntimeEventRecord::new("artifact.written")
                        .with_task(run.task_id)
                        .with_actor("continuous_worker")
                        .with_payload(serde_json::json!({
                            "step": checkpoint.step,
                            "uri": uri,
                        })),
                );
            }
        }
        events
    }
}
