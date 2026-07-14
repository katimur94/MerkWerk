//! No-I/O [`Inference`] implementation for tests.
//!
//! Deliberately a normal (non-`#[cfg(test)]`) module: downstream crates —
//! chiefly the future distiller (`docs/ROADMAP.md` Etappe 2) — depend on it
//! from their own test code to exercise `Inference`-consuming logic without
//! standing up a real Ollama server. That is the whole point of hiding
//! inference behind a trait (`ENTSCHEIDUNGEN.md` D9).

use crate::{Inference, Result};

/// Canonical text returned by [`MockInference::generate`] unless a custom
/// response was configured via [`MockInference::with_response`].
pub const DEFAULT_RESPONSE: &str = "mock inference response";

/// Fixed embedding vector returned by [`MockInference::embed`] unless a
/// custom vector was configured via [`MockInference::with_embedding`].
///
/// Deliberately independent of the input text — this mock exists to make
/// `Inference`-consuming code paths runnable in tests, not to simulate real
/// embedding semantics.
pub const DEFAULT_EMBEDDING: [f32; 4] = [0.1, 0.2, 0.3, 0.4];

/// [`Inference`] implementation with no I/O, for tests.
///
/// `generate` always returns the same configured (or canonical default)
/// string, regardless of the prompt; `embed` always returns the same
/// configured (or fixed default) vector, regardless of the input text.
#[derive(Debug, Clone, PartialEq)]
pub struct MockInference {
    response: String,
    embedding: Vec<f32>,
}

impl MockInference {
    /// Creates a mock returning [`DEFAULT_RESPONSE`] / [`DEFAULT_EMBEDDING`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a mock whose [`Inference::generate`] returns `response`
    /// instead of [`DEFAULT_RESPONSE`].
    pub fn with_response(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            ..Self::default()
        }
    }

    /// Creates a mock whose [`Inference::embed`] returns `embedding`
    /// instead of [`DEFAULT_EMBEDDING`].
    pub fn with_embedding(embedding: Vec<f32>) -> Self {
        Self {
            embedding,
            ..Self::default()
        }
    }
}

impl Default for MockInference {
    fn default() -> Self {
        Self {
            response: DEFAULT_RESPONSE.to_string(),
            embedding: DEFAULT_EMBEDDING.to_vec(),
        }
    }
}

impl Inference for MockInference {
    fn generate(&self, _prompt: &str) -> Result<String> {
        Ok(self.response.clone())
    }

    fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(self.embedding.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_generate_returns_canonical_response() {
        let mock = MockInference::new();
        assert_eq!(mock.generate("anything").unwrap(), DEFAULT_RESPONSE);
    }

    #[test]
    fn default_embed_returns_fixed_vector() {
        let mock = MockInference::new();
        assert_eq!(mock.embed("anything").unwrap(), DEFAULT_EMBEDDING.to_vec());
    }

    #[test]
    fn generate_output_does_not_depend_on_prompt() {
        let mock = MockInference::new();
        assert_eq!(mock.generate("prompt a").unwrap(), mock.generate("prompt b").unwrap());
    }

    #[test]
    fn embed_output_does_not_depend_on_text() {
        let mock = MockInference::new();
        assert_eq!(mock.embed("text a").unwrap(), mock.embed("text b").unwrap());
    }

    #[test]
    fn with_response_overrides_generate_output() {
        let mock = MockInference::with_response("custom response");
        assert_eq!(mock.generate("ignored").unwrap(), "custom response");
    }

    #[test]
    fn with_embedding_overrides_embed_output() {
        let mock = MockInference::with_embedding(vec![9.0, 8.0, 7.0]);
        assert_eq!(mock.embed("ignored").unwrap(), vec![9.0, 8.0, 7.0]);
    }

    #[test]
    fn with_response_keeps_default_embedding() {
        let mock = MockInference::with_response("custom response");
        assert_eq!(mock.embed("x").unwrap(), DEFAULT_EMBEDDING.to_vec());
    }
}
