# Windows API Recherche für MerkWerk-Capture-Daemon
## windows-rs 0.58+ API-Signaturen und Feature-Flags

**Zielversion:** windows 0.58.0 oder neuer  
**Recherche-Datum:** 2026-07-14

---

## Feature Flags (Übersicht)

Folgende Feature-Flags sind für die hier dokumentierten APIs erforderlich. Sie werden in `Cargo.toml` unter `[dependencies]` eingetragen:

```toml
[dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",                      # HWND, HANDLE, BOOL
    "Win32_Graphics_Gdi",                    # GDI types (abhängig von WindowsAndMessaging)
    "Win32_System_Com",                      # CoInitializeEx, COM basics
    "Win32_System_Memory",                   # Memory management
    "Win32_System_Threading",                # OpenProcess, GetWindowThreadProcessId
    "Win32_UI_Accessibility",                # UIAutomation COM interfaces
    "Win32_UI_WindowsAndMessaging",          # SetWinEventHook, GetMessage, Hooks
] }
```

**Hinweis:** Die komplette Feature-Liste unter: https://microsoft.github.io/windows-rs/features/#/0.58.0

---

## 1. SetWinEventHook / UnhookWinEvent — Event-Hooks

### Modul-Pfad
```rust
windows::Win32::UI::WindowsAndMessaging
```

### Konstanten
| Konstante | Wert | Pfad | Bedeutung |
|---|---|---|---|
| `EVENT_SYSTEM_FOREGROUND` | `3u32` | `windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_FOREGROUND` | Foreground-Fenster hat sich geändert |
| `EVENT_OBJECT_FOCUS` | `8u32` | `windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_FOCUS` | Ein Objekt erhielt den Fokus |
| `WINEVENT_OUTOFCONTEXT` | `0u32` | `windows::Win32::UI::WindowsAndMessaging::WINEVENT_OUTOFCONTEXT` | Callback liegt nicht im Prozessadressraum; asynchron, ereignisgeordnet |

### Funktions-Signaturen

#### SetWinEventHookW
```rust
pub unsafe fn SetWinEventHookW(
    eventmin: u32,                    // EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS, etc.
    eventmax: u32,                    // endgültiges Event in der Range
    hmodwinwindowproc: Option<HINSTANCE>,  // Module handle (NULL für WINEVENT_OUTOFCONTEXT)
    pfnwinwventproc: WINEVENTPROC,    // Callback-Funktion
    idprocess: u32,                   // Prozess-ID (0 = alle)
    idthread: u32,                    // Thread-ID (0 = alle)
    dwflags: u32,                     // WINEVENT_OUTOFCONTEXT, etc.
) -> Result<HWINEVENTHOOK>
```

**Rückgabewert:** `Result<HWINEVENTHOOK>` — Hook-Handle bei Erfolg, Error bei Misserfolg.

#### UnhookWinEvent
```rust
pub unsafe fn UnhookWinEvent(hwineventhook: HWINEVENTHOOK) -> Result<()>
```

### WinEventProc Callback-Signatur
```rust
// Typ im windows-crate:
pub type WINEVENTPROC = unsafe extern "system" fn(
    hwineventhook: HWINEVENTHOOK,     // Hook-Handle
    event: u32,                       // Event-Typ (z.B. EVENT_SYSTEM_FOREGROUND)
    hwnd: HWND,                       // Fenster-Handle (kann NULL sein)
    idobject: i32,                    // Objekt-ID
    idchild: i32,                     // Kind-Element-ID
    ideventthread: u32,               // Thread, der das Event erzeugt hat
    dwmseventtime: u32,               // Zeit in Millisekunden
) -> ();
```

### Message-Loop (Nachricht-Verarbeitung)
```rust
// Signatur: GetMessageW
pub unsafe fn GetMessageW(
    lpmsg: *mut MSG,                  // Zeiger auf MSG-Struktur
    hwnd: Option<HWND>,               // Fenster-Filter (None = alle)
    wmsgfiltermin: u32,               // Min-Message-ID (0 = keine Filterung)
    wmsgfiltermax: u32,               // Max-Message-ID (0 = keine Filterung)
) -> BOOL

// TranslateMessage
pub unsafe fn TranslateMessage(lpmsg: *const MSG) -> BOOL

// DispatchMessageW
pub unsafe fn DispatchMessageW(lppmsg: *const MSG) -> LRESULT
```

