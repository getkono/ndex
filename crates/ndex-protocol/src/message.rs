//! IPC message types (PRD §12.4, §12.7).
//!
//! Both message enums are **externally tagged** (serde default — no `#[serde(tag)]`):
//! internally/adjacently tagged enums have known `rmp-serde` deserialization bugs (PRD §12.4).
//! Always serialize via [`crate::codec::to_vec_named`] so struct fields become named map keys.

use ndex_core::{DocMeta, MediaMeta, NdexPath, SearchFilters, SearchMode};
use serde::{Deserialize, Serialize};

/// Output rendering format (PRD §13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Plain,
    Json,
    Jsonl,
    Paths,
    Csv,
}

/// Which index a reindex targets (PRD §12.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReindexTarget {
    #[default]
    All,
    Vectors,
    Fts,
}

/// Client → server messages (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    Handshake(HandshakeReq),
    SearchRequest(SearchRequestData),
    IndexRequest(IndexOptions),
    InfoRequest(InfoRequestData),
    StatsRequest,
    VerifyRequest(VerifyRequestData),
    ReindexRequest(ReindexRequestData),
    DeleteRequest(DeleteRequestData),
    CancelRequest,
}

/// Server → client messages (PRD §12.4).
///
/// Note: version-negotiation failure is reported via [`ServerMessage::Error`] with
/// `code = 5`, not a dedicated `HandshakeErr` variant (skeleton reconciliation of PRD §12.3).
//
// `InfoResult` carries a large `FileInfo` (inline doc/media metadata); the size gap is
// accepted because server messages are constructed and sent one at a time, never stored in
// bulk, and boxing would add friction at every handler/client construction site.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    Handshake(HandshakeResp),
    SearchResult(SearchResultData),
    IndexComplete(IndexCompleteData),
    InfoResult(InfoResultData),
    StatsResult(StatsResultData),
    VerifyResult(VerifyResultData),
    DeleteResult(DeleteResultData),
    Progress(ProgressEvent),
    Error(ErrorData),
}

/// Terminal capabilities advertised by the client (PRD §12.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalCaps {
    pub width: u16,
    pub height: u16,
    pub color: bool,
    pub hyperlinks: bool,
    pub unicode: bool,
}

/// Handshake request: the first frame the client sends (PRD §12.3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HandshakeReq {
    pub min_protocol: u32,
    pub max_protocol: u32,
    pub client_version: String,
    pub capabilities: Vec<String>,
    pub terminal: TerminalCaps,
}

/// Handshake response (PRD §12.3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HandshakeResp {
    pub protocol_version: u32,
    pub server_version: String,
    pub index_schema_version: u32,
    pub index_model: String,
    pub index_file_count: u64,
    pub index_last_reconciled_ns: i64,
    pub capabilities: Vec<String>,
    pub index_healthy: bool,
}

/// Search request payload (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchRequestData {
    pub query: String,
    pub mode: SearchMode,
    pub filters: SearchFilters,
    pub limit: u32,
    pub offset: u32,
    pub format: OutputFormat,
    pub explain: bool,
}

/// Index/reindex options (PRD §12.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexOptions {
    pub full: bool,
    pub verify: bool,
    pub dry_run: bool,
    pub jobs: Option<u32>,
    pub batch_size: Option<u32>,
    pub no_vectors: bool,
    pub enable_ner: bool,
    pub max_file_size: Option<u64>,
    pub only_new: bool,
}

/// `info` request (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoRequestData {
    pub path: NdexPath,
}

/// `verify` request (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VerifyRequestData {
    pub paths: Option<Vec<NdexPath>>,
    pub sample: Option<f64>,
}

/// `reindex` request (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReindexRequestData {
    pub target: ReindexTarget,
}

/// `delete` request (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeleteRequestData {
    pub glob: String,
    pub dry_run: bool,
}

/// One search hit (PRD §12.7).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchHit {
    pub file_id: u64,
    pub chunk_ord: u32,
    pub path: NdexPath,
    pub score: f32,
    pub score_raw: f32,
    pub score_fts: Option<f32>,
    pub score_vec: Option<f32>,
    pub mime: String,
    pub size: u64,
    pub mtime_ns: i64,
    pub tags: Vec<String>,
    pub snippet: Option<String>,
    pub byte_start: u64,
    pub byte_end: u64,
}

/// Search result payload (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchResultData {
    pub hits: Vec<SearchHit>,
    pub total: u64,
    pub mode: SearchMode,
    pub duration_ms: u64,
    pub truncated: bool,
    pub stale_warning: Option<String>,
}

/// Index run statistics (PRD §12.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexStats {
    pub new: u64,
    pub modified: u64,
    pub deleted: u64,
    pub unchanged: u64,
    pub processed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub duration_ms: u64,
    pub timed_out: bool,
}

