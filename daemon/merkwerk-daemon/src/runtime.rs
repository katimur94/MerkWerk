//! Daemon-Laufzeit (Windows): verdrahtet Hooks → Debouncer → Blacklist → UIA →
//! Storage und den IPC-Server zu einem laufenden System (Etappe 0, T8).
//!
//! Threads (Kommunikation über `crossbeam-channel`):
//! - **Hook-Thread** (in `capture_win::hooks`): sendet [`RawSignal`].
//! - **UIA-Thread** ([`uia_thread`]): besitzt den (`!Send`) [`UiaSnapshotter`],
//!   erzeugt Snapshots off-thread, damit langsame COM-Aufrufe den Erfassungs-
//!   Loop nie blockieren.
//! - **IPC-Thread** ([`crate::ipc_server`]): Named-Pipe-Steuerkanal.
//! - **Erfassungs-Loop** (dieser Thread): einziger Besitzer des [`Store`] (D2 —
//!   ein Schreiber), führt den `app_session`-Lebenszyklus und wendet die
//!   Blacklist *an der Quelle* an.
//!
//! **Privacy an der Quelle:** Ein Fenster, dessen Prozess/Titel (bei Fokuswechsel)
//! oder dessen aufgelöste URL/Titel (nach dem Snapshot) auf die Blacklist trifft,
//! erzeugt **keine** Zeile in der DB — weder Session noch Event noch Snapshot.
//! Und da nur aggregierte Zähler aus dem Debouncer geschrieben werden (nie ein
//! [`RawSignal::KeyTick`]-Inhalt — den gibt es nicht), kann kein Tastenanschlag
//! persistiert werden.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{select, unbounded, Receiver, Sender};

use blacklist::Blacklist;
use capture_win::debounce::Debouncer;
use capture_win::text_budget::SnapshotConfig;
use capture_win::{hooks, uia::UiaSnapshotter, window, RawSignal, Snapshot, Trigger};
use config::Config;
use storage::Store;

use crate::control::Shared;
use crate::ipc_server;
use crate::policy;

/// Auftrag an den UIA-Thread, einen Snapshot für ein Fenster zu erstellen.
struct SnapshotJob {
    hwnd: isize,
    session_id: i64,
    event_id: Option<i64>,
    process_name: String,
    ts_ms: i64,
    cfg: SnapshotConfig,
}

/// Ergebnis des UIA-Threads: der erzeugte Snapshot samt Zuordnung.
struct SnapshotResult {
    session_id: i64,
    event_id: Option<i64>,
    process_name: String,
    ts_ms: i64,
    snap: Snapshot,
}

/// Aktuell fokussiertes Fenster (Erfassungs-Loop-lokaler Zustand).
struct Active {
    hwnd: isize,
    /// `None`, wenn für dieses Fenster bewusst keine Session existiert (Blacklist).
    session_id: Option<i64>,
    process_name: String,
    blocked: bool,
}

/// Wall-clock-Zeit in Unix-Millisekunden (gleiche Zeitbasis wie die Hook-Signale).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// TTL-Ablaufzeitpunkt (`ts + ttl`) oder `None`, wenn TTL deaktiviert (0) ist.
fn ttl(ts_ms: i64, ttl_ms: i64) -> Option<i64> {
    if ttl_ms > 0 {
        Some(ts_ms + ttl_ms)
    } else {
        None
    }
}

fn build_blacklist(cfg: &Config) -> Blacklist {
    Blacklist::new(
        cfg.blacklist.process_names.clone(),
        cfg.blacklist.title_patterns.clone(),
        cfg.blacklist.url_patterns.clone(),
    )
}

fn build_debouncer(cfg: &Config) -> Debouncer {
    Debouncer::new(
        cfg.debounce.typing_pause_ms,
        cfg.debounce.click_cluster_ms,
        cfg.debounce.scroll_end_ms,
        cfg.debounce.min_focus_ms,
    )
}

fn build_snap_cfg(cfg: &Config) -> SnapshotConfig {
    SnapshotConfig {
        max_text_bytes: cfg.snapshot.max_text_bytes,
        max_tree_depth: cfg.snapshot.max_tree_depth,
        max_nodes: cfg.snapshot.max_nodes,
    }
}

/// UIA-Thread: erstellt den Snapshotter (muss auf *diesem* Thread passieren, da
/// `UiaSnapshotter` `!Send` ist) und bedient Snapshot-Aufträge, bis der Kanal schließt.
fn uia_thread(rx: Receiver<SnapshotJob>, tx: Sender<SnapshotResult>) {
    let snapshotter = match UiaSnapshotter::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[uia] Initialisierung fehlgeschlagen, Snapshots deaktiviert: {e}");
            // Kanal leerlaufen lassen, damit der Erfassungs-Loop nicht blockiert.
            while rx.recv().is_ok() {}
            return;
        }
    };
    while let Ok(job) = rx.recv() {
        let snap = snapshotter.snapshot(job.hwnd, job.cfg);
        let _ = tx.send(SnapshotResult {
            session_id: job.session_id,
            event_id: job.event_id,
            process_name: job.process_name,
            ts_ms: job.ts_ms,
            snap,
        });
    }
}

