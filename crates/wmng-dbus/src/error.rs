//! Typed errors for the shared D-Bus helpers.

use thiserror::Error;

/// Errors from zbus I/O and snapshot parsing.
#[derive(Debug, Error)]
pub enum Error {
    #[error("D-Bus transport error: {0}")]
    ZBus(#[from] zbus::Error),

    #[error("D-Bus FDO error: {0}")]
    Fdo(#[from] zbus::fdo::Error),

    #[error("D-Bus value error: {0}")]
    Variant(#[from] zbus::zvariant::Error),

    #[error("missing {property} on {interface}")]
    MissingProperty {
        interface: &'static str,
        property: &'static str,
    },

    #[error("invalid {property} on {interface}: {reason}")]
    InvalidProperty {
        interface: &'static str,
        property: &'static str,
        reason: &'static str,
    },
}

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, Error>;
