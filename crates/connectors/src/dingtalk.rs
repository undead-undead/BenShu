use crate::DingTalkConfig;
use async_trait::async_trait;
use benshu_infra::bus::{MessageBus, OutboundMessage};
use benshu_infra::error::{Error, Result};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

pub struct DingTalkConnector {
    config: DingTalkConfig,
    client: Client,
}

impl DingTalkConnector {
    pub fn try_new(config: DingTalkConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }
}

#[async_trait]
impl super::Connector for DingTalkConnector {
    fn name(&self) -> &str {
        "dingtalk"
    }

    fn metadata() -> super::ChannelMetadata {
        super::ChannelMetadata {
            id: "dingtalk".to_string(),
            name: "DingTalk".to_string(),
            description: "Enterprise messaging integration for DingTalk".to_string(),
            icon: "🏢".to_string(),
            fields: vec![
                super::ChannelField {
                    key: "DINGTALK_APP_KEY".to_string(),
                    label: "App Key".to_string(),
                    field_type: "text".to_string(),
                    description: "DingTalk generic Application Key".to_string(),
                    required: true,
                },
                super::ChannelField {
                    key: "DINGTALK_APP_SECRET".to_string(),
                    label: "App Secret".to_string(),
                    field_type: "password".to_string(),
                    description: "Secret key for the DingTalk App".to_string(),
                    required: true,
                },
            ],
        }
    }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        info!("DingTalk Connector started. Listening for webhook events...");

        // Handle outbound messages
        let mut outbound_rx = bus.subscribe_outbound();
        let this = Arc::new(Self {
            config: self.config.clone(),
            client: self.client.clone(),
        });

        let outbound_this = this.clone();
        tokio::spawn(async move {
            while let Ok(msg) = outbound_rx.recv().await {
                if msg.channel == "dingtalk" || msg.channel == "broadcast" {
                    if let Err(e) = outbound_this.send(msg).await {
                        error!("DingTalk send error: {}", e);
                    }
                }
            }
        });

        let mut rx = bus.subscribe_webhook_event();
        let bus = bus.clone();

