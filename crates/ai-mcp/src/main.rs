//! `ai-mcp` — the heart of wmaker-ai (PLAN §5, Layer 3).
//!
//! A model-agnostic MCP server exposing computer-use tools over existing X11
//! extensions — a broker + capture engine, not an ML runtime. Input synthesis
//! and capture go through `wmng-x11` (XTEST/XShm); window control through
//! `wmng-ewmh` (`_NET_*`). Any MCP client connects over stdio and drives a real
//! Window Maker desktop; the WM never learns it is being driven.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ai_proto::{DiffConfig, DiffEncoder, ScreenUpdate};
use base64::Engine as _;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use wmng_ewmh::{Ewmh, TileSlot};
use wmng_x11::{DamageFeed, SharedCapture, X};

/// The MCP server: holds the shared X connection. Cheap to clone (Arc).
#[derive(Clone)]
struct WmCtl {
    x: Arc<X>,
    capture: Arc<Mutex<SharedCapture>>,
    diff: Arc<Mutex<DiffEncoder>>,
    damage: Arc<Mutex<DamageFeed>>,
    clipboard: Arc<Mutex<Option<arboard::Clipboard>>>,
}

// ── Tool parameter / output schemas (auto-generate the MCP contract) ─────────

#[derive(Deserialize, JsonSchema)]
struct MoveMouse {
    x: i16,
    y: i16,
}

#[derive(Deserialize, JsonSchema)]
struct Click {
    /// Optional x coordinate; when present with y, click happens at that point.
    x: Option<i16>,
    /// Optional y coordinate; when present with x, click happens at that point.
    y: Option<i16>,
    /// Pointer button: 1=left, 2=middle, 3=right.
    #[serde(default = "default_button")]
    button: u8,
    /// Number of clicks to synthesize.
    #[serde(default = "default_click_count")]
    count: u8,
}
fn default_button() -> u8 {
    1
}
fn default_click_count() -> u8 {
    1
}

#[derive(Deserialize, JsonSchema)]
struct Scroll {
    /// Horizontal wheel steps. Positive scrolls right, negative left.
    #[serde(default)]
    dx: i16,
    /// Vertical wheel steps. Positive scrolls down, negative up.
    #[serde(default)]
    dy: i16,
}

#[derive(Deserialize, JsonSchema)]
struct Drag {
    x1: i16,
    y1: i16,
    x2: i16,
    y2: i16,
    /// Pointer button: 1=left, 2=middle, 3=right.
    #[serde(default = "default_button")]
    button: u8,
}

#[derive(Deserialize, JsonSchema)]
struct TypeText {
    text: String,
}

#[derive(Deserialize, JsonSchema)]
struct Key {
    /// X keysym (e.g. 0xff0d = Return). Either `keysym` or `key` is required.
    keysym: Option<u32>,
    /// Friendly key name (Return, Tab, Escape, Page_Down) or a single character.
    key: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct KeyCombo {
    /// Ordered key names, e.g. ["ctrl", "l"] or ["ctrl", "shift", "r"].
    keys: Option<Vec<String>>,
    /// Final key when using the explicit form.
    key: Option<String>,
    /// Final X keysym when using the explicit form.
    keysym: Option<u32>,
    /// Modifiers for the explicit form: ctrl, alt, shift, super.
    #[serde(default)]
    modifiers: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct Focus {
    window: u32,
}

#[derive(Deserialize, JsonSchema)]
struct WindowRef {
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
struct SetClipboard {
    text: String,
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
struct PointerOut {
    x: i16,
    y: i16,
    window: Option<u32>,
}

#[derive(Serialize, JsonSchema)]
struct ClipboardOut {
    text: String,
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

#[derive(Serialize, JsonSchema)]
struct TimingOut {
    capture_ms: u128,
    damage_ms: u128,
    encode_ms: u128,
    serialize_ms: u128,
    total_ms: u128,
}

#[derive(Serialize, JsonSchema)]
struct RawRegionOut {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    stride: u32,
    encoding: String,
    data_base64: String,
}

#[derive(Serialize, JsonSchema)]
struct ScreenUpdateFastOut {
    kind: String,
    width: u16,
    height: u16,
    bytes_per_pixel: u8,
    pixel_format: String,
    dirty_area: u32,
    rebaseline_reason: Option<String>,
    needs_keyframe: bool,
    encoded_bytes: usize,
    raw_bytes: usize,
    timings: TimingOut,
    regions: Vec<RawRegionOut>,
}

#[derive(Deserialize, JsonSchema)]
struct WaitForIdle {
    /// Required quiet period before returning idle.
    quiet_ms: u64,
    /// Maximum time to wait.
    timeout_ms: u64,
}

#[derive(Serialize, JsonSchema)]
struct WaitForIdleOut {
    idle: bool,
    elapsed_ms: u128,
    damage_events: usize,
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

    #[tool(description = "Click a pointer button, optionally at absolute root coordinates.")]
    async fn click(&self, Parameters(p): Parameters<Click>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            match (p.x, p.y) {
                (Some(px), Some(py)) => x.click_at(px, py, p.button, p.count),
                _ => {
                    for _ in 0..p.count.max(1) {
                        x.click(p.button).map_err(to_err)?;
                    }
                    Ok(())
                }
            }
            .map(|_| ok())
            .map_err(to_err)
        })
        .await
    }

    #[tool(description = "Scroll with XTEST wheel buttons. Positive dy scrolls down.")]
    async fn scroll(&self, Parameters(p): Parameters<Scroll>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.scroll(p.dx, p.dy).map(|_| ok()).map_err(to_err)).await
    }

    #[tool(description = "Drag from one absolute root coordinate to another.")]
    async fn drag(&self, Parameters(p): Parameters<Drag>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            x.drag(p.x1, p.y1, p.x2, p.y2, p.button)
                .map(|_| ok())
                .map_err(to_err)
        })
        .await
    }

