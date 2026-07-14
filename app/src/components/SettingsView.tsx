// Platzhalter-Komponente für die Settings-Ansicht.
//
// TODO (spätere Task): Blacklist-Editor (Prozess-/Titel-/URL-Muster) und
// Autostart-Toggle. Liest/schreibt %APPDATA%\MerkWerk\config.toml über
// zukünftige Tauri-Commands und stößt danach `reload_config` per IPC an
// (siehe ENTSCHEIDUNGEN.md D1, daemon/ipc-protocol/src/lib.rs:
// Request::ReloadConfig).
export function SettingsView() {
  return (
    <section aria-label="Settings">
      <h2>Settings</h2>
      <p>Platzhalter — wird in einer späteren Task implementiert.</p>
    </section>
  );
}
