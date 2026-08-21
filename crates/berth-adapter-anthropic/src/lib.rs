//! Map Anthropic `computer_toolset_20260801` tool_use blocks onto protocol
//! actions, and Ack+Frame onto tool_result blocks.
//!
//! This crate does not call the network.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use berth_protocol::{Ack, Action, ActionBatch, ActionBatchKind, Button, Frame, Point, Region};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(msg) | Self::Unsupported(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Anthropic `tool_use` block. Member tools set `toolset_name=computer`;
/// the earlier single-tool shape uses `name=computer` and `input.action`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUse {
    #[serde(default)]
    pub r#type: Option<String>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub toolset_name: Option<String>,
    #[serde(default)]
    pub input: Value,
}

impl ToolUse {
    #[must_use]
    pub fn is_computer(&self) -> bool {
        self.toolset_name.as_deref() == Some("computer") || self.name == "computer"
    }

    fn member(&self) -> Result<&str> {
        if self.name == "computer" {
            self.input
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("computer tool_use is missing input.action".into()))
        } else {
            Ok(self.name.as_str())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolResultKind {
    #[serde(rename = "tool_result")]
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub kind: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(rename = "type")]
    pub kind: ToolResultKind,
    pub tool_use_id: String,
    pub toolset_name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
    pub content: Vec<ContentBlock>,
}

impl ToolResult {
    fn ok_text(tool_use_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: ToolResultKind::ToolResult,
            tool_use_id: tool_use_id.into(),
            toolset_name: "computer".into(),
            is_error: false,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    fn err_text(tool_use_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: ToolResultKind::ToolResult,
            tool_use_id: tool_use_id.into(),
            toolset_name: "computer".into(),
            is_error: true,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    fn image(
        tool_use_id: impl Into<String>,
        mime: impl Into<String>,
        data: impl Into<String>,
    ) -> Self {
        Self {
            kind: ToolResultKind::ToolResult,
            tool_use_id: tool_use_id.into(),
            toolset_name: "computer".into(),
            is_error: false,
            content: vec![ContentBlock::Image {
                source: ImageSource {
                    kind: "base64".into(),
                    media_type: mime.into(),
                    data: data.into(),
                },
            }],
        }
    }
}

/// Collect computer `tool_use` blocks from a Messages `content` array (or a single block).
pub fn computer_tool_uses(value: &Value) -> Result<Vec<ToolUse>> {
    let blocks = match value {
        Value::Array(items) => items.clone(),
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("tool_use") => {
            vec![value.clone()]
        }
        Value::Object(map) => match map.get("content") {
            Some(Value::Array(items)) => items.clone(),
            _ => {
                return Err(Error::Invalid(
                    "expected tool_use block, content array, or message with content".into(),
                ));
            }
        },
        _ => {
            return Err(Error::Invalid(
                "expected tool_use block, content array, or message with content".into(),
            ));
        }
    };
    let mut out = Vec::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let use_block: ToolUse = serde_json::from_value(block)
            .map_err(|err| Error::Invalid(format!("tool_use: {err}")))?;
        if use_block.is_computer() {
            out.push(use_block);
        }
    }
    Ok(out)
}

/// Map computer tool_use blocks to a protocol ActionBatch.
///
/// Coordinates pass through last-frame pixel space. If a block includes
/// `display_width_px`/`display_height_px` (or `width`/`height`) that differ
/// from `last_frame`, coordinates are scaled with [`ActionBatch::scale_coordinates`].
pub fn actions_from_tool_uses(
    session_id: impl Into<String>,
    batch_id: impl Into<String>,
    uses: &[ToolUse],
    last_frame: Option<(u32, u32)>,
) -> Result<ActionBatch> {
    let computer: Vec<&ToolUse> = uses.iter().filter(|u| u.is_computer()).collect();
    let mut items = Vec::with_capacity(computer.len());
    for use_block in &computer {
        items.push(action_from_use(use_block, last_frame)?);
    }
    Ok(ActionBatch {
        kind: ActionBatchKind::Actions,
        id: batch_id.into(),
        session_id: session_id.into(),
        items,
    })
}

/// Map Ack + Frames onto one tool_result per computer tool_use.
///
/// Screenshot/zoom become an image block; other successful actions become `OK`.
/// Failed ack results become `is_error` with the error text. This does not
/// invent a successful result when `ok` is false.
pub fn results_from_ack(uses: &[ToolUse], ack: &Ack, frames: &[Frame]) -> Vec<ToolResult> {
    let computer: Vec<&ToolUse> = uses.iter().filter(|u| u.is_computer()).collect();
    let mut frame_i = 0usize;
    let mut out = Vec::with_capacity(computer.len());
    for (i, use_block) in computer.iter().enumerate() {
        let result = ack.results.get(i);
        let ok = result.map(|r| r.ok).unwrap_or(false);
        if !ok {
            let text = result
                .and_then(|r| r.error.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "action failed".into());
            out.push(ToolResult::err_text(&use_block.id, text));
            continue;
        }
        match use_block.member() {
            Ok("screenshot" | "zoom") => match frames.get(frame_i) {
                Some(frame) => {
                    frame_i += 1;
                    let mime = if frame.mime.is_empty() {
                        "image/png".to_string()
                    } else {
                        frame.mime.clone()
                    };
                    out.push(ToolResult::image(
                        &use_block.id,
                        mime,
                        STANDARD.encode(&frame.data),
                    ));
                }
                None => {
                    out.push(ToolResult::err_text(
                        &use_block.id,
                        "screenshot produced no frame",
                    ));
                }
            },
            Ok(_) | Err(_) => {
                if result.map(|r| r.frame).unwrap_or(false) {
                    frame_i += 1;
                }
                out.push(ToolResult::ok_text(&use_block.id, "OK"));
            }
        }
    }
    out
}

fn action_from_use(use_block: &ToolUse, last_frame: Option<(u32, u32)>) -> Result<Action> {
    let member = use_block.member()?;
    let mut action = action_from_member(member, &use_block.input)?;
    if let (Some((from_w, from_h)), Some((to_w, to_h))) =
        (display_size(&use_block.input), last_frame)
    {
        action.scale_coordinates(from_w, from_h, to_w, to_h);
    }
    Ok(action)
}

fn action_from_member(member: &str, input: &Value) -> Result<Action> {
    match member {
        "screenshot" => Ok(Action::Screenshot {}),
        "left_click" => click(input, Button::Left),
        "right_click" => click(input, Button::Right),
        "middle_click" => click(input, Button::Middle),
        "double_click" => Ok(Action::DoubleClick {
            xy: point(input, "coordinate")?,
            button: Button::Left,
        }),
        "left_click_drag" => Ok(Action::Drag {
            path: vec![
                point(input, "start_coordinate")?,
                point(input, "coordinate")?,
            ],
        }),
        "mouse_move" => Ok(Action::Move {
            xy: point(input, "coordinate")?,
        }),
        "scroll" => scroll(input),
        "type" => {
            let text = text_field(input, "text")?;
            if text.is_empty() {
                return Err(Error::Invalid("type text is empty".into()));
            }
            Ok(Action::Type { text })
        }
        "key" => key_action(input),
        "hold_key" => {
            let keys = keys_from_input(input)?;
            let ms = duration_ms(input)?;
            Ok(Action::HoldKey { keys, ms })
        }
        "wait" => Ok(Action::Wait {
            ms: duration_ms(input)?,
        }),
        "zoom" => Ok(Action::Zoom {
            region: region(input, "region")?,
        }),
        "cursor_position" => Ok(Action::CursorPosition {}),
        "triple_click" | "left_mouse_down" | "left_mouse_up" => Err(Error::Unsupported(format!(
            "computer member `{member}` is not mapped in the protocol MVP"
        ))),
        other => Err(Error::Unsupported(format!(
            "unknown computer member `{other}`"
        ))),
    }
}

fn click(input: &Value, button: Button) -> Result<Action> {
    Ok(Action::Click {
        button,
        xy: point(input, "coordinate")?,
        mods: mods_from_text(input),
    })
}

fn scroll(input: &Value) -> Result<Action> {
    let xy = point(input, "coordinate")?;
    let amount = match input.get("scroll_amount") {
        None => 1,
        Some(v) => i32_number(v, "scroll_amount")?,
    };
    let (dx, dy) = match input.get("scroll_direction").and_then(Value::as_str) {
        Some("down") => (0, amount),
        Some("up") => (0, -amount),
        Some("right") => (amount, 0),
        Some("left") => (-amount, 0),
        Some(other) => {
            return Err(Error::Invalid(format!(
                "unknown scroll_direction `{other}`"
            )));
        }
        None => {
            let dx = match input.get("dx") {
                None => 0,
                Some(v) => i32_number(v, "dx")?,
            };
            let dy = match input.get("dy") {
                Some(v) => i32_number(v, "dy")?,
                None => {
                    return Err(Error::Invalid(
                        "scroll requires scroll_direction or dy".into(),
                    ));
                }
            };
            (dx, dy)
        }
    };
    Ok(Action::Scroll { xy, dx, dy })
}

fn key_action(input: &Value) -> Result<Action> {
    let keys = keys_from_input(input)?;
    let repeat = match input.get("repeat") {
        None => 1,
        Some(v) => {
            let n = u32_number(v, "repeat")?;
            if n == 0 {
                return Err(Error::Invalid("key repeat must be >= 1".into()));
            }
            n
        }
    };
    Ok(Action::Key { keys, repeat })
}

fn keys_from_input(input: &Value) -> Result<Vec<String>> {
    let keys = if let Some(text) = input.get("text").and_then(Value::as_str) {
        split_keys(text)
    } else if let Some(arr) = input.get("keys").and_then(Value::as_array) {
        arr.iter()
            .filter_map(Value::as_str)
            .flat_map(split_keys)
            .collect()
    } else if let Some(text) = input.get("keys").and_then(Value::as_str) {
        split_keys(text)
    } else {
        return Err(Error::Invalid("key requires text or keys".into()));
    };
    if keys.is_empty() {
        return Err(Error::Invalid("key keys is empty".into()));
    }
    Ok(keys)
}

fn split_keys(text: &str) -> Vec<String> {
    text.split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn mods_from_text(input: &Value) -> Vec<String> {
    input
        .get("text")
        .and_then(Value::as_str)
        .map(split_keys)
        .unwrap_or_default()
}

fn display_size(input: &Value) -> Option<(u32, u32)> {
    let w = input
        .get("display_width_px")
        .or_else(|| input.get("width"))
        .and_then(as_u32)?;
    let h = input
        .get("display_height_px")
        .or_else(|| input.get("height"))
        .and_then(as_u32)?;
    Some((w, h))
}

fn as_u32(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok();
    }
    let f = value.as_f64()?;
    if !f.is_finite() || f < 0.0 || f > f64::from(u32::MAX) {
        return None;
    }
    Some(f as u32)
}

fn point(input: &Value, key: &str) -> Result<Point> {
    let arr = input
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Invalid(format!("{key} is required as [x, y]")))?;
    if arr.len() != 2 {
        return Err(Error::Invalid(format!("{key} is required as [x, y]")));
    }
    Ok([i32_number(&arr[0], key)?, i32_number(&arr[1], key)?])
}

fn region(input: &Value, key: &str) -> Result<Region> {
    let arr = input
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Invalid(format!("{key} is required as [x, y, x2, y2]")))?;
    if arr.len() != 4 {
        return Err(Error::Invalid(format!(
            "{key} is required as [x, y, x2, y2]"
        )));
    }
    Ok([
        i32_number(&arr[0], key)?,
        i32_number(&arr[1], key)?,
        i32_number(&arr[2], key)?,
        i32_number(&arr[3], key)?,
    ])
}

