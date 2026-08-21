use berth_protocol::{Action, ActionBatch, ActionBatchKind, Button, LeaseRequest, Os};

use crate::client::NodeClient;
use crate::config::{
    Config, DEFAULT_NODE, Session, clear_session, existing_session, resolve_session,
    resolve_ws_url, save_session,
};
use crate::error::{Error, Result};
use crate::{Mcp, McpContent, ToolResult};

impl Mcp {
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> ToolResult {
        match self.dispatch(name, arguments).await {
            Ok(result) => result,
            Err(err) => ToolResult::err(err.to_string()),
        }
    }

    async fn dispatch(&self, name: &str, args: serde_json::Value) -> Result<ToolResult> {
        match name {
            "berth_lease" => self.lease(&args).await,
            "berth_screenshot" => self.screenshot().await,
            "berth_click" => self.click(&args).await,
            "berth_type" => self.type_text(&args).await,
            "berth_key" => self.key(&args).await,
            "berth_scroll" => self.scroll(&args).await,
            "berth_end" => self.end().await,
            other => Err(Error::Usage(format!("unknown tool `{other}`"))),
        }
    }

    async fn lease(&self, args: &serde_json::Value) -> Result<ToolResult> {
        if let Some(cur) = existing_session(&self.home)? {
            return Ok(ToolResult::text(format_lease(
                "current lease",
                &cur.lease_id,
                &cur.session_id,
                cur.viewer_url.as_deref(),
            )));
        }

        let os = parse_os(
            args.get("os")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Usage("os is required".into()))?,
        )?;
        let seconds = optional_seconds(args);
        let mut req = mvp_lease_request(os)?;
        if let Some(seconds) = seconds
            && seconds >= 60
        {
            req.min_seconds = seconds;
        }
        req.validate_mvp()?;

        let cfg = Config::load(&self.home)?;
        let node = cfg.node(DEFAULT_NODE)?;
        let client = NodeClient::new(&node.url, Some(&node.token))?;
        let lease = client.create_lease(&req).await?;
        let ws_url = resolve_ws_url(Some(&lease.ws_url), &node.url, &lease.session_id);
        save_session(
            &self.home,
            &Session {
                node: DEFAULT_NODE.to_string(),
                lease_id: lease.lease_id.clone(),
                session_id: lease.session_id.clone(),
                viewer_url: lease.viewer_url.clone(),
                ws_url: Some(ws_url),
            },
        )?;
        Ok(ToolResult::text(format_lease(
            "leased",
            &lease.lease_id,
            &lease.session_id,
            lease.viewer_url.as_deref(),
        )))
    }

    async fn screenshot(&self) -> Result<ToolResult> {
        let (_ack, frames) = self.exec_one(Action::Screenshot {}).await?;
        let frame = frames
            .into_iter()
            .next()
            .ok_or_else(|| Error::Protocol("screenshot produced no frame".into()))?;
        let mime = if frame.mime.is_empty() {
            "image/png".to_string()
        } else {
            frame.mime
        };
        Ok(ToolResult {
            content: vec![McpContent::Image {
                data: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    &frame.data,
                ),
                mime_type: mime,
            }],
            is_error: false,
        })
    }

    async fn click(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let x = require_i32(args, "x")?;
        let y = require_i32(args, "y")?;
        let button = parse_button(args.get("button"))?;
        self.exec_one(Action::Click {
            button,
            xy: [x, y],
            mods: Vec::new(),
        })
        .await?;
        Ok(ToolResult::text("OK"))
    }

    async fn type_text(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Usage("text is required".into()))?;
        if text.is_empty() {
            return Err(Error::Usage("text is empty".into()));
        }
        self.exec_one(Action::Type {
            text: text.to_string(),
        })
        .await?;
        Ok(ToolResult::text("OK"))
    }

    async fn key(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let keys = parse_keys(args.get("keys"))?;
        self.exec_one(Action::Key { keys, repeat: 1 }).await?;
        Ok(ToolResult::text("OK"))
    }

    async fn scroll(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let x = require_i32(args, "x")?;
        let y = require_i32(args, "y")?;
        let dy = require_i32(args, "dy")?;
        let dx = match args.get("dx") {
            None => 0,
            Some(v) => i32_from(v, "dx")?,
        };
        self.exec_one(Action::Scroll { xy: [x, y], dx, dy }).await?;
        Ok(ToolResult::text("OK"))
    }

    async fn end(&self) -> Result<ToolResult> {
        let session = resolve_session(&self.home)?;
        if session.lease_id.is_empty() {
            return Err(Error::Usage(
                "no lease_id in session; call berth_lease first".into(),
            ));
        }
        let cfg = Config::load(&self.home)?;
        let node = cfg.node(&session.node)?;
        let client = NodeClient::new(&node.url, Some(&node.token))?;
        match client.delete_lease(&session.lease_id).await {
            Ok(_) => {
                clear_session(&self.home)?;
                Ok(ToolResult::text(format!(
                    "lease {} ended",
                    session.lease_id
                )))
            }
            Err(Error::Api { status: 404, .. }) => {
                clear_session(&self.home)?;
                Ok(ToolResult::text(format!("lease {} gone", session.lease_id)))
            }
            Err(err) => Err(err),
        }
    }

    async fn exec_one(
        &self,
        action: Action,
    ) -> Result<(berth_protocol::Ack, Vec<berth_protocol::Frame>)> {
        let session = resolve_session(&self.home)?;
        let cfg = Config::load(&self.home)?;
        let node = cfg.node(&session.node)?;
        let ws_url = resolve_ws_url(session.ws_url.as_deref(), &node.url, &session.session_id);
        let client = NodeClient::new(&node.url, Some(&node.token))?;
        let id = format!(
            "a_mcp_{}",
            self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let batch = ActionBatch {
            kind: ActionBatchKind::Actions,
            id,
            session_id: session.session_id.clone(),
            items: vec![action],
        };
        client.exec(&ws_url, &batch).await
    }
}

