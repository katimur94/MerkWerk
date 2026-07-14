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

/// Schema version 2: a full-text index over `snapshots` (`window_title`,
/// `url`, `text_content`), per `ARCHITEKTUR.md` ("Anbaubarkeit für später")
/// and `docs/ROADMAP.md` (Etappe 1, "Volltextsuche (FTS5)").
///
/// `snapshots_fts` is an *external content* FTS5 table (`content =
/// 'snapshots', content_rowid = 'id'`): its shadow tables don't duplicate
/// the row text, SQLite instead reads it back from `snapshots` — by column
/// name, not position, so declaring only a subset of `snapshots`' columns
/// here (in a different position than they appear in `snapshots`) is fine —
/// whenever `snippet()`/`highlight()` or a plain `SELECT` need it. That
/// means the *inverted index* itself is not auto-maintained and must be
/// kept in sync by hand, which is exactly what the three triggers below do
/// (the standard SQLite "External Content Tables" pattern, see
/// <https://sqlite.org/fts5.html> §4.4.3, "Keeping An External Content
/// Table And Its Index In Sync"):
///   - `AFTER INSERT`: index the new row's text.
///   - `AFTER DELETE`: the special `('delete', rowid, ...)` form removes
///     exactly the tokens that were indexed for that row. This needs the
///     *old* column values as arguments because by the time the trigger
///     runs the row is already gone from `snapshots`, so FTS5 can no longer
///     read them back itself.
///   - `AFTER UPDATE`: delete the old tokens, then insert the new ones.
///
/// `window_title`/`url`/`text_content` are all nullable in `snapshots`, but
/// FTS5 index text should never be NULL, so every trigger coalesces to `''`.
///
/// The triggers only cover rows written *after* this migration runs. A
/// database migrating from v1 may already have `snapshots` rows (per the
/// migration runner's own contract: "a live database may already be
/// sitting between two versions"), so the final statement backfills the
/// index for any pre-existing rows in one pass, using the same coalesce
/// rule as the triggers.
const SCHEMA_V2: &str = r#"
CREATE VIRTUAL TABLE snapshots_fts USING fts5(
    window_title, url, text_content,
    content='snapshots', content_rowid='id'
);

CREATE TRIGGER snapshots_ai AFTER INSERT ON snapshots BEGIN
  INSERT INTO snapshots_fts(rowid, window_title, url, text_content)
  VALUES (
    new.id,
    coalesce(new.window_title, ''),
    coalesce(new.url, ''),
    coalesce(new.text_content, '')
  );
END;

CREATE TRIGGER snapshots_ad AFTER DELETE ON snapshots BEGIN
  INSERT INTO snapshots_fts(snapshots_fts, rowid, window_title, url, text_content)
  VALUES (
    'delete',
    old.id,
    coalesce(old.window_title, ''),
    coalesce(old.url, ''),
    coalesce(old.text_content, '')
  );
END;

CREATE TRIGGER snapshots_au AFTER UPDATE ON snapshots BEGIN
  INSERT INTO snapshots_fts(snapshots_fts, rowid, window_title, url, text_content)
  VALUES (
    'delete',
    old.id,
    coalesce(old.window_title, ''),
    coalesce(old.url, ''),
    coalesce(old.text_content, '')
  );
  INSERT INTO snapshots_fts(rowid, window_title, url, text_content)
  VALUES (
    new.id,
    coalesce(new.window_title, ''),
    coalesce(new.url, ''),
    coalesce(new.text_content, '')
  );
END;

INSERT INTO snapshots_fts(rowid, window_title, url, text_content)
SELECT id, coalesce(window_title, ''), coalesce(url, ''), coalesce(text_content, '')
FROM snapshots;
"#;

struct Migration {
    version: i64,
    up: &'static str,
}

/// Ordered list of schema migrations. Append new versions here — never
/// mutate or remove an existing entry.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        up: SCHEMA_V1,
    },
    Migration {
        version: 2,
        up: SCHEMA_V2,
    },
];

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

    #[test]
    fn fts5_is_available() {
        // Bare availability probe, independent of SCHEMA_V2: proves the
        // bundled SQLite (rusqlite "bundled" feature, pinned to 0.31 /
        // libsqlite3-sys 0.28 per ENTSCHEIDUNGEN.md D7) was actually
        // compiled with FTS5 support. If this ever fails, SCHEMA_V2 below
        // cannot possibly succeed either.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE fts5_probe USING fts5(x);")
            .expect("FTS5 not available in this SQLite build");
    }

    #[test]
    fn migrate_upgrades_v1_db_to_v2() {
        // Build a v1-only database by hand — i.e. simulate a database that
        // was created before SCHEMA_V2 existed — with a pre-existing
        // `snapshots` row, *without* going through `migrate()` (which would
        // always jump straight to `latest`).
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', '1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO snapshots (ts, window_title, url, text_content)
             VALUES (1, 'Pre-existing', NULL, 'archaic backfill needle')",
            [],
        )
        .unwrap();
        assert_eq!(current_version(&conn).unwrap(), 1);

        // snapshots_fts must not exist yet on a v1 database.
        let fts_exists_before: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master \
                 WHERE type = 'table' AND name = 'snapshots_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fts_exists_before, 0);

        // Re-opening (i.e. calling migrate() again, exactly as `Store::open`
        // does on every open) must lift the DB from v1 to v2.
        migrate(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
        assert_eq!(latest_known_version(), 2);

        for name in [
            "snapshots_fts",
            "snapshots_ai",
            "snapshots_ad",
            "snapshots_au",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "{name} should exist after v1->v2 migration");
        }

        // The pre-existing row (inserted before v2's triggers existed) was
        // backfilled into the FTS index by the migration itself.
        let backfilled: i64 = conn
            .query_row(
                "SELECT count(*) FROM snapshots_fts WHERE snapshots_fts MATCH 'backfill'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            backfilled, 1,
            "pre-existing v1 row must be searchable after the v2 backfill"
        );

        // A second migrate() call (opening the now-v2 database again) stays
        // a no-op — must not try to re-create the FTS5 table/triggers.
        migrate(&mut conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
    }
}
