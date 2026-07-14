//! `merkwerk-daemon` — headless Capture-Daemon (Etappe 0).
//!
//! Verdrahtet die reviewten Bausteine zu einer Laufzeit: die OS-Hooks
//! ([`capture_win::hooks`]) speisen den Debouncer ([`capture_win::debounce`]),
//! dessen Trigger den `app_session`-Lebenszyklus, die Blacklist-Prüfung *an der
//! Quelle* ([`blacklist`]) und die UIA-Snapshots ([`capture_win::uia`]) steuern;
//! erlaubte Daten landen gebündelt in der SQLite-DB ([`storage`]). Ein
//! Named-Pipe-IPC-Server ([`ipc_server`], Kommandos aus [`ipc_protocol`])
//! erlaubt der App, Status abzufragen und Pause/Resume/Reload zu steuern.
//!
//! Die Erfassung ist naturgemäß Windows-spezifisch und lebt hinter
//! `#[cfg(windows)]`. Auf anderen Plattformen kompiliert das Binary weiterhin
//! (für Entwicklung/CI in der Linux-Sandbox), gibt beim Start aber nur einen
//! Hinweis aus und beendet sich — es gibt dort keine Bildschirmaktivität zu
//! erfassen.

// Auf Nicht-Windows werden `control`/`policy` nur von ihren Unit-Tests benutzt —
// die Laufzeit-Konsumenten `runtime`/`ipc_server` sind `#[cfg(windows)]`. Dort ist
// „dead code" also erwartbar; auf dem echten Windows-Target greift weiterhin
// `clippy -D warnings` und würde echten toten Code melden.
#[cfg_attr(not(windows), allow(dead_code))]
mod control;
#[cfg_attr(not(windows), allow(dead_code))]
mod policy;

#[cfg(windows)]
mod ipc_server;
#[cfg(windows)]
mod runtime;

use std::path::PathBuf;

/// Verzeichnis für Konfig + DB: `%APPDATA%\MerkWerk` auf Windows; sonst ein
/// Entwicklungs-Fallback (über `MERKWERK_DATA_DIR` steuerbar).
fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("MerkWerk");
        }
    }
    std::env::var("MERKWERK_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("merkwerk-data"))
}

fn main() {
    let dir = data_dir();
    let config_path = dir.join("config.toml");

    // Konfig laden (legt bei Nichtexistenz eine Default-Datei an).
    let cfg = match config::Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[merkwerk-daemon] Konfig konnte nicht geladen werden ({config_path:?}): {e}"
            );
            std::process::exit(1);
        }
    };

    // DB-Pfad: relative Pfade unter das Datenverzeichnis legen.
    let db_path = if cfg.db_path.is_absolute() {
        cfg.db_path.clone()
    } else {
        dir.join(&cfg.db_path)
    };

    #[cfg(windows)]
    {
        if let Err(e) = runtime::run(cfg, config_path.clone(), db_path) {
            eprintln!("[merkwerk-daemon] Laufzeitfehler: {e}");
            std::process::exit(1);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (cfg, db_path);
        eprintln!(
            "[merkwerk-daemon] Dieses Binary erfasst nur unter Windows. \
             Auf dieser Plattform ist nichts zu tun (Konfig unter {config_path:?} \
             wurde validiert/angelegt)."
        );
    }
}
