use std::time::Duration;

use async_trait::async_trait;
use benshu_hardness::{
    ComplexityEstimator as CoreComplexityEstimator, MediaKind, MessageSnapshot,
    SemanticComplexityAnalyzer,
};

pub use benshu_hardness::ComplexityScore;

use crate::agent::message::{Content, ContentPart, Message};
use crate::agent::multi_agent::MultiAgent;

/// Thin brain-side adapter over the standalone `benshu-hardness` crate.
///
/// This preserves the existing public API that the rest of `brain` uses while
/// centralizing the actual hardness / complexity logic in its own crate.
pub struct ComplexityEstimator {
    core: CoreComplexityEstimator,
}

impl Default for ComplexityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityEstimator {
    pub fn new() -> Self {
        Self {
            core: CoreComplexityEstimator::new(),
        }
    }

    pub async fn estimate(
        &self,
        messages: &[Message],
        agent: Option<&dyn MultiAgent>,
    ) -> ComplexityScore {
        let snapshots: Vec<MessageSnapshot> = messages.iter().map(message_to_snapshot).collect();
        let analyzer = agent.map(MultiAgentComplexityAdapter::new);
        self.core
            .estimate(
                &snapshots,
                analyzer
                    .as_ref()
                    .map(|adapter| adapter as &dyn SemanticComplexityAnalyzer),
            )
            .await
    }

    pub fn current_usage(&self) -> f32 {
        self.core.current_usage()
    }
}

fn message_to_snapshot(message: &Message) -> MessageSnapshot {
    let media = match &message.content {
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image { .. } => Some(MediaKind::Image),
                ContentPart::Audio { .. } => Some(MediaKind::Audio),
                ContentPart::Video { .. } => Some(MediaKind::Video),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    MessageSnapshot {
        text: message.content.as_text(),
        media,
    }
}

struct MultiAgentComplexityAdapter<'a> {
    agent: &'a dyn MultiAgent,
}

impl<'a> MultiAgentComplexityAdapter<'a> {
    fn new(agent: &'a dyn MultiAgent) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl SemanticComplexityAnalyzer for MultiAgentComplexityAdapter<'_> {
    async fn analyze_complexity(&self, prompt: &str) -> Result<String, String> {
        tokio::time::timeout(
            Duration::from_secs(12),
            self.agent.analyze_complexity(prompt),
        )
        .await
        .map_err(|_| "Complexity L2 timed out".to_string())?
        .map_err(|error| error.to_string())
    }
}
