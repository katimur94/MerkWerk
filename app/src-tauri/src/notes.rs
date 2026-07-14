//! Notizen-Commands: liest KI-Notizen (Metadaten aus der `notes`-Tabelle plus
//! Markdown-Inhalt aus der Vault-Datei) read-only aus der Daemon-DB/-Vault
//! und stößt eine Sofort-Destillation beim Daemon an.
//!
//! `list_notes`/`get_note_markdown` folgen exakt dem read-only-Muster aus
//! `timeline.rs`/`search.rs` (siehe dort für die ausführliche Begründung):
//! `merkwerk-app` liest die Daemon-DB ausschließlich über
//! `storage::Store::open_readonly` (erzwingt `PRAGMA query_only`, siehe
//! ARCHITEKTUR.md, ENTSCHEIDUNGEN.md D2/D8) und schreibt nie hinein. Der
//! eigentliche Notizinhalt liegt laut ENTSCHEIDUNGEN.md D10 nicht in der DB,
//! sondern als `.md`-Datei im Vault-Verzeichnis; `notes.file_path` zeigt
//! darauf, die Datei ist die Quelle der Wahrheit.
//!
//! `distill_now` schickt dagegen ein IPC-Kommando über die Named Pipe an den
//! Daemon (gleiches Wire-Muster wie `settings::send_reload_config_over_pipe`)
//! — anders als der dortige best-effort-Reload liefert dieser Command einen
//! echten Fehler zurück, wenn der Daemon nicht erreichbar ist, damit der
//! „Jetzt destillieren"-Button im Frontend sichtbar scheitern kann.

use serde::Serialize;

use crate::paths;

/// Untere/obere Grenze, auf die `limit` geklemmt wird, bevor es an
/// `storage::Store::list_recent_notes` weitergereicht wird — gleiches Muster
/// wie in `search.rs` (`MIN_LIMIT`/`MAX_LIMIT`): schützt vor `limit <= 0` und
/// vor unbegrenzt großen Anfragen vom Frontend.
const MIN_LIMIT: i64 = 1;
const MAX_LIMIT: i64 = 200;

/// Liefert die `limit` zuletzt erstellten Notizen (Metadaten, ohne
/// Markdown-Inhalt — siehe [`get_note_markdown`] dafür), neueste zuerst.
///
/// Ablauf identisch zu `timeline::list_timeline`: Konfig laden → DB-Pfad
/// auflösen → wenn die DB-Datei noch nicht existiert (Daemon lief noch nie
/// oder hat noch nie destilliert) ist eine leere Liste der korrekte
/// Erfolgsfall, kein Fehler → sonst read-only öffnen und lesen.
#[tauri::command]
pub fn list_notes(limit: i64) -> Result<Vec<storage::NoteRow>, String> {
    let clamped_limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);

    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    let db_path = paths::resolve_db_path(&cfg);

    // Der Daemon legt die DB-Datei erst beim ersten Schreibzugriff an. Lief
    // er noch nie, ist "keine Notizen" der korrekte Zustand — kein Fehler.
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let store = storage::Store::open_readonly(&db_path)
        .map_err(|e| format!("DB nicht lesbar: {e}"))?;

    store
        .list_recent_notes(clamped_limit)
        .map_err(|e| format!("Notizen konnten nicht gelesen werden: {e}"))
}

/// Eine Notiz inkl. ihres Markdown-Inhalts, wie sie [`get_note_markdown`] an
/// das Frontend liefert.
#[derive(Debug, Clone, Serialize)]
pub struct NoteContent {
    pub note: storage::NoteRow,
    pub markdown: String,
}

