use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use crate::SkillLoader;
use benshu_brain::agent::multi_agent::{AgentRole, Coordinator, WorkerBlueprint};
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};

const MAX_SKILL_MANUAL_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
struct SkillManagerArgs {
    action: SkillManagerAction,
    #[serde(default)]
    skill_name: Option<String>,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    confirmed: bool,
    #[serde(default)]
    worker_role: Option<String>,
    #[serde(default)]
    test_args: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SkillManagerAction {
    List,
    Resolve,
    Install,
}

#[derive(Debug, Clone, Serialize)]
struct SkillCandidate {
    name: String,
    repository: String,
    source_url: String,
    description: String,
    score: i32,
}

pub struct SkillManagerTool {
    loader: Arc<SkillLoader>,
    data_dir: PathBuf,
    coordinator: Weak<Coordinator>,
}

impl SkillManagerTool {
    pub fn new(
        loader: Arc<SkillLoader>,
        data_dir: PathBuf,
        _enabled_tools: Arc<parking_lot::RwLock<HashSet<String>>>,
        coordinator: Weak<Coordinator>,
    ) -> Self {
        Self {
            loader,
            data_dir,
            coordinator,
        }
    }

    fn skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    fn agents_dir(&self) -> PathBuf {
        self.data_dir.join("agents")
    }

    fn slug(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut prev_dash = false;
        for ch in value.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
                prev_dash = false;
            } else if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        out.trim_matches('-').to_string()
    }

    fn safe_skill_name(value: &str) -> anyhow::Result<String> {
        let slug = Self::slug(value);
        if slug.is_empty() {
            anyhow::bail!("skill name is empty after sanitization");
        }
        if slug.len() > 80 {
            anyhow::bail!("skill name is too long after sanitization");
        }
        Ok(slug)
    }

    fn worker_role(skill_name: &str, explicit: Option<&str>) -> String {
        if let Some(role) = explicit.map(str::trim).filter(|role| !role.is_empty()) {
            return Self::slug(role).replace('-', "_");
        }
        let slug = Self::slug(skill_name)
            .strip_suffix("-skill")
            .unwrap_or(&Self::slug(skill_name))
            .to_string();
        format!("{}_worker", slug.replace('-', "_"))
    }

