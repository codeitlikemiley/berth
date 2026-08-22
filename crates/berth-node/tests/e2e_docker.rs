//! Ignored unless `BERTH_E2E=1` (and `--ignored`). Needs Docker + the guest image.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use berth_node::{FRAME_HEIGHT, FRAME_WIDTH, SessionManager};
use berth_protocol::{
    Action, ActionBatch, ActionBatchKind, Button, Class, Density, Egress, Isolation, LeaseRequest,
    License, Network, Os, Resources, Term, Workspace,
};

fn e2e_enabled() -> bool {
    matches!(std::env::var("BERTH_E2E"), Ok(v) if v == "1")
}

fn unique_ws() -> String {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("e2e-{}-{ns:x}", std::process::id())
}

fn lease(workspace_id: &str) -> LeaseRequest {
    LeaseRequest {
        os: Os::Linux,
        class: Class::Private,
        license: License::Linux,
        density: Density::Isolated,
        pooled: false,
        term: Term::OnDemand,
        resources: Resources {
            vcpu: 2,
            mem_gib: 2,
            disk_gib: 10,
        },
        workspace: Some(Workspace {
            id: workspace_id.to_string(),
            disk_gib: 10,
        }),
        object: None,
        cpu_overcommit: None,
        min_seconds: 60,
        max_seconds: 0,
        exclusive_hardware: false,
        capabilities: vec![],
        image: None,
        region: None,
        isolation: Isolation::Vm,
        network: None,
        recording: None,
        human_confirm: vec![],
        preemptible: None,
    }
}

