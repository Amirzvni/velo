//! SQLite persistence for Velo.
//!
//! One connection behind a mutex. Downloads are IO bound, not database bound,
//! so a pool would add complexity for no gain.

pub mod models;
pub mod schema;

use models::{status, BatchRow, DownloadRow, NewDownload};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use velo_core::types::SegmentState;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(i64),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // ---- downloads -------------------------------------------------------

    /// Insert with an explicit starting status. Browser downloads land as
    /// `pending` when the user wants to confirm them first.
    pub fn insert_download_with_status(&self, d: &NewDownload, st: &str) -> Result<i64> {
        let conn = self.conn.lock();
        let ts = now();
        let headers = serde_json::to_string(&d.headers)?;
        let file_name = d.file_name.clone().unwrap_or_else(|| "…".to_string());
        conn.execute(
            "INSERT INTO downloads
             (url, file_name, out_dir, headers, segments, batch_id, scheduled_at,
              priority, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                d.url,
                file_name,
                d.out_dir,
                headers,
                d.segments.unwrap_or(8),
                d.batch_id,
                d.scheduled_at,
                d.priority.unwrap_or(100),
                st,
                ts,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_download(&self, d: &NewDownload) -> Result<i64> {
        let conn = self.conn.lock();
        let ts = now();
        let headers = serde_json::to_string(&d.headers)?;
        // file_name is a placeholder until probe tells us the real one.
        let file_name = d.file_name.clone().unwrap_or_else(|| "…".to_string());
        conn.execute(
            "INSERT INTO downloads
             (url, file_name, out_dir, headers, segments, batch_id, scheduled_at,
              priority, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                d.url,
                file_name,
                d.out_dir,
                headers,
                d.segments.unwrap_or(8),
                d.batch_id,
                d.scheduled_at,
                d.priority.unwrap_or(100),
                status::QUEUED,
                ts,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<DownloadRow> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT * FROM downloads WHERE id = ?1",
            params![id],
            row_to_download,
        )
        .optional()?
        .ok_or(StoreError::NotFound(id))
    }

    pub fn list(&self, status_filter: Option<&str>) -> Result<Vec<DownloadRow>> {
        let conn = self.conn.lock();
        let mut out = Vec::new();
        match status_filter {
            Some(s) => {
                let mut stmt = conn
                    .prepare("SELECT * FROM downloads WHERE status = ?1 ORDER BY priority, id")?;
                for r in stmt.query_map(params![s], row_to_download)? {
                    out.push(r?);
                }
            }
            None => {
                let mut stmt = conn.prepare("SELECT * FROM downloads ORDER BY id DESC")?;
                for r in stmt.query_map([], row_to_download)? {
                    out.push(r?);
                }
            }
        }
        Ok(out)
    }

    /// Next downloads eligible to start: queued, and either unscheduled or due.
    pub fn next_queued(&self, limit: usize) -> Result<Vec<DownloadRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM downloads
             WHERE status = ?1 AND (scheduled_at IS NULL OR scheduled_at <= ?2)
             ORDER BY priority, id
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![status::QUEUED, now(), limit as i64],
            row_to_download,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn count_by_status(&self, s: &str) -> Result<i64> {
        let conn = self.conn.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM downloads WHERE status = ?1",
            params![s],
            |r| r.get(0),
        )?)
    }

    pub fn set_status(&self, id: i64, s: &str, error: Option<&str>) -> Result<()> {
        let conn = self.conn.lock();
        let completed = if s == status::COMPLETED {
            Some(now())
        } else {
            None
        };
        conn.execute(
            "UPDATE downloads
             SET status = ?1, error = ?2, updated_at = ?3,
                 completed_at = COALESCE(?4, completed_at)
             WHERE id = ?5",
            params![s, error, now(), completed, id],
        )?;
        Ok(())
    }

    /// Save what probing discovered so a resume does not have to guess.
    pub fn apply_probe(
        &self,
        id: i64,
        final_url: &str,
        file_name: &str,
        path: &str,
        total_size: Option<u64>,
        supports_range: bool,
        etag: Option<&str>,
        last_modified: Option<&str>,
        mime: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE downloads SET final_url=?1, file_name=?2, path=?3, total_size=?4,
                    supports_range=?5, etag=?6, last_modified=?7, mime=?8, updated_at=?9
             WHERE id=?10",
            params![
                final_url,
                file_name,
                path,
                total_size.map(|v| v as i64),
                supports_range as i32,
                etag,
                last_modified,
                mime,
                now(),
                id
            ],
        )?;
        Ok(())
    }

    pub fn set_progress(&self, id: i64, downloaded: u64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE downloads SET downloaded = ?1, updated_at = ?2 WHERE id = ?3",
            params![downloaded as i64, now(), id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ---- segments (the resume journal) -----------------------------------

    /// Replace the whole segment set in one transaction. Called on a timer
    /// while running and once at pause, so a crash costs at most a few seconds.
    pub fn save_segments(&self, id: i64, segs: &[SegmentState]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE download_id = ?1", params![id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (download_id, idx, start, end, cursor)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for s in segs {
                stmt.execute(params![
                    id,
                    s.index as i64,
                    s.start as i64,
                    // u64::MAX does not fit in SQLite's signed integer.
                    if s.end == u64::MAX {
                        i64::MAX
                    } else {
                        s.end as i64
                    },
                    s.cursor as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_segments(&self, id: i64) -> Result<Vec<SegmentState>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT idx, start, end, cursor FROM segments WHERE download_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            let end: i64 = r.get(2)?;
            Ok(SegmentState {
                index: r.get::<_, i64>(0)? as u16,
                start: r.get::<_, i64>(1)? as u64,
                end: if end == i64::MAX {
                    u64::MAX
                } else {
                    end as u64
                },
                cursor: r.get::<_, i64>(3)? as u64,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ---- batches ---------------------------------------------------------

    pub fn create_batch(&self, name: &str, source_page: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO batches (name, source_page, created_at) VALUES (?1, ?2, ?3)",
            params![name, source_page, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_batches(&self) -> Result<Vec<BatchRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT * FROM batches ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(BatchRow {
                id: r.get("id")?,
                name: r.get("name")?,
                source_page: r.get("source_page")?,
                created_at: r.get("created_at")?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ---- settings and host limits ---------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_host_limit(&self, host: &str) -> Result<Option<u32>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT max_conns FROM host_limits WHERE host = ?1",
                params![host],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|v| v as u32))
    }

    pub fn set_host_limit(&self, host: &str, max_conns: u32) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO host_limits (host, max_conns, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET max_conns = excluded.max_conns,
                                             updated_at = excluded.updated_at",
            params![host, max_conns as i64, now()],
        )?;
        Ok(())
    }

    /// Anything left mid-flight by a crash goes back to the queue on startup.
    pub fn recover_running(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE downloads SET status = ?1, updated_at = ?2 WHERE status = ?3",
            params![status::QUEUED, now(), status::RUNNING],
        )?;
        Ok(n)
    }
}

fn row_to_download(r: &Row) -> rusqlite::Result<DownloadRow> {
    let headers: String = r.get("headers")?;
    Ok(DownloadRow {
        id: r.get("id")?,
        url: r.get("url")?,
        final_url: r.get("final_url")?,
        file_name: r.get("file_name")?,
        out_dir: r.get("out_dir")?,
        path: r.get("path")?,
        total_size: r.get("total_size")?,
        downloaded: r.get("downloaded")?,
        supports_range: r.get::<_, i64>("supports_range")? != 0,
        segments: r.get::<_, i64>("segments")? as u8,
        headers: serde_json::from_str::<HashMap<String, String>>(&headers).unwrap_or_default(),
        etag: r.get("etag")?,
        last_modified: r.get("last_modified")?,
        mime: r.get("mime")?,
        status: r.get("status")?,
        error: r.get("error")?,
        priority: r.get("priority")?,
        batch_id: r.get("batch_id")?,
        scheduled_at: r.get("scheduled_at")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        completed_at: r.get("completed_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_dl(url: &str) -> NewDownload {
        NewDownload {
            url: url.into(),
            out_dir: "/tmp".into(),
            file_name: None,
            headers: HashMap::new(),
            segments: None,
            batch_id: None,
            scheduled_at: None,
            priority: None,
        }
    }

    #[test]
    fn insert_and_read_back() {
        let s = Store::open_in_memory().unwrap();
        let id = s.insert_download(&new_dl("https://x.com/a.zip")).unwrap();
        let row = s.get(id).unwrap();
        assert_eq!(row.status, status::QUEUED);
        assert_eq!(row.segments, 8);
    }

    #[test]
    fn segments_round_trip_with_open_end() {
        let s = Store::open_in_memory().unwrap();
        let id = s.insert_download(&new_dl("https://x.com/a.zip")).unwrap();
        let segs = vec![
            SegmentState {
                index: 0,
                start: 0,
                end: 100,
                cursor: 50,
            },
            SegmentState {
                index: 1,
                start: 100,
                end: u64::MAX,
                cursor: 100,
            },
        ];
        s.save_segments(id, &segs).unwrap();
        let back = s.load_segments(id).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].cursor, 50);
        assert_eq!(back[1].end, u64::MAX);
    }

    #[test]
    fn queue_skips_future_schedules() {
        let s = Store::open_in_memory().unwrap();
        s.insert_download(&new_dl("https://x.com/now.zip")).unwrap();
        let mut later = new_dl("https://x.com/later.zip");
        later.scheduled_at = Some(now() + 3600);
        s.insert_download(&later).unwrap();
        let next = s.next_queued(10).unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].url.ends_with("now.zip"));
    }

    #[test]
    fn crash_recovery_requeues_running() {
        let s = Store::open_in_memory().unwrap();
        let id = s.insert_download(&new_dl("https://x.com/a.zip")).unwrap();
        s.set_status(id, status::RUNNING, None).unwrap();
        assert_eq!(s.recover_running().unwrap(), 1);
        assert_eq!(s.get(id).unwrap().status, status::QUEUED);
    }

    #[test]
    fn deleting_download_drops_its_segments() {
        let s = Store::open_in_memory().unwrap();
        let id = s.insert_download(&new_dl("https://x.com/a.zip")).unwrap();
        s.save_segments(id, &[SegmentState::new(0, 0, 10)]).unwrap();
        s.delete(id).unwrap();
        assert!(s.load_segments(id).unwrap().is_empty());
    }
}