    async fn resolve(&self, requested_name: &str) -> anyhow::Result<Vec<SkillCandidate>> {
        if requested_name.trim().is_empty() {
            return Err(anyhow::anyhow!("skill_name is required"));
        }

        if requested_name.starts_with("https://github.com/") {
            let source_url = github_repo_to_raw_skill_url(requested_name)
                .unwrap_or_else(|| requested_name.to_string());
            let content = fetch_text(&source_url).await?;
            let parsed_name = parse_skill_name(&content).unwrap_or_else(|| {
                requested_name
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("custom-skill")
                    .to_string()
            });
            return Ok(vec![SkillCandidate {
                name: parsed_name,
                repository: requested_name.to_string(),
                source_url,
                description: parse_skill_description(&content).unwrap_or_default(),
                score: 10_000,
            }]);
        }

        let client = reqwest::Client::builder()
            .user_agent("benshu-skill-manager/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let mut candidates = Vec::new();
        for query in skill_search_queries(requested_name) {
            let url = format!(
                "https://api.github.com/search/repositories?q={}&per_page=12",
                urlencoding::encode(&query)
            );
            let value: serde_json::Value = client
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let Some(items) = value.get("items").and_then(|items| items.as_array()) else {
                continue;
            };
            for item in items {
                let Some(full_name) = item.get("full_name").and_then(|v| v.as_str()) else {
                    continue;
                };
                if candidates
                    .iter()
                    .any(|candidate: &SkillCandidate| candidate.repository.ends_with(full_name))
                {
                    continue;
                }
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let Some(source_url) = first_existing_skill_url(full_name).await else {
                    continue;
                };
                let content = fetch_text(&source_url).await.unwrap_or_default();
                let name = parse_skill_name(&content).unwrap_or_else(|| {
                    full_name
                        .rsplit('/')
                        .next()
                        .unwrap_or("custom-skill")
                        .to_string()
                });
                let parsed_description =
                    parse_skill_description(&content).unwrap_or_else(|| description.clone());
                candidates.push(SkillCandidate {
                    score: score_candidate(requested_name, full_name, &name, &parsed_description),
                    name,
                    repository: format!("https://github.com/{full_name}"),
                    source_url,
                    description: parsed_description,
                });
            }
            if !candidates.is_empty() {
                break;
            }
        }

        candidates.sort_by(|left, right| right.score.cmp(&left.score));
        candidates.truncate(5);
        Ok(candidates)
    }

    async fn install(
        &self,
        skill_name: Option<&str>,
        source_url: Option<&str>,
        worker_role: Option<&str>,
        test_args: Option<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let (source_url, resolved_name_raw) = if let Some(source_url) = source_url {
            let raw_url =
                github_repo_to_raw_skill_url(source_url).unwrap_or_else(|| source_url.to_string());
            let content = fetch_text(&raw_url).await?;
            (
                raw_url,
                parse_skill_name(&content)
                    .or_else(|| skill_name.map(ToString::to_string))
                    .unwrap_or_else(|| "custom-skill".to_string()),
            )
        } else {
            let requested = skill_name.ok_or_else(|| anyhow::anyhow!("skill_name is required"))?;
            let candidates = self.resolve(requested).await?;
            let best = candidates.first().ok_or_else(|| {
                anyhow::anyhow!("No installable skill source found for {}", requested)
            })?;
            (best.source_url.clone(), best.name.clone())
        };
        let resolved_name = Self::safe_skill_name(&resolved_name_raw)?;

        let skills_root = self.skills_dir();
        tokio::fs::create_dir_all(&skills_root).await?;
        let skill_dir = skills_root.join(&resolved_name);
        if skill_dir.exists() {
            tokio::fs::remove_dir_all(&skill_dir).await?;
        }
        tokio::fs::create_dir_all(&skill_dir).await?;
        let skill_file = skill_dir.join("SKILL.md");
        let content = fetch_text(&source_url).await?;
        tokio::fs::write(&skill_file, content).await?;
        ensure_instruction_adapter(&skill_dir, &skill_file).await?;

        self.loader.load_all().await?;

        let role = self.create_worker(&resolved_name, worker_role).await?;
        let test_result = if let Some(args) = test_args {
            self.loader
                .skills
                .get(&resolved_name)
                .map(|skill| skill.clone())
                .map(|skill| async move { skill.call(&args.to_string()).await })
        } else {
            None
        };
        let test_result = match test_result {
            Some(fut) => Some(match fut.await {
                Ok(value) => json!({"ok": true, "result": value}),
                Err(err) => json!({"ok": false, "error": err.to_string()}),
            }),
            None => None,
        };

        Ok(json!({
            "status": "installed",
            "skill_name": resolved_name,
            "source_url": source_url,
            "worker_role": role,
            "api_key_hint": required_env_hint(&skill_file).await?,
            "test_result": test_result,
            "next_step": "If api_key_hint is present, configure that secret in the panel/vault, then ask BenShu to delegate to this worker."
        }))
    }

    fn list_installed(&self) -> serde_json::Value {
        let manuals: Vec<_> = self
            .loader
            .manual_summaries()
            .into_iter()
            .map(|summary| {
                json!({
                    "name": summary.name,
                    "description": summary.description,
                    "runtime": summary.runtime,
                    "executable": summary.executable,
                    "classification": summary.classification,
                    "execution_surface": summary.execution_surface,
                    "loaded": true,
                    "enabled": true,
                })
            })
            .collect();
        json!({
            "status": "ok",
            "installed_count": manuals.len(),
            "skills": manuals,
        })
    }

    fn is_inventory_query(value: &str) -> bool {
        let normalized = value.trim().to_ascii_lowercase();
        matches!(
            normalized.as_str(),
            "skill"
                | "skills"
                | "installed skill"
                | "installed skills"
                | "local skill"
                | "local skills"
                | "list skills"
                | "skill list"
                | "inventory"
                | "本地技能"
                | "已安装技能"
                | "技能列表"
        ) || normalized.contains("installed")
            || normalized.contains("已安装")
            || normalized.contains("本地")
            || normalized.contains("列表")
    }

    async fn create_worker(
        &self,
        skill_name: &str,
        explicit_role: Option<&str>,
    ) -> anyhow::Result<String> {
        let role = Self::worker_role(skill_name, explicit_role);
        if role.is_empty() || role.len() > 80 {
            anyhow::bail!("worker_role is empty or exceeds the 80 character safety limit");
        }
        let role_dir = self.agents_dir().join(&role);
        tokio::fs::create_dir_all(&role_dir).await?;
        let name = skill_name.replace(['-', '_'], " ");
        let content = format!(
            "---\nname: BenShu {name} Worker\ntemperature: 0.2\ndescription: Single-responsibility worker for the installed `{skill_name}` skill.\ntools:\n  - {skill_name}\n---\n\n# {name} Worker\n\nYou are a single-responsibility skill worker.\n\n- Use only `{skill_name}` for delegated tasks that explicitly need this installed skill.\n- If the skill requires an API key or external account and it is missing, return a concise blocker naming the missing environment variable.\n- Return compact structured results for BenShu to synthesize.\n"
        );
        tokio::fs::write(role_dir.join("AGENT.md"), content).await?;

        if let Some(coordinator) = self.coordinator.upgrade() {
            let role_obj = AgentRole::Custom(role.clone());
            coordinator.unregister_agent(&role_obj);
            coordinator.register_worker_blueprint(WorkerBlueprint {
                role: role_obj,
                agent_path: role_dir,
                display_name: format!("BenShu {name} Worker"),
                description: Some(format!(
                    "Single-responsibility worker for the installed `{skill_name}` skill."
                )),
                tools: vec![skill_name.to_string()],
                artifact_policy: None,
            });
        }

        Ok(role)
    }
}

#[async_trait]
impl Tool for SkillManagerTool {
    fn name(&self) -> String {
        "skill_manager".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Resolve, confirm, install, enable, and equip third-party BenShu skills by name or URL. Use resolve first when the user only provides a skill name; install only after explicit user confirmation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list", "resolve", "install"]},
                    "skill_name": {"type": "string", "description": "User-facing skill name, such as BlockBeats."},
                    "source_url": {"type": "string", "description": "Confirmed GitHub repository URL or raw SKILL.md URL."},
                    "confirmed": {"type": "boolean", "description": "Must be true before installation. If false, return candidates and ask the user to confirm."},
                    "worker_role": {"type": "string", "description": "Optional worker role to create/equip. Defaults to <skill>_worker."},
                    "test_args": {"type": "object", "description": "Optional smoke-test arguments passed to the installed skill after install."}
                },
                "required": ["action"]
            }),
            parameters_ts: Some("type SkillManagerArgs = { action: 'list' | 'resolve' | 'install'; skill_name?: string; source_url?: string; confirmed?: boolean; worker_role?: string; test_args?: Record<string, unknown> }".to_string()),
            usage_guidelines: Some("Use action=list for local inventory queries such as installed/enabled skills. When the user gives only a skill name for installation, call action=resolve and present the best candidate source for confirmation. Never install unless confirmed=true or the user has explicitly confirmed the selected source. After install, report the worker role and any required API key environment variable.".to_string()),
            safety_level: SafetyLevel::Yellow,
            is_binary: false,
            is_verified: true,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: SkillManagerArgs = serde_json::from_str(arguments)?;
        match args.action {
            SkillManagerAction::List => Ok(serde_json::to_string_pretty(&self.list_installed())?),
            SkillManagerAction::Resolve => {
                let name = args
                    .skill_name
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("skill_name is required"))?;
                if Self::is_inventory_query(name) {
                    return Ok(serde_json::to_string_pretty(&self.list_installed())?);
                }
                let candidates = self.resolve(name).await?;
                Ok(serde_json::to_string_pretty(&json!({
                    "status": "needs_confirmation",
                    "requested": name,
                    "candidates": candidates,
                    "instruction": "Ask the user to confirm the intended skill/source before calling action=install with confirmed=true."
                }))?)
            }
            SkillManagerAction::Install => {
                let source_hint = args.source_url.as_deref().unwrap_or_default();
                let skill_hint = args.skill_name.as_deref().unwrap_or_default();
                if Self::looks_like_inventory_install_mistake(source_hint)
                    || Self::is_inventory_query(skill_hint)
                {
                    return Ok(serde_json::to_string_pretty(&self.list_installed())?);
                }
                if !args.confirmed {
                    let name = args.skill_name.as_deref().unwrap_or("the requested skill");
                    let candidates = self.resolve(name).await.unwrap_or_default();
                    return Ok(serde_json::to_string_pretty(&json!({
                        "status": "blocked_confirmation_required",
                        "requested": name,
                        "candidates": candidates,
                        "instruction": "Do not install yet. Ask the user to confirm this source."
                    }))?);
                }
                if args
                    .source_url
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    let name = args.skill_name.as_deref().unwrap_or("the requested skill");
                    let candidates = self.resolve(name).await.unwrap_or_default();
                    return Ok(serde_json::to_string_pretty(&json!({
                        "status": "blocked_source_required",
                        "requested": name,
                        "candidates": candidates,
                        "instruction": "Installation requires a confirmed source_url. Show candidates and ask the user to confirm the exact source before installing."
                    }))?);
                }
                let result = self
                    .install(
                        args.skill_name.as_deref(),
                        args.source_url.as_deref(),
                        args.worker_role.as_deref(),
                        args.test_args,
                    )
                    .await?;
                Ok(serde_json::to_string_pretty(&result)?)
            }
        }
    }
}

