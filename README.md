# MerkWerk

„Obsidian, das sich selbst schreibt." Eine Windows-Desktop-App, die im
Hintergrund Bildschirmaktivität erfasst und per **lokaler** KI zu Markdown-
Notizen destilliert. Alles bleibt lokal.

> **Stand: Etappen 0–3 im Code fertig.** Erfassung, Volltext- & semantische
> Suche, lokale KI-Destillation in einen Markdown-Vault, App mit Navigation und
> Live-Status. Die Endabnahme (24/7-Lauf, GUI, Ollama-Modell) läuft auf einem
> Windows-Rechner — siehe [docs/ROADMAP.md](./docs/ROADMAP.md).

## Funktionen

- **Erfassung** (Daemon, 24/7): Fokus-/Fensterwechsel und Aktivitätssignale
  (Tippen/Klicken als Zähler — **niemals Tasteninhalte**); Kontext-Snapshots bei
  relevanten Ereignissen: aktive App, Fenstertitel, sichtbarer Text
  (UIAutomation), bei Browsern die URL.
- **Filter an der Quelle:** Passwortfelder und Blacklist-Treffer (Prozess/Titel/
  URL) erreichen die Datenbank nie.
- **Speicher:** lokale SQLite-DB (WAL); TTL-Löschjob räumt Rohdaten auf.
- **Suche:** FTS5-Volltextsuche über Snapshots; semantische Suche über die
  KI-Notizen (Embeddings + Cosinus).
- **KI-Destillation (lokal, Ollama):** fasst einen Zeitraum zu einer
  strukturierten Markdown-Notiz zusammen und legt sie als `.md`-Datei im Vault ab
  (Obsidian-kompatibel) — manuell per Knopf oder automatisch im Intervall.
- **App:** Navigationslayout (Timeline · Suche · Semantik · Notizen ·
  Einstellungen), Live-Statusleiste mit Pause/Resume, Blacklist-Editor,
  Autostart-Toggle. Dark/Light.

## Lokale KI einrichten (Ollama)

Die Destillation nutzt einen lokalen [Ollama](https://ollama.com)-Server (kein
Python, kein Docker, keine Cloud). Installieren, dann die konfigurierten Modelle
ziehen (Defaults in `config.toml`, Abschnitt `[ai]`):

```powershell
ollama pull llama3.1          # Textmodell (Destillation)
ollama pull nomic-embed-text  # Embeddings (semantische Suche)
```

Endpoint/Modelle sind in `%APPDATA%\MerkWerk\config.toml` änderbar. Ist Ollama
nicht erreichbar, läuft die Erfassung normal weiter; nur Destillation/semantische
Suche melden dann einen Hinweis.

## Architektur

Zwei Prozesse: der headless **`merkwerk-daemon`** (Rust) erfasst und schreibt;
die **`merkwerk-app`** (Tauri 2 + React) zeigt an und steuert per Named-Pipe-IPC.
Details, Datenfluss-Diagramm und DB-Schema: [ARCHITEKTUR.md](./ARCHITEKTUR.md).
Entscheidungen: [ENTSCHEIDUNGEN.md](./ENTSCHEIDUNGEN.md).

## Privacy-Garantien

- Keine Roh-Tastenanschläge — durch Typ-/Modulgrenze erzwungen, nicht durch Disziplin.
- Passwortfelder (`IsPassword`) werden komplett übersprungen.
- Blacklist (Prozess/Titel/URL) verwirft Daten vor der Persistierung.
- Alle Rohdaten tragen ein TTL-Feld; ein periodischer Löschjob entfernt Abgelaufenes.
- Die KI läuft **lokal** (Ollama); es verlässt nichts den Rechner.

## Repo-Layout

| Pfad | Inhalt |
|---|---|
| `/daemon` | Rust-Workspace: `storage`, `blacklist`, `config`, `ipc-protocol`, `capture-win`, `inference` (Ollama), `distiller`, Binary `merkwerk-daemon` |
| `/app` | Tauri 2 + React + TypeScript + Vite |
| `/docs` | Architektur-/Recherche-/Planungsnotizen, ROADMAP |
| `/scripts` | CI-Check, Langzeit-/Ressourcentest |

## Build (Ziel: Windows)

```powershell
# Daemon
cd daemon
cargo build --release

# App (baut den Daemon als Sidecar mit)
cd ../app
npm install
npm run tauri build
```

Entwicklung/Validierung erfolgt teils in einer Linux-Sandbox per
Cross-Check (`cargo check --target x86_64-pc-windows-gnu`); echte Läufe und der
finale MSVC-Build passieren unter Windows. Siehe ARCHITEKTUR.md → „Build-Realität".
