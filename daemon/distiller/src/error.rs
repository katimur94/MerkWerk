//! Error type for the distiller crate.

/// Errors that can occur while distilling a time range into a
/// [`crate::DistilledNote`]: either the storage layer or the local
/// inference backend failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Loading sessions/snapshots from the store failed.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),

    /// The local inference backend failed to generate the note.
    #[error("inference error: {0}")]
    Inference(#[from] inference::Error),
}

/// Convenience `Result` alias used throughout the distiller crate.
pub type Result<T> = std::result::Result<T, Error>;
