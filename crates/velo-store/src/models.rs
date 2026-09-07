use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A row in `downloads`, as the UI and scheduler see it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRow {
    pub id: i64,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub out_dir: String,
    pub path: Option<String>,
    pub total_size: Option<i64>,
    pub downloaded: i64,
    pub supports_range: bool,
    pub segments: u8,
    pub headers: HashMap<String, String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub priority: i64,
    pub batch_id: Option<i64>,
    pub scheduled_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

/// What the UI or the browser extension sends us to enqueue something.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDownload {
    pub url: String,
    pub out_dir: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub segments: Option<u8>,
    #[serde(default)]
    pub batch_id: Option<i64>,
    #[serde(default)]
    pub scheduled_at: Option<i64>,
    #[serde(default)]
    pub priority: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRow {
    pub id: i64,
    pub name: String,
    pub source_page: Option<String>,
    pub created_at: i64,
}

/// Status strings kept in one place so SQL and Rust never drift apart.
pub mod status {
    /// Came from the browser and is waiting for the user to confirm it.
    /// The scheduler ignores these, so nothing downloads until they say yes.
    pub const PENDING: &str = "pending";
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const PAUSED: &str = "paused";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}
