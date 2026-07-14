//! Settings-Commands: Blacklist-Editor (schreibt die Konfig-TOML, die der Daemon
//! liest) und Autostart-Toggle (Windows HKCU Run-Key).
//!
//! Alle vier Commands sind in `lib.rs` registriert (siehe dort,
//! `invoke_handler`). `set_blacklist` stößt nach dem Schreiben best-effort
//! einen Daemon-Reload über die Named Pipe an (`notify_daemon_config_reload`
//! unten); schlägt das fehl, ist das kein Fehler — die Konfig-Datei ist
//! bereits geschrieben, der Daemon lädt sie beim nächsten Start ohnehin neu
//! (siehe ARCHITEKTUR.md, Abschnitt "Konfiguration"). Autostart schreibt
//! bzw. löscht den HKCU-Run-Key-Wert `MerkWerk`; die 30-s-Startverzögerung
//! übernimmt der Daemon selbst (siehe ARCHITEKTUR.md, Komponente
//! `merkwerk-daemon`).

use serde::{Deserialize, Serialize};

use crate::paths;

/// Blacklist-Sicht für das Frontend (spiegelt `config::BlacklistConfig`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlacklistDto {
    pub process_names: Vec<String>,
    pub title_patterns: Vec<String>,
    pub url_patterns: Vec<String>,
}

impl From<config::BlacklistConfig> for BlacklistDto {
    fn from(blacklist: config::BlacklistConfig) -> Self {
        Self {
            process_names: blacklist.process_names,
            title_patterns: blacklist.title_patterns,
            url_patterns: blacklist.url_patterns,
        }
    }
}

/// Liest die aktuelle Blacklist aus der Konfig-Datei.
///
/// Existiert die Datei noch nicht, legt `config::Config::load` sie mit
/// Default-Werten an (leere Blacklist) — das ist hier der korrekte
/// Erfolgsfall, kein Fehler.
#[tauri::command]
pub fn get_blacklist() -> Result<BlacklistDto, String> {
    let cfg = config::Config::load(&paths::config_path()).map_err(|e| e.to_string())?;
    Ok(cfg.blacklist.into())
}

/// Schreibt die Blacklist in die Konfig-Datei und stößt einen Daemon-Reload an.
///
/// Lädt zunächst die bestehende Konfig (oder Defaults, falls die Datei noch
/// nicht existiert — siehe `get_blacklist`), ersetzt darin nur die
/// Blacklist-Felder und schreibt die Datei zurück, damit alle anderen
/// Konfig-Abschnitte (Debounce, Snapshot, Retention, DB-Pfad) unverändert
/// erhalten bleiben.
#[tauri::command]
pub fn set_blacklist(blacklist: BlacklistDto) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = config::Config::load(&path).map_err(|e| e.to_string())?;

    cfg.blacklist.process_names = blacklist.process_names;
    cfg.blacklist.title_patterns = blacklist.title_patterns;
    cfg.blacklist.url_patterns = blacklist.url_patterns;

    cfg.save(&path).map_err(|e| e.to_string())?;

    notify_daemon_config_reload();

    Ok(())
}

/// Best-effort: informiert einen laufenden Daemon per IPC, dass sich die
/// Konfig-Datei geändert hat, damit er sofort neu lädt statt erst beim
/// nächsten Start. Ist die Pipe nicht erreichbar (Daemon läuft nicht), wird
/// der Fehler bewusst verschluckt — siehe Modul-Doc oben.
fn notify_daemon_config_reload() {
    #[cfg(windows)]
    {
        let _ = send_reload_config_over_pipe();
    }
}

/// Öffnet die Daemon-Named-Pipe und schickt `Request::ReloadConfig`.
///
/// Pipe-Name und Wire-Format kommen direkt aus dem `ipc-protocol`-Crate
/// (`PIPE_NAME`, `encode_request`) — so gibt es genau eine Quelle der Wahrheit
/// für das Protokoll, dieselbe, die auch der Daemon-Server parst.
#[cfg(windows)]
fn send_reload_config_over_pipe() -> std::io::Result<()> {
    use std::io::Write;

    let line = ipc_protocol::encode_request(&ipc_protocol::Request::ReloadConfig);

    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(ipc_protocol::PIPE_NAME)?;

    pipe.write_all(line.as_bytes())
}

