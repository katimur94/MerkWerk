//! Row structs returned by the storage layer's query helpers.
//!
//! These mirror the tables described in `ARCHITEKTUR.md` ("DB-Schema (v0,
//! migrierbar)") and derive `serde::Serialize` so `merkwerk-app` can hand
//! them straight to the frontend as JSON.

use serde::Serialize;

/// A contiguous focus interval on one application (`app_sessions` row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppSessionRow {
    pub id: i64,
    pub process_name: String,
    /// Unix milliseconds.
    pub started_at: i64,
    /// Unix milliseconds; `None` means the session is still running.
    pub ended_at: Option<i64>,
    /// Unix milliseconds TTL; deletion job lands in Etappe 1.
    pub expires_at: Option<i64>,
}

/// A low-frequency activity event (`events` row): focus change, typing
/// burst, click cluster, scroll end, idle, ...
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub kind: String,
    /// Unix milliseconds.
    pub ts: i64,
    pub duration_ms: Option<i64>,
    pub count: Option<i64>,
    pub expires_at: Option<i64>,
}

/// A context snapshot (`snapshots` row) — the raw material later fed to the
/// on-device summarizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub event_id: Option<i64>,
    /// Unix milliseconds.
    pub ts: i64,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub text_content: Option<String>,
    /// Byte length of `text_content` (0 when `text_content` is `None`).
    pub text_bytes: i64,
    /// Whether `text_content` was truncated at the 20 KB snapshot budget.
    pub truncated: bool,
    pub expires_at: Option<i64>,
}

/// One full-text search hit against the `snapshots_fts` index (migration
/// v2), returned by [`crate::Store::search`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    pub snapshot_id: i64,
    pub session_id: Option<i64>,
    /// Unix milliseconds.
    pub ts: i64,
    pub window_title: Option<String>,
    pub url: Option<String>,
    /// `text_content` excerpt around the match, built by FTS5's `snippet()`
    /// with `[` `]` markers around matched terms and `…` for elided text.
    pub snippet: String,
}
