//! MerkWerk app shell — Tauri 2 backend.
//!
//! Per `ARCHITEKTUR.md`, `merkwerk-app` only ever *reads* the daemon's
//! SQLite database (read-only) and *controls* the daemon over the Named
//! Pipe IPC channel described in `ENTSCHEIDUNGEN.md` D1/D2 and implemented
//! by `daemon/ipc-protocol`. This crate is intentionally standalone (its
//! own `Cargo.toml`/`Cargo.lock`, empty `[workspace]`) and does not depend
//! on the daemon crates — see `app/src-tauri/Cargo.toml`.
//!
//! The tray icon and its menu are built entirely in Rust (`setup()` below)
//! rather than declared in `tauri.conf.json`'s `app.trayIcon`, so there is
//! exactly one tray icon and its "Start/Pause"/"Status"/"Beenden" items can
//! carry real event handlers (JSON tray config can't express that).
//!
//! `get_daemon_status`/`pause_daemon`/`resume_daemon` below and the tray's
//! "Start/Pause" item all go over the same real IPC round-trip
//! (`ipc_client::ipc_request`, see there) — there is no locally cached
//! daemon state in this process anymore; every poll/toggle asks the daemon
//! itself over `\\.\pipe\merkwerk`, so the frontend and the tray can never
//! disagree with the daemon's actual state.

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

mod ipc_client;
mod notes;
mod paths;
mod search;
mod semantic;
mod settings;
mod timeline;

/// Mirrors `ipc_protocol::Response::Status` (see
/// `daemon/ipc-protocol/src/lib.rs`) — the shape `get_daemon_status` fills
/// from the real IPC reply below. `Default` deliberately produces the
/// "offline" reading (nothing running, nothing paused, all counters at
/// zero), which doubles as the fallback value on any IPC error.
#[derive(Debug, Clone, Default, Serialize)]
struct DaemonStatus {
    running: bool,
    paused: bool,
    events_captured: u64,
    snapshots_captured: u64,
    uptime_secs: u64,
}

/// Read the daemon's current status via IPC (`Request::GetStatus`).
///
/// Returns `DaemonStatus` rather than a `Result`: if the daemon isn't
/// reachable (pipe missing because the daemon isn't running) or answers
/// with anything other than `Response::Status`, this deliberately returns
/// `DaemonStatus::default()` (`running: false`, everything else `0`)
/// instead of an `Err`. The frontend polls this every few seconds — turning
/// "daemon offline" into an error would make every poll tick while the
/// daemon isn't running look like a failure, when it's simply the normal
/// "not running yet" state the status bar needs to render.
#[tauri::command]
fn get_daemon_status() -> DaemonStatus {
    match ipc_client::ipc_request(&ipc_protocol::Request::GetStatus) {
        Ok(ipc_protocol::Response::Status {
            running,
            paused,
            events_captured,
            snapshots_captured,
            uptime_secs,
        }) => DaemonStatus {
            running,
            paused,
            events_captured,
            snapshots_captured,
            uptime_secs,
        },
        Ok(_) | Err(_) => DaemonStatus::default(),
    }
}

/// Ask the daemon to pause capturing via IPC (`Request::Pause`).
///
/// Unlike `get_daemon_status`, this *does* surface failures: a Pause click
/// that silently did nothing (daemon unreachable) must be visible to the
/// caller, which re-fetches the status right after — see
/// `StatusBar.tsx`/the tray's `toggle_pause` handler below.
#[tauri::command]
fn pause_daemon() -> Result<(), String> {
    expect_ok(ipc_client::ipc_request(&ipc_protocol::Request::Pause)?)
}

/// Ask the daemon to resume capturing via IPC (`Request::Resume`).
#[tauri::command]
fn resume_daemon() -> Result<(), String> {
    expect_ok(ipc_client::ipc_request(&ipc_protocol::Request::Resume)?)
}

/// Turns an IPC `Response` for a Pause/Resume request into `Result<(), String>`:
/// `Ok` succeeds, `Error { message }` surfaces that message, and a
/// `Status { .. }` reply (never sent for these two requests, see
/// `daemon/merkwerk-daemon/src/control.rs::Shared::handle`) is treated as a
/// protocol mismatch rather than silently accepted.
fn expect_ok(response: ipc_protocol::Response) -> Result<(), String> {
    match response {
        ipc_protocol::Response::Ok => Ok(()),
        ipc_protocol::Response::Error { message } => Err(message),
        ipc_protocol::Response::Status { .. } => {
            Err("Unerwartete Antwort vom Daemon (Status statt Ok/Error)".to_string())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let toggle_pause =
                MenuItem::with_id(app, "toggle_pause", "Start/Pause", true, None::<&str>)?;
            let status = MenuItem::with_id(app, "status", "Status", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_pause, &status, &quit])?;

            TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .expect("default window icon must be set via tauri.conf.json bundle.icon"),
                )
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "toggle_pause" => {
                        // Aktuellen Zustand per IPC erfragen und je nachdem
                        // Pause oder Resume schicken — kein lokal
                        // mitgeführtes Flag mehr, das vom echten
                        // Daemon-Zustand abweichen könnte. Ist der Daemon
                        // nicht erreichbar, meldet get_daemon_status()
                        // `running: false, paused: false`, der Handler
                        // versucht dann `pause_daemon()`, was ebenfalls
                        // fehlschlägt; der Fehler wird geloggt statt das
                        // Menüevent abstürzen zu lassen.
                        let status = get_daemon_status();
                        let result = if status.paused {
                            resume_daemon()
                        } else {
                            pause_daemon()
                        };
                        if let Err(err) = result {
                            eprintln!("toggle_pause: IPC an den Daemon fehlgeschlagen: {err}");
                        }
                    }
                    "status" => {
                        // Der Live-Status (Running/Paused, Zähler, Laufzeit)
                        // wird bereits in der App-Statusleiste angezeigt
                        // (StatusBar.tsx, pollt get_daemon_status()) — dieser
                        // Menüeintrag muss ihn also nicht zusätzlich selbst
                        // abfragen, sondern holt nur das Hauptfenster nach
                        // vorne, damit die Statusleiste sichtbar wird.
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_daemon_status,
            pause_daemon,
            resume_daemon,
            timeline::list_timeline,
            search::search_snapshots,
            notes::list_notes,
            notes::get_note_markdown,
            notes::distill_now,
            semantic::semantic_search_notes,
            settings::get_blacklist,
            settings::set_blacklist,
            settings::get_autostart,
            settings::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the MerkWerk tauri application");
}
