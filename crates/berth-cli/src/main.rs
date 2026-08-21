use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    berth_cli::exit(berth_cli::Cli::parse())
}