    #[tool(name = "type", description = "Type a string of text.")]
    async fn type_text(
        &self,
        Parameters(p): Parameters<TypeText>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || x.type_text(&p.text).map(|_| ok()).map_err(to_err)).await
    }

    #[tool(description = "Tap a key by X keysym or friendly key name.")]
    async fn key(&self, Parameters(p): Parameters<Key>) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let keysym = resolve_key(p.keysym, p.key.as_deref())?;
            x.key(keysym).map(|_| ok()).map_err(to_err)
        })
        .await
    }

    #[tool(description = "Tap a key chord such as Ctrl+L, Ctrl+T, Alt+Tab, or Ctrl+Shift+R.")]
    async fn key_combo(
        &self,
        Parameters(p): Parameters<KeyCombo>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let (modifiers, key) = resolve_combo(p)?;
            x.key_combo(&modifiers, key).map(|_| ok()).map_err(to_err)
        })
        .await
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

    #[tool(description = "Close a window via _NET_CLOSE_WINDOW.")]
    async fn close_window(
        &self,
        Parameters(p): Parameters<WindowRef>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.close(p.window).map(|_| ok()).map_err(to_err)
        })
        .await
    }

    #[tool(description = "Minimize/iconify a window.")]
    async fn minimize(
        &self,
        Parameters(p): Parameters<WindowRef>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.minimize(p.window).map(|_| ok()).map_err(to_err)
        })
        .await
    }

    #[tool(description = "Maximize a window horizontally and vertically.")]
    async fn maximize(
        &self,
        Parameters(p): Parameters<WindowRef>,
    ) -> Result<Json<Status>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            ewmh.maximize(p.window).map(|_| ok()).map_err(to_err)
        })
        .await
    }

    #[tool(description = "Return pointer coordinates and the window under the cursor.")]
    async fn pointer(&self) -> Result<Json<PointerOut>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let p = x.pointer().map_err(to_err)?;
            Ok(Json(PointerOut {
                x: p.x,
                y: p.y,
                window: p.child,
            }))
        })
        .await
    }

    #[tool(description = "Set the desktop clipboard text.")]
    async fn set_clipboard(
        &self,
        Parameters(p): Parameters<SetClipboard>,
    ) -> Result<Json<Status>, ErrorData> {
        let clipboard = self.clipboard.clone();
        run(move || {
            let mut guard = clipboard.lock().map_err(lock_err)?;
            if guard.is_none() {
                *guard = Some(arboard::Clipboard::new().map_err(to_err)?);
            }
            guard
                .as_mut()
                .expect("clipboard initialized")
                .set_text(p.text)
                .map_err(to_err)?;
            Ok(ok())
        })
        .await
    }

    #[tool(description = "Get the desktop clipboard text.")]
    async fn get_clipboard(&self) -> Result<Json<ClipboardOut>, ErrorData> {
        let clipboard = self.clipboard.clone();
        run(move || {
            let mut guard = clipboard.lock().map_err(lock_err)?;
            if guard.is_none() {
                *guard = Some(arboard::Clipboard::new().map_err(to_err)?);
            }
            let text = guard
                .as_mut()
                .expect("clipboard initialized")
                .get_text()
                .map_err(to_err)?;
            Ok(Json(ClipboardOut { text }))
        })
        .await
    }

    // ── Capture (XShm) ───────────────────────────────────────────────────────
    #[tool(description = "Capture the screen and return it as a PNG image.")]
    async fn screenshot(&self) -> Result<CallToolResult, ErrorData> {
        let capture = self.capture.clone();
        let diff = self.diff.clone();
        run(move || {
            let mut cap = capture.lock().map_err(lock_err)?;
            let frame = cap.frame().map_err(to_err)?;
            let png = ai_proto::encode_full_png(&frame).map_err(to_err)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            diff.lock().map_err(lock_err)?.note_keyframe();
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
        let capture = self.capture.clone();
        let diff = self.diff.clone();
        let damage = self.damage.clone();
        run(move || {
            let dirty = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
            let mut cap = capture.lock().map_err(lock_err)?;
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

    #[tool(
        name = "changed_regions_fast",
        description = "Return XDamage dirty rectangles as raw X11 ZPixmap crops, avoiding PNG compression for low-latency local observation."
    )]
    async fn changed_regions_fast(&self) -> Result<Json<ScreenUpdateFastOut>, ErrorData> {
        let capture = self.capture.clone();
        let diff = self.diff.clone();
        let damage = self.damage.clone();
        run(move || {
            let total_start = Instant::now();

            let capture_start = Instant::now();
            let mut cap = capture.lock().map_err(lock_err)?;
            let frame = cap.frame().map_err(to_err)?;
            let capture_ms = capture_start.elapsed().as_millis();

            let damage_start = Instant::now();
            let dirty = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
            let damage_ms = damage_start.elapsed().as_millis();

            let encode_start = Instant::now();
            let update = diff
                .lock()
                .map_err(lock_err)?
                .changed_regions_raw(&frame, &dirty)
                .map_err(to_err)?;
            let encode_ms = encode_start.elapsed().as_millis();

            let serialize_start = Instant::now();
            let out = to_screen_update_fast_out(
                update,
                TimingOut {
                    capture_ms,
                    damage_ms,
                    encode_ms,
                    serialize_ms: 0,
                    total_ms: 0,
                },
            );
            let serialize_ms = serialize_start.elapsed().as_millis();
            let total_ms = total_start.elapsed().as_millis();

            Ok(Json(ScreenUpdateFastOut {
                timings: TimingOut {
                    capture_ms,
                    damage_ms,
                    encode_ms,
                    serialize_ms,
                    total_ms,
                },
                ..out
            }))
        })
        .await
    }

    #[tool(description = "Wait until XDamage has been quiet for quiet_ms, or timeout_ms expires.")]
    async fn wait_for_idle(
        &self,
        Parameters(p): Parameters<WaitForIdle>,
    ) -> Result<Json<WaitForIdleOut>, ErrorData> {
        let damage = self.damage.clone();
        run(move || {
            let quiet = Duration::from_millis(p.quiet_ms.max(1));
            let timeout = Duration::from_millis(p.timeout_ms.max(p.quiet_ms).max(1));
            let start = Instant::now();
            let mut quiet_since = Instant::now();
            let mut events = 0usize;
            while start.elapsed() < timeout {
                let rects = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
                if rects.is_empty() {
                    if quiet_since.elapsed() >= quiet {
                        return Ok(Json(WaitForIdleOut {
                            idle: true,
                            elapsed_ms: start.elapsed().as_millis(),
                            damage_events: events,
                        }));
                    }
                } else {
                    events += rects.len();
                    quiet_since = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(Json(WaitForIdleOut {
                idle: false,
                elapsed_ms: start.elapsed().as_millis(),
                damage_events: events,
            }))
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

fn resolve_combo(p: KeyCombo) -> Result<(Vec<u32>, u32), ErrorData> {
    if let Some(keys) = p.keys {
        let mut parts = keys.iter().map(String::as_str).collect::<Vec<_>>();
        let Some(key) = parts.pop() else {
            return Err(ErrorData::invalid_params(
                "key_combo keys cannot be empty",
                None,
            ));
        };
        let modifiers = parts
            .iter()
            .map(|name| modifier_keysym(name))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((modifiers, parse_key_name(key)?));
    }

    let modifiers = p
        .modifiers
        .iter()
        .map(|name| modifier_keysym(name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((modifiers, resolve_key(p.keysym, p.key.as_deref())?))
}

fn resolve_key(keysym: Option<u32>, key: Option<&str>) -> Result<u32, ErrorData> {
    match (keysym, key) {
        (Some(keysym), _) => Ok(keysym),
        (None, Some(key)) => parse_key_name(key),
        (None, None) => Err(ErrorData::invalid_params(
            "either keysym or key is required",
            None,
        )),
    }
}

fn modifier_keysym(name: &str) -> Result<u32, ErrorData> {
    match normalize_key_name(name).as_str() {
        "ctrl" | "control" | "control_l" => Ok(0xffe3),
        "alt" | "alt_l" => Ok(0xffe9),
        "shift" | "shift_l" => Ok(0xffe1),
        "super" | "super_l" | "meta" | "win" => Ok(0xffeb),
        other => Err(ErrorData::invalid_params(
            format!("unknown modifier: {other}"),
            None,
        )),
    }
}

fn parse_key_name(name: &str) -> Result<u32, ErrorData> {
    let trimmed = name.trim();
    let mut chars = trimmed.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Ok(ch as u32);
    }

    match normalize_key_name(trimmed).as_str() {
        "return" | "enter" => Ok(0xff0d),
        "tab" => Ok(0xff09),
        "escape" | "esc" => Ok(0xff1b),
        "backspace" => Ok(0xff08),
        "delete" | "del" => Ok(0xffff),
        "insert" | "ins" => Ok(0xff63),
        "space" => Ok(0x20),
        "page_down" | "pagedown" => Ok(0xff56),
        "page_up" | "pageup" => Ok(0xff55),
        "home" => Ok(0xff50),
        "end" => Ok(0xff57),
        "left" => Ok(0xff51),
        "up" => Ok(0xff52),
        "right" => Ok(0xff53),
        "down" => Ok(0xff54),
        "f1" => Ok(0xffbe),
        "f2" => Ok(0xffbf),
        "f3" => Ok(0xffc0),
        "f4" => Ok(0xffc1),
        "f5" => Ok(0xffc2),
        "f6" => Ok(0xffc3),
        "f7" => Ok(0xffc4),
        "f8" => Ok(0xffc5),
        "f9" => Ok(0xffc6),
        "f10" => Ok(0xffc7),
        "f11" => Ok(0xffc8),
        "f12" => Ok(0xffc9),
        other => Err(ErrorData::invalid_params(
            format!("unknown key name: {other}"),
            None,
        )),
    }
}

fn normalize_key_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
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

fn to_screen_update_fast_out(
    update: ai_proto::RawScreenUpdate,
    timings: TimingOut,
) -> ScreenUpdateFastOut {
    let mut raw_bytes = 0usize;
    let mut encoded_bytes = 0usize;
    let regions = update
        .regions
        .into_iter()
        .map(|r| {
            raw_bytes += r.bytes.len();
            let compressed = lz4_flex::compress_prepend_size(&r.bytes);
            let data_base64 = base64::engine::general_purpose::STANDARD.encode(compressed);
            encoded_bytes += data_base64.len();
            RawRegionOut {
                x: r.rect.x,
                y: r.rect.y,
                width: r.rect.width,
                height: r.rect.height,
                stride: r.stride,
                encoding: "lz4_flex_size_prepended_x11_zpixmap_native_bgrx".to_string(),
                data_base64,
            }
        })
        .collect();

    ScreenUpdateFastOut {
        kind: format!("{:?}", update.kind).to_ascii_lowercase(),
        width: update.width,
        height: update.height,
        bytes_per_pixel: update.bytes_per_pixel,
        pixel_format: "x11_zpixmap_native_bgrx".to_string(),
        dirty_area: update.dirty_area,
        rebaseline_reason: update
            .rebaseline_reason
            .map(|r| format!("{r:?}").to_ascii_lowercase()),
        needs_keyframe: update.needs_keyframe,
        encoded_bytes,
        raw_bytes,
        timings,
        regions,
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
    let capture = Arc::new(Mutex::new(x.shared_capture()?));
    let damage = Arc::new(Mutex::new(x.damage_feed()?));
    tracing::info!("ai-mcp: connected to X, serving MCP over stdio");
    let service = WmCtl {
        x,
        capture,
        diff: Arc::new(Mutex::new(DiffEncoder::new(diff_config_from_env()))),
        damage,
        clipboard: Arc::new(Mutex::new(None)),
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
