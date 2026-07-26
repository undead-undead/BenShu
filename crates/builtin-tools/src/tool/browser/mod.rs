use async_trait::async_trait;
use base64::Engine;
use benshu_compression::{line_window, render_search_results, SearchResultSummaryItem};
use benshu_infra::error::{Error, Result};
use benshu_infra::{SecretVault, Tool, ToolDefinition};
use benshu_routing::{
    build_search_result_followup_plan, build_source_observed_followup_plan,
    build_verified_verification_result_envelope, route_reason_for_plan, QueryVerificationPlan,
    VerificationDomain, VerificationFollowupPlan, VerificationMode, VerificationRequirement,
    VerificationSource, WebVerificationOrchestrator,
};
use headless_chrome::{Browser, LaunchOptions, Tab};
use parking_lot::Mutex;
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use self::runtime::{resolve_browser_runtime, wsl_path_to_windows_path, BrowserFamily};
use self::safety::{BrowserSafetyGate, BrowserSafetyRequest};
use self::session::{
    resolve_default_managed_user_data_dir, resolve_managed_user_data_dir, BrowserSessionConfig,
};
use crate::tool::web_search::policy::SearchPolicy;

pub mod cdp;
pub mod helper;
pub mod observe;
pub mod provider;
pub mod runtime;
pub mod safety;
pub mod session;
pub mod types;

const DEFAULT_WINDOWS_BROWSER_DUMP_DOM_TIMEOUT_SECS: u64 = 30;
const DEFAULT_WINDOWS_BROWSER_VIRTUAL_TIME_BUDGET_MS: u64 = 5_000;
const DEFAULT_WINDOWS_BROWSER_CDP_CONNECT_TIMEOUT_SECS: u64 = 20;
const DEFAULT_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS: u64 = 12;
const DEFAULT_BROWSER_DOM_LINK_LIMIT: usize = 240;

