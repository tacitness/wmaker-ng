//! Minimal AT-SPI client for semantic UI-tree spikes.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use tokio::time::timeout;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy, connection::Builder};

use crate::Result;

const A11Y_BUS_DESTINATION: &str = "org.a11y.Bus";
const A11Y_BUS_PATH: &str = "/org/a11y/bus";
const A11Y_BUS_INTERFACE: &str = "org.a11y.Bus";
const ATSPI_REGISTRY_DESTINATION: &str = "org.a11y.atspi.Registry";
const ATSPI_ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const ATSPI_ACCESSIBLE_INTERFACE: &str = "org.a11y.atspi.Accessible";
const ATSPI_COMPONENT_INTERFACE: &str = "org.a11y.atspi.Component";
const ATSPI_COORD_TYPE_SCREEN: u32 = 0;
const DBUS_DESTINATION: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const NODE_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_ROOT_SERVICES: usize = 8;

/// One sampled AT-SPI accessible node.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityNode {
    pub id: usize,
    pub parent: Option<usize>,
    pub bus_name: String,
    pub object_path: String,
    pub depth: u8,
    pub name: Option<String>,
    pub role_name: Option<String>,
    pub description: Option<String>,
    pub child_count: u32,
    pub extents: Option<AccessibilityExtents>,
    pub errors: Vec<String>,
}

/// Screen-relative bounds reported by AT-SPI's Component interface.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityExtents {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Bounded AT-SPI snapshot suitable for model-facing spike output.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilitySnapshot {
    pub available: bool,
    pub bus_address: Option<String>,
    pub max_depth: u8,
    pub max_children_per_node: u8,
    pub nodes: Vec<AccessibilityNode>,
    pub errors: Vec<String>,
}

/// Async helper bound to the private AT-SPI bus.
#[derive(Debug, Clone)]
pub struct AtSpi {
    connection: Connection,
    bus_address: String,
}

impl AtSpi {
    /// Resolve the private AT-SPI bus from the user session bus and connect.
    pub async fn connect_from_session() -> Result<Self> {
        let session = Connection::session().await?;
        let bus = Proxy::new(
            &session,
            A11Y_BUS_DESTINATION,
            A11Y_BUS_PATH,
            A11Y_BUS_INTERFACE,
        )
        .await?;
        let bus_address: String = bus.call("GetAddress", &()).await?;
        let connection = Builder::address(bus_address.as_str())?.build().await?;
        Ok(Self {
            connection,
            bus_address,
        })
    }

    /// Read a bounded accessibility tree from the AT-SPI desktop root.
    pub async fn snapshot(
        &self,
        max_depth: u8,
        max_children_per_node: u8,
    ) -> AccessibilitySnapshot {
        let max_depth = max_depth.max(1);
        let max_children_per_node = max_children_per_node.max(1);
        let mut snapshot = AccessibilitySnapshot {
            available: true,
            bus_address: Some(self.bus_address.clone()),
            max_depth,
            max_children_per_node,
            nodes: Vec::new(),
            errors: Vec::new(),
        };
        let mut seen = HashSet::new();
        let mut queue = self.root_nodes(&mut snapshot.errors).await;

        while let Some(pending) = queue.pop_front() {
            if pending.depth > max_depth {
                continue;
            }
            let key = (pending.bus_name.clone(), pending.object_path.clone());
            if !seen.insert(key) {
                continue;
            }

            match timeout(
                NODE_PROBE_TIMEOUT,
                self.read_node(pending, snapshot.nodes.len()),
            )
            .await
            {
                Ok(Ok((node, children))) => {
                    let node_id = node.id;
                    let enqueue_children = node.depth < max_depth;
                    snapshot.nodes.push(node);
                    if enqueue_children {
                        queue.extend(
                            children
                                .into_iter()
                                .take(max_children_per_node as usize)
                                .map(|child| PendingNode {
                                    parent: Some(node_id),
                                    bus_name: child.bus_name,
                                    object_path: child.object_path,
                                    depth: child.depth,
                                }),
                        );
                    }
                }
                Ok(Err(error)) => snapshot.errors.push(error.to_string()),
                Err(_) => snapshot
                    .errors
                    .push("AT-SPI node probe timed out".to_string()),
            }
        }

        if snapshot.nodes.is_empty() && snapshot.errors.is_empty() {
            snapshot
                .errors
                .push("AT-SPI returned no accessible nodes".to_string());
        }
        snapshot
    }