impl SkillManagerTool {
    fn looks_like_inventory_install_mistake(source_url: &str) -> bool {
        let lower = source_url.trim().to_ascii_lowercase();
        [
            "inventory/list",
            "installed/skills",
            "local/skills",
            "skill/list",
            "skills/list",
            "list/skills",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    }
}

fn github_repo_to_raw_skill_url(input: &str) -> Option<String> {
    let input = input.trim().trim_end_matches('/');
    let path = input.strip_prefix("https://github.com/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    if parts.len() >= 5 && parts[2] == "tree" {
        let branch = parts[3];
        let sub_path = parts[4..].join("/");
        return Some(format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}/SKILL.md",
            parts[0], parts[1], branch, sub_path
        ));
    }
    Some(format!(
        "https://raw.githubusercontent.com/{}/{}/main/SKILL.md",
        parts[0], parts[1]
    ))
}

fn skill_search_queries(requested_name: &str) -> Vec<String> {
    let trimmed = requested_name.trim();
    let slug = SkillManagerTool::slug(trimmed);
    let mut queries = Vec::new();

    for query in [
        format!("{trimmed} skill"),
        format!("{trimmed} SKILL.md"),
        trimmed.to_string(),
        format!("{slug} skill"),
    ] {
        if !query.trim().is_empty() && !queries.iter().any(|seen| seen == &query) {
            queries.push(query);
        }
    }

    queries
}

