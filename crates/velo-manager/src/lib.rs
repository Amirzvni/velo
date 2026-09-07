//! The scheduler. Owns the queue, keeps `max_concurrent` downloads running,
//! and starts the next one the moment a slot frees up.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use velo_core::engine::{download, DownloadHandle};
use velo_core::types::{DownloadSpec, DownloadStatus, ProgressSnapshot};
use velo_core::{build_client, DEFAULT_USER_AGENT};
use velo_store::models::{status, DownloadRow, NewDownload};
use velo_store::Store;

/// How many downloads run at once by default. Feature 3 wants 4.
pub const DEFAULT_MAX_CONCURRENT: usize = 4;
/// How often we write segment cursors to disk so a crash costs little.
const JOURNAL_INTERVAL: Duration = Duration::from_millis(1000);
/// How often the scheduler looks for work when idle.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error(transparent)]
    Store(#[from] velo_store::StoreError),
    #[error(transparent)]
    Core(#[from] velo_core::VeloError),
}

pub type Result<T> = std::result::Result<T, ManagerError>;

/// Pushed to the UI whenever something changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Progress { id: i64, downloaded: u64, total: Option<u64>, bytes_per_sec: u64, segments: u16 },
    Started { id: i64, file_name: String, total: Option<u64> },
    Finished { id: i64, path: String },
    Failed { id: i64, error: String },
    Paused { id: i64 },
    Queued { id: i64 },
    Removed { id: i64 },
}

/// A download currently in flight.
struct Active {
    handle: Arc<DownloadHandle>,
    /// Set when the user asked to pause, so we do not mark it failed.
    pausing: Arc<AtomicBool>,
}

pub struct Manager {
    store: Arc<Store>,
    client: reqwest::Client,
    active: Arc<Mutex<HashMap<i64, Active>>>,
    events: mpsc::UnboundedSender<Event>,
    max_concurrent: Arc<Mutex<usize>>,
}