/// A stateful browser tool that maintains sessions and provides semantic snapshots with refs.
pub struct BrowserTool {
    browser: Arc<Mutex<Option<Browser>>>,
    browser_process: Arc<Mutex<Option<Child>>>,
    browser_profile_dir: Arc<Mutex<Option<PathBuf>>>,
    browser_family: Arc<Mutex<Option<BrowserFamily>>>,
    session: BrowserSessionConfig,
    user_data_dir: Option<PathBuf>,
    /// Cache for ref mapping: "@e1" -> "CSS selector / Internal ID"
    ref_map: Arc<Mutex<HashMap<String, String>>>,
    /// Optional vault for persisting cookies/sessions locally
    vault: Option<Arc<dyn SecretVault>>,
    /// Store the last captured snapshot tree for diffing
    last_snapshot: Arc<Mutex<Option<String>>>,
    /// Static WSL browser mode keeps a lightweight current URL without a DevTools session.
    current_url: Arc<Mutex<Option<String>>>,
    sensory: Arc<benshu_sensory::SensoryHub>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BrowserSnapshot {
    tree: String,
    refs: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BrowserWaitUntil {
    DomContentLoaded,
    Load,
    NetworkIdle,
}

impl BrowserWaitUntil {
    fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("load").trim().to_ascii_lowercase().as_str() {
            "domcontentloaded" | "dom_content_loaded" | "dom-ready" | "dom_ready" => {
                Ok(Self::DomContentLoaded)
            }
            "load" | "loaded" => Ok(Self::Load),
            "networkidle" | "network_idle" | "networkidle0" | "networkidle2" => {
                Ok(Self::NetworkIdle)
            }
            other => Err(anyhow::anyhow!(
                "Unsupported wait_until '{other}'. Use domcontentloaded, load, or networkidle."
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::DomContentLoaded => "domcontentloaded",
            Self::Load => "load",
            Self::NetworkIdle => "networkidle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BrowserSnapshotFormat {
    Semantic,
    Text,
    Links,
    Html,
    Markdown,
}

impl BrowserSnapshotFormat {
    fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("semantic").trim().to_ascii_lowercase().as_str() {
            "semantic" | "aria" | "tree" => Ok(Self::Semantic),
            "text" | "plain_text" => Ok(Self::Text),
            "links" | "link" => Ok(Self::Links),
            "html" => Ok(Self::Html),
            "markdown" | "md" => Ok(Self::Markdown),
            other => Err(anyhow::anyhow!(
                "Unsupported snapshot format '{other}'. Use semantic, text, links, html, or markdown."
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Text => "text",
            Self::Links => "links",
            Self::Html => "html",
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BrowserPageKind {
    Normal,
    Challenge,
    LoginWall,
    EmptyShell,
}

impl BrowserPageKind {
    fn note(self) -> &'static str {
        match self {
            Self::Normal => "browser page content observed",
            Self::Challenge => "browser page appears to be an anti-bot or verification challenge",
            Self::LoginWall => {
                "browser page appears to require authentication before content is accessible"
            }
            Self::EmptyShell => "browser page loaded but contains little or no parseable content",
        }
    }

    fn is_blocking(self) -> bool {
        !matches!(self, Self::Normal)
    }

    fn blocker_code(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Challenge => Some("verification_challenge"),
            Self::LoginWall => Some("login_wall"),
            Self::EmptyShell => Some("page_empty_shell"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSearchResult {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: String,
    pub(crate) source: String,
    pub(crate) position: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct BrowserExtractedLink {
    text: String,
    url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct BrowserExtractedRecord {
    title: String,
    url: String,
    metadata: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
struct BrowserNetworkSummary {
    provider: String,
    final_url: String,
    status_code: Option<u16>,
    content_type: Option<String>,
    redirect_chain: Vec<String>,
    main_document_observed: bool,
    notes: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct WindowsCdpBridgeObservation {
    final_url: String,
    title: Option<String>,
    ready_state: Option<String>,
    text: String,
    html: String,
    markdown: String,
    links: Vec<BrowserExtractedLink>,
    resource_count: Option<u64>,
}

impl BrowserTool {
    fn first_url_like(text: &str) -> Option<String> {
        text.split_whitespace()
            .map(|part| {
                part.trim_matches(|ch: char| {
                    matches!(
                        ch,
                        '"' | '\''
                            | '`'
                            | '<'
                            | '>'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | ','
                            | '，'
                            | '。'
                            | '.'
                    )
                })
            })
            .find(|part| part.starts_with("http://") || part.starts_with("https://"))
            .map(ToOwned::to_owned)
    }

    pub(crate) fn search_query_with_task_context(
        query: &str,
        task_context: Option<&str>,
    ) -> String {
        SearchPolicy::browser_search_query_with_task_context(query, task_context)
    }

    fn static_wsl_windows_browser_runtime() -> Option<self::runtime::BrowserRuntime> {
        let runtime = resolve_browser_runtime()?;
        if Self::is_wsl() && runtime.is_windows_executable() {
            Some(runtime)
        } else {
            None
        }
    }

    fn windows_interactive_cdp_enabled() -> bool {
        std::env::var("BENSHU_WINDOWS_BROWSER_INTERACTIVE_CDP")
            .ok()
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off" | "static"
                )
            })
            .unwrap_or(true)
    }

    fn windows_direct_cdp_under_wsl_enabled() -> bool {
        std::env::var("BENSHU_WINDOWS_BROWSER_DIRECT_CDP_UNDER_WSL")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn direct_devtools_session_available(runtime: &self::runtime::BrowserRuntime) -> bool {
        !runtime.is_windows_executable()
    }

    fn is_wsl() -> bool {
        std::env::var("WSL_DISTRO_NAME").is_ok()
            || std::env::var("WSL_INTEROP").is_ok()
            || std::fs::read_to_string("/proc/version")
                .map(|version| version.to_ascii_lowercase().contains("microsoft"))
                .unwrap_or(false)
    }

    fn windows_browser_dump_dom_timeout() -> Duration {
        let secs = std::env::var("BENSHU_WINDOWS_BROWSER_DUMP_DOM_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_WINDOWS_BROWSER_DUMP_DOM_TIMEOUT_SECS)
            .clamp(5, 90);
        Duration::from_secs(secs)
    }

    fn windows_browser_virtual_time_budget_ms() -> u64 {
        std::env::var("BENSHU_WINDOWS_BROWSER_VIRTUAL_TIME_BUDGET_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_WINDOWS_BROWSER_VIRTUAL_TIME_BUDGET_MS)
            .min(60_000)
    }

    fn windows_browser_cdp_connect_timeout() -> Duration {
        let secs = std::env::var("BENSHU_WINDOWS_BROWSER_CDP_CONNECT_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_WINDOWS_BROWSER_CDP_CONNECT_TIMEOUT_SECS)
            .clamp(5, 90);
        Duration::from_secs(secs)
    }

    fn windows_browser_search_cdp_timeout() -> Duration {
        let secs = std::env::var("BENSHU_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS)
            .clamp(5, 60);
        Duration::from_secs(secs)
    }

    fn windows_cdp_bridge_enabled() -> bool {
        std::env::var("BENSHU_WINDOWS_BROWSER_CDP_BRIDGE")
            .ok()
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off" | "static"
                )
            })
            .unwrap_or(true)
    }

    fn url_prefers_mobile_browser_profile(url: &str) -> bool {
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
            .map(|host| {
                host.starts_with("m.")
                    || host.starts_with("mobile.")
                    || host.contains(".m.")
                    || host.contains(".mobile.")
            })
            .unwrap_or(false)
    }

    fn powershell_executable() -> Option<&'static str> {
        if Path::new("/mnt/c/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe").is_file() {
            Some("/mnt/c/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe")
        } else if Path::new("powershell.exe").is_file() {
            Some("powershell.exe")
        } else {
            Some("powershell.exe")
        }
    }

    fn resolve_windows_browser_cdp_port() -> anyhow::Result<u16> {
        if let Some(port) = std::env::var("BENSHU_WINDOWS_BROWSER_CDP_PORT")
            .ok()
            .and_then(|value| value.trim().parse::<u16>().ok())
            .filter(|port| *port > 0)
        {
            return Ok(port);
        }

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        Ok(port)
    }

    fn windows_dump_dom_reuse_managed_profile() -> bool {
        std::env::var("BENSHU_WINDOWS_BROWSER_DUMP_DOM_REUSE_PROFILE")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn windows_dump_dom_profile_dir() -> PathBuf {
        if Self::windows_dump_dom_reuse_managed_profile() {
            if let Some(managed) = resolve_default_managed_user_data_dir() {
                return managed;
            }
        }

        if Self::is_wsl() {
            if let Some(managed) = resolve_default_managed_user_data_dir() {
                if let Some(base) = managed.parent() {
                    return base
                        .join("browser-dumpdom")
                        .join(uuid::Uuid::new_v4().to_string());
                }
            }
        }

        std::env::temp_dir().join(format!("benshu-browser-dumpdom-{}", uuid::Uuid::new_v4()))
    }

    fn windows_cdp_reuse_managed_profile() -> bool {
        std::env::var("BENSHU_WINDOWS_BROWSER_CDP_REUSE_PROFILE")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn windows_cdp_profile_dir() -> PathBuf {
        if Self::windows_cdp_reuse_managed_profile() {
            if let Some(managed) = resolve_default_managed_user_data_dir() {
                return managed;
            }
        }

        if Self::is_wsl() {
            if let Some(managed) = resolve_default_managed_user_data_dir() {
                if let Some(base) = managed.parent() {
                    return base
                        .join("browser-cdp")
                        .join(uuid::Uuid::new_v4().to_string());
                }
            }
        }

        std::env::temp_dir().join(format!("benshu-browser-cdp-{}", uuid::Uuid::new_v4()))
    }

    async fn dump_dom_once_with_windows_browser(url: &str) -> anyhow::Result<String> {
        let runtime = Self::static_wsl_windows_browser_runtime()
            .ok_or_else(|| anyhow::anyhow!("No WSL Windows browser runtime available"))?;
        let url = url.to_string();
        let executable_path = runtime.executable_path.clone();
        let browser_family = runtime.family;
        tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
            let profile_dir = Self::windows_dump_dom_profile_dir();
            std::fs::create_dir_all(&profile_dir)?;
            let ephemeral_profile = !Self::windows_dump_dom_reuse_managed_profile();
            let timeout = Self::windows_browser_dump_dom_timeout();
            let virtual_time_budget_ms = Self::windows_browser_virtual_time_budget_ms();
            let stdout_path = profile_dir.join("dump-dom.stdout.html");
            let stderr_path = profile_dir.join("dump-dom.stderr.log");
            let stdout_file = File::create(&stdout_path)?;
            let stderr_file = File::create(&stderr_path)?;
            let browser_profile_arg = wsl_path_to_windows_path(&profile_dir)
                .unwrap_or_else(|| profile_dir.display().to_string());

            let mut command = Command::new(&executable_path);
            command
                .arg("--headless=new")
                .arg("--disable-gpu")
                .arg("--disable-extensions")
                .arg("--disable-sync")
                .arg("--disable-component-update")
                .arg("--no-first-run")
                .arg("--no-service-autorun")
                .arg("--no-default-browser-check")
                .arg("--disable-background-networking")
                .arg("--disable-renderer-backgrounding")
                .arg("--blink-settings=imagesEnabled=false")
                .arg(format!("--user-data-dir={}", browser_profile_arg))
                .arg("--dump-dom");
            if virtual_time_budget_ms > 0 {
                command.arg(format!("--virtual-time-budget={}", virtual_time_budget_ms));
            }
            let mut child = command
                .arg(&url)
                .stdout(Stdio::from(stdout_file))
                .stderr(Stdio::from(stderr_file))
                .spawn()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to launch Windows browser at {}: {}",
                        executable_path.display(),
                        e
                    )
                })?;

            let started_at = Instant::now();
            while child.try_wait()?.is_none() {
                if started_at.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    Self::cleanup_windows_dump_dom_processes(&profile_dir, browser_family);
                    if ephemeral_profile {
                        let _ = std::fs::remove_dir_all(&profile_dir);
                    }
                    anyhow::bail!(
                        "Windows browser dump-dom timed out after {}s",
                        timeout.as_secs()
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let status = child.wait()?;
            let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            Self::cleanup_windows_dump_dom_processes(&profile_dir, browser_family);
            if ephemeral_profile {
                let _ = std::fs::remove_dir_all(&profile_dir);
            }
            if !status.success() {
                anyhow::bail!(
                    "Windows browser dump-dom failed with status {}: {}",
                    status,
                    stderr.trim()
                );
            }
            Ok(stdout)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Windows browser dump-dom task join failed: {}", e))?
    }

    fn launch_windows_interactive_cdp_browser(&self) -> anyhow::Result<Browser> {
        let runtime = Self::static_wsl_windows_browser_runtime()
            .ok_or_else(|| anyhow::anyhow!("No WSL Windows browser runtime available"))?;
        if !Self::windows_interactive_cdp_enabled() {
            anyhow::bail!("Windows interactive CDP provider is disabled by policy");
        }

        let port = Self::resolve_windows_browser_cdp_port()?;
        let profile_dir = Self::windows_cdp_profile_dir();
        std::fs::create_dir_all(&profile_dir)?;
        let browser_profile_arg = wsl_path_to_windows_path(&profile_dir)
            .unwrap_or_else(|| profile_dir.display().to_string());
        let stderr_path = profile_dir.join("cdp.stderr.log");
        let stdout_path = profile_dir.join("cdp.stdout.log");
        let stderr_file = File::create(&stderr_path)?;
        let stdout_file = File::create(&stdout_path)?;
        let remote_debugging_address = if Self::is_wsl() {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };

        let mut command = Command::new(&runtime.executable_path);
        command
            .arg("--headless=new")
            .arg("--disable-gpu")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--disable-component-update")
            .arg("--no-first-run")
            .arg("--no-service-autorun")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-renderer-backgrounding")
            .arg("--window-size=1920,1080")
            .arg(format!(
                "--remote-debugging-address={remote_debugging_address}"
            ))
            .arg(format!("--remote-debugging-port={port}"))
            .arg("--remote-allow-origins=*")
            .arg(format!("--user-data-dir={browser_profile_arg}"))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));

        let mut child = command.spawn().map_err(|error| {
            anyhow::anyhow!(
                "Failed to launch Windows browser CDP runtime at {}: {}",
                runtime.executable_path.display(),
                error
            )
        })?;

        match Self::wait_for_windows_cdp_websocket_url(
            port,
            Self::windows_browser_cdp_connect_timeout(),
        ) {
            Ok(ws_url) => match Browser::connect_with_timeout(ws_url, Duration::from_secs(60)) {
                Ok(browser) => {
                    *self.browser_process.lock() = Some(child);
                    *self.browser_profile_dir.lock() = Some(profile_dir);
                    *self.browser_family.lock() = Some(runtime.family);
                    Ok(browser)
                }
                Err(error) => {
                    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                    let _ = child.kill();
                    let _ = child.wait();
                    Self::cleanup_windows_dump_dom_processes(&profile_dir, runtime.family);
                    if !Self::windows_cdp_reuse_managed_profile() {
                        let _ = std::fs::remove_dir_all(&profile_dir);
                    }
                    Err(anyhow::anyhow!(
                        "Windows browser CDP websocket connection failed: {}; stderr={}",
                        error,
                        stderr.trim()
                    ))
                }
            },
            Err(error) => {
                let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                let _ = child.kill();
                let _ = child.wait();
                Self::cleanup_windows_dump_dom_processes(&profile_dir, runtime.family);
                if !Self::windows_cdp_reuse_managed_profile() {
                    let _ = std::fs::remove_dir_all(&profile_dir);
                }
                Err(anyhow::anyhow!("{}; stderr={}", error, stderr.trim()))
            }
        }
    }

    fn wait_for_windows_cdp_websocket_url(port: u16, timeout: Duration) -> anyhow::Result<String> {
        let started_at = Instant::now();
        let mut last_error = String::new();
        let hosts = Self::windows_cdp_connect_hosts();

        while started_at.elapsed() < timeout {
            for host in &hosts {
                let endpoint = format!("{host}:{port}");
                match TcpStream::connect(&endpoint) {
                    Ok(mut stream) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                        let request = format!(
                            "GET /json/version HTTP/1.1\r\nHost: {endpoint}\r\nConnection: close\r\n\r\n"
                        );
                        if let Err(error) = stream.write_all(request.as_bytes()) {
                            last_error = format!("{endpoint}: {error}");
                        } else {
                            let mut response = String::new();
                            match stream.read_to_string(&mut response) {
                                Ok(_) => {
                                    if let Some(ws_url) =
                                        Self::parse_cdp_websocket_url_from_http_response(&response)
                                    {
                                        return Ok(Self::rewrite_cdp_websocket_host(
                                            &ws_url, host, port,
                                        ));
                                    }
                                    last_error = format!(
                                        "{endpoint}: CDP /json/version did not include webSocketDebuggerUrl"
                                    );
                                }
                                Err(error) => {
                                    last_error = format!("{endpoint}: {error}");
                                }
                            }
                        }
                    }
                    Err(error) => {
                        last_error = format!("{endpoint}: {error}");
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(150));
        }

        anyhow::bail!(
            "Windows browser CDP endpoint did not become ready after {}s: {}",
            timeout.as_secs(),
            last_error
        )
    }

    fn windows_cdp_connect_hosts() -> Vec<String> {
        let mut hosts = vec!["127.0.0.1".to_string(), "localhost".to_string()];
        if Self::is_wsl() {
            if let Some(host) = Self::windows_host_ip_from_wsl() {
                hosts.insert(0, host);
            }
        }
        hosts.sort();
        hosts.dedup();
        hosts
    }

    fn windows_host_ip_from_wsl() -> Option<String> {
        std::fs::read_to_string("/etc/resolv.conf")
            .ok()
            .and_then(|content| {
                content.lines().find_map(|line| {
                    let line = line.trim();
                    let value = line.strip_prefix("nameserver")?.trim();
                    if value.is_empty() || value.contains(':') {
                        return None;
                    }
                    Some(value.to_string())
                })
            })
    }

    fn rewrite_cdp_websocket_host(ws_url: &str, connect_host: &str, port: u16) -> String {
        let Ok(mut parsed) = Url::parse(ws_url) else {
            return ws_url.to_string();
        };
        let _ = parsed.set_host(Some(connect_host));
        let _ = parsed.set_port(Some(port));
        parsed.to_string()
    }

    fn parse_cdp_websocket_url_from_http_response(response: &str) -> Option<String> {
        let (_, body) = response.split_once("\r\n\r\n")?;
        let value: Value = serde_json::from_str(body.trim()).ok()?;
        value
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("ws://") || url.starts_with("wss://"))
            .map(ToOwned::to_owned)
    }

    fn should_try_static_http_fallback(error: &anyhow::Error) -> bool {
        let message = error.to_string().to_ascii_lowercase();
        message.contains("dump-dom timed out")
            || message.contains("dump-dom failed")
            || message.contains("failed to launch windows browser")
    }

    async fn fetch_static_html_browser_fallback(url: &str) -> anyhow::Result<String> {
        BrowserSafetyGate::validate_public_web_url(url)
            .map_err(|reason| anyhow::anyhow!("browser static fallback blocked URL: {reason}"))?;
        let parsed = Url::parse(url)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            anyhow::bail!("browser static fallback only supports http/https URLs");
        }

        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36 BENSHUBrowserFallback/1.0")
            .build()?
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
            .map_err(Into::into)
    }

    async fn static_html_once_with_windows_browser(url: &str) -> anyhow::Result<String> {
        BrowserSafetyGate::validate_public_web_url(url).map_err(|reason| {
            anyhow::anyhow!("Windows browser static observation blocked URL: {reason}")
        })?;
        match Self::dump_dom_once_with_windows_browser(url).await {
            Ok(html) => Ok(html),
            Err(error) if Self::should_try_static_http_fallback(&error) => {
                Self::fetch_static_html_browser_fallback(url)
                    .await
                    .map_err(|fallback_error| {
                        anyhow::anyhow!(
                            "{}; static HTML fallback also failed: {}",
                            error,
                            fallback_error
                        )
                    })
            }
            Err(error) => Err(error),
        }
    }

    async fn observe_once_with_windows_cdp_bridge(
        url: &str,
        wait_until: BrowserWaitUntil,
        wait_time: tokio::time::Duration,
    ) -> anyhow::Result<WindowsCdpBridgeObservation> {
        let timeout = Self::windows_browser_cdp_connect_timeout()
            .max(Self::windows_browser_dump_dom_timeout());
        Self::observe_once_with_windows_cdp_bridge_with_timeout(url, wait_until, wait_time, timeout)
            .await
    }

    async fn observe_once_with_windows_cdp_bridge_with_timeout(
        url: &str,
        wait_until: BrowserWaitUntil,
        wait_time: tokio::time::Duration,
        timeout: Duration,
    ) -> anyhow::Result<WindowsCdpBridgeObservation> {
        let runtime = Self::static_wsl_windows_browser_runtime()
            .ok_or_else(|| anyhow::anyhow!("No WSL Windows browser runtime available"))?;
        if !Self::windows_cdp_bridge_enabled() {
            anyhow::bail!("Windows CDP bridge provider is disabled by policy");
        }
        let url = url.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<WindowsCdpBridgeObservation> {
            let profile_dir = Self::windows_cdp_profile_dir();
            std::fs::create_dir_all(&profile_dir)?;
            let script_path = profile_dir.join("observe-cdp.ps1");
            std::fs::write(&script_path, Self::windows_cdp_bridge_script())?;

            let script_windows_path = wsl_path_to_windows_path(&script_path)
                .unwrap_or_else(|| script_path.display().to_string());
            let browser_windows_path = wsl_path_to_windows_path(&runtime.executable_path)
                .unwrap_or_else(|| runtime.executable_path.display().to_string());
            let browser_profile_arg = wsl_path_to_windows_path(&profile_dir)
                .unwrap_or_else(|| profile_dir.display().to_string());
            let port = Self::resolve_windows_browser_cdp_port()?;
            let wait_ms = wait_time.as_millis().min(u128::from(u32::MAX)) as u32;
            let powershell = Self::powershell_executable()
                .ok_or_else(|| anyhow::anyhow!("powershell.exe is not available"))?;
            let mobile_profile = Self::url_prefers_mobile_browser_profile(&url);

            let mut command = Command::new(powershell);
            command
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(script_windows_path)
                .arg("-BrowserPath")
                .arg(browser_windows_path)
                .arg("-UserDataDir")
                .arg(browser_profile_arg)
                .arg("-Url")
                .arg(&url)
                .arg("-Port")
                .arg(port.to_string())
                .arg("-WaitUntil")
                .arg(wait_until.as_str())
                .arg("-TimeoutSec")
                .arg(timeout.as_secs().to_string())
                .arg("-WaitMs")
                .arg(wait_ms.to_string())
                .arg("-MobileProfile")
                .arg(if mobile_profile { "true" } else { "false" })
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = command.spawn().map_err(|error| {
                anyhow::anyhow!("Failed to start Windows CDP bridge helper: {}", error)
            })?;
            let started_at = Instant::now();
            let kill_after = timeout + Duration::from_secs(5);
            loop {
                if child.try_wait()?.is_some() {
                    break;
                }
                if started_at.elapsed() > kill_after {
                    let _ = child.kill();
                    let output = child.wait_with_output()?;
                    Self::cleanup_windows_dump_dom_processes(&profile_dir, runtime.family);
                    if !Self::windows_cdp_reuse_managed_profile() {
                        let _ = std::fs::remove_dir_all(&profile_dir);
                    }
                    anyhow::bail!(
                        "Windows CDP bridge timed out after {}s: {}",
                        kill_after.as_secs(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let output = child.wait_with_output()?;
            Self::cleanup_windows_dump_dom_processes(&profile_dir, runtime.family);
            if !Self::windows_cdp_reuse_managed_profile() {
                let _ = std::fs::remove_dir_all(&profile_dir);
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                anyhow::bail!(
                    "Windows CDP bridge failed with status {}: {}",
                    output.status,
                    stderr.trim()
                );
            }
            serde_json::from_str(stdout.trim()).map_err(|error| {
                anyhow::anyhow!(
                    "Windows CDP bridge returned invalid observation JSON: {}; stderr={}; stdout={}",
                    error,
                    stderr.trim(),
                    stdout.trim()
                )
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("Windows CDP bridge task join failed: {}", e))?
    }

    fn windows_cdp_bridge_script() -> &'static str {
        r#"
param(
    [Parameter(Mandatory=$true)][string]$BrowserPath,
    [Parameter(Mandatory=$true)][string]$UserDataDir,
    [Parameter(Mandatory=$true)][string]$Url,
    [Parameter(Mandatory=$true)][int]$Port,
    [Parameter(Mandatory=$true)][string]$WaitUntil,
    [Parameter(Mandatory=$true)][int]$TimeoutSec,
    [Parameter(Mandatory=$true)][int]$WaitMs,
    [Parameter(Mandatory=$false)][string]$MobileProfile = "false"
)
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

function Invoke-CdpHttpJson($Path, $Method) {
    $uri = "http://127.0.0.1:$Port$Path"
    return Invoke-RestMethod -Method $Method -Uri $uri -TimeoutSec 2
}

$isMobileProfile = @("1", "true", "yes", "on") -contains $MobileProfile.ToLowerInvariant()
$windowSize = if ($isMobileProfile) { "430,932" } else { "1920,1080" }

$browserArgs = @(
    "--headless=new",
    "--disable-gpu",
    "--disable-extensions",
    "--disable-sync",
    "--disable-component-update",
    "--no-first-run",
    "--no-service-autorun",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-renderer-backgrounding",
    "--window-size=$windowSize",
    "--remote-debugging-port=$Port",
    "--remote-allow-origins=*",
    "--user-data-dir=$UserDataDir",
    "about:blank"
)

$proc = Start-Process -FilePath $BrowserPath -ArgumentList $browserArgs -PassThru -WindowStyle Hidden
$client = $null
try {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSec)
    do {
        try {
            $null = Invoke-CdpHttpJson "/json/version" "Get"
            break
        } catch {
            Start-Sleep -Milliseconds 150
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    if ([DateTime]::UtcNow -ge $deadline) { throw "CDP HTTP endpoint did not become ready" }

    $escapedUrl = [System.Uri]::EscapeDataString($Url)
    try {
        $page = Invoke-CdpHttpJson "/json/new?$escapedUrl" "Put"
    } catch {
        $pages = Invoke-CdpHttpJson "/json/list" "Get"
        $page = @($pages | Where-Object { $_.type -eq "page" })[0]
    }
    if ($null -eq $page -or [string]::IsNullOrWhiteSpace($page.webSocketDebuggerUrl)) {
        throw "CDP page websocket URL was not available"
    }

    $client = [System.Net.WebSockets.ClientWebSocket]::new()
    $socketTimeoutSeconds = [Math]::Max(2, [Math]::Min($TimeoutSec, 10))
    $connectCts = [Threading.CancellationTokenSource]::new()
    $connectCts.CancelAfter([TimeSpan]::FromSeconds($socketTimeoutSeconds))
    try {
        $null = $client.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $connectCts.Token).GetAwaiter().GetResult()
    } catch [OperationCanceledException] {
        throw "CDP websocket connect timed out"
    } finally {
        $connectCts.Dispose()
    }
    $script:nextId = 0

    function Receive-CdpMessage {
        $builder = [System.Text.StringBuilder]::new()
        do {
            $buffer = New-Object byte[] 1048576
            $segment = [System.ArraySegment[byte]]::new($buffer)
            $receiveCts = [Threading.CancellationTokenSource]::new()
            $receiveCts.CancelAfter([TimeSpan]::FromSeconds($socketTimeoutSeconds))
            try {
                $result = $client.ReceiveAsync($segment, $receiveCts.Token).GetAwaiter().GetResult()
            } catch [OperationCanceledException] {
                throw "CDP websocket receive timed out"
            } finally {
                $receiveCts.Dispose()
            }
            if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                throw "CDP websocket closed"
            }
            [void]$builder.Append([System.Text.Encoding]::UTF8.GetString($buffer, 0, $result.Count))
        } while (-not $result.EndOfMessage)
        return ($builder.ToString() | ConvertFrom-Json)
    }

    function Send-Cdp($Method, $Params) {
        $script:nextId += 1
        $id = $script:nextId
        $payload = [ordered]@{ id = $id; method = $Method }
        if ($null -ne $Params) { $payload.params = $Params }
        $json = ($payload | ConvertTo-Json -Depth 50 -Compress)
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
        $segment = [System.ArraySegment[byte]]::new($bytes)
        $sendCts = [Threading.CancellationTokenSource]::new()
        $sendCts.CancelAfter([TimeSpan]::FromSeconds($socketTimeoutSeconds))
        try {
            $null = $client.SendAsync($segment, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $sendCts.Token).GetAwaiter().GetResult()
        } catch [OperationCanceledException] {
            throw "CDP websocket send timed out"
        } finally {
            $sendCts.Dispose()
        }
        do {
            $message = Receive-CdpMessage
            if ($message.id -eq $id) {
                if ($null -ne $message.error) {
                    throw ("CDP " + $Method + " failed: " + ($message.error | ConvertTo-Json -Compress))
                }
                return $message.result
            }
        } while ($true)
    }

    $null = Send-Cdp "Page.enable" $null
    $null = Send-Cdp "Runtime.enable" $null
    $null = Send-Cdp "Network.enable" $null
    if ($isMobileProfile) {
        $mobileUserAgent = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
        $null = Send-Cdp "Network.setUserAgentOverride" @{
            userAgent = $mobileUserAgent
            platform = "iPhone"
        }
        $null = Send-Cdp "Emulation.setDeviceMetricsOverride" @{
            width = 430
            height = 932
            deviceScaleFactor = 3
            mobile = $true
        }
        $null = Send-Cdp "Emulation.setTouchEmulationEnabled" @{ enabled = $true }
    }
    $null = Send-Cdp "Page.navigate" @{ url = $Url }

    $stableResourceTicks = 0
    $lastResourceCount = -1
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSec)
    do {
        $stateResult = Send-Cdp "Runtime.evaluate" @{
            expression = "(function(){return {ready:document.readyState, resources:(performance.getEntriesByType('resource')||[]).length};})()"
            returnByValue = $true
            awaitPromise = $true
        }
        $ready = [string]$stateResult.result.value.ready
        $resourceCount = [int]$stateResult.result.value.resources
        if ($resourceCount -eq $lastResourceCount) { $stableResourceTicks += 1 } else { $stableResourceTicks = 0 }
        $lastResourceCount = $resourceCount
        if ($WaitUntil -eq "domcontentloaded" -and $ready -ne "loading") { break }
        if ($WaitUntil -eq "load" -and $ready -eq "complete") { break }
        if ($WaitUntil -eq "networkidle" -and $ready -eq "complete" -and $stableResourceTicks -ge 2) { break }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)

    if ($WaitMs -gt 0) { Start-Sleep -Milliseconds $WaitMs }

    $extractScript = @'
(function() {
  function clean(s) { return (s || '').replace(/\s+/g, ' ').trim(); }
  const links = [];
  const seen = new Set();
  for (const a of Array.from(document.querySelectorAll('a[href]'))) {
    if (links.length >= 240) break;
    const href = a.href || '';
    if (!href || href.startsWith('javascript:') || seen.has(href)) continue;
    seen.add(href);
    const imageText = Array.from(a.querySelectorAll('img[alt]')).map(img => img.getAttribute('alt')).filter(Boolean).join(' ');
    const nearby = a.closest('li,article,section,div');
    const nearbyText = nearby ? clean(nearby.innerText || nearby.textContent || '') : '';
    const label = clean(
      a.innerText ||
      a.textContent ||
      a.getAttribute('aria-label') ||
      a.getAttribute('title') ||
      imageText ||
      nearbyText ||
      ''
    );
    links.push({ text: label.slice(0, 240), url: href, context: nearbyText.slice(0, 500) });
  }
  const lines = [];
  for (const el of Array.from(document.body ? document.body.querySelectorAll('h1,h2,h3,p,li,blockquote,td,th') : [])) {
    const text = clean(el.innerText || el.textContent || '');
    if (!text) continue;
    const tag = el.tagName.toLowerCase();
    if (tag === 'h1') lines.push('# ' + text);
    else if (tag === 'h2') lines.push('## ' + text);
    else if (tag === 'h3') lines.push('### ' + text);
    else if (tag === 'li') lines.push('- ' + text);
    else if (tag === 'blockquote') lines.push('> ' + text);
    else lines.push(text);
  }
  return {
    final_url: location.href,
    title: document.title || null,
    ready_state: document.readyState,
    text: (document.body ? document.body.innerText : '').slice(0, 30000),
    html: (document.documentElement ? document.documentElement.outerHTML : '').slice(0, 24000),
    markdown: lines.join('\n\n').slice(0, 18000),
    links: links,
    resource_count: (performance.getEntriesByType('resource') || []).length
  };
})()
'@
    $observation = Send-Cdp "Runtime.evaluate" @{
        expression = $extractScript
        returnByValue = $true
        awaitPromise = $true
    }
    $observation.result.value | ConvertTo-Json -Depth 50 -Compress
} finally {
    if ($null -ne $client) {
        try { $client.Dispose() } catch {}
    }
    if ($null -ne $proc -and -not $proc.HasExited) {
        try { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
}
"#
    }

    fn windows_cleanup_profile_needles(profile_dir: &Path) -> Vec<String> {
        let mut needles = Vec::new();
        needles.push(profile_dir.display().to_string());
        if let Some(windows_path) = wsl_path_to_windows_path(profile_dir) {
            needles.push(windows_path);
        }
        needles.sort();
        needles.dedup();
        needles
            .into_iter()
            .filter(|needle| needle.chars().count() >= 8)
            .collect()
    }

    fn cleanup_windows_dump_dom_processes(profile_dir: &Path, browser_family: BrowserFamily) {
        if !Self::is_wsl() {
            return;
        }

        let profile_needles = Self::windows_cleanup_profile_needles(profile_dir);
        if profile_needles.is_empty() {
            return;
        }

        let script = r#"
$needles = @($args[0] -split "`n" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
$family = $args[1]
$names = if ($family -eq "chrome") { @("chrome.exe") } else { @("msedge.exe", "chrome.exe") }
Get-CimInstance Win32_Process |
    Where-Object {
        $cmd = [string]$_.CommandLine
        if (-not ($names -contains $_.Name)) { return $false }
        if ($cmd -notlike "*--user-data-dir=*") { return $false }
        if (($cmd -notlike "*--remote-debugging-port=*") -and ($cmd -notlike "*--remote-debugging-address=*") -and ($cmd -notlike "*--dump-dom*")) { return $false }
        foreach ($needle in $needles) {
            if ($cmd -like "*$needle*") { return $true }
        }
        return $false
    } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
"#;
        let family = match browser_family {
            BrowserFamily::Chrome => "chrome",
            _ => "edge",
        };

        let _ = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                script,
                &profile_needles.join("\n"),
                family,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    fn decode_html_entities(input: &str) -> String {
        input
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
            .replace("&nbsp;", " ")
    }

    fn strip_html_tags(input: &str) -> String {
        let tag_re = Regex::new(r"(?s)<[^>]+>").expect("valid tag regex");
        let without_tags = tag_re.replace_all(input, " ");
        let decoded = Self::decode_html_entities(without_tags.as_ref());
        decoded
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    }

    fn html_title(input: &str) -> Option<String> {
        let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("valid title regex");
        title_re
            .captures(input)
            .and_then(|captures| captures.get(1))
            .map(|value| Self::strip_html_tags(value.as_str()))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn html_to_text_lightweight(html: &str) -> String {
        let mut result = String::with_capacity(html.len() / 3);
        let mut in_tag = false;
        let mut in_script = false;
        let mut in_style = false;
        let mut tag_name = String::new();
        let mut last_was_space = false;

        let chars: Vec<char> = html.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            let ch = chars[i];

            if ch == '<' {
                in_tag = true;
                tag_name.clear();
                i += 1;
                continue;
            }

            if in_tag {
                if ch == '>' {
                    in_tag = false;
                    let tag_lower = tag_name.to_lowercase();

                    if tag_lower.starts_with("script") {
                        in_script = true;
                    } else if tag_lower.starts_with("/script") {
                        in_script = false;
                    } else if tag_lower.starts_with("style") {
                        in_style = true;
                    } else if tag_lower.starts_with("/style") {
                        in_style = false;
                    }

                    let block_tags = [
                        "p",
                        "/p",
                        "div",
                        "/div",
                        "br",
                        "br/",
                        "br /",
                        "h1",
                        "/h1",
                        "h2",
                        "/h2",
                        "h3",
                        "/h3",
                        "h4",
                        "/h4",
                        "h5",
                        "/h5",
                        "h6",
                        "/h6",
                        "li",
                        "/li",
                        "tr",
                        "/tr",
                        "blockquote",
                        "/blockquote",
                        "hr",
                        "hr/",
                    ];
                    let clean_tag = tag_lower.split_whitespace().next().unwrap_or("");
                    if block_tags.contains(&clean_tag) {
                        if !result.ends_with('\n') {
                            result.push('\n');
                        }
                        last_was_space = true;
                        if clean_tag == "li" {
                            result.push_str("• ");
                        }
                    }
                } else {
                    tag_name.push(ch);
                }
                i += 1;
                continue;
            }

            if in_script || in_style {
                i += 1;
                continue;
            }

            if ch == '&' {
                let rest: String = chars[i..].iter().take(10).collect();
                if rest.starts_with("&amp;") {
                    result.push('&');
                    i += 5;
                    last_was_space = false;
                    continue;
                } else if rest.starts_with("&lt;") {
                    result.push('<');
                    i += 4;
                    last_was_space = false;
                    continue;
                } else if rest.starts_with("&gt;") {
                    result.push('>');
                    i += 4;
                    last_was_space = false;
                    continue;
                } else if rest.starts_with("&quot;") {
                    result.push('"');
                    i += 6;
                    last_was_space = false;
                    continue;
                } else if rest.starts_with("&#39;") || rest.starts_with("&apos;") {
                    result.push('\'');
                    i += if rest.starts_with("&#39;") { 5 } else { 6 };
                    last_was_space = false;
                    continue;
                } else if rest.starts_with("&nbsp;") {
                    result.push(' ');
                    i += 6;
                    last_was_space = true;
                    continue;
                }
            }

            if ch.is_whitespace() {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            } else {
                result.push(ch);
                last_was_space = false;
            }

            i += 1;
        }

        result
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn extract_title_from_html(html: &str) -> Option<String> {
        Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
            .expect("valid title regex")
            .captures(html)
            .and_then(|captures| captures.get(1))
            .map(|value| Self::strip_html_tags(value.as_str()))
            .filter(|value| !value.trim().is_empty())
    }

    fn extract_links_from_html(
        html: &str,
        base_url: &str,
        limit: usize,
    ) -> Vec<BrowserExtractedLink> {
        let anchor_re = Regex::new(r#"(?is)<a\b[^>]*href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#)
            .expect("valid anchor regex");
        let base = Url::parse(base_url).ok();
        let mut seen = std::collections::HashSet::new();
        anchor_re
            .captures_iter(html)
            .filter_map(|captures| {
                let href = Self::decode_html_entities(captures.get(1)?.as_str())
                    .trim()
                    .to_string();
                if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
                    return None;
                }
                let url = if href.starts_with("http://") || href.starts_with("https://") {
                    href
                } else {
                    base.as_ref()?.join(&href).ok()?.to_string()
                };
                if !seen.insert(url.clone()) {
                    return None;
                }
                let text = captures
                    .get(2)
                    .map(|value| Self::strip_html_tags(value.as_str()))
                    .unwrap_or_default();
                Some(BrowserExtractedLink { text, url })
            })
            .take(limit)
            .collect()
    }

    fn extract_records_from_html(
        html: &str,
        base_url: &str,
        limit: usize,
    ) -> Vec<BrowserExtractedRecord> {
        let script_re = Regex::new(r"(?is)<script\b[^>]*>(.*?)</script>")
            .expect("valid script extraction regex");
        let mut records = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for captures in script_re.captures_iter(html) {
            let Some(script) = captures.get(1).map(|value| value.as_str().trim()) else {
                continue;
            };
            if !(script.starts_with('{') || script.starts_with('[')) {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(script) else {
                continue;
            };
            Self::collect_records_from_json(&value, base_url, limit, &mut seen, &mut records);
            if records.len() >= limit {
                break;
            }
        }
        records
    }

    fn collect_records_from_json(
        value: &serde_json::Value,
        base_url: &str,
        limit: usize,
        seen: &mut std::collections::HashSet<String>,
        records: &mut Vec<BrowserExtractedRecord>,
    ) {
        if records.len() >= limit {
            return;
        }
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_records_from_json(item, base_url, limit, seen, records);
                    if records.len() >= limit {
                        break;
                    }
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(record) = Self::record_from_json_object(map, base_url) {
                    let key = format!("{}\n{}", record.title, record.url);
                    if seen.insert(key) {
                        records.push(record);
                    }
                }
                for value in map.values() {
                    Self::collect_records_from_json(value, base_url, limit, seen, records);
                    if records.len() >= limit {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    fn record_from_json_object(
        map: &serde_json::Map<String, serde_json::Value>,
        base_url: &str,
    ) -> Option<BrowserExtractedRecord> {
        let title = Self::first_json_string(map, &["title", "name", "bookName", "bName"])?
            .trim()
            .to_string();
        if title.chars().count() < 2 || title.chars().count() > 100 {
            return None;
        }

        let url = Self::first_json_string(map, &["url", "href", "link", "uri"])
            .and_then(|value| Self::normalize_observed_url(value, base_url))
            .or_else(|| {
                Self::first_json_numberish(map, &["bid", "bookId", "id"]).map(|id| {
                    let origin = Url::parse(base_url)
                        .ok()
                        .and_then(|parsed| {
                            Some(format!("{}://{}", parsed.scheme(), parsed.host_str()?))
                        })
                        .unwrap_or_else(|| base_url.trim_end_matches('/').to_string());
                    format!("{}/book/{}/", origin.trim_end_matches('/'), id)
                })
            })
            .unwrap_or_else(|| base_url.to_string());

        let mut metadata = Vec::new();
        for (label, keys) in [
            ("author", &["author", "writer", "bAuth"] as &[&str]),
            ("category", &["category", "cat", "genre"]),
            ("state", &["state", "status"]),
            ("words", &["cnt", "wordCount", "words"]),
            ("price", &["price", "bPrice"]),
            ("summary", &["summary", "description", "desc", "intro"]),
        ] {
            if let Some(value) = Self::first_json_scalar(map, keys) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    metadata.push(format!("{label}: {trimmed}"));
                }
            }
        }

        if metadata.is_empty() {
            return None;
        }

        Some(BrowserExtractedRecord {
            title,
            url,
            metadata: metadata.join(" / "),
        })
    }

    fn first_json_string<'a>(
        map: &'a serde_json::Map<String, serde_json::Value>,
        keys: &[&str],
    ) -> Option<&'a str> {
        keys.iter().find_map(|key| {
            map.get(*key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
        })
    }

    fn first_json_numberish(
        map: &serde_json::Map<String, serde_json::Value>,
        keys: &[&str],
    ) -> Option<String> {
        keys.iter().find_map(|key| {
            map.get(*key).and_then(|value| {
                value
                    .as_u64()
                    .map(|number| number.to_string())
                    .or_else(|| value.as_str().map(str::to_string))
            })
        })
    }

    fn first_json_scalar(
        map: &serde_json::Map<String, serde_json::Value>,
        keys: &[&str],
    ) -> Option<String> {
        keys.iter().find_map(|key| {
            map.get(*key).and_then(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| value.as_i64().map(|number| number.to_string()))
                    .or_else(|| value.as_u64().map(|number| number.to_string()))
                    .or_else(|| value.as_f64().map(|number| number.to_string()))
            })
        })
    }

    fn normalize_observed_url(raw: &str, base_url: &str) -> Option<String> {
        let raw = raw.trim();
        if raw.is_empty()
            || raw.starts_with('#')
            || raw.starts_with("javascript:")
            || raw.starts_with("data:")
        {
            return None;
        }
        if raw.starts_with("http://") || raw.starts_with("https://") {
            return Some(raw.to_string());
        }
        Url::parse(base_url)
            .ok()
            .and_then(|base| base.join(raw).ok())
            .map(|url| url.to_string())
    }

    fn html_to_markdown_lightweight(html: &str) -> String {
        let mut prepared = html.to_string();
        let replacements = [
            (r"(?is)<h1[^>]*>(.*?)</h1>", "\n# $1\n\n"),
            (r"(?is)<h2[^>]*>(.*?)</h2>", "\n## $1\n\n"),
            (r"(?is)<h3[^>]*>(.*?)</h3>", "\n### $1\n\n"),
            (r"(?is)<li[^>]*>(.*?)</li>", "\n- $1"),
            (r"(?is)<p[^>]*>(.*?)</p>", "\n$1\n\n"),
            (r"(?is)<br\s*/?>", "\n"),
        ];
        for (pattern, replacement) in replacements {
            prepared = Regex::new(pattern)
                .expect("valid markdown regex")
                .replace_all(&prepared, replacement)
                .to_string();
        }
        Self::html_to_text_lightweight(&prepared)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn truncate_chars(input: &str, max_chars: usize) -> String {
        let mut output = String::new();
        let mut count = 0usize;
        for ch in input.chars() {
            if count >= max_chars {
                output.push_str("\n[truncated]");
                break;
            }
            output.push(ch);
            count += 1;
        }
        output
    }

    fn page_blockers(page_kind: BrowserPageKind, content: &str) -> Vec<String> {
        let mut blockers = Vec::new();
        if let Some(code) = page_kind.blocker_code() {
            blockers.push(code.to_string());
        }
        if content.trim().is_empty() {
            blockers.push("no_parseable_content".to_string());
        }
        blockers.sort();
        blockers.dedup();
        blockers
    }

    fn provider_descriptor_payload() -> Value {
        if let Some(runtime) = resolve_browser_runtime() {
            let descriptor = provider::BrowserProviderDescriptor::from_runtime_with_interactive_cdp(
                &runtime,
                Self::direct_devtools_session_available(&runtime),
            );
            json!({
                "kind": descriptor.kind.label(),
                "origin": descriptor.origin.label(),
                "executable_path": descriptor.executable_path,
                "semantic_layer": descriptor.semantic_layer(),
                "capabilities": descriptor.capabilities,
                "cdp_inspired_domains": descriptor.cdp_inspired_domains(),
            })
        } else {
            json!({
                "kind": "unavailable",
                "origin": "none",
                "executable_path": null,
                "semantic_layer": "browser_browse",
                "capabilities": {
                    "dump_dom": false,
                    "managed_profile": false,
                    "devtools_session": false,
                    "windows_first": true,
                    "lifecycle_wait": false,
                    "structured_dom_read": false,
                    "readonly_evaluate": false,
                    "network_summary": false,
                    "request_interception_policy": false,
                    "page_session_pool": false
                },
                "cdp_inspired_domains": ["Page", "Runtime", "DOM", "Network", "Fetch", "Input"],
            })
        }
    }

    fn network_summary_payload(
        provider_label: &str,
        final_url: &str,
        notes: Vec<String>,
    ) -> BrowserNetworkSummary {
        BrowserNetworkSummary {
            provider: provider_label.to_string(),
            final_url: final_url.to_string(),
            status_code: None,
            content_type: None,
            redirect_chain: Vec::new(),
            main_document_observed: true,
            notes,
        }
    }

    fn session_guard_payload(action: &str) -> Value {
        json!(session::guard::guard_for_tool_action(action))
    }

    fn render_static_observation_payload(
        action: &str,
        url: &str,
        html: &str,
        format: BrowserSnapshotFormat,
        wait_until: BrowserWaitUntil,
        compact: bool,
        trace: Vec<String>,
    ) -> Value {
        let text = Self::html_to_text_lightweight(html);
        let links = Self::extract_links_from_html(html, url, DEFAULT_BROWSER_DOM_LINK_LIMIT);
        let records = Self::extract_records_from_html(html, url, DEFAULT_BROWSER_DOM_LINK_LIMIT);
        let page_kind = Self::classify_page_content(&text);
        let title = Self::extract_title_from_html(html);
        let content = match format {
            BrowserSnapshotFormat::Semantic | BrowserSnapshotFormat::Text => {
                if compact {
                    line_window(&text, 160).content
                } else {
                    text.clone()
                }
            }
            BrowserSnapshotFormat::Links => {
                serde_json::to_string_pretty(&links).unwrap_or_default()
            }
            BrowserSnapshotFormat::Html => Self::truncate_chars(html, 24_000),
            BrowserSnapshotFormat::Markdown => {
                Self::truncate_chars(&Self::html_to_markdown_lightweight(html), 18_000)
            }
        };
        let blockers = Self::page_blockers(page_kind, &text);
        json!({
            "provider": Self::provider_descriptor_payload(),
            "session": {
                "mode": "one_shot_static",
                "page_session_pool": false,
                "task_bound_handle": null
            },
            "session_guard": Self::session_guard_payload(action),
            "html_input": observe::html_input::html_input_receipt_payload(
                html,
                observe::html_input::DEFAULT_INLINE_HTML_LIMIT.min(html.chars().count())
            ),
            "cdp_boundary": {
                "page": { "wait_until": wait_until.as_str(), "final_url": url, "title": title },
                "runtime": { "readonly_evaluate": false },
                "dom": { "format": format.as_str(), "structured_read": true },
                "network": Self::network_summary_payload(
                    "wsl_bridge_dump_dom",
                    url,
                    vec![
                        "status_code_unavailable_from_dump_dom".to_string(),
                        "interactive_devtools_disabled_under_wsl_bridge".to_string()
                    ]
                ),
                "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                "input": { "interactive_actions": false }
            },
            "action_trace": trace,
            "page": Self::page_classification_payload(page_kind, url, &text),
            "blockers": blockers,
            "title": title,
            "content": content,
            "links": links,
            "records": records,
            "content_chars": text.chars().count(),
            "action": action,
        })
    }

    fn render_windows_cdp_bridge_observation_payload(
        action: &str,
        observation: &WindowsCdpBridgeObservation,
        format: BrowserSnapshotFormat,
        wait_until: BrowserWaitUntil,
        compact: bool,
        mut trace: Vec<String>,
    ) -> Value {
        trace.push(format!("wait {}", wait_until.as_str()));
        trace.push(format!("read_dom {}", format.as_str()));
        let text = observation.text.trim().to_string();
        let content = match format {
            BrowserSnapshotFormat::Semantic | BrowserSnapshotFormat::Text => {
                if compact {
                    line_window(&text, 160).content
                } else {
                    text.clone()
                }
            }
            BrowserSnapshotFormat::Links => {
                serde_json::to_string_pretty(&observation.links).unwrap_or_default()
            }
            BrowserSnapshotFormat::Html => Self::truncate_chars(&observation.html, 24_000),
            BrowserSnapshotFormat::Markdown => Self::truncate_chars(&observation.markdown, 18_000),
        };
        let page_kind = Self::classify_page_content(&text);
        let blockers = Self::page_blockers(page_kind, &text);
        let mut notes = vec![
            "cdp_session_executed_inside_windows_bridge".to_string(),
            "status_code_unavailable_from_current_bridge".to_string(),
        ];
        if let Some(ready_state) = observation.ready_state.as_deref() {
            notes.push(format!("document_ready_state={ready_state}"));
        }
        if let Some(resource_count) = observation.resource_count {
            notes.push(format!("resource_count={resource_count}"));
        }
        json!({
            "provider": Self::provider_descriptor_payload(),
            "session": {
                "mode": "one_shot_windows_cdp_bridge",
                "page_session_pool": false,
                "task_bound_handle": null
            },
            "session_guard": Self::session_guard_payload(action),
            "html_input": observe::html_input::html_input_receipt_payload(
                &observation.html,
                observe::html_input::DEFAULT_INLINE_HTML_LIMIT.min(observation.html.chars().count())
            ),
            "cdp_boundary": {
                "page": {
                    "wait_until": wait_until.as_str(),
                    "final_url": observation.final_url,
                    "title": observation.title
                },
                "runtime": { "readonly_evaluate": true },
                "dom": { "format": format.as_str(), "structured_read": true },
                "network": Self::network_summary_payload(
                    "wsl_windows_cdp_bridge",
                    &observation.final_url,
                    notes
                ),
                "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                "input": { "interactive_actions": false }
            },
            "action_trace": trace,
            "page": Self::page_classification_payload(page_kind, &observation.final_url, &text),
            "blockers": blockers,
            "title": observation.title,
            "content": content,
            "links": observation.links,
            "records": Self::extract_records_from_html(
                &observation.html,
                &observation.final_url,
                DEFAULT_BROWSER_DOM_LINK_LIMIT
            ),
            "content_chars": text.chars().count(),
            "action": action,
        })
    }

    fn validate_readonly_evaluate_expression(expression: &str) -> anyhow::Result<()> {
        let lowered = expression.to_ascii_lowercase();
        let denied = [
            "fetch(",
            "xmlhttprequest",
            "sendbeacon",
            "websocket",
            "localstorage.",
            "sessionstorage.",
            "indexeddb.",
            "document.cookie",
            "document.write",
            "innerhtml",
            "outerhtml",
            "appendchild",
            "removechild",
            "replacechild",
            "insertadjacent",
            "setattribute",
            "removeattribute",
            "click(",
            "submit(",
            "location.href",
            "window.location",
            "history.pushstate",
            "history.replacestate",
        ];
        if let Some(term) = denied.iter().find(|term| lowered.contains(**term)) {
            anyhow::bail!(
                "evaluate is limited to read-only DOM extraction; expression contains blocked term '{}'",
                term
            );
        }
        Ok(())
    }

    pub(crate) fn parse_bing_search_results(
        html: &str,
        max_results: usize,
    ) -> Vec<BrowserSearchResult> {
        let item_re = Regex::new(r#"(?s)<li[^>]*class="[^"]*\bb_algo\b[^"]*"[^>]*>(.*?)</li>"#)
            .expect("valid bing item regex");
        let title_re =
            Regex::new(r#"(?s)<h2[^>]*>\s*<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>\s*</h2>"#)
                .expect("valid bing title regex");
        let snippet_re = Regex::new(r#"(?s)<p[^>]*>(.*?)</p>"#).expect("valid snippet regex");

        item_re
            .captures_iter(html)
            .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
            .filter_map(|item_html| {
                let title_caps = title_re.captures(&item_html)?;
                let raw_url = title_caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let url = Self::normalize_bing_result_url(raw_url);
                let title = title_caps
                    .get(2)
                    .map(|m| Self::strip_html_tags(m.as_str()))
                    .unwrap_or_default();
                let snippet = snippet_re
                    .captures(&item_html)
                    .and_then(|snippet_caps| snippet_caps.get(1).map(|v| v.as_str().to_string()))
                    .map(|value| Self::strip_html_tags(&value))
                    .unwrap_or_default();

                if url.is_empty() || title.is_empty() {
                    return None;
                }

                Some((title, url, snippet))
            })
            .take(max_results)
            .enumerate()
            .map(|(index, (title, url, snippet))| BrowserSearchResult {
                title,
                url,
                snippet,
                source: "bing".to_string(),
                position: index + 1,
            })
            .collect()
    }

    fn normalize_google_result_url(raw_url: &str) -> String {
        let decoded_url = Self::decode_html_entities(raw_url);
        let parsed = match Url::parse(&decoded_url) {
            Ok(parsed) => parsed,
            Err(_) => return decoded_url,
        };

        if !parsed.host_str().unwrap_or_default().contains("google.") {
            return parsed.to_string();
        }

        if parsed.path() == "/url" {
            if let Some(target) = parsed.query_pairs().find_map(|(key, value)| {
                matches!(key.as_ref(), "q" | "url").then(|| value.to_string())
            }) {
                if target.starts_with("http://") || target.starts_with("https://") {
                    return target;
                }
            }
        }

        parsed.to_string()
    }

    fn normalize_search_result_url(engine: &str, raw_url: &str) -> String {
        match engine {
            "bing" => Self::normalize_bing_result_url(raw_url),
            "google" => Self::normalize_google_result_url(raw_url),
            _ => Self::decode_html_entities(raw_url),
        }
    }

    fn push_search_result_candidate(
        results: &mut Vec<BrowserSearchResult>,
        seen: &mut std::collections::HashSet<String>,
        engine: &str,
        title: &str,
        raw_url: &str,
        snippet: &str,
        max_results: usize,
    ) {
        if results.len() >= max_results {
            return;
        }
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let url = Self::normalize_search_result_url(engine, raw_url);
        let Ok(parsed) = Url::parse(&url) else {
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return;
        }
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("bing.com") || host.contains("google.") {
            return;
        }
        if !seen.insert(url.clone()) {
            return;
        }
        results.push(BrowserSearchResult {
            title: title.to_string(),
            url,
            snippet: snippet.trim().to_string(),
            source: engine.to_string(),
            position: results.len() + 1,
        });
    }

    fn parse_generic_search_links_from_html(
        html: &str,
        engine: &str,
        max_results: usize,
    ) -> Vec<BrowserSearchResult> {
        let anchor_re = Regex::new(r#"(?is)<a\b[^>]*href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#)
            .expect("valid anchor regex");
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for captures in anchor_re.captures_iter(html) {
            let raw_url = captures.get(1).map(|value| value.as_str()).unwrap_or("");
            let title = captures
                .get(2)
                .map(|value| Self::strip_html_tags(value.as_str()))
                .unwrap_or_default();
            Self::push_search_result_candidate(
                &mut results,
                &mut seen,
                engine,
                &title,
                raw_url,
                "",
                max_results,
            );
        }
        results
    }

    fn parse_search_results_from_html(
        html: &str,
        engine: &str,
        max_results: usize,
    ) -> Vec<BrowserSearchResult> {
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();

        let structured = match engine {
            "bing" => Self::parse_bing_search_results(html, max_results),
            _ => Vec::new(),
        };
        for item in structured {
            Self::push_search_result_candidate(
                &mut results,
                &mut seen,
                engine,
                &item.title,
                &item.url,
                &item.snippet,
                max_results,
            );
        }

        if results.len() < max_results {
            for item in Self::parse_generic_search_links_from_html(html, engine, max_results) {
                Self::push_search_result_candidate(
                    &mut results,
                    &mut seen,
                    engine,
                    &item.title,
                    &item.url,
                    &item.snippet,
                    max_results,
                );
            }
        }

        results
    }

    fn parse_search_results_from_observation(
        observation: &WindowsCdpBridgeObservation,
        engine: &str,
        max_results: usize,
    ) -> Vec<BrowserSearchResult> {
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();

        for item in Self::parse_search_results_from_html(&observation.html, engine, max_results) {
            Self::push_search_result_candidate(
                &mut results,
                &mut seen,
                engine,
                &item.title,
                &item.url,
                &item.snippet,
                max_results,
            );
        }

        if results.len() < max_results {
            for link in &observation.links {
                Self::push_search_result_candidate(
                    &mut results,
                    &mut seen,
                    engine,
                    &link.text,
                    &link.url,
                    "",
                    max_results,
                );
            }
        }

        results
    }

    fn query_site_constraint(query: &str) -> Option<String> {
        query
            .split_whitespace()
            .find_map(|token| token.strip_prefix("site:"))
            .map(|domain| {
                domain
                    .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '/' | ',' | '.'))
                    .to_ascii_lowercase()
            })
            .filter(|domain| !domain.is_empty())
    }

    fn result_matches_query_constraints(
        query: &str,
        url: &str,
        title: &str,
        snippet: &str,
    ) -> bool {
        if let Some(site_domain) = Self::query_site_constraint(query) {
            let Ok(parsed) = Url::parse(url) else {
                return false;
            };
            let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
            if host != site_domain && !host.ends_with(&format!(".{site_domain}")) {
                return false;
            }
        }

        let lowered = format!(
            "{} {} {}",
            url.to_ascii_lowercase(),
            title.to_ascii_lowercase(),
            snippet.to_ascii_lowercase()
        );
        if lowered.contains("bing.com/ck/")
            || lowered.contains("www.bing.com/")
            || lowered.contains("google.com/search")
        {
            return false;
        }

        Self::result_matches_query_terms(query, url, title, snippet)
    }

    fn result_matches_query_terms(query: &str, url: &str, title: &str, snippet: &str) -> bool {
        let cjk_terms = Self::meaningful_cjk_query_terms(query);
        let ascii_terms = Self::meaningful_ascii_query_terms(query);
        let haystack = format!("{title} {url} {snippet}");
        let lowered_haystack = haystack.to_ascii_lowercase();
        if !cjk_terms.is_empty() {
            if cjk_terms.iter().any(|term| haystack.contains(term)) {
                return true;
            }
            return !ascii_terms.is_empty()
                && ascii_terms
                    .iter()
                    .any(|term| lowered_haystack.contains(term.as_str()));
        }

        let terms = ascii_terms;
        if terms.is_empty() {
            return true;
        }
        let matches = terms
            .iter()
            .filter(|term| lowered_haystack.contains(term.as_str()))
            .count();
        if terms.len() <= 2 {
            return matches >= 1;
        }
        matches >= 2 || matches >= terms.len().saturating_sub(1)
    }

    fn meaningful_ascii_query_terms(query: &str) -> Vec<String> {
        let mut terms = Vec::new();
        let mut seen = HashSet::new();
        for raw in query.split_whitespace() {
            let token = raw
                .trim()
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
                .to_ascii_lowercase();
            if token.len() < 3
                || token.starts_with("site:")
                || token.chars().all(|ch| ch.is_ascii_digit())
                || Self::is_common_search_stopword(&token)
                || Self::is_search_access_modifier(&token)
            {
                continue;
            }
            if seen.insert(token.clone()) {
                terms.push(token);
            }
        }
        terms
    }

    fn meaningful_cjk_query_terms(query: &str) -> Vec<String> {
        let mut terms = Vec::new();
        let mut seen = HashSet::new();
        for run in query
            .split(|ch: char| !('\u{4e00}'..='\u{9fff}').contains(&ch))
            .map(str::trim)
            .filter(|run| run.chars().count() >= 2)
        {
            let chars = run.chars().collect::<Vec<_>>();
            for size in [2usize, 3, 4] {
                if chars.len() < size {
                    continue;
                }
                for window in chars.windows(size) {
                    let term = window.iter().collect::<String>();
                    if Self::is_cjk_search_noise(&term) {
                        continue;
                    }
                    if seen.insert(term.clone()) {
                        terms.push(term);
                    }
                    if terms.len() >= 12 {
                        return terms;
                    }
                }
            }
        }
        terms
    }

    fn is_cjk_search_noise(term: &str) -> bool {
        if term.contains("热门")
            || term.contains("免费")
            || term.contains("下载")
            || term.contains("完整")
            || term.contains("内容")
            || term.contains("公网")
            || term.contains("公开")
            || term.contains("可入")
            || term.contains("入库")
            || term.contains("查找")
            || term.contains("搜索")
            || term.contains("保存")
        {
            return true;
        }
        matches!(
            term,
            "热门"
                | "免费"
                | "下载"
                | "完整"
                | "内容"
                | "公网"
                | "公开"
                | "查找"
                | "搜索"
                | "保存"
        )
    }

    fn is_common_search_stopword(token: &str) -> bool {
        matches!(
            token,
            "the"
                | "and"
                | "for"
                | "with"
                | "from"
                | "into"
                | "onto"
                | "about"
                | "what"
                | "when"
                | "where"
                | "which"
                | "that"
                | "this"
                | "list"
                | "lists"
                | "top"
                | "best"
                | "popular"
                | "representative"
                | "reading"
                | "reference"
                | "source"
                | "sources"
                | "result"
                | "results"
                | "official"
                | "record"
                | "records"
                | "data"
        )
    }

    fn is_search_access_modifier(token: &str) -> bool {
        matches!(
            token,
            "free"
                | "full"
                | "text"
                | "public"
                | "web"
                | "online"
                | "available"
                | "availability"
                | "download"
                | "downloadable"
                | "downloaded"
                | "downloads"
                | "ebook"
                | "ebooks"
                | "content"
                | "contents"
        )
    }

    pub(crate) fn filter_results_for_query(
        query: &str,
        results: Vec<BrowserSearchResult>,
    ) -> Vec<BrowserSearchResult> {
        results
            .into_iter()
            .filter(|result| {
                Self::result_matches_query_constraints(
                    query,
                    &result.url,
                    &result.title,
                    &result.snippet,
                )
            })
            .enumerate()
            .map(|(index, mut result)| {
                result.position = index + 1;
                result
            })
            .collect()
    }

    fn direct_site_link_candidate_looks_useful(task: &str, title: &str, url: &str) -> bool {
        let title = title.trim();
        if title.chars().count() < 2 || url.trim().is_empty() {
            return false;
        }
        let lowered_title = title.to_ascii_lowercase();
        if SearchPolicy::filter_navigation_text_terms_for_task(task)
            .iter()
            .any(|term| {
                let term = term.trim();
                !term.is_empty()
                    && (title == term
                        || (term.is_ascii() && lowered_title == term.to_ascii_lowercase()))
            })
        {
            return false;
        }
        if SearchPolicy::navigation_noise_text_terms_for_task(task)
            .iter()
            .any(|term| {
                let term = term.trim();
                !term.is_empty()
                    && (title == term
                        || (term.is_ascii() && lowered_title == term.to_ascii_lowercase()))
            })
        {
            return false;
        }
        true
    }

    fn push_direct_site_observation_results(
        query: &str,
        task: &str,
        source_url: &str,
        observation: &WindowsCdpBridgeObservation,
        results: &mut Vec<BrowserSearchResult>,
        seen: &mut std::collections::HashSet<String>,
        max_results: usize,
    ) {
        if let Some(title) = observation
            .title
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let snippet = line_window(&observation.text, 320).content;
            Self::push_search_result_candidate(
                results,
                seen,
                "direct_site",
                title,
                &observation.final_url,
                &snippet,
                max_results,
            );
        }

        for link in &observation.links {
            if results.len() >= max_results {
                break;
            }
            if !Self::direct_site_link_candidate_looks_useful(task, &link.text, &link.url) {
                continue;
            }
            Self::push_search_result_candidate(
                results,
                seen,
                "direct_site",
                &link.text,
                &link.url,
                &format!("observed on {source_url}"),
                max_results,
            );
        }

        let filtered = Self::filter_results_for_query(query, std::mem::take(results));
        results.extend(filtered);
    }

    async fn direct_site_search_once(
        query: &str,
        task_context: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<(String, String, Vec<BrowserSearchResult>)> {
        let task = task_context
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(query);
        let seed_urls = SearchPolicy::browser_site_seed_urls_for_task(task);
        if seed_urls.is_empty() {
            anyhow::bail!("no policy-derived direct site seed URLs were available");
        }

        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        let max_pages = SearchPolicy::browser_direct_site_max_pages_for_task(task).min(8);
        let per_attempt_timeout = Duration::from_secs(
            SearchPolicy::browser_direct_site_attempt_timeout_secs_for_task(task),
        );

        for seed_url in seed_urls.into_iter().take(max_pages) {
            let observation = if Self::windows_cdp_bridge_enabled() {
                match Self::observe_once_with_windows_cdp_bridge_with_timeout(
                    &seed_url,
                    BrowserWaitUntil::DomContentLoaded,
                    tokio::time::Duration::from_millis(750),
                    per_attempt_timeout,
                )
                .await
                {
                    Ok(observation) => observation,
                    Err(error) => {
                        errors.push(format!("{seed_url}: {error}"));
                        continue;
                    }
                }
            } else {
                match Self::static_html_once_with_windows_browser(&seed_url).await {
                    Ok(html) => WindowsCdpBridgeObservation {
                        final_url: seed_url.clone(),
                        title: Self::html_title(&html),
                        ready_state: None,
                        text: Self::strip_html_tags(&html),
                        html,
                        markdown: String::new(),
                        links: Vec::new(),
                        resource_count: None,
                    },
                    Err(error) => {
                        errors.push(format!("{seed_url}: {error}"));
                        continue;
                    }
                }
            };

            if Self::page_indicates_search_challenge(&observation.html) {
                errors.push(format!(
                    "{seed_url}: direct site observation hit a challenge page"
                ));
                continue;
            }
            Self::push_direct_site_observation_results(
                query,
                task,
                &seed_url,
                &observation,
                &mut results,
                &mut seen,
                max_results.max(1),
            );
            if !results.is_empty() {
                return Ok((
                    "direct_site".to_string(),
                    seed_url,
                    results.into_iter().take(max_results.max(1)).collect(),
                ));
            }
        }

        Err(anyhow::anyhow!(
            "direct site observation returned no usable link or page records. {}",
            errors.join(" | ")
        ))
    }

    fn page_indicates_search_challenge(html: &str) -> bool {
        let lowered = html.to_ascii_lowercase();
        lowered.contains("turnstile-widget")
            || lowered.contains("cf-chl-opt")
            || lowered.contains("challenge-platform")
            || lowered.contains("just a moment...")
            || lowered.contains("enable javascript and cookies to continue")
            || lowered.contains("g-recaptcha")
            || lowered.contains("our systems have detected unusual traffic")
            || lowered.contains("please solve the following challenge to continue")
    }

    fn classify_page_content(content: &str) -> BrowserPageKind {
        let lowered = content.trim().to_ascii_lowercase();
        if lowered.is_empty() {
            return BrowserPageKind::EmptyShell;
        }
        if Self::page_indicates_search_challenge(&lowered)
            || lowered.contains("cloudflare ray id")
            || lowered.contains("checking if the site connection is secure")
            || lowered.contains("verify you are human")
            || lowered.contains("enable javascript and cookies to continue")
        {
            return BrowserPageKind::Challenge;
        }
        if lowered.contains("sign in")
            || lowered.contains("log in")
            || lowered.contains("login to continue")
            || lowered.contains("please log in")
            || lowered.contains("authentication required")
            || lowered.contains("subscribe to continue")
        {
            return BrowserPageKind::LoginWall;
        }
        let significant_chars = lowered.chars().filter(|ch| !ch.is_whitespace()).count();
        if significant_chars < 120 {
            return BrowserPageKind::EmptyShell;
        }
        BrowserPageKind::Normal
    }

    fn normalize_bing_result_url(raw_url: &str) -> String {
        let decoded_url = Self::decode_html_entities(raw_url);
        let parsed = match Url::parse(&decoded_url) {
            Ok(parsed) => parsed,
            Err(_) => return decoded_url,
        };

        if !parsed.host_str().unwrap_or_default().contains("bing.com") {
            return parsed.to_string();
        }

        let Some(encoded_target) = parsed
            .query_pairs()
            .find_map(|(key, value)| (key == "u").then(|| value.to_string()))
        else {
            return parsed.to_string();
        };

        let payload = encoded_target
            .strip_prefix("a1")
            .unwrap_or(encoded_target.as_str());
        let mut padded = payload.to_string();
        let remainder = padded.len() % 4;
        if remainder != 0 {
            padded.extend(std::iter::repeat_n('=', 4 - remainder));
        }

        base64::engine::general_purpose::STANDARD
            .decode(padded.as_bytes())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|decoded| decoded.starts_with("http://") || decoded.starts_with("https://"))
            .unwrap_or_else(|| parsed.to_string())
    }

    fn pending_browser_followup(action: &str) -> VerificationFollowupPlan {
        VerificationFollowupPlan {
            answer_readiness: "verification_pending".to_string(),
            next_tools: vec!["browser_browse.snapshot".to_string()],
            cite_required: true,
            note: format!(
                "Browser {action} completed, but source content has not been directly read yet. Capture a semantic snapshot before treating the page as confirmed."
            ),
        }
    }

    pub fn new(
        user_data_dir: Option<PathBuf>,
        vault: Option<Arc<dyn SecretVault>>,
        sensory: Arc<benshu_sensory::SensoryHub>,
    ) -> Self {
        let session = BrowserSessionConfig::with_user_data_dir(user_data_dir.clone());
        let user_data_dir = resolve_managed_user_data_dir(user_data_dir);
        Self {
            browser: Arc::new(Mutex::new(None)),
            browser_process: Arc::new(Mutex::new(None)),
            browser_profile_dir: Arc::new(Mutex::new(None)),
            browser_family: Arc::new(Mutex::new(None)),
            session,
            user_data_dir,
            ref_map: Arc::new(Mutex::new(HashMap::new())),
            vault,
            last_snapshot: Arc::new(Mutex::new(None)),
            current_url: Arc::new(Mutex::new(None)),
            sensory,
        }
    }

    fn resolve_search_engine_order(engine: Option<&str>) -> anyhow::Result<Vec<&'static str>> {
        match engine.unwrap_or("auto").to_ascii_lowercase().as_str() {
            "auto" => Ok(vec!["google", "bing"]),
            "google" => Ok(vec!["google"]),
            "bing" => Ok(vec!["bing"]),
            other => Err(anyhow::anyhow!(
                "Unsupported browser search engine: {other}. Use 'auto', 'google', or 'bing'."
            )),
        }
    }

    fn browser_search_url(engine: &str, query: &str) -> anyhow::Result<String> {
        match engine {
            "google" => Ok(format!(
                "https://www.google.com/search?q={}&hl=en",
                urlencoding::encode(query)
            )),
            "bing" => Ok(format!(
                "https://www.bing.com/search?q={}",
                urlencoding::encode(query)
            )),
            other => Err(anyhow::anyhow!(
                "Unsupported browser search engine: {other}"
            )),
        }
    }

    fn get_browser(&self) -> Result<Browser> {
        let mut guard = self.browser.lock();
        if let Some(browser) = guard.as_ref() {
            return Ok(browser.clone());
        }

        if let Some(browser_runtime) = resolve_browser_runtime() {
            if Self::is_wsl() && browser_runtime.is_windows_executable() {
                let browser = self.launch_windows_interactive_cdp_browser().map_err(|error| {
                    Error::Internal(format!(
                        "Failed to initialize Windows interactive browser CDP provider. Static dump-dom fallback remains available for one-shot search/snapshot. Browser runtime: {} ({}). Error: {}",
                        browser_runtime.diagnostic_summary(),
                        browser_runtime.origin.user_description(),
                        error
                    ))
                })?;
                *guard = Some(browser.clone());
                return Ok(browser);
            }
        }

        let mut options = LaunchOptions::default();
        options.headless = true;

        options.args = vec![
            std::ffi::OsStr::new("--disable-blink-features=AutomationControlled"),
            std::ffi::OsStr::new("--no-sandbox"),
            std::ffi::OsStr::new("--disable-infobars"),
            std::ffi::OsStr::new("--window-size=1920,1080"),
            std::ffi::OsStr::new("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
        ];

        if let Some(dir) = &self.user_data_dir {
            options.user_data_dir = Some(dir.clone());
        }

        if let Some(browser_runtime) = resolve_browser_runtime() {
            tracing::debug!(
                browser_runtime = %browser_runtime.diagnostic_summary(),
                browser_runtime_origin = browser_runtime.origin.label(),
                "launching browser runtime"
            );
            options.path = Some(browser_runtime.executable_path);
        }

        let browser = Browser::new(options)
            .map_err(|e| Error::Internal(format!("Failed to launch browser: {}", e)))?;

        *guard = Some(browser.clone());
        Ok(browser)
    }

    async fn extract_aria_tree_with_refs(
        &self,
        tab: Arc<Tab>,
        interactive_only: bool,
        compact: bool,
    ) -> Result<BrowserSnapshot> {
        let script = format!(
            r#"
            (function() {{
                let refCounter = 0;
                let refs = {{}};
                let nameCounter = {{}};

                function getSemanticInfo(node, depth = 0) {{
                    if (depth > 15) return "";
                    if (!node || node.nodeType !== 1) return "";

                    const style = getComputedStyle(node);
                    if (style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') return "";

                    let role = node.getAttribute ? node.getAttribute('role') : null;
                    let label = node.ariaLabel || node.innerText || node.value || "";
                    
                    const interactiveTags = ['BUTTON', 'A', 'INPUT', 'SELECT', 'TEXTAREA', 'DETAILS', 'SUMMARY'];
                    const isAlwaysInteractive = interactiveTags.includes(node.tagName);
                    const hasCursorPointer = style.cursor === 'pointer';
                    const hasOnClick = node.hasAttribute('onclick') || node.onclick !== null;
                    const hasTabIndex = node.hasAttribute('tabindex') && node.getAttribute('tabindex') !== '-1';

                    const isInteractive = role || isAlwaysInteractive || hasCursorPointer || hasOnClick || hasTabIndex;
                    const isHeading = node.tagName.startsWith('H') && node.tagName.length <= 2;

                    let info = "";
                    if (isInteractive || isHeading || !{}) {{
                        let indent = "  ".repeat(depth);
                        let name = node.tagName.toLowerCase();
                        let cleanLabel = label.trim().substring(0, 100).replace(/\n/g, ' ');
                        
                        if (isInteractive || (isHeading && !{})) {{
                            let refId = `e${{++refCounter}}`;
                            node.setAttribute('data-benshu-ref', refId);
                            let selector = `[data-benshu-ref="${{refId}}"]`;
                            
                            let key = `${{role || name}}:${{cleanLabel}}`;
                            nameCounter[key] = (nameCounter[key] || 0) + 1;
                            let nth = nameCounter[key] > 1 ? ` [nth=${{nameCounter[key]-1}}]` : "";
                            
                            refs[refId] = selector;
                            info += `${{indent}}[${{role || name}}] "${{cleanLabel}}" [ref=@${{refId}}]${{nth}}\n`;
                            
                            for (let child of node.children) {{
                                info += getSemanticInfo(child, depth + 1);
                            }}
                        }} else if (!{}) {{
                            if (cleanLabel || !{}) {{
                                info += `${{indent}}<${{name}}>\n`;
                                for (let child of node.children) {{
                                    info += getSemanticInfo(child, depth + 1);
                                }}
                            }}
                        }}
                    }} else {{
                        for (let child of node.children) {{
                            info += getSemanticInfo(child, depth);
                        }}
                    }}
                    return info;
                }}
                
                const tree = getSemanticInfo(document.body);
                return {{ tree, refs }};
            }})()
        "#,
            interactive_only, compact, interactive_only, compact
        );

        let remote_object = tab
            .evaluate(&script, false)
            .map_err(|e| Error::Internal(format!("Failed to evaluate ARIA script: {}", e)))?;

        let value: Value = remote_object
            .value
            .ok_or_else(|| Error::Internal("No value returned from ARIA script".to_string()))?;
        let snapshot: BrowserSnapshot = serde_json::from_value(value)
            .map_err(|e| Error::Internal(format!("Failed to parse snapshot JSON: {}", e)))?;

        let mut map_guard = self.ref_map.lock();
        map_guard.clear();
        for (k, v) in &snapshot.refs {
            map_guard.insert(format!("@{}", k), v.clone());
        }

        Ok(snapshot)
    }

    pub fn resolve_selector(&self, selector: &str) -> String {
        let guard = self.ref_map.lock();
        if let Some(resolved) = guard.get(selector) {
            resolved.clone()
        } else {
            selector.to_string()
        }
    }

    /// Resolves which @eN ref is at the given coordinates (Phase 11.2)
    pub async fn resolve_ref_from_coords(&self, x: f32, y: f32) -> Result<String> {
        let browser = self.get_browser()?;
        let tabs = browser.get_tabs();
        let tab = tabs
            .lock()
            .map_err(|e| Error::Internal(format!("Mutex poisoned: {}", e)))?
            .get(0)
            .cloned()
            .ok_or_else(|| Error::NotFound("No open tabs to resolve coordinates".into()))?;

        let script = format!(
            r#"
            (function() {{
                let el = document.elementFromPoint({}, {});
                while (el) {{
                    let ref = el.getAttribute('data-benshu-ref');
                    if (ref) return "@" + ref;
                    el = el.parentElement;
                }}
                return null;
            }})()
        "#,
            x, y
        );

        let remote_object = tab
            .evaluate(&script, false)
            .map_err(|e| Error::Internal(format!("Failed to resolve coordinate ref: {}", e)))?;

        let value: Value = remote_object
            .value
            .ok_or_else(|| Error::NotFound("No element found at coordinates".into()))?;
        if value.is_null() {
            return Err(Error::NotFound(format!(
                "No interactive element found at [{}, {}]",
                x, y
            )));
        }

        Ok(value.as_str().unwrap_or("unknown").to_string())
    }

    /// Internal method for other tools to get raw image data without base64 truncation.
    pub async fn screenshot_binary(&self, som: bool) -> anyhow::Result<Vec<u8>> {
        let browser = self.get_browser()?;
        let tabs_arc = browser.get_tabs();
        let tab = {
            let tabs = tabs_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
            if tabs.is_empty() {
                browser
                    .new_tab()
                    .map_err(|e| anyhow::anyhow!("Failed to open tab: {}", e))?
            } else {
                tabs[0].clone()
            }
        };

        if som {
            let _ = self
                .extract_aria_tree_with_refs(tab.clone(), true, true)
                .await?;
            let inject_som = r#"
                (function() {
                    const somContainerId = 'benshu-som-overlay';
                    let container = document.getElementById(somContainerId);
                    if (container) container.remove();
                    
                    container = document.createElement('div');
                    container.id = somContainerId;
                    container.style.position = 'absolute';
                    container.style.top = '0';
                    container.style.left = '0';
                    container.style.width = '100%';
                    container.style.height = '100%';
                    container.style.pointerEvents = 'none';
                    container.style.zIndex = '2147483647';
                    document.body.appendChild(container);

                    const elements = document.querySelectorAll('[data-benshu-ref]');
                    elements.forEach(el => {
                        const rect = el.getBoundingClientRect();
                        if (rect.width > 0 && rect.height > 0) {
                            const ref = el.getAttribute('data-benshu-ref');
                            const tag = document.createElement('div');
                            tag.textContent = ref;
                            tag.style.position = 'absolute';
                            tag.style.left = (rect.left + window.scrollX) + 'px';
                            tag.style.top = (rect.top + window.scrollY) - 15 + 'px'; 
                            tag.style.background = 'rgba(0, 0, 0, 0.85)';
                            tag.style.color = '#00ff00'; 
                            tag.style.fontSize = '12px';
                            tag.style.padding = '1px 3px';
                            tag.style.borderRadius = '2px';
                            tag.style.fontFamily = 'monospace';
                            tag.style.fontWeight = 'bold';
                            tag.style.border = '1px solid #00ff00';
                            tag.style.boxShadow = '0 0 5px rgba(0,255,0,0.5)';
                            container.appendChild(tag);
                        }
                    });
                })()
            "#;
            tab.evaluate(inject_som, false)?;
        }

        let png_data = tab
            .capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                None,
                true,
            )
            .map_err(|e| anyhow::anyhow!("Failed to capture screenshot: {}", e))?;

        if som {
            let _ = tab.evaluate(
                "document.getElementById('benshu-som-overlay')?.remove()",
                false,
            );
        }

        Ok(png_data)
    }

    fn extract_browser_search_results(
        tab: &Arc<Tab>,
        query: &str,
        engine: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<BrowserSearchResult>> {
        let script = match engine {
            "google" => format!(
                r#"
                (function() {{
                    const parseGoogleUrl = (rawUrl) => {{
                        if (!rawUrl) return "";
                        try {{
                            const parsed = new URL(rawUrl, window.location.origin);
                            if (parsed.hostname.includes("google.") && parsed.pathname === "/url") {{
                                return parsed.searchParams.get("q") || parsed.searchParams.get("url") || "";
                            }}
                            return parsed.href;
                        }} catch {{
                            return "";
                        }}
                    }};
                    const isSearchPageUrl = (url) => {{
                        if (!url) return true;
                        try {{
                            const parsed = new URL(url);
                            return parsed.hostname.includes("google.") &&
                                (parsed.pathname === "/search" || parsed.pathname === "/url");
                        }} catch {{
                            return true;
                        }}
                    }};
                    const extracted = [];
                    const seen = new Set();
                    const items = Array.from(document.querySelectorAll("div#search div.g"));
                    for (const item of items) {{
                        if (extracted.length >= {max_results}) break;
                        const titleNode = item.querySelector("h3");
                        const anchorNode = titleNode?.closest("a") || item.querySelector("a[href]");
                        const title = titleNode?.textContent?.trim() || anchorNode?.textContent?.trim() || "";
                        const url = parseGoogleUrl(anchorNode?.href || "");
                        const snippetNode = item.querySelector(".VwiC3b, .yXK7lf, span.aCOpRe, div.IsZvec");
                        const snippet = snippetNode?.textContent?.trim() || "";
                        if (!title || !url || isSearchPageUrl(url) || seen.has(url)) continue;
                        seen.add(url);
                        extracted.push({{
                            title,
                            url,
                            snippet,
                            source: "google",
                            position: extracted.length + 1
                        }});
                    }}
                    return extracted;
                }})()
                "#
            ),
            "bing" => format!(
                r#"
                (function() {{
                    const extracted = [];
                    const items = Array.from(document.querySelectorAll("li.b_algo"));
                    for (let i = 0; i < Math.min(items.length, {max_results}); i++) {{
                        const item = items[i];
                        const titleEl = item.querySelector("h2 a");
                        const snippetEl = item.querySelector(".b_caption p, .b_caption");
                        if (!titleEl) continue;
                        const title = titleEl.textContent?.trim() || "";
                        const url = titleEl.href || "";
                        const snippet = snippetEl?.textContent?.trim() || "";
                        if (!title || !url) continue;
                        extracted.push({{
                            title,
                            url,
                            snippet,
                            source: "bing",
                            position: extracted.length + 1
                        }});
                    }}
                    return extracted;
                }})()
                "#
            ),
            other => {
                return Err(anyhow::anyhow!(
                    "Unsupported browser search engine: {other}"
                ));
            }
        };

        let remote_object = tab
            .evaluate(&script, false)
            .map_err(|e| anyhow::anyhow!("Failed to evaluate browser search extraction: {}", e))?;
        let value = remote_object
            .value
            .ok_or_else(|| anyhow::anyhow!("Browser search extraction returned no value"))?;
        let results: Vec<BrowserSearchResult> = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to parse browser search results: {}", e))?;
        Ok(Self::filter_results_for_query(query, results))
    }

    fn render_plain_search_result(
        query: &str,
        engine: &str,
        results: &[BrowserSearchResult],
    ) -> String {
        render_search_results(
            query,
            engine,
            &results
                .iter()
                .map(|result| SearchResultSummaryItem {
                    title: result.title.clone(),
                    url: result.url.clone(),
                    snippet: result.snippet.clone(),
                })
                .collect::<Vec<_>>(),
            800,
        )
    }

    fn render_structured_search_result(
        query: &str,
        engine: &str,
        search_url: &str,
        results: &[BrowserSearchResult],
    ) -> anyhow::Result<String> {
        let verification_plan = QueryVerificationPlan {
            domain: VerificationDomain::KnowledgeFact,
            requirement: VerificationRequirement::Required,
            mode: VerificationMode::WebSearchFetch,
            route_hint: None,
        };
        let verification_preview = build_verified_verification_result_envelope(
            VerificationDomain::KnowledgeFact,
            VerificationMode::WebSearchFetch,
            vec![VerificationSource {
                kind: "browser_search_results".to_string(),
                title: format!("{engine} browser search results"),
                uri: search_url.to_string(),
                observed_at: Some(chrono::Utc::now().to_rfc3339()),
            }],
            "browser search results observed",
        );
        let verification_followup = build_search_result_followup_plan();
        let orchestration_decision = WebVerificationOrchestrator::new().decide(
            Some(&verification_plan),
            Some(&verification_preview),
            Some(&verification_followup),
        );
        Ok(serde_json::to_string_pretty(&json!({
            "kind": "browser_browse",
            "action": "search",
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "route_reason": route_reason_for_plan(Some(&verification_plan)).as_str(),
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "orchestration_decision": orchestration_decision,
            "result": {
                "query": query,
                "engine": engine,
                "search_url": search_url,
                "results": results,
                "total_results": results.len()
            },
        }))?)
    }

    async fn collect_browser_search(
        &self,
        tab: Arc<Tab>,
        query: &str,
        engine: Option<&str>,
        wait_time: tokio::time::Duration,
        max_results: usize,
    ) -> anyhow::Result<(String, String, Vec<BrowserSearchResult>)> {
        let mut errors = Vec::new();

        for engine_name in Self::resolve_search_engine_order(engine)? {
            let search_url = Self::browser_search_url(engine_name, query)?;
            if let Err(error) = tab.navigate_to(&search_url)?.wait_until_navigated() {
                errors.push(format!("{engine_name}: {error}"));
                continue;
            }

            tokio::time::sleep(wait_time).await;

            match tab.get_content() {
                Ok(html) if Self::page_indicates_search_challenge(&html) => {
                    errors.push(format!(
                        "{engine_name}: search engine challenge page blocked automated access"
                    ));
                    continue;
                }
                Ok(_) => {}
                Err(error) => {
                    errors.push(format!(
                        "{engine_name}: failed to inspect browser content before parsing: {error}"
                    ));
                    continue;
                }
            }

            match Self::extract_browser_search_results(&tab, query, engine_name, max_results) {
                Ok(results) if !results.is_empty() => {
                    return Ok((engine_name.to_string(), search_url, results));
                }
                Ok(_) => errors.push(format!("{engine_name}: no parsable search results")),
                Err(error) => errors.push(format!("{engine_name}: {error}")),
            }
        }

        Err(anyhow::anyhow!(
            "All browser search engines failed. {}",
            errors.join(" | ")
        ))
    }

    async fn perform_browser_search(
        &self,
        tab: Arc<Tab>,
        query: &str,
        engine: Option<&str>,
        wait_time: tokio::time::Duration,
        max_results: usize,
        structured: bool,
    ) -> anyhow::Result<String> {
        let (engine_name, search_url, results) = self
            .collect_browser_search(tab, query, engine, wait_time, max_results)
            .await?;
        if structured {
            Self::render_structured_search_result(query, &engine_name, &search_url, &results)
        } else {
            Ok(Self::render_plain_search_result(
                query,
                &engine_name,
                &results,
            ))
        }
    }

    pub(crate) async fn search_once(
        query: &str,
        engine: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<(String, String, Vec<BrowserSearchResult>)> {
        if Self::static_wsl_windows_browser_runtime().is_some() {
            let cdp_error =
                Some("not_attempted_under_wsl_bridge; using Windows-side CDP bridge".to_string());
            let engine_name = match engine.unwrap_or("auto").to_ascii_lowercase().as_str() {
                "bing" | "auto" | "google" => "bing".to_string(),
                other => {
                    return Err(anyhow::anyhow!(
                    "Unsupported browser search engine: {other}. Use 'auto', 'google', or 'bing'."
                ))
                }
            };
            let search_url = Self::browser_search_url(&engine_name, query)?;
            let bridge_error = if Self::windows_cdp_bridge_enabled() {
                match Self::observe_once_with_windows_cdp_bridge_with_timeout(
                    &search_url,
                    BrowserWaitUntil::DomContentLoaded,
                    tokio::time::Duration::from_millis(750),
                    Self::windows_browser_search_cdp_timeout(),
                )
                .await
                {
                    Ok(observation) => {
                        if Self::page_indicates_search_challenge(&observation.html) {
                            Some(
                                "Windows CDP bridge observed an anti-bot challenge page"
                                    .to_string(),
                            )
                        } else {
                            let results = Self::filter_results_for_query(
                                query,
                                Self::parse_search_results_from_observation(
                                    &observation,
                                    &engine_name,
                                    max_results.max(1),
                                ),
                            );
                            if !results.is_empty() {
                                return Ok((engine_name, search_url, results));
                            }
                            Some(
                                "Windows CDP bridge returned no relevant parsable results"
                                    .to_string(),
                            )
                        }
                    }
                    Err(error) => Some(error.to_string()),
                }
            } else {
                None
            };
            let static_error = match Self::static_html_once_with_windows_browser(&search_url).await
            {
                Ok(html) => {
                    if Self::page_indicates_search_challenge(&html) {
                        Some(
                            "Browser search engine returned an anti-bot challenge page".to_string(),
                        )
                    } else {
                        let results = Self::filter_results_for_query(
                            query,
                            Self::parse_search_results_from_html(
                                &html,
                                &engine_name,
                                max_results.max(1),
                            ),
                        );
                        if !results.is_empty() {
                            return Ok((engine_name, search_url, results));
                        }
                        Some(
                            "Windows browser search returned no relevant parsable results"
                                .to_string(),
                        )
                    }
                }
                Err(error) => Some(error.to_string()),
            };

            return Err(anyhow::anyhow!(
                "Windows browser search failed. interactive_cdp={}; cdp_error={}; cdp_bridge_error={}; static_error={}",
                Self::windows_interactive_cdp_enabled(),
                cdp_error.unwrap_or_else(|| "not_attempted".to_string()),
                bridge_error.unwrap_or_else(|| "not_attempted".to_string()),
                static_error.unwrap_or_else(|| "unknown static browser error".to_string())
            ));
        }

        Self::search_once_via_devtools(query, engine, max_results).await
    }

    async fn search_once_with_direct_site_fallback(
        query: &str,
        task_context: Option<&str>,
        engine: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<(String, String, Vec<BrowserSearchResult>)> {
        match Self::search_once(query, engine, max_results).await {
            Ok(result) => Ok(result),
            Err(search_error) => {
                match Self::direct_site_search_once(query, task_context, max_results).await {
                    Ok(result) => Ok(result),
                    Err(direct_error) => Err(anyhow::anyhow!(
                        "{search_error}; direct_site_fallback_error={direct_error}"
                    )),
                }
            }
        }
    }

    async fn search_once_via_devtools(
        query: &str,
        engine: Option<&str>,
        max_results: usize,
    ) -> anyhow::Result<(String, String, Vec<BrowserSearchResult>)> {
        let tool = Self::new(
            None,
            None,
            Arc::new(benshu_sensory::SensoryHub::new(
                benshu_sensory::SensoryConfig::default(),
            )),
        );
        let browser = tool
            .get_browser()
            .map_err(|e| anyhow::anyhow!("Failed to initialize browser search: {}", e))?;
        let tabs_arc = browser.get_tabs();
        let tab = {
            let tabs = tabs_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
            if tabs.is_empty() {
                browser
                    .new_tab()
                    .map_err(|e| anyhow::anyhow!("Failed to open tab: {}", e))?
            } else {
                tabs[0].clone()
            }
        };

        let result = tool
            .collect_browser_search(
                tab,
                query,
                engine,
                tokio::time::Duration::from_millis(1000),
                max_results.max(1),
            )
            .await;
        tool.close_browser_for_one_shot();
        result
    }

    fn close_browser_for_one_shot(&self) {
        self.close_browser_process_tree();
    }

    fn close_browser_process_tree(&self) {
        let profile_dir = self.browser_profile_dir.lock().take();
        let browser_family = self.browser_family.lock().take();

        if let Some(mut child) = self.browser_process.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        if let (Some(profile_dir), Some(browser_family)) = (profile_dir, browser_family) {
            Self::cleanup_windows_dump_dom_processes(&profile_dir, browser_family);
            if self.user_data_dir.as_deref() != Some(profile_dir.as_path()) {
                let _ = std::fs::remove_dir_all(&profile_dir);
            }
        }

        *self.browser.lock() = None;
    }

    pub(crate) async fn snapshot_once(
        url: &str,
        interactive_only: bool,
        compact: bool,
    ) -> anyhow::Result<String> {
        BrowserSafetyGate::validate_public_web_url(url)
            .map_err(|reason| anyhow::anyhow!("browser snapshot blocked URL: {reason}"))?;
        if Self::static_wsl_windows_browser_runtime().is_some() {
            if Self::windows_cdp_bridge_enabled() {
                if let Ok(observation) = Self::observe_once_with_windows_cdp_bridge(
                    url,
                    BrowserWaitUntil::Load,
                    tokio::time::Duration::from_millis(1000),
                )
                .await
                {
                    let payload = Self::render_windows_cdp_bridge_observation_payload(
                        "snapshot",
                        &observation,
                        BrowserSnapshotFormat::Text,
                        BrowserWaitUntil::Load,
                        compact,
                        vec![format!("navigate {url}")],
                    );
                    if let Some(content) = payload.get("content").and_then(Value::as_str) {
                        return Ok(content.to_string());
                    }
                }
            }
            if Self::windows_direct_cdp_under_wsl_enabled()
                && Self::windows_interactive_cdp_enabled()
            {
                let tool = Self::new(
                    None,
                    None,
                    Arc::new(benshu_sensory::SensoryHub::new(
                        benshu_sensory::SensoryConfig::default(),
                    )),
                );
                let result = async {
                    let browser = tool.get_browser().map_err(|e| {
                        anyhow::anyhow!("Failed to initialize browser snapshot: {}", e)
                    })?;
                    let tabs_arc = browser.get_tabs();
                    let tab = {
                        let tabs = tabs_arc
                            .lock()
                            .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
                        if tabs.is_empty() {
                            browser
                                .new_tab()
                                .map_err(|e| anyhow::anyhow!("Failed to open tab: {}", e))?
                        } else {
                            tabs[0].clone()
                        }
                    };

                    tab.navigate_to(url)?.wait_until_navigated()?;
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    let snapshot = tool
                        .extract_aria_tree_with_refs(tab, interactive_only, compact)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to capture browser snapshot: {}", e)
                        })?;
                    Ok(snapshot.tree)
                }
                .await;
                tool.close_browser_for_one_shot();
                if result.is_ok() {
                    return result;
                }
            }
            let html = Self::static_html_once_with_windows_browser(url).await?;
            let text = Self::html_to_text_lightweight(&html);
            let snapshot = if interactive_only || compact {
                line_window(&text, if compact { 80 } else { 160 }).content
            } else {
                text
            };
            return Ok(snapshot);
        }

        let tool = Self::new(
            None,
            None,
            Arc::new(benshu_sensory::SensoryHub::new(
                benshu_sensory::SensoryConfig::default(),
            )),
        );
        let result = async {
            let browser = tool
                .get_browser()
                .map_err(|e| anyhow::anyhow!("Failed to initialize browser snapshot: {}", e))?;
            let tabs_arc = browser.get_tabs();
            let tab = {
                let tabs = tabs_arc
                    .lock()
                    .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
                if tabs.is_empty() {
                    browser
                        .new_tab()
                        .map_err(|e| anyhow::anyhow!("Failed to open tab: {}", e))?
                } else {
                    tabs[0].clone()
                }
            };

            tab.navigate_to(url)?.wait_until_navigated()?;
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            let snapshot = tool
                .extract_aria_tree_with_refs(tab, interactive_only, compact)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to capture browser snapshot: {}", e))?;
            Ok(snapshot.tree)
        }
        .await;
        tool.close_browser_for_one_shot();
        result
    }

    async fn wait_for_page_lifecycle(
        tab: &Arc<Tab>,
        wait_until: BrowserWaitUntil,
        wait_time: tokio::time::Duration,
    ) -> anyhow::Result<()> {
        match wait_until {
            BrowserWaitUntil::DomContentLoaded => {
                tokio::time::sleep(wait_time.min(tokio::time::Duration::from_millis(750))).await;
            }
            BrowserWaitUntil::Load => {
                let _ = tab
                    .evaluate("document.readyState", false)
                    .map_err(|e| anyhow::anyhow!("Failed to inspect document readyState: {}", e))?;
                tokio::time::sleep(wait_time).await;
            }
            BrowserWaitUntil::NetworkIdle => {
                let started = Instant::now();
                let timeout = wait_time.max(tokio::time::Duration::from_millis(1_000));
                let mut last_resource_count = None;
                let mut stable_ticks = 0usize;
                while started.elapsed() < timeout {
                    let value = tab
                        .evaluate("performance.getEntriesByType('resource').length", false)
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to inspect browser resource timing: {}", e)
                        })?
                        .value
                        .unwrap_or(Value::Null);
                    let count = value.as_u64().unwrap_or(0);
                    if Some(count) == last_resource_count {
                        stable_ticks += 1;
                        if stable_ticks >= 2 {
                            break;
                        }
                    } else {
                        stable_ticks = 0;
                        last_resource_count = Some(count);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                }
            }
        }
        Ok(())
    }

    fn extract_links_from_tab(
        tab: &Arc<Tab>,
        limit: usize,
    ) -> anyhow::Result<Vec<BrowserExtractedLink>> {
        let script = format!(
            r#"
            (function() {{
                const out = [];
                const seen = new Set();
                for (const a of Array.from(document.querySelectorAll('a[href]'))) {{
                    if (out.length >= {limit}) break;
                    const href = a.href || '';
                    if (!href || seen.has(href) || href.startsWith('javascript:')) continue;
                    seen.add(href);
                    out.push({{ text: (a.innerText || a.textContent || '').trim().slice(0, 240), url: href }});
                }}
                return out;
            }})()
            "#
        );
        let value = tab
            .evaluate(&script, false)
            .map_err(|e| anyhow::anyhow!("Failed to extract links from browser DOM: {}", e))?
            .value
            .unwrap_or(Value::Null);
        serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to parse browser links: {}", e))
    }

    fn markdown_from_tab(tab: &Arc<Tab>) -> anyhow::Result<String> {
        let script = r#"
        (function() {
            function clean(s) { return (s || '').replace(/\s+/g, ' ').trim(); }
            const lines = [];
            for (const el of Array.from(document.body ? document.body.querySelectorAll('h1,h2,h3,p,li,blockquote,td,th') : [])) {
                const text = clean(el.innerText || el.textContent || '');
                if (!text) continue;
                const tag = el.tagName.toLowerCase();
                if (tag === 'h1') lines.push('# ' + text);
                else if (tag === 'h2') lines.push('## ' + text);
                else if (tag === 'h3') lines.push('### ' + text);
                else if (tag === 'li') lines.push('- ' + text);
                else if (tag === 'blockquote') lines.push('> ' + text);
                else lines.push(text);
            }
            return lines.join('\n\n');
        })()
        "#;
        let value = tab
            .evaluate(script, false)
            .map_err(|e| anyhow::anyhow!("Failed to render browser DOM as markdown: {}", e))?
            .value
            .unwrap_or(Value::Null);
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    async fn render_interactive_observation_payload(
        &self,
        tab: Arc<Tab>,
        action: &str,
        format: BrowserSnapshotFormat,
        wait_until: BrowserWaitUntil,
        wait_time: tokio::time::Duration,
        interactive_only: bool,
        compact: bool,
        mut trace: Vec<String>,
    ) -> anyhow::Result<Value> {
        Self::wait_for_page_lifecycle(&tab, wait_until, wait_time).await?;
        trace.push(format!("wait {}", wait_until.as_str()));
        let url = tab.get_url();
        let title = tab.get_title().ok();
        let links =
            Self::extract_links_from_tab(&tab, DEFAULT_BROWSER_DOM_LINK_LIMIT).unwrap_or_default();
        let content = match format {
            BrowserSnapshotFormat::Semantic => {
                let snapshot = self
                    .extract_aria_tree_with_refs(tab.clone(), interactive_only, compact)
                    .await?;
                *self.last_snapshot.lock() = Some(snapshot.tree.clone());
                snapshot.tree
            }
            BrowserSnapshotFormat::Text => {
                let value = tab
                    .evaluate("document.body ? document.body.innerText : ''", false)
                    .map_err(|e| anyhow::anyhow!("Failed to read browser text: {}", e))?
                    .value
                    .unwrap_or(Value::Null);
                value.as_str().unwrap_or_default().to_string()
            }
            BrowserSnapshotFormat::Links => {
                serde_json::to_string_pretty(&links).unwrap_or_default()
            }
            BrowserSnapshotFormat::Html => Self::truncate_chars(&tab.get_content()?, 24_000),
            BrowserSnapshotFormat::Markdown => {
                Self::truncate_chars(&Self::markdown_from_tab(&tab)?, 18_000)
            }
        };
        let page_kind = Self::classify_page_content(&content);
        let blockers = Self::page_blockers(page_kind, &content);
        let html_snapshot = if matches!(format, BrowserSnapshotFormat::Html) {
            content.clone()
        } else {
            tab.get_content().unwrap_or_default()
        };
        Ok(json!({
            "provider": Self::provider_descriptor_payload(),
            "session": {
                "mode": "interactive_devtools",
                "page_session_pool": true,
                "task_bound_handle": "browser://current-tab"
            },
            "session_guard": Self::session_guard_payload(action),
            "html_input": observe::html_input::html_input_receipt_payload(
                &html_snapshot,
                observe::html_input::DEFAULT_INLINE_HTML_LIMIT.min(html_snapshot.chars().count())
            ),
            "cdp_boundary": {
                "page": { "wait_until": wait_until.as_str(), "final_url": url, "title": title },
                "runtime": { "readonly_evaluate": true },
                "dom": { "format": format.as_str(), "structured_read": true },
                "network": Self::network_summary_payload(
                    "edge_chrome_devtools",
                    &url,
                    vec!["full_network_event_ledger_not_enabled_for_current_headless_chrome_backend".to_string()]
                ),
                "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                "input": { "interactive_actions": true }
            },
            "action_trace": trace,
            "page": Self::page_classification_payload(page_kind, &url, &content),
            "blockers": blockers,
            "title": title,
            "content": content,
            "links": links,
            "content_chars": content.chars().count(),
            "action": action,
        }))
    }

    fn render_structured_result(
        action: &str,
        domain: VerificationDomain,
        note: &str,
        sources: Vec<VerificationSource>,
        result: Value,
    ) -> anyhow::Result<String> {
        let verification_followup = match (action, domain) {
            ("snapshot", VerificationDomain::KnowledgeFact) => {
                Some(build_source_observed_followup_plan(true))
            }
            ("navigate" | "screenshot", VerificationDomain::KnowledgeFact) => {
                Some(Self::pending_browser_followup(action))
            }
            _ => None,
        };
        let verification_plan = QueryVerificationPlan {
            domain,
            requirement: VerificationRequirement::Required,
            mode: VerificationMode::BrowserValidation,
            route_hint: None,
        };
        let verification_preview = build_verified_verification_result_envelope(
            domain,
            VerificationMode::BrowserValidation,
            sources,
            note,
        );
        let orchestration_decision = WebVerificationOrchestrator::new().decide(
            Some(&verification_plan),
            Some(&verification_preview),
            verification_followup.as_ref(),
        );
        Ok(serde_json::to_string_pretty(&json!({
            "kind": "browser_browse",
            "action": action,
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "route_reason": route_reason_for_plan(Some(&verification_plan)).as_str(),
            "verification_preview": verification_preview,
            "verification_followup": verification_followup,
            "orchestration_decision": orchestration_decision,
            "result": result,
        }))?)
    }

    fn page_classification_payload(
        page_kind: BrowserPageKind,
        target_url: &str,
        tree: &str,
    ) -> Value {
        json!({
            "page_kind": page_kind,
            "blocking": page_kind.is_blocking(),
            "url": target_url,
            "content_chars": tree.chars().count(),
        })
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> String {
        "browser_browse".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        let provider_summary = resolve_browser_runtime()
            .map(|runtime| {
                provider::BrowserProviderDescriptor::from_runtime_with_interactive_cdp(
                    &runtime,
                    Self::direct_devtools_session_available(&runtime),
                )
            })
            .map(|provider| provider.diagnostic_summary())
            .unwrap_or_else(|| {
                "provider=unavailable origin=none path=none semantic_layer=browser_browse"
                    .to_string()
            });
        ToolDefinition {
            name: "browser_browse".to_string(),
            description: format!("Advanced browser automation tool. Supports navigation, page lifecycle waits, structured DOM snapshots, readonly evaluation, link extraction, clicking, filling forms, and stateful sessions with diffing. Uses Deterministic Refs (@eN) for reliable interaction. 'screenshot' captures visual state. Provider protocol: {provider_summary}."),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "navigate", "click", "fill", "snapshot", "extract_links", "evaluate", "hover", "scroll", "save_session", "load_session", "diff", "screenshot", "visual_analyze"],
                        "description": "The action to perform"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL for 'navigate' action"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector or Ref (e.g., @e1) for interaction"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text for 'fill', the search query for 'search', or the session key for 'save/load'"
                    },
                    "engine": {
                        "type": "string",
                        "enum": ["auto", "google", "bing"],
                        "description": "For 'search': search engine preference. 'auto' tries Google first, then Bing."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "For 'search': maximum number of results to return",
                        "default": 5
                    },
                    "wait_ms": {
                        "type": "integer",
                        "description": "Wait time after action (ms)",
                        "default": 1000
                    },
                    "wait_until": {
                        "type": "string",
                        "enum": ["domcontentloaded", "load", "networkidle"],
                        "description": "Page lifecycle condition for navigation/snapshot extraction",
                        "default": "load"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["semantic", "text", "links", "html", "markdown"],
                        "description": "Snapshot/extraction output format",
                        "default": "semantic"
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional logical task/session handle for traceability; browser provider may bind a page to it when supported"
                    },
                    "interactive_only": {
                        "type": "boolean",
                        "description": "For 'snapshot': Only include interactive elements",
                        "default": true
                    },
                    "compact": {
                        "type": "boolean",
                        "description": "For 'snapshot': Filter out structural nodes without text",
                        "default": true
                    },
                    "som": {
                        "type": "boolean",
                        "description": "For 'screenshot': Overlay visual UID tags (Set-of-Mark) on the image",
                        "default": false
                    },
                    "structured": {
                        "type": "boolean",
                        "description": "When true, return a structured payload with verification preview instead of legacy plain text."
                    },
                    "readonly": {
                        "type": "boolean",
                        "description": "For 'evaluate': must remain true; evaluate is restricted to read-only DOM extraction",
                        "default": true
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: Some("interface BrowserActionArgs {\n  action: 'search' | 'navigate' | 'click' | 'fill' | 'snapshot' | 'extract_links' | 'evaluate' | 'hover' | 'scroll' | 'save_session' | 'load_session' | 'diff' | 'screenshot' | 'visual_analyze';\n  url?: string;\n  selector?: string;\n  text?: string; // fill text, search query, session key, or readonly evaluate expression\n  engine?: 'auto' | 'google' | 'bing';\n  max_results?: number;\n  wait_ms?: number;\n  wait_until?: 'domcontentloaded' | 'load' | 'networkidle';\n  format?: 'semantic' | 'text' | 'links' | 'html' | 'markdown';\n  session_id?: string;\n  interactive_only?: boolean;\n  compact?: boolean;\n  som?: boolean;\n  structured?: boolean;\n  readonly?: boolean;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use 'search' for zero-config browser-based web search. Use 'navigate' with wait_until before inspecting dynamic pages. Use 'snapshot' with format=semantic/text/links/html/markdown to observe source content. Use 'extract_links' before choosing follow-up URLs. Use 'evaluate' only for read-only DOM extraction. Use 'diff' to see changes and 'screenshot' for visual verification.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            url: Option<String>,
            selector: Option<String>,
            text: Option<String>,
            engine: Option<String>,
            max_results: Option<usize>,
            wait_ms: Option<u64>,
            wait_until: Option<String>,
            format: Option<String>,
            session_id: Option<String>,
            interactive_only: Option<bool>,
            compact: Option<bool>,
            som: Option<bool>,
            structured: Option<bool>,
            readonly: Option<bool>,
            #[serde(rename = "_benshu_task_context")]
            task_context: Option<String>,
        }

        let mut normalized_arguments: Value = serde_json::from_str(arguments)?;
        if let Some(object) = normalized_arguments.as_object_mut() {
            let task_context_text = object
                .get("_benshu_task_context")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            let fallback_text_from_task_context = || {
                task_context_text
                    .as_deref()
                    .map(|context| Self::search_query_with_task_context("", Some(context)))
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            };
            let has_action = object
                .get("action")
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.trim().is_empty());
            if has_action
                && object
                    .get("action")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value.trim().eq_ignore_ascii_case("search"))
            {
                let has_text = object
                    .get("text")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| !value.trim().is_empty());
                if !has_text {
                    let fallback_text = object
                        .get("query")
                        .and_then(|value| value.as_str())
                        .or_else(|| object.get("task").and_then(|value| value.as_str()))
                        .or_else(|| object.get("prompt").and_then(|value| value.as_str()))
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                        .or_else(fallback_text_from_task_context);
                    if let Some(fallback_text) = fallback_text {
                        object.insert("text".to_string(), Value::String(fallback_text));
                    }
                }
            }
            if !has_action {
                let has_url = object
                    .get("url")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| !value.trim().is_empty());
                let query_text = object
                    .get("query")
                    .and_then(|value| value.as_str())
                    .or_else(|| object.get("text").and_then(|value| value.as_str()))
                    .or_else(|| object.get("task").and_then(|value| value.as_str()))
                    .or_else(|| object.get("prompt").and_then(|value| value.as_str()))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                let query_text = query_text.or_else(fallback_text_from_task_context);

                if has_url {
                    object.insert("action".to_string(), Value::String("navigate".to_string()));
                } else if let Some(query_text) = query_text {
                    if let Some(url) = Self::first_url_like(&query_text) {
                        object.insert("action".to_string(), Value::String("navigate".to_string()));
                        object
                            .entry("url".to_string())
                            .or_insert_with(|| Value::String(url));
                    } else {
                        object.insert("action".to_string(), Value::String("search".to_string()));
                        object
                            .entry("text".to_string())
                            .or_insert_with(|| Value::String(query_text));
                    }
                } else if let Some(value) = object
                    .values()
                    .find_map(|value| value.as_str().map(str::to_string))
                {
                    let value = value.trim().to_string();
                    if let Some(url) = Self::first_url_like(&value) {
                        object.insert("action".to_string(), Value::String("navigate".to_string()));
                        object
                            .entry("url".to_string())
                            .or_insert_with(|| Value::String(url));
                    } else if !value.is_empty() {
                        object.insert("action".to_string(), Value::String("search".to_string()));
                        object
                            .entry("text".to_string())
                            .or_insert_with(|| Value::String(value));
                    }
                }
            }
        }

        let args: Args = serde_json::from_value(normalized_arguments)?;
        let structured = args.structured.unwrap_or(false);
        let wait_until = BrowserWaitUntil::parse(args.wait_until.as_deref())?;
        let format = BrowserSnapshotFormat::parse(args.format.as_deref())?;
        let wait_time = tokio::time::Duration::from_millis(args.wait_ms.unwrap_or(1000));
        let mut base_trace = vec![format!("action {}", args.action)];
        if let Some(session_id) = args
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            base_trace.push(format!("session {}", session_id.trim()));
        }
        let current_url_for_safety = self.current_url.lock().clone();
        let safety_notes = BrowserSafetyGate::enforce(&BrowserSafetyRequest {
            action: &args.action,
            url: args.url.as_deref(),
            selector: args.selector.as_deref(),
            text: args.text.as_deref(),
            current_url: current_url_for_safety.as_deref(),
            readonly: args.readonly,
        })?;
        base_trace.extend(
            safety_notes
                .into_iter()
                .map(|note| format!("safety {note}")),
        );
        if Self::static_wsl_windows_browser_runtime().is_some() {
            match args.action.as_str() {
                "search" => {
                    let query = args
                        .text
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Search query required in 'text' field"))?;
                    let query =
                        Self::search_query_with_task_context(query, args.task_context.as_deref());
                    let (engine_name, search_url, results) =
                        Self::search_once_with_direct_site_fallback(
                            &query,
                            args.task_context.as_deref(),
                            args.engine.as_deref(),
                            args.max_results.unwrap_or(5).max(1),
                        )
                        .await?;
                    return if structured {
                        Self::render_structured_search_result(
                            &query,
                            &engine_name,
                            &search_url,
                            &results,
                        )
                    } else {
                        Ok(Self::render_plain_search_result(
                            &query,
                            &engine_name,
                            &results,
                        ))
                    };
                }
                "snapshot" | "extract_links" => {
                    let _interactive = args.interactive_only.unwrap_or(true);
                    let compact = args.compact.unwrap_or(true);
                    let target_url = args
                        .url
                        .clone()
                        .or_else(|| self.current_url.lock().clone())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Snapshot requires a URL or a previous static navigate action"
                            )
                        })?;
                    let action_format = if args.action == "extract_links" {
                        BrowserSnapshotFormat::Links
                    } else {
                        format
                    };
                    let mut trace = base_trace.clone();
                    trace.push(format!("navigate {}", target_url));
                    let payload = match Self::observe_once_with_windows_cdp_bridge(
                        &target_url,
                        wait_until,
                        wait_time,
                    )
                    .await
                    {
                        Ok(observation) => Self::render_windows_cdp_bridge_observation_payload(
                            &args.action,
                            &observation,
                            action_format,
                            wait_until,
                            compact,
                            trace,
                        ),
                        Err(bridge_error) => {
                            let html = Self::static_html_once_with_windows_browser(&target_url)
                                .await
                                .map_err(|static_error| {
                                    anyhow::anyhow!(
                                        "Windows browser observation failed. cdp_bridge_error={}; static_error={}",
                                        bridge_error,
                                        static_error
                                    )
                                })?;
                            trace.push(format!(
                                "cdp_bridge_fallback {}",
                                bridge_error
                                    .to_string()
                                    .chars()
                                    .take(240)
                                    .collect::<String>()
                            ));
                            trace.push(format!("wait {}", wait_until.as_str()));
                            trace.push(format!("read_dom {}", action_format.as_str()));
                            Self::render_static_observation_payload(
                                &args.action,
                                &target_url,
                                &html,
                                action_format,
                                wait_until,
                                compact,
                                trace,
                            )
                        }
                    };
                    if let Some(content) = payload.get("content").and_then(|value| value.as_str()) {
                        *self.last_snapshot.lock() = Some(content.to_string());
                    }
                    let note = payload["page"]["page_kind"]
                        .as_str()
                        .unwrap_or("browser page content observed")
                        .to_string();
                    return if structured {
                        Self::render_structured_result(
                            &args.action,
                            VerificationDomain::KnowledgeFact,
                            &note,
                            vec![VerificationSource {
                                kind: "browser_snapshot".to_string(),
                                title: "Current browser DOM snapshot".to_string(),
                                uri: target_url.clone(),
                                observed_at: Some(chrono::Utc::now().to_rfc3339()),
                            }],
                            payload,
                        )
                    } else {
                        Ok(format!(
                            "Snapshot ({}):\n\n{}",
                            action_format.as_str(),
                            payload["content"].as_str().unwrap_or_default()
                        ))
                    };
                }
                "evaluate" => {
                    let expression = args.text.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("Readonly evaluate expression required in 'text' field")
                    })?;
                    Self::validate_readonly_evaluate_expression(expression)?;
                    return Err(anyhow::anyhow!(
                        "Readonly evaluate is not exposed through the WSL bridge. Use snapshot/extract_links with format=text/html/markdown/links for DOM extraction."
                    ));
                }
                "navigate" => {
                    let url = args
                        .url
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("URL required"))?;
                    *self.current_url.lock() = Some(url.clone());
                    let payload = match Self::observe_once_with_windows_cdp_bridge(
                        &url, wait_until, wait_time,
                    )
                    .await
                    {
                        Ok(observation) => {
                            Some(Self::render_windows_cdp_bridge_observation_payload(
                                "navigate",
                                &observation,
                                format,
                                wait_until,
                                args.compact.unwrap_or(true),
                                {
                                    let mut trace = base_trace.clone();
                                    trace.push(format!("navigate {}", url));
                                    trace
                                },
                            ))
                        }
                        Err(_) => None,
                    };
                    return if structured {
                        if let Some(payload) = payload {
                            let note = payload["page"]["page_kind"]
                                .as_str()
                                .unwrap_or("browser page content observed")
                                .to_string();
                            Self::render_structured_result(
                                "navigate",
                                VerificationDomain::KnowledgeFact,
                                &note,
                                vec![VerificationSource {
                                    kind: "browser_page".to_string(),
                                    title: "Navigated URL".to_string(),
                                    uri: url.clone(),
                                    observed_at: Some(chrono::Utc::now().to_rfc3339()),
                                }],
                                payload,
                            )
                        } else {
                            let page_kind = BrowserPageKind::Normal;
                            Self::render_structured_result(
                                "navigate",
                                VerificationDomain::KnowledgeFact,
                                page_kind.note(),
                                vec![VerificationSource {
                                    kind: "browser_page".to_string(),
                                    title: "Navigated URL".to_string(),
                                    uri: url.clone(),
                                    observed_at: Some(chrono::Utc::now().to_rfc3339()),
                                }],
                                json!({
                                    "provider": Self::provider_descriptor_payload(),
                                    "session": {
                                        "mode": "one_shot_static",
                                        "page_session_pool": false,
                                        "task_bound_handle": null
                                    },
                                    "cdp_boundary": {
                                        "page": { "wait_until": wait_until.as_str(), "final_url": url },
                                        "runtime": { "readonly_evaluate": false },
                                        "dom": { "structured_read": true },
                                        "network": Self::network_summary_payload("wsl_bridge_dump_dom", &url, vec!["navigation_recorded_without_content_snapshot".to_string()]),
                                        "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                                        "input": { "interactive_actions": false }
                                    },
                                    "action_trace": base_trace,
                                    "message": format!("Navigated to {}", url),
                                    "url": url,
                                    "page": Self::page_classification_payload(page_kind, &url, ""),
                                }),
                            )
                        }
                    } else {
                        Ok(format!("Navigated to {}", url))
                    };
                }
                "diff" => {
                    let interactive = args.interactive_only.unwrap_or(true);
                    let compact = args.compact.unwrap_or(true);
                    let target_url = args
                        .url
                        .clone()
                        .or_else(|| self.current_url.lock().clone())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Diff requires a URL or a previous static navigate action"
                            )
                        })?;
                    let current = Self::snapshot_once(&target_url, interactive, compact).await?;
                    let page_kind = Self::classify_page_content(&current);
                    let last_opt = self.last_snapshot.lock().clone();
                    *self.last_snapshot.lock() = Some(current.clone());
                    let message = if let Some(last) = last_opt {
                        if last == current {
                            "No changes detected since last snapshot.".to_string()
                        } else {
                            format!("Snapshot changed.\n\n{}", current)
                        }
                    } else {
                        format!(
                            "No previous snapshot found. Captured initial snapshot:\n\n{current}"
                        )
                    };
                    return if structured {
                        Self::render_structured_result(
                            "diff",
                            VerificationDomain::StateFact,
                            page_kind.note(),
                            vec![VerificationSource {
                                kind: "browser_diff".to_string(),
                                title: "Browser static snapshot diff".to_string(),
                                uri: target_url.clone(),
                                observed_at: Some(chrono::Utc::now().to_rfc3339()),
                            }],
                            json!({
                                "message": message,
                                "page": Self::page_classification_payload(page_kind, &target_url, &current),
                            }),
                        )
                    } else {
                        Ok(message)
                    };
                }
                "save_session" | "load_session" => {
                    return Ok(format!(
                        "{} is a no-op in WSL static browser mode; Edge/Chrome sessions are isolated per request.",
                        args.action
                    ));
                }
                other => {
                    return Err(anyhow::anyhow!(
                        "Browser action '{}' requires an interactive DevTools session, which is disabled in WSL static browser mode to avoid Edge/Chrome hangs. Use search, navigate, snapshot, extract_links, or diff for research tasks.",
                        other
                    ));
                }
            }
        }
        let browser = self.get_browser()?;

        let tabs_arc = browser.get_tabs();
        let tab = {
            let tabs = tabs_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
            if tabs.is_empty() {
                browser
                    .new_tab()
                    .map_err(|e| anyhow::anyhow!("Failed to open tab: {}", e))?
            } else {
                tabs[0].clone()
            }
        };

        match args.action.as_str() {
            "search" => {
                let query = args
                    .text
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Search query required in 'text' field"))?;
                let query =
                    Self::search_query_with_task_context(query, args.task_context.as_deref());
                self.perform_browser_search(
                    tab,
                    &query,
                    args.engine.as_deref(),
                    wait_time,
                    args.max_results.unwrap_or(5).max(1),
                    structured,
                )
                .await
            }
            "navigate" => {
                let url = args.url.ok_or_else(|| anyhow::anyhow!("URL required"))?;
                tab.navigate_to(&url)?.wait_until_navigated()?;
                Self::wait_for_page_lifecycle(&tab, wait_until, wait_time).await?;
                let mut trace = base_trace.clone();
                trace.push(format!("navigate {}", url));
                trace.push(format!("wait {}", wait_until.as_str()));
                if structured {
                    Self::render_structured_result(
                        "navigate",
                        VerificationDomain::KnowledgeFact,
                        "browser navigation completed",
                        vec![VerificationSource {
                            kind: "browser_page".to_string(),
                            title: "Navigated URL".to_string(),
                            uri: url.clone(),
                            observed_at: Some(chrono::Utc::now().to_rfc3339()),
                        }],
                        json!({
                            "provider": Self::provider_descriptor_payload(),
                            "session": {
                                "mode": "interactive_devtools",
                                "page_session_pool": true,
                                "task_bound_handle": "browser://current-tab"
                            },
                            "cdp_boundary": {
                                "page": { "wait_until": wait_until.as_str(), "final_url": tab.get_url(), "title": tab.get_title().ok() },
                                "runtime": { "readonly_evaluate": true },
                                "dom": { "structured_read": true },
                                "network": Self::network_summary_payload("edge_chrome_devtools", &tab.get_url(), vec!["full_network_event_ledger_not_enabled_for_current_headless_chrome_backend".to_string()]),
                                "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                                "input": { "interactive_actions": true }
                            },
                            "action_trace": trace,
                            "message": format!("Navigated to {}", url),
                            "url": url
                        }),
                    )
                } else {
                    Ok(format!("Navigated to {}", url))
                }
            }
            "snapshot" => {
                let interactive = args.interactive_only.unwrap_or(true);
                let compact = args.compact.unwrap_or(true);
                let mut trace = base_trace.clone();
                trace.push(format!("read_dom {}", format.as_str()));
                let payload = self
                    .render_interactive_observation_payload(
                        tab,
                        "snapshot",
                        format,
                        wait_until,
                        wait_time,
                        interactive,
                        compact,
                        trace,
                    )
                    .await?;
                if structured {
                    Self::render_structured_result(
                        "snapshot",
                        VerificationDomain::KnowledgeFact,
                        "browser semantic snapshot captured",
                        vec![VerificationSource {
                            kind: "browser_snapshot".to_string(),
                            title: "Current browser semantic snapshot".to_string(),
                            uri: "browser://current-tab".to_string(),
                            observed_at: Some(chrono::Utc::now().to_rfc3339()),
                        }],
                        payload,
                    )
                } else {
                    Ok(format!(
                        "Snapshot ({}):\n\n{}",
                        format.as_str(),
                        payload["content"].as_str().unwrap_or_default()
                    ))
                }
            }
            "extract_links" => {
                let mut trace = base_trace.clone();
                trace.push("extract_links".to_string());
                let payload = self
                    .render_interactive_observation_payload(
                        tab,
                        "extract_links",
                        BrowserSnapshotFormat::Links,
                        wait_until,
                        wait_time,
                        false,
                        true,
                        trace,
                    )
                    .await?;
                if structured {
                    Self::render_structured_result(
                        "extract_links",
                        VerificationDomain::KnowledgeFact,
                        "browser links extracted from source DOM",
                        vec![VerificationSource {
                            kind: "browser_links".to_string(),
                            title: "Current browser DOM links".to_string(),
                            uri: "browser://current-tab#links".to_string(),
                            observed_at: Some(chrono::Utc::now().to_rfc3339()),
                        }],
                        payload,
                    )
                } else {
                    Ok(format!(
                        "Links:\n\n{}",
                        payload["content"].as_str().unwrap_or_default()
                    ))
                }
            }
            "evaluate" => {
                if args.readonly == Some(false) {
                    anyhow::bail!("browser_browse evaluate only supports readonly=true");
                }
                let expression = args.text.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Readonly evaluate expression required in 'text' field")
                })?;
                Self::validate_readonly_evaluate_expression(expression)?;
                let value = tab
                    .evaluate(expression, false)
                    .map_err(|e| anyhow::anyhow!("Readonly evaluate failed: {}", e))?
                    .value
                    .unwrap_or(Value::Null);
                let rendered = if value.is_string() {
                    value.as_str().unwrap_or_default().to_string()
                } else {
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                };
                let output = Self::truncate_chars(&rendered, 18_000);
                let trace = {
                    let mut trace = base_trace.clone();
                    trace.push("runtime.evaluate readonly".to_string());
                    trace
                };
                if structured {
                    Self::render_structured_result(
                        "evaluate",
                        VerificationDomain::KnowledgeFact,
                        "browser readonly DOM evaluation completed",
                        vec![VerificationSource {
                            kind: "browser_runtime_evaluate".to_string(),
                            title: "Current browser Runtime.evaluate result".to_string(),
                            uri: "browser://current-tab#runtime.evaluate".to_string(),
                            observed_at: Some(chrono::Utc::now().to_rfc3339()),
                        }],
                        json!({
                            "provider": Self::provider_descriptor_payload(),
                            "session": {
                                "mode": "interactive_devtools",
                                "page_session_pool": true,
                                "task_bound_handle": "browser://current-tab"
                            },
                            "cdp_boundary": {
                                "page": { "wait_until": wait_until.as_str(), "final_url": tab.get_url(), "title": tab.get_title().ok() },
                                "runtime": { "readonly_evaluate": true },
                                "dom": { "structured_read": true },
                                "network": Self::network_summary_payload("edge_chrome_devtools", &tab.get_url(), vec!["evaluate_did_not_initiate_network_by_policy".to_string()]),
                                "fetch": { "request_interception_policy": "site_policy_and_ssrf_guard" },
                                "input": { "interactive_actions": true }
                            },
                            "action_trace": trace,
                            "result": value,
                            "content": output,
                            "content_chars": rendered.chars().count(),
                            "blockers": []
                        }),
                    )
                } else {
                    Ok(output)
                }
            }
            "diff" => {
                let interactive = args.interactive_only.unwrap_or(true);
                let compact = args.compact.unwrap_or(true);
                let current = self
                    .extract_aria_tree_with_refs(tab, interactive, compact)
                    .await?;
                let last_opt = self.last_snapshot.lock().clone();

                if let Some(last) = last_opt {
                    if last == current.tree {
                        Ok("No changes detected since last snapshot.".to_string())
                    } else {
                        // Improved line-based diff showing additions and removals
                        let last_lines: Vec<&str> = last.lines().collect();
                        let current_lines: Vec<&str> = current.tree.lines().collect();
                        let last_set: std::collections::HashSet<&str> =
                            last_lines.iter().cloned().collect();
                        let current_set: std::collections::HashSet<&str> =
                            current_lines.iter().cloned().collect();

                        let mut diff = String::new();
                        diff.push_str("### Snapshot Diff:\n");

                        // Show removals
                        for line in last_lines {
                            if !current_set.contains(line) {
                                diff.push_str(&format!("- {}\n", line));
                            }
                        }

                        // Show additions
                        for line in current_lines {
                            if !last_set.contains(line) {
                                diff.push_str(&format!("+ {}\n", line));
                            }
                        }

                        *self.last_snapshot.lock() = Some(current.tree);
                        if structured {
                            Self::render_structured_result(
                                "diff",
                                VerificationDomain::StateFact,
                                "browser snapshot diff completed",
                                vec![VerificationSource {
                                    kind: "browser_diff".to_string(),
                                    title: "Browser snapshot diff".to_string(),
                                    uri: "browser://current-tab#diff".to_string(),
                                    observed_at: Some(chrono::Utc::now().to_rfc3339()),
                                }],
                                json!({ "diff": diff }),
                            )
                        } else {
                            Ok(diff)
                        }
                    }
                } else {
                    *self.last_snapshot.lock() = Some(current.tree.clone());
                    let message = format!(
                        "No previous snapshot found. Captured initial snapshot:\n\n{}",
                        current.tree
                    );
                    if structured {
                        Self::render_structured_result(
                            "diff",
                            VerificationDomain::StateFact,
                            "initial browser snapshot captured because no prior diff baseline existed",
                            vec![VerificationSource {
                                kind: "browser_snapshot".to_string(),
                                title: "Initial browser snapshot".to_string(),
                                uri: "browser://current-tab#initial-snapshot".to_string(),
                                observed_at: Some(chrono::Utc::now().to_rfc3339()),
                            }],
                            json!({ "message": message, "tree": current.tree }),
                        )
                    } else {
                        Ok(message)
                    }
                }
            }
            "screenshot" => {
                let som_enabled = args.som.unwrap_or(false);

                if som_enabled {
                    // 1. Ensure refs are present in DOM (run snapshot logic if needed)
                    // We don't need the string, just the tags in DOM
                    let _ = self
                        .extract_aria_tree_with_refs(tab.clone(), true, true)
                        .await?;

                    // 2. Inject SOM overlay
                    let inject_som = r#"
                        (function() {
                            const somContainerId = 'benshu-som-overlay';
                            let container = document.getElementById(somContainerId);
                            if (container) container.remove();
                            
                            container = document.createElement('div');
                            container.id = somContainerId;
                            container.style.position = 'absolute';
                            container.style.top = '0';
                            container.style.left = '0';
                            container.style.width = '100%';
                            container.style.height = '100%';
                            container.style.pointerEvents = 'none';
                            container.style.zIndex = '2147483647';
                            document.body.appendChild(container);

                            const elements = document.querySelectorAll('[data-benshu-ref]');
                            elements.forEach(el => {
                                const rect = el.getBoundingClientRect();
                                if (rect.width > 0 && rect.height > 0) {
                                    const ref = el.getAttribute('data-benshu-ref');
                                    const tag = document.createElement('div');
                                    tag.textContent = ref;
                                    tag.style.position = 'absolute';
                                    tag.style.left = (rect.left + window.scrollX) + 'px';
                                    tag.style.top = (rect.top + window.scrollY) - 15 + 'px'; // Offset a bit up
                                    tag.style.background = 'rgba(0, 0, 0, 0.85)';
                                    tag.style.color = '#00ff00'; // Neon green for high contrast
                                    tag.style.fontSize = '12px';
                                    tag.style.padding = '1px 3px';
                                    tag.style.borderRadius = '2px';
                                    tag.style.fontFamily = 'monospace';
                                    tag.style.fontWeight = 'bold';
                                    tag.style.border = '1px solid #00ff00';
                                    tag.style.boxShadow = '0 0 5px rgba(0,255,0,0.5)';
                                    container.appendChild(tag);
                                }
                            });
                        })()
                    "#;
                    tab.evaluate(inject_som, false)?;
                }

                let png_data = tab
                    .capture_screenshot(
                        headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                        None,
                        None,
                        true,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to capture screenshot: {}", e))?;

                if som_enabled {
                    // 3. Clean up SOM overlay
                    let _ = tab.evaluate(
                        "document.getElementById('benshu-som-overlay')?.remove()",
                        false,
                    );
                }

                // For now, return as base64 or save to a known location
                let b64 = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, &png_data);
                let message = format!(
                    "Screenshot captured successfully (base64: {}...)",
                    &b64[..50]
                );
                if structured {
                    Self::render_structured_result(
                        "screenshot",
                        VerificationDomain::KnowledgeFact,
                        "browser screenshot captured",
                        vec![VerificationSource {
                            kind: "browser_screenshot".to_string(),
                            title: "Browser screenshot".to_string(),
                            uri: "browser://current-tab#screenshot".to_string(),
                            observed_at: Some(chrono::Utc::now().to_rfc3339()),
                        }],
                        json!({ "message": message, "som": som_enabled }),
                    )
                } else {
                    Ok(message)
                }
            }
            "click" => {
                let sel = args
                    .selector
                    .ok_or_else(|| anyhow::anyhow!("Selector required"))?;
                tab.wait_for_element(&self.resolve_selector(&sel))?
                    .click()?;
                tokio::time::sleep(wait_time).await;
                Ok(format!("Clicked {}", sel))
            }
            "fill" => {
                let sel = args
                    .selector
                    .ok_or_else(|| anyhow::anyhow!("Selector required"))?;
                let text = args.text.ok_or_else(|| anyhow::anyhow!("Text required"))?;
                let el = tab.wait_for_element(&self.resolve_selector(&sel))?;
                el.click().ok();
                el.type_into(&text)?;
                tokio::time::sleep(wait_time).await;
                Ok(format!("Filled '{}' into '{}'", text, sel))
            }
            "hover" => {
                let sel = args
                    .selector
                    .ok_or_else(|| anyhow::anyhow!("Selector required"))?;
                let el = tab.wait_for_element(&self.resolve_selector(&sel))?;
                let midpoint = el
                    .get_midpoint()
                    .map_err(|e| anyhow::anyhow!("Failed to get element midpoint: {}", e))?;
                tab.move_mouse_to_point(midpoint)
                    .map_err(|e| anyhow::anyhow!("Failed to move mouse: {}", e))?;
                tokio::time::sleep(wait_time).await;
                Ok(format!("Hovered {}", sel))
            }
            "scroll" => {
                if let Some(sel) = args.selector {
                    tab.wait_for_element(&self.resolve_selector(&sel))?
                        .scroll_into_view()?;
                } else {
                    tab.evaluate("window.scrollBy(0, 500)", false)?;
                }
                tokio::time::sleep(wait_time).await;
                Ok("Scrolled".to_string())
            }
            "save_session" => {
                let key = args
                    .text
                    .unwrap_or_else(|| self.session.session_name.clone());
                let vault = self
                    .vault
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("SecretVault not configured"))?;

                let cookies = tab
                    .get_cookies()
                    .map_err(|e| anyhow::anyhow!("Failed to get cookies: {}", e))?;
                let serialized = serde_json::to_string(&cookies)?;
                vault.set(&format!("browser_session_{}", key), &serialized)?;

                Ok(format!(
                    "Successfully saved {} cookies to local Vault under key '{}'",
                    cookies.len(),
                    key
                ))
            }
            "load_session" => {
                let key = args
                    .text
                    .unwrap_or_else(|| self.session.session_name.clone());
                let vault = self
                    .vault
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("SecretVault not configured"))?;

                if let Some(data) = vault.get(&format!("browser_session_{}", key))? {
                    // Use tab.call_method with correct protocol path
                    tab.call_method(headless_chrome::protocol::cdp::Network::SetCookies {
                        cookies: serde_json::from_str::<serde_json::Value>(&data)?
                            .as_array()
                            .ok_or_else(|| anyhow::anyhow!("Invalid cookie data"))?
                            .iter()
                            .map(|v| serde_json::from_value(v.clone()).unwrap())
                            .collect(),
                    })
                    .map_err(|e| anyhow::anyhow!("Failed to set cookies: {}", e))?;
                    Ok(format!(
                        "Successfully loaded session '{}' from local Vault.",
                        key
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "Session '{}' not found in local Vault",
                        key
                    ))
                }
            }
            "visual_analyze" => {
                let prompt = args
                    .text
                    .ok_or_else(|| anyhow::anyhow!("Analysis prompt required in 'text' field"))?;
                let som = args.som.unwrap_or(true);

                let png_data = self.screenshot_binary(som).await?;
                let img = image::load_from_memory(&png_data)?;

                let output = self.sensory.vision_check(img, Some(&prompt), None).await?;
                match output {
                    benshu_sensory::SensoryOutput::Text(t) => Ok(t),
                    benshu_sensory::SensoryOutput::Coordinates { x, y, label } => {
                        if let Ok(ref_id) = self.resolve_ref_from_coords(x, y).await {
                            Ok(format!(
                                "Target identified at {} ([{}, {}]) - {:?}",
                                ref_id, x, y, label
                            ))
                        } else {
                            Ok(format!(
                                "Coordinates identified: [{}, {}] - {:?}",
                                x, y, label
                            ))
                        }
                    }
                    _ => Ok(format!("Analysis complete: {:?}", output)),
                }
            }
            _ => Err(anyhow::anyhow!("Unknown action: {}", args.action)),
        }
    }
}

impl Drop for BrowserTool {
    fn drop(&mut self) {
        self.close_browser_process_tree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_browser_dump_dom_defaults_are_chat_fast() {
        assert_eq!(DEFAULT_WINDOWS_BROWSER_DUMP_DOM_TIMEOUT_SECS, 30);
        assert_eq!(DEFAULT_WINDOWS_BROWSER_VIRTUAL_TIME_BUDGET_MS, 5_000);
        assert_eq!(DEFAULT_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS, 12);
    }

    #[test]
    fn windows_browser_search_cdp_timeout_is_shorter_than_full_dom_dump() {
        std::env::remove_var("BENSHU_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS");
        assert!(
            BrowserTool::windows_browser_search_cdp_timeout()
                < BrowserTool::windows_browser_dump_dom_timeout()
        );

        std::env::set_var("BENSHU_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS", "120");
        assert_eq!(
            BrowserTool::windows_browser_search_cdp_timeout(),
            Duration::from_secs(60)
        );
        std::env::remove_var("BENSHU_WINDOWS_BROWSER_SEARCH_CDP_TIMEOUT_SECS");
    }

    #[test]
    fn windows_browser_static_fallback_only_handles_browser_dump_failures() {
        let timeout_error = anyhow::anyhow!("Windows browser dump-dom timed out after 30s");
        assert!(BrowserTool::should_try_static_http_fallback(&timeout_error));

        let page_error = anyhow::anyhow!("HTTP 403: Forbidden");
        assert!(!BrowserTool::should_try_static_http_fallback(&page_error));
    }

    #[test]
    fn windows_browser_cleanup_uses_full_managed_profile_needles() {
        let needles = BrowserTool::windows_cleanup_profile_needles(Path::new(
            "/mnt/c/Users/test/AppData/Local/BenShu/browser-cdp/session-12345678",
        ));

        assert!(needles
            .iter()
            .any(|needle| needle.contains("BenShu/browser-cdp/session-12345678")));
        assert!(!needles.iter().any(|needle| needle == "session-12345678"));
    }

    #[test]
    fn cdp_websocket_url_parser_reads_json_version_response() {
        let response = concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n",
            r#"{"Browser":"Chrome","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser/abc"}"#
        );

        assert_eq!(
            BrowserTool::parse_cdp_websocket_url_from_http_response(response),
            Some("ws://127.0.0.1:9222/devtools/browser/abc".to_string())
        );
    }

    #[test]
    fn windows_interactive_cdp_policy_defaults_to_enabled_and_can_be_disabled() {
        std::env::remove_var("BENSHU_WINDOWS_BROWSER_INTERACTIVE_CDP");
        assert!(BrowserTool::windows_interactive_cdp_enabled());

        std::env::set_var("BENSHU_WINDOWS_BROWSER_INTERACTIVE_CDP", "off");
        assert!(!BrowserTool::windows_interactive_cdp_enabled());
        std::env::remove_var("BENSHU_WINDOWS_BROWSER_INTERACTIVE_CDP");
    }

    #[test]
    #[ignore = "requires a real local Edge/Chrome DevTools runtime"]
    fn direct_interactive_cdp_provider_smoke() {
        if BrowserTool::static_wsl_windows_browser_runtime().is_none() {
            return;
        }
        eprintln!(
            "Skipping direct CDP smoke under WSL: Windows Edge/Chrome binds DevTools to Windows localhost. Use windows_cdp_bridge_observation_smoke instead."
        );
    }

    #[tokio::test]
    #[ignore = "requires a real Windows Edge/Chrome runtime through the WSL bridge"]
    async fn windows_cdp_bridge_observation_smoke() {
        if BrowserTool::static_wsl_windows_browser_runtime().is_none() {
            return;
        }
        let observation = BrowserTool::observe_once_with_windows_cdp_bridge(
            "https://example.com/",
            BrowserWaitUntil::Load,
            tokio::time::Duration::from_millis(250),
        )
        .await
        .expect("Windows CDP bridge should observe a real page");
        assert!(observation.final_url.starts_with("https://example.com"));
        assert!(observation.text.contains("Example Domain"));
    }

    #[test]
    #[ignore = "requires a same-host Edge/Chrome DevTools runtime"]
    fn same_host_interactive_cdp_provider_smoke() {
        if BrowserTool::static_wsl_windows_browser_runtime().is_some() {
            return;
        }
        let tool = BrowserTool::new(
            None,
            None,
            Arc::new(benshu_sensory::SensoryHub::new(
                benshu_sensory::SensoryConfig::default(),
            )),
        );

        let browser = tool
            .get_browser()
            .expect("Windows interactive CDP provider should launch");
        assert!(browser.new_tab().is_ok());
        tool.close_browser_for_one_shot();
    }

    #[test]
    fn browser_static_payload_extracts_embedded_json_records() {
        let html = r#"
            <html>
              <head><title>Free list</title></head>
              <body>
                <script id="page-state" type="application/json">
                {"records":[{"bName":"碧阳仙门","bAuth":"鹤守月满池","cat":"玄幻","bPrice":0,"cnt":"3.72万字","desc":"仙门称碧阳。","bid":1048992740}]}
                </script>
              </body>
            </html>
        "#;

        let payload = BrowserTool::render_static_observation_payload(
            "snapshot",
            "https://m.example.com/free/",
            html,
            BrowserSnapshotFormat::Text,
            BrowserWaitUntil::DomContentLoaded,
            true,
            Vec::new(),
        );

        let records = payload["records"].as_array().expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["title"], "碧阳仙门");
        assert_eq!(records[0]["url"], "https://m.example.com/book/1048992740/");
        assert!(records[0]["metadata"]
            .as_str()
            .unwrap()
            .contains("category: 玄幻"));
    }

    #[test]
    fn browser_structured_result_contains_verification_preview() {
        let rendered = BrowserTool::render_structured_result(
            "snapshot",
            VerificationDomain::KnowledgeFact,
            "browser semantic snapshot captured",
            vec![VerificationSource {
                kind: "browser_snapshot".to_string(),
                title: "Current browser semantic snapshot".to_string(),
                uri: "browser://current-tab".to_string(),
                observed_at: Some("2026-03-29T00:00:00Z".to_string()),
            }],
            json!({ "tree": "[button] \"Go\" [ref=@e1]" }),
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(payload["kind"], "browser_browse");
        assert_eq!(payload["action"], "snapshot");
        assert_eq!(payload["verification_preview"]["mode"], "BrowserValidation");
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "source_content_observed"
        );
        assert_eq!(payload["verification_followup"]["cite_required"], true);
        assert_eq!(
            payload["route_reason"],
            "browser_observation_must_read_source_content"
        );
        assert_eq!(
            payload["orchestration_decision"]["termination"],
            "FinalizeWithSources"
        );
    }

    #[test]
    fn browser_navigate_does_not_claim_source_content_observed() {
        let rendered = BrowserTool::render_structured_result(
            "navigate",
            VerificationDomain::KnowledgeFact,
            "browser navigation completed",
            vec![VerificationSource {
                kind: "browser_page".to_string(),
                title: "Navigated URL".to_string(),
                uri: "https://example.com".to_string(),
                observed_at: Some("2026-03-29T00:00:00Z".to_string()),
            }],
            json!({ "message": "Navigated to https://example.com", "url": "https://example.com" }),
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "verification_pending"
        );
        assert_eq!(payload["orchestration_decision"]["termination"], "NotReady");
    }

    #[test]
    fn browser_search_reports_search_results_only_followup() {
        let rendered = BrowserTool::render_structured_search_result(
            "react 19 features",
            "google",
            "https://www.google.com/search?q=react%2019%20features&hl=en",
            &[BrowserSearchResult {
                title: "React 19 Release".to_string(),
                url: "https://react.dev/blog/react-19".to_string(),
                snippet: "Official React 19 announcement".to_string(),
                source: "google".to_string(),
                position: 1,
            }],
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(payload["kind"], "browser_browse");
        assert_eq!(payload["action"], "search");
        assert_eq!(payload["verification_preview"]["mode"], "WebSearchFetch");
        assert_eq!(
            payload["verification_followup"]["answer_readiness"],
            "search_results_only"
        );
        assert_eq!(payload["verification_followup"]["cite_required"], true);
        assert_eq!(
            payload["orchestration_decision"]["termination"],
            "TentativeOnly"
        );
    }

    #[test]
    fn browser_search_query_inherits_missing_task_context_facets() {
        let query = BrowserTool::search_query_with_task_context(
            "site:qidian.com 排行 榜单",
            Some("搜索起点玄幻小说把可以下载的免费玄幻小说下载前10部，之后放到知识库"),
        );

        assert!(query.contains("site:qidian.com"));
        assert!(query.contains("fantasy") || query.contains("玄幻"));
        assert!(query.contains("free") || query.contains("免费"));
        assert!(query.contains("download") || query.contains("下载"));
    }

    #[test]
    fn browser_search_query_uses_lookup_phase_not_downstream_writing_phase() {
        let query = BrowserTool::search_query_with_task_context(
            "",
            Some("The prior lookup needs recovery. User task: 搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库。然后基于知识库里的素材进行推理，写一部全新的玄幻小说，不能简单复制素材内容，要求情节完善、角色名字不漂移、总长度超过50万字，并保存成txt文件。"),
        );

        assert!(query.contains("玄幻") || query.contains("fantasy"));
        assert!(query.contains("下载") || query.contains("download"));
        assert!(query.contains("小说") || query.contains("novel"));
        assert!(!query.contains("不能简单复制"));
        assert!(!query.to_ascii_lowercase().contains("draft"));
        assert!(!query.to_ascii_lowercase().contains("revise"));
        assert!(!query.to_ascii_lowercase().contains("architect"));
        assert!(!query.contains("保存成txt"));
    }

    #[test]
    fn browser_wait_until_accepts_cdp_lifecycle_aliases() {
        assert_eq!(
            BrowserWaitUntil::parse(Some("domcontentloaded")).unwrap(),
            BrowserWaitUntil::DomContentLoaded
        );
        assert_eq!(
            BrowserWaitUntil::parse(Some("networkidle0")).unwrap(),
            BrowserWaitUntil::NetworkIdle
        );
        assert!(BrowserWaitUntil::parse(Some("forever")).is_err());
    }

    #[test]
    fn browser_snapshot_format_accepts_structured_outputs() {
        assert_eq!(
            BrowserSnapshotFormat::parse(Some("markdown")).unwrap(),
            BrowserSnapshotFormat::Markdown
        );
        assert_eq!(
            BrowserSnapshotFormat::parse(Some("links")).unwrap(),
            BrowserSnapshotFormat::Links
        );
        assert!(BrowserSnapshotFormat::parse(Some("pdf")).is_err());
    }

    #[test]
    fn browser_extract_links_resolves_relative_urls() {
        let html = r#"
            <html><head><title>Demo</title></head><body>
              <a href="/rank/free">免费榜</a>
              <a href="https://example.com/book/1">Book One</a>
              <a href="javascript:void(0)">Ignore</a>
            </body></html>
        "#;
        let links = BrowserTool::extract_links_from_html(html, "https://example.com/base/page", 10);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].text, "免费榜");
        assert_eq!(links[0].url, "https://example.com/rank/free");
    }

    #[test]
    fn browser_readonly_evaluate_blocks_side_effects() {
        assert!(BrowserTool::validate_readonly_evaluate_expression(
            "Array.from(document.querySelectorAll('a')).map(a => a.href)"
        )
        .is_ok());
        assert!(
            BrowserTool::validate_readonly_evaluate_expression("fetch('https://example.com')")
                .is_err()
        );
        assert!(BrowserTool::validate_readonly_evaluate_expression(
            "document.querySelector('button').click()"
        )
        .is_err());
    }

    #[test]
    fn browser_static_observation_payload_contains_cdp_boundary_and_trace() {
        let html = r#"
            <html><head><title>榜单</title></head>
            <body><h1>免费榜</h1><a href="/book/1">第一本</a>
            <p>这是一个包含足够页面正文的榜单示例，用于验证浏览器观察结果不会被误判为空壳页面。这里包含标题、链接、说明文字和可解析内容。</p>
            <p>第二段继续提供页面主体信息，让内容长度超过最低有效页面阈值。页面还包含若干候选条目、来源说明、更新时间、分类信息、阅读入口和榜单解释。</p>
            <p>第三段提供更多可见文本，模拟真实浏览器页面中的主体区域，确保诊断逻辑将其视为正常页面而不是空白壳。</p></body></html>
        "#;
        let payload = BrowserTool::render_static_observation_payload(
            "snapshot",
            "https://example.com/free",
            html,
            BrowserSnapshotFormat::Markdown,
            BrowserWaitUntil::NetworkIdle,
            true,
            vec!["action snapshot".to_string()],
        );
        assert_eq!(payload["cdp_boundary"]["page"]["wait_until"], "networkidle");
        assert_eq!(payload["cdp_boundary"]["dom"]["format"], "markdown");
        assert_eq!(payload["title"], "榜单");
        assert_eq!(payload["links"][0]["url"], "https://example.com/book/1");
        assert_eq!(payload["blockers"].as_array().unwrap().len(), 0);
        assert_eq!(payload["action_trace"][0], "action snapshot");
    }

    #[test]
    fn bing_parser_handles_modern_markup_and_redirect_urls() {
        let html = r#"
        <ol id="b_results">
          <li class="b_algo" data-id="1">
            <h2 class="">
              <a href="https://www.bing.com/ck/a?!&amp;&amp;p=foo&amp;u=a1aHR0cHM6Ly93d3cudGhlbGFuY2V0LmNvbS9qb3VybmFscy9sYW5jZXQvYXJ0aWNsZS9QSUlTMDE0MC02NzM2KDI1KTAxNTc4LTgvZnVsbHRleHQ&amp;ntb=1">
                Assessment of adverse effects attributed to statin therapy ... - The Lancet
              </a>
            </h2>
            <div class="b_caption">
              <p class="b_lineclamp2">2026年2月5日 · Adverse event data from blinded randomised trials...</p>
            </div>
          </li>
        </ol>
        "#;

        let results = BrowserTool::parse_bing_search_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].url,
            "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(25)01578-8/fulltext"
        );
        assert!(results[0].title.contains("The Lancet"));
        assert!(results[0].snippet.contains("Adverse event data"));
    }

