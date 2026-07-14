import { useCallback, useEffect, useState, type CSSProperties } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Timeline-Ansicht: lädt `app_sessions` (+ jüngster Snapshot je Session) für
// das heutige Zeitfenster über den Tauri-Command `list_timeline(from_ms,
// to_ms)` (siehe src-tauri/src/timeline.rs) und rendert sie als Tabelle.
// Die App liest die Daemon-DB ausschließlich read-only (ARCHITEKTUR.md,
// ENTSCHEIDUNGEN.md D2/D8) — dieser Command tut nichts anderes.

/**
 * Eine Zeile der Timeline. Muss exakt zu `TimelineEntry` in
 * `src-tauri/src/timeline.rs` passen — die Felder kommen unverändert
 * (snake_case) vom serde-Serializer, es gibt keine `rename_all`-Umbenennung.
 */
interface TimelineEntry {
  session_id: number;
  process_name: string;
  /** Unix-ms. */
  started_at: number;
  /** Unix-ms; `null` = Session läuft noch. */
  ended_at: number | null;
  window_title: string | null;
  url: string | null;
  /** Gekürzte Vorschau des Snapshot-Texts. */
  text_preview: string | null;
}

/** Formatiert einen Unix-ms-Zeitstempel als lokale "HH:MM"-Uhrzeit. */
function formatTime(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** "HH:MM – HH:MM", bzw. "HH:MM – läuft" für eine noch laufende Session. */
function formatRange(startedAt: number, endedAt: number | null): string {
  const start = formatTime(startedAt);
  const end = endedAt === null ? "läuft" : formatTime(endedAt);
  return `${start} – ${end}`;
}

/** Heutiges Zeitfenster [00:00 Uhr, jetzt] in Unix-ms. */
function todayRangeMs(): { fromMs: number; toMs: number } {
  const now = new Date();
  const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return { fromMs: startOfDay.getTime(), toMs: now.getTime() };
}

export function TimelineView() {
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadTimeline = useCallback(async () => {
    setLoading(true);
    setError(null);

    // Reine Browser-Vorschau (z. B. `vite dev`/`vite preview` ohne die
    // Tauri-Laufzeit) hat kein IPC-Backend — `invoke` würde werfen. Das hier
    // vorab abfangen liefert einen klaren Hinweis statt eines Absturzes.
    if (!isTauri()) {
      setError(
        "Kein Tauri-Kontext erkannt (Browser-Vorschau?). Die Timeline kann nur " +
          "innerhalb der MerkWerk-Desktop-App geladen werden.",
      );
      setEntries([]);
      setLoading(false);
      return;
    }

    try {
      const { fromMs, toMs } = todayRangeMs();
      const result = await invoke<TimelineEntry[]>("list_timeline", { fromMs, toMs });
      setEntries(result);
    } catch (err) {
      // `list_timeline` liefert bei Fehlern `Err(String)` (siehe
      // src-tauri/src/timeline.rs) — invoke() lehnt das Promise damit ab.
      setError(err instanceof Error ? err.message : String(err));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadTimeline();
  }, [loadTimeline]);

  return (
    <section aria-label="Timeline">
      <div style={styles.header}>
        <h2 style={styles.heading}>Timeline</h2>
        <button onClick={() => void loadTimeline()} disabled={loading}>
          {loading ? "Lädt…" : "Aktualisieren"}
        </button>
      </div>

      {error && <p style={styles.error}>{error}</p>}

      {!error && !loading && entries.length === 0 && <p>Noch keine Aktivität heute.</p>}

      {entries.length > 0 && (
        <table style={styles.table}>
          <thead>
            <tr>
              <th style={styles.th}>Zeit</th>
              <th style={styles.th}>Programm</th>
              <th style={styles.th}>Fenster / URL</th>
              <th style={styles.th}>Vorschau</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.session_id}>
                <td style={styles.tdNowrap}>{formatRange(entry.started_at, entry.ended_at)}</td>
                <td style={styles.td}>{entry.process_name}</td>
                <td style={styles.td}>
                  {entry.window_title ?? "–"}
                  {entry.url && (
                    <>
                      <br />
                      <span style={styles.url}>{entry.url}</span>
                    </>
                  )}
                </td>
                <td style={styles.td}>{entry.text_preview ?? "–"}</td>
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
