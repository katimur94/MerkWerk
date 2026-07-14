# Plan Etappe 0 — Task-Zerlegung & Subagent-Zuordnung

Reihenfolge respektiert Abhängigkeiten. Modell/Effort nach Kickoff-Tabelle.
Jeder Task hat Dateipfade, Schnittstelle und Definition-of-Done (DoD).

## Phase A — Fundament (parallelisierbar)

### T1 — Cargo-Workspace + Projekt-Setup · Haiku, niedrig
- **Ziel:** `daemon/` als Workspace (D6): Crates `storage`, `blacklist`, `config`,
  `ipc-protocol` (plattformneutral, lib) + `capture-win` (`#[cfg(windows)]`, lib)
  + `merkwerk-daemon` (bin). `.gitignore`, `rust-toolchain`, Workspace-`Cargo.toml`.
- **DoD:** `cargo check` (host) grün für neutrale Crates; leere `lib.rs`/`main.rs`
  kompilieren; `cargo check --target x86_64-pc-windows-gnu` grün.

### T2 — Recherche windows-rs Signaturen · Haiku, mittel
- **Ziel:** `docs/RECHERCHE-winapi.md`: konkrete `windows`-crate-Feature-Flags und
  Signaturen für `SetWinEventHook`, `WH_KEYBOARD_LL`/`WH_MOUSE_LL`,
  `GetForegroundWindow`, `GetWindowThreadProcessId`, Prozessname via
  `QueryFullProcessImageNameW`, und UIAutomation (`CUIAutomation`,
  `IUIAutomation`, `TreeWalker`, `IsPasswordPropertyId`,
  Adressleisten-Muster für Chrome/Edge/Firefox).
- **DoD:** Kompilierbare Signatur-Snippets + Feature-Liste; wird von T5/T6 genutzt.

## Phase B — Plattformneutraler Kern (nativ testbar)

### T3 — Storage-Layer · Sonnet, mittel
- **Datei:** `daemon/storage/`. **Schnittstelle:** `Store` mit `open(path)`,
  `insert_session/end_session`, `insert_event`, `insert_snapshot`,
  Batch-Commit; Migrations-Runner (`meta.schema_version`).
- **DoD:** Schema aus ARCHITEKTUR.md; WAL an; `cargo test` deckt Insert/Query/
  Migration ab; native Tests grün.

### T4 — Blacklist-Filter · Sonnet, mittel
- **Datei:** `daemon/blacklist/`. **Schnittstelle:**
  `Blacklist::from_config(&Config) -> Blacklist`,
  `fn is_blocked(process, title, url) -> bool` (Glob/Substring-Muster).
- **DoD:** Prozess-/Titel-/URL-Muster; Unit-Tests inkl. Negativfälle; grün.

### T4b — Config (TOML) · Sonnet, mittel
- **Datei:** `daemon/config/`. **Schnittstelle:** `Config::load(path)` /
  `default()`; Felder: DB-Pfad, Blacklist-Listen, Debounce-Parameter, TTL-Tage.
- **DoD:** Serde-TOML, Default-Datei-Erzeugung, Round-Trip-Test; grün.

### T5 — IPC-Protokoll + Named-Pipe-Server · Sonnet, mittel
- **Datei:** `daemon/ipc-protocol/` (neutral, Serde-Typen + Tests) und
  Server-Teil in `merkwerk-daemon` (`#[cfg(windows)]`).
- **Schnittstelle:** `Request`/`Response`-Enums (JSONL), Kommandos
  `get_status|pause|resume|reload_config`.
- **DoD:** Protokoll-Serde-Tests nativ grün; Pipe-Server cross-checkt.

## Phase C — Windows-Erfassung (höchstes Risiko)

### T6 — Hooks + Debouncer · **Sonnet, hoch**
- **Datei:** `daemon/capture-win/src/hooks.rs`, `debounce.rs`.
- **Schnittstelle:** `start_hooks(tx: Sender<RawSignal>)`; `RawSignal` enthält
  NUR `focus_change{hwnd,ts}`, `key_tick{ts}`, `mouse_click{ts}`,
  `scroll{ts}` — **kein Keycode-Feld** (D3). Debouncer → `Trigger`-Events.
- **DoD:** cross-check grün; Code-Review bestätigt Privacy-Invariante; STA-Message-
  Loop korrekt; Kommentar dokumentiert das Verwerfen der Keycodes im Callback.

### T7 — UIAutomation-Snapshotter · **Sonnet, hoch**
- **Datei:** `daemon/capture-win/src/uia.rs`.
- **Schnittstelle:** `snapshot(hwnd) -> Snapshot{title,url,text,truncated}`;
  MTA-Thread; `IsPassword`-Subtree übersprungen; 20-KB-Deckel; Browser-URL.
- **DoD:** cross-check grün; Review bestätigt Passwort-Skip + Budget; Tiefenlimit.

### T7b — Screenshot-Fallback-Gerüst · Haiku, niedrig
- **Datei:** `daemon/capture-win/src/capture/mod.rs`. Trait `FallbackCapture`
  + `NoopCapture`. **DoD:** kompiliert, nur Gerüst, dokumentiert.

## Phase D — Verdrahtung Daemon

### T8 — Daemon-Runtime · Fable (ich) integriere, Sonnet-Bausteine
- Threads verdrahten (Hook/UIA/Writer/IPC), app_session-Lebenszyklus,
  Blacklist an der Quelle, Snapshot-Pipeline. **DoD:** cross-check grün;
  Datenfluss end-to-end konsistent mit ARCHITEKTUR.md.

## Phase E — App

### T9 — Tauri-Shell + Tray · Sonnet, mittel
- Tauri 2 Grundgerüst, Tray (Start/Pause/Status), IPC-Client zum Daemon,
  Daemon als Sidecar. **DoD:** `npm run tauri build`-Konfig steht; TS kompiliert.

### T10 — Timeline-UI · Sonnet, mittel
- React-Timeline (read-only-Reader über Tauri-Command/SQLite): App, Zeitraum,
  Titel, Snapshot-Vorschau. **DoD:** rendert Beispieldaten; TS-Build grün.

### T11 — Settings (Blacklist-Editor + Autostart-Toggle) · Sonnet, mittel
- Blacklist-Editor schreibt TOML → IPC `reload_config`; Autostart via Registry
  Run-Key (30 s verzögert). **DoD:** TS-Build grün; schreibt gültiges TOML.

## Phase F — Abnahme

### T12 — CI-Skript + Langzeit-/Ressourcentest · Haiku, mittel
- `scripts/ci-check.*` (fmt, clippy, cross-check, native tests, TS-build) und
  `scripts/longrun.ps1` (8 h, CPU/RAM-Sampling, Leak-Heuristik).
- **DoD:** CI-Skript läuft in Sandbox grün; PS-Skript dokumentiert für Windows.

### T13 — Doku-Sync · Haiku, niedrig
- ARCHITEKTUR/ENTSCHEIDUNGEN/README auf Endstand; Blacklist-Wirksamkeitstest
  dokumentiert. **DoD:** aktuell, konsistent.

## Kritische Review-Punkte (bei JEDER Integration)
1. Kompiliert (host + cross-target).
2. Kein Keycode/Roh-Tastenanschlag irgendwo — auch nicht in Logs.
3. Passwort-Skip + Blacklist wirken vor Persistierung.
4. Schnittstelle eingehalten, keine verbotenen Speicherpfade.
