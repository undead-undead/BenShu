//! Connectors module for external messaging platforms.
//!
//! This module provides the `Connector` trait and implementations for
//! bi-directional communication channels like Telegram, Discord, and others.

use async_trait::async_trait;
use benshu_infra::bus::{MessageBus, OutboundMessage};
use benshu_infra::error::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QQConfig {
    pub app_id: String,
    pub app_secret: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_pass: String,
    pub imap_server: String,
    pub imap_port: u16,
    pub imap_user: String,
    pub imap_pass: String,
    pub from_address: String,
    pub broadcast_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_chat_ids: Vec<String>,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordConfig {
    pub bot_token: String,
    pub channel_ids: Vec<String>,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub verification_token: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DingTalkConfig {
    pub app_key: String,
    pub app_secret: String,
    pub broadcast_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>,
    pub verification_token: String,
    pub broadcast_chat_id: Option<String>,
}

/// Describes a configuration field required by a Channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelField {
    pub key: String,        // e.g. "telegram_bot_token"
    pub label: String,      // e.g. "Bot Token"
    pub field_type: String, // e.g. "password", "text"
    pub description: String,
    pub required: bool,
}

/// Metadata describing a supported channel and its configuration schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetadata {
    pub id: String,   // e.g. "telegram"
    pub name: String, // e.g. "Telegram"
    pub description: String,
    pub icon: String, // Emoji or icon name
    pub fields: Vec<ChannelField>,
}

/// A Connector bridges an external platform (Telegram, Discord) to the internal MessageBus.
#[async_trait]
pub trait Connector: Send + Sync {
    /// Start the connector loop (listening for messages)
    async fn start(&self, bus: Arc<MessageBus>) -> Result<()>;

    /// Send a message back to the platform
    async fn send(&self, message: OutboundMessage) -> Result<()>;

    /// Get the unique name of this connector (e.g., "telegram")
    fn name(&self) -> &str;

    /// Return the metadata schema for configuring this connector via the Panel
    fn metadata() -> ChannelMetadata
    where
        Self: Sized;
}

pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod qq;
pub mod slack;
pub mod telegram;

pub use dingtalk::DingTalkConnector;
pub use discord::DiscordConnector;
pub use email::EmailConnector;
pub use feishu::FeishuConnector;
pub use qq::QQConnector;
pub use slack::SlackConnector;
pub use telegram::TelegramConnector;
