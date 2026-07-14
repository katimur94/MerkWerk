import { useCallback, useState, type CSSProperties, type KeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Semantische Notizsuche (ENTSCHEIDUNGEN.md D11, ROADMAP.md Etappe 3): bettet
// die Suchanfrage über Ollama ein und vergleicht sie per Cosinus-Ähnlichkeit
// gegen die Notiz-Embeddings — Tauri-Command `semantic_search_notes(query,
// limit)` (siehe src-tauri/src/semantic.rs, das intern
// `storage::Store::search_notes_semantic` aufruft). Anders als SearchView
// (FTS5 über Snapshot-Text) durchsucht dies die destillierten *Notizen* nach
// Bedeutungsähnlichkeit, nicht nach wörtlicher Übereinstimmung, und braucht
// dafür einen erreichbaren lokalen Ollama-Server zum Einbetten der Anfrage —
// schlägt das fehl, liefert der Command einen Fehler, der genau das sagt
// (siehe `formatInvokeError` unten), statt still leer zu bleiben.
//
// Klick auf einen Treffer lädt den Markdown-Inhalt der zugehörigen Notiz über
// denselben `get_note_markdown(noteId)`-Command wie NotesView und zeigt ihn
// in einem scrollbaren <pre> (gleiches Darstellungsmuster wie dort).

/**
 * Ein Treffer der semantischen Suche. Muss exakt zu `storage::SemanticHit`
 * (daemon/storage/src/model.rs) passen, das `src-tauri/src/semantic.rs`
 * unverändert durchreicht — snake_case, kein `rename_all`.
 */
interface SemanticHit {
  note_id: number;
  /** Cosinus-Ähnlichkeit zur Suchanfrage, i. d. R. in [-1.0, 1.0]. Höher = ähnlicher. */
  score: number;
  title: string | null;
  /** Pfad zur `.md`-Datei im Vault (Quelle der Wahrheit für den Inhalt). */
  file_path: string;
  /** Unix-ms: Anfang des Quell-Zeitraums. */
  range_start: number;
  /** Unix-ms: Ende des Quell-Zeitraums. */
  range_end: number;
  /** Unix-ms: Erstellzeit der Notiz. */
  created_at: number;
}

/**
 * Nur der hier tatsächlich benötigte Ausschnitt der Antwort von
 * `get_note_markdown` (siehe `notes::NoteContent` in src-tauri/src/notes.rs)
 * — Anzeige-Metadaten (Titel/Datum/Score) liefert bereits der Treffer selbst
 * (`SemanticHit`), daher wird `note` hier bewusst nicht dupliziert.
 */
interface NoteMarkdown {
  markdown: string;
}

/** An `invoke("semantic_search_notes", { query, limit })` übergebene Obergrenze. */
const RESULT_LIMIT = 20;

type SearchStatus =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "success"; hits: SemanticHit[]; query: string }
  | { kind: "error"; message: string };

type ContentStatus =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "success"; markdown: string }
  | { kind: "error"; message: string };

const NO_TAURI_MESSAGE =
  "Kein Tauri-Kontext erkannt (Browser-Vorschau?). Die semantische Suche kann nur " +
  "innerhalb der MerkWerk-Desktop-App verwendet werden.";

/** Formatiert einen Unix-ms-Zeitstempel als lokales "DD.MM.YYYY"-Datum. */
function formatDate(ms: number): string {
  const d = new Date(ms);
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  return `${dd}.${mm}.${d.getFullYear()}`;
}

/** Formatiert eine Kosinus-Ähnlichkeit (i. d. R. [-1.0, 1.0]) als Prozentzahl mit einer Nachkommastelle. */
function formatScore(score: number): string {
  return `${(score * 100).toFixed(1)} %`;
}

/**
 * Formatiert Fehler aus `invoke()`. Tauri-Command-Fehler sind Strings (siehe
 * `Result<_, String>` in semantic.rs/notes.rs) — `err instanceof Error`
 * deckt daneben den Fall ab, dass `invoke` selbst technisch fehlschlägt.
 */
