//! Error type for the inference crate.

/// Errors that can occur while talking to an [`crate::Inference`] backend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The HTTP request to the inference backend failed: connection
    /// refused (Ollama not running), timed out, or a non-2xx status.
    #[error("inference backend request failed: {0}")]
    Request(#[from] ureq::Error),

    /// The backend responded, but the body was not the JSON shape we
    /// expected (invalid JSON, or missing/mistyped field).
    #[error("inference backend returned unexpected JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience `Result` alias used throughout the inference crate.
pub type Result<T> = std::result::Result<T, Error>;
