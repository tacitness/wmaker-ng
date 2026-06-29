//! Minimal logind client for power/session companions.

use zbus::{Connection, proxy, proxy::PropertyStream};

use crate::Result;

const LOGIN1_DESTINATION: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub trait LoginManager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;

    #[zbus(property)]
    fn idle_hint(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn idle_since_hint(&self) -> zbus::Result<u64>;

    #[zbus(property)]
    fn preparing_for_sleep(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn lid_closed(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn docked(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn on_external_power(&self) -> zbus::Result<bool>;
}

/// Typed view of the manager properties relevant to `ng-power`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginStateSnapshot {
    pub idle_hint: bool,
    pub idle_since_hint_usec: u64,
    pub preparing_for_sleep: bool,
    pub lid_closed: bool,
    pub docked: bool,
    pub on_external_power: bool,
}

/// Discrete logind state signals useful to consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginState {
    PrepareForSleep(bool),
    IdleHint(bool),
    LidClosed(bool),
}

/// Async logind helper bound to the system bus.
#[derive(Debug, Clone)]
pub struct Login1 {
    connection: Connection,
}

impl Login1 {
    /// Bind a logind helper to an existing system-bus connection.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Read the current logind state in one round-trip bundle.
    pub async fn snapshot(&self) -> Result<LoginStateSnapshot> {
        let proxy = self.proxy().await?;
        Ok(LoginStateSnapshot {
            idle_hint: proxy.idle_hint().await?,
            idle_since_hint_usec: proxy.idle_since_hint().await?,
            preparing_for_sleep: proxy.preparing_for_sleep().await?,
            lid_closed: proxy.lid_closed().await?,
            docked: proxy.docked().await?,
            on_external_power: proxy.on_external_power().await?,
        })
    }

    /// Stream `PrepareForSleep` signals.
    pub async fn receive_prepare_for_sleep(&self) -> Result<PrepareForSleepStream> {
        Ok(self.proxy().await?.receive_prepare_for_sleep().await?)
    }

    /// Stream `PropertiesChanged` updates for `IdleHint`.
    pub async fn receive_idle_hint_changed(&self) -> Result<PropertyStream<'static, bool>> {
        Ok(self.proxy().await?.receive_idle_hint_changed().await)
    }

    /// Stream `PropertiesChanged` updates for `LidClosed`.
    pub async fn receive_lid_closed_changed(&self) -> Result<PropertyStream<'static, bool>> {
        Ok(self.proxy().await?.receive_lid_closed_changed().await)
    }

    async fn proxy(&self) -> Result<LoginManagerProxy<'static>> {
        Ok(LoginManagerProxy::builder(&self.connection)
            .destination(LOGIN1_DESTINATION)?
            .path(LOGIN1_PATH)?
            .build()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::LoginStateSnapshot;

    #[test]
    fn login_state_snapshot_is_plain_old_data() {
        let snapshot = LoginStateSnapshot {
            idle_hint: false,
            idle_since_hint_usec: 0,
            preparing_for_sleep: false,
            lid_closed: true,
            docked: false,
            on_external_power: true,
        };

        assert!(snapshot.lid_closed);
        assert!(snapshot.on_external_power);
        assert!(!snapshot.idle_hint);
    }
}
