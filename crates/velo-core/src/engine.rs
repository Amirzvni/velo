use crate::error::{Result, VeloError};
use crate::fsutil::{create_preallocated, unique_path};
use crate::probe::probe;
use crate::segment::plan_segments;
use crate::types::{DownloadSpec, DownloadStatus, ProbeResult, ProgressSnapshot, SegmentState};
use crate::worker::{run_segment, SegmentHandle, WorkerCtx};
use parking_lot::Mutex;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

/// Smallest tail worth stealing. Below this the extra request costs more.
const MIN_STEAL_BYTES: u64 = 2 * 1024 * 1024;
/// How often we look for a stall / publish progress.
const TICK: Duration = Duration::from_millis(500);
/// Times we retry a segment on a normal error before giving up.
const MAX_RETRIES: u32 = 5;
/// Rate limiting is the server's problem, not a fault, so it gets its own budget.
const MAX_RATE_LIMIT_RETRIES: u32 = 30;

pub struct DownloadHandle {
    pub probe: ProbeResult,
    pub path: PathBuf,
    total_written: Arc<AtomicU64>,
    segments: Arc<Mutex<Vec<Arc<SegmentHandle>>>>,
    stop_tx: watch::Sender<bool>,
    status: Arc<Mutex<DownloadStatus>>,
    speed: Arc<AtomicU64>,
    concurrency: Arc<AtomicU64>,
}

impl DownloadHandle {
    pub fn snapshot(&self) -> ProgressSnapshot {
        let segs = self.segments.lock();
        ProgressSnapshot {
            downloaded: self.total_written.load(Ordering::Relaxed),
            total: self.probe.total_size,
            bytes_per_sec: self.speed.load(Ordering::Relaxed),
            active_segments: segs
                .iter()
                .filter(|s| !s.finished.load(Ordering::Acquire))
                .count() as u16,
            status: *self.status.lock(),
        }
    }

    /// Connection limit we settled on. Useful to remember per host later.
    pub fn concurrency(&self) -> u64 {
        self.concurrency.load(Ordering::Relaxed)
    }

    /// Byte ranges as they stand right now, for writing to the resume journal.
    pub fn segment_states(&self) -> Vec<SegmentState> {
        self.segments
            .lock()
            .iter()
            .map(|s| SegmentState {
                index: s.index,
                start: s.start.load(Ordering::Acquire),
                end: s.end.load(Ordering::Acquire),
                cursor: s.cursor.load(Ordering::Acquire),
            })
            .collect()
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }
}

