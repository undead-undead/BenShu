use crate::api::state::{AppError, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use benshu_brain::skills::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path as FsPath, PathBuf};

#[derive(Serialize)]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub runtime: Option<String>,
    pub homepage: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub dependencies: Vec<String>,
    pub kind: String,
}

pub async fn list_skills(State(state): State<AppState>) -> Result<Json<Vec<SkillDto>>, AppError> {
    let skill_loader = state.kernel.skill_loader();
    let mut skills: Vec<SkillDto> = skill_loader
        .manuals
        .iter()
        .map(|entry| {
            let meta = entry.value().metadata();
            let name = entry.key().clone();
            let version = meta
                .metadata
                .get("version")
                .and_then(|v| v.as_str())
                .map(String::from);
            let author = meta
                .metadata
                .get("author")
                .and_then(|v| v.as_str())
                .map(String::from);
            SkillDto {
                name,
                description: meta.description.clone(),
                enabled: true,
                runtime: meta.runtime.clone(),
                homepage: meta.homepage.clone(),
                version,
                author,
                dependencies: meta.dependencies.clone(),
                kind: meta.kind.clone(),
            }
        })
        .collect();
    skills.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(Json(skills))
}

pub async fn toggle_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    if !state.kernel.skill_loader().manuals.contains_key(&name) {
        return Err(AppError(anyhow::anyhow!("Skill not found")));
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RunSkillRequest {
    pub args: serde_json::Value,
}

#[derive(Serialize)]
pub struct RunSkillResponse {
    pub result: String,
}

pub async fn run_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<RunSkillRequest>,
) -> Result<Json<RunSkillResponse>, AppError> {
    let skill = state
        .kernel
        .skill_loader()
        .skills
        .get(&name)
        .ok_or_else(|| AppError(anyhow::anyhow!("Skill '{}' not found", name)))?;

    let args_str = serde_json::to_string(&payload.args)
        .map_err(|e| AppError(anyhow::anyhow!("Invalid arguments: {}", e)))?;

    let result = skill.call(&args_str).await.map_err(AppError)?;

    Ok(Json(RunSkillResponse { result }))
}

#[derive(Deserialize)]
pub struct InstallSkillRequest {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct InstallSkillResponse {
    pub success: bool,
    pub skill_name: String,
    pub status: String,
    pub message: String,
}

pub async fn install_skill(
    State(state): State<AppState>,
    Json(req): Json<InstallSkillRequest>,
) -> Result<Json<InstallSkillResponse>, AppError> {
    use tokio::fs;

    let skill_loader = state.kernel.skill_loader().clone();
    let local_source = resolve_local_skill_source(&req.url);
    let mut pairs = if local_source.is_some() {
        let default_name = local_source
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("local-skill")
            .to_string();
        vec![(default_name, req.url.trim().to_string())]
    } else {
        resolve_skill_urls(&req.url).map_err(|e| AppError(anyhow::anyhow!("{}", e)))?
    };
    if let Some(name) = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        for (skill_name, _) in &mut pairs {
            if skill_name == "custom-skill" {
                *skill_name = name.to_string();
            }
        }
    }

    let skills_dir = state.config_path.parent().unwrap().join("skills");
    fs::create_dir_all(&skills_dir).await?;

    if let Some(source_dir) = local_source {
        let mut installed_names = Vec::new();
        for (skill_name, _) in &pairs {
            let skill_dir = skills_dir.join(skill_name);
            if skill_dir.exists() {
                fs::remove_dir_all(&skill_dir).await?;
            }
            copy_skill_dir(&source_dir, &skill_dir).await?;
            ensure_instruction_skill_adapter(&skill_dir, &skill_dir.join("SKILL.md")).await?;
            installed_names.push(skill_name.clone());
        }

        skill_loader
            .load_all()
            .await
            .map_err(|e| AppError(anyhow::anyhow!(e.to_string())))?;
        let primary_name = installed_names
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        return Ok(Json(InstallSkillResponse {
            success: true,
            skill_name: primary_name,
            status: "ok".into(),
            message: format!("Installed: {}", installed_names.join(", ")),
        }));
    }

