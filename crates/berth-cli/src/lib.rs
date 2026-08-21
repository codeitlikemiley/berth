mod client;
mod config;
mod error;

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;

use berth_protocol::{LeaseRequest, Os, Quote};
use clap::{Parser, Subcommand};

use crate::client::{LeaseView, NodeClient};
use crate::config::{
    Config, DEFAULT_NODE, DEFAULT_URL, NodeConfig, Session, berth_home, clear_session,
    load_session, normalize_url, require_session, save_session, validate_node_name,
};
pub use crate::error::{Error, Result};

#[derive(Debug, Parser)]
#[command(
    name = "berth",
    version,
    about = "Lease an isolated computer to an agent"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start a computer session
    Up {
        /// Guest OS (MVP: linux)
        #[arg(long, default_value = "linux")]
        os: String,
        /// Paired node name from config.toml
        #[arg(long, default_value = DEFAULT_NODE)]
        node: String,
    },
    /// Run a berth node
    #[command(subcommand)]
    Node(NodeCmd),
    /// MCP stdio server
    Mcp,
    /// Pair with a node
    Pair {
        /// Node base URL
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
        /// Pairing code printed by `berth node up`
        #[arg(long)]
        code: String,
        /// Name stored under [nodes.<name>] in config.toml
        #[arg(long, default_value = DEFAULT_NODE)]
        node: String,
    },
    /// End a session
    End,
    /// Print the session viewer URL
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

