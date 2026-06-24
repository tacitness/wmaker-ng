//! Shared async D-Bus client helpers for the wmaker-ng companions.
//!
//! The system already does the privileged work (udisks2 / logind / upower);
//! the `ng-*` daemons are thin reactors that subscribe here and surface state
//! to the window manager over EWMH (PLAN §5). D-Bus lives ONLY in these
//! companions — never in the C event loop.
//!
//! No behavior yet — Week 1 scaffold only.

/// Placeholder marker until the udisks2/logind/upower proxies are wired.
pub const SCAFFOLD: &str = "wmng-dbus";
