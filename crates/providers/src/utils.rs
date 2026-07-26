//! Utilities for LLM providers

use crate::{Error, Result};
use base64::Engine;
use benshu_protocol_core::{AudioSource, ImageSource, VideoSource};
use bytes::{BufMut, BytesMut};

const MAX_LOCAL_MEDIA_BYTES: u64 = 100 * 1024 * 1024;
const MAX_REMOTE_MEDIA_BYTES: u64 = 100 * 1024 * 1024;

/// A buffer for accumulating SSE (Server-Sent Events) bytes.
///
/// This buffer is resilient to UTF-8 characters being split across network chunks.
/// It accumulates bytes and only returns complete UTF-8 strings.
#[derive(Debug)]
pub struct SseBuffer {
    buffer: BytesMut,
    max_capacity: usize,
}

impl Default for SseBuffer {
    fn default() -> Self {
        Self {
            buffer: BytesMut::new(),
            max_capacity: 10 * 1024 * 1024, // Default 10MB
        }
    }
}

impl SseBuffer {
    /// Create a new empty SSE buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom capacity limit
    pub fn with_capacity_limit(max_capacity: usize) -> Self {
        Self {
            buffer: BytesMut::new(),
            max_capacity,
        }
    }

    /// Add bytes to the buffer
    pub fn extend_from_slice(&mut self, bytes: &[u8]) -> Result<()> {
        if self.buffer.len() + bytes.len() > self.max_capacity {
            return Err(Error::StreamInterrupted(format!(
                "SSE buffer exceeded max capacity of {} bytes",
                self.max_capacity
            )));
        }
        self.buffer.put_slice(bytes);
        Ok(())
    }

    /// Extract all complete UTF-8 SSE messages from the buffer.
    ///
    /// Returns a list of strings, each representing one or more lines of SSE data.
    /// Any incomplete UTF-8 sequence or incomplete SSE message (missing \n\n)
    /// will remain in the buffer for the next call.
    pub fn extract_messages(&mut self) -> Result<Vec<String>> {
        let mut messages = Vec::new();

        while let Some((pos, delimiter_len)) = self.find_sse_delimiter() {
            // Found a complete SSE message. Providers may use LF or CRLF
            // line endings, so the delimiter length is not always 2 bytes.
            let end_pos = pos + delimiter_len;
            let chunk = self.buffer.split_to(end_pos);

            // Try to convert to UTF-8
            match String::from_utf8(chunk.to_vec()) {
                Ok(s) => messages.push(s),
                Err(e) => {
                    // This should ideally not happen if we only split at \n\n
                    // unless the delimiter itself is part of a multi-byte char (impossible for ASCII \n)
                    return Err(Error::StreamInterrupted(format!(
                        "Invalid UTF-8 in SSE stream: {}",
                        e
                    )));
                }
            }
        }

        Ok(messages)
    }

    /// Check if the buffer ends with an incomplete UTF-8 sequence.
    /// This is a simplified check - in practice, we rely on the fact that
    /// SSE messages are delimited by \n\n (ASCII), which cannot be part of a
    /// multi-byte UTF-8 character's trailing bytes.
    fn find_sse_delimiter(&self) -> Option<(usize, usize)> {
        let bytes = self.buffer.as_ref();
        for i in 0..bytes.len().saturating_sub(1) {
            if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
                return Some((i, 2));
            }
            if i + 3 < bytes.len()
                && bytes[i] == b'\r'
                && bytes[i + 1] == b'\n'
                && bytes[i + 2] == b'\r'
                && bytes[i + 3] == b'\n'
            {
                return Some((i, 4));
            }
        }
        None
    }

    /// Convert the entire buffer to a string, handling potentially split UTF-8 at the very end.
    pub fn push_and_get_text(&mut self, bytes: &[u8]) -> Result<String> {
        self.extend_from_slice(bytes)?;

        let bytes = self.buffer.as_ref();
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                let text = s.to_string();
                self.buffer.clear();
                Ok(text)
            }
            Err(e) => {
                let valid_len = e.valid_up_to();
                let valid_bytes = self.buffer.split_to(valid_len);
                // The remaining invalid (incomplete) bytes stay in the buffer
                Ok(String::from_utf8_lossy(&valid_bytes).to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_buffer_split_utf8() {
        let mut buffer = SseBuffer::new();

        // "心" in UTF-8 is [0xE5, 0xBF, 0x83]
        let part1 = [0xE5, 0xBF];
        let part2 = [0x83];

        let text1 = buffer.push_and_get_text(&part1).unwrap();
        assert_eq!(text1, "");
        assert_eq!(buffer.buffer.len(), 2);

        let text2 = buffer.push_and_get_text(&part2).unwrap();
        assert_eq!(text2, "心");
        assert_eq!(buffer.buffer.len(), 0);
    }

    #[test]
    fn test_sse_buffer_accepts_crlf_delimiter() {
        let mut buffer = SseBuffer::new();

        buffer
            .extend_from_slice(b"data: {\"delta\":\"hello\"}\r\n\r\n")
            .unwrap();

        let messages = buffer.extract_messages().unwrap();
        assert_eq!(messages, vec!["data: {\"delta\":\"hello\"}\r\n\r\n"]);
        assert_eq!(buffer.buffer.len(), 0);
    }

    #[test]
    fn test_sse_buffer_overflow() {
        let mut buffer = SseBuffer::with_capacity_limit(10);
        let data = vec![0u8; 11];
        let res = buffer.extend_from_slice(&data);
        assert!(res.is_err());
    }

    #[test]
    fn resolve_root_session_prefers_existing_stable_id() {
        let session = resolve_root_session_id(Some("session-123"), "native-ephemeral-1".into());
        assert_eq!(session, "session-123");
    }

    #[test]
    fn resolve_root_session_uses_fallback_when_missing() {
        let session = resolve_root_session_id(None, "native-ephemeral-1".into());
        assert_eq!(session, "native-ephemeral-1");
    }

    #[test]
    fn derive_child_session_keeps_root_family() {
        let child = derive_child_session_id("session-123", "vision");
        assert_eq!(child, "session-123::vision");
    }
}

/// Mask sensitive API keys for safe logging
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}

