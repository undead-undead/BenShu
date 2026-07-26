use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    Json,
};
use benshu_brain::agent::memory::Memory;
use benshu_brain::prelude::Tool;
use benshu_builtin_tools::tool::{OfficeParseTool, PdfParseTool};
use benshu_engram::QuantLevel;
use benshu_inference::{
    backend::{BackendBindingDescriptor, BackendCapability, InferenceFactory},
    describe_local_model_contract, detect_windows_native_runtime_status, AccelerationProfile,
    HardwareStatus, LocalModelArtifactKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::api::llama_cpp_runtime::{
    discover_windows_llama_server_status, llama_server_status_from_restart_command,
    LlamaCppServerStatus, MIN_SUPPORTED_LLAMA_CPP_BUILD,
};
use crate::api::state::{AppError, AppState};

const SEALED_RESTORE_METADATA_PREFIX: &str = "engram.recovery.sealed_restore";
const KNOWLEDGE_IMPORT_MAX_SIZE: u64 = 20 * 1024 * 1024;
const KNOWLEDGE_IMPORT_SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "json", "rs", "toml", "yaml", "yml", "js", "ts", "tsx", "py", "sh", "html", "css",
    "xml", "csv", "pdf", "docx", "xlsx", "pptx",
];

#[derive(Serialize)]
pub struct HealthStatus {
    pub status: &'static str,
    /// Number of configured agent/worker roles that can be routed to.
    pub agent_count: usize,
    /// Number of restored or active session -> agent-role mappings.
    ///
    /// This used to be reported as `agent_count`, which made old chat history
    /// look like hundreds of running agents after gateway restart.
    pub session_agent_mapping_count: usize,
}

pub async fn health_check(State(state): State<AppState>) -> Json<HealthStatus> {
    let agent_count = state.kernel.coordinator().roles().len();
    let session_agent_mapping_count = state.kernel.coordinator().active_agents().len();
    Json(HealthStatus {
        status: "ok",
        agent_count,
        session_agent_mapping_count,
    })
}

fn local_vision_runtime_surface(state: &AppState) -> (bool, String) {
    let requested = state.app_config.read().sensory.enable_local_vision;
    if requested {
        return (
            false,
            "removed:wsl_in_process_llama_cpp_disabled".to_string(),
        );
    }
    (false, "off".to_string())
}

fn knowledge_import_supports_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| {
            KNOWLEDGE_IMPORT_SUPPORTED_EXTENSIONS
                .iter()
                .any(|allowed| *allowed == ext)
        })
}

fn knowledge_quant_level(collection: &str) -> QuantLevel {
    match collection.to_ascii_lowercase().as_str() {
        "experience" | "anti_pattern" => QuantLevel::Cold,
        "agent" | "core" | "identity" => QuantLevel::Full,
        _ => QuantLevel::Warm,
    }
}

fn knowledge_import_extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

async fn extract_knowledge_import_content(
    file_path: &Path,
    extension: &str,
) -> anyhow::Result<(String, &'static str)> {
    match extension {
        "pdf" => {
            let sensory = std::sync::Arc::new(benshu_sensory::SensoryHub::new(
                benshu_sensory::SensoryConfig::default(),
            ));
            let tool = PdfParseTool::new(None, None, sensory);
            let args = serde_json::json!({
                "path": file_path.to_string_lossy(),
                "mode": "text",
                "format": "markdown",
                "image_output": "off",
                "page_limit": 400
            });
            let content = tool.call(&args.to_string()).await?;
            if content.trim_start().starts_with(r#"{"error""#) {
                anyhow::bail!("pdf_parse failed: {}", content);
            }
            Ok((content, "pdf_parse"))
        }
        "docx" | "xlsx" | "pptx" => {
            let parsed = OfficeParseTool::parse_path(file_path)?;
            Ok((OfficeParseTool::to_markdown(&parsed), "office_parse"))
        }
        _ => {
            let metadata = std::fs::metadata(file_path)?;
            if metadata.len() > KNOWLEDGE_IMPORT_MAX_SIZE {
                anyhow::bail!(
                    "file is larger than the 20MB single-file knowledge import limit: {} bytes",
                    metadata.len()
                );
            }
            let bytes = std::fs::read(file_path)?;
            Ok((String::from_utf8_lossy(&bytes).into_owned(), "text"))
        }
    }
}

fn walk_knowledge_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), AppError> {
    let entries = std::fs::read_dir(root)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_knowledge_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn infer_file_virtual_path(
    file_path: &Path,
    folder_root: Option<&Path>,
    used_virtual_paths: &mut HashSet<String>,
) -> String {
    let preferred = folder_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .unwrap_or(file_path);
    let mut candidate = preferred.to_string_lossy().replace('\\', "/");
    if candidate.is_empty() {
        candidate = file_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_string());
    }

    if used_virtual_paths.insert(candidate.clone()) {
        return candidate;
    }

    let stem = Path::new(&candidate)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let ext = Path::new(&candidate)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{}", s))
        .unwrap_or_default();

    let mut dedup_index = 2usize;
    loop {
        let deduped = format!("{}-{}{}", stem, dedup_index, ext);
        if used_virtual_paths.insert(deduped.clone()) {
            return deduped;
        }
        dedup_index += 1;
    }
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeImportRequest {
    pub collection: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeImportReportDto {
    pub collection: String,
    pub imported_count: usize,
    pub skipped_unchanged_count: usize,
    pub skipped_unsupported_count: usize,
    pub skipped_too_large_count: usize,
    pub skipped_missing_count: usize,
    pub failed_count: usize,
    pub imported_paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeDocumentsQuery {
    #[serde(default)]
    pub collection: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeDocumentDto {
    pub collection: String,
    pub path: String,
    pub title: String,
    pub docid: String,
    pub updated_at_ms: i64,
    pub unverified: bool,
    pub source_url: Option<String>,
    pub import_source: Option<String>,
    pub lifecycle_state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeDocumentsReportDto {
    pub collection: Option<String>,
    pub documents: Vec<KnowledgeDocumentDto>,
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeDeleteRequest {
    pub collection: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeDeleteReportDto {
    pub collection: String,
    pub path: String,
    pub deleted: bool,
}

fn knowledge_document_dto(doc: benshu_engram::prelude::Document) -> KnowledgeDocumentDto {
    KnowledgeDocumentDto {
        collection: doc.collection,
        path: doc.path,
        title: doc.title,
        docid: doc.docid,
        updated_at_ms: doc.updated_at_ms,
        unverified: doc.unverified,
        source_url: doc.metadata.get("source_url").cloned(),
        import_source: doc.metadata.get("import_source").cloned(),
        lifecycle_state: doc.metadata.get("document_lifecycle_state").cloned(),
    }
}

pub async fn list_knowledge_documents(
    State(state): State<AppState>,
    Query(query): Query<KnowledgeDocumentsQuery>,
) -> Result<Json<KnowledgeDocumentsReportDto>, AppError> {
    let store = state.kernel.search_engine().engram_store();
    let mut documents = if let Some(collection) = query
        .collection
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .list_documents_in_collection(collection)
            .map_err(|error| {
                AppError(anyhow::anyhow!(
                    "failed to list knowledge documents: {}",
                    error
                ))
            })?
    } else {
        store
            .list_documents_in_collection("knowledge")
            .map_err(|error| {
                AppError(anyhow::anyhow!(
                    "failed to list knowledge documents: {}",
                    error
                ))
            })?
    };

    documents.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.collection.cmp(&right.collection))
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(Json(KnowledgeDocumentsReportDto {
        collection: query.collection,
        documents: documents.into_iter().map(knowledge_document_dto).collect(),
    }))
}

pub async fn delete_knowledge_document(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeDeleteRequest>,
) -> Result<Json<KnowledgeDeleteReportDto>, AppError> {
    let collection = payload.collection.trim();
    let path = payload.path.trim();
    if collection.is_empty() || path.is_empty() {
        return Err(AppError(anyhow::anyhow!(
            "knowledge delete requires collection and path"
        )));
    }

    let search_engine = state.kernel.search_engine();
    let existed = search_engine
        .get_by_path(collection, path)
        .map_err(|error| AppError(anyhow::anyhow!("failed to inspect document: {}", error)))?
        .is_some();
    if existed {
        search_engine
            .delete_document(collection, path)
            .map_err(|error| {
                AppError(anyhow::anyhow!(
                    "failed to delete knowledge document: {}",
                    error
                ))
            })?;
    }

    Ok(Json(KnowledgeDeleteReportDto {
        collection: collection.to_string(),
        path: path.to_string(),
        deleted: existed,
    }))
}

pub async fn import_knowledge(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeImportRequest>,
) -> Result<Json<KnowledgeImportReportDto>, AppError> {
    let collection = payload.collection.trim();
    if collection.is_empty() {
        return Err(AppError(anyhow::anyhow!(
            "knowledge import collection cannot be empty"
        )));
    }

    let folder_root = payload
        .folder
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());

    let mut targets = Vec::new();
    if let Some(root) = folder_root.as_deref() {
        if !root.exists() {
            return Err(AppError(anyhow::anyhow!(
                "knowledge import folder does not exist: {}",
                root.display()
            )));
        }
        if !root.is_dir() {
            return Err(AppError(anyhow::anyhow!(
                "knowledge import folder is not a directory: {}",
                root.display()
            )));
        }
        walk_knowledge_files(root, &mut targets)?;
    }

    for file in payload.files {
        let trimmed = file.trim();
        if trimmed.is_empty() {
            continue;
        }
        targets.push(PathBuf::from(trimmed));
    }

    if targets.is_empty() {
        return Err(AppError(anyhow::anyhow!(
            "knowledge import requires a folder or at least one file"
        )));
    }

    let mut report = KnowledgeImportReportDto {
        collection: collection.to_string(),
        imported_count: 0,
        skipped_unchanged_count: 0,
        skipped_unsupported_count: 0,
        skipped_too_large_count: 0,
        skipped_missing_count: 0,
        failed_count: 0,
        imported_paths: Vec::new(),
        warnings: Vec::new(),
    };

    let store = state.kernel.search_engine().engram_store();
    let search_engine = state.kernel.search_engine();
    let folder_root_ref = folder_root.as_deref();
    let quant_level = knowledge_quant_level(collection);
    let mut used_virtual_paths = HashSet::new();

    for file_path in targets {
        if !file_path.exists() || !file_path.is_file() {
            report.skipped_missing_count += 1;
            report.warnings.push(format!(
                "Skipped missing file: {}",
                file_path.to_string_lossy()
            ));
            continue;
        }

        if !knowledge_import_supports_path(&file_path) {
            report.skipped_unsupported_count += 1;
            report.warnings.push(format!(
                "Skipped unsupported file type: {}",
                file_path.to_string_lossy()
            ));
            continue;
        }

        let extension = knowledge_import_extension(&file_path);

        let metadata = match std::fs::metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                report.failed_count += 1;
                report.warnings.push(format!(
                    "Failed to read metadata for {}: {}",
                    file_path.to_string_lossy(),
                    error
                ));
                continue;
            }
        };

        if metadata.len() > KNOWLEDGE_IMPORT_MAX_SIZE {
            report.skipped_too_large_count += 1;
            report.warnings.push(format!(
                "Skipped file over 20MB limit: {}",
                file_path.to_string_lossy()
            ));
            continue;
        }

        let virtual_path =
            infer_file_virtual_path(&file_path, folder_root_ref, &mut used_virtual_paths);
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs().to_string())
            .unwrap_or_default();

        if let Ok(Some(existing)) = store.get_by_path(collection, &virtual_path) {
            if existing.metadata.get("mtime") == Some(&mtime) {
                report.skipped_unchanged_count += 1;
                continue;
            }
        }

        let (content, parser) = match extract_knowledge_import_content(&file_path, &extension).await
        {
            Ok(extracted) => extracted,
            Err(error) => {
                report.failed_count += 1;
                report.warnings.push(format!(
                    "Failed to extract {}: {}",
                    file_path.to_string_lossy(),
                    error
                ));
                continue;
            }
        };
        let title = file_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("untitled")
            .to_string();

        let mut doc_metadata = HashMap::new();
        doc_metadata.insert("mtime".to_string(), mtime);
        doc_metadata.insert("size".to_string(), metadata.len().to_string());
        doc_metadata.insert("import_source".to_string(), "knowledge_import".to_string());
        doc_metadata.insert("parser".to_string(), parser.to_string());
        doc_metadata.insert("extension".to_string(), extension);

        let result = search_engine.index_at_level(
            collection,
            &virtual_path,
            &title,
            content.as_str(),
            quant_level,
            false,
            doc_metadata,
        );

        match result {
            Ok(()) => {
                report.imported_count += 1;
                report.imported_paths.push(virtual_path);
            }
            Err(error) => {
                report.failed_count += 1;
                report.warnings.push(format!(
                    "Failed to import {}: {}",
                    file_path.to_string_lossy(),
                    error
                ));
            }
        }
    }

    if let Some(root) = folder_root_ref {
        match search_engine.project_hierarchy_path(root).await {
            Ok(projected) if projected > 0 => report.warnings.push(format!(
                "Hierarchy index updated with {} filesystem nodes.",
                projected
            )),
            Ok(_) => {}
            Err(error) => report.warnings.push(format!(
                "Hierarchy index update skipped for {}: {}",
                root.display(),
                error
            )),
        }
    }

    Ok(Json(report))
}

#[derive(Deserialize)]
pub struct RollbackRequest {
    pub original_path: String,
    pub backup_path: String,
}

pub async fn rollback_handler(
    State(_state): State<AppState>,
    Json(payload): Json<RollbackRequest>,
) -> Result<StatusCode, AppError> {
    let bak_manager = benshu_security::internal_backup::ShadowBak::new();
    let orig = std::path::PathBuf::from(&payload.original_path);
    let bak = std::path::PathBuf::from(&payload.backup_path);

    bak_manager
        .rollback(&orig, &bak)
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Rollback failed: {}", e)))?;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RestorePointQuery {
    pub backup_id: String,
}

#[derive(Deserialize)]
pub struct RestorePointRequest {
    pub backup_id: String,
}

#[derive(Deserialize)]
pub struct DeleteRestorePointRequest {
    pub backup_id: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize)]
