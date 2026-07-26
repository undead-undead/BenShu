//! BenShu — Unified AI Desktop Application.
use eframe::egui::Vec2;
use std::sync::Arc;

mod api;
mod app;
mod app_state;
mod common;
mod i18n;
mod ui;

use app::ClawPanel;
use benshu_gateway::LaunchOptions;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    // 0. Initialize global logging with BroadcastLayer bridge
    let (log_tx, _) = tokio::sync::broadcast::channel(100);
    let log_tx_for_tracing = log_tx.clone();
    let log_tx_for_gw = Some(log_tx);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with(common::logger::BroadcastLayer::new(log_tx_for_tracing))
        .init();

    // 1. Single Instance Protection (Windows)
    #[cfg(target_os = "windows")]
    let _instance = single_instance::SingleInstance::new("benshu-unified")
        .expect("Failed to create single instance");
    #[cfg(target_os = "windows")]
    if !_instance.is_single() {
        eprintln!("BenShu is already running.");
        return Ok(());
    }

    // 2. Setup Tokio Runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    let _guard = rt.enter();
    let handle = rt.handle().clone();

    // 3. Unified Path Resolution (Portable-First)
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();

    let is_system_restricted = cfg!(windows)
        && (exe_dir
            .to_string_lossy()
            .to_lowercase()
            .contains("program files")
            || exe_dir
                .to_string_lossy()
                .to_lowercase()
                .contains("windows\\system32"));

    let base_dir = if let Ok(data_dir) = std::env::var("BENSHU_DATA_DIR") {
        std::path::PathBuf::from(data_dir)
    } else if is_system_restricted {
        dirs::data_local_dir()
            .map(|d| d.join("benshu").join("data"))
            .unwrap_or_else(|| exe_dir.join("data"))
    } else {
        exe_dir.join("data")
    };
    std::env::set_var("BENSHU_DATA_DIR", &base_dir);

    // Ensure data directory exists
    if !base_dir.exists() {
        let _ = std::fs::create_dir_all(&base_dir);
    }

    // 4. Silent Handshaking — Generate a session-scoped secure token
    let session_token = std::env::var("BENSHU_SESSION_TOKEN").unwrap_or_else(|_| {
        let t = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        std::env::set_var("BENSHU_SESSION_TOKEN", &t);
        t
    });
    let token_for_ui = Some(session_token);

    // 5. Launch Gateway Engine in Background
    let engine_base_dir = base_dir.clone();
    handle.spawn(async move {
        tracing::info!(
            "Unified: Booting Gateway Engine at {}...",
            engine_base_dir.display()
        );
        // Note: launch_engine will see the BENSHU_SESSION_TOKEN env var we just set
        if let Err(e) = benshu_gateway::launch_engine(
            engine_base_dir,
            LaunchOptions::embedded(Some(3000)),
            log_tx_for_gw,
        )
        .await
        {
            tracing::error!("Gateway Engine failed: {}", e);
        }
    });

    // 6. GUI Options
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BenShu")
            .with_inner_size(Vec2::new(1024.0, 768.0))
            .with_resizable(true)
            .with_icon(Arc::new(eframe::egui::IconData::default())), // Load a real icon later
        ..Default::default()
    };

    // 7. Run Native egui
    eframe::run_native(
        "benshu-unified",
        options,
        Box::new(move |cc| Ok(Box::new(ClawPanel::new(cc, handle, token_for_ui)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // WASM implementation stays minimal as it doesn't run the backend locally
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("the_canvas_id"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("Failed to find canvas element");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(ClawPanel::new(cc)))),
            )
            .await
            .expect("Failed to start eframe");
    });
}
