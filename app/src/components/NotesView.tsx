import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Notizen-Ansicht: lädt die zuletzt erstellten KI-Notizen (Metadaten) über
// den Tauri-Command `list_notes(limit)` (siehe src-tauri/src/notes.rs),
// zeigt den Markdown-Inhalt einer ausgewählten Notiz über
// `get_note_markdown(id)` an und stößt über den Button „Jetzt destillieren
// (heute)" per `distill_now(fromMs, toMs)` eine Sofort-Destillation beim
// Daemon an (IPC über die Named Pipe, siehe ENTSCHEIDUNGEN.md D1).
//
// Wie TimelineView/SearchView liest diese Ansicht die Daemon-DB nur
// read-only (ARCHITEKTUR.md, ENTSCHEIDUNGEN.md D2/D8); `distill_now`
// schickt lediglich ein Kommando an den Daemon, der die eigentliche Arbeit
// (und jeden Schreibzugriff auf die DB) selbst übernimmt — die App schreibt
// nie direkt.

/**
 * Eine Zeile der Notizliste. Muss exakt zu `storage::NoteRow`
 * (daemon/storage/src/model.rs) passen, das `src-tauri/src/notes.rs`
 * unverändert durchreicht — snake_case, kein `rename_all`.
 */
interface NoteRow {
  id: number;
  /** Pfad zur `.md`-Datei im Vault (Quelle der Wahrheit für den Inhalt). */
  file_path: string;
  title: string | null;
  /** Unix-ms: Anfang des Quell-Zeitraums. */
  range_start: number;
  /** Unix-ms: Ende des Quell-Zeitraums. */
  range_end: number;
  /** Unix-ms: Erstellzeit der Notiz. */
  created_at: number;
  model: string | null;
  source_snapshot_count: number;
}

/**
 * Antwort von `get_note_markdown` — Metadaten der Notiz plus der geladene
 * Markdown-Text ihrer Vault-Datei. Spiegelt `notes::NoteContent`
 * (src-tauri/src/notes.rs) 1:1.
 */
interface NoteContent {
  note: NoteRow;
  markdown: string;
}

/** An `invoke("list_notes", { limit })` übergebene Obergrenze. */
const NOTE_LIMIT = 50;

/**
 * Wartezeit, bis nach angestoßener Destillation die Liste automatisch neu
 * geladen wird. Der Daemon destilliert asynchron im Hintergrund — 1,5 s sind
 * kein Garant, dass die neue Notiz schon fertig ist, aber ein pragmatischer
 * erster Versuch; die Liste lässt sich jederzeit auch manuell aktualisieren.
 */
const RELOAD_DELAY_MS = 1500;

type ContentStatus =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "success"; content: NoteContent }
  | { kind: "error"; message: string };

type DistillStatus =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "success"; message: string }
  | { kind: "error"; message: string };

const NO_TAURI_MESSAGE =
  "Kein Tauri-Kontext erkannt (Browser-Vorschau?). Notizen können nur innerhalb der MerkWerk-Desktop-App geladen werden.";

