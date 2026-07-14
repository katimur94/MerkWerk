# Entscheidungen (ADR-Log)

Kurzformat. Neueste unten. Jede Entscheidung ist bewusst und rückverfolgbar.

## D1 — IPC: Named Pipe (nicht localhost-Socket)
**Status:** entschieden.
Named Pipe (`\\.\pipe\merkwerk`) statt TCP-localhost. Gründe: kein offener
Port (Firewall-Prompts, Port-Kollisionen, versehentliche Netzwerk-Exposition),
Windows-ACLs beschränken den Zugriff auf den User, und für ein reines
lokales Steuerkanal-Protokoll ist das die idiomatische Windows-Lösung.
Protokoll: zeilenweise JSON (JSONL), Request/Response. Kommandos v0:
`get_status`, `pause`, `resume`, `reload_config`.

## D2 — Daemon ist einziger DB-Schreiber; App liest read-only
**Status:** entschieden.
Ein Schreiber vermeidet Lock-Konflikte trotz WAL. Die App öffnet die DB mit
`mode=ro` und pollt/liest für die Timeline. Steuerung ausschließlich über IPC.

## D3 — Low-Level-Hooks liefern nur Metadaten, keine Keycodes
**Status:** entschieden (Privacy-Invariante).
`WH_KEYBOARD_LL` wird verwendet, um „es wird getippt" zu erkennen — aber der
Callback verwirft `vkCode`/`scanCode` sofort und meldet nur einen Zähler-Tick
mit Zeitstempel über den Channel. Der Event-Typ, der den Hook-Thread verlässt,
hat kein Feld für Tasteninhalte. Damit ist „kein Roh-Tastenanschlag" nicht
Disziplin, sondern durch die Typ-/Modulgrenze erzwungen.

## D4 — Cross-Target `x86_64-pc-windows-gnu` für CI-Validierung
**Status:** entschieden (Umgebungsbedingt).
Entwicklung in Linux-Sandbox. `windows-rs` cross-checkt sauber mit
mingw-w64 (verifiziert). CI/Reviews nutzen `cargo check --target
x86_64-pc-windows-gnu`. Der finale MSVC-Release-Build + echte Laufzeittests
laufen auf dem Windows-Rechner des Users. Plattformneutrale Crates
(storage/blacklist/config) werden nativ getestet.

## D5 — Snapshot-Textbudget 20 KB, UIA-Walk tiefenbegrenzt
**Status:** entschieden.
Pro Snapshot max. 20 KB sichtbarer Text (`truncated`-Flag bei Überschreitung).
UIA-TreeWalk mit Tiefen- und Knotenlimit gegen Latenz-Spitzen bei riesigen
Element-Bäumen. Passwortfeld-Subtrees werden übersprungen.

## D11 — Semantische Suche via BLOB-Embeddings + Brute-Force-Cosinus (nicht sqlite-vec)
**Status:** entschieden (Etappe 3), revidiert Roadmap.
Die Roadmap nannte `sqlite-vec`. Diese C-Extension muss geladen/gebündelt werden
und ist über die Linux→Windows-Cross-Compile-Grenze (D4) heikel. Für persönliche
Datenmengen (einige tausend Notizen) genügt Brute-Force: Embeddings als
`BLOB` (little-endian f32) in `note_embeddings` speichern und die Cosinus-
Ähnlichkeit in reinem Rust berechnen — plattformneutral, nativ testbar,
Millisekunden-schnell. `sqlite-vec` kann später nachgerüstet werden, wenn die
Datenmenge es erfordert (die Embedding-Spalte bleibt wiederverwendbar).

## D10 — Notizen als Markdown-Vault (Dateien), Metadaten in `notes`-Tabelle
**Status:** entschieden (Etappe 2).
Die KI-Destillate werden als `.md`-Dateien in einem Vault-Verzeichnis abgelegt
(`%APPDATA%\MerkWerk\vault`, konfigurierbar) — direkt Obsidian-kompatibel, das
ist die Produktidee. Eine Tabelle `notes` (Migration in Etappe 2) hält Metadaten
(Dateipfad, Quellzeitraum, Erstellzeit, Modell), nicht den Notizinhalt selbst —
die Datei ist die Quelle der Wahrheit. So bleibt der Vault auch ohne die App
nutzbar/versionierbar.

## D9 — Lokale Inferenz über Trait, Backend v1 = Ollama
**Status:** entschieden (Etappe 2), austauschbar.
Die KI läuft lokal. Wegen „kein Python, kein Docker" (Kickoff) und für einfache,
robuste Integration ist Backend v1 **Ollama** (eigenständige lokale App, HTTP-API
auf `127.0.0.1:11434`) — aus Rust nur HTTP+JSON. Gekapselt hinter einem Trait
`Inference { generate(prompt), embed(text) }`, sodass ein späterer Wechsel auf
eingebettetes llama.cpp (`llama-cpp-2`) oder Candle nur eine neue Trait-Impl ist,
ohne Änderung an Destillierer/Embedding-Code. Modellname/Endpoint sind konfigurierbar.

## D8 — App liest read-only via `PRAGMA query_only`, nicht `SQLITE_OPEN_READ_ONLY`
**Status:** entschieden.
Die App (`storage::Store::open_readonly`) öffnet die DB read-**write** (damit die
WAL-Sidecars `-shm`/`-wal` für einen Leser funktionieren) und erzwingt die
Nur-Lese-Semantik über `PRAGMA query_only = ON`. Ein striktes
`SQLITE_OPEN_READ_ONLY` scheitert bei WAL, weil der Leser das benötigte `-shm`
nicht anlegen darf. `query_only` weist jede Mutation auf SQLite-Ebene ab
(per Test verifiziert) und erfüllt so D2 („App schreibt nie") verlässlich.
Es wird keine Migration ausgeführt — ein Leser verändert das Schema nie.

## D7 — rusqlite auf 0.31 gepinnt (Toolchain-Kompatibilität)
**Status:** entschieden (Umgebungsbedingt, nach Subagent-Blocker).
`rusqlite 0.40` zieht `libsqlite3-sys 0.38`, dessen build.rs das **unstable**
`cfg_select!`-Makro nutzt (rust-lang #115585) und damit auf stable rustc 1.94.1
NICHT baut — der Storage-Subagent lief hier auf. Fix: `rusqlite = "0.31"`
(bundled → `libsqlite3-sys 0.28`, kein `cfg_select`). Die genutzte API
(Connection, transaction, execute, query_row) ist zwischen 0.31 und 0.40
identisch, kein Code-Umbau nötig. Beim Upgrade der Toolchain kann rusqlite
wieder angehoben werden — dann diesen Pin entfernen.

## D6 — Workspace-Struktur des Daemons: Bibliotheks-Crates + dünnes Binary
**Status:** entschieden.
`daemon/` als Cargo-Workspace: plattformneutrale Logik (storage, blacklist,
config, ipc-protocol) in eigenen lib-Crates mit nativen Tests; die
Windows-spezifische Erfassung (hooks, uia) in einem Crate hinter
`#[cfg(windows)]`; `merkwerk-daemon` bin verdrahtet alles. So laufen die
Test-Suites in der Linux-Sandbox, ohne dass Windows-APIs sie blockieren.
