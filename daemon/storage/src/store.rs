//! `Store`: the single public entry point into the SQLite-backed database.
//!
//! Per `ENTSCHEIDUNGEN.md` D2, `merkwerk-daemon` is the sole writer; this
//! type is what the daemon's writer thread wraps.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::migrations::migrate;
use crate::model::{AppSessionRow, EventRow, SnapshotRow};

/// One row to persist as part of a batch (see [`Store::insert_batch`]).
///
/// Mirrors the single-row `insert_*` methods on [`Store`] so the writer
/// thread can accumulate a mixed batch of sessions/events/snapshots and
/// commit them together.
pub enum BatchItem<'a> {
    AppSession {
        process_name: &'a str,
        started_at_ms: i64,
        expires_at: Option<i64>,
    },
    EndAppSession {
        id: i64,
        ended_at_ms: i64,
    },
    Event {
        session_id: Option<i64>,
        kind: &'a str,
        ts_ms: i64,
        duration_ms: Option<i64>,
        count: Option<i64>,
        expires_at: Option<i64>,
    },
    Snapshot {
        session_id: Option<i64>,
        event_id: Option<i64>,
        ts_ms: i64,
        window_title: Option<&'a str>,
        url: Option<&'a str>,
        text_content: Option<&'a str>,
        truncated: bool,
        expires_at: Option<i64>,
    },
}

