use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait SystemEnvironment: Send + Sync {
    /// Materialize an environment (e.g. via pixi/uv)
    async fn provision(
        &self,
        id: &str,
        dependencies: &[String],
        use_browser: bool,
    ) -> anyhow::Result<PathBuf>;

    /// Path where models should be stored for this ID
    fn models_path(&self, id: &str) -> PathBuf;

    /// Ensure models are present
    async fn provision_models(
        &self,
        id: &str,
        models: &[crate::skill::ModelSpec],
    ) -> anyhow::Result<PathBuf>;

    /// Ensure `uv` is available
    async fn ensure_uv(&self) -> anyhow::Result<PathBuf>;

    /// Ensure `pixi` is available
    async fn ensure_pixi(&self) -> anyhow::Result<PathBuf>;

    /// Ensure `bun` is available
    async fn ensure_bun(&self) -> anyhow::Result<PathBuf>;

    /// Ensure `git` is available
    async fn ensure_git(&self) -> anyhow::Result<PathBuf>;

    /// Ensure `gcc` is available
    async fn ensure_gcc(&self) -> anyhow::Result<PathBuf>;
}
