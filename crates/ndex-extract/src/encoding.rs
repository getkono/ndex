//! Text encoding detection, transcoding, and Unicode normalization (PRD §4.8, §10.2).

use std::borrow::Cow;

/// A detected leading byte-order mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bom {
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl Bom {
    /// Length of the BOM marker in bytes.
    pub const fn byte_len(self) -> usize {
        match self {
            Bom::Utf8 => 3,
            Bom::Utf16Le | Bom::Utf16Be => 2,
        }
    }
}

/// Detect a leading BOM (PRD §4.8).
pub fn detect_bom(bytes: &[u8]) -> Option<Bom> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some(Bom::Utf8)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(Bom::Utf16Le)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(Bom::Utf16Be)
    } else {
        None
    }
}

/// Strip a leading BOM, returning the remaining bytes (PRD §4.8).
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    match detect_bom(bytes) {
        Some(bom) => &bytes[bom.byte_len()..],
        None => bytes,
    }
}

/// Decode arbitrary bytes to UTF-8: BOM → `chardetng` detection → `encoding_rs` transcode →
/// lossy fallback (PRD §4.8). The caller typically follows with [`nfc_normalize`].
pub fn decode_to_utf8(bytes: &[u8]) -> Cow<'_, str> {
    match detect_bom(bytes) {
        Some(Bom::Utf16Le) => return Cow::Owned(decode_utf16(&bytes[2..], true)),
        Some(Bom::Utf16Be) => return Cow::Owned(decode_utf16(&bytes[2..], false)),
        Some(Bom::Utf8) => return Cow::Owned(String::from_utf8_lossy(&bytes[3..]).into_owned()),
        None => {}
    }
    // Fast path: the bytes are already valid UTF-8.
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(s);
    }
    // Detect a legacy encoding and transcode to UTF-8 (lossy on undecodable bytes).
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
    Cow::Owned(encoding.decode(bytes).0.into_owned())
}

/// Decode a UTF-16 byte body (BOM already stripped) into a `String`, replacing unpaired
/// surrogates with `U+FFFD`.
fn decode_utf16(body: &[u8], little_endian: bool) -> String {
    let units = body.chunks_exact(2).map(|c| {
        if little_endian {
            u16::from_le_bytes([c[0], c[1]])
        } else {
            u16::from_be_bytes([c[0], c[1]])
        }
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// NFC-normalize text before tokenization so NFC/NFD spellings match (PRD §10.2).
pub fn nfc_normalize(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_detection_and_stripping() {
        assert_eq!(detect_bom(&[0xEF, 0xBB, 0xBF, b'h', b'i']), Some(Bom::Utf8));
        assert_eq!(detect_bom(&[0xFF, 0xFE, 0x68]), Some(Bom::Utf16Le));
        assert_eq!(detect_bom(b"plain"), None);
        assert_eq!(strip_bom(&[0xEF, 0xBB, 0xBF, b'h', b'i']), b"hi");
        assert_eq!(strip_bom(b"hi"), b"hi");
    }

    #[test]
    fn nfc_normalizes_decomposed_text() {
        // "café" written NFD (e + combining acute) normalizes to NFC (é).
        let nfd = "cafe\u{0301}";
        let nfc = "caf\u{00e9}";
        assert_ne!(nfd, nfc);
        assert_eq!(nfc_normalize(nfd), nfc);
    }
}
