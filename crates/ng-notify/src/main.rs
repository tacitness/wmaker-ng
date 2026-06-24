//! `ng-notify` — `org.freedesktop.Notifications` server (PLAN §5, Layer 2).
//!
//! Implements the freedesktop notification spec and renders native
//! notifications in the Window Maker idiom, talking to the WM over EWMH.
//!
//! No behavior yet — Week 1 scaffold.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("ng-notify: scaffold only — no notification server wired yet");
    Ok(())
}