/// Gibt zurück, ob der Autostart-Eintrag (HKCU Run-Key, Wert `MerkWerk`)
/// gesetzt ist.
#[tauri::command]
pub fn get_autostart() -> Result<bool, String> {
    #[cfg(windows)]
    {
        win_autostart::is_enabled()
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

/// Setzt bzw. entfernt den Autostart-Eintrag (HKCU Run-Key, Wert `MerkWerk`).
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        win_autostart::set_enabled(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// Windows-Registry-Zugriff für den Autostart-Toggle
/// (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
///
/// Nur unter `#[cfg(windows)]` kompiliert; `get_autostart`/`set_autostart`
/// liefern auf anderen Zielplattformen die in ihnen definierten Stubs
/// (`Ok(false)` / `Ok(())`), damit ein Nicht-Windows-Host-Build nicht bricht.
#[cfg(windows)]
mod win_autostart {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ,
    };

    const AUTOSTART_VALUE_NAME: &str = "MerkWerk";
    const RUN_KEY_SUBPATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// RAII-Wrapper um ein offenes Registry-Handle: `RegCloseKey` läuft
    /// zuverlässig beim Verlassen des Scopes, auch bei frühem `?`-Ausstieg.
    struct OpenKey(HKEY);

    impl Drop for OpenKey {
        fn drop(&mut self) {
            // Best-effort: ein Fehler beim Schließen eines Handles ist an
            // dieser Stelle nicht sinnvoll behebbar (kein Panic in Drop).
            let _ = unsafe { RegCloseKey(self.0) };
        }
    }

    /// UTF-16-Kodierung inkl. Null-Terminator, wie sie die `*W`-Registry-
    /// Funktionen über `PCWSTR` erwarten.
    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reinterpretiert einen UTF-16-Puffer (inkl. Null-Terminator) als
    /// Byte-Slice für `RegSetValueExW`, das `REG_SZ`-Daten roh in Bytes
    /// erwartet.
    ///
    /// SAFETY: `u16` hat kein internes Padding und eine mindestens so
    /// strenge Ausrichtung wie `u8`; eine byteweise Sicht auf den Puffer ist
    /// daher immer gültig.
    fn wide_bytes(wide: &[u16]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), std::mem::size_of_val(wide))
        }
    }

    /// Öffnet `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` mit den
    /// gewünschten Zugriffsrechten. Dieser Schlüssel existiert auf jeder
    /// Windows-Installation; ein Fehlschlag hier ist daher ein echter Fehler
    /// (z. B. Rechte) und kein "Autostart ist aus"-Zustand.
    fn open_run_key(sam_desired: REG_SAM_FLAGS) -> Result<OpenKey, String> {
        let subkey = to_wide_null(RUN_KEY_SUBPATH);
        let mut hkey = HKEY(std::ptr::null_mut());

        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                sam_desired,
                &mut hkey,
            )
        };

        if status.is_err() {
            return Err(format!(
                "HKCU\\{RUN_KEY_SUBPATH} konnte nicht geöffnet werden: {status:?}"
            ));
        }

        Ok(OpenKey(hkey))
    }

    /// Prüft, ob der Run-Key-Wert `MerkWerk` existiert. Ein fehlender Wert
    /// ist `Ok(false)`, kein Fehler.
    pub fn is_enabled() -> Result<bool, String> {
        let key = open_run_key(KEY_QUERY_VALUE)?;
        let value_name = to_wide_null(AUTOSTART_VALUE_NAME);
        let mut data_len: u32 = 0;

        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(value_name.as_ptr()),
                None,
                None,
                None,
                Some(&mut data_len as *mut u32),
            )
        };

        if status == ERROR_FILE_NOT_FOUND {
            Ok(false)
        } else if status.is_err() {
            Err(format!(
                "Autostart-Wert konnte nicht gelesen werden: {status:?}"
            ))
        } else {
            Ok(true)
        }
    }

    /// Setzt den Run-Key-Wert `MerkWerk` auf den Pfad des Daemon-Binaries
    /// (`enabled == true`) bzw. löscht ihn (`enabled == false`; ein bereits
    /// fehlender Wert ist dabei `Ok(())`, kein Fehler).
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let key = open_run_key(KEY_SET_VALUE)?;
        let value_name = to_wide_null(AUTOSTART_VALUE_NAME);

        if enabled {
            // In Anführungszeichen, damit ein Pfad mit Leerzeichen (z. B.
            // unter "C:\Program Files\...") beim Autostart als ein
            // einzelnes Kommandozeilenargument erkannt wird.
            let quoted_path = format!("\"{}\"", daemon_exe_path());
            let data = to_wide_null(&quoted_path);

            let status = unsafe {
                RegSetValueExW(
                    key.0,
                    PCWSTR(value_name.as_ptr()),
                    0,
                    REG_SZ,
                    Some(wide_bytes(&data)),
                )
            };

            if status.is_err() {
                return Err(format!(
                    "Autostart-Wert konnte nicht gesetzt werden: {status:?}"
                ));
            }
        } else {
            let status = unsafe { RegDeleteValueW(key.0, PCWSTR(value_name.as_ptr())) };

            if status.is_err() && status != ERROR_FILE_NOT_FOUND {
                return Err(format!(
                    "Autostart-Wert konnte nicht gelöscht werden: {status:?}"
                ));
            }
        }

        Ok(())
    }

    /// Pfad zum Daemon-Binary, das laut ARCHITEKTUR.md neben der App-Exe
    /// installiert wird. Die Existenz wird bewusst nicht geprüft — App und
    /// Daemon werden gemeinsam installiert; selbst wenn die Datei (noch)
    /// fehlt, soll sich der Autostart-Eintrag trotzdem setzen lassen.
    fn daemon_exe_path() -> String {
        let install_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()));

        match install_dir {
            Some(dir) => dir.join("merkwerk-daemon.exe").display().to_string(),
            None => "merkwerk-daemon.exe".to_string(),
        }
    }
}
