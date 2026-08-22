//! Ignored unless `BERTH_E2E=1` (and `--ignored`). Needs Docker + the guest image.
//!
//! Mapping tests prove the adapter produces the actions we intended. They
//! cannot prove the guest can execute them -- which is exactly how the Anthropic
//! adapter came to emit drag, hold_key and cursor_position for months while the
//! node rejected all three at argv mapping. This runs a realistic Responses
//! payload through the adapter and into a real desktop.

use berth_adapter_openai::{actions_from_calls, computer_calls, outputs_from_ack};
use berth_node::SessionManager;
use berth_protocol::{Class, Density, Isolation, LeaseRequest, License, Os, Resources, Term};
use serde_json::json;

fn e2e_enabled() -> bool {
    matches!(std::env::var("BERTH_E2E"), Ok(v) if v == "1")
}

fn lease() -> LeaseRequest {
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
        workspace: None,
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

#[tokio::test]
#[ignore = "requires Docker; BERTH_E2E=1 cargo test -p berth-adapter-openai -- --ignored"]
async fn a_real_responses_payload_drives_a_real_desktop() {
    if !e2e_enabled() {
        eprintln!("skipping: set BERTH_E2E=1 to run docker e2e");
        return;
    }

    // Shaped like a captured Responses reply: batched actions, a call id to
    // echo, and a safety check to acknowledge.
    let reply = json!({
        "output": [
            { "type": "reasoning", "id": "rs_1" },
            {
                "type": "computer_call",
                "id": "cu_1",
                "call_id": "call_e2e",
                "status": "completed",
                "pending_safety_checks": [
                    { "id": "cu_sc_1", "code": "irrelevant_domain", "message": "ack me" }
                ],
                "actions": [
                    { "type": "move", "x": 640, "y": 400 },
                    { "type": "click", "button": "left", "x": 640, "y": 400 },
                    { "type": "type", "text": "openai-adapter-e2e" },
                    { "type": "keypress", "keys": ["ENTER"] },
                    { "type": "scroll", "x": 640, "y": 400, "scroll_x": 0, "scroll_y": 300 },
                    { "type": "drag", "path": [[100, 100], [300, 250]] },
                    { "type": "wait", "ms": 200 },
                    { "type": "screenshot" }
                ]
            }
        ]
    });

    let calls = computer_calls(&reply).expect("extract computer_call");
    assert_eq!(calls.len(), 1, "reasoning items must be ignored, not fatal");

    let manager = SessionManager::connect().expect("docker connect");
    let req = lease();
    let mut session = manager.start(&req).await.expect("start session");

    let batch = actions_from_calls(
        session.session_id(),
        "b_openai_e2e",
        &calls,
        Some((1280, 800)),
        Some((1280, 800)),
    )
    .expect("map to a protocol batch");
    assert_eq!(batch.items.len(), 8);

    let out = session.exec(batch).await.expect("exec");
    for (i, r) in out.ack.results.iter().enumerate() {
        assert!(
            r.ok,
            "adapter produced an action the guest cannot run: item {i}: {:?}",
            r.error
        );
    }

    let outputs = outputs_from_ack(&calls, &out.ack, &out.frames, None);
    assert_eq!(outputs.len(), 1);
    assert!(outputs[0].error.is_none(), "{:?}", outputs[0].error);
    assert_eq!(outputs[0].call_id, "call_e2e");
    assert!(
        outputs[0]
            .output
            .image_url
            .starts_with("data:image/png;base64,"),
        "screenshot must come back as a data URL"
    );
    // The safety check has to survive the round trip or the model stalls.
    assert_eq!(outputs[0].acknowledged_safety_checks.len(), 1);

    let volume = session.volume_name().to_string();
    session.stop().await.expect("stop");
    let _ = std::process::Command::new("docker")
        .args(["volume", "rm", "-f", &volume])
        .status();
}
