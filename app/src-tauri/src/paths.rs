//! Gemeinsame Pfadauflösung für die App — identisch zu der des Daemons
//! (`merkwerk-daemon/src/main.rs`), damit App und Daemon dieselbe Konfig-Datei
//! und dieselbe DB verwenden.

use std::path::PathBuf;

/// `%APPDATA%\MerkWerk` (Windows) bzw. Entwicklungs-Fallback über
/// `MERKWERK_DATA_DIR` oder `./merkwerk-data`.
pub fn data_dir() -> PathBuf {
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

/// Pfad der Konfig-Datei.
pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// Löst den DB-Pfad aus der Konfig auf: absolute Pfade unverändert, relative
/// unter das Datenverzeichnis (gleiche Regel wie der Daemon).
pub fn resolve_db_path(cfg: &config::Config) -> PathBuf {
    if cfg.db_path.is_absolute() {
        cfg.db_path.clone()
    } else {
        data_dir().join(&cfg.db_path)
    }
}
