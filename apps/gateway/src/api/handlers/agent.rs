use crate::api::state::{AppError, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AgentRuntimeConfigDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_model_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_mmproj_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_runtime_family: Option<String>,
}

#[derive(Serialize)]
pub struct FileDto {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<AgentRuntimeConfigDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_policy: Option<Value>,
}

#[derive(Deserialize)]
pub struct FileUpdateDto {
    pub content: String,
    #[serde(default)]
    pub runtime: Option<AgentRuntimeConfigDto>,
    #[serde(default)]
    pub artifact_policy: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AgentArtifactPolicyDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_policy: Option<Value>,
    pub yaml: String,
    pub source: String,
}

#[derive(Deserialize)]
pub struct AgentArtifactPolicyUpdateDto {
    #[serde(default)]
    pub artifact_policy: Option<Value>,
    #[serde(default)]
    pub yaml: Option<String>,
}

#[derive(Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub name: Option<String>,
}

fn validate_agent_role(role: &str) -> Result<(), AppError> {
    if role.contains("..") || role.contains('/') || role.contains('\\') || role.trim().is_empty() {
        return Err(AppError(anyhow::anyhow!("Invalid role parameter")));
    }
    Ok(())
}

fn base_agent_path(state: &AppState) -> PathBuf {
    let config = state.app_config.read();
    let base_dir = state
        .config_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    config
        .agent_path
        .clone()
        .unwrap_or_else(|| base_dir.join("agents"))
}

fn agent_role_dir(state: &AppState, role: &str) -> PathBuf {
    base_agent_path(state).join(role)
}

pub async fn list_agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentSummary>>, AppError> {
    let dir = {
        let config = state.app_config.read();
        let base_dir = state
            .config_path
            .parent()
            .unwrap_or(std::path::Path::new("."));
        config
            .agent_path
            .clone()
            .unwrap_or_else(|| base_dir.join("agents"))
    };

    let mut summaries = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_dir() {
                let id = entry.file_name().to_string_lossy().to_string();

                // Try to extract name from AGENT.md
                let agent_path = entry.path().join("AGENT.md");
                let name = if let Ok(content) = tokio::fs::read_to_string(&agent_path).await {
                    let (ovr, _) =
                        benshu_brain::config::AgentConfigOverrides::parse_frontmatter(&content);
                    ovr.name
                } else {
                    None
                };

                summaries.push(AgentSummary { id, name });
            }
        }
    }

    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(summaries))
}

#[derive(serde::Deserialize)]
pub struct AgentParams {
    pub role: String,
}

#[axum::debug_handler]
pub async fn get_agent(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
) -> Result<Json<FileDto>, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;

    let base_agent_path = base_agent_path(&state);

    let role_dir = base_agent_path.join(&role);
    let agent_path = role_dir.join("AGENT.md");
    // Attempt normal read, then lowercase fallback
    let (content, policy_root) = if let Ok(c) = tokio::fs::read_to_string(&agent_path).await {
        (Some(c), role_dir.clone())
    } else {
        let low_root = base_agent_path.join(role.to_lowercase());
        let low_path = low_root.join("AGENT.md");
        (tokio::fs::read_to_string(&low_path).await.ok(), low_root)
    };

    let content = match content {
        Some(c) => c,
        None => {
            let role_lower = role.to_lowercase();
            if let Some(t) = state
                .agent_templates
                .iter()
                .find(|t| t.name.to_lowercase() == role_lower)
            {
                let tools_yaml = t
                    .tools
                    .iter()
                    .map(|s| format!("  - {}", s))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "---\nname: {}\ntemperature: {}\ntools:\n{}\n# Custom Endpoint: https://your-custom-endpoint.com/v1\n---\n{}",
                    t.name, t.temperature, tools_yaml, t.body
                )
            } else {
                format!(
                    "# Agent Identity ({})\nWho are you, and what are your primary responsibilities?\n\n## Identity\nYou are a highly capable AI assistant operating under the {} role.\n\n## Behavior Guidelines\n- Respond concisely and accurately.\n- Use your tools to fetch necessary context before answering.\n",
                    role.to_uppercase(),
                    role
                )
            }
        }
    };

    let artifact_policy = read_artifact_policy_file(&policy_root.join("artifact_policy.yaml"))
        .await
        .or_else(|| read_agent_frontmatter_artifact_policy(&content));

    let runtime = {
        let config = state.app_config.read();
        let mut ovr = config.agents.get(&role).cloned().unwrap_or_default();
        let local_fields_missing = ovr.local_model_artifact.is_none()
            && ovr.local_mmproj_artifact.is_none()
            && ovr.local_runtime_family.is_none();

        if local_fields_missing {
            if let Ok(raw) = std::fs::read_to_string(&state.config_path) {
                if let Ok(file_config) =
                    serde_yaml_ng::from_str::<benshu_brain::config::AppConfig>(&raw)
                {
                    if let Some(file_ovr) = file_config.agents.get(&role) {
                        if ovr.provider.is_none() {
                            ovr.provider = file_ovr.provider.clone();
                        }
                        if ovr.base_url.is_none() {
                            ovr.base_url = file_ovr.base_url.clone();
                        }
                        if ovr.model.is_none() {
                            ovr.model = file_ovr.model.clone();
                        }
                        if ovr.local_model_artifact.is_none() {
                            ovr.local_model_artifact = file_ovr.local_model_artifact.clone();
                        }
                        if ovr.local_mmproj_artifact.is_none() {
                            ovr.local_mmproj_artifact = file_ovr.local_mmproj_artifact.clone();
                        }
                        if ovr.local_runtime_family.is_none() {
                            ovr.local_runtime_family = file_ovr.local_runtime_family.clone();
                        }
                    }
                }
            }
        }

        if ovr.is_runtime_empty() {
            None
        } else {
            Some(AgentRuntimeConfigDto {
                provider: ovr.provider,
                base_url: ovr.base_url,
                model: ovr.model,
                local_model_artifact: ovr.local_model_artifact,
                local_mmproj_artifact: ovr.local_mmproj_artifact,
                local_runtime_family: ovr.local_runtime_family,
            })
        }
    };

    Ok(Json(FileDto {
        content,
        runtime,
        artifact_policy,
    }))
}

