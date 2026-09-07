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
/// Total connections we allow against a single host, across every download.
/// Servers cap connections per client ip, so two downloads each opening eight
/// makes both of them slow instead of making either one fast.
const DEFAULT_HOST_CONNECTIONS: usize = 8;
/// Downloads allowed to run at once against a single host. One is what curl
/// and pyload settle on: a server caps connections per client anyway, so two
/// downloads sharing that cap finish no sooner than one after the other, and
/// both look broken while they crawl. Files from different hosts still run in
/// parallel up to `max_concurrent`.
const DEFAULT_DOWNLOADS_PER_HOST: usize = 1;

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
    Progress {
        id: i64,
        downloaded: u64,
        total: Option<u64>,
        bytes_per_sec: u64,
        segments: u16,
    },
    Started {
        id: i64,
        file_name: String,
        total: Option<u64>,
    },
    Finished {
        id: i64,
        path: String,
    },
    Failed {
        id: i64,
        error: String,
    },
    Paused {
        id: i64,
    },
    Queued {
        id: i64,
    },
    Removed {
        id: i64,
    },
    /// A browser download is waiting for the user to confirm it.
    Confirm {
        id: i64,
        url: String,
        file_name: String,
        out_dir: String,
    },
    /// Probe finished for a pending download: real name, size and type.
    ConfirmDetails {
        id: i64,
        file_name: String,
        total: Option<u64>,
        mime: Option<String>,
        resumable: bool,
    },
}

/// A download currently in flight.
struct Active {
    handle: Arc<DownloadHandle>,
    /// Set when the user asked to pause, so we do not mark it failed.
    pausing: Arc<AtomicBool>,
}

/// Best guess at a name before we have probed the server, just for the prompt.
fn file_name_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|s| s.filter(|p| !p.is_empty()).next_back())
                .map(String::from)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "download".into())
}

pub struct Manager {
    store: Arc<Store>,
    client: reqwest::Client,
    active: Arc<Mutex<HashMap<i64, Active>>>,
    events: mpsc::UnboundedSender<Event>,
    max_concurrent: Arc<Mutex<usize>>,
    /// How many downloads are currently running against each host.
    active_hosts: Arc<Mutex<HashMap<String, usize>>>,
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
            active_hosts: Arc::new(Mutex::new(HashMap::new())),
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

    /// Add a download that will not start until `confirm` is called.
    /// Used for links handed over by the browser, so the user sees what is
    /// about to happen and where it will be saved.
    pub fn add_pending(self: &Arc<Self>, d: &NewDownload) -> Result<i64> {
        let id = self.store.insert_download_with_status(d, status::PENDING)?;
        let name = d
            .file_name
            .clone()
            .unwrap_or_else(|| file_name_from_url(&d.url));
        let _ = self.events.send(Event::Confirm {
            id,
            url: d.url.clone(),
            file_name: name,
            out_dir: d.out_dir.clone(),
        });

        // Fill in the real details behind the prompt.
        self.probe_pending(
            id,
            DownloadSpec {
                url: d.url.clone(),
                out_dir: d.out_dir.clone(),
                file_name: d.file_name.clone(),
                headers: d.headers.clone(),
                segments: d.segments.unwrap_or(8),
            },
        );

        Ok(id)
    }

    /// Ask the server what this file actually is, without downloading it.
    /// Runs while the confirmation prompt is open so the user sees a real
    /// name and size instead of a guess from the url.
    fn probe_pending(self: &Arc<Self>, id: i64, spec: DownloadSpec) {
        let me = self.clone();
        tokio::spawn(async move {
            match velo_core::probe::probe(&me.client, &spec).await {
                Ok(info) => {
                    let _ = me.store.apply_probe(
                        id,
                        &info.final_url,
                        &info.file_name,
                        "",
                        info.total_size,
                        info.supports_range,
                        info.etag.as_deref(),
                        info.last_modified.as_deref(),
                        info.mime.as_deref(),
                    );
                    let resumable = info.resumable();
                    let _ = me.events.send(Event::ConfirmDetails {
                        id,
                        file_name: info.file_name,
                        total: info.total_size,
                        mime: info.mime,
                        resumable,
                    });
                }
                Err(e) => {
                    tracing::warn!("could not probe pending download {id}: {e}");
                }
            }
        });
    }

    /// The user answered a confirmation prompt.
    pub fn confirm(&self, id: i64, start: bool) -> Result<()> {
        if start {
            self.store.set_status(id, status::QUEUED, None)?;
            let _ = self.events.send(Event::Queued { id });
        } else {
            self.store.delete(id)?;
            let _ = self.events.send(Event::Removed { id });
        }
        Ok(())
    }

