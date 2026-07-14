//! Geteilter Steuer-/Statuszustand des Daemons — plattformneutral.
//!
//! Bündelt die Laufzeit-Flags (Pause, Reload-Anforderung) und Zähler
//! (Events/Snapshots, Uptime) hinter atomaren Feldern, sodass der IPC-Thread und
//! der Erfassungs-Loop denselben Zustand ohne Locks teilen. [`Shared::handle`]
//! bildet ein IPC-`Request` auf ein `Response` ab; die Methode ist bewusst hier
//! (ohne Named-Pipe-/Windows-Bezug) definiert, damit die IPC-Semantik nativ
//! getestet werden kann — der Named-Pipe-Server ([`crate::ipc_server`]) ist dann
//! nur noch Transport, der `handle` aufruft.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use ipc_protocol::{Request, Response};

/// Gemeinsamer, thread-sicherer Daemon-Zustand. Wird als `Arc<Shared>` geteilt.
#[derive(Debug)]
pub struct Shared {
    /// Erfassung pausiert? Der Erfassungs-Loop schreibt dann nichts in die DB.
    paused: AtomicBool,
    /// Anforderung, Konfig + Blacklist neu zu laden (vom IPC-Thread gesetzt,
    /// vom Erfassungs-Loop konsumiert).
    reload_requested: AtomicBool,
    /// Ausstehende Destillier-Anforderung `(from_ms, to_ms)` (vom IPC-Thread
    /// gesetzt, vom Erfassungs-Loop konsumiert und an den Destillier-Worker
    /// weitergereicht). Nur die jeweils jüngste Anforderung bleibt stehen.
    pending_distill: Mutex<Option<(i64, i64)>>,
    /// Anzahl bislang persistierter Events.
    events_captured: AtomicU64,
    /// Anzahl bislang persistierter Snapshots.
    snapshots_captured: AtomicU64,
    /// Startzeitpunkt für die Uptime-Berechnung.
    started: Instant,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            paused: AtomicBool::new(false),
            reload_requested: AtomicBool::new(false),
            pending_distill: Mutex::new(None),
            events_captured: AtomicU64::new(0),
            snapshots_captured: AtomicU64::new(0),
            started: Instant::now(),
        }
    }
}

impl Shared {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Konsumiert eine ausstehende Reload-Anforderung (gibt `true` zurück, wenn
    /// eine vorlag, und setzt das Flag dabei zurück).
    pub fn take_reload_request(&self) -> bool {
        self.reload_requested.swap(false, Ordering::Relaxed)
    }

    /// Konsumiert eine ausstehende Destillier-Anforderung `(from_ms, to_ms)`
    /// (gibt sie zurück und löscht sie dabei). `None`, wenn keine vorliegt.
    pub fn take_distill_request(&self) -> Option<(i64, i64)> {
        self.pending_distill
            .lock()
            .expect("distill-request mutex poisoned")
            .take()
    }

    pub fn inc_events(&self, n: u64) {
        self.events_captured.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc_snapshots(&self, n: u64) {
        self.snapshots_captured.fetch_add(n, Ordering::Relaxed);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Bildet ein IPC-`Request` auf ein `Response` ab und wendet dabei
    /// Seiteneffekte auf den geteilten Zustand an (Pause setzen/lösen,
    /// Reload anfordern). Reiner, nativ testbarer Kern der IPC-Semantik.
    pub fn handle(&self, req: Request) -> Response {
        match req {
            Request::GetStatus => Response::Status {
                running: true,
                paused: self.paused.load(Ordering::Relaxed),
                events_captured: self.events_captured.load(Ordering::Relaxed),
                snapshots_captured: self.snapshots_captured.load(Ordering::Relaxed),
                uptime_secs: self.uptime_secs(),
            },
            Request::Pause => {
                self.paused.store(true, Ordering::Relaxed);
                Response::Ok
            }
            Request::Resume => {
                self.paused.store(false, Ordering::Relaxed);
                Response::Ok
            }
            Request::DistillNow { from_ms, to_ms } => {
                *self
                    .pending_distill
                    .lock()
                    .expect("distill-request mutex poisoned") = Some((from_ms, to_ms));
                Response::Ok
            }
            Request::ReloadConfig => {
                self.reload_requested.store(true, Ordering::Relaxed);
                Response::Ok
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_and_resume_toggle_state_and_reflect_in_status() {
        let s = Shared::new();
        assert!(!s.is_paused());

        assert!(matches!(s.handle(Request::Pause), Response::Ok));
        assert!(s.is_paused());
        match s.handle(Request::GetStatus) {
            Response::Status {
                paused, running, ..
            } => {
                assert!(paused);
                assert!(running);
            }
            other => panic!("expected Status, got {other:?}"),
        }

        assert!(matches!(s.handle(Request::Resume), Response::Ok));
        assert!(!s.is_paused());
    }

    #[test]
    fn distill_request_is_stashed_by_handle_and_taken_once() {
        let s = Shared::new();
        assert_eq!(s.take_distill_request(), None);

        assert!(matches!(
            s.handle(Request::DistillNow {
                from_ms: 1_000,
                to_ms: 2_000
            }),
            Response::Ok
        ));
        assert_eq!(s.take_distill_request(), Some((1_000, 2_000)));
        assert_eq!(s.take_distill_request(), None, "second take is cleared");
    }

    #[test]
    fn reload_request_is_set_by_handle_and_consumed_once() {
        let s = Shared::new();
        assert!(!s.take_reload_request());

        assert!(matches!(s.handle(Request::ReloadConfig), Response::Ok));
        assert!(s.take_reload_request(), "first take sees the request");
        assert!(!s.take_reload_request(), "second take is already cleared");
    }

    #[test]
    fn status_reports_accumulated_counters() {
        let s = Shared::new();
        s.inc_events(3);
        s.inc_events(2);
        s.inc_snapshots(4);
        match s.handle(Request::GetStatus) {
            Response::Status {
                events_captured,
                snapshots_captured,
                ..
            } => {
                assert_eq!(events_captured, 5);
                assert_eq!(snapshots_captured, 4);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }
}
