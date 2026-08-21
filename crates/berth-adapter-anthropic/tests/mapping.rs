//! Table-driven mapping tests for computer_toolset_20260801.

use std::fs;
use std::path::PathBuf;

use berth_adapter_anthropic::{
    ContentBlock, ToolUse, actions_from_tool_uses, computer_tool_uses, results_from_ack,
};
use berth_protocol::{Ack, AckKind, AckResult, Action, ActionBatchKind, Button, Frame, FrameKind};
use serde_json::{Value, json};

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap()
}

fn load_use(name: &str) -> ToolUse {
    serde_json::from_value(fixture(name)).unwrap()
}

fn map_one(name: &str) -> Action {
    let uses = [load_use(name)];
    let batch = actions_from_tool_uses("s_1", "a_1", &uses, None, None).unwrap();
    assert_eq!(batch.kind, ActionBatchKind::Actions);
    assert_eq!(batch.session_id, "s_1");
    assert_eq!(batch.items.len(), 1);
    batch.items.into_iter().next().unwrap()
}

#[test]
fn fixture_click_type_screenshot_scroll_key() {
    match map_one("click.json") {
        Action::Click { button, xy, mods } => {
            assert_eq!(button, Button::Left);
            assert_eq!(xy, [100, 200]);
            assert!(mods.is_empty());
        }
        other => panic!("{other:?}"),
    }
    match map_one("type.json") {
        Action::Type { text } => assert_eq!(text, "hello"),
        other => panic!("{other:?}"),
    }
    match map_one("screenshot.json") {
        Action::Screenshot {} => {}
        other => panic!("{other:?}"),
    }
    match map_one("scroll.json") {
        Action::Scroll { xy, dx, dy } => {
            assert_eq!(xy, [100, 200]);
            assert_eq!(dx, 0);
            assert_eq!(dy, 3);
        }
        other => panic!("{other:?}"),
    }
    match map_one("key.json") {
        Action::Key { keys, repeat } => {
            assert_eq!(keys, ["ctrl", "s"]);
            assert_eq!(repeat, 1);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn name_computer_maps_input_action() {
    match map_one("computer_named.json") {
        Action::Click { button, xy, mods } => {
            assert_eq!(button, Button::Left);
            assert_eq!(xy, [50, 60]);
            assert!(mods.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn table_member_mapping() {
    let cases = [
        r#"{"id":"1","name":"right_click","toolset_name":"computer","input":{"coordinate":[1,2]}}"#,
        r#"{"id":"2","name":"middle_click","toolset_name":"computer","input":{"coordinate":[3,4]}}"#,
        r#"{"id":"3","name":"double_click","toolset_name":"computer","input":{"coordinate":[8,9]}}"#,
        r#"{"id":"4","name":"scroll","toolset_name":"computer","input":{"coordinate":[10,20],"scroll_direction":"up","scroll_amount":2}}"#,
        r#"{"id":"5","name":"key","toolset_name":"computer","input":{"text":"Return","repeat":4}}"#,
        r#"{"id":"6","name":"left_click","toolset_name":"computer","input":{"coordinate":[5,6],"text":"ctrl+shift"}}"#,
    ];
    let mapped: Vec<Action> = cases
        .iter()
        .map(|raw| {
            let use_block: ToolUse = serde_json::from_str(raw).unwrap();
            actions_from_tool_uses("s", "a", &[use_block], None, None)
                .unwrap()
                .items
                .remove(0)
        })
        .collect();
    match &mapped[0] {
        Action::Click { button, xy, .. } => {
            assert_eq!(*button, Button::Right);
            assert_eq!(*xy, [1, 2]);
        }
        other => panic!("{other:?}"),
    }
    match &mapped[1] {
        Action::Click { button, xy, .. } => {
            assert_eq!(*button, Button::Middle);
            assert_eq!(*xy, [3, 4]);
        }
        other => panic!("{other:?}"),
    }
    match &mapped[2] {
        Action::DoubleClick { xy, button } => {
            assert_eq!(*xy, [8, 9]);
            assert_eq!(*button, Button::Left);
        }
        other => panic!("{other:?}"),
    }
    match &mapped[3] {
        Action::Scroll { xy, dx, dy } => {
            assert_eq!(*xy, [10, 20]);
            assert_eq!(*dx, 0);
            assert_eq!(*dy, -2);
        }
        other => panic!("{other:?}"),
    }
    match &mapped[4] {
        Action::Key { keys, repeat } => {
            assert_eq!(keys, &["Return"]);
            assert_eq!(*repeat, 4);
        }
        other => panic!("{other:?}"),
    }
    match &mapped[5] {
        Action::Click { mods, .. } => assert_eq!(mods, &["ctrl", "shift"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn skips_non_computer_tool_uses() {
    let content = json!([
        { "type": "text", "text": "hi" },
        {
            "type": "tool_use",
            "id": "browser_shot",
            "name": "screenshot",
            "toolset_name": "browser",
            "input": {}
        },
        {
            "type": "tool_use",
            "id": "toolu_shot",
            "name": "screenshot",
            "toolset_name": "computer",
            "input": {}
        }
    ]);
    let uses = computer_tool_uses(&content).unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].id, "toolu_shot");
    let batch = actions_from_tool_uses("s_1", "a_1", &uses, None, None).unwrap();
    assert_eq!(batch.items, vec![Action::Screenshot {}]);
}

#[test]
fn scales_when_tool_use_size_differs_from_last_frame() {
    let use_block = ToolUse {
        r#type: Some("tool_use".into()),
        id: "toolu_click".into(),
        name: "left_click".into(),
        toolset_name: Some("computer".into()),
        input: json!({
            "coordinate": [1000, 400],
            "display_width_px": 2000,
            "display_height_px": 800
        }),
    };
    let batch =
        actions_from_tool_uses("s_1", "a_1", &[use_block], Some((1000, 800)), None).unwrap();
    match &batch.items[0] {
        Action::Click { xy, .. } => assert_eq!(*xy, [500, 400]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pass_through_when_frame_matches_or_missing() {
    let use_block = ToolUse {
        r#type: Some("tool_use".into()),
        id: "toolu_click".into(),
        name: "left_click".into(),
        toolset_name: Some("computer".into()),
        input: json!({
            "coordinate": [100, 200],
            "display_width_px": 1280,
            "display_height_px": 800
        }),
    };
    let same = actions_from_tool_uses(
        "s",
        "a",
        std::slice::from_ref(&use_block),
        Some((1280, 800)),
        None,
    )
    .unwrap();
    match &same.items[0] {
        Action::Click { xy, .. } => assert_eq!(*xy, [100, 200]),
        other => panic!("{other:?}"),
    }
    let none = actions_from_tool_uses("s", "a", &[use_block], None, None).unwrap();
    match &none.items[0] {
        Action::Click { xy, .. } => assert_eq!(*xy, [100, 200]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn empty_type_and_missing_coordinate_are_errors() {
    let empty_type = ToolUse {
        r#type: Some("tool_use".into()),
        id: "t".into(),
        name: "type".into(),
        toolset_name: Some("computer".into()),
        input: json!({ "text": "" }),
    };
    assert!(actions_from_tool_uses("s", "a", &[empty_type], None, None).is_err());

    let no_xy = ToolUse {
        r#type: Some("tool_use".into()),
        id: "c".into(),
        name: "left_click".into(),
        toolset_name: Some("computer".into()),
        input: json!({}),
    };
    assert!(actions_from_tool_uses("s", "a", &[no_xy], None, None).is_err());
}

#[test]
fn omitted_click_and_scroll_use_last_cursor() {
    let click = ToolUse {
        r#type: Some("tool_use".into()),
        id: "c".into(),
        name: "left_click".into(),
        toolset_name: Some("computer".into()),
        input: json!({}),
    };
    let batch = actions_from_tool_uses("s", "a", &[click], None, Some([512, 384])).unwrap();
    match &batch.items[0] {
        Action::Click { xy, button, .. } => {
            assert_eq!(*xy, [512, 384]);
            assert_eq!(*button, Button::Left);
        }
        other => panic!("{other:?}"),
    }

    let scroll = ToolUse {
        r#type: Some("tool_use".into()),
        id: "s".into(),
        name: "scroll".into(),
        toolset_name: Some("computer".into()),
        input: json!({ "scroll_direction": "down", "scroll_amount": 3 }),
    };
    let batch = actions_from_tool_uses("s", "a", &[scroll], None, Some([10, 20])).unwrap();
    match &batch.items[0] {
        Action::Scroll { xy, dx, dy } => {
            assert_eq!(*xy, [10, 20]);
            assert_eq!(*dx, 0);
            assert_eq!(*dy, 3);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn unimplemented_members_error() {
    for name in ["triple_click", "left_mouse_down", "nope"] {
        let use_block = ToolUse {
            r#type: Some("tool_use".into()),
            id: "x".into(),
            name: name.into(),
            toolset_name: Some("computer".into()),
            input: json!({}),
        };
        let err = actions_from_tool_uses("s", "a", &[use_block], None, None).unwrap_err();
        assert!(err.to_string().contains(name), "{err}");
    }
}

fn sample_frame() -> Frame {
    Frame {
        kind: FrameKind::Frame,
        session_id: "s_1".into(),
        ts: 0,
        width: 1280,
        height: 800,
        mime: "image/png".into(),
        data: b"\x89PNG".to_vec(),
        cursor: None,
    }
}

#[test]
fn screenshot_result_is_image_others_ok() {
    let shot = load_use("screenshot.json");
    let click = load_use("click.json");
    let ack = Ack {
        kind: AckKind::Ack,
        id: "a_1".into(),
        results: vec![
            AckResult {
                i: 0,
                ok: true,
                frame: true,
                error: None,
            },
            AckResult {
                i: 1,
                ok: true,
                frame: false,
                error: None,
            },
        ],
    };
    let results = results_from_ack(&[shot, click], &ack, &[sample_frame()], None);
    assert_eq!(results.len(), 2);
    assert!(!results[0].is_error);
    match &results[0].content[0] {
        ContentBlock::Image { source } => {
            assert_eq!(source.kind, "base64");
            assert_eq!(source.media_type, "image/png");
            assert_eq!(source.data, "iVBORw==");
        }
        other => panic!("{other:?}"),
    }
    assert!(!results[1].is_error);
    match &results[1].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "OK"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn failed_ack_is_error_not_ok() {
    let click = load_use("click.json");
    let shot = load_use("screenshot.json");
    let ack = Ack {
        kind: AckKind::Ack,
        id: "a_1".into(),
        results: vec![
            AckResult {
                i: 0,
                ok: false,
                frame: false,
                error: Some("denied".into()),
            },
            AckResult {
                i: 1,
                ok: false,
                frame: false,
                error: Some("skipped".into()),
            },
        ],
    };
    let results = results_from_ack(&[click, shot], &ack, &[], None);
    assert!(results[0].is_error);
    match &results[0].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "denied"),
        other => panic!("{other:?}"),
    }
    assert!(results[1].is_error);
    match &results[1].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "skipped"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn screenshot_without_frame_is_error() {
    let shot = load_use("screenshot.json");
    let ack = Ack {
        kind: AckKind::Ack,
        id: "a_1".into(),
        results: vec![AckResult {
            i: 0,
            ok: true,
            frame: true,
            error: None,
        }],
    };
    let results = results_from_ack(&[shot], &ack, &[], None);
    assert!(results[0].is_error);
    match &results[0].content[0] {
        ContentBlock::Text { text } => assert!(text.contains("no frame")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn tool_result_json_shape() {
    let shot = load_use("screenshot.json");
    let ack = Ack {
        kind: AckKind::Ack,
        id: "a_1".into(),
        results: vec![AckResult {
            i: 0,
            ok: true,
            frame: true,
            error: None,
        }],
    };
    let results = results_from_ack(&[shot], &ack, &[sample_frame()], None);
    let json = serde_json::to_value(&results[0]).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["tool_use_id"], "toolu_shot");
    assert_eq!(json["toolset_name"], "computer");
    assert!(json.get("is_error").is_none());
    assert_eq!(json["content"][0]["type"], "image");
    assert_eq!(json["content"][0]["source"]["type"], "base64");
}

#[test]
fn cursor_position_result_is_xy_text() {
    let use_block = ToolUse {
        r#type: Some("tool_use".into()),
        id: "toolu_cur".into(),
        name: "cursor_position".into(),
        toolset_name: Some("computer".into()),
        input: json!({}),
    };
    let ack = Ack {
        kind: AckKind::Ack,
        id: "a_1".into(),
        results: vec![AckResult {
            i: 0,
            ok: true,
            frame: false,
            error: None,
        }],
    };
    let from_arg = results_from_ack(
        std::slice::from_ref(&use_block),
        &ack,
        &[],
        Some([512, 384]),
    );
    match &from_arg[0].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "X=512, Y=384"),
        other => panic!("{other:?}"),
    }

    let mut frame = sample_frame();
    frame.cursor = Some([1, 2]);
    let from_frame = results_from_ack(std::slice::from_ref(&use_block), &ack, &[frame], None);
    match &from_frame[0].content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "X=1, Y=2"),
        other => panic!("{other:?}"),
    }

    let missing = results_from_ack(std::slice::from_ref(&use_block), &ack, &[], None);
    assert!(missing[0].is_error);
    match &missing[0].content[0] {
        ContentBlock::Text { text } => assert!(text.contains("unknown")),
        other => panic!("{other:?}"),
    }
}