    let http = reqwest::Client::builder()
        .user_agent("benshu-gateway/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError(anyhow::anyhow!("HTTP client error: {}", e)))?;

    let mut installed_names = Vec::new();

    for (skill_name, primary_url) in &pairs {
        let mut candidates: Vec<String> = {
            let prefix = "https://raw.githubusercontent.com/";
            if let Some(path) = primary_url.strip_prefix(prefix) {
                let parts: Vec<&str> = path.splitn(4, '/').collect();
                if parts.len() >= 3 {
                    let base = format!("{}{}/{}/{}", prefix, parts[0], parts[1], parts[2]);
                    let owner = parts[0];
                    let owner_short = owner.split('-').next().unwrap_or(owner);
                    let stripped = skill_name
                        .strip_prefix(&format!("{}-", owner_short))
                        .unwrap_or(skill_name.as_str());

                    let mut c = Vec::new();
                    if primary_url.ends_with(".md") {
                        c.push(primary_url.clone());
                        if primary_url.contains("/main/") {
                            c.push(primary_url.replace("/main/", "/master/"));
                        }
                    }

                    let test_dirs = ["skills", ".claude/skills", ""];
                    for dir in test_dirs {
                        let b = if dir.is_empty() {
                            base.clone()
                        } else {
                            format!("{}/{}", base, dir)
                        };
                        c.push(format!("{}/{}/SKILL.md", b, skill_name));
                        if stripped != skill_name.as_str() {
                            c.push(format!("{}/{}/SKILL.md", b, stripped));
                        }
                        if b.contains("/main") {
                            let b_master = b.replace("/main", "/master");
                            c.push(format!("{}/{}/SKILL.md", b_master, skill_name));
                            if stripped != skill_name.as_str() {
                                c.push(format!("{}/{}/SKILL.md", b_master, stripped));
                            }
                        }
                    }
                    c
                } else {
                    vec![primary_url.clone()]
                }
            } else {
                let mut c = Vec::new();
                if primary_url.ends_with("SKILL.md") || primary_url.ends_with(".md") {
                    c.push(primary_url.clone());
                } else {
                    let trim_url = primary_url.trim_end_matches("/");
                    c.push(format!("{}/SKILL.md", trim_url));
                    c.push(format!("{}/main/SKILL.md", trim_url));
                }
                c
            }
        };
        if !candidates.contains(primary_url) {
            candidates.push(primary_url.clone());
        }

        let mut found_contents = Vec::new();
        for url in &candidates {
            match http.get(url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.text().await {
                    Ok(text) => {
                        found_contents.push((skill_name.clone(), url.clone(), text));
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read body from {}: {}", url, e);
                    }
                },
                _ => {}
            }
        }

        if found_contents.is_empty() {
            return Err(AppError(anyhow::anyhow!(
                "Failed to locate SKILL.md for {}",
                skill_name
            )));
        }

        for (name, source_url, content) in found_contents {
            let skill_dir = skills_dir.join(&name);
            fs::create_dir_all(&skill_dir).await?;
            let skill_file = skill_dir.join("SKILL.md");
            fs::write(&skill_file, content).await?;
            install_declared_skill_assets(&http, &skill_dir, &source_url, &skill_file).await?;
            installed_names.push(name);
        }
    }

    skill_loader
        .load_all()
        .await
        .map_err(|e| AppError(anyhow::anyhow!(e.to_string())))?;
    let primary_name = installed_names
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Json(InstallSkillResponse {
        success: true,
        skill_name: primary_name,
        status: "ok".into(),
        message: format!("Installed: {}", installed_names.join(", ")),
    }))
}

pub async fn uninstall_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let skills_dir = state.config_path.parent().unwrap().join("skills");
    let skill_dir = skills_dir.join(&name);

    if skill_dir.exists() {
        tokio::fs::remove_dir_all(&skill_dir).await?;
        state
            .kernel
            .skill_loader()
            .load_all()
            .await
            .map_err(|e| AppError(anyhow::anyhow!(e.to_string())))?;
        Ok(StatusCode::OK)
    } else {
        Err(AppError(anyhow::anyhow!("Skill not found")))
    }
}

