//! Characterization tests for the public `ndex-core` interface.
//!
//! Black-box: exercises only the crate's public API, pinning the observable contract of every
//! real function. `todo!()` interfaces get full-assertion contract tests marked
//! `#[ignore = "impl pending: PR #3"]` so CI stays green while the spec is recorded.
//!
//! `ndex-core` is wire-agnostic, so round-trips here go through `serde_json`; the MessagePack
//! `bin` distinction for [`NdexPath`] is characterized in `ndex-protocol`.

use std::collections::{BTreeSet, HashMap};

use ndex_core::config::{ByteSize, Config, DurationSetting};
use ndex_core::constants;
use ndex_core::error::NdexError;
use ndex_core::filters::{SearchFilters, SearchMode};
use ndex_core::identity::{
    EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity, SCHEMA_VERSION,
};
use ndex_core::model::{
    ArchiveMeta, Block, BlockType, Chunk, DocMeta, Embedding, FileRecord, MediaMeta,
};
use ndex_core::path::NdexPath;
use ndex_core::progress::{
    NullSink, ProgressChildUpdate, ProgressKind, ProgressSink, ProgressUpdate,
};
use ndex_core::status::{FileStatus, InvalidFileStatus};
use ndex_core::tokens::TokenCounter;

/// Assert a value survives a `serde_json` round-trip unchanged.
fn json_roundtrip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, value, "round-trip changed the value (json={text})");
}

// ---------------------------------------------------------------------------
// ByteSize — every documented unit, plus the error surface.
// ---------------------------------------------------------------------------

#[test]
fn bytesize_decimal_and_binary_units() {
    let cases = [
        ("0", 0u64),
        ("1024", 1024),
        ("1b", 1),
        ("1B", 1),
        ("1k", 1_000),
        ("1kb", 1_000),
        ("1KB", 1_000),
        ("1kib", 1 << 10),
        ("1KiB", 1 << 10),
        ("1m", 1_000_000),
        ("1mb", 1_000_000),
        ("1mib", 1 << 20),
        ("1g", 1_000_000_000),
        ("1gb", 1_000_000_000),
        ("2GiB", 2 << 30),
        ("1t", 1_000_000_000_000),
        ("1tib", 1 << 40),
    ];
    for (input, want) in cases {
        assert_eq!(
            input.parse::<ByteSize>().unwrap(),
            ByteSize(want),
            "parsing {input:?}"
        );
    }
}

#[test]
fn bytesize_accepts_fractional_and_whitespace() {
    assert_eq!("1.5KiB".parse::<ByteSize>().unwrap(), ByteSize(1536));
    assert_eq!("  2 GiB  ".parse::<ByteSize>().unwrap(), ByteSize(2 << 30));
    assert_eq!(ByteSize(4096).bytes(), 4096);
}

#[test]
fn bytesize_rejects_garbage() {
    assert!("".parse::<ByteSize>().is_err());
    assert!("nope".parse::<ByteSize>().is_err());
    assert!("12zz".parse::<ByteSize>().is_err());
}

#[test]
fn bytesize_serializes_as_raw_u64() {
    // Serde representation is a bare integer count of bytes, not a struct or string.
    assert_eq!(serde_json::to_string(&ByteSize(2048)).unwrap(), "2048");
    let back: ByteSize = serde_json::from_str("2048").unwrap();
    assert_eq!(back, ByteSize(2048));
    // It also deserializes from a human string (config ergonomics).
    let from_str: ByteSize = serde_json::from_str("\"2KiB\"").unwrap();
    assert_eq!(from_str, ByteSize(2048));
    assert!(serde_json::from_str::<ByteSize>("-1").is_err());
}

// ---------------------------------------------------------------------------
// DurationSetting — every documented unit.
// ---------------------------------------------------------------------------

#[test]
fn duration_units() {
    let cases = [
        ("30", 30u64),
        ("45s", 45),
        ("45sec", 45),
        ("2m", 120),
        ("2min", 120),
        ("1h", 3_600),
        ("1hr", 3_600),
        ("7d", 604_800),
        ("7day", 604_800),
        ("2w", 1_209_600),
        ("2wk", 1_209_600),
    ];
    for (input, want_secs) in cases {
        assert_eq!(
            input.parse::<DurationSetting>().unwrap().secs(),
            want_secs,
            "parsing {input:?}"
        );
    }
}

