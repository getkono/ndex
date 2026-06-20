//! Raw-bytes filesystem paths (PRD §8).

use std::borrow::Cow;
use std::fmt;

use serde::de::{Error as DeError, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A filesystem path stored as raw platform bytes.
///
/// Paths are not guaranteed to be valid UTF-8 (PRD §8), so they are kept as bytes and
/// serialized as a MessagePack `bin` (via [`Serializer::serialize_bytes`]) — never as a
/// lossy string. In SQLite they are stored as `BLOB`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NdexPath(Vec<u8>);

impl NdexPath {
    /// Construct from raw bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Borrow the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume into the owned byte vector.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Lossy UTF-8 rendering for display (invalid bytes become `U+FFFD`).
    pub fn display_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// `xxh3_64` of the raw path bytes: a non-cryptographic lookup accelerator
    /// (PRD §10.1 `path_hash`), not a content hash.
    pub fn path_hash(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(&self.0)
    }

    /// Build from an OS string (Unix: borrow the raw bytes). ndex is Unix-only for v0.1.
    #[cfg(unix)]
    pub fn from_os_str(s: &std::ffi::OsStr) -> Self {
        use std::os::unix::ffi::OsStrExt;
        Self(s.as_bytes().to_vec())
    }

    /// Convert back to an OS string (Unix).
    #[cfg(unix)]
    pub fn to_os_string(&self) -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(self.0.clone())
    }

    /// JSON-safe rendering: valid UTF-8 passes through; invalid bytes are `\uXXXX`-escaped
    /// (PRD §8). Used by the client's JSON output renderer.
    pub fn to_json_escaped(&self) -> String {
        // TODO(skeleton): implement the byte-preserving \uXXXX escaping policy (PRD §8).
        todo!("byte-preserving JSON escaping for non-UTF-8 paths")
    }
}

impl Serialize for NdexPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for NdexPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ByteVisitor;

        impl<'de> Visitor<'de> for ByteVisitor {
            type Value = NdexPath;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("raw path bytes (a msgpack bin, byte sequence, or string)")
            }

            fn visit_bytes<E: DeError>(self, v: &[u8]) -> Result<NdexPath, E> {
                Ok(NdexPath(v.to_vec()))
            }

            fn visit_byte_buf<E: DeError>(self, v: Vec<u8>) -> Result<NdexPath, E> {
                Ok(NdexPath(v))
            }

            fn visit_str<E: DeError>(self, v: &str) -> Result<NdexPath, E> {
                Ok(NdexPath(v.as_bytes().to_vec()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NdexPath, A::Error> {
                let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(b) = seq.next_element::<u8>()? {
                    bytes.push(b);
                }
                Ok(NdexPath(bytes))
            }
        }

        deserializer.deserialize_byte_buf(ByteVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip_preserves_non_utf8() {
        let p = NdexPath::new(vec![0xff, b'/', b'c', 0x80, b'f', 0xc3, 0x28]);
        let json = serde_json::to_string(&p).unwrap();
        let back: NdexPath = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn path_hash_is_deterministic_and_distinguishes() {
        let a = NdexPath::new(b"/pool/archive/a".to_vec());
        let b = NdexPath::new(b"/pool/archive/b".to_vec());
        assert_eq!(a.path_hash(), a.clone().path_hash());
        assert_ne!(a.path_hash(), b.path_hash());
    }

    #[test]
    fn display_is_lossy_for_invalid_utf8() {
        let p = NdexPath::new(vec![b'a', 0xff, b'b']);
        assert_eq!(p.display_lossy(), "a\u{fffd}b");
    }

    #[cfg(unix)]
    #[test]
    fn os_str_roundtrip() {
        use std::ffi::OsStr;
        let original = OsStr::new("/pool/archive/contract.pdf");
        let p = NdexPath::from_os_str(original);
        assert_eq!(p.to_os_string(), original);
    }
}
