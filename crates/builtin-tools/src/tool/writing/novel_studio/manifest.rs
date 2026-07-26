use super::{
    ChapterContractRecord, ContextPackageRecord, HookDebtReportRecord, LongformArchiveRecord,
    NovelProjectManifest, TruthFileRecord, TruthValidationRecord,
};

pub(super) fn project_total_units(manifest: &NovelProjectManifest) -> usize {
    manifest
        .chapters
        .iter()
        .map(|chapter| chapter.unit_count)
        .sum()
}

pub(super) fn upsert_truth_record(manifest: &mut NovelProjectManifest, record: TruthFileRecord) {
    manifest
        .truth_files
        .retain(|truth| truth.section != record.section);
    manifest.truth_files.push(record);
    manifest
        .truth_files
        .sort_by(|left, right| left.section.cmp(&right.section));
}

pub(super) fn upsert_archive_record(
    manifest: &mut NovelProjectManifest,
    record: LongformArchiveRecord,
) {
    manifest.archives.retain(|archive| {
        archive.kind != record.kind
            || archive.range_start != record.range_start
            || archive.range_end != record.range_end
    });
    manifest.archives.push(record);
    manifest
        .archives
        .sort_by_key(|archive| (archive.range_start, archive.range_end, archive.kind.clone()));
}

pub(super) fn upsert_chapter_contract_record(
    manifest: &mut NovelProjectManifest,
    record: ChapterContractRecord,
) {
    manifest
        .chapter_contracts
        .retain(|item| item.number != record.number);
    manifest.chapter_contracts.push(record);
    manifest.chapter_contracts.sort_by_key(|item| item.number);
}

pub(super) fn upsert_context_package_record(
    manifest: &mut NovelProjectManifest,
    record: ContextPackageRecord,
) {
    manifest
        .context_packages
        .retain(|item| item.number != record.number);
    manifest.context_packages.push(record);
    manifest.context_packages.sort_by_key(|item| item.number);
}

pub(super) fn require_sealed_chapter_authority(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<&ContextPackageRecord> {
    let record = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter_number && record.sealed)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chapter {chapter_number} has no sealed authority; normal prose persistence is blocked"
            )
        })?;
    if record.authority_root_fingerprint.trim().is_empty()
        || !record
            .protected_coverage
            .get("complete")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        anyhow::bail!(
            "chapter {chapter_number} sealed authority has incomplete protected coverage"
        );
    }
    Ok(record)
}

pub(super) async fn read_sealed_chapter_authority(
    project_dir: &std::path::Path,
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> anyhow::Result<crate::tool::writing::novel_governance::SealedChapterAuthority> {
    let record = require_sealed_chapter_authority(manifest, chapter_number)?;
    let raw = tokio::fs::read_to_string(project_dir.join(&record.path)).await?;
    let authority = serde_json::from_str::<
        crate::tool::writing::novel_governance::SealedChapterAuthority,
    >(&raw)?;
    if authority.schema_version
        != crate::tool::writing::novel_governance::sealed_authority_version()
        || authority.chapter_number != chapter_number
        || authority.authority_root_fingerprint != record.authority_root_fingerprint
    {
        anyhow::bail!(
            "chapter {chapter_number} sealed authority does not match its manifest record"
        );
    }
    Ok(authority)
}

pub(super) fn upsert_truth_validation_record(
    manifest: &mut NovelProjectManifest,
    record: TruthValidationRecord,
) {
    manifest
        .truth_validations
        .retain(|item| item.chapter_number != record.chapter_number);
    manifest.truth_validations.push(record);
    manifest
        .truth_validations
        .sort_by_key(|item| item.chapter_number);
}

pub(super) fn upsert_hook_debt_report_record(
    manifest: &mut NovelProjectManifest,
    record: HookDebtReportRecord,
) {
    manifest
        .hook_debt_reports
        .retain(|item| item.chapter_number != record.chapter_number);
    manifest.hook_debt_reports.push(record);
    manifest
        .hook_debt_reports
        .sort_by_key(|item| item.chapter_number);
}
