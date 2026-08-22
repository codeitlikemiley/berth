#[cfg(test)]
use berthos_protocol::{Ack, AckKind};
use berthos_protocol::{AckResult, Action, Button};

/// The image ships `driver -> action.sh`, and MVP.md calls that symlink the
/// seam a different driver swaps in behind. Calling action.sh directly meant
/// the seam did not actually exist; repointing the symlink changed nothing.
pub const ACTION_BIN: &str = "/usr/local/bin/driver";
pub const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
pub const FRAME_WIDTH: u32 = 1280;
pub const FRAME_HEIGHT: u32 = 800;

/// Map a protocol action onto guest driver argv (including the binary).
///
/// Ops the guest driver does not implement fail here so the batch can skip the rest.
pub fn action_argv(action: &Action) -> std::result::Result<Vec<String>, String> {
    match action {
        Action::Screenshot {} => Ok(vec![ACTION_BIN.to_string(), "screenshot".into()]),
        Action::Click { button, xy, mods } => {
            let mut argv = vec![
                ACTION_BIN.to_string(),
                "click".into(),
                xy[0].to_string(),
                xy[1].to_string(),
                button_arg(*button).into(),
            ];
            argv.extend(mods.iter().cloned());
            Ok(argv)
        }
        Action::DoubleClick { xy, button } => Ok(vec![
            ACTION_BIN.to_string(),
            "click".into(),
            xy[0].to_string(),
            xy[1].to_string(),
            match button {
                Button::Left => "double_left",
                Button::Middle => "double_middle",
                Button::Right => "double_right",
            }
            .into(),
        ]),
        Action::Move { xy } => Ok(vec![
            ACTION_BIN.to_string(),
            "move".into(),
            xy[0].to_string(),
            xy[1].to_string(),
        ]),
        Action::Scroll { xy, dx, dy } => Ok(vec![
            ACTION_BIN.to_string(),
            "scroll".into(),
            xy[0].to_string(),
            xy[1].to_string(),
            dx.to_string(),
            dy.to_string(),
        ]),
        Action::Type { text } => Ok(vec![ACTION_BIN.to_string(), "type".into(), text.clone()]),
        Action::Key { keys, repeat } => {
            if keys.is_empty() {
                return Err("key requires KEY".into());
            }
            if *repeat == 0 {
                return Err("key repeat must be > 0".into());
            }
            let mut argv = vec![ACTION_BIN.to_string(), "key".into()];
            argv.extend(keys.iter().cloned());
            Ok(argv)
        }
        Action::Wait { ms } => Ok(vec![ACTION_BIN.to_string(), "wait".into(), ms.to_string()]),
        Action::Drag { path } => {
            if path.len() < 2 {
                return Err("drag requires at least two points".into());
            }
            let mut argv = vec![ACTION_BIN.to_string(), "drag".into()];
            for p in path {
                argv.push(p[0].to_string());
                argv.push(p[1].to_string());
            }
            Ok(argv)
        }
        Action::HoldKey { keys, ms } => {
            if keys.is_empty() {
                return Err("hold_key requires KEY".into());
            }
            let mut argv = vec![ACTION_BIN.to_string(), "hold_key".into(), ms.to_string()];
            argv.extend(keys.iter().cloned());
            Ok(argv)
        }
        Action::Zoom { region } => {
            // Region is [x, y, x2, y2]; see berthos_protocol::Region.
            if region[2] <= region[0] || region[3] <= region[1] {
                return Err("zoom region must have positive width and height".into());
            }
            Ok(vec![
                ACTION_BIN.to_string(),
                "zoom".into(),
                region[0].to_string(),
                region[1].to_string(),
                region[2].to_string(),
                region[3].to_string(),
            ])
        }
        Action::CursorPosition {} => Ok(vec![ACTION_BIN.to_string(), "cursor_position".into()]),
        // Deliberate, not missing. Arbitrary shell in the guest is a different
        // security decision from driving its desktop, and the protocol has no
        // carrier for command output anyway.
        Action::Shell { .. } => Err("shell is not exposed by the guest driver".into()),
    }
}

