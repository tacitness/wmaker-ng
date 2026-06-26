//! Typed errors for the shared X surface. No panics on the hot path — every
//! fallible X call surfaces as one of these.

use thiserror::Error;

/// Errors from connecting to or driving the X server.
#[derive(Debug, Error)]
pub enum Error {
    #[error("X11 connect failed: {0}")]
    Connect(#[from] x11rb::errors::ConnectError),

    #[error("X11 connection error: {0}")]
    Connection(#[from] x11rb::errors::ConnectionError),

    #[error("X11 reply error: {0}")]
    Reply(#[from] x11rb::errors::ReplyError),

    #[error("X11 reply/id error: {0}")]
    ReplyOrId(#[from] x11rb::errors::ReplyOrIdError),

    #[error("required X extension missing: {0}")]
    MissingExtension(&'static str),

    #[error("MIT-SHM allocation failed: {0}")]
    Shm(&'static str),

    #[error("no keycode maps to keysym {0:#x}")]
    NoKeycode(u32),
}

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, Error>;
