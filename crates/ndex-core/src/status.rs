//! File processing status (PRD §10.1 `files.status`).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Processing status of a manifest file entry.
///
/// Serializes as a bare integer (the on-wire and in-DB representation), never as a
/// tagged enum — see the `#[serde(into / try_from)]` attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
#[repr(u8)]
pub enum FileStatus {
    /// Inserted, not yet processed.
    Pending = 0,
    /// Fully indexed.
    Indexed = 1,
    /// Failed; eligible for retry.
    FailedTransient = 2,
    /// No longer present on disk.
    Deleted = 3,
    /// Failed permanently (retry limit reached, unsupported, etc.).
    FailedPermanent = 4,
    /// Intentionally not indexed (too large, binary, depth limit, …).
    Skipped = 5,
}

impl FileStatus {
    /// The numeric discriminant.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl From<FileStatus> for u8 {
    fn from(s: FileStatus) -> Self {
        s as u8
    }
}

impl TryFrom<u8> for FileStatus {
    type Error = InvalidFileStatus;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Ok(match v {
            0 => Self::Pending,
            1 => Self::Indexed,
            2 => Self::FailedTransient,
            3 => Self::Deleted,
            4 => Self::FailedPermanent,
            5 => Self::Skipped,
            other => return Err(InvalidFileStatus(other)),
        })
    }
}

/// Error returned when an integer does not map to a [`FileStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFileStatus(pub u8);

impl fmt::Display for InvalidFileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid file status discriminant: {}", self.0)
    }
}

impl std::error::Error for InvalidFileStatus {}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [FileStatus; 6] = [
        FileStatus::Pending,
        FileStatus::Indexed,
        FileStatus::FailedTransient,
        FileStatus::Deleted,
        FileStatus::FailedPermanent,
        FileStatus::Skipped,
    ];

    #[test]
    fn u8_roundtrip() {
        for s in ALL {
            assert_eq!(FileStatus::try_from(s.as_u8()).unwrap(), s);
        }
        assert_eq!(FileStatus::try_from(6), Err(InvalidFileStatus(6)));
    }

    #[test]
    fn serde_is_a_bare_integer() {
        assert_eq!(serde_json::to_string(&FileStatus::Indexed).unwrap(), "1");
        let s: FileStatus = serde_json::from_str("4").unwrap();
        assert_eq!(s, FileStatus::FailedPermanent);
        assert!(serde_json::from_str::<FileStatus>("9").is_err());
    }
}
