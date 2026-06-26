//! Typed errors for the EWMH client.

use thiserror::Error;

/// Errors from reading EWMH state or sending `_NET_*` requests.
#[derive(Debug, Error)]
pub enum Error {
    #[error("X11 connection error: {0}")]
    Connection(#[from] x11rb::errors::ConnectionError),

    #[error("X11 reply error: {0}")]
    Reply(#[from] x11rb::errors::ReplyError),

    #[error("X11 reply/id error: {0}")]
    ReplyOrId(#[from] x11rb::errors::ReplyOrIdError),

    #[error("EWMH property unavailable or malformed: {0}")]
    Property(&'static str),
}

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, Error>;
