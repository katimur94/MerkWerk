//! Persistenz-Politik — plattformneutral und nativ testbar.
//!
//! Kapselt die Entscheidung „darf das in die DB?" an *einer* Stelle, sodass die
//! Privacy-Invariante „Blacklist wirkt an der Quelle" (ARCHITEKTUR.md) nicht nur
//! im Windows-Erfassungs-Loop lebt, sondern direkt getestet werden kann. Der
//! Loop ([`crate::runtime`]) ruft ausschließlich diese Funktionen, statt die
//! Blacklist-Logik selbst zu buchstabieren — so entspricht der getestete
//! Entscheidungsbaum exakt dem, der in Produktion läuft.

use blacklist::Blacklist;

/// Entscheidung beim Fokuswechsel auf ein Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDecision {
    /// Fenster ist erfassbar — Session/Events/Snapshots dürfen entstehen.
    Capture,
    /// Fenster ist gesperrt (Blacklist oder kein ermittelbarer Prozess) —
    /// es darf **keine** Zeile in der DB entstehen.
    Block,
}

impl FocusDecision {
    pub fn is_block(self) -> bool {
        matches!(self, FocusDecision::Block)
    }
}

/// Entscheidet beim Fokuswechsel, ob ein Fenster erfasst werden darf.
///
/// Gesperrt wird, wenn der Prozessname leer ist (nicht ermittelbar → wir können
/// weder Blacklist prüfen noch sinnvoll zuordnen) oder Prozessname/Titel auf die
/// Blacklist treffen. Die URL ist zum Fokuszeitpunkt noch nicht bekannt und wird
/// erst in [`snapshot_blocked`] geprüft.
pub fn focus_decision(process_name: &str, title: &str, blacklist: &Blacklist) -> FocusDecision {
    if process_name.is_empty() {
        return FocusDecision::Block;
    }
    let title = (!title.is_empty()).then_some(title);
    if blacklist.is_blocked(process_name, title, None) {
        FocusDecision::Block
    } else {
        FocusDecision::Capture
    }
}

/// Prüft nach erstelltem Snapshot erneut gegen die Blacklist — jetzt *mit* der
/// aufgelösten URL/Titel. So greift die URL-Blacklist an der Quelle: ein Snapshot,
/// dessen URL/Titel gesperrt ist, wird verworfen, bevor er persistiert wird.
pub fn snapshot_blocked(
    process_name: &str,
    window_title: Option<&str>,
    url: Option<&str>,
    blacklist: &Blacklist,
) -> bool {
    blacklist.is_blocked(process_name, window_title, url)
}

/// Ein Snapshot ohne Titel, URL und Text trägt nichts bei und wird nicht gespeichert.
pub fn snapshot_is_empty(
    window_title: Option<&str>,
    url: Option<&str>,
    text_content: Option<&str>,
) -> bool {
    window_title.is_none() && url.is_none() && text_content.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bl(proc: &[&str], titles: &[&str], urls: &[&str]) -> Blacklist {
        Blacklist::new(
            proc.iter().map(|s| s.to_string()).collect(),
            titles.iter().map(|s| s.to_string()).collect(),
            urls.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn blacklisted_process_is_blocked_at_focus() {
        // DoD (Etappe 0): Prozess auf Blacklist -> keine Erfassung -> null Zeilen.
        let blacklist = bl(&["keepassxc.exe"], &[], &[]);
        assert_eq!(
            focus_decision("keepassxc.exe", "KeePassXC", &blacklist),
            FocusDecision::Block
        );
    }

    #[test]
    fn allowed_process_is_captured() {
        let blacklist = bl(&["keepassxc.exe"], &[], &[]);
        assert_eq!(
            focus_decision("code.exe", "main.rs — MerkWerk", &blacklist),
            FocusDecision::Capture
        );
    }

    #[test]
    fn empty_process_name_is_blocked() {
        let blacklist = bl(&[], &[], &[]);
        assert!(focus_decision("", "irgendein Titel", &blacklist).is_block());
    }

    #[test]
    fn blacklisted_title_pattern_is_blocked() {
        let blacklist = bl(&[], &["*Passwort*"], &[]);
        assert!(focus_decision("chrome.exe", "Mein Passwort-Manager", &blacklist).is_block());
    }

    #[test]
    fn url_blacklist_blocks_snapshot_even_if_focus_allowed() {
        // Prozess/Titel erlaubt -> Fokus wird erfasst; aber die URL ist gesperrt,
        // also darf der Snapshot (mit sichtbarem Text) NICHT gespeichert werden.
        let blacklist = bl(&[], &[], &["*://*.bank.example/*"]);
        assert_eq!(
            focus_decision("chrome.exe", "Online-Banking", &blacklist),
            FocusDecision::Capture
        );
        assert!(snapshot_blocked(
            "chrome.exe",
            Some("Online-Banking"),
            Some("https://secure.bank.example/konto"),
            &blacklist
        ));
    }

    #[test]
    fn non_blacklisted_snapshot_is_not_blocked() {
        let blacklist = bl(&[], &[], &["*://*.bank.example/*"]);
        assert!(!snapshot_blocked(
            "chrome.exe",
            Some("Rust Docs"),
            Some("https://docs.rs/"),
            &blacklist
        ));
    }

    #[test]
    fn empty_snapshot_is_detected() {
        assert!(snapshot_is_empty(None, None, None));
        assert!(!snapshot_is_empty(Some("Titel"), None, None));
        assert!(!snapshot_is_empty(None, None, Some("Text")));
    }
}