/// Handle to a MerkWerk SQLite database.
///
/// Opening a `Store` runs the migration runner (see `migrations.rs`), so a
/// freshly created file ends up with the full schema in place, and an
/// already-current database is opened as-is.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if necessary) the database at `path`, set the WAL and
    /// foreign-key pragmas, and migrate the schema to the latest version
    /// known to this build.
    pub fn open(path: &Path) -> Result<Store> {
        let mut conn = Connection::open(path)?;
        Self::init_connection(&mut conn)?;
        Ok(Store { conn })
    }

    /// Open a purely in-memory database (handy for tests).
    pub fn open_in_memory() -> Result<Store> {
        let mut conn = Connection::open_in_memory()?;
        Self::init_connection(&mut conn)?;
        Ok(Store { conn })
    }

    /// Open the database at `path` with read-only *semantics* for the app
    /// (ENTSCHEIDUNGEN.md D2/D8): the connection is opened read-write so WAL's
    /// `-shm`/`-wal` sidecars work for a reader, but `PRAGMA query_only = ON`
    /// forbids any mutation at the SQLite level. No migration is run — a reader
    /// must never alter the schema (and `query_only` would reject it anyway).
    ///
    /// This is what `merkwerk-app` uses to render the timeline while the daemon
    /// keeps writing on its own connection.
    pub fn open_readonly(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "query_only", true)?;
        Ok(Store { conn })
    }

    fn init_connection(conn: &mut Connection) -> Result<()> {
        // journal_mode returns the resulting mode as a row, so it needs
        // pragma_update_and_check rather than plain pragma_update.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_row| Ok(()))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(conn)?;
        Ok(())
    }

    /// Start a raw transaction for callers that need more control than
    /// [`Store::insert_batch`] offers.
    pub fn transaction(&mut self) -> Result<rusqlite::Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// Insert a new `app_sessions` row and return its id.
    pub fn insert_app_session(
        &self,
        process_name: &str,
        started_at_ms: i64,
        expires_at: Option<i64>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO app_sessions (process_name, started_at, expires_at)
             VALUES (?1, ?2, ?3)",
            params![process_name, started_at_ms, expires_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Mark an `app_sessions` row as finished.
    pub fn end_app_session(&self, id: i64, ended_at_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE app_sessions SET ended_at = ?1 WHERE id = ?2",
            params![ended_at_ms, id],
        )?;
        Ok(())
    }

    /// Insert a new `events` row and return its id.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_event(
        &self,
        session_id: Option<i64>,
        kind: &str,
        ts_ms: i64,
        duration_ms: Option<i64>,
        count: Option<i64>,
        expires_at: Option<i64>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO events (session_id, kind, ts, duration_ms, count, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, kind, ts_ms, duration_ms, count, expires_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a new `snapshots` row and return its id. `text_bytes` is
    /// derived automatically from the byte length of `text_content`.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_snapshot(
        &self,
        session_id: Option<i64>,
        event_id: Option<i64>,
        ts_ms: i64,
        window_title: Option<&str>,
        url: Option<&str>,
        text_content: Option<&str>,
        truncated: bool,
        expires_at: Option<i64>,
    ) -> Result<i64> {
        let text_bytes = text_content.map(|s| s.len() as i64).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO snapshots
                (session_id, event_id, ts, window_title, url, text_content,
                 text_bytes, truncated, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                event_id,
                ts_ms,
                window_title,
                url,
                text_content,
                text_bytes,
                truncated,
                expires_at
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Persist a batch of mixed inserts/updates in a single transaction.
    /// Returns, for each input item in order, the rowid it produced
    /// (`0` for [`BatchItem::EndAppSession`], which has no id of its own).
    pub fn insert_batch(&mut self, items: &[BatchItem<'_>]) -> Result<Vec<i64>> {
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(items.len());

        for item in items {
            let id = match item {
                BatchItem::AppSession {
                    process_name,
                    started_at_ms,
                    expires_at,
                } => {
                    tx.execute(
                        "INSERT INTO app_sessions (process_name, started_at, expires_at)
                         VALUES (?1, ?2, ?3)",
                        params![process_name, started_at_ms, expires_at],
                    )?;
                    tx.last_insert_rowid()
                }
                BatchItem::EndAppSession { id, ended_at_ms } => {
                    tx.execute(
                        "UPDATE app_sessions SET ended_at = ?1 WHERE id = ?2",
                        params![ended_at_ms, id],
                    )?;
                    0
                }
                BatchItem::Event {
                    session_id,
                    kind,
                    ts_ms,
                    duration_ms,
                    count,
                    expires_at,
                } => {
                    tx.execute(
                        "INSERT INTO events (session_id, kind, ts, duration_ms, count, expires_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![session_id, kind, ts_ms, duration_ms, count, expires_at],
                    )?;
                    tx.last_insert_rowid()
                }
                BatchItem::Snapshot {
                    session_id,
                    event_id,
                    ts_ms,
                    window_title,
                    url,
                    text_content,
                    truncated,
                    expires_at,
                } => {
                    let text_bytes = text_content.map(|s| s.len() as i64).unwrap_or(0);
                    tx.execute(
                        "INSERT INTO snapshots
                            (session_id, event_id, ts, window_title, url, text_content,
                             text_bytes, truncated, expires_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            session_id,
                            event_id,
                            ts_ms,
                            window_title,
                            url,
                            text_content,
                            text_bytes,
                            truncated,
                            expires_at
                        ],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            ids.push(id);
        }

        tx.commit()?;
        Ok(ids)
    }

    /// App sessions that overlap the half-open... actually closed `[from_ms,
    /// to_ms]` window: started at or before `to_ms`, and either still
    /// running or ended at or after `from_ms`. Ordered by `started_at`.
    pub fn sessions_between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<AppSessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, process_name, started_at, ended_at, expires_at
             FROM app_sessions
             WHERE started_at <= ?2 AND (ended_at IS NULL OR ended_at >= ?1)
             ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms], |row| {
            Ok(AppSessionRow {
                id: row.get(0)?,
                process_name: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                expires_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All events belonging to `session_id`, ordered by timestamp.
    pub fn events_for_session(&self, session_id: i64) -> Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, kind, ts, duration_ms, count, expires_at
             FROM events WHERE session_id = ?1 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(EventRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                kind: row.get(2)?,
                ts: row.get(3)?,
                duration_ms: row.get(4)?,
                count: row.get(5)?,
                expires_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// All snapshots belonging to `session_id`, ordered by timestamp.
    pub fn snapshots_for_session(&self, session_id: i64) -> Result<Vec<SnapshotRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, event_id, ts, window_title, url, text_content,
                    text_bytes, truncated, expires_at
             FROM snapshots WHERE session_id = ?1 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(SnapshotRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                event_id: row.get(2)?,
                ts: row.get(3)?,
                window_title: row.get(4)?,
                url: row.get(5)?,
                text_content: row.get(6)?,
                text_bytes: row.get(7)?,
                truncated: row.get::<_, i64>(8)? != 0,
                expires_at: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
