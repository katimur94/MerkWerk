//! Screenshot-Fallback für UIAutomation-arme Fenster (Spiele, Canvas-Apps, …).
//!
//! Etappe 0 implementiert nur das Trait und eine `NoopCapture`-Implementierung.
//! Die echte Windows.Graphics.Capture-Implementierung kommt in einer späteren Etappe.

use crate::Snapshot;

/// Fallback-Strategie für Fenster, die UIAutomation nicht ausreichend Kontext liefert.
///
/// Wird vom Snapshotter aufgerufen, wenn UIA keinen brauchbaren Text extrahieren konnte
/// (z. B. bei Spielen, Canvas-basierten Apps, proprietären Frameworks).
/// Das Trait ist plattformneutral; implementiert wird es am Windows-Fensterhandle `hwnd`.
pub trait FallbackCapture: Send + Sync {
    /// Versucht, einen Fallback-Snapshot des Fensters zu erfassen.
    ///
    /// # Arguments
    /// * `hwnd` - Fensterhandle (als `isize`, plattformneutral transportiert).
    ///
    /// # Returns
    /// * `Some(Snapshot)` wenn der Fallback ein Resultat liefern konnte.
    /// * `None` wenn der Fallback nicht verfügbar oder nicht anwendbar ist.
    fn capture(&self, hwnd: isize) -> Option<Snapshot>;

    /// Name dieser Fallback-Strategie (für Logs und Debugging).
    fn name(&self) -> &'static str;
}

/// Noop-Implementierung des Screenshot-Fallbacks für Etappe 0.
///
/// Liefert immer `None` zurück — keine echte Erfassung. Dies ist ein Platzhalter,
/// bis die echte Windows.Graphics.Capture-Implementierung verfügbar ist.
#[derive(Debug)]
pub struct NoopCapture;

impl FallbackCapture for NoopCapture {
    fn capture(&self, _hwnd: isize) -> Option<Snapshot> {
        None
    }

    fn name(&self) -> &'static str {
        "noop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_capture_always_returns_none() {
        let noop = NoopCapture;
        assert_eq!(noop.capture(0), None);
        assert_eq!(noop.capture(0x12345678), None);
    }

    #[test]
    fn test_noop_capture_name() {
        let noop = NoopCapture;
        assert_eq!(noop.name(), "noop");
    }
}
