//! `ndex-remote` binary entry point.

use clap::Parser;
use ndex_remote::cli::Cli;

fn main() {
    let cli = Cli::parse();
    ndex_remote::init_tracing(cli.verbose, cli.quiet);
    if let Err(err) = ndex_remote::run(cli) {
        eprintln!("error: {err}");
        std::process::exit(err.exit_code());
    }
}