#[axum::debug_handler]
pub async fn put_agent(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
    Json(payload): Json<FileUpdateDto>,
) -> Result<StatusCode, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;

    let dir = agent_role_dir(&state, &role);

    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join("AGENT.md");
    let (content_without_policy, extracted_policy) =
        strip_frontmatter_artifact_policy(&payload.content);
    let artifact_policy = payload.artifact_policy.or(extracted_policy);
    tokio::fs::write(&path, &content_without_policy).await?;
    if artifact_policy.is_some() {
        write_artifact_policy_file(&dir.join("artifact_policy.yaml"), artifact_policy).await?;
    }

    {
        let mut config = state.app_config.write();
        let runtime = payload.runtime.unwrap_or_default();
        let entry = config.agents.entry(role.clone()).or_default();
        *entry = benshu_brain::config::AgentConfigOverrides {
            provider: runtime.provider,
            base_url: runtime.base_url,
            model: runtime.model,
            local_model_artifact: runtime.local_model_artifact,
            local_mmproj_artifact: runtime.local_mmproj_artifact,
            local_runtime_family: runtime.local_runtime_family,
            ..Default::default()
        };
        if entry.is_runtime_empty() {
            config.agents.remove(&role);
        }
        config.sanitize_agent_runtime_overrides();
        config.save_to_file(&state.config_path)?;
    }

    if role.eq_ignore_ascii_case("benshu") {
        if let Err(e) = state.factory.reload_agent(&role).await {
            tracing::error!("Failed to reload agent '{}' after update: {}", role, e);
        }
    } else if let Err(e) = state.factory.load_worker_blueprint(&role).await {
        tracing::error!(
            "Failed to refresh worker blueprint '{}' after update: {}",
            role,
            e
        );
    }

    Ok(StatusCode::OK)
}

#[axum::debug_handler]
pub async fn get_agent_artifact_policy(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
) -> Result<Json<AgentArtifactPolicyDto>, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;

    let role_dir =
        resolve_existing_role_dir(&state, &role).unwrap_or_else(|| agent_role_dir(&state, &role));
    let policy_path = role_dir.join("artifact_policy.yaml");

    if let Some(policy) = read_artifact_policy_file(&policy_path).await {
        return Ok(Json(artifact_policy_dto(
            Some(policy),
            "artifact_policy.yaml",
        )));
    }

    let agent_path = role_dir.join("AGENT.md");
    let frontmatter_policy = match tokio::fs::read_to_string(&agent_path).await {
        Ok(content) => read_agent_frontmatter_artifact_policy(&content),
        Err(_) => None,
    };

    Ok(Json(artifact_policy_dto(
        frontmatter_policy,
        "agent_frontmatter",
    )))
}