/// Liest eine einzelne Notiz (Metadaten aus der DB) plus den Markdown-Inhalt
/// ihrer Vault-Datei.
///
/// Sicherheit: gelesen wird ausschließlich der Pfad aus `note.file_path` —
/// ein Wert, den allein der Daemon beim Anlegen der Notiz in die DB
/// geschrieben hat, niemals ein vom Frontend übergebener Pfad. `note_id` ist
/// die einzige Eingabe vom Frontend, und sie wählt nur *welche* DB-Zeile
/// gelesen wird, nicht *welche Datei*; das Frontend kann auf diesem Weg also
/// keine beliebige Datei vom System auslesen.
#[tauri::command]
pub fn get_note_markdown(note_id: i64) -> Result<NoteContent, String> {
    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    let db_path = paths::resolve_db_path(&cfg);

    // Wie list_notes oben: eine (noch) nicht existierende DB-Datei enthält
    // keine Notizen — "Notiz nicht gefunden" ist hier der korrekte Fehler.
    // Wichtig ist zusätzlich, `open_readonly` gar nicht erst aufzurufen, wenn
    // die Datei fehlt: `Connection::open` legt eine fehlende SQLite-Datei
    // sonst neu an, was diese App laut D2/D8 nie tun darf.
    if !db_path.exists() {
        return Err("Notiz nicht gefunden".to_string());
    }

    let store = storage::Store::open_readonly(&db_path)
        .map_err(|e| format!("DB nicht lesbar: {e}"))?;

    let note = store
        .get_note(note_id)
        .map_err(|e| format!("Notiz konnte nicht gelesen werden: {e}"))?
        .ok_or_else(|| "Notiz nicht gefunden".to_string())?;

    let markdown = std::fs::read_to_string(&note.file_path).map_err(|e| {
        format!(
            "Notizdatei konnte nicht gelesen werden ({}): {e}",
            note.file_path
        )
    })?;

    Ok(NoteContent { note, markdown })
}

/// Stößt beim Daemon eine Sofort-Destillation für `[from_ms, to_ms]` an.
///
/// Schickt `Request::DistillNow` über dieselbe Named Pipe wie
/// `settings::send_reload_config_over_pipe` (siehe dort für das
/// IPC-Wire-Format). Anders als der dortige best-effort-Reload liefert
/// dieser Command einen echten Fehler zurück, wenn die Pipe nicht geöffnet
/// bzw. beschrieben werden kann (Daemon läuft nicht) — der „Jetzt
/// destillieren"-Button im Frontend soll das sichtbar melden können, statt
/// den Fehler stillschweigend zu verschlucken. Auf eine Antwort vom Daemon
/// wird bewusst nicht gewartet: Die Destillation läuft asynchron im Daemon,
/// das Frontend lädt die Notizliste nach einer kurzen Wartezeit selbst neu.
#[tauri::command]
pub fn distill_now(from_ms: i64, to_ms: i64) -> Result<(), String> {
    #[cfg(windows)]
    {
        send_distill_now_over_pipe(from_ms, to_ms)
            .map_err(|_| "MerkWerk-Daemon nicht erreichbar — läuft er?".to_string())
    }
    #[cfg(not(windows))]
    {
        // Named-Pipe-IPC ist laut ENTSCHEIDUNGEN.md D1/D6 Windows-only; auf
        // anderen Zielplattformen existiert kein Daemon, der erreichbar wäre.
        // Ein echter Fehler statt eines stillen `Ok(())` ist hier ehrlicher:
        // der Aufruf hat faktisch nichts bewirkt.
        let _ = (from_ms, to_ms);
        Err("Destillation ist nur unter Windows verfügbar.".to_string())
    }
}

/// Öffnet die Daemon-Named-Pipe und schickt `Request::DistillNow`.
///
/// Identisches Muster zu `settings::send_reload_config_over_pipe`: Pipe-Name
/// und Wire-Format kommen direkt aus dem `ipc-protocol`-Crate (`PIPE_NAME`,
/// `encode_request`) — so gibt es genau eine Quelle der Wahrheit für das
/// Protokoll, dieselbe, die auch der Daemon-Server parst.
#[cfg(windows)]
fn send_distill_now_over_pipe(from_ms: i64, to_ms: i64) -> std::io::Result<()> {
    use std::io::Write;

    let line = ipc_protocol::encode_request(&ipc_protocol::Request::DistillNow { from_ms, to_ms });

    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(ipc_protocol::PIPE_NAME)?;

    pipe.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::MAX_LIMIT;
    use super::MIN_LIMIT;

    #[test]
    fn limit_bounds_are_sane() {
        assert!(MIN_LIMIT >= 1);
        assert!(MAX_LIMIT >= MIN_LIMIT);
    }
}
