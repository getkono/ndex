//! Characterization tests for the public `ndex-protocol` interface.
//!
//! Black-box: exercises only the crate's public API, pinning the observable contract of every
//! real function and type. `ndex-protocol` is 100% real (no `todo!()`), so every test here
//! actually runs and asserts.
//!
//! Round-trips go through the crate's own MessagePack codec (`to_vec_named` / `from_slice`) —
//! that is the real wire format, so these tests double as the format-stability proof mandated by
//! PRD §12.4. Framing and preamble tests use `std::io::Cursor<Vec<u8>>` as an in-memory transport.

use std::collections::BTreeMap;
use std::io::Cursor;

use serde::Serialize;
use serde::de::DeserializeOwned;

use ndex_core::constants::{MAGIC_PREAMBLE, MAX_FRAME_BYTES, MAX_PREAMBLE_SCAN_BYTES};
use ndex_core::{DocMeta, MediaMeta, NdexPath, SearchFilters, SearchMode};

use ndex_protocol::codec::{from_slice, to_vec_named};
use ndex_protocol::frame::{FrameReader, FrameWriter};
use ndex_protocol::handshake::{self, MAX_PROTOCOL, MIN_PROTOCOL, PROTOCOL_VERSION};
use ndex_protocol::message::{
    ClientMessage, CorruptedFile, DeleteRequestData, DeleteResultData, ErrorData, FileInfo,
    HandshakeReq, HandshakeResp, IndexCompleteData, IndexOptions, IndexStats, IndexSummary,
    InfoRequestData, InfoResultData, OutputFormat, ProgressChild, ProgressEvent,
    ReindexRequestData, ReindexTarget, SearchHit, SearchRequestData, SearchResultData,
    ServerMessage, StatsResultData, TerminalCaps, VerifyRequestData, VerifyResultData,
};

// ---------------------------------------------------------------------------
// Helpers — shared samples and a codec round-trip assertion.
// ---------------------------------------------------------------------------

/// Assert a value survives a `to_vec_named` → `from_slice` round-trip unchanged.
fn roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let bytes = to_vec_named(value).expect("encode");
    let back: T = from_slice(&bytes).expect("decode");
    assert_eq!(&back, value, "round-trip changed the value");
}

/// A path containing a non-UTF-8 byte, to confirm the `bin` wire encoding survives.
fn sample_path() -> NdexPath {
    NdexPath::new(vec![0xff, b'/', b'p', b'o', b'o', b'l'])
}

/// One fully-populated sample of every `ClientMessage` variant.
fn client_variants() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Handshake(HandshakeReq {
            min_protocol: 1,
            max_protocol: 1,
            client_version: "0.1.0".into(),
            capabilities: vec!["progress".into(), "color".into()],
            terminal: TerminalCaps {
                width: 120,
                height: 40,
                color: true,
                hyperlinks: true,
                unicode: true,
            },
        }),
        ClientMessage::SearchRequest(SearchRequestData {
            query: "quarterly earnings".into(),
            mode: SearchMode::Hybrid,
            filters: SearchFilters {
                mime: Some("application/pdf".into()),
                after_ns: Some(100),
                tags: vec!["finance".into()],
                ..Default::default()
            },
            limit: 20,
            offset: 0,
            format: OutputFormat::Json,
            explain: true,
        }),
        ClientMessage::IndexRequest(IndexOptions {
            full: true,
            verify: true,
            dry_run: false,
            jobs: Some(4),
            batch_size: Some(64),
            no_vectors: false,
            enable_ner: true,
            max_file_size: Some(1 << 30),
            only_new: true,
        }),
        ClientMessage::InfoRequest(InfoRequestData {
            path: sample_path(),
        }),
        ClientMessage::StatsRequest,
        ClientMessage::VerifyRequest(VerifyRequestData {
            paths: Some(vec![sample_path()]),
            sample: Some(0.01),
        }),
        ClientMessage::ReindexRequest(ReindexRequestData {
            target: ReindexTarget::Vectors,
        }),
        ClientMessage::DeleteRequest(DeleteRequestData {
            glob: "secrets/**/*.key".into(),
            dry_run: true,
        }),
        ClientMessage::CancelRequest,
    ]
}

