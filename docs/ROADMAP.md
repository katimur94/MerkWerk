# MerkWerk — Roadmap bis zur fertigen App

Etappe 0 (Skelett: Capture-Daemon + Rohdaten-Timeline) ist abgeschlossen.
Diese Roadmap führt bis zur fertigen App: „Obsidian, das sich selbst schreibt".
Alles bleibt lokal. Grundprinzipien aus Etappe 0 gelten weiter (keine
Roh-Tastenanschläge, Passwort-/Blacklist-Filter an der Quelle, TTL auf Rohdaten).

## Etappe 1 — Datenschicht solide machen (keine KI)
Fundament, auf dem die KI später aufsetzt. Alles nativ testbar.
- **Retention/TTL-Löschjob:** periodisch `expires_at < now` aus
  `snapshots`/`events`/`app_sessions` löschen (FK-schonend). Konfig-Intervall.
- **Volltextsuche (FTS5):** Migration v2 legt eine contentless FTS5-Tabelle über
  `snapshots` an (text_content + window_title + url), per Trigger synchron
  gehalten. `Store::search(query, limit)`.
- **Such-UI:** Suchfeld + Trefferliste in der App (read-only).

## Etappe 2 — Lokale KI-Destillation → Markdown-Notizen
Der Kern der Produktidee.
- **Inference-Abstraktion (D9):** Trait `Inference` im Daemon; Backend v1 =
  **Ollama** (lokaler HTTP-Server `127.0.0.1:11434`, kein Python/Docker).
  Austauschbar (später llama.cpp/Candle) ohne Änderung der Aufrufer.
- **Destillierer:** fasst pro Zeitblock/Tag die (gefilterten) Snapshots+Sessions
  zu einer strukturierten Markdown-Notiz zusammen (Prompt-Vorlage konfigurierbar).
- **Notiz-Vault (D10):** generierte Notizen als `.md`-Dateien in einem Vault-
  Verzeichnis (`%APPDATA%\MerkWerk\vault`, konfigurierbar) — Obsidian-kompatibel.
  Tabelle `notes` verweist auf Datei + Quellzeitraum.
- **Notiz-UI:** Notizen-Liste + Vorschau; „Jetzt destillieren"-Aktion.

## Etappe 3 — Semantik & Politur
- **Embeddings (sqlite-vec):** Vektor-Index über Snapshots/Notizen (Embeddings
  via `Inference`-Backend), semantische Suche neben FTS5.
- **Review-/Vault-UX:** Notiz öffnen/bearbeiten, Tagesnavigation, KI-Einstellungen
  (Modell, Prompt, Cadence), Pausen-/Datenschutz-Kontrollen sichtbar.
- **Abnahme:** End-to-End auf Windows, Ressourcen im Rahmen, Vault sinnvoll.

## Realitäts-Hinweis (Entwicklungsumgebung)
Entwickelt in einer Linux-Sandbox: plattformneutrale Logik (Storage, Suche,
Destillier-Logik, Prompt-Bau, Vault-Schreiber, Vektor-Mathe) wird hier **nativ
getestet**; Windows-Code per `--target x86_64-pc-windows-gnu` cross-validiert.
Echte Läufe (24/7-Capture, UIA, Tauri-GUI, lokales Modell via Ollama, Vault
im Dateisystem) und die Endabnahme passieren auf dem **Windows-Rechner** — dort
läuft auch das lokale Modell. Skripte/Anleitungen liegen in `/scripts`.
