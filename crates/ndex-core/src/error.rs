//! The crate-wide error type and the CLI exit-code mapping (PRD §13.7).

use thiserror::Error;

/// Convenience alias used throughout ndex.
pub type Result<T, E = NdexError> = std::result::Result<T, E>;

/// Top-level error type for ndex.
///
/// [`NdexError::exit_code`] maps variants to the process exit codes documented in PRD §13.7.
/// Usage errors (exit code 2) are emitted by `clap`, not by this type.
#[derive(Debug, Error)]
pub enum NdexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("index not found: {0}")]
    IndexNotFound(String),

    #[error("index schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("remote connection failed: {0}")]
    RemoteConnection(String),

    #[error("remote version incompatible: {0}")]
    VersionIncompatible(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("transient extraction failure: {0}")]
    ExtractionTransient(String),

    #[error("permanent extraction failure: {0}")]
    ExtractionPermanent(String),

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("file too large: {0}")]
    TooLarge(String),

    #[error("text encoding error: {0}")]
    Encoding(String),

    #[error("embedding model error: {0}")]
    Model(String),

    #[error("index engine error: {0}")]
    Index(String),

    #[error("lock error: {0}")]
    Lock(String),

    #[error(".ndex/ is on an NFS filesystem: {0}")]
    Nfs(String),

    #[error("operation interrupted")]
    Interrupted,

    #[error("no results")]
    NoResults,

    #[error("{0}")]
    Other(String),
}

impl NdexError {
    /// Map this error to a process exit code (PRD §13.7).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::IndexNotFound(_) => 3,
            Self::RemoteConnection(_) | Self::Nfs(_) => 4,
            Self::VersionIncompatible(_) => 5,
            Self::SchemaMismatch(_) => 6,
            Self::NoResults => 7,
            Self::Config(_) => 78,
            Self::Interrupted => 130,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_prd() {
        assert_eq!(NdexError::IndexNotFound("x".into()).exit_code(), 3);
        assert_eq!(NdexError::RemoteConnection("x".into()).exit_code(), 4);
        assert_eq!(NdexError::VersionIncompatible("x".into()).exit_code(), 5);
        assert_eq!(NdexError::SchemaMismatch("x".into()).exit_code(), 6);
        assert_eq!(NdexError::NoResults.exit_code(), 7);
        assert_eq!(NdexError::Config("x".into()).exit_code(), 78);
        assert_eq!(NdexError::Interrupted.exit_code(), 130);
        assert_eq!(NdexError::Other("x".into()).exit_code(), 1);
    }
}
