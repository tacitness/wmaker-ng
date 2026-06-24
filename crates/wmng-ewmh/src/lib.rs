//! Shared EWMH (`_NET_*`) window-control client.
//!
//! This is how every companion talks to the window manager: list windows,
//! focus, move, resize, tile — all via standard `_NET_*` messages, so the C
//! core never learns it is being driven (README philosophy; PLAN §5).
//!
//! No behavior yet — Week 1 scaffold only.

/// Placeholder marker until the `_NET_*` client surface is implemented.
pub const SCAFFOLD: &str = "wmng-ewmh";
