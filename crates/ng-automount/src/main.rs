//! `ng-automount` — UDisks2 reactor + dockapp-facing status surface.
//!
//! The daemon keeps the policy small and auditable: only removable filesystem
//! block devices are candidates, already-mounted devices are left alone, and
//! every decision is logged for a dockapp or supervisor to consume.

use std::{collections::BTreeMap, env, time::Duration};

use anyhow::Context;
use futures_util::StreamExt as _;
use tokio::time;
use tracing::{debug, error, info, warn};
use wmng_dbus::{BlockDeviceSnapshot, UDisks2, system_connection};

const RECONCILE_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Config {
    dry_run: bool,
    once: bool,
}

impl Config {
    fn from_env_args() -> anyhow::Result<Self> {
        let mut config = Self {
            dry_run: false,
            once: false,
        };
        for arg in env::args().skip(1) {
            match arg.as_str() {
                "--dry-run" => config.dry_run = true,
                "--once" => config.once = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                unknown => anyhow::bail!("unknown argument: {unknown}"),
            }
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeviceDecision {
    Mount { object_path: String },
    AlreadyMounted { mount_points: Vec<String> },
    Ignore { reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceReport {
    device: String,
    object_path: String,
    decision: DeviceDecision,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct AutomountStatus {
    devices_seen: usize,
    mounted: BTreeMap<String, String>,
    skipped: BTreeMap<String, &'static str>,
}

impl AutomountStatus {
    fn record(&mut self, report: &DeviceReport, mount_point: Option<String>) {
        self.devices_seen += 1;
        match (&report.decision, mount_point) {
            (DeviceDecision::Mount { .. }, Some(path)) => {
                self.mounted.insert(report.device.clone(), path);
            }
            (DeviceDecision::AlreadyMounted { mount_points }, _) => {
                self.mounted.insert(
                    report.device.clone(),
                    mount_points.first().cloned().unwrap_or_default(),
                );
            }
            (DeviceDecision::Ignore { reason }, _) => {
                self.skipped.insert(report.device.clone(), reason);
            }
            (DeviceDecision::Mount { .. }, None) => {
                self.skipped.insert(report.device.clone(), "dry-run");
            }
        }
    }
}

fn main_policy(device: &BlockDeviceSnapshot) -> DeviceReport {
    DeviceReport {
        device: device.preferred_device.clone(),
        object_path: device.object_path.clone(),
        decision: decide(device),
    }
}

fn decide(device: &BlockDeviceSnapshot) -> DeviceDecision {
    if device.id_usage.as_deref() != Some("filesystem") {
        return DeviceDecision::Ignore {
            reason: "not-filesystem",
        };
    }
    if !device.mount_points.is_empty() {
        return DeviceDecision::AlreadyMounted {
            mount_points: device.mount_points.clone(),
        };
    }
    if device.size == 0 {
        return DeviceDecision::Ignore {
            reason: "empty-device",
        };
    }
    if !is_removable_candidate(device) {
        return DeviceDecision::Ignore {
            reason: "not-removable",
        };
    }
    DeviceDecision::Mount {
        object_path: device.object_path.clone(),
    }
}

fn is_removable_candidate(device: &BlockDeviceSnapshot) -> bool {
    device.removable || matches!(device.connection_bus.as_deref(), Some("usb") | Some("sdio"))
}

async fn reconcile(udisks: &UDisks2, config: Config) -> anyhow::Result<AutomountStatus> {
    let devices = udisks
        .snapshot()
        .await
        .context("snapshot UDisks2 devices")?;
    let mut status = AutomountStatus::default();

    for device in devices {
        let report = main_policy(&device);
        match &report.decision {
            DeviceDecision::Mount { object_path } if !config.dry_run => {
                info!(
                    device = %report.device,
                    object_path = %object_path,
                    "mounting removable filesystem"
                );
                match udisks.mount(object_path).await {
                    Ok(mount_point) => {
                        info!(
                            device = %report.device,
                            mount_point = %mount_point,
                            "mounted removable filesystem"
                        );
                        status.record(&report, Some(mount_point));
                    }
                    Err(err) => {
                        warn!(
                            device = %report.device,
                            object_path = %object_path,
                            error = %err,
                            "failed to mount removable filesystem"
                        );
                        status.skipped.insert(report.device.clone(), "mount-failed");
                    }
                }
            }
            DeviceDecision::Mount { object_path } => {
                info!(
                    device = %report.device,
                    object_path = %object_path,
                    "would mount removable filesystem"
                );
                status.record(&report, None);
            }
            DeviceDecision::AlreadyMounted { mount_points } => {
                debug!(
                    device = %report.device,
                    mount_points = ?mount_points,
                    "removable filesystem already mounted"
                );
                status.record(&report, None);
            }
            DeviceDecision::Ignore { reason } => {
                debug!(
                    device = %report.device,
                    object_path = %report.object_path,
                    reason,
                    "skipping block device"
                );
                status.record(&report, None);
            }
        }
    }

    info!(
        devices_seen = status.devices_seen,
        mounted = status.mounted.len(),
        skipped = status.skipped.len(),
        "automount reconcile complete"
    );
    Ok(status)
}

async fn run(config: Config) -> anyhow::Result<()> {
    let connection = system_connection().await.context("connect to system bus")?;
    let udisks = UDisks2::new(connection);

    reconcile(&udisks, config).await?;
    if config.once {
        return Ok(());
    }

    let mut added = udisks
        .receive_interfaces_added()
        .await
        .context("subscribe to UDisks2 InterfacesAdded")?;
    let mut removed = udisks
        .receive_interfaces_removed()
        .await
        .context("subscribe to UDisks2 InterfacesRemoved")?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            signal = added.next() => {
                if signal.is_none() {
                    warn!("UDisks2 InterfacesAdded stream ended");
                    return Ok(());
                }
                time::sleep(RECONCILE_DEBOUNCE).await;
                if let Err(err) = reconcile(&udisks, config).await {
                    error!(error = %err, "automount reconcile failed after add signal");
                }
            }
            signal = removed.next() => {
                if signal.is_none() {
                    warn!("UDisks2 InterfacesRemoved stream ended");
                    return Ok(());
                }
                time::sleep(RECONCILE_DEBOUNCE).await;
                if let Err(err) = reconcile(&udisks, config).await {
                    error!(error = %err, "automount reconcile failed after remove signal");
                }
            }
        }
    }
}

fn print_help() {
    println!(
        "ng-automount\n\nUsage: ng-automount [--dry-run] [--once]\n\n  --dry-run  report mount candidates without calling UDisks2 Mount\n  --once     reconcile current devices once, then exit"
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "ng_automount=info,wmng_dbus=warn".into()),
        )
        .init();

    run(Config::from_env_args()?).await
}

#[cfg(test)]
mod tests {
    use super::{DeviceDecision, decide};
    use wmng_dbus::BlockDeviceSnapshot;

    fn device() -> BlockDeviceSnapshot {
        BlockDeviceSnapshot {
            object_path: "/org/freedesktop/UDisks2/block_devices/sdb1".into(),
            drive_path: Some("/org/freedesktop/UDisks2/drives/usb".into()),
            device: "/dev/sdb1".into(),
            preferred_device: "/dev/disk/by-uuid/demo".into(),
            id_usage: Some("filesystem".into()),
            id_type: Some("vfat".into()),
            size: 1024,
            mount_points: Vec::new(),
            removable: true,
            connection_bus: Some("usb".into()),
        }
    }

    #[test]
    fn mounts_removable_filesystem_candidate() {
        assert_eq!(
            decide(&device()),
            DeviceDecision::Mount {
                object_path: "/org/freedesktop/UDisks2/block_devices/sdb1".into()
            }
        );
    }

    #[test]
    fn skips_non_filesystem_devices() {
        let mut device = device();
        device.id_usage = Some("crypto".into());
        assert_eq!(
            decide(&device),
            DeviceDecision::Ignore {
                reason: "not-filesystem"
            }
        );
    }

    #[test]
    fn reports_already_mounted_devices() {
        let mut device = device();
        device.mount_points = vec!["/run/media/joel/USB".into()];
        assert_eq!(
            decide(&device),
            DeviceDecision::AlreadyMounted {
                mount_points: vec!["/run/media/joel/USB".into()]
            }
        );
    }

    #[test]
    fn skips_fixed_internal_devices() {
        let mut device = device();
        device.removable = false;
        device.connection_bus = Some("ata".into());
        assert_eq!(
            decide(&device),
            DeviceDecision::Ignore {
                reason: "not-removable"
            }
        );
    }

    #[test]
    fn treats_usb_bus_as_removable_candidate() {
        let mut device = device();
        device.removable = false;
        device.connection_bus = Some("usb".into());
        assert!(matches!(decide(&device), DeviceDecision::Mount { .. }));
    }
}