/// Startet alle Threads und betreibt den Erfassungs-Loop. Kehrt nur bei einem
/// nicht behebbaren Startfehler zurück (z. B. DB nicht öffenbar).
pub fn run(
    mut cfg: Config,
    config_path: PathBuf,
    db_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = Store::open(&db_path)?;
    let mut blacklist = build_blacklist(&cfg);
    let mut debouncer = build_debouncer(&cfg);
    let mut snap_cfg = build_snap_cfg(&cfg);
    let mut ttl_ms = cfg.retention.ttl_days as i64 * 86_400_000;

    let shared = Arc::new(Shared::new());

    // IPC-Server-Thread.
    {
        let s = shared.clone();
        thread::spawn(move || {
            if let Err(e) = ipc_server::serve_blocking(s) {
                eprintln!("[ipc] Named-Pipe-Server beendet: {e}");
            }
        });
    }

    // Kanäle: Hooks -> Loop, Loop -> UIA, UIA -> Loop.
    let (tx_raw, rx_raw) = unbounded::<RawSignal>();
    let (tx_job, rx_job) = unbounded::<SnapshotJob>();
    let (tx_snap, rx_snap) = unbounded::<SnapshotResult>();

    // UIA-Thread.
    thread::spawn(move || uia_thread(rx_job, tx_snap));

    // Hook-Thread (läuft in capture_win). `_hooks` muss am Leben bleiben — sein
    // `Drop` deinstalliert die Hooks und beendet den Hook-Thread.
    let _hooks = hooks::start_hooks(tx_raw);

    let mut active: Option<Active> = None;
    let mut prev_paused = false;
    let tick = Duration::from_millis(250);

    loop {
        // Reload angefordert? Konfig + abgeleitete Objekte neu aufbauen.
        if shared.take_reload_request() {
            match Config::load(&config_path) {
                Ok(newcfg) => {
                    cfg = newcfg;
                    blacklist = build_blacklist(&cfg);
                    debouncer = build_debouncer(&cfg);
                    snap_cfg = build_snap_cfg(&cfg);
                    ttl_ms = cfg.retention.ttl_days as i64 * 86_400_000;
                }
                Err(e) => eprintln!("[config] Reload fehlgeschlagen: {e}"),
            }
        }

        // Pause-Flanke: beim Pausieren die laufende Session sauber beenden.
        let paused = shared.is_paused();
        if paused && !prev_paused {
            if let Some(a) = active.as_ref() {
                if let Some(sid) = a.session_id {
                    let _ = store.end_app_session(sid, now_ms());
                }
            }
            active = None;
        }
        prev_paused = paused;

        select! {
            recv(rx_raw) -> msg => {
                if let Ok(sig) = msg {
                    for t in debouncer.feed(sig) {
                        handle_trigger(t, &store, &blacklist, &shared, &tx_job, snap_cfg, ttl_ms, &mut active);
                    }
                }
            }
            recv(rx_snap) -> msg => {
                if let Ok(res) = msg {
                    handle_snapshot_result(res, &store, &blacklist, &shared, ttl_ms);
                }
            }
            default(tick) => {
                for t in debouncer.tick(now_ms() as u64) {
                    handle_trigger(t, &store, &blacklist, &shared, &tx_job, snap_cfg, ttl_ms, &mut active);
                }
            }
        }
    }
}

