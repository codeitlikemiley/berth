use std::fmt;
use std::io;

use berthos_protocol::MvpError;

#[derive(Debug)]
pub enum Error {
    Usage(String),
    Config(String),
    Io(io::Error),
    Json(serde_json::Error),
    Http(reqwest::Error),
    Api { status: u16, message: String },
    Mvp(MvpError),
    Ws(String),
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(msg) | Self::Config(msg) | Self::Ws(msg) | Self::Protocol(msg) => {
                write!(f, "{msg}")
            }
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "json: {err}"),
            Self::Http(err) => {
                if err.is_connect() {
                    write!(f, "could not reach node: {err}")
                } else {
                    write!(f, "{err}")
                }
            }
            Self::Api { status, message } => {
                if message.is_empty() {
                    write!(f, "http {status}")
                } else {
                    write!(f, "{message}")
                }
            }
            Self::Mvp(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Http(err) => Some(err),
            Self::Mvp(err) => Some(err),
            Self::Usage(_)
            | Self::Config(_)
            | Self::Api { .. }
            | Self::Ws(_)
            | Self::Protocol(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Self::Config(err.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Self::Config(err.to_string())
    }
}

impl From<MvpError> for Error {
    fn from(err: MvpError) -> Self {
        Self::Mvp(err)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Ws(err.to_string())
    }
}