async fn first_existing_skill_url(full_name: &str) -> Option<String> {
    for branch in ["main", "master"] {
        let url = format!("https://raw.githubusercontent.com/{full_name}/{branch}/SKILL.md");
        if fetch_text(&url).await.is_ok() {
            return Some(url);
        }
    }
    None
}

async fn fetch_text(url: &str) -> anyhow::Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https" {
        anyhow::bail!("skill source must use https: {}", url);
    }

    let response = reqwest::Client::builder()
        .user_agent("benshu-skill-manager/1.0")
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?;

    if let Some(length) = response.content_length() {
        if length as usize > MAX_SKILL_MANUAL_BYTES {
            anyhow::bail!(
                "remote SKILL.md is larger than the {}KB safety limit",
                MAX_SKILL_MANUAL_BYTES / 1024
            );
        }
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > MAX_SKILL_MANUAL_BYTES {
            anyhow::bail!(
                "remote SKILL.md exceeded the {}KB safety limit while downloading",
                MAX_SKILL_MANUAL_BYTES / 1024
            );
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8(bytes)?)
}

fn parse_skill_name(content: &str) -> Option<String> {
    parse_frontmatter_value(content, "name")
}

fn parse_skill_description(content: &str) -> Option<String> {
    parse_frontmatter_value(content, "description")
}

fn parse_frontmatter_value(content: &str, key: &str) -> Option<String> {
    let yaml = content
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .map(|(yaml, _)| yaml)?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).ok()?;
    value
        .as_mapping()?
        .get(serde_yaml_ng::Value::String(key.to_string()))?
        .as_str()
        .map(ToString::to_string)
}