fn mvp_lease_request(os: Os) -> Result<LeaseRequest> {
    let req: LeaseRequest = serde_json::from_value(serde_json::json!({
        "os": os,
        "class": "private",
        "license": "linux",
        "density": "isolated",
        "term": "on_demand",
        "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 }
    }))?;
    req.validate_mvp()?;
    Ok(req)
}

fn parse_os(s: &str) -> Result<Os> {
    match s.to_ascii_lowercase().as_str() {
        "linux" => Ok(Os::Linux),
        "windows" => Ok(Os::Windows),
        "macos" => Ok(Os::Macos),
        other => Err(Error::Usage(format!(
            "unknown os `{other}`; expected linux, windows, or macos"
        ))),
    }
}

fn parse_button(value: Option<&serde_json::Value>) -> Result<Button> {
    match value {
        None => Ok(Button::Left),
        Some(v) => match v.as_str() {
            Some("left") => Ok(Button::Left),
            Some("right") => Ok(Button::Right),
            Some("middle") => Ok(Button::Middle),
            Some(other) => Err(Error::Usage(format!(
                "unknown button `{other}`; expected left, right, or middle"
            ))),
            None => Err(Error::Usage("button must be a string".into())),
        },
    }
}

fn parse_keys(value: Option<&serde_json::Value>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Err(Error::Usage("keys is required".into()));
    };
    let keys = match value {
        serde_json::Value::String(s) => split_keys(s),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str())
            .flat_map(split_keys)
            .collect(),
        _ => {
            return Err(Error::Usage(
                "keys must be a string or array of strings".into(),
            ));
        }
    };
    if keys.is_empty() {
        return Err(Error::Usage("keys is empty".into()));
    }
    Ok(keys)
}

fn split_keys(text: &str) -> Vec<String> {
    text.split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn optional_seconds(args: &serde_json::Value) -> Option<u64> {
    let value = args.get("seconds")?;
    if let Some(n) = value.as_u64() {
        return Some(n);
    }
    let f = value.as_f64()?;
    if f.is_finite() && f >= 0.0 {
        Some(f as u64)
    } else {
        None
    }
}

fn require_i32(args: &serde_json::Value, key: &str) -> Result<i32> {
    let value = args
        .get(key)
        .ok_or_else(|| Error::Usage(format!("{key} is required")))?;
    i32_from(value, key)
}

fn i32_from(value: &serde_json::Value, field: &str) -> Result<i32> {
    if let Some(n) = value.as_i64() {
        return i32::try_from(n).map_err(|_| Error::Usage(format!("{field} is out of range")));
    }
    if let Some(n) = value.as_u64() {
        return i32::try_from(n).map_err(|_| Error::Usage(format!("{field} is out of range")));
    }
    if let Some(f) = value.as_f64()
        && f.is_finite()
        && f >= f64::from(i32::MIN)
        && f <= f64::from(i32::MAX)
        && f.fract() == 0.0
    {
        return Ok(f as i32);
    }
    Err(Error::Usage(format!("{field} must be an integer")))
}

fn format_lease(kind: &str, lease_id: &str, session_id: &str, viewer: Option<&str>) -> String {
    let mut lines = vec![kind.to_string()];
    if !lease_id.is_empty() {
        lines.push(format!("lease_id={lease_id}"));
    }
    lines.push(format!("session_id={session_id}"));
    if let Some(url) = viewer {
        lines.push(format!("viewer_url={url}"));
    }
    lines.join("\n")
}
