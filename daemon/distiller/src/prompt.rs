//! Builds the final prompt string handed to `inference::Inference::generate`.

/// Builds the full prompt for the local model: a German system/instruction
/// preamble telling it to distill the following work context into a
/// concise, structured Markdown note, followed by `context` itself
/// (typically produced by [`crate::build_context`]).
///
/// Pure string formatting — no I/O, so this is natively testable without a
/// running inference backend (`ENTSCHEIDUNGEN.md` D9).
pub fn build_prompt(context: &str) -> String {
    let instructions = [
        "Du bist ein lokal laufender Assistent für persönliche Notizen. Deine Aufgabe: Destilliere den folgenden Arbeitskontext (Fensterwechsel, Apps, Seitentitel, URLs und erfasste Textausschnitte eines Zeitraums) zu einer knappen, strukturierten Markdown-Notiz.",
        "",
        "Regeln:",
        "1. Beginne mit genau einer Überschrift der Form \"# ...\", die den Zeitraum oder Schwerpunkt in wenigen Worten benennt.",
        "2. Fasse danach in Stichpunkten (\"- \") zusammen, welche Tätigkeiten, Apps/Programme und Themen im Arbeitskontext vorkommen.",
        "3. Sei knapp: verdichte, wiederhole den Rohkontext nicht wörtlich.",
        "4. Erfinde nichts hinzu. Verwende ausschließlich Informationen, die tatsächlich im Arbeitskontext unten stehen. Ist etwas unklar oder fehlt es, lass es weg, statt zu spekulieren.",
        "5. Antworte ausschließlich mit der fertigen Markdown-Notiz, ohne Einleitung, Erklärung oder Codeblock drumherum.",
        "",
        "Arbeitskontext:",
    ]
    .join("\n");

    format!("{instructions}\n{context}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_embeds_the_context_verbatim() {
        let context = "- 08:15 chrome.exe — Weekly Report (https://example.com): quarterly numbers";
        let prompt = build_prompt(context);

        assert!(
            prompt.contains(context),
            "prompt must embed the raw context verbatim: {prompt:?}"
        );
    }

    #[test]
    fn build_prompt_contains_the_distillation_instruction() {
        let prompt = build_prompt("irrelevant context");

        assert!(prompt.contains("Markdown-Notiz"));
        assert!(prompt.contains("Überschrift"));
        assert!(prompt.contains("Stichpunkten"));
        assert!(prompt.contains("Erfinde nichts hinzu"));
        assert!(prompt.contains("Arbeitskontext:"));
    }

    #[test]
    fn build_prompt_with_empty_context_still_has_instruction() {
        let prompt = build_prompt("");
        assert!(prompt.contains("Markdown-Notiz"));
        assert!(prompt.ends_with("Arbeitskontext:\n\n"));
    }
}
