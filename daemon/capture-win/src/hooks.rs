//! Windows-Hooks: Fokuswechsel (`SetWinEventHook`) und Low-Level-Eingabe-Hooks
//! (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`). Läuft komplett auf einem dedizierten
//! Hook-Thread mit eigener Message-Loop (Low-Level-Hooks liefern ihre
//! Callbacks nur, solange der installierende Thread Nachrichten pumpt).
//!
//! ## D3 — strukturell erzwungen, hier konkret
//!
//! Der Keyboard-Hook-Callback ([`keyboard_hook_proc`]) fasst `lparam`
//! (ein Zeiger auf `KBDLLHOOKSTRUCT` mit `vkCode`/`scanCode`) **nicht an** —
//! er wird nicht gecastet, nicht dereferenziert, `KBDLLHOOKSTRUCT` wird in
//! dieser Datei nicht einmal importiert. Der Callback erzeugt bei
//! `WM_KEYDOWN` ausschließlich `RawSignal::KeyTick { ts_ms: now_ms() }`.
//! `KEY_UP` (`WM_KEYUP`) wird ignoriert. Der Typ `RawSignal` hat ohnehin
//! kein Feld für Tasteninhalte (siehe `lib.rs`) — hier ist zusätzlich
//! dokumentiert, dass der Callback erst gar nicht in die Nähe eines
//! Keycodes kommt.
//!
//! ## Warum `thread_local!` statt `static OnceLock<Sender<_>>`
//!
//! `WINEVENTPROC` und `HOOKPROC` sind rohe C-Funktionszeiger ohne
//! `void* userdata`-Parameter — der Sender muss also außerhalb der
//! Funktionssignatur erreichbar sein. Alle drei Hooks werden auf demselben
//! dedizierten Hook-Thread installiert und ihre Callbacks werden
//! ausschließlich synchron aus dessen eigener Message-Loop heraus
//! aufgerufen (so funktionieren Low-Level-Hooks unter Windows) — es gibt
//! also nie einen Zugriff von einem anderen Thread aus. Ein `thread_local!`
//! ist damit ausreichend und einem global synchronisierten `OnceLock`
//! (oder `Mutex`) vorzuziehen: kein Lock auf dem Eingabe-Pfad, keine
//! Cross-Thread-Race, und der Zustand verschwindet automatisch mit dem
//! Hook-Thread.
//!
//! ## COM wird hier bewusst nicht initialisiert
//!
//! Dieser Thread ruft keine COM-/UIAutomation-APIs auf (das übernimmt der
//! separate UIA-Thread, siehe `uia.rs` und `ARCHITEKTUR.md`). Ein
//! `CoInitializeEx` wäre hier totes Gerüst ohne Gegenstück — deshalb fehlt
//! es absichtlich.

use std::cell::RefCell;
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetMessageW, HC_ACTION, HHOOK,
    HOOKPROC, MSG, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WINDOWS_HOOK_ID, WINEVENT_OUTOFCONTEXT, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN,
};

use crate::RawSignal;

thread_local! {
    /// Siehe Modul-Doc: nur vom Hook-Thread selbst gelesen/geschrieben.
    static SIGNAL_TX: RefCell<Option<Sender<RawSignal>>> = const { RefCell::new(None) };
}

/// Sendet ein Signal über den im TLS hinterlegten Channel — nicht-blockierend.
///
/// `try_send` statt `send`: Low-Level-Hook-Callbacks müssen in Millisekunden
/// zurückkehren, sonst wirft Windows den Hook nach einem Timeout aus dem
/// System-weiten Eingabepfad (spürbar für ALLE Anwendungen, nicht nur
/// MerkWerk). Ein voller oder verwaister Channel darf das niemals blockieren;
/// im Zweifel wird das Signal verworfen statt den Eingabe-Pfad zu stallen.
fn send_signal(sig: RawSignal) {
    SIGNAL_TX.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            let _ = tx.try_send(sig);
        }
    });
}