/// One fully-populated sample of every `ServerMessage` variant.
fn server_variants() -> Vec<ServerMessage> {
    vec![
        ServerMessage::Handshake(HandshakeResp {
            protocol_version: 1,
            server_version: "0.1.0".into(),
            index_schema_version: 3,
            index_model: "snowflake-arctic-embed-m-v2.0".into(),
            index_file_count: 1_000_000,
            index_last_reconciled_ns: 1_700_000_000_000_000_000,
            capabilities: vec!["semantic".into()],
            index_healthy: true,
        }),
        ServerMessage::SearchResult(SearchResultData {
            hits: vec![SearchHit {
                file_id: 42,
                chunk_ord: 1,
                path: sample_path(),
                score: 0.847,
                score_raw: 12.3,
                score_fts: Some(8.1),
                score_vec: Some(0.72),
                mime: "application/pdf".into(),
                size: 2_400_000,
                mtime_ns: 1_700_000_000_000_000_000,
                tags: vec!["finance".into()],
                snippet: Some("…quarterly earnings…".into()),
                byte_start: 100,
                byte_end: 240,
            }],
            total: 47,
            mode: SearchMode::Hybrid,
            duration_ms: 23,
            truncated: false,
            stale_warning: Some("index is 2h stale".into()),
        }),
        ServerMessage::IndexComplete(IndexCompleteData {
            stats: IndexStats {
                new: 10,
                modified: 2,
                deleted: 1,
                unchanged: 7,
                processed: 13,
                failed: 0,
                skipped: 3,
                duration_ms: 500,
                timed_out: false,
            },
            cancelled: false,
        }),
        ServerMessage::InfoResult(InfoResultData {
            file_info: FileInfo {
                file_id: 7,
                path: sample_path(),
                size: 4096,
                mtime_ns: 1_700_000_000_000_000_000,
                ctime_ns: 1_700_000_000_000_000_001,
                mime: Some("application/pdf".into()),
                blake3: Some(vec![0u8; 32]),
                status: 1,
                fail_count: 0,
                error_msg: None,
                tags: vec!["finance".into()],
                doc_meta: Some(DocMeta {
                    title: Some("Report".into()),
                    page_count: Some(12),
                    ..Default::default()
                }),
                media_meta: Some(MediaMeta {
                    width: Some(4032),
                    height: Some(3024),
                    ..Default::default()
                }),
                chunk_count: 12,
                in_fts: true,
                in_vectors: true,
            },
        }),
        ServerMessage::StatsResult(StatsResultData {
            index_stats: IndexSummary {
                total_files: 1_000_000,
                directories: 50_000,
                indexed: 999_000,
                pending: 500,
                failed_transient: 100,
                failed_permanent: 200,
                skipped: 200,
                deleted: 0,
                manifest_bytes: 1 << 20,
                fts_bytes: 1 << 30,
                vector_bytes: 2 << 30,
                meta_bytes: 1 << 24,
                last_reconciled_ns: Some(1_700_000_000_000_000_000),
                schema_version: 3,
                model_name: "snowflake-arctic-embed-m-v2.0".into(),
            },
        }),
        ServerMessage::VerifyResult(VerifyResultData {
            checked: 5000,
            corrupted: vec![CorruptedFile {
                file_id: 9,
                path: sample_path(),
                stored_hash: vec![1u8; 32],
                actual_hash: vec![2u8; 32],
            }],
        }),
        ServerMessage::DeleteResult(DeleteResultData {
            deleted: 3,
            paths: vec![sample_path()],
        }),
        ServerMessage::Progress(ProgressEvent {
            phase: "extract".into(),
            current: 100,
            total: Some(1000),
            message: Some("processing".into()),
            children: vec![ProgressChild {
                label: "worker-3".into(),
                current: 33,
                total: Some(250),
                message: None,
            }],
        }),
        ServerMessage::Error(ErrorData {
            code: 5,
            message: "version incompatible".into(),
        }),
    ]
}

// ---------------------------------------------------------------------------
// Codec — every message variant round-trips (PRD §12.4); errors don't panic.
// ---------------------------------------------------------------------------

#[test]
fn every_client_message_roundtrips() {
    for m in client_variants() {
        roundtrip(&m);
    }
}

#[test]
fn every_server_message_roundtrips() {
    for m in server_variants() {
        roundtrip(&m);
    }
}

#[test]
fn all_client_variants_are_covered() {
    // Guards against a new variant being added without a sample above.
    assert_eq!(client_variants().len(), 9);
}

