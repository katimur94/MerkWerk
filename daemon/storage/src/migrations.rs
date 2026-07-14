//! Schema DDL and the migration runner.
//!
//! The DDL for schema version 1 is transcribed verbatim from
//! `ARCHITEKTUR.md` ("DB-Schema (v0, migrierbar)"), minus the
//! `PRAGMA journal_mode = WAL;` line — that pragma is a connection-level
//! setting, not schema, so `Store::open` sets it directly on the
//! connection instead of baking it into a migration.
//!
//! Later schema changes (FTS5, sqlite-vec, ...) are added as new entries in
//! [`MIGRATIONS`] with an incrementing `version`; existing entries must
//! never be edited once released, since a live database may already be
//! sitting between two versions.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Error, Result};

/// Schema version 1: the initial `meta` / `app_sessions` / `events` /
/// `snapshots` tables plus their timeline indexes, exactly as specified in
/// `ARCHITEKTUR.md`.
const SCHEMA_V1: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE app_sessions (
    id           INTEGER PRIMARY KEY,
    process_name TEXT NOT NULL,
    started_at   INTEGER NOT NULL,
    ended_at     INTEGER,
    expires_at   INTEGER
);

CREATE TABLE events (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER REFERENCES app_sessions(id),
    kind         TEXT NOT NULL,
    ts           INTEGER NOT NULL,
    duration_ms  INTEGER,
    count        INTEGER,
    expires_at   INTEGER
);

CREATE TABLE snapshots (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER REFERENCES app_sessions(id),
    event_id     INTEGER REFERENCES events(id),
    ts           INTEGER NOT NULL,
    window_title TEXT,
    url          TEXT,
    text_content TEXT,
    text_bytes   INTEGER NOT NULL DEFAULT 0,
    truncated    INTEGER NOT NULL DEFAULT 0,
    expires_at   INTEGER
);

CREATE INDEX idx_sessions_started ON app_sessions(started_at);
CREATE INDEX idx_events_ts        ON events(ts);
CREATE INDEX idx_snapshots_ts     ON snapshots(ts);
"#;

struct Migration {
    version: i64,
    up: &'static str,
}

/// Ordered list of schema migrations. Append new versions here — never
/// mutate or remove an existing entry.
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    up: SCHEMA_V1,
}];

/// Highest schema version this build of the crate understands.
fn latest_known_version() -> i64 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// Read `meta.schema_version`. Returns `0` for a brand-new (empty) database
/// that doesn't have a `meta` table yet.
fn current_version(conn: &Connection) -> Result<i64> {
    let meta_exists: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;

    if !meta_exists {
        return Ok(0);
    }

    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    Ok(raw.and_then(|s| s.parse::<i64>().ok()).unwrap_or(0))
}

/// Bring `conn` up to [`latest_known_version`], applying any pending
/// migrations in a single transaction. Idempotent: calling it again on an
/// already-current database is a no-op. Refuses to open a database whose
/// recorded schema version is newer than this build knows about.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let current = current_version(conn)?;
    let latest = latest_known_version();

    if current > latest {
        return Err(Error::SchemaTooNew {
            db: current,
            code: latest,
        });
    }

    if current == latest {
        return Ok(());
    }

    let tx = conn.transaction()?;
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        tx.execute_batch(migration.up)?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![migration.version.to_string()],
        )?;
    }
    tx.commit()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_migrates_to_latest_and_creates_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();

        assert_eq!(current_version(&conn).unwrap(), latest_known_version());

        for table in ["meta", "app_sessions", "events", "snapshots"] {
            let exists: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "table {table} should exist after migration");
        }
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        // Second run must not try to re-create tables (which would error).
        migrate(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), latest_known_version());
    }

    #[test]
    fn refuses_to_open_newer_db() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
            params![(latest_known_version() + 1).to_string()],
        )
        .unwrap();

        let err = migrate(&mut conn).unwrap_err();
        match err {
            Error::SchemaTooNew { db, code } => {
                assert_eq!(db, latest_known_version() + 1);
                assert_eq!(code, latest_known_version());
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }
}