fn resolve_skill_urls(input: &str) -> Result<Vec<(String, String)>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("URL cannot be empty".into());
    }

    if input.starts_with("https://github.com/") {
        let parts: Vec<&str> = input
            .strip_prefix("https://github.com/")
            .unwrap()
            .split('/')
            .collect();
        if parts.len() < 2 {
            return Err("Invalid Github URL".into());
        }

        let owner = parts[0];
        let repo = parts[1];

        if parts.len() >= 5 && parts[2] == "tree" {
            let branch = parts[3];
            let sub_path = parts[4..].join("/");
            let skill_name = parts.last().copied().unwrap_or(repo);
            let raw_url = format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}/SKILL.md",
                owner, repo, branch, sub_path
            );
            return Ok(vec![(skill_name.to_string(), raw_url)]);
        }

        let raw_url = format!(
            "https://raw.githubusercontent.com/{}/{}/main/SKILL.md",
            owner, repo
        );
        return Ok(vec![(repo.to_string(), raw_url)]);
    }

    Ok(vec![("custom-skill".into(), input.to_string())])
}

async fn install_declared_skill_assets(
    http: &reqwest::Client,
    skill_dir: &std::path::Path,
    source_url: &str,
    skill_file: &std::path::Path,
) -> Result<(), AppError> {
    let content = tokio::fs::read_to_string(skill_file).await?;
    let Ok((metadata, _)) =
        benshu_builtin_tools::compiler::SkillParser::parse_str(&content, skill_dir)
    else {
        return Ok(());
    };

    let Some(script) = metadata.script.as_deref() else {
        ensure_instruction_skill_adapter(skill_dir, skill_file).await?;
        return Ok(());
    };

    let Some(script_url) = sibling_skill_asset_url(source_url, &format!("scripts/{}", script))
    else {
        return Ok(());
    };

    let resp = http
        .get(&script_url)
        .send()
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to fetch skill script: {}", e)))?;
    if !resp.status().is_success() {
        return Err(AppError(anyhow::anyhow!(
            "Failed to fetch skill script '{}' from {}: HTTP {}",
            script,
            script_url,
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError(anyhow::anyhow!("Failed to read skill script body: {}", e)))?;
    let script_path = skill_dir.join("scripts").join(script);
    if let Some(parent) = script_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(script_path, bytes).await?;
    Ok(())
}

async fn ensure_instruction_skill_adapter(
    skill_dir: &FsPath,
    skill_file: &FsPath,
) -> Result<(), AppError> {
    let content = tokio::fs::read_to_string(skill_file).await?;
    let (metadata, _) = benshu_builtin_tools::compiler::SkillParser::parse_str(&content, skill_dir)
        .map_err(|e| AppError(anyhow::anyhow!(e.to_string())))?;
    if metadata.script.is_some() {
        return Ok(());
    }

    let script_name = "benshu_instruction_http_adapter.py";
    let scripts_dir = skill_dir.join("scripts");
    tokio::fs::create_dir_all(&scripts_dir).await?;
    tokio::fs::write(
        scripts_dir.join(script_name),
        instruction_http_adapter_script(),
    )
    .await?;
    let updated = inject_instruction_skill_adapter_frontmatter(&content, script_name)?;
    tokio::fs::write(skill_file, updated).await?;
    Ok(())
}

fn inject_instruction_skill_adapter_frontmatter(
    content: &str,
    script_name: &str,
) -> Result<String, AppError> {
    let Some((yaml, body)) = split_skill_frontmatter(content) else {
        return Err(AppError(anyhow::anyhow!(
            "SKILL.md must start with YAML frontmatter"
        )));
    };

    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).map_err(|e| {
        AppError(anyhow::anyhow!(
            "Failed to parse SKILL.md frontmatter: {}",
            e
        ))
    })?;
    let mapping = value.as_mapping_mut().ok_or_else(|| {
        AppError(anyhow::anyhow!(
            "SKILL.md frontmatter must be a YAML mapping"
        ))
    })?;

    mapping.insert(
        serde_yaml_ng::Value::String("runtime".to_string()),
        serde_yaml_ng::Value::String("python3".to_string()),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("script".to_string()),
        serde_yaml_ng::Value::String(script_name.to_string()),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("kind".to_string()),
        serde_yaml_ng::Value::String("tool".to_string()),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("parameters".to_string()),
        serde_yaml_ng::to_value(instruction_skill_parameters()).map_err(|e| {
            AppError(anyhow::anyhow!(
                "Failed to serialize generated parameters: {}",
                e
            ))
        })?,
    );
    mapping.insert(
        serde_yaml_ng::Value::String("interface".to_string()),
        serde_yaml_ng::Value::String(
            "interface InstructionHttpSkillArgs {\n  action?: string;\n  endpoint_path?: string;\n  query?: string;\n  lang?: string;\n  size?: number;\n  page?: number;\n  params?: Record<string, string | number | boolean>;\n}".to_string(),
        ),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("usage_guidelines".to_string()),
        serde_yaml_ng::Value::String(
            "This instruction-only skill is executed through BenShu's generated HTTP adapter. Prefer action shortcuts for common flows, or pass endpoint_path plus params from the manual. The adapter restricts calls to the manual-declared API base URL and reads the manual-declared API key environment variable.".to_string(),
        ),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("permissions".to_string()),
        serde_yaml_ng::to_value(json!({
            "filesystem": "read_skill",
            "network": true
        }))
        .map_err(|e| AppError(anyhow::anyhow!("Failed to serialize permissions: {}", e)))?,
    );
    mapping.insert(
        serde_yaml_ng::Value::String("resources".to_string()),
        serde_yaml_ng::to_value(json!({
            "timeout_secs": 30,
            "max_output_bytes": 131072
        }))
        .map_err(|e| AppError(anyhow::anyhow!("Failed to serialize resources: {}", e)))?,
    );

    let frontmatter = serde_yaml_ng::to_string(&value).map_err(|e| {
        AppError(anyhow::anyhow!(
            "Failed to render SKILL.md frontmatter: {}",
            e
        ))
    })?;
    Ok(format!("---\n{}---\n{}", frontmatter, body.trim_start()))
}

fn split_skill_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;
    let separator = content.find("\n---")?;
    let yaml = &content[..separator];
    let body_start = separator + "\n---".len();
    let body = content
        .get(body_start..)
        .unwrap_or_default()
        .strip_prefix('\n')
        .unwrap_or_else(|| content.get(body_start..).unwrap_or_default());
    Some((yaml, body))
}

fn instruction_skill_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "description": "Optional shortcut action inferred from the skill manual, such as latest_newsflash, important_newsflash, latest_articles, search, market_overview, capital_flow, macro, or derivatives."
            },
            "endpoint_path": {
                "type": "string",
                "description": "Manual-declared API path such as /v1/newsflash. Must stay under the skill manual's Base URL."
            },
            "query": {
                "type": "string",
                "description": "Search keyword or topic when the selected endpoint supports it."
            },
            "lang": {
                "type": "string",
                "description": "Language code when supported by the API, for example cn, en, or cht."
            },
            "size": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "Maximum number of rows/items to request."
            },
            "page": {
                "type": "integer",
                "minimum": 1,
                "description": "Page number when the API supports pagination."
            },
            "params": {
                "type": "object",
                "description": "Additional query parameters copied from the skill manual."
            }
        }
    })
}