function formatInvokeError(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function SemanticSearchView() {
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<SearchStatus>({ kind: "idle" });

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [contentStatus, setContentStatus] = useState<ContentStatus>({ kind: "idle" });

  const runSearch = useCallback(async () => {
    const trimmed = query.trim();
    if (trimmed.length === 0) {
      setStatus({ kind: "idle" });
      return;
    }

    setStatus({ kind: "loading" });
    setSelectedId(null);
    setContentStatus({ kind: "idle" });

    // Reine Browser-Vorschau (z. B. `vite dev`/`vite preview` ohne die
    // Tauri-Laufzeit) hat kein IPC-Backend — `invoke` würde werfen. Das hier
    // vorab abfangen liefert einen klaren Hinweis statt eines Absturzes.
    if (!isTauri()) {
      setStatus({ kind: "error", message: NO_TAURI_MESSAGE });
      return;
    }

    try {
      const hits = await invoke<SemanticHit[]>("semantic_search_notes", {
        query: trimmed,
        limit: RESULT_LIMIT,
      });
      setStatus({ kind: "success", hits, query: trimmed });
    } catch (err) {
      // `semantic_search_notes` liefert bei Fehlern `Err(String)` (siehe
      // src-tauri/src/semantic.rs) — u. a. wenn der lokale Ollama-Server
      // nicht erreichbar ist. invoke() lehnt das Promise damit ab; die
      // Meldung wird unverändert angezeigt, damit "Ollama läuft nicht" von
      // "keine Treffer" unterscheidbar bleibt.
      setStatus({ kind: "error", message: formatInvokeError(err) });
    }
  }, [query]);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Enter") {
        void runSearch();
      }
    },
    [runSearch],
  );

  const selectHit = useCallback((hit: SemanticHit) => {
    setSelectedId(hit.note_id);

    if (!isTauri()) {
      setContentStatus({ kind: "error", message: NO_TAURI_MESSAGE });
      return;
    }

    setContentStatus({ kind: "loading" });

    void (async () => {
      try {
        const content = await invoke<NoteMarkdown>("get_note_markdown", {
          noteId: hit.note_id,
        });
        setContentStatus({ kind: "success", markdown: content.markdown });
      } catch (err) {
        // `get_note_markdown` liefert bei Fehlern `Err(String)` (siehe
        // src-tauri/src/notes.rs), z. B. wenn die Notiz-Datei fehlt.
        setContentStatus({ kind: "error", message: formatInvokeError(err) });
      }
    })();
  }, []);

  const loading = status.kind === "loading";

  return (
    <section aria-label="Semantische Suche">
      <div style={styles.header}>
        <h2 style={styles.heading}>Semantische Suche</h2>
      </div>

      <div style={styles.searchBar}>
        <input
          type="text"
          value={query}
          placeholder="Wonach suchst du (in eigenen Worten, kein Stichwort nötig)…"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={handleKeyDown}
          style={styles.input}
          aria-label="Semantische Suchanfrage"
        />
        <button onClick={() => void runSearch()} disabled={loading}>
          {loading ? "Sucht…" : "Suchen"}
        </button>
      </div>

      {status.kind === "idle" && <p>Suchbegriff eingeben und "Suchen" oder Enter drücken.</p>}

      {status.kind === "error" && <p style={styles.error}>{status.message}</p>}

      {status.kind === "success" && status.hits.length === 0 && (
        <p>Keine semantisch ähnlichen Notizen für: {status.query}</p>
      )}

      {status.kind === "success" && status.hits.length > 0 && (
        <div style={styles.layout}>
          <ul style={styles.list}>
            {status.hits.map((hit) => {
              const label = hit.title ?? `Notiz vom ${formatDate(hit.range_start)}`;
              const selected = hit.note_id === selectedId;
              return (
                <li key={hit.note_id} style={styles.listItem}>
                  <button
                    type="button"
                    onClick={() => selectHit(hit)}
                    style={
                      selected
                        ? { ...styles.listButton, ...styles.listButtonSelected }
                        : styles.listButton
                    }
                  >
                    <div style={styles.listTitle}>{label}</div>
                    <div style={styles.listMeta}>
                      {formatScore(hit.score)} Ähnlichkeit · {formatDate(hit.created_at)}
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>

          <div style={styles.content}>
            {contentStatus.kind === "idle" && (
              <p>Treffer aus der Liste auswählen, um die Notiz anzuzeigen.</p>
            )}
            {contentStatus.kind === "loading" && <p>Lädt…</p>}
            {contentStatus.kind === "error" && <p style={styles.error}>{contentStatus.message}</p>}
            {contentStatus.kind === "success" && (
              <pre style={styles.markdown}>{contentStatus.markdown}</pre>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

const styles: Record<string, CSSProperties> = {
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "1rem",
  },
  heading: {
    margin: 0,
  },
  searchBar: {
    display: "flex",
    gap: "0.5rem",
    margin: "0.5rem 0 1rem",
  },
  input: {
    flex: 1,
    padding: "4px 8px",
  },
  error: {
    color: "#b00020",
  },
  layout: {
    display: "flex",
    gap: "1rem",
    marginTop: "0.5rem",
    alignItems: "flex-start",
  },
  list: {
    listStyle: "none",
    margin: 0,
    padding: 0,
    width: "18rem",
    flexShrink: 0,
    display: "flex",
    flexDirection: "column",
    gap: "0.35rem",
    maxHeight: "70vh",
    overflowY: "auto",
  },
  listItem: {
    margin: 0,
  },
  listButton: {
    width: "100%",
    textAlign: "left",
    padding: "0.5rem",
    border: "1px solid var(--border-color, #ccc)",
    background: "transparent",
    cursor: "pointer",
    borderRadius: "4px",
  },
  listButtonSelected: {
    borderColor: "#396cd8",
    background: "rgba(57, 108, 216, 0.1)",
  },
  listTitle: {
    fontWeight: 600,
  },
  listMeta: {
    fontSize: "0.85em",
    color: "#666",
  },
  content: {
    flex: 1,
    minWidth: 0,
  },
  markdown: {
    whiteSpace: "pre-wrap",
    overflowY: "auto",
    maxHeight: "60vh",
    border: "1px solid var(--border-color, #ccc)",
    borderRadius: "4px",
    padding: "0.75rem",
    marginTop: "0.5rem",
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: "0.9em",
  },
};
