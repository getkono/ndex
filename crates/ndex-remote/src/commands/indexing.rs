//! Indexing-family handlers: `init`, `index`, `reindex`.

use ndex_core::error::Result;

use crate::cli::{IndexArgs, InitArgs, ReindexArgs};

/// `ndex-remote init` — create a fresh index (PRD §13.4).
pub fn init(args: InitArgs) -> Result<()> {
    // TODO(skeleton): create .ndex/, write index.toml + default config.toml, open Store::create.
    let _ = args;
    todo!()
}

/// `ndex-remote index` — build or update the index (PRD §13.3).
pub fn index(args: IndexArgs) -> Result<()> {
    // TODO(skeleton): Store::open → Reconciler::run(ReconcileOptions) with a NullSink.
    let _ = args;
    todo!()
}

/// `ndex-remote reindex` — rebuild from scratch (PRD §13.6).
pub fn reindex(args: ReindexArgs) -> Result<()> {
    // TODO(skeleton): move .ndex/ → .ndex.old/, rebuild, restore on failure.
    let _ = args;
    todo!()
}
