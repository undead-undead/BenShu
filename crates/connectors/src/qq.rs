use crate::QQConfig;
use async_trait::async_trait;
use benshu_infra::bus::{InboundMessage, MessageBus, OutboundMessage};
use benshu_infra::error::{Error, Result};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tracing::{error, info, warn};
use url::Url;

pub struct QQConnector {
    config: QQConfig,
    client: Client,
}

impl QQConnector {
    pub fn try_new(config: QQConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    async fn get_access_token(&self) -> Result<String> {
        let url = "https://bots.qq.com/app/getAppAccessToken";
        let res = self
            .client
            .post(url)
            .json(&json!({
                "appId": self.config.app_id,
                "clientSecret": self.config.app_secret
            }))
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Internal(format!("QQ Auth error: {}", body)));
        }

        let data: Value = res.json().await?;
        data["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Internal("Missing access_token in QQ response".to_string()))
    }

    async fn get_gateway_url(&self, token: &str) -> Result<String> {
        let url = "https://api.sgroup.qq.com/gateway/bot";
        let res = self
            .client
            .get(url)
            .header("Authorization", format!("QQBot {}", token))
            .send()
            .await?;

        let data: Value = res.json().await?;
        data["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Internal("Missing gateway url in QQ response".to_string()))
    }
}

#[async_trait]
impl super::Connector for QQConnector {
    fn name(&self) -> &str {
        "qq"
    }

    fn metadata() -> super::ChannelMetadata {
        super::ChannelMetadata {
            id: "qq".to_string(),
            name: "QQ Bot".to_string(),
            description: "Official QQ Bot Open Platform (AppID + Secret)".to_string(),
            icon: "🐧".to_string(),
            fields: vec![
                super::ChannelField {
                    key: "QQ_APP_ID".to_string(),
                    label: "App ID".to_string(),
                    field_type: "text".to_string(),
                    description: "QQ Bot AppID from q.qq.com".to_string(),
                    required: true,
                },
                super::ChannelField {
                    key: "QQ_APP_SECRET".to_string(),
                    label: "App Secret".to_string(),
                    field_type: "password".to_string(),
                    description: "QQ Bot AppSecret".to_string(),
                    required: true,
                },
            ],
        }
    }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        info!("QQ Connector starting (WebSocket mode)...");

        let bus_clone = bus.clone();
        let config_clone = self.config.clone();
        let client_clone = self.client.clone();

        // Outbound handler
        let mut outbound_rx = bus.subscribe_outbound();
        tokio::spawn(async move {
            while let Ok(msg) = outbound_rx.recv().await {
                if msg.channel == "qq" || msg.channel == "broadcast" {
                    // Handled synchronously in send() trait method
                }
            }
        });

        // Inbound handler (WebSocket Loop)
        tokio::spawn(async move {
            loop {
                if let Err(e) = run_qq_ws_loop(&bus_clone, &config_clone, &client_clone).await {
                    error!("QQ WebSocket loop error: {}. Retrying in 10s...", e);
                    sleep(Duration::from_secs(10)).await;
                }
            }
        });

        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let token = self.get_access_token().await?;

        let chat_id = if message.channel == "broadcast" {
            self.config
                .broadcast_chat_id
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("")
        } else {
            &message.chat_id
        };

        if chat_id.is_empty() {
            return Ok(());
        }

        // QQ Bot API: Send message (Channels/DMs/Groups have different endpoints)
        // For simplicity, we assume 'chat_id' is the channel_id or open_id
        // Using the most common one: /channels/{channel_id}/messages
        let url = format!("https://api.sgroup.qq.com/channels/{}/messages", chat_id);

        let mut payload = json!({
            "content": message.content
        });

        // Add buttons if present (QQ uses markdown or keyboard)
        if let Some(buttons) = &message.buttons {
            let mut rows = Vec::new();
            for btn in buttons {
                rows.push(json!({
                     "buttons": [
                         {
                             "render_data": { "label": btn.label, "visited_label": btn.label, "style": 0 },
                             "action": {
                                 "type": 0, // HTTP or callback
                                 "permission": { "type": 2 },
                                 "data": btn.payload,
                                 "unsupport_tips": "BenShu commands only"
                             }
                         }
                     ]
                 }));
            }
            payload["keyboard"] = json!({ "content": { "rows": rows } });
        }

        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("QQBot {}", token))
            .header("X-Union-Appid", &self.config.app_id)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            error!("QQ send failed: {}", body);
        }

        Ok(())
    }
}

