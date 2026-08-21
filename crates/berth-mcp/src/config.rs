use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_NODE: &str = "default";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub node: String,
    pub lease_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
}

impl Config {
    pub fn load(home: &Path) -> Result<Self> {
        let path = home.join("config.toml");
        match fs::read_to_string(&path) {
            Ok(raw) if raw.trim().is_empty() => Ok(Self::default()),
            Ok(raw) => Ok(toml::from_str(&raw)?),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn node(&self, name: &str) -> Result<&NodeConfig> {
        self.nodes.get(name).ok_or_else(|| {
            Error::Config(format!(
                "node `{name}` is not paired; run berth pair --url http://127.0.0.1:7432 --code <code>"
            ))
        })
    }
}

pub fn load_session(home: &Path) -> Result<Option<Session>> {
    let path = home.join("session.toml");
    match fs::read_to_string(&path) {
        Ok(raw) if raw.trim().is_empty() => Ok(None),
        Ok(raw) => Ok(Some(toml::from_str(&raw)?)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn save_session(home: &Path, session: &Session) -> Result<()> {
    write_secret_file(
        &home.join("session.toml"),
        &toml::to_string_pretty(session)?,
    )
}

pub fn clear_session(home: &Path) -> Result<()> {
    let path = home.join("session.toml");
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn berth_session_env() -> Option<String> {
    std::env::var("BERTH_SESSION")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Last session.toml, optionally overridden by BERTH_SESSION=session_id.
pub fn existing_session(home: &Path) -> Result<Option<Session>> {
    let file = load_session(home)?;
    let env_id = berth_session_env();
    match (file, env_id) {
        (Some(mut session), Some(id)) => {
            if session.session_id != id {
                session.session_id = id;
                session.ws_url = None;
            }
            Ok(Some(session))
        }
        (Some(session), None) => Ok(Some(session)),
        (None, Some(id)) => Ok(Some(Session {
            node: DEFAULT_NODE.into(),
            lease_id: String::new(),
            session_id: id,
            viewer_url: None,
            ws_url: None,
        })),
        (None, None) => Ok(None),
    }
}

pub fn resolve_session(home: &Path) -> Result<Session> {
    existing_session(home)?
        .ok_or_else(|| Error::Usage("no active session; call berth_lease first".into()))
}

pub fn http_to_ws(base: &str, session_id: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if base.starts_with("ws://") || base.starts_with("wss://") {
        base.to_string()
    } else {
        format!("ws://{base}")
    };
    format!("{ws}/v1/sessions/{session_id}")
}

pub fn resolve_ws_url(ws_url: Option<&str>, node_url: &str, session_id: &str) -> String {
    let fallback = http_to_ws(node_url, session_id);
    let Some(url) = ws_url else {
        return fallback;
    };
    if url.contains('?') || !url.contains(session_id) {
        return fallback;
    }
    if url_is_loopback(url) && !url_is_loopback(node_url) {
        return fallback;
    }
    url.to_string()
}

fn url_is_loopback(url: &str) -> bool {
    url_host(url).is_some_and(host_is_loopback)
}

fn url_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("wss://"))
        .or_else(|| url.strip_prefix("ws://"))?;
    if let Some(rest) = rest.strip_prefix('[') {
        return rest.split(']').next();
    }
    rest.split(['/', ':', '?']).next().filter(|h| !h.is_empty())
}

fn host_is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        secure_mkdir(parent)?;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Config("invalid config path".into()))?;

    let mut last_exists = None;
    for _ in 0..32 {
        let tmp = unique_tmp_path(parent, name);
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&tmp) {
            Ok(mut file) => {
                let wrote = write_all_sync(&mut file, contents);
                if let Err(err) = wrote {
                    let _ = fs::remove_file(&tmp);
                    return Err(err.into());
                }
                if let Err(err) = fs::rename(&tmp, path) {
                    let _ = fs::remove_file(&tmp);
                    return Err(err.into());
                }
                #[cfg(unix)]
                set_mode(path, 0o600)?;
                return Ok(());
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                last_exists = Some(err);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(last_exists.map(Into::into).unwrap_or_else(|| {
        Error::Config(format!("could not create temp file for {}", path.display()))
    }))
}

fn unique_tmp_path(parent: &Path, name: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{name}.{}.{n}.tmp", std::process::id()))
}

fn write_all_sync(file: &mut File, contents: &str) -> std::io::Result<()> {
    file.write_all(contents.as_bytes())?;
    if !contents.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.sync_all()
}

fn secure_mkdir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    set_mode(path, 0o700)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ws_rewrites_loopback_onto_https_origin() {
        assert_eq!(
            resolve_ws_url(
                Some("ws://127.0.0.1:7432/v1/sessions/s_1"),
                "https://unit-test.trycloudflare.com",
                "s_1"
            ),
            "wss://unit-test.trycloudflare.com/v1/sessions/s_1"
        );
        assert_eq!(
            resolve_ws_url(
                Some("wss://unit-test.trycloudflare.com/v1/sessions/s_1"),
                "https://unit-test.trycloudflare.com",
                "s_1"
            ),
            "wss://unit-test.trycloudflare.com/v1/sessions/s_1"
        );
        assert_eq!(
            resolve_ws_url(
                Some("ws://127.0.0.1:7432/v1/sessions/s_1"),
                "http://127.0.0.1:7432",
                "s_1"
            ),
            "ws://127.0.0.1:7432/v1/sessions/s_1"
        );
        assert_eq!(
            resolve_ws_url(
                Some("ws://localhost:7432/v1/sessions/s_1"),
                "https://unit-test.trycloudflare.com",
                "s_1"
            ),
            "wss://unit-test.trycloudflare.com/v1/sessions/s_1"
        );
        assert_eq!(
            resolve_ws_url(
                Some("wss://unit-test.trycloudflare.com/v1/sessions/s_1?token=brt_secret"),
                "https://unit-test.trycloudflare.com",
                "s_1"
            ),
            "wss://unit-test.trycloudflare.com/v1/sessions/s_1"
        );
    }
}
