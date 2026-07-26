//! Data visualization tool — chart generation via Python backends.
//!
//! Generates matplotlib/plotly scripts and executes them to produce
//! PNG/SVG images or interactive HTML charts.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_runtimes::python_utils;
use benshu_state::{ArtifactLifecycle, ArtifactManager};

use super::{register_tool_output_artifact, ToolArtifactRegistration, ToolCleanup};

pub struct ChartTool {
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl ChartTool {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[derive(Deserialize)]
struct ChartArgs {
    action: String,
    #[serde(default = "default_chart_type")]
    chart_type: String,
    #[serde(default)]
    data: serde_json::Value,
    #[serde(default)]
    title: String,
    #[serde(default)]
    x_label: String,
    #[serde(default)]
    y_label: String,
    #[serde(default)]
    output: String,
    #[serde(default = "default_backend")]
    backend: String,
}

fn default_chart_type() -> String {
    "bar".into()
}
fn default_backend() -> String {
    "svg".into()
}

#[async_trait]
impl Tool for ChartTool {
    fn name(&self) -> String {
        "chart".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "chart".to_string(),
            description: "Generate charts and data visualizations (bar, line, pie, scatter, histogram, heatmap)".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["generate", "info"], "description": "Action to perform" },
                    "chart_type": { "type": "string", "enum": ["bar", "line", "pie", "scatter", "histogram", "heatmap"], "description": "Chart type" },
                    "data": { "type": "object", "description": "Chart data: {labels: [...], values: [...]} or {x: [...], y: [...]}" },
                    "title": { "type": "string", "description": "Chart title" },
                    "x_label": { "type": "string", "description": "X-axis label" },
                    "y_label": { "type": "string", "description": "Y-axis label" },
                    "output": { "type": "string", "description": "Output file path (e.g., chart.png, chart.html)" },
                    "backend": { "type": "string", "enum": ["svg", "matplotlib", "plotly"], "description": "Rendering backend. svg is dependency-free and preferred for quick local charts." }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use to create visual charts. Prefer backend=svg for dependency-free local charts. Python backends require matplotlib or plotly.".into()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ChartArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "chart".into(),
                message: e.to_string(),
            })?;

        let result = match args.action.as_str() {
            "info" => check_backends().await?,
            "generate" => {
                generate_chart_with_registry(
                    &args,
                    self.artifact_manager.as_deref(),
                    &self.agent_id,
                )
                .await?
            }
            _ => json!({"error": format!("Unknown action: {}", args.action)}),
        };

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

async fn check_backends() -> anyhow::Result<serde_json::Value> {
    let python_bin = python_utils::find_python().await;
    let has_python = python_bin.is_some();

    // We don't check modules here as we will install them on-demand in a venv
    Ok(json!({
        "python_available": has_python,
        "managed_python": python_bin.map(|p| p.to_string_lossy().contains(".benshu")).unwrap_or(false),
        "note": "Dependencies (matplotlib/plotly) are installed automatically in an isolated venv on first run.",
        "cleanup": ToolCleanup::active(
            "ephemeral_output_default",
            "chart_default_output_is_temp",
            "Chart helper scripts are auto-removed after execution. If you do not provide an output path, the generated chart stays in the OS temp directory until you move or delete it.",
            "move_temp_output_to_durable_path_if_needed",
            true,
        ).as_json()
    }))
}

async fn generate_chart_with_registry(
    args: &ChartArgs,
    artifact_manager: Option<&ArtifactManager>,
    agent_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let temp_dir = std::env::temp_dir();
    let user_supplied_output = !args.output.is_empty();
    let output_path = if !user_supplied_output {
        let ext = match args.backend.as_str() {
            "plotly" => "html",
            "svg" => "svg",
            _ => "png",
        };
        let filename = format!("benshu_chart_{}.{}", chrono::Utc::now().timestamp(), ext);
        temp_dir.join(filename).to_string_lossy().into_owned()
    } else {
        args.output.clone()
    };

    if args.backend == "svg" {
        let svg = build_svg_chart(args)?;
        tokio::fs::write(&output_path, svg).await?;
        let cleanup = chart_cleanup(user_supplied_output);
        let artifact_registration = register_chart_output_artifact(
            args,
            artifact_manager,
            agent_id,
            &output_path,
            user_supplied_output,
        )
        .await?;
        return Ok(json!({
            "success": true,
            "output_path": output_path,
            "chart_type": args.chart_type,
            "backend": args.backend,
            "cleanup": cleanup.as_json(),
            "artifact_registration": artifact_registration,
        }));
    }

    // 1. Resolve Python and deps
    let base_python = match python_utils::find_python().await {
        Some(p) => p,
        None => python_utils::provision_python_via_uv().await?,
    };

    let deps = if args.backend == "plotly" {
        vec!["plotly".to_string(), "pandas".to_string()]
    } else {
        vec!["matplotlib".to_string()]
    };

    let python_bin = python_utils::ensure_venv(&base_python, "chart_tool", &deps).await?;

    let script = if args.backend == "plotly" {
        build_plotly_script(args, &output_path)?
    } else {
        build_matplotlib_script(args, &output_path)?
    };

    // Write script to temp file
    let script_filename = format!("benshu_chart_{}.py", chrono::Utc::now().timestamp_millis());
    let script_path = temp_dir.join(script_filename);
    tokio::fs::write(&script_path, &script).await?;

    let output = tokio::process::Command::new(python_bin)
        .arg(&script_path)
        .output()
        .await?;

    let _ = tokio::fs::remove_file(&script_path).await;
    let cleanup = chart_cleanup(user_supplied_output);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("ModuleNotFoundError") {
            if let Ok(svg) = build_svg_chart(args) {
                let fallback_path = if user_supplied_output {
                    format!("{output_path}.svg")
                } else {
                    temp_dir
                        .join(format!(
                            "benshu_chart_{}_fallback.svg",
                            chrono::Utc::now().timestamp()
                        ))
                        .to_string_lossy()
                        .into_owned()
                };
                tokio::fs::write(&fallback_path, svg).await?;
                let artifact_registration = register_chart_output_artifact(
                    args,
                    artifact_manager,
                    agent_id,
                    &fallback_path,
                    user_supplied_output,
                )
                .await?;
                return Ok(json!({
                    "success": true,
                    "output_path": fallback_path,
                    "chart_type": args.chart_type,
                    "backend": "svg",
                    "degraded_from": args.backend,
                    "degradation": stderr.to_string(),
                    "cleanup": cleanup.as_json(),
                    "artifact_registration": artifact_registration,
                }));
            }
        }
        return Ok(json!({
            "error": format!("Python execution failed: {}", stderr),
            "cleanup": cleanup.as_json(),
        }));
    }

