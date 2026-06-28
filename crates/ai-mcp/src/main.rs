//! `ai-mcp` — the heart of wmaker-ai (PLAN §5, Layer 3).
//!
//! A model-agnostic MCP server exposing computer-use tools over existing X11
//! extensions — a broker + capture engine, not an ML runtime. Input synthesis
//! and capture go through `wmng-x11` (XTEST/XShm); window control through
//! `wmng-ewmh` (`_NET_*`). Any MCP client connects over stdio and drives a real
//! Window Maker desktop; the WM never learns it is being driven.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ai_proto::{DiffConfig, DiffEncoder, ScreenUpdate};
use base64::Engine as _;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use wmng_ewmh::{Ewmh, TileSlot};
use wmng_x11::{DamageFeed, X};

/// The MCP server: holds the shared X connection. Cheap to clone (Arc).
#[derive(Clone)]
struct WmCtl {
    x: Arc<X>,
    diff: Arc<Mutex<DiffEncoder>>,
    damage: Arc<Mutex<DamageFeed>>,
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

#[derive(Serialize, JsonSchema)]
struct RegionOut {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    png_base64: String,
}

#[derive(Serialize, JsonSchema)]
struct ScreenUpdateOut {
    kind: String,
    width: u16,
    height: u16,
    dirty_area: u32,
    rebaseline_reason: Option<String>,
    regions: Vec<RegionOut>,
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
            let png = ai_proto::encode_full_png(&frame).map_err(to_err)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            Ok(CallToolResult::success(vec![Content::image(
                b64,
                "image/png",
            )]))
        })
        .await
    }

    #[tool(
        name = "changed_regions",
        description = "Return XDamage dirty rectangles as PNG crops, with keyframe re-baseline when needed."
    )]
    async fn changed_regions(&self) -> Result<Json<ScreenUpdateOut>, ErrorData> {
        let x = self.x.clone();
        let diff = self.diff.clone();
        let damage = self.damage.clone();
        run(move || {
            let mut cap = x.capture().map_err(to_err)?;
            let dirty = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
            let frame = cap.frame().map_err(to_err)?;
            let update = diff
                .lock()
                .map_err(lock_err)?
                .changed_regions(&frame, &dirty)
                .map_err(to_err)?;
            Ok(Json(to_screen_update_out(update)))
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

fn lock_err<T>(e: PoisonError<T>) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

fn to_screen_update_out(update: ScreenUpdate) -> ScreenUpdateOut {
    ScreenUpdateOut {
        kind: format!("{:?}", update.kind).to_ascii_lowercase(),
        width: update.width,
        height: update.height,
        dirty_area: update.dirty_area,
        rebaseline_reason: update
            .rebaseline_reason
            .map(|r| format!("{r:?}").to_ascii_lowercase()),
        regions: update
            .regions
            .into_iter()
            .map(|r| RegionOut {
                x: r.rect.x,
                y: r.rect.y,
                width: r.rect.width,
                height: r.rect.height,
                png_base64: base64::engine::general_purpose::STANDARD.encode(r.png),
            })
            .collect(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("ai-mcp {}", version());
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--check") {
        check_runtime()?;
        return Ok(());
    }

    // Logs MUST go to stderr — stdout is the MCP transport.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let x = Arc::new(X::connect()?);
    let damage = Arc::new(Mutex::new(x.damage_feed()?));
    tracing::info!("ai-mcp: connected to X, serving MCP over stdio");
    let service = WmCtl {
        x,
        diff: Arc::new(Mutex::new(DiffEncoder::new(diff_config_from_env()))),
        damage,
    }
    .serve(stdio())
    .await?;
    service.waiting().await?;
    Ok(())
}

fn version() -> &'static str {
    option_env!("WMAKER_NG_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn check_runtime() -> anyhow::Result<()> {
    let x = Arc::new(X::connect()?);
    let (width, height) = x.dimensions();
    let mut cap = x.capture()?;
    let frame = cap.frame()?;
    let _damage = x.damage_feed()?;
    println!(
        "ai-mcp check ok display={} size={}x{} depth={} bpp={} shm={}",
        std::env::var("DISPLAY").unwrap_or_else(|_| "<unset>".to_string()),
        width,
        height,
        frame.depth,
        frame.bytes_per_pixel,
        x.shm_available()
    );
    Ok(())
}

fn diff_config_from_env() -> DiffConfig {
    let mut config = DiffConfig::default();
    if let Some(ms) = std::env::var("WMAKER_AI_KEYFRAME_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        config.keyframe_interval = Duration::from_millis(ms);
    }
    if let Some(ratio) = std::env::var("WMAKER_AI_MAX_DIRTY_RATIO")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        config.max_dirty_ratio = ratio.clamp(0.01, 1.0);
    }
    if let Some(max_regions) = std::env::var("WMAKER_AI_MAX_DIRTY_REGIONS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        config.max_regions = max_regions.max(1);
    }
    config
}
