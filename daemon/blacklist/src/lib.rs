//! Blacklist filter engine for MerkWerk.
//!
//! Wirkt an der Quelle (siehe ARCHITEKTUR.md, Privacy-Invarianten Punkt 3):
//! Ein Treffer auf Prozessname, Fenstertitel- oder URL-Muster bedeutet, dass
//! das zugehörige Event/Snapshot verworfen wird, *bevor* es den Writer
//! erreicht. Diese Crate ist rein plattformneutral und hat bewusst KEINE
//! Abhängigkeit auf die `config`-Crate (die `config`-Crate hängt umgekehrt
//! von `blacklist` ab bzw. ruft `Blacklist::new` auf) — das vermeidet einen
//! Dependency-Zyklus im Workspace.
//!
//! Wichtig: Diese Crate loggt niemals den geblockten Inhalt selbst
//! (Fenstertitel, URL, Prozessname aus einem konkreten Event). `reason()`
//! gibt ausschließlich zurück, welches *konfigurierte Muster* gegriffen hat
//! — diese Muster stammen aus der Blacklist-Konfiguration des Nutzers, nicht
//! aus erfasstem Bildschirminhalt, und sind daher unbedenklich zu loggen.

use std::collections::HashSet;

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

/// Grund, warum ein Event/Snapshot geblockt wurde. Enthält nur das
/// *konfigurierte* Muster, nie den tatsächlich erfassten Inhalt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// Prozessname matcht (case-insensitive, `.exe`-toleriert).
    ProcessName { pattern: String },
    /// Fenstertitel matcht ein Glob-Muster.
    TitlePattern { pattern: String },
    /// URL matcht ein Glob-Muster.
    UrlPattern { pattern: String },
}

/// Blacklist-Filter: verwirft Events/Snapshots anhand von drei
/// Musterlisten (Prozessname, Fenstertitel, URL), bevor sie persistiert
/// werden.
pub struct Blacklist {
    /// Normalisierte Prozessnamen (lowercase, `.exe`-Suffix entfernt).
    process_names: HashSet<String>,

    /// Original-Konfigurationsmuster für Titel, parallel zu `title_set`
    /// (Index in `title_set.matches()` referenziert diesen Vec).
    title_patterns: Vec<String>,
    title_set: GlobSet,

    /// Original-Konfigurationsmuster für URLs, parallel zu `url_set`.
    url_patterns: Vec<String>,
    url_set: GlobSet,
}

impl Blacklist {
    /// Baut eine neue Blacklist aus drei Musterlisten.
    ///
    /// - `process_names`: exakte Prozessnamen, case-insensitive, `.exe`
    ///   optional (z. B. "chrome.exe" oder "chrome" matchen beide dieselbe
    ///   App).
    /// - `title_patterns` / `url_patterns`: Glob-Muster mit `*`-Wildcard,
    ///   case-insensitive (z. B. `*Bank*`, `*://*.bank.example/*`).
    ///
    /// Ungültige Glob-Muster werden übersprungen (nicht panicked) — eine
    /// kaputte Config-Zeile darf den gesamten Filter nicht lahmlegen. Leere
    /// Listen führen dazu, dass die jeweilige Kategorie nie matcht.
    pub fn new(
        process_names: Vec<String>,
        title_patterns: Vec<String>,
        url_patterns: Vec<String>,
    ) -> Blacklist {
        let process_names = process_names
            .iter()
            .map(|p| normalize_process_name(p))
            .filter(|p| !p.is_empty())
            .collect();

        let (title_patterns, title_set) = build_glob_set(title_patterns);
        let (url_patterns, url_set) = build_glob_set(url_patterns);

        Blacklist {
            process_names,
            title_patterns,
            title_set,
            url_patterns,
            url_set,
        }
    }

    /// True, wenn irgendeine Liste matcht (Prozessname ODER Titel ODER
    /// URL). `title`/`url` sind optional, da nicht jedes Event beides hat
    /// (z. B. Nicht-Browser-Fenster haben keine URL).
    pub fn is_blocked(&self, process_name: &str, title: Option<&str>, url: Option<&str>) -> bool {
        self.reason(process_name, title, url).is_some()
    }

