//! `ai-mcp` — the heart of wmaker-ai (PLAN §5, Layer 3).
//!
//! A model-agnostic MCP server exposing computer-use tools over existing X11
//! extensions — a broker + capture engine, not an ML runtime. Input synthesis
//! and capture go through `wmng-x11` (XTEST/XShm); window control through
//! `wmng-ewmh` (`_NET_*`). Any MCP client connects over stdio and drives a real
//! Window Maker desktop; the WM never learns it is being driven.

use std::sync::Arc;

use base64::Engine as _;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use wmng_ewmh::{Ewmh, TileSlot};
use wmng_x11::X;

/// The MCP server: holds the shared X connection. Cheap to clone (Arc).
#[derive(Clone)]
struct WmCtl {
    x: Arc<X>,
}

// ── Tool parameter / output schemas (auto-generate the MCP contract) ─────────

#[derive(Deserialize, JsonSchema)]
struct MoveMouse {
    x: i16,
    y: i16,
}

#[derive(Deserialize, JsonSchema)]
struct Click {
    /// Pointer button: 1=left, 2=middle, 3=right.
    #[serde(default = "default_button")]
    button: u8,
}
fn default_button() -> u8 {
    1
}

#[derive(Deserialize, JsonSchema)]
struct TypeText {
    text: String,
}

#[derive(Deserialize, JsonSchema)]
struct Key {
    /// X keysym (e.g. 0xff0d = Return).
    keysym: u32,
}

#[derive(Deserialize, JsonSchema)]
struct Focus {
    window: u32,
}

#[derive(Deserialize, JsonSchema)]
struct MoveResize {
    window: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Deserialize, JsonSchema)]
struct Tile {
    window: u32,
    slot: Slot,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Slot {
    Left,
    Right,
    Top,
    Bottom,
    Full,
}

impl From<Slot> for TileSlot {
    fn from(s: Slot) -> Self {
        match s {
            Slot::Left => TileSlot::Left,
            Slot::Right => TileSlot::Right,
            Slot::Top => TileSlot::Top,
            Slot::Bottom => TileSlot::Bottom,
            Slot::Full => TileSlot::Full,
        }
    }
}

#[derive(Serialize, JsonSchema)]
struct Status {
    ok: bool,
}
fn ok() -> Json<Status> {
    Json(Status { ok: true })
}

#[derive(Serialize, JsonSchema)]
struct WindowOut {
    id: u32,
    title: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Serialize, JsonSchema)]
struct WindowList {
    windows: Vec<WindowOut>,
}

#[tool_router(server_handler)]
impl WmCtl {
    // ── Input synthesis (XTEST) ──────────────────────────────────────────────
    #[tool(description = "Move the pointer to absolute root coordinates.")]
    async fn move_mouse(
        &self,
        Parameters(p): Parameters<MoveMouse>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.move_pointer(p.x, p.y).map(|_| ok()).map_err(to_err)).await
    }

    #[tool(description = "Click a pointer button at the current position.")]
    async fn click(&self, Parameters(p): Parameters<Click>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.click(p.button).map(|_| ok()).map_err(to_err)).await
    }

    #[tool(name = "type", description = "Type a string of text.")]
    async fn type_text(
        &self,
        Parameters(p): Parameters<TypeText>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.type_text(&p.text).map(|_| ok()).map_err(to_err)).await
    }

    #[tool(description = "Tap a key by X keysym.")]
    async fn key(&self, Parameters(p): Parameters<Key>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.key(p.keysym).map(|_| ok()).map_err(to_err)).await
    }

    // ── Window control (EWMH) ────────────────────────────────────────────────
    #[tool(description = "List managed top-level windows with titles and geometry.")]
    async fn list_windows(&self) -> Result<Json<WindowList>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            let windows = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .map(|w| WindowOut {
                    id: w.id,
                    title: w.title,
                    x: w.x.into(),
                    y: w.y.into(),
                    width: w.width.into(),
                    height: w.height.into(),
                })
                .collect();
            Ok(Json(WindowList { windows }))
        })
        .await
    }

    #[tool(description = "Focus (activate + raise) a window by id.")]
    async fn focus(&self, Parameters(p): Parameters<Focus>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.focus(p.window).map(|_| ok()).map_err(to_err)
        })
        .await
    }

    #[tool(name = "move_resize", description = "Move and resize a window.")]
    async fn move_resize(
        &self,
        Parameters(p): Parameters<MoveResize>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.move_resize(p.window, p.x, p.y, p.width, p.height)
                .map(|_| ok())
                .map_err(to_err)
        })
        .await
    }

    #[tool(description = "Snap a window to a half/full slot (left/right/top/bottom/full).")]
    async fn tile(&self, Parameters(p): Parameters<Tile>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.tile(p.window, p.slot.into())
                .map(|_| ok())
                .map_err(to_err)
        })
        .await
    }

    // ── Capture (XShm) ───────────────────────────────────────────────────────
    #[tool(description = "Capture the screen and return it as a PNG image.")]
    async fn screenshot(&self) -> Result<CallToolResult, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let mut cap = x.capture().map_err(to_err)?;
            let frame = cap.frame().map_err(to_err)?;
            let png = encode_png(
                frame.bytes(),
                frame.width,
                frame.height,
                frame.bytes_per_pixel,
            )
            .map_err(to_err)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            Ok(CallToolResult::success(vec![Content::image(
                b64,
                "image/png",
            )]))
        })
        .await
    }
}

/// Run blocking X work off the async reactor.
async fn run<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ErrorData> + Send + 'static,
) -> Result<T, ErrorData> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
}

fn to_err<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Encode an X `Z_PIXMAP` frame (little-endian B,G,R,X) as an RGBA PNG.
fn encode_png(src: &[u8], width: u16, height: u16, bpp: u8) -> Result<Vec<u8>, String> {
    let (w, h, bpp) = (width as usize, height as usize, bpp as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in src.chunks_exact(bpp) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs MUST go to stderr — stdout is the MCP transport.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let x = X::connect()?;
    tracing::info!("ai-mcp: connected to X, serving MCP over stdio");
    let service = WmCtl { x: Arc::new(x) }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
