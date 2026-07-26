use std::io::Write;
use std::path::PathBuf;

pub(super) async fn atomic_write_file(path: PathBuf, content: String) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            anyhow::anyhow!("atomic write target has no parent: {}", path.display())
        })?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file_mut().sync_all()?;
        temporary
            .persist(&path)
            .map_err(|error| anyhow::anyhow!(error.error))?;
        Ok(())
    })
    .await
    .map_err(|error| anyhow::anyhow!("atomic write worker failed: {error}"))??;
    Ok(())
}