    /// Wie `is_blocked`, liefert aber zusätzlich den Grund (welches
    /// konfigurierte Muster gegriffen hat) für Debug-Zwecke. Loggt dabei
    /// nie den tatsächlich erfassten Titel/URL/Prozessnamen — nur das
    /// Muster aus der Konfiguration.
    pub fn reason(
        &self,
        process_name: &str,
        title: Option<&str>,
        url: Option<&str>,
    ) -> Option<BlockReason> {
        if !self.process_names.is_empty() {
            let normalized = normalize_process_name(process_name);
            if self.process_names.contains(&normalized) {
                return Some(BlockReason::ProcessName {
                    pattern: normalized,
                });
            }
        }

        if let Some(title) = title {
            if let Some(idx) = self.title_set.matches(title).into_iter().next() {
                return Some(BlockReason::TitlePattern {
                    pattern: self.title_patterns[idx].clone(),
                });
            }
        }

        if let Some(url) = url {
            if let Some(idx) = self.url_set.matches(url).into_iter().next() {
                return Some(BlockReason::UrlPattern {
                    pattern: self.url_patterns[idx].clone(),
                });
            }
        }

        None
    }
}

/// Normalisiert einen Prozessnamen für den Vergleich: lowercase, optionales
/// `.exe`-Suffix entfernt. Damit matchen "chrome.exe" und "chrome"
/// gegeneinander.
fn normalize_process_name(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// Baut aus rohen Musterstrings ein case-insensitives `GlobSet`. Muster, die
/// syntaktisch ungültig sind, werden verworfen statt die ganze Blacklist zu
/// zerstören. Gibt die (gefilterte, in Reihenfolge erhaltene) Musterliste
/// sowie das kompilierte Set zurück; der Index in `GlobSet::matches`
/// entspricht dem Index in der zurückgegebenen Musterliste.
fn build_glob_set(patterns: Vec<String>) -> (Vec<String>, GlobSet) {
    let mut kept = Vec::with_capacity(patterns.len());
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        if pattern.trim().is_empty() {
            continue;
        }
        match compile_glob(&pattern) {
            Ok(glob) => {
                builder.add(glob);
                kept.push(pattern);
            }
            Err(_) => {
                // Ungültiges Muster ignorieren statt zu panicen; die
                // Config-Crate ist für Validierung/Nutzer-Feedback
                // zuständig, hier zählt Robustheit.
                continue;
            }
        }
    }

    let set = builder.build().unwrap_or_else(|_| {
        // Sollte praktisch nie passieren, da nur erfolgreich kompilierte
        // Globs hinzugefügt wurden. Fallback: leeres Set (matcht nie).
        GlobSetBuilder::new().build().expect("empty glob set")
    });

    (kept, set)
}

