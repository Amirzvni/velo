use crate::error::{Result, VeloError};
use crate::fsutil::write_at;
use crate::probe::build_headers;
use crate::types::DownloadSpec;
use futures_util::StreamExt;
use reqwest::header::{HeaderValue, RANGE, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Semaphore};

/// Shared per-segment state the engine can read and adjust while we run.
/// `end` is atomic because the engine shrinks it when another worker steals
/// our tail; we re-read it every chunk and stop early when we pass it.
pub struct SegmentHandle {
    pub index: u16,
    pub start: AtomicU64,
    pub end: AtomicU64,
    pub cursor: AtomicU64,
    pub finished: AtomicBool,
}

impl SegmentHandle {
    pub fn new(index: u16, start: u64, end: u64, cursor: u64) -> Self {
        Self {
            index,
            start: AtomicU64::new(start),
            end: AtomicU64::new(end),
            cursor: AtomicU64::new(cursor),
            finished: AtomicBool::new(false),
        }
    }
    pub fn remaining(&self) -> u64 {
        self.end
            .load(Ordering::Acquire)
            .saturating_sub(self.cursor.load(Ordering::Acquire))
    }
}

pub struct WorkerCtx {
    pub client: Client,
    pub spec: DownloadSpec,
    pub url: String,
    pub file: Arc<File>,
    /// Total bytes written across all segments, for the progress meter.
    pub total_written: Arc<AtomicU64>,
    /// Caps how many requests this one download has open. The engine shrinks
    /// it when the server answers 429 so we stop annoying it.
    pub permits: Arc<Semaphore>,
    /// Flips to true on pause or cancel.
    pub stop: watch::Receiver<bool>,
}

/// How many bytes we buffer before touching the disk. Bigger = fewer syscalls.
const WRITE_BUFFER: usize = 256 * 1024;

fn retry_after_secs(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Download one segment. Retries are handled by the engine, not here.
pub async fn run_segment(ctx: &WorkerCtx, seg: Arc<SegmentHandle>) -> Result<()> {
    loop {
        let cursor = seg.cursor.load(Ordering::Acquire);
        let end = seg.end.load(Ordering::Acquire);
        if cursor >= end {
            seg.finished.store(true, Ordering::Release);
            return Ok(());
        }

        // Held for the whole transfer so the host never sees more concurrent
        // requests from us than the current limit allows.
        let _permit = ctx
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| VeloError::Other("permit pool closed".into()))?;

        let mut headers = build_headers(&ctx.spec)?;
        let range = if end == u64::MAX {
            format!("bytes={cursor}-")
        } else {
            format!("bytes={cursor}-{}", end - 1)
        };
        headers.insert(
            RANGE,
            HeaderValue::from_str(&range).map_err(|e| VeloError::Other(e.to_string()))?,
        );

        let resp = ctx.client.get(&ctx.url).headers(headers).send().await?;
        let status = resp.status();

        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
            return Err(VeloError::RateLimited(retry_after_secs(&resp)));
        }
        if status != StatusCode::PARTIAL_CONTENT && status != StatusCode::OK {
            return Err(VeloError::BadStatus(status.as_u16()));
        }
        // A 200 to a ranged request means the server ignored Range. Only safe
        // if we are at offset 0; otherwise the bytes would land in the wrong place.
        if status == StatusCode::OK && cursor != 0 {
            return Err(VeloError::Other("server ignored range request".into()));
        }

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUFFER);
        let mut buf_offset = cursor;

        while let Some(chunk) = stream.next().await {
            if *ctx.stop.borrow() {
                flush(ctx, &seg, &mut buf, &mut buf_offset)?;
                return Err(VeloError::Cancelled);
            }

            let chunk = chunk?;
            if chunk.is_empty() {
                continue;
            }

            // The engine may have shrunk our end while this chunk was in flight.
            let end_now = seg.end.load(Ordering::Acquire);
            let pos = buf_offset + buf.len() as u64;
            if pos >= end_now {
                flush(ctx, &seg, &mut buf, &mut buf_offset)?;
                seg.finished.store(true, Ordering::Release);
                return Ok(());
            }
            let room = (end_now - pos) as usize;
            let take = room.min(chunk.len());
            buf.extend_from_slice(&chunk[..take]);

            if buf.len() >= WRITE_BUFFER {
                flush(ctx, &seg, &mut buf, &mut buf_offset)?;
            }
            if take < chunk.len() {
                flush(ctx, &seg, &mut buf, &mut buf_offset)?;
                seg.finished.store(true, Ordering::Release);
                return Ok(());
            }
        }

        flush(ctx, &seg, &mut buf, &mut buf_offset)?;

        let cursor = seg.cursor.load(Ordering::Acquire);
        let end = seg.end.load(Ordering::Acquire);
        if cursor >= end || end == u64::MAX {
            seg.finished.store(true, Ordering::Release);
            return Ok(());
        }
        // Stream ended before the range did: connection dropped. Loop and
        // re-request from the current cursor. Permit is released here.
    }
}

fn flush(
    ctx: &WorkerCtx,
    seg: &SegmentHandle,
    buf: &mut Vec<u8>,
    buf_offset: &mut u64,
) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    write_at(&ctx.file, *buf_offset, buf)?;
    let n = buf.len() as u64;
    *buf_offset += n;
    seg.cursor.store(*buf_offset, Ordering::Release);
    ctx.total_written.fetch_add(n, Ordering::Relaxed);
    buf.clear();
    Ok(())
}