/// Aktuelle Unix-Zeit in Millisekunden für Signal-Zeitstempel. Wall-Clock
/// (nicht monoton) — für die relativen Deltas, die der Debouncer bildet,
/// ausreichend; `unwrap_or(0)` hält den Callback panic-frei, falls die
/// Systemuhr je vor 1970 stünde.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `EVENT_SYSTEM_FOREGROUND`-Callback: meldet Fensterwechsel.
///
/// Zusätzlich zur registrierten Event-Range (min==max==
/// `EVENT_SYSTEM_FOREGROUND`, wodurch die OS uns ohnehin nur dieses Event
/// liefert) filtern wir defensiv auf `idObject == OBJID_WINDOW` (0) und
/// `idChild == CHILDID_SELF` (0), wie von Microsoft für `WinEventProc`-
/// Implementierungen empfohlen, sowie auf ein nicht-null `hwnd`.
unsafe extern "system" fn win_event_proc(
    _hwineventhook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    idchild: i32,
    _ideventthread: u32,
    _dwmseventtime: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND && idobject == 0 && idchild == 0 && !hwnd.is_invalid() {
        send_signal(RawSignal::FocusChange {
            hwnd: hwnd.0 as isize,
            ts_ms: now_ms(),
        });
    }
}

/// `WH_KEYBOARD_LL`-Callback.
///
/// **D3:** `lparam` zeigt auf `KBDLLHOOKSTRUCT` (`vkCode`, `scanCode`, ...).
/// Diese Zeile hier ist absichtlich die einzige, die `lparam` erwähnt — es
/// wird an `CallNextHookEx` durchgereicht, nie zu `*const KBDLLHOOKSTRUCT`
/// gecastet oder dereferenziert. Nur `WM_KEYDOWN` erzeugt ein Signal, und
/// zwar ausschließlich `RawSignal::KeyTick { ts_ms }` — kein Keycode, kein
/// Zeichen. `WM_KEYUP` wird ignoriert.
unsafe extern "system" fn keyboard_hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 && wparam.0 as u32 == WM_KEYDOWN {
        send_signal(RawSignal::KeyTick { ts_ms: now_ms() });
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// `WH_MOUSE_LL`-Callback.
///
/// `WM_LBUTTONDOWN`/`WM_RBUTTONDOWN` → `RawSignal::MouseClick`,
/// `WM_MOUSEWHEEL` → `RawSignal::Scroll`. Mausbewegung (`WM_MOUSEMOVE`) und
/// alle anderen Botschaften werden ignoriert. Wie beim Keyboard-Hook wird
/// `lparam` (Zeiger auf `MSLLHOOKSTRUCT`, u. a. Cursor-Position) nie
/// dereferenziert — nur `wparam` (die Botschafts-ID) wird ausgewertet.
unsafe extern "system" fn mouse_hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 {
        match wparam.0 as u32 {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                send_signal(RawSignal::MouseClick { ts_ms: now_ms() });
            }
            WM_MOUSEWHEEL => {
                send_signal(RawSignal::Scroll { ts_ms: now_ms() });
            }
            _ => {}
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}

/// Installiert einen Low-Level-Hook (`WH_KEYBOARD_LL`/`WH_MOUSE_LL`) und
/// loggt (statt zu paniken), falls die Installation fehlschlägt — ein
/// einzelner fehlgeschlagener Hook soll die anderen nicht mitreißen.
fn install_ll_hook(id: WINDOWS_HOOK_ID, proc: HOOKPROC, label: &str) -> Option<HHOOK> {
    match unsafe { SetWindowsHookExW(id, proc, None, 0) } {
        Ok(hook) => Some(hook),
        Err(e) => {
            eprintln!("merkwerk-capture: {label}-Hook konnte nicht installiert werden: {e}");
            None
        }
    }
}

/// Läuft vollständig auf dem dedizierten Hook-Thread:
/// 1. Sender im TLS ablegen.
/// 2. Alle drei Hooks installieren (Fehler einzeln loggen, nicht fatal).
/// 3. Eigene Thread-ID an [`start_hooks`] zurückmelden (Rendezvous).
/// 4. Message-Loop fahren, bis `WM_QUIT` (von [`HookHandle::drop`] gepostet)
///    oder ein `GetMessageW`-Fehler kommt.
/// 5. Alle erfolgreich installierten Hooks wieder entfernen, TLS aufräumen.
fn run_hook_thread(tx: Sender<RawSignal>, ready_tx: Sender<u32>) {
    SIGNAL_TX.with(|cell| *cell.borrow_mut() = Some(tx));

    // SAFETY: Hook-Installation und Message-Loop laufen auf demselben
    // Thread (dieser Funktion) — das ist die von Windows geforderte
    // Voraussetzung dafür, dass WH_KEYBOARD_LL/WH_MOUSE_LL/WINEVENT_OUTOFCONTEXT
    // Callbacks überhaupt zugestellt bekommen.
    let win_event_hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if win_event_hook.is_invalid() {
        eprintln!("merkwerk-capture: SetWinEventHook (Fokuswechsel) konnte nicht installiert werden");
    }

    let keyboard_hook = install_ll_hook(WH_KEYBOARD_LL, Some(keyboard_hook_proc), "WH_KEYBOARD_LL");
    let mouse_hook = install_ll_hook(WH_MOUSE_LL, Some(mouse_hook_proc), "WH_MOUSE_LL");

    // Rendezvous mit start_hooks: Thread-ID erst melden, wenn alle
    // Hook-Installationsversuche (erfolgreich oder nicht) abgeschlossen
    // sind. Wenn der Empfänger schon weg ist, gibt es ohnehin nichts mehr
    // zu tun als die Message-Loop laufen zu lassen, bis WM_QUIT kommt.
    let _ = ready_tx.send(unsafe { GetCurrentThreadId() });
    drop(ready_tx);

    let mut msg = MSG::default();
    loop {
        // GetMessageW: 0 == WM_QUIT, -1 == Fehler, sonst > 0.
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        match ret.0 {
            0 => break,
            -1 => {
                eprintln!("merkwerk-capture: GetMessageW-Fehler, Hook-Thread beendet sich");
                break;
            }
            _ => unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            },
        }
    }

    if !win_event_hook.is_invalid() {
        let _ = unsafe { UnhookWinEvent(win_event_hook) };
    }
    if let Some(hook) = keyboard_hook {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
    }
    if let Some(hook) = mouse_hook {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
    }

    SIGNAL_TX.with(|cell| *cell.borrow_mut() = None);
}

