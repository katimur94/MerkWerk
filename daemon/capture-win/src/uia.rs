//! UIAutomation-Snapshotter: sichtbarer Text + Browser-URL für ein Fensterhandle.
//!
//! Setzt zwei Privacy-Invarianten strukturell um (siehe ARCHITEKTUR.md, ENTSCHEIDUNGEN.md D5):
//!
//! 1. **Passwortfelder:** Jedes UIA-Element mit `CurrentIsPassword() == true` wird beim
//!    TreeWalk komplett übersprungen — Element UND kompletter Subtree. Siehe [`is_password`]
//!    und dessen Verwendung in [`UiaSnapshotter::collect_text`] / [`UiaSnapshotter::find_browser_url`]:
//!    Der `continue` dort passiert *bevor* Name/Value gelesen werden und *bevor* Kinder
//!    des Elements überhaupt aufgezählt werden — es gibt keinen Pfad, auf dem Text aus
//!    einem Passwort-Subtree in den Snapshot gelangen kann.
//! 2. **Textbudget:** `Snapshot.text_content` ist über [`SnapshotConfig::max_text_bytes`]
//!    auf 20480 Bytes (Default) gedeckelt; bei Überschreitung wird an einer gültigen
//!    UTF-8-Zeichengrenze abgeschnitten und `truncated = true` gesetzt (siehe [`push_capped`]).
//!    Der Walk bricht ab, sobald das Budget erreicht ist (keine weitere unnötige COM-Arbeit).
//!
//! Zusätzlich begrenzen `max_tree_depth`/`max_nodes` den Walk gegen Latenzspitzen bei
//! riesigen Element-Bäumen (D5).

use windows::core::Result as WinResult;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    IUIAutomationValuePattern, UIA_ComboBoxControlTypeId, UIA_EditControlTypeId,
    UIA_ValuePatternId,
};

use crate::text_budget::{dedup_pair, push_capped};
use crate::{window, Snapshot};

// Die Snapshot-Grenzwerte und die reine Text-Budget-/Dedup-Logik liegen in
// `crate::text_budget` (plattformneutral, nativ getestet — siehe dortige
// Modul-Dokumentation und ENTSCHEIDUNGEN.md D5). Hier nur re-exportiert, damit
// bestehende Aufrufer `uia::SnapshotConfig` weiterhin auflösen können.
pub use crate::text_budget::SnapshotConfig;

/// Bekannte Browser-Prozessnamen, für die eine Adressleisten-URL gesucht wird
/// (siehe RECHERCHE-winapi.md Abschnitt 5). Vergleich erfolgt case-insensitiv.
const BROWSER_PROCESS_NAMES: &[&str] = &["chrome.exe", "msedge.exe", "firefox.exe"];

/// Bounds für die (separate, flache) Adressleisten-Suche in [`UiaSnapshotter::find_browser_url`].
/// Klein gehalten, weil die Adressleiste in der Control-View aller drei Zielbrowser nahe an
/// der Fensterwurzel liegt — die Grenze schützt nur davor, dass eine erfolglose Suche in
/// Seiteninhalt (Webseiten-DOM über UIA) abtaucht und dort teuer wird.
const ADDRESS_BAR_SEARCH_MAX_NODES: u32 = 500;
const ADDRESS_BAR_SEARCH_MAX_DEPTH: u32 = 8;

/// Hält die COM-/UIAutomation-Session für einen dedizierten Thread.
///
/// **Threading-Vertrag:** `CoInitializeEx(..., COINIT_MULTITHREADED)` bindet den
/// COM-Apartment-Zustand an den *aufrufenden OS-Thread*. [`UiaSnapshotter::new`] muss
/// deshalb auf dem Thread laufen, der die Instanz für ihre gesamte Lebenszeit benutzt
/// (siehe ARCHITEKTUR.md: dedizierter „UIA-Thread (MTA)" — nicht der Hook-Thread, der
/// als STA mit eigener Message-Loop läuft). `Drop` ruft `CoUninitialize()` auf demselben
/// Thread auf, der `new()` aufgerufen hat — das ist nur korrekt, wenn die Instanz nicht
/// über Thread-Grenzen wandert. Praktischerweise erzwingt das der Rust-Compiler bereits
/// strukturell: `IUIAutomation` kapselt einen rohen COM-Zeiger (`NonNull<c_void>` in
/// `IUnknown`) und implementiert daher automatisch **nicht** `Send` — `UiaSnapshotter`
/// kann also gar nicht versehentlich an einen anderen Thread verschickt werden; ein
/// `move`-Versuch über eine Thread-Grenze ist bereits ein Compile-Fehler.
pub struct UiaSnapshotter {
    automation: IUIAutomation,
}