/// Start a download and run it to completion.
/// `resume` carries segment state from a previous run, if any.
pub async fn download(
    client: &Client,
    spec: DownloadSpec,
    resume: Option<(PathBuf, Vec<SegmentState>)>,
    on_progress: impl Fn(ProgressSnapshot) + Send + 'static,
) -> Result<DownloadHandle> {
    let info = probe(client, &spec).await?;

    let (path, planned) = match resume {
        Some((p, segs)) if !segs.is_empty() => (p, segs),
        _ => {
            let dir = PathBuf::from(&spec.out_dir);
            std::fs::create_dir_all(&dir)?;
            let p = unique_path(&dir, &info.file_name);
            let segs = plan_segments(info.total_size, spec.segments, info.supports_range);
            (p, segs)
        }
    };

    let file = Arc::new(create_preallocated(&path, info.total_size)?);
    let already: u64 = planned.iter().map(|s| s.cursor - s.start).sum();
    let total_written = Arc::new(AtomicU64::new(already));

    let handles: Vec<Arc<SegmentHandle>> = planned
        .iter()
        .map(|s| Arc::new(SegmentHandle::new(s.index, s.start, s.end, s.cursor)))
        .collect();
    let segment_count = handles.len();
    let segments = Arc::new(Mutex::new(handles));

    let (stop_tx, stop_rx) = watch::channel(false);
    let status = Arc::new(Mutex::new(DownloadStatus::Running));
    let speed = Arc::new(AtomicU64::new(0));
    let permits = Arc::new(Semaphore::new(segment_count));
    let concurrency = Arc::new(AtomicU64::new(segment_count as u64));

    let ctx = Arc::new(WorkerCtx {
        client: client.clone(),
        spec: spec.clone(),
        url: info.final_url.clone(),
        file: file.clone(),
        total_written: total_written.clone(),
        permits: permits.clone(),
        stop: stop_rx.clone(),
    });

    let handle = DownloadHandle {
        probe: info.clone(),
        path: path.clone(),
        total_written: total_written.clone(),
        segments: segments.clone(),
        stop_tx,
        status: status.clone(),
        speed: speed.clone(),
        concurrency: concurrency.clone(),
    };

    // Supervisor: runs the workers, hands finished workers new work stolen from
    // the slowest segment, adapts concurrency, and publishes progress.
    let sup_segments = segments.clone();
    let sup_status = status.clone();
    let sup_written = total_written.clone();
    let sup_speed = speed.clone();
    let sup_conc = concurrency.clone();
    let sup_permits = permits.clone();
    let sup_total = info.total_size;
    let supports_range = info.supports_range;
    let mut stop_watch = stop_rx.clone();

    tokio::spawn(async move {
        let mut tasks: JoinSet<(u16, Result<()>)> = JoinSet::new();
        {
            let segs = sup_segments.lock().clone();
            for seg in segs {
                let ctx = ctx.clone();
                let idx = seg.index;
                tasks.spawn(async move { (idx, run_segment(&ctx, seg).await) });
            }
        }

        let mut retries: std::collections::HashMap<u16, u32> = Default::default();
        let mut rl_retries: u32 = 0;
        let mut last_bytes = sup_written.load(Ordering::Relaxed);
        let mut last_tick = Instant::now();
        let mut next_index: u16 = sup_segments.lock().len() as u16;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {
                    let now = Instant::now();
                    let bytes = sup_written.load(Ordering::Relaxed);
                    let dt = now.duration_since(last_tick).as_secs_f64().max(0.001);
                    sup_speed.store(
                        ((bytes.saturating_sub(last_bytes)) as f64 / dt) as u64,
                        Ordering::Relaxed,
                    );
                    last_bytes = bytes;
                    last_tick = now;
                    on_progress(ProgressSnapshot {
                        downloaded: bytes,
                        total: sup_total,
                        bytes_per_sec: sup_speed.load(Ordering::Relaxed),
                        active_segments: sup_segments
                            .lock()
                            .iter()
                            .filter(|s| !s.finished.load(Ordering::Acquire))
                            .count() as u16,
                        status: *sup_status.lock(),
                    });
                }

                Some(joined) = tasks.join_next() => {
                    let (idx, res) = match joined {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!("segment task panicked: {e}");
                            continue;
                        }
                    };

                    match res {
                        Err(VeloError::Cancelled) => break,

                        Err(e) => {
                            let rate_limited = matches!(e, VeloError::RateLimited(_));
                            let give_up = if rate_limited {
                                rl_retries += 1;
                                rl_retries > MAX_RATE_LIMIT_RETRIES
                            } else {
                                let n = retries.entry(idx).or_insert(0);
                                *n += 1;
                                !e.is_retryable() || *n > MAX_RETRIES
                            };

                            if give_up {
                                tracing::error!("segment {idx} gave up: {e}");
                                *sup_status.lock() = DownloadStatus::Failed;
                                break;
                            }

                            let backoff = if let VeloError::RateLimited(after) = &e {
                                // Server said slow down: use fewer connections
                                // from now on, and wait as long as it asked.
                                let cur = sup_conc.load(Ordering::Relaxed);
                                if cur > 1 {
                                    sup_permits.forget_permits(1);
                                    sup_conc.store(cur - 1, Ordering::Relaxed);
                                    tracing::warn!("rate limited, connections now {}", cur - 1);
                                }
                                Duration::from_secs(after.unwrap_or(2).clamp(1, 30))
                            } else {
                                let n = retries.get(&idx).copied().unwrap_or(1);
                                tracing::warn!("segment {idx} retry {n}: {e}");
                                Duration::from_millis(500 * (1u64 << (n - 1).min(5)))
                            };

                            let Some(seg) = sup_segments
                                .lock()
                                .iter()
                                .find(|s| s.index == idx)
                                .cloned() else { continue };
                            let ctx = ctx.clone();
                            tasks.spawn(async move {
                                tokio::time::sleep(backoff).await;
                                (idx, run_segment(&ctx, seg).await)
                            });
                        }

                        Ok(()) => {
                            // This worker is free. Give it half of whatever
                            // segment has the most bytes left. That is the
                            // trick that keeps every connection busy to the end.
                            if !supports_range {
                                continue;
                            }
                            let stolen = {
                                let segs = sup_segments.lock();
                                let target = segs
                                    .iter()
                                    .filter(|s| {
                                        !s.finished.load(Ordering::Acquire)
                                            && s.end.load(Ordering::Acquire) != u64::MAX
                                            && s.remaining() >= MIN_STEAL_BYTES * 2
                                    })
                                    .max_by_key(|s| s.remaining())
                                    .cloned();
                                target.and_then(|t| {
                                    let cursor = t.cursor.load(Ordering::Acquire);
                                    let end = t.end.load(Ordering::Acquire);
                                    let split = cursor + (end - cursor) / 2;
                                    if split <= cursor || split >= end {
                                        return None;
                                    }
                                    // Shrink the victim first, then claim the tail.
                                    t.end.store(split, Ordering::Release);
                                    Some((split, end))
                                })
                            };

                            if let Some((start, end)) = stolen {
                                let seg = Arc::new(SegmentHandle::new(next_index, start, end, start));
                                next_index += 1;
                                sup_segments.lock().push(seg.clone());
                                let ctx = ctx.clone();
                                let idx = seg.index;
                                tasks.spawn(async move { (idx, run_segment(&ctx, seg).await) });
                            }
                        }
                    }

                    if tasks.is_empty() {
                        let all_done = sup_segments
                            .lock()
                            .iter()
                            .all(|s| s.finished.load(Ordering::Acquire));
                        if all_done && *sup_status.lock() == DownloadStatus::Running {
                            *sup_status.lock() = DownloadStatus::Completed;
                        }
                        break;
                    }
                }

                _ = stop_watch.changed() => {
                    if *stop_watch.borrow() {
                        *sup_status.lock() = DownloadStatus::Paused;
                        break;
                    }
                }
            }
        }

        tasks.shutdown().await;
        let _ = file.sync_all();
        on_progress(ProgressSnapshot {
            downloaded: sup_written.load(Ordering::Relaxed),
            total: sup_total,
            bytes_per_sec: 0,
            active_segments: 0,
            status: *sup_status.lock(),
        });
    });

    Ok(handle)
}
