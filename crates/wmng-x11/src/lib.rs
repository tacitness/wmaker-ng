//! Shared X11 primitives for the wmaker-ng companions.
//!
//! The single seam both facets (`ng-*` and `ai-*`) reach the X extension
//! surface through:
//!
//! - **XTEST** — pointer/keyboard input synthesis ([`X::move_pointer`],
//!   [`X::click`], [`X::key`], [`X::type_text`])
//! - **XShm** — fast shared-memory capture ([`X::capture`] → [`Capture::frame`])
//! - **XDamage** — dirty-rectangle change feed ([`X::damage_feed`])
//! - **XFixes** — cursor image ([`X::cursor_image`])
//!
//! Pure-Rust x11rb (no `libxcb` link). All fallible calls return [`Error`]; the
//! hot paths never panic.

#![forbid(unsafe_op_in_unsafe_fn)]

mod capture;
mod damage;
mod error;

pub use capture::{Capture, Frame, SharedCapture};
pub use damage::DamageFeed;
pub use error::{Error, Result};

use std::collections::HashMap;
use std::sync::Arc;

use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::damage::ConnectionExt as _;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

const SHM_EXT: &str = "MIT-SHM";
const KEYSYM_SHIFT_L: u32 = 0xffe1;

/// A connection to the X server plus the cached state the companions need.
pub struct X {
    conn: RustConnection,
    root: Window,
    width: u16,
    height: u16,
    root_depth: u8,
    bpp: u8,
    shm_available: bool,
    keymap: KeyMap,
}