### Aufruf-Beispiel
```rust
use windows::Win32::System::Com::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe {
    // COM initialisieren
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    
    // Hook installieren
    let hook = SetWinEventHookW(
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_FOREGROUND,
        None,                           // Kein Modul (WINEVENT_OUTOFCONTEXT)
        my_winevent_callback,
        0,                              // Alle Prozesse
        0,                              // Alle Threads
        WINEVENT_OUTOFCONTEXT,
    )?;
    
    // Message-Loop
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    
    // Hook entfernen
    UnhookWinEvent(hook)?;
    CoUninitialize();
    Ok(())
}

unsafe extern "system" fn my_winevent_callback(
    hwineventhook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    idchild: i32,
    ideventthread: u32,
    dwmseventtime: u32,
) {
    // Callback-Logik — nur Zeitpunkt ist relevant
    // KEINE vkCode/scanCode-Verarbeitung (Privacy D3)
}
```

---

## 2. Low-Level-Hooks: SetWindowsHookExW

### Modul-Pfad
```rust
windows::Win32::UI::WindowsAndMessaging
```

### Hook-Typen
| Konstante | Pfad | Bedeutung |
|---|---|---|
| `WH_KEYBOARD_LL` | `windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL` | Low-level Keyboard-Hook (global) |
| `WH_MOUSE_LL` | `windows::Win32::UI::WindowsAndMessaging::WH_MOUSE_LL` | Low-level Mouse-Hook (global) |

### Funktions-Signatur

#### SetWindowsHookExW
```rust
pub unsafe fn SetWindowsHookExW(
    idhook: WINDOWS_HOOK_ID,          // WH_KEYBOARD_LL oder WH_MOUSE_LL
    lpfn: HOOKPROC,                   // Callback-Funktion
    hmod: Option<HINSTANCE>,          // Module-Handle (für DLL-basierte Hooks)
    dwthreadid: u32,                  // Thread-ID (0 für globale Hooks)
) -> Result<HHOOK>
```

**Wichtig für MerkWerk:** Für globale Low-Level-Hooks MUSS `dwthreadid = 0` gesetzt werden.

#### UnhookWindowsHookEx
```rust
pub unsafe fn UnhookWindowsHookEx(hhook: HHOOK) -> Result<()>
```

### Callback-Signaturen

#### LowLevelKeyboardProc
```rust
pub type HOOKPROC = unsafe extern "system" fn(
    ncode: i32,                       // Hook-Code: HC_ACTION (0) oder negativ
    wparam: WPARAM,                   // WM_KEYDOWN, WM_KEYUP, etc.
    lparam: LPARAM,                   // Zeiger auf KBDLLHOOKSTRUCT
) -> LRESULT;
```

#### LowLevelMouseProc
```rust
pub type HOOKPROC = unsafe extern "system" fn(
    ncode: i32,                       // Hook-Code
    wparam: WPARAM,                   // WM_MOUSEMOVE, WM_LBUTTONDOWN, etc.
    lparam: LPARAM,                   // Zeiger auf MSLLHOOKSTRUCT
) -> LRESULT;
```

### Hook-Struktur: KBDLLHOOKSTRUCT
```rust
#[repr(C)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,                  // Virtual Key Code
    pub scanCode: u32,                // Hardware Scan Code
    pub flags: KBDLLHOOKSTRUCT_FLAGS, // Flags (Extended-Key, Injected, etc.)
    pub time: u32,                    // Timestamp in Millisekunden
    pub dwExtraInfo: usize,           // Zusätzliche Infos
}
```

**Kritisch für MerkWerk — Privacy-Invariante D3:**  
MerkWerk liest `vkCode` und `scanCode` NICHT aus! Nur der Callback-Aufruf-Zeitpunkt ist relevant für die Aktivitätserfassung.

### Hook-Struktur: MSLLHOOKSTRUCT
```rust
#[repr(C)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,                    // Maus-Position (x, y)
    pub mouseData: i32,               // Wheel-Daten, etc.
    pub flags: u32,                   // Injected, Lower IL Privileges, etc.
    pub time: u32,                    // Timestamp
    pub dwExtraInfo: usize,           // Zusätzliche Infos
}
```

