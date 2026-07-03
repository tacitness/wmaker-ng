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
struct AccessibilityTree {
    /// Maximum depth from the AT-SPI root. Defaults to 2 and is capped at 4.
    max_depth: Option<u8>,
    /// Maximum children sampled from each node. Defaults to 16 and is capped at 64.
    max_children_per_node: Option<u8>,
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
    class: Option<String>,
    instance: Option<String>,
    pid: Option<u32>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    workspace: Option<u32>,
    mapped: bool,
    minimized: bool,
    maximized: bool,
}

#[derive(Serialize, JsonSchema)]
struct WindowList {
    windows: Vec<WindowOut>,
}

#[derive(Clone, Serialize, JsonSchema)]
struct RectOut {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Serialize, JsonSchema)]
struct SceneWindowOut {
    id: u32,
    title: String,
    class: Option<String>,
    instance: Option<String>,
    pid: Option<u32>,
    geometry: RectOut,
    workspace: Option<u32>,
    mapped: bool,
    minimized: bool,
    maximized: bool,
    active: bool,
    under_pointer: bool,
    stacking_index: Option<usize>,
    transient_for: Option<u32>,
    group_leader: Option<u32>,
    capabilities: WindowCapabilitiesOut,
}

#[derive(Serialize, JsonSchema)]
struct WindowCapabilitiesOut {
    ewmh_focus: bool,
    ewmh_move_resize: bool,
    ewmh_close: bool,
    xtest_input: bool,
    semantic_adapter: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct FocusOut {
    active_window: Option<u32>,
    pointer: PointerOut,
}

#[derive(Serialize, JsonSchema)]
struct DesktopSceneOut {
    screen: RectOut,
    focus: FocusOut,
    stacking_order: Vec<u32>,
    windows: Vec<SceneWindowOut>,
}

#[derive(Serialize, JsonSchema)]
struct ObservationOut {
    preferred_model_lane: bool,
    summary: String,
    focused_window: Option<ObservationWindowOut>,
    actionable_windows: Vec<ObservationWindowOut>,
    recent_changes: RecentChangesOut,
    semantic_adapters: Vec<String>,
    vision_fallback_policy: VisionFallbackPolicyOut,
    pixel_fallbacks: PixelFallbacksOut,
}

#[derive(Clone, Serialize, JsonSchema)]
struct ObservationWindowOut {
    handle: u32,
    title: String,
    app: Option<String>,
    geometry: RectOut,
    workspace: Option<u32>,
    active: bool,
    actions: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct RecentChangesOut {
    available: bool,
    dirty_rect_count: usize,
    dirty_area: u32,
    rects: Vec<RectOut>,
}

#[derive(Serialize, JsonSchema)]
struct PixelFallbacksOut {
    embedded_pixels: bool,
    full_screenshot_tool: String,
    dirty_png_delta_tool: String,
    fast_delta_tool: String,
    local_crop_reference: String,
}

#[derive(Serialize, JsonSchema)]
struct VisionFallbackPolicyOut {
    policy_version: String,
    default_lane: String,
    pixels_required: bool,
    recommended_next: String,
    crop_target: Option<CropTargetOut>,
    escalation_order: Vec<String>,
    request_pixels_when: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct CropTargetOut {
    kind: String,
    handle: Option<u32>,
    geometry: RectOut,
    reason: String,
}

#[derive(Serialize, JsonSchema)]
struct AccessibilityTreeOut {
    available: bool,
    max_depth: u8,
    max_children_per_node: u8,
    node_count: usize,
    nodes: Vec<AccessibilityNodeOut>,
    errors: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct AccessibilityNodeOut {
    id: usize,
    parent: Option<usize>,
    bus_name: String,
    object_path: String,
    depth: u8,
    name: Option<String>,
    role_name: Option<String>,
    description: Option<String>,
    child_count: u32,
    extents: Option<AccessibilityExtentsOut>,
    errors: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
struct AccessibilityExtentsOut {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
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
                    class: w.class,
                    instance: w.instance,
                    pid: w.pid,
                    x: w.x.into(),
                    y: w.y.into(),
                    width: w.width.into(),
                    height: w.height.into(),
                    workspace: w.workspace,
                    mapped: w.mapped,
                    minimized: w.minimized,
                    maximized: w.maximized,
                })
                .collect();
            Ok(Json(WindowList { windows }))
        })
        .await
    }

