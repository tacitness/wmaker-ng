//! Minimal UDisks2 client for removable-media daemons.

use std::collections::HashMap;

use zbus::{
    Connection,
    fdo::{InterfacesAddedStream, InterfacesRemovedStream, ManagedObjects, ObjectManagerProxy},
    proxy,
    zvariant::{OwnedObjectPath, Value},
};

use crate::{
    Result,
    util::{InterfaceMap, PropertyMap, bytes_to_string, normalize_string, optional, required},
};

const UDISKS2_DESTINATION: &str = "org.freedesktop.UDisks2";
const UDISKS2_PATH: &str = "/org/freedesktop/UDisks2";
const BLOCK_IFACE: &str = "org.freedesktop.UDisks2.Block";
const DRIVE_IFACE: &str = "org.freedesktop.UDisks2.Drive";
const FILESYSTEM_IFACE: &str = "org.freedesktop.UDisks2.Filesystem";

#[proxy(interface = "org.freedesktop.UDisks2.Filesystem")]
trait Filesystem {
    fn mount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<Vec<u8>>;
    fn unmount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// A UDisks2 block device joined with the filesystem and drive properties
/// commonly needed by the automount daemon.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDeviceSnapshot {
    pub object_path: String,
    pub drive_path: Option<String>,
    pub device: String,
    pub preferred_device: String,
    pub id_usage: Option<String>,
    pub id_type: Option<String>,
    pub size: u64,
    pub mount_points: Vec<String>,
    pub removable: bool,
    pub connection_bus: Option<String>,
}

/// Async UDisks2 helper bound to the system bus.
#[derive(Debug, Clone)]
pub struct UDisks2 {
    connection: Connection,
}

impl UDisks2 {
    /// Bind a UDisks2 helper to an existing system-bus connection.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Current managed block devices, filtered to objects that expose
    /// `org.freedesktop.UDisks2.Block`.
    pub async fn snapshot(&self) -> Result<Vec<BlockDeviceSnapshot>> {
        let objects = self.object_manager().await?.get_managed_objects().await?;
        let mut devices = parse_managed_objects(objects)?;
        devices.sort_by(|left, right| left.object_path.cmp(&right.object_path));
        Ok(devices)
    }

    /// Stream `InterfacesAdded` signals from the UDisks2 object manager.
    pub async fn receive_interfaces_added(&self) -> Result<InterfacesAddedStream> {
        Ok(self
            .object_manager()
            .await?
            .receive_interfaces_added()
            .await?)
    }

    /// Stream `InterfacesRemoved` signals from the UDisks2 object manager.
    pub async fn receive_interfaces_removed(&self) -> Result<InterfacesRemovedStream> {
        Ok(self
            .object_manager()
            .await?
            .receive_interfaces_removed()
            .await?)
    }

    /// Mount a filesystem object and return the resulting mount point.
    pub async fn mount(&self, object_path: &str) -> Result<String> {
        let path = self
            .filesystem_proxy(object_path)
            .await?
            .mount(HashMap::<&str, Value<'_>>::new())
            .await?;
        bytes_to_string(path, FILESYSTEM_IFACE, "Mount")
    }

    /// Unmount a filesystem object.
    pub async fn unmount(&self, object_path: &str) -> Result<()> {
        self.filesystem_proxy(object_path)
            .await?
            .unmount(HashMap::<&str, Value<'_>>::new())
            .await?;
        Ok(())
    }

    async fn object_manager(&self) -> Result<ObjectManagerProxy<'_>> {
        Ok(ObjectManagerProxy::builder(&self.connection)
            .destination(UDISKS2_DESTINATION)?
            .path(UDISKS2_PATH)?
            .build()
            .await?)
    }

    async fn filesystem_proxy<'a>(&self, object_path: &'a str) -> Result<FilesystemProxy<'a>> {
        Ok(FilesystemProxy::builder(&self.connection)
            .destination(UDISKS2_DESTINATION)?
            .path(object_path)?
            .build()
            .await?)
    }
}

pub(crate) fn parse_managed_objects(objects: ManagedObjects) -> Result<Vec<BlockDeviceSnapshot>> {
    let objects: Vec<(OwnedObjectPath, InterfaceMap)> = objects
        .into_iter()
        .map(|(path, interfaces)| {
            (
                path,
                interfaces
                    .into_iter()
                    .map(|(name, props)| (name.to_string(), props))
                    .collect(),
            )
        })
        .collect();
    let drive_data: HashMap<String, (bool, Option<String>)> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces.get(DRIVE_IFACE).map(|props| {
                Ok((
                    path.to_string(),
                    (
                        optional::<bool>(props, DRIVE_IFACE, "MediaRemovable")?.unwrap_or(false),
                        normalize_string(optional::<String>(props, DRIVE_IFACE, "ConnectionBus")?),
                    ),
                ))
            })
        })
        .collect::<Result<_>>()?;
    let mut devices = Vec::new();
    for (path, interfaces) in objects {
        if let Some(device) = block_device_from_interfaces(path, &interfaces, &drive_data)? {
            devices.push(device);
        }
    }
    Ok(devices)
}