pub fn key_repeats(action: &Action) -> u32 {
    match action {
        Action::Key { repeat, .. } => *repeat,
        _ => 1,
    }
}

/// Plan a batch using only argv mapping: first mapping error skips the rest.
/// Used by unit tests; `Session::exec` applies the same skip rule around docker exec.
#[cfg(test)]
pub(crate) fn argv_batch(id: &str, items: &[Action]) -> Ack {
    let mut results = Vec::with_capacity(items.len());
    let mut skip = false;
    for (i, item) in items.iter().enumerate() {
        let i = i as u32;
        if skip {
            results.push(skipped(i));
            continue;
        }
        match action_argv(item) {
            Ok(_) => results.push(AckResult {
                i,
                ok: true,
                frame: matches!(item, Action::Screenshot {}),
                error: None,
            }),
            Err(error) => {
                results.push(AckResult {
                    i,
                    ok: false,
                    frame: false,
                    error: Some(error),
                });
                skip = true;
            }
        }
    }
    Ack {
        kind: AckKind::Ack,
        id: id.to_string(),
        results,
    }
}

pub(crate) fn skipped(i: u32) -> AckResult {
    AckResult {
        i,
        ok: false,
        frame: false,
        error: Some("skipped".into()),
    }
}

fn button_arg(button: Button) -> &'static str {
    match button {
        Button::Left => "left",
        Button::Right => "right",
        Button::Middle => "middle",
    }
}

