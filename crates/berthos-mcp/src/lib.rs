//! MCP stdio server: lease an isolated Linux guest and drive it with protocol actions.

mod client;
mod config;
mod error;
mod rpc;
mod tools;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub use error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError", skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

pub struct Mcp {
    home: PathBuf,
    seq: AtomicU64,
}

impl Mcp {
    #[must_use]
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            seq: AtomicU64::new(1),
        }
    }
}

pub fn serve_blocking(home: &Path) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve(home))
}

pub async fn serve(home: &Path) -> Result<()> {
    let mcp = Mcp::new(home);
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(msg) => {
                if let Some(resp) = mcp.handle_rpc(msg).await {
                    write_line(&mut stdout, &resp).await?;
                }
            }
            Err(err) => {
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("parse error: {err}") }
                });
                write_line(&mut stdout, &resp).await?;
            }
        }
    }
    Ok(())
}

async fn write_line(stdout: &mut tokio::io::Stdout, value: &serde_json::Value) -> Result<()> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}
