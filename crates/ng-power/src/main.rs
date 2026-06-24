//! `ng-power` — logind / UPower session daemon (PLAN §5, Layer 2).
//!
//! Reacts to suspend, idle, lid, and battery events from
//! `org.freedesktop.login1` and UPower; surfaces session state to the WM over
//! EWMH.
//!
//! No behavior yet — Week 1 scaffold.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("ng-power: scaffold only — no session daemon wired yet");
    Ok(())
}
