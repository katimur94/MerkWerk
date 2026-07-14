//! Aktive-Fenster-Metadaten: Fenstertitel + Prozessname aus einem HWND.
//!
//! Wird sowohl von der Blacklist-Prüfung (Prozessname/Titel-Muster) als auch für
//! `app_sessions.process_name` gebraucht (siehe ARCHITEKTUR.md DB-Schema) und vom
//! UIA-Snapshotter benutzt, um zu erkennen, ob ein Fenster zu einem bekannten
//! Browser gehört (siehe `uia.rs`).

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId};

use crate::WindowInfo;

/// Ermittelt Fenstertitel und Prozessname (z. B. `"chrome.exe"`) für ein Fensterhandle.
///
/// `hwnd` wird plattformneutral als `isize` übergeben (siehe [`crate::RawSignal::FocusChange`]).
///
/// Liefert `None`, wenn das Handle selbst ungültig ist (Null) oder der Fensterbesitz-
/// Prozess nicht mehr ermittelbar ist (`GetWindowThreadProcessId` liefert dann `pid == 0`,
/// typischerweise weil das Fenster zwischenzeitlich geschlossen wurde). In allen anderen
/// Fällen wird best-effort ein `WindowInfo` geliefert: einzelne Teilausfälle (z. B.
/// `OpenProcess` wird für einen geschützten Prozess verweigert) führen nur dazu, dass
/// `process_name` leer bleibt, nicht zu `None` insgesamt — der Titel bleibt dann
/// trotzdem nutzbar.
///
/// Kein COM-Aufruf involviert; reines Win32. Kann daher von jedem Thread aus
/// aufgerufen werden (anders als [`crate::uia::UiaSnapshotter`]).
pub fn window_info(hwnd: isize) -> Option<WindowInfo> {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    if hwnd.0.is_null() {
        return None;
    }

    let title = window_title(hwnd);

    let mut pid: u32 = 0;
    // SAFETY: `hwnd` ist als Wert gültig (Null-Check oben); GetWindowTextW/
    // GetWindowThreadProcessId tolerieren ein zwischenzeitlich zerstörtes/
    // ungültiges Fenster ohne UB und liefern dann einfach 0/leer zurück statt
    // zu crashen — das ist dokumentiertes Win32-Verhalten, kein unsicherer Fall.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
    }
    if pid == 0 {
        // Fenster existiert nicht (mehr) bzw. Prozess nicht ermittelbar -> kein
        // sinnvolles WindowInfo möglich (weder Blacklist-Prüfung noch app_sessions
        // können ohne Prozessnamen etwas anfangen).
        return None;
    }

    let process_name = process_name_for_pid(pid).unwrap_or_default();

    Some(WindowInfo {
        process_name,
        title,
    })
}

/// Liest den Fenstertitel via `GetWindowTextW`. Liefert `""`, wenn das Fenster
/// keinen Titel hat oder der Aufruf fehlschlägt (z. B. Fenster bereits weg) —
/// bewusst kein `Option`, ein leerer Titel ist ein legitimer, häufiger Zustand
/// (viele Werkzeug-/Hintergrundfenster haben keinen Titeltext).
fn window_title(hwnd: HWND) -> String {
    // 1024 UTF-16-Einheiten decken praktisch jeden real vorkommenden Fenstertitel
    // ab, ohne vorher per GetWindowTextLengthW einen zweiten Roundtrip zu machen.
    let mut buf = [0u16; 1024];
    // SAFETY: `buf` ist ein gültiger, exklusiv geliehener Slice mit bekannter
    // Länge; GetWindowTextW schreibt laut Doku höchstens `buf.len()` UTF-16-Einheiten
    // (inkl. abschließendem NUL) und gibt die tatsächlich kopierte Zeichenzahl
    // (ohne NUL) zurück oder 0 bei Fehler/leerem Titel.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

/// Öffnet den Prozess read-only (`PROCESS_QUERY_LIMITED_INFORMATION`) und liest
/// dessen Image-Dateinamen. Schließt das Handle in jedem Fall (Erfolg wie Fehler).
fn process_name_for_pid(pid: u32) -> Option<String> {
    // SAFETY: PROCESS_QUERY_LIMITED_INFORMATION ist ein reiner Metadaten-Lesezugriff
    // (kein Speicherzugriff/Codeausführung im Zielprozess) und für praktisch jeden
    // Prozess ohne erhöhte Rechte erlaubt; `false` = Handle nicht vererbbar.
    let hprocess = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    let name = full_image_file_name(hprocess);

    // SAFETY: `hprocess` stammt aus dem obigen erfolgreichen OpenProcess-Aufruf,
    // wird nirgendwo sonst gehalten/dupliziert und hier genau einmal geschlossen.
    unsafe {
        let _ = CloseHandle(hprocess);
    }

    name
}

/// Liest den vollen Image-Pfad via `QueryFullProcessImageNameW` und extrahiert
/// daraus den Dateinamen (z. B. `"chrome.exe"`).
fn full_image_file_name(hprocess: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size: u32 = buf.len() as u32;
    // SAFETY: `buf` lebt für die Dauer des Aufrufs, `size` beschreibt beim Eintritt
    // seine Kapazität in UTF-16-Einheiten; QueryFullProcessImageNameW schreibt
    // höchstens `size` Einheiten und aktualisiert `size` auf die tatsächlich
    // geschriebene Länge (ohne NUL) bei Erfolg.
    unsafe {
        QueryFullProcessImageNameW(
            hprocess,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .ok()?;
    }
    let full_path = String::from_utf16_lossy(&buf[..size as usize]);
    file_name_from_path(&full_path)
}

/// Extrahiert den Dateinamen (z. B. `"chrome.exe"`) aus einem vollen Windows-Pfad.
///
/// Bewusst als reine String-Logik (statt `std::path::Path`) implementiert: der
/// Pfad kommt von `QueryFullProcessImageNameW` und ist deshalb *immer* ein
/// Windows-Pfad (mit `\`, ggf. `\\?\`-Präfix für lange Pfade), unabhängig davon,
/// wie robust `Path` sich auf dem jeweiligen Host verhält. Nimmt zusätzlich `/`
/// als Trenner mit, schadet nicht und ist strenger als nötig.
fn file_name_from_path(full_path: &str) -> Option<String> {
    let name = full_path.rsplit(['\\', '/']).next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}