pub struct RestoreReceiptQuery {
    pub backup_id: String,
    pub receipt_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestorePointFileEntryDto {
    pub label: String,
    pub relative_path: String,
    pub payload_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestorePointManifestDto {
    pub backup_id: String,
    pub product: String,
    pub contract_version: String,
    pub created_at: String,
    pub storage_root_hint: String,
    pub encryption_key_fingerprint: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<MemoryRestorePointFileEntryDto>,
}

impl From<benshu_security::MemoryRestorePointManifest> for MemoryRestorePointManifestDto {
    fn from(value: benshu_security::MemoryRestorePointManifest) -> Self {
        Self {
            backup_id: value.backup_id,
            product: value.product,
            contract_version: value.contract_version,
            created_at: value.created_at.to_rfc3339(),
            storage_root_hint: value.storage_root_hint,
            encryption_key_fingerprint: value.encryption_key_fingerprint,
            file_count: value.file_count,
            total_bytes: value.total_bytes,
            files: value
                .files
                .into_iter()
                .map(|file| MemoryRestorePointFileEntryDto {
                    label: file.label,
                    relative_path: file.relative_path,
                    payload_path: file.payload_path,
                    size_bytes: file.size_bytes,
                    sha256: file.sha256,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestoreReceiptDto {
    pub receipt_id: String,
    pub backup_id: String,
    pub restored_at: String,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub restored_files: usize,
    pub restored_bytes: u64,
}

impl From<benshu_security::MemoryRestoreReceipt> for MemoryRestoreReceiptDto {
    fn from(value: benshu_security::MemoryRestoreReceipt) -> Self {
        Self {
            receipt_id: value.receipt_id,
            backup_id: value.backup_id,
            restored_at: value.restored_at.to_rfc3339(),
            contract_version: value.contract_version,
            encryption_key_fingerprint: value.encryption_key_fingerprint,
            restored_files: value.restored_files,
            restored_bytes: value.restored_bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestoreDryRunReportDto {
    pub backup_id: String,
    pub checked_at: String,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub valid: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub restorable_files: usize,
    pub missing_payloads: Vec<String>,
    pub integrity_mismatches: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestorePolicyBasisDto {
    pub backup_id: String,
    pub decision_kind: String,
    pub policy_basis: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRestoreDeleteReportDto {
    pub backup_id: String,
    pub deleted_at: String,
    pub dry_run: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub receipt_count: usize,
}

impl From<benshu_security::MemoryRestoreDeleteReport> for MemoryRestoreDeleteReportDto {
    fn from(value: benshu_security::MemoryRestoreDeleteReport) -> Self {
        Self {
            backup_id: value.backup_id,
            deleted_at: value.deleted_at.to_rfc3339(),
            dry_run: value.dry_run,
            file_count: value.file_count,
            total_bytes: value.total_bytes,
            receipt_count: value.receipt_count,
        }
    }
}

impl From<benshu_security::MemoryRestorePolicyBasis> for MemoryRestorePolicyBasisDto {
    fn from(value: benshu_security::MemoryRestorePolicyBasis) -> Self {
        Self {
            backup_id: value.backup_id,
            decision_kind: value.decision_kind,
            policy_basis: value.policy_basis,
            reasons: value.reasons,
            warnings: value.warnings,
        }
    }
}

impl From<benshu_security::MemoryRestoreDryRunReport> for MemoryRestoreDryRunReportDto {
    fn from(value: benshu_security::MemoryRestoreDryRunReport) -> Self {
        Self {
            backup_id: value.backup_id,
            checked_at: value.checked_at.to_rfc3339(),
            contract_version: value.contract_version,
            encryption_key_fingerprint: value.encryption_key_fingerprint,
            valid: value.valid,
            file_count: value.file_count,
            total_bytes: value.total_bytes,
            restorable_files: value.restorable_files,
            missing_payloads: value.missing_payloads,
            integrity_mismatches: value.integrity_mismatches,
        }
    }
}

fn manifest_metadata_entries(
    manifest: &benshu_security::MemoryRestorePointManifest,
) -> Result<Vec<(String, String)>, AppError> {
    let manifest_json =
        serde_json::to_string(&MemoryRestorePointManifestDto::from(manifest.clone()))?;
    Ok(vec![
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_backup_id"),
            manifest.backup_id.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_manifest_created_at"),
            manifest.created_at.to_rfc3339(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_contract_version"),
            manifest.contract_version.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_storage_root_hint"),
            manifest.storage_root_hint.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_encryption_key_fingerprint"),
            manifest.encryption_key_fingerprint.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_manifest_file_count"),
            manifest.file_count.to_string(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_manifest_total_bytes"),
            manifest.total_bytes.to_string(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_manifest_json"),
            manifest_json,
        ),
    ])
}

fn receipt_metadata_entries(
    receipt: &benshu_security::MemoryRestoreReceipt,
) -> Result<Vec<(String, String)>, AppError> {
    let receipt_json = serde_json::to_string(&MemoryRestoreReceiptDto::from(receipt.clone()))?;
    Ok(vec![
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_receipt_id"),
            receipt.receipt_id.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_receipt_backup_id"),
            receipt.backup_id.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_restored_at"),
            receipt.restored_at.to_rfc3339(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_receipt_contract_version"),
            receipt.contract_version.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_receipt_encryption_key_fingerprint"),
            receipt.encryption_key_fingerprint.clone(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_restored_files"),
            receipt.restored_files.to_string(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_restored_bytes"),
            receipt.restored_bytes.to_string(),
        ),
        (
            format!("{SEALED_RESTORE_METADATA_PREFIX}.last_receipt_json"),
            receipt_json,
        ),
    ])
}

async fn sync_manifest_metadata(
    state: &AppState,
    manifest: &benshu_security::MemoryRestorePointManifest,
) -> Result<(), AppError> {
    for (key, value) in manifest_metadata_entries(manifest)? {
        state.kernel.memory().set_metadata(&key, &value).await?;
    }
    Ok(())
}

async fn sync_receipt_metadata(
    state: &AppState,
    receipt: &benshu_security::MemoryRestoreReceipt,
) -> Result<(), AppError> {
    for (key, value) in receipt_metadata_entries(receipt)? {
        state.kernel.memory().set_metadata(&key, &value).await?;
    }
    Ok(())
}

pub async fn create_memory_restore_point(
    State(state): State<AppState>,
) -> Result<Json<MemoryRestorePointManifestDto>, AppError> {
    let manifest = state
        .kernel
        .security()
        .create_memory_restore_point()
        .await?;
    sync_manifest_metadata(&state, &manifest).await?;
    Ok(Json(MemoryRestorePointManifestDto::from(manifest)))
}

pub async fn list_memory_restore_points(
    State(state): State<AppState>,
) -> Result<Json<Vec<MemoryRestorePointManifestDto>>, AppError> {
    Ok(Json(
        state
            .kernel
            .security()
            .list_memory_restore_points()
            .await?
            .into_iter()
            .map(MemoryRestorePointManifestDto::from)
            .collect(),
    ))
}

pub async fn inspect_memory_restore_point(
    Query(query): Query<RestorePointQuery>,
    State(state): State<AppState>,
) -> Result<Json<MemoryRestorePointManifestDto>, AppError> {
    Ok(Json(MemoryRestorePointManifestDto::from(
        state
            .kernel
            .security()
            .inspect_memory_restore_point(&query.backup_id)
            .await?,
    )))
}

pub async fn restore_memory_restore_point(
    State(state): State<AppState>,
    Json(payload): Json<RestorePointRequest>,
) -> Result<Json<MemoryRestoreReceiptDto>, AppError> {
    let receipt = state
        .kernel
        .security()
        .restore_memory_restore_point(&payload.backup_id)
        .await?;
    sync_receipt_metadata(&state, &receipt).await?;
    Ok(Json(MemoryRestoreReceiptDto::from(receipt)))
}

pub async fn delete_memory_restore_point(
    State(state): State<AppState>,
    Json(payload): Json<DeleteRestorePointRequest>,
) -> Result<Json<MemoryRestoreDeleteReportDto>, AppError> {
    Ok(Json(MemoryRestoreDeleteReportDto::from(
        state
            .kernel
            .security()
            .delete_memory_restore_point(&payload.backup_id, payload.dry_run)
            .await?,
    )))
}

pub async fn dry_run_memory_restore_point(
    Query(query): Query<RestorePointQuery>,
    State(state): State<AppState>,
) -> Result<Json<MemoryRestoreDryRunReportDto>, AppError> {
    Ok(Json(MemoryRestoreDryRunReportDto::from(
        state
            .kernel
            .security()
            .dry_run_memory_restore_point(&query.backup_id)
            .await?,
    )))
}

pub async fn explain_memory_restore_policy(
    Query(query): Query<RestorePointQuery>,
    State(state): State<AppState>,
) -> Result<Json<MemoryRestorePolicyBasisDto>, AppError> {
    Ok(Json(MemoryRestorePolicyBasisDto::from(
        state
            .kernel
            .security()
            .explain_memory_restore_policy(&query.backup_id)
            .await?,
    )))
}

pub async fn list_memory_restore_receipts(
    Query(query): Query<RestorePointQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<MemoryRestoreReceiptDto>>, AppError> {
    Ok(Json(
        state
            .kernel
            .security()
            .list_memory_restore_receipts(&query.backup_id)
            .await?
            .into_iter()
            .map(MemoryRestoreReceiptDto::from)
            .collect(),
    ))
}

pub async fn inspect_memory_restore_receipt(
    Query(query): Query<RestoreReceiptQuery>,
    State(state): State<AppState>,
) -> Result<Json<MemoryRestoreReceiptDto>, AppError> {
    Ok(Json(MemoryRestoreReceiptDto::from(
        state
            .kernel
            .security()
            .inspect_memory_restore_receipt(&query.backup_id, &query.receipt_id)
            .await?,
    )))
}

#[derive(Serialize)]
pub struct GatewaySnapshot {
    pub status: &'static str,
    pub version: &'static str,
    /// Number of configured agent/worker roles that can be routed to.
    pub agent_count: usize,
    /// Number of restored or active session -> agent-role mappings.
    pub session_agent_mapping_count: usize,
    pub skill_count: usize,
    pub cron_job_count: usize,
    pub connectors: Vec<ConnectorStatus>,
    pub custom_providers: Vec<String>,
    pub vault_keys: Vec<String>,
    pub agents: Vec<String>,
    pub model_ram_usage_mb: usize,
    pub model_vram_usage_mb: usize,
    pub model_ram_limit_gb: u32,
    pub model_vram_limit_gb: u32,
    pub whisper_status: String,
    pub piper_status: String,
    pub auto_consolidation_enabled: bool,
    pub fact_check_enabled: bool,
    pub enable_global_voice: bool,
    pub enable_local_vision: bool,
    pub local_vision_status: String,
    pub nlu_status: String,
    pub nlu_mode: String,
    pub nlu_model: String,
    pub fact_check_status: String,
    pub image_gen_model: String,
    pub image_gen_status: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub category: String,
    pub status: String,
    pub provider: String,
}

#[derive(Serialize)]
pub struct ConnectorStatus {
    pub name: String,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeModeDto {
    pub gateway_version: &'static str,
    pub connected: bool,
    pub model_ram_limit_gb: u32,
    pub model_vram_limit_gb: u32,
    pub auto_consolidation_enabled: bool,
    pub enable_global_voice: bool,
    pub enable_local_vision: bool,
    pub local_vision_status: String,
    pub vision_model: String,
    pub image_edit_model: String,
    pub audio_understanding_model: String,
    pub realtime_vad_model: String,
    pub duplex_voice_model: String,
    pub local_classifier_model: String,
    pub local_router_model: String,
    pub local_safety_model: String,
    pub nlu_status: String,
    pub fact_check_status: String,
    pub image_gen_model: String,
    pub image_gen_status: String,
    pub llama_cpp_runtime: benshu_brain::config::LlamaCppRuntimeConfig,
    pub windows_ml_runtime: benshu_brain::config::WindowsMlRuntimeConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlamaCppCompatibilityDto {
    pub compatibility: String,
    pub note: String,
    pub role_support: Vec<String>,
    pub mmproj_status: String,
    pub current_host_status: String,
    pub current_host_note: String,
    pub server_path: Option<String>,
    pub server_build: Option<u32>,
    pub minimum_supported_build: u32,
    pub server_supported: bool,
    pub server_note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelRoleBindingDto {
    pub role: String,
    pub configured_model: String,
    pub source: String,
    pub factory_id: Option<String>,
    pub declared_roles: Vec<String>,
    pub readiness: String,
    pub runtime_profile: String,
    pub product_track: String,
    pub preferred_backend: String,
    pub current_backend: String,
    pub execution_provider: String,
    pub artifact_kind: String,
    pub effective_runtime_state: String,
    pub effective_runtime_reason: String,
    pub effective_runtime_outcome: String,
    pub effective_runtime_class: String,
    pub effective_runtime_failure_reason: Option<String>,
    pub effective_runtime_strategy: String,
    pub windows_native_plan_status: String,
    pub windows_native_plan_note: String,
    pub target_readiness: String,
    pub target_reason: String,
    pub host_validation_status: String,
    pub host_validation_note: String,
    pub fallback_hint: Option<String>,
    pub llama_cpp: LlamaCppCompatibilityDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaRuntimeSurfaceDto {
    pub global_voice_enabled: bool,
    pub local_vision_enabled: bool,
    pub local_vision_status: String,
    pub source_contracts: Vec<String>,
    pub followup_contracts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelStackDto {
    pub host_runtime: String,
    pub deployment_lane: String,
    pub deployment_strategy: String,
    pub deployment_note: String,
    pub product_mainline: String,
    pub validation_tracks: Vec<String>,
    pub windows_native_priority: bool,
    pub small_model_runtime_target: String,
    pub small_model_execution_linked: bool,
    pub small_model_execution_provider: String,
    pub small_model_device_target: String,
    pub small_model_fallback_mode: String,
    pub small_model_runtime_outcome: String,
    pub small_model_runtime_strategy: String,
    pub small_model_runtime_readiness: String,
    pub small_model_runtime_reason: String,
    pub main_brain_runtime_target: String,
    pub model_pool_loaded_count: usize,
    pub model_pool_loaded_models: Vec<String>,
    pub entries: Vec<LocalModelRoleBindingDto>,
    pub media_runtime: MediaRuntimeSurfaceDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelArtifactDto {
    pub label: String,
    pub path: String,
    pub relative_path: String,
    pub artifact_kind: String,
    pub size_bytes: u64,
    pub selectable_as_main_brain: bool,
    pub source: String,
    pub factory_id: Option<String>,
    pub declared_roles: Vec<String>,
    pub resolved_mmproj_path: Option<String>,
    pub llama_cpp: LlamaCppCompatibilityDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelArtifactCatalogDto {
    pub root: String,
    pub artifacts: Vec<LocalModelArtifactDto>,
}

fn describe_binding_lossy(
    model: &str,
    capability: BackendCapability,
) -> Option<BackendBindingDescriptor> {
    if model.trim().is_empty() {
        return None;
    }
    InferenceFactory::describe_binding(std::path::Path::new(model), None, capability).ok()
}

fn collect_model_artifacts_recursive(root: &Path, current: &Path, out: &mut Vec<PathBuf>) {
    if current != root && current.is_dir() {
        let contract = describe_local_model_contract(current);
        if matches!(
            contract.kind,
            LocalModelArtifactKind::OnnxDirectory
                | LocalModelArtifactKind::SafetensorsDirectory
                | LocalModelArtifactKind::DiffusersDirectory
                | LocalModelArtifactKind::ImageOnnxDirectory
        ) {
            if current.starts_with(root) {
                out.push(current.to_path_buf());
            }
            return;
        }
    }

    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_model_artifacts_recursive(root, &path, out);
            continue;
        }

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !matches!(ext, "gguf" | "onnx") {
            continue;
        }

        // Keep only assets under the configured models tree.
        if path.starts_with(root) {
            out.push(path);
        }
    }
}

fn artifact_size_bytes(path: &Path) -> Option<u64> {
    if path.is_file() {
        return std::fs::metadata(path).ok().map(|meta| meta.len());
    }

    if path.is_dir() {
        let mut total = 0u64;
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() {
                    stack.push(child);
                } else if let Ok(meta) = std::fs::metadata(&child) {
                    total = total.saturating_add(meta.len());
                }
            }
        }
        return Some(total);
    }

    None
}

fn inferred_workspace_root_from_config(config_path: &Path) -> PathBuf {
    let data_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    if data_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "data")
    {
        if let Some(parent) = data_dir.parent() {
            return parent.to_path_buf();
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("Cargo.toml").exists() && cwd.join("models").exists() {
            return cwd;
        }
    }
    data_dir.to_path_buf()
}

fn local_model_artifact_roots(config_path: &Path) -> (PathBuf, Vec<PathBuf>) {
    let workspace_root = inferred_workspace_root_from_config(config_path);
    let data_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let mut roots = vec![
        data_dir.join("models"),
        workspace_root.join("models").join("live"),
    ];
    if let Ok(value) = std::env::var("BENSHU_LOCAL_MODEL_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            roots.push(PathBuf::from(trimmed));
        }
    }
    roots.retain(|root| root.exists());
    roots.sort();
    roots.dedup();
    (workspace_root, roots)
}

fn local_model_artifact_catalog(
    catalog_root: &Path,
    scan_roots: &[PathBuf],
    server_status: Option<&LlamaCppServerStatus>,
) -> LocalModelArtifactCatalogDto {
    let mut paths = Vec::new();
    for root in scan_roots {
        collect_model_artifacts_recursive(root, root, &mut paths);
    }
    paths.sort();
    paths.dedup();

    let artifacts = paths
        .into_iter()
        .filter_map(|path| {
            let size_bytes = artifact_size_bytes(&path)?;
            let rel = path
                .strip_prefix(catalog_root)
                .ok()
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .to_string();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            let artifact_kind = describe_local_model_contract(&path).kind;
            let binding =
                InferenceFactory::describe_binding(&path, None, BackendCapability::LLM).ok();
            let declared_roles = binding
                .as_ref()
                .map(|binding| {
                    binding
                        .declared_roles
                        .iter()
                        .map(|role| role.as_str().to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let source = binding
                .as_ref()
                .map(|binding| binding.source.as_str().to_string())
                .unwrap_or_else(|| "compatibility_only".to_string());
            let factory_id = binding.as_ref().map(|binding| binding.factory_id.clone());
            let resolved_mmproj_path = binding
                .as_ref()
                .and_then(|binding| binding.mmproj_path.clone());
            let llama_cpp = llama_cpp_compatibility_surface(
                &path.to_string_lossy(),
                BackendCapability::LLM,
                &artifact_kind,
                binding.as_ref(),
                factory_id.as_deref().unwrap_or(""),
                "catalog_only",
                "catalog_only",
                "catalog_only",
                "Catalog discovery does not evaluate runtime host readiness.",
                server_status,
            );
            let selectable_as_main_brain = matches!(artifact_kind, LocalModelArtifactKind::GGUF)
                && !file_name.contains("mmproj");

            Some(LocalModelArtifactDto {
                label: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string(),
                path: path.to_string_lossy().to_string(),
                relative_path: rel,
                artifact_kind: artifact_kind.as_str().to_string(),
                size_bytes,
                selectable_as_main_brain,
                source,
                factory_id,
                declared_roles,
                resolved_mmproj_path,
                llama_cpp,
            })
        })
        .collect();

    LocalModelArtifactCatalogDto {
        root: catalog_root.to_string_lossy().to_string(),
        artifacts,
    }
}

fn role_binding_entry(
    role: &str,
    configured_model: impl Into<String>,
    capability: BackendCapability,
    runtime_profile: impl Into<String>,
    product_track: impl Into<String>,
    preferred_backend: impl Into<String>,
    execution_provider: impl Into<String>,
    small_model_runtime: &benshu_inference::WindowsNativeRuntimeStatus,
    fallback_hint: Option<&str>,
    server_status: Option<&LlamaCppServerStatus>,
) -> LocalModelRoleBindingDto {
    let configured_model = configured_model.into();
    let runtime_profile = runtime_profile.into();
    let product_track = product_track.into();
    let preferred_backend = preferred_backend.into();
    let execution_provider = execution_provider.into();
    let configured_path = std::path::Path::new(&configured_model);
    let contract = describe_local_model_contract(std::path::Path::new(&configured_model));
    let binding = describe_binding_lossy(&configured_model, capability);
    let tokenizer_present = if configured_path.is_dir() {
        configured_path.join("tokenizer.json").exists()
    } else {
        configured_path
            .parent()
            .map(|parent| parent.join("tokenizer.json").exists())
            .unwrap_or(false)
    };
    let onnx_text_role_requires_tokenizer = matches!(
        capability,
        BackendCapability::Embedding | BackendCapability::Rerank
    ) && matches!(
        contract.kind,
        LocalModelArtifactKind::OnnxDirectory | LocalModelArtifactKind::OnnxFile
    );
    let image_role_contract_ready = matches!(
        contract.kind,
        LocalModelArtifactKind::ApiReference
            | LocalModelArtifactKind::ImageBridge
            | LocalModelArtifactKind::DiffusersDirectory
            | LocalModelArtifactKind::ImageOnnxDirectory
    );
    let role_contract_ready = if matches!(capability, BackendCapability::ImageGeneration) {
        image_role_contract_ready
    } else {
        contract.ready_for_windows_native_small_model_runtime
            && (!onnx_text_role_requires_tokenizer || tokenizer_present)
    };
    let role_contract_reason = if matches!(capability, BackendCapability::ImageGeneration) {
        contract.reason.clone()
    } else if !contract.ready_for_windows_native_small_model_runtime {
        contract.reason.clone()
    } else if onnx_text_role_requires_tokenizer && !tokenizer_present {
        format!(
            "{} tokenizer.json is required next to the ONNX model for the Windows-native text runtime.",
            contract.reason
        )
    } else {
        contract.reason.clone()
    };
    let (source, factory_id, declared_roles, readiness) = if let Some(ref binding) = binding {
        (
            binding.source.as_str().to_string(),
            Some(binding.factory_id.clone()),
            binding
                .declared_roles
                .iter()
                .map(|role| role.as_str().to_string())
                .collect(),
            "configured".to_string(),
        )
    } else if configured_model.trim().is_empty() {
        (
            "unconfigured".to_string(),
            None,
            Vec::new(),
            "unconfigured".to_string(),
        )
    } else {
        (
            "unknown".to_string(),
            None,
            Vec::new(),
            "compatibility_only".to_string(),
        )
    };

    let current_backend = current_backend_label(capability, &factory_id, &declared_roles);
    let (
        effective_runtime_state,
        effective_runtime_reason,
        effective_runtime_outcome,
        effective_runtime_strategy,
    ) = effective_runtime_surface(
        capability,
        &configured_model,
        &product_track,
        &current_backend,
        &contract.kind,
        role_contract_ready,
        &role_contract_reason,
        small_model_runtime,
    );
    let effective_runtime_class = windows_native_outcome_class(&effective_runtime_outcome);
    let effective_runtime_failure_reason =
        windows_native_failure_reason(role, &effective_runtime_outcome);
    let (windows_native_plan_status, windows_native_plan_note, target_readiness, target_reason) =
        windows_native_role_plan(
            capability,
            &product_track,
            role_contract_ready,
            &role_contract_reason,
            small_model_runtime,
        );
    let (host_validation_status, host_validation_note) = windows_native_host_validation_surface(
        capability,
        &product_track,
        &effective_runtime_outcome,
        small_model_runtime,
    );
    let llama_cpp = llama_cpp_compatibility_surface(
        &configured_model,
        capability,
        &contract.kind,
        binding.as_ref(),
        &current_backend,
        &effective_runtime_state,
        &effective_runtime_reason,
        &host_validation_status,
        &host_validation_note,
        server_status,
    );

    LocalModelRoleBindingDto {
        role: role.to_string(),
        configured_model,
        source,
        factory_id,
        declared_roles,
        readiness,
        runtime_profile,
        product_track,
        preferred_backend,
        current_backend,
        execution_provider,
        artifact_kind: contract.kind.as_str().to_string(),
        effective_runtime_state,
        effective_runtime_reason,
        effective_runtime_outcome,
        effective_runtime_class,
        effective_runtime_failure_reason,
        effective_runtime_strategy,
        windows_native_plan_status,
        windows_native_plan_note,
        target_readiness,
        target_reason,
        host_validation_status,
        host_validation_note,
        fallback_hint: fallback_hint.map(|hint| hint.to_string()),
        llama_cpp,
    }
}

fn llama_cpp_compatibility_surface(
    configured_model: &str,
    capability: BackendCapability,
    artifact_kind: &LocalModelArtifactKind,
    binding: Option<&BackendBindingDescriptor>,
    current_backend: &str,
    effective_runtime_state: &str,
    effective_runtime_reason: &str,
    host_validation_status: &str,
    host_validation_note: &str,
    configured_server_status: Option<&LlamaCppServerStatus>,
) -> LlamaCppCompatibilityDto {
    let discovered_server_status;
    let server_status = if let Some(status) = configured_server_status {
        Some(status)
    } else {
        discovered_server_status = discover_windows_llama_server_status();
        discovered_server_status.as_ref()
    };
    let server_path = server_status.map(|status| status.path.to_string_lossy().to_string());
    let server_build = server_status.and_then(|status| status.build);
    let server_supported = server_status
        .map(|status| status.supported)
        .unwrap_or(false);
    let server_note = server_status
        .map(|status| status.note.clone())
        .unwrap_or_else(|| {
            format!(
                "No llama.cpp server was discovered; required minimum is b{MIN_SUPPORTED_LLAMA_CPP_BUILD}."
            )
        });
    if configured_model.trim().is_empty() {
        return LlamaCppCompatibilityDto {
            compatibility: "unconfigured".to_string(),
            note: "No model is configured, so llama.cpp compatibility cannot be evaluated yet."
                .to_string(),
            role_support: Vec::new(),
            mmproj_status: "not_applicable".to_string(),
            current_host_status: "unconfigured".to_string(),
            current_host_note: "Configure a local model path first.".to_string(),
            server_path,
            server_build,
            minimum_supported_build: MIN_SUPPORTED_LLAMA_CPP_BUILD,
            server_supported,
            server_note,
        };
    }

    if !matches!(artifact_kind, LocalModelArtifactKind::GGUF) {
        return LlamaCppCompatibilityDto {
            compatibility: "not_llama_cpp_artifact".to_string(),
            note: "This binding is not a GGUF artifact, so it does not target the llama.cpp execution path."
                .to_string(),
            role_support: Vec::new(),
            mmproj_status: "not_applicable".to_string(),
            current_host_status: "not_applicable".to_string(),
            current_host_note:
                "Current host llama.cpp readiness is not relevant for non-GGUF bindings."
                    .to_string(),
            server_path,
            server_build,
            minimum_supported_build: MIN_SUPPORTED_LLAMA_CPP_BUILD,
            server_supported,
            server_note,
        };
    }

    let mut role_support: Vec<String> = binding
        .map(|binding| {
            binding
                .declared_roles
                .iter()
                .map(|role| role.as_str().to_string())
                .collect()
        })
        .unwrap_or_else(|| vec!["llm".to_string(), "slm".to_string()]);
    role_support.sort();
    role_support.dedup();

    let mmproj_resolved = binding
        .and_then(|binding| binding.mmproj_path.as_ref())
        .map(|path| !path.trim().is_empty())
        .unwrap_or(false);
    let supports_vlm = role_support.iter().any(|role| role == "vlm");
    let mmproj_status = if supports_vlm || mmproj_resolved {
        "resolved".to_string()
    } else if matches!(capability, BackendCapability::Vision) {
        "required_for_vlm_missing".to_string()
    } else {
        "text_only_no_mmproj".to_string()
    };

    let compatibility = if supports_vlm || mmproj_resolved {
        "multimodal_compatible".to_string()
    } else {
        "text_compatible".to_string()
    };

    let note = if supports_vlm || mmproj_resolved {
        "GGUF binding is on the llama.cpp path and a vision projection is resolved, so this model can participate in multimodal/VLM routing."
            .to_string()
    } else if matches!(capability, BackendCapability::Vision) {
        "GGUF binding is on the llama.cpp path, but no mmproj is resolved yet; the binding remains text-only until a matching mmproj is configured or discovered."
            .to_string()
    } else {
        "GGUF binding is on the llama.cpp path and is ready for text roles; add a matching mmproj if you want VLM/multimodal routing."
            .to_string()
    };

    let current_host_status = if current_backend == "llama_cpp" {
        "llama_cpp_runtime_selected".to_string()
    } else if current_backend == "prime_multimodal_runtime" {
        if supports_vlm || mmproj_resolved {
            "llama_cpp_multimodal_runtime_selected".to_string()
        } else {
            "llama_cpp_text_runtime_selected".to_string()
        }
    } else if effective_runtime_state == "main_brain_runtime_active" {
        "main_brain_llama_cpp_track_active".to_string()
    } else {
        host_validation_status.to_string()
    };

    let current_host_note = if current_backend == "llama_cpp" {
        "Current host has selected the direct llama.cpp execution path for this binding."
            .to_string()
    } else if current_backend == "prime_multimodal_runtime" {
        if supports_vlm || mmproj_resolved {
            "Current host selected the prime multimodal runtime family and resolved multimodal capability for this GGUF binding."
                .to_string()
        } else {
            "Current host selected the prime multimodal runtime family, but this GGUF binding is still text-only because no mmproj is resolved."
                .to_string()
        }
    } else if effective_runtime_state == "main_brain_runtime_active" {
        effective_runtime_reason.to_string()
    } else {
        host_validation_note.to_string()
    };

    LlamaCppCompatibilityDto {
        compatibility,
        note,
        role_support,
        mmproj_status,
        current_host_status,
        current_host_note,
        server_path,
        server_build,
        minimum_supported_build: MIN_SUPPORTED_LLAMA_CPP_BUILD,
        server_supported,
        server_note,
    }
}

fn windows_native_host_validation_surface(
    capability: BackendCapability,
    product_track: &str,
    effective_runtime_outcome: &str,
    small_model_runtime: &benshu_inference::WindowsNativeRuntimeStatus,
) -> (String, String) {
    if product_track != "windows_native_small_model_layer" {
        return match capability {
            BackendCapability::STT | BackendCapability::OCR => (
                "not_required_specialized_runtime".to_string(),
                "This role has already been evaluated and intentionally remains on the specialized runtime."
                    .to_string(),
            ),
            BackendCapability::TTS => (
                "not_required_specialized_runtime".to_string(),
                "This role intentionally stays on the specialized TTS runtime rather than the Windows-native ONNX layer."
                    .to_string(),
            ),
            BackendCapability::LLM => (
                "not_required_main_brain_track".to_string(),
                "Main-brain validation stays on the llama.cpp track; Windows-native small-model host validation does not apply to this role."
                    .to_string(),
            ),
            _ => (
                "not_applicable".to_string(),
                "Windows-native host validation is not required for this role on the current product track."
                    .to_string(),
            ),
        };
    }

    if small_model_runtime.deployment_lane != "product_mainline" {
        return (
            "pending_windows_host_validation".to_string(),
            "Current host is validation-only; capture the first successful native Windows execution on a product-mainline host to complete host validation."
                .to_string(),
        );
    }

    if effective_runtime_outcome == "windows_native_active" {
        return (
            "validated_on_current_windows_host".to_string(),
            "Windows-native execution has already been observed on the current Windows product host."
                .to_string(),
        );
    }

    (
        "pending_windows_runtime_observation".to_string(),
        format!(
            "Backend integration is complete, but a successful Windows-native runtime observation has not been captured on the current Windows host yet (current outcome: {effective_runtime_outcome})."
        ),
    )
}

fn windows_native_outcome_class(outcome: &str) -> String {
    match outcome {
        "windows_native_active" | "active" => "active",
        "cpu_fallback_provider_downgrade" => "provider_downgrade",
        "cpu_fallback_no_accelerator_route" => "no_accelerator_route",
        "cpu_fallback_active" => "cpu_fallback",
        "windows_native_provider_execution_failed" => "provider_failure",
        "windows_native_execution_failed" => "runtime_failure",
        "fallback_runtime_active" | "migrate_to_windows_native_runtime" => "fallback_runtime",
        "backend_unlinked"
        | "runtime_missing"
        | "validation_only"
        | "specialized_runtime_pending" => "pending_runtime",
        "model_contract_incompatible" => "contract_incompatible",
        "accelerator_resource_exhausted" => "resource_exhausted",
        "accelerator_unavailable" => "accelerator_unavailable",
        "unconfigured" | "specialized_runtime_active" | "main_brain_runtime_active" => {
            "not_applicable"
        }
        _ => "other",
    }
    .to_string()
}

fn windows_native_failure_reason(role: &str, outcome: &str) -> Option<String> {
    match outcome {
        "windows_native_active"
        | "active"
        | "specialized_runtime_active"
        | "main_brain_runtime_active"
        | "unconfigured" => None,
        other => Some(format!("windows_native::{role}::{other}")),
    }
}

fn windows_native_role_plan(
    capability: BackendCapability,
    product_track: &str,
    role_contract_ready: bool,
    role_contract_reason: &str,
    small_model_runtime: &benshu_inference::WindowsNativeRuntimeStatus,
) -> (String, String, String, String) {
    match capability {
        BackendCapability::Embedding | BackendCapability::Rerank => {
            let target_readiness = if !role_contract_ready {
                "target_contract_pending".to_string()
            } else {
                format!(
                    "target_contract_ready:{}",
                    small_model_runtime.small_model_runtime_readiness
                )
            };
            let target_reason = if !role_contract_ready {
                role_contract_reason.to_string()
            } else {
                format!(
                    "{} Runtime status: {}.",
                    role_contract_reason, small_model_runtime.small_model_runtime_reason
                )
            };
            (
                "windows_native_target_active".to_string(),
                "This role is on the formal Windows-native small-model adoption track.".to_string(),
                target_readiness,
                target_reason,
            )
        }
        BackendCapability::NLU | BackendCapability::FactCheck => {
            let target_readiness = if !role_contract_ready {
                "target_contract_pending".to_string()
            } else {
                format!(
                    "target_contract_ready:{}",
                    small_model_runtime.small_model_runtime_readiness
                )
            };
            let target_reason = if !role_contract_ready {
                role_contract_reason.to_string()
            } else {
                format!(
                    "{} Runtime status: {}.",
                    role_contract_reason, small_model_runtime.small_model_runtime_reason
                )
            };
            (
                "windows_native_tactical_small_model_target".to_string(),
                "This tactical/runtime-side small model is now tracked through the same Windows-native ONNX role-binding and readiness contract as the rest of the Windows-native small-model layer.".to_string(),
                target_readiness,
                target_reason,
            )
        }
        BackendCapability::STT => (
            "evaluation_complete_keep_specialized_runtime".to_string(),
            "Windows-native STT was evaluated for the Windows-first roadmap, but the formal product path remains the shared STT runtime until a validated ONNX/WinML backend exists.".to_string(),
            "evaluation_complete_keep_specialized_runtime".to_string(),
            "Current product decision keeps STT on the specialized voice runtime; Windows-native ONNX/WinML remains an evaluation track, not the default execution path.".to_string(),
        ),
        BackendCapability::OCR => (
            "evaluation_complete_keep_specialized_runtime".to_string(),
            "Windows-native OCR was evaluated for the Windows-first roadmap, but the formal product path remains document OCR / Tesseract-style specialized runtimes until a validated ONNX/WinML OCR backend exists.".to_string(),
            "evaluation_complete_keep_specialized_runtime".to_string(),
            "Current product decision keeps OCR on specialized runtimes; Windows-native ONNX/WinML OCR remains evaluation-only and must not be presented as active by default.".to_string(),
        ),
        BackendCapability::TTS => (
            "specialized_runtime_intentional".to_string(),
            "TTS intentionally stays on its specialized runtime path and is not being forced into the Windows-native ONNX small-model lane.".to_string(),
            "specialized_runtime_intentional".to_string(),
            "TTS remains intentionally specialized; no Windows-native ONNX target is required for this role.".to_string(),
        ),
        BackendCapability::ImageGeneration => (
            "specialized_image_runtime_target".to_string(),
            "Image generation now targets a dedicated image-runtime lane: cloud image APIs, bridge-image runtimes, or image-specific ONNX/diffusers services exposed behind the global binding."
                .to_string(),
            if !role_contract_ready {
                "target_contract_pending".to_string()
            } else {
                "target_contract_ready".to_string()
            },
            role_contract_reason.to_string(),
        ),
        BackendCapability::LLM if product_track == "windows_native_main_brain" => (
            "main_brain_llama_cpp".to_string(),
            "The formal Windows-native main-brain path remains llama.cpp.".to_string(),
            "main_brain_llama_cpp".to_string(),
            "The main brain is intentionally kept on llama.cpp rather than moved into the Windows-native ONNX small-model lane.".to_string(),
        ),
        _ => (
            "specialized_runtime_active".to_string(),
            "This role currently remains on its specialized runtime track.".to_string(),
            if !role_contract_ready {
                "target_contract_pending".to_string()
            } else {
                "target_contract_ready".to_string()
            },
            role_contract_reason.to_string(),
        ),
    }
}

fn effective_runtime_surface(
    capability: BackendCapability,
    configured_model: &str,
    product_track: &str,
    current_backend: &str,
    artifact_kind: &LocalModelArtifactKind,
    role_contract_ready: bool,
    role_contract_reason: &str,
    small_model_runtime: &benshu_inference::WindowsNativeRuntimeStatus,
) -> (String, String, String, String) {
    if configured_model.trim().is_empty() {
        return (
            "unconfigured".to_string(),
            "No local model is configured for this role yet.".to_string(),
            "unconfigured".to_string(),
            "configure_model_binding".to_string(),
        );
    }

    if matches!(capability, BackendCapability::ImageGeneration) {
        return match current_backend {
            "openai_image_bridge" => (
                "specialized_runtime_active".to_string(),
                "Image generation is active through the bridge-image runtime, so BenShu is forwarding requests into a dedicated image backend."
                    .to_string(),
                "active".to_string(),
                "active".to_string(),
            ),
            "cloud_img" => (
                "specialized_runtime_active".to_string(),
                "Image generation is active through a cloud/API image backend.".to_string(),
                "active".to_string(),
                "active".to_string(),
            ),
            "diffusion" => (
                "fallback_runtime_active".to_string(),
                "Configured image model is still binding to the legacy local diffusion runtime. The preferred product path is now a dedicated ML image runtime (bridge-image or image-specific ONNX service), not the old Candle-style in-process route."
                    .to_string(),
                "fallback_runtime_active".to_string(),
                "migrate_to_specialized_image_runtime".to_string(),
            ),
            _ if role_contract_ready => (
                "specialized_runtime_pending".to_string(),
                format!(
                    "The image model package is recognized ({artifact_kind:?}), but no active image runtime is linked yet. Bind it to a bridge-image endpoint, a cloud image backend, or another dedicated image service."
                ),
                "backend_unlinked".to_string(),
                "link_specialized_image_runtime".to_string(),
            ),
            _ => (
                "specialized_runtime_pending".to_string(),
                role_contract_reason.to_string(),
                "model_contract_incompatible".to_string(),
                "rebind_image_runtime".to_string(),
            ),
        };
    }

    let windows_small_model_role = matches!(
        capability,
        BackendCapability::Embedding
            | BackendCapability::Rerank
            | BackendCapability::STT
            | BackendCapability::OCR
    ) || product_track == "windows_native_small_model_layer";

    let onnx_windows_backend_selected =
        current_backend == "onnx_embedding_winml" || current_backend == "onnx_rerank_winml";

    if onnx_windows_backend_selected {
        if role_contract_ready
            && small_model_runtime.small_model_runtime_readiness == "windows_native_ready"
        {
            return (
                "windows_native_active".to_string(),
                format!(
                    "Windows-native ONNX runtime is active for this role via {}.",
                    current_backend
                ),
                "windows_native_active".to_string(),
                "active".to_string(),
            );
        }

        if !role_contract_ready {
            return (
                "windows_native_pending_contract".to_string(),
                role_contract_reason.to_string(),
                "model_contract_incompatible".to_string(),
                "rebind_model_contract".to_string(),
            );
        }

        return (
            "windows_native_pending_runtime".to_string(),
            format!(
                "The ONNX Windows-native backend is selected, but it is not executable yet ({}). {}",
                small_model_runtime.small_model_runtime_readiness,
                small_model_runtime.small_model_runtime_reason
            ),
            small_model_runtime.small_model_runtime_outcome.clone(),
            small_model_runtime.small_model_runtime_strategy.clone(),
        );
    }

    if product_track == "windows_native_main_brain" {
        return (
            "main_brain_runtime_active".to_string(),
            format!("The current main-brain runtime is {}.", current_backend),
            "main_brain_runtime_active".to_string(),
            "active".to_string(),
        );
    }

    if windows_small_model_role {
        if matches!(
            artifact_kind,
            LocalModelArtifactKind::OnnxDirectory | LocalModelArtifactKind::OnnxFile
        ) {
            return (
                "fallback_runtime_active".to_string(),
                format!(
                    "This role targets the Windows-native small-model layer, but the active runtime is still {}. {}",
                    current_backend, small_model_runtime.small_model_runtime_reason
                ),
                "fallback_runtime_active".to_string(),
                "migrate_to_windows_native_runtime".to_string(),
            );
        }

        return (
            "fallback_runtime_active".to_string(),
            format!(
                "This role is still running through {} while the Windows-native small-model layer remains a target path. {}",
                current_backend, role_contract_reason
            ),
            "fallback_runtime_active".to_string(),
            "fallback_runtime".to_string(),
        );
    }

    (
        "specialized_runtime_active".to_string(),
        format!("The current specialized runtime is {}.", current_backend),
        "specialized_runtime_active".to_string(),
        "active".to_string(),
    )
}

fn main_brain_execution_provider(hw: &HardwareStatus) -> String {
    match hw.acceleration_profile() {
        AccelerationProfile::CudaPreferred => "cuda".to_string(),
        AccelerationProfile::VulkanPreferred => "vulkan".to_string(),
        AccelerationProfile::MetalPreferred => "metal".to_string(),
        AccelerationProfile::CpuOnly => "cpu".to_string(),
    }
}

fn current_backend_label(
    capability: BackendCapability,
    factory_id: &Option<String>,
    declared_roles: &[String],
) -> String {
    if let Some(factory_id) = factory_id {
        return factory_id.clone();
    }

    match capability {
        BackendCapability::OCR => "tesseract_runtime".to_string(),
        BackendCapability::TTS => "specialized_tts_runtime".to_string(),
        BackendCapability::STT => "shared_stt_runtime".to_string(),
        BackendCapability::NLU => {
            if declared_roles.is_empty() {
                "local_nlu_runtime".to_string()
            } else {
                "declared_runtime".to_string()
            }
        }
        BackendCapability::FactCheck => {
            if declared_roles.is_empty() {
                "local_fact_check_runtime".to_string()
            } else {
                "declared_runtime".to_string()
            }
        }
        BackendCapability::Embedding | BackendCapability::Rerank => {
            if declared_roles.is_empty() {
                "candle_cpu_runtime".to_string()
            } else {
                "declared_runtime".to_string()
            }
        }
        BackendCapability::ImageGeneration => "specialized_image_runtime".to_string(),
        BackendCapability::LLM => {
            if declared_roles.iter().any(|role| role == "vlm") {
                "prime_multimodal_runtime".to_string()
            } else {
                "llama_cpp_runtime".to_string()
            }
        }
        _ => "runtime_binding".to_string(),
    }
}

pub async fn gateway_snapshot(State(state): State<AppState>) -> Json<GatewaySnapshot> {
    let cron_job_count = {
        #[cfg(feature = "cron")]
        {
            state
                .kernel
                .coordinator()
                .scheduler
                .get()
                .map(|s| s.list_jobs().len())
                .unwrap_or(0)
        }
        #[cfg(not(feature = "cron"))]
        {
            0
        }
    };

    let config = state.app_config.read();
    let connectors = vec![
        ConnectorStatus {
            name: "telegram".into(),
            configured: config.connectors.telegram.is_some(),
        },
        ConnectorStatus {
            name: "discord".into(),
            configured: config.connectors.discord.is_some(),
        },
    ];

    let mut vault_keys = state.kernel.vault().list_keys().unwrap_or_default();
    let standard = [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "DEEPSEEK_API_KEY",
        "MINIMAX_API_KEY",
    ];
    for k in standard {
        if !vault_keys.contains(&k.to_string()) && std::env::var(k).is_ok() {
            vault_keys.push(format!("{}(ENV)", k));
        }
    }

    let (model_ram_usage_mb, model_vram_usage_mb, whisper_loaded, piper_loaded) =
        if let Some(pool) = state.kernel.search_engine().model_pool() {
            let (ram, vram) = pool.current_usage();
            let w_loaded = pool.is_whisper_loaded();
            let p_loaded = pool.is_piper_loaded();
            (ram / 1024 / 1024, vram / 1024 / 1024, w_loaded, p_loaded)
        } else {
            (0, 0, false, false)
        };

    let model_dir = state.config_path.parent().unwrap().join("models");
    let nlu_status = format!("{:?}", state.nlu.status());
    let nlu_model = state.nlu.model_info();
    let nlu_mode = "Auto-Adaptive".to_string(); // Placeholder or real mode if we can get it

    let mut models = vec![ModelInfo {
        id: nlu_model.clone(),
        category: "NLU".into(),
        status: nlu_status.clone(),
        provider: "internal".into(),
    }];

    for (name, plugin) in state.kernel.sensory().audio_plugins() {
        let status = if plugin.is_loaded() {
            "Loaded".to_string()
        } else {
            "Installed".to_string()
        };
        if name.starts_with("whisper-") {
            models.push(ModelInfo {
                id: name.strip_prefix("whisper-").unwrap_or(&name).to_string(),
                category: "Sensory-STT".into(),
                status,
                provider: "Whisper (Candle)".into(),
            });
        } else if name.starts_with("piper-") {
            models.push(ModelInfo {
                id: name.strip_prefix("piper-").unwrap_or(&name).to_string(),
                category: "Sensory-TTS".into(),
                status,
                provider: "Piper".into(),
            });
        }
    }

    let available_whisper = ["tiny.en", "base", "small", "medium"];
    for id in available_whisper {
        if !models
            .iter()
            .any(|m| m.id == id && m.category == "Sensory-STT")
        {
            models.push(ModelInfo {
                id: id.to_string(),
                category: "Sensory-STT".into(),
                status: "Available".into(),
                provider: "Whisper (Candle)".into(),
            });
        }
    }

    let knowledge_bindings = [
        (
            "Knowledge-Embed",
            config
                .effective_global_model_binding("embedding", config.knowledge.embed_model.clone()),
        ),
        (
            "Knowledge-Rerank",
            config.effective_global_model_binding("rerank", config.knowledge.rerank_model.clone()),
        ),
    ];
    for (category, id) in knowledge_bindings {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let installed = model_dir.join(trimmed).exists();
        let loaded = if let Some(pool) = state.kernel.search_engine().model_pool() {
            pool.is_model_loaded(trimmed)
        } else {
            false
        };

        models.push(ModelInfo {
            id: trimmed.to_string(),
            category: category.into(),
            status: if loaded {
                "Loaded".into()
            } else if installed {
                "Installed".into()
            } else {
                "Configured".into()
            },
            provider: "Binding Resolver".into(),
        });
    }

    if let Ok(entries) = std::fs::read_dir(&model_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "gguf" || ext == "safetensors" {
                        let id = entry.file_name().to_string_lossy().to_string();
                        models.push(ModelInfo {
                            id,
                            category: "Brain".into(),
                            status: "Installed".into(),
                            provider: if ext == "gguf" {
                                "Local (Llama.cpp)".into()
                            } else {
                                "Local (Candle)".into()
                            },
                        });
                    }
                }
            } else if path.is_dir()
                && !path.to_string_lossy().contains("whisper")
                && !path.to_string_lossy().contains("piper")
            {
                if path.join("model.safetensors").exists() || path.join("config.json").exists() {
                    let id = entry.file_name().to_string_lossy().to_string();
                    if !models.iter().any(|m| m.id == id) {
                        models.push(ModelInfo {
                            id,
                            category: "Brain".into(),
                            status: "Installed".into(),
                            provider: "Local (Candle)".into(),
                        });
                    }
                }
            }
        }
    }

    let agents: Vec<String> = state
        .kernel
        .coordinator()
        .roles()
        .iter()
        .map(|r| r.name().to_string())
        .collect();
    let agent_count = agents.len();
    let session_agent_mapping_count = state.kernel.coordinator().active_agents().len();
    let model_ram_limit_gb = config.knowledge.model_ram_limit_gb;
    let model_vram_limit_gb = config.knowledge.model_vram_limit_gb;

    Json(GatewaySnapshot {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        agent_count,
        session_agent_mapping_count,
        skill_count: state.kernel.skill_loader().manuals.len(),
        cron_job_count,
        connectors,
        custom_providers: config.providers.custom_providers.clone(),
        vault_keys,
        agents,
        model_ram_usage_mb,
        model_vram_usage_mb,
        model_ram_limit_gb,
        model_vram_limit_gb,
        whisper_status: if whisper_loaded {
            "Loaded".into()
        } else {
            "Available".into()
        },
        piper_status: if piper_loaded {
            "Loaded".into()
        } else {
            "Available".into()
        },
        auto_consolidation_enabled: config.knowledge.auto_consolidation_enabled,
        enable_global_voice: config.sensory.enable_global_voice,
        enable_local_vision: false,
        local_vision_status: local_vision_runtime_surface(&state).1,
        nlu_status,
        nlu_mode,
        nlu_model,
        fact_check_enabled: config.sensory.fact_check_enabled,
        fact_check_status: "Active".into(),
        image_gen_model: config.sensory.image_gen_model.clone().unwrap_or_default(),
        image_gen_status: state.kernel.image_gen().model_info(),
        models,
    })
}

pub async fn runtime_mode(State(state): State<AppState>) -> Json<RuntimeModeDto> {
    let config = state.app_config.read();
    let auto_consolidation_enabled = config.knowledge.auto_consolidation_enabled;
    let enable_global_voice = config.sensory.enable_global_voice;
    let enable_local_vision = false;
    let vision_model = config.sensory.vision_model.clone().unwrap_or_default();
    let image_edit_model = config.sensory.image_edit_model.clone().unwrap_or_default();
    let audio_understanding_model = config
        .sensory
        .audio_understanding_model
        .clone()
        .unwrap_or_default();
    let realtime_vad_model = config
        .sensory
        .realtime_vad_model
        .clone()
        .unwrap_or_default();
    let duplex_voice_model = config
        .sensory
        .duplex_voice_model
        .clone()
        .unwrap_or_default();
    let local_classifier_model = config
        .sensory
        .local_classifier_model
        .clone()
        .unwrap_or_default();
    let local_router_model = config
        .sensory
        .local_router_model
        .clone()
        .unwrap_or_default();
    let local_safety_model = config
        .sensory
        .local_safety_model
        .clone()
        .unwrap_or_default();
    let image_gen_model = config.sensory.image_gen_model.clone().unwrap_or_default();
    let model_ram_limit_gb = config.knowledge.model_ram_limit_gb;
    let model_vram_limit_gb = config.knowledge.model_vram_limit_gb;
    let llama_cpp_runtime = config.llama_cpp_runtime.clone();
    let windows_ml_runtime = config.windows_ml_runtime.clone();
    drop(config);
    let (_, local_vision_status) = local_vision_runtime_surface(&state);

    Json(RuntimeModeDto {
        gateway_version: env!("CARGO_PKG_VERSION"),
        connected: true,
        model_ram_limit_gb,
        model_vram_limit_gb,
        auto_consolidation_enabled,
        enable_global_voice,
        enable_local_vision,
        local_vision_status,
        vision_model,
        image_edit_model,
        audio_understanding_model,
        realtime_vad_model,
        duplex_voice_model,
        local_classifier_model,
        local_router_model,
        local_safety_model,
        nlu_status: format!("{:?}", state.nlu.status()),
        fact_check_status: if enable_global_voice {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        },
        image_gen_model,
        image_gen_status: state.kernel.image_gen().model_info(),
        llama_cpp_runtime,
        windows_ml_runtime,
    })
}

pub async fn local_model_stack(
    State(state): State<AppState>,
) -> Result<Json<LocalModelStackDto>, AppError> {
    let config = state.app_config.read().clone();
    let server_status = llama_server_status_from_restart_command(
        &config.runtime_host_control.main_brain.restart_command,
    );
    let model_dir = state.config_path.parent().unwrap().join("models");
    let hw = HardwareStatus::detect();
    let windows_native = detect_windows_native_runtime_status();
    let main_brain_provider = main_brain_execution_provider(&hw);
    let auto_consolidation_enabled = config.knowledge.auto_consolidation_enabled;
    let voice_enabled = config.sensory.enable_global_voice;
    let vision_binding = config.effective_global_model_binding(
        "vision",
        config.sensory.vision_model.clone().unwrap_or_default(),
    );
    let (vision_runtime_ready, local_vision_status) = local_vision_runtime_surface(&state);
    let main_brain_model = config
        .agents
        .get("benshu")
        .and_then(|agent| {
            agent
                .local_model_artifact
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    agent
                        .model
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                })
        })
        .unwrap_or_default()
        .to_string();

    let mut entries = vec![
        role_binding_entry(
            "benshu",
            main_brain_model,
            BackendCapability::LLM,
            "foreground_prime_agent",
            "windows_native_main_brain",
            "llama.cpp_openai_compatible",
            main_brain_provider.clone(),
            &windows_native,
            Some("provider_bridge_unavailable"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "slm",
            config.effective_global_model_binding(
                "slm_tactical",
                config.sensory.tactical_model.clone().unwrap_or_default(),
            ),
            BackendCapability::LLM,
            "tactical_prepass",
            "windows_native_main_brain",
            "llama.cpp",
            main_brain_provider.clone(),
            &windows_native,
            Some("passthrough_to_main_llm"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "embedding",
            config
                .effective_global_model_binding("embedding", config.knowledge.embed_model.clone()),
            BackendCapability::Embedding,
            "knowledge_indexing",
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml",
            "cpu_today_directml_target",
            &windows_native,
            Some("null_embedding_backend"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "rerank",
            config.effective_global_model_binding("rerank", config.knowledge.rerank_model.clone()),
            BackendCapability::Rerank,
            "knowledge_rerank",
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml",
            "cpu_today_directml_target",
            &windows_native,
            Some("null_rerank_backend"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "nlu",
            model_dir
                .join("nlu")
                .join("optimal")
                .to_string_lossy()
                .to_string(),
            BackendCapability::NLU,
            "intent_understanding",
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml",
            "candle_today_directml_target",
            &windows_native,
            Some("llm_nlu_fallback"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "fact_check",
            config.effective_global_model_binding(
                "fact_check",
                config.sensory.fact_check_model.clone().unwrap_or_default(),
            ),
            BackendCapability::FactCheck,
            "factual_validation",
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml",
            "candle_today_directml_target",
            &windows_native,
            Some("llm_fact_check_fallback"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "stt",
            config.effective_global_model_binding(
                "speech_to_text",
                config.sensory.stt_model.clone().unwrap_or_default(),
            ),
            BackendCapability::STT,
            if voice_enabled {
                "shared_voice_runtime"
            } else {
                "disabled"
            },
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml",
            "specialized_runtime_today",
            &windows_native,
            Some("clarification_or_manual_review"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "tts",
            config.effective_global_model_binding(
                "text_to_speech",
                config.sensory.tts_model.clone().unwrap_or_default(),
            ),
            BackendCapability::TTS,
            if voice_enabled {
                "shared_voice_runtime"
            } else {
                "disabled"
            },
            "specialized_audio_runtime",
            "piper_or_specialized_tts_backend",
            "specialized_runtime",
            &windows_native,
            Some("text_only_response"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "vlm",
            vision_binding.clone(),
            BackendCapability::LLM,
            if !vision_binding.trim().is_empty() {
                "provider_or_bridge_media_perception"
            } else {
                "disabled"
            },
            "windows_native_main_brain",
            "prime_multimodal_backend",
            main_brain_provider.clone(),
            &windows_native,
            Some("attachment_fallback"),
            server_status.as_ref(),
        ),
        role_binding_entry(
            "ocr",
            config.effective_global_model_binding(
                "ocr",
                config.sensory.ocr_model.clone().unwrap_or_default(),
            ),
            BackendCapability::OCR,
            "document_ocr_runtime",
            "windows_native_small_model_layer",
            "onnx_runtime_directml_winml_or_tesseract",
            "tesseract_today_directml_target",
            &windows_native,
            Some("pdf_text_or_manual_review"),
            server_status.as_ref(),
        ),
    ];

    let image_gen_entry = role_binding_entry(
        "image_generation",
        config.effective_global_model_binding(
            "image_generation",
            config.sensory.image_gen_model.clone().unwrap_or_default(),
        ),
        BackendCapability::ImageGeneration,
        "creative_runtime",
        "specialized_creative_runtime",
        "specialized_image_generation_backend",
        "creative_runtime",
        &windows_native,
        Some("text_only_generation"),
        server_status.as_ref(),
    );
    entries.push(image_gen_entry);

    Ok(Json(LocalModelStackDto {
        host_runtime: windows_native.host_runtime,
        deployment_lane: windows_native.deployment_lane,
        deployment_strategy: windows_native.deployment_strategy,
        deployment_note: windows_native.deployment_note,
        product_mainline: windows_native.product_mainline,
        validation_tracks: windows_native.validation_tracks,
        windows_native_priority: windows_native.windows_native_priority,
        small_model_runtime_target: windows_native.small_model_runtime_target,
        small_model_execution_linked: windows_native.small_model_execution_linked,
        small_model_execution_provider: windows_native.small_model_execution_provider,
        small_model_device_target: windows_native.small_model_device_target,
        small_model_fallback_mode: windows_native.small_model_fallback_mode,
        small_model_runtime_outcome: windows_native.small_model_runtime_outcome,
        small_model_runtime_strategy: windows_native.small_model_runtime_strategy,
        small_model_runtime_readiness: windows_native.small_model_runtime_readiness,
        small_model_runtime_reason: windows_native.small_model_runtime_reason,
        main_brain_runtime_target: windows_native.main_brain_runtime_target,
        model_pool_loaded_count: state
            .kernel
            .search_engine()
            .model_pool()
            .as_ref()
            .map(|pool| pool.loaded_models_count())
            .unwrap_or_default(),
        model_pool_loaded_models: state
            .kernel
            .search_engine()
            .model_pool()
            .as_ref()
            .map(|pool| pool.list_loaded_models())
            .unwrap_or_default(),
        entries,
        media_runtime: MediaRuntimeSurfaceDto {
            global_voice_enabled: voice_enabled,
            local_vision_enabled: vision_runtime_ready,
            local_vision_status,
            source_contracts: vec![
                "direct_image".to_string(),
                "pdf_page_image".to_string(),
                "video_frame_image".to_string(),
            ],
            followup_contracts: vec![
                "attachment_fallback".to_string(),
                "alternate_model_fallback".to_string(),
                "clarification_or_manual_review".to_string(),
                if auto_consolidation_enabled {
                    "background_consolidation_enabled".to_string()
                } else {
                    "background_consolidation_disabled".to_string()
                },
            ],
        },
    }))
}

pub async fn local_model_artifacts(
    State(state): State<AppState>,
) -> Result<Json<LocalModelArtifactCatalogDto>, AppError> {
    let config = state.app_config.read().clone();
    let server_status = llama_server_status_from_restart_command(
        &config.runtime_host_control.main_brain.restart_command,
    );
    let (catalog_root, scan_roots) = local_model_artifact_roots(&state.config_path);
    Ok(Json(local_model_artifact_catalog(
        &catalog_root,
        &scan_roots,
        server_status.as_ref(),
    )))
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelPoolUnloadRequest {
    pub model_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalModelPoolPruneRequest {
    pub idle_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelPoolReport {
    pub action: String,
    pub available: bool,
    pub requested_model_id: Option<String>,
    pub unloaded: bool,
    pub unloaded_count: usize,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub note: String,
}

fn model_pool_unavailable_report(
    action: &str,
    requested_model_id: Option<String>,
) -> LocalModelPoolReport {
    LocalModelPoolReport {
        action: action.to_string(),
        available: false,
        requested_model_id,
        unloaded: false,
        unloaded_count: 0,
        before: Vec::new(),
        after: Vec::new(),
        note: "local model pool is not available for this runtime".to_string(),
    }
}

pub async fn local_model_pool_unload(
    State(state): State<AppState>,
    Json(payload): Json<LocalModelPoolUnloadRequest>,
) -> Json<LocalModelPoolReport> {
    let requested = payload.model_id.trim().to_string();
    let Some(pool) = state.kernel.search_engine().model_pool() else {
        return Json(model_pool_unavailable_report("unload", Some(requested)));
    };

    let before = pool.list_loaded_models();
    let unloaded = if requested.is_empty() {
        false
    } else {
        pool.unload_model(&requested)
    };
    let after = pool.list_loaded_models();
    Json(LocalModelPoolReport {
        action: "unload".to_string(),
        available: true,
        requested_model_id: Some(requested.clone()),
        unloaded,
        unloaded_count: usize::from(unloaded),
        before,
        after,
        note: if unloaded {
            "model unloaded from the shared local model pool; this does not stop the active main-brain runtime host".to_string()
        } else if requested.is_empty() {
            "model_id was empty".to_string()
        } else {
            "model was not loaded in the shared local model pool; the active main-brain runtime host is controlled by system shutdown/runtime host control".to_string()
        },
    })
}

pub async fn local_model_pool_prune(
    State(state): State<AppState>,
    Json(payload): Json<LocalModelPoolPruneRequest>,
) -> Json<LocalModelPoolReport> {
    let Some(pool) = state.kernel.search_engine().model_pool() else {
        return Json(model_pool_unavailable_report("prune", None));
    };

    let before = pool.list_loaded_models();
    let idle_seconds = payload.idle_seconds.unwrap_or(0);
    let unloaded_count = pool.prune(idle_seconds);
    let after = pool.list_loaded_models();
    Json(LocalModelPoolReport {
        action: "prune".to_string(),
        available: true,
        requested_model_id: None,
        unloaded: unloaded_count > 0,
        unloaded_count,
        before,
        after,
        note: format!(
            "pruned shared local model pool entries idle for at least {idle_seconds}s; this does not stop the active main-brain runtime host"
        ),
    })
}

pub async fn local_model_pool_clear(State(state): State<AppState>) -> Json<LocalModelPoolReport> {
    let Some(pool) = state.kernel.search_engine().model_pool() else {
        return Json(model_pool_unavailable_report("clear", None));
    };

    let before = pool.list_loaded_models();
    let unloaded_count = pool.clear();
    let after = pool.list_loaded_models();
    Json(LocalModelPoolReport {
        action: "clear".to_string(),
        available: true,
        requested_model_id: None,
        unloaded: unloaded_count > 0,
        unloaded_count,
        before,
        after,
        note: "cleared the shared local model pool; this does not stop the active main-brain runtime host. Use full system shutdown to stop BenShu-managed runtime hosts.".to_string(),
    })
}

#[derive(Serialize)]
pub struct RuntimeHostRestartReport {
    pub role: String,
    pub control_mode: String,
    pub started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_context_size: Option<u32>,
    pub stdout: String,
    pub stderr: String,
    pub note: String,
}

pub async fn restart_runtime_host(
    State(state): State<AppState>,
    AxumPath(role): AxumPath<String>,
) -> Result<Json<RuntimeHostRestartReport>, AppError> {
    let config = state.app_config.read().clone();
    let normalized_role = role.trim().to_ascii_lowercase();
    let (role_name, control) = match normalized_role.as_str() {
        "main_brain" | "main-brain" | "benshu" => {
            ("main_brain", config.runtime_host_control.main_brain)
        }
        "windows_ml" | "windows-ml" => ("windows_ml", config.runtime_host_control.windows_ml),
        other => {
            return Err(AppError(anyhow::anyhow!(
                "Unsupported runtime host role: {other}"
            )))
        }
    };

    let report = restart_configured_runtime_host(role_name, &control).await?;
    if report.started {
        sync_runtime_config_after_host_restart_with_base_url(
            &state,
            role_name,
            report.actual_base_url.as_deref(),
        )
        .await;
    }
    Ok(Json(report))
}

pub(crate) async fn sync_runtime_config_after_host_restart_with_base_url(
    state: &AppState,
    role_name: &str,
    actual_base_url: Option<&str>,
) {
    let active_roles = state.kernel.coordinator().get_active_roles();
    let mut loaded_config =
        match benshu_brain::config::AppConfig::load_from_file(&state.config_path) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(
                    target: "benshu::runtime_host_control",
                    role = role_name,
                    error = %error,
                    "Runtime host restarted, but refreshed runtime config could not be loaded."
                );
                return;
            }
        };

    if matches!(role_name, "main_brain" | "main-brain" | "benshu") {
        if let Some(base_url) = actual_base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let agent = loaded_config
                .agents
                .entry("benshu".to_string())
                .or_default();
            if agent.base_url.as_deref() != Some(base_url) {
                agent.provider = Some("openai".to_string());
                agent.base_url = Some(base_url.to_string());
                if let Err(error) = loaded_config.save_to_file(&state.config_path) {
                    tracing::warn!(
                        target: "benshu::runtime_host_control",
                        role = role_name,
                        base_url,
                        error = %error,
                        "Runtime host restarted, but discovered base_url could not be persisted."
                    );
                } else {
                    tracing::info!(
                        target: "benshu::runtime_host_control",
                        role = role_name,
                        base_url,
                        "Runtime host discovered base_url was persisted and will be used by resolver reload."
                    );
                }
            }
        }
    }

    {
        let mut config = state.app_config.write();
        *config = loaded_config;
    }
    state.factory.shared_provider_pool.write().clear();

    if matches!(role_name, "main_brain" | "main-brain" | "benshu") {
        let mut reloaded_roles = HashSet::new();
        if let Err(error) = state.factory.reload_agent("benshu").await {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                error = %error,
                "Runtime host restarted, but BenShu agent reload failed."
            );
        }
        reloaded_roles.insert("benshu".to_string());

        for role in active_roles {
            let live_role_name = role.name().to_string();
            let normalized = live_role_name.to_ascii_lowercase();
            if !reloaded_roles.insert(normalized.clone()) {
                continue;
            }
            if let Err(error) = state.factory.reload_agent(&live_role_name).await {
                tracing::warn!(
                    target: "benshu::runtime_host_control",
                    role = %live_role_name,
                    error = %error,
                    "Runtime host restarted, but live agent reload failed."
                );
            }
        }
    }
}

pub async fn shutdown_handler(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::info!("Shutdown requested via API. Stopping managed runtime hosts before exit...");
    let state_snapshot = state.clone();

    tokio::spawn(async move {
        shutdown_project_runtime_surfaces(state_snapshot).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        std::process::exit(0);
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": "Gateway shutdown initiated; managed runtime hosts are stopping"
        })),
    )
}

async fn shutdown_project_runtime_surfaces(state: AppState) {
    if let Some(pool) = state.kernel.search_engine().model_pool() {
        let unloaded_count = pool.clear();
        tracing::info!(
            target: "benshu::runtime_host_control",
            unloaded_count,
            "Cleared shared local model pool during full system shutdown."
        );
    }

    let config_snapshot = state.app_config.read().clone();
    shutdown_managed_runtime_hosts(config_snapshot).await;
}

async fn shutdown_managed_runtime_hosts(config: benshu_brain::config::AppConfig) {
    let main_brain_stopped =
        shutdown_configured_runtime_host("main_brain", &config.runtime_host_control.main_brain)
            .await;
    if !main_brain_stopped {
        shutdown_inferred_llama_cpp_main_brain(&config).await;
    }

    let _ = shutdown_configured_runtime_host("windows_ml", &config.runtime_host_control.windows_ml)
        .await;
}

async fn shutdown_configured_runtime_host(
    role: &str,
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
) -> bool {
    let mode = control.control_mode.trim();
    if mode.is_empty() || mode.eq_ignore_ascii_case("disabled") {
        return false;
    }

    let Some(command) = derive_runtime_stop_command(control) else {
        tracing::warn!(
            target: "benshu::runtime_host_control",
            role,
            control_mode = mode,
            "Runtime host shutdown skipped because no stop command could be derived."
        );
        return false;
    };

    let role = role.to_string();
    let role_for_task = role.clone();
    let result =
        tokio::task::spawn_blocking(move || run_stop_command(&role_for_task, &command)).await;
    match result {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                role,
                error = %error,
                "Runtime host shutdown command failed."
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                role,
                error = %error,
                "Runtime host shutdown task failed."
            );
            false
        }
    }
}

async fn restart_configured_runtime_host(
    role: &str,
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
) -> Result<RuntimeHostRestartReport, AppError> {
    let mode = control.control_mode.trim().to_string();
    if mode.is_empty() || mode.eq_ignore_ascii_case("disabled") {
        return Ok(RuntimeHostRestartReport {
            role: role.to_string(),
            control_mode: mode,
            started: false,
            actual_base_url: None,
            actual_context_size: None,
            stdout: String::new(),
            stderr: String::new(),
            note: "runtime host control is disabled for this role".to_string(),
        });
    }
    let role_for_task = role.to_string();
    let control_for_task = control.clone();
    let timeout_secs = control.timeout_secs.max(1);
    let mode_for_task = mode.clone();
    let output = tokio::task::spawn_blocking(move || match mode_for_task.as_str() {
        "command" => run_restart_command(&role_for_task, &control_for_task, timeout_secs),
        "windows_service" => {
            restart_windows_service(&role_for_task, &control_for_task, timeout_secs)
        }
        other => Err(anyhow::anyhow!(
            "unsupported runtime host control mode for {}: {other}",
            role_for_task
        )),
    })
    .await
    .map_err(|error| AppError(anyhow::anyhow!("runtime host restart task failed: {error}")))??;

    Ok(RuntimeHostRestartReport {
        role: role.to_string(),
        control_mode: mode,
        started: true,
        actual_base_url: extract_runtime_base_url_from_restart_stdout(&output.stdout),
        actual_context_size: extract_runtime_context_size_from_restart_stdout(&output.stdout),
        stdout: output.stdout,
        stderr: output.stderr,
        note: "runtime host restart command completed".to_string(),
    })
}

pub(crate) async fn restart_main_brain_runtime_host_if_configured(state: &AppState) -> bool {
    let config = state.app_config.read().clone();
    let control = config.runtime_host_control.main_brain;
    match restart_configured_runtime_host("main_brain", &control).await {
        Ok(report) if report.started => {
            sync_runtime_config_after_host_restart_with_base_url(
                state,
                "main_brain",
                report.actual_base_url.as_deref(),
            )
            .await;
            true
        }
        Ok(report) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                role = "main_brain",
                note = %report.note,
                "Provider recovery skipped because runtime host control did not start a host."
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                role = "main_brain",
                error = %error.0,
                "Provider recovery restart failed."
            );
            false
        }
    }
}

fn restart_windows_service(
    role: &str,
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
    timeout_secs: u64,
) -> anyhow::Result<RuntimeCommandOutput> {
    if !cfg!(windows) {
        return Err(anyhow::anyhow!(
            "windows_service restart for {role} is only supported on Windows-native deployments"
        ));
    }
    let service_name = control
        .service_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing Windows service name for {role}"))?;
    let timeout_ms = timeout_secs.saturating_mul(1000);
    let service_name = service_name.replace('\'', "''");
    let command = format!(
        "Restart-Service -Name '{service_name}' -Force; $svc = Get-Service -Name '{service_name}'; $svc.WaitForStatus('Running', [TimeSpan]::FromMilliseconds({timeout_ms}))"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()?;
    let result = RuntimeCommandOutput::from_process_output(output);
    if result.status_success {
        Ok(result)
    } else {
        Err(anyhow::anyhow!(
            "failed to restart Windows service {service_name} for {role} (stdout={}, stderr={})",
            result.stdout,
            result.stderr
        ))
    }
}

fn run_restart_command(
    role: &str,
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
    timeout_secs: u64,
) -> anyhow::Result<RuntimeCommandOutput> {
    let program = control
        .restart_command
        .first()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing restart command for {role}"))?;
    let mut child = Command::new(program)
        .args(control.restart_command.iter().skip(1))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    loop {
        if child.try_wait()?.is_some() {
            let output = RuntimeCommandOutput::from_process_output(child.wait_with_output()?);
            if output.status_success {
                return Ok(output);
            }
            return Err(anyhow::anyhow!(
                "restart command failed for {role} (stdout={}, stderr={})",
                output.stdout,
                output.stderr
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = RuntimeCommandOutput::from_process_output(child.wait_with_output()?);
            return Err(anyhow::anyhow!(
                "restart command timed out for {role} after {timeout_secs}s (stdout={}, stderr={})",
                output.stdout,
                output.stderr
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn derive_runtime_stop_command(
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
) -> Option<Vec<String>> {
    if control.control_mode.eq_ignore_ascii_case("windows_service") {
        let service_name = control.service_name.as_deref()?.trim();
        if service_name.is_empty() {
            return None;
        }
        return Some(vec![
            powershell_program(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            format!(
                "Stop-Service -Name '{}' -Force -ErrorAction SilentlyContinue",
                service_name.replace('\'', "''")
            ),
        ]);
    }

    if !control.control_mode.eq_ignore_ascii_case("command") {
        return None;
    }

    let Some(restart_script) = command_arg_after(&control.restart_command, "-File") else {
        return derive_shell_runtime_stop_command(control);
    };
    let restart_script_path = PathBuf::from(restart_script);
    let script_name = restart_script_path.file_name()?.to_str()?;
    let stop_script_name = match script_name {
        "restart_llama_server_vulkan.ps1" => "stop_llama_server_vulkan.ps1",
        "restart_onnx_directml_image_bridge.ps1" => "stop_onnx_directml_image_bridge.ps1",
        "restart_image_bridge_service.ps1" => "stop_image_bridge_service.ps1",
        _ => return None,
    };
    let stop_script = restart_script_path.with_file_name(stop_script_name);
    let mut command = vec![
        control
            .restart_command
            .first()
            .cloned()
            .unwrap_or_else(powershell_program),
        "-NoProfile".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        stop_script.to_string_lossy().to_string(),
    ];

    if stop_script_name == "stop_llama_server_vulkan.ps1" {
        push_optional_command_arg(&mut command, &control.restart_command, "-PidFile");
        push_optional_command_arg(&mut command, &control.restart_command, "-Port");
        push_optional_command_arg(&mut command, &control.restart_command, "-Alias");
    } else {
        push_optional_command_arg(&mut command, &control.restart_command, "-PidFile");
    }

    Some(command)
}

fn derive_shell_runtime_stop_command(
    control: &benshu_brain::config::ManagedRuntimeHostConfig,
) -> Option<Vec<String>> {
    let restart_script = find_command_script_arg(
        &control.restart_command,
        &[
            "enable_windows_llama_bridge.sh",
            "enable_windows_image_bridge.sh",
            "enable_windows_directml_diffusers_image_bridge.sh",
            "enable_windows_onnx_image_bridge.sh",
            "enable_windows_comfyui_image_bridge.sh",
        ],
    )?;
    let restart_script_path = PathBuf::from(restart_script);
    let script_name = restart_script_path.file_name()?.to_str()?;
    let stop_script_name = match script_name {
        "enable_windows_llama_bridge.sh" => "disable_windows_llama_bridge.sh",
        "enable_windows_image_bridge.sh" => "disable_windows_image_bridge.sh",
        "enable_windows_directml_diffusers_image_bridge.sh" => "disable_windows_image_bridge.sh",
        "enable_windows_onnx_image_bridge.sh" => "disable_windows_image_bridge.sh",
        "enable_windows_comfyui_image_bridge.sh" => "disable_windows_image_bridge.sh",
        _ => return None,
    };
    let stop_script = restart_script_path.with_file_name(stop_script_name);
    Some(vec![
        shell_program_for_script(&stop_script),
        stop_script.to_string_lossy().to_string(),
    ])
}

async fn shutdown_inferred_llama_cpp_main_brain(config: &benshu_brain::config::AppConfig) {
    let Some(agent_cfg) = config.agents.get("benshu") else {
        return;
    };
    let Some(base_url) = agent_cfg
        .base_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return;
    };
    let Some((host, port)) = parse_runtime_base_url(base_url) else {
        return;
    };
    if !is_local_runtime_host(&host) {
        return;
    }
    let runtime_is_llama_like = agent_cfg.provider.as_deref().is_some_and(|value| {
        let lowered = value.to_ascii_lowercase();
        lowered.contains("llama") || lowered.contains("openai") || lowered.contains("local")
    }) || agent_cfg
        .local_model_artifact
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().ends_with(".gguf"))
        || agent_cfg
            .model
            .as_deref()
            .is_some_and(|value| value.to_ascii_lowercase().contains("llama"));
    if !runtime_is_llama_like {
        tracing::debug!(
            target: "benshu::runtime_host_control",
            base_url,
            "Skipping inferred main-brain shutdown because the configured runtime is not local llama-like."
        );
        return;
    }
    let Some(stop_script) = discover_windows_script_for_shutdown("stop_llama_server_vulkan.ps1")
    else {
        tracing::warn!(
            target: "benshu::runtime_host_control",
            base_url,
            "Could not find stop_llama_server_vulkan.ps1 for inferred main-brain shutdown."
        );
        return;
    };

    let alias = agent_cfg
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("benshu-main-brain");
    let command = vec![
        powershell_program(),
        "-NoProfile".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        stop_script.to_string_lossy().to_string(),
        "-Port".to_string(),
        port.to_string(),
        "-Alias".to_string(),
        alias.to_string(),
    ];

    let result =
        tokio::task::spawn_blocking(move || run_stop_command("main_brain_inferred", &command))
            .await;
    match result {
        Ok(Ok(())) => {
            tracing::info!(
                target: "benshu::runtime_host_control",
                "Inferred llama.cpp main-brain shutdown completed."
            );
        }
        Ok(Err(error)) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                error = %error,
                "Inferred llama.cpp main-brain shutdown failed."
            );
        }
        Err(error) => {
            tracing::warn!(
                target: "benshu::runtime_host_control",
                error = %error,
                "Inferred llama.cpp main-brain shutdown task failed."
            );
        }
    }
}

fn run_stop_command(role: &str, command: &[String]) -> anyhow::Result<()> {
    let output = run_runtime_command(role, command)?;
    if output.status_success {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "stop command failed for {role} (stdout={}, stderr={})",
            output.stdout,
            output.stderr
        ))
    }
}

struct RuntimeCommandOutput {
    status_success: bool,
    stdout: String,
    stderr: String,
}

impl RuntimeCommandOutput {
    fn from_process_output(output: std::process::Output) -> Self {
        Self {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }
    }
}

fn run_runtime_command(role: &str, command: &[String]) -> anyhow::Result<RuntimeCommandOutput> {
    let program = command
        .first()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing runtime command for {role}"))?;
    let output = Command::new(program)
        .args(command.iter().skip(1))
        .output()?;
    Ok(RuntimeCommandOutput::from_process_output(output))
}

fn command_arg_after(command: &[String], flag: &str) -> Option<String> {
    command
        .windows(2)
        .find(|pair| pair[0].eq_ignore_ascii_case(flag))
        .map(|pair| pair[1].clone())
}

fn find_command_script_arg(command: &[String], script_names: &[&str]) -> Option<String> {
    command.iter().find_map(|arg| {
        let path = Path::new(arg);
        let file_name = path.file_name()?.to_str()?;
        script_names
            .iter()
            .any(|name| file_name.eq_ignore_ascii_case(name))
            .then(|| arg.clone())
    })
}

fn shell_program_for_script(script: &Path) -> String {
    if script
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ps1"))
    {
        powershell_program()
    } else {
        "bash".to_string()
    }
}

fn push_optional_command_arg(target: &mut Vec<String>, source: &[String], flag: &str) {
    if let Some(value) = command_arg_after(source, flag).filter(|value| !value.trim().is_empty()) {
        target.push(flag.to_string());
        target.push(value);
    }
}

fn parse_runtime_base_url(base_url: &str) -> Option<(String, u16)> {
    let url = reqwest::Url::parse(base_url).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default()?;
    Some((host, port))
}

pub(crate) fn extract_runtime_base_url_from_restart_stdout(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix("URL=")
            .or_else(|| trimmed.strip_prefix("BASE_URL="))
            .or_else(|| trimmed.split_once("base_url=").map(|(_, value)| value));
        if let Some(value) = value {
            let candidate = value
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(|ch| matches!(ch, '"' | '\'' | ',' | ';' | ')'));
            if reqwest::Url::parse(candidate).is_ok() {
                return Some(candidate.trim_end_matches('/').to_string());
            }
        }

        if let Some((_, tail)) = trimmed.rsplit_once(" ready at ") {
            let candidate = tail
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(|ch| matches!(ch, '"' | '\'' | ',' | ';' | ')'));
            if reqwest::Url::parse(candidate).is_ok() {
                return Some(candidate.trim_end_matches('/').to_string());
            }
        }
    }
    None
}

pub(crate) fn extract_runtime_context_size_from_restart_stdout(stdout: &str) -> Option<u32> {
    for line in stdout.lines() {
        let lowered = line.to_ascii_lowercase();
        for marker in ["actual_n_ctx=", "n_ctx=", "ctx_size=", "context_size="] {
            let Some(index) = lowered.find(marker) else {
                continue;
            };
            let tail = &line[index + marker.len()..];
            let digits = tail
                .chars()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if let Ok(value) = digits.parse::<u32>() {
                if value > 0 {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn is_local_runtime_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    host == "localhost"
        || host == "127.0.0.1"
        || host == "0.0.0.0"
        || host == "::1"
        || host.starts_with("172.")
        || host.starts_with("192.168.")
        || host.starts_with("10.")
}

fn powershell_program() -> String {
    if cfg!(windows) {
        "powershell".to_string()
    } else {
        "powershell.exe".to_string()
    }
}

fn discover_windows_script_for_shutdown(script_name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let cwd = std::env::current_dir().ok();
    let mut candidates = Vec::new();
    if let Some(exe_dir) = exe_dir {
        candidates.push(exe_dir.join("scripts").join("windows").join(script_name));
        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.join("scripts").join("windows").join(script_name));
        }
    }
    if let Some(cwd) = cwd {
        candidates.push(cwd.join("scripts").join("windows").join(script_name));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("scripts").join("windows").join(script_name));
        }
    }
    candidates.into_iter().find(|path| path.exists())
}

pub async fn doctor_api_handler() -> Json<Vec<crate::doctor::DoctorCheckResult>> {
    Json(crate::doctor::check_all().await)
}

#[derive(Deserialize)]
pub struct RepairRequest {
    pub name: String,
}

pub async fn repair_api_handler(Json(payload): Json<RepairRequest>) -> Result<String, AppError> {
    crate::doctor::repair(&payload.name).await.map_err(AppError)
}

pub async fn system_update_handler() -> (StatusCode, &'static str) {
    tracing::info!("System update triggered");
    (StatusCode::ACCEPTED, "System update triggered")
}

#[derive(Serialize)]
pub struct SwarmSummary {
    pub agents: Vec<String>,
    pub board: std::collections::HashMap<String, String>,
}

pub async fn swarm_summary(State(state): State<AppState>) -> Json<SwarmSummary> {
    let mut agents = Vec::new();

    // 1. Get agents from coordinator
    for entry in state.kernel.coordinator().roles() {
        agents.push(entry.name().to_string());
    }

    // 2. Add some "Board" info (Global state)
    let mut board = std::collections::HashMap::new();
    board.insert("Status".to_string(), "A2A Active".to_string());
    board.insert("Transport".to_string(), "Memory/Bridge/Bus".to_string());

    #[cfg(feature = "cron")]
    if let Some(s) = state.kernel.coordinator().scheduler.get() {
        board.insert(
            "Scheduled Jobs".to_string(),
            s.list_jobs().len().to_string(),
        );
    }

    // 3. Real-time stats from MessageBus
    let stats = state.bus.get_stats();
    board.insert("Total Inbound".to_string(), stats.inbound_total.to_string());
    board.insert(
        "Total Outbound".to_string(),
        stats.outbound_total.to_string(),
    );
    board.insert(
        "Total A2A Messages".to_string(),
        stats.comm_total.to_string(),
    );

    let is_throttled = state
        .kernel
        .coordinator()
        .get_metabolic_pressure()
        .is_throttled;
    board.insert(
        "Throttling".to_string(),
        if is_throttled { "Throttled" } else { "Normal" }.to_string(),
    );

    Json(SwarmSummary { agents, board })
}

#[derive(Deserialize)]
pub struct ThrottleRequest {
    pub tenant_id: Option<String>,
    pub agent_role: Option<String>,
    pub limit: u32,
}

pub async fn set_swarm_throttle(
    State(state): State<AppState>,
    Json(payload): Json<ThrottleRequest>,
) -> StatusCode {
    use benshu_brain::agent::multi_agent::AgentRole;

    if let Some(tenant) = payload.tenant_id {
        state
            .kernel
            .coordinator()
            .set_tenant_throttle(&tenant, payload.limit);
    } else if let Some(role_name) = payload.agent_role {
        if let Ok(role) = role_name.parse::<AgentRole>() {
            state
                .kernel
                .coordinator()
                .set_agent_throttle(&role, payload.limit);
        } else {
            return StatusCode::BAD_REQUEST;
        }
    } else {
        return StatusCode::BAD_REQUEST;
    }

    StatusCode::OK
}

#[derive(Deserialize)]
pub struct ModelLoadRequest {
    pub model_id: String,
}

pub async fn download_model(Json(payload): Json<ModelLoadRequest>) -> StatusCode {
    tracing::info!("Downloading model: {}", payload.model_id);
    StatusCode::ACCEPTED
}

pub async fn load_model(
    State(state): State<AppState>,
    Json(payload): Json<ModelLoadRequest>,
) -> StatusCode {
    if let Some(pool) = state.kernel.search_engine().model_pool() {
        if pool.is_model_loaded(&payload.model_id) {
            return StatusCode::OK;
        }
        StatusCode::ACCEPTED
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

pub async fn cancel_handler(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let count = state.cancel_tokens.len();
    for token_ref in state.cancel_tokens.iter() {
        token_ref.value().cancel();
    }
    state.cancel_tokens.clear();

    tracing::info!("Cancelled {} active task(s)", count);
    (
        axum::http::StatusCode::OK,
        format!("Cancelled {} active task(s)", count),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        derive_runtime_stop_command, extract_runtime_context_size_from_restart_stdout,
        manifest_metadata_entries, receipt_metadata_entries,
    };
    use benshu_brain::config::ManagedRuntimeHostConfig;
    use benshu_security::memory_backup::MemoryBackupFileEntry;
    use benshu_security::{MemoryRestorePointManifest, MemoryRestoreReceipt};
    use chrono::Utc;

    #[test]
    fn derives_stop_command_for_wsl_llama_bridge_restart_script() {
        let control = ManagedRuntimeHostConfig {
            control_mode: "command".to_string(),
            restart_command: vec![
                "env".to_string(),
                "BENSHU_WINDOWS_LLAMA_SERVER_EXE=D:\\llama.cpp\\llama-server.exe".to_string(),
                "bash".to_string(),
                "/home/biubiuboy/BenShu/scripts/wsl/enable_windows_llama_bridge.sh".to_string(),
            ],
            ..ManagedRuntimeHostConfig::default()
        };

        let command = derive_runtime_stop_command(&control)
            .expect("wsl bridge restart command should derive a stop command");

        assert_eq!(command.first().map(String::as_str), Some("bash"));
        assert!(command
            .iter()
            .any(|arg| arg.ends_with("disable_windows_llama_bridge.sh")));
    }

    #[test]
    fn extracts_runtime_context_size_from_restart_stdout() {
        let stdout = "\
bridge ready at http://172.18.176.1:18013/v1
llama.cpp registry: actual_n_ctx=40960 configured_ctx=98304
";

        assert_eq!(
            extract_runtime_context_size_from_restart_stdout(stdout),
            Some(40_960)
        );
    }

    #[test]
    fn manifest_metadata_entries_include_summary_and_json() {
        let manifest = MemoryRestorePointManifest {
            backup_id: "backup-1".to_string(),
            product: "BenShu".to_string(),
            contract_version: "1".to_string(),
            created_at: Utc::now(),
            storage_root_hint: "/tmp/benshu".to_string(),
            encryption_key_fingerprint: "fingerprint-1".to_string(),
            file_count: 1,
            total_bytes: 42,
            files: vec![MemoryBackupFileEntry {
                label: "engram".to_string(),
                relative_path: "data/engram.redb".to_string(),
                payload_path: "payloads/data/engram.redb.sealed".to_string(),
                size_bytes: 42,
                sha256: "deadbeef".to_string(),
            }],
        };

        let entries = match manifest_metadata_entries(&manifest) {
            Ok(entries) => entries,
            Err(err) => panic!("manifest metadata failed: {}", err.0),
        };
        assert!(entries
            .iter()
            .any(|(key, value)| key.ends_with(".last_backup_id") && value == "backup-1"));
        assert!(entries
            .iter()
            .any(|(key, value)| key.ends_with(".last_manifest_json")
                && value.contains("\"backup_id\":\"backup-1\"")
                && value.contains("\"file_count\":1")));
    }

    #[test]
    fn receipt_metadata_entries_include_summary_and_json() {
        let receipt = MemoryRestoreReceipt {
            receipt_id: "receipt-1".to_string(),
            backup_id: "backup-1".to_string(),
            restored_at: Utc::now(),
            contract_version: "1".to_string(),
            encryption_key_fingerprint: "fingerprint-1".to_string(),
            restored_files: 2,
            restored_bytes: 128,
        };

        let entries = match receipt_metadata_entries(&receipt) {
            Ok(entries) => entries,
            Err(err) => panic!("receipt metadata failed: {}", err.0),
        };
        assert!(entries
            .iter()
            .any(|(key, value)| key.ends_with(".last_receipt_id") && value == "receipt-1"));
        assert!(entries
            .iter()
            .any(|(key, value)| key.ends_with(".last_receipt_json")
                && value.contains("\"receipt_id\":\"receipt-1\"")
                && value.contains("\"restored_files\":2")));
    }
}
