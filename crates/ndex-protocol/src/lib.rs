//! `ndex-protocol` — the IPC wire protocol between the thin client and the fat server.
//!
//! Length-prefixed MessagePack frames over stdio (SSH or a local subprocess). This crate
//! holds the message types ([`message`]), framing ([`frame`]), (de)serialization
//! ([`codec`]), and version negotiation ([`handshake`]). It depends only on `ndex-core`,
//! so the thin `ndex` client can speak the protocol without linking any engine crate.

pub mod codec;
pub mod frame;
pub mod handshake;
pub mod message;

pub use codec::{from_slice, to_vec_named};
pub use frame::{FrameReader, FrameWriter};
pub use handshake::{MAX_PROTOCOL, MIN_PROTOCOL, PROTOCOL_VERSION, negotiate};
pub use message::{
    ClientMessage, CorruptedFile, DeleteRequestData, DeleteResultData, ErrorData, FileInfo,
    HandshakeReq, HandshakeResp, IndexCompleteData, IndexOptions, IndexStats, IndexSummary,
    InfoRequestData, InfoResultData, OutputFormat, ProgressChild, ProgressEvent,
    ReindexRequestData, ReindexTarget, SearchHit, SearchRequestData, SearchResultData,
    ServerMessage, StatsResultData, TerminalCaps, VerifyRequestData, VerifyResultData,
};

// Convenience re-exports of the core types that also appear on the wire.
pub use ndex_core::{DocMeta, MediaMeta, NdexPath, SearchFilters, SearchMode};
