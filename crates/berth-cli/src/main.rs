use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "berth",
    version,
    about = "Lease an isolated computer to an agent"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start a computer session
    Up,
    /// Run a berth node
    Node,
    /// MCP stdio server
    Mcp,
    /// Pair with a node
    Pair,
    /// End a session
    End,
    /// Open the session viewer
    View,
    /// Show session status
    Status,
    /// Diagnose local setup
    Doctor,
}

fn main() -> ExitCode {
    let _cli = Cli::parse();
    eprintln!("not implemented");
    ExitCode::from(2)
}