    #[test]
    fn search_observation_parser_uses_cdp_links_when_markup_shape_changes() {
        let observation = WindowsCdpBridgeObservation {
            final_url: "https://www.bing.com/search?q=site%3Aexample.com+records".to_string(),
            title: Some("Search".to_string()),
            ready_state: Some("complete".to_string()),
            text: "Search results".to_string(),
            html: "<html><body><main data-layout='modern'></main></body></html>".to_string(),
            markdown: String::new(),
            links: vec![
                BrowserExtractedLink {
                    text: "Search - Microsoft Bing".to_string(),
                    url: "https://www.bing.com/".to_string(),
                },
                BrowserExtractedLink {
                    text: "Example public record".to_string(),
                    url: "https://example.com/records/1".to_string(),
                },
            ],
            resource_count: Some(4),
        };

        let results = BrowserTool::filter_results_for_query(
            "site:example.com records",
            BrowserTool::parse_search_results_from_observation(&observation, "bing", 5),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example public record");
        assert_eq!(results[0].url, "https://example.com/records/1");
    }

    #[test]
    fn search_challenge_pages_are_detected() {
        let html = r#"
        <html><head><title>Just a moment...</title></head>
        <body><div id="turnstile-widget"></div><script>window._cf_chl_opt = {}</script></body></html>
        "#;
        assert!(BrowserTool::page_indicates_search_challenge(html));
    }

    #[test]
    fn page_classification_detects_challenge_and_login_wall() {
        assert_eq!(
            BrowserTool::classify_page_content(
                "<html><body>Just a moment... Enable JavaScript and cookies to continue. Cloudflare Ray ID</body></html>"
            ),
            BrowserPageKind::Challenge
        );
        assert_eq!(
            BrowserTool::classify_page_content(
                "<html><body><h1>Sign in</h1><p>Please log in to continue.</p></body></html>"
            ),
            BrowserPageKind::LoginWall
        );
    }

    #[test]
    fn search_results_honor_site_constraint() {
        let results = vec![
            BrowserSearchResult {
                title: "Search - Microsoft Bing".to_string(),
                url: "https://www.bing.com/".to_string(),
                snippet: "Landing page".to_string(),
                source: "bing".to_string(),
                position: 1,
            },
            BrowserSearchResult {
                title: "The Lancet article".to_string(),
                url: "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(25)01578-8/fulltext".to_string(),
                snippet: "Cardiovascular treatment study".to_string(),
                source: "bing".to_string(),
                position: 2,
            },
        ];

        let filtered = BrowserTool::filter_results_for_query(
            "site:thelancet.com lancet heart disease treatment 2026",
            results,
        );
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].url.contains("thelancet.com"));
        assert_eq!(filtered[0].position, 1);
    }

    #[test]
    fn browser_search_filter_rejects_access_modifier_only_matches() {
        let query =
            "up 10 popular downloadable free fantasy Xuanhuan novels official results records data";
        let results = vec![
            BrowserSearchResult {
                title: "Free up drive space in Windows".to_string(),
                url: "https://support.microsoft.com/windows/free-up-drive-space".to_string(),
                snippet: "Learn how to free up storage and download updates.".to_string(),
                source: "bing".to_string(),
                position: 1,
            },
            BrowserSearchResult {
                title: "Free fantasy novels download".to_string(),
                url: "https://example.org/fantasy/free-novels".to_string(),
                snippet: "A catalogue of xuanhuan novels and fantasy fiction.".to_string(),
                source: "bing".to_string(),
                position: 2,
            },
        ];

        let filtered = BrowserTool::filter_results_for_query(query, results);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Free fantasy novels download");
        assert_eq!(filtered[0].position, 1);
    }

    #[test]
    fn browser_search_filter_rejects_chinese_access_modifier_only_matches() {
        let results = vec![
            BrowserSearchResult {
                title: "免费 在线游戏".to_string(),
                url: "https://example.com/free-games".to_string(),
                snippet: "热门内容下载".to_string(),
                source: "bing".to_string(),
                position: 1,
            },
            BrowserSearchResult {
                title: "免费玄幻小说目录".to_string(),
                url: "https://example.org/books/xuanhuan".to_string(),
                snippet: "奇幻小说与玄幻小说正文目录".to_string(),
                source: "bing".to_string(),
                position: 2,
            },
        ];

        let filtered =
            BrowserTool::filter_results_for_query("热门免费玄幻奇幻小说下载 完整内容", results);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "免费玄幻小说目录");
    }
}
