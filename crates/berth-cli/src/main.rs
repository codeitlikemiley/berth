use std::net::SocketAddr;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    #[command(subcommand)]
    Node(NodeCmd),
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

#[derive(Debug, Subcommand)]
enum NodeCmd {
    /// Start the HTTP/WS control plane
    Up {
        /// Listen address (loopback default; never host-network)
        #[arg(long, default_value = "127.0.0.1:7432")]
        bind: SocketAddr,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Node(NodeCmd::Up { bind }) => match berth_node::serve_blocking(bind) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("not implemented");
            ExitCode::from(2)
        }
    }
}