pub fn exit(cli: Cli) -> ExitCode {
    let home = match berth_home() {
        Ok(home) => home,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    match execute(cli, &home) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}

pub fn execute(cli: Cli, home: &Path) -> Result<String> {
    match cli.command {
        Command::Node(NodeCmd::Up { bind }) => {
            berth_node::serve_blocking(bind)?;
            Ok(String::new())
        }
        Command::Pair { url, code, node } => cmd_pair(home, &url, &code, &node),
        Command::Up { os, node } => cmd_up(home, &os, &node),
        Command::View => cmd_view(home),
        Command::End => cmd_end(home),
        Command::Status => cmd_status(home),
        Command::Mcp => Err(Error::NotImplemented("mcp")),
        Command::Doctor => Err(Error::NotImplemented("doctor")),
    }
}

fn cmd_pair(home: &Path, url: &str, code: &str, node: &str) -> Result<String> {
    validate_node_name(node)?;
    let url = normalize_url(url)?;
    if code.trim().is_empty() {
        return Err(Error::Usage("code is empty".into()));
    }
    let token = NodeClient::new(&url, None)?.pair(code.trim())?;
    let mut cfg = Config::load(home)?;
    cfg.nodes.insert(
        node.to_string(),
        NodeConfig {
            url: url.clone(),
            token,
        },
    );
    cfg.save(home)?;
    Ok(format!("paired {node} at {url}"))
}

fn cmd_up(home: &Path, os: &str, node: &str) -> Result<String> {
    validate_node_name(node)?;
    let req = mvp_lease_request(parse_os(os)?)?;
    if let Some(cur) = load_session(home)? {
        return Err(Error::Usage(format!(
            "lease {} is current; run berth end first",
            cur.lease_id
        )));
    }
    let cfg = Config::load(home)?;
    let node_cfg = cfg.node(node)?;
    let lease = NodeClient::new(&node_cfg.url, Some(&node_cfg.token))?.create_lease(&req)?;
    save_session(
        home,
        &Session {
            node: node.to_string(),
            lease_id: lease.lease_id.clone(),
            session_id: lease.session_id.clone(),
            viewer_url: lease.viewer_url.clone(),
            ws_url: Some(lease.ws_url.clone()),
        },
    )?;
    Ok(format_up(&lease))
}

fn cmd_view(home: &Path) -> Result<String> {
    let session = require_session(home)?;
    session
        .viewer_url
        .ok_or_else(|| Error::Usage(format!("lease {} has no viewer_url", session.lease_id)))
}

fn cmd_status(home: &Path) -> Result<String> {
    let session = require_session(home)?;
    let cfg = Config::load(home)?;
    let node = cfg.node(&session.node)?;
    let lease = NodeClient::new(&node.url, Some(&node.token))?.get_lease(&session.lease_id)?;
    Ok(format_status(&session.node, &node.url, &lease))
}

fn cmd_end(home: &Path) -> Result<String> {
    let session = require_session(home)?;
    let cfg = Config::load(home)?;
    let node = cfg.node(&session.node)?;
    let result = NodeClient::new(&node.url, Some(&node.token))?.delete_lease(&session.lease_id);
    match result {
        Ok(lease) => {
            clear_session(home)?;
            Ok(format_end(&lease))
        }
        Err(Error::Api { status: 404, .. }) => {
            clear_session(home)?;
            Ok(format!("lease {} gone", session.lease_id))
        }
        Err(err) => Err(err),
    }
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

fn format_up(lease: &LeaseView) -> String {
    let mut lines = vec![format!(
        "lease {} session {}",
        lease.lease_id, lease.session_id
    )];
    if let Some(viewer) = &lease.viewer_url {
        lines.push(format!("viewer {viewer}"));
    }
    lines.push(format_quote(&lease.lease_id, &lease.quote));
    lines.join("\n")
}

fn format_status(node: &str, url: &str, lease: &LeaseView) -> String {
    let mut lines = vec![
        format!("lease: {}", lease.lease_id),
        format!("status: {}", lease.status),
        format!("session: {}", lease.session_id),
        format!("node: {node} ({url})"),
    ];
    if let Some(viewer) = &lease.viewer_url {
        lines.push(format!("viewer: {viewer}"));
    }
    if let Some(secs) = lease.billable_seconds {
        lines.push(format!("billable_seconds: {secs}"));
    }
    if let Some(secs) = lease.elapsed_seconds {
        lines.push(format!("elapsed_seconds: {secs}"));
    }
    lines.join("\n")
}

fn format_end(lease: &LeaseView) -> String {
    match lease.billable_seconds {
        Some(secs) => format!("lease {} stopped billable_seconds={secs}", lease.lease_id),
        None => format!("lease {} {}", lease.lease_id, lease.status),
    }
}

fn format_quote(lease_id: &str, quote: &Quote) -> String {
    let usd = quote
        .usd_per_second()
        .ok()
        .map(|rate| rate * quote.min_seconds as f64)
        .unwrap_or(0.0);
    format!(
        "lease {lease_id} quote ${usd:.6} USD for {}s min (not charged)",
        quote.min_seconds
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{Expectation, Server, matchers::*, responders::*};
    use serde_json::json;
    use std::path::Path;

    fn run(home: &Path, args: &[&str]) -> Result<String> {
        let mut full = vec!["berth"];
        full.extend(args);
        let cli = Cli::try_parse_from(&full).unwrap();
        execute(cli, home)
    }

    fn sample_quote() -> serde_json::Value {
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

    fn sample_lease(status: &str, billable: Option<u64>) -> serde_json::Value {
        let mut lease = json!({
            "lease_id": "l_1",
            "session_id": "s_1",
            "ws_url": "ws://127.0.0.1:7432/v1/sessions/s_1",
            "viewer_url": "http://127.0.0.1:6080/vnc.html",
            "quote": sample_quote(),
            "status": status
        });
        if let Some(secs) = billable {
            lease["billable_seconds"] = json!(secs);
            lease["elapsed_seconds"] = json!(secs);
        }
        lease
    }

    #[test]
    fn parse_pair_and_up_flags() {
        let pair = Cli::try_parse_from([
            "berth",
            "pair",
            "--url",
            "http://127.0.0.1:7432",
            "--code",
            "ABCD-EFGH",
        ])
        .unwrap();
        assert!(matches!(
            pair.command,
            Command::Pair { ref url, ref code, ref node }
                if url == DEFAULT_URL && code == "ABCD-EFGH" && node == DEFAULT_NODE
        ));
        let up =
            Cli::try_parse_from(["berth", "up", "--os", "linux", "--node", "home-nuc"]).unwrap();
        assert!(matches!(
            up.command,
            Command::Up { ref os, ref node } if os == "linux" && node == "home-nuc"
        ));
    }

    fn unauthorized() -> impl httptest::responders::Responder {
        status_code(401)
            .append_header("content-type", "application/json")
            .body(r#"{"error":"unauthorized"}"#)
    }

    fn seed_pair(home: &Path, url: &str, token: &str) {
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: url.into(),
                token: token.into(),
            },
        );
        cfg.save(home).unwrap();
    }

    fn seed_session(home: &Path) {
        save_session(
            home,
            &Session {
                node: "default".into(),
                lease_id: "l_1".into(),
                session_id: "s_1".into(),
                viewer_url: Some("http://127.0.0.1:6080/vnc.html".into()),
                ws_url: Some("ws://127.0.0.1:7432/v1/sessions/s_1".into()),
            },
        )
        .unwrap();
    }

    #[test]
    fn up_windows_rejected_before_pair() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &["up", "--os", "windows"]).unwrap_err();
        assert!(err.to_string().contains("Windows"));
    }

    #[test]
    fn up_macos_rejected_without_http() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        let url = format!("http://{}", server.addr());
        seed_pair(dir.path(), &url, "brt_secret");
        let err = run(dir.path(), &["up", "--os", "macos"]).unwrap_err();
        assert!(err.to_string().contains("macOS"));
    }

    #[test]
    fn up_lease_unauthorized() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("POST", "/v1/leases"),
                request::headers(contains(("authorization", "Bearer brt_stale"))),
            ])
            .respond_with(unauthorized()),
        );
        let url = format!("http://{}", server.addr());
        seed_pair(dir.path(), &url, "brt_stale");
        let err = run(dir.path(), &["up", "--os", "linux"]).unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
        assert!(load_session(dir.path()).unwrap().is_none());
    }

    #[test]
    fn status_lease_unauthorized() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("GET", "/v1/leases/l_1"),
                request::headers(contains(("authorization", "Bearer brt_stale"))),
            ])
            .respond_with(unauthorized()),
        );
        let url = format!("http://{}", server.addr());
        seed_pair(dir.path(), &url, "brt_stale");
        seed_session(dir.path());
        let err = run(dir.path(), &["status"]).unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
        assert!(load_session(dir.path()).unwrap().is_some());
    }

    #[test]
    fn end_lease_unauthorized_keeps_session() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("DELETE", "/v1/leases/l_1"),
                request::headers(contains(("authorization", "Bearer brt_stale"))),
            ])
            .respond_with(unauthorized()),
        );
        let url = format!("http://{}", server.addr());
        seed_pair(dir.path(), &url, "brt_stale");
        seed_session(dir.path());
        let err = run(dir.path(), &["end"]).unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
        assert_eq!(require_session(dir.path()).unwrap().lease_id, "l_1");
    }

    #[test]
    fn end_404_clears_session() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("DELETE", "/v1/leases/l_1"),
                request::headers(contains(("authorization", "Bearer brt_secret"))),
            ])
            .respond_with(
                status_code(404)
                    .append_header("content-type", "application/json")
                    .body(r#"{"error":"not found"}"#),
            ),
        );
        let url = format!("http://{}", server.addr());
        seed_pair(dir.path(), &url, "brt_secret");
        seed_session(dir.path());
        let out = run(dir.path(), &["end"]).unwrap();
        assert!(out.contains("l_1"));
        assert!(out.contains("gone"));
        assert!(load_session(dir.path()).unwrap().is_none());
    }

    #[test]
    fn up_without_pair_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &["up", "--os", "linux"]).unwrap_err();
        assert!(err.to_string().contains("not paired"));
    }

    #[test]
    fn view_without_session_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &["view"]).unwrap_err();
        assert!(err.to_string().contains("no active session"));
    }

    #[test]
    fn mcp_not_implemented() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(dir.path(), &["mcp"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("mcp"));
    }

    #[test]
    fn pair_writes_config() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("POST", "/v1/pair"),
                request::body(json_decoded(
                    |v: &serde_json::Value| v["code"] == "ABCD-EFGH"
                )),
            ])
            .respond_with(json_encoded(json!({ "token": "brt_secret" }))),
        );
        let url = format!("http://{}", server.addr());
        let out = run(dir.path(), &["pair", "--url", &url, "--code", "ABCD-EFGH"]).unwrap();
        assert!(out.contains("paired default"));
        let cfg = Config::load(dir.path()).unwrap();
        let node = cfg.node("default").unwrap();
        assert_eq!(node.url, url);
        assert_eq!(node.token, "brt_secret");
    }

    #[test]
    fn pair_unauthorized() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/v1/pair")).respond_with(
                status_code(401)
                    .append_header("content-type", "application/json")
                    .body(r#"{"error":"unauthorized"}"#),
            ),
        );
        let url = format!("http://{}", server.addr());
        let err = run(dir.path(), &["pair", "--url", &url, "--code", "NOPE"]).unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
    }

    #[test]
    fn pair_up_view_status_end() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("POST", "/v1/pair"),
                request::body(json_decoded(
                    |v: &serde_json::Value| v["code"] == "ABCD-EFGH"
                )),
            ])
            .respond_with(json_encoded(json!({ "token": "brt_secret" }))),
        );
        server.expect(
            Expectation::matching(all_of![
                request::method_path("POST", "/v1/leases"),
                request::headers(contains(("authorization", "Bearer brt_secret"))),
                request::body(json_decoded(|v: &serde_json::Value| {
                    v["os"] == "linux"
                        && v["class"] == "private"
                        && v["license"] == "linux"
                        && v["density"] == "isolated"
                        && v["resources"]["vcpu"] == 2
                        && v["resources"]["mem_gib"] == 4
                        && v["resources"]["disk_gib"] == 40
                })),
            ])
            .respond_with(json_encoded(sample_lease("active", None))),
        );
        server.expect(
            Expectation::matching(all_of![
                request::method_path("GET", "/v1/leases/l_1"),
                request::headers(contains(("authorization", "Bearer brt_secret"))),
            ])
            .respond_with(json_encoded(sample_lease("active", None))),
        );
        server.expect(
            Expectation::matching(all_of![
                request::method_path("DELETE", "/v1/leases/l_1"),
                request::headers(contains(("authorization", "Bearer brt_secret"))),
            ])
            .respond_with(json_encoded(sample_lease("stopped", Some(60)))),
        );

        let url = format!("http://{}", server.addr());
        run(
            dir.path(),
            &[
                "pair",
                "--url",
                &url,
                "--code",
                "ABCD-EFGH",
                "--node",
                "home-nuc",
            ],
        )
        .unwrap();

        let up = run(dir.path(), &["up", "--os", "linux", "--node", "home-nuc"]).unwrap();
        assert!(up.contains("lease l_1 session s_1"));
        assert!(up.contains("http://127.0.0.1:6080/vnc.html"));
        assert!(up.contains("not charged"));

        let view = run(dir.path(), &["view"]).unwrap();
        assert_eq!(view, "http://127.0.0.1:6080/vnc.html");

        let status = run(dir.path(), &["status"]).unwrap();
        assert!(status.contains("status: active"));
        assert!(status.contains("home-nuc"));

        let ended = run(dir.path(), &["end"]).unwrap();
        assert!(ended.contains("stopped"));
        assert!(ended.contains("billable_seconds=60"));
        assert!(load_session(dir.path()).unwrap().is_none());
    }

    #[test]
    fn second_up_requires_end() {
        let dir = tempfile::tempdir().unwrap();
        save_session(
            dir.path(),
            &Session {
                node: "default".into(),
                lease_id: "l_old".into(),
                session_id: "s_old".into(),
                viewer_url: None,
                ws_url: None,
            },
        )
        .unwrap();
        let mut cfg = Config::default();
        cfg.nodes.insert(
            "default".into(),
            NodeConfig {
                url: DEFAULT_URL.into(),
                token: "brt_x".into(),
            },
        );
        cfg.save(dir.path()).unwrap();
        let err = run(dir.path(), &["up", "--os", "linux"]).unwrap_err();
        assert!(err.to_string().contains("l_old"));
        assert!(err.to_string().contains("berth end"));
    }
}
