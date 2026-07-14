//! Plattformneutrale Text-Budget- und Dedup-Logik für den UIA-Snapshotter.
//!
//! Diese Funktionen tragen keinerlei Windows-/COM-Bezug und sind bewusst aus
//! [`crate::uia`] herausgelöst, damit sie **unbedingt** (auch auf einem
//! Nicht-Windows-Host in der CI/Sandbox) kompiliert und mit `cargo test`
//! ausgeführt werden — die Budget-/UTF-8-Abschneide-Logik ist genau die Art
//! subtiler Logik, die einen echten Testlauf verdient (siehe ENTSCHEIDUNGEN.md
//! D5). `uia.rs` (Windows-only) importiert von hier.

/// Grenzwerte für einen einzelnen UIA-Snapshot (siehe ENTSCHEIDUNGEN.md D5).
///
/// Bewusst hier (plattformneutral) statt in der `config`-Crate definiert — das
/// hielte die Windows-Erfassung frei von einer Abhängigkeit auf die neutrale
/// `config`-Crate und vermeidet Kopplung zwischen den Workspace-Crates
/// (ENTSCHEIDUNGEN.md D6). Der Daemon (der beide kennt) mappt
/// `config::SnapshotConfig` beim Snapshot-Aufruf 1:1 hierauf — die Feldnamen
/// sind absichtlich identisch, damit das Mapping trivial bleibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotConfig {
    /// Maximale Größe von `Snapshot.text_content` in Bytes (D5: 20 KB Default).
    pub max_text_bytes: usize,
    /// Maximale Tiefe beim UIA-TreeWalk.
    pub max_tree_depth: u32,
    /// Maximale Anzahl besuchter UIA-Knoten pro Snapshot.
    pub max_nodes: u32,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_text_bytes: 20 * 1024,
            max_tree_depth: 40,
            max_nodes: 4000,
        }
    }
}

/// Hängt `add` an `buf` an, sofern das Byte-Budget `max_bytes` das zulässt — sonst
/// wird an der letzten gültigen UTF-8-Zeichengrenze `<= verbleibendes Budget`
/// abgeschnitten.
///
/// Gibt zurück, ob durch diesen Aufruf (neu) abgeschnitten wurde bzw. das Budget
/// bereits vorher voll war.
pub fn push_capped(buf: &mut String, add: &str, max_bytes: usize) -> bool {
    if buf.len() >= max_bytes {
        return true;
    }
    let remaining = max_bytes - buf.len();
    if add.len() <= remaining {
        buf.push_str(add);
        false
    } else {
        let mut cut = remaining;
        while cut > 0 && !add.is_char_boundary(cut) {
            cut -= 1;
        }
        buf.push_str(&add[..cut]);
        true
    }
}

/// Liefert Name und Value als Liste eindeutiger, nicht-leerer Kandidaten für
/// denselben Knoten: `value` wird verworfen, wenn es exakt `name` entspricht
/// (häufig bei einfachen Controls, deren Legacy-Value die Name-Eigenschaft
/// spiegelt) — vermeidet den offensichtlichsten Duplikat-Fall an der Quelle.
pub fn dedup_pair(name: Option<String>, value: Option<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    if let Some(n) = name {
        out.push(n);
    }
    if let Some(v) = value {
        if out.first() != Some(&v) {
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_append_under_budget() {
        let mut buf = String::new();
        let truncated = push_capped(&mut buf, "hello", 100);
        assert_eq!(buf, "hello");
        assert!(!truncated);
    }

    #[test]
    fn appends_accumulate_across_multiple_calls_under_budget() {
        let mut buf = String::new();
        assert!(!push_capped(&mut buf, "foo", 100));
        assert!(!push_capped(&mut buf, "bar", 100));
        assert_eq!(buf, "foobar");
    }

    #[test]
    fn append_exactly_at_budget_is_not_truncated() {
        let mut buf = String::new();
        let truncated = push_capped(&mut buf, "12345", 5);
        assert_eq!(buf, "12345");
        assert_eq!(buf.len(), 5);
        assert!(!truncated);
    }

    #[test]
    fn further_append_after_exact_budget_short_circuits_without_mutation() {
        let mut buf = String::new();
        assert!(!push_capped(&mut buf, "12345", 5));
        let truncated = push_capped(&mut buf, "more", 5);
        assert_eq!(buf, "12345", "buffer must stay unchanged once budget is exhausted");
        assert!(truncated);
    }

    #[test]
    fn overflow_truncates_plain_ascii_at_budget_boundary() {
        let mut buf = String::new();
        let truncated = push_capped(&mut buf, "abcdefgh", 3);
        assert_eq!(buf, "abc");
        assert!(truncated);
    }

    #[test]
    fn overflow_cuts_back_to_previous_char_boundary_no_panic() {
        // '€' ist 3 Bytes (E2 82 AC) in UTF-8. "ab€c" = a(1) b(1) €(3) c(1) = 6 Bytes.
        // Byte-Offset 4 (max_bytes) liegt mitten in der '€'-Sequenz -- die Funktion
        // darf dort NICHT schneiden, sondern muss auf die vorherige gültige Grenze (2).
        let mut buf = String::new();
        let truncated = push_capped(&mut buf, "ab\u{20AC}c", 4);
        assert_eq!(buf, "ab");
        assert_eq!(buf.len(), 2);
        assert!(truncated);
    }

    #[test]
    fn overflow_at_zero_remaining_budget_produces_no_partial_char_no_panic() {
        let mut buf = String::from("xxxx");
        let truncated = push_capped(&mut buf, "\u{1F600}", 4);
        assert_eq!(buf, "xxxx");
        assert!(truncated);
    }

    #[test]
    fn empty_add_under_budget_is_noop_not_truncated() {
        let mut buf = String::from("hi");
        let truncated = push_capped(&mut buf, "", 100);
        assert_eq!(buf, "hi");
        assert!(!truncated);
    }

    #[test]
    fn zero_byte_budget_on_empty_buffer_truncates_immediately() {
        let mut buf = String::new();
        let truncated = push_capped(&mut buf, "x", 0);
        assert_eq!(buf, "");
        assert!(truncated);
    }

    #[test]
    fn dedup_pair_drops_value_equal_to_name() {
        let out = dedup_pair(Some("same".to_string()), Some("same".to_string()));
        assert_eq!(out, vec!["same".to_string()]);
    }

    #[test]
    fn dedup_pair_keeps_distinct_name_and_value() {
        let out = dedup_pair(Some("label".to_string()), Some("typed text".to_string()));
        assert_eq!(out, vec!["label".to_string(), "typed text".to_string()]);
    }

    #[test]
    fn dedup_pair_handles_missing_name_or_value() {
        assert_eq!(dedup_pair(None, Some("v".to_string())), vec!["v".to_string()]);
        assert_eq!(dedup_pair(Some("n".to_string()), None), vec!["n".to_string()]);
        assert!(dedup_pair(None, None).is_empty());
    }

    #[test]
    fn default_snapshot_config_matches_d5() {
        let cfg = SnapshotConfig::default();
        assert_eq!(cfg.max_text_bytes, 20 * 1024);
        assert_eq!(cfg.max_tree_depth, 40);
        assert_eq!(cfg.max_nodes, 4000);
    }
}
