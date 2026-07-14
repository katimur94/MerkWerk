//! Tunable knobs ([`DistillerConfig`]) and the output type
//! ([`DistilledNote`]) for [`crate::distill`].

use serde::{Deserialize, Serialize};

/// Tunable limits for [`crate::distill`]: which local model to call, and
/// how much of the captured material is allowed into the prompt context
/// built by [`crate::build_context`].
///
/// `max_snapshots` caps how many individual snapshots are considered at
/// all; `max_chars_per_snapshot` and `max_total_context_chars` keep the
/// resulting prompt within a size the local model can handle in reasonable
/// time (`docs/ROADMAP.md` Etappe 2, `ENTSCHEIDUNGEN.md` D9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillerConfig {
    /// Ollama model name (`ENTSCHEIDUNGEN.md` D9), e.g. `"llama3.1"`.
    pub model: String,
    /// Maximum number of snapshots considered across the whole requested
    /// time range.
    pub max_snapshots: usize,
    /// Maximum number of characters kept from each snapshot's text
    /// excerpt, after char-safe truncation (see [`crate::truncate_chars`]).
    pub max_chars_per_snapshot: usize,
    /// Hard cap on the total length (in characters) of the context text
    /// [`crate::build_context`] hands to the model.
    pub max_total_context_chars: usize,
}

impl Default for DistillerConfig {
    fn default() -> Self {
        Self {
            model: "llama3.1".to_string(),
            max_snapshots: 60,
            max_chars_per_snapshot: 800,
            max_total_context_chars: 12_000,
        }
    }
}

/// Result of [`crate::distill`]: a Markdown note synthesized from a time
/// range of captured sessions/snapshots by a local model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistilledNote {
    /// Heading text extracted from the model's first `# ...` Markdown
    /// line, if any (see the private `extract_title` helper).
    pub title: Option<String>,
    /// The full Markdown note body, as produced by the model.
    pub markdown: String,
    /// Number of source snapshots the note was distilled from.
    pub source_snapshot_count: usize,
    /// Unix milliseconds: start of the requested time range.
    pub range_start: i64,
    /// Unix milliseconds: end of the requested time range.
    pub range_end: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_documented_defaults() {
        let cfg = DistillerConfig::default();
        assert_eq!(cfg.model, "llama3.1");
        assert_eq!(cfg.max_snapshots, 60);
        assert_eq!(cfg.max_chars_per_snapshot, 800);
        assert_eq!(cfg.max_total_context_chars, 12_000);
    }
}
