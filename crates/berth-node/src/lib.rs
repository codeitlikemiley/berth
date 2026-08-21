//! Isolated Linux desktop sessions via Docker, plus the private node control plane.
//!
//! The guest is Xvfb inside `berthos-linux-xfce:dev`. This crate never drives
//! the host desktop: no `--network=host`, no `/tmp/.X11-unix`, no host DISPLAY.
//! Callers must [`Session::stop`] a live session; Drop best-effort removes the
//! container and keeps the workspace volume.

mod action;
mod db;
mod docker;
mod error;
mod guest;
mod http;
mod id;
mod session;
mod tunnel;

pub use action::{ACTION_BIN, FRAME_HEIGHT, FRAME_WIDTH};
pub use docker::{DEFAULT_IMAGE, WORKSPACE_MOUNT, volume_name};
pub use error::{Error, Result};
pub use http::{serve, serve_blocking};
pub use session::{ExecOutput, Session, SessionManager};
pub use tunnel::TunnelKind;