/// Index completion payload (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexCompleteData {
    pub stats: IndexStats,
    pub cancelled: bool,
}

/// Detailed file info (PRD §12.7). `status` is a raw `u8` on the wire (PRD §12.7);
/// `blake3` is raw bytes, wire-encoded as MessagePack bin via `serde_bytes`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileInfo {
    pub file_id: u64,
    pub path: NdexPath,
    pub size: u64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub mime: Option<String>,
    #[serde(with = "serde_bytes")]
    pub blake3: Option<Vec<u8>>,
    pub status: u8,
    pub fail_count: u32,
    pub error_msg: Option<String>,
    pub tags: Vec<String>,
    pub doc_meta: Option<DocMeta>,
    pub media_meta: Option<MediaMeta>,
    pub chunk_count: u32,
    pub in_fts: bool,
    pub in_vectors: bool,
}

/// `info` result (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoResultData {
    pub file_info: FileInfo,
}

/// Whole-index summary (PRD §12.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexSummary {
    pub total_files: u64,
    pub directories: u64,
    pub indexed: u64,
    pub pending: u64,
    pub failed_transient: u64,
    pub failed_permanent: u64,
    pub skipped: u64,
    pub deleted: u64,
    pub manifest_bytes: u64,
    pub fts_bytes: u64,
    pub vector_bytes: u64,
    pub meta_bytes: u64,
    pub last_reconciled_ns: Option<i64>,
    pub schema_version: u32,
    pub model_name: String,
}

/// `stats` result (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsResultData {
    pub index_stats: IndexSummary,
}

/// A file whose stored hash did not match its recomputed hash (PRD §12.7).
/// Hashes are raw bytes, wire-encoded as MessagePack bin via `serde_bytes`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CorruptedFile {
    pub file_id: u64,
    pub path: NdexPath,
    #[serde(with = "serde_bytes")]
    pub stored_hash: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub actual_hash: Vec<u8>,
}

/// `verify` result (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VerifyResultData {
    pub checked: u64,
    pub corrupted: Vec<CorruptedFile>,
}

/// `delete` result (PRD §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeleteResultData {
    pub deleted: u64,
    pub paths: Vec<NdexPath>,
}

/// A progress sub-task (PRD §13.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProgressChild {
    pub label: String,
    pub current: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// A progress event streamed during long operations (PRD §13.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProgressEvent {
    pub phase: String,
    pub current: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    pub children: Vec<ProgressChild>,
}

/// An error reported by the server (PRD §12.4). `code` mirrors the CLI exit codes (§13.7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorData {
    pub code: u32,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{from_slice, to_vec_named};
    use serde::de::DeserializeOwned;

    fn roundtrip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let bytes = to_vec_named(value).expect("encode");
        let back: T = from_slice(&bytes).expect("decode");
        assert_eq!(*value, back, "round-trip mismatch");
    }

    fn sample_path() -> NdexPath {
        NdexPath::new(vec![0xff, b'/', b'p', b'o', b'o', b'l'])
    }

    fn client_variants() -> Vec<ClientMessage> {
        vec![
            ClientMessage::Handshake(HandshakeReq {
                min_protocol: 1,
                max_protocol: 1,
                client_version: "0.1.0".into(),
                capabilities: vec!["progress".into()],
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
                    ..Default::default()
                },
                limit: 20,
                offset: 0,
                format: OutputFormat::Json,
                explain: true,
            }),
            ClientMessage::IndexRequest(IndexOptions {
                full: true,
                jobs: Some(4),
                max_file_size: Some(1 << 30),
                ..Default::default()
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
                stale_warning: None,
            }),
            ServerMessage::IndexComplete(IndexCompleteData {
                stats: IndexStats {
                    new: 10,
                    processed: 10,
                    duration_ms: 500,
                    ..Default::default()
                },
                cancelled: false,
            }),
            ServerMessage::InfoResult(InfoResultData {
                file_info: FileInfo {
                    file_id: 7,
                    path: sample_path(),
                    blake3: Some(vec![0u8; 32]),
                    status: 1,
                    doc_meta: Some(DocMeta {
                        title: Some("Report".into()),
                        ..Default::default()
                    }),
                    chunk_count: 12,
                    in_fts: true,
                    in_vectors: true,
                    ..Default::default()
                },
            }),
            ServerMessage::StatsResult(StatsResultData {
                index_stats: IndexSummary {
                    total_files: 1_000_000,
                    indexed: 999_000,
                    schema_version: 3,
                    model_name: "snowflake-arctic-embed-m-v2.0".into(),
                    last_reconciled_ns: Some(1_700_000_000_000_000_000),
                    ..Default::default()
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
    fn unit_variants_roundtrip() {
        roundtrip(&ClientMessage::StatsRequest);
        roundtrip(&ClientMessage::CancelRequest);
    }
}