impl X {
    /// Connect to the X server named by `$DISPLAY`, negotiate the extensions we
    /// drive, and cache root geometry + the keyboard map.
    pub fn connect() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)?;

        // XTEST is the riskiest capability — fail fast and clearly if absent.
        if conn
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)?
            .is_none()
        {
            return Err(Error::MissingExtension("XTEST"));
        }
        // DAMAGE + XFIXES are needed for the feed / cursor; negotiate versions.
        conn.damage_query_version(1, 1)?.reply()?;
        conn.xfixes_query_version(5, 0)?.reply()?;
        let shm_available = conn.extension_information(SHM_EXT)?.is_some();

        let setup = conn.setup();
        let screen = &setup.roots[screen_num];
        let root = screen.root;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;
        let root_depth = screen.root_depth;
        let bpp = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == root_depth)
            .map(|f| f.bits_per_pixel / 8)
            .unwrap_or(4);
        let keymap = KeyMap::build(&conn, setup.min_keycode, setup.max_keycode)?;

        Ok(X {
            conn,
            root,
            width,
            height,
            root_depth,
            bpp,
            shm_available,
            keymap,
        })
    }

    // ── Accessors (used by the capture/damage modules and downstream crates) ─
    /// The underlying connection (for callers needing raw protocol access).
    pub fn conn(&self) -> &RustConnection {
        &self.conn
    }
    /// The root window id.
    pub fn root(&self) -> Window {
        self.root
    }
    /// Root dimensions in pixels.
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    /// Root window depth.
    pub fn root_depth(&self) -> u8 {
        self.root_depth
    }
    /// Bytes per pixel for `Z_PIXMAP` captures at the root depth.
    pub fn bytes_per_pixel(&self) -> u8 {
        self.bpp
    }
    /// Whether MIT-SHM is available (capture uses it when so).
    pub fn shm_available(&self) -> bool {
        self.shm_available
    }

    // ── Input synthesis (XTEST) ──────────────────────────────────────────────
    /// Warp the pointer to an absolute root coordinate.
    pub fn move_pointer(&self, x: i16, y: i16) -> Result<()> {
        self.conn
            .xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, self.root, x, y, 0)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Press and release a pointer button (1=left, 2=middle, 3=right) at the
    /// current pointer position.
    pub fn click(&self, button: u8) -> Result<()> {
        self.button(button, true)?;
        self.button(button, false)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Move the pointer, then click one or more times.
    pub fn click_at(&self, x: i16, y: i16, button: u8, count: u8) -> Result<()> {
        self.move_pointer(x, y)?;
        for _ in 0..count.max(1) {
            self.button(button, true)?;
            self.button(button, false)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Press or release a single pointer button.
    pub fn button(&self, button: u8, press: bool) -> Result<()> {
        let ty = if press {
            BUTTON_PRESS_EVENT
        } else {
            BUTTON_RELEASE_EVENT
        };
        self.conn
            .xtest_fake_input(ty, button, 0, self.root, 0, 0, 0)?;
        Ok(())
    }

    /// Synthesize wheel scrolling. Positive `dy` scrolls down, negative up;
    /// positive `dx` scrolls right, negative left.
    pub fn scroll(&self, dx: i16, dy: i16) -> Result<()> {
        self.scroll_axis(dy, 5, 4)?;
        self.scroll_axis(dx, 7, 6)?;
        self.conn.flush()?;
        Ok(())
    }

    fn scroll_axis(&self, delta: i16, positive_button: u8, negative_button: u8) -> Result<()> {
        let button = if delta >= 0 {
            positive_button
        } else {
            negative_button
        };
        for _ in 0..delta.unsigned_abs() {
            self.button(button, true)?;
            self.button(button, false)?;
        }
        Ok(())
    }

    /// Drag from one absolute root coordinate to another.
    pub fn drag(&self, x1: i16, y1: i16, x2: i16, y2: i16, button: u8) -> Result<()> {
        self.move_pointer(x1, y1)?;
        self.button(button, true)?;
        self.move_pointer(x2, y2)?;
        self.button(button, false)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Tap a key by keysym, applying Shift when the keysym sits in the shifted
    /// column of the keyboard map.
    pub fn key(&self, keysym: u32) -> Result<()> {
        let (keycode, shifted) = self.keymap.lookup(keysym).ok_or(Error::NoKeycode(keysym))?;
        let shift = self.keymap.shift;
        if shifted && shift != 0 {
            self.key_code(shift, true)?;
        }
        self.key_code(keycode, true)?;
        self.key_code(keycode, false)?;
        if shifted && shift != 0 {
            self.key_code(shift, false)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Press modifiers, tap a key, then release modifiers in reverse order.
    pub fn key_combo(&self, modifiers: &[u32], keysym: u32) -> Result<()> {
        let modifier_keycodes = modifiers
            .iter()
            .map(|keysym| {
                self.keymap
                    .lookup(*keysym)
                    .map(|(keycode, _)| keycode)
                    .ok_or(Error::NoKeycode(*keysym))
            })
            .collect::<Result<Vec<_>>>()?;
        let (keycode, shifted) = self.keymap.lookup(keysym).ok_or(Error::NoKeycode(keysym))?;
        let shift = self.keymap.shift;

        for keycode in &modifier_keycodes {
            self.key_code(*keycode, true)?;
        }
        if shifted && shift != 0 && !modifier_keycodes.contains(&shift) {
            self.key_code(shift, true)?;
        }
        self.key_code(keycode, true)?;
        self.key_code(keycode, false)?;
        if shifted && shift != 0 && !modifier_keycodes.contains(&shift) {
            self.key_code(shift, false)?;
        }
        for keycode in modifier_keycodes.iter().rev() {
            self.key_code(*keycode, false)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Press or release a raw keycode.
    pub fn key_code(&self, keycode: u8, press: bool) -> Result<()> {
        let ty = if press {
            KEY_PRESS_EVENT
        } else {
            KEY_RELEASE_EVENT
        };
        self.conn
            .xtest_fake_input(ty, keycode, 0, self.root, 0, 0, 0)?;
        Ok(())
    }

    /// Type a string. ASCII/Latin-1 characters map to keysyms directly; an
    /// unmapped character yields [`Error::NoKeycode`].
    pub fn type_text(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            self.key(ch as u32)?;
        }
        Ok(())
    }

    /// Current pointer coordinates and the child window under the cursor.
    pub fn pointer(&self) -> Result<PointerInfo> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        Ok(PointerInfo {
            x: reply.root_x,
            y: reply.root_y,
            child: (reply.child != 0).then_some(reply.child),
        })
    }

    // ── Capture / damage / cursor ────────────────────────────────────────────
    /// Build a reusable root-window capturer (SHM when available).
    pub fn capture(&self) -> Result<Capture<'_>> {
        Capture::new(self)
    }

    /// Build a reusable root-window capturer owned by the shared X connection.
    pub fn shared_capture(self: &Arc<Self>) -> Result<SharedCapture> {
        SharedCapture::new(self.clone())
    }

    /// Subscribe to the root-window damage stream.
    pub fn damage_feed(self: &Arc<Self>) -> Result<DamageFeed> {
        DamageFeed::new(self.clone())
    }

    /// Fetch the current cursor image (XFixes).
    pub fn cursor_image(&self) -> Result<CursorImage> {
        let r = self.conn.xfixes_get_cursor_image()?.reply()?;
        Ok(CursorImage {
            width: r.width,
            height: r.height,
            xhot: r.xhot,
            yhot: r.yhot,
            pixels: r.cursor_image,
        })
    }
}

/// A cursor image: `pixels` is `width * height` ARGB, premultiplied (XFixes).
pub struct CursorImage {
    pub width: u16,
    pub height: u16,
    pub xhot: u16,
    pub yhot: u16,
    pub pixels: Vec<u32>,
}

/// Pointer position on the root window.
pub struct PointerInfo {
    pub x: i16,
    pub y: i16,
    pub child: Option<Window>,
}

/// keysym → (keycode, needs-shift), plus the Shift_L keycode.
struct KeyMap {
    map: HashMap<u32, (u8, bool)>,
    shift: u8,
}

impl KeyMap {
    fn build(conn: &RustConnection, min: u8, max: u8) -> Result<Self> {
        let count = max - min + 1;
        let reply = conn.get_keyboard_mapping(min, count)?.reply()?;
        let per = reply.keysyms_per_keycode as usize;
        let mut map: HashMap<u32, (u8, bool)> = HashMap::new();
        for kc in 0..count as usize {
            // Column 0 = unshifted, 1 = shifted; visit unshifted first so it
            // wins ties.
            for col in 0..per.min(2) {
                let keysym = reply.keysyms[kc * per + col];
                if keysym != 0 {
                    map.entry(keysym).or_insert((min + kc as u8, col == 1));
                }
            }
        }
        let shift = map.get(&KEYSYM_SHIFT_L).map(|&(kc, _)| kc).unwrap_or(0);
        Ok(KeyMap { map, shift })
    }

    fn lookup(&self, keysym: u32) -> Option<(u8, bool)> {
        self.map.get(&keysym).copied()
    }
}