fn instruction_http_adapter_script() -> &'static str {
    r#"#!/usr/bin/env python3
import html
import json
import os
import re
import sys
import urllib.parse
import urllib.request


def read_args():
    if len(sys.argv) < 2:
        return {}
    try:
        value = json.loads(sys.argv[1])
        return value if isinstance(value, dict) else {"query": str(value)}
    except Exception:
        return {"query": sys.argv[1]}


def read_manual():
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(os.path.dirname(here), "SKILL.md"), "r", encoding="utf-8") as fh:
        return fh.read()


def clean(value):
    text = html.unescape(str(value or ""))
    text = re.sub(r"<[^>]+>", " ", text)
    return re.sub(r"\s+", " ", text).strip()


def extract_base_url(manual):
    match = re.search(r"Base URL\*\*:\s*`([^`]+)`", manual)
    if match:
        return match.group(1).rstrip("/")
    urls = re.findall(r"https://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+", manual)
    if not urls:
        raise RuntimeError("No HTTPS API base URL found in SKILL.md")
    parsed = urllib.parse.urlparse(urls[0])
    return f"{parsed.scheme}://{parsed.netloc}"


def extract_env_name(manual):
    match = re.search(r"primaryEnv:\s*([A-Za-z_][A-Za-z0-9_]*)", manual)
    if match:
        return match.group(1)
    match = re.search(r"\$([A-Z][A-Z0-9_]*API_KEY|[A-Z][A-Z0-9_]*_API_KEY)", manual)
    if match:
        return match.group(1)
    return ""