#[test]
fn duration_as_duration_and_serde() {
    let d: DurationSetting = "1h".parse().unwrap();
    assert_eq!(d.as_duration(), std::time::Duration::from_secs(3_600));
    assert_eq!(serde_json::to_string(&d).unwrap(), "3600");
    let back: DurationSetting = serde_json::from_str("\"1h\"").unwrap();
    assert_eq!(back, d);
    assert!("".parse::<DurationSetting>().is_err());
    assert!("5fortnights".parse::<DurationSetting>().is_err());
}

// ---------------------------------------------------------------------------
// Config — defaults, partial parse, full round-trip.
// ---------------------------------------------------------------------------

#[test]
fn config_default_matches_prd_section_17() {
    let c = Config::default();
    assert_eq!(c.chunking.target_tokens, 512);
    assert_eq!(c.chunking.overlap_tokens, 128);
    assert_eq!(c.chunking.min_tokens, 32);
    assert!(c.chunking.heading_prefix);
    assert_eq!(c.extraction.max_file_size.bytes(), 2 << 30);
    assert_eq!(c.extraction.max_retries, 3);
    assert_eq!(c.embedding.batch_size, 64);
    assert_eq!(c.embedding.inter_op_threads, 1);
    assert!(c.auto_refresh.enabled);
    assert_eq!(c.auto_refresh.threshold.secs(), 3_600);
    assert_eq!(c.auto_refresh.warn_threshold.secs(), 604_800);
    assert!(c.ignore.respect_gitignore);
    assert!(c.ignore.respect_ndexignore);
    assert!(c.walk.follow_symlinks);
    assert!(c.walk.hidden);
    assert_eq!(c.search.default_limit, 20);
    assert_eq!(c.search.rrf_k, 60);
    assert_eq!(c.search.title_boost, 2.0);
    assert_eq!(c.search.fts_weight, 1.0);
    assert_eq!(c.search.ef_search, 128);
    assert_eq!(c.archive.max_archive_total_size.bytes(), 8 << 30);
    assert_eq!(c.archive.max_archive_members, 100_000);
    assert_eq!(c.archive.max_archive_depth, 3);
    assert_eq!(c.archive.compression_ratio_limit, 200);
}

#[test]
fn config_empty_toml_is_all_defaults() {
    assert_eq!(Config::from_toml("").unwrap(), Config::default());
}

#[test]
fn config_partial_toml_fills_defaults_and_roundtrips() {
    let cfg = Config::from_toml(
        r#"
        [chunking]
        target_tokens = 256

        [search]
        rrf_k = 42
        "#,
    )
    .unwrap();
    assert_eq!(cfg.chunking.target_tokens, 256);
    assert_eq!(cfg.search.rrf_k, 42);
    // Untouched keys keep their defaults.
    assert_eq!(cfg.chunking.overlap_tokens, 128);
    assert_eq!(cfg.archive.max_archive_members, 100_000);
    // Full round-trip is lossless.
    let round = Config::from_toml(&cfg.to_toml().unwrap()).unwrap();
    assert_eq!(cfg, round);
}

#[test]
fn config_invalid_toml_is_config_error() {
    let err = Config::from_toml("this is not = = toml").unwrap_err();
    assert!(matches!(err, NdexError::Config(_)));
    assert_eq!(err.exit_code(), 78);
}

// ---------------------------------------------------------------------------
// NdexError — exhaustive exit-code mapping (PRD §13.7).
// ---------------------------------------------------------------------------

#[test]
fn every_error_variant_maps_to_documented_exit_code() {
    let io = NdexError::Io(std::io::Error::other("x"));
    assert_eq!(io.exit_code(), 1);
    assert_eq!(NdexError::IndexNotFound("x".into()).exit_code(), 3);
    assert_eq!(NdexError::RemoteConnection("x".into()).exit_code(), 4);
    assert_eq!(NdexError::Nfs("x".into()).exit_code(), 4);
    assert_eq!(NdexError::VersionIncompatible("x".into()).exit_code(), 5);
    assert_eq!(NdexError::SchemaMismatch("x".into()).exit_code(), 6);
    assert_eq!(NdexError::NoResults.exit_code(), 7);
    assert_eq!(NdexError::Config("x".into()).exit_code(), 78);
    assert_eq!(NdexError::Interrupted.exit_code(), 130);
    // The generic / engine families collapse to 1.
    for e in [
        NdexError::Protocol("x".into()),
        NdexError::ExtractionTransient("x".into()),
        NdexError::ExtractionPermanent("x".into()),
        NdexError::Unsupported("x".into()),
        NdexError::TooLarge("x".into()),
        NdexError::Encoding("x".into()),
        NdexError::Model("x".into()),
        NdexError::Index("x".into()),
        NdexError::Lock("x".into()),
        NdexError::Other("x".into()),
    ] {
        assert_eq!(e.exit_code(), 1, "{e:?}");
    }
}

