//! Archive decompression safety: bomb limits, path-traversal guards, panic isolation (PRD §4.9).

use ndex_core::error::{NdexError, Result};
use ndex_core::path::NdexPath;

/// The `!/` delimiter separating an archive path from a member path (JAR convention, PRD §4.6).
pub const MEMBER_DELIM: &[u8] = b"!/";

/// Build a member path: `<archive>!/<member>` (PRD §4.6).
pub fn member_path(archive: &NdexPath, member: &str) -> NdexPath {
    let mut bytes = archive.as_bytes().to_vec();
    bytes.extend_from_slice(MEMBER_DELIM);
    bytes.extend_from_slice(member.as_bytes());
    NdexPath::new(bytes)
}

/// Reject unsafe archive member paths: traversal (`../`), absolute, or containing NUL (PRD §4.9).
pub fn is_unsafe_member_path(member: &str) -> bool {
    member.starts_with('/')
        || member.contains("../")
        || member.contains("..\\")
        || member.contains('\0')
}

/// Whether a member's decompressed:compressed ratio exceeds the bomb `limit` (PRD §4.9).
pub fn exceeds_ratio(compressed: u64, decompressed: u64, limit: u32) -> bool {
    let denom = compressed.max(1);
    decompressed / denom > u64::from(limit)
}

/// Run third-party archive extraction with panic isolation so malformed input cannot crash the
/// indexer (PRD §4.9). Does not protect against `abort()` or stack overflow in native code.
pub fn with_panic_isolation<T>(f: impl FnOnce() -> T) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| NdexError::ExtractionTransient("archive extractor panicked".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_paths_are_rejected() {
        assert!(is_unsafe_member_path("../../etc/passwd"));
        assert!(is_unsafe_member_path("/etc/passwd"));
        assert!(is_unsafe_member_path("a\0b"));
        assert!(!is_unsafe_member_path("2024/Q3-report.pdf"));
    }

    #[test]
    fn ratio_limit() {
        assert!(exceeds_ratio(1, 300, 200));
        assert!(!exceeds_ratio(1, 100, 200));
        assert!(!exceeds_ratio(0, 0, 200));
    }

    #[test]
    fn member_path_uses_delimiter() {
        let archive = NdexPath::new(b"/pool/reports.tar.gz".to_vec());
        let joined = member_path(&archive, "2024/Q3.pdf");
        assert_eq!(joined.as_bytes(), b"/pool/reports.tar.gz!/2024/Q3.pdf");
    }

    #[test]
    fn panic_isolation_catches() {
        let ok = with_panic_isolation(|| 1 + 1);
        assert_eq!(ok.unwrap(), 2);
        let boom = with_panic_isolation(|| panic!("malformed archive"));
        assert!(boom.is_err());
    }
}
