//! Named-Pipe-IPC-Server (`\\.\pipe\merkwerk`, siehe ENTSCHEIDUNGEN.md D1).
//!
//! Reiner Transport: nimmt jeweils einen Client (die App) an, liest
//! zeilenweise JSON-Requests (JSONL), reicht sie an [`Shared::handle`]
//! (plattformneutrale IPC-Semantik, dort getestet) und schreibt die
//! JSONL-Response zurück. Ein Byte-Mode-Pipe genügt für das kleine, strikt
//! Request/Response-förmige Protokoll; pro Verbindung wird sequentiell
//! bedient, nach Disconnect wird die nächste Verbindung angenommen.

use std::sync::Arc;

use ipc_protocol::{decode_request, encode_response, Response, PIPE_NAME};
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};

use crate::control::Shared;

const BUF_SIZE: u32 = 8 * 1024;

/// Blockierender IPC-Server-Loop. Läuft, bis der Prozess endet; erzeugt für den
/// Named-Pipe-Namen jeweils eine Instanz pro Client und bedient sie sequentiell.
///
/// Fehler beim Anlegen der Pipe (z. B. bereits belegter Name) werden als Ergebnis
/// zurückgegeben; transiente Fehler pro Verbindung (Client bricht ab) beenden nur
/// die aktuelle Verbindung, nicht den Server.
pub fn serve_blocking(shared: Arc<Shared>) -> windows::core::Result<()> {
    let pipe_name = HSTRING::from(PIPE_NAME);
    loop {
        // Eine frische Pipe-Instanz je Verbindung. `PIPE_WAIT` = blockierende
        // Semantik; `nMaxInstances = 1`, weil nur die App als einziger Client
        // spricht. Default-Security (kein `lpSecurityAttributes`) beschränkt den
        // Zugriff auf die aktuelle Session/den User.
        // SAFETY: `pipe_name` lebt für die Dauer des Aufrufs; alle Parameter sind
        // gültige Konstanten des windows-crate.
        let handle = unsafe {
            CreateNamedPipeW(
                &pipe_name,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                BUF_SIZE,
                BUF_SIZE,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(windows::core::Error::from_win32());
        }

        // Auf einen Client warten. Schlägt das fehl, Pipe schließen und neu anlegen.
        // SAFETY: `handle` ist eine gültige, eben erzeugte Pipe-Instanz.
        let connected = unsafe { ConnectNamedPipe(handle, None) }.is_ok();
        if connected {
            serve_one_client(handle, &shared);
        }

        // SAFETY: `handle` stammt aus dem erfolgreichen CreateNamedPipeW oben und
        // wird hier genau einmal freigegeben.
        unsafe {
            let _ = DisconnectNamedPipe(handle);
            let _ = CloseHandle(handle);
        }
    }
}

/// Bedient eine einzelne Verbindung: liest Bytes, zerlegt sie an `\n` in
/// JSONL-Requests, beantwortet jeden Request. Endet, wenn der Client die
/// Verbindung schließt (ReadFile liefert 0 Bytes / Fehler).
fn serve_one_client(handle: HANDLE, shared: &Shared) {
    let mut pending = String::new();
    let mut buf = [0u8; BUF_SIZE as usize];

    loop {
        let mut read: u32 = 0;
        // SAFETY: `handle` ist verbunden; `buf` ist ein gültiger, exklusiv
        // geliehener Slice; `read` nimmt die Anzahl gelesener Bytes auf.
        let ok = unsafe { ReadFile(handle, Some(&mut buf), Some(&mut read), None) }.is_ok();
        if !ok || read == 0 {
            break; // Client hat die Verbindung geschlossen.
        }

        pending.push_str(&String::from_utf8_lossy(&buf[..read as usize]));

        // Alle vollständigen Zeilen im Puffer verarbeiten; ein evtl. unvollständiger
        // Rest bleibt für den nächsten ReadFile stehen.
        while let Some(nl) = pending.find('\n') {
            let line: String = pending.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            let response = match decode_request(line) {
                Ok(req) => shared.handle(req),
                Err(e) => Response::Error {
                    message: format!("ungültiger Request: {e}"),
                },
            };
            if !write_all(handle, encode_response(&response).as_bytes()) {
                return; // Schreiben fehlgeschlagen -> Verbindung aufgeben.
            }
        }
    }
}

/// Schreibt den kompletten Puffer (ReadFile/WriteFile können teilweise schreiben).
/// Gibt `false` zurück, sobald ein Schreibversuch fehlschlägt.
fn write_all(handle: HANDLE, mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        let mut written: u32 = 0;
        // SAFETY: `handle` ist verbunden; `bytes` ist ein gültiger Slice; `written`
        // nimmt die Anzahl geschriebener Bytes auf.
        let ok = unsafe { WriteFile(handle, Some(bytes), Some(&mut written), None) }.is_ok();
        if !ok || written == 0 {
            return false;
        }
        bytes = &bytes[written as usize..];
    }
    true
}
