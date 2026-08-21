#![allow(clippy::result_large_err)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use berth_mcp::{Mcp, McpContent, ToolResult};
use berth_protocol::{Action, ActionBatch};
use futures_util::{SinkExt, StreamExt};
use httptest::{Expectation, Server, matchers::*, responders::*};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const TOKEN: &str = "brt_secret";

fn sample_quote() -> Value {
    json!({
        "vcpu": 2,
        "mem_gib": 4,
        "disk_gib": 40,
        "os": "linux",
        "os_mult": 1.0,
        "density": "isolated",
        "density_mult": 1.0,
        "term": "on_demand",
        "min_seconds": 60,
        "pooled": false,
        "gas_per_second": "0.00134",
        "currency": "gas",
        "usd_per_gas": "0.01"
    })
}

fn sample_lease() -> Value {
    json!({
        "lease_id": "l_1",
        "session_id": "s_1",
        "ws_url": "ws://127.0.0.1:7432/v1/sessions/s_1",
        "viewer_url": "http://127.0.0.1:6080/vnc.html",
        "quote": sample_quote(),
        "status": "active"
    })
}

fn seed_pair(home: &Path, url: &str, token: &str) {
    fs::create_dir_all(home).unwrap();
    fs::write(
        home.join("config.toml"),
        format!("[nodes.default]\nurl = \"{url}\"\ntoken = \"{token}\"\n"),
    )
    .unwrap();
}

fn seed_session(home: &Path, ws_url: &str) {
    fs::write(
        home.join("session.toml"),
        format!(
            "node = \"default\"\nlease_id = \"l_1\"\nsession_id = \"s_1\"\nviewer_url = \"http://127.0.0.1:6080/vnc.html\"\nws_url = \"{ws_url}\"\n"
        ),
    )
    .unwrap();
}

fn text_of(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            McpContent::Text { text } => Some(text.as_str()),
            McpContent::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct WsMock {
    token: String,
    fail: bool,
    batches: Vec<ActionBatch>,
    uris: Vec<String>,
    auths: Vec<Option<String>>,
}

async fn spawn_ws(token: &str, fail: bool) -> (String, Arc<Mutex<WsMock>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock = Arc::new(Mutex::new(WsMock {
        token: token.to_string(),
        fail,
        batches: Vec::new(),
        uris: Vec::new(),
        auths: Vec::new(),
    }));
    let accept = mock.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let mock = accept.clone();
            tokio::spawn(async move {
                serve_ws(stream, mock).await;
            });
        }
    });
    (format!("ws://{addr}/v1/sessions/s_1"), mock)
}

async fn serve_ws(stream: tokio::net::TcpStream, mock: Arc<Mutex<WsMock>>) {
    let expected = mock.lock().expect("mock").token.clone();
    let cb = mock.clone();
    let mut ws = match accept_hdr_async(stream, move |req: &Request, resp: Response| {
        let mut g = cb.lock().expect("mock");
        g.uris.push(req.uri().to_string());
        let auth = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        g.auths.push(auth.clone());
        let want = format!("Bearer {expected}");
        if auth.as_deref() != Some(want.as_str()) {
            let mut err = ErrorResponse::new(Some("unauthorized".into()));
            *err.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
            return Err(err);
        }
        Ok(resp)
    })
    .await
    {
        Ok(ws) => ws,
        Err(_) => return,
    };

    while let Some(msg) = ws.next().await {
        let Ok(Message::Text(text)) = msg else {
            continue;
        };
        let batch: ActionBatch = serde_json::from_str(text.as_str()).unwrap();
        let fail = {
            let mut g = mock.lock().expect("mock");
            g.batches.push(batch.clone());
            g.fail
        };
        let ack = json!({
            "type": "ack",
            "id": batch.id,
            "results": batch.items.iter().enumerate().map(|(i, item)| {
                json!({
                    "i": i,
                    "ok": !fail,
                    "frame": !fail && matches!(item, Action::Screenshot {}),
                    "error": if fail { Value::String("denied".into()) } else { Value::Null }
                })
            }).collect::<Vec<_>>()
        });
        if ws
            .send(Message::Text(ack.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
        if !fail
            && batch
                .items
                .iter()
                .any(|i| matches!(i, Action::Screenshot {}))
        {
            let frame = json!({
                "type": "frame",
                "session_id": batch.session_id,
                "ts": 0,
                "width": 1280,
                "height": 800,
                "mime": "image/png",
                "data": PNG
            });
            let _ = ws.send(Message::Text(frame.to_string().into())).await;
        }
    }
}

#[tokio::test]
async fn initialize_and_tools_list() {
    let dir = tempfile::tempdir().unwrap();
    let mcp = Mcp::new(dir.path());
    let init = mcp
        .handle_rpc(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }
        }))
        .await
        .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "berth");
    let listed = mcp
        .handle_rpc(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .await
        .unwrap();
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "berth_lease",
            "berth_screenshot",
            "berth_click",
            "berth_type",
            "berth_key",
            "berth_scroll",
            "berth_end"
        ]
    );
}

