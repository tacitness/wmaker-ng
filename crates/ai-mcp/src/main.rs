//! `ai-mcp` — the heart of wmaker-ai (PLAN §5, Layer 3).
//!
//! An MCP server exposing computer-use-style tools over existing X11
//! extensions — model-agnostic, a broker + capture engine, not an ML runtime:
//!
//! - input synthesis (`move_mouse`, `click`, `type`, `key`) → XTEST
//! - window control (`list_windows`, `focus`, `move`, `resize`, `tile`) → EWMH
//! - screen capture (`screenshot`) → XShm
//! - change feed (`get_changed_regions`) → XDamage (via `ai-proto`)
//!
//! No tools wired yet — Week 1 scaffold. The skeleton lands in Week 2 (PLAN §8).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("ai-mcp: scaffold only — no MCP tools wired yet");
    Ok(())
}
