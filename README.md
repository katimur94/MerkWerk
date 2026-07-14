# MerkWerk

„Obsidian, das sich selbst schreibt." Eine Windows-Desktop-App, die im
Hintergrund Bildschirmaktivität erfasst und (ab Etappe 1) per lokaler KI zu
Markdown-Notizen destilliert. Alles bleibt lokal.

> **Aktueller Stand: Etappe 0 — Skelett.** Capture-Daemon + Rohdaten-Timeline.
> Noch keine KI-Anbindung.

## Was Etappe 0 tut

- Erfasst Fokus-/Fensterwechsel und Aktivitätssignale (Tippen/Klicken als
  Zähler — **niemals Tasteninhalte**).
- Macht bei relevanten Ereignissen einen Kontext-Snapshot: aktive App,
  Fenstertitel, sichtbarer Text (UIAutomation), bei Browsern die URL.
- Filtert an der Quelle: Passwortfelder und Blacklist-Treffer erreichen die
  Datenbank nie.
- Schreibt gebatcht in eine lokale SQLite-DB (WAL).
- Zeigt eine Live-Timeline des Tages, plus Settings (Blacklist, Autostart).

## Architektur

Zwei Prozesse: der headless **`merkwerk-daemon`** (Rust) erfasst und schreibt;
die **`merkwerk-app`** (Tauri 2 + React) zeigt an und steuert per Named-Pipe-IPC.
Details, Datenfluss-Diagramm und DB-Schema: [ARCHITEKTUR.md](./ARCHITEKTUR.md).
Entscheidungen: [ENTSCHEIDUNGEN.md](./ENTSCHEIDUNGEN.md).

## Privacy-Garantien

- Keine Roh-Tastenanschläge — durch Typ-/Modulgrenze erzwungen, nicht durch Disziplin.
- Passwortfelder (`IsPassword`) werden komplett übersprungen.
- Blacklist (Prozess/Titel/URL) verwirft Daten vor der Persistierung.
- Alle Rohdaten tragen ein TTL-Feld (Löschjob ab Etappe 1).

## Repo-Layout

| Pfad | Inhalt |
|---|---|
| `/daemon` | Rust-Workspace: Erfassung, Storage, Blacklist, IPC, Config |
| `/app` | Tauri 2 + React + TypeScript + Vite |
| `/docs` | Recherche- und Planungsnotizen |
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