    async fn read_node(
        &self,
        pending: PendingNode,
        id: usize,
    ) -> std::result::Result<(AccessibilityNode, Vec<PendingNode>), zbus::Error> {
        let proxy = Proxy::new(
            &self.connection,
            pending.bus_name.as_str(),
            pending.object_path.as_str(),
            ATSPI_ACCESSIBLE_INTERFACE,
        )
        .await?;

        let mut errors = Vec::new();
        let name = optional_property::<String>(&proxy, "Name", &mut errors).await;
        let role_name = match optional_property::<String>(&proxy, "RoleName", &mut errors).await {
            Some(role_name) => Some(role_name),
            None => match proxy.call::<_, _, String>("GetRoleName", &()).await {
                Ok(role_name) => Some(role_name),
                Err(error) => {
                    errors.push(format!("GetRoleName unavailable: {error}"));
                    None
                }
            },
        };
        let description = optional_property::<String>(&proxy, "Description", &mut errors).await;
        let child_count = optional_property::<i32>(&proxy, "ChildCount", &mut errors)
            .await
            .unwrap_or_default()
            .max(0) as u32;
        let extents = self
            .read_extents(pending.bus_name.as_str(), pending.object_path.as_str())
            .await;
        let children = self
            .read_children(
                pending.bus_name.as_str(),
                pending.object_path.as_str(),
                pending.depth.saturating_add(1),
                child_count.min(u32::from(u8::MAX)),
                &mut errors,
            )
            .await;

        Ok((
            AccessibilityNode {
                id,
                parent: pending.parent,
                bus_name: pending.bus_name,
                object_path: pending.object_path,
                depth: pending.depth,
                name,
                role_name,
                description,
                child_count,
                extents,
                errors,
            },
            children,
        ))
    }

    async fn root_nodes(&self, errors: &mut Vec<String>) -> VecDeque<PendingNode> {
        let mut roots = VecDeque::from([PendingNode {
            parent: None,
            bus_name: ATSPI_REGISTRY_DESTINATION.to_string(),
            object_path: ATSPI_ROOT_PATH.to_string(),
            depth: 0,
        }]);

        match Proxy::new(
            &self.connection,
            DBUS_DESTINATION,
            DBUS_PATH,
            DBUS_INTERFACE,
        )
        .await
        {
            Ok(proxy) => match proxy.call::<_, _, Vec<String>>("ListNames", &()).await {
                Ok(names) => roots.extend(
                    names
                        .into_iter()
                        .filter(|name| name.starts_with(':'))
                        .take(MAX_ROOT_SERVICES)
                        .map(|bus_name| PendingNode {
                            parent: None,
                            bus_name,
                            object_path: ATSPI_ROOT_PATH.to_string(),
                            depth: 0,
                        }),
                ),
                Err(error) => errors.push(format!("AT-SPI ListNames failed: {error}")),
            },
            Err(error) => errors.push(format!("AT-SPI D-Bus proxy failed: {error}")),
        }

        roots
    }

    async fn read_extents(
        &self,
        bus_name: &str,
        object_path: &str,
    ) -> Option<AccessibilityExtents> {
        let proxy = Proxy::new(
            &self.connection,
            bus_name,
            object_path,
            ATSPI_COMPONENT_INTERFACE,
        )
        .await
        .ok()?;
        let (x, y, width, height): (i32, i32, i32, i32) = proxy
            .call("GetExtents", &(ATSPI_COORD_TYPE_SCREEN,))
            .await
            .ok()?;
        Some(AccessibilityExtents {
            x,
            y,
            width,
            height,
        })
    }

    async fn read_children(
        &self,
        bus_name: &str,
        object_path: &str,
        depth: u8,
        child_count: u32,
        errors: &mut Vec<String>,
    ) -> Vec<PendingNode> {
        let Ok(proxy) = Proxy::new(
            &self.connection,
            bus_name,
            object_path,
            ATSPI_ACCESSIBLE_INTERFACE,
        )
        .await
        else {
            return Vec::new();
        };

        let mut children = Vec::new();
        for index in 0..child_count {
            match proxy
                .call::<_, _, (String, OwnedObjectPath)>("GetChildAtIndex", &(index as i32,))
                .await
            {
                Ok((child_bus, child_path)) => children.push(PendingNode {
                    parent: None,
                    bus_name: child_bus,
                    object_path: child_path.to_string(),
                    depth,
                }),
                Err(error) => errors.push(format!("GetChildAtIndex({index}) failed: {error}")),
            }
        }
        children
    }
}

#[derive(Debug)]
struct PendingNode {
    parent: Option<usize>,
    bus_name: String,
    object_path: String,
    depth: u8,
}

async fn optional_property<T>(proxy: &Proxy<'_>, name: &str, errors: &mut Vec<String>) -> Option<T>
where
    T: TryFrom<zbus::zvariant::OwnedValue>,
    T::Error: Into<zbus::Error>,
{
    match proxy.get_property(name).await {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{name} unavailable: {error}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessibilityNode, AccessibilitySnapshot};

    #[test]
    fn snapshot_carries_bounds_and_errors_without_pixels() {
        let snapshot = AccessibilitySnapshot {
            available: true,
            bus_address: Some("unix:path=/tmp/atspi".to_string()),
            max_depth: 2,
            max_children_per_node: 8,
            nodes: vec![AccessibilityNode {
                id: 0,
                parent: None,
                bus_name: "org.a11y.atspi.Registry".to_string(),
                object_path: "/org/a11y/atspi/accessible/root".to_string(),
                depth: 0,
                name: Some("main".to_string()),
                role_name: Some("application".to_string()),
                description: None,
                child_count: 1,
                extents: None,
                errors: Vec::new(),
            }],
            errors: Vec::new(),
        };

        assert!(snapshot.available);
        assert_eq!(snapshot.nodes[0].role_name.as_deref(), Some("application"));
        assert_eq!(snapshot.max_children_per_node, 8);
    }
}
