//! Ollama backend: `Inference` over Ollama's local HTTP API.
//!
//! Backend v1 per `ENTSCHEIDUNGEN.md` D9: Ollama is a standalone local app
//! exposing HTTP+JSON on `127.0.0.1:11434` (default). This module only
//! speaks plain HTTP — no TLS — since Ollama is loopback-only; see the
//! crate's `ureq` dependency (`default-features = false`) in `Cargo.toml`.
//!
//! The HTTP glue (`generate`/`embed`) is a thin wrapper around free, pure
//! functions that build request bodies and parse response bodies. Those
//! functions are the natively-tested surface of this module (see the tests
//! below) — no running Ollama server is required.

use std::time::Duration;

use serde::Deserialize;
use ureq::Agent;

use crate::{Inference, Result};

/// End-to-end timeout (DNS through response body) for a single request.
///
/// Generous on purpose: local CPU-bound generation for a longer prompt can
/// take a while, and there is no server-side load to protect against here
/// (single local user, loopback only).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// [`Inference`] backend talking to a local Ollama server.
#[derive(Debug, Clone)]
pub struct OllamaBackend {
    /// Base URL of the Ollama server, e.g. `"http://127.0.0.1:11434"`.
    endpoint: String,
    /// Model used for `generate`, e.g. `"llama3.1"`.
    model: String,
    /// Model used for `embed`, e.g. `"nomic-embed-text"`.
    embed_model: String,
    /// HTTP client with the shared timeout applied.
    agent: Agent,
}

impl OllamaBackend {
    /// Creates a new backend pointing at `endpoint`, using `model` for text
    /// generation and `embed_model` for embeddings.
    ///
    /// This performs no I/O; the connection to Ollama is only attempted
    /// once `generate`/`embed` is called.
    pub fn new(
        endpoint: impl Into<String>,
        model: impl Into<String>,
        embed_model: impl Into<String>,
    ) -> Self {
        let config = Agent::config_builder()
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .build();

        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            embed_model: embed_model.into(),
            agent: Agent::new_with_config(config),
        }
    }

    /// Joins `endpoint` and `path` into a request URL, tolerating a
    /// trailing slash on the configured endpoint.
    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.endpoint.trim_end_matches('/'))
    }
}

impl Inference for OllamaBackend {
    fn generate(&self, prompt: &str) -> Result<String> {
        let body = build_generate_body(&self.model, prompt);

        let raw = self
            .agent
            .post(self.url("api/generate"))
            .header("Content-Type", "application/json")
            .send(serde_json::to_string(&body)?)?
            .body_mut()
            .read_to_string()?;

        parse_generate_response(&raw)
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = build_embed_body(&self.embed_model, text);

        let raw = self
            .agent
            .post(self.url("api/embeddings"))
            .header("Content-Type", "application/json")
            .send(serde_json::to_string(&body)?)?
            .body_mut()
            .read_to_string()?;

        parse_embed_response(&raw)
    }
}

/// Builds the JSON body for `POST {endpoint}/api/generate`.
///
/// `stream: false` so Ollama returns a single JSON object instead of a
/// stream of JSONL chunks — simpler for a first backend (D9); the daemon
/// does not need token-by-token streaming yet.
fn build_generate_body(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
    })
}

/// Shape of an Ollama `/api/generate` response (`stream: false`), reduced to
/// the one field this crate cares about.
#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
}

/// Parses the JSON body of a `/api/generate` response into its text.
///
/// Any JSON parse error, or a well-formed object missing the `"response"`
/// string field, is surfaced as [`crate::Error::Json`].
fn parse_generate_response(raw: &str) -> Result<String> {
    let parsed: GenerateResponse = serde_json::from_str(raw)?;
    Ok(parsed.response)
}

/// Builds the JSON body for `POST {endpoint}/api/embeddings`.
fn build_embed_body(model: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "prompt": text,
    })
}

/// Shape of an Ollama `/api/embeddings` response.
#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

/// Parses the JSON body of an `/api/embeddings` response into its vector.
fn parse_embed_response(raw: &str) -> Result<Vec<f32>> {
    let parsed: EmbedResponse = serde_json::from_str(raw)?;
    Ok(parsed.embedding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_endpoint_model_and_embed_model() {
        let backend = OllamaBackend::new("http://127.0.0.1:11434", "llama3.1", "nomic-embed-text");
        assert_eq!(backend.endpoint, "http://127.0.0.1:11434");
        assert_eq!(backend.model, "llama3.1");
        assert_eq!(backend.embed_model, "nomic-embed-text");
    }

    #[test]
    fn url_joins_endpoint_and_path() {
        let backend = OllamaBackend::new("http://127.0.0.1:11434", "m", "e");
        assert_eq!(
            backend.url("api/generate"),
            "http://127.0.0.1:11434/api/generate"
        );
    }

    #[test]
    fn url_tolerates_trailing_slash_on_endpoint() {
        let backend = OllamaBackend::new("http://127.0.0.1:11434/", "m", "e");
        assert_eq!(
            backend.url("api/generate"),
            "http://127.0.0.1:11434/api/generate"
        );
    }

    #[test]
    fn build_generate_body_matches_ollama_wire_format() {
        let body = build_generate_body("llama3.1", "Hallo Welt");
        assert_eq!(
            body,
            serde_json::json!({
                "model": "llama3.1",
                "prompt": "Hallo Welt",
                "stream": false,
            })
        );
    }

    #[test]
    fn parse_generate_response_extracts_response_field() {
        let parsed = parse_generate_response(r#"{"response":"Hallo"}"#).expect("parse");
        assert_eq!(parsed, "Hallo");
    }

    #[test]
    fn parse_generate_response_ignores_unknown_extra_fields() {
        // Real Ollama replies carry additional fields (model, done,
        // eval_count, ...); we only care about `response` and must not
        // choke on the rest.
        let parsed = parse_generate_response(
            r#"{"model":"llama3.1","created_at":"now","response":"Hallo","done":true}"#,
        )
        .expect("parse");
        assert_eq!(parsed, "Hallo");
    }

    #[test]
    fn parse_generate_response_rejects_missing_response_field() {
        assert!(parse_generate_response("{}").is_err());
    }

    #[test]
    fn parse_generate_response_rejects_garbage() {
        assert!(parse_generate_response("not json").is_err());
    }

    #[test]
    fn build_embed_body_matches_ollama_wire_format() {
        let body = build_embed_body("nomic-embed-text", "Hallo Welt");
        assert_eq!(
            body,
            serde_json::json!({
                "model": "nomic-embed-text",
                "prompt": "Hallo Welt",
            })
        );
    }

    #[test]
    fn parse_embed_response_extracts_embedding_field() {
        let parsed = parse_embed_response(r#"{"embedding":[0.1,0.2]}"#).expect("parse");
        assert_eq!(parsed, vec![0.1_f32, 0.2_f32]);
    }

    #[test]
    fn parse_embed_response_rejects_missing_embedding_field() {
        assert!(parse_embed_response("{}").is_err());
    }

    #[test]
    fn parse_embed_response_rejects_garbage() {
        assert!(parse_embed_response("not json").is_err());
    }
}