/// Verarbeitet einen Debouncer-Trigger: Session-Lebenszyklus, Blacklist an der
/// Quelle, Event-Persistenz und Snapshot-Anforderung.
#[allow(clippy::too_many_arguments)]
fn handle_trigger(
    trigger: Trigger,
    store: &Store,
    blacklist: &Blacklist,
    shared: &Shared,
    tx_job: &Sender<SnapshotJob>,
    snap_cfg: SnapshotConfig,
    ttl_ms: i64,
    active: &mut Option<Active>,
) {
    if shared.is_paused() {
        return;
    }

    match trigger {
        Trigger::FocusChange { hwnd, ts_ms } => {
            let ts = ts_ms as i64;
            let info = window::window_info(hwnd);
            let (process, title) = match info {
                Some(i) => (i.process_name, i.title),
                None => (String::new(), String::new()),
            };

            // Vorherige (andere) Session beenden.
            if let Some(a) = active.as_ref() {
                if a.hwnd != hwnd {
                    if let Some(sid) = a.session_id {
                        let _ = store.end_app_session(sid, ts);
                    }
                }
            }

            // Blacklist an der Quelle: kein ermittelbarer Prozess ODER Treffer ->
            // keine Session, kein Event, kein Snapshot für dieses Fenster.
            if policy::focus_decision(&process, &title, blacklist).is_block() {
                *active = Some(Active {
                    hwnd,
                    session_id: None,
                    process_name: process,
                    blocked: true,
                });
                return;
            }

            let expires = ttl(ts, ttl_ms);
            match store.insert_app_session(&process, ts, expires) {
                Ok(sid) => {
                    let event_id = store
                        .insert_event(Some(sid), "focus_change", ts, None, None, expires)
                        .ok();
                    shared.inc_events(1);
                    *active = Some(Active {
                        hwnd,
                        session_id: Some(sid),
                        process_name: process.clone(),
                        blocked: false,
                    });
                    let _ = tx_job.send(SnapshotJob {
                        hwnd,
                        session_id: sid,
                        event_id,
                        process_name: process,
                        ts_ms: ts,
                        cfg: snap_cfg,
                    });
                }
                Err(e) => eprintln!("[db] insert_app_session: {e}"),
            }
        }
        Trigger::TypingSettled {
            hwnd,
            ts_ms,
            key_count,
            duration_ms,
        } => record_activity(
            store,
            shared,
            tx_job,
            snap_cfg,
            ttl_ms,
            active,
            hwnd,
            ts_ms as i64,
            "typing_burst",
            Some(duration_ms as i64),
            Some(key_count as i64),
        ),
        Trigger::ClickCluster {
            hwnd,
            ts_ms,
            click_count,
        } => record_activity(
            store,
            shared,
            tx_job,
            snap_cfg,
            ttl_ms,
            active,
            hwnd,
            ts_ms as i64,
            "click_cluster",
            None,
            Some(click_count as i64),
        ),
        Trigger::ScrollEnd { hwnd, ts_ms } => record_activity(
            store,
            shared,
            tx_job,
            snap_cfg,
            ttl_ms,
            active,
            hwnd,
            ts_ms as i64,
            "scroll_end",
            None,
            None,
        ),
    }
}

/// Persistiert ein Aktivitäts-Event (Tippen/Klicken/Scrollen) und fordert einen
/// Snapshot an — aber nur, wenn das betreffende Fenster das aktive, nicht
/// geblockte Fenster mit einer Session ist. So erhält ein Blacklist-Fenster
/// niemals Aktivitäts-Zeilen.
#[allow(clippy::too_many_arguments)]
fn record_activity(
    store: &Store,
    shared: &Shared,
    tx_job: &Sender<SnapshotJob>,
    snap_cfg: SnapshotConfig,
    ttl_ms: i64,
    active: &Option<Active>,
    hwnd: isize,
    ts: i64,
    kind: &str,
    duration_ms: Option<i64>,
    count: Option<i64>,
) {
    let Some(a) = active.as_ref() else {
        return;
    };
    if a.blocked || a.hwnd != hwnd {
        return;
    }
    let Some(sid) = a.session_id else {
        return;
    };

    let expires = ttl(ts, ttl_ms);
    match store.insert_event(Some(sid), kind, ts, duration_ms, count, expires) {
        Ok(event_id) => {
            shared.inc_events(1);
            let _ = tx_job.send(SnapshotJob {
                hwnd,
                session_id: sid,
                event_id: Some(event_id),
                process_name: a.process_name.clone(),
                ts_ms: ts,
                cfg: snap_cfg,
            });
        }
        Err(e) => eprintln!("[db] insert_event({kind}): {e}"),
    }
}

/// Schreibt einen fertigen Snapshot — nach einer erneuten Blacklist-Prüfung *mit*
/// der jetzt bekannten URL/Titel (URL-Blacklist an der Quelle). Leere Snapshots
/// (kein Titel, keine URL, kein Text) werden verworfen.
fn handle_snapshot_result(
    res: SnapshotResult,
    store: &Store,
    blacklist: &Blacklist,
    shared: &Shared,
    ttl_ms: i64,
) {
    if shared.is_paused() {
        return;
    }

    // URL-/Titel-Blacklist an der Quelle: erst jetzt ist die URL bekannt.
    if policy::snapshot_blocked(
        &res.process_name,
        res.snap.window_title.as_deref(),
        res.snap.url.as_deref(),
        blacklist,
    ) {
        return;
    }

    if policy::snapshot_is_empty(
        res.snap.window_title.as_deref(),
        res.snap.url.as_deref(),
        res.snap.text_content.as_deref(),
    ) {
        return;
    }

    let expires = ttl(res.ts_ms, ttl_ms);
    if store
        .insert_snapshot(
            Some(res.session_id),
            res.event_id,
            res.ts_ms,
            res.snap.window_title.as_deref(),
            res.snap.url.as_deref(),
            res.snap.text_content.as_deref(),
            res.snap.truncated,
            expires,
        )
        .is_ok()
    {
        shared.inc_snapshots(1);
    }
}