    #[tool(
        description = "Return a structured desktop scene graph of windows, focus, pointer, stacking order, and control capabilities without screenshot pixels."
    )]
    async fn desktop_scene(&self) -> Result<Json<DesktopSceneOut>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            let active = ewmh.active_window().map_err(to_err)?;
            let pointer = x.pointer().map_err(to_err)?;
            let stacking_order = ewmh.stacking_windows().map_err(to_err)?;
            let windows = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .map(|w| {
                    let stacking_index = stacking_order.iter().position(|id| *id == w.id);
                    SceneWindowOut {
                        id: w.id,
                        title: w.title,
                        class: w.class,
                        instance: w.instance,
                        pid: w.pid,
                        geometry: RectOut {
                            x: w.x.into(),
                            y: w.y.into(),
                            width: w.width.into(),
                            height: w.height.into(),
                        },
                        workspace: w.workspace,
                        mapped: w.mapped,
                        minimized: w.minimized,
                        maximized: w.maximized,
                        active: active.is_some_and(|id| id == w.id),
                        under_pointer: pointer.child.is_some_and(|id| id == w.id),
                        stacking_index,
                        transient_for: w.transient_for,
                        group_leader: w.group_leader,
                        capabilities: WindowCapabilitiesOut {
                            ewmh_focus: true,
                            ewmh_move_resize: true,
                            ewmh_close: true,
                            xtest_input: true,
                            semantic_adapter: None,
                        },
                    }
                })
                .collect();
            let (width, height) = x.dimensions();
            Ok(Json(DesktopSceneOut {
                screen: RectOut {
                    x: 0,
                    y: 0,
                    width: width.into(),
                    height: height.into(),
                },
                focus: FocusOut {
                    active_window: active,
                    pointer: PointerOut {
                        x: pointer.x,
                        y: pointer.y,
                        window: pointer.child,
                    },
                },
                stacking_order,
                windows,
            }))
        })
        .await
    }

    #[tool(
        description = "Preferred model-facing observation lane: compact text/JSON desktop state with focus, actionable windows, and damage metadata; embeds no screenshot pixels."
    )]
    async fn observe(&self) -> Result<Json<ObservationOut>, ErrorData> {
        let x = self.x.clone();
        let damage = self.damage.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            let active = ewmh.active_window().map_err(to_err)?;
            let pointer = x.pointer().map_err(to_err)?;
            let dirty = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
            let dirty_area = dirty.iter().fold(0u32, |total, rect| {
                total.saturating_add(u32::from(rect.width) * u32::from(rect.height))
            });
            let mut windows = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .filter(|w| w.mapped && !w.minimized)
                .map(|w| ObservationWindowOut {
                    handle: w.id,
                    title: w.title,
                    app: w.class.or(w.instance),
                    geometry: RectOut {
                        x: w.x.into(),
                        y: w.y.into(),
                        width: w.width.into(),
                        height: w.height.into(),
                    },
                    workspace: w.workspace,
                    active: active.is_some_and(|id| id == w.id),
                    actions: vec![
                        "focus".to_string(),
                        "move_resize".to_string(),
                        "close_window".to_string(),
                    ],
                })
                .collect::<Vec<_>>();
            windows.sort_by_key(|w| (!w.active, w.handle));
            let focused_window = windows.iter().find(|w| w.active).cloned();
            let pointer_summary = pointer
                .child
                .map(|id| format!(" pointer over 0x{id:x}."))
                .unwrap_or_default();
            let summary = match &focused_window {
                Some(w) if w.title.is_empty() => {
                    format!("Focused window 0x{:x}.{}", w.handle, pointer_summary)
                }
                Some(w) => format!(
                    "Focused window 0x{:x}: {}.{}",
                    w.handle, w.title, pointer_summary
                ),
                None => format!(
                    "No active window. {} visible/actionable windows.{}",
                    windows.len(),
                    pointer_summary
                ),
            };
            let vision_fallback_policy =
                vision_fallback_policy_for(active, pointer.child, &windows);
            Ok(Json(ObservationOut {
                preferred_model_lane: true,
                summary,
                focused_window,
                actionable_windows: windows,
                recent_changes: RecentChangesOut {
                    available: true,
                    dirty_rect_count: dirty.len(),
                    dirty_area,
                    rects: dirty
                        .into_iter()
                        .map(|r| RectOut {
                            x: r.x.into(),
                            y: r.y.into(),
                            width: r.width.into(),
                            height: r.height.into(),
                        })
                        .collect(),
                },
                semantic_adapters: Vec::new(),
                vision_fallback_policy,
                pixel_fallbacks: PixelFallbacksOut {
                    embedded_pixels: false,
                    full_screenshot_tool: "screenshot".to_string(),
                    dirty_png_delta_tool: "changed_regions".to_string(),
                    fast_delta_tool: "changed_regions_fast".to_string(),
                    local_crop_reference:
                        "future framebuffer crop tools; no pixels embedded by observe".to_string(),
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Spike semantic UI observation through AT-SPI. Returns a bounded accessibility tree when the desktop/app exposes one; embeds no pixels."
    )]
    async fn accessibility_tree(
        &self,
        Parameters(p): Parameters<AccessibilityTree>,
    ) -> Result<Json<AccessibilityTreeOut>, ErrorData> {
        let max_depth = p.max_depth.unwrap_or(2).clamp(1, 4);
        let max_children_per_node = p.max_children_per_node.unwrap_or(16).clamp(1, 64);
        let atspi = wmng_dbus::AtSpi::connect_from_session()
            .await
            .map_err(to_err)?;
        let snapshot = atspi.snapshot(max_depth, max_children_per_node).await;
        Ok(Json(to_accessibility_tree_out(snapshot)))
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

fn to_accessibility_tree_out(snapshot: wmng_dbus::AccessibilitySnapshot) -> AccessibilityTreeOut {
    let nodes = snapshot
        .nodes
        .into_iter()
        .map(|node| AccessibilityNodeOut {
            id: node.id,
            parent: node.parent,
            bus_name: node.bus_name,
            object_path: node.object_path,
            depth: node.depth,
            name: node.name,
            role_name: node.role_name,
            description: node.description,
            child_count: node.child_count,
            extents: node.extents.map(|extents| AccessibilityExtentsOut {
                x: extents.x,
                y: extents.y,
                width: extents.width,
                height: extents.height,
            }),
            errors: node.errors,
        })
        .collect::<Vec<_>>();
    AccessibilityTreeOut {
        available: snapshot.available,
        max_depth: snapshot.max_depth,
        max_children_per_node: snapshot.max_children_per_node,
        node_count: nodes.len(),
        nodes,
        errors: snapshot.errors,
    }
}

fn vision_fallback_policy_for(
    active_window: Option<u32>,
    pointer_window: Option<u32>,
    windows: &[ObservationWindowOut],
) -> VisionFallbackPolicyOut {
    let crop_source = active_window
        .and_then(|id| windows.iter().find(|window| window.handle == id))
        .or_else(|| pointer_window.and_then(|id| windows.iter().find(|window| window.handle == id)))
        .or_else(|| windows.first());
    let crop_target = crop_source.map(|window| CropTargetOut {
        kind: "window".to_string(),
        handle: Some(window.handle),
        geometry: window.geometry.clone(),
        reason: if window.active {
            "focused window is the narrowest useful visual fallback target".to_string()
        } else {
            "no focused window; use the best actionable window before full-screen pixels"
                .to_string()
        },
    });

    VisionFallbackPolicyOut {
        policy_version: "2026-07-03.1".to_string(),
        default_lane: "structured_observe".to_string(),
        pixels_required: false,
        recommended_next: "act_from_structured_state".to_string(),
        crop_target,
        escalation_order: vec![
            "observe".to_string(),
            "desktop_scene".to_string(),
            "accessibility_tree".to_string(),
            "browser_semantic_adapter_when_connected".to_string(),
            "focused_window_crop_when_available".to_string(),
            "changed_regions".to_string(),
            "screenshot".to_string(),
        ],
        request_pixels_when: vec![
            "the target control or content is visually ambiguous after structured observation"
                .to_string(),
            "the agent must inspect canvas/image/video or other non-semantic pixels".to_string(),
            "recent damage overlaps the intended action target and semantic state is stale"
                .to_string(),
            "an action failed and the next recovery step requires visual confirmation".to_string(),
            "accessibility/browser semantic adapters are unavailable or report low coverage"
                .to_string(),
        ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_scene_serializes_object_first_contract() {
        let scene = DesktopSceneOut {
            screen: RectOut {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
            focus: FocusOut {
                active_window: Some(42),
                pointer: PointerOut {
                    x: 10,
                    y: 20,
                    window: Some(42),
                },
            },
            stacking_order: vec![7, 42],
            windows: vec![SceneWindowOut {
                id: 42,
                title: "Example".to_string(),
                class: Some("Firefox".to_string()),
                instance: Some("Navigator".to_string()),
                pid: Some(1234),
                geometry: RectOut {
                    x: 100,
                    y: 80,
                    width: 800,
                    height: 600,
                },
                workspace: Some(0),
                mapped: true,
                minimized: false,
                maximized: false,
                active: true,
                under_pointer: true,
                stacking_index: Some(1),
                transient_for: None,
                group_leader: Some(42),
                capabilities: WindowCapabilitiesOut {
                    ewmh_focus: true,
                    ewmh_move_resize: true,
                    ewmh_close: true,
                    xtest_input: true,
                    semantic_adapter: None,
                },
            }],
        };

        let json = serde_json::to_value(&scene).expect("scene graph serializes");
        assert_eq!(json["screen"]["width"], 1280);
        assert_eq!(json["focus"]["active_window"], 42);
        assert_eq!(json["windows"][0]["class"], "Firefox");
        assert_eq!(json["windows"][0]["geometry"]["x"], 100);
        assert_eq!(json["windows"][0]["capabilities"]["xtest_input"], true);
    }

    #[test]
    fn observation_serializes_text_first_contract() {
        let focused = ObservationWindowOut {
            handle: 42,
            title: "Example".to_string(),
            app: Some("Firefox".to_string()),
            geometry: RectOut {
                x: 100,
                y: 80,
                width: 800,
                height: 600,
            },
            workspace: Some(0),
            active: true,
            actions: vec!["focus".to_string(), "move_resize".to_string()],
        };
        let observation = ObservationOut {
            preferred_model_lane: true,
            summary: "Focused window 0x2a: Example.".to_string(),
            focused_window: Some(focused.clone()),
            actionable_windows: Vec::new(),
            recent_changes: RecentChangesOut {
                available: true,
                dirty_rect_count: 1,
                dirty_area: 400,
                rects: vec![RectOut {
                    x: 10,
                    y: 20,
                    width: 20,
                    height: 20,
                }],
            },
            semantic_adapters: Vec::new(),
            vision_fallback_policy: vision_fallback_policy_for(Some(42), Some(42), &[focused]),
            pixel_fallbacks: PixelFallbacksOut {
                embedded_pixels: false,
                full_screenshot_tool: "screenshot".to_string(),
                dirty_png_delta_tool: "changed_regions".to_string(),
                fast_delta_tool: "changed_regions_fast".to_string(),
                local_crop_reference: "framebuffer crop".to_string(),
            },
        };

        let json = serde_json::to_value(&observation).expect("observation serializes");
        assert_eq!(json["preferred_model_lane"], true);
        assert_eq!(json["focused_window"]["handle"], 42);
        assert_eq!(json["pixel_fallbacks"]["embedded_pixels"], false);
        assert_eq!(json["vision_fallback_policy"]["pixels_required"], false);
        assert_eq!(json["vision_fallback_policy"]["crop_target"]["handle"], 42);
        assert!(json["pixel_fallbacks"]["full_screenshot_tool"].is_string());
    }

    #[test]
    fn accessibility_tree_serializes_semantic_nodes_without_pixels() {
        let snapshot = wmng_dbus::AccessibilitySnapshot {
            available: true,
            bus_address: Some("unix:path=/tmp/atspi".to_string()),
            max_depth: 2,
            max_children_per_node: 16,
            nodes: vec![wmng_dbus::AccessibilityNode {
                id: 0,
                parent: None,
                bus_name: "org.a11y.atspi.Registry".to_string(),
                object_path: "/org/a11y/atspi/accessible/root".to_string(),
                depth: 0,
                name: Some("desktop".to_string()),
                role_name: Some("application".to_string()),
                description: None,
                child_count: 1,
                extents: Some(wmng_dbus::AccessibilityExtents {
                    x: 10,
                    y: 20,
                    width: 640,
                    height: 480,
                }),
                errors: Vec::new(),
            }],
            errors: Vec::new(),
        };

        let json = serde_json::to_value(to_accessibility_tree_out(snapshot))
            .expect("accessibility tree serializes");
        assert_eq!(json["available"], true);
        assert_eq!(json["node_count"], 1);
        assert_eq!(json["nodes"][0]["role_name"], "application");
        assert_eq!(json["nodes"][0]["extents"]["width"], 640);
        assert!(json.get("pixels").is_none());
    }
}
