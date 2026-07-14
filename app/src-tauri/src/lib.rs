//! MerkWerk app shell — Tauri 2 backend (Etappe 0).
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
//! Every command here is a typed placeholder for Etappe 0: they compile and
//! return well-shaped (but fake) data, and each is marked with a `TODO` for
//! the real implementation that a later task wires up.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};

mod paths;
mod settings;
mod timeline;

/// Placeholder in-process stand-in for the daemon's real run state.
///
/// Once IPC is wired up this goes away: `get_daemon_status`/`pause_daemon`/
/// `resume_daemon` will ask the daemon itself (over `\\.\pipe\merkwerk`)
/// instead of reading/writing a local flag. Until then, this is the single
/// shared source of truth for both the tray menu and the frontend, so they
/// can't disagree with each other.
#[derive(Default)]
struct DaemonState {
    paused: Mutex<bool>,
}

/// Mirrors `ipc_protocol::Response::Status` (see
/// `daemon/ipc-protocol/src/lib.rs`) — the shape the real `get_status` IPC
/// reply will eventually fill in.
#[derive(Debug, Clone, Serialize)]
struct DaemonStatus {
    running: bool,
    paused: bool,
    events_captured: u64,
    snapshots_captured: u64,
    uptime_secs: u64,
}

/// Read the daemon's current status.
///
/// TODO: IPC an \\.\pipe\merkwerk — `Request::GetStatus` senden (siehe
/// `daemon/ipc-protocol`) und die echte `Response::Status` zurückgeben,
/// statt den lokal mitgeführten Platzhalter-Zustand.
#[tauri::command]
fn get_daemon_status(state: State<'_, DaemonState>) -> DaemonStatus {
    let paused = *state.paused.lock().expect("daemon state mutex poisoned");
    DaemonStatus {
        running: false,
        paused,
        events_captured: 0,
        snapshots_captured: 0,
        uptime_secs: 0,
    }
}

/// Ask the daemon to pause capturing.
///
/// TODO: IPC an \\.\pipe\merkwerk — `Request::Pause` senden.
#[tauri::command]
fn pause_daemon(state: State<'_, DaemonState>) -> Result<(), String> {
    *state.paused.lock().expect("daemon state mutex poisoned") = true;
    Ok(())
}

/// Ask the daemon to resume capturing.
///
/// TODO: IPC an \\.\pipe\merkwerk — `Request::Resume` senden.
#[tauri::command]
fn resume_daemon(state: State<'_, DaemonState>) -> Result<(), String> {
    *state.paused.lock().expect("daemon state mutex poisoned") = false;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(DaemonState::default())
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
                        // TODO: IPC an \\.\pipe\merkwerk (Pause bzw. Resume,
                        // je nach aktuellem Zustand) — siehe pause_daemon /
                        // resume_daemon oben.
                        let state = app.state::<DaemonState>();
                        let mut paused =
                            state.paused.lock().expect("daemon state mutex poisoned");
                        *paused = !*paused;
                    }
                    "status" => {
                        // TODO: get_daemon_status() abfragen und im Fenster
                        // anzeigen. Für jetzt: Hauptfenster in den
                        // Vordergrund holen.
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
            settings::get_blacklist,
            settings::set_blacklist,
            settings::get_autostart,
            settings::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the MerkWerk tauri application");
}