### Aufruf-Beispiel (Keyboard-Hook)
```rust
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Foundation::*;

unsafe {
    let hook = SetWindowsHookExW(
        WH_KEYBOARD_LL,
        Some(keyboard_hook_proc),
        None,                          // Kein Module für low-level hooks
        0,                             // Global (Threads ID = 0)
    )?;

    // Hook verwenden ...

    UnhookWindowsHookEx(hook)?;
}

unsafe extern "system" fn keyboard_hook_proc(
    ncode: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if ncode == 0 { // HC_ACTION
        // WICHTIG: vkCode NICHT auslesen (Privacy D3)
        // Nur Zeitpunkt (Callback-Zeit) ist relevant
        log_activity_timestamp();
    }
    // Hook-Kette fortsetzen
    CallNextHookEx(None, ncode, wparam, lparam)
}
```

---

## 3. Aktives Fenster — Window-Informationen

### Modul-Pfade
```rust
windows::Win32::UI::WindowsAndMessaging  // GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId
windows::Win32::System::Threading        // OpenProcess, QueryFullProcessImageNameW
```

### Funktions-Signaturen

#### GetForegroundWindow
```rust
pub unsafe fn GetForegroundWindow() -> HWND
```
**Rückgabewert:** `HWND` des aktuellen Foreground-Fensters.

#### GetWindowTextW
```rust
pub unsafe fn GetWindowTextW(
    hwnd: HWND,
    lpstring: &mut [u16],  // Mutable Slice für UTF-16 Fenster-Text
) -> i32                    // Länge des eingelesenen Texts
```

#### GetWindowThreadProcessId
```rust
pub unsafe fn GetWindowThreadProcessId(
    hwnd: HWND,
    lpdwprocessid: *mut u32,  // Zeiger auf u32 für Prozess-ID
) -> u32                      // Thread-ID
```

#### OpenProcess
```rust
pub unsafe fn OpenProcess(
    dwdesiredaccess: PROCESS_ACCESS_RIGHTS,  // PROCESS_QUERY_LIMITED_INFORMATION, etc.
    binherithandle: bool,                     // Handle-Vererbung
    dwprocessid: u32,                        // Prozess-ID
) -> Result<HANDLE>
```

#### QueryFullProcessImageNameW
```rust
pub unsafe fn QueryFullProcessImageNameW(
    hprocess: HANDLE,           // Prozess-Handle
    dwflags: PROCESS_NAME_FORMAT, // 0 = normaler Pfad, 1 = Device-Pfad
    lpexename: PWSTR,           // Zeiger auf UTF-16 Buffer für Pfad
    lpdwsize: *mut u32,         // Zeiger auf Größe (rein/raus)
) -> Result<()>
```

### Aufruf-Beispiel
```rust
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Threading::*;
use windows::Win32::Foundation::*;

unsafe {
    // Aktives Fenster abrufen
    let hwnd = GetForegroundWindow();
    
    // Fenster-Titel
    let mut title: [u16; 256] = [0; 256];
    let len = GetWindowTextW(hwnd, &mut title);
    let title_str = String::from_utf16_lossy(&title[..len as usize]);
    
    // Prozess-ID
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    
    // Prozessname via OpenProcess + QueryFullProcessImageNameW
    let access = PROCESS_QUERY_LIMITED_INFORMATION;
    if let Ok(hprocess) = OpenProcess(access, false, pid) {
        let mut exe_path: [u16; 260] = [0; 260];
        let mut size = 260u32;
        if QueryFullProcessImageNameW(hprocess, 0, PWSTR(exe_path.as_mut_ptr()), &mut size).is_ok() {
            let exe = String::from_utf16_lossy(&exe_path[..size as usize]);
            println!("Prozess: {}", exe);
        }
    }
}
```

---

## 4. UIAutomation — COM-basierte UI-Element-Analyse

### Modul-Pfade
```rust
windows::Win32::System::Com           // CoInitializeEx, CoCreateInstance
windows::Win32::UI::Accessibility     // IUIAutomation, IUIAutomationElement, TreeWalker
```

### COM-Initialisierung

#### CoInitializeEx
```rust
pub unsafe fn CoInitializeEx(
    pvreserved: Option<*const c_void>,  // Typisch: None
    dwcoinit: COINIT,                   // COINIT_MULTITHREADED, COINIT_APARTMENTTHREADED
) -> HRESULT
```

**COINIT-Werte:**
- `COINIT_MULTITHREADED = 0u32` — Multithreaded Apartment
- `COINIT_APARTMENTTHREADED = 2u32` — Single-Threaded Apartment

**Empfehlung für MerkWerk:** `COINIT_MULTITHREADED` für Daemon-Threads.

