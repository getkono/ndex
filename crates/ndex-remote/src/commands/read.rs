//! Read-family handlers: `search`, `info`, `stats`.

use ndex_core::error::Result;

use crate::cli::{InfoArgs, PathArg, SearchArgs};

/// `ndex-remote search` (PRD §13.2).
pub fn search(args: SearchArgs) -> Result<()> {
    // TODO(skeleton): Store::open → ndex_search::run → render to args.format.
    let _ = args;
    todo!()
}

/// `ndex-remote info` (PRD §13.5).
pub fn info(args: InfoArgs) -> Result<()> {
    let _ = args;
    todo!()
}

/// `ndex-remote stats` (PRD §13.5).
pub fn stats(args: PathArg) -> Result<()> {
    let _ = args;
    todo!()
}