        while let Ok(event) = rx.recv().await {
            if event.connector_id != "dingtalk" {
                continue;
            }

            let payload = event.payload;

            // DingTalk Outgoing Webhook format:
            // { "text": { "content": "hello" }, "senderId": "123", "conversationId": "456", "msgtype": "text" }
            let msg_type = payload["msgtype"].as_str().unwrap_or_default();

            let sender_id = payload["senderId"].as_str().unwrap_or("unknown");
            let chat_id = payload["conversationId"].as_str().unwrap_or_default();
            let mut media = Vec::new();
            use benshu_infra::bus::{MediaAttachment, MediaType};

            let text = match msg_type {
                "text" => payload["text"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                "image" => {
                    if let Some(url) = payload["content"]["downloadCode"].as_str() {
                        media.push(MediaAttachment {
                            media_type: MediaType::Image,
                            url: url.to_string(),
                            caption: None,
                        });
                    }
                    "[Image Attachment]".to_string()
                }
                "audio" | "voice" => {
                    if let Some(url) = payload["content"]["downloadCode"].as_str() {
                        media.push(MediaAttachment {
                            media_type: MediaType::Voice,
                            url: url.to_string(),
                            caption: None,
                        });
                    }
                    "[Audio Attachment]".to_string()
                }
                "video" => {
                    if let Some(url) = payload["content"]["downloadCode"].as_str() {
                        media.push(MediaAttachment {
                            media_type: MediaType::Video,
                            url: url.to_string(),
                            caption: None,
                        });
                    }
                    "[Video Attachment]".to_string()
                }
                "file" => {
                    if let Some(url) = payload["content"]["downloadCode"].as_str() {
                        media.push(MediaAttachment {
                            media_type: MediaType::Document,
                            url: url.to_string(),
                            caption: payload["content"]["fileName"]
                                .as_str()
                                .map(|s| s.to_string()),
                        });
                    }
                    "[File Attachment]".to_string()
                }
                _ => payload["text"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            };

            if (!text.is_empty() || !media.is_empty()) && !chat_id.is_empty() {
                info!(
                    "DingTalk connector received {} message from {}: {}",
                    msg_type, sender_id, text
                );

                let mut inbound =
                    benshu_infra::bus::InboundMessage::new("dingtalk", sender_id, chat_id, text);

                if !media.is_empty() {
                    inbound = inbound.with_media(media);
                }

                if let Err(e) = bus.publish_inbound(inbound).await {
                    error!("Failed to publish inbound DingTalk message: {}", e);
                }
            } else if payload["actionKey"].is_string() {
                // DingTalk Card Interaction
                let payload_val = payload["actionKey"].as_str().unwrap_or_default();
                let user_id = payload["senderId"].as_str().unwrap_or("unknown");
                let chat_id = payload["conversationId"].as_str().unwrap_or_default();

                if !payload_val.is_empty() {
                    info!(
                        "DingTalk connector received card interaction from {}: {}",
                        user_id, payload_val
                    );
                    let inbound = benshu_infra::bus::InboundMessage::new_callback(
                        "dingtalk",
                        user_id,
                        chat_id,
                        payload_val,
                    );
                    let _ = bus.publish_inbound(inbound).await;
                }
            }
        }

        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let app_key = &self.config.app_key;
        let app_secret = &self.config.app_secret;

        // 1. Get Access Token
        let token_url = format!(
            "https://oapi.dingtalk.com/gettoken?appkey={}&appsecret={}",
            app_key, app_secret
        );
        let token_resp = self.client.get(token_url).send().await?;
        if !token_resp.status().is_success() {
            return Err(Error::Internal(format!(
                "DingTalk token fetch failed: {}",
                token_resp.status()
            )));
        }
        let token_data: serde_json::Value = token_resp.json().await?;
        let access_token = token_data["access_token"].as_str().ok_or_else(|| {
            Error::Internal("Failed to extract DingTalk access_token".to_string())
        })?;

        // 2. Send Message (Assuming Chatbot Internal App Message for now)
        // Similar to Feishu, we need a target
        let target_id = if message.channel == "broadcast" {
            self.config.broadcast_chat_id.clone().unwrap_or_default()
        } else if message.chat_id.is_empty() {
            return Err(Error::Internal(
                "DingTalk message chat_id (open_conversation_id/userid) is missing".to_string(),
            ));
        } else {
            message.chat_id.clone()
        };

        if target_id.is_empty() {
            return Ok(());
        }

        // Standard DingTalk Robot/Message API
        let send_url = format!("https://oapi.dingtalk.com/topapi/message/corpconversation/asyncsend_v2?access_token={}", access_token);

        let mut msgtype = "text";
        let mut msg_content = json!({ "content": message.content });

        if let Some(buttons) = &message.buttons {
            msgtype = "action_card";
            let mut btns = Vec::new();
            for btn in buttons {
                btns.push(json!({
                    "title": btn.label,
                    "action_id": btn.payload // Using action_id for internal button mapping
                }));
            }
            msg_content = json!({
                "title": "Approval Request",
                "markdown": format!("### Approval Request\n\n{}", message.content),
                "btn_orientation": "1",
                "btn_json_list": btns
            });
        }

        let payload = json!({
            "msg": {
                "msgtype": msgtype,
                msgtype: msg_content
            },
            "to_all_user": false,
            "userid_list": target_id
        });

        let send_resp = self.client.post(send_url).json(&payload).send().await?;
        if !send_resp.status().is_success() {
            let error_body = send_resp.text().await.unwrap_or_default();
            error!("DingTalk send failed: {}", error_body);
            return Err(Error::Internal(format!(
                "DingTalk send failed: {}",
                error_body
            )));
        }

        info!("DingTalk message dispatched successfully.");
        Ok(())
    }
}
