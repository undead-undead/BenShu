#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task(
    rt: &tokio::runtime::Handle,
    future: impl std::future::Future<Output = ()> + Send + 'static,
) {
    rt.spawn(future);
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
pub fn spawn_task(future: impl std::future::Future<Output = ()> + 'static) {
    spawn_local(future);
}
