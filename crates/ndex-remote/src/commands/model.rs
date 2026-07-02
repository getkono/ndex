//! Model-management handlers: `model …` and the `self-update` stub.

use ndex_core::error::Result;

use crate::cli::{ModelCommand, SelfUpdateArgs};

/// `ndex-remote model …` — list/fetch/verify/delete/path/import (PRD §7.4).
pub fn run(command: ModelCommand) -> Result<()> {
    // TODO(skeleton): dispatch to ndex_embed::model::{list,fetch,verify,delete,model_path,import}.
    let _ = command;
    todo!()
}

/// `ndex-remote self-update` — stub for v0.1 (PRD §7.3).
pub fn self_update(args: SelfUpdateArgs) -> Result<()> {
    let _ = args;
    println!(
        "Self-update is planned for v0.2. Update manually via your package manager or: \
         curl -fsSL https://get.ndex.dev/install.sh | sh"
    );
    Ok(())
}
