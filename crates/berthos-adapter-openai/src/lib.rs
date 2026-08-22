//! Map OpenAI Responses `computer_call` items onto protocol actions, and
//! Ack+Frame back onto `computer_call_output` items.
//!
//! This crate does not call the network.
//!
//! Shape differs from the Anthropic adapter in one structural way: a single
//! `computer_call` carries an `actions` array, so one call maps to several
//! protocol actions but still gets exactly one output.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use berthos_protocol::{Ack, Action, ActionBatch, ActionBatchKind, Button, Frame, Point};
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

/// OpenAI's wheel deltas are pixels; the protocol's are wheel notches, because
/// the guest replays them as Button4/5 clicks. Chrome and Playwright treat a
/// notch as roughly this many pixels. Anything non-zero moves at least one
/// notch, so a small nudge is never silently rounded away to nothing.
const PIXELS_PER_NOTCH: i32 = 100;

/// A safety check the model wants acknowledged before it will continue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyCheck {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// One `computer_call` output item from a Responses reply.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputerCall {
    /// The item id (`cu_...`). Not what you reference in the reply.
    pub id: Option<String>,
    /// The id the reply must echo (`call_...`).
    pub call_id: String,
    /// Raw action objects, in order. The GA shape batches them; the older
    /// single-`action` shape is normalised into a one-element vec.
    pub actions: Vec<Value>,
    pub pending_safety_checks: Vec<SafetyCheck>,
}

/// The image half of a `computer_call_output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotOutput {
    #[serde(rename = "type")]
    pub kind: String,
    /// A data URL, which is the documented encoding -- not a bare base64 field.
    pub image_url: String,
}

/// A `computer_call_output` input item to send back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerCallOutput {
    #[serde(rename = "type")]
    pub kind: String,
    pub call_id: String,
    pub output: ScreenshotOutput,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged_safety_checks: Vec<SafetyCheck>,
    /// Why the batch failed, when it did.
    ///
    /// Not serialised: the API documents no error channel on
    /// `computer_call_output`, so the wire form stays a screenshot of whatever
    /// the desktop actually looks like. Fabricating success would be worse, and
    /// silently dropping the reason would be worse still, so the caller gets it
    /// here and decides whether to stop the loop.
    #[serde(skip)]
    pub error: Option<String>,
}

impl ComputerCallOutput {
    fn new(call_id: &str, image_url: String, checks: Vec<SafetyCheck>) -> Self {
        Self {
            kind: "computer_call_output".into(),
            call_id: call_id.to_string(),
            output: ScreenshotOutput {
                kind: "computer_screenshot".into(),
                image_url,
            },
            acknowledged_safety_checks: checks,
            error: None,
        }
    }
}

/// Pull `computer_call` items out of a Responses reply.
///
/// Accepts the whole response object (reads `output`), a bare array of output
/// items, or a single item.
pub fn computer_calls(value: &Value) -> Result<Vec<ComputerCall>> {
    let items = match value {
        Value::Array(items) => items.clone(),
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("computer_call") => {
            vec![value.clone()]
        }
        Value::Object(map) => match map.get("output") {
            Some(Value::Array(items)) => items.clone(),
            _ => {
                return Err(Error::Invalid(
                    "expected a computer_call item, an output array, or a response with output"
                        .into(),
                ));
            }
        },
        _ => {
            return Err(Error::Invalid(
                "expected a computer_call item, an output array, or a response with output".into(),
            ));
        }
    };

    let mut out = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("computer_call") {
            continue;
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("computer_call is missing call_id".into()))?
            .to_string();

        // GA batches under `actions`; the older shape carried one `action`, and
        // leaves `actions` absent or `action` null, so accept either.
        let actions = match item.get("actions") {
            Some(Value::Array(list)) => list.clone(),
            _ => match item.get("action") {
                Some(Value::Object(_)) => vec![item["action"].clone()],
                _ => {
                    return Err(Error::Invalid(format!(
                        "computer_call {call_id} has neither actions nor action"
                    )));
                }
            },
        };
        if actions.is_empty() {
            return Err(Error::Invalid(format!(
                "computer_call {call_id} has an empty actions array"
            )));
        }

        let pending_safety_checks = match item.get("pending_safety_checks") {
            Some(Value::Array(list)) => list
                .iter()
                .cloned()
                .map(serde_json::from_value)
                .collect::<std::result::Result<Vec<SafetyCheck>, _>>()
                .map_err(|err| Error::Invalid(format!("pending_safety_checks: {err}")))?,
            _ => Vec::new(),
        };

        out.push(ComputerCall {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .map(std::string::ToString::to_string),
            call_id,
            actions,
            pending_safety_checks,
        });
    }
    Ok(out)
}

