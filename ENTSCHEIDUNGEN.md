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

## D6 — Workspace-Struktur des Daemons: Bibliotheks-Crates + dünnes Binary
**Status:** entschieden.
`daemon/` als Cargo-Workspace: plattformneutrale Logik (storage, blacklist,
config, ipc-protocol) in eigenen lib-Crates mit nativen Tests; die
Windows-spezifische Erfassung (hooks, uia) in einem Crate hinter
`#[cfg(windows)]`; `merkwerk-daemon` bin verdrahtet alles. So laufen die
Test-Suites in der Linux-Sandbox, ohne dass Windows-APIs sie blockieren.
