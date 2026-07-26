use crate::error::TransportError;
use crate::protocol::CommEnvelope;
use crate::transport::Transport;
use async_trait::async_trait;
use prost::Message;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Bridge transport for cross-process / cross-node communication via TCP
///
/// Responsibility: Serialize to Protobuf over TCP stream
pub struct BridgeTransport {
    remote_addr: String,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl BridgeTransport {
    pub fn new(remote_addr: String) -> Self {
        Self {
            remote_addr,
            stream: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_stream(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<TcpStream>>, TransportError> {
        let mut stream_guard = self.stream.lock().await;
        if stream_guard.is_none() {
            let stream = TcpStream::connect(&self.remote_addr).await.map_err(|e| {
                TransportError::Network(format!(
                    "Bridge connection failed to {}: {}",
                    self.remote_addr, e
                ))
            })?;
            *stream_guard = Some(stream);
        }
        Ok(stream_guard)
    }
}

#[async_trait]
impl Transport for BridgeTransport {
    async fn send(&self, envelope: CommEnvelope) -> Result<(), TransportError> {
        let proto_env: crate::protocol::proto::CommEnvelope = envelope.into();
        let mut buf = Vec::new();
        proto_env
            .encode(&mut buf)
            .map_err(|e| TransportError::Protocol(format!("Proto encode failed: {}", e)))?;

        let mut stream_guard = self.get_stream().await?;
        if let Some(stream) = stream_guard.as_mut() {
            // Write length prefix (u32 LE)
            let len = buf.len() as u32;
            stream
                .write_all(&len.to_le_bytes())
                .await
                .map_err(|e| TransportError::Network(format!("TCP send len failed: {}", e)))?;
            stream
                .write_all(&buf)
                .await
                .map_err(|e| TransportError::Network(format!("TCP send payload failed: {}", e)))?;
            Ok(())
        } else {
            Err(TransportError::Network("No active TCP stream".into()))
        }
    }

    async fn status(&self) -> crate::transport::TransportStatus {
        let is_connected = self.stream.lock().await.is_some();
        crate::transport::TransportStatus {
            name: "BridgeTransport".to_string(),
            is_connected,
            metrics: [("remote_addr".to_string(), self.remote_addr.clone())]
                .into_iter()
                .collect(),
        }
    }

    async fn receive(&self) -> Result<CommEnvelope, TransportError> {
        // A BridgeTransport in client mode usually only receives responses if the protocol is duplex
        // But for a true BRIDGE, it might be an 'Acceptor'.
        // This client implementation waits for the next chunk from the stream.
        let mut stream_guard = self.get_stream().await?;
        if let Some(stream) = stream_guard.as_mut() {
            // Read length prefix
            let mut len_buf = [0u8; 4];
            stream
                .read_exact(&mut len_buf)
                .await
                .map_err(|e| TransportError::Network(format!("TCP read len failed: {}", e)))?;
            let len = u32::from_le_bytes(len_buf) as usize;

            let mut payload = vec![0u8; len];
            stream
                .read_exact(&mut payload)
                .await
                .map_err(|e| TransportError::Network(format!("TCP read payload failed: {}", e)))?;

            let proto_env = crate::protocol::proto::CommEnvelope::decode(&payload[..])
                .map_err(|e| TransportError::Protocol(format!("Proto decode failed: {}", e)))?;

            Ok(proto_env.into())
        } else {
            Err(TransportError::Network("No active TCP stream".into()))
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        let mut stream_guard = self.stream.lock().await;
        *stream_guard = None;
        Ok(())
    }
}
