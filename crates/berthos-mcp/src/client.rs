use std::time::Duration;

use berthos_protocol::{Ack, Action, ActionBatch, Frame, LeaseRequest, Quote};
use futures_util::{SinkExt, StreamExt};
use reqwest::StatusCode;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::error::{Error, Result};

const LEASE_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct NodeClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LeaseView {
    pub lease_id: String,
    pub session_id: String,
    pub ws_url: String,
    pub viewer_url: Option<String>,
    #[allow(dead_code)]
    pub quote: Quote,
    #[allow(dead_code)]
    pub status: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub billable_seconds: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub elapsed_seconds: Option<u64>,
    /// Whether the guest is actually running. A stored session outlives it.
    #[serde(default)]
    pub live: bool,
}

impl NodeClient {
    pub fn new(base: &str, token: Option<&str>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.map(str::to_string),
        })
    }

    pub async fn create_lease(&self, req: &LeaseRequest) -> Result<LeaseView> {
        let url = format!("{}/v1/leases", self.base);
        let res = self
            .authed(self.http.post(url))
            .timeout(LEASE_TIMEOUT)
            .json(req)
            .send()
            .await?;
        decode(res).await
    }

    pub async fn get_lease(&self, lease_id: &str) -> Result<LeaseView> {
        let url = format!("{}/v1/leases/{lease_id}", self.base);
        let res = self.authed(self.http.get(url)).send().await?;
        decode(res).await
    }

    pub async fn delete_lease(&self, lease_id: &str) -> Result<LeaseView> {
        let url = format!("{}/v1/leases/{lease_id}", self.base);
        let res = self.authed(self.http.delete(url)).send().await?;
        decode(res).await
    }

    pub async fn exec(&self, ws_url: &str, batch: &ActionBatch) -> Result<(Ack, Vec<Frame>)> {
        if ws_url.contains('?') {
            return Err(Error::Protocol(
                "ws url must not include a query string".into(),
            ));
        }
        let mut req = ws_url
            .into_client_request()
            .map_err(|err| Error::Ws(err.to_string()))?;
        if let Some(token) = &self.token {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|err| Error::Ws(format!("authorization header: {err}")))?;
            req.headers_mut().insert(AUTHORIZATION, value);
        }
        if req.uri().query().is_some() {
            return Err(Error::Protocol(
                "ws url must not include a query string".into(),
            ));
        }
        let (mut ws, _) =
            tokio::time::timeout(DEFAULT_TIMEOUT, tokio_tungstenite::connect_async(req))
                .await
                .map_err(|_| Error::Ws("timed out connecting to session".into()))??;

        let payload = serde_json::to_string(batch)?;
        ws.send(Message::Text(payload.into())).await?;

        let mut ack: Option<Ack> = None;
        let mut frames = Vec::new();
        loop {
            let msg = tokio::time::timeout(DEFAULT_TIMEOUT, ws.next())
                .await
                .map_err(|_| Error::Ws("timed out waiting for ack".into()))?
                .ok_or_else(|| Error::Ws("session closed before ack".into()))??;
            match msg {
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                Message::Close(_) => {
                    return Err(Error::Ws("session closed before ack".into()));
                }
                Message::Binary(_) => {
                    return Err(Error::Ws("expected text ActionBatch reply".into()));
                }
                Message::Text(text) => {
                    let value: serde_json::Value = serde_json::from_str(text.as_str())?;
                    match value.get("type").and_then(|t| t.as_str()) {
                        Some("ack") => {
                            let parsed: Ack = serde_json::from_value(value)?;
                            ack = Some(parsed);
                        }
                        Some("frame") => {
                            frames.push(serde_json::from_value(value)?);
                        }
                        Some("error") => {
                            let msg = value
                                .get("error")
                                .and_then(|e| e.as_str())
                                .unwrap_or("session error");
                            return Err(Error::Protocol(msg.to_string()));
                        }
                        other => {
                            return Err(Error::Protocol(format!(
                                "unexpected ws message type {other:?}"
                            )));
                        }
                    }
                }
            }
            if let Some(ack) = &ack {
                if ack.results.is_empty() {
                    return Err(Error::Protocol("empty ack".into()));
                }
                if let Some(bad) = ack.results.iter().find(|r| !r.ok) {
                    return Err(Error::Protocol(
                        bad.error
                            .clone()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "action failed".into()),
                    ));
                }
                let expected = ack.results.iter().filter(|r| r.frame).count();
                if frames.len() >= expected {
                    break;
                }
            }
        }
        let ack = ack.ok_or_else(|| Error::Ws("missing ack".into()))?;
        let wants_frame = batch
            .items
            .iter()
            .any(|item| matches!(item, Action::Screenshot {}));
        if wants_frame && frames.is_empty() {
            return Err(Error::Protocol("screenshot produced no frame".into()));
        }
        let _ = ws.close(None).await;
        Ok((ack, frames))
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => req.header(AUTHORIZATION, format!("Bearer {token}")),
            None => req,
        }
    }
}

async fn decode<T: DeserializeOwned>(res: reqwest::Response) -> Result<T> {
    let status = res.status();
    let bytes = res.bytes().await?;
    if !status.is_success() {
        return Err(Error::Api {
            status: status.as_u16(),
            message: api_message(status, &bytes),
        });
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn api_message(status: StatusCode, bytes: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes)
        && let Some(msg) = v.get("error").and_then(|e| e.as_str())
        && !msg.is_empty()
    {
        return msg.to_string();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let text = text.trim();
        if !text.is_empty() {
            return text.to_string();
        }
    }
    format!("http {status}")
}