#[test]
fn io_errors_convert_via_from() {
    let e: NdexError = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
    assert!(matches!(e, NdexError::Io(_)));
    assert!(e.to_string().contains("missing"));
}

// ---------------------------------------------------------------------------
// NdexPath — bytes in, bytes out; serde preserves non-UTF-8; usable as a key.
// ---------------------------------------------------------------------------

#[test]
fn ndexpath_accessors_roundtrip() {
    let raw = vec![b'/', 0xff, b'a'];
    let p = NdexPath::new(raw.clone());
    assert_eq!(p.as_bytes(), &raw[..]);
    assert_eq!(p.clone().into_bytes(), raw);
}

#[test]
fn ndexpath_serde_preserves_non_utf8() {
    let p = NdexPath::new(vec![0xff, b'/', 0x80, 0xc3, 0x28]);
    json_roundtrip(&p);
}

#[test]
fn ndexpath_display_is_lossy() {
    assert_eq!(
        NdexPath::new(vec![b'a', 0xff, b'b']).display_lossy(),
        "a\u{fffd}b"
    );
}

#[test]
fn ndexpath_hash_is_deterministic_and_discriminating() {
    let a = NdexPath::new(b"/pool/a".to_vec());
    let b = NdexPath::new(b"/pool/b".to_vec());
    assert_eq!(
        a.path_hash(),
        NdexPath::new(b"/pool/a".to_vec()).path_hash()
    );
    assert_ne!(a.path_hash(), b.path_hash());
}

#[test]
fn ndexpath_orders_and_hashes_by_bytes() {
    let mut set = BTreeSet::new();
    set.insert(NdexPath::new(b"/b".to_vec()));
    set.insert(NdexPath::new(b"/a".to_vec()));
    set.insert(NdexPath::new(b"/a".to_vec())); // duplicate
    let ordered: Vec<_> = set.iter().map(|p| p.as_bytes().to_vec()).collect();
    assert_eq!(ordered, vec![b"/a".to_vec(), b"/b".to_vec()]);

    let mut map = HashMap::new();
    map.insert(NdexPath::new(b"/k".to_vec()), 7);
    assert_eq!(map.get(&NdexPath::new(b"/k".to_vec())), Some(&7));
}

#[cfg(unix)]
#[test]
fn ndexpath_os_str_roundtrip_preserves_invalid_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let os = std::ffi::OsStr::from_bytes(&[b'/', 0xff, b'x']);
    let p = NdexPath::from_os_str(os);
    assert_eq!(p.to_os_string(), os);
}

#[test]
#[ignore = "impl pending: PR #3"]
fn ndexpath_json_escaping_contract() {
    // PRD §8: byte-preserving JSON escaping. ASCII passes through; bytes that are not valid
    // UTF-8 are emitted as `\u00XX` (lowercase hex) so the rendering is reversible.
    assert_eq!(
        NdexPath::new(b"/plain/path".to_vec()).to_json_escaped(),
        "/plain/path"
    );
    assert_eq!(
        NdexPath::new(b"a\"b\\c".to_vec()).to_json_escaped(),
        "a\\\"b\\\\c"
    );
    assert_eq!(
        NdexPath::new(vec![b'a', 0xff, b'b']).to_json_escaped(),
        "a\\u00ffb"
    );
}

// ---------------------------------------------------------------------------
// FileStatus — discriminants, fallible conversion, bare-integer serde.
// ---------------------------------------------------------------------------

#[test]
fn filestatus_discriminants_are_stable() {
    assert_eq!(FileStatus::Pending.as_u8(), 0);
    assert_eq!(FileStatus::Indexed.as_u8(), 1);
    assert_eq!(FileStatus::FailedTransient.as_u8(), 2);
    assert_eq!(FileStatus::Deleted.as_u8(), 3);
    assert_eq!(FileStatus::FailedPermanent.as_u8(), 4);
    assert_eq!(FileStatus::Skipped.as_u8(), 5);
}

