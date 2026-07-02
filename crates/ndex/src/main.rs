//! `ndex` thin-client binary entry point.

use clap::Parser;
use ndex::args::Cli;

fn main() {
    let cli = Cli::parse();
    ndex::init_tracing(cli.global.verbose, cli.global.quiet);
    if let Err(err) = ndex::run(cli) {
        eprintln!("error: {err}");
        std::process::exit(err.exit_code());
    }
}