/// Startet den Hook-Thread und liefert ein [`HookHandle`], über das er
/// wieder sauber gestoppt werden kann.
///
/// Blockiert kurz (Rendezvous über einen internen Channel), bis der
/// Hook-Thread seine Thread-ID gemeldet hat — danach sind alle drei Hooks
/// installiert (oder ihr Fehlschlagen wurde geloggt) und `HookHandle` ist
/// sofort funktionsfähig (`PostThreadMessageW` in `Drop` kann die korrekte
/// Thread-ID adressieren).
///
/// # Panics
/// Falls das Betriebssystem keinen neuen Thread mehr erzeugen kann, oder
/// falls der Hook-Thread sich beendet, bevor er sich bereit gemeldet hat
/// (sollte praktisch nie vorkommen — `run_hook_thread` meldet sich vor
/// jeder Möglichkeit eines frühen Returns).
pub fn start_hooks(tx: Sender<RawSignal>) -> HookHandle {
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<u32>(1);

    let join_handle = std::thread::Builder::new()
        .name("merkwerk-hooks".to_string())
        .spawn(move || run_hook_thread(tx, ready_tx))
        .expect("merkwerk-capture: Hook-Thread konnte nicht gestartet werden");

    let thread_id = ready_rx
        .recv()
        .expect("merkwerk-capture: Hook-Thread hat sich vor der Bereitschaftsmeldung beendet");

    HookHandle {
        thread_id,
        join_handle: Some(join_handle),
    }
}

/// Handle auf den laufenden Hook-Thread. Beim `Drop` wird der Thread sauber
/// beendet: `WM_QUIT` an seine Message-Loop posten (die Hooks entfernt der
/// Thread anschließend selbst, siehe [`run_hook_thread`]), dann auf sein
/// Ende warten (`join`), damit nach Ablauf von `drop` garantiert keine
/// Hook-Callbacks mehr laufen und keine Signale mehr über den Channel
/// kommen können.
pub struct HookHandle {
    thread_id: u32,
    join_handle: Option<JoinHandle<()>>,
}

impl HookHandle {
    /// Windows-Thread-ID des Hook-Threads (z. B. für Diagnose/Logging).
    pub fn thread_id(&self) -> u32 {
        self.thread_id
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        // WPARAM/LPARAM bei WM_QUIT ohne Bedeutung für uns; 0/0 ist üblich.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(handle) = self.join_handle.take() {
            // Best effort: ein Join-Fehler (Thread-Panic) lässt sich hier
            // nicht mehr sinnvoll propagieren.
            let _ = handle.join();
        }
    }
}
