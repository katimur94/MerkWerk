import { useCallback, useState, type CSSProperties, type KeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Such-Ansicht: Volltextsuche über Fenstertitel/URL/Snapshot-Text via den
// Tauri-Command `search_snapshots(query, limit)` (siehe
// src-tauri/src/search.rs, das intern `storage::Store::search` — FTS5 —
// aufruft) und rendert die Treffer als Tabelle. Wie TimelineView liest das
// ausschließlich die read-only Daemon-DB (ARCHITEKTUR.md, ENTSCHEIDUNGEN.md
// D2/D8); anders als TimelineView wird nicht beim Mounten automatisch
// geladen, sondern erst auf Nutzeraktion (Button/Enter).

/**
 * Ein Treffer der Volltextsuche. Muss exakt zu `storage::SearchHit`
 * (daemon/storage/src/model.rs) passen, das `src-tauri/src/search.rs`
 * unverändert durchreicht — snake_case, kein `rename_all`.
 */
interface SearchHit {
  snapshot_id: number;
  session_id: number | null;
  /** Unix-ms. */
  ts: number;
  window_title: string | null;
  url: string | null;
  /**
   * Ausschnitt aus dem Snapshot-Text rund um den Treffer, von FTS5s
   * `snippet()` erzeugt: enthält `[...]`-Markierungen um die getroffenen
   * Tokens und `…` für ausgelassenen Text. Wird hier bewusst roh
   * angezeigt, ohne die Markierungen weiter zu verarbeiten.
   */
  snippet: string;
}

/** An `invoke("search_snapshots", { query, limit })` übergebene Obergrenze. */
const RESULT_LIMIT = 50;

type SearchStatus =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "success"; hits: SearchHit[]; query: string }
  | { kind: "error"; message: string };

/** Formatiert einen Unix-ms-Zeitstempel als lokale "HH:MM"-Uhrzeit. */
function formatTime(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function SearchView() {
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<SearchStatus>({ kind: "idle" });

  const runSearch = useCallback(async () => {
    const trimmed = query.trim();
    if (trimmed.length === 0) {
      setStatus({ kind: "idle" });
      return;
    }

    setStatus({ kind: "loading" });

    // Reine Browser-Vorschau (z. B. `vite dev`/`vite preview` ohne die
    // Tauri-Laufzeit) hat kein IPC-Backend — `invoke` würde werfen. Das hier
    // vorab abfangen liefert einen klaren Hinweis statt eines Absturzes.
    if (!isTauri()) {
      setStatus({
        kind: "error",
        message:
          "Kein Tauri-Kontext erkannt (Browser-Vorschau?). Die Suche kann nur " +
          "innerhalb der MerkWerk-Desktop-App verwendet werden.",
      });
      return;
    }

    try {
      const hits = await invoke<SearchHit[]>("search_snapshots", {
        query: trimmed,
        limit: RESULT_LIMIT,
      });
      setStatus({ kind: "success", hits, query: trimmed });
    } catch (err) {
      // `search_snapshots` liefert bei Fehlern `Err(String)` (siehe
      // src-tauri/src/search.rs) — invoke() lehnt das Promise damit ab.
      setStatus({
        kind: "error",
        message: err instanceof Error ? err.message : String(err),
      });
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

  const loading = status.kind === "loading";

  return (
    <section aria-label="Suche">
      <div style={styles.header}>
        <h2 style={styles.heading}>Suche</h2>
      </div>

      <div style={styles.searchBar}>
        <input
          type="text"
          value={query}
          placeholder="Volltextsuche über Fenstertitel, URL und erfassten Text…"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={handleKeyDown}
          style={styles.input}
          aria-label="Suchbegriff"
        />
        <button onClick={() => void runSearch()} disabled={loading}>
          {loading ? "Sucht…" : "Suchen"}
        </button>
      </div>

      {status.kind === "idle" && <p>Suchbegriff eingeben und "Suchen" oder Enter drücken.</p>}

      {status.kind === "error" && <p style={styles.error}>{status.message}</p>}

      {status.kind === "success" && status.hits.length === 0 && (
        <p>Keine Treffer für: {status.query}</p>
      )}

      {status.kind === "success" && status.hits.length > 0 && (
        <table style={styles.table}>
          <thead>
            <tr>
              <th style={styles.th}>Zeit</th>
              <th style={styles.th}>Fenster / URL</th>
              <th style={styles.th}>Ausschnitt</th>
            </tr>
          </thead>
          <tbody>
            {status.hits.map((hit) => (
              <tr key={hit.snapshot_id}>
                <td style={styles.tdNowrap}>{formatTime(hit.ts)}</td>
                <td style={styles.td}>
                  {hit.window_title ?? "–"}
                  {hit.url && (
                    <>
                      <br />
                      <span style={styles.url}>{hit.url}</span>
                    </>
                  )}
                </td>
                <td style={styles.td}>{hit.snippet || "–"}</td>
              </tr>
            ))}
          </tbody>
        </table>
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
  table: {
    width: "100%",
    borderCollapse: "collapse",
    fontSize: "0.9rem",
  },
  th: {
    textAlign: "left",
    borderBottom: "1px solid var(--border-color, #ccc)",
    padding: "4px 8px",
    whiteSpace: "nowrap",
  },
  td: {
    borderBottom: "1px solid var(--border-color, #eee)",
    padding: "4px 8px",
    verticalAlign: "top",
  },
  tdNowrap: {
    borderBottom: "1px solid var(--border-color, #eee)",
    padding: "4px 8px",
    verticalAlign: "top",
    whiteSpace: "nowrap",
  },
  url: {
    color: "#666",
    fontSize: "0.85em",
    wordBreak: "break-all",
  },
};