fn text_field(input: &Value, key: &str) -> Result<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid(format!("{key} is required")))
}

fn duration_ms(input: &Value) -> Result<u64> {
    let value = input
        .get("duration")
        .or_else(|| input.get("ms"))
        .ok_or_else(|| Error::Invalid("duration is required".into()))?;
    if let Some(ms) = input.get("ms").and_then(Value::as_u64)
        && input.get("duration").is_none()
    {
        return Ok(ms);
    }
    if let Some(n) = value.as_f64() {
        if !n.is_finite() || n < 0.0 {
            return Err(Error::Invalid(
                "duration must be a non-negative number".into(),
            ));
        }
        return Ok((n * 1000.0).round() as u64);
    }
    Err(Error::Invalid(
        "duration must be a number of seconds".into(),
    ))
}

fn i32_number(value: &Value, field: &str) -> Result<i32> {
    if let Some(n) = value.as_i64() {
        return i32::try_from(n).map_err(|_| Error::Invalid(format!("{field} is out of range")));
    }
    if let Some(n) = value.as_u64() {
        return i32::try_from(n).map_err(|_| Error::Invalid(format!("{field} is out of range")));
    }
    if let Some(f) = value.as_f64()
        && f.is_finite()
        && f >= f64::from(i32::MIN)
        && f <= f64::from(i32::MAX)
        && f.fract() == 0.0
    {
        return Ok(f as i32);
    }
    Err(Error::Invalid(format!("{field} must be an integer")))
}

fn u32_number(value: &Value, field: &str) -> Result<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).map_err(|_| Error::Invalid(format!("{field} is out of range")));
    }
    if let Some(f) = value.as_f64()
        && f.is_finite()
        && f >= 0.0
        && f <= f64::from(u32::MAX)
        && f.fract() == 0.0
    {
        return Ok(f as u32);
    }
    Err(Error::Invalid(format!("{field} must be an integer")))
}
