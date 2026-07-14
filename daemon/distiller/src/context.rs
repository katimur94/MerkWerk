//! Builds compact text context for the distillation prompt out of loaded
//! sessions/snapshots, respecting the budgets in [`crate::DistillerConfig`].

use crate::DistillerConfig;

/// Compact, intermediate representation of one captured snapshot, ready to
/// be rendered into prompt context text by [`build_context`].
///
/// This sits between the raw storage rows (`storage::AppSessionRow` /
/// `storage::SnapshotRow`) and the flat prompt text: [`crate::distill`]
/// builds one `SessionContext` per (session, snapshot) pair it selects,
/// carrying the parent session's process name alongside that snapshot's
/// own time/title/URL/text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    /// Local clock time ("HH:MM") the snapshot was taken at, see
    /// [`ms_to_hh_mm`].
    pub time: String,
    /// Process name of the owning app session (e.g. `"chrome.exe"`).
    pub process_name: String,
    /// Window title captured with the snapshot, if any.
    pub window_title: Option<String>,
    /// URL captured with the snapshot (browser tabs only), if any.
    pub url: Option<String>,
    /// Snapshot text content, not yet truncated — [`build_context`] applies
    /// `max_chars_per_snapshot`.
    pub text_excerpt: Option<String>,
}

const MS_PER_DAY: i64 = 24 * 60 * 60 * 1000;
const MS_PER_HOUR: i64 = 60 * 60 * 1000;
const MS_PER_MINUTE: i64 = 60 * 1000;

/// Formats Unix milliseconds as a "HH:MM" clock time using plain modulo
/// arithmetic on the millisecond-of-day — no `chrono` dependency, so this
/// stays a pure, natively-testable function (`docs/ROADMAP.md`
/// "Realitäts-Hinweis").
///
/// `ts_ms` is reduced modulo one day (via `rem_euclid`, so any stray
/// negative input still lands in range) with no timezone conversion: good
/// enough for a human-readable label in the prompt context, not a
/// general-purpose time formatter.
pub fn ms_to_hh_mm(ts_ms: i64) -> String {
    let ms_of_day = ts_ms.rem_euclid(MS_PER_DAY);
    let hours = ms_of_day / MS_PER_HOUR;
    let minutes = (ms_of_day % MS_PER_HOUR) / MS_PER_MINUTE;
    format!("{hours:02}:{minutes:02}")
}

/// Truncates `s` to at most `max_chars` Unicode scalar values (`char`s),
/// always cutting at a char boundary so it never panics on multi-byte
/// UTF-8 (mirrors the char-safety `ENTSCHEIDUNGEN.md` D5 already requires
/// at the snapshot-capture layer, applied here again for prompt budgets).
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

/// Renders one context line, e.g.:
/// `- 08:15 chrome.exe — Weekly Report (https://example.com): quarterly numbers...`
/// Missing optional fields (title/url/text) are simply omitted. Always ends
/// in `\n`. `max_chars_per_snapshot` is applied to the text excerpt here,
/// char-safely (see [`truncate_chars`]).
fn render_entry(entry: &SessionContext, max_chars_per_snapshot: usize) -> String {
    let mut line = String::new();
    line.push_str("- ");
    line.push_str(&entry.time);
    line.push(' ');
    line.push_str(&entry.process_name);

    if let Some(title) = entry.window_title.as_deref().filter(|t| !t.is_empty()) {
        line.push_str(" — ");
        line.push_str(title);
    }
    if let Some(url) = entry.url.as_deref().filter(|u| !u.is_empty()) {
        line.push_str(" (");
        line.push_str(url);
        line.push(')');
    }
    if let Some(text) = entry.text_excerpt.as_deref().filter(|t| !t.is_empty()) {
        let truncated = truncate_chars(text, max_chars_per_snapshot);
        if !truncated.is_empty() {
            line.push_str(": ");
            line.push_str(truncated);
        }
    }
    line.push('\n');
    line
}

