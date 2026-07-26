use crate::TelegramConfig;
use async_trait::async_trait;
use benshu_infra::bus::{InboundMessage, MessageBus, OutboundMessage};
use benshu_infra::error::{Error, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

pub struct TelegramConnector {
    config: TelegramConfig,
    client: Client,
}

impl TelegramConnector {
    pub fn try_new(config: TelegramConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<Value>> {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates",
            self.config.bot_token
        );

        let res = self
            .client
            .post(&url)
            .json(&json!({
                "offset": offset,
                "timeout": 25, // Long polling
                "allowed_updates": ["message", "callback_query"]
            }))
            .send()
            .await?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Internal(format!("Telegram API error: {}", body)));
        }

        let json: Value = res.json().await?;
        if let Some(updates) = json.get("result").and_then(|v| v.as_array()) {
            Ok(updates.clone())
        } else {
            Ok(Vec::new())
        }
    }

    async fn get_file_url(&self, file_id: &str) -> Result<String> {
        let url = format!(
            "https://api.telegram.org/bot{}/getFile?file_id={}",
            self.config.bot_token, file_id
        );

        let res = self.client.get(&url).send().await?;
        let json: Value = res.json().await?;

        if let Some(file_path) = json
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(|p| p.as_str())
        {
            Ok(format!(
                "https://api.telegram.org/file/bot{}/{}",
                self.config.bot_token, file_path
            ))
        } else {
            Err(Error::Internal(
                "Failed to get file path from Telegram".to_string(),
            ))
        }
    }

    /// Internal helper to process a Telegram update object.
    /// Returns false if the update was already seen (dedup).
    async fn process_update(
        connector: &TelegramConnector, // Change to reference connector for get_file_url
        bus: &MessageBus,
        config: &TelegramConfig,
        update: Value,
        seen: &std::sync::Mutex<HashSet<i64>>,
    ) {
        // Dedup: skip if we already processed this update_id
        if let Some(update_id) = update.get("update_id").and_then(|v| v.as_i64()) {
            let mut set = seen.lock().unwrap_or_else(|e| e.into_inner());
            if !set.insert(update_id) {
                return; // Already processed
            }
            // Keep the set from growing unboundedly
            if set.len() > 10_000 {
                set.clear();
            }
        }

        if let Some(msg) = update.get("message") {
            let chat_id = msg
                .get("chat")
                .and_then(|c| c.get("id"))
                .map(|id| id.to_string());
            let text = msg.get("text").and_then(|t| t.as_str());

            if let Some(cid) = chat_id.clone() {
                if !config.allowed_chat_ids.is_empty() && !config.allowed_chat_ids.contains(&cid) {
                    warn!("Ignored message from unauthorized chat: {}", cid);
                    return;
                }
            }

            let sender = msg
                .get("from")
                .and_then(|f| f.get("username").and_then(|u| u.as_str()))
                .unwrap_or("unknown");

            let mut media = Vec::new();
            use benshu_infra::bus::{MediaAttachment, MediaType};

            // Detect Voice
            if let Some(voice) = msg.get("voice") {
                if let Some(file_id) = voice.get("file_id").and_then(|v| v.as_str()) {
                    if let Ok(url) = connector.get_file_url(file_id).await {
                        media.push(MediaAttachment {
                            media_type: MediaType::Voice,
                            url,
                            caption: None,
                        });
                    }
                }
            }

            // Detect Video
            if let Some(video) = msg.get("video") {
                if let Some(file_id) = video.get("file_id").and_then(|v| v.as_str()) {
                    if let Ok(url) = connector.get_file_url(file_id).await {
                        media.push(MediaAttachment {
                            media_type: MediaType::Video,
                            url,
                            caption: msg
                                .get("caption")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        });
                    }
                }
            }

            // Detect Photo
            if let Some(photos) = msg.get("photo").and_then(|p| p.as_array()) {
                if let Some(last_photo) = photos.last() {
                    if let Some(file_id) = last_photo.get("file_id").and_then(|v| v.as_str()) {
                        if let Ok(url) = connector.get_file_url(file_id).await {
                            media.push(MediaAttachment {
                                media_type: MediaType::Image,
                                url,
                                caption: msg
                                    .get("caption")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                            });
                        }
                    }
                }
            }

            if let (Some(chat_id), Some(text)) = (chat_id.clone(), text) {
                info!("Received Telegram text message from {}: {}", sender, text);
                let mut inbound = InboundMessage::new("telegram", sender, chat_id, text);
                if !media.is_empty() {
                    inbound = inbound.with_media(media);
                }
                let _ = bus.publish_inbound(inbound).await;
            } else if let Some(chat_id) = chat_id {
                // Media message without text caption
                if !media.is_empty() {
                    info!("Received Telegram media-only message from {}", sender);
                    let inbound =
                        InboundMessage::new("telegram", sender, chat_id, "[Media Attachment]")
                            .with_media(media);
                    let _ = bus.publish_inbound(inbound).await;
                }
            }
        } else if let Some(cb) = update.get("callback_query") {
            let sender = cb
                .get("from")
                .and_then(|f| f.get("username").and_then(|u| u.as_str()))
                .unwrap_or("unknown");
            let chat_id = cb
                .get("message")
                .and_then(|m| m.get("chat"))
                .and_then(|c| c.get("id"))
                .map(|id| id.to_string());
            let data = cb.get("data").and_then(|v| v.as_str());

            if let (Some(chat_id), Some(payload)) = (chat_id, data) {
                info!("Received Telegram callback from {}: {}", sender, payload);
                let inbound = InboundMessage::new_callback("telegram", sender, chat_id, payload);
                let _ = bus.publish_inbound(inbound).await;
            }
        }
    }

    fn chunk_message_text(text: &str, max_chars: usize) -> Vec<String> {
        if text.is_empty() {
            return vec![String::new()];
        }

        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut current_len = 0usize;

        for ch in text.chars() {
            current.push(ch);
            current_len += 1;
            if current_len >= max_chars {
                chunks.push(std::mem::take(&mut current));
                current_len = 0;
            }
        }

        if !current.is_empty() {
            chunks.push(current);
        }

        chunks
    }
}

