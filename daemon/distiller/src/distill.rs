//! Orchestration: loads sessions/snapshots for a time range, builds prompt
//! context, calls into `Inference`, and packages a [`DistilledNote`].

use crate::context::{build_context, ms_to_hh_mm, SessionContext};
use crate::prompt::build_prompt;
use crate::{DistilledNote, DistillerConfig, Result};

/// Markdown returned for a time range with no captured activity at all
/// (see [`distill`]'s doc comment for why this skips calling `inference`).
const NO_ACTIVITY_MARKDOWN: &str = "_Keine Aktivität in diesem Zeitraum._";

/// Extracts the note title from the model's Markdown output: the text of
/// the first line that is a top-level heading (`"# ..."`), trimmed. `None`
/// if there is no such line, or its heading text is empty.
fn extract_title(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        line.trim()
            .strip_prefix("# ")
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
    })
}

/// Distills the captured sessions/snapshots in `[from_ms, to_ms]` into a
/// Markdown note, using `inference` for the actual summarization
/// (`docs/ROADMAP.md` Etappe 2, "Destillierer").
///
/// Steps:
/// 1. Load `app_sessions` overlapping the range (`Store::sessions_between`).
/// 2. For each session, load its snapshots and keep only the most recent
///    `cfg.max_snapshots` of them ("jüngste/relevante") — a generous
///    per-session ceiling (never above the overall budget) so one very
///    long session cannot alone starve every other session's snapshots out
///    of [`build_context`]'s own global cap.
/// 3. Render everything into compact text context ([`build_context`]),
///    embed it in the distillation prompt ([`build_prompt`]), and call
///    `inference.generate`.
/// 4. Derive `title` from the model's first `"# ..."` heading, if any.
///
/// If there are no sessions at all in the range, `inference` is *not*
/// called: [`distill`] returns `Ok` with a short, fixed "no activity" note
/// and `source_snapshot_count: 0` instead. There is nothing for a model to
/// summarize in that case, and skipping the call avoids a pointless
/// round-trip to a local model that may be slow to load.
pub fn distill(
    store: &storage::Store,
    inference: &dyn inference::Inference,
    cfg: &DistillerConfig,
    from_ms: i64,
    to_ms: i64,
) -> Result<DistilledNote> {
    let sessions = store.sessions_between(from_ms, to_ms)?;

    if sessions.is_empty() {
        return Ok(DistilledNote {
            title: None,
            markdown: NO_ACTIVITY_MARKDOWN.to_string(),
            source_snapshot_count: 0,
            range_start: from_ms,
            range_end: to_ms,
        });
    }

    let mut entries: Vec<SessionContext> = Vec::new();
    for session in &sessions {
        let snapshots = store.snapshots_for_session(session.id)?;
        let start = snapshots.len().saturating_sub(cfg.max_snapshots);
        for snapshot in &snapshots[start..] {
            entries.push(SessionContext {
                time: ms_to_hh_mm(snapshot.ts),
                process_name: session.process_name.clone(),
                window_title: snapshot.window_title.clone(),
                url: snapshot.url.clone(),
                text_excerpt: snapshot.text_content.clone(),
            });
        }
    }

    // Mirrors the global cap `build_context` itself applies via
    // `cfg.max_snapshots`, so this reflects what was actually selected for
    // the prompt (the char budgets may trim the *rendered text* further,
    // but do not change how many snapshots were selected as input).
    let source_snapshot_count = entries.len().min(cfg.max_snapshots);

    let context = build_context(&entries, cfg);
    let prompt = build_prompt(&context);
    let markdown = inference.generate(&prompt)?;
    let title = extract_title(&markdown);

    Ok(DistilledNote {
        title,
        markdown,
        source_snapshot_count,
        range_start: from_ms,
        range_end: to_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inference::MockInference;
    use storage::Store;

    /// Two sessions: `chrome.exe` with two snapshots, `code.exe` with one.
    fn seeded_store() -> Store {
        let store = Store::open_in_memory().expect("open in-memory store");

        let session_a = store
            .insert_app_session("chrome.exe", 1_000, Some(5_000))
            .unwrap();
        store
            .insert_snapshot(
                Some(session_a),
                None,
                1_500,
                Some("Weekly Report — Draft"),
                Some("https://docs.example.com/report"),
                Some("quarterly numbers look good"),
                false,
                None,
            )
            .unwrap();
        store
            .insert_snapshot(
                Some(session_a),
                None,
                2_500,
                Some("Weekly Report — Draft"),
                Some("https://docs.example.com/report"),
                Some("added the summary section"),
                false,
                None,
            )
            .unwrap();

        let session_b = store.insert_app_session("code.exe", 3_000, None).unwrap();
        store
            .insert_snapshot(
                Some(session_b),
                None,
                3_200,
                Some("distill.rs — MerkWerk"),
                None,
                Some("implementing the distiller crate"),
                false,
                None,
            )
            .unwrap();

        store
    }

    #[test]
    fn distill_returns_mock_markdown_title_and_snapshot_count() {
        let store = seeded_store();
        let inference =
            MockInference::with_response("# Titel\n\n- Arbeit an Report\n- Code geschrieben\n");
        let cfg = DistillerConfig::default();

        let note = distill(&store, &inference, &cfg, 0, 10_000).expect("distill");

        assert_eq!(
            note.markdown,
            "# Titel\n\n- Arbeit an Report\n- Code geschrieben\n"
        );
        assert_eq!(note.title.as_deref(), Some("Titel"));
        assert_eq!(note.source_snapshot_count, 3);
        assert_eq!(note.range_start, 0);
        assert_eq!(note.range_end, 10_000);
    }

    #[test]
    fn distill_without_h1_heading_yields_no_title() {
        let store = seeded_store();
        let inference = MockInference::with_response("no heading here, just prose");
        let cfg = DistillerConfig::default();

        let note = distill(&store, &inference, &cfg, 0, 10_000).expect("distill");

        assert_eq!(note.title, None);
        assert_eq!(note.markdown, "no heading here, just prose");
    }

    #[test]
    fn distill_on_empty_range_skips_inference_and_returns_no_activity_note() {
        let store = Store::open_in_memory().unwrap();
        let inference = MockInference::with_response("should never be called");
        let cfg = DistillerConfig::default();

        let note = distill(&store, &inference, &cfg, 0, 10_000).expect("distill");

        assert_eq!(note.source_snapshot_count, 0);
        assert_eq!(note.title, None);
        assert_eq!(note.markdown, NO_ACTIVITY_MARKDOWN);
        assert_ne!(
            note.markdown, "should never be called",
            "inference::generate must not be called for an empty range"
        );
        assert_eq!(note.range_start, 0);
        assert_eq!(note.range_end, 10_000);
    }

    #[test]
    fn distill_respects_max_snapshots_budget_in_source_count() {
        let store = seeded_store(); // 3 snapshots total across 2 sessions
        let inference = MockInference::new();
        let cfg = DistillerConfig {
            max_snapshots: 2,
            ..DistillerConfig::default()
        };

        let note = distill(&store, &inference, &cfg, 0, 10_000).expect("distill");

        assert_eq!(note.source_snapshot_count, 2);
    }

    // ---- extract_title -----------------------------------------------------

    #[test]
    fn extract_title_reads_first_h1_heading() {
        assert_eq!(extract_title("# Hello\nbody").as_deref(), Some("Hello"));
    }

    #[test]
    fn extract_title_ignores_h2_and_returns_none_without_h1() {
        assert_eq!(extract_title("## Not h1\nbody"), None);
    }

    #[test]
    fn extract_title_skips_leading_blank_lines() {
        assert_eq!(
            extract_title("\n\n# Title here\nbody").as_deref(),
            Some("Title here")
        );
    }

    #[test]
    fn extract_title_returns_none_for_markdown_without_heading() {
        assert_eq!(extract_title("just text, no heading"), None);
    }
}
