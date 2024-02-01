use std::fmt;

use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    Io(std::io::Error),
    Anyhow(#[from] anyhow::Error),
    Serde(serde_json::Error),
    JoinHandle(tokio::task::JoinError),
    String(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Anyhow(e) => e.fmt(f),
            Error::Serde(e) => e.fmt(f),
            Error::JoinHandle(e) => e.fmt(f),
            Error::String(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serde(e)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        Error::JoinHandle(e)
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::String(e)
    }
}

pub type Result<T, E = Error> = core::result::Result<T, E>;
