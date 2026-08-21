//! Isolated Linux desktop sessions via Docker.
//!
//! The guest is Xvfb inside `berthos-linux-xfce:dev`. This crate never drives
//! the host desktop: no `--network=host`, no `/tmp/.X11-unix`, no host DISPLAY.

mod action;
mod docker;
mod error;
mod session;

pub use action::{ACTION_BIN, FRAME_HEIGHT, FRAME_WIDTH};
pub use docker::{DEFAULT_IMAGE, WORKSPACE_MOUNT, volume_name};
pub use error::{Error, Result};
pub use session::{Session, SessionManager};
