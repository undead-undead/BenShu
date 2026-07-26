use std::path::Path;

use super::{
    copy_dir_recursive, copy_file_if_exists, now_iso, NovelProjectManifest, SnapshotRecord,
    AUTO_SNAPSHOT_CHAPTER_INTERVAL,
};

const SNAPSHOT_ROOT_FILES: &[&str] = &["project.json", "README.md", "contract.md", "continuity.md"];
const SNAPSHOT_STATE_DIRS: &[&str] = &[
    "sources", "chapters", "plans", "reviews", "truth", "runtime", "exports", "archives",
];

pub(super) async fn copy_project_state_for_snapshot(
    project_dir: &Path,
    target: &Path,
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(target).await?;
    for file in SNAPSHOT_ROOT_FILES {
        copy_file_if_exists(&project_dir.join(file), &target.join(file)).await?;
    }
    for dir in SNAPSHOT_STATE_DIRS {
        let source = project_dir.join(dir);
        if source.exists() {
            copy_dir_recursive(&source, &target.join(dir)).await?;
        }
    }
    Ok(())
}

pub(super) fn should_write_auto_chapter_snapshot(chapter_number: usize) -> bool {
    chapter_number == 1
        || (AUTO_SNAPSHOT_CHAPTER_INTERVAL > 0
            && chapter_number % AUTO_SNAPSHOT_CHAPTER_INTERVAL == 0)
}

