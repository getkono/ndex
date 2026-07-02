//! Characterization tests for the public `ndex-extract` interface.
//!
//! Detection/classification helpers (MIME, BOM, NFC, language, archive-safety, JSON shape) are
//! REAL and exercised live. Transcoding, routing-to-blocks, chunking, and the tree-sitter grammar
//! map are `todo!()`; their contracts are pinned by `#[ignore = "impl pending: PR #3"]` tests.

use ndex_core::config::{Chunking, Config};
use ndex_core::model::{Block, BlockType};
use ndex_core::path::NdexPath;
use ndex_core::tokens::TokenCounter;
use ndex_extract::archive_safety::{
    MEMBER_DELIM, exceeds_ratio, is_unsafe_member_path, member_path, with_panic_isolation,
};
use ndex_extract::extractor::{ExtractCtx, Extractor, is_archive_mime, router};
use ndex_extract::formats::code::language_for;
use ndex_extract::formats::text::{JsonVariant, PlaintextExtractor, json_variant};
use ndex_extract::{Chunker, encoding, lang, mime};

fn p(s: &str) -> NdexPath {
    NdexPath::new(s.as_bytes().to_vec())
}

/// A whitespace token counter for `ExtractCtx`/`Chunker` construction in todo-contract tests.
struct Whitespace;
impl TokenCounter for Whitespace {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

// ---------------------------------------------------------------------------
// MIME detection + text/binary classification (PRD §4.4, §4.8).
// ---------------------------------------------------------------------------

#[test]
fn magic_bytes_beat_extension() {
    let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
    assert_eq!(mime::detect(&p("masquerade.txt"), png), "image/png");
}

#[test]
fn extension_and_fallbacks() {
    assert_eq!(mime::detect(&p("notes.txt"), b"just words"), "text/plain");
    assert_eq!(
        mime::detect(&p("blob"), b"\x00\x01\x02\x03"),
        "application/octet-stream"
    );
    // No extension, textual content ⇒ text/plain.
    assert_eq!(mime::detect(&p("README"), b"plain text"), "text/plain");
}

#[test]
fn is_text_uses_nul_heuristic() {
    assert!(mime::is_text(b"hello world\n"));
    assert!(mime::is_text(b"")); // empty is text
    assert!(!mime::is_text(b"a\0b"));
    assert_eq!(mime::TEXT_SNIFF_BYTES, 8192);
    // A NUL beyond the sniff window does not flip the classification.
    let mut big = vec![b'a'; mime::TEXT_SNIFF_BYTES + 10];
    big[mime::TEXT_SNIFF_BYTES + 5] = 0;
    assert!(mime::is_text(&big));
}

#[test]
fn known_filenames_and_extension_languages() {
    assert_eq!(
        mime::known_filename(&p("/src/Makefile")),
        Some("text/x-makefile")
    );
    assert_eq!(
        mime::known_filename(&p("/Dockerfile")),
        Some("text/x-dockerfile")
    );
    assert_eq!(
        mime::known_filename(&p("/a/CMakeLists.txt")),
        Some("text/x-cmake")
    );
    assert_eq!(mime::known_filename(&p("/src/main.rs")), None);

    assert_eq!(mime::extension_language(&p("a.rs")), Some("rust"));
    assert_eq!(mime::extension_language(&p("a.py")), Some("python"));
    assert_eq!(mime::extension_language(&p("a.mjs")), Some("javascript"));
    assert_eq!(mime::extension_language(&p("a.h")), Some("c")); // .h → C, not C++
    assert_eq!(mime::extension_language(&p("a.hpp")), Some("cpp"));
    assert_eq!(mime::extension_language(&p("a.unknownext")), None);
}

// ---------------------------------------------------------------------------
// Encoding: BOM + NFC (PRD §4.8, §10.2).
// ---------------------------------------------------------------------------

#[test]
fn bom_detection_lengths_and_stripping() {
    use encoding::Bom;
    assert_eq!(
        encoding::detect_bom(&[0xEF, 0xBB, 0xBF, b'h']),
        Some(Bom::Utf8)
    );
    assert_eq!(
        encoding::detect_bom(&[0xFF, 0xFE, 0x68]),
        Some(Bom::Utf16Le)
    );
    assert_eq!(
        encoding::detect_bom(&[0xFE, 0xFF, 0x00]),
        Some(Bom::Utf16Be)
    );
    assert_eq!(encoding::detect_bom(b"plain"), None);
    assert_eq!(Bom::Utf8.byte_len(), 3);
    assert_eq!(Bom::Utf16Le.byte_len(), 2);
    assert_eq!(Bom::Utf16Be.byte_len(), 2);
    assert_eq!(encoding::strip_bom(&[0xEF, 0xBB, 0xBF, b'h', b'i']), b"hi");
    assert_eq!(encoding::strip_bom(b"hi"), b"hi");
}

#[test]
fn nfc_normalization() {
    let nfd = "cafe\u{0301}"; // e + combining acute
    let nfc = "caf\u{00e9}"; // é
    assert_ne!(nfd, nfc);
    assert_eq!(encoding::nfc_normalize(nfd), nfc);
    // Already-NFC text is unchanged.
    assert_eq!(encoding::nfc_normalize("plain"), "plain");
}

// ---------------------------------------------------------------------------
// Language detection (PRD §10.2).
// ---------------------------------------------------------------------------

#[test]
fn language_detection_and_short_text_guard() {
    assert_eq!(lang::MIN_DETECT_LEN, 20);
    assert!(lang::detect("hi").is_none());
    let english = "The annual report summarizes the company's financial performance over the \
                   past fiscal year, including revenue growth and projected earnings.";
    assert_eq!(lang::detect(english).as_deref(), Some("eng"));
}

// ---------------------------------------------------------------------------
// Archive safety (PRD §4.9).
// ---------------------------------------------------------------------------

#[test]
fn unsafe_member_paths_rejected() {
    assert!(is_unsafe_member_path("/etc/passwd"));
    assert!(is_unsafe_member_path("../../etc/passwd"));
    assert!(is_unsafe_member_path("a\0b"));
    assert!(is_unsafe_member_path("dir\\..\\x"));
    assert!(!is_unsafe_member_path("2024/Q3-report.pdf"));
}

#[test]
fn compression_ratio_and_member_path() {
    assert!(exceeds_ratio(1, 300, 200));
    assert!(!exceeds_ratio(1, 100, 200));
    assert!(!exceeds_ratio(0, 0, 200)); // zero-compressed never divides-by-zero
    assert_eq!(MEMBER_DELIM, b"!/");
    let joined = member_path(&p("/pool/reports.tar.gz"), "2024/Q3.pdf");
    assert_eq!(joined.as_bytes(), b"/pool/reports.tar.gz!/2024/Q3.pdf");
}

#[test]
fn panic_isolation_catches_and_passes_through() {
    assert_eq!(with_panic_isolation(|| 1 + 1).unwrap(), 2);
    assert!(with_panic_isolation(|| panic!("malformed archive")).is_err());
}

// ---------------------------------------------------------------------------
// Routing + JSON shape (PRD §4.4, §4.8).
// ---------------------------------------------------------------------------

#[test]
fn archive_mimes_recognized() {
    for m in [
        "application/zip",
        "application/x-tar",
        "application/gzip",
        "application/x-bzip2",
        "application/x-xz",
    ] {
        assert!(is_archive_mime(m), "{m}");
    }
    assert!(!is_archive_mime("text/plain"));
}

#[test]
fn router_constructs_for_every_branch_without_panicking() {
    for m in [
        "application/pdf",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "text/markdown",
        "text/html",
        "application/json",
        "text/csv",
        "application/sql",
        "application/zip",
        "image/png",
        "text/x-rust",
        "application/octet-stream",
    ] {
        let _boxed = router(m); // construction is real even though extract() is todo
    }
}

#[test]
fn json_variant_by_first_nonws_byte() {
    assert_eq!(json_variant(b"  {\"a\":1}"), Some(JsonVariant::Object));
    assert_eq!(json_variant(b"\n\t[1,2]"), Some(JsonVariant::Array));
    assert_eq!(json_variant(b"42"), None);
    assert_eq!(json_variant(b""), None);
}

// ---------------------------------------------------------------------------
// todo!() contracts (PR #3 targets).
// ---------------------------------------------------------------------------

#[test]
fn decode_to_utf8_handles_utf16_and_passthrough() {
    assert_eq!(encoding::decode_to_utf8(b"hello"), "hello");
    let utf16le = [0xFF, 0xFE, b'h', 0x00, b'i', 0x00];
    assert_eq!(encoding::decode_to_utf8(&utf16le), "hi");
}

#[test]
fn plaintext_extractor_yields_blocks() {
    let cfg = Config::default();
    let path = p("/x/notes.txt");
    let ws = Whitespace;
    let ctx = ExtractCtx {
        mime: "text/plain",
        path: &path,
        tokens: &ws,
        depth: 0,
        config: &cfg,
    };
    let out = PlaintextExtractor
        .extract(b"first paragraph\n\nsecond paragraph", &ctx)
        .unwrap();
    assert!(!out.blocks.is_empty());
}

#[test]
fn chunker_produces_ordered_chunks_for_the_file() {
    let ws = Whitespace;
    let chunking = Chunking::default();
    let chunker = Chunker::new(&ws, &chunking);
    let blocks = vec![Block {
        block_type: BlockType::Paragraph,
        text: "word ".repeat(1000),
        byte_start: 0,
        byte_end: 5000,
        heading_path: vec!["Intro".into()],
    }];
    let chunks = chunker.chunk(7, &blocks);
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|c| c.file_id == 7));
    // chunk_ord is monotonically increasing from 0.
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.chunk_ord as usize, i);
    }
}

#[test]
fn language_for_maps_bundled_grammars() {
    assert!(language_for("rust").is_some());
    assert!(language_for("python").is_some());
    assert!(language_for("javascript").is_some());
    assert!(language_for("klingon").is_none());
}
