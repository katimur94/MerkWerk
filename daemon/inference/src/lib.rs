//! Local AI inference abstraction for the MerkWerk daemon.
//!
//! Per `ENTSCHEIDUNGEN.md` D9, inference lives behind the [`Inference`]
//! trait so the concrete backend is swappable. Backend v1 is
//! [`OllamaBackend`], talking to a local Ollama server over plain HTTP
//! (`127.0.0.1:11434`, see `docs/ROADMAP.md` Etappe 2). Switching to a later
//! backend (embedded llama.cpp via `llama-cpp-2`, Candle, ...) is a new
//! `Inference` impl, with no change required at call sites (distiller,
//! embeddings).
//!
//! This crate is platform-neutral and natively testable (`ENTSCHEIDUNGEN.md`
//! D4/D6): the HTTP glue in [`OllamaBackend`] is a thin wrapper around free,
//! pure functions that build request bodies and parse response bodies, unit
//! tested against known JSON strings without a running Ollama server.
//! [`MockInference`] is provided for tests in *other* crates that need an
//! `Inference` impl without any I/O (e.g. the future distiller).

mod error;
mod mock;
mod ollama;

pub use error::{Error, Result};
pub use mock::{MockInference, DEFAULT_EMBEDDING, DEFAULT_RESPONSE};
pub use ollama::OllamaBackend;

/// Abstraction over a local AI inference backend (`ENTSCHEIDUNGEN.md` D9).
///
/// Implementations must be safe to share across threads (`Send + Sync`):
/// the daemon calls inference from a background worker while other threads
/// keep capturing.
pub trait Inference: Send + Sync {
    /// Generates free-form text from `prompt`.
    ///
    /// Used by the distiller (`docs/ROADMAP.md` Etappe 2) to turn captured,
    /// filtered context into a Markdown note.
    fn generate(&self, prompt: &str) -> Result<String>;

    /// Computes an embedding vector for `text`.
    ///
    /// Used for the semantic search index over snapshots/notes
    /// (`docs/ROADMAP.md` Etappe 3).
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
