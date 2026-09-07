//! Velo desktop shell. Owns the store and manager, forwards manager events to
//! the webview, and exposes the command surface in `commands`.

mod api;
mod commands;

use commands::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager as _};
use velo_core::paths;
use velo_manager::Manager;
use velo_store::Store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            paths::ensure_dirs()?;
            let store = Arc::new(Store::open(paths::database_path())?);
            let (manager, mut events) = Manager::new(store)?;
            // The scheduler spawns tasks, so it must start inside Tauri's
            // tokio runtime rather than on the setup thread.
            {
                let mgr = manager.clone();
                tauri::async_runtime::block_on(async move { mgr.start() })?;
            }

            // One channel of truth: manager events become webview events.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = events.recv().await {
                    // A prompt the user cannot see is useless, so surface the
                    // window whenever one arrives.
                    if matches!(ev, velo_manager::Event::Confirm { .. }) {
                        if let Some(w) = handle.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    if let Err(err) = handle.emit("velo://event", &ev) {
                        tracing::warn!("failed to emit event: {err}");
                    }
                }
            });

            // Local HTTP API for the browser extension.
            let api_mgr = manager.clone();
            let api_handle = app.handle().clone();
            let ctrl_handle = app.handle().clone();

            let (info, control) = tauri::async_runtime::block_on(async move {
                let notify_handle = api_handle.clone();
                api::start(api_mgr, move |req| {
                    // Bring the window forward so the prompt is not missed.
                    if let Some(w) = notify_handle.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                    let _ = notify_handle.emit("velo://pair-request", &req);
                })
                .await
            })?;

            tracing::info!("extension can connect on port {}", info.port);
            let _ = ctrl_handle.emit("velo://api-ready", &info);

            app.manage(control);
            app.manage(AppState { manager });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_download,
            commands::add_batch,
            commands::list_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::remove_download,
            commands::get_settings,
            commands::set_max_concurrent,
            commands::confirm_download,
            commands::set_confirm_downloads,
            commands::answer_pairing,
            commands::pending_pairing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Velo");
}
