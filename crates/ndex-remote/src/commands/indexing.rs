//! Indexing-family handlers: `init`, `index`, `reindex`.

use ndex_core::config::{ByteSize, Config};
use ndex_core::error::{NdexError, Result};
use ndex_core::identity::{
    EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity, SCHEMA_VERSION,
};
use ndex_core::progress::NullSink;
use ndex_reconcile::{ReconcileOptions, Reconciler};
use ndex_store::Store;

use crate::cli::{IndexArgs, InitArgs, ReindexArgs};

/// Build the immutable `index.toml` identity for a new index from the chosen model (PRD §5.3).
fn build_identity(model: &str) -> Result<IndexIdentity> {
    let created_at = jiff::Timestamp::now().to_string();
    let embedding = if model == "none" {
        EmbeddingIdentity {
            model_name: "none".into(),
            model_hash: String::new(),
            dimensions: 0,
            mrl_dimensions: 0,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        }
    } else {
        let shortname = if model == "default" { "arctic" } else { model };
        let info = ndex_embed::lookup(shortname)
            .ok_or_else(|| NdexError::Config(format!("unknown embedding model: {model}")))?;
        EmbeddingIdentity {
            model_name: info.full_name.into(),
            // Empty = unpinned: the registry has no release hash yet (34-embedding.md);
            // never bake a placeholder into the immutable identity.
            model_hash: info.onnx_blake3.unwrap_or("").into(),
            dimensions: info.dimensions,
            mrl_dimensions: info.mrl_dimensions,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        }
    };
    Ok(IndexIdentity {
        identity: Identity {
            schema_version: SCHEMA_VERSION,
            created_by: concat!("ndex-remote ", env!("CARGO_PKG_VERSION")).into(),
            created_at,
        },
        embedding,
        hashing: Hashing {
            algorithm: "blake3".into(),
        },
        fts: FtsIdentity {
            tokenizer_version: 1,
        },
    })
}

/// `ndex-remote init` — create a fresh index (PRD §13.4).
pub fn init(args: InitArgs) -> Result<()> {
    // Honesty over silence: these flags parse but no handler consumes them yet.
    if !args.exclude.is_empty() {
        eprintln!("warning: --exclude is not implemented in v0.1; ignoring");
    }
    if args.no_fts {
        eprintln!("warning: --no-fts is not implemented in v0.1; ignoring");
    }
    if args.no_meta {
        eprintln!("warning: --no-meta is not implemented in v0.1; ignoring");
    }
    let identity = build_identity(&args.model)?;
    let _store = Store::create(&args.path, identity, Config::default())?;
    println!(
        "Initialized ndex index at {}/.ndex (model: {})",
        args.path.display(),
        args.model
    );
    Ok(())
}

/// `ndex-remote index` — build or update the index (PRD §13.3).
pub fn index(args: IndexArgs) -> Result<()> {
    // Fail fast instead of queueing behind another writer's exclusive flock.
    let Some(mut store) = Store::try_open(&args.path)? else {
        return Err(NdexError::Lock(format!(
            "another ndex process holds the index lock at {}/.ndex/lock; \
             retry after it finishes",
            args.path.display()
        )));
    };

    if args.status {
        let last = store.manifest.last_reconciliation_ns()?;
        let n = store.manifest.live_files()?.len();
        match last {
            Some(ns) => println!("{n} files indexed; last reconciled at {ns} ns"),
            None => println!("{n} files indexed; never reconciled"),
        }
        return Ok(());
    }

    let options = ReconcileOptions {
        full: args.full,
        verify: args.verify,
        dry_run: args.dry_run,
        jobs: args.jobs.map(|j| j as usize),
        batch_size: args.batch_size.map(|b| b as usize),
        no_vectors: args.no_vectors,
        max_file_size: args
            .max_file_size
            .as_deref()
            .map(|s| {
                s.parse::<ByteSize>()
                    .map_err(|e| NdexError::Config(format!("invalid --max-file-size: {e}")))
            })
            .transpose()?
            .map(ByteSize::bytes),
        only_new: args.only_new,
    };

    let stats = {
        let mut reconciler = Reconciler::new(&mut store, None);
        reconciler.run(&options, &NullSink)?
    };

    println!(
        "{} new, {} modified, {} deleted, {} unchanged, {} processed, {} failed ({} ms)",
        stats.new,
        stats.modified,
        stats.deleted,
        stats.unchanged,
        stats.processed,
        stats.failed,
        stats.duration_ms,
    );
    Ok(())
}

/// `ndex-remote reindex` — rebuild from scratch (PRD §13.6).
pub fn reindex(_args: ReindexArgs) -> Result<()> {
    // The `.ndex/` → `.ndex.old/` swap-and-rebuild flow is a follow-up; rebuilding today is done
    // by removing the index and running `init` + `index` again.
    Err(NdexError::Other(
        "`reindex` (atomic full rebuild) is planned for a follow-up; recreate with `init` + `index`"
            .into(),
    ))
}