impl UiaSnapshotter {
    /// Initialisiert COM (Multi-Threaded Apartment) auf dem aufrufenden Thread und
    /// erstellt die `IUIAutomation`-Instanz. Muss auf dem dedizierten UIA-Thread
    /// aufgerufen werden (siehe Typ-Dokumentation oben).
    pub fn new() -> WinResult<Self> {
        // SAFETY: wird laut Vertrag genau einmal pro dediziertem UIA-Thread aufgerufen.
        // CoInitializeEx ist für denselben Apartment-Typ auf demselben Thread idempotent
        // (liefert dann S_FALSE statt S_OK zurück — `.ok()` behandelt beides als Erfolg,
        // wie von der COM-API vorgesehen).
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        // SAFETY: `CLSCTX_ALL` + die (korrekten, aus dem windows-crate stammenden)
        // CLSID/IID von `CUIAutomation`/`IUIAutomation`; Fehler (z. B. UIA-Dienst nicht
        // verfügbar) werden als `Err` durchgereicht, kein Absturz.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)? };
        Ok(Self { automation })
    }

    /// Erstellt einen Kontext-Snapshot für `hwnd` gemäß den Grenzwerten in `cfg`.
    ///
    /// `hwnd` wird plattformneutral als `isize` übergeben (siehe [`crate::RawSignal::FocusChange`]).
    /// Jedes einzelne COM-/Win32-Ergebnis wird über `.ok()`/`if let`/`let-else` behandelt —
    /// kein `unwrap`/`expect` auf einem COM-Ergebnis, kein Panic-Pfad. Ist ein Wert nicht
    /// ermittelbar (z. B. Fenster bereits geschlossen, Element ohne Value-Pattern), bleibt
    /// das entsprechende Snapshot-Feld schlicht `None`.
    pub fn snapshot(&self, hwnd: isize, cfg: SnapshotConfig) -> Snapshot {
        let win_handle = HWND(hwnd as *mut core::ffi::c_void);

        // Fenstertitel + Prozessname kommen bewusst aus window.rs (GetWindowTextW /
        // QueryFullProcessImageNameW) statt aus der UIA-CurrentName() des Root-Elements:
        // GetWindowTextW ist exakt die API, die auch Taskleiste/Alt-Tab für den Titel
        // benutzen, während UIA-Namen von Root-Elementen je nach App-Framework abweichen
        // oder leer sein können. Liefert außerdem den Prozessnamen "gratis" mit, den wir
        // für die Browser-Erkennung unten ohnehin brauchen — kein zweiter COM-Umweg nötig.
        let info = window::window_info(hwnd);
        let window_title = info
            .as_ref()
            .map(|i| i.title.clone())
            .filter(|t| !t.is_empty());
        let is_browser = info
            .as_ref()
            .is_some_and(|i| is_known_browser(&i.process_name));

        let root: Option<IUIAutomationElement> =
            unsafe { self.automation.ElementFromHandle(win_handle) }.ok();

        let Some(root) = root else {
            // Root-Element nicht ermittelbar (z. B. Fenster inzwischen geschlossen) ->
            // teilweiser Snapshot statt Absturz; Titel/Prozessinfo bleiben nutzbar.
            return Snapshot {
                window_title,
                url: None,
                text_content: None,
                truncated: false,
            };
        };

        let url = if is_browser {
            self.find_browser_url(&root)
        } else {
            None
        };
        let (text_content, truncated) = self.collect_text(&root, &cfg);

        Snapshot {
            window_title,
            url,
            text_content,
            truncated,
        }
    }

