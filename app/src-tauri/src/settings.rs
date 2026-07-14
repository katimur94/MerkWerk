//! Settings-Commands: Blacklist-Editor (schreibt die Konfig-TOML, die der Daemon
//! liest) und Autostart-Toggle (HKCU Run-Key).
//!
//! STUB (Seam für Task T11): Signaturen + DTO stehen fest; T11 implementiert die
//! Bodies. Alle vier Commands sind in `lib.rs` registriert.

use serde::{Deserialize, Serialize};

/// Blacklist-Sicht für das Frontend (spiegelt `config::BlacklistConfig`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlacklistDto {
    pub process_names: Vec<String>,
    pub title_patterns: Vec<String>,
    pub url_patterns: Vec<String>,
}

/// Liest die aktuelle Blacklist aus der Konfig-Datei.
///
/// TODO(T11): `config::Config::load(paths::config_path())` und die Blacklist-Felder
/// in `BlacklistDto` übernehmen.
#[tauri::command]
pub fn get_blacklist() -> Result<BlacklistDto, String> {
    Ok(BlacklistDto::default())
}

/// Schreibt die Blacklist in die Konfig-Datei und stößt einen Daemon-Reload an.
///
/// TODO(T11): Konfig laden, Blacklist-Felder ersetzen, `Config::save` schreiben,
/// danach dem Daemon `Request::ReloadConfig` über die Named Pipe schicken
/// (`ipc_protocol`); falls die Pipe nicht erreichbar ist, nur die Datei schreiben
/// (der Daemon lädt beim nächsten Start neu).
#[tauri::command]
pub fn set_blacklist(blacklist: BlacklistDto) -> Result<(), String> {
    let _ = blacklist;
    Ok(())
}

/// Gibt zurück, ob der Autostart-Eintrag (HKCU Run-Key) gesetzt ist.
///
/// TODO(T11): HKCU\Software\Microsoft\Windows\CurrentVersion\Run auf einen
/// `MerkWerk`-Wert prüfen.
#[tauri::command]
pub fn get_autostart() -> Result<bool, String> {
    Ok(false)
}

/// Setzt/entfernt den Autostart-Eintrag.
///
/// TODO(T11): HKCU Run-Key `MerkWerk` schreiben/löschen (Pfad zum Daemon-Binary;
/// die 30-s-Verzögerung übernimmt der Daemon beim Start, siehe ARCHITEKTUR.md).
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let _ = enabled;
    Ok(())
}
