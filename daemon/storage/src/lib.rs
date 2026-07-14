//! Storage layer for MerkWerk — SQLite interface and batch writing.
//!
//! Implements the schema from `ARCHITEKTUR.md` ("DB-Schema (v0,
//! migrierbar)"): `meta`, `app_sessions`, `events`, `snapshots`, plus their
//! timeline indexes. Per `ENTSCHEIDUNGEN.md` D2, `merkwerk-daemon` is the
//! sole writer of this database; `merkwerk-app` opens it read-only.

mod error;
mod migrations;
mod model;
mod store;

pub use error::{Error, Result};
pub use model::{AppSessionRow, EventRow, SnapshotRow};
pub use store::{BatchItem, Store};

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("merkwerk.sqlite3");
        let store = Store::open(&path).expect("open store");
        (dir, store)
    }

    #[test]
    fn open_on_temp_file_creates_schema() {
        let (_dir, _store) = temp_store();
        // Store::open() already ran the migration; no panic means schema
        // creation + WAL/foreign_keys pragmas succeeded.
    }

    #[test]
    fn open_readonly_reads_but_rejects_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("merkwerk.sqlite3");

        // Writer (daemon) creates the DB and inserts a row.
        let writer = Store::open(&path).unwrap();
        let sid = writer
            .insert_app_session("code.exe", 1_000, None)
            .unwrap();
        drop(writer);

        // Reader (app) sees the row read-only...
        let reader = Store::open_readonly(&path).unwrap();
        let sessions = reader.sessions_between(0, 10_000).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, sid);

        // ...but any write is rejected by PRAGMA query_only.
        let write = reader.insert_app_session("evil.exe", 2_000, None);
        assert!(write.is_err(), "read-only store must reject writes");
    }

    #[test]
    fn opening_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("merkwerk.sqlite3");

        let store1 = Store::open(&path).unwrap();
        drop(store1);
        // Re-opening an already-migrated database must not error or
        // attempt to re-create tables.
        let _store2 = Store::open(&path).unwrap();
    }

    #[test]
    fn insert_and_end_app_session() {
        let (_dir, store) = temp_store();

        let id = store
            .insert_app_session("chrome.exe", 1_000, Some(2_000_000))
            .unwrap();
        assert!(id > 0);

        let sessions = store.sessions_between(0, 10_000).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id);
        assert_eq!(sessions[0].process_name, "chrome.exe");
        assert_eq!(sessions[0].started_at, 1_000);
        assert_eq!(sessions[0].ended_at, None);

        store.end_app_session(id, 5_000).unwrap();

        let sessions = store.sessions_between(0, 10_000).unwrap();
        assert_eq!(sessions[0].ended_at, Some(5_000));
    }

    #[test]
    fn insert_event_roundtrip() {
        let (_dir, store) = temp_store();

        let session_id = store.insert_app_session("code.exe", 0, None).unwrap();
        let event_id = store
            .insert_event(Some(session_id), "typing_burst", 1_500, Some(2_300), None, None)
            .unwrap();
        assert!(event_id > 0);

        let events = store.events_for_session(session_id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "typing_burst");
        assert_eq!(events[0].duration_ms, Some(2_300));
        assert_eq!(events[0].count, None);
    }

    #[test]
    fn insert_snapshot_sets_text_bytes_and_truncated() {
        let (_dir, store) = temp_store();

        let session_id = store.insert_app_session("chrome.exe", 0, None).unwrap();
        let text = "hällo wörld"; // multi-byte UTF-8 on purpose
        let snapshot_id = store
            .insert_snapshot(
                Some(session_id),
                None,
                42,
                Some("Inbox — Mail"),
                Some("https://example.com"),
                Some(text),
                true,
                Some(99_999),
            )
            .unwrap();
        assert!(snapshot_id > 0);

        let snapshots = store.snapshots_for_session(session_id).unwrap();
        assert_eq!(snapshots.len(), 1);
        let s = &snapshots[0];
        assert_eq!(s.text_bytes, text.len() as i64);
        assert_ne!(s.text_bytes, text.chars().count() as i64); // proves it's bytes, not chars
        assert!(s.truncated);
        assert_eq!(s.window_title.as_deref(), Some("Inbox — Mail"));
        assert_eq!(s.url.as_deref(), Some("https://example.com"));
        assert_eq!(s.expires_at, Some(99_999));
    }

    #[test]
    fn insert_snapshot_without_text_has_zero_bytes() {
        let (_dir, store) = temp_store();
        let session_id = store.insert_app_session("game.exe", 0, None).unwrap();
        let snapshot_id = store
            .insert_snapshot(Some(session_id), None, 1, None, None, None, false, None)
            .unwrap();
        let snapshots = store.snapshots_for_session(session_id).unwrap();
        let s = snapshots.iter().find(|s| s.id == snapshot_id).unwrap();
        assert_eq!(s.text_bytes, 0);
        assert!(!s.truncated);
        assert_eq!(s.text_content, None);
    }

    #[test]
    fn sessions_between_filters_correctly() {
        let (_dir, store) = temp_store();

        // Entirely before the window.
        let before = store.insert_app_session("a.exe", 0, None).unwrap();
        store.end_app_session(before, 100).unwrap();

        // Overlaps the start of the window.
        let overlap_start = store.insert_app_session("b.exe", 900, None).unwrap();
        store.end_app_session(overlap_start, 1_100).unwrap();

        // Fully inside the window.
        let inside = store.insert_app_session("c.exe", 1_200, None).unwrap();
        store.end_app_session(inside, 1_800).unwrap();

        // Still running, started inside the window.
        let still_running = store.insert_app_session("d.exe", 1_900, None).unwrap();

        // Entirely after the window.
        let after = store.insert_app_session("e.exe", 5_000, None).unwrap();
        store.end_app_session(after, 5_500).unwrap();

        let results = store.sessions_between(1_000, 2_000).unwrap();
        let names: Vec<&str> = results.iter().map(|r| r.process_name.as_str()).collect();

        assert_eq!(names, vec!["b.exe", "c.exe", "d.exe"]);
        let _ = still_running; // silence unused warning if reordered later
    }

    #[test]
    fn insert_batch_commits_all_rows_in_one_transaction() {
        let (_dir, mut store) = temp_store();

        let ids = store
            .insert_batch(&[
                BatchItem::AppSession {
                    process_name: "chrome.exe",
                    started_at_ms: 10,
                    expires_at: None,
                },
                BatchItem::Event {
                    session_id: None, // session id from the first item isn't known yet in this simple batch
                    kind: "focus_change",
                    ts_ms: 11,
                    duration_ms: None,
                    count: None,
                    expires_at: None,
                },
                BatchItem::Snapshot {
                    session_id: None,
                    event_id: None,
                    ts_ms: 12,
                    window_title: Some("New Tab"),
                    url: None,
                    text_content: Some("hello"),
                    truncated: false,
                    expires_at: None,
                },
            ])
            .unwrap();

        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|&id| id > 0));

        // All three rows must be visible — proves the transaction committed.
        let sessions = store.sessions_between(0, 100).unwrap();
        assert_eq!(sessions.len(), 1);

        let count_events: i64 = {
            let tx = store.transaction().unwrap();
            tx.query_row("SELECT count(*) FROM events", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(count_events, 1);

        let count_snapshots: i64 = {
            let tx = store.transaction().unwrap();
            tx.query_row("SELECT count(*) FROM snapshots", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(count_snapshots, 1);
    }

    #[test]
    fn insert_batch_rolls_back_nothing_committed_on_error() {
        let (_dir, mut store) = temp_store();

        // A foreign key violation (bogus session_id) should make the whole
        // batch fail and commit nothing, since foreign_keys=ON.
        let result = store.insert_batch(&[
            BatchItem::Event {
                session_id: Some(999_999),
                kind: "idle",
                ts_ms: 1,
                duration_ms: None,
                count: None,
                expires_at: None,
            },
        ]);
        assert!(result.is_err());

        let count: i64 = {
            let tx = store.transaction().unwrap();
            tx.query_row("SELECT count(*) FROM events", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(count, 0);
    }
}
