use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What the caller asks us to download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSpec {
    pub url: String,
    /// Directory the final file lands in.
    pub out_dir: String,
    /// Optional forced file name. When None we detect it.
    pub file_name: Option<String>,
    /// Extra headers (cookies, referer, user-agent) forwarded from the browser.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Requested number of segments. Clamped to 1..=MAX_SEGMENTS.
    #[serde(default = "default_segments")]
    pub segments: u8,
}

fn default_segments() -> u8 {
    8
}

pub const MAX_SEGMENTS: u8 = 32;
/// Below this size splitting costs more than it gains.
pub const MIN_SPLIT_BYTES: u64 = 1024 * 1024;

/// Result of asking the server what we are dealing with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    /// URL after redirects. We download from this one.
    pub final_url: String,
    pub file_name: String,
    pub total_size: Option<u64>,
    pub supports_range: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime: Option<String>,
}

impl ProbeResult {
    pub fn resumable(&self) -> bool {
        self.supports_range && self.total_size.is_some()
    }
}

/// One byte range owned by one worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub index: u16,
    pub start: u64,
    /// Exclusive end. Shrinks when another worker steals our tail.
    pub end: u64,
    /// Absolute offset of the next byte we will write.
    pub cursor: u64,
}

impl SegmentState {
    pub fn new(index: u16, start: u64, end: u64) -> Self {
        Self { index, start, end, cursor: start }
    }
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.cursor)
    }
    pub fn done(&self) -> bool {
        self.cursor >= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Probing,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Live numbers for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub bytes_per_sec: u64,
    pub active_segments: u16,
    pub status: DownloadStatus,
}
