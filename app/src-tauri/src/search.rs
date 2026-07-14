//! Such-Command: durchsucht die Daemon-DB read-only per FTS5 (`storage::Store::search`)
//! und liefert Treffer an das Frontend.
//!
//! Gleiches Muster wie `timeline.rs` (siehe dort für die ausführliche
//! Begründung): `merkwerk-app` öffnet die DB ausschließlich über
//! `storage::Store::open_readonly` (erzwingt `PRAGMA query_only`, siehe
//! ARCHITEKTUR.md, ENTSCHEIDUNGEN.md D2/D8) und schreibt nie.

use crate::paths;

/// Untere/obere Grenze, auf die `limit` geklemmt wird, bevor es an
/// `storage::Store::search` weitergereicht wird — schützt vor `limit <= 0`
/// (SQLite `LIMIT` mit 0 oder negativen Werten ist nicht das, was ein
/// Aufrufer hier erwartet) und vor unbegrenzt großen Anfragen vom Frontend.
const MIN_LIMIT: i64 = 1;
const MAX_LIMIT: i64 = 200;

/// Durchsucht Fenstertitel/URL/Snapshot-Text per Volltextsuche und liefert
/// bis zu `limit` Treffer (neueste Relevanz zuerst, siehe `Store::search`).
///
/// Ablauf identisch zu `timeline::list_timeline`: Konfig laden → DB-Pfad
/// auflösen → wenn die DB-Datei noch nicht existiert (Daemon lief noch nie)
/// ist eine leere Trefferliste der korrekte Erfolgsfall → sonst read-only
/// öffnen und suchen. Eine leere/nur-Whitespace-Query liefert ebenfalls
/// `Ok(vec![])`, ohne die DB überhaupt zu öffnen (`Store::search` macht das
/// zwar auch selbst, aber so spart sich eine leere Suche vom leeren
/// Suchfeld aus jeden DB-Zugriff).
#[tauri::command]
pub fn search_snapshots(query: String, limit: i64) -> Result<Vec<storage::SearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let clamped_limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);

    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    let db_path = paths::resolve_db_path(&cfg);

    // Der Daemon legt die DB-Datei erst beim ersten Schreibzugriff an. Lief
    // er noch nie, ist "keine Treffer" der korrekte Zustand — kein Fehler.
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let store = storage::Store::open_readonly(&db_path)
        .map_err(|e| format!("DB nicht lesbar: {e}"))?;

    store
        .search(&query, clamped_limit)
        .map_err(|e| format!("Suche fehlgeschlagen: {e}"))
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
