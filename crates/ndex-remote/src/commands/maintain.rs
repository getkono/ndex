//! Maintenance handlers: `verify`, `delete`, `config`, `checkpoint`.

use ndex_core::error::Result;

use crate::cli::{ConfigArgs, DeleteArgs, PathArg, VerifyArgs};

/// `ndex-remote verify` — recompute BLAKE3 and compare against the manifest (PRD §13.5).
pub fn verify(args: VerifyArgs) -> Result<()> {
    let _ = args;
    todo!()
}

/// `ndex-remote delete` — remove matching files from all indices (PRD §13.8).
pub fn delete(args: DeleteArgs) -> Result<()> {
    let _ = args;
    todo!()
}

/// `ndex-remote config` — print config, or read a single key (PRD §13.10).
pub fn config(args: ConfigArgs) -> Result<()> {
    let _ = args;
    todo!()
}

/// `ndex-remote checkpoint` — `PRAGMA wal_checkpoint(TRUNCATE)` on both databases (PRD §18.3).
pub fn checkpoint(args: PathArg) -> Result<()> {
    let _ = args;
    todo!()
}