pub fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || !data.starts_with(PNG_MAGIC) {
        return None;
    }
    if &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use berthos_protocol::Button;

    #[test]
    fn maps_supported_ops() {
        assert_eq!(
            action_argv(&Action::Screenshot {}).unwrap(),
            [ACTION_BIN, "screenshot"]
        );
        assert_eq!(
            action_argv(&Action::Click {
                button: Button::Right,
                xy: [10, 20],
                mods: vec![],
            })
            .unwrap(),
            [ACTION_BIN, "click", "10", "20", "right"]
        );
        assert_eq!(
            action_argv(&Action::DoubleClick {
                xy: [1, 2],
                button: Button::Left,
            })
            .unwrap(),
            [ACTION_BIN, "click", "1", "2", "double_left"]
        );
        assert_eq!(
            action_argv(&Action::Move { xy: [3, 4] }).unwrap(),
            [ACTION_BIN, "move", "3", "4"]
        );
        assert_eq!(
            action_argv(&Action::Scroll {
                xy: [5, 6],
                dx: -1,
                dy: 3,
            })
            .unwrap(),
            [ACTION_BIN, "scroll", "5", "6", "-1", "3"]
        );
        assert_eq!(
            action_argv(&Action::Type {
                text: "hello world".into(),
            })
            .unwrap(),
            [ACTION_BIN, "type", "hello world"]
        );
        assert_eq!(
            action_argv(&Action::Key {
                keys: vec!["META".into(), "s".into()],
                repeat: 2,
            })
            .unwrap(),
            [ACTION_BIN, "key", "META", "s"]
        );
        assert_eq!(
            key_repeats(&Action::Key {
                keys: vec!["Return".into()],
                repeat: 3,
            }),
            3
        );
        assert_eq!(
            action_argv(&Action::Wait { ms: 400 }).unwrap(),
            [ACTION_BIN, "wait", "400"]
        );
    }

    /// Anthropic's computer-use toolset issues drag, hold_key and
    /// cursor_position; every one of them used to fail here and abort the rest
    /// of the batch.
    #[test]
    fn maps_the_ops_that_used_to_be_refused() {
        assert_eq!(
            action_argv(&Action::Drag {
                path: vec![[0, 0], [5, 6], [7, 8]],
            })
            .unwrap(),
            [ACTION_BIN, "drag", "0", "0", "5", "6", "7", "8"]
        );
        assert_eq!(
            action_argv(&Action::HoldKey {
                keys: vec!["SHIFT".into(), "a".into()],
                ms: 250,
            })
            .unwrap(),
            [ACTION_BIN, "hold_key", "250", "SHIFT", "a"]
        );
        assert_eq!(
            action_argv(&Action::Zoom {
                region: [10, 20, 110, 220],
            })
            .unwrap(),
            [ACTION_BIN, "zoom", "10", "20", "110", "220"]
        );
        assert_eq!(
            action_argv(&Action::CursorPosition {}).unwrap(),
            [ACTION_BIN, "cursor_position"]
        );
        // Modifiers ride after the button rather than being refused.
        assert_eq!(
            action_argv(&Action::Click {
                button: Button::Left,
                xy: [0, 0],
                mods: vec!["SHIFT".into(), "ctrl".into()],
            })
            .unwrap(),
            [ACTION_BIN, "click", "0", "0", "left", "SHIFT", "ctrl"]
        );
        for (button, arg) in [
            (Button::Right, "double_right"),
            (Button::Middle, "double_middle"),
        ] {
            assert_eq!(
                action_argv(&Action::DoubleClick { xy: [1, 2], button }).unwrap(),
                [ACTION_BIN, "click", "1", "2", arg]
            );
        }
    }

    #[test]
    fn rejects_what_the_driver_really_cannot_do() {
        // Deliberate policy, not a gap: arbitrary shell in the guest is a
        // separate security decision from driving its desktop.
        assert!(
            action_argv(&Action::Shell {
                cmd: "uname".into(),
            })
            .unwrap_err()
            .contains("shell is not exposed")
        );
        assert!(
            action_argv(&Action::Drag { path: vec![[0, 0]] })
                .unwrap_err()
                .contains("two points")
        );
        assert!(
            action_argv(&Action::HoldKey {
                keys: vec![],
                ms: 10,
            })
            .is_err()
        );
        // A zero-or-negative region would crop to nothing.
        assert!(
            action_argv(&Action::Zoom {
                region: [10, 10, 10, 20],
            })
            .unwrap_err()
            .contains("positive")
        );
        assert!(
            action_argv(&Action::Key {
                keys: vec![],
                repeat: 1,
            })
            .is_err()
        );
        assert!(
            action_argv(&Action::Key {
                keys: vec!["a".into()],
                repeat: 0,
            })
            .unwrap_err()
            .contains("repeat")
        );
    }

    #[test]
    fn first_argv_error_skips_rest() {
        let ack = argv_batch(
            "a_skip",
            &[
                Action::Wait { ms: 1 },
                Action::Shell {
                    cmd: "uname -a".into(),
                },
                Action::Wait { ms: 2 },
                Action::Screenshot {},
            ],
        );
        assert!(ack.results[0].ok);
        assert!(!ack.results[1].ok);
        assert_eq!(ack.results[2].error.as_deref(), Some("skipped"));
        assert_eq!(ack.results[3].error.as_deref(), Some("skipped"));
        assert!(!ack.results[2].ok);
    }

    #[test]
    fn png_ihdr() {
        let mut data = vec![0_u8; 24];
        data[..8].copy_from_slice(PNG_MAGIC);
        data[12..16].copy_from_slice(b"IHDR");
        data[16..20].copy_from_slice(&1280_u32.to_be_bytes());
        data[20..24].copy_from_slice(&800_u32.to_be_bytes());
        assert_eq!(png_dimensions(&data), Some((1280, 800)));
        assert_eq!(png_dimensions(b"not png"), None);
        let mut no_ihdr = vec![0_u8; 24];
        no_ihdr[..8].copy_from_slice(PNG_MAGIC);
        assert_eq!(png_dimensions(&no_ihdr), None);
    }
}