#[axum::debug_handler]
pub async fn put_agent_artifact_policy(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
    Json(payload): Json<AgentArtifactPolicyUpdateDto>,
) -> Result<StatusCode, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;

    let dir = agent_role_dir(&state, &role);
    tokio::fs::create_dir_all(&dir).await?;

    let policy = if let Some(yaml) = payload.yaml {
        benshu_brain::config::AgentConfigOverrides::parse_artifact_policy_yaml(&yaml)
            .map_err(|err| AppError(anyhow::anyhow!(err)))?
    } else {
        payload.artifact_policy
    };

    write_artifact_policy_file(&dir.join("artifact_policy.yaml"), policy).await?;
    remove_frontmatter_artifact_policy_file(&dir.join("AGENT.md")).await?;
    refresh_agent_runtime(&state, &role).await;

    Ok(StatusCode::OK)
}

fn artifact_policy_dto(policy: Option<Value>, source: &str) -> AgentArtifactPolicyDto {
    let yaml = policy
        .as_ref()
        .map(|value| {
            benshu_brain::config::AgentConfigOverrides {
                artifact_policy: Some(value.clone()),
                ..Default::default()
            }
            .artifact_policy_yaml()
        })
        .unwrap_or_default();
    AgentArtifactPolicyDto {
        artifact_policy: policy,
        yaml,
        source: source.to_string(),
    }
}

fn resolve_existing_role_dir(state: &AppState, role: &str) -> Option<PathBuf> {
    let base = base_agent_path(state);
    let exact = base.join(role);
    if exact.exists() {
        return Some(exact);
    }
    let lowercase = base.join(role.to_lowercase());
    lowercase.exists().then_some(lowercase)
}

async fn read_artifact_policy_file(path: &Path) -> Option<Value> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    let value = serde_yaml_ng::from_str::<Value>(&content).ok()?;
    if value.is_null() {
        None
    } else if value.get("artifact_policy").is_some() {
        value.get("artifact_policy").cloned()
    } else {
        Some(value)
    }
}

fn read_agent_frontmatter_artifact_policy(content: &str) -> Option<Value> {
    let (overrides, _) = benshu_brain::config::AgentConfigOverrides::parse_frontmatter(content);
    overrides.artifact_policy
}

fn strip_frontmatter_artifact_policy(content: &str) -> (String, Option<Value>) {
    let (mut overrides, body) =
        benshu_brain::config::AgentConfigOverrides::parse_frontmatter(content);
    let policy = overrides.artifact_policy.take();
    (overrides.to_markdown(&body), policy)
}

async fn write_artifact_policy_file(path: &Path, policy: Option<Value>) -> Result<(), AppError> {
    let Some(policy) = policy else {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path).await?;
        }
        return Ok(());
    };
    let should_remove = policy
        .get("handles")
        .and_then(Value::as_array)
        .is_some_and(|handles| handles.is_empty())
        && policy.get("terms").is_none();
    if policy.is_null() || should_remove {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path).await?;
        }
        return Ok(());
    }
    let yaml = serde_yaml_ng::to_string(&policy)
        .map_err(|err| AppError(anyhow::anyhow!("Invalid artifact policy: {err}")))?;
    tokio::fs::write(path, yaml).await?;
    Ok(())
}

async fn remove_frontmatter_artifact_policy_file(path: &Path) -> Result<(), AppError> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Ok(());
    };
    let (content_without_policy, extracted_policy) = strip_frontmatter_artifact_policy(&content);
    if extracted_policy.is_some() && content_without_policy != content {
        tokio::fs::write(path, content_without_policy).await?;
    }
    Ok(())
}

async fn refresh_agent_runtime(state: &AppState, role: &str) {
    if role.eq_ignore_ascii_case("benshu") {
        if let Err(e) = state.factory.reload_agent(role).await {
            tracing::error!(
                "Failed to reload agent '{}' after policy update: {}",
                role,
                e
            );
        }
    } else if let Err(e) = state.factory.load_worker_blueprint(role).await {
        tracing::error!(
            "Failed to refresh worker blueprint '{}' after policy update: {}",
            role,
            e
        );
    }
}