impl Manager {
    pub fn new(store: Arc<Store>) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Event>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let max = store
            .get_setting("max_concurrent")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONCURRENT);

        let mgr = Arc::new(Self {
            store,
            client: build_client(DEFAULT_USER_AGENT)?,
            active: Arc::new(Mutex::new(HashMap::new())),
            events: tx,
            max_concurrent: Arc::new(Mutex::new(max)),
        });
        Ok((mgr, rx))
    }

    /// Put anything the last run left mid-flight back in the queue, then start
    /// the loop that keeps slots full.
    pub fn start(self: &Arc<Self>) -> Result<()> {
        let n = self.store.recover_running()?;
        if n > 0 {
            tracing::info!("requeued {n} downloads left running by a previous session");
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = me.fill_slots().await {
                    tracing::error!("scheduler: {e}");
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        });
        Ok(())
    }

    pub fn add(&self, d: &NewDownload) -> Result<i64> {
        let id = self.store.insert_download(d)?;
        let _ = self.events.send(Event::Queued { id });
        Ok(id)
    }

    /// Enqueue many links at once. This is what "download all links" calls.
    pub fn add_batch(
        &self,
        name: &str,
        source_page: Option<&str>,
        items: &[NewDownload],
    ) -> Result<(i64, Vec<i64>)> {
        let batch_id = self.store.create_batch(name, source_page)?;
        let mut ids = Vec::with_capacity(items.len());
        for item in items {
            let mut item = item.clone();
            item.batch_id = Some(batch_id);
            ids.push(self.add(&item)?);
        }
        Ok((batch_id, ids))
    }

    pub fn set_max_concurrent(&self, n: usize) -> Result<()> {
        let n = n.clamp(1, 16);
        *self.max_concurrent.lock() = n;
        self.store.set_setting("max_concurrent", &n.to_string())?;
        Ok(())
    }

    pub fn max_concurrent(&self) -> usize {
        *self.max_concurrent.lock()
    }

    pub fn pause(&self, id: i64) -> Result<()> {
        if let Some(a) = self.active.lock().get(&id) {
            a.pausing.store(true, Ordering::Release);
            a.handle.stop();
        } else {
            // Not running yet: just take it out of the queue.
            self.store.set_status(id, status::PAUSED, None)?;
            let _ = self.events.send(Event::Paused { id });
        }
        Ok(())
    }

    pub fn resume(&self, id: i64) -> Result<()> {
        self.store.set_status(id, status::QUEUED, None)?;
        let _ = self.events.send(Event::Queued { id });
        Ok(())
    }

    pub fn remove(&self, id: i64, delete_file: bool) -> Result<()> {
        if let Some(a) = self.active.lock().get(&id) {
            a.pausing.store(true, Ordering::Release);
            a.handle.stop();
        }
        if delete_file {
            if let Ok(row) = self.store.get(id) {
                if let Some(p) = row.path {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        self.store.delete(id)?;
        let _ = self.events.send(Event::Removed { id });
        Ok(())
    }

    pub fn list(&self, filter: Option<&str>) -> Result<Vec<DownloadRow>> {
        Ok(self.store.list(filter)?)
    }

    /// Start downloads until we hit the concurrency limit.
    async fn fill_slots(self: &Arc<Self>) -> Result<()> {
        let max = *self.max_concurrent.lock();
        let running = self.active.lock().len();
        if running >= max {
            return Ok(());
        }
        let free = max - running;
        for row in self.store.next_queued(free)? {
            if self.active.lock().contains_key(&row.id) {
                continue;
            }
            if let Err(e) = self.spawn_one(row.clone()).await {
                tracing::error!("failed to start {}: {e}", row.id);
                self.store.set_status(row.id, status::FAILED, Some(&e.to_string()))?;
                let _ = self.events.send(Event::Failed { id: row.id, error: e.to_string() });
            }
        }
        Ok(())
    }

    async fn spawn_one(self: &Arc<Self>, row: DownloadRow) -> Result<()> {
        self.store.set_status(row.id, status::RUNNING, None)?;

        // Respect what we learned about this host from an earlier 429.
        let host = url::Url::parse(&row.url).ok().and_then(|u| u.host_str().map(String::from));
        let segments = match &host {
            Some(h) => self
                .store
                .get_host_limit(h)
                .ok()
                .flatten()
                .map(|lim| (lim as u8).min(row.segments))
                .unwrap_or(row.segments),
            None => row.segments,
        };

        let spec = DownloadSpec {
            url: row.url.clone(),
            out_dir: row.out_dir.clone(),
            // "…" is the placeholder we store before probing knows the real name.
            file_name: if row.file_name == "…" { None } else { Some(row.file_name.clone()) },
            headers: row.headers.clone(),
            segments,
        };

        // Resume only if we have both a path and saved cursors.
        let saved = self.store.load_segments(row.id).unwrap_or_default();
        let resume = match (&row.path, saved.is_empty()) {
            (Some(p), false) => Some((PathBuf::from(p), saved)),
            _ => None,
        };

        let id = row.id;
        let events = self.events.clone();
        let store = self.store.clone();
        let handle = download(&self.client, spec, resume, move |p: ProgressSnapshot| {
            match p.status {
                DownloadStatus::Running => {
                    let _ = events.send(Event::Progress {
                        id,
                        downloaded: p.downloaded,
                        total: p.total,
                        bytes_per_sec: p.bytes_per_sec,
                        segments: p.active_segments,
                    });
                    let _ = store.set_progress(id, p.downloaded);
                }
                _ => {
                    let _ = store.set_progress(id, p.downloaded);
                }
            }
        })
        .await?;

        let handle = Arc::new(handle);
        self.store.apply_probe(
            id,
            &handle.probe.final_url,
            &handle.probe.file_name,
            &handle.path.to_string_lossy(),
            handle.probe.total_size,
            handle.probe.supports_range,
            handle.probe.etag.as_deref(),
            handle.probe.last_modified.as_deref(),
            handle.probe.mime.as_deref(),
        )?;
        let _ = self.events.send(Event::Started {
            id,
            file_name: handle.probe.file_name.clone(),
            total: handle.probe.total_size,
        });

        let pausing = Arc::new(AtomicBool::new(false));
        self.active.lock().insert(id, Active { handle: handle.clone(), pausing: pausing.clone() });

        // Watcher: journals segment cursors, then settles the final status and
        // frees the slot so the next queued download can start.
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(JOURNAL_INTERVAL).await;
                let snap = handle.snapshot();
                let _ = me.store.save_segments(id, &handle.segment_states());

                match snap.status {
                    DownloadStatus::Running => continue,
                    DownloadStatus::Completed => {
                        let _ = me.store.set_status(id, status::COMPLETED, None);
                        let _ = me.store.set_progress(id, snap.downloaded);
                        let _ = me.events.send(Event::Finished {
                            id,
                            path: handle.path.to_string_lossy().into_owned(),
                        });
                    }
                    DownloadStatus::Paused => {
                        let s = if pausing.load(Ordering::Acquire) {
                            status::PAUSED
                        } else {
                            status::QUEUED
                        };
                        let _ = me.store.set_status(id, s, None);
                        let _ = me.events.send(Event::Paused { id });
                    }
                    DownloadStatus::Failed => {
                        // Remember a lower connection cap for this host.
                        if let Some(h) = &host {
                            let c = handle.concurrency() as u32;
                            if c < segments as u32 {
                                let _ = me.store.set_host_limit(h, c.max(1));
                            }
                        }
                        let _ = me.store.set_status(id, status::FAILED, Some("download failed"));
                        let _ = me
                            .events
                            .send(Event::Failed { id, error: "download failed".into() });
                    }
                    _ => {}
                }
                break;
            }
            me.active.lock().remove(&id);
        });

        Ok(())
    }
}
