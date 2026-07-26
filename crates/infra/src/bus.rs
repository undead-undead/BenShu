use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Inbound message from external channels (Telegram, CLI, Scheduler, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    pub media: Option<Vec<MediaAttachment>>,
    pub timestamp: DateTime<Utc>,
    pub session_key: String,
    pub payload: Option<String>,
}

impl InboundMessage {
    pub fn new(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let channel = channel.into();
        let chat_id = chat_id.into();
        let session_key = format!("{}:{}", channel, chat_id);

        Self {
            channel,
            sender_id: sender_id.into(),
            chat_id,
            content: content.into(),
            media: None,
            timestamp: Utc::now(),
            session_key,
            payload: None,
        }
    }

    pub fn new_callback(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let content = content.into();
        let mut msg = Self::new(channel, sender_id, chat_id, content.clone());
        msg.payload = Some(content);
        msg
    }

    pub fn with_media(mut self, media: Vec<MediaAttachment>) -> Self {
        self.media = Some(media);
        self
    }
}

/// Outbound message to external channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub content: String,
    pub media: Option<Vec<MediaAttachment>>,
    pub buttons: Option<Vec<Button>>,
}

impl OutboundMessage {
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            media: None,
            buttons: None,
        }
    }

    pub fn with_media(mut self, media: Vec<MediaAttachment>) -> Self {
        self.media = Some(media);
        self
    }

    pub fn with_buttons(mut self, buttons: Vec<Button>) -> Self {
        self.buttons = Some(buttons);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Button {
    pub label: String,
    pub payload: String,
}

impl Button {
    pub fn new(label: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            payload: payload.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub media_type: MediaType,
    pub url: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    #[serde(
        alias = "image/png",
        alias = "image/jpeg",
        alias = "image/jpg",
        alias = "image/webp",
        alias = "image/bmp",
        alias = "image/gif"
    )]
    Image,
    #[serde(
        alias = "audio/mpeg",
        alias = "audio/wav",
        alias = "audio/x-wav",
        alias = "audio/ogg",
        alias = "audio/flac",
        alias = "audio/aac",
        alias = "audio/opus",
        alias = "audio/mp4"
    )]
    Voice,
    #[serde(
        alias = "video/mp4",
        alias = "video/quicktime",
        alias = "video/x-msvideo",
        alias = "video/x-matroska",
        alias = "video/webm"
    )]
    Video,
    #[serde(
        alias = "application/pdf",
        alias = "application/json",
        alias = "text/plain",
        alias = "text/markdown",
        alias = "text/csv",
        alias = "text/html",
        alias = "text/css",
        alias = "application/xml",
        alias = "text/xml",
        alias = "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        alias = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        alias = "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    )]
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub connector_id: String,
    pub payload: serde_json::Value,
    pub headers: std::collections::HashMap<String, String>,
}

impl WebhookEvent {
    pub fn new(connector_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            connector_id: connector_id.into(),
            payload,
            headers: std::collections::HashMap::new(),
        }
    }

    pub fn with_headers(mut self, headers: std::collections::HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }
}

/// Message Bus - central routing for all messages
pub struct MessageBus {
    inbound_tx: mpsc::Sender<InboundMessage>,
    inbound_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<InboundMessage>>>,
    outbound_tx: broadcast::Sender<OutboundMessage>,
    webhook_tx: broadcast::Sender<WebhookEvent>,
    comm_tx: broadcast::Sender<(String, Vec<u8>)>, // (Target Address, Payload)
    inbound_count: Arc<std::sync::atomic::AtomicU64>,
    outbound_count: Arc<std::sync::atomic::AtomicU64>,
    comm_count: Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusStats {
    pub inbound_total: u64,
    pub outbound_total: u64,
    pub comm_total: u64,
}

impl MessageBus {
    pub fn new(buffer_size: usize) -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(buffer_size);
        let (outbound_tx, _) = broadcast::channel(buffer_size);
        let (webhook_tx, _) = broadcast::channel(buffer_size);
        let (comm_tx, _) = broadcast::channel(buffer_size);

        Self {
            inbound_tx,
            inbound_rx: Arc::new(tokio::sync::Mutex::new(inbound_rx)),
            outbound_tx,
            webhook_tx,
            comm_tx,
            inbound_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            outbound_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            comm_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub async fn publish_inbound(&self, message: InboundMessage) -> Result<()> {
        self.inbound_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inbound_tx.send(message).await.map_err(|e| {
            crate::error::Error::Internal(format!("Failed to publish inbound message: {}", e))
        })
    }

    pub async fn consume_inbound(&self) -> Result<InboundMessage> {
        let mut rx = self.inbound_rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| crate::error::Error::Internal("Inbound channel closed".to_string()))
    }

    pub async fn publish_outbound(&self, message: OutboundMessage) -> Result<()> {
        self.outbound_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = self.outbound_tx.send(message);
        Ok(())
    }

    pub fn subscribe_outbound(&self) -> broadcast::Receiver<OutboundMessage> {
        self.outbound_tx.subscribe()
    }

    pub fn inbound_sender(&self) -> mpsc::Sender<InboundMessage> {
        self.inbound_tx.clone()
    }

    pub async fn publish_webhook_event(&self, event: WebhookEvent) -> Result<()> {
        let _ = self.webhook_tx.send(event);
        Ok(())
    }

    pub fn subscribe_webhook_event(&self) -> broadcast::Receiver<WebhookEvent> {
        self.webhook_tx.subscribe()
    }

    pub async fn publish_comm(&self, target: String, payload: Vec<u8>) -> Result<()> {
        self.comm_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = self.comm_tx.send((target, payload));
        Ok(())
    }

    pub fn subscribe_comm(&self) -> broadcast::Receiver<(String, Vec<u8>)> {
        self.comm_tx.subscribe()
    }

    pub async fn broadcast_notification(&self, content: String) -> Result<()> {
        let message = OutboundMessage::new("broadcast", "all", content);
        self.publish_outbound(message).await
    }

    pub fn get_stats(&self) -> BusStats {
        BusStats {
            inbound_total: self
                .inbound_count
                .load(std::sync::atomic::Ordering::Relaxed),
            outbound_total: self
                .outbound_count
                .load(std::sync::atomic::Ordering::Relaxed),
            comm_total: self.comm_count.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Clone for MessageBus {
    fn clone(&self) -> Self {
        Self {
            inbound_tx: self.inbound_tx.clone(),
            inbound_rx: Arc::clone(&self.inbound_rx),
            outbound_tx: self.outbound_tx.clone(),
            webhook_tx: self.webhook_tx.clone(),
            comm_tx: self.comm_tx.clone(),
            inbound_count: Arc::clone(&self.inbound_count),
            outbound_count: Arc::clone(&self.outbound_count),
            comm_count: Arc::clone(&self.comm_count),
        }
    }
}
