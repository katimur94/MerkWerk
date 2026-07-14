import { TimelineView } from "./components/TimelineView";
import { SearchView } from "./components/SearchView";
import { SettingsView } from "./components/SettingsView";

// MerkWerk-App-Grundgerüst (Etappe 0).
//
// Bereiche laut ARCHITEKTUR.md ("merkwerk-app"):
//   - Tray-Status: Kurzstatus des Daemons (Running/Paused, Zähler).
//   - Timeline:    Zeitleiste aus der read-only SQLite-DB.
//   - Suche:       Volltextsuche (FTS5) über dieselbe read-only DB.
//   - Settings:    Blacklist-Editor + Autostart-Toggle.
//
// Timeline-, Such- und Settings-Inhalte sind Komponenten aus eigenen Dateien
// (siehe ./components/TimelineView.tsx, ./components/SearchView.tsx,
// ./components/SettingsView.tsx). Der Tray-Status-Bereich bleibt hier
// vorerst inline, nur als Platzhalter.
function App() {
  return (
    <div className="app">
      <header className="app__tray-status">
        {/* TODO (spätere Task): get_daemon_status() per @tauri-apps/api
            invoke() abfragen (Platzhalter-Command, siehe src-tauri/src/lib.rs)
            und periodisch pollen, um Running/Paused/Events/Uptime
            anzuzeigen. */}
        <strong>MerkWerk</strong>
        <span className="app__status-placeholder">Status: unbekannt (Platzhalter)</span>
      </header>

      <div className="app__body">
        <main className="app__timeline">
          <TimelineView />
          <SearchView />
        </main>

        <aside className="app__settings">
          <SettingsView />
        </aside>
      </div>
    </div>
  );
}

export default App;
