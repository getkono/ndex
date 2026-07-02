//! Core domain types shared across the engine crates.

use half::f16;
use serde::{Deserialize, Serialize};

use crate::path::NdexPath;
use crate::status::FileStatus;

/// A 32-byte BLAKE3 content hash (PRD §4).
pub type Blake3 = [u8; 32];

/// One row of the manifest `files` table (PRD §10.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: i64,
    pub path: NdexPath,
    pub path_hash: u64,
    pub inode: Option<u64>,
    pub dev: Option<u64>,
    pub size: u64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub mode: u32,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub blake3: Option<Blake3>,
    pub mime_type: Option<String>,
    pub status: FileStatus,
    pub fail_count: u32,
    pub first_seen_ns: i64,
    pub last_verified_ns: i64,
    pub error_msg: Option<String>,
    /// Canonical `file_id` if this path is a hard link (PRD §11.1); else `None`.
    pub hard_link_of: Option<i64>,
    /// Owning archive's `file_id` if this is an archive member; else `None`.
    pub parent_archive_id: Option<i64>,
}

/// Filesystem metadata captured for a regular file during Phase 1 walk (PRD §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkEntry {
    pub size: u64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub inode: u64,
    pub dev: u64,
    pub mode: u32,
}

/// Filesystem metadata captured for a directory during Phase 1 walk (PRD §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirWalkEntry {
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub inode: u64,
    pub dev: u64,
    pub mode: u32,
}

/// A normalized extracted block (PRD §4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockType {
    Heading(u8),
    Paragraph,
    CodeBlock(Option<String>),
    ListItem,
    Table,
    Quote,
    Raw,
}

/// An ordered, typed block produced by an extractor, with source byte offsets (PRD §4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub block_type: BlockType,
    pub text: String,
    pub byte_start: u64,
    pub byte_end: u64,
    /// Most-recent heading context, propagated to chunks (PRD §4.5).
    pub heading_path: Vec<String>,
}

/// A chunk of text to be indexed and embedded (PRD §4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub file_id: i64,
    pub chunk_ord: u32,
    pub byte_start: u64,
    pub byte_end: u64,
    pub block_type: BlockType,
    pub text: String,
}

/// The result of running a file through the extraction pipeline (PRD §4.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessedFile {
    pub blake3: Blake3,
    pub mime_type: String,
    pub chunks: Vec<Chunk>,
    pub doc_meta: Option<DocMeta>,
    pub media_meta: Option<MediaMeta>,
    pub lang: Option<String>,
}

/// Extracted document metadata (PRD §10.4 `doc_meta`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub page_count: Option<u32>,
    pub word_count: Option<u32>,
    pub lang: Option<String>,
}

/// Image/video/audio metadata (PRD §10.4 `media_meta`).
///
/// `lens` is present here and in the wire `MediaMeta` (skeleton reconciliation of PRD §12.7).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaMeta {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub fps: Option<f32>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub focal_length: Option<f32>,
    pub aperture: Option<f32>,
    pub shutter_speed: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub gps_alt: Option<f64>,
    pub taken_at: Option<String>,
}

/// Summary metadata for an archive file (PRD §10.4 `archive_meta`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMeta {
    pub member_count: Option<u32>,
    pub total_size: Option<u64>,
    pub format: Option<String>,
    /// `complete` | `partial` | `metadata_only` (PRD §10.4).
    pub extraction_status: Option<String>,
}

/// A 256-dimensional, MRL-truncated, L2-normalized embedding stored as `f16` (PRD §10.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embedding(pub Vec<f16>);

impl Embedding {
    /// Number of stored dimensions.
    pub fn dims(&self) -> usize {
        self.0.len()
    }
}
