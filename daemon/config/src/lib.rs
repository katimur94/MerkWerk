//! Configuration management for MerkWerk daemon.
//!
//! Liest/schreibt die TOML-Konfiguration unter `%APPDATA%\MerkWerk\config.toml`
//! (siehe ARCHITEKTUR.md, Abschnitt "Konfiguration"). Das Auflösen von
//! `%APPDATA%` ist Aufgabe des Aufrufers (`Config::load`/`Config::save`
//! nehmen einen fertigen Pfad entgegen); dieses Modul kennt nur den
//! Default-Dateinamen als Platzhalter.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Fehler der Konfigurations-Ladung/-Speicherung.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO-Fehler beim Lesen, Schreiben oder Anlegen von Verzeichnissen.
    #[error("IO-Fehler bei Konfigurationsdatei {path}: {source}")]
    Io {
        /// Betroffener Pfad.
        path: PathBuf,
        /// Zugrunde liegender IO-Fehler.
        #[source]
        source: std::io::Error,
    },

    /// Die TOML-Datei konnte nicht geparst werden.
    #[error("Konfigurationsdatei {path} ist kein gültiges TOML: {source}")]
    Parse {
        /// Betroffener Pfad.
        path: PathBuf,
        /// Zugrunde liegender Parse-Fehler.
        #[source]
        source: toml::de::Error,
    },

    /// Die Konfiguration konnte nicht in TOML serialisiert werden.
    #[error("Konfiguration konnte nicht serialisiert werden: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Ergebnistyp für Operationen dieses Moduls.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Blacklist-Regeln: Treffer werden vor der Persistierung verworfen
/// (siehe ARCHITEKTUR.md, Privacy-Invariante 3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlacklistConfig {
    /// Prozessnamen, die vollständig ignoriert werden (z. B. "keepass.exe").
    #[serde(default)]
    pub process_names: Vec<String>,

    /// Muster (Glob) für Fenstertitel, die verworfen werden.
    #[serde(default)]
    pub title_patterns: Vec<String>,

    /// Muster (Glob) für Browser-URLs, die verworfen werden.
    #[serde(default)]
    pub url_patterns: Vec<String>,
}

/// Debounce-Schwellwerte für den Event-Loop (siehe ARCHITEKTUR.md, Debouncer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebounceConfig {
    /// Tipp-Pause in Millisekunden, ab der ein `typing_burst`-Event abgeschlossen gilt.
    #[serde(default = "DebounceConfig::default_typing_pause_ms")]
    pub typing_pause_ms: u64,

    /// Zeitfenster in Millisekunden, in dem Klicks zu einem `click_cluster` gebündelt werden.
    #[serde(default = "DebounceConfig::default_click_cluster_ms")]
    pub click_cluster_ms: u64,

    /// Ruhezeit in Millisekunden nach der letzten Scroll-Bewegung, bis `scroll_end` feuert.
    #[serde(default = "DebounceConfig::default_scroll_end_ms")]
    pub scroll_end_ms: u64,

    /// Minimale Fokusdauer in Millisekunden, damit ein Fensterwechsel als Event zählt.
    #[serde(default = "DebounceConfig::default_min_focus_ms")]
    pub min_focus_ms: u64,
}

impl DebounceConfig {
    const fn default_typing_pause_ms() -> u64 {
        2000
    }

    const fn default_click_cluster_ms() -> u64 {
        800
    }

    const fn default_scroll_end_ms() -> u64 {
        500
    }

    const fn default_min_focus_ms() -> u64 {
        300
    }
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            typing_pause_ms: Self::default_typing_pause_ms(),
            click_cluster_ms: Self::default_click_cluster_ms(),
            scroll_end_ms: Self::default_scroll_end_ms(),
            min_focus_ms: Self::default_min_focus_ms(),
        }
    }
}

/// Grenzwerte für UIA-Kontext-Snapshots (siehe ENTSCHEIDUNGEN.md D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Maximale Größe des erfassten sichtbaren Texts pro Snapshot in Bytes (D5: 20 KB).
    #[serde(default = "SnapshotConfig::default_max_text_bytes")]
    pub max_text_bytes: usize,

    /// Maximale Tiefe beim UIA-TreeWalk gegen Latenz-Spitzen bei großen Element-Bäumen.
    #[serde(default = "SnapshotConfig::default_max_tree_depth")]
    pub max_tree_depth: u32,

    /// Maximale Anzahl besuchter UIA-Knoten pro Snapshot.
    #[serde(default = "SnapshotConfig::default_max_nodes")]
    pub max_nodes: u32,
}

impl SnapshotConfig {
    const fn default_max_text_bytes() -> usize {
        20 * 1024
    }

    const fn default_max_tree_depth() -> u32 {
        40
    }

    const fn default_max_nodes() -> u32 {
        4000
    }
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_text_bytes: Self::default_max_text_bytes(),
            max_tree_depth: Self::default_max_tree_depth(),
            max_nodes: Self::default_max_nodes(),
        }
    }
}

