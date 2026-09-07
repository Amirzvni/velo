//! Everything the UI is allowed to ask the backend to do.
//! Each function is exposed over Tauri's IPC; nothing else is.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use velo_core::paths;
use velo_manager::Manager;
use velo_store::models::{status, DownloadRow, NewDownload};

pub struct AppState {
    pub manager: Arc<Manager>,
}

/// Tauri needs a String error; we never leak internals beyond the message.
type CmdResult<T> = Result<T, String>;

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

#[derive(Debug, Deserialize)]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub out_dir: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub segments: Option<u8>,
    #[serde(default)]
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Settings {
    pub max_concurrent: usize,
    pub default_dir: String,
    pub default_segments: u8,
    pub confirm_downloads: bool,
}

#[tauri::command]
pub fn add_download(state: tauri::State<AppState>, req: AddRequest) -> CmdResult<i64> {
    let item = NewDownload {
        url: req.url,
        out_dir: req
            .out_dir
            .unwrap_or_else(|| paths::default_download_dir().to_string_lossy().into_owned()),
        file_name: req.file_name,
        headers: req.headers,
        segments: req.segments,
        batch_id: None,
        scheduled_at: req.scheduled_at,
        priority: None,
    };
    state.manager.add(&item).map_err(e)
}

/// Used by "download all links": one call, many urls, all in one batch.
#[tauri::command]
pub fn add_batch(
    state: tauri::State<AppState>,
    name: String,
    source_page: Option<String>,
    items: Vec<AddRequest>,
) -> CmdResult<Vec<i64>> {
    let default_dir = paths::default_download_dir().to_string_lossy().into_owned();
    let list: Vec<NewDownload> = items
        .into_iter()
        .map(|r| NewDownload {
            url: r.url,
            out_dir: r.out_dir.unwrap_or_else(|| default_dir.clone()),
            file_name: r.file_name,
            headers: r.headers,
            segments: r.segments,
            batch_id: None,
            scheduled_at: r.scheduled_at,
            priority: None,
        })
        .collect();
    state
        .manager
        .add_batch(&name, source_page.as_deref(), &list)
        .map(|(_, ids)| ids)
        .map_err(e)
}

#[tauri::command]
pub fn list_downloads(
    state: tauri::State<AppState>,
    filter: Option<String>,
) -> CmdResult<Vec<DownloadRow>> {
    // Only accept the statuses we actually define.
    let f = match filter.as_deref() {
        Some(status::QUEUED) => Some(status::QUEUED),
        Some(status::RUNNING) => Some(status::RUNNING),
        Some(status::PAUSED) => Some(status::PAUSED),
        Some(status::COMPLETED) => Some(status::COMPLETED),
        Some(status::FAILED) => Some(status::FAILED),
        _ => None,
    };
    state.manager.list(f).map_err(e)
}

#[tauri::command]
pub fn pause_download(state: tauri::State<AppState>, id: i64) -> CmdResult<()> {
    state.manager.pause(id).map_err(e)
}

#[tauri::command]
pub fn resume_download(state: tauri::State<AppState>, id: i64) -> CmdResult<()> {
    state.manager.resume(id).map_err(e)
}

#[tauri::command]
pub fn remove_download(state: tauri::State<AppState>, id: i64, delete_file: bool) -> CmdResult<()> {
    state.manager.remove(id, delete_file).map_err(e)
}

#[tauri::command]
pub fn get_settings(state: tauri::State<AppState>) -> CmdResult<Settings> {
    Ok(Settings {
        max_concurrent: state.manager.max_concurrent(),
        default_dir: paths::default_download_dir().to_string_lossy().into_owned(),
        default_segments: 8,
        confirm_downloads: state.manager.confirm_browser_downloads(),
    })
}

#[tauri::command]
pub fn set_max_concurrent(state: tauri::State<AppState>, n: usize) -> CmdResult<()> {
    state.manager.set_max_concurrent(n).map_err(e)
}

/// The user answered the "start this download?" prompt.
#[tauri::command]
pub fn confirm_download(state: tauri::State<AppState>, id: i64, start: bool) -> CmdResult<()> {
    state.manager.confirm(id, start).map_err(e)
}

#[tauri::command]
pub fn set_confirm_downloads(state: tauri::State<AppState>, on: bool) -> CmdResult<()> {
    state.manager.set_confirm_browser_downloads(on).map_err(e)
}

/// The user clicked Allow or Deny on the browser pairing prompt.
#[tauri::command]
pub fn answer_pairing(ctrl: tauri::State<crate::api::PairControl>, allow: bool) -> CmdResult<bool> {
    Ok(ctrl.answer(allow))
}

/// If a prompt was already waiting when the UI loaded, show it.
#[tauri::command]
pub fn pending_pairing(
    ctrl: tauri::State<crate::api::PairControl>,
) -> CmdResult<Option<crate::api::PairRequest>> {
    Ok(ctrl.peek())
}
