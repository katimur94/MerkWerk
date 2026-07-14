//! Semantische Notizsuche (ENTSCHEIDUNGEN.md D11, ROADMAP.md Etappe 3):
//! bettet die Suchanfrage über Ollama ein (`inference::OllamaBackend::embed`,
//! D9) und vergleicht sie per Cosinus-Ähnlichkeit gegen die in
//! `note_embeddings` gespeicherten Notiz-Embeddings
//! (`storage::Store::search_notes_semantic`, Migration v4).
//!
//! Gleiches read-only-Muster wie `notes.rs`/`search.rs` (siehe dort für die
//! ausführliche Begründung): `merkwerk-app` öffnet die Daemon-DB
//! ausschließlich über `storage::Store::open_readonly` (erzwingt `PRAGMA
//! query_only`, siehe ARCHITEKTUR.md, ENTSCHEIDUNGEN.md D2/D8) und schreibt
//! nie hinein. Neu gegenüber den anderen Such-Commands: das Einbetten der
//! Suchanfrage braucht einen erreichbaren lokalen Ollama-Server — schlägt
//! das fehl, liefert dieser Command einen Fehler, der das im Klartext sagt,
//! statt eine leere Trefferliste vorzutäuschen (die DB selbst ist davon gar
//! nicht betroffen, "kein Treffer" und "Ollama läuft nicht" sind für die
//! Nutzerin zwei verschiedene Dinge).

use inference::Inference;

use crate::paths;

/// Untere/obere Grenze, auf die `limit` geklemmt wird, bevor es an
/// `storage::Store::search_notes_semantic` weitergereicht wird — gleiches
/// Muster wie in `notes.rs`/`search.rs` (`MIN_LIMIT`/`MAX_LIMIT`): schützt
/// vor `limit <= 0` und vor unbegrenzt großen Anfragen vom Frontend.
const MIN_LIMIT: i64 = 1;
const MAX_LIMIT: i64 = 100;

/// Durchsucht die Notiz-Embeddings per Cosinus-Ähnlichkeit zur eingebetteten
/// `query` und liefert bis zu `limit` Treffer (höchste Ähnlichkeit zuerst,
/// siehe `Store::search_notes_semantic`).
///
/// Ablauf:
///   1. Eine leere/nur-Whitespace-Query liefert `Ok(vec![])`, ohne Ollama
///      oder die DB überhaupt anzufassen (gleiches Muster wie
///      `search::search_snapshots`).
///   2. Konfig laden, DB-Pfad auflösen.
///   3. Existiert die DB-Datei noch nicht (Daemon lief noch nie oder hat
///      noch nie destilliert), ist eine leere Trefferliste der korrekte
///      Erfolgsfall (gleiches Muster wie `notes::list_notes`) — dieser Fall
///      spart sich zusätzlich den Ollama-Aufruf.
///   4. Die Suchanfrage wird per `OllamaBackend::embed` in einen Vektor
///      umgewandelt. Schlägt das fehl (Ollama nicht erreichbar, Modell
///      fehlt, ...), ist das ein echter Fehler statt eines leeren
///      Ergebnisses.
///   5. DB read-only öffnen und den Query-Vektor per Cosinus vergleichen.
#[tauri::command]
pub fn semantic_search_notes(
    query: String,
    limit: i64,
) -> Result<Vec<storage::SemanticHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let clamped_limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);

    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    let db_path = paths::resolve_db_path(&cfg);

    // Der Daemon legt die DB-Datei erst beim ersten Schreibzugriff an. Lief
    // er noch nie, ist "keine Treffer" der korrekte Zustand — kein Fehler.
    // Dieser Check steht bewusst VOR dem Ollama-Aufruf: ein frischer Daemon
    // ohne Notizen soll nicht erst auf eine Embedding-Antwort warten, nur um
    // danach doch eine leere Liste zu liefern.
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let backend = inference::OllamaBackend::new(
        cfg.ai.endpoint.clone(),
        cfg.ai.model.clone(),
        cfg.ai.embed_model.clone(),
    );
    let query_vector = backend.embed(&query).map_err(|e| {
        format!("Semantische Suche braucht Ollama — läuft der lokale Server? ({e})")
    })?;

    let store = storage::Store::open_readonly(&db_path)
        .map_err(|e| format!("DB nicht lesbar: {e}"))?;

    store
        .search_notes_semantic(&query_vector, clamped_limit)
        .map_err(|e| format!("Semantische Suche fehlgeschlagen: {e}"))
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
