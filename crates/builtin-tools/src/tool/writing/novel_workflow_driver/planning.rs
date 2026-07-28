use super::*;

const ROLLING_BATCH_CHAPTER_LIMIT: usize = 3;

pub(super) const fn rolling_batch_chapter_limit() -> usize {
    ROLLING_BATCH_CHAPTER_LIMIT
}

pub(super) fn build_novel_continuous_plan(
    task: &str,
    worker_label: &str,
    project_path: &str,
    target_units: Option<usize>,
    chapter_unit_target: Option<usize>,
    chapter_count: usize,
    start_chapter: usize,
) -> ContinuousTaskPlan {
    let chapter_numbers = start_chapter..start_chapter + chapter_count.max(1);
    let steps = chapter_numbers
        .map(|chapter_number| benshu_brain::runtime::continuous_task::ContinuousTaskStep {
            index: chapter_number,
            label: format!("novel-chapter-{chapter_number}"),
            instruction: "Execute one complete governed novel chapter loop.".to_string(),
            expected_output: Some(
                "One persisted chapter with plan, context, architecture, draft, audit, truth update, validation, and approval when valid.".to_string(),
            ),
            depends_on: if chapter_number > start_chapter {
                vec![chapter_number - 1]
            } else {
                Vec::new()
            },
            action: ContinuousStepAction::Custom {
                action: "novel_workflow_chapter".to_string(),
                payload: json!({ "chapter_number": chapter_number }),
            },
        })
        .collect::<Vec<_>>();
    ContinuousTaskPlan::new(task.trim(), worker_label)
        .with_steps(steps)
        .with_policy(ContinuousTaskPolicy {
            max_steps: chapter_count.max(1),
            max_retries_per_step: super::MAX_CHAPTER_STEP_RETRY_ATTEMPTS,
            stop_on_exact_repeat: true,
            max_step_duration_secs: Some(chapter_step_duration_secs(
                chapter_unit_target,
                target_units,
            )),
            max_step_total_duration_secs: None,
        })
        .with_artifact_target(ContinuousArtifactTarget {
            uri: project_path.to_string(),
            kind: "longform_fiction_project".to_string(),
            media_type: Some("text/markdown".to_string()),
        })
        .with_contract(ContinuousTaskContract {
            invariants: vec![
                "Use the writer worker runtime model; do not open a private provider session."
                    .to_string(),
                "Keep full prose in novel_studio artifacts, not in chat progress.".to_string(),
                "Preserve story title, characters, rules, timeline, and unresolved hooks."
                    .to_string(),
            ],
            anchors: vec![
                ContinuousTaskAnchor {
                    name: "workflow_driver".to_string(),
                    value: novel_pipeline::novel_workflow_descriptor().id,
                },
                ContinuousTaskAnchor {
                    name: "project_path".to_string(),
                    value: project_path.to_string(),
                },
                ContinuousTaskAnchor {
                    name: "target_units".to_string(),
                    value: target_units
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unspecified".to_string()),
                },
            ],
            completion_criteria: vec![
                "Every planned chapter has a durable novel_studio chapter artifact.".to_string(),
                "The target unit count is a minimum scale, not a stopping point.".to_string(),
                "The project completion gate checks both approved units and narrative closure before final export.".to_string(),
            ],
            required_events: vec!["artifact.written".to_string()],
            completion_event: Some("artifact.exported".to_string()),
        })
}

pub(super) fn existing_project_turn_chapter_count(
    requested_chapter_count: usize,
    approved_units: usize,
    target_units: Option<usize>,
    chapter_unit_target: Option<usize>,
    has_unapproved_chapter: bool,
    single_chapter_turn: bool,
    project_scale_turn: bool,
) -> usize {
    let requested = requested_chapter_count.max(1);
    if single_chapter_turn {
        return 1;
    }
    if !project_scale_turn {
        // Small explicit batches are bounded turn requests. Large configured
        // counts may come from the full-project estimate and must not silently
        // turn a normal continuation into an entire-book run.
        return if requested <= super::super::creation_contract::FICTION_EXPLICIT_TURN_UNITS_MAX {
            requested
        } else {
            1
        };
    }
    let Some(target) = target_units.filter(|value| *value > 0) else {
        return requested;
    };
    if approved_units >= target {
        return 1;
    }
    let remaining = target.saturating_sub(approved_units);
    let per_chapter = chapter_unit_target
        .filter(|value| *value > 0)
        .unwrap_or_else(|| longform_policy::dynamic_chapter_unit_target(Some(target)));
    let needed = longform_policy::expected_chapter_count(remaining, per_chapter).unwrap_or(1);
    let needed = if has_unapproved_chapter {
        needed.max(1)
    } else {
        needed
    };
    if project_scale_turn && requested == 1 {
        return needed.max(1);
    }
    requested.min(needed).max(1)
}