    let artifact_registration = register_chart_output_artifact(
        args,
        artifact_manager,
        agent_id,
        &output_path,
        user_supplied_output,
    )
    .await?;

    Ok(json!({
        "success": true,
        "output_path": output_path,
        "chart_type": args.chart_type,
        "backend": args.backend,
        "cleanup": cleanup.as_json(),
        "artifact_registration": artifact_registration,
    }))
}

fn build_svg_chart(args: &ChartArgs) -> anyhow::Result<String> {
    let labels = args
        .data
        .get("labels")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("svg chart requires data.labels"))?;
    let values = args
        .data
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("svg chart requires data.values"))?;
    if labels.is_empty() || labels.len() != values.len() {
        return Err(anyhow::anyhow!(
            "svg chart requires non-empty labels and values with matching lengths"
        ));
    }

    let numeric_values: Vec<f64> = values
        .iter()
        .map(|value| value.as_f64().unwrap_or(0.0).max(0.0))
        .collect();
    let max_value = numeric_values
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let width = 800.0_f64;
    let height = 480.0_f64;
    let margin_left = 72.0_f64;
    let margin_bottom = 72.0_f64;
    let plot_width = width - margin_left - 40.0;
    let plot_height = height - 80.0 - margin_bottom;
    let bar_gap = 12.0_f64;
    let bar_width =
        ((plot_width - bar_gap * (labels.len() as f64 + 1.0)) / labels.len() as f64).max(8.0);

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\
         <rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\
         <text x=\"{cx}\" y=\"36\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"22\" fill=\"#111827\">{title}</text>\
         <line x1=\"{ml}\" y1=\"{base}\" x2=\"{right}\" y2=\"{base}\" stroke=\"#374151\"/>\
         <line x1=\"{ml}\" y1=\"80\" x2=\"{ml}\" y2=\"{base}\" stroke=\"#374151\"/>",
        cx = width / 2.0,
        title = escape_svg(if args.title.is_empty() {
            "BenShu Chart"
        } else {
            &args.title
        }),
        ml = margin_left,
        base = height - margin_bottom,
        right = width - 40.0,
    );

    for (idx, (label, value)) in labels.iter().zip(numeric_values.iter()).enumerate() {
        let x = margin_left + bar_gap + idx as f64 * (bar_width + bar_gap);
        let bar_height = (*value / max_value) * plot_height;
        let y = height - margin_bottom - bar_height;
        svg.push_str(&format!(
            "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{bar_width:.2}\" height=\"{bar_height:.2}\" rx=\"4\" fill=\"#2563eb\"/>\
             <text x=\"{tx:.2}\" y=\"{vy:.2}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"13\" fill=\"#111827\">{value:.2}</text>\
             <text x=\"{tx:.2}\" y=\"{ly:.2}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"13\" fill=\"#374151\">{label}</text>",
            tx = x + bar_width / 2.0,
            vy = y - 8.0,
            ly = height - margin_bottom + 24.0,
            label = escape_svg(label.as_str().unwrap_or_default()),
        ));
    }

    svg.push_str("</svg>");
    Ok(svg)
}

