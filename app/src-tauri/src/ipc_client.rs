//! IPC-Client für die Daemon-Named-Pipe (`\\.\pipe\merkwerk`, siehe
//! `daemon/ipc-protocol` für das Wire-Format und
//! `daemon/merkwerk-daemon/src/ipc_server.rs` für den Server, der jede
//! Verbindung entgegennimmt).
//!
//! [`ipc_request`] ist die eine Stelle, die tatsächlich Bytes über die Pipe
//! schickt und liest — `get_daemon_status`/`pause_daemon`/`resume_daemon`
//! (siehe `lib.rs`) bauen jeweils nur den passenden `Request` und werten die
//! `Response` aus. `settings::send_reload_config_over_pipe` und
//! `notes::send_distill_now_over_pipe` bleiben bewusst eigenständig
//! (fire-and-forget ohne Antwort zu lesen) — dieses Modul kommt hinzu, weil
//! `get_daemon_status` erstmals eine *Antwort* braucht, nicht nur ein
//! zugestelltes Kommando.
//!
//! Eine Verbindung = ein Request/Response-Paar: Pipe öffnen, eine JSONL-Zeile
//! schreiben, eine JSONL-Zeile zurücklesen, Handle beim Verlassen der
//! Funktion fallen lassen (schließt die Verbindung). Der Server bedient pro
//! Verbindung beliebig viele Requests sequenziell und beendet die Verbindung
//! erst, wenn der Client sie schließt (siehe `ipc_server.rs`,
//! `serve_one_client`) — ein Client, der nach der ersten Antwort schließt,
//! ist für ihn ein ganz normaler Disconnect.

use ipc_protocol::{decode_response, encode_request, Request, Response};

/// Schickt `request` über die Daemon-Named-Pipe und liefert die dekodierte
/// `Response`.
///
/// Alle Fehlerfälle (Pipe nicht vorhanden bzw. nicht verbindbar — der
/// häufigste Fall: der Daemon läuft nicht —, Schreib-/Lesefehler mitten in
/// der Verbindung, eine leere oder nicht als `Response` dekodierbare
/// Antwortzeile) werden einheitlich als `Err(String)` gemeldet. Die
/// Aufrufer in `lib.rs` entscheiden selbst, wie sie damit umgehen (z. B.
/// `get_daemon_status` verwandelt jeden Fehler hier in einen
/// "Daemon offline"-Status statt ihn weiterzureichen).
#[cfg(windows)]
pub fn ipc_request(request: &Request) -> Result<Response, String> {
    use std::io::{BufRead, BufReader, Write};

    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(ipc_protocol::PIPE_NAME)
        .map_err(|e| format!("MerkWerk-Daemon nicht erreichbar — läuft er? ({e})"))?;

    pipe.write_all(encode_request(request).as_bytes())
        .map_err(|e| format!("IPC-Anfrage konnte nicht gesendet werden: {e}"))?;

    // `&File` implementiert `Read` (siehe std::fs::File-Doku), daher genügt
    // eine geliehene Referenz — `pipe` bleibt Eigentümerin des Handles und
    // wird am Funktionsende automatisch geschlossen.
    let mut line = String::new();
    BufReader::new(&pipe)
        .read_line(&mut line)
        .map_err(|e| format!("IPC-Antwort konnte nicht gelesen werden: {e}"))?;

    if line.trim().is_empty() {
        return Err("MerkWerk-Daemon hat die Verbindung ohne Antwort beendet".to_string());
    }

    decode_response(&line).map_err(|e| format!("Ungültige IPC-Antwort vom Daemon: {e}"))
}

/// Named-Pipe-IPC ist laut ENTSCHEIDUNGEN.md D1/D6 Windows-only — auf
/// anderen Zielplattformen gibt es keinen Daemon, der erreichbar wäre.
#[cfg(not(windows))]
pub fn ipc_request(_request: &Request) -> Result<Response, String> {
    Err("IPC ist nur unter Windows verfügbar.".to_string())
}