pub(super) fn expand_chapter_count_to_explicit_target(
    start_chapter: usize,
    chapter_count: usize,
    requested_start_chapter: Option<usize>,
) -> usize {
    let Some(requested_start_chapter) =
        requested_start_chapter.filter(|chapter| *chapter > start_chapter)
    else {
        return chapter_count.max(1);
    };
    let required_span = requested_start_chapter
        .saturating_sub(start_chapter)
        .saturating_add(1);
    chapter_count.max(required_span).max(1)
}

pub(super) fn task_requests_single_chapter_turn(task: &str) -> bool {
    if task_requests_complete_narrative(task) || task_requests_project_scale_generation(task) {
        return false;
    }
    if super::super::creation_contract::creation_draft_requested_turn_units(task, "fiction")
        .is_some_and(|count| count > 1)
    {
        return false;
    }
    extract_target_chapter_number(task).is_some() || surface_requests_single_chapter_turn(task)
}

pub(super) fn task_requests_project_scale_generation(task: &str) -> bool {
    task_intent_surfaces(task).iter().any(|surface| {
        let lowered = surface.to_ascii_lowercase();
        if super::super::creation_contract::creation_draft_requests_all_remaining(
            surface, "fiction",
        ) {
            return true;
        }
        let single_chapter_turn = surface_requests_single_chapter_turn(surface);
        if super::super::creation_contract::requested_total_unit_target(surface).is_some()
            && !single_chapter_turn
        {
            return true;
        }
        [
            "全部写完",
            "完整写完",
            "写完整本",
            "写完全文",
            "写到结尾",
            "完成整本",
            "整本",
            "全书",
            "全部生成",
            "continue until complete",
            "finish the book",
            "finish the whole",
            "write all",
        ]
        .iter()
        .any(|term| {
            let matched = surface.contains(term) || lowered.contains(term);
            matched && !project_scale_match_is_negated(surface, term)
        })
    })
}

fn surface_requests_single_chapter_turn(surface: &str) -> bool {
    let lowered = surface.to_ascii_lowercase();
    surface.contains("只写下一章")
        || surface.contains("只写下章")
        || surface.contains("只写第")
        || surface.contains("只生成下一章")
        || surface.contains("只生成第")
        || surface.contains("第一章")
        || surface.contains("第1章")
        || surface.contains("第 1 章")
        || surface.contains("下一章")
        || surface.contains("下章")
        || surface.contains("本章")
        || surface.contains("当前章")
        || surface.contains("一章一章")
        || lowered.contains("next chapter")
        || lowered.contains("current chapter")
        || lowered.contains("one chapter")
        || lowered.contains("single chapter")
}

pub(super) fn task_intent_surfaces(task: &str) -> Vec<String> {
    let marked = [
        "用户最新要求：",
        "用户最新要求:",
        "用户原话：",
        "用户原话:",
        "本轮范围：",
        "本轮范围:",
    ]
    .iter()
    .filter_map(|marker| extract_marked_line(task, marker))
    .collect::<Vec<_>>();
    if marked.is_empty() {
        vec![task.to_string()]
    } else {
        marked
    }
}

fn project_scale_match_is_negated(surface: &str, term: &str) -> bool {
    let Some(idx) = surface.find(term).or_else(|| {
        surface
            .to_ascii_lowercase()
            .find(&term.to_ascii_lowercase())
    }) else {
        return false;
    };
    super::super::creation_contract::operation_term_is_negated(surface, idx)
}