#[tokio::test]
async fn lease_post_body_isolated_2_4_40() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method_path("POST", "/v1/leases"),
            request::headers(contains(("authorization", format!("Bearer {TOKEN}")))),
            request::body(json_decoded(|v: &Value| {
                v["os"] == "linux"
                    && v["class"] == "private"
                    && v["license"] == "linux"
                    && v["density"] == "isolated"
                    && v["resources"]["vcpu"] == 2
                    && v["resources"]["mem_gib"] == 4
                    && v["resources"]["disk_gib"] == 40
                    && v.get("min_seconds").map(|m| m == 120).unwrap_or(false)
            })),
        ])
        .respond_with(json_encoded(sample_lease())),
    );
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, TOKEN);
    let mcp = Mcp::new(dir.path());
    let result = mcp
        .call_tool("berth_lease", json!({ "os": "linux", "seconds": 120 }))
        .await;
    assert!(!result.is_error, "{}", text_of(&result));
    let text = text_of(&result);
    assert!(text.contains("lease_id=l_1"));
    assert!(text.contains("session_id=s_1"));
    assert!(text.contains("viewer_url=http://127.0.0.1:6080/vnc.html"));
    let session = fs::read_to_string(dir.path().join("session.toml")).unwrap();
    assert!(session.contains("l_1"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path().join("session.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[tokio::test]
async fn seconds_below_60_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method_path("POST", "/v1/leases"),
            request::body(json_decoded(|v: &Value| {
                v.get("min_seconds").map(|m| m == 0).unwrap_or(true)
            })),
        ])
        .respond_with(json_encoded(sample_lease())),
    );
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, TOKEN);
    let mcp = Mcp::new(dir.path());
    let result = mcp
        .call_tool("berth_lease", json!({ "os": "linux", "seconds": 30 }))
        .await;
    assert!(!result.is_error, "{}", text_of(&result));
}

#[tokio::test]
async fn windows_macos_rejected_without_http() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, TOKEN);
    let mcp = Mcp::new(dir.path());
    let win = mcp
        .call_tool("berth_lease", json!({ "os": "windows" }))
        .await;
    assert!(win.is_error);
    assert!(text_of(&win).contains("Windows"));
    let mac = mcp.call_tool("berth_lease", json!({ "os": "macos" })).await;
    assert!(mac.is_error);
    assert!(text_of(&mac).contains("macOS"));
    assert!(!dir.path().join("session.toml").exists());
}

#[tokio::test]
async fn existing_session_does_not_create_second_lease() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, TOKEN);
    seed_session(dir.path(), "ws://127.0.0.1:9/v1/sessions/s_1");
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_lease", json!({ "os": "linux" })).await;
    assert!(!result.is_error, "{}", text_of(&result));
    assert!(text_of(&result).contains("current lease"));
    assert!(text_of(&result).contains("l_1"));
}

#[tokio::test]
async fn lease_unauthorized() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method_path("POST", "/v1/leases"),
            request::headers(contains(("authorization", "Bearer brt_stale"))),
        ])
        .respond_with(
            status_code(401)
                .append_header("content-type", "application/json")
                .body(r#"{"error":"unauthorized"}"#),
        ),
    );
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, "brt_stale");
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_lease", json!({ "os": "linux" })).await;
    assert!(result.is_error);
    assert!(text_of(&result).contains("unauthorized"));
    assert!(!dir.path().join("session.toml").exists());
}

#[tokio::test]
async fn screenshot_returns_png_from_frame() {
    let dir = tempfile::tempdir().unwrap();
    let (ws_url, mock) = spawn_ws(TOKEN, false).await;
    seed_pair(dir.path(), "http://127.0.0.1:7432", TOKEN);
    seed_session(dir.path(), &ws_url);
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_screenshot", json!({})).await;
    assert!(!result.is_error, "{}", text_of(&result));
    match &result.content[0] {
        McpContent::Image { data, mime_type } => {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, PNG);
        }
        other => panic!("{other:?}"),
    }
    let g = mock.lock().unwrap();
    assert_eq!(g.batches.len(), 1);
    assert!(matches!(g.batches[0].items[0], Action::Screenshot {}));
    assert!(
        g.uris
            .iter()
            .all(|u| !u.contains("token") && !u.contains(TOKEN))
    );
    assert_eq!(g.auths[0].as_deref(), Some("Bearer brt_secret"));
}