/// Exec as `berth`, the way the node itself does.
///
/// Not a detail: the guest drops ALL capabilities, so root has no
/// CAP_DAC_OVERRIDE and cannot write to a berth-owned /workspace either. Root
/// here fails with EACCES, which is the isolation working rather than a bug.
fn docker_exec(container: &str, shell: &str) -> String {
    let out = Command::new("docker")
        .args(["exec", "-u", "berth", container, "sh", "-c", shell])
        .output()
        .expect("docker exec");
    assert!(
        out.status.success(),
        "docker exec failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn inspect_host_config(container: &str) -> serde_json::Value {
    let out = Command::new("docker")
        .args(["inspect", container])
        .output()
        .expect("docker inspect");
    assert!(
        out.status.success(),
        "docker inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("inspect json");
    v[0]["HostConfig"].clone()
}

#[tokio::test]
#[ignore = "requires Docker; BERTH_E2E=1 cargo test -p berth-node -- --ignored"]
async fn session_screenshot_and_workspace_persists() {
    if !e2e_enabled() {
        eprintln!("skipping: set BERTH_E2E=1 to run docker e2e");
        return;
    }

    let manager = SessionManager::connect().expect("docker connect");
    let ws = unique_ws();
    let req = lease(&ws);

    let mut session = manager.start(&req).await.expect("start session");
    assert_ne!(
        session
            .volume_name()
            .strip_prefix("berth-ws-")
            .unwrap_or(""),
        ""
    );

    let hc = inspect_host_config(session.container_id());
    assert_ne!(hc["NetworkMode"], "host");
    assert_ne!(hc["NetworkMode"], "bridge");
    assert_eq!(hc["Privileged"], false);
    let cap_drop = hc["CapDrop"]
        .as_array()
        .expect("CapDrop")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert!(cap_drop.iter().any(|c| c.eq_ignore_ascii_case("ALL")));
    assert!(hc["Binds"].is_null() || hc["Binds"].as_array().is_some_and(|b| b.is_empty()));
    assert_eq!(hc["PortBindings"]["6080/tcp"][0]["HostIp"], "127.0.0.1");
    let sec = hc["SecurityOpt"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert!(
        sec.iter().any(|s| s.contains("no-new-privileges")),
        "{sec:?}"
    );
    let cap_add = hc["CapAdd"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert!(
        cap_add.iter().any(|c| c.eq_ignore_ascii_case("NET_ADMIN")),
        "{cap_add:?}"
    );

    let inspect = Command::new("docker")
        .args(["inspect", session.container_id()])
        .output()
        .expect("docker inspect");
    let inspect_json: serde_json::Value =
        serde_json::from_slice(&inspect.stdout).expect("inspect json");
    let env = inspect_json[0]["Config"]["Env"]
        .as_array()
        .expect("Env")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert!(env.iter().any(|e| e == &"DISPLAY=:99"), "{env:?}");
    assert!(
        env.iter().any(|e| e.starts_with("BERTH_ALLOWLIST=")),
        "{env:?}"
    );

    let frame = session.screenshot().await.expect("screenshot");
    assert!(
        frame.data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "expected PNG magic"
    );
    assert!(!frame.data.is_empty());
    assert_eq!(frame.width, FRAME_WIDTH);
    assert_eq!(frame.height, FRAME_HEIGHT);
    assert_eq!(frame.mime, "image/png");

    let typed = session
        .exec(ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_type".into(),
            session_id: session.session_id().to_string(),
            items: vec![
                Action::Click {
                    button: Button::Left,
                    xy: [200, 120],
                    mods: vec![],
                },
                Action::Type {
                    text: "echo typed-ok > /workspace/typed.txt".into(),
                },
                Action::Key {
                    keys: vec!["Return".into()],
                    repeat: 1,
                },
                Action::Wait { ms: 800 },
            ],
        })
        .await
        .expect("type into xterm");
    assert!(typed.ack.results.iter().all(|r| r.ok), "{:?}", typed.ack);

    let typed_body = docker_exec(session.container_id(), "cat /workspace/typed.txt");
    assert_eq!(typed_body, "typed-ok");

    let marker = "berth-persist-ok";
    docker_exec(
        session.container_id(),
        &format!("echo {marker} > /workspace/persist.txt"),
    );
    assert_eq!(
        docker_exec(session.container_id(), "cat /workspace/persist.txt"),
        marker
    );

    let shot = session
        .exec(ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_e2e".into(),
            session_id: session.session_id().to_string(),
            items: vec![
                Action::Wait { ms: 50 },
                Action::Screenshot {},
                Action::Shell {
                    cmd: "should-skip".into(),
                },
            ],
        })
        .await
        .expect("exec batch");
    assert!(shot.ack.results[0].ok);
    assert!(shot.ack.results[1].ok && shot.ack.results[1].frame);
    assert_eq!(shot.frames.len(), 1);
    assert_eq!(shot.frames[0].width, FRAME_WIDTH);
    assert!(!shot.ack.results[2].ok);

    let fail = session
        .exec(ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_skip".into(),
            session_id: session.session_id().to_string(),
            items: vec![
                Action::Shell {
                    cmd: "uname -a".into(),
                },
                Action::Wait { ms: 1 },
            ],
        })
        .await
        .expect("skip batch");
    assert!(!fail.ack.results[0].ok);
    assert_eq!(fail.ack.results[1].error.as_deref(), Some("skipped"));

    session.stop().await.expect("stop keeps volume");

    let mut session2 = manager.start(&req).await.expect("start again");
    let body = docker_exec(session2.container_id(), "cat /workspace/persist.txt");
    assert_eq!(body, marker, "workspace volume must survive stop");
    let volume = session2.volume_name().to_string();
    session2.stop().await.expect("stop 2");
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &volume])
        .status();
}