pub(super) fn extract_marked_line(task: &str, marker: &str) -> Option<String> {
    let (_, tail) = task.split_once(marker)?;
    tail.lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn extract_target_chapter_number(task: &str) -> Option<usize> {
    let target = extract_marked_line(task, "目标章节：")?;
    if target.contains("未明确") || target.contains("不明确") || target.contains("未指定")
    {
        return None;
    }
    let explicit_target = target
        .split(['；', ';', '。', '.', ',', '，'])
        .next()
        .unwrap_or(&target)
        .trim();
    let after = explicit_target
        .split_once('第')
        .map(|(_, tail)| tail)
        .unwrap_or(explicit_target);
    let digits = after
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok().filter(|value| *value > 0)
}

pub(super) fn resolve_chapter_unit_target(
    explicit_worker_target: Option<usize>,
    setup_or_project_target: Option<usize>,
    total_target_units: Option<usize>,
    chapter_count: usize,
) -> Option<usize> {
    if let Some(explicit) = explicit_worker_target.filter(|value| *value > 0) {
        return longform_policy::normalize_chapter_unit_target(Some(explicit), total_target_units);
    }

    if let Some(project_target) = setup_or_project_target.filter(|value| *value > 0) {
        return longform_policy::normalize_chapter_unit_target(
            Some(project_target),
            total_target_units,
        );
    }

    if let Some(derived) =
        chapter_unit_target_from_total_and_steps(total_target_units, chapter_count)
    {
        return Some(derived);
    }

    let fallback = Some(longform_policy::step_target_chars());
    longform_policy::normalize_chapter_unit_target(fallback, total_target_units)
}

pub(super) fn sanitize_existing_project_target_update(
    requested: Option<usize>,
    existing: Option<usize>,
) -> Option<usize> {
    let requested = requested.filter(|value| *value > 0)?;
    if let Some(existing) = existing.filter(|value| *value > 0) {
        let looks_like_chapter_scale = requested <= longform_policy::normal_body_range().1;
        let would_shrink_existing_project = requested < existing;
        if looks_like_chapter_scale && would_shrink_existing_project {
            return None;
        }
    }
    Some(requested)
}

pub(super) fn chapter_unit_target_from_total_and_steps(
    total_target_units: Option<usize>,
    chapter_count: usize,
) -> Option<usize> {
    let total = total_target_units.filter(|value| *value > 0)?;
    let count = chapter_count.max(1);
    let natural = total.div_ceil(count);
    let (min_units, max_units) = longform_policy::normal_body_range();
    longform_policy::normalize_chapter_unit_target(
        Some(natural.clamp(min_units, max_units)),
        total_target_units,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn project_scale_generation_detects_total_target_and_completion_request() {
        let task = "用户要求继续当前写作项目。不要重新规划合同，不要新开项目。\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\
用户原话：继续完成当前这本小说，按照当前合同和已批准章节继续写到至少5万字，最后要有完整结局。\n\
用户最新要求：继续完成当前这本小说，按照当前合同和已批准章节继续写到至少5万字，最后要有完整结局。";

        assert!(super::task_requests_project_scale_generation(task));
        assert!(!super::task_requests_single_chapter_turn(task));
    }

    #[test]
    fn project_scale_generation_reads_authoritative_turn_scope_after_short_approval() {
        let task = "用户已经确认小说创作草案。\n\
用户最新要求：确认合同，开始写作。\n\
本轮范围：用户本轮要求直接生成完剩余内容；从当前项目进度继续，按已确认总目标推进到完成。";

        assert!(super::task_requests_project_scale_generation(task));
        assert!(!super::task_requests_single_chapter_turn(task));
    }

    #[test]
    fn project_scale_chapter_count_expands_to_remaining_target() {
        let count = super::existing_project_turn_chapter_count(
            1,
            32_000,
            Some(50_000),
            Some(2_500),
            false,
            false,
            true,
        );

        assert_eq!(count, 8);
    }

    #[test]
    fn explicit_bounded_batch_is_not_collapsed_to_one_chapter() {
        let count = super::existing_project_turn_chapter_count(
            3,
            2_500,
            Some(100_000),
            Some(2_500),
            false,
            false,
            false,
        );

        assert_eq!(count, 3);
    }

    #[test]
    fn explicit_ten_chapter_batch_is_preserved_for_rolling_execution() {
        let count = super::existing_project_turn_chapter_count(
            10,
            0,
            Some(100_000),
            Some(2_500),
            false,
            false,
            false,
        );

        assert_eq!(count, 10);
        assert_eq!(super::rolling_batch_chapter_limit(), 3);
    }

    #[test]
    fn explicit_batch_count_wins_over_ordinal_range_markers() {
        let task = "请继续写后续7章，从第4章开始一直写到第10章；每章 approved 后再进入下一章。";

        assert!(!super::task_requests_single_chapter_turn(task));
        assert_eq!(
            super::super::super::creation_contract::creation_draft_requested_turn_units(
                task, "fiction"
            ),
            Some(7)
        );
    }
}