/// Validate API key format for critical providers
pub fn validate_api_key(key: &str, provider: &str) -> Result<()> {
    match provider.to_lowercase().as_str() {
        "openai" => {
            if !key.starts_with("sk-") && !key.starts_with("org-") {
                return Err(Error::ProviderAuth(
                    "Invalid OpenAI API key format (must start with sk-)".into(),
                ));
            }
            if key.len() < 20 {
                return Err(Error::ProviderAuth("OpenAI API key too short".into()));
            }
        }
        "anthropic" => {
            if !key.starts_with("sk-ant-") {
                return Err(Error::ProviderAuth(
                    "Invalid Anthropic API key format (must start with sk-ant-)".into(),
                ));
            }
        }
        "gemini" => {
            if key.len() < 30 {
                return Err(Error::ProviderAuth(
                    "Gemini API key appears invalid (too short)".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve the root session id for a provider request.
///
/// Providers must reuse the upstream session id whenever one exists. Only
/// requests with no upstream session authority should fall back to a
/// provider-local ephemeral root id.
pub fn resolve_root_session_id(existing: Option<&str>, fallback_root: String) -> String {
    existing
        .map(str::trim)
        .filter(|session| !session.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(fallback_root)
}

/// Derive a stable child session id from an existing root session id.
pub fn derive_child_session_id(root_session_id: &str, child_scope: &str) -> String {
    format!("{}::{}", root_session_id, child_scope.trim())
}

async fn load_media_bytes_from_url(url: &str) -> Result<Vec<u8>> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| Error::Internal(format!("Invalid media URL '{}': {}", url, e)))?;

    match parsed.scheme() {
        "file" => {
            let path = parsed.to_file_path().map_err(|_| {
                Error::Internal(format!(
                    "Unsupported local file URL for media source: {}",
                    url
                ))
            })?;
            let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
                Error::Internal(format!(
                    "Failed to stat local media file '{}': {}",
                    path.display(),
                    e
                ))
            })?;
            if metadata.len() > MAX_LOCAL_MEDIA_BYTES {
                return Err(Error::Internal(format!(
                    "Local media file '{}' is larger than the {}MB safety limit",
                    path.display(),
                    MAX_LOCAL_MEDIA_BYTES / 1024 / 1024
                )));
            }
            tokio::fs::read(&path).await.map_err(|e| {
                Error::Internal(format!(
                    "Failed to read local media file '{}': {}",
                    path.display(),
                    e
                ))
            })
        }
        "http" | "https" => {
            let response = reqwest::get(parsed).await.map_err(|e| {
                Error::Internal(format!("Failed to fetch remote media '{}': {}", url, e))
            })?;
            let response = response.error_for_status().map_err(|e| {
                Error::Internal(format!("Remote media request failed for '{}': {}", url, e))
            })?;
            if let Some(length) = response.content_length() {
                if length > MAX_REMOTE_MEDIA_BYTES {
                    return Err(Error::Internal(format!(
                        "Remote media '{}' is larger than the {}MB safety limit",
                        url,
                        MAX_REMOTE_MEDIA_BYTES / 1024 / 1024
                    )));
                }
            }
            let bytes = response.bytes().await.map_err(|e| {
                Error::Internal(format!("Failed to read remote media body '{}': {}", url, e))
            })?;
            if bytes.len() as u64 > MAX_REMOTE_MEDIA_BYTES {
                return Err(Error::Internal(format!(
                    "Remote media '{}' exceeded the {}MB safety limit while downloading",
                    url,
                    MAX_REMOTE_MEDIA_BYTES / 1024 / 1024
                )));
            }
            Ok(bytes.to_vec())
        }
        other => Err(Error::Internal(format!(
            "Unsupported media URL scheme '{}' for '{}'",
            other, url
        ))),
    }
}

pub async fn image_source_to_bytes(source: &ImageSource) -> Result<Vec<u8>> {
    match source {
        ImageSource::Base64 { data, .. } => base64::prelude::BASE64_STANDARD
            .decode(data)
            .map_err(|e| Error::Internal(format!("Base64 decode failed for image: {}", e))),
        ImageSource::Url { url } => load_media_bytes_from_url(url).await,
    }
}

pub async fn audio_source_to_bytes(source: &AudioSource) -> Result<Vec<u8>> {
    match source {
        AudioSource::Base64 { data, .. } => base64::prelude::BASE64_STANDARD
            .decode(data)
            .map_err(|e| Error::Internal(format!("Base64 decode failed for audio: {}", e))),
        AudioSource::Url { url } => load_media_bytes_from_url(url).await,
    }
}

pub async fn video_source_to_bytes(source: &VideoSource) -> Result<Vec<u8>> {
    match source {
        VideoSource::Base64 { data, .. } => base64::prelude::BASE64_STANDARD
            .decode(data)
            .map_err(|e| Error::Internal(format!("Base64 decode failed for video: {}", e))),
        VideoSource::Url { url } => load_media_bytes_from_url(url).await,
    }
}
