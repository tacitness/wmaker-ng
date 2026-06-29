//! Minimal UPower client for battery and AC state.

use zbus::{
    Connection,
    fdo::{PropertiesChangedStream, PropertiesProxy},
    proxy,
    proxy::PropertyStream,
};

use crate::Result;

const UPOWER_DESTINATION: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";

#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
pub trait UPowerManager {
    #[zbus(signal)]
    fn device_added(&self, device: zbus::zvariant::OwnedObjectPath) -> zbus::Result<()>;

    #[zbus(signal)]
    fn device_removed(&self, device: zbus::zvariant::OwnedObjectPath) -> zbus::Result<()>;

    fn get_display_device(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    #[zbus(property)]
    fn on_battery(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn lid_is_closed(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn lid_is_present(&self) -> zbus::Result<bool>;
}

#[proxy(interface = "org.freedesktop.UPower.Device")]
trait UPowerDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn percentage(&self) -> zbus::Result<f64>;

    #[zbus(property)]
    fn is_present(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn online(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn warning_level(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn icon_name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn time_to_empty(&self) -> zbus::Result<i64>;

    #[zbus(property)]
    fn time_to_full(&self) -> zbus::Result<i64>;
}

/// Snapshot of the UPower display device used by the desktop surface.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayDeviceSnapshot {
    pub object_path: String,
    pub kind: u32,
    pub state: u32,
    pub percentage: f64,
    pub is_present: bool,
    pub online: bool,
    pub warning_level: u32,
    pub icon_name: String,
    pub time_to_empty: i64,
    pub time_to_full: i64,
}

/// Snapshot of the UPower manager state plus the display device.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerStateSnapshot {
    pub on_battery: bool,
    pub lid_is_closed: bool,
    pub lid_is_present: bool,
    pub display_device: DisplayDeviceSnapshot,
}

/// Async UPower helper bound to the system bus.
#[derive(Debug, Clone)]
pub struct UPower {
    connection: Connection,
}

impl UPower {
    /// Bind a UPower helper to an existing system-bus connection.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Read the current root power state and the display-device snapshot.
    pub async fn snapshot(&self) -> Result<PowerStateSnapshot> {
        let proxy = self.proxy().await?;
        Ok(PowerStateSnapshot {
            on_battery: proxy.on_battery().await?,
            lid_is_closed: proxy.lid_is_closed().await?,
            lid_is_present: proxy.lid_is_present().await?,
            display_device: self.display_device_snapshot().await?,
        })
    }

    /// Read the current UPower display-device snapshot.
    pub async fn display_device_snapshot(&self) -> Result<DisplayDeviceSnapshot> {
        let proxy = self.proxy().await?;
        let path = proxy.get_display_device().await?;
        let device = self.display_device_proxy(path.as_str()).await?;
        Ok(DisplayDeviceSnapshot {
            object_path: path.to_string(),
            kind: device.kind().await?,
            state: device.state().await?,
            percentage: device.percentage().await?,
            is_present: device.is_present().await?,
            online: device.online().await?,
            warning_level: device.warning_level().await?,
            icon_name: device.icon_name().await?,
            time_to_empty: device.time_to_empty().await?,
            time_to_full: device.time_to_full().await?,
        })
    }

    /// Stream root-manager `DeviceAdded` signals.
    pub async fn receive_device_added(&self) -> Result<DeviceAddedStream> {
        Ok(self.proxy().await?.receive_device_added().await?)
    }

    /// Stream root-manager `DeviceRemoved` signals.
    pub async fn receive_device_removed(&self) -> Result<DeviceRemovedStream> {
        Ok(self.proxy().await?.receive_device_removed().await?)
    }

    /// Stream `OnBattery` property changes from the root manager object.
    pub async fn receive_on_battery_changed(&self) -> Result<PropertyStream<'static, bool>> {
        Ok(self.proxy().await?.receive_on_battery_changed().await)
    }

    /// Stream all property changes for the display-device object.
    pub async fn receive_display_device_changed(&self) -> Result<PropertiesChangedStream> {
        let path = self.proxy().await?.get_display_device().await?;
        Ok(self
            .properties_proxy(path.as_str())
            .await?
            .receive_properties_changed()
            .await?)
    }

    async fn proxy(&self) -> Result<UPowerManagerProxy<'static>> {
        Ok(UPowerManagerProxy::builder(&self.connection)
            .destination(UPOWER_DESTINATION)?
            .path(UPOWER_PATH)?
            .build()
            .await?)
    }

    async fn display_device_proxy<'a>(
        &self,
        object_path: &'a str,
    ) -> Result<UPowerDeviceProxy<'a>> {
        Ok(UPowerDeviceProxy::builder(&self.connection)
            .destination(UPOWER_DESTINATION)?
            .path(object_path)?
            .build()
            .await?)
    }

    async fn properties_proxy<'a>(&self, object_path: &'a str) -> Result<PropertiesProxy<'a>> {
        Ok(PropertiesProxy::builder(&self.connection)
            .destination(UPOWER_DESTINATION)?
            .path(object_path)?
            .build()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::{DisplayDeviceSnapshot, PowerStateSnapshot};

    #[test]
    fn power_snapshot_embeds_display_device() {
        let snapshot = PowerStateSnapshot {
            on_battery: true,
            lid_is_closed: false,
            lid_is_present: true,
            display_device: DisplayDeviceSnapshot {
                object_path: "/org/freedesktop/UPower/devices/DisplayDevice".into(),
                kind: 2,
                state: 1,
                percentage: 87.5,
                is_present: true,
                online: false,
                warning_level: 1,
                icon_name: "battery-good-symbolic".into(),
                time_to_empty: 3_600,
                time_to_full: 0,
            },
        };

        assert!(snapshot.on_battery);
        assert_eq!(snapshot.display_device.icon_name, "battery-good-symbolic");
    }
}
