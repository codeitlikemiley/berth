use serde::{Deserialize, Serialize};

use crate::coord::{Point, Region, scale_coordinates, scale_region};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionBatchKind {
    #[serde(rename = "actions")]
    Actions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AckKind {
    #[serde(rename = "ack")]
    Ack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    #[serde(rename = "frame")]
    Frame,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

/// One driver operation. Unknown fields are rejected so adapters cannot smuggle
/// lab-specific keys through the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    // Empty structs (not unit variants) so deny_unknown_fields rejects extras.
    Screenshot {},
    Click {
        #[serde(default)]
        button: Button,
        xy: Point,
        #[serde(default)]
        mods: Vec<String>,
    },
    DoubleClick {
        xy: Point,
        #[serde(default, skip_serializing_if = "is_left")]
        button: Button,
    },
    Move {
        xy: Point,
    },
    Drag {
        path: Vec<Point>,
    },
    Scroll {
        xy: Point,
        dx: i32,
        dy: i32,
    },
    Type {
        text: String,
    },
    Key {
        keys: Vec<String>,
        #[serde(default = "one")]
        repeat: u32,
    },
    HoldKey {
        keys: Vec<String>,
        ms: u64,
    },
    Wait {
        ms: u64,
    },
    Zoom {
        region: Region,
    },
    CursorPosition {},
    Shell {
        cmd: String,
    },
}

fn one() -> u32 {
    1
}

fn is_left(button: &Button) -> bool {
    *button == Button::Left
}

impl Action {
    pub fn op(&self) -> &'static str {
        match self {
            Self::Screenshot {} => "screenshot",
            Self::Click { .. } => "click",
            Self::DoubleClick { .. } => "double_click",
            Self::Move { .. } => "move",
            Self::Drag { .. } => "drag",
            Self::Scroll { .. } => "scroll",
            Self::Type { .. } => "type",
            Self::Key { .. } => "key",
            Self::HoldKey { .. } => "hold_key",
            Self::Wait { .. } => "wait",
            Self::Zoom { .. } => "zoom",
            Self::CursorPosition {} => "cursor_position",
            Self::Shell { .. } => "shell",
        }
    }

    /// Scale embedded coordinates from last-frame pixels into guest pixels.
    pub fn scale_coordinates(
        &mut self,
        from_width: u32,
        from_height: u32,
        to_width: u32,
        to_height: u32,
    ) {
        if from_width == to_width && from_height == to_height {
            return;
        }
        match self {
            Self::Click { xy, .. }
            | Self::DoubleClick { xy, .. }
            | Self::Move { xy }
            | Self::Scroll { xy, .. } => {
                *xy = scale_coordinates(*xy, from_width, from_height, to_width, to_height);
            }
            Self::Drag { path } => {
                for p in path {
                    *p = scale_coordinates(*p, from_width, from_height, to_width, to_height);
                }
            }
            Self::Zoom { region } => {
                *region = scale_region(*region, from_width, from_height, to_width, to_height);
            }
            Self::Screenshot {}
            | Self::Type { .. }
            | Self::Key { .. }
            | Self::HoldKey { .. }
            | Self::Wait { .. }
            | Self::CursorPosition {}
            | Self::Shell { .. } => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBatch {
    #[serde(rename = "type")]
    pub kind: ActionBatchKind,
    pub id: String,
    pub session_id: String,
    pub items: Vec<Action>,
}

impl ActionBatch {
    pub fn scale_coordinates(
        &mut self,
        from_width: u32,
        from_height: u32,
        to_width: u32,
        to_height: u32,
    ) {
        for item in &mut self.items {
            item.scale_coordinates(from_width, from_height, to_width, to_height);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    #[serde(rename = "type")]
    pub kind: AckKind,
    pub id: String,
    pub results: Vec<AckResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckResult {
    pub i: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub frame: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(rename = "type")]
    pub kind: FrameKind,
    pub session_id: String,
    pub ts: u64,
    pub width: u32,
    pub height: u32,
    #[serde(default = "png_mime")]
    pub mime: String,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Point>,
}

fn png_mime() -> String {
    "image/png".into()
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}