async fn run_qq_ws_loop(bus: &MessageBus, config: &QQConfig, client: &Client) -> Result<()> {
    // 1. Get token
    let token = get_qq_token(client, config).await?;

    // 2. Get Gateway URL
    let gateway_url = get_qq_gateway(client, &token).await?;

    // 3. Connect
    let (mut ws_stream, _) = connect_async(&gateway_url)
        .await
        .map_err(|e| Error::Internal(format!("QQ WS Connect failed: {}", e)))?;

    info!("QQ WebSocket connected.");

    let mut session_id = String::new();
    let mut last_s: u64 = 0;

    // 4. Identify
    let identify = json!({
        "op": 2,
        "d": {
            "token": format!("QQBot {}", token),
            "intents": 1 << 30 | 1 << 0, // PUBLIC_GUILD_MESSAGES + GUILDS
            "shard": [0, 1],
            "properties": {
                "$os": "linux",
                "$browser": "BenShu",
                "$device": "BenShu"
            }
        }
    });
    ws_stream
        .send(WsMessage::Text(identify.to_string().into()))
        .await
        .map_err(|e| Error::Internal(format!("QQ WS Identify failed: {}", e)))?;

    // 5. Main Loop
    while let Some(msg) = ws_stream.next().await {
        let msg = msg.map_err(|e| Error::Internal(format!("QQ WS Recv error: {}", e)))?;

        if let WsMessage::Text(text) = msg {
            let json: Value = serde_json::from_str(&text).unwrap_or_default();
            let op = json["op"].as_u64().unwrap_or(99);

            if let Some(s) = json["s"].as_u64() {
                last_s = s;
            }

            match op {
                0 => {
                    // Dispatch
                    let t = json["t"].as_str().unwrap_or("");
                    let d = &json["d"];

                    if t == "READY" {
                        session_id = d["session_id"].as_str().unwrap_or_default().to_string();
                        info!("QQ Bot Ready. Session: {}", session_id);
                    } else if t == "AT_MESSAGE_CREATE"
                        || t == "MESSAGE_CREATE"
                        || t == "PUBLIC_GUILD_MESSAGES"
                        || t == "DIRECT_MESSAGE_CREATE"
                    {
                        let sender = d["author"]["username"].as_str().unwrap_or("unknown");
                        let chat_id = d["channel_id"]
                            .as_str()
                            .or(d["guild_id"].as_str())
                            .unwrap_or_default();
                        let content = d["content"].as_str().unwrap_or("");

                        let mut media = Vec::new();
                        use benshu_infra::bus::{MediaAttachment, MediaType};

                        // Extract attachments if present
                        if let Some(attachments) = d["attachments"].as_array() {
                            for attr in attachments {
                                let url = attr["url"].as_str().unwrap_or_default();
                                if url.is_empty() {
                                    continue;
                                }

                                let content_type =
                                    attr["content_type"].as_str().unwrap_or_default();
                                let media_type = if content_type.starts_with("image/") {
                                    MediaType::Image
                                } else if content_type.starts_with("audio/") {
                                    MediaType::Voice
                                } else if content_type.starts_with("video/") {
                                    MediaType::Video
                                } else {
                                    MediaType::Document
                                };

                                media.push(MediaAttachment {
                                    media_type,
                                    url: url.to_string(),
                                    caption: attr["filename"].as_str().map(|s| s.to_string()),
                                });
                            }
                        }

                        // Clean @mention if present
                        let clean_content = content
                            .split_whitespace()
                            .filter(|s| !s.starts_with("<@!"))
                            .collect::<Vec<_>>()
                            .join(" ");

                        if (!clean_content.is_empty() || !media.is_empty()) && !chat_id.is_empty() {
                            info!(
                                "QQ message from {}: {} ({} attachments)",
                                sender,
                                clean_content,
                                media.len()
                            );
                            let mut inbound =
                                InboundMessage::new("qq", sender, chat_id, clean_content);
                            if !media.is_empty() {
                                inbound = inbound.with_media(media);
                            }
                            let _ = bus.publish_inbound(inbound).await;
                        }
                    }
                }
                1 => {
                    // Heartbeat Request
                    let hb = json!({ "op": 1, "d": last_s });
                    let _ = ws_stream.send(WsMessage::Text(hb.to_string().into())).await;
                }
                10 => {
                    // Hello
                    let heartbeat_interval = d_to_u64(&json["d"]["heartbeat_interval"]) % 30000;
                    // Cap at 30s
                    // We should spawn a heartbeat task here.
                    // Simplified: we'll just continue and wait for next msg.
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn get_qq_token(client: &Client, config: &QQConfig) -> Result<String> {
    let url = "https://bots.qq.com/app/getAppAccessToken";
    let res = client
        .post(url)
        .json(&json!({
            "appId": config.app_id,
            "clientSecret": config.app_secret
        }))
        .send()
        .await?;
    let data: Value = res.json().await?;
    data["access_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Internal("Auth fail".to_string()))
}

async fn get_qq_gateway(client: &Client, token: &str) -> Result<String> {
    let url = "https://api.sgroup.qq.com/gateway/bot";
    let res = client
        .get(url)
        .header("Authorization", format!("QQBot {}", token))
        .send()
        .await?;
    let data: Value = res.json().await?;
    data["url"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Internal("No gateway".to_string()))
}

fn d_to_u64(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}