/// Builds a compact, line-per-snapshot text context for
/// [`crate::build_prompt`] out of `sessions`, respecting every budget in
/// `cfg`:
/// - at most `cfg.max_snapshots` entries are rendered at all (callers,
///   i.e. [`crate::distill`], are expected to already have ordered
///   `sessions` chronologically and pre-selected the most relevant/recent
///   ones — this is the final, authoritative cap);
/// - each entry's snapshot text is truncated to `cfg.max_chars_per_snapshot`
///   characters, at a char boundary (see [`truncate_chars`]);
/// - the *total* returned string never exceeds `cfg.max_total_context_chars`
///   characters — if adding the next entry in full would overflow that cap,
///   rendering stops after fitting as much of that one entry as still fits
///   (again truncated at a char boundary) rather than exceeding the cap.
pub fn build_context(sessions: &[SessionContext], cfg: &DistillerConfig) -> String {
    let mut out = String::new();
    let mut total_chars = 0usize;

    for entry in sessions.iter().take(cfg.max_snapshots) {
        let line = render_entry(entry, cfg.max_chars_per_snapshot);
        let line_chars = line.chars().count();

        if total_chars + line_chars > cfg.max_total_context_chars {
            let remaining = cfg.max_total_context_chars.saturating_sub(total_chars);
            if remaining > 0 {
                out.push_str(truncate_chars(&line, remaining));
            }
            break;
        }

        out.push_str(&line);
        total_chars += line_chars;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(
        max_snapshots: usize,
        max_chars_per_snapshot: usize,
        max_total_context_chars: usize,
    ) -> DistillerConfig {
        DistillerConfig {
            model: "llama3.1".to_string(),
            max_snapshots,
            max_chars_per_snapshot,
            max_total_context_chars,
        }
    }

    fn entry(time: &str, process: &str, text: &str) -> SessionContext {
        SessionContext {
            time: time.to_string(),
            process_name: process.to_string(),
            window_title: None,
            url: None,
            text_excerpt: Some(text.to_string()),
        }
    }

    // ---- ms_to_hh_mm -----------------------------------------------------

    #[test]
    fn ms_to_hh_mm_midnight() {
        assert_eq!(ms_to_hh_mm(0), "00:00");
    }

    #[test]
    fn ms_to_hh_mm_one_hour() {
        assert_eq!(ms_to_hh_mm(3_600_000), "01:00");
    }

    #[test]
    fn ms_to_hh_mm_23_00() {
        assert_eq!(ms_to_hh_mm(82_800_000), "23:00");
    }

    #[test]
    fn ms_to_hh_mm_ignores_which_day_only_ms_of_day_matters() {
        let one_day = MS_PER_DAY;
        assert_eq!(ms_to_hh_mm(one_day + 90 * MS_PER_MINUTE), "01:30");
    }

    // ---- truncate_chars ----------------------------------------------------

    #[test]
    fn truncate_chars_keeps_short_strings_untouched() {
        assert_eq!(truncate_chars("hi", 10), "hi");
    }

    #[test]
    fn truncate_chars_cuts_at_exact_char_count() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
    }

    #[test]
    fn truncate_chars_is_multibyte_safe() {
        // 'ä'/'ö' are 2-byte, '€' is 3-byte in UTF-8: a byte-index
        // truncation could panic (mid-character split) or corrupt the
        // string; char_indices-based truncation must not, for every cut
        // point from 0 up past the string's own length.
        let text = "hällo wörld €€€";
        let char_count = text.chars().count();
        for n in 0..=char_count + 3 {
            let truncated = truncate_chars(text, n);
            assert_eq!(truncated.chars().count(), n.min(char_count));
        }
    }

    // ---- build_context: rendering ------------------------------------------

    #[test]
    fn build_context_renders_all_fields_when_present() {
        let entries = vec![SessionContext {
            time: "08:15".to_string(),
            process_name: "chrome.exe".to_string(),
            window_title: Some("Weekly Report".to_string()),
            url: Some("https://example.com".to_string()),
            text_excerpt: Some("quarterly numbers".to_string()),
        }];
        let cfg = DistillerConfig::default();

        let context = build_context(&entries, &cfg);

        assert_eq!(
            context,
            "- 08:15 chrome.exe — Weekly Report (https://example.com): quarterly numbers\n"
        );
    }

    #[test]
    fn build_context_empty_input_yields_empty_string() {
        let cfg = DistillerConfig::default();
        assert_eq!(build_context(&[], &cfg), "");
    }

    // ---- build_context: max_snapshots ---------------------------------------

    #[test]
    fn build_context_respects_max_snapshots() {
        let entries: Vec<SessionContext> = (0..10)
            .map(|i| entry(&format!("{i:02}:00"), "app.exe", "text"))
            .collect();
        let cfg = cfg_with(3, 800, 12_000);

        let context = build_context(&entries, &cfg);

        assert_eq!(context.lines().count(), 3);
        assert!(context.contains("02:00"));
        assert!(
            !context.contains("03:00"),
            "must stop after max_snapshots entries"
        );
    }

    // ---- build_context: max_chars_per_snapshot ------------------------------

    #[test]
    fn build_context_truncates_snapshot_text_char_safely() {
        let text = "ä".repeat(50); // 50 chars, 100 bytes
        let entries = vec![entry("08:00", "app.exe", &text)];
        let cfg = cfg_with(60, 10, 12_000);

        let context = build_context(&entries, &cfg);

        let kept = "ä".repeat(10);
        let one_too_many = "ä".repeat(11);
        assert!(context.contains(&kept));
        assert!(!context.contains(&one_too_many));
    }

    // ---- build_context: max_total_context_chars -----------------------------

    #[test]
    fn build_context_never_exceeds_total_char_budget() {
        let entries: Vec<SessionContext> = (0..100)
            .map(|i| entry(&format!("{:02}:00", i % 24), "app.exe", &"x".repeat(100)))
            .collect();
        let cfg = cfg_with(1000, 1000, 500);

        let context = build_context(&entries, &cfg);

        assert!(context.chars().count() <= 500);
        assert!(!context.is_empty());
    }

    #[test]
    fn build_context_total_cap_cuts_mid_multibyte_run_without_panicking() {
        // Each rendered line is 68 chars: 17 ASCII prefix chars
        // ("- 08:00 app.exe: ") + 50 '€' + a trailing '\n'. A budget of
        // 108 = 68 (first line, kept whole) + 40 (second line, cut after
        // its 23rd '€' — squarely inside the multi-byte run) forces the
        // cap logic to slice through a run of 3-byte characters.
        let entries: Vec<SessionContext> = (0..3)
            .map(|_| entry("08:00", "app.exe", &"€".repeat(50)))
            .collect();
        let cfg = cfg_with(1000, 1000, 108);

        let context = build_context(&entries, &cfg);

        assert_eq!(context.chars().count(), 108);
        assert_eq!(context.chars().filter(|&c| c == '€').count(), 50 + 23);
        assert_eq!(context.lines().count(), 2);
    }
}
