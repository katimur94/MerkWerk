import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

// Status-Leiste (oben, über der Navigation): pollt den echten Daemon-Status
// per IPC-Roundtrip — `get_daemon_status` (siehe src-tauri/src/lib.rs) sendet
// intern `Request::GetStatus` über die Named Pipe an den Daemon
// (`ipc_client::ipc_request`, ENTSCHEIDUNGEN.md D1) und liefert die
// dekodierte `Response::Status` bzw. bei nicht erreichbarem Daemon einen
// "offline"-Status (running: false) — nie einen Fehler, siehe dort. Der
// Pause/Resume-Umschalter ruft `pause_daemon`/`resume_daemon` auf (echtes
// IPC, kein lokales Flag mehr) und holt danach sofort den Status neu, statt
// auf den nächsten Poll-Tick zu warten.

/** Spiegelt `DaemonStatus` (src-tauri/src/lib.rs) 1:1 — snake_case, kein `rename_all`. */
interface DaemonStatus {
  running: boolean;
  paused: boolean;
  events_captured: number;
  snapshots_captured: number;
  uptime_secs: number;
}

/** Wie oft `get_daemon_status` abgefragt wird. */
const POLL_INTERVAL_MS = 5000;

/**
 * Zustand, wenn der Daemon nicht erreichbar ist bzw. kein Tauri-Kontext
 * vorliegt — identisch zu `DaemonStatus::default()` in src-tauri/src/lib.rs
 * (dieselbe "offline"-Lesart: nichts läuft, nichts pausiert, alle Zähler 0).
 */
const OFFLINE_STATUS: DaemonStatus = {
  running: false,
  paused: false,
  events_captured: 0,
  snapshots_captured: 0,
  uptime_secs: 0,
};

const NO_TAURI_MESSAGE =
  "Kein Tauri-Kontext erkannt (Browser-Vorschau?). Der Daemon-Status kann nur " +
  "innerhalb der MerkWerk-Desktop-App abgefragt werden.";

/** Formatiert Sekunden als "H:MM" (Stunden:Minuten seit Daemon-Start). */
function formatUptime(totalSeconds: number): string {
  const totalMinutes = Math.floor(totalSeconds / 60);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${hours}:${String(minutes).padStart(2, "0")}`;
}

/**
 * Formatiert Fehler aus `invoke()`. Tauri-Command-Fehler sind Strings (siehe
 * `Result<_, String>` in lib.rs) — `err instanceof Error` deckt daneben den
 * Fall ab, dass `invoke` selbst technisch fehlschlägt.
 */
function formatInvokeError(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function StatusBar() {
  const [status, setStatus] = useState<DaemonStatus>(OFFLINE_STATUS);
  const [tauriAvailable, setTauriAvailable] = useState(true);
  const [toggling, setToggling] = useState(false);
  const [toggleError, setToggleError] = useState<string | null>(null);
  const pollIntervalRef = useRef<number | undefined>(undefined);

  const loadStatus = useCallback(async () => {
    // Reine Browser-Vorschau (z. B. `vite dev`/`vite preview` ohne die
    // Tauri-Laufzeit) hat kein IPC-Backend — `invoke` würde werfen. Das hier
    // vorab abfangen liefert einen klaren Hinweis statt eines Absturzes.
    if (!isTauri()) {
      setTauriAvailable(false);
      setStatus(OFFLINE_STATUS);
      return;
    }

    setTauriAvailable(true);

    try {
      const result = await invoke<DaemonStatus>("get_daemon_status");
      setStatus(result);
    } catch {
      // get_daemon_status liefert bei nicht erreichbarem Daemon bereits
      // selbst einen "offline"-Status statt eines Err (siehe
      // src-tauri/src/lib.rs) — dieser Zweig ist nur ein Sicherheitsnetz für
      // den Fall, dass invoke() technisch fehlschlägt.
      setStatus(OFFLINE_STATUS);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
    pollIntervalRef.current = window.setInterval(() => {
      void loadStatus();
    }, POLL_INTERVAL_MS);

    return () => {
      if (pollIntervalRef.current !== undefined) {
        window.clearInterval(pollIntervalRef.current);
      }
    };
  }, [loadStatus]);

  const handleToggle = useCallback(() => {
    if (!isTauri()) {
      setToggleError(NO_TAURI_MESSAGE);
      return;
    }

    setToggling(true);
    setToggleError(null);

    void (async () => {
      try {
        // pause_daemon/resume_daemon liefern Result<(), String> (siehe
        // src-tauri/src/lib.rs) — invoke() lehnt das Promise bei Err ab.
        await invoke(status.paused ? "resume_daemon" : "pause_daemon");
      } catch (err) {
        setToggleError(formatInvokeError(err));
      } finally {
        // Unabhängig von Erfolg/Fehler den echten Stand neu holen, statt
        // optimistisch umzuschalten — der Daemon (nicht dieser Klick) ist
        // die Quelle der Wahrheit.
        await loadStatus();
        setToggling(false);
      }
    })();
  }, [status.paused, loadStatus]);

  const offline = !tauriAvailable || !status.running;

  return (
    <header className="status-bar">
      <div className="status-bar__brand">
        <strong>MerkWerk</strong>
      </div>

      <div className="status-bar__info">
        {!tauriAvailable && (
          <span
            className="status-bar__badge status-bar__badge--offline"
            title={NO_TAURI_MESSAGE}
          >
            Kein Tauri-Kontext
          </span>
        )}

        {tauriAvailable && (
          <span
            className={
              offline
                ? "status-bar__badge status-bar__badge--offline"
                : status.paused
                  ? "status-bar__badge status-bar__badge--paused"
                  : "status-bar__badge status-bar__badge--running"
            }
          >
            {offline ? "Offline" : status.paused ? "Pausiert" : "Läuft"}
          </span>
        )}

        {tauriAvailable && !offline && (
          <>
            <span className="status-bar__metric">{status.events_captured} Events</span>
            <span className="status-bar__metric">{status.snapshots_captured} Snapshots</span>
            <span className="status-bar__metric">Laufzeit {formatUptime(status.uptime_secs)}</span>
          </>
        )}
      </div>

      <div className="status-bar__actions">
        {toggleError && (
          <span className="status-bar__error" role="alert" title={toggleError}>
            {toggleError}
          </span>
        )}
        <button
          type="button"
          onClick={handleToggle}
          disabled={toggling || offline}
          title={offline ? "Daemon nicht erreichbar — Pause/Resume nicht möglich" : undefined}
        >
          {toggling ? "…" : status.paused ? "Fortsetzen" : "Pause"}
        </button>
      </div>
    </header>
  );
}
