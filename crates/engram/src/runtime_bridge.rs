use std::future::Future;

/// Run async work from sync code without panicking when already inside Tokio.
pub(crate) fn block_on_sync<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create temporary tokio runtime")
            .block_on(future),
    }
}