fn inspect_env(container: &str) -> Vec<String> {
    let out = Command::new("docker")
        .args(["inspect", container])
        .output()
        .expect("docker inspect");
    assert!(
        out.status.success(),
        "docker inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("inspect json");
    v[0]["Config"]["Env"]
        .as_array()
        .expect("Env")
        .iter()
        .filter_map(|e| e.as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker; BERTH_E2E=1 cargo test -p berth-node -- --ignored"]
async fn empty_allowlist_denies_outbound() {
    if !e2e_enabled() {
        eprintln!("skipping: set BERTH_E2E=1 to run docker e2e");
        return;
    }

    let manager = SessionManager::connect().expect("docker connect");
    let ws = unique_ws();
    let mut req = lease(&ws);
    req.network = Some(Network {
        egress: Egress::Allowlist,
        domains: vec![],
    });

    let mut session = manager.start(&req).await.expect("start session");
    let id = session.container_id().to_string();
    let env = inspect_env(&id);
    assert!(
        env.iter().any(|e| e == "BERTH_ALLOWLIST="),
        "expected empty BERTH_ALLOWLIST, got {env:?}"
    );

    let curl = Command::new("docker")
        .args([
            "exec",
            "-u",
            "berth",
            &id,
            "curl",
            "-4",
            "-sS",
            "-m",
            "5",
            "--connect-timeout",
            "3",
            "-o",
            "/dev/null",
            "https://1.1.1.1",
        ])
        .output()
        .expect("docker exec curl");
    assert!(
        !curl.status.success(),
        "berth curl to 1.1.1.1:443 should fail under deny-all: stdout={} stderr={}",
        String::from_utf8_lossy(&curl.stdout),
        String::from_utf8_lossy(&curl.stderr)
    );

    let ipt = Command::new("docker")
        .args([
            "exec", "-u", "berth", &id, "iptables", "-P", "OUTPUT", "ACCEPT",
        ])
        .output()
        .expect("docker exec iptables");
    assert!(
        !ipt.status.success(),
        "berth must not be able to change OUTPUT policy: stdout={} stderr={}",
        String::from_utf8_lossy(&ipt.stdout),
        String::from_utf8_lossy(&ipt.stderr)
    );

    session.stop().await.expect("stop");
}

/// Anthropic's computer-use toolset issues drag, hold_key and cursor_position.
/// All three used to fail argv mapping and abort the rest of the batch, so an
/// agent driving a berth guest hit errors on verbs the protocol advertises.
#[tokio::test]
#[ignore = "requires Docker; BERTH_E2E=1 cargo test -p berth-node -- --ignored"]
async fn driver_covers_every_action_the_protocol_advertises() {
    if !e2e_enabled() {
        eprintln!("skipping: set BERTH_E2E=1 to run docker e2e");
        return;
    }

    let manager = SessionManager::connect().expect("docker connect");
    let ws = unique_ws();
    let req = lease(&ws);
    let mut session = manager.start(&req).await.expect("start session");

    let out = session
        .exec(ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_verbs".into(),
            session_id: session.session_id().to_string(),
            items: vec![
                Action::Move { xy: [640, 400] },
                // The pointer is read back below; Frame.cursor was in the
                // protocol from the start and nothing ever populated it.
                Action::CursorPosition {},
                Action::Drag {
                    path: vec![[100, 100], [300, 200], [500, 400]],
                },
                Action::HoldKey {
                    keys: vec!["shift".into()],
                    ms: 120,
                },
                Action::Click {
                    button: Button::Left,
                    xy: [400, 300],
                    mods: vec!["ctrl".into()],
                },
                Action::DoubleClick {
                    xy: [400, 300],
                    button: Button::Right,
                },
                // [x, y, x2, y2], so this is a 100x50 crop.
                Action::Zoom {
                    region: [10, 20, 110, 70],
                },
            ],
        })
        .await
        .expect("exec batch");

    for (i, r) in out.ack.results.iter().enumerate() {
        assert!(r.ok, "item {i} failed: {:?}", r.error);
    }

    // cursor_position and zoom both answer with a frame; the protocol has no
    // other carrier for a reply.
    assert_eq!(out.frames.len(), 2, "cursor_position and zoom each reply");

    let cursor_frame = &out.frames[0];
    assert_eq!(cursor_frame.width, FRAME_WIDTH);
    assert_eq!(cursor_frame.height, FRAME_HEIGHT);
    assert_eq!(
        cursor_frame.cursor,
        Some([640, 400]),
        "cursor_position must report where Move just put the pointer"
    );

    let zoom_frame = &out.frames[1];
    assert_eq!(zoom_frame.width, 100, "zoom crops to the region's width");
    assert_eq!(zoom_frame.height, 50, "zoom crops to the region's height");

    // Nothing may be left held down after drag and hold_key.
    let loc = docker_exec(session.container_id(), "xdotool getmouselocation --shell");
    assert!(loc.contains("X=400") && loc.contains("Y=300"), "{loc}");

    // Shell stays refused on purpose: running arbitrary commands in the guest
    // is a different security decision from driving its desktop.
    let refused = session
        .exec(ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_shell".into(),
            session_id: session.session_id().to_string(),
            items: vec![Action::Shell {
                cmd: "uname -a".into(),
            }],
        })
        .await
        .expect("shell batch");
    assert!(!refused.ack.results[0].ok);
    assert!(
        refused.ack.results[0]
            .error
            .as_deref()
            .is_some_and(|e| e.contains("shell is not exposed")),
        "{:?}",
        refused.ack.results[0].error
    );

    let volume = session.volume_name().to_string();
    session.stop().await.expect("stop");
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &volume])
        .status();
}
