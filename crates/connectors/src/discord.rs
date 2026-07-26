use crate::DiscordConfig;
use async_trait::async_trait;
use benshu_infra::bus::{MessageBus, OutboundMessage};
use benshu_infra::error::{Error, Result};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

pub struct DiscordConnector {
    config: DiscordConfig,
    client: Client,
}

impl DiscordConnector {
    pub fn try_new(config: DiscordConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }
}

#[async_trait]
impl super::Connector for DiscordConnector {
    fn name(&self) -> &str {
        "discord"
    }

    fn metadata() -> super::ChannelMetadata {
        super::ChannelMetadata {
            id: "discord".to_string(),
            name: "Discord".to_string(),
            description: "Push notifications to Discord channels via Webhooks".to_string(),
            icon: "🎮".to_string(),
            fields: vec![
                super::ChannelField {
                    key: "DISCORD_BOT_TOKEN".to_string(),
                    label: "Bot Token".to_string(),
                    field_type: "password".to_string(),
                    description: "Discord Bot Token from Developer Portal".to_string(),
                    required: true,
                },
                super::ChannelField {
                    key: "DISCORD_CHANNEL_ID".to_string(),
                    label: "Channel ID".to_string(),
                    field_type: "text".to_string(),
                    description: "Target Discord Channel ID (Numeric)".to_string(),
                    required: true,
                },
            ],
        }
    }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        info!("Discord Connector started. Listening for webhook interactions...");

        // Handle outbound messages
        let mut outbound_rx = bus.subscribe_outbound();
        let this = Arc::new(Self {
            config: self.config.clone(),
            client: self.client.clone(),
        });

        let outbound_this = this.clone();
        tokio::spawn(async move {
            while let Ok(msg) = outbound_rx.recv().await {
                if msg.channel == "discord" || msg.channel == "broadcast" {
                    if let Err(e) = outbound_this.send(msg).await {
                        error!("Discord send error: {}", e);
                    }
                }
            }
        });

        let mut rx = bus.subscribe_webhook_event();
        let bus = bus.clone();

        while let Ok(event) = rx.recv().await {
            if event.connector_id != "discord" {
                continue;
            }

            let payload = event.payload;

            // Discord Interactions (Webhook mode)
            // https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-object
            let interaction_type = payload["type"].as_u64().unwrap_or(0);

            if interaction_type == 2 {
                // APPLICATION_COMMAND
                let data = &payload["data"];
                let command_name = match data["name"].as_str() {
                    Some(n) => n,
                    None => {
                        warn!("Discord interaction missing command name, skipping");
                        continue;
                    }
                };
                let chat_id = match payload["channel_id"].as_str() {
                    Some(id) => id.to_string(),
                    None => {
                        warn!("Discord interaction missing channel_id, skipping");
                        continue;
                    }
                };
                let sender_id = payload["member"]["user"]["id"]
                    .as_str()
                    .or(payload["user"]["id"].as_str())
                    .unwrap_or("unknown");

                // Check for attachments in resolved data or options
                let mut media = Vec::new();
                use benshu_infra::bus::{MediaAttachment, MediaType};

                if let Some(attachments) = payload["data"]["resolved"]["attachments"].as_object() {
                    for (_, attr) in attachments {
                        let url = attr["url"].as_str().unwrap_or_default();
                        if url.is_empty() {
                            continue;
                        }

                        let content_type = attr["content_type"].as_str().unwrap_or_default();
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

                // Get command content (options)
                let options = data["options"].as_array();
                let text = if let Some(opts) = options {
                    opts.iter()
                        .filter_map(|o| o["value"].as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    command_name.to_string()
                };

                info!(
                    "Discord connector received interaction /{} from {} ({} attachments)",
                    command_name,
                    sender_id,
                    media.len()
                );

                let mut inbound = benshu_infra::bus::InboundMessage::new(
                    "discord",
                    sender_id,
                    chat_id,
                    format!("/{} {}", command_name, text).trim().to_string(),
                );

                if !media.is_empty() {
                    inbound = inbound.with_media(media);
                }

                if let Err(e) = bus.publish_inbound(inbound).await {
                    error!("Failed to publish inbound Discord interaction: {}", e);
                }
            } else if interaction_type == 3 {
                // MESSAGE_COMPONENT
                let data = &payload["data"];
                let custom_id = data["custom_id"].as_str().unwrap_or("");
                let chat_id = payload["channel_id"].as_str().unwrap_or("").to_string();
                let sender_id = payload["member"]["user"]["id"]
                    .as_str()
                    .or(payload["user"]["id"].as_str())
                    .unwrap_or("unknown");

                info!(
                    "Discord connector received component interaction {} from {}",
                    custom_id, sender_id
                );

                let inbound = benshu_infra::bus::InboundMessage::new_callback(
                    "discord",
                    sender_id,
                    chat_id,
                    custom_id.to_string(),
                );

                if let Err(e) = bus.publish_inbound(inbound).await {
                    error!(
                        "Failed to publish inbound Discord component interaction: {}",
                        e
                    );
                }
            } else if interaction_type != 0 {
                tracing::debug!("Ignoring Discord interaction type {}", interaction_type);
            }
        }

        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        // We use the bot token to send via API (not just webhooks) if we have it
        // But the user might be using a Webhook URL.
        // Let's assume the config has a bot token as implemented in config/mod.rs.

        for channel_id in &self.config.channel_ids {
            let url = format!(
                "https://discord.com/api/v10/channels/{}/messages",
                channel_id
            );

            let mut payload = json!({
                "content": message.content
            });

            if let Some(buttons) = &message.buttons {
                let mut components = Vec::new();
                for btn in buttons {
                    components.push(json!({
                        "type": 2, // BUTTON
                        "style": 1, // PRIMARY
                        "label": btn.label,
                        "custom_id": btn.payload
                    }));
                }
                payload["components"] = json!([{
                    "type": 1, // ACTION_ROW
                    "components": components
                }]);
            }

            let res = self
                .client
                .post(&url)
                .header("Authorization", format!("Bot {}", self.config.bot_token))
                .json(&payload)
                .send()
                .await?;

            if !res.status().is_success() {
                let body = res.text().await.unwrap_or_default();
                error!("Discord send failed for channel {}: {}", channel_id, body);
            }
        }

        Ok(())
    }
}