/** Formatiert einen Unix-ms-Zeitstempel als lokale "HH:MM"-Uhrzeit. */
function formatTime(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** Formatiert einen Unix-ms-Zeitstempel als lokales "DD.MM.YYYY"-Datum. */
function formatDate(ms: number): string {
  const d = new Date(ms);
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  return `${dd}.${mm}.${d.getFullYear()}`;
}

/** Formatiert einen Unix-ms-Zeitstempel als "DD.MM.YYYY HH:MM". */
function formatDateTime(ms: number): string {
  return `${formatDate(ms)} ${formatTime(ms)}`;
}

function isSameDay(aMs: number, bMs: number): boolean {
  const a = new Date(aMs);
  const b = new Date(bMs);
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/**
 * Formatiert einen Quell-Zeitraum: "DD.MM.YYYY, HH:MM – HH:MM" wenn Start
 * und Ende am selben Tag liegen, sonst zwei volle Datum/Uhrzeit-Angaben.
 */
function formatRange(startMs: number, endMs: number): string {
  if (isSameDay(startMs, endMs)) {
    return `${formatDate(startMs)}, ${formatTime(startMs)} – ${formatTime(endMs)}`;
  }
  return `${formatDateTime(startMs)} – ${formatDateTime(endMs)}`;
}

/** Heutiges Zeitfenster [00:00 Uhr, jetzt] in Unix-ms. */
function todayRangeMs(): { fromMs: number; toMs: number } {
  const now = new Date();
  const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return { fromMs: startOfDay.getTime(), toMs: now.getTime() };
}

/**
 * Formatiert Fehler aus `invoke()`. Tauri-Command-Fehler sind Strings (siehe
 * `Result<_, String>` in notes.rs) — `err instanceof Error` deckt daneben
 * den Fall ab, dass `invoke` selbst technisch fehlschlägt.
 */
function formatInvokeError(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function NotesView() {
  const [notes, setNotes] = useState<NoteRow[]>([]);
  const [notesLoading, setNotesLoading] = useState(true);
  const [notesError, setNotesError] = useState<string | null>(null);

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [contentStatus, setContentStatus] = useState<ContentStatus>({ kind: "idle" });

  const [distillStatus, setDistillStatus] = useState<DistillStatus>({ kind: "idle" });
  const reloadTimeoutRef = useRef<number | undefined>(undefined);

  const loadNotes = useCallback(async () => {
    setNotesLoading(true);
    setNotesError(null);

    // Reine Browser-Vorschau (z. B. `vite dev`/`vite preview` ohne die
    // Tauri-Laufzeit) hat kein IPC-Backend — `invoke` würde werfen. Das hier
    // vorab abfangen liefert einen klaren Hinweis statt eines Absturzes.
    if (!isTauri()) {
      setNotesError(NO_TAURI_MESSAGE);
      setNotes([]);
      setNotesLoading(false);
      return;
    }

    try {
      const result = await invoke<NoteRow[]>("list_notes", { limit: NOTE_LIMIT });
      setNotes(result);
    } catch (err) {
      // `list_notes` liefert bei Fehlern `Err(String)` (siehe
      // src-tauri/src/notes.rs) — invoke() lehnt das Promise damit ab.
      setNotesError(formatInvokeError(err));
      setNotes([]);
    } finally {
      setNotesLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadNotes();
  }, [loadNotes]);

  // Ausstehendes Nachladen nach einer angestoßenen Destillation abbrechen,
  // wenn die Ansicht vorher verlassen wird.
  useEffect(() => {
    return () => {
      if (reloadTimeoutRef.current !== undefined) {
        window.clearTimeout(reloadTimeoutRef.current);
      }
    };
  }, []);

  const selectNote = useCallback((id: number) => {
    setSelectedId(id);

    if (!isTauri()) {
      setContentStatus({ kind: "error", message: NO_TAURI_MESSAGE });
      return;
    }

    setContentStatus({ kind: "loading" });

    void (async () => {
      try {
        const content = await invoke<NoteContent>("get_note_markdown", { noteId: id });
        setContentStatus({ kind: "success", content });
      } catch (err) {
        // `get_note_markdown` liefert bei Fehlern `Err(String)` (siehe
        // src-tauri/src/notes.rs), z. B. wenn die Notiz-Datei fehlt.
        setContentStatus({ kind: "error", message: formatInvokeError(err) });
      }
    })();
  }, []);

  const handleDistillNow = useCallback(() => {
    if (!isTauri()) {
      setDistillStatus({ kind: "error", message: NO_TAURI_MESSAGE });
      return;
    }

    setDistillStatus({ kind: "running" });

    void (async () => {
      try {
        const { fromMs, toMs } = todayRangeMs();
        await invoke("distill_now", { fromMs, toMs });
        setDistillStatus({
          kind: "success",
          message: "Destillation angestoßen — der Daemon arbeitet im Hintergrund weiter.",
        });

        // Der Daemon destilliert asynchron; die Liste einmal automatisch neu
        // laden, damit eine neue Notiz ohne manuellen Klick auftaucht.
        reloadTimeoutRef.current = window.setTimeout(() => {
          void loadNotes();
        }, RELOAD_DELAY_MS);
      } catch (err) {
        // `distill_now` liefert bei Fehlern `Err(String)` (siehe
        // src-tauri/src/notes.rs), u. a. wenn der Daemon nicht erreichbar ist.
        setDistillStatus({ kind: "error", message: formatInvokeError(err) });
      }
    })();
  }, [loadNotes]);

  const distilling = distillStatus.kind === "running";

  return (
    <section aria-label="Notizen">
      <div style={styles.header}>
        <h2 style={styles.heading}>Notizen</h2>
        <div style={styles.headerActions}>
          <button onClick={handleDistillNow} disabled={distilling}>
            {distilling ? "Destilliert…" : "Jetzt destillieren (heute)"}
          </button>
          <button onClick={() => void loadNotes()} disabled={notesLoading}>
            {notesLoading ? "Lädt…" : "Aktualisieren"}
          </button>
        </div>
      </div>

      {distillStatus.kind === "success" && <p style={styles.success}>{distillStatus.message}</p>}
      {distillStatus.kind === "error" && <p style={styles.error}>{distillStatus.message}</p>}

      {notesError && <p style={styles.error}>{notesError}</p>}

      {!notesError && !notesLoading && notes.length === 0 && (
        <p>Noch keine Notizen — auf „Jetzt destillieren (heute)" klicken, um die erste zu erzeugen.</p>
      )}

      {notes.length > 0 && (
        <div style={styles.layout}>
          <ul style={styles.list}>
            {notes.map((note) => {
              const label = note.title ?? `Notiz vom ${formatDate(note.range_start)}`;
              const selected = note.id === selectedId;
              return (
                <li key={note.id} style={styles.listItem}>
                  <button
                    type="button"
                    onClick={() => selectNote(note.id)}
                    style={
                      selected
                        ? { ...styles.listButton, ...styles.listButtonSelected }
                        : styles.listButton
                    }
                  >
                    <div style={styles.listTitle}>{label}</div>
                    <div style={styles.listMeta}>{formatRange(note.range_start, note.range_end)}</div>
                    <div style={styles.listMeta}>
                      Erstellt: {formatDateTime(note.created_at)}
                      {note.model && ` · ${note.model}`}
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>

          <div style={styles.content}>
            {contentStatus.kind === "idle" && <p>Notiz aus der Liste auswählen, um sie anzuzeigen.</p>}
            {contentStatus.kind === "loading" && <p>Lädt…</p>}
            {contentStatus.kind === "error" && <p style={styles.error}>{contentStatus.message}</p>}
            {contentStatus.kind === "success" && (
              <>
                <h3 style={styles.contentHeading}>
                  {contentStatus.content.note.title ??
                    `Notiz vom ${formatDate(contentStatus.content.note.range_start)}`}
                </h3>
                <p style={styles.listMeta}>
                  {formatRange(
                    contentStatus.content.note.range_start,
                    contentStatus.content.note.range_end,
                  )}
                  {" · "}
                  {contentStatus.content.note.source_snapshot_count} Snapshot(s)
                  {contentStatus.content.note.model && ` · ${contentStatus.content.note.model}`}
                </p>
                <pre style={styles.markdown}>{contentStatus.content.markdown}</pre>
              </>
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
    flexWrap: "wrap",
  },
  heading: {
    margin: 0,
  },
  headerActions: {
    display: "flex",
    gap: "0.5rem",
  },
  error: {
    color: "#b00020",
  },
  success: {
    color: "#1a7f37",
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
  contentHeading: {
    marginTop: 0,
    marginBottom: "0.25rem",
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
