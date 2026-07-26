//! Notification steps for BenShu pipelines.
//!
//! This module provides ready-to-use pipeline steps for sending notifications
//! via Telegram, Discord, and Email (via webhook/API).

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Notification channel types shared by agents and tool runtimes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NotifyChannel {
    /// Send an email.
    Email,
    /// Send via Telegram.
    Telegram,
    /// Send via Discord.
    Discord,
    /// Send via Slack.
    Slack,
    /// Send to a generic webhook.
    Webhook { url: String },
    /// Log to console/file.
    Log,
}

/// Trait for sending notifications from runtime components.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Send a notification.
    async fn notify(&self, channel: NotifyChannel, message: &str) -> Result<()>;
}

/// A no-op notifier that writes to tracing.
pub struct LogNotifier;

#[async_trait]
impl Notifier for LogNotifier {
    async fn notify(&self, channel: NotifyChannel, message: &str) -> Result<()> {
        let channel_name = match channel {
            NotifyChannel::Email => "Email",
            NotifyChannel::Telegram => "Telegram",
            NotifyChannel::Discord => "Discord",
            NotifyChannel::Slack => "Slack",
            NotifyChannel::Webhook { .. } => "Webhook",
            NotifyChannel::Log => "Log",
        };
        tracing::info!("[Notification via {}]: {}", channel_name, message);
        Ok(())
    }
}

// --- Telegram Notification ---

/// A step that sends a message to a Telegram chat using a bot token.
#[derive(Debug)]
pub struct TelegramStep {
    _bot_token: String,
    _chat_id: String,
    _message_template: String, // Simple template string
}

impl TelegramStep {
    /// Create a new Telegram notification step
    ///
    /// `message_template` can contain placeholders like `{key}` which will be replaced
    /// by values from `context.data` if they exist and are strings/numbers.
    pub fn new(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        message_template: impl Into<String>,
    ) -> Self {
        Self {
            _bot_token: bot_token.into(),
            _chat_id: chat_id.into(),
            _message_template: message_template.into(),
        }
    }
}

// --- Discord Notification ---

/// A step that sends a message to a Discord channel via Webhook.
#[derive(Debug)]
pub struct DiscordStep {
    _webhook_url: String,
    username: Option<String>,
    _avatar_url: Option<String>,
    _message_template: String,
}

impl DiscordStep {
    pub fn new(webhook_url: impl Into<String>, message_template: impl Into<String>) -> Self {
        Self {
            _webhook_url: webhook_url.into(),
            _message_template: message_template.into(),
            username: None,
            _avatar_url: None,
        }
    }

    pub fn username(mut self, name: impl Into<String>) -> Self {
        self.username = Some(name.into());
        self
    }
}

// --- Email Notification (Generic Webhook) ---

/// A step that sends an email via a generic HTTP API (like SendGrid/Mailgun).
/// Since SMTP is heavy, we recommend using HTTP APIs for agents.
#[derive(Debug)]
pub struct EmailStep {
    _api_url: String,
    _api_key: String,
    _to: String,
    _subject: String,
    _provider: EmailProvider,
}

#[derive(Debug)]
pub enum EmailProvider {
    Mailgun { domain: String },
    SendGrid,
    CustomWebhook, // Assumes a generic POST {to, subject, body}
}

impl EmailStep {
    pub fn new_mailgun(api_key: &str, domain: &str, to: &str, subject: &str) -> Self {
        Self {
            _api_url: format!("https://api.mailgun.net/v3/{}/messages", domain),
            _api_key: api_key.to_string(),
            _to: to.to_string(),
            _subject: subject.to_string(),
            _provider: EmailProvider::Mailgun {
                domain: domain.to_string(),
            },
        }
    }

    pub fn new_sendgrid(api_key: &str, to: &str, subject: &str) -> Self {
        Self {
            _api_url: "https://api.sendgrid.com/v3/mail/send".to_string(),
            _api_key: api_key.to_string(),
            _to: to.to_string(),
            _subject: subject.to_string(),
            _provider: EmailProvider::SendGrid,
        }
    }
}