fn score_candidate(requested: &str, repo: &str, skill_name: &str, description: &str) -> i32 {
    let req = SkillManagerTool::slug(requested);
    let repo_l = repo.to_ascii_lowercase();
    let skill_l = skill_name.to_ascii_lowercase();
    let desc_l = description.to_ascii_lowercase();
    let mut score = 0;
    if skill_l.contains(&req) {
        score += 1000;
    }
    if repo_l.contains(&req) {
        score += 800;
    }
    if repo_l.contains("official") {
        score += 400;
    }
    if desc_l.contains(&req) {
        score += 200;
    }
    if skill_l.contains("skill") {
        score += 100;
    }
    score
}

async fn ensure_instruction_adapter(skill_dir: &Path, skill_file: &Path) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(skill_file).await?;
    let (metadata, _) = crate::compiler::SkillParser::parse_str(&content, skill_dir)?;
    if metadata.script.is_some() {
        return Ok(());
    }

    let scripts_dir = skill_dir.join("scripts");
    tokio::fs::create_dir_all(&scripts_dir).await?;
    let script_name = "benshu_instruction_http_adapter.py";
    tokio::fs::write(scripts_dir.join(script_name), instruction_adapter_script()).await?;
    let updated = inject_adapter_frontmatter(&content, script_name)?;
    tokio::fs::write(skill_file, updated).await?;
    Ok(())
}

fn inject_adapter_frontmatter(content: &str, script_name: &str) -> anyhow::Result<String> {
    let rest = content
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow::anyhow!("SKILL.md must start with YAML frontmatter"))?;
    let (yaml, body) = rest
        .split_once("\n---")
        .ok_or_else(|| anyhow::anyhow!("SKILL.md frontmatter is not closed"))?;
    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml)?;
    let mapping = value
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("SKILL.md frontmatter must be a mapping"))?;
    mapping.insert("runtime".into(), "python3".into());
    mapping.insert("script".into(), script_name.into());
    mapping.insert("kind".into(), "tool".into());
    mapping.insert(
        "parameters".into(),
        serde_yaml_ng::to_value(json!({
            "type": "object",
            "properties": {
                "action": {"type": "string"},
                "endpoint_path": {"type": "string"},
                "query": {"type": "string"},
                "lang": {"type": "string"},
                "size": {"type": "integer"},
                "page": {"type": "integer"},
                "params": {"type": "object"}
            }
        }))?,
    );
    mapping.insert(
        "permissions".into(),
        serde_yaml_ng::to_value(json!({"network": true, "filesystem": "read_skill"}))?,
    );
    mapping.insert(
        "resources".into(),
        serde_yaml_ng::to_value(json!({"timeout_secs": 30, "max_output_bytes": 131072}))?,
    );
    Ok(format!(
        "---\n{}---\n{}",
        serde_yaml_ng::to_string(&value)?,
        body.trim_start_matches('\n')
    ))
}

async fn required_env_hint(skill_file: &Path) -> anyhow::Result<Option<String>> {
    let content = tokio::fs::read_to_string(skill_file).await?;
    Ok(
        regex::Regex::new(r"primaryEnv:\s*([A-Za-z_][A-Za-z0-9_]*)")?
            .captures(&content)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string())),
    )
}

fn instruction_adapter_script() -> &'static str {
    r#"#!/usr/bin/env python3
import html, json, os, re, sys, urllib.parse, urllib.request

def args():
    if len(sys.argv) < 2:
        return {}
    try:
        value = json.loads(sys.argv[1])
        return value if isinstance(value, dict) else {"query": str(value)}
    except Exception:
        return {"query": sys.argv[1]}

def manual():
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(os.path.dirname(here), "SKILL.md"), "r", encoding="utf-8") as f:
        return f.read()

def clean(value):
    text = html.unescape(str(value or ""))
    text = re.sub(r"<[^>]+>", " ", text)
    return re.sub(r"\s+", " ", text).strip()