def auth_header_name(manual):
    if "api-key:" in manual or "api-key`" in manual:
        return "api-key"
    if "X-API-Key" in manual:
        return "X-API-Key"
    return "Authorization"


def action_to_request(action, args):
    action = (action or "").lower().strip()
    lang = args.get("lang") or "en"
    size = int(args.get("size") or 10)
    page = int(args.get("page") or 1)
    query = args.get("query") or args.get("name") or ""

    mapping = {
        "latest": "/v1/newsflash",
        "latest_news": "/v1/newsflash",
        "latest_newsflash": "/v1/newsflash",
        "newsflash": "/v1/newsflash",
        "important": "/v1/newsflash/important",
        "important_news": "/v1/newsflash/important",
        "important_newsflash": "/v1/newsflash/important",
        "ai": "/v1/newsflash/ai",
        "ai_news": "/v1/newsflash/ai",
        "onchain": "/v1/newsflash/onchain",
        "financing": "/v1/newsflash/financing",
        "prediction": "/v1/newsflash/prediction",
        "latest_articles": "/v1/article",
        "article": "/v1/article",
        "articles": "/v1/article",
    }
    if action == "search":
        return [{"path": "/v1/search", "params": {"name": query, "size": size, "lang": lang}}]
    if action == "market_overview":
        return [
            {"path": "/v1/data/bottom_top_indicator", "params": {}},
            {"path": "/v1/newsflash/important", "params": {"size": min(size, 5), "lang": lang}},
            {"path": "/v1/data/btc_etf", "params": {}},
            {"path": "/v1/data/daily_tx", "params": {}},
        ]
    if action == "capital_flow":
        network = args.get("network") or args.get("chain") or "solana"
        return [
            {"path": "/v1/data/top10_netflow", "params": {"network": network}},
            {"path": "/v1/data/stablecoin_marketcap", "params": {}},
            {"path": "/v1/data/btc_etf", "params": {}},
        ]
    if action == "macro":
        return [
            {"path": "/v1/data/m2_supply", "params": {"type": args.get("type") or "1Y"}},
            {"path": "/v1/data/us10y", "params": {"type": "1M"}},
            {"path": "/v1/data/dxy", "params": {"type": "1M"}},
            {"path": "/v1/data/compliant_total", "params": {}},
        ]
    if action == "derivatives":
        return [
            {"path": "/v1/data/contract", "params": {"dataType": args.get("dataType") or "1D"}},
            {"path": "/v1/data/exchanges", "params": {"size": size}},
            {"path": "/v1/data/bitfinex_long", "params": {"symbol": args.get("symbol") or "btc", "type": args.get("type") or "1D"}},
        ]
    if action in mapping:
        return [{"path": mapping[action], "params": {"page": page, "size": size, "lang": lang}}]
    return []


def build_requests(args):
    params = args.get("params") if isinstance(args.get("params"), dict) else {}
    if args.get("endpoint_path"):
        return [{"path": args["endpoint_path"], "params": params}]
    requests = action_to_request(args.get("action"), args)
    if requests:
        return requests
    if args.get("query"):
        return action_to_request("search", args)
    return action_to_request("latest_newsflash", args)


def request_json(base_url, path, params, header_name, api_key):
    if not path.startswith("/"):
        path = "/" + path
    url = base_url + path
    if params:
        url += "?" + urllib.parse.urlencode(params)
    headers = {"Accept": "application/json", "User-Agent": "BenShu instruction skill adapter/1.0"}
    if api_key:
        headers[header_name] = f"Bearer {api_key}" if header_name == "Authorization" else api_key
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw_bytes = resp.read(1024 * 1024 + 1)
        if len(raw_bytes) > 1024 * 1024:
            raise RuntimeError("response exceeded 1MB safety limit")
        raw = raw_bytes.decode(resp.headers.get_content_charset() or "utf-8", "replace")
    try:
        payload = json.loads(raw)
    except Exception:
        payload = {"raw": raw}
    return {"url": url, "payload": payload}