fn compile_glob(pattern: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .literal_separator(false)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Blacklist {
        Blacklist::new(vec![], vec![], vec![])
    }

    // ---- Leere Blacklist ----

    #[test]
    fn empty_blacklist_blocks_nothing() {
        let bl = empty();
        assert!(!bl.is_blocked("chrome.exe", Some("Meine Bank"), Some("https://x.bank.example/")));
        assert!(!bl.is_blocked("anything.exe", None, None));
        assert!(bl.reason("chrome.exe", Some("Meine Bank"), Some("https://x.bank.example/")).is_none());
    }

    // ---- Prozessname ----

    #[test]
    fn process_name_exact_match_blocks() {
        let bl = Blacklist::new(vec!["chrome.exe".to_string()], vec![], vec![]);
        assert!(bl.is_blocked("chrome.exe", None, None));
    }

    #[test]
    fn process_name_no_match_does_not_block() {
        let bl = Blacklist::new(vec!["chrome.exe".to_string()], vec![], vec![]);
        assert!(!bl.is_blocked("firefox.exe", None, None));
    }

    #[test]
    fn process_name_case_insensitive() {
        let bl = Blacklist::new(vec!["Chrome.EXE".to_string()], vec![], vec![]);
        assert!(bl.is_blocked("CHROME.exe", None, None));
        assert!(bl.is_blocked("chrome.exe", None, None));
    }

    #[test]
    fn process_name_exe_suffix_tolerated_config_without_exe() {
        // Config ohne .exe, Input mit .exe.
        let bl = Blacklist::new(vec!["chrome".to_string()], vec![], vec![]);
        assert!(bl.is_blocked("chrome.exe", None, None));
    }

    #[test]
    fn process_name_exe_suffix_tolerated_input_without_exe() {
        // Config mit .exe, Input ohne .exe.
        let bl = Blacklist::new(vec!["chrome.exe".to_string()], vec![], vec![]);
        assert!(bl.is_blocked("chrome", None, None));
    }

    #[test]
    fn process_name_partial_name_does_not_match() {
        // "chrome" darf nicht "chromedriver.exe" matchen (exakter Match, kein Substring).
        let bl = Blacklist::new(vec!["chrome.exe".to_string()], vec![], vec![]);
        assert!(!bl.is_blocked("chromedriver.exe", None, None));
    }

    // ---- Titel ----

    #[test]
    fn title_glob_match_blocks() {
        let bl = Blacklist::new(vec![], vec!["*Bank*".to_string()], vec![]);
        assert!(bl.is_blocked("firefox.exe", Some("Meine Bank - Online-Banking"), None));
    }

    #[test]
    fn title_glob_no_match_does_not_block() {
        let bl = Blacklist::new(vec![], vec!["*Bank*".to_string()], vec![]);
        assert!(!bl.is_blocked("firefox.exe", Some("Wikipedia"), None));
    }

    #[test]
    fn title_glob_case_insensitive() {
        let bl = Blacklist::new(vec![], vec!["*bank*".to_string()], vec![]);
        assert!(bl.is_blocked("firefox.exe", Some("MEINE BANK"), None));
    }

    #[test]
    fn title_none_does_not_block_even_with_patterns() {
        let bl = Blacklist::new(vec![], vec!["*Bank*".to_string()], vec![]);
        assert!(!bl.is_blocked("firefox.exe", None, None));
    }

    // ---- URL ----

    #[test]
    fn url_glob_match_blocks() {
        let bl = Blacklist::new(vec![], vec![], vec!["*://*.bank.example/*".to_string()]);
        assert!(bl.is_blocked("chrome.exe", None, Some("https://secure.bank.example/login")));
    }

    #[test]
    fn url_glob_no_match_does_not_block() {
        let bl = Blacklist::new(vec![], vec![], vec!["*://*.bank.example/*".to_string()]);
        assert!(!bl.is_blocked("chrome.exe", None, Some("https://example.com/")));
    }

    #[test]
    fn url_glob_case_insensitive() {
        let bl = Blacklist::new(vec![], vec![], vec!["*://*.BANK.example/*".to_string()]);
        assert!(bl.is_blocked("chrome.exe", None, Some("HTTPS://SECURE.bank.EXAMPLE/login")));
    }

    // ---- Kombination / reason() ----

    #[test]
    fn any_list_matching_blocks() {
        let bl = Blacklist::new(
            vec!["chrome.exe".to_string()],
            vec!["*Bank*".to_string()],
            vec!["*://*.bank.example/*".to_string()],
        );
        // Nur der Titel matcht, Prozess und URL nicht.
        assert!(bl.is_blocked("firefox.exe", Some("Online-Bank"), Some("https://example.com/")));
    }

    #[test]
    fn reason_reports_process_name_pattern_not_captured_content() {
        let bl = Blacklist::new(vec!["chrome.exe".to_string()], vec![], vec![]);
        let reason = bl.reason("Chrome.EXE", None, None);
        assert_eq!(
            reason,
            Some(BlockReason::ProcessName {
                pattern: "chrome".to_string()
            })
        );
    }

    #[test]
    fn reason_reports_title_pattern() {
        let bl = Blacklist::new(vec![], vec!["*Bank*".to_string()], vec![]);
        let reason = bl.reason("firefox.exe", Some("Meine Bank"), None);
        assert_eq!(
            reason,
            Some(BlockReason::TitlePattern {
                pattern: "*Bank*".to_string()
            })
        );
    }

    #[test]
    fn reason_reports_url_pattern() {
        let bl = Blacklist::new(vec![], vec![], vec!["*://*.bank.example/*".to_string()]);
        let reason = bl.reason("chrome.exe", None, Some("https://x.bank.example/"));
        assert_eq!(
            reason,
            Some(BlockReason::UrlPattern {
                pattern: "*://*.bank.example/*".to_string()
            })
        );
    }

    #[test]
    fn empty_pattern_strings_are_ignored() {
        let bl = Blacklist::new(
            vec!["".to_string(), "  ".to_string()],
            vec!["".to_string()],
            vec!["".to_string()],
        );
        assert!(!bl.is_blocked("", Some(""), Some("")));
    }

    #[test]
    fn invalid_glob_pattern_is_skipped_not_panicking() {
        // Unausgewogene Klammer ist in globset ein Fehler; darf nicht
        // panicen, sondern wird beim Bauen einfach ignoriert.
        let bl = Blacklist::new(vec![], vec!["[".to_string()], vec![]);
        assert!(!bl.is_blocked("firefox.exe", Some("["), None));
    }
}