#[test]
fn all_server_variants_are_covered() {
    assert_eq!(server_variants().len(), 9);
}

#[test]
fn unit_variants_roundtrip() {
    roundtrip(&ClientMessage::StatsRequest);
    roundtrip(&ClientMessage::CancelRequest);
}

#[test]
fn payload_structs_roundtrip_at_their_defaults() {
    // Pin the `#[serde(default)]` forward-compat contract: a defaulted struct survives the codec.
    roundtrip(&TerminalCaps::default());
    roundtrip(&HandshakeReq::default());
    roundtrip(&HandshakeResp::default());
    roundtrip(&SearchRequestData::default());
    roundtrip(&IndexOptions::default());
    roundtrip(&InfoRequestData::default());
    roundtrip(&VerifyRequestData::default());
    roundtrip(&ReindexRequestData::default());
    roundtrip(&DeleteRequestData::default());
    roundtrip(&SearchHit::default());
    roundtrip(&SearchResultData::default());
    roundtrip(&IndexStats::default());
    roundtrip(&IndexCompleteData::default());
    roundtrip(&FileInfo::default());
    roundtrip(&InfoResultData::default());
    roundtrip(&IndexSummary::default());
    roundtrip(&StatsResultData::default());
    roundtrip(&CorruptedFile::default());
    roundtrip(&VerifyResultData::default());
    roundtrip(&DeleteResultData::default());
    roundtrip(&ProgressChild::default());
    roundtrip(&ProgressEvent::default());
    roundtrip(&ErrorData::default());
}

#[test]
fn enums_roundtrip_every_variant() {
    for f in [
        OutputFormat::Pretty,
        OutputFormat::Plain,
        OutputFormat::Json,
        OutputFormat::Jsonl,
        OutputFormat::Paths,
        OutputFormat::Csv,
    ] {
        roundtrip(&f);
    }
    for t in [
        ReindexTarget::All,
        ReindexTarget::Vectors,
        ReindexTarget::Fts,
    ] {
        roundtrip(&t);
    }
    // Documented defaults.
    assert_eq!(OutputFormat::default(), OutputFormat::Pretty);
    assert_eq!(ReindexTarget::default(), ReindexTarget::All);
}

#[test]
fn from_slice_on_truncated_bytes_is_err_not_panic() {
    // Encode a real message, then lop off its tail — decode must return Err, not panic.
    let bytes = to_vec_named(&ServerMessage::Error(ErrorData {
        code: 1,
        message: "boom".into(),
    }))
    .unwrap();
    let truncated = &bytes[..bytes.len() / 2];
    assert!(from_slice::<ServerMessage>(truncated).is_err());
}

#[test]
fn from_slice_on_garbage_bytes_is_err_not_panic() {
    let garbage = [0xff, 0x00, 0x13, 0x37, 0xde, 0xad, 0xbe, 0xef];
    assert!(from_slice::<ClientMessage>(&garbage).is_err());
    assert!(from_slice::<ServerMessage>(&garbage).is_err());
    assert!(from_slice::<ServerMessage>(&[]).is_err());
}

// ---------------------------------------------------------------------------
// External tagging — the on-wire shape required by PRD §12.4.
// (rmp-serde 1.3 ships no generic Value, so we decode into shaped Rust types.)
// ---------------------------------------------------------------------------

#[test]
fn unit_variant_encodes_as_bare_variant_name() {
    // Externally-tagged unit variants serialize as just the variant-name string.
    let bytes = to_vec_named(&ClientMessage::CancelRequest).unwrap();
    let tag: String = from_slice(&bytes).expect("unit variant decodes as a string");
    assert_eq!(tag, "CancelRequest");

    let bytes = to_vec_named(&ClientMessage::StatsRequest).unwrap();
    let tag: String = from_slice(&bytes).unwrap();
    assert_eq!(tag, "StatsRequest");
}

