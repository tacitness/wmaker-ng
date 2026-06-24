//! Shared X11 primitives for the wmaker-ng companions.
//!
//! This crate is the single seam through which both facets (`ng-*` and `ai-*`)
//! reach the X server's extension surface:
//!
//! - **XDamage** — dirty-rectangle change feed (the screen-diff protocol, PLAN §6)
//! - **XTEST**   — pointer/keyboard input synthesis (PLAN §5)
//! - **XShm**    — fast shared-memory pixel capture (PLAN §5)
//! - **XFixes**  — cursor and region helpers
//!
//! No behavior yet — Week 1 scaffold only. Capture/input wiring lands with the
//! `ai-mcp` skeleton in Week 2 (PLAN §8).

#![forbid(unsafe_op_in_unsafe_fn)]

/// Placeholder marker so downstream crates have something to link against
/// before the real connection helpers are implemented.
pub const SCAFFOLD: &str = "wmng-x11";
