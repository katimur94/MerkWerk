//! Timeline-Command: liest die Daemon-DB read-only (D2/D8) und liefert die
//! Tages-Timeline an das Frontend.
//!
//! `merkwerk-app` schreibt niemals in die DB (siehe ARCHITEKTUR.md,
//! ENTSCHEIDUNGEN.md D2/D8): `storage::Store::open_readonly` erzwingt
//! `PRAGMA query_only`, hier wird ausschließlich gelesen.

use serde::Serialize;

use crate::paths;

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

/// Zielgröße der Textvorschau in Zeichen (nicht Bytes) — an einer
/// Char-Grenze geschnitten, damit Multi-Byte-UTF-8 nie mitten im Zeichen
/// zerrissen wird.
const PREVIEW_MAX_CHARS: usize = 160;

/// Liefert die Timeline-Einträge im Zeitfenster `[from_ms, to_ms]`, neueste
/// Session zuerst.
///
/// Ablauf: Konfig laden → DB-Pfad auflösen → read-only öffnen →
/// `sessions_between` → je Session der jüngste Snapshot für
/// Fenstertitel/URL/Textvorschau. Existiert die DB-Datei noch nicht (Daemon
/// lief noch nie), ist eine leere Liste der korrekte Erfolgsfall, kein
/// Fehler. Scheitert das Snapshot-Lesen für eine einzelne Session, wird nur
/// diese Session übersprungen statt die gesamte Anfrage fehlschlagen zu
/// lassen.
#[tauri::command]
pub fn list_timeline(from_ms: i64, to_ms: i64) -> Result<Vec<TimelineEntry>, String> {
    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    let db_path = paths::resolve_db_path(&cfg);

    // Der Daemon legt die DB-Datei erst beim ersten Schreibzugriff an. Lief
    // er noch nie, ist "keine Aktivität" der korrekte Zustand — kein Fehler.
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let store = storage::Store::open_readonly(&db_path)
        .map_err(|e| format!("DB nicht lesbar: {e}"))?;

    let sessions = store
        .sessions_between(from_ms, to_ms)
        .map_err(|e| format!("Sessions konnten nicht gelesen werden: {e}"))?;

    let mut entries: Vec<TimelineEntry> = sessions
        .into_iter()
        .filter_map(|session| build_entry(&store, session))
        .collect();

    // Neueste zuerst.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.started_at));

    Ok(entries)
}

/// Baut einen `TimelineEntry` aus einer Session und ihrem jüngsten Snapshot
/// (größtes `ts`). Sessions ohne Snapshot liefern `None`-Felder statt eines
/// Fehlers.
///
/// Schlägt das Lesen der Snapshots für *diese eine* Session fehl, wird
/// defensiv `None` zurückgegeben (Aufrufer filtert die Session per
/// `filter_map` einfach aus) — ein einzelner defekter Datensatz darf die
/// restliche Timeline nicht unsichtbar machen.
fn build_entry(store: &storage::Store, session: storage::AppSessionRow) -> Option<TimelineEntry> {
    let snapshots = store.snapshots_for_session(session.id).ok()?;
    let latest = snapshots.into_iter().max_by_key(|snap| snap.ts);

    let (window_title, url, text_preview) = match latest {
        Some(snap) => (
            snap.window_title,
            snap.url,
            snap.text_content.as_deref().map(preview_text),
        ),
        None => (None, None, None),
    };

    Some(TimelineEntry {
        session_id: session.id,
        process_name: session.process_name,
        started_at: session.started_at,
        ended_at: session.ended_at,
        window_title,
        url,
        text_preview,
    })
}

/// Kürzt `text` auf ~`PREVIEW_MAX_CHARS` Zeichen an einer Char-Grenze
/// (niemals mitten in einem Multi-Byte-UTF-8-Zeichen) und hängt bei
/// tatsächlicher Kürzung ein "…" an.
fn preview_text(text: &str) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(PREVIEW_MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::preview_text;

    #[test]
    fn preview_text_short_string_is_unchanged() {
        assert_eq!(preview_text("hallo welt"), "hallo welt");
    }

    #[test]
    fn preview_text_truncates_at_char_boundary_with_ellipsis() {
        let text = "a".repeat(200);
        let preview = preview_text(&text);
        assert_eq!(preview.chars().count(), 161); // 160 + "…"
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn preview_text_does_not_split_multibyte_chars() {
        // Multi-byte UTF-8 chars (e.g. "ä") repeated well past the char
        // limit — a byte-based split would panic or produce invalid UTF-8.
        let text = "ä".repeat(200);
        let preview = preview_text(&text);
        assert!(preview.is_char_boundary(preview.len()));
        assert_eq!(preview.chars().count(), 161);
    }

    #[test]
    fn preview_text_exact_length_is_not_marked_truncated() {
        let text = "x".repeat(super::PREVIEW_MAX_CHARS);
        let preview = preview_text(&text);
        assert_eq!(preview, text);
        assert!(!preview.ends_with('…'));
    }
}