#[test]
fn filestatus_try_from_roundtrips_and_rejects_unknown() {
    for v in 0u8..=5 {
        assert_eq!(FileStatus::try_from(v).unwrap().as_u8(), v);
    }
    assert_eq!(FileStatus::try_from(6), Err(InvalidFileStatus(6)));
    assert_eq!(FileStatus::try_from(255), Err(InvalidFileStatus(255)));
    assert!(InvalidFileStatus(9).to_string().contains('9'));
}

#[test]
fn filestatus_serde_is_a_bare_integer() {
    assert_eq!(serde_json::to_string(&FileStatus::Skipped).unwrap(), "5");
    assert_eq!(
        serde_json::from_str::<FileStatus>("2").unwrap(),
        FileStatus::FailedTransient
    );
    assert!(serde_json::from_str::<FileStatus>("99").is_err());
}

// ---------------------------------------------------------------------------
// Domain model — serde round-trips through the derived (de)serializers.
// ---------------------------------------------------------------------------

fn sample_record() -> FileRecord {
    FileRecord {
        file_id: 42,
        path: NdexPath::new(vec![b'/', 0xff, b'f']),
        path_hash: 0xdead_beef,
        inode: Some(7),
        dev: Some(2049),
        size: 1234,
        mtime_ns: 1_700_000_000_000_000_000,
        ctime_ns: 1_700_000_000_000_000_001,
        mode: 0o100644,
        uid: Some(1000),
        gid: Some(1000),
        blake3: Some([0xab; 32]),
        mime_type: Some("text/plain".into()),
        status: FileStatus::Indexed,
        fail_count: 0,
        first_seen_ns: 1,
        last_verified_ns: 2,
        error_msg: None,
        hard_link_of: None,
        parent_archive_id: None,
    }
}

#[test]
fn file_record_roundtrips() {
    json_roundtrip(&sample_record());
}

#[test]
fn block_types_all_roundtrip() {
    for bt in [
        BlockType::Heading(2),
        BlockType::Paragraph,
        BlockType::CodeBlock(Some("rust".into())),
        BlockType::CodeBlock(None),
        BlockType::ListItem,
        BlockType::Table,
        BlockType::Quote,
        BlockType::Raw,
    ] {
        let block = Block {
            block_type: bt,
            text: "body".into(),
            byte_start: 0,
            byte_end: 4,
            heading_path: vec!["Top".into(), "Sub".into()],
        };
        json_roundtrip(&block);
    }
}

#[test]
fn chunk_and_meta_roundtrip() {
    json_roundtrip(&Chunk {
        file_id: 1,
        chunk_ord: 3,
        byte_start: 10,
        byte_end: 20,
        block_type: BlockType::Paragraph,
        text: "hello".into(),
    });
    json_roundtrip(&DocMeta {
        title: Some("Q3 Report".into()),
        page_count: Some(12),
        ..DocMeta::default()
    });
    json_roundtrip(&MediaMeta {
        width: Some(4032),
        height: Some(3024),
        lens: Some("EF24-70".into()),
        gps_lat: Some(37.5),
        ..MediaMeta::default()
    });
    json_roundtrip(&ArchiveMeta {
        member_count: Some(3),
        format: Some("zip".into()),
        extraction_status: Some("complete".into()),
        ..ArchiveMeta::default()
    });
}

#[test]
fn embedding_dims_reports_length() {
    use half::f16;
    let e = Embedding(vec![f16::from_f32(0.1), f16::from_f32(0.2)]);
    assert_eq!(e.dims(), 2);
    assert_eq!(Embedding(vec![]).dims(), 0);
}

// ---------------------------------------------------------------------------
// IndexIdentity — TOML round-trip + schema gate.
// ---------------------------------------------------------------------------

fn sample_identity() -> IndexIdentity {
    IndexIdentity {
        identity: Identity {
            schema_version: SCHEMA_VERSION,
            created_by: "ndex-remote 0.1.0".into(),
            created_at: "2026-06-19T00:00:00Z".into(),
        },
        embedding: EmbeddingIdentity {
            model_name: constants::DEFAULT_MODEL.into(),
            model_hash: "abc123".into(),
            dimensions: 768,
            mrl_dimensions: 256,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        },
        hashing: Hashing {
            algorithm: "blake3".into(),
        },
        fts: FtsIdentity {
            tokenizer_version: 1,
        },
    }
}