#### CoUninitialize
```rust
pub unsafe fn CoUninitialize()
```

### CoCreateInstance für CUIAutomation
```rust
pub unsafe fn CoCreateInstance(
    rclsid: *const GUID,                 // &CUIAutomation (CLSID)
    punkouter: Option<IUnknown>,        // Typically None
    dwclsctx: CLSCTX,                   // CLSCTX_ALL (= 0x17)
    riid: *const GUID,                  // &IUIAutomation::IID
) -> Result<T>                          // T meist IUIAutomation
```

**Rückgabewert:** `Result<IUIAutomation>` — COM-Interface zur Automation.

### IUIAutomation Interface

#### ElementFromHandle
```rust
// Methode auf IUIAutomation:
pub fn ElementFromHandle(
    &self,
    hwnd: UIA_HWND,  // HWND geparkt als UIA_HWND(hwnd.0)
) -> Result<IUIAutomationElement>
```

### IUIAutomationElement Properties

Eigenschaften werden via `CurrentXXX()` oder `CachedXXX()` Methoden abgerufen:

| Property | Methode | Rückgabewert | Bedeutung |
|---|---|---|---|
| Name | `CurrentName()` | `BSTR` | UI-Element-Text |
| Value | `CurrentValue()` | `VARIANT` | Wert (z.B. Text in Textfeld) |
| ControlType | `CurrentControlType()` | `CONTROLTYPEID` | Art des Controls |
| IsPassword | `CurrentIsPassword()` | `BOOL` | Ist Passwortfeld? |
| IsControlElement | `CurrentIsControlElement()` | `BOOL` | Ist Kontrollelement? |
| IsContentElement | `CurrentIsContentElement()` | `BOOL` | Hat Content? |

### TreeWalker für Element-Navigation

#### RawViewWalker
```rust
// Alle Elemente im Baum
let walker = unsafe { RawViewWalker()? };
let first_child = walker.GetFirstChildElement(&element)?;
let parent = walker.GetParentElement(&element)?;
let next_sibling = walker.GetNextSiblingElement(&element)?;
```

**RawViewWalker:** Zeigt jeden Automation-Element in der kompletten Baumstruktur.

#### ControlViewWalker
```rust
let walker = unsafe { ControlViewWalker()? };
let first_control = walker.GetFirstChildElement(&element)?;
```

**ControlViewWalker:** Zeigt nur Elemente, wo `IsControlElement = TRUE`.

### Passwortfeld-Erkennung
```rust
unsafe {
    let is_password = element.CurrentIsPassword()?;
    if is_password {
        // Feld wird als Passwort behandelt — Inhalt nicht auslesen!
    }
}
```

### Aufruf-Beispiel (UIAutomation)
```rust
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::Foundation::*;

unsafe {
    // COM initialisieren
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    
    // CUIAutomation-Instanz erstellen
    let automation: IUIAutomation = CoCreateInstance(
        &CUIAutomation,
        None,
        CLSCTX_ALL,
    )?;
    
    // Aktives Fenster
    let hwnd = GetForegroundWindow();
    let element = automation.ElementFromHandle(UIA_HWND(hwnd.0))?;
    
    // Name abrufen
    let name = element.CurrentName()?;
    println!("Window Name: {}", String::from_utf16_lossy(&name));
    
    // RawViewWalker verwenden
    let walker = RawViewWalker()?;
    let first_child = walker.GetFirstChildElement(&element);
    
    // Aufräumen
    CoUninitialize();
}
```

**Property-IDs (für erweiterte Abfragen):**
- `UIA_NamePropertyId = 30005`
- `UIA_IsPasswordPropertyId = 30019`
- `UIA_ValueValuePropertyId` — **korrekt: `UIA_ValuePropertyId = 30045`**
- `UIA_ControlTypePropertyId = 30003`

---

## 5. Browser-URL aus Adressleiste per UIAutomation

### Übliche Herangehensweise für Chromium-Browser (Chrome, Edge)

#### Chrome/Edge Muster
```
ControlType:    Edit
Name:           "Adress- und Suchleiste" (DE) oder "Address and search bar" (EN)
AutomationId:   Variabel, meist leer oder "address-bar"
```

**Lokalisierungs-Varianten:**
- DE: "Adress- und Suchleiste"
- EN: "Address and search bar"
- ES: "Barra de direcciones y búsqueda"
- FR: "Barre d'adresse et de recherche"