/// Map every action across every call onto one protocol batch, in order.
///
/// `display` is the size the caller advertised to OpenAI. The GA tool
/// declaration carries no display size, so the model's pixel space is whatever
/// the caller screenshotted at; pass it and coordinates are scaled into
/// `last_frame` space, exactly as the Anthropic adapter does.
pub fn actions_from_calls(
    session_id: impl Into<String>,
    batch_id: impl Into<String>,
    calls: &[ComputerCall],
    display: Option<(u32, u32)>,
    last_frame: Option<(u32, u32)>,
) -> Result<ActionBatch> {
    let mut items = Vec::new();
    for call in calls {
        for raw in &call.actions {
            let mut action = action_from_value(raw)?;
            if let (Some((from_w, from_h)), Some((to_w, to_h))) = (display, last_frame)
                && (from_w, from_h) != (to_w, to_h)
            {
                action.scale_coordinates(from_w, from_h, to_w, to_h);
            }
            items.push(action);
        }
    }
    if items.is_empty() {
        return Err(Error::Invalid("no computer actions to run".into()));
    }
    Ok(ActionBatch {
        kind: ActionBatchKind::Actions,
        id: batch_id.into(),
        session_id: session_id.into(),
        items,
    })
}

/// Build one `computer_call_output` per call.
///
/// A call's actions occupy a contiguous run of ack results, so each call is
/// judged on its own actions. Every call answers with a screenshot -- the
/// newest frame the batch produced, else `fallback` -- because that is the only
/// documented output shape. When a call's actions failed, `error` says so
/// rather than the screenshot implying everything went fine.
pub fn outputs_from_ack(
    calls: &[ComputerCall],
    ack: &Ack,
    frames: &[Frame],
    fallback: Option<&Frame>,
) -> Vec<ComputerCallOutput> {
    let mut out = Vec::with_capacity(calls.len());
    let mut at = 0usize;
    let mut frame_i = 0usize;

    for call in calls {
        let n = call.actions.len();
        let results = ack.results.get(at..at + n).unwrap_or(&[]);
        at += n;

        // Frames arrive in action order, so consume the ones this call produced.
        let produced = call
            .actions
            .iter()
            .filter(|a| {
                matches!(
                    a.get("type").and_then(Value::as_str),
                    Some("screenshot" | "zoom" | "cursor_position")
                )
            })
            .count();
        let mine = frames.get(frame_i..frame_i + produced).unwrap_or(&[]);
        frame_i = (frame_i + produced).min(frames.len());

        let image = mine
            .last()
            .or_else(|| frames.last())
            .or(fallback)
            .map(data_url)
            .unwrap_or_default();

        let mut item =
            ComputerCallOutput::new(&call.call_id, image, call.pending_safety_checks.clone());
        if let Some(failed) = results.iter().find(|r| !r.ok) {
            item.error = Some(
                failed
                    .error
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "action failed".into()),
            );
        } else if results.len() < n {
            item.error = Some("ack is missing results for this call".into());
        }
        out.push(item);
    }
    out
}

fn data_url(frame: &Frame) -> String {
    let mime = if frame.mime.is_empty() {
        "image/png"
    } else {
        frame.mime.as_str()
    };
    format!("data:{mime};base64,{}", STANDARD.encode(&frame.data))
}

