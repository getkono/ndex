//! Phase 3 — extract, chunk, index (PRD §11.1, §11.2).
//!
//! # Crash-safety invariant
//!
//! `status = Indexed` implies the file's chunks are durably committed in the FTS index.
//! The pipeline enforces this by ordering: chunks are staged into tantivy, then
//! `fts.commit()` runs **first**, and only afterwards are the batch's manifest statuses
//! flipped to `Indexed` ([`Manifest::mark_indexed`]) — every [`BATCH_COMMIT_FILES`] files
//! and once at end of run. A crash on either side of the commit leaves the affected files
//! `Pending`; they are reprocessed on the next run, where delete-before-add
//! (`fts.delete_file` before `add_chunk`) prevents duplicate chunks.

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
use ndex_extract::extractor::{ExtractCtx, Route, router};
use ndex_extract::{Chunker, mime};
use ndex_store::{FtsFileMeta, Store};

use crate::diff::DiffOutcome;
use crate::reconciler::{ReconcileOptions, ReconcileStats};

/// Successfully processed files are FTS-committed and status-flipped in batches of this
/// many files, bounding how much work a crash can lose (PRD §11.2).
pub const BATCH_COMMIT_FILES: usize = 100;

/// Approximate token counter used for chunk sizing when no model tokenizer is loaded (v0.1).
struct WordTokens;

impl TokenCounter for WordTokens {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

/// Per-file outcome inside Phase 3.
enum Disposition {
    /// Chunks staged in the FTS writer; status flip deferred to the post-commit batch.
    Indexed {
        file_id: i64,
        blake3: [u8; 32],
    },
    /// Intentionally not indexed (no extractor for the MIME, or over `max_file_size`).
    Skipped,
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

/// Mark one file `Skipped`: purge any stale chunks from a previously indexed version,
/// record the reason, and drop its progress rows. No content is written to the FTS.
fn skip_file(store: &mut Store, file_id: i64, reason: &str) -> Result<Disposition> {
    store.fts.delete_file(file_id)?;
    store
        .manifest
        .set_status(file_id, FileStatus::Skipped, Some(reason))?;
    store.manifest.clear_progress(file_id)?;
    Ok(Disposition::Skipped)
}

/// Read, hash, extract, chunk, and stage one file into the FTS writer, returning its
/// disposition. Does **not** flip the manifest status to `Indexed` — that happens in the
/// post-commit batch (see the module-level invariant).
fn process_one(
    store: &mut Store,
    tokens: &dyn TokenCounter,
    path: &NdexPath,
    max_file_size: u64,
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

    // Enforce `max_file_size` before reading (PRD §11.5: too large → Skipped, no retry).
    if entry.size > max_file_size {
        tracing::debug!(path = %fs_path.display(), size = entry.size, max_file_size, "skipping oversized file");
        return skip_file(
            store,
            file_id,
            &format!(
                "file size {} exceeds max_file_size {max_file_size}",
                entry.size
            ),
        );
    }

    let bytes = match std::fs::read(fs_path) {
        Ok(b) => b,
        Err(e) => {
            let status = classify_io_error(&e);
            tracing::warn!(path = %fs_path.display(), error = %e, "read failed");
            store
                .manifest
                .set_status(file_id, status, Some(&e.to_string()))?;
            return Ok(Disposition::Failed);
        }
    };

    let blake3 = *blake3::hash(&bytes).as_bytes();
    let mime = mime::detect(path, &bytes);

    // Extraction runs third-party parsers; isolate panics so one bad file cannot crash the run.
    let extractor = match router(&mime) {
        Route::Extract(extractor) => extractor,
        Route::Skip => {
            tracing::debug!(path = %fs_path.display(), mime, "no extractor for mime; skipping");
            return skip_file(store, file_id, &format!("no extractor for mime {mime}"));
        }
    };
    let extraction = {
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
            // PRD §11.5: extraction errors are transient for the first `max_retries`
            // attempts; promotion to FailedPermanent happens at the start of a later run.
            tracing::warn!(path = %fs_path.display(), mime, error = %e, "extraction failed");
            store.manifest.set_status(
                file_id,
                FileStatus::FailedTransient,
                Some(&e.to_string()),
            )?;
            return Ok(Disposition::Failed);
        }
    };

    // Re-index: clear any prior chunks, then add the freshly chunked content
    // (delete-before-add keeps reprocessing idempotent — see module docs).
    store.fts.delete_file(file_id)?;
    let chunks = Chunker::new(tokens, &store.config.chunking).chunk(file_id, &extraction.blocks);
    let display = path.display_lossy();
    let fts_meta = FtsFileMeta {
        mime: &mime,
        lang: extraction.lang.as_deref(),
        path_text: &display,
        size: entry.size,
        mtime_ns: entry.mtime_ns,
        title: extraction
            .doc_meta
            .as_ref()
            .and_then(|d| d.title.as_deref()),
    };
    for chunk in &chunks {
        store.fts.add_chunk(file_id, chunk, &fts_meta)?;
    }

    if let Some(doc) = &extraction.doc_meta {
        store.meta.upsert_doc_meta(file_id, doc)?;
    }
    if let Some(media) = &extraction.media_meta {
        store.meta.upsert_media_meta(file_id, media)?;
    }

