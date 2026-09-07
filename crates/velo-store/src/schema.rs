use rusqlite::Connection;

/// Bump this when adding a migration below.
pub const SCHEMA_VERSION: i32 = 1;

/// Apply every migration the database has not seen yet.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // WAL = readers never block the writer, and we survive a hard power cut.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    let current: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if current < 1 {
        conn.execute_batch(V1)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

const V1: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    url             TEXT    NOT NULL,
    final_url       TEXT,
    file_name       TEXT    NOT NULL,
    out_dir         TEXT    NOT NULL,
    path            TEXT,
    total_size      INTEGER,
    downloaded      INTEGER NOT NULL DEFAULT 0,
    supports_range  INTEGER NOT NULL DEFAULT 0,
    segments        INTEGER NOT NULL DEFAULT 8,
    -- JSON map of request headers (cookies, referer, user-agent) from the browser
    headers         TEXT    NOT NULL DEFAULT '{}',
    etag            TEXT,
    last_modified   TEXT,
    mime            TEXT,
    status          TEXT    NOT NULL DEFAULT 'queued',
    error           TEXT,
    -- lower number runs first; used by the scheduler
    priority        INTEGER NOT NULL DEFAULT 100,
    -- optional batch this download arrived with, for "grab all links"
    batch_id        INTEGER REFERENCES batches(id) ON DELETE SET NULL,
    -- unix seconds; NULL means start as soon as a slot frees up
    scheduled_at    INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    completed_at    INTEGER
);

CREATE INDEX IF NOT EXISTS idx_downloads_status   ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_queue    ON downloads(status, priority, id);
CREATE INDEX IF NOT EXISTS idx_downloads_batch    ON downloads(batch_id);

CREATE TABLE IF NOT EXISTS segments (
    download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start       INTEGER NOT NULL,
    end         INTEGER NOT NULL,
    cursor      INTEGER NOT NULL,
    PRIMARY KEY (download_id, idx)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS batches (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    source_page TEXT,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- Remembered per-host connection limit, learned when a host answers 429.
CREATE TABLE IF NOT EXISTS host_limits (
    host        TEXT PRIMARY KEY,
    max_conns   INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) WITHOUT ROWID;
"#;
