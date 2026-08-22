use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    berthos::exit(berthos::Cli::parse())
}
