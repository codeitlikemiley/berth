use berth_adapter_openai::{ComputerCall, actions_from_calls, computer_calls, outputs_from_ack};
use berth_protocol::{Ack, AckKind, AckResult, Action, Button, Frame, FrameKind};
use serde_json::{Value, json};

fn call_item(actions: Value) -> Value {
    json!({
        "type": "computer_call",
        "id": "cu_1",
        "call_id": "call_1",
        "status": "completed",
        "actions": actions,
    })
}

fn calls_of(actions: Value) -> Vec<ComputerCall> {
    computer_calls(&json!({ "output": [call_item(actions)] })).expect("extract")
}

fn map_one(action: Value) -> Action {
    let calls = calls_of(json!([action]));
    let batch = actions_from_calls("s_1", "b_1", &calls, None, None).expect("map");
    batch.items.into_iter().next().expect("one action")
}

fn frame(w: u32, h: u32, data: &[u8]) -> Frame {
    Frame {
        kind: FrameKind::Frame,
        session_id: "s_1".into(),
        ts: 0,
        width: w,
        height: h,
        mime: "image/png".into(),
        data: data.to_vec(),
        cursor: None,
    }
}

fn ok_ack(n: u32) -> Ack {
    Ack {
        kind: AckKind::Ack,
        id: "b_1".into(),
        results: (0..n)
            .map(|i| AckResult {
                i,
                ok: true,
                frame: false,
                error: None,
            })
            .collect(),
    }
}