#### Extraktions-Logik
```rust
unsafe {
    let walker = ControlViewWalker()?;
    let mut child = walker.GetFirstChildElement(&window_element)?;
    
    while !child.is_null() {
        let name = child.CurrentName().ok();
        let ctrl_type = child.CurrentControlType().ok();
        
        // Suche: ControlType = Edit UND Name enthält "address" / "Adress"
        if let (Some(n), Some(ct)) = (name, ctrl_type) {
            let name_str = String::from_utf16_lossy(&n);
            if name_str.to_lowercase().contains("address") && ct == UIA_EditControlTypeId {
                // Potenzielle Adressleiste gefunden
                let value = child.CurrentValue()?;
                let url = String::from_utf16_lossy(&value);
                return Ok(url.to_string());
            }
        }
        
        child = walker.GetNextSiblingElement(&child)?;
    }
}
```

**Konstante für Edit-ControlType:**
```rust
const UIA_EditControlTypeId: CONTROLTYPEID = CONTROLTYPEID(50004);
```

### Firefox Muster

#### Firefox Address Bar
```
ControlType:    Combo Box oder Edit (variabel je Firefox-Version)
Name:           "Search with Google or enter address" (EN)
                "Suchen mit Google oder Adresse eingeben" (DE)
AutomationId:   Oft "url-bar-input" oder "urlbar"
```

**Hinweis:** Firefox address bar kann schwierig zu automatisieren sein. Es wird empfohlen, auch den Browser-History oder Debug-Schnittstellen zu prüfen.

#### Firefox Extraktions-Alternative
```rust
// Fall-insensitiv nach "search" oder "address" im Namen suchen
let name_lower = name_str.to_lowercase();
if (name_lower.contains("search") && name_lower.contains("google")) || 
   name_lower.contains("address") {
    // Potenzieller Treffer
}
```

### Vollständiges Beispiel: Browser-URL auslesen
```rust
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

unsafe fn get_browser_url() -> Result<String, Box<dyn std::error::Error>> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    
    let automation: IUIAutomation = CoCreateInstance(
        &CUIAutomation,
        None,
        CLSCTX_ALL,
    )?;
    
    let hwnd = GetForegroundWindow();
    let window_elem = automation.ElementFromHandle(UIA_HWND(hwnd.0))?;
    
    let walker = ControlViewWalker()?;
    let mut child = walker.GetFirstChildElement(&window_elem)?;
    
    while !child.is_null() {
        if let Ok(name) = child.CurrentName() {
            let name_str = String::from_utf16_lossy(&name).to_string();
            
            // Chrome/Edge/Firefox Adressleiste-Muster
            let is_address_bar = name_str.to_lowercase().contains("address") ||
                                 name_str.to_lowercase().contains("adress") ||
                                 (name_str.contains("Search") && name_str.contains("Google"));
            
            if is_address_bar {
                if let Ok(value) = child.CurrentValue() {
                    let url = String::from_utf16_lossy(&value).to_string();
                    CoUninitialize();
                    return Ok(url);
                }
            }
        }
        
        child = walker.GetNextSiblingElement(&child)?;
    }
    
    CoUninitialize();
    Err("Address bar not found".into())
}
```

---

## Zusammenfassung der Feature-Flags

```toml
[dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",              # Basis-Typen (HWND, HANDLE, etc.)
    "Win32_Graphics_Gdi",            # GDI (benötigt durch WindowsAndMessaging)
    "Win32_System_Com",              # COM (CoInitializeEx, CoCreateInstance)
    "Win32_System_Memory",           # Memory operations
    "Win32_System_Threading",        # Prozess & Thread APIs
    "Win32_UI_Accessibility",        # UIAutomation
    "Win32_UI_WindowsAndMessaging",  # Hooks, Fenster, Nachrichten
] }
```

---

## Quellen und Referenzen

- [windows 0.58.0 — crates.io](https://crates.io/crates/windows/0.58.0)
- [microsoft/windows-rs — GitHub](https://github.com/microsoft/windows-rs)
- [microsoft.github.io/windows-docs-rs — Offizielle Dokumentation](https://microsoft.github.io/windows-docs-rs)
- [SetWinEventHook — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
- [SetWindowsHookExW — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw)
- [UIAutomation — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-architecture)
- [Low-Level Keyboard Hook — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)
- [Low-Level Mouse Hook — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc)
- [CoInitializeEx — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex)