pub async fn delete_agent(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;

    if role == "benshu" {
        return Err(AppError(anyhow::anyhow!(
            "Cannot delete the primary core agent 'benshu'"
        )));
    }

    let dir = agent_role_dir(&state, &role);

    if dir.exists() {
        tokio::fs::remove_dir_all(&dir).await?;
    }
    state.factory.unload_worker_blueprint(&role);
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ExportAgentRequest {
    #[allow(dead_code)]
    pub limit: usize,
}

pub async fn export_agent(
    Query(params): Query<AgentParams>,
    State(state): State<AppState>,
    Json(payload): Json<ExportAgentRequest>,
) -> Result<Json<benshu_brain::agent::layered_agent::vessel_pack::VesselPackage>, AppError> {
    let role = params.role;
    validate_agent_role(&role)?;
    if role == "benshu" {
        return Err(AppError(anyhow::anyhow!(
            "The primary core agent 'benshu' cannot be exported"
        )));
    }

    let role_dir = agent_role_dir(&state, &role);

    if !role_dir.exists() {
        return Err(AppError(anyhow::anyhow!(
            "Role directory not found: {}",
            role
        )));
    }

    let memory = state.kernel.coordinator().memory.get().map(|m| m.as_ref());
    let user_id = "default";

    let package = benshu_brain::agent::layered_agent::vessel_pack::VesselPackage::pack(
        &role_dir,
        Some("BenShu User".to_string()),
        memory,
        user_id,
        payload.limit,
        Some(state.kernel.security().as_ref()),
    )
    .await?;

    Ok(Json(package))
}

#[derive(Deserialize)]
pub struct ImportAgentRequest {
    pub vessel_json: String,
}

#[axum::debug_handler]
pub async fn import_agent(
    State(state): State<AppState>,
    Json(payload): Json<ImportAgentRequest>,
) -> Result<StatusCode, AppError> {
    let package = benshu_brain::agent::layered_agent::vessel_pack::VesselPackage::from_json(
        &payload.vessel_json,
    )?;
    if package.metadata.role == "benshu" {
        return Err(AppError(anyhow::anyhow!(
            "The primary core agent 'benshu' cannot be imported"
        )));
    }

    let base_agent_path = {
        let config = state.app_config.read();
        let base_dir = state
            .config_path
            .parent()
            .unwrap_or(std::path::Path::new("."));
        config
            .agent_path
            .clone()
            .unwrap_or_else(|| base_dir.join("agents"))
    };

    let memory = state.kernel.coordinator().memory.get().map(|m| m.as_ref());
    let user_id = Some("default");

    let auditor = state
        .factory
        .evolution_manager
        .as_ref()
        .map(|m| m.auditor());
    let inspector: Option<&dyn benshu_brain::security::VesselInspector> = auditor
        .as_ref()
        .map(|a| a.as_ref() as &dyn benshu_brain::security::VesselInspector);

    let role = benshu_brain::agent::layered_agent::vessel_pack::VesselPackage::import_vessel(
        &payload.vessel_json,
        &base_agent_path,
        memory,
        user_id,
        inspector,
    )
    .await?;

    let _ = state.factory.load_worker_blueprint(&role).await;

    Ok(StatusCode::OK)
}

pub async fn agent_delete_route_todo() -> Result<(), AppError> {
    Ok(())
}

pub async fn get_agent_identity_templates(
    State(state): State<AppState>,
) -> Json<Vec<benshu_kernel::AgentTemplate>> {
    Json(state.agent_templates.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn artifact_policy_file_roundtrips_standalone_yaml() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("artifact_policy.yaml");
        if let Err(err) = write_artifact_policy_file(
            &path,
            Some(json!({
                "handles": [
                    {
                        "artifact": "web_page",
                        "triggers": ["search"]
                    }
                ]
            })),
        )
        .await
        {
            panic!("write policy: {}", err.0);
        }

        let policy = read_artifact_policy_file(&path)
            .await
            .expect("policy should read");
        assert_eq!(policy["handles"][0]["artifact"], "web_page");
        let raw = tokio::fs::read_to_string(path).await.expect("raw yaml");
        assert!(!raw.contains("artifact_policy:"));
    }

    #[tokio::test]
    async fn artifact_policy_save_removes_legacy_frontmatter_policy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("AGENT.md");
        tokio::fs::write(
            &path,
            "---\nname: Worker\nartifact_policy:\n  handles:\n    - artifact: stale\n---\n\n# Worker\n",
        )
        .await
        .expect("write agent");

        if let Err(err) = remove_frontmatter_artifact_policy_file(&path).await {
            panic!("remove legacy policy: {}", err.0);
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .expect("updated agent");
        assert!(!content.contains("artifact_policy:"));
        assert!(content.contains("name: Worker"));
        assert!(content.contains("# Worker"));
    }
}