#[async_trait]
impl super::Connector for TelegramConnector {
    fn name(&self) -> &str {
        "telegram"
    }

    fn metadata() -> super::ChannelMetadata {
        super::ChannelMetadata {
            id: "telegram".to_string(),
            name: "Telegram".to_string(),
            description: "Bi-directional text and command interface via Telegram Bot API"
                .to_string(),
            icon: "💬".to_string(),
            fields: vec![
                super::ChannelField {
                    key: "TELEGRAM_BOT_TOKEN".to_string(),
                    label: "Bot API Token".to_string(),
                    field_type: "password".to_string(),
                    description: "Get this from @BotFather".to_string(),
                    required: true,
                },
                super::ChannelField {
                    key: "TELEGRAM_ALLOWED_CHAT_IDS".to_string(),
                    label: "Whitelisted Chat IDs".to_string(),
                    field_type: "text".to_string(),
                    description: "Comma-separated chat IDs (blank to allow all)".to_string(),
                    required: false,
                },
            ],
        }
    }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        info!("Telegram Connector started. Monitoring (Polling + Webhooks)...");

        // Handle outbound messages
        let mut outbound_rx = bus.subscribe_outbound();
        let this = Arc::new(Self {
            config: self.config.clone(),
            client: self.client.clone(),
        });

        let outbound_this = this.clone();
        tokio::spawn(async move {
            while let Ok(msg) = outbound_rx.recv().await {
                if msg.channel == "telegram" || msg.channel == "broadcast" {
                    if let Err(e) = outbound_this.send(msg).await {
                        error!("Telegram send error: {}", e);
                    }
                }
            }
        });

        // Shared dedup set to prevent processing the same update from both webhook and polling
        let seen_updates: Arc<std::sync::Mutex<HashSet<i64>>> =
            Arc::new(std::sync::Mutex::new(HashSet::new()));

        let bus_webhook = bus.clone();
        let mut webhook_rx = bus.subscribe_webhook_event();
        let config_webhook = self.config.clone();
        let seen_wh = seen_updates.clone();