def base_url(doc):
    m = re.search(r"Base URL\*\*:\s*`([^`]+)`", doc)
    if m: return m.group(1).rstrip("/")
    urls = re.findall(r"https://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+", doc)
    if not urls: raise RuntimeError("No HTTPS API base URL found in SKILL.md")
    p = urllib.parse.urlparse(urls[0])
    return f"{p.scheme}://{p.netloc}"

def env_name(doc):
    m = re.search(r"primaryEnv:\s*([A-Za-z_][A-Za-z0-9_]*)", doc)
    if m: return m.group(1)
    m = re.search(r"\$([A-Z][A-Z0-9_]*_API_KEY|[A-Z][A-Z0-9_]*API_KEY)", doc)
    return m.group(1) if m else ""

def header_name(doc):
    if "api-key" in doc: return "api-key"
    if "X-API-Key" in doc: return "X-API-Key"
    return "Authorization"

def requests(a):
    lang, size, page = a.get("lang") or "en", int(a.get("size") or 10), int(a.get("page") or 1)
    query = a.get("query") or a.get("name") or ""
    if a.get("endpoint_path"):
        return [{"path": a["endpoint_path"], "params": a.get("params") or {}}]
    action = (a.get("action") or "latest_newsflash").lower()
    mapping = {
        "latest": "/v1/newsflash", "latest_news": "/v1/newsflash", "latest_newsflash": "/v1/newsflash",
        "newsflash": "/v1/newsflash", "important": "/v1/newsflash/important",
        "important_newsflash": "/v1/newsflash/important", "ai": "/v1/newsflash/ai",
        "ai_news": "/v1/newsflash/ai", "onchain": "/v1/newsflash/onchain",
        "financing": "/v1/newsflash/financing", "prediction": "/v1/newsflash/prediction",
        "latest_articles": "/v1/article", "article": "/v1/article", "articles": "/v1/article",
    }
    if action == "search":
        return [{"path": "/v1/search", "params": {"name": query, "size": size, "lang": lang}}]
    if action in mapping:
        return [{"path": mapping[action], "params": {"page": page, "size": size, "lang": lang}}]
    if query:
        return [{"path": "/v1/search", "params": {"name": query, "size": size, "lang": lang}}]
    raise RuntimeError("No endpoint_path or supported action provided")

def call(base, path, params, header, key):
    if not path.startswith("/"): path = "/" + path
    url = base + path + (("?" + urllib.parse.urlencode(params)) if params else "")
    headers = {"Accept": "application/json", "User-Agent": "BenShu skill adapter/1.0"}
    if key: headers[header] = f"Bearer {key}" if header == "Authorization" else key
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw_bytes = resp.read(1024 * 1024 + 1)
        if len(raw_bytes) > 1024 * 1024:
            raise RuntimeError("response exceeded 1MB safety limit")
        raw = raw_bytes.decode(resp.headers.get_content_charset() or "utf-8", "replace")
    payload = json.loads(raw)
    data = payload.get("data", payload) if isinstance(payload, dict) else payload
    return {"url": url, "payload": payload, "compact": compact(data)}

def item(x):
    if not isinstance(x, dict): return x
    return {"title": clean(x.get("title") or x.get("name") or ""), "summary": clean(x.get("abstract") or x.get("description") or x.get("content") or ""), "time": x.get("time_cn") or x.get("create_time") or x.get("time") or "", "url": x.get("url") or x.get("link") or ""}

def compact(data):
    if isinstance(data, dict):
        rows = data.get("list") or data.get("data") or data.get("items") or data.get("rows")
        if isinstance(rows, list):
            data = dict(data)
            data["items"] = [item(v) for v in rows[:10]]
    elif isinstance(data, list):
        data = [item(v) for v in data[:10]]
    return data

def main():
    a, doc = args(), manual()
    base, env, header = base_url(doc), env_name(doc), header_name(doc)
    key = os.environ.get(env, "") if env else ""
    out, errors = [], []
    for r in requests(a):
        try: out.append(call(base, r["path"], r.get("params") or {}, header, key))
        except Exception as e: errors.append({"path": r.get("path"), "error": f"{type(e).__name__}: {e}"})
    print(json.dumps({"ok": bool(out) and not errors, "base_url": base, "used_env": env if key else "", "results": out, "errors": errors}, ensure_ascii=False))

if __name__ == "__main__":
    main()
"#
}
