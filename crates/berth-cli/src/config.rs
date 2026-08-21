use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_NODE: &str = "default";
pub const DEFAULT_URL: &str = "http://127.0.0.1:7432";

/// $BERTH_HOME if set, otherwise ~/.berth (same directory the node uses for sqlite).
pub fn berth_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("BERTH_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::Config("HOME is not set; cannot open ~/.berth/config.toml".into()))?;
    Ok(PathBuf::from(home).join(".berth"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeConfig>,
    /// Comma-separated egress hosts. Missing = default. Empty = deny-all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist: Option<String>,
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

    pub fn save(&self, home: &Path) -> Result<()> {
        write_secret_file(&home.join("config.toml"), &toml::to_string_pretty(self)?)
    }

    pub fn node(&self, name: &str) -> Result<&NodeConfig> {
        self.nodes.get(name).ok_or_else(|| {
            Error::Config(format!(
                "node `{name}` is not paired; run berth pair --url {DEFAULT_URL} --code <code>"
            ))
        })
    }
}

pub fn validate_node_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Usage("node name is empty".into()));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(Error::Usage(format!(
            "node name `{name}` must be alphanumeric, hyphen, or underscore"
        )));
    }
    Ok(())
}

pub fn normalize_url(raw: &str) -> Result<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(Error::Usage("url is empty".into()));
    }
    if raw.contains('?') || raw.contains('#') {
        return Err(Error::Usage("url must not include a query string".into()));
    }
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return Err(Error::Usage(
            "url must start with http:// or https://".into(),
        ));
    }
    Ok(raw.to_string())
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

pub(crate) fn url_is_loopback(url: &str) -> bool {
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

pub fn load_session(home: &Path) -> Result<Option<Session>> {
    let path = home.join("session.toml");
    match fs::read_to_string(&path) {
        Ok(raw) if raw.trim().is_empty() => Ok(None),
        Ok(raw) => Ok(Some(toml::from_str(&raw)?)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn require_session(home: &Path) -> Result<Session> {
    load_session(home)?
        .ok_or_else(|| Error::Usage("no active session; run berth up --os linux".into()))
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
    fn parse_nodes_default() {
        let cfg: Config = toml::from_str(
            r#"
[nodes.default]
url = "http://127.0.0.1:7432"
token = "brt_test"
"#,
        )
        .unwrap();
        let node = cfg.node("default").unwrap();
        assert_eq!(node.url, "http://127.0.0.1:7432");
        assert_eq!(node.token, "brt_test");
    }

    #[test]
    fn missing_node_mentions_pair() {
        let cfg = Config::default();
        let err = cfg.node("home-nuc").unwrap_err();
        assert!(err.to_string().contains("not paired"));
        assert!(err.to_string().contains("berth pair"));
    }

    #[test]
    fn roundtrip_named_node() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "home-nuc".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "brt_secret".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded, cfg);
        let raw = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(raw.contains("[nodes.home-nuc]"));
        assert!(raw.contains("brt_secret"));
    }

    #[test]
    fn load_missing_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(dir.path()).unwrap();
        assert!(cfg.nodes.is_empty());
        assert!(cfg.allowlist.is_none());
    }

    #[test]
    fn empty_allowlist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config {
            allowlist: Some(String::new()),
            ..Config::default()
        };
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "brt_secret".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.allowlist.as_deref(), Some(""));
        let raw = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(raw.contains("allowlist"));
        assert!(raw.contains("brt_secret"));
    }

    #[test]
    fn session_roundtrip_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_session(dir.path()).unwrap().is_none());
        let session = Session {
            node: "default".into(),
            lease_id: "l_1".into(),
            session_id: "s_1".into(),
            viewer_url: Some("http://127.0.0.1:6080/vnc.html".into()),
            ws_url: Some("ws://127.0.0.1:7432/v1/sessions/s_1".into()),
        };
        save_session(dir.path(), &session).unwrap();
        assert_eq!(require_session(dir.path()).unwrap(), session);
        clear_session(dir.path()).unwrap();
        assert!(load_session(dir.path()).unwrap().is_none());
    }

    #[test]
    fn normalize_url_strips_slash() {
        assert_eq!(
            normalize_url("http://127.0.0.1:7432/").unwrap(),
            "http://127.0.0.1:7432"
        );
        assert!(normalize_url("ftp://x").is_err());
    }

    #[test]
    fn normalize_url_rejects_query_and_fragment() {
        let err = normalize_url("https://host.example/?token=brt_secret").unwrap_err();
        assert!(err.to_string().contains("query string"));
        assert!(!err.to_string().contains("brt_secret"));
        assert!(!err.to_string().contains("host.example"));
        let hash = normalize_url("https://host.example/#token").unwrap_err();
        assert!(hash.to_string().contains("query string"));
        assert!(!hash.to_string().contains("token"));
    }

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
                Some("wss://unit-test.trycloudflare.com/v1/sessions/s_1?token=brt_secret"),
                "https://unit-test.trycloudflare.com",
                "s_1"
            ),
            "wss://unit-test.trycloudflare.com/v1/sessions/s_1"
        );
    }

    fn leftover_tmps(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp") || name == "config.tmp")
            .collect()
    }

    #[test]
    fn save_does_not_use_shared_config_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "brt_a".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        cfg.nodes.get_mut("default").unwrap().token = "brt_b".into();
        cfg.save(dir.path()).unwrap();
        assert!(!dir.path().join("config.tmp").exists());
        assert!(leftover_tmps(dir.path()).is_empty());
        assert_eq!(
            Config::load(dir.path())
                .unwrap()
                .node("default")
                .unwrap()
                .token,
            "brt_b"
        );
    }

    #[test]
    fn concurrent_saves_use_unique_tmp_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        std::thread::scope(|s| {
            for i in 0..8 {
                let home = home.clone();
                s.spawn(move || {
                    let mut cfg = Config::default();
                    cfg.nodes.insert(
                        format!("n{i}"),
                        NodeConfig {
                            url: "http://127.0.0.1:7432".into(),
                            token: format!("brt_{i}"),
                        },
                    );
                    cfg.save(&home).unwrap();
                });
            }
        });
        assert!(!home.join("config.tmp").exists());
        assert!(
            leftover_tmps(&home).is_empty(),
            "{:?}",
            leftover_tmps(&home)
        );
        let cfg = Config::load(&home).unwrap();
        assert!(!cfg.nodes.is_empty());
        toml::from_str::<Config>(&fs::read_to_string(home.join("config.toml")).unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn config_dir_and_file_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let nest = dir.path().join("berth");
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: "http://127.0.0.1:7432".into(),
                token: "brt_secret".into(),
            },
        );
        cfg.save(&nest).unwrap();
        let dir_mode = fs::metadata(&nest).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = fs::metadata(nest.join("config.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
    }
}