/// Aufbewahrungsdauer für Rohdaten (TTL, siehe ARCHITEKTUR.md DB-Schema `expires_at`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Anzahl Tage, nach denen Rohdaten (Events/Snapshots/Sessions) ablaufen.
    #[serde(default = "RetentionConfig::default_ttl_days")]
    pub ttl_days: u32,

    /// Intervall in Sekunden zwischen zwei Läufen des Retention-Löschjobs
    /// (`Store::purge_expired`), siehe ROADMAP.md Etappe 1.
    #[serde(default = "RetentionConfig::default_purge_interval_secs")]
    pub purge_interval_secs: u64,
}

impl RetentionConfig {
    const fn default_ttl_days() -> u32 {
        30
    }

    /// Stündlich: häufig genug, damit abgelaufene Rohdaten zeitnah verschwinden,
    /// selten genug, um auf einer 24/7 laufenden Erfassung nicht ins Gewicht zu fallen.
    const fn default_purge_interval_secs() -> u64 {
        3600
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            ttl_days: Self::default_ttl_days(),
            purge_interval_secs: Self::default_purge_interval_secs(),
        }
    }
}

/// Lokale KI-Inferenz (siehe ENTSCHEIDUNGEN.md D9). Backend v1 ist Ollama
/// (`endpoint`), das Textmodell `model` destilliert, `embed_model` erzeugt
/// Embeddings (Etappe 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConfig {
    /// Ollama-HTTP-Endpoint (localhost).
    #[serde(default = "AiConfig::default_endpoint")]
    pub endpoint: String,
    /// Modellname für die Text-Generierung (Destillation).
    #[serde(default = "AiConfig::default_model")]
    pub model: String,
    /// Modellname für Embeddings (semantische Suche, Etappe 3).
    #[serde(default = "AiConfig::default_embed_model")]
    pub embed_model: String,
}

impl AiConfig {
    fn default_endpoint() -> String {
        "http://127.0.0.1:11434".to_string()
    }
    fn default_model() -> String {
        "llama3.1".to_string()
    }
    fn default_embed_model() -> String {
        "nomic-embed-text".to_string()
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            endpoint: Self::default_endpoint(),
            model: Self::default_model(),
            embed_model: Self::default_embed_model(),
        }
    }
}

/// Destillation & Notiz-Vault (siehe ENTSCHEIDUNGEN.md D10, ROADMAP.md Etappe 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillConfig {
    /// Vault-Verzeichnis für die generierten `.md`-Notizen. Relativer Default;
    /// der Daemon löst ihn (wie `db_path`) unter `%APPDATA%\MerkWerk` auf.
    #[serde(default = "DistillConfig::default_vault_path")]
    pub vault_path: PathBuf,
    /// Intervall in Sekunden für automatische Destillation (`0` = nur manuell
    /// via IPC `DistillNow`).
    #[serde(default)]
    pub auto_interval_secs: u64,
    /// Höchstzahl Sessions/Snapshots, die in einen Destillat-Kontext eingehen.
    #[serde(default = "DistillConfig::default_max_snapshots")]
    pub max_snapshots: usize,
    /// Maximale Zeichen je Snapshot-Text im Kontext (char-sicher gekürzt).
    #[serde(default = "DistillConfig::default_max_chars_per_snapshot")]
    pub max_chars_per_snapshot: usize,
    /// Gesamt-Zeichendeckel des an das Modell übergebenen Kontexts.
    #[serde(default = "DistillConfig::default_max_total_context_chars")]
    pub max_total_context_chars: usize,
}

impl DistillConfig {
    fn default_vault_path() -> PathBuf {
        PathBuf::from("vault")
    }
    const fn default_max_snapshots() -> usize {
        60
    }
    const fn default_max_chars_per_snapshot() -> usize {
        800
    }
    const fn default_max_total_context_chars() -> usize {
        12000
    }
}

impl Default for DistillConfig {
    fn default() -> Self {
        Self {
            vault_path: Self::default_vault_path(),
            auto_interval_secs: 0,
            max_snapshots: Self::default_max_snapshots(),
            max_chars_per_snapshot: Self::default_max_chars_per_snapshot(),
            max_total_context_chars: Self::default_max_total_context_chars(),
        }
    }
}

