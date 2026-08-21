use serde_json::{Value, json};

use crate::{Mcp, ToolResult};

impl Mcp {
    /// Handle one JSON-RPC message. Notifications return `None`.
    pub async fn handle_rpc(&self, msg: Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        if method.is_empty() {
            return Some(rpc_error(id, -32600, "invalid request"));
        }
        match method {
            "initialize" => Some(rpc_ok(id, initialize_result(&params))),
            "notifications/initialized" | "initialized" => None,
            "ping" => Some(rpc_ok(id, json!({}))),
            "tools/list" => Some(rpc_ok(id, json!({ "tools": tools() }))),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let result = self.call_tool(name, args).await;
                match serde_json::to_value(&result) {
                    Ok(value) => Some(rpc_ok(id, value)),
                    Err(err) => Some(rpc_error(id, -32603, &format!("serialize: {err}"))),
                }
            }
            other => {
                if id.is_none() {
                    None
                } else {
                    Some(rpc_error(id, -32601, &format!("method not found: {other}")))
                }
            }
        }
    }
}

fn initialize_result(params: &Value) -> Value {
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "berth",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Lease an isolated Linux guest with berth_lease, then berth_screenshot / berth_click / berth_type / berth_key / berth_scroll. Never drive the host desktop."
    })
}

fn rpc_ok(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result
    })
}

fn rpc_error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": { "code": code, "message": message }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "berth_lease",
            "Create a private isolated Linux desktop lease (2 vCPU, 4 GiB, 40 GiB). If a session already exists, returns it instead of starting a second guest. Windows and macOS are rejected locally. Requires a paired node.",
            json!({
                "type": "object",
                "properties": {
                    "os": { "type": "string", "description": "Guest OS. MVP accepts linux only." },
                    "seconds": { "type": "number", "description": "Optional min_seconds if >= 60." }
                },
                "required": ["os"]
            }),
        ),
        tool(
            "berth_screenshot",
            "Capture the guest desktop as a PNG. Requires an active session from berth_lease. Never captures the host display.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "berth_click",
            "Click in the guest at last-frame pixel coordinates (origin top-left).",
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "button": { "type": "string", "enum": ["left", "right", "middle"] }
                },
                "required": ["x", "y"]
            }),
        ),
        tool(
            "berth_type",
            "Type text into the guest desktop.",
            json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }),
        ),
        tool(
            "berth_key",
            "Press a key or key combination in the guest (string or array of strings).",
            json!({
                "type": "object",
                "properties": {
                    "keys": {
                        "type": ["string", "array"],
                        "items": { "type": "string" }
                    }
                },
                "required": ["keys"]
            }),
        ),
        tool(
            "berth_scroll",
            "Scroll in the guest at last-frame pixel coordinates.",
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "dy": { "type": "integer" },
                    "dx": { "type": "integer" }
                },
                "required": ["x", "y", "dy"]
            }),
        ),
        tool(
            "berth_end",
            "End the current lease. A 404 still clears the local session.toml.",
            json!({ "type": "object", "properties": {} }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

impl ToolResult {
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![crate::McpContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    pub(crate) fn err(text: impl Into<String>) -> Self {
        Self {
            content: vec![crate::McpContent::Text { text: text.into() }],
            is_error: true,
        }
    }
}
