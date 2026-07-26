use crate::skill::{SkillExecutionConfig, SkillMetadata};
use crate::traits::env::SystemEnvironment;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

/// The unified execution abstraction for all skill runtimes.
#[async_trait]
pub trait SkillRuntime: Send + Sync {
    async fn execute(
        &self,
        metadata: &SkillMetadata,
        arguments: &str,
        base_dir: &Path,
        config: &SkillExecutionConfig,
        env_manager: Option<&Arc<dyn SystemEnvironment>>,
    ) -> anyhow::Result<std::process::Output>;
}
