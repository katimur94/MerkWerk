<p align="center">
  <img src="assets/maskottchen.svg" width="180" alt="Memo, das MerkWerk-Maskottchen — ein Elefant, der eine Notiz hält">
</p>

<h1 align="center">MerkWerk</h1>
<p align="center"><em>„Obsidian, das sich selbst schreibt."</em></p>

> Sag hallo zu **Memo** 🐘 — unserem Maskottchen. Ein Elefant, weil Elefanten nie
> vergessen. Genau das macht MerkWerk: Es merkt sich für dich, woran du gearbeitet hast.

---

## Was ist MerkWerk? (in einfachen Worten)

Stell dir ein **Tagebuch vor, das sich von selbst schreibt.**

MerkWerk läuft leise im Hintergrund auf deinem Windows-PC und merkt sich, womit du
dich beschäftigst — welche Programme, welche Fenster, welche Webseiten. Ab und zu
fasst eine **KI direkt auf deinem Rechner** das Ganze zu kurzen, lesbaren Notizen
zusammen. So entsteht nach und nach eine durchsuchbare Sammlung. Die Frage „Was
habe ich letzten Dienstag eigentlich gemacht?" beantwortet MerkWerk dann für dich.

**Das Wichtigste zuerst — deine Privatsphäre:**

- 🔒 **Alles bleibt auf deinem Computer.** Kein Internet, keine Cloud, kein Konto.
- ⌨️ **Deine Tastatureingaben werden nie mitgeschnitten.** MerkWerk zählt nur, *dass*
  getippt wurde — nie *was*. (Das ist technisch fest eingebaut, nicht bloß versprochen.)
- 🙈 **Passwörter werden übersprungen**, und alles, was du auf eine Sperrliste setzt
  (z. B. dein Online-Banking), wird gar nicht erst gespeichert.
- ⏸️ **Du hast die Kontrolle:** jederzeit pausierbar; alte Daten räumen sich von selbst auf.

## Was kann Memo für dich tun?

- 📝 **Schreibt dir automatisch Notizen** darüber, was du getan hast.
- 🔎 **Findet alles wieder** — per Stichwort *und* per Bedeutung („zeig mir den Tag,
  an dem ich an der Präsentation saß").
- 📁 **Ganz normale Markdown-Dateien** — du kannst sie auch direkt in Obsidian öffnen.
- 🖥️ **Eine aufgeräumte App** mit Zeitleiste, Suche, Notizen und Einstellungen.

## Was brauche ich dafür?

Einen Windows-PC und einmalig das kostenlose, lokale KI-Programm
[**Ollama**](https://ollama.com) (Details unten). Danach läuft alles ohne Internet.

---

<details>
<summary><strong>Technische Details (für Entwickler)</strong> — aufklappen</summary>

> **Stand: Etappen 0–3 im Code fertig.** Erfassung, Volltext- & semantische Suche,
> lokale KI-Destillation in einen Markdown-Vault, App mit Navigation und Live-Status.
> Die Endabnahme (24/7-Lauf, GUI, Ollama-Modell) läuft auf einem Windows-Rechner —
> siehe [docs/ROADMAP.md](./docs/ROADMAP.md).

### Funktionen (technisch)

- **Erfassung** (Daemon, 24/7): Fokus-/Fensterwechsel und Aktivitätssignale
  (Tippen/Klicken als Zähler — **niemals Tasteninhalte**); Kontext-Snapshots bei
  relevanten Ereignissen: aktive App, Fenstertitel, sichtbarer Text (UIAutomation),
  bei Browsern die URL.
- **Filter an der Quelle:** Passwortfelder und Blacklist-Treffer (Prozess/Titel/URL)
  erreichen die Datenbank nie.
- **Speicher:** lokale SQLite-DB (WAL); TTL-Löschjob räumt Rohdaten auf.
- **Suche:** FTS5-Volltextsuche über Snapshots; semantische Suche über die KI-Notizen
  (Embeddings + Cosinus).
- **KI-Destillation (lokal, Ollama):** fasst einen Zeitraum zu einer strukturierten
  Markdown-Notiz zusammen und legt sie als `.md`-Datei im Vault ab — manuell per Knopf
  oder automatisch im Intervall.
- **App:** Navigation (Timeline · Suche · Semantik · Notizen · Einstellungen),
  Live-Statusleiste mit Pause/Resume, Blacklist-Editor, Autostart-Toggle, Dark/Light.

### Lokale KI einrichten (Ollama)

Kein Python, kein Docker, keine Cloud. [Ollama](https://ollama.com) installieren, dann
die konfigurierten Modelle ziehen (Defaults in `config.toml`, Abschnitt `[ai]`):

```powershell
ollama pull llama3.1          # Textmodell (Destillation)
ollama pull nomic-embed-text  # Embeddings (semantische Suche)
```

Endpoint/Modelle sind in `%APPDATA%\MerkWerk\config.toml` änderbar. Ist Ollama nicht
erreichbar, läuft die Erfassung normal weiter; nur Destillation/semantische Suche
melden dann einen Hinweis.

### Architektur

Zwei Prozesse: der headless **`merkwerk-daemon`** (Rust) erfasst und schreibt; die
**`merkwerk-app`** (Tauri 2 + React) zeigt an und steuert per Named-Pipe-IPC. Details,
Datenfluss-Diagramm und DB-Schema: [ARCHITEKTUR.md](./ARCHITEKTUR.md). Entscheidungen:
[ENTSCHEIDUNGEN.md](./ENTSCHEIDUNGEN.md).

### Privacy-Garantien (technisch)

- Keine Roh-Tastenanschläge — durch Typ-/Modulgrenze erzwungen, nicht durch Disziplin.
- Passwortfelder (`IsPassword`) werden komplett übersprungen (Element + Subtree).
- Blacklist (Prozess/Titel/URL) verwirft Daten vor der Persistierung.
- Alle Rohdaten tragen ein TTL-Feld; ein periodischer Löschjob entfernt Abgelaufenes.
- Die KI läuft **lokal** (Ollama); es verlässt nichts den Rechner.

### Repo-Layout

| Pfad | Inhalt |
|---|---|
| `/daemon` | Rust-Workspace: `storage`, `blacklist`, `config`, `ipc-protocol`, `capture-win`, `inference` (Ollama), `distiller`, Binary `merkwerk-daemon` |
| `/app` | Tauri 2 + React + TypeScript + Vite |
| `/docs` | Architektur-/Recherche-/Planungsnotizen, ROADMAP |
| `/scripts` | CI-Check, Langzeit-/Ressourcentest |
| `/assets` | Maskottchen |

### Build (Ziel: Windows)

```powershell
# Daemon
cd daemon
cargo build --release

# App (baut den Daemon als Sidecar mit)
cd ../app
npm install
npm run tauri build
```

Entwicklung/Validierung erfolgt teils in einer Linux-Sandbox per Cross-Check
(`cargo check --target x86_64-pc-windows-gnu`); echte Läufe und der finale MSVC-Build
passieren unter Windows. Siehe ARCHITEKTUR.md → „Build-Realität".

</details>