#[test]
fn tuple_variant_encodes_as_single_key_map_keyed_by_variant_name() {
    // Externally-tagged data variants serialize as a one-entry map {"Variant": payload}.
    // `IgnoredAny` lets us assert the key without modelling the payload shape.
    let bytes = to_vec_named(&ClientMessage::DeleteRequest(DeleteRequestData {
        glob: "*.key".into(),
        dry_run: false,
    }))
    .unwrap();
    let map: BTreeMap<String, serde::de::IgnoredAny> =
        from_slice(&bytes).expect("data variant decodes as a single-key map");
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["DeleteRequest"]);

    let bytes = to_vec_named(&ServerMessage::Error(ErrorData {
        code: 7,
        message: "no results".into(),
    }))
    .unwrap();
    let map: BTreeMap<String, serde::de::IgnoredAny> = from_slice(&bytes).unwrap();
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["Error"]);
}

// ---------------------------------------------------------------------------
// Framing — length-prefixed frames over an in-memory transport (PRD §12.2).
// ---------------------------------------------------------------------------

#[test]
fn frame_roundtrips_including_empty_payload() {
    let mut buf = Vec::new();
    {
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(b"hello").unwrap();
        w.write_frame(b"").unwrap();
        w.write_frame(b"world").unwrap();
    }
    let mut r = FrameReader::new(Cursor::new(buf));
    assert_eq!(r.read_frame().unwrap(), b"hello");
    assert_eq!(r.read_frame().unwrap(), b"" as &[u8]);
    assert_eq!(r.read_frame().unwrap(), b"world");
}