#[tokio::test]
async fn click_sends_action_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (ws_url, mock) = spawn_ws(TOKEN, false).await;
    seed_pair(dir.path(), "http://127.0.0.1:7432", TOKEN);
    seed_session(dir.path(), &ws_url);
    let mcp = Mcp::new(dir.path());
    let result = mcp
        .call_tool(
            "berth_click",
            json!({ "x": 100, "y": 200, "button": "right" }),
        )
        .await;
    assert!(!result.is_error, "{}", text_of(&result));
    assert_eq!(text_of(&result), "OK");
    let g = mock.lock().unwrap();
    match &g.batches[0].items[0] {
        Action::Click { xy, button, mods } => {
            assert_eq!(*xy, [100, 200]);
            assert_eq!(*button, berth_protocol::Button::Right);
            assert!(mods.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(g.batches[0].session_id, "s_1");
}

#[tokio::test]
async fn type_key_scroll_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (ws_url, mock) = spawn_ws(TOKEN, false).await;
    seed_pair(dir.path(), "http://127.0.0.1:7432", TOKEN);
    seed_session(dir.path(), &ws_url);
    let mcp = Mcp::new(dir.path());
    let typed = mcp
        .call_tool("berth_type", json!({ "text": "hello" }))
        .await;
    let keyed = mcp
        .call_tool("berth_key", json!({ "keys": ["META", "s"] }))
        .await;
    let scrolled = mcp
        .call_tool("berth_scroll", json!({ "x": 1, "y": 2, "dy": 3 }))
        .await;
    assert!(!typed.is_error && !keyed.is_error && !scrolled.is_error);
    let g = mock.lock().unwrap();
    assert_eq!(g.batches.len(), 3);
    assert!(matches!(&g.batches[0].items[0], Action::Type { text } if text == "hello"));
    assert!(matches!(&g.batches[1].items[0], Action::Key { keys, .. } if keys == &["META", "s"]));
    assert!(
        matches!(&g.batches[2].items[0], Action::Scroll { xy, dx, dy } if *xy == [1, 2] && *dx == 0 && *dy == 3)
    );
}

#[tokio::test]
async fn failed_ack_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let (ws_url, _) = spawn_ws(TOKEN, true).await;
    seed_pair(dir.path(), "http://127.0.0.1:7432", TOKEN);
    seed_session(dir.path(), &ws_url);
    let mcp = Mcp::new(dir.path());
    let result = mcp
        .call_tool("berth_click", json!({ "x": 1, "y": 2 }))
        .await;
    assert!(result.is_error);
    assert!(text_of(&result).contains("denied"));
}

#[tokio::test]
async fn ws_unauthorized() {
    let dir = tempfile::tempdir().unwrap();
    let (ws_url, _) = spawn_ws(TOKEN, false).await;
    seed_pair(dir.path(), "http://127.0.0.1:7432", "brt_stale");
    seed_session(dir.path(), &ws_url);
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_screenshot", json!({})).await;
    assert!(result.is_error);
}

#[tokio::test]
async fn action_without_session_tells_agent_to_lease() {
    let dir = tempfile::tempdir().unwrap();
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_screenshot", json!({})).await;
    assert!(result.is_error);
    assert!(text_of(&result).contains("berth_lease"));
}

#[tokio::test]
async fn end_404_clears_session() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method_path("DELETE", "/v1/leases/l_1"),
            request::headers(contains(("authorization", format!("Bearer {TOKEN}")))),
        ])
        .respond_with(
            status_code(404)
                .append_header("content-type", "application/json")
                .body(r#"{"error":"not found"}"#),
        ),
    );
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, TOKEN);
    seed_session(dir.path(), "ws://127.0.0.1:9/v1/sessions/s_1");
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_end", json!({})).await;
    assert!(!result.is_error, "{}", text_of(&result));
    assert!(text_of(&result).contains("gone"));
    assert!(!dir.path().join("session.toml").exists());
}

#[tokio::test]
async fn end_unauthorized_keeps_session() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::run();
    server.expect(
        Expectation::matching(all_of![
            request::method_path("DELETE", "/v1/leases/l_1"),
            request::headers(contains(("authorization", "Bearer brt_stale"))),
        ])
        .respond_with(
            status_code(401)
                .append_header("content-type", "application/json")
                .body(r#"{"error":"unauthorized"}"#),
        ),
    );
    let url = format!("http://{}", server.addr());
    seed_pair(dir.path(), &url, "brt_stale");
    seed_session(dir.path(), "ws://127.0.0.1:9/v1/sessions/s_1");
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_end", json!({})).await;
    assert!(result.is_error);
    assert!(text_of(&result).contains("unauthorized"));
    assert!(dir.path().join("session.toml").exists());
}

#[tokio::test]
async fn missing_click_coords_error() {
    let dir = tempfile::tempdir().unwrap();
    seed_session(dir.path(), "ws://127.0.0.1:9/v1/sessions/s_1");
    let mcp = Mcp::new(dir.path());
    let result = mcp.call_tool("berth_click", json!({})).await;
    assert!(result.is_error);
    assert!(text_of(&result).contains("x is required"));
}
