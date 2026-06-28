//! Shared EWMH (`_NET_*`) window-control client.
//!
//! How every companion talks to the window manager — list, focus, move,
//! resize, tile — via standard `_NET_*` properties and client messages, so the
//! C core never learns it is being driven (README philosophy; PLAN §5).
//!
//! Rides on the [`wmng_x11::X`] connection.

mod error;
pub use error::{Error, Result};

use wmng_x11::X;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};

/// Source indication "pager/tool" (EWMH) — WMs honour these requests.
const SOURCE_PAGER: u32 = 2;

/// A window as reported by the WM via EWMH.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: Window,
    pub title: String,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

/// Where to snap a window with [`Ewmh::tile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileSlot {
    Left,
    Right,
    Top,
    Bottom,
    Full,
}

/// EWMH client bound to an X connection. Interns the atoms it uses once.
pub struct Ewmh<'x> {
    x: &'x X,
    atoms: Atoms,
}

struct Atoms {
    supported: Atom,
    client_list: Atom,
    active_window: Atom,
    moveresize: Atom,
    close_window: Atom,
    net_wm_name: Atom,
    workarea: Atom,
}

impl<'x> Ewmh<'x> {
    /// Intern the `_NET_*` atoms over the given connection.
    pub fn new(x: &'x X) -> Result<Self> {
        let intern = |name: &str| -> Result<Atom> {
            Ok(x.conn().intern_atom(false, name.as_bytes())?.reply()?.atom)
        };
        let atoms = Atoms {
            supported: intern("_NET_SUPPORTED")?,
            client_list: intern("_NET_CLIENT_LIST")?,
            active_window: intern("_NET_ACTIVE_WINDOW")?,
            moveresize: intern("_NET_MOVERESIZE_WINDOW")?,
            close_window: intern("_NET_CLOSE_WINDOW")?,
            net_wm_name: intern("_NET_WM_NAME")?,
            workarea: intern("_NET_WORKAREA")?,
        };
        Ok(Self { x, atoms })
    }

    /// All managed top-level windows, in the WM's stacking/`_NET_CLIENT_LIST`
    /// order, with title + absolute geometry.
    pub fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                self.x.root(),
                self.atoms.client_list,
                AtomEnum::WINDOW,
                0,
                u32::MAX,
            )?
            .reply()?;
        let ids: Vec<Window> = reply.value32().map(|it| it.collect()).unwrap_or_default();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            // A window may vanish mid-iteration; degrade gracefully, never panic.
            let title = self.window_title(id).unwrap_or_default();
            let (x, y, width, height) = self.window_geometry(id).unwrap_or((0, 0, 0, 0));
            out.push(WindowInfo {
                id,
                title,
                x,
                y,
                width,
                height,
            });
        }
        Ok(out)
    }

    /// The currently active window, if the WM advertises one.
    pub fn active_window(&self) -> Result<Option<Window>> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                self.x.root(),
                self.atoms.active_window,
                AtomEnum::WINDOW,
                0,
                1,
            )?
            .reply()?;
        Ok(reply
            .value32()
            .and_then(|mut it| it.next())
            .filter(|&w| w != 0))
    }

    /// Ask the WM to activate (focus + raise) a window.
    pub fn focus(&self, window: Window) -> Result<()> {
        self.require_supported(self.atoms.active_window, "_NET_ACTIVE_WINDOW")?;
        self.send_root_message(window, self.atoms.active_window, [SOURCE_PAGER, 0, 0, 0, 0])
    }

    /// Ask the WM to move + resize a window (`_NET_MOVERESIZE_WINDOW`).
    pub fn move_resize(
        &self,
        window: Window,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.require_supported(self.atoms.moveresize, "_NET_MOVERESIZE_WINDOW")?;
        // flags: bits 8-11 mark x/y/width/height present; bits 12-13 = source.
        let flags = (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (SOURCE_PAGER << 12);
        self.send_root_message(
            window,
            self.atoms.moveresize,
            [flags, x as u32, y as u32, width, height],
        )
    }

    /// Ask the WM to close a window (`_NET_CLOSE_WINDOW`).
    pub fn close(&self, window: Window) -> Result<()> {
        self.require_supported(self.atoms.close_window, "_NET_CLOSE_WINDOW")?;
        self.send_root_message(window, self.atoms.close_window, [0, SOURCE_PAGER, 0, 0, 0])
    }

    /// Snap a window into a half/full slot of the work area.
    pub fn tile(&self, window: Window, slot: TileSlot) -> Result<()> {
        let (ax, ay, aw, ah) = self.work_area().unwrap_or_else(|_| {
            let (w, h) = self.x.dimensions();
            (0, 0, u32::from(w), u32::from(h))
        });
        let (hw, hh) = (aw / 2, ah / 2);
        let (x, y, w, h) = match slot {
            TileSlot::Left => (ax, ay, hw, ah),
            TileSlot::Right => (ax + hw as i32, ay, hw, ah),
            TileSlot::Top => (ax, ay, aw, hh),
            TileSlot::Bottom => (ax, ay + hh as i32, aw, hh),
            TileSlot::Full => (ax, ay, aw, ah),
        };
        self.move_resize(window, x, y, w, h)
    }

    // ── internals ────────────────────────────────────────────────────────────
    fn require_supported(&self, atom: Atom, name: &'static str) -> Result<()> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                self.x.root(),
                self.atoms.supported,
                AtomEnum::ATOM,
                0,
                u32::MAX,
            )?
            .reply()?;
        let supported = reply
            .value32()
            .is_some_and(|mut atoms| atoms.any(|supported| supported == atom));
        if supported {
            Ok(())
        } else {
            Err(Error::Unsupported(name))
        }
    }

    fn window_title(&self, window: Window) -> Result<String> {
        // Prefer _NET_WM_NAME (UTF-8); fall back to the legacy WM_NAME.
        for atom in [self.atoms.net_wm_name, AtomEnum::WM_NAME.into()] {
            let reply = self
                .x
                .conn()
                .get_property(false, window, atom, AtomEnum::ANY, 0, u32::MAX)?
                .reply()?;
            if !reply.value.is_empty() {
                return Ok(String::from_utf8_lossy(&reply.value).into_owned());
            }
        }
        Ok(String::new())
    }

    fn window_geometry(&self, window: Window) -> Result<(i16, i16, u16, u16)> {
        let geom = self.x.conn().get_geometry(window)?.reply()?;
        // get_geometry is parent-relative (often the WM frame); translate to root.
        let abs = self
            .x
            .conn()
            .translate_coordinates(window, self.x.root(), 0, 0)?
            .reply()?;
        Ok((abs.dst_x, abs.dst_y, geom.width, geom.height))
    }

    fn work_area(&self) -> Result<(i32, i32, u32, u32)> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                self.x.root(),
                self.atoms.workarea,
                AtomEnum::CARDINAL,
                0,
                4,
            )?
            .reply()?;
        let v: Vec<u32> = reply.value32().map(|it| it.collect()).unwrap_or_default();
        match v[..] {
            [x, y, w, h, ..] => Ok((x as i32, y as i32, w, h)),
            _ => Err(Error::Property("_NET_WORKAREA")),
        }
    }

    fn send_root_message(&self, window: Window, type_: Atom, data: [u32; 5]) -> Result<()> {
        let event = ClientMessageEvent::new(32, window, type_, data);
        self.x.conn().send_event(
            false,
            self.x.root(),
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )?;
        self.x.conn().flush()?;
        Ok(())
    }
}
