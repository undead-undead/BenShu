use crate::api::state::{AppError, AppState, ChannelObservability};
use axum::{extract::State, response::IntoResponse, Json};
use benshu_connectors::{
    ChannelMetadata, Connector, DingTalkConnector, DiscordConnector, EmailConnector,
    FeishuConnector, QQConnector, SlackConnector, TelegramConnector,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChannelSchemaResponse {
    pub channels: Vec<ChannelMetadata>,
    pub running: Vec<String>,
    pub observability: Vec<ChannelObservability>,
}

pub async fn get_channel_schema(State(state): State<AppState>) -> Json<ChannelSchemaResponse> {
    let schemas = vec![
        TelegramConnector::metadata(),
        DiscordConnector::metadata(),
        SlackConnector::metadata(),
        EmailConnector::metadata(),
        FeishuConnector::metadata(),
        DingTalkConnector::metadata(),
        QQConnector::metadata(),
    ];
    let running = state.running_connectors.read().iter().cloned().collect();
    let observability = state
        .channel_observability
        .read()
        .values()
        .cloned()
        .collect();
    Json(ChannelSchemaResponse {
        channels: schemas,
        running,
        observability,
    })
}

#[derive(Debug, Deserialize)]
pub struct ChannelConfigRequest {
    pub channel_id: String,
    pub values: std::collections::HashMap<String, String>,
}

pub async fn save_channel_config(
    State(state): State<AppState>,
    Json(req): Json<ChannelConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    {
        let mut config = state.app_config.write();

        for (key, value) in &req.values {
            if value.trim().is_empty() {
                continue;
            }

            match key.as_str() {
                "TELEGRAM_BOT_TOKEN" => {
                    let mut tg = config.connectors.telegram.clone().unwrap_or_default();
                    tg.bot_token = value.clone();
                    config.connectors.telegram = Some(tg);
                }
                "TELEGRAM_ALLOWED_CHAT_IDS" => {
                    let mut tg = config.connectors.telegram.clone().unwrap_or_default();
                    tg.allowed_chat_ids = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    config.connectors.telegram = Some(tg);
                }
                "DISCORD_BOT_TOKEN" => {
                    let mut ds = config.connectors.discord.clone().unwrap_or_default();
                    ds.bot_token = value.clone();
                    config.connectors.discord = Some(ds);
                }
                "DISCORD_CHANNEL_ID" => {
                    let mut ds = config.connectors.discord.clone().unwrap_or_default();
                    ds.channel_ids = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    config.connectors.discord = Some(ds);
                }
                "SLACK_BOT_TOKEN" => {
                    let mut slack = config.connectors.slack.clone().unwrap_or_default();
                    slack.bot_token = value.clone();
                    config.connectors.slack = Some(slack);
                }
                "SLACK_APP_TOKEN" => {
                    let mut slack = config.connectors.slack.clone().unwrap_or_default();
                    slack.app_token = Some(value.clone());
                    config.connectors.slack = Some(slack);
                }
                "SLACK_VERIFICATION_TOKEN" => {
                    let mut slack = config.connectors.slack.clone().unwrap_or_default();
                    slack.verification_token = value.clone();
                    config.connectors.slack = Some(slack);
                }
                "FEISHU_APP_ID" => {
                    let mut feishu = config.connectors.feishu.clone().unwrap_or_default();
                    feishu.app_id = value.clone();
                    config.connectors.feishu = Some(feishu);
                }
                "FEISHU_APP_SECRET" => {
                    let mut feishu = config.connectors.feishu.clone().unwrap_or_default();
                    feishu.app_secret = value.clone();
                    config.connectors.feishu = Some(feishu);
                }
                "FEISHU_VERIFICATION_TOKEN" => {
                    let mut feishu = config.connectors.feishu.clone().unwrap_or_default();
                    feishu.verification_token = value.clone();
                    config.connectors.feishu = Some(feishu);
                }
                "DINGTALK_APP_KEY" => {
                    let mut dingtalk = config.connectors.dingtalk.clone().unwrap_or_default();
                    dingtalk.app_key = value.clone();
                    config.connectors.dingtalk = Some(dingtalk);
                }
                "DINGTALK_APP_SECRET" => {
                    let mut dingtalk = config.connectors.dingtalk.clone().unwrap_or_default();
                    dingtalk.app_secret = value.clone();
                    config.connectors.dingtalk = Some(dingtalk);
                }
                "QQ_APP_ID" => {
                    let mut qq = config.connectors.qq.clone().unwrap_or_default();
                    qq.app_id = value.clone();
                    config.connectors.qq = Some(qq);
                }
                "QQ_APP_SECRET" => {
                    let mut qq = config.connectors.qq.clone().unwrap_or_default();
                    qq.app_secret = value.clone();
                    config.connectors.qq = Some(qq);
                }
                "SMTP_SERVER" => {
                    let mut email = config.connectors.email.clone().unwrap_or_default();
                    email.smtp_server = value.clone();
                    config.connectors.email = Some(email);
                }
                "SMTP_PORT" => {
                    if let Ok(port) = value.parse::<u16>() {
                        let mut email = config.connectors.email.clone().unwrap_or_default();
                        email.smtp_port = port;
                        config.connectors.email = Some(email);
                    }
                }
                "SMTP_USER" => {
                    let mut email = config.connectors.email.clone().unwrap_or_default();
                    email.smtp_user = value.clone();
                    config.connectors.email = Some(email);
                }
                "SMTP_PASS" => {
                    let mut email = config.connectors.email.clone().unwrap_or_default();
                    email.smtp_pass = value.clone();
                    config.connectors.email = Some(email);
                }
                "FROM_ADDRESS" => {
                    let mut email = config.connectors.email.clone().unwrap_or_default();
                    email.from_address = value.clone();
                    config.connectors.email = Some(email);
                }
                _ => {
                    // Unknown key - ignore or handle via vault
                }
            }
        }
        config.save_to_file(&state.config_path)?;
    }

    let _ = state.connector_trigger.send(());

    Ok(axum::http::StatusCode::OK)
}

use axum::response::Response;

pub async fn webhook_handler(
    axum::extract::Path(connector_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    if connector_id == "dingtalk" && payload["type"] == "sync_http_push" {
        // Placeholder
    }

    let mut header_map = std::collections::HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(val) = value.to_str() {
            header_map.insert(name.to_string(), val.to_string());
        }
    }

    let event = benshu_infra::bus::WebhookEvent::new(connector_id.clone(), payload)
        .with_headers(header_map);

    if let Err(e) = state.bus.publish_webhook_event(event).await {
        {
            let mut guard = state.channel_observability.write();
            let entry = guard
                .entry(connector_id.clone())
                .or_insert_with(|| ChannelObservability {
                    channel_id: connector_id.clone(),
                    ..Default::default()
                });
            entry.last_failure_kind = Some("webhook_publish_failed".to_string());
            entry.last_failure_detail = Some(e.to_string());
            entry.last_observed_at = Some(chrono::Utc::now());
        }
        tracing::error!("Failed to publish webhook event to bus: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Error: {}", e),
        )
            .into_response();
    }

    {
        let mut guard = state.channel_observability.write();
        let entry = guard
            .entry(connector_id.clone())
            .or_insert_with(|| ChannelObservability {
                channel_id: connector_id,
                ..Default::default()
            });
        entry.inbound_total += 1;
        entry.last_observed_at = Some(chrono::Utc::now());
    }

    axum::http::StatusCode::OK.into_response()
}
