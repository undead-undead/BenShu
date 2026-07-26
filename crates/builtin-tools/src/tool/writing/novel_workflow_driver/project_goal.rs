use super::*;
use serde::{Deserialize, Serialize};

const PROJECT_GOAL_SCHEMA: &str = "benshu.novel_project_goal.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DurableRunStatus {
    Active,
    ExplicitlyPaused,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DurableProjectGoal {
    pub(super) schema_version: String,
    pub(super) target_units: Option<usize>,
    pub(super) chapter_unit_target: Option<usize>,
    pub(super) contract_fingerprint: String,
    pub(super) next_approved_chapter: usize,
    pub(super) run_status: DurableRunStatus,
    pub(super) explicit_pause: bool,
    pub(super) cancelled: bool,
    #[serde(default)]
    pub(super) pause_reason: String,
    pub(super) updated_at: String,
}

pub(super) async fn activate_project_goal(
    project_path: &str,
    target_units: Option<usize>,
    chapter_unit_target: Option<usize>,
    next_approved_chapter: usize,
) -> anyhow::Result<DurableProjectGoal> {
    let project_dir = Path::new(project_path);
    let contract_fingerprint = current_contract_fingerprint(project_dir).await?;
    let existing = read_project_goal(project_dir).await?;
    let mut durable_next_chapter = next_approved_chapter.max(1);
    if let Some(existing) = existing.as_ref() {
        if existing.cancelled {
            anyhow::bail!("durable project goal was cancelled; start a new governed goal");
        }
        if !existing.contract_fingerprint.is_empty()
            && existing.contract_fingerprint != contract_fingerprint
        {
            anyhow::bail!(
                "durable project goal contract fingerprint is stale; start a new governed run"
            );
        }
        durable_next_chapter = durable_next_chapter.max(existing.next_approved_chapter);
    }
    let goal = DurableProjectGoal {
        schema_version: PROJECT_GOAL_SCHEMA.to_string(),
        target_units: target_units.or_else(|| existing.as_ref().and_then(|goal| goal.target_units)),
        chapter_unit_target: chapter_unit_target
            .or_else(|| existing.as_ref().and_then(|goal| goal.chapter_unit_target)),
        contract_fingerprint,
        next_approved_chapter: durable_next_chapter,
        run_status: DurableRunStatus::Active,
        explicit_pause: false,
        cancelled: false,
        pause_reason: String::new(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    write_project_goal(project_dir, &goal).await?;
    Ok(goal)
}

pub(super) async fn update_project_goal_progress(
    project_path: &str,
    next_approved_chapter: usize,
    status: DurableRunStatus,
    pause_reason: &str,
) -> anyhow::Result<()> {
    let project_dir = Path::new(project_path);
    let Some(mut goal) = read_project_goal(project_dir).await? else {
        anyhow::bail!("durable project goal is missing")
    };
    goal.next_approved_chapter = next_approved_chapter.max(1);
    goal.run_status = status;
    goal.explicit_pause = status == DurableRunStatus::ExplicitlyPaused;
    goal.cancelled = status == DurableRunStatus::Cancelled;
    goal.pause_reason = pause_reason.trim().to_string();
    goal.updated_at = chrono::Utc::now().to_rfc3339();
    write_project_goal(project_dir, &goal).await
}

async fn current_contract_fingerprint(project_dir: &Path) -> anyhow::Result<String> {
    let raw = tokio::fs::read_to_string(project_dir.join("project.json")).await?;
    let manifest: Value = serde_json::from_str(&raw)?;
    let contract = manifest
        .pointer("/contract/authority_contract")
        .filter(|value| !value.is_null())
        .or_else(|| manifest.get("contract"))
        .or_else(|| manifest.get("structured_contract_v2"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(crate::tool::writing::novel_governance::authority_fingerprint(&contract))
}

async fn read_project_goal(project_dir: &Path) -> anyhow::Result<Option<DurableProjectGoal>> {
    let path = project_goal_path(project_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str(&raw)?))
}

async fn write_project_goal(project_dir: &Path, goal: &DurableProjectGoal) -> anyhow::Result<()> {
    let path = project_goal_path(project_dir);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("project goal path has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    let temporary = parent.join(format!(
        ".project-goal-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&temporary, serde_json::to_vec_pretty(goal)?).await?;
    tokio::fs::rename(&temporary, &path).await?;
    Ok(())
}

fn project_goal_path(project_dir: &Path) -> PathBuf {
    project_dir.join("runtime").join("project-goal.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn write_manifest(path: &Path, contract: Value) {
        tokio::fs::write(
            path.join("project.json"),
            serde_json::to_vec_pretty(&json!({"structured_contract_v2": contract}))
                .expect("manifest"),
        )
        .await
        .expect("write manifest");
    }

    #[tokio::test]
    async fn durable_goal_resumes_from_last_committed_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_manifest(dir.path(), json!({"revision": 1})).await;
        let path = dir.path().to_string_lossy();

        activate_project_goal(&path, Some(100_000), Some(2_500), 1)
            .await
            .expect("activate");
        update_project_goal_progress(&path, 4, DurableRunStatus::Active, "provider unavailable")
            .await
            .expect("checkpoint");
        let resumed = activate_project_goal(&path, Some(100_000), Some(2_500), 1)
            .await
            .expect("resume");

        assert_eq!(resumed.next_approved_chapter, 4);
        assert_eq!(resumed.run_status, DurableRunStatus::Active);
        assert!(project_goal_path(dir.path()).exists());
    }

    #[tokio::test]
    async fn durable_goal_rejects_stale_contract_fingerprint() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_manifest(dir.path(), json!({"revision": 1})).await;
        let path = dir.path().to_string_lossy();
        activate_project_goal(&path, Some(100_000), Some(2_500), 1)
            .await
            .expect("activate");

        write_manifest(dir.path(), json!({"revision": 2})).await;
        let error = activate_project_goal(&path, Some(100_000), Some(2_500), 1)
            .await
            .expect_err("stale contract must block");
        assert!(error.to_string().contains("fingerprint is stale"));
    }

    async fn assert_rolling_capacity(
        target_units: usize,
        chapter_unit_target: usize,
        expected_chapters: usize,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_manifest(
            dir.path(),
            json!({
                "target_units": target_units,
                "chapter_unit_target": chapter_unit_target
            }),
        )
        .await;
        let path = dir.path().to_string_lossy();
        let planned = existing_project_turn_chapter_count(
            1,
            0,
            Some(target_units),
            Some(chapter_unit_target),
            false,
            false,
            true,
        );
        assert_eq!(planned, expected_chapters);
        assert!(rolling_batch_chapter_limit() < expected_chapters);

        activate_project_goal(&path, Some(target_units), Some(chapter_unit_target), 1)
            .await
            .expect("activate capacity goal");

        let mut next_chapter = 1usize;
        let mut remaining = expected_chapters;
        while remaining > 0 {
            let batch_size = remaining.min(rolling_batch_chapter_limit());
            assert!(batch_size <= rolling_batch_chapter_limit());
            next_chapter += batch_size;
            remaining -= batch_size;
            update_project_goal_progress(&path, next_chapter, DurableRunStatus::Active, "")
                .await
                .expect("write rolling checkpoint");
            let resumed =
                activate_project_goal(&path, Some(target_units), Some(chapter_unit_target), 1)
                    .await
                    .expect("resume rolling checkpoint");
            assert_eq!(resumed.next_approved_chapter, next_chapter);
            assert_eq!(resumed.target_units, Some(target_units));
            assert_eq!(resumed.chapter_unit_target, Some(chapter_unit_target));
        }

        assert_eq!(next_chapter, expected_chapters + 1);
    }

    #[tokio::test]
    async fn hundred_thousand_2500_tier_has_40_chapter_rolling_capacity() {
        assert_rolling_capacity(100_000, 2_500, 40).await;
    }

    #[tokio::test]
    async fn million_5000_tier_has_200_chapter_checkpoint_recovery_capacity() {
        assert_rolling_capacity(1_000_000, 5_000, 200).await;
    }
}
