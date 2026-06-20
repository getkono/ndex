//! Shell-completion generation (PRD §13.7).

use clap::CommandFactory;
use ndex_core::error::Result;

use crate::cli::{Cli, CompletionsArgs};

/// `ndex-remote completions <SHELL>` — emit shell completions to stdout.
pub fn run(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}
