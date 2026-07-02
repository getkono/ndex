//! Thin-client command handlers (PRD §13).
//!
//! Each connecting handler resolves the `[HOST:]PATH` target ([`crate::hosts`]), builds a
//! [`Transport`](crate::transport::Transport), opens a [`Session`](crate::session::Session),
//! sends the request, and renders the response ([`crate::render`]). Bodies are `todo!()`;
//! `completions` is real and the v0.2 commands return a clear error.

use clap::CommandFactory;
use ndex_core::error::{NdexError, Result};

use crate::args::{
    Cli, CompletionsArgs, ConfigArgs, DeleteArgs, GlobalOpts, IndexArgs, InfoArgs, InitArgs,
    ReindexArgs, SearchArgs, TargetArg, VerifyArgs,
};

/// `ndex search` (PRD §13.2).
pub fn search(args: SearchArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex index` (PRD §13.3).
pub fn index(args: IndexArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex init` — local-only in v0.1 (PRD §13.4).
pub fn init(args: InitArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex info` (PRD §13.5).
pub fn info(args: InfoArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex stats` (PRD §13.5).
pub fn stats(args: TargetArg, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex verify` (PRD §13.5).
pub fn verify(args: VerifyArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex reindex` (PRD §13.6).
pub fn reindex(args: ReindexArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex delete` (PRD §13.8).
pub fn delete(args: DeleteArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex config` (PRD §13.10).
pub fn config(args: ConfigArgs, global: &GlobalOpts) -> Result<()> {
    let _ = (args, global);
    todo!()
}

/// `ndex completions <SHELL>` — emit shell completions to stdout.
pub fn completions(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}

/// Shared handler for the v0.2 command stubs (PRD §13.1).
pub fn unavailable_v0_2(command: &str) -> Result<()> {
    Err(NdexError::Other(format!(
        "'ndex {command}' is planned for v0.2 and not yet available."
    )))
}
