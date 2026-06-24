//! `ng-automount` — UDisks2 reactor + dockapp surface (PLAN §5, Layer 2).
//!
//! Event-driven, idle-until-poked: subscribes to `org.freedesktop.UDisks2` over
//! D-Bus and surfaces mounts via a dockapp, talking to the WM over EWMH.
//!
//! No behavior yet — Week 1 scaffold. The reactor lands in Week 2 (PLAN §8).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("ng-automount: scaffold only — no reactor wired yet");
    Ok(())
}