#[test]
fn extracts_from_response_array_or_single_item() {
    let item = call_item(json!([{ "type": "screenshot" }]));
    assert_eq!(
        computer_calls(&json!({ "output": [item.clone()] }))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(computer_calls(&json!([item.clone()])).unwrap().len(), 1);
    assert_eq!(computer_calls(&item).unwrap().len(), 1);
    // Non-computer output items are ignored, not an error.
    let mixed = json!({ "output": [ { "type": "message" }, item ] });
    assert_eq!(computer_calls(&mixed).unwrap().len(), 1);
}

/// GA batches actions; the older shape carried a single `action` and leaves
/// `actions` absent.
#[test]
fn accepts_batched_actions_and_the_older_single_action() {
    let batched = calls_of(json!([
        { "type": "move", "x": 1, "y": 2 },
        { "type": "screenshot" }
    ]));
    assert_eq!(batched[0].actions.len(), 2);

    let legacy = computer_calls(&json!({
        "output": [{
            "type": "computer_call",
            "call_id": "call_legacy",
            "action": { "type": "screenshot" }
        }]
    }))
    .expect("legacy shape");
    assert_eq!(legacy[0].actions.len(), 1);
    assert_eq!(legacy[0].call_id, "call_legacy");
}

#[test]
fn maps_every_action_openai_can_emit() {
    assert_eq!(
        map_one(json!({ "type": "screenshot" })),
        Action::Screenshot {}
    );
    assert_eq!(
        map_one(json!({ "type": "click", "button": "left", "x": 4, "y": 5 })),
        Action::Click {
            button: Button::Left,
            xy: [4, 5],
            mods: vec![],
        }
    );
    assert_eq!(
        map_one(json!({ "type": "click", "button": "right", "x": 4, "y": 5, "keys": ["SHIFT"] })),
        Action::Click {
            button: Button::Right,
            xy: [4, 5],
            mods: vec!["SHIFT".to_string()],
        }
    );
    assert_eq!(
        map_one(json!({ "type": "double_click", "x": 7, "y": 8 })),
        Action::DoubleClick {
            xy: [7, 8],
            button: Button::Left,
        }
    );
    assert_eq!(
        map_one(json!({ "type": "move", "x": 9, "y": 10 })),
        Action::Move { xy: [9, 10] }
    );
    assert_eq!(
        map_one(json!({ "type": "type", "text": "penguin" })),
        Action::Type {
            text: "penguin".into()
        }
    );
    assert_eq!(
        map_one(json!({ "type": "keypress", "keys": ["CTRL", "C"] })),
        Action::Key {
            keys: vec!["CTRL".into(), "C".into()],
            repeat: 1,
        }
    );
}

/// OpenAI sends pixels; the protocol carries wheel notches, because the guest
/// replays them as Button4/5 clicks. Passing pixels straight through would
/// scroll ~100x too far.
#[test]
fn scroll_pixels_become_wheel_notches() {
    assert_eq!(
        map_one(json!({ "type": "scroll", "x": 1, "y": 2, "scroll_x": 0, "scroll_y": 300 })),
        Action::Scroll {
            xy: [1, 2],
            dx: 0,
            dy: 3,
        }
    );
    // A nudge smaller than one notch still moves, in the right direction.
    assert_eq!(
        map_one(json!({ "type": "scroll", "x": 1, "y": 2, "scroll_y": 40 })),
        Action::Scroll {
            xy: [1, 2],
            dx: 0,
            dy: 1,
        }
    );
    assert_eq!(
        map_one(json!({ "type": "scroll", "x": 1, "y": 2, "scroll_y": -40 })),
        Action::Scroll {
            xy: [1, 2],
            dx: 0,
            dy: -1,
        }
    );
    // Zero stays zero rather than becoming a phantom notch.
    assert_eq!(
        map_one(json!({ "type": "scroll", "x": 1, "y": 2, "scroll_y": 0 })),
        Action::Scroll {
            xy: [1, 2],
            dx: 0,
            dy: 0,
        }
    );
}

/// The guide describes both shapes; the only reference implementation handles
/// only objects. Accepting both costs nothing.
#[test]
fn drag_path_takes_pairs_or_objects() {
    let want = Action::Drag {
        path: vec![[1, 2], [3, 4]],
    };
    assert_eq!(
        map_one(json!({ "type": "drag", "path": [[1, 2], [3, 4]] })),
        want
    );
    assert_eq!(
        map_one(json!({ "type": "drag", "path": [{ "x": 1, "y": 2 }, { "x": 3, "y": 4 }] })),
        want
    );
}

#[test]
fn wait_defaults_when_no_duration_is_given() {
    assert_eq!(
        map_one(json!({ "type": "wait" })),
        Action::Wait { ms: 1000 }
    );
    assert_eq!(
        map_one(json!({ "type": "wait", "ms": 250 })),
        Action::Wait { ms: 250 }
    );
}

#[test]
fn refuses_what_it_cannot_honestly_map() {
    let bad = |v: Value| {
        let calls = calls_of(json!([v]));
        actions_from_calls("s_1", "b_1", &calls, None, None).is_err()
    };
    // Buttons with no protocol equivalent must not quietly become a left click.
    assert!(bad(
        json!({ "type": "click", "button": "wheel", "x": 1, "y": 2 })
    ));
    assert!(bad(
        json!({ "type": "click", "button": "back", "x": 1, "y": 2 })
    ));
    assert!(bad(json!({ "type": "totally_new_verb" })));
    assert!(bad(json!({ "type": "type", "text": "" })));
    assert!(bad(json!({ "type": "keypress", "keys": [] })));
    assert!(bad(json!({ "type": "click", "x": 1 })));
    assert!(bad(json!({ "type": "drag", "path": [[1, 2]] })));
    assert!(
        computer_calls(&json!({ "output": [{ "type": "computer_call", "id": "cu_1" }] })).is_err()
    );
}

#[test]
fn coordinates_scale_from_the_advertised_display_into_frame_space() {
    let calls = calls_of(json!([{ "type": "click", "x": 100, "y": 100 }]));
    let batch =
        actions_from_calls("s_1", "b_1", &calls, Some((640, 400)), Some((1280, 800))).unwrap();
    assert_eq!(
        batch.items[0],
        Action::Click {
            button: Button::Left,
            xy: [200, 200],
            mods: vec![],
        }
    );
}

#[test]
fn one_output_per_call_carrying_a_data_url() {
    let calls = calls_of(json!([{ "type": "screenshot" }]));
    let outs = outputs_from_ack(&calls, &ok_ack(1), &[frame(4, 4, b"PNGDATA")], None);
    assert_eq!(outs.len(), 1);
    assert_eq!(outs[0].kind, "computer_call_output");
    assert_eq!(outs[0].call_id, "call_1");
    assert_eq!(outs[0].output.kind, "computer_screenshot");
    assert!(
        outs[0]
            .output
            .image_url
            .starts_with("data:image/png;base64,"),
        "{}",
        outs[0].output.image_url
    );
    assert!(outs[0].error.is_none());

    let wire = serde_json::to_value(&outs[0]).unwrap();
    assert_eq!(wire["type"], "computer_call_output");
    assert_eq!(wire["call_id"], "call_1");
    assert_eq!(wire["output"]["type"], "computer_screenshot");
    // No error field on the wire: the API documents no such channel.
    assert!(wire.get("error").is_none());
}

/// Actions of one call occupy a contiguous run of ack results, so a failure in
/// the first call must not be blamed on the second.
#[test]
fn failures_are_attributed_to_the_call_that_failed() {
    let calls = computer_calls(&json!({
        "output": [
            { "type": "computer_call", "call_id": "call_a",
              "actions": [{ "type": "move", "x": 1, "y": 1 }, { "type": "screenshot" }] },
            { "type": "computer_call", "call_id": "call_b",
              "actions": [{ "type": "screenshot" }] }
        ]
    }))
    .unwrap();

    let ack = Ack {
        kind: AckKind::Ack,
        id: "b_1".into(),
        results: vec![
            AckResult {
                i: 0,
                ok: true,
                frame: false,
                error: None,
            },
            AckResult {
                i: 1,
                ok: false,
                frame: false,
                error: Some("boom".into()),
            },
            AckResult {
                i: 2,
                ok: true,
                frame: true,
                error: None,
            },
        ],
    };
    let outs = outputs_from_ack(&calls, &ack, &[frame(4, 4, b"B")], None);
    assert_eq!(outs.len(), 2);
    assert_eq!(outs[0].error.as_deref(), Some("boom"));
    assert!(outs[1].error.is_none());
    // Even the failed call answers with a screenshot: that is the only shape
    // the API accepts, and the model needs to see the real screen to recover.
    assert!(outs[0].output.image_url.starts_with("data:"));
}

#[test]
fn pending_safety_checks_are_echoed_back_for_acknowledgement() {
    let calls = computer_calls(&json!({
        "output": [{
            "type": "computer_call",
            "call_id": "call_sc",
            "actions": [{ "type": "screenshot" }],
            "pending_safety_checks": [{
                "id": "cu_sc_1",
                "code": "malicious_instructions",
                "message": "please acknowledge"
            }]
        }]
    }))
    .unwrap();
    let outs = outputs_from_ack(&calls, &ok_ack(1), &[frame(1, 1, b"X")], None);
    assert_eq!(outs[0].acknowledged_safety_checks.len(), 1);
    assert_eq!(outs[0].acknowledged_safety_checks[0].id, "cu_sc_1");

    let wire = serde_json::to_value(&outs[0]).unwrap();
    assert_eq!(wire["acknowledged_safety_checks"][0]["id"], "cu_sc_1");
    assert_eq!(
        wire["acknowledged_safety_checks"][0]["code"],
        "malicious_instructions"
    );
}

#[test]
fn a_call_with_no_frame_falls_back_rather_than_sending_nothing() {
    let calls = calls_of(json!([{ "type": "move", "x": 1, "y": 1 }]));
    let last = frame(2, 2, b"LAST");
    let outs = outputs_from_ack(&calls, &ok_ack(1), &[], Some(&last));
    assert!(
        outs[0]
            .output
            .image_url
            .starts_with("data:image/png;base64,")
    );
}
