//! `ai-mcp` — the heart of wmaker-ai (PLAN §5, Layer 3).
//!
//! A model-agnostic MCP server exposing computer-use tools over existing X11
//! extensions — a broker + capture engine, not an ML runtime. Input synthesis
//! and capture go through `wmng-x11` (XTEST/XShm); window control through
//! `wmng-ewmh` (`_NET_*`). Any MCP client connects over stdio and drives a real
//! Window Maker desktop; the WM never learns it is being driven.

mod command;

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ai_proto::{DiffConfig, DiffEncoder, ScreenUpdate};
use base64::Engine as _;
use command::{
    AppSkillRegistry, CommandResult, CommandSource, PlannedAction, RouteCommandParams,
    RoutedCommand, SkillAcquisitionParams, SkillAcquisitionPlan, SkillRegistryParams,
};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use rmcp::transport::stdio;
use rmcp::{ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use wmng_ewmh::{Ewmh, TileSlot};
use wmng_x11::{DamageFeed, MonitorInfo, SharedCapture, X};

const BROWSER_ADAPTER_SCHEMA_VERSION: u16 = 1;
const DEFAULT_BROWSER_SOCKET_NAME: &str = "wmaker-ai/browser-adapter.sock";

/// The MCP server: holds the shared X connection. Cheap to clone (Arc).
#[derive(Clone)]
struct WmCtl {
    x: Arc<X>,
    capture: Arc<Mutex<SharedCapture>>,
    diff: Arc<Mutex<DiffEncoder>>,
    damage: Arc<Mutex<DamageFeed>>,
    clipboard: Arc<Mutex<Option<arboard::Clipboard>>>,
    browser_adapter: BrowserAdapterState,
}

#[derive(Clone, Default)]
struct BrowserAdapterState {
    latest: Arc<Mutex<Option<BrowserAdapterMessage>>>,
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

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
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
    output: Option<WindowOutputOut>,
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
    outputs: Vec<MonitorOut>,
    focus: FocusOut,
    stacking_order: Vec<u32>,
    windows: Vec<SceneWindowOut>,
}

#[derive(Serialize, JsonSchema)]
struct ObservationOut {
    preferred_model_lane: bool,
    summary: String,
    outputs: Vec<MonitorOut>,
    focused_window: Option<ObservationWindowOut>,
    actionable_windows: Vec<ObservationWindowOut>,
    browser: BrowserObservationOut,
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
    output: Option<WindowOutputOut>,
    workspace: Option<u32>,
    active: bool,
    actions: Vec<String>,
}

#[derive(Clone, Serialize, JsonSchema)]
struct MonitorOut {
    name: String,
    geometry: RectOut,
    width_mm: Option<u32>,
    height_mm: Option<u32>,
    primary: bool,
    automatic: bool,
    output_count: u16,
}

#[derive(Clone, Serialize, JsonSchema)]
struct WindowOutputOut {
    name: String,
    local_geometry: RectOut,
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

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
struct BrowserObservationOut {
    connected: bool,
    adapter: Option<String>,
    extension_id: Option<String>,
    tab_id: Option<i64>,
    url: Option<String>,
    title: Option<String>,
    controls: Vec<BrowserControlOut>,
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
struct BrowserControlOut {
    handle: String,
    role: String,
    label: String,
    enabled: bool,
    bounds: RectOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<bool>,
    redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_type: Option<String>,
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
    output: Option<WindowOutputOut>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BrowserAdapterMessage {
    schema_version: u16,
    kind: String,
    #[serde(default)]
    extension_id: Option<String>,
    #[serde(default)]
    tab_id: Option<i64>,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct BrowserAdapterReply {
    schema_version: u16,
    ok: bool,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
            let outputs = to_monitor_outs(x.monitors().map_err(to_err)?);
            let stacking_order = ewmh.stacking_windows().map_err(to_err)?;
            let windows = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .map(|w| {
                    let stacking_index = stacking_order.iter().position(|id| *id == w.id);
                    let geometry = RectOut {
                        x: w.x.into(),
                        y: w.y.into(),
                        width: w.width.into(),
                        height: w.height.into(),
                    };
                    let output = output_for_rect(&geometry, &outputs);
                    SceneWindowOut {
                        id: w.id,
                        title: w.title,
                        class: w.class,
                        instance: w.instance,
                        pid: w.pid,
                        geometry,
                        workspace: w.workspace,
                        mapped: w.mapped,
                        minimized: w.minimized,
                        maximized: w.maximized,
                        active: active.is_some_and(|id| id == w.id),
                        under_pointer: pointer.child.is_some_and(|id| id == w.id),
                        stacking_index,
                        transient_for: w.transient_for,
                        group_leader: w.group_leader,
                        output,
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
                outputs,
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
        let browser_adapter = self.browser_adapter.clone();
        run(move || {
            let ewmh = Ewmh::new(&x).map_err(to_err)?;
            let active = ewmh.active_window().map_err(to_err)?;
            let pointer = x.pointer().map_err(to_err)?;
            let outputs = to_monitor_outs(x.monitors().map_err(to_err)?);
            let browser_adapter_latest = browser_adapter.latest.lock().map_err(lock_err)?.clone();
            let dirty = damage.lock().map_err(lock_err)?.poll().map_err(to_err)?;
            let dirty_area = dirty.iter().fold(0u32, |total, rect| {
                total.saturating_add(u32::from(rect.width) * u32::from(rect.height))
            });
            let mut windows = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .filter(|w| w.mapped && !w.minimized)
                .map(|w| {
                    let geometry = RectOut {
                        x: w.x.into(),
                        y: w.y.into(),
                        width: w.width.into(),
                        height: w.height.into(),
                    };
                    ObservationWindowOut {
                        handle: w.id,
                        title: w.title,
                        app: w.class.or(w.instance),
                        output: output_for_rect(&geometry, &outputs),
                        geometry,
                        workspace: w.workspace,
                        active: active.is_some_and(|id| id == w.id),
                        actions: vec![
                            "focus".to_string(),
                            "move_resize".to_string(),
                            "close_window".to_string(),
                        ],
                    }
                })
                .collect::<Vec<_>>();
            windows.sort_by_key(|w| (!w.active, w.handle));
            let focused_window = windows.iter().find(|w| w.active).cloned();
            let pointer_summary = pointer
                .child
                .map(|id| format!(" pointer over 0x{id:x}."))
                .unwrap_or_default();
            let mut summary = match &focused_window {
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
            let browser = browser_observation_from_latest(browser_adapter_latest.as_ref());
            let semantic_adapters = if browser.connected {
                browser_adapter_latest
                    .as_ref()
                    .map(|message| {
                        let suffix = message
                            .extension_id
                            .as_deref()
                            .map(|id| format!(" from extension {id}"))
                            .unwrap_or_default();
                        summary.push_str(&format!(" Browser semantic adapter connected{suffix}."));
                        vec!["browser_native_messaging".to_string()]
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let vision_fallback_policy =
                vision_fallback_policy_for(active, pointer.child, &windows);
            Ok(Json(ObservationOut {
                preferred_model_lane: true,
                summary,
                outputs,
                focused_window,
                actionable_windows: windows,
                browser,
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
                semantic_adapters,
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

    #[tool(
        description = "List app skill manifests used by the command router, including browser and Blender seed skills."
    )]
    async fn list_app_skills(
        &self,
        Parameters(p): Parameters<SkillRegistryParams>,
    ) -> Result<Json<AppSkillRegistry>, ErrorData> {
        Ok(Json(command::skills(p.include_draft)))
    }

    #[tool(
        description = "Produce a reviewable app-skill acquisition plan for an unfamiliar application; never trusts the result automatically."
    )]
    async fn app_skill_acquisition_plan(
        &self,
        Parameters(p): Parameters<SkillAcquisitionParams>,
    ) -> Result<Json<SkillAcquisitionPlan>, ErrorData> {
        Ok(Json(command::acquisition_plan(&p)))
    }

    #[tool(
        description = "Route a short typed or transcript-fixture command into a bounded desktop/app action with safety and audit metadata."
    )]
    async fn route_command(
        &self,
        Parameters(p): Parameters<RouteCommandParams>,
    ) -> Result<Json<RoutedCommand>, ErrorData> {
        let x = self.x.clone();
        run(move || {
            let mut routed = command::route(&p);
            if !p.dry_run
                && (!routed.safety.confirmation_required || p.confirmed)
                && routed.result.status == "ready_to_execute"
            {
                routed.result =
                    execute_planned_action(&x, &routed.action, p.wait_ms.unwrap_or(500))?;
            }
            Ok(Json(routed))
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
            Ok(CallToolResult::success(vec![ContentBlock::image(
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

fn execute_planned_action(
    x: &X,
    action: &PlannedAction,
    wait_ms: u64,
) -> Result<CommandResult, ErrorData> {
    let mut details = std::collections::BTreeMap::new();
    match action {
        PlannedAction::LaunchApp { command, args }
        | PlannedAction::OpenUrl { command, args }
        | PlannedAction::RunSkill {
            command: Some(command),
            args,
            ..
        } => {
            let (command, args) = command_with_resolved_assets(command, args);
            let mut child = Command::new(&command).args(&args).spawn().map_err(to_err)?;
            details.insert("command".to_string(), command);
            details.insert(
                "args".to_string(),
                serde_json::to_string(&args).map_err(to_err)?,
            );
            if wait_ms > 0 {
                std::thread::sleep(Duration::from_millis(wait_ms));
            }
            match child.try_wait().map_err(to_err)? {
                Some(status) => {
                    details.insert("exit_status".to_string(), status.to_string());
                    Ok(CommandResult {
                        executed: true,
                        ok: status.success(),
                        status: if status.success() {
                            "completed".to_string()
                        } else {
                            "process_exited_nonzero".to_string()
                        },
                        details,
                    })
                }
                None => {
                    details.insert("pid".to_string(), child.id().to_string());
                    if let Ok(ewmh) = Ewmh::new(x) {
                        if let Ok(windows) = ewmh.list_windows() {
                            details.insert("window_count".to_string(), windows.len().to_string());
                        }
                    }
                    Ok(CommandResult {
                        executed: true,
                        ok: true,
                        status: "spawned".to_string(),
                        details,
                    })
                }
            }
        }
        PlannedAction::FocusWindow { window } => {
            Ewmh::new(x)
                .map_err(to_err)?
                .focus(*window)
                .map_err(to_err)?;
            details.insert("window".to_string(), window.to_string());
            Ok(CommandResult {
                executed: true,
                ok: true,
                status: "focused".to_string(),
                details,
            })
        }
        PlannedAction::TileWindow { window, slot } => {
            let ewmh = Ewmh::new(x).map_err(to_err)?;
            let target = match window {
                Some(window) => *window,
                None => ewmh
                    .active_window()
                    .map_err(to_err)?
                    .ok_or_else(|| ErrorData::invalid_params("no active window to tile", None))?,
            };
            let slot = match slot.as_str() {
                "left" => TileSlot::Left,
                "right" => TileSlot::Right,
                "top" => TileSlot::Top,
                "bottom" => TileSlot::Bottom,
                "full" => TileSlot::Full,
                other => {
                    return Err(ErrorData::invalid_params(
                        format!("unknown tile slot: {other}"),
                        None,
                    ));
                }
            };
            ewmh.tile(target, slot).map_err(to_err)?;
            details.insert("window".to_string(), target.to_string());
            Ok(CommandResult {
                executed: true,
                ok: true,
                status: "tiled".to_string(),
                details,
            })
        }
        PlannedAction::ResizeWindow {
            window,
            width,
            height,
        } => {
            let ewmh = Ewmh::new(x).map_err(to_err)?;
            let target = match window {
                Some(window) => *window,
                None => ewmh
                    .active_window()
                    .map_err(to_err)?
                    .ok_or_else(|| ErrorData::invalid_params("no active window to resize", None))?,
            };
            let info = ewmh
                .list_windows()
                .map_err(to_err)?
                .into_iter()
                .find(|window| window.id == target)
                .ok_or_else(|| ErrorData::invalid_params("target window not found", None))?;
            ewmh.move_resize(
                target,
                i32::from(info.x),
                i32::from(info.y),
                *width,
                *height,
            )
            .map_err(to_err)?;
            details.insert("window".to_string(), target.to_string());
            Ok(CommandResult {
                executed: true,
                ok: true,
                status: "resized".to_string(),
                details,
            })
        }
        PlannedAction::SwitchWorkspace { workspace } => {
            details.insert("workspace".to_string(), workspace.to_string());
            Ok(CommandResult {
                executed: false,
                ok: false,
                status: "workspace_switch_not_implemented".to_string(),
                details,
            })
        }
        PlannedAction::RunSkill { command: None, .. } | PlannedAction::Clarify { .. } => {
            Ok(CommandResult {
                executed: false,
                ok: false,
                status: "no_executable_action".to_string(),
                details,
            })
        }
    }
}

fn command_with_resolved_assets(command: &str, args: &[String]) -> (String, Vec<String>) {
    let mut resolved = args.to_vec();
    if command == "blender" {
        if let Some(index) = resolved
            .iter()
            .position(|arg| arg == "scripts/blender-cylinder.py")
        {
            let script = std::env::var("WMAKER_NG_BLENDER_CYLINDER_SCRIPT")
                .unwrap_or_else(|_| "/usr/share/wmaker-ng/blender-cylinder.py".to_string());
            resolved[index] = script;
        }
    }
    (command.to_string(), resolved)
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

fn to_monitor_outs(monitors: Vec<MonitorInfo>) -> Vec<MonitorOut> {
    monitors
        .into_iter()
        .map(|monitor| MonitorOut {
            name: monitor.name,
            geometry: RectOut {
                x: i32::from(monitor.x),
                y: i32::from(monitor.y),
                width: u32::from(monitor.width),
                height: u32::from(monitor.height),
            },
            width_mm: monitor.width_mm,
            height_mm: monitor.height_mm,
            primary: monitor.primary,
            automatic: monitor.automatic,
            output_count: monitor.output_count,
        })
        .collect()
}

fn output_for_rect(rect: &RectOut, outputs: &[MonitorOut]) -> Option<WindowOutputOut> {
    let output = outputs
        .iter()
        .max_by_key(|output| intersection_area(rect, &output.geometry))?;
    let intersection = intersection_area(rect, &output.geometry);
    if intersection == 0 && !rect_center_inside(rect, &output.geometry) {
        return None;
    }
    Some(WindowOutputOut {
        name: output.name.clone(),
        local_geometry: RectOut {
            x: rect.x - output.geometry.x,
            y: rect.y - output.geometry.y,
            width: rect.width,
            height: rect.height,
        },
    })
}

fn intersection_area(a: &RectOut, b: &RectOut) -> u64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = rect_right(a).min(rect_right(b));
    let bottom = rect_bottom(a).min(rect_bottom(b));
    if right <= left || bottom <= top {
        return 0;
    }
    let width = u64::try_from(right - left).unwrap_or(0);
    let height = u64::try_from(bottom - top).unwrap_or(0);
    width * height
}

fn rect_center_inside(rect: &RectOut, output: &RectOut) -> bool {
    let center_x = rect.x + i32::try_from(rect.width / 2).unwrap_or(i32::MAX);
    let center_y = rect.y + i32::try_from(rect.height / 2).unwrap_or(i32::MAX);
    center_x >= output.x
        && center_x < rect_right(output)
        && center_y >= output.y
        && center_y < rect_bottom(output)
}

fn rect_right(rect: &RectOut) -> i32 {
    rect.x
        .saturating_add(i32::try_from(rect.width).unwrap_or(i32::MAX))
}

fn rect_bottom(rect: &RectOut) -> i32 {
    rect.y
        .saturating_add(i32::try_from(rect.height).unwrap_or(i32::MAX))
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
        output: window.output.clone(),
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

async fn serve_browser_adapter_socket(
    state: BrowserAdapterState,
    socket_path: PathBuf,
) -> anyhow::Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    tracing::info!(
        path = %socket_path.display(),
        "ai-mcp: browser adapter socket listening"
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_browser_adapter_client(state, stream).await {
                tracing::warn!(%err, "ai-mcp: browser adapter client failed");
            }
        });
    }
}

async fn handle_browser_adapter_client(
    state: BrowserAdapterState,
    stream: UnixStream,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<BrowserAdapterMessage>(&line) {
            Ok(message) => handle_browser_adapter_message(&state, message),
            Err(err) => BrowserAdapterReply {
                schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
                ok: false,
                kind: "error".to_string(),
                error: Some(format!("invalid browser adapter message: {err}")),
            },
        };
        let mut encoded = serde_json::to_vec(&reply)?;
        encoded.push(b'\n');
        writer.write_all(&encoded).await?;
    }
    Ok(())
}

fn handle_browser_adapter_message(
    state: &BrowserAdapterState,
    message: BrowserAdapterMessage,
) -> BrowserAdapterReply {
    if message.schema_version != BROWSER_ADAPTER_SCHEMA_VERSION {
        return BrowserAdapterReply {
            schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
            ok: false,
            kind: message.kind,
            error: Some(format!(
                "unsupported schema_version {}; expected {}",
                message.schema_version, BROWSER_ADAPTER_SCHEMA_VERSION
            )),
        };
    }

    match message.kind.as_str() {
        "ping" => BrowserAdapterReply {
            schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
            ok: true,
            kind: "pong".to_string(),
            error: None,
        },
        "browser.summary" | "browser.controls" | "browser.action_result" => {
            match state.latest.lock() {
                Ok(mut latest) => {
                    *latest = Some(message.clone());
                    BrowserAdapterReply {
                        schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
                        ok: true,
                        kind: format!("{}.ack", message.kind),
                        error: None,
                    }
                }
                Err(err) => BrowserAdapterReply {
                    schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
                    ok: false,
                    kind: message.kind,
                    error: Some(format!("browser adapter state lock failed: {err}")),
                },
            }
        }
        other => BrowserAdapterReply {
            schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
            ok: false,
            kind: other.to_string(),
            error: Some(format!("unsupported browser adapter message kind: {other}")),
        },
    }
}

fn browser_observation_from_latest(
    latest: Option<&BrowserAdapterMessage>,
) -> BrowserObservationOut {
    let Some(message) = latest else {
        return BrowserObservationOut {
            connected: false,
            adapter: None,
            extension_id: None,
            tab_id: None,
            url: None,
            title: None,
            controls: Vec::new(),
        };
    };

    let payload = serde_json::from_value::<BrowserSummaryPayload>(message.payload.clone()).ok();
    BrowserObservationOut {
        connected: true,
        adapter: Some("browser_native_messaging".to_string()),
        extension_id: message.extension_id.clone(),
        tab_id: message.tab_id,
        url: payload.as_ref().and_then(|payload| payload.url.clone()),
        title: payload.as_ref().and_then(|payload| payload.title.clone()),
        controls: payload
            .map(|payload| payload.controls)
            .unwrap_or_default()
            .into_iter()
            .take(128)
            .collect(),
    }
}

#[derive(Deserialize)]
struct BrowserSummaryPayload {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    controls: Vec<BrowserControlOut>,
}

fn browser_socket_path() -> PathBuf {
    std::env::var_os("WMAKER_AI_BROWSER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            runtime.join(DEFAULT_BROWSER_SOCKET_NAME)
        })
}

fn browser_socket_enabled() -> bool {
    !matches!(
        std::env::var("WMAKER_AI_BROWSER_SOCKET").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

fn run_browser_host(args: &[String]) -> anyhow::Result<()> {
    let mut socket_path = browser_socket_path();
    let mut allowed_extension_ids = allowed_extension_ids_from_env();
    let mut allow_any_extension = std::env::var("WMAKER_AI_BROWSER_ALLOW_ANY_EXTENSION")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" => {
                i += 1;
                let Some(path) = args.get(i) else {
                    anyhow::bail!("--socket requires a path");
                };
                socket_path = PathBuf::from(path);
            }
            "--allow-extension-id" => {
                i += 1;
                let Some(id) = args.get(i) else {
                    anyhow::bail!("--allow-extension-id requires an extension id");
                };
                allowed_extension_ids.push(id.clone());
            }
            "--allow-any-extension" => allow_any_extension = true,
            "--help" | "-h" => {
                print_browser_host_help();
                return Ok(());
            }
            other => anyhow::bail!("unknown browser-host argument: {other}"),
        }
        i += 1;
    }

    if allowed_extension_ids.is_empty() && !allow_any_extension {
        anyhow::bail!(
            "browser-host requires at least one allowed extension id; pass --allow-extension-id or set WMAKER_AI_BROWSER_ALLOWED_EXTENSION_IDS"
        );
    }

    loop {
        let Some(message) = read_native_message()? else {
            return Ok(());
        };
        let reply = forward_native_message(
            &socket_path,
            &allowed_extension_ids,
            allow_any_extension,
            message,
        );
        write_native_message(&reply)?;
    }
}

fn allowed_extension_ids_from_env() -> Vec<String> {
    std::env::var("WMAKER_AI_BROWSER_ALLOWED_EXTENSION_IDS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn forward_native_message(
    socket_path: &PathBuf,
    allowed_extension_ids: &[String],
    allow_any_extension: bool,
    value: serde_json::Value,
) -> serde_json::Value {
    let message = match serde_json::from_value::<BrowserAdapterMessage>(value) {
        Ok(message) => message,
        Err(err) => {
            return serde_json::json!({
                "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
                "ok": false,
                "kind": "error",
                "error": format!("invalid message schema: {err}")
            });
        }
    };

    if !allow_any_extension {
        match &message.extension_id {
            Some(id) if allowed_extension_ids.iter().any(|allowed| allowed == id) => {}
            Some(id) => {
                return serde_json::json!({
                    "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
                    "ok": false,
                    "kind": message.kind,
                    "error": format!("extension id {id} is not allowed")
                });
            }
            None => {
                return serde_json::json!({
                    "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
                    "ok": false,
                    "kind": message.kind,
                    "error": "extension_id is required"
                });
            }
        }
    }

    match send_socket_message(socket_path, &message) {
        Ok(reply) => reply,
        Err(err) => serde_json::json!({
            "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
            "ok": false,
            "kind": message.kind,
            "error": format!("ai-mcp socket bridge failed: {err}")
        }),
    }
}

fn send_socket_message(
    socket_path: &PathBuf,
    message: &BrowserAdapterMessage,
) -> anyhow::Result<serde_json::Value> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path)?;
    let mut encoded = serde_json::to_vec(message)?;
    encoded.push(b'\n');
    stream.write_all(&encoded)?;
    stream.flush()?;

    let mut reader = std::io::BufReader::new(stream);
    let mut reply = String::new();
    std::io::BufRead::read_line(&mut reader, &mut reply)?;
    if reply.trim().is_empty() {
        anyhow::bail!("empty reply from ai-mcp socket");
    }
    Ok(serde_json::from_str(reply.trim_end())?)
}

fn read_native_message() -> anyhow::Result<Option<serde_json::Value>> {
    let mut len = [0u8; 4];
    let mut stdin = std::io::stdin().lock();
    match stdin.read_exact(&mut len) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > 16 * 1024 * 1024 {
        anyhow::bail!("native message too large: {len} bytes");
    }
    let mut buf = vec![0u8; len];
    stdin.read_exact(&mut buf)?;
    Ok(Some(serde_json::from_slice(&buf)?))
}

fn write_native_message(value: &serde_json::Value) -> anyhow::Result<()> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() > u32::MAX as usize {
        anyhow::bail!("native response too large: {} bytes", encoded.len());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&(encoded.len() as u32).to_le_bytes())?;
    stdout.write_all(&encoded)?;
    stdout.flush()?;
    Ok(())
}

fn print_browser_host_help() {
    eprintln!(
        "Usage: ai-mcp browser-host [--socket PATH] --allow-extension-id EXTENSION_ID\n\n\
         Chrome/Brave native messaging host. Reads length-prefixed JSON from\n\
         stdin, validates extension_id against the allowlist, and forwards\n\
         schema-versioned messages to ai-mcp over an owner-only Unix socket."
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("ai-mcp {}", version());
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "browser-host") {
        return run_browser_host(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "route-command") {
        return run_route_command_cli(&args[1..]);
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
    let browser_adapter = BrowserAdapterState::default();
    if browser_socket_enabled() {
        let socket_state = browser_adapter.clone();
        let socket_path = browser_socket_path();
        tokio::spawn(async move {
            if let Err(err) = serve_browser_adapter_socket(socket_state, socket_path).await {
                tracing::error!(%err, "ai-mcp: browser adapter socket stopped");
            }
        });
    }
    tracing::info!("ai-mcp: connected to X, serving MCP over stdio");
    let service = WmCtl {
        x,
        capture,
        diff: Arc::new(Mutex::new(DiffEncoder::new(diff_config_from_env()))),
        damage,
        clipboard: Arc::new(Mutex::new(None)),
        browser_adapter,
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

fn run_route_command_cli(args: &[String]) -> anyhow::Result<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_route_command_help();
        return Ok(());
    }
    let mut text = None;
    let mut dry_run = false;
    let mut source = CommandSource::TranscriptFixture;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--text" => {
                i += 1;
                text = args.get(i).cloned();
            }
            "--dry-run" => dry_run = true,
            "--source" => {
                i += 1;
                source = match args.get(i).map(String::as_str) {
                    Some("transcript_fixture") => CommandSource::TranscriptFixture,
                    Some("push_to_talk_asr") => CommandSource::PushToTalkAsr,
                    Some("typed") => CommandSource::Typed,
                    Some("model_plan") => CommandSource::ModelPlan,
                    Some("skill_replay") => CommandSource::SkillReplay,
                    Some(other) => anyhow::bail!("unknown source: {other}"),
                    None => anyhow::bail!("--source requires a value"),
                };
            }
            other if !other.starts_with('-') && text.is_none() => text = Some(other.to_string()),
            other => anyhow::bail!("unknown route-command argument: {other}"),
        }
        i += 1;
    }
    let text = text.ok_or_else(|| anyhow::anyhow!("route-command requires --text or TEXT"))?;
    let routed = command::route(&RouteCommandParams {
        text,
        source,
        confidence: None,
        dry_run,
        confirmed: false,
        wait_ms: None,
    });
    println!("{}", serde_json::to_string_pretty(&routed)?);
    Ok(())
}

fn print_route_command_help() {
    eprintln!(
        "Usage: ai-mcp route-command [--dry-run] [--source transcript_fixture|push_to_talk_asr|typed|model_plan|skill_replay] --text TEXT\n\n\
         Parses a short command fixture without connecting to X. Use this for\n\
         CI and non-audio transcript tests; live ASR feeds the same router later."
    );
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

    fn rect(x: i32, y: i32, width: u32, height: u32) -> RectOut {
        RectOut {
            x,
            y,
            width,
            height,
        }
    }

    fn monitor(name: &str, x: i32, y: i32, width: u32, height: u32) -> MonitorOut {
        MonitorOut {
            name: name.to_string(),
            geometry: rect(x, y, width, height),
            width_mm: None,
            height_mm: None,
            primary: x == 0 && y == 0,
            automatic: true,
            output_count: 1,
        }
    }

    #[test]
    fn desktop_scene_serializes_object_first_contract() {
        let outputs = vec![monitor("HDMI-1", 0, 0, 1280, 720)];
        let scene = DesktopSceneOut {
            screen: RectOut {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
            outputs: outputs.clone(),
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
                output: output_for_rect(
                    &RectOut {
                        x: 100,
                        y: 80,
                        width: 800,
                        height: 600,
                    },
                    &outputs,
                ),
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
        assert_eq!(json["windows"][0]["output"]["name"], "HDMI-1");
        assert_eq!(json["windows"][0]["output"]["local_geometry"]["x"], 100);
        assert_eq!(json["windows"][0]["capabilities"]["xtest_input"], true);
    }

    #[test]
    fn observation_serializes_text_first_contract() {
        let outputs = vec![monitor("HDMI-1", 0, 0, 1280, 720)];
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
            output: output_for_rect(
                &RectOut {
                    x: 100,
                    y: 80,
                    width: 800,
                    height: 600,
                },
                &outputs,
            ),
            workspace: Some(0),
            active: true,
            actions: vec!["focus".to_string(), "move_resize".to_string()],
        };
        let observation = ObservationOut {
            preferred_model_lane: true,
            summary: "Focused window 0x2a: Example.".to_string(),
            outputs,
            focused_window: Some(focused.clone()),
            actionable_windows: Vec::new(),
            browser: browser_observation_from_latest(None),
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
        assert_eq!(json["outputs"][0]["name"], "HDMI-1");
        assert_eq!(json["focused_window"]["output"]["name"], "HDMI-1");
        assert_eq!(json["browser"]["connected"], false);
        assert!(
            json["browser"]["controls"]
                .as_array()
                .expect("controls")
                .is_empty()
        );
        assert_eq!(json["pixel_fallbacks"]["embedded_pixels"], false);
        assert_eq!(json["vision_fallback_policy"]["pixels_required"], false);
        assert_eq!(json["vision_fallback_policy"]["crop_target"]["handle"], 42);
        assert_eq!(
            json["vision_fallback_policy"]["crop_target"]["output"]["name"],
            "HDMI-1"
        );
        assert!(json["pixel_fallbacks"]["full_screenshot_tool"].is_string());
    }

    #[test]
    fn output_for_rect_handles_required_monitor_layouts() {
        let single = vec![monitor("root", 0, 0, 1280, 720)];
        assert_eq!(
            output_for_rect(&rect(200, 100, 300, 200), &single)
                .expect("single monitor match")
                .name,
            "root"
        );

        let dual_horizontal = vec![
            monitor("left", 0, 0, 1920, 1080),
            monitor("right", 1920, 0, 1920, 1080),
        ];
        let out = output_for_rect(&rect(2000, 100, 640, 480), &dual_horizontal)
            .expect("right monitor match");
        assert_eq!(out.name, "right");
        assert_eq!(out.local_geometry.x, 80);

        let dual_vertical = vec![
            monitor("top", 0, 0, 1600, 900),
            monitor("bottom", 0, 900, 1600, 900),
        ];
        let out = output_for_rect(&rect(200, 980, 640, 480), &dual_vertical)
            .expect("bottom monitor match");
        assert_eq!(out.name, "bottom");
        assert_eq!(out.local_geometry.y, 80);

        let mixed = vec![
            monitor("wide", 0, 0, 2560, 1440),
            monitor("portrait", 2560, 0, 1080, 1920),
        ];
        let out =
            output_for_rect(&rect(2500, 200, 300, 800), &mixed).expect("largest intersection wins");
        assert_eq!(out.name, "portrait");
        assert_eq!(out.local_geometry.x, -60);
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

    #[test]
    fn browser_adapter_message_acknowledges_supported_schema() {
        let state = BrowserAdapterState::default();
        let message = BrowserAdapterMessage {
            schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
            kind: "browser.summary".to_string(),
            extension_id: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
            tab_id: Some(1),
            payload: serde_json::json!({"title": "Example"}),
        };

        let reply = handle_browser_adapter_message(&state, message);

        assert!(reply.ok);
        assert_eq!(reply.kind, "browser.summary.ack");
        assert!(
            state
                .latest
                .lock()
                .expect("state lock")
                .as_ref()
                .is_some_and(|latest| latest.kind == "browser.summary")
        );
    }

    #[test]
    fn browser_observation_projects_adapter_summary_without_pixels() {
        let message = BrowserAdapterMessage {
            schema_version: BROWSER_ADAPTER_SCHEMA_VERSION,
            kind: "browser.summary".to_string(),
            extension_id: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
            tab_id: Some(12),
            payload: serde_json::json!({
                "url": "https://example.com/",
                "title": "Example Domain",
                "controls": [
                    {
                        "handle": "dom:abc123",
                        "role": "button",
                        "label": "Continue",
                        "enabled": true,
                        "bounds": {"x": 10, "y": 20, "width": 100, "height": 30},
                        "redacted": false
                    }
                ]
            }),
        };

        let observation = browser_observation_from_latest(Some(&message));
        let json = serde_json::to_value(observation).expect("browser observation serializes");

        assert_eq!(json["connected"], true);
        assert_eq!(json["adapter"], "browser_native_messaging");
        assert_eq!(json["extension_id"], "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(json["tab_id"], 12);
        assert_eq!(json["url"], "https://example.com/");
        assert_eq!(json["title"], "Example Domain");
        assert_eq!(json["controls"][0]["handle"], "dom:abc123");
        assert_eq!(json["controls"][0]["bounds"]["width"], 100);
        assert!(json.get("pixels").is_none());
    }

    #[test]
    fn browser_adapter_rejects_unsupported_schema() {
        let state = BrowserAdapterState::default();
        let reply = handle_browser_adapter_message(
            &state,
            BrowserAdapterMessage {
                schema_version: BROWSER_ADAPTER_SCHEMA_VERSION + 1,
                kind: "browser.summary".to_string(),
                extension_id: None,
                tab_id: None,
                payload: serde_json::Value::Null,
            },
        );

        assert!(!reply.ok);
        assert!(
            reply
                .error
                .as_deref()
                .expect("error")
                .contains("unsupported schema_version")
        );
    }

    #[test]
    fn native_host_rejects_missing_extension_id_before_socket_connect() {
        let reply = forward_native_message(
            &PathBuf::from("/tmp/wmaker-ai-test-missing.sock"),
            &["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()],
            false,
            serde_json::json!({
                "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
                "kind": "browser.summary",
                "payload": {}
            }),
        );

        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"], "extension_id is required");
    }

    #[test]
    fn native_host_rejects_unlisted_extension_id_before_socket_connect() {
        let reply = forward_native_message(
            &PathBuf::from("/tmp/wmaker-ai-test-unlisted.sock"),
            &["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()],
            false,
            serde_json::json!({
                "schema_version": BROWSER_ADAPTER_SCHEMA_VERSION,
                "kind": "browser.summary",
                "extension_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "payload": {}
            }),
        );

        assert_eq!(reply["ok"], false);
        assert!(
            reply["error"]
                .as_str()
                .expect("error")
                .contains("not allowed")
        );
    }
}