def summarize_item(item):
    if not isinstance(item, dict):
        return item
    return {
        "title": clean(item.get("title") or item.get("name") or ""),
        "summary": clean(item.get("abstract") or item.get("description") or item.get("content") or ""),
        "time": item.get("time_cn") or item.get("create_time") or item.get("time") or "",
        "url": item.get("url") or item.get("link") or "",
        "type": item.get("type"),
    }


def compact(payload):
    data = payload.get("data") if isinstance(payload, dict) else payload
    if isinstance(data, dict):
        rows = data.get("list") or data.get("data") or data.get("items") or data.get("rows")
        if isinstance(rows, list):
            data = {**data, "items": [summarize_item(row) for row in rows[:10]]}
    elif isinstance(data, list):
        data = [summarize_item(row) for row in data[:10]]
    return data


def main():
    args = read_args()
    manual = read_manual()
    base_url = extract_base_url(manual)
    env_name = extract_env_name(manual)
    api_key = os.environ.get(env_name, "") if env_name else ""
    header_name = auth_header_name(manual)
    requests = build_requests(args)
    results = []
    errors = []

    for request in requests:
        try:
            result = request_json(base_url, request["path"], request.get("params") or {}, header_name, api_key)
            results.append({**result, "data": compact(result["payload"])})
        except Exception as exc:
            errors.append({"path": request.get("path"), "error": f"{type(exc).__name__}: {exc}"})

    print(json.dumps({
        "ok": bool(results) and not errors,
        "skill": os.path.basename(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
        "base_url": base_url,
        "used_env": env_name if bool(api_key) else "",
        "requests": requests,
        "results": results,
        "errors": errors,
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()
"#
}

fn sibling_skill_asset_url(source_url: &str, relative_asset: &str) -> Option<String> {
    let base = source_url.rsplit_once('/')?.0;
    Some(format!("{}/{}", base, relative_asset))
}

fn resolve_local_skill_source(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();
    let path = trimmed
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(trimmed));
    if path.is_dir() && path.join("SKILL.md").exists() {
        Some(path)
    } else {
        None
    }
}

async fn copy_skill_dir(source: &FsPath, target: &FsPath) -> Result<(), AppError> {
    tokio::fs::create_dir_all(target).await?;
    let mut stack = vec![source.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let relative = dir
            .strip_prefix(source)
            .map_err(|e| AppError(anyhow::anyhow!("Failed to resolve skill directory: {}", e)))?;
        let target_dir = target.join(relative);
        tokio::fs::create_dir_all(&target_dir).await?;

        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if file_type.is_file() {
                let relative_file = path.strip_prefix(source).map_err(|e| {
                    AppError(anyhow::anyhow!("Failed to resolve skill file: {}", e))
                })?;
                let target_file = target.join(relative_file);
                if let Some(parent) = target_file.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(&path, &target_file).await?;
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ProviderSchemaEntry {
    #[serde(flatten)]
    pub metadata: benshu_brain::agent::provider::ProviderMetadata,
    pub capability_view: benshu_brain::agent::provider::ProviderCapabilityView,
}

#[derive(Serialize)]
pub struct ProviderSchemaResponse {
    pub providers: Vec<ProviderSchemaEntry>,
}

pub async fn get_provider_schema() -> Json<ProviderSchemaResponse> {
    use benshu_brain::agent::provider::Provider as _;
    use benshu_providers::{
        anthropic::Anthropic, baidu::Baidu, deepseek::DeepSeek, doubao::Doubao, gemini::Gemini,
        groq::Groq, minimax::MiniMax, moonshot::Moonshot, openai::OpenAI, openrouter::OpenRouter,
        qwen::Qwen, siliconflow::SiliconFlow, xunfei::Xunfei, zhipu::Zhipu,
    };

    let providers = vec![
        OpenAI::metadata(),
        Anthropic::metadata(),
        Gemini::metadata(),
        DeepSeek::metadata(),
        Groq::metadata(),
        MiniMax::metadata(),
        Moonshot::metadata(),
        Zhipu::metadata(),
        Qwen::metadata(),
        Doubao::metadata(),
        SiliconFlow::metadata(),
        Baidu::metadata(),
        Xunfei::metadata(),
        OpenRouter::metadata(),
    ]
    .into_iter()
    .map(|metadata| ProviderSchemaEntry {
        capability_view: metadata.capability_view(),
        metadata,
    })
    .collect();

    Json(ProviderSchemaResponse { providers })
}
