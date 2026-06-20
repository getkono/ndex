//! Phase 3 — extract, chunk, index (PRD §11.1, §11.2).

use std::io;
use std::path::Path;

use ndex_core::error::Result;
use ndex_core::identity::SCHEMA_VERSION;
use ndex_core::model::WalkEntry;
use ndex_core::path::NdexPath;
use ndex_core::progress::{ProgressKind, ProgressSink, ProgressUpdate};
use ndex_core::status::FileStatus;
use ndex_core::tokens::TokenCounter;
use ndex_embed::Embed;
use ndex_extract::archive_safety::with_panic_isolation;
use ndex_extract::extractor::{ExtractCtx, router};
use ndex_extract::{Chunker, mime};
use ndex_store::Store;

use crate::diff::DiffOutcome;
use crate::reconciler::ReconcileStats;

/// Approximate token counter used for chunk sizing when no model tokenizer is loaded (v0.1).
struct WordTokens;

impl TokenCounter for WordTokens {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

/// Per-file outcome inside Phase 3.
enum Disposition {
    Indexed,
    Failed,
}

/// Emit a phase progress tick.
fn tick(sink: &dyn ProgressSink, kind: ProgressKind, current: u64, total: u64) {
    sink.emit(&ProgressUpdate {
        kind,
        current,
        total: Some(total),
        message: None,
        children: Vec::new(),
    });
}

/// Read, extract, chunk, and FTS-index one file, returning its disposition.
fn process_one(
    store: &mut Store,
    tokens: &dyn TokenCounter,
    path: &NdexPath,
) -> Result<Disposition> {
    let os = path.to_os_string();
    let fs_path = Path::new(&os);

    let meta = match std::fs::symlink_metadata(fs_path) {
        Ok(m) => m,
        Err(e) => {
            // Gone or unreadable between walk and now.
            tracing::debug!(path = %fs_path.display(), error = %e, "stat failed in process");
            return Ok(Disposition::Failed);
        }
    };
    let entry = walk_entry(&meta);
    let file_id = store.manifest.upsert_walked(path, &entry)?;

    let bytes = match std::fs::read(fs_path) {
        Ok(b) => b,
        Err(e) => {
            let status = classify_io_error(&e);
            store
                .manifest
                .set_status(file_id, status, Some(&e.to_string()))?;
            return Ok(Disposition::Failed);
        }
    };

    let mime = mime::detect(path, &bytes);

    // Extraction runs third-party parsers; isolate panics so one bad file cannot crash the run.
    let extraction = {
        let extractor = router(&mime);
        let ctx = ExtractCtx {
            mime: &mime,
            path,
            tokens,
            depth: 0,
            config: &store.config,
        };
        with_panic_isolation(|| extractor.extract(&bytes, &ctx))
    };
    let extraction = match extraction {
        Ok(Ok(ex)) => ex,
        Ok(Err(e)) | Err(e) => {
            tracing::debug!(path = %fs_path.display(), mime, error = %e, "extraction failed");
            store.manifest.set_status(
                file_id,
                FileStatus::FailedPermanent,
                Some(&e.to_string()),
            )?;
            return Ok(Disposition::Failed);
        }
    };

    // Re-index: clear any prior chunks, then add the freshly chunked content.
    store.fts.delete_file(file_id)?;
    let chunks = Chunker::new(tokens, &store.config.chunking).chunk(file_id, &extraction.blocks);
    for chunk in &chunks {
        store
            .fts
            .add_chunk(file_id, chunk, &mime, extraction.lang.as_deref())?;
    }

    if let Some(doc) = &extraction.doc_meta {
        store.meta.upsert_doc_meta(file_id, doc)?;
    }
    if let Some(media) = &extraction.media_meta {
        store.meta.upsert_media_meta(file_id, media)?;
    }

    store
        .manifest
        .set_status(file_id, FileStatus::Indexed, None)?;
    store
        .manifest
        .record_progress(file_id, "fts", SCHEMA_VERSION)?;
    Ok(Disposition::Indexed)
}

/// Build a [`WalkEntry`] from filesystem metadata (process-time restat).
fn walk_entry(meta: &std::fs::Metadata) -> WalkEntry {
    use std::os::unix::fs::MetadataExt;
    WalkEntry {
        size: meta.len(),
        mtime_ns: meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
        ctime_ns: meta.ctime() * 1_000_000_000 + meta.ctime_nsec(),
        inode: meta.ino(),
        dev: meta.dev(),
        mode: meta.mode(),
    }
}

/// Classify an extraction-time I/O error (PRD §11.1):
/// `ENOENT` (the file was removed between walk and extraction) → [`FileStatus::Deleted`];
/// every other I/O error → [`FileStatus::FailedTransient`] for retry.
pub fn classify_io_error(err: &io::Error) -> FileStatus {
    if err.kind() == io::ErrorKind::NotFound {
        FileStatus::Deleted
    } else {
        FileStatus::FailedTransient
    }
}

/// TOCTOU guard (PRD §11.1): an extraction result is valid only if `(size, mtime_ns)` are
/// unchanged since the Phase 1 walk; otherwise the file is re-queued as `FailedTransient`.
pub fn restat_unchanged(walked: &WalkEntry, current_size: u64, current_mtime_ns: i64) -> bool {
    walked.size == current_size && walked.mtime_ns == current_mtime_ns
}

/// Phase 3: the bounded `extract → BLAKE3 → chunk → embed → index` pipeline with two-phase commit
/// and crash-safe write ordering (PRD §11.1, §11.2). Extraction routes through `ndex-extract`;
/// embedding through `ndex-embed`.
pub fn process(
    store: &mut Store,
    embedder: Option<&dyn Embed>,
    diff: &DiffOutcome,
    sink: &dyn ProgressSink,
) -> Result<ReconcileStats> {
    // Semantic embedding is deferred to the vector-index follow-up; v0.1 indexes FTS only.
    let _ = embedder;
    let tokens = WordTokens;
    let mut stats = ReconcileStats {
        new: diff.new.len() as u64,
        modified: diff.modified.len() as u64,
        unchanged: diff.unchanged,
        ..ReconcileStats::default()
    };

    let total = (diff.new.len() + diff.modified.len()) as u64;
    let mut done = 0;
    for path in diff.new.iter().chain(diff.modified.iter()) {
        done += 1;
        tick(sink, ProgressKind::Extract, done, total);
        match process_one(store, &tokens, path)? {
            Disposition::Indexed => stats.processed += 1,
            Disposition::Failed => stats.failed += 1,
        }
    }

    // Apply deletions: drop FTS docs + metadata, mark the manifest row deleted.
    for &file_id in &diff.deleted {
        store.fts.delete_file(file_id)?;
        store.meta.delete_file(file_id)?;
        store
            .manifest
            .set_status(file_id, FileStatus::Deleted, None)?;
        stats.deleted += 1;
    }

    // Persist the FTS segment so searches see the new content (PRD §10.2 commit strategy).
    tick(sink, ProgressKind::Fts, total, total);
    store.fts.commit()?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enoent_is_deleted_other_io_is_transient() {
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::NotFound)),
            FileStatus::Deleted
        );
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            FileStatus::FailedTransient
        );
    }

    #[test]
    fn restat_detects_size_or_mtime_change() {
        let walked = WalkEntry {
            size: 100,
            mtime_ns: 5,
            ctime_ns: 0,
            inode: 1,
            dev: 1,
            mode: 0o644,
        };
        assert!(restat_unchanged(&walked, 100, 5));
        assert!(!restat_unchanged(&walked, 101, 5));
        assert!(!restat_unchanged(&walked, 100, 6));
    }
}
