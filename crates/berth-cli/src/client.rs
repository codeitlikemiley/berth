use std::time::Duration;

use berth_protocol::{LeaseRequest, Quote};
use reqwest::StatusCode;
use reqwest::blocking::Client as Http;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// Guest boot can take ~30s; lease POST waits longer than pair/status.
const LEASE_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct NodeClient {
    http: Http,
    base: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LeaseView {
    pub lease_id: String,
    pub session_id: String,
    pub ws_url: String,
    pub viewer_url: Option<String>,
    pub quote: Quote,
    pub status: String,
    #[serde(default)]
    pub billable_seconds: Option<u64>,
    #[serde(default)]
    pub elapsed_seconds: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub started_at: i64,
    #[serde(default)]
    #[allow(dead_code)]
    pub stopped_at: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub workspace_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub live: bool,
}

impl NodeClient {
    pub fn new(base: &str, token: Option<&str>) -> Result<Self> {
        let http = Http::builder().timeout(DEFAULT_TIMEOUT).build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.map(str::to_string),
        })
    }

    pub fn pair(&self, code: &str, revoke_others: bool) -> Result<String> {
        let url = format!("{}/v1/pair", self.base);
        let res = self
            .http
            .post(url)
            .json(&serde_json::json!({ "code": code, "revoke_others": revoke_others }))
            .send()?;
        let body: serde_json::Value = decode(res)?;
        let token = body
            .get("token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Config("pair response missing token".into()))?;
        Ok(token.to_string())
    }

    pub fn create_lease(&self, req: &LeaseRequest) -> Result<LeaseView> {
        let url = format!("{}/v1/leases", self.base);
        let res = self
            .authed(self.http.post(url))
            .timeout(LEASE_TIMEOUT)
            .json(req)
            .send()?;
        decode(res)
    }

    pub fn get_lease(&self, lease_id: &str) -> Result<LeaseView> {
        let url = format!("{}/v1/leases/{lease_id}", self.base);
        let res = self.authed(self.http.get(url)).send()?;
        decode(res)
    }

    pub fn delete_lease(&self, lease_id: &str) -> Result<LeaseView> {
        let url = format!("{}/v1/leases/{lease_id}", self.base);
        let res = self.authed(self.http.delete(url)).send()?;
        decode(res)
    }

    fn authed(&self, req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        match &self.token {
            Some(token) => req.header("Authorization", format!("Bearer {token}")),
            None => req,
        }
    }
}

fn decode<T: DeserializeOwned>(res: reqwest::blocking::Response) -> Result<T> {
    let status = res.status();
    let bytes = res.bytes()?;
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
