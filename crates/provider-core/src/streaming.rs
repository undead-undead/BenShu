use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;

use benshu_infra::error::{Error, Result};
use benshu_protocol_core::ToolCall;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
    Error,
    Other(String),
}

impl FinishReason {
    pub fn from_provider_reason(reason: impl AsRef<str>) -> Self {
        let normalized = reason.as_ref().trim().to_lowercase();
        match normalized.as_str() {
            "" => Self::Stop,
            "stop" | "end_turn" | "end" => Self::Stop,
            "tool_calls" | "tool_use" | "function_call" | "function_calls" => Self::ToolCalls,
            "length" | "max_tokens" | "max_output_tokens" => Self::Length,
            "content_filter" | "safety" | "blocked" => Self::ContentFilter,
            "cancelled" | "canceled" => Self::Cancelled,
            "error" | "failed" => Self::Error,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Stop => "stop",
            Self::ToolCalls => "tool_calls",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::Other(other) => other.as_str(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderTelemetry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<crate::ContinuationTelemetry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum StreamingChoice {
    Message(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ParallelToolCalls(HashMap<usize, ToolCall>),
    Thought(String),
    Usage(Usage),
    Finish(FinishReason),
    Telemetry(ProviderTelemetry),
    Done,
}

impl StreamingChoice {
    pub fn is_message(&self) -> bool {
        matches!(self, Self::Message(_))
    }

    pub fn is_tool_call(&self) -> bool {
        matches!(self, Self::ToolCall { .. } | Self::ParallelToolCalls(_))
    }

    pub fn is_done(&self) -> bool {
        matches!(self, Self::Done)
    }

    pub fn is_usage(&self) -> bool {
        matches!(self, Self::Usage(_))
    }

    pub fn is_finish(&self) -> bool {
        matches!(self, Self::Finish(_))
    }

    pub fn is_telemetry(&self) -> bool {
        matches!(self, Self::Telemetry(_))
    }

    pub fn is_thought(&self) -> bool {
        matches!(self, Self::Thought(_))
    }

    pub fn as_message(&self) -> Option<&str> {
        match self {
            Self::Message(s) => Some(s),
            _ => None,
        }
    }
}

pub type StreamingResult = Pin<Box<dyn Stream<Item = Result<StreamingChoice>> + Send>>;

pub struct StreamingResponse {
    inner: StreamingResult,
}

impl StreamingResponse {
    pub fn new(stream: StreamingResult) -> Self {
        Self { inner: stream }
    }

    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<StreamingChoice>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
        }
    }

    pub async fn collect_text(mut self) -> Result<String> {
        use futures::StreamExt;

        let mut result = String::new();
        while let Some(chunk) = self.inner.next().await {
            match chunk? {
                StreamingChoice::Message(text) => result.push_str(&text),
                StreamingChoice::Done => break,
                _ => {}
            }
        }
        Ok(result)
    }

    pub fn into_inner(self) -> StreamingResult {
        self.inner
    }
}

impl Stream for StreamingResponse {
    type Item = Result<StreamingChoice>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

pub struct MockStreamBuilder {
    chunks: Vec<Result<StreamingChoice>>,
}

impl Default for MockStreamBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MockStreamBuilder {
    pub fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    pub fn message(mut self, text: impl Into<String>) -> Self {
        self.chunks.push(Ok(StreamingChoice::Message(text.into())));
        self
    }

    pub fn thought(mut self, text: impl Into<String>) -> Self {
        self.chunks.push(Ok(StreamingChoice::Thought(text.into())));
        self
    }

    pub fn tool_call(
        mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        self.chunks.push(Ok(StreamingChoice::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }));
        self
    }

    pub fn messages(mut self, texts: Vec<impl Into<String>>) -> Self {
        for text in texts {
            self.chunks.push(Ok(StreamingChoice::Message(text.into())));
        }
        self
    }

    pub fn done(mut self) -> Self {
        self.chunks.push(Ok(StreamingChoice::Done));
        self
    }

    pub fn error(mut self, error: Error) -> Self {
        self.chunks.push(Err(error));
        self
    }

    pub fn usage(mut self, usage: Usage) -> Self {
        self.chunks.push(Ok(StreamingChoice::Usage(usage)));
        self
    }

    pub fn finish(mut self, finish_reason: FinishReason) -> Self {
        self.chunks.push(Ok(StreamingChoice::Finish(finish_reason)));
        self
    }

    pub fn telemetry(mut self, telemetry: ProviderTelemetry) -> Self {
        self.chunks.push(Ok(StreamingChoice::Telemetry(telemetry)));
        self
    }

    pub fn build(self) -> StreamingResponse {
        StreamingResponse::from_stream(futures::stream::iter(self.chunks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn streaming_response_collects_text() {
        let stream = MockStreamBuilder::new()
            .message("Hello, ")
            .message("world!")
            .done()
            .build();

        let text = stream.collect_text().await.expect("collect should succeed");
        assert_eq!(text, "Hello, world!");
    }

    #[tokio::test]
    async fn streaming_response_iterates_chunks() {
        let mut stream = MockStreamBuilder::new()
            .message("chunk1")
            .message("chunk2")
            .done()
            .build();

        let mut messages = Vec::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamingChoice::Message(text)) = chunk {
                messages.push(text);
            }
        }

        assert_eq!(messages, vec!["chunk1", "chunk2"]);
    }
}
