//! Timeline-Command: liest die Daemon-DB read-only (D2/D8) und liefert die
//! Tages-Timeline an das Frontend.
//!
//! STUB (Seam für Task T10): Signaturen + DTO stehen fest; die Query-Implementierung
//! füllt T10 aus. `list_timeline` ist in `lib.rs` registriert.

use serde::Serialize;

/// Eine Zeile der Timeline-Ansicht. Aus `app_sessions` + zugehörigen `snapshots`
/// zusammengesetzt (siehe `storage`-Row-Typen).
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub session_id: i64,
    pub process_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    /// Kurze Vorschau des Snapshot-Textes (gekürzt).
    pub text_preview: Option<String>,
}

/// Liefert die Timeline-Einträge im Zeitfenster `[from_ms, to_ms]`.
///
/// TODO(T10): Konfig laden (`paths::config_path`), DB-Pfad auflösen
/// (`paths::resolve_db_path`), `storage::Store::open_readonly` öffnen,
/// `sessions_between` + je Session `snapshots_for_session` lesen und daraus
/// `TimelineEntry`s bauen (Textvorschau gekürzt).
#[tauri::command]
pub fn list_timeline(from_ms: i64, to_ms: i64) -> Result<Vec<TimelineEntry>, String> {
    let _ = (from_ms, to_ms);
    Ok(Vec::new())
}