pub(super) async fn upsert_auto_chapter_snapshot(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<SnapshotRecord> {
    let id = format!("chapter-{chapter_number:04}-approved");
    let path = format!("snapshots/{id}");
    let target = project_dir.join(&path);
    if target.exists() {
        tokio::fs::remove_dir_all(&target).await?;
    }
    copy_project_state_for_auto_snapshot(project_dir, &target).await?;
    let snapshot = SnapshotRecord {
        id,
        path,
        reason: format!(
            "automatic lightweight checkpoint after approving chapter {chapter_number}"
        ),
        created_at: now_iso(),
    };
    manifest.snapshots.retain(|item| item.id != snapshot.id);
    manifest.snapshots.push(snapshot.clone());
    manifest
        .snapshots
        .sort_by(|left, right| left.id.cmp(&right.id));
    Ok(snapshot)
}

pub(super) async fn copy_project_state_from_snapshot(
    source: &Path,
    project_dir: &Path,
) -> anyhow::Result<()> {
    let transaction_id = uuid::Uuid::new_v4().simple().to_string();
    let staging = project_dir.join(format!(".restore-stage-{transaction_id}"));
    let backup = project_dir.join(format!(".restore-backup-{transaction_id}"));
    tokio::fs::create_dir_all(&staging).await?;
    tokio::fs::create_dir_all(&backup).await?;

    for file in SNAPSHOT_ROOT_FILES {
        copy_file_if_exists(&source.join(file), &staging.join(file)).await?;
    }
    for dir in SNAPSHOT_STATE_DIRS {
        let snapshot_dir = source.join(dir);
        if snapshot_dir.exists() {
            copy_dir_recursive(&snapshot_dir, &staging.join(dir)).await?;
        }
    }
    let staged_manifest = tokio::fs::read_to_string(staging.join("project.json")).await?;
    let staged_manifest = serde_json::from_str::<NovelProjectManifest>(&staged_manifest)
        .map_err(|error| anyhow::anyhow!("snapshot project.json is invalid: {error}"))?;
    validate_staged_approval_dependencies(&staging, &staged_manifest).await?;

    let managed = SNAPSHOT_ROOT_FILES
        .iter()
        .chain(SNAPSHOT_STATE_DIRS.iter())
        .copied()
        .collect::<Vec<_>>();
    let mut backed_up = Vec::new();
    for name in &managed {
        let current = project_dir.join(name);
        if !current.exists() {
            continue;
        }
        if let Err(error) = tokio::fs::rename(&current, backup.join(name)).await {
            restore_backed_up_entries(project_dir, &backup, &backed_up).await;
            let _ = tokio::fs::remove_dir_all(&staging).await;
            let _ = tokio::fs::remove_dir_all(&backup).await;
            return Err(error.into());
        }
        backed_up.push(*name);
    }

    let mut installed = Vec::new();
    for name in &managed {
        let staged = staging.join(name);
        if !staged.exists() {
            continue;
        }
        if let Err(error) = tokio::fs::rename(&staged, project_dir.join(name)).await {
            remove_installed_entries(project_dir, &installed).await;
            restore_backed_up_entries(project_dir, &backup, &backed_up).await;
            let _ = tokio::fs::remove_dir_all(&staging).await;
            let _ = tokio::fs::remove_dir_all(&backup).await;
            return Err(error.into());
        }
        installed.push(*name);
    }

    if installed.is_empty() {
        restore_backed_up_entries(project_dir, &backup, &backed_up).await;
        let _ = tokio::fs::remove_dir_all(&staging).await;
        let _ = tokio::fs::remove_dir_all(&backup).await;
        anyhow::bail!("snapshot did not contain any managed project state");
    }

    let _ = tokio::fs::remove_dir_all(&staging).await;
    tokio::fs::remove_dir_all(&backup).await?;
    Ok(())
}

async fn validate_staged_approval_dependencies(
    staging: &Path,
    manifest: &NovelProjectManifest,
) -> anyhow::Result<()> {
    let latest_approved = manifest
        .chapters
        .iter()
        .filter(|chapter| super::chapter_is_approved(chapter))
        .map(|chapter| chapter.number)
        .max();
    for chapter in manifest
        .chapters
        .iter()
        .filter(|chapter| super::chapter_is_approved(chapter))
    {
        let receipt = super::read_approval_receipt(staging, chapter.number)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "snapshot approved chapter {} has no approval receipt",
                    chapter.number
                )
            })?;
        let raw = tokio::fs::read_to_string(staging.join(&chapter.path)).await?;
        let body = super::normalize_chapter_body_for_record(
            &super::strip_frontmatter(&raw),
            &chapter.title,
        );
        let body_fingerprint = super::chapter_quality::chapter_body_fingerprint(&body);
        if receipt.body_fingerprint != body_fingerprint {
            anyhow::bail!(
                "snapshot approval receipt for chapter {} does not match its body",
                chapter.number
            );
        }
        if receipt.legacy {
            continue;
        }
        let authority = super::read_sealed_chapter_authority(staging, manifest, chapter.number)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "snapshot approved chapter {} authority is invalid: {error}",
                    chapter.number
                )
            })?;
        if receipt.authority_fingerprint != authority.authority_root_fingerprint {
            anyhow::bail!(
                "snapshot approval receipt for chapter {} does not match sealed authority",
                chapter.number
            );
        }
        let settlement = super::read_approved_settlement(staging, chapter.number)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "snapshot approved chapter {} has no approved settlement",
                    chapter.number
                )
            })?;
        if receipt.settlement_fingerprint
            != crate::tool::writing::novel_governance::authority_fingerprint(&settlement)
        {
            anyhow::bail!(
                "snapshot approval receipt for chapter {} does not match settlement",
                chapter.number
            );
        }
        if !super::accepted_best_candidate_matches(
            staging,
            chapter.number,
            &receipt.authority_fingerprint,
            &receipt.body_fingerprint,
        )
        .await?
        {
            anyhow::bail!(
                "snapshot approved chapter {} has no matching accepted best candidate",
                chapter.number
            );
        }
        if receipt.metadata_fingerprint
            != super::approval_transaction::chapter_metadata_fingerprint(chapter)
        {
            anyhow::bail!(
                "snapshot approval receipt for chapter {} does not match metadata",
                chapter.number
            );
        }
        let review_matches = manifest
            .reviews
            .iter()
            .rev()
            .find(|review| {
                review.chapter_number == chapter.number
                    && review.verdict == "passed"
                    && review.locally_validated
                    && review.chapter_fingerprint == receipt.body_fingerprint
                    && review.authority_fingerprint == receipt.authority_fingerprint
            })
            .is_some_and(|review| {
                crate::tool::writing::novel_governance::authority_fingerprint(review)
                    == receipt.review_fingerprint
            });
        if !review_matches {
            anyhow::bail!(
                "snapshot approval receipt for chapter {} does not match review",
                chapter.number
            );
        }
        if latest_approved == Some(chapter.number)
            && (receipt.truth_fingerprint.is_empty()
                || receipt.truth_fingerprint
                    != super::approval_transaction::approval_truth_fingerprint(manifest))
        {
            anyhow::bail!(
                "snapshot approval receipt for latest chapter {} does not match current truth",
                chapter.number
            );
        }
    }
    Ok(())
}

async fn remove_installed_entries(project_dir: &Path, names: &[&str]) {
    for name in names.iter().rev() {
        let installed = project_dir.join(name);
        if installed.is_dir() {
            let _ = tokio::fs::remove_dir_all(installed).await;
        } else if installed.exists() {
            let _ = tokio::fs::remove_file(installed).await;
        }
    }
}

async fn restore_backed_up_entries(project_dir: &Path, backup: &Path, names: &[&str]) {
    for name in names.iter().rev() {
        let previous = backup.join(name);
        if previous.exists() {
            let _ = tokio::fs::rename(previous, project_dir.join(name)).await;
        }
    }
}

async fn copy_project_state_for_auto_snapshot(
    project_dir: &Path,
    target: &Path,
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(target).await?;
    for file in SNAPSHOT_ROOT_FILES {
        copy_file_if_exists(&project_dir.join(file), &target.join(file)).await?;
    }
    for dir in [
        "sources", "chapters", "plans", "reviews", "truth", "runtime", "archives",
    ] {
        let source = project_dir.join(dir);
        if source.exists() {
            copy_dir_recursive(&source, &target.join(dir)).await?;
        }
    }
    Ok(())
}