    /// Connections for one download. The whole host budget goes to it, because
    /// only one download per host runs at a time.
    fn segments_for_host(&self, host: Option<&str>, requested: u8) -> u8 {
        let Some(host) = host else {
            return requested;
        };
        let budget = self
            .store
            .get_host_limit(host)
            .ok()
            .flatten()
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_HOST_CONNECTIONS)
            .clamp(1, 32);
        (budget as u8).min(requested)
    }

    /// True when this host already has as many downloads running as we allow.
    /// The download stays queued and starts the moment a slot frees up.
    fn host_is_busy(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return false;
        };
        self.active_hosts.lock().get(host).copied().unwrap_or(0) >= DEFAULT_DOWNLOADS_PER_HOST
    }

    /// Whether browser downloads should ask first. Defaults to yes.
    pub fn confirm_browser_downloads(&self) -> bool {
        self.store
            .get_setting("confirm_downloads")
            .ok()
            .flatten()
            .map(|v| v != "false")
            .unwrap_or(true)
    }

    pub fn set_confirm_browser_downloads(&self, on: bool) -> Result<()> {
        self.store
            .set_setting("confirm_downloads", if on { "true" } else { "false" })?;
        Ok(())
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

    pub async fn remove(&self, id: i64, delete_file: bool) -> Result<()> {
        let was_running = {
            let active = self.active.lock();
            match active.get(&id) {
                Some(a) => {
                    a.pausing.store(true, Ordering::Release);
                    a.handle.stop();
                    true
                }
                None => false,
            }
        };

        // Windows will not delete a file that is still open, and the workers
        // need a moment to notice the stop flag and drop their handles.
        if was_running {
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if !self.active.lock().contains_key(&id) {
                    break;
                }
            }
        }

        // A finished file is the user's; only they say when it goes. Anything
        // unfinished is a partial file that is useless on its own, so removing
        // the download takes the bytes with it.
        if let Ok(row) = self.store.get(id) {
            let finished = row.status == status::COMPLETED;
            if delete_file || !finished {
                if let Some(p) = row.path.filter(|p| !p.is_empty()) {
                    if let Err(e) = std::fs::remove_file(&p) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            tracing::warn!("could not delete {p}: {e}");
                        }
                    }
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
        // Ask for more than we can start: rows whose host is already busy get
        // skipped, and we want to reach the ones behind them.
        for row in self.store.next_queued(free * 8 + 8)? {
            if self.active.lock().len() >= max {
                break;
            }
            if self.active.lock().contains_key(&row.id) {
                continue;
            }
            // One download per host at a time. The rest stay queued and start
            // the moment that host frees up, each getting the full connection
            // budget instead of a slice of it.
            let host = url::Url::parse(&row.url)
                .ok()
                .and_then(|u| u.host_str().map(String::from));
            if self.host_is_busy(host.as_deref()) {
                continue;
            }
            if let Err(e) = self.spawn_one(row.clone()).await {
                tracing::error!("failed to start {}: {e}", row.id);
                self.store
                    .set_status(row.id, status::FAILED, Some(&e.to_string()))?;
                let _ = self.events.send(Event::Failed {
                    id: row.id,
                    error: e.to_string(),
                });
            }
        }
        Ok(())
    }

    async fn spawn_one(self: &Arc<Self>, row: DownloadRow) -> Result<()> {
        self.store.set_status(row.id, status::RUNNING, None)?;

        // Respect what we learned about this host from an earlier 429.
        let host = url::Url::parse(&row.url)
            .ok()
            .and_then(|u| u.host_str().map(String::from));
        // Share the host's connection budget with anything already running
        // against it, so N downloads on one server do not open N times the
        // connections the server is willing to serve.
        let segments = self.segments_for_host(host.as_deref(), row.segments);
        if let Some(h) = &host {
            *self.active_hosts.lock().entry(h.clone()).or_insert(0) += 1;
        }

        let spec = DownloadSpec {
            url: row.url.clone(),
            out_dir: row.out_dir.clone(),
            // "…" is the placeholder we store before probing knows the real name.
            file_name: if row.file_name == "…" {
                None
            } else {
                Some(row.file_name.clone())
            },
            headers: row.headers.clone(),
            segments,
        };

        // Resume only if we have both a path and saved cursors.
        let saved = self.store.load_segments(row.id).unwrap_or_default();
        let resume = match (&row.path, saved.is_empty()) {
            // An empty path comes from the pre-confirmation probe, not a real
            // partial file, so it must never be treated as resumable.
            (Some(p), false) if !p.is_empty() => Some((PathBuf::from(p), saved)),
            _ => None,
        };

        let id = row.id;
        let events = self.events.clone();
        let store = self.store.clone();
        // Every download to the same host shares one connection budget.
        let handle = download(
            &self.client,
            spec,
            resume,
            move |p: ProgressSnapshot| match p.status {
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
            },
        )
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
        self.active.lock().insert(
            id,
            Active {
                handle: handle.clone(),
                pausing: pausing.clone(),
            },
        );

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
                        let _ = me
                            .store
                            .set_status(id, status::FAILED, Some("download failed"));
                        let _ = me.events.send(Event::Failed {
                            id,
                            error: "download failed".into(),
                        });
                    }
                    _ => {}
                }
                break;
            }
            me.active.lock().remove(&id);
            if let Some(h) = &host {
                let mut hosts = me.active_hosts.lock();
                if let Some(n) = hosts.get_mut(h) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        hosts.remove(h);
                    }
                }
            }
        });

        Ok(())
    }
}