    /// Läuft den UIA-Baum ab `root` ab und sammelt sichtbaren Text (Name + Value) bis
    /// zum Byte-/Tiefen-/Knotenlimit aus `cfg`. Passwort-Subtrees werden komplett
    /// übersprungen (siehe Modul-Dokumentation, Punkt 1).
    ///
    /// **Walker-Wahl — `RawViewWalker` statt `ControlViewWalker`:** Der Control-View
    /// filtert auf `IsControlElement == true` und lässt damit reine Content-/Text-Elemente
    /// aus, die insbesondere in Chromium-/Electron-Oberflächen (und generell vielen
    /// Custom-Rendering-Frameworks) den eigentlichen sichtbaren Text tragen, ohne selbst
    /// als „Control" zu gelten. Für das Ziel „möglichst vollständiger sichtbarer Text"
    /// ist der Raw-View deshalb die robustere Wahl. Das dadurch größere, „lautere"
    /// Baumvolumen (mehr rein strukturelle/dekorative Knoten) wird durch die
    /// Tiefen-/Knoten-/Byte-Limits (D5) sicher begrenzt — die Vollständigkeit kostet also
    /// nur begrenzt zusätzliche, budgetierte Arbeit, nie unbeschränkte Latenz.
    /// Für die gezielte, flache Adressleisten-Suche in [`Self::find_browser_url`] wird
    /// bewusst der `ControlViewWalker` verwendet: dort ist die Filterung auf echte
    /// Controls erwünscht (die Adressleiste ist immer ein Control) und hält die Suche
    /// kompakt.
    fn collect_text(
        &self,
        root: &IUIAutomationElement,
        cfg: &SnapshotConfig,
    ) -> (Option<String>, bool) {
        // SAFETY: reiner COM-Aufruf ohne Ausgabeparameter außer dem Rückgabewert.
        let Ok(walker) = (unsafe { self.automation.RawViewWalker() }) else {
            return (None, false);
        };

        let mut buf = String::new();
        let mut last_pushed: Option<String> = None;
        let mut node_count: u32 = 0;
        let mut truncated = false;
        // Explizite Stack-basierte Tiefensuche statt Rekursion: vermeidet jedes
        // Stack-Overflow-Risiko bei pathologisch tiefen Bäumen, selbst wenn
        // `max_tree_depth` großzügig/falsch konfiguriert wäre.
        let mut stack: Vec<(IUIAutomationElement, u32)> = vec![(root.clone(), 0)];

        while !truncated {
            let Some((elem, depth)) = stack.pop() else {
                break;
            };

            if node_count >= cfg.max_nodes {
                truncated = true;
                break;
            }
            node_count += 1;

            // --- Privacy-Invariante 1: Passwort-Element -> Element UND kompletter
            // Subtree werden übersprungen. `continue` hier passiert VOR jedem Lesen
            // von Name/Value dieses Elements und VOR dem Aufzählen seiner Kinder —
            // es gibt also keinen Weg, wie Text aus einem Passwort-Subtree in `buf`
            // landen könnte.
            if is_password(&elem) {
                continue;
            }

            for candidate in dedup_pair(current_name(&elem), current_value(&elem)) {
                if last_pushed.as_deref() == Some(candidate.as_str()) {
                    continue; // unmittelbare Wiederholung (z. B. Name == Value des Vorgängers)
                }
                let sep_truncated =
                    !buf.is_empty() && push_capped(&mut buf, "\n", cfg.max_text_bytes);
                let content_truncated =
                    !sep_truncated && push_capped(&mut buf, &candidate, cfg.max_text_bytes);
                last_pushed = Some(candidate);
                if sep_truncated || content_truncated {
                    // --- Privacy-Invariante 2: Budget erreicht -> Walk stoppt sofort,
                    // kein weiterer COM-Traffic für Text, der ohnehin verworfen würde.
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }

            if depth < cfg.max_tree_depth {
                push_children(&walker, &elem, depth, &mut stack);
            }
        }

        (if buf.is_empty() { None } else { Some(buf) }, truncated)
    }

    /// Sucht die Adressleiste unter den (Control-View-)Nachfahren von `root` — bounded
    /// durch [`ADDRESS_BAR_SEARCH_MAX_NODES`]/[`ADDRESS_BAR_SEARCH_MAX_DEPTH`] — und
    /// liefert deren aktuellen Wert als URL. Namens-Heuristik nach RECHERCHE-winapi.md
    /// Abschnitt 5 (lokalisierte Chrome/Edge/Firefox-Muster), siehe [`looks_like_address_bar`].
    fn find_browser_url(&self, root: &IUIAutomationElement) -> Option<String> {
        // SAFETY: reiner COM-Aufruf ohne Ausgabeparameter außer dem Rückgabewert.
        let walker: IUIAutomationTreeWalker =
            unsafe { self.automation.ControlViewWalker() }.ok()?;

        let mut stack: Vec<(IUIAutomationElement, u32)> = vec![(root.clone(), 0)];
        let mut visited = 0u32;

        while let Some((elem, depth)) = stack.pop() {
            if visited >= ADDRESS_BAR_SEARCH_MAX_NODES {
                break;
            }
            visited += 1;

            // Dieselbe Privacy-Invariante gilt hier genauso, falls je ein als Passwort
            // markiertes Element in diesem (an sich untypischen) Teilbaum auftaucht.
            if is_password(&elem) {
                continue;
            }

            if looks_like_address_bar(&elem) {
                if let Some(url) = current_value(&elem) {
                    return Some(url);
                }
            }

            if depth < ADDRESS_BAR_SEARCH_MAX_DEPTH {
                push_children(&walker, &elem, depth, &mut stack);
            }
        }

        None
    }
}

impl Drop for UiaSnapshotter {
    fn drop(&mut self) {
        // SAFETY: Muss auf demselben Thread laufen, der `new()` aufgerufen hat (siehe
        // Typ-Dokumentation) — `UiaSnapshotter` ist `!Send`, kann also gar nicht auf
        // einem anderen Thread gedroppt werden. `CoUninitialize` ist das vorgeschriebene
        // Gegenstück zu `CoInitializeEx` und muss unabhängig vom Erfolg von `new()`
        // NICHT aufgerufen werden, wenn `new()` fehlschlug — hier ist es aber immer
        // korrekt, weil `Self` nur bei erfolgreichem `CoInitializeEx` konstruiert wird.
        unsafe { CoUninitialize() };
    }
}

/// Zählt `elem` (auf einem gemeinsamen `IUIAutomationTreeWalker`) zu den Kindern von
/// `elem` und legt sie in `depth + 1` auf `stack` — in umgekehrter Reihenfolge, damit
/// die Tiefensuche das erste Kind zuerst besucht (Lesereihenfolge folgt damit grob der
/// visuellen/Dokumentreihenfolge). Fehler bei der Enumeration (kein erstes Kind, COM-
/// Fehler) werden als „keine Kinder" behandelt statt zu propagieren.
fn push_children(
    walker: &IUIAutomationTreeWalker,
    elem: &IUIAutomationElement,
    depth: u32,
    stack: &mut Vec<(IUIAutomationElement, u32)>,
) {
    // SAFETY: `elem` ist ein gültiges, lebendiges Element (stammt aus einem vorherigen
    // erfolgreichen ElementFromHandle/GetFirstChildElement/GetNextSiblingElement-Aufruf).
    let Ok(mut child) = (unsafe { walker.GetFirstChildElement(elem) }) else {
        return;
    };
    let mut children = Vec::new();
    loop {
        // SAFETY: `child` ist zu diesem Zeitpunkt immer ein gültiges Element (siehe oben
        // bzw. vorherige Schleifeniteration).
        let next = unsafe { walker.GetNextSiblingElement(&child) };
        children.push(child);
        match next {
            Ok(sibling) => child = sibling,
            Err(_) => break,
        }
    }
    for c in children.into_iter().rev() {
        stack.push((c, depth + 1));
    }
}

/// Case-insensitiver Abgleich gegen [`BROWSER_PROCESS_NAMES`].
fn is_known_browser(process_name: &str) -> bool {
    let lower = process_name.to_ascii_lowercase();
    BROWSER_PROCESS_NAMES.iter().any(|&b| b == lower)
}

/// Privacy-Invariante: liefert `true` nur bei explizitem `CurrentIsPassword() == Ok(TRUE)`.
/// Ein COM-Fehler beim Ermitteln der Eigenschaft wird bewusst NICHT als „ist Passwort"
/// gewertet (`unwrap_or(false)`) — sonst könnten vereinzelte COM-Fehler flächendeckend
/// Text verschlucken. Das ist kein Widerspruch zur Privacy-Invariante: diese verlangt,
/// dass *als Passwort erkannte* Felder übersprungen werden, nicht dass bei Unsicherheit
/// pauschal alles verworfen wird.
fn is_password(elem: &IUIAutomationElement) -> bool {
    // SAFETY: `elem` ist ein gültiges Element aus dem laufenden Walk.
    unsafe { elem.CurrentIsPassword() }
        .map(|b| b.as_bool())
        .unwrap_or(false)
}

/// `CurrentName()`, getrimmt; `None` bei COM-Fehler oder wenn nach dem Trimmen nichts
/// übrig bleibt (vermeidet leere Strings im gesammelten Text).
fn current_name(elem: &IUIAutomationElement) -> Option<String> {
    // SAFETY: `elem` ist ein gültiges Element aus dem laufenden Walk.
    let bstr = unsafe { elem.CurrentName() }.ok()?;
    trimmed_or_none(&bstr.to_string())
}

/// Liest den Wert über `IUIAutomationValuePattern` (nicht über `IUIAutomationElement`
/// direkt — die Value-Eigenschaft ist dort kein Property, sondern nur über das
/// Value-Pattern erreichbar). `None`, wenn das Element kein Value-Pattern unterstützt
/// (z. B. reine Labels/Container) oder der Wert nach dem Trimmen leer ist.
fn current_value(elem: &IUIAutomationElement) -> Option<String> {
    // SAFETY: `elem` ist ein gültiges Element aus dem laufenden Walk; `GetCurrentPatternAs`
    // liefert bei nicht unterstütztem Pattern einen COM-Fehler (kein UB), den `.ok()?` abfängt.
    let pattern =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
            .ok()?;
    // SAFETY: `pattern` stammt aus dem soeben erfolgreichen GetCurrentPatternAs-Aufruf.
    let bstr = unsafe { pattern.CurrentValue() }.ok()?;
    trimmed_or_none(&bstr.to_string())
}

fn trimmed_or_none(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Namens-/Typ-Heuristik für die Adressleiste (Chrome/Edge/Firefox), siehe
/// RECHERCHE-winapi.md Abschnitt 5. `ControlType` muss `Edit` oder `ComboBox` sein
/// (Firefox exponiert die kombinierte Adress-/Suchleiste je nach Version als eines
/// von beiden); zusätzlich muss der (lokalisierte) Name auf ein bekanntes Muster passen.
fn looks_like_address_bar(elem: &IUIAutomationElement) -> bool {
    // SAFETY: `elem` ist ein gültiges Element aus der laufenden Suche.
    let ctrl_type_ok = unsafe { elem.CurrentControlType() }
        .map(|ct| ct == UIA_EditControlTypeId || ct == UIA_ComboBoxControlTypeId)
        .unwrap_or(false);
    if !ctrl_type_ok {
        return false;
    }
    // SAFETY: `elem` ist ein gültiges Element aus der laufenden Suche.
    let Some(name) = (unsafe { elem.CurrentName() }).ok().map(|b| b.to_string()) else {
        return false;
    };
    let lower = name.to_lowercase();
    // "address" (EN/ES/FR-Fragmente über "adress*") und "adress" (DE "Adress[e/leiste]")
    // decken beide Schreibweisen ab; Firefox' kombinierte Such-/Adressleiste zusätzlich
    // über "search" + "google".
    lower.contains("address")
        || lower.contains("adress")
        || (lower.contains("search") && lower.contains("google"))
}