fn block_device_from_interfaces(
    path: OwnedObjectPath,
    interfaces: &InterfaceMap,
    drive_data: &HashMap<String, (bool, Option<String>)>,
) -> Result<Option<BlockDeviceSnapshot>> {
    let Some(block) = interfaces.get(BLOCK_IFACE) else {
        return Ok(None);
    };

    let device = bytes_to_string(
        required::<Vec<u8>>(block, BLOCK_IFACE, "Device")?,
        BLOCK_IFACE,
        "Device",
    )?;
    let preferred_device = bytes_to_string(
        required::<Vec<u8>>(block, BLOCK_IFACE, "PreferredDevice")?,
        BLOCK_IFACE,
        "PreferredDevice",
    )?;
    let drive_path = normalize_string(
        optional::<OwnedObjectPath>(block, BLOCK_IFACE, "Drive")?.map(|path| path.to_string()),
    );
    let id_usage = normalize_string(optional::<String>(block, BLOCK_IFACE, "IdUsage")?);
    let id_type = normalize_string(optional::<String>(block, BLOCK_IFACE, "IdType")?);
    let size = required::<u64>(block, BLOCK_IFACE, "Size")?;

    let mount_points = interfaces
        .get(FILESYSTEM_IFACE)
        .map(parse_mount_points)
        .transpose()?
        .unwrap_or_default();

    let (removable, connection_bus) = drive_path
        .as_ref()
        .and_then(|drive| drive_data.get(drive))
        .cloned()
        .unwrap_or((false, None));

    Ok(Some(BlockDeviceSnapshot {
        object_path: path.to_string(),
        drive_path,
        device,
        preferred_device,
        id_usage,
        id_type,
        size,
        mount_points,
        removable,
        connection_bus,
    }))
}

fn parse_mount_points(props: &PropertyMap) -> Result<Vec<String>> {
    let Some(raw_points) = optional::<Vec<Vec<u8>>>(props, FILESYSTEM_IFACE, "MountPoints")? else {
        return Ok(Vec::new());
    };

    raw_points
        .into_iter()
        .map(|bytes| bytes_to_string(bytes, FILESYSTEM_IFACE, "MountPoints"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::zvariant::{ObjectPath, OwnedValue, Value};

    use super::{
        BLOCK_IFACE, BlockDeviceSnapshot, DRIVE_IFACE, FILESYSTEM_IFACE, parse_managed_objects,
    };
    use zbus::fdo::ManagedObjects;

    #[test]
    fn parse_block_device_snapshot() {
        let mut objects: ManagedObjects = HashMap::new();

        let mut block_object = HashMap::<_, _>::new();
        let mut block = HashMap::<String, OwnedValue>::new();
        block.insert(
            "Device".into(),
            OwnedValue::try_from(Value::from(b"/dev/sdb1\0".to_vec())).unwrap(),
        );
        block.insert(
            "PreferredDevice".into(),
            OwnedValue::try_from(Value::from(b"/dev/disk/by-uuid/demo\0".to_vec())).unwrap(),
        );
        block.insert(
            "Drive".into(),
            OwnedValue::from(ObjectPath::try_from("/org/freedesktop/UDisks2/drives/usb0").unwrap()),
        );
        block.insert(
            "IdUsage".into(),
            OwnedValue::try_from(Value::from(String::from("filesystem"))).unwrap(),
        );
        block.insert(
            "IdType".into(),
            OwnedValue::try_from(Value::from(String::from("vfat"))).unwrap(),
        );
        block.insert("Size".into(), OwnedValue::from(8_589_934_592_u64));
        block_object.insert(BLOCK_IFACE.try_into().unwrap(), block);

        let mut filesystem = HashMap::<String, OwnedValue>::new();
        filesystem.insert(
            "MountPoints".into(),
            OwnedValue::try_from(Value::from(vec![b"/run/media/demo\0".to_vec()])).unwrap(),
        );
        block_object.insert(FILESYSTEM_IFACE.try_into().unwrap(), filesystem);
        objects.insert(
            ObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sdb1")
                .unwrap()
                .into(),
            block_object,
        );

        let mut drive_object = HashMap::<_, _>::new();
        let mut drive = HashMap::<String, OwnedValue>::new();
        drive.insert("MediaRemovable".into(), OwnedValue::from(true));
        drive.insert(
            "ConnectionBus".into(),
            OwnedValue::try_from(Value::from(String::from("usb"))).unwrap(),
        );
        drive_object.insert(DRIVE_IFACE.try_into().unwrap(), drive);
        objects.insert(
            ObjectPath::try_from("/org/freedesktop/UDisks2/drives/usb0")
                .unwrap()
                .into(),
            drive_object,
        );

        let snapshots = parse_managed_objects(objects).unwrap();

        assert_eq!(
            snapshots,
            vec![BlockDeviceSnapshot {
                object_path: "/org/freedesktop/UDisks2/block_devices/sdb1".into(),
                drive_path: Some("/org/freedesktop/UDisks2/drives/usb0".into()),
                device: "/dev/sdb1".into(),
                preferred_device: "/dev/disk/by-uuid/demo".into(),
                id_usage: Some("filesystem".into()),
                id_type: Some("vfat".into()),
                size: 8_589_934_592,
                mount_points: vec!["/run/media/demo".into()],
                removable: true,
                connection_bus: Some("usb".into()),
            }]
        );
    }
}
