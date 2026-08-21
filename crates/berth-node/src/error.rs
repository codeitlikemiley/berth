use std::fmt;
use std::io;

use berth_protocol::MvpError;

#[derive(Debug)]
pub enum Error {
    Mvp(MvpError),
    Docker(bollard::errors::Error),
    ReadyTimeout { last_stderr: String },
    Guest(String),
    InvalidPng,
    InvalidResources,
    Stopped,
    ResourceOverflow(&'static str),
    Db(rusqlite::Error),
    Json(serde_json::Error),
    Io(io::Error),
    Unauthorized,
    NotFound,
    BadRequest(String),
    ShuttingDown,
    Tunnel(String),
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mvp(err) => write!(f, "{err}"),
            Self::Docker(err) => write!(f, "docker: {err}"),
            Self::ReadyTimeout { last_stderr } => {
                if last_stderr.is_empty() {
                    write!(f, "guest desktop did not become ready")
                } else {
                    write!(f, "guest desktop did not become ready: {last_stderr}")
                }
            }
            Self::Guest(msg) => write!(f, "{msg}"),
            Self::InvalidPng => write!(f, "screenshot was not a PNG with a valid IHDR"),
            Self::InvalidResources => {
                write!(
                    f,
                    "vcpu and mem_gib must be greater than zero (0 is not unlimited)"
                )
            }
            Self::Stopped => write!(f, "session is stopped"),
            Self::ResourceOverflow(what) => {
                write!(f, "resource {what} is too large for a container cap")
            }
            Self::Db(err) => write!(f, "sqlite: {err}"),
            Self::Json(err) => write!(f, "json: {err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::NotFound => write!(f, "not found"),
            Self::BadRequest(msg) => write!(f, "{msg}"),
            Self::ShuttingDown => write!(f, "node is shutting down"),
            Self::Tunnel(msg) | Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mvp(err) => Some(err),
            Self::Docker(err) => Some(err),
            Self::Db(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::ReadyTimeout { .. }
            | Self::Guest(_)
            | Self::InvalidPng
            | Self::InvalidResources
            | Self::Stopped
            | Self::ResourceOverflow(_)
            | Self::Unauthorized
            | Self::NotFound
            | Self::BadRequest(_)
            | Self::ShuttingDown
            | Self::Tunnel(_)
            | Self::Internal(_) => None,
        }
    }
}

impl From<MvpError> for Error {
    fn from(err: MvpError) -> Self {
        Self::Mvp(err)
    }
}

impl From<bollard::errors::Error> for Error {
    fn from(err: bollard::errors::Error) -> Self {
        Self::Docker(err)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
