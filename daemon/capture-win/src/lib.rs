//! Windows-Erfassungsschicht für MerkWerk.
//!
//! Diese Crate kapselt alles Betriebssystemnahe: die WinEvent-/Low-Level-Hooks,
//! den UIAutomation-Snapshotter und (als Gerüst) den Screenshot-Fallback. Der
//! Windows-spezifische Code lebt hinter `#[cfg(windows)]`; die geteilten
//! Datentypen unten sind plattformneutral, damit die Debouncer-Logik nativ
//! (auch in der Linux-Sandbox) getestet werden kann.
//!
//! ## Privacy-Invariante D3 — strukturell erzwungen
//!
//! Der einzige Typ, der den Hook-Thread verlässt, ist [`RawSignal`]. Er hat
//! **kein Feld für Tasteninhalte** — kein `vk_code`, kein `scan_code`, nichts.
//! Ein Tastendruck wird im Hook-Callback sofort auf `RawSignal::KeyTick { ts_ms }`
//! reduziert; der Keycode wird nie gelesen. „Kein Roh-Tastenanschlag" ist damit
//! nicht Disziplin des aufrufenden Codes, sondern eine Eigenschaft des Typs:
//! es gibt schlicht keinen Ort, an dem ein Keycode gespeichert werden könnte.

/// Rohsignal aus den OS-Hooks. Bewusst minimal.
///
/// **D3:** `KeyTick` trägt ausschließlich einen Zeitstempel. Niemals ein
/// zusätzliches Feld für `vk_code`/`scan_code`/Zeichen hinzufügen — das würde
/// die Privacy-Invariante brechen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawSignal {
    /// Der Vordergrund-/Fokus hat auf ein neues Fenster gewechselt.
    FocusChange {
        /// Fensterhandle als `isize` (plattformneutral transportiert).
        hwnd: isize,
        ts_ms: u64,
    },
    /// Eine Taste wurde gedrückt. NUR der Zeitpunkt — kein Tasteninhalt (D3).
    KeyTick { ts_ms: u64 },
    /// Ein Mausklick fand statt.
    MouseClick { ts_ms: u64 },
    /// Ein Scroll-Ereignis (Mausrad) fand statt.
    Scroll { ts_ms: u64 },
}

impl RawSignal {
    /// Zeitstempel des Signals in Unix-Millisekunden.
    pub fn ts_ms(&self) -> u64 {
        match *self {
            RawSignal::FocusChange { ts_ms, .. }
            | RawSignal::KeyTick { ts_ms }
            | RawSignal::MouseClick { ts_ms }
            | RawSignal::Scroll { ts_ms } => ts_ms,
        }
    }
}

/// Ergebnis des Debouncers: ein „relevantes Ereignis", bei dem ein
/// Kontext-Snapshot sinnvoll ist. Wird an die Snapshot-/Writer-Pipeline gereicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Fensterwechsel — sofort ein Snapshot des neuen Fensters.
    FocusChange { hwnd: isize, ts_ms: u64 },
    /// Tipp-Pause: nach einem Tipp-Burst >2 s Ruhe. Snapshot lohnt sich.
    TypingSettled {
        hwnd: isize,
        ts_ms: u64,
        /// Anzahl Tastenanschläge im vorangegangenen Burst (nur Zähler, D3).
        key_count: u32,
        /// Dauer des Bursts in Millisekunden.
        duration_ms: u64,
    },
    /// Klick-Cluster abgeschlossen.
    ClickCluster {
        hwnd: isize,
        ts_ms: u64,
        click_count: u32,
    },
    /// Scrollen beendet.
    ScrollEnd { hwnd: isize, ts_ms: u64 },
}

impl Trigger {
    /// Fensterhandle, auf das sich der Trigger bezieht.
    pub fn hwnd(&self) -> isize {
        match *self {
            Trigger::FocusChange { hwnd, .. }
            | Trigger::TypingSettled { hwnd, .. }
            | Trigger::ClickCluster { hwnd, .. }
            | Trigger::ScrollEnd { hwnd, .. } => hwnd,
        }
    }

    /// Zeitstempel des Triggers in Unix-Millisekunden.
    pub fn ts_ms(&self) -> u64 {
        match *self {
            Trigger::FocusChange { ts_ms, .. }
            | Trigger::TypingSettled { ts_ms, .. }
            | Trigger::ClickCluster { ts_ms, .. }
            | Trigger::ScrollEnd { ts_ms, .. } => ts_ms,
        }
    }
}

/// Kontext-Snapshot eines Fensters, erzeugt vom UIAutomation-Snapshotter.
/// Plattformneutraler Datentyp (die Erzeugung ist Windows-spezifisch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Fenstertitel (falls ermittelbar).
    pub window_title: Option<String>,
    /// URL aus der Adressleiste — nur bei Browsern.
    pub url: Option<String>,
    /// Sichtbarer Text der UIA-Elemente, auf das Budget gedeckelt.
    pub text_content: Option<String>,
    /// True, wenn `text_content` das Byte-Budget überschritt und abgeschnitten wurde.
    pub truncated: bool,
}

/// Aktives-Fenster-Metadaten (Prozessname + Titel), ermittelt für Blacklist-
/// Prüfung und `app_sessions`. Plattformneutraler Datentyp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowInfo {
    /// Prozessname, z. B. "chrome.exe".
    pub process_name: String,
    /// Fenstertitel.
    pub title: String,
}

// Debouncer: reine Zustandsmaschine über RawSignal -> Trigger. Plattformneutral,
// nativ testbar. (T6 implementiert das Modul.)
pub mod debounce;

// Text-Budget-/Dedup-Logik für den UIA-Snapshotter. Plattformneutral, nativ
// testbar; von uia.rs (Windows-only) genutzt. (Etappe-0-Integration.)
pub mod text_budget;

// Screenshot-Fallback: nur Trait + Noop in Etappe 0. Plattformneutral. (T7b.)
pub mod capture;

// Windows-spezifische Erfassung: Hooks + UIA. Nur auf Windows kompiliert.
#[cfg(windows)]
pub mod hooks;
#[cfg(windows)]
pub mod uia;
#[cfg(windows)]
pub mod window;