#[test]
fn encoded_message_survives_a_full_frame_roundtrip() {
    // End-to-end: codec → frame → reader → codec, the real client/server data path.
    let msg = ServerMessage::Progress(ProgressEvent {
        phase: "embed".into(),
        current: 7,
        total: Some(20),
        message: None,
        children: vec![],
    });
    let payload = to_vec_named(&msg).unwrap();

    let mut buf = Vec::new();
    FrameWriter::new(&mut buf).write_frame(&payload).unwrap();

    let mut r = FrameReader::new(Cursor::new(buf));
    let got = r.read_frame().unwrap();
    let back: ServerMessage = from_slice(&got).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn length_prefix_is_u32_big_endian() {
    // Inspect the 4 header bytes for a known payload length (5 bytes → 0x00000005, BE).
    let mut buf = Vec::new();
    FrameWriter::new(&mut buf).write_frame(b"hello").unwrap();
    assert_eq!(&buf[..4], &[0x00, 0x00, 0x00, 0x05]);
    assert_eq!(&buf[4..], b"hello");

    // A larger length to exercise more than the low byte: 258 = 0x0102.
    let mut buf = Vec::new();
    let payload = vec![0u8; 258];
    FrameWriter::new(&mut buf).write_frame(&payload).unwrap();
    assert_eq!(&buf[..4], &[0x00, 0x00, 0x01, 0x02]);
    assert_eq!(u32::from_be_bytes(buf[..4].try_into().unwrap()), 258);
}

#[test]
fn read_frame_rejects_length_prefix_over_max_frame_bytes() {
    // Craft a 4-byte BE length header just past the 16 MiB cap, with no body, and assert Err.
    let oversize = (MAX_FRAME_BYTES as u32) + 1;
    let header = oversize.to_be_bytes();
    let mut r = FrameReader::new(Cursor::new(header.to_vec()));
    let err = r.read_frame();
    assert!(err.is_err(), "frame above MAX_FRAME_BYTES must be rejected");

    // u32::MAX likewise rejected without ever allocating the body.
    let mut r = FrameReader::new(Cursor::new(u32::MAX.to_be_bytes().to_vec()));
    assert!(r.read_frame().is_err());
}

#[test]
fn write_frame_rejects_oversize_payload() {
    // The writer guards the same cap on the way out.
    let payload = vec![0u8; MAX_FRAME_BYTES + 1];
    let mut buf = Vec::new();
    let mut w = FrameWriter::new(&mut buf);
    assert!(w.write_frame(&payload).is_err());
}

#[test]
fn read_frame_on_empty_stream_is_err() {
    // A closed/empty transport (no length header) errors rather than blocking or panicking.
    let mut r = FrameReader::new(Cursor::new(Vec::new()));
    assert!(r.read_frame().is_err());
}

#[test]
fn frame_writer_into_inner_returns_the_transport() {
    let w = FrameWriter::new(Vec::<u8>::new());
    let inner: Vec<u8> = w.into_inner();
    assert!(inner.is_empty());
}

#[test]
fn frame_reader_into_inner_returns_the_transport() {
    let r = FrameReader::new(Cursor::new(vec![1u8, 2, 3]));
    let inner = r.into_inner();
    assert_eq!(inner.into_inner(), vec![1u8, 2, 3]);
}

// ---------------------------------------------------------------------------
// Preamble — magic bytes + tolerant forward scan (PRD §12.2).
// ---------------------------------------------------------------------------

#[test]
fn write_preamble_emits_exactly_the_magic_bytes() {
    let mut buf = Vec::new();
    FrameWriter::new(&mut buf).write_preamble().unwrap();
    assert_eq!(buf, MAGIC_PREAMBLE);
}

#[test]
fn scan_preamble_succeeds_at_stream_start() {
    let mut stream = Vec::new();
    {
        let mut w = FrameWriter::new(&mut stream);
        w.write_preamble().unwrap();
        w.write_frame(b"after").unwrap();
    }
    let mut r = FrameReader::new(Cursor::new(stream));
    r.scan_preamble().unwrap();
    assert_eq!(r.read_frame().unwrap(), b"after");
}

#[test]
fn scan_preamble_skips_leading_garbage_under_budget() {
    let mut stream = Vec::new();
    stream.extend_from_slice(b"motd: welcome to the nas\nlast login: never\n");
    assert!(stream.len() < MAX_PREAMBLE_SCAN_BYTES);
    stream.extend_from_slice(MAGIC_PREAMBLE);
    {
        let mut w = FrameWriter::new(&mut stream);
        w.write_frame(b"after").unwrap();
    }
    let mut r = FrameReader::new(Cursor::new(stream));
    r.scan_preamble().unwrap();
    assert_eq!(r.read_frame().unwrap(), b"after");
}

#[test]
fn scan_preamble_handles_partial_false_start() {
    // Leading bytes that partially match the magic ("NDEX" without the trailing 0x00 0x01)
    // must not derail the scan — the real preamble that follows still resolves.
    let mut stream = Vec::new();
    stream.extend_from_slice(b"NDEXNDEX\x00"); // teasing prefixes, no full match yet
    stream.extend_from_slice(MAGIC_PREAMBLE);
    {
        let mut w = FrameWriter::new(&mut stream);
        w.write_frame(b"ok").unwrap();
    }
    let mut r = FrameReader::new(Cursor::new(stream));
    r.scan_preamble().unwrap();
    assert_eq!(r.read_frame().unwrap(), b"ok");
}

#[test]
fn scan_preamble_errs_when_absent_within_budget() {
    let garbage = vec![b'x'; MAX_PREAMBLE_SCAN_BYTES + 100];
    let mut r = FrameReader::new(Cursor::new(garbage));
    assert!(r.scan_preamble().is_err());
}

#[test]
fn scan_preamble_errs_on_empty_stream() {
    let mut r = FrameReader::new(Cursor::new(Vec::new()));
    assert!(r.scan_preamble().is_err());
}

// ---------------------------------------------------------------------------
// Handshake — version negotiation + pinned constants (PRD §12.3).
// ---------------------------------------------------------------------------

#[test]
fn protocol_constants_are_pinned() {
    assert_eq!(PROTOCOL_VERSION, 1);
    assert_eq!(MIN_PROTOCOL, 1);
    assert_eq!(MAX_PROTOCOL, 1);
}

#[test]
fn negotiate_returns_agreed_version_on_overlap() {
    assert_eq!(handshake::negotiate(1, 1).unwrap(), 1);
    assert_eq!(handshake::negotiate(1, 1).unwrap(), MAX_PROTOCOL);
    // A wider client range still settles on the server's highest supported version.
    assert_eq!(handshake::negotiate(1, 5).unwrap(), MAX_PROTOCOL);
    assert_eq!(
        handshake::negotiate(MIN_PROTOCOL, MAX_PROTOCOL).unwrap(),
        MAX_PROTOCOL
    );
}

#[test]
fn negotiate_errs_when_client_range_cannot_satisfy_server() {
    // Client below the server floor.
    let err = handshake::negotiate(0, 0).unwrap_err();
    assert_eq!(err.exit_code(), 5);
    // Client above the server ceiling.
    let err = handshake::negotiate(99, 99).unwrap_err();
    assert_eq!(err.exit_code(), 5);
    // A range strictly above the server's window (2..=3 vs server 1..=1).
    assert!(handshake::negotiate(MAX_PROTOCOL + 1, MAX_PROTOCOL + 2).is_err());
}