    Ok(Disposition::Indexed { file_id, blake3 })
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

/// Classify an extraction-time I/O error (PRD §11.1, §11.5):
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

/// Commit staged FTS documents, then flip the batch's statuses to `Indexed` (persisting
/// BLAKE3 hashes and progress rows) — commit FIRST, flip SECOND (module invariant).
fn flush_indexed(store: &mut Store, batch: &mut Vec<(i64, [u8; 32])>) -> Result<()> {
    store.fts.commit()?;
    store.manifest.mark_indexed(batch, "fts", SCHEMA_VERSION)?;
    batch.clear();
    Ok(())
}

/// Phase 3: the synchronous `extract → BLAKE3 → chunk → index` pipeline with crash-safe
/// write ordering (PRD §11.1, §11.2; see the module-level invariant). Extraction routes
/// through `ndex-extract`. With `options.only_new`, only `diff.new` files are processed —
/// modified files and deletions are left for a full reconcile.
pub fn process(
    store: &mut Store,
    embedder: Option<&dyn Embed>,
    diff: &DiffOutcome,
    options: &ReconcileOptions,
    sink: &dyn ProgressSink,
) -> Result<ReconcileStats> {
    // Semantic embedding is deferred to the vector-index follow-up; v0.1 indexes FTS only.
    let _ = embedder;
    let tokens = WordTokens;
    let max_file_size = options
        .max_file_size
        .unwrap_or_else(|| store.config.extraction.max_file_size.bytes());
    let mut stats = ReconcileStats {
        new: diff.new.len() as u64,
        modified: diff.modified.len() as u64,
        unchanged: diff.unchanged,
        ..ReconcileStats::default()
    };

    let modified: &[NdexPath] = if options.only_new {
        &[]
    } else {
        &diff.modified
    };
    let total = (diff.new.len() + modified.len()) as u64;
    let mut done = 0;
    let mut indexed_batch: Vec<(i64, [u8; 32])> = Vec::new();
    for path in diff.new.iter().chain(modified.iter()) {
        done += 1;
        tick(sink, ProgressKind::Extract, done, total);
        match process_one(store, &tokens, path, max_file_size)? {
            Disposition::Indexed { file_id, blake3 } => {
                indexed_batch.push((file_id, blake3));
                stats.processed += 1;
            }
            Disposition::Skipped => stats.skipped += 1,
            Disposition::Failed => stats.failed += 1,
        }
        if indexed_batch.len() >= BATCH_COMMIT_FILES {
            flush_indexed(store, &mut indexed_batch)?;
        }
    }

    // Stage deletions: drop FTS docs + metadata now; the manifest flip to Deleted is
    // deferred until after the commit (same ordering rule as Indexed).
    if !options.only_new {
        for &file_id in &diff.deleted {
            store.fts.delete_file(file_id)?;
            store.meta.delete_file(file_id)?;
            stats.deleted += 1;
        }
    }

    // Persist the FTS segment so searches see the new content (PRD §10.2 commit strategy),
    // then flip the remaining statuses.
    tick(sink, ProgressKind::Fts, total, total);
    flush_indexed(store, &mut indexed_batch)?;
    if !options.only_new {
        store.manifest.mark_deleted(&diff.deleted)?;
    }
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

    fn test_store(root: &std::path::Path) -> Store {
        use ndex_core::identity::{
            EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity,
        };
        let identity = IndexIdentity {
            identity: Identity {
                schema_version: SCHEMA_VERSION,
                created_by: "test".into(),
                created_at: "2026-06-19T00:00:00Z".into(),
            },
            embedding: EmbeddingIdentity {
                model_name: ndex_core::constants::DEFAULT_MODEL.into(),
                model_hash: "test".into(),
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
        };
        Store::create(root, identity, ndex_core::config::Config::default()).unwrap()
    }

    /// Crash invariant: without the post-commit batch flip, a processed file's manifest
    /// status stays `Pending` and no `index_progress` row exists — a crash between
    /// processing and `fts.commit()`/`mark_indexed` therefore only ever loses work, never
    /// records `Indexed` for uncommitted chunks.
    #[test]
    fn status_stays_pending_until_post_commit_flip() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"alpha bravo").unwrap();
        let mut store = test_store(tmp.path());

        let path = NdexPath::from_os_str(tmp.path().join("a.txt").as_os_str());
        // Drive process_one WITHOUT the final fts.commit()/mark_indexed (simulated crash).
        let disposition = process_one(&mut store, &WordTokens, &path, u64::MAX).unwrap();
        let file_id = match disposition {
            Disposition::Indexed { file_id, .. } => file_id,
            _ => panic!("expected Indexed disposition"),
        };

        let rec = store.manifest.get_by_path(&path).unwrap().unwrap();
        assert_eq!(rec.status, FileStatus::Pending, "flip must be deferred");
        assert_eq!(rec.blake3, None, "hash persists only with the flip");
        let progress: i64 = store
            .manifest
            .connection()
            .query_row(
                "SELECT count(*) FROM index_progress WHERE file_id = ?1",
                [file_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(progress, 0, "progress rows are written only with the flip");
    }
}
