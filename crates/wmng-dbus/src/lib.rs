//! Shared async D-Bus clients for the wmaker-ng companions.
//!
//! The `ng-*` daemons stay idle until the system bus tells them something
//! changed: removable media appeared, power state flipped, or logind is about
//! to suspend. This crate is the shared zbus surface for those event-driven
//! daemons; the Window Maker core remains untouched and out-of-process.

mod error;
mod login1;
mod udisks2;
mod upower;
mod util;

pub use error::{Error, Result};
pub use login1::{Login1, LoginState, LoginStateSnapshot};
pub use udisks2::{BlockDeviceSnapshot, UDisks2};
pub use upower::{DisplayDeviceSnapshot, PowerStateSnapshot, UPower};

/// Open a connection to the system bus.
pub async fn system_connection() -> Result<zbus::Connection> {
    Ok(zbus::Connection::system().await?)
}

#[cfg(test)]
mod tests {
    use super::system_connection;

    #[tokio::test]
    #[ignore = "requires a live system bus"]
    async fn system_bus_connection_smoke() {
        system_connection().await.unwrap();
    }
}