fn escape_svg(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn register_chart_output_artifact(
    args: &ChartArgs,
    artifact_manager: Option<&ArtifactManager>,
    agent_id: &str,
    output_path: &str,
    user_supplied_output: bool,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(manager) = artifact_manager else {
        return Ok(None);
    };

    let mut metadata = HashMap::new();
    metadata.insert("backend".to_string(), args.backend.clone());
    metadata.insert("chart_type".to_string(), args.chart_type.clone());
    metadata.insert(
        "output_origin".to_string(),
        if user_supplied_output {
            "user_supplied".to_string()
        } else {
            "tool_temp_default".to_string()
        },
    );
    let record = register_tool_output_artifact(
        manager,
        agent_id,
        "chart",
        output_path,
        ArtifactLifecycle::Session,
        "chart_output",
        metadata,
    )
    .await?;
    Ok(Some(
        ToolArtifactRegistration::from_record(&record).as_json(),
    ))
}

fn chart_cleanup(user_supplied_output: bool) -> ToolCleanup {
    if user_supplied_output {
        ToolCleanup::inactive()
    } else {
        ToolCleanup::active(
            "ephemeral_output_default",
            "chart_default_output_is_temp",
            "The chart was written into the OS temp directory. Move it to a durable path if you want to keep it.",
            "move_temp_output_to_durable_path_if_needed",
            true,
        )
    }
}

fn quote_py(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn build_matplotlib_script(args: &ChartArgs, output_path: &str) -> anyhow::Result<String> {
    let labels = args.data.get("labels").and_then(|v| v.as_array());
    let values = args.data.get("values").and_then(|v| v.as_array());
    let x = args.data.get("x").and_then(|v| v.as_array());
    let y = args.data.get("y").and_then(|v| v.as_array());

    let mut script = String::from(
        "import matplotlib\nmatplotlib.use('Agg')\nimport matplotlib.pyplot as plt\n\n",
    );

    match args.chart_type.as_str() {
        "bar" => {
            if let (Some(l), Some(v)) = (labels, values) {
                script.push_str(&format!("plt.bar({:?}, {:?})\n", l, v));
            }
        }
        "line" => {
            let xd = x.or(labels);
            let yd = y.or(values);
            if let (Some(xv), Some(yv)) = (xd, yd) {
                script.push_str(&format!("plt.plot({:?}, {:?})\n", xv, yv));
            }
        }
        "pie" => {
            if let (Some(l), Some(v)) = (labels, values) {
                script.push_str(&format!(
                    "plt.pie({:?}, labels={:?}, autopct='%1.1f%%')\n",
                    v, l
                ));
            }
        }
        "scatter" => {
            if let (Some(xv), Some(yv)) = (x, y) {
                script.push_str(&format!("plt.scatter({:?}, {:?})\n", xv, yv));
            }
        }
        "histogram" => {
            if let Some(v) = values.or(y) {
                script.push_str(&format!("plt.hist({:?}, bins=20)\n", v));
            }
        }
        _ => {}
    }

    if !args.title.is_empty() {
        script.push_str(&format!("plt.title('{}')\n", quote_py(&args.title)));
    }
    if !args.x_label.is_empty() {
        script.push_str(&format!("plt.xlabel('{}')\n", quote_py(&args.x_label)));
    }
    if !args.y_label.is_empty() {
        script.push_str(&format!("plt.ylabel('{}')\n", quote_py(&args.y_label)));
    }
    script.push_str("plt.tight_layout()\n");
    script.push_str(&format!(
        "plt.savefig('{}', dpi=150)\n",
        quote_py(output_path)
    ));
    script.push_str("plt.close()\n");

    Ok(script)
}

fn build_plotly_script(args: &ChartArgs, output_path: &str) -> anyhow::Result<String> {
    let labels = args.data.get("labels").and_then(|v| v.as_array());
    let values = args.data.get("values").and_then(|v| v.as_array());
    let x = args.data.get("x").and_then(|v| v.as_array());
    let y = args.data.get("y").and_then(|v| v.as_array());

    let mut script = String::from("import plotly.graph_objects as go\n\n");

    match args.chart_type.as_str() {
        "bar" => {
            if let (Some(l), Some(v)) = (labels, values) {
                script.push_str(&format!("fig = go.Figure(go.Bar(x={:?}, y={:?}))\n", l, v));
            }
        }
        "line" => {
            let xd = x.or(labels);
            let yd = y.or(values);
            if let (Some(xv), Some(yv)) = (xd, yd) {
                script.push_str(&format!(
                    "fig = go.Figure(go.Scatter(x={:?}, y={:?}, mode='lines'))\n",
                    xv, yv
                ));
            }
        }
        "pie" => {
            if let (Some(l), Some(v)) = (labels, values) {
                script.push_str(&format!(
                    "fig = go.Figure(go.Pie(labels={:?}, values={:?}))\n",
                    l, v
                ));
            }
        }
        _ => {
            script.push_str("fig = go.Figure()\n");
        }
    }

    if !args.title.is_empty() {
        script.push_str(&format!(
            "fig.update_layout(title='{}')\n",
            quote_py(&args.title)
        ));
    }
    script.push_str(&format!("fig.write_html('{}')\n", quote_py(output_path)));

    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{TOOL_ARTIFACT_REGISTRATION_SCHEMA_VERSION, TOOL_CLEANUP_SCHEMA_VERSION};
    use benshu_state::ArtifactQuery;

    #[tokio::test]
    async fn test_definition() {
        let tool = ChartTool::new("chart-test");
        let def = tool.definition().await;
        assert_eq!(def.name, "chart");
    }

    #[tokio::test]
    async fn test_info_reports_cleanup_contract() {
        let info = check_backends().await.expect("info");
        assert_eq!(
            info["cleanup"]["schema_version"].as_str(),
            Some(TOOL_CLEANUP_SCHEMA_VERSION)
        );
        assert_eq!(info["cleanup"]["active"].as_bool(), Some(true));
        assert_eq!(
            info["cleanup"]["cleanup_hint"].as_str(),
            Some("move_temp_output_to_durable_path_if_needed")
        );
    }

    #[test]
    fn test_matplotlib_script_generation() {
        let temp_dir = std::env::temp_dir();
        let test_out = temp_dir
            .join("test_chart.png")
            .to_string_lossy()
            .into_owned();
        let args = ChartArgs {
            action: "generate".into(),
            chart_type: "bar".into(),
            data: json!({"labels": ["A", "B", "C"], "values": [10, 20, 30]}),
            title: "Test Chart".into(),
            x_label: "Category".into(),
            y_label: "Value".into(),
            output: test_out.clone(),
            backend: "matplotlib".into(),
        };
        let script = build_matplotlib_script(&args, &test_out).unwrap();
        assert!(script.contains("plt.bar"));
        assert!(script.contains("Test Chart"));
    }

    #[test]
    fn test_chart_cleanup_uses_ephemeral_profile_when_output_is_implicit() {
        let cleanup = chart_cleanup(false);
        assert_eq!(cleanup.schema_version, TOOL_CLEANUP_SCHEMA_VERSION);
        assert!(cleanup.active);
        assert_eq!(cleanup.reason, "chart_default_output_is_temp");
    }

    #[tokio::test]
    async fn test_chart_registers_artifact_when_registry_is_present() {
        let db_path = std::env::temp_dir().join(format!(
            "benshu_chart_artifact_test_{}.redb",
            uuid::Uuid::new_v4()
        ));
        let db = redb::Database::create(&db_path).expect("db");
        let manager = ArtifactManager::new(Arc::new(db));

        let output_path = std::env::temp_dir()
            .join(format!("benshu_chart_test_{}.png", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        tokio::fs::write(&output_path, b"fake chart bytes")
            .await
            .expect("output");

        let args = ChartArgs {
            action: "generate".into(),
            chart_type: "bar".into(),
            data: json!({"labels": ["A"], "values": [1]}),
            title: "Chart".into(),
            x_label: String::new(),
            y_label: String::new(),
            output: output_path.clone(),
            backend: "matplotlib".into(),
        };

        let result = register_chart_output_artifact(
            &args,
            Some(&manager),
            "chart-agent",
            &output_path,
            true,
        )
        .await
        .expect("ok")
        .expect("registration");
        assert_eq!(
            result["schema_version"].as_str(),
            Some(TOOL_ARTIFACT_REGISTRATION_SCHEMA_VERSION)
        );

        let artifacts = manager
            .query(&ArtifactQuery {
                source_kind: Some("builtin_tool_output".to_string()),
                limit: Some(10),
                ..ArtifactQuery::default()
            })
            .await
            .expect("query");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].tool_name.as_deref(), Some("chart"));
        assert_eq!(artifacts[0].uri, output_path);

        let _ = tokio::fs::remove_file(&db_path).await;
        let _ = tokio::fs::remove_file(&output_path).await;
    }
}
