import { useCallback, useEffect, useState } from "react";
import type { ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

// Settings-Ansicht: Blacklist-Editor (schreibt die Konfig-TOML, die der
// Daemon liest) und Autostart-Toggle (Windows HKCU Run-Key). Ruft die in
// src-tauri/src/settings.rs implementierten Commands über `invoke()` auf;
// `set_blacklist` stößt serverseitig best-effort einen Daemon-Reload per IPC
// an (siehe ENTSCHEIDUNGEN.md D1, daemon/ipc-protocol/src/lib.rs:
// Request::ReloadConfig) — dafür ist hier nichts weiter zu tun.

/**
 * Spiegelt `settings::BlacklistDto` (src-tauri/src/settings.rs) 1:1.
 * Die Feldnamen sind bewusst snake_case: `BlacklistDto` trägt kein
 * `#[serde(rename_all = ...)]`, serde erwartet also exakt die Rust-
 * Feldnamen im JSON.
 */
interface BlacklistDto {
  process_names: string[];
  title_patterns: string[];
  url_patterns: string[];
}

type SaveStatus =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "success"; message: string }
  | { kind: "error"; message: string };

/** Eine Regel pro Zeile <-> Textarea-Inhalt. Leere/nur-Whitespace-Zeilen
 * werden beim Speichern verworfen, Rand-Whitespace wird getrimmt. */
function linesToRules(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

function rulesToLines(rules: string[]): string {
  return rules.join("\n");
}

/**
 * Formatiert Fehler aus `invoke()`. Tauri-Command-Fehler sind Strings
 * (siehe `Result<_, String>` in settings.rs); daneben deckt das auch den
 * Fall ab, dass `invoke` selbst fehlschlägt, weil die Komponente in einer
 * reinen Browser-Vorschau ohne Tauri-Laufzeit rendert.
 */
function describeInvokeError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unbekannter Fehler (läuft diese Ansicht außerhalb der Tauri-App?).";
}

export function SettingsView() {
  const [processNamesText, setProcessNamesText] = useState("");
  const [titlePatternsText, setTitlePatternsText] = useState("");
  const [urlPatternsText, setUrlPatternsText] = useState("");
  const [blacklistLoadError, setBlacklistLoadError] = useState<string | null>(null);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ kind: "idle" });

  const [autostart, setAutostart] = useState(false);
  const [autostartLoadError, setAutostartLoadError] = useState<string | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  const [autostartPending, setAutostartPending] = useState(false);

  useEffect(() => {
    let cancelled = false;

    async function loadBlacklist() {
      try {
        const blacklist = await invoke<BlacklistDto>("get_blacklist");
        if (cancelled) return;
        setProcessNamesText(rulesToLines(blacklist.process_names));
        setTitlePatternsText(rulesToLines(blacklist.title_patterns));
        setUrlPatternsText(rulesToLines(blacklist.url_patterns));
      } catch (error) {
        if (cancelled) return;
        setBlacklistLoadError(
          `Blacklist konnte nicht geladen werden: ${describeInvokeError(error)}`,
        );
      }
    }

    async function loadAutostart() {
      try {
        const enabled = await invoke<boolean>("get_autostart");
        if (cancelled) return;
        setAutostart(enabled);
      } catch (error) {
        if (cancelled) return;
        setAutostartLoadError(
          `Autostart-Status konnte nicht gelesen werden: ${describeInvokeError(error)}`,
        );
      }
    }

    void loadBlacklist();
    void loadAutostart();

    return () => {
      cancelled = true;
    };
  }, []);

  const handleSaveBlacklist = useCallback(() => {
    const blacklist: BlacklistDto = {
      process_names: linesToRules(processNamesText),
      title_patterns: linesToRules(titlePatternsText),
      url_patterns: linesToRules(urlPatternsText),
    };

    setSaveStatus({ kind: "saving" });

    void (async () => {
      try {
        await invoke("set_blacklist", { blacklist });
        setSaveStatus({ kind: "success", message: "Blacklist gespeichert." });
      } catch (error) {
        setSaveStatus({
          kind: "error",
          message: `Speichern fehlgeschlagen: ${describeInvokeError(error)}`,
        });
      }
    })();
  }, [processNamesText, titlePatternsText, urlPatternsText]);

  const handleAutostartToggle = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    const enabled = event.target.checked;

    setAutostart(enabled);
    setAutostartError(null);
    setAutostartPending(true);

    void (async () => {
      try {
        await invoke("set_autostart", { enabled });
      } catch (error) {
        // Rollback: die Registry-Änderung ist nicht durchgegangen, die
        // Checkbox muss den vorherigen Zustand wieder zeigen.
        setAutostart(!enabled);
        setAutostartError(
          `Autostart konnte nicht geändert werden: ${describeInvokeError(error)}`,
        );
      } finally {
        setAutostartPending(false);
      }
    })();
  }, []);

  return (
    <section aria-label="Settings">
      <h2>Settings</h2>

      <fieldset className="settings__blacklist">
        <legend>Blacklist</legend>
        <p>
          Prozesse, Fenstertitel und URLs, die hier eingetragen sind, werden vor dem
          Speichern in der Datenbank verworfen (siehe ARCHITEKTUR.md, Privacy-Invariante 3)
          — pro Zeile ein Muster.
        </p>

        {blacklistLoadError && (
          <p role="alert" className="settings__error">
            {blacklistLoadError}
          </p>
        )}

        <div className="settings__field">
          <label htmlFor="settings-process-names">Prozessnamen</label>
          <textarea
            id="settings-process-names"
            rows={4}
            placeholder="z. B. keepass.exe"
            value={processNamesText}
            onChange={(event) => {
              setProcessNamesText(event.target.value);
              setSaveStatus({ kind: "idle" });
            }}
          />
        </div>

        <div className="settings__field">
          <label htmlFor="settings-title-patterns">Fenstertitel-Muster</label>
          <textarea
            id="settings-title-patterns"
            rows={4}
            placeholder="z. B. *Passwort*"
            value={titlePatternsText}
            onChange={(event) => {
              setTitlePatternsText(event.target.value);
              setSaveStatus({ kind: "idle" });
            }}
          />
        </div>

        <div className="settings__field">
          <label htmlFor="settings-url-patterns">URL-Muster</label>
          <textarea
            id="settings-url-patterns"
            rows={4}
            placeholder="z. B. *://*.bank.example/*"
            value={urlPatternsText}
            onChange={(event) => {
              setUrlPatternsText(event.target.value);
              setSaveStatus({ kind: "idle" });
            }}
          />
        </div>

        <button type="button" onClick={handleSaveBlacklist} disabled={saveStatus.kind === "saving"}>
          {saveStatus.kind === "saving" ? "Speichert…" : "Speichern"}
        </button>

        {saveStatus.kind === "success" && (
          <p role="status" className="settings__success">
            {saveStatus.message}
          </p>
        )}
        {saveStatus.kind === "error" && (
          <p role="alert" className="settings__error">
            {saveStatus.message}
          </p>
        )}
      </fieldset>

      <fieldset className="settings__autostart">
        <legend>Autostart</legend>

        {autostartLoadError && (
          <p role="alert" className="settings__error">
            {autostartLoadError}
          </p>
        )}

        <label>
          <input
            type="checkbox"
            checked={autostart}
            disabled={autostartPending}
            onChange={handleAutostartToggle}
          />
          MerkWerk beim Anmelden starten (mit 30 s Verzögerung, siehe ARCHITEKTUR.md)
        </label>

        {autostartError && (
          <p role="alert" className="settings__error">
            {autostartError}
          </p>
        )}
      </fieldset>
    </section>
  );
}