#[test]
fn identity_toml_roundtrip() {
    let id = sample_identity();
    let round: IndexIdentity = toml::from_str(&id.to_toml().unwrap()).unwrap();
    assert_eq!(id, round);
}

#[test]
fn identity_schema_gate() {
    let mut id = sample_identity();
    assert!(id.check_compatible().is_ok());
    id.identity.schema_version = SCHEMA_VERSION + 1;
    let err = id.check_compatible().unwrap_err();
    assert!(matches!(err, NdexError::SchemaMismatch(_)));
    assert_eq!(err.exit_code(), 6);
}

// ---------------------------------------------------------------------------
// SearchMode / SearchFilters — variant-name serde + defaults.
// ---------------------------------------------------------------------------

#[test]
fn search_mode_default_is_auto_and_serializes_by_name() {
    assert_eq!(SearchMode::default(), SearchMode::Auto);
    assert_eq!(
        serde_json::to_string(&SearchMode::Hybrid).unwrap(),
        "\"Hybrid\""
    );
    assert_eq!(
        serde_json::from_str::<SearchMode>("\"Semantic\"").unwrap(),
        SearchMode::Semantic
    );
}

#[test]
fn search_filters_default_is_empty_and_roundtrips() {
    let f = SearchFilters::default();
    assert!(f.mime.is_none() && f.tags.is_empty() && f.lang.is_none());
    json_roundtrip(&SearchFilters {
        mime: Some("image/*".into()),
        after_ns: Some(100),
        larger: Some(1024),
        path_glob: Some("invoices/**/*.pdf".into()),
        tags: vec!["work".into(), "2024".into()],
        ..SearchFilters::default()
    });
}

// ---------------------------------------------------------------------------
// Progress — sink dispatch + update serde.
// ---------------------------------------------------------------------------

#[test]
fn null_sink_swallows_updates() {
    NullSink.emit(&ProgressUpdate {
        kind: ProgressKind::Walk,
        current: 1,
        total: Some(10),
        message: None,
        children: vec![],
    });
}

#[test]
fn custom_sink_receives_updates() {
    use std::sync::Mutex;
    #[derive(Default)]
    struct Collector(Mutex<Vec<ProgressKind>>);
    impl ProgressSink for Collector {
        fn emit(&self, u: &ProgressUpdate) {
            self.0.lock().unwrap().push(u.kind);
        }
    }
    let c = Collector::default();
    for kind in [
        ProgressKind::Walk,
        ProgressKind::Extract,
        ProgressKind::Embed,
    ] {
        c.emit(&ProgressUpdate {
            kind,
            current: 0,
            total: None,
            message: Some("tick".into()),
            children: vec![ProgressChildUpdate {
                label: "worker-0".into(),
                current: 0,
                total: None,
                message: None,
            }],
        });
    }
    assert_eq!(c.0.lock().unwrap().len(), 3);
}

#[test]
fn progress_update_roundtrips() {
    json_roundtrip(&ProgressUpdate {
        kind: ProgressKind::Fts,
        current: 5,
        total: Some(20),
        message: Some("indexing".into()),
        children: vec![],
    });
}

// ---------------------------------------------------------------------------
// Constants — the wire/layout invariants other crates rely on.
// ---------------------------------------------------------------------------

#[test]
fn constants_are_pinned() {
    assert_eq!(constants::MAGIC_PREAMBLE, b"NDEX\x00\x01");
    assert_eq!(constants::MAX_FRAME_BYTES, 16 * 1024 * 1024);
    assert_eq!(constants::MAX_PREAMBLE_SCAN_BYTES, 4096);
    assert_eq!(constants::NDEX_DIR, ".ndex");
    assert_eq!(constants::LOCK_FILE, "lock");
    assert_eq!(constants::QUERY_PREFIX, "query: ");
    assert!(constants::DEFAULT_MODEL.contains("arctic"));
}

// ---------------------------------------------------------------------------
// TokenCounter — trait is object-safe and usable behind a reference.
// ---------------------------------------------------------------------------

#[test]
fn token_counter_is_object_safe() {
    struct Whitespace;
    impl TokenCounter for Whitespace {
        fn count(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }
    }
    let counter: &dyn TokenCounter = &Whitespace;
    assert_eq!(counter.count("a b c"), 3);
    assert_eq!(counter.count(""), 0);
}
