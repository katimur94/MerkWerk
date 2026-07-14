//! Destillat erzeugen und in den Vault schreiben — plattformneutral, nativ testbar.
//!
//! Führt die drei rein datei-/datenbezogenen Schritte einer Destillation aus:
//! den (langsamen) KI-Aufruf über [`distiller::distill`], das Schreiben der
//! Markdown-Datei in den Vault (D10) und das Zurückgeben der Metadaten, die der
//! Erfassungs-Loop dann als einziger DB-Schreiber (D2) via `insert_note`
//! persistiert. Der langsame Teil läuft im Destillier-Worker-Thread
//! (`runtime.rs`), damit der Erfassungs-Loop nie blockiert; nur der kurze
//! `insert_note`-Schreibvorgang kehrt in den Loop zurück.

use std::path::Path;

use distiller::DistillerConfig;

/// Fertig geschriebene Notiz, bereit zum Eintrag in die `notes`-Tabelle.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingNote {
    pub file_path: String,
    pub title: Option<String>,
    pub range_start: i64,
    pub range_end: i64,
    pub created_at: i64,
    pub model: String,
    pub source_snapshot_count: i64,
    /// Embedding des Notiztexts (für semantische Suche, D11). `None`, wenn das
    /// Embedding-Modell nicht erreichbar war — die Notiz bleibt trotzdem gültig,
    /// nur ohne semantischen Index.
    pub embedding: Option<Vec<f32>>,
}

/// Fehler der Destillation bzw. des Vault-Schreibens.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Destillation fehlgeschlagen: {0}")]
    Distill(#[from] distiller::Error),
    #[error("Vault-Schreibfehler ({path}): {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Erzeugt für den Zeitraum `[from_ms, to_ms]` ein Destillat, schreibt es als
/// `.md`-Datei in `vault_path` und liefert die Metadaten zurück.
///
/// `store` darf eine read-only-Verbindung sein — `distiller::distill` liest nur.
/// `created_at` (Unix-ms) bestimmt Dateiname und Metadaten (wird vom Aufrufer
/// gesetzt, damit diese Funktion frei von Wall-clock-Seiteneffekten und damit
/// deterministisch testbar bleibt).
pub fn produce_note(
    store: &storage::Store,
    inference: &dyn inference::Inference,
    cfg: &DistillerConfig,
    vault_path: &Path,
    from_ms: i64,
    to_ms: i64,
    created_at: i64,
) -> Result<PendingNote, Error> {
    let note = distiller::distill(store, inference, cfg, from_ms, to_ms)?;

    std::fs::create_dir_all(vault_path).map_err(|source| Error::Io {
        path: vault_path.display().to_string(),
        source,
    })?;

    // Dateiname aus der Erstellzeit (eindeutig, sortierbar, ohne Kalender-Mathe).
    let file = vault_path.join(format!("note-{created_at}.md"));
    std::fs::write(&file, &note.markdown).map_err(|source| Error::Io {
        path: file.display().to_string(),
        source,
    })?;

    // Embedding des Notiztexts für die semantische Suche (D11). Ein Fehler des
    // Embedding-Modells darf die Notiz nicht scheitern lassen — dann eben ohne
    // semantischen Index (`None`); der Aufrufer loggt das.
    let embedding = inference.embed(&note.markdown).ok();

    Ok(PendingNote {
        file_path: file.to_string_lossy().into_owned(),
        title: note.title,
        range_start: note.range_start,
        range_end: note.range_end,
        created_at,
        model: cfg.model.clone(),
        source_snapshot_count: note.source_snapshot_count as i64,
        embedding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inference::MockInference;
    use storage::Store;

    #[test]
    fn produce_note_writes_vault_file_and_returns_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("merkwerk.sqlite3");
        let vault = dir.path().join("vault");

        // Etwas Aktivität anlegen, damit distill nicht den Leerfall trifft.
        let store = Store::open(&db).unwrap();
        let sid = store.insert_app_session("code.exe", 1_000, None).unwrap();
        store
            .insert_snapshot(
                Some(sid),
                None,
                1_500,
                Some("main.rs — MerkWerk"),
                None,
                Some("fn main() {}"),
                false,
                None,
            )
            .unwrap();

        let inference = MockInference::with_response("# Arbeit an MerkWerk\n- Rust-Code");
        let cfg = DistillerConfig::default();

        let pending =
            produce_note(&store, &inference, &cfg, &vault, 0, 10_000, 5_555).unwrap();

        // Datei wurde geschrieben und enthält das Destillat.
        let written = std::fs::read_to_string(&pending.file_path).unwrap();
        assert_eq!(written, "# Arbeit an MerkWerk\n- Rust-Code");
        assert!(pending.file_path.ends_with("note-5555.md"));
        assert_eq!(pending.title.as_deref(), Some("Arbeit an MerkWerk"));
        assert_eq!(pending.created_at, 5_555);
        assert_eq!(pending.model, cfg.model);
        assert_eq!(pending.source_snapshot_count, 1);
        // Das Embedding (für semantische Suche) wurde mitberechnet.
        assert!(pending.embedding.is_some());

        // Die zurückgegebenen Metadaten lassen sich in die notes-Tabelle eintragen.
        let note_id = store
            .insert_note(
                &pending.file_path,
                pending.title.as_deref(),
                pending.range_start,
                pending.range_end,
                pending.created_at,
                Some(&pending.model),
                pending.source_snapshot_count,
            )
            .unwrap();
        assert!(note_id > 0);
    }

    #[test]
    fn produce_note_handles_empty_range_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("merkwerk.sqlite3");
        let vault = dir.path().join("vault");
        let store = Store::open(&db).unwrap();

        let inference = MockInference::new();
        let cfg = DistillerConfig::default();

        // Leerer Zeitraum: distill liefert die "keine Aktivität"-Notiz, die Datei
        // wird trotzdem geschrieben (leere Tage sollen nachvollziehbar sein).
        let pending =
            produce_note(&store, &inference, &cfg, &vault, 0, 10_000, 42).unwrap();
        assert_eq!(pending.source_snapshot_count, 0);
        assert!(std::path::Path::new(&pending.file_path).exists());
    }
}