        // Task A: Webhook Receiver
        let webhook_this = this.clone();
        tokio::spawn(async move {
            while let Ok(event) = webhook_rx.recv().await {
                if event.connector_id != "telegram" {
                    continue;
                }
                // Telegram webhooks usually send the "Update" object directly
                Self::process_update(
                    &webhook_this,
                    &bus_webhook,
                    &config_webhook,
                    event.payload,
                    &seen_wh,
                )
                .await;
            }
        });

        // Task B: Long Polling (Fallback/Self-contained)
        let mut offset = 0;
        let bus = bus.clone();
        let config = self.config.clone();
        let seen_poll = seen_updates;
        let poll_this = this.clone();

        loop {
            match poll_this.get_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        if let Some(update_id) = update.get("update_id").and_then(|v| v.as_i64()) {
                            offset = update_id + 1;
                        }
                        Self::process_update(&poll_this, &bus, &config, update, &seen_poll).await;
                    }
                }
                Err(e) => {
                    error!("Telegram getUpdates failed: {}. Retrying in 5s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.config.bot_token
        );

        let target_chat = if message.channel == "broadcast" {
            self.config
                .broadcast_chat_id
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("")
        } else {
            &message.chat_id
        };

        if target_chat.is_empty() {
            return Ok(());
        }

        let chunks = Self::chunk_message_text(&message.content, 3500);
        let total_chunks = chunks.len();

        for (idx, chunk) in chunks.into_iter().enumerate() {
            let mut payload = json!({
                "chat_id": target_chat,
                "text": chunk,
            });

            if idx + 1 == total_chunks {
                if let Some(buttons) = &message.buttons {
                    let mut keyboard = Vec::new();
                    for btn in buttons {
                        keyboard.push(vec![json!({
                            "text": btn.label,
                            "callback_data": btn.payload
                        })]);
                    }
                    payload["reply_markup"] = json!({ "inline_keyboard": keyboard });
                }
            }

            let res = self.client.post(&url).json(&payload).send().await?;
            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                return Err(Error::Internal(format!(
                    "Telegram sendMessage failed: status={} body={}",
                    status, body
                )));
            }
        }

        info!(
            "Telegram message delivered to chat {} ({} chunk(s))",
            target_chat, total_chunks
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn test_connector() -> TelegramConnector {
        TelegramConnector::try_new(TelegramConfig::default()).expect("test connector")
    }

    #[tokio::test]
    async fn telegram_update_parsing_covers_text_and_callback_semantics() {
        let connector = test_connector();
        let bus = MessageBus::new(8);
        let config = TelegramConfig::default();
        let seen = std::sync::Mutex::new(HashSet::new());

        TelegramConnector::process_update(
            &connector,
            &bus,
            &config,
            json!({
                "update_id": 1001,
                "message": {
                    "chat": { "id": 424242 },
                    "from": { "username": "jarvis_user" },
                    "text": "/stop"
                }
            }),
            &seen,
        )
        .await;

        let text_msg = bus.consume_inbound().await.expect("text inbound message");
        assert_eq!(text_msg.channel, "telegram");
        assert_eq!(text_msg.sender_id, "jarvis_user");
        assert_eq!(text_msg.chat_id, "424242");
        assert_eq!(text_msg.content, "/stop");
        assert_eq!(text_msg.session_key, "telegram:424242");
        assert!(text_msg.payload.is_none());

        TelegramConnector::process_update(
            &connector,
            &bus,
            &config,
            json!({
                "update_id": 1002,
                "callback_query": {
                    "from": { "username": "jarvis_user" },
                    "message": {
                        "chat": { "id": 424242 }
                    },
                    "data": "approve:task-1"
                }
            }),
            &seen,
        )
        .await;

        let callback_msg = bus
            .consume_inbound()
            .await
            .expect("callback inbound message");
        assert_eq!(callback_msg.channel, "telegram");
        assert_eq!(callback_msg.chat_id, "424242");
        assert_eq!(callback_msg.content, "approve:task-1");
        assert_eq!(callback_msg.payload.as_deref(), Some("approve:task-1"));
        assert_eq!(callback_msg.session_key, "telegram:424242");
    }
}