/// Wurzel-Konfiguration des MerkWerk-Daemons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Pfad zur SQLite-Datenbank. Default ist ein relativer Platzhalter;
    /// der Daemon löst `%APPDATA%\MerkWerk` zur Laufzeit auf.
    #[serde(default = "Config::default_db_path")]
    pub db_path: PathBuf,

    /// Blacklist-Regeln (Prozess/Titel/URL).
    #[serde(default)]
    pub blacklist: BlacklistConfig,

    /// Debounce-Parameter des Event-Loops.
    #[serde(default)]
    pub debounce: DebounceConfig,

    /// Grenzwerte für Kontext-Snapshots.
    #[serde(default)]
    pub snapshot: SnapshotConfig,

    /// TTL/Aufbewahrung für Rohdaten.
    #[serde(default)]
    pub retention: RetentionConfig,

    /// Lokale KI-Inferenz (Ollama).
    #[serde(default)]
    pub ai: AiConfig,

    /// Destillation & Notiz-Vault.
    #[serde(default)]
    pub distill: DistillConfig,
}

impl Config {
    fn default_db_path() -> PathBuf {
        PathBuf::from("merkwerk.db")
    }

    /// Lädt die Konfiguration von `path`.
    ///
    /// Existiert die Datei nicht, wird `Config::default()` zurückgegeben UND
    /// als neue Default-Datei an `path` geschrieben (inkl. Anlegen fehlender
    /// Verzeichnisse). Nur echte IO- oder Parse-Fehler werden als `Err`
    /// propagiert.
    pub fn load(path: &Path) -> Result<Config> {
        match fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            }),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                config.save(path)?;
                Ok(config)
            }
            Err(source) => Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Schreibt die Konfiguration als hübsch formatiertes TOML nach `path`.
    ///
    /// Fehlende Elternverzeichnisse werden angelegt.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }

        let toml_string = toml::to_string_pretty(self)?;

        fs::write(path, toml_string).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_path: Self::default_db_path(),
            blacklist: BlacklistConfig::default(),
            debounce: DebounceConfig::default(),
            snapshot: SnapshotConfig::default(),
            retention: RetentionConfig::default(),
            ai: AiConfig::default(),
            distill: DistillConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let original = Config::default();
        original.save(&path).expect("save");

        let loaded = Config::load(&path).expect("load");

        assert_eq!(original, loaded);
        // Explizit: das neue Retention-Intervall-Feld nimmt am Round-Trip teil
        // und behält seinen Default (stündlich).
        assert_eq!(loaded.retention.purge_interval_secs, 3600);
        // AI-/Destillier-Defaults (D9/D10) nehmen ebenfalls teil.
        assert_eq!(loaded.ai.endpoint, "http://127.0.0.1:11434");
        assert_eq!(loaded.ai.model, "llama3.1");
        assert_eq!(loaded.distill.vault_path, PathBuf::from("vault"));
        assert_eq!(loaded.distill.auto_interval_secs, 0);
        assert_eq!(loaded.distill.max_total_context_chars, 12000);
    }

    #[test]
    fn load_on_missing_path_creates_file_and_returns_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Nicht-existentes Unterverzeichnis, um das Anlegen fehlender
        // Verzeichnisse mitzutesten.
        let path = dir.path().join("nested").join("config.toml");

        assert!(!path.exists());

        let loaded = Config::load(&path).expect("load should create default file");

        assert_eq!(loaded, Config::default());
        assert!(path.exists(), "load() should have written the default file");

        // Erneutes Laden liest jetzt die geschriebene Datei.
        let reloaded = Config::load(&path).expect("reload");
        assert_eq!(reloaded, Config::default());
    }

    #[test]
    fn partial_toml_fills_remaining_fields_with_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        // Nur ein einzelnes, tief verschachteltes Feld gesetzt.
        fs::write(&path, "[debounce]\ntyping_pause_ms = 12345\n").expect("write partial toml");

        let loaded = Config::load(&path).expect("load partial");

        assert_eq!(loaded.debounce.typing_pause_ms, 12345);
        assert_eq!(
            loaded.debounce.click_cluster_ms,
            DebounceConfig::default().click_cluster_ms
        );
        assert_eq!(
            loaded.debounce.scroll_end_ms,
            DebounceConfig::default().scroll_end_ms
        );
        assert_eq!(
            loaded.debounce.min_focus_ms,
            DebounceConfig::default().min_focus_ms
        );

        assert_eq!(loaded.db_path, Config::default_db_path());
        assert_eq!(loaded.blacklist, BlacklistConfig::default());
        assert_eq!(loaded.snapshot, SnapshotConfig::default());
        assert_eq!(loaded.retention, RetentionConfig::default());
    }

    #[test]
    fn retention_partial_toml_keeps_ttl_and_defaults_purge_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        // Nur `ttl_days` gesetzt; `purge_interval_secs` muss auf den Default fallen.
        fs::write(&path, "[retention]\nttl_days = 7\n").expect("write partial toml");

        let loaded = Config::load(&path).expect("load partial retention");

        assert_eq!(loaded.retention.ttl_days, 7);
        assert_eq!(
            loaded.retention.purge_interval_secs,
            RetentionConfig::default().purge_interval_secs
        );
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(&path, "").expect("write empty toml");

        let loaded = Config::load(&path).expect("load empty");
        assert_eq!(loaded, Config::default());
    }
}
