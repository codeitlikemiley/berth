//! Ignored unless `BERTH_E2E=1` (and `--ignored`). Needs Docker + the guest image.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use berth_node::{FRAME_HEIGHT, FRAME_WIDTH, SessionManager};
use berth_protocol::{
    Action, ActionBatch, ActionBatchKind, Class, Density, Isolation, LeaseRequest, License, Os,
    Resources, Term, Workspace,
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

fn docker_exec(container: &str, shell: &str) -> String {
    let out = Command::new("docker")
        .args(["exec", container, "sh", "-c", shell])
        .output()
        .expect("docker exec");
    assert!(
        out.status.success(),
        "docker exec failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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

    let session = manager.start(&req).await.expect("start session");
    assert_ne!(
        session
            .volume_name()
            .strip_prefix("berth-ws-")
            .unwrap_or(""),
        ""
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

    let marker = "berth-persist-ok";
    docker_exec(
        session.container_id(),
        &format!("echo {marker} > /workspace/persist.txt"),
    );
    assert_eq!(
        docker_exec(session.container_id(), "cat /workspace/persist.txt"),
        marker
    );

    let ack = session
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
    assert!(ack.results[0].ok);
    assert!(ack.results[1].ok && ack.results[1].frame);
    assert!(!ack.results[2].ok);

    let fail_ack = session
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
    assert!(!fail_ack.results[0].ok);
    assert_eq!(fail_ack.results[1].error.as_deref(), Some("skipped"));

    session.stop().await.expect("stop keeps volume");

    let session2 = manager.start(&req).await.expect("start again");
    let body = docker_exec(session2.container_id(), "cat /workspace/persist.txt");
    assert_eq!(body, marker, "workspace volume must survive stop");
    let volume = session2.volume_name().to_string();
    session2.stop().await.expect("stop 2");
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &volume])
        .status();
}