fn action_from_value(raw: &Value) -> Result<Action> {
    let kind = raw
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Invalid("action is missing type".into()))?;
    match kind {
        "screenshot" => Ok(Action::Screenshot {}),
        "click" => Ok(Action::Click {
            button: button(raw)?,
            xy: xy(raw)?,
            mods: keys(raw, "keys").unwrap_or_default(),
        }),
        "double_click" => Ok(Action::DoubleClick {
            xy: xy(raw)?,
            button: button(raw)?,
        }),
        "move" => Ok(Action::Move { xy: xy(raw)? }),
        "scroll" => Ok(Action::Scroll {
            xy: xy(raw)?,
            dx: notches(raw, "scroll_x")?,
            dy: notches(raw, "scroll_y")?,
        }),
        "type" => {
            let text = raw
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("type is missing text".into()))?;
            if text.is_empty() {
                return Err(Error::Invalid("type text is empty".into()));
            }
            Ok(Action::Type {
                text: text.to_string(),
            })
        }
        "keypress" => {
            let keys = keys(raw, "keys")
                .ok_or_else(|| Error::Invalid("keypress is missing keys".into()))?;
            if keys.is_empty() {
                return Err(Error::Invalid("keypress keys is empty".into()));
            }
            Ok(Action::Key { keys, repeat: 1 })
        }
        "drag" => Ok(Action::Drag { path: path(raw)? }),
        // The GA schema documents no duration. One reference implementation
        // reads an optional `ms`, so honour it when present and otherwise wait
        // a beat rather than refusing the action.
        "wait" => Ok(Action::Wait {
            ms: raw.get("ms").and_then(Value::as_u64).unwrap_or(1000),
        }),
        other => Err(Error::Unsupported(format!(
            "unknown computer action `{other}`"
        ))),
    }
}

fn button(raw: &Value) -> Result<Button> {
    match raw.get("button").and_then(Value::as_str) {
        None | Some("left") => Ok(Button::Left),
        Some("right") => Ok(Button::Right),
        Some("middle") => Ok(Button::Middle),
        // Real values a reference implementation handles, with no protocol
        // equivalent. Refusing is better than silently clicking the left button
        // somewhere the model did not ask for.
        Some(other @ ("wheel" | "back" | "forward")) => Err(Error::Unsupported(format!(
            "click button `{other}` has no protocol equivalent"
        ))),
        Some(other) => Err(Error::Invalid(format!("unknown click button `{other}`"))),
    }
}

fn coord(raw: &Value, field: &str) -> Result<i32> {
    raw.get(field)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| Error::Invalid(format!("action is missing integer `{field}`")))
}

fn xy(raw: &Value) -> Result<Point> {
    Ok([coord(raw, "x")?, coord(raw, "y")?])
}

/// Pixels to wheel notches, keeping a small scroll from vanishing.
fn notches(raw: &Value, field: &str) -> Result<i32> {
    let px = match raw.get(field) {
        None => return Ok(0),
        Some(v) => v
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| Error::Invalid(format!("scroll `{field}` must be an integer")))?,
    };
    if px == 0 {
        return Ok(0);
    }
    let n = px / PIXELS_PER_NOTCH;
    Ok(if n == 0 { px.signum() } else { n })
}

fn keys(raw: &Value, field: &str) -> Option<Vec<String>> {
    let list = raw.get(field)?.as_array()?;
    Some(
        list.iter()
            .filter_map(Value::as_str)
            .map(std::string::ToString::to_string)
            .collect(),
    )
}

/// Accept `[[x, y], ...]` and `[{"x": x, "y": y}, ...]`.
///
/// The guide describes both; the only working reference implementation handles
/// only the object form. Taking both costs nothing and avoids a drag that
/// silently does not happen.
fn path(raw: &Value) -> Result<Vec<Point>> {
    let list = raw
        .get("path")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Invalid("drag is missing path".into()))?;
    let mut out = Vec::with_capacity(list.len());
    for point in list {
        match point {
            Value::Object(_) => out.push(xy(point)?),
            Value::Array(pair) if pair.len() == 2 => {
                let x = pair[0]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| Error::Invalid("drag path x must be an integer".into()))?;
                let y = pair[1]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| Error::Invalid("drag path y must be an integer".into()))?;
                out.push([x, y]);
            }
            _ => {
                return Err(Error::Invalid(
                    "drag path point must be [x, y] or {x, y}".into(),
                ));
            }
        }
    }
    if out.len() < 2 {
        return Err(Error::Invalid("drag path needs at least two points".into()));
    }
    Ok(out)
}
