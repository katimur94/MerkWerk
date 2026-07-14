import { useState } from "react";
import { StatusBar } from "./components/StatusBar";
import { TimelineView } from "./components/TimelineView";
import { SearchView } from "./components/SearchView";
import { NotesView } from "./components/NotesView";
import { SemanticSearchView } from "./components/SemanticSearchView";
import { SettingsView } from "./components/SettingsView";

// MerkWerk-App-Grundgerüst.
//
// Bereiche laut ARCHITEKTUR.md ("merkwerk-app"):
//   - Status-Leiste:      Kurzstatus des Daemons (Running/Paused, Zähler,
//                          Laufzeit) + Pause/Resume-Umschalter, siehe
//                          ./components/StatusBar.tsx — pollt
//                          `get_daemon_status` (echtes IPC über die Named
//                          Pipe, src-tauri/src/lib.rs) alle paar Sekunden.
//   - Timeline:            Zeitleiste aus der read-only SQLite-DB.
//   - Suche:                Volltextsuche (FTS5) über dieselbe read-only DB.
//   - Semantische Suche:    Cosinus-Suche über Notiz-Embeddings via Ollama (D11).
//   - Notizen:              KI-Notizen (Markdown-Vault, D10) + "Jetzt destillieren".
//   - Einstellungen:        Blacklist-Editor + Autostart-Toggle.
//
// Navigation: eine feste Seitenleiste links schaltet zwischen den fünf
// Bereichen um (State `activeView` unten) — es wird immer nur die aktive
// View gerendert, nicht mehr alle gestapelt untereinander. Jede View lädt
// ihre Daten beim Mounten selbst (siehe die jeweilige Komponente), ein
// Wechsel liefert also automatisch frische Daten, ganz ohne dass App.tsx
// selbst etwas synchronisieren müsste.
type ViewId = "timeline" | "search" | "semantic" | "notes" | "settings";

const NAV_ITEMS: ReadonlyArray<{ id: ViewId; label: string }> = [
  { id: "timeline", label: "Timeline" },
  { id: "search", label: "Suche" },
  { id: "semantic", label: "Semantik" },
  { id: "notes", label: "Notizen" },
  { id: "settings", label: "Einstellungen" },
];

function App() {
  const [activeView, setActiveView] = useState<ViewId>("timeline");

  return (
    <div className="app-shell">
      <StatusBar />

      <div className="app-body">
        <nav className="app-nav" aria-label="Bereiche">
          {NAV_ITEMS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={
                item.id === activeView ? "app-nav__item app-nav__item--active" : "app-nav__item"
              }
              aria-current={item.id === activeView ? "page" : undefined}
              onClick={() => setActiveView(item.id)}
            >
              {item.label}
            </button>
          ))}
        </nav>

        <main className="app-content">
          {activeView === "timeline" && <TimelineView />}
          {activeView === "search" && <SearchView />}
          {activeView === "semantic" && <SemanticSearchView />}
          {activeView === "notes" && <NotesView />}
          {activeView === "settings" && <SettingsView />}
        </main>
      </div>
    </div>
  );
}

export default App;
