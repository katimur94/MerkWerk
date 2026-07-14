//! Error type for the storage crate.

/// Errors that can occur while opening or operating on the MerkWerk store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Any error surfaced by the underlying SQLite driver.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// The database on disk was created by a newer version of this crate
    /// than the one currently running. We refuse to touch it rather than
    /// silently downgrading (which could corrupt data or lose migrations).
    #[error(
        "database schema version {db} is newer than the highest version \
         known to this build ({code}); refusing to open (no downgrade)"
    )]
    SchemaTooNew {
        /// Schema version recorded in the database's `meta` table.
        db: i64,
        /// Highest schema version this build of the crate knows how to apply.
        code: i64,
    },
}

/// Convenience `Result` alias used throughout the storage crate.
pub type Result<T> = std::result::Result<T, Error>;
