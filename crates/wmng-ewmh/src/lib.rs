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
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask,
    MapState, Window,
};

/// Source indication "pager/tool" (EWMH) — WMs honour these requests.
const SOURCE_PAGER: u32 = 2;

/// A window as reported by the WM via EWMH.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: Window,
    pub title: String,
    pub class: Option<String>,
    pub instance: Option<String>,
    pub pid: Option<u32>,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub workspace: Option<u32>,
    pub mapped: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub transient_for: Option<Window>,
    pub group_leader: Option<Window>,
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
    client_list_stacking: Atom,
    active_window: Atom,
    moveresize: Atom,
    close_window: Atom,
    wm_change_state: Atom,
    wm_state: Atom,
    wm_class: Atom,
    wm_client_leader: Atom,
    wm_transient_for: Atom,
    net_wm_state: Atom,
    net_wm_state_maximized_horz: Atom,
    net_wm_state_maximized_vert: Atom,
    net_wm_name: Atom,
    net_wm_pid: Atom,
    net_wm_desktop: Atom,
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
            client_list_stacking: intern("_NET_CLIENT_LIST_STACKING")?,
            active_window: intern("_NET_ACTIVE_WINDOW")?,
            moveresize: intern("_NET_MOVERESIZE_WINDOW")?,
            close_window: intern("_NET_CLOSE_WINDOW")?,
            wm_change_state: intern("WM_CHANGE_STATE")?,
            wm_state: intern("WM_STATE")?,
            wm_class: intern("WM_CLASS")?,
            wm_client_leader: intern("WM_CLIENT_LEADER")?,
            wm_transient_for: intern("WM_TRANSIENT_FOR")?,
            net_wm_state: intern("_NET_WM_STATE")?,
            net_wm_state_maximized_horz: intern("_NET_WM_STATE_MAXIMIZED_HORZ")?,
            net_wm_state_maximized_vert: intern("_NET_WM_STATE_MAXIMIZED_VERT")?,
            net_wm_name: intern("_NET_WM_NAME")?,
            net_wm_pid: intern("_NET_WM_PID")?,
            net_wm_desktop: intern("_NET_WM_DESKTOP")?,
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
            let (instance, class) = self.window_class(id).unwrap_or((None, None));
            let pid = self.property_u32(id, self.atoms.net_wm_pid, AtomEnum::CARDINAL);
            let (x, y, width, height) = self.window_geometry(id).unwrap_or((0, 0, 0, 0));
            let workspace = self.property_u32(id, self.atoms.net_wm_desktop, AtomEnum::CARDINAL);
            let mapped = self.window_mapped(id).unwrap_or(false);
            let minimized = self.window_minimized(id).unwrap_or(false);
            let maximized = self.window_maximized(id).unwrap_or(false);
            let transient_for = self.property_window(id, self.atoms.wm_transient_for);
            let group_leader = self.property_window(id, self.atoms.wm_client_leader);
            out.push(WindowInfo {
                id,
                title,
                class,
                instance,
                pid,
                x,
                y,
                width,
                height,
                workspace,
                mapped,
                minimized,
                maximized,
                transient_for,
                group_leader,
            });
        }
        Ok(out)
    }

    /// Top-level windows in bottom-to-top stacking order when the WM supports
    /// `_NET_CLIENT_LIST_STACKING`; falls back to [`Self::list_windows`].
    pub fn stacking_windows(&self) -> Result<Vec<Window>> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                self.x.root(),
                self.atoms.client_list_stacking,
                AtomEnum::WINDOW,
                0,
                u32::MAX,
            )?
            .reply()?;
        let ids = reply
            .value32()
            .map(|it| it.collect::<Vec<_>>())
            .unwrap_or_default();
        if ids.is_empty() {
            Ok(self.list_windows()?.into_iter().map(|w| w.id).collect())
        } else {
            Ok(ids)
        }
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
        if !self.is_supported(self.atoms.moveresize)? {
            return self.configure_window(window, x, y, width, height);
        }
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

    /// Ask the WM to iconify/minimize a window via the ICCCM `WM_CHANGE_STATE`.
    pub fn minimize(&self, window: Window) -> Result<()> {
        const ICONIC_STATE: u32 = 3;
        self.send_root_message(
            window,
            self.atoms.wm_change_state,
            [ICONIC_STATE, 0, 0, 0, 0],
        )
    }

    /// Add the horizontal + vertical maximized EWMH state.
    pub fn maximize(&self, window: Window) -> Result<()> {
        self.set_maximized(window, true)
    }

    /// Remove the horizontal + vertical maximized EWMH state.
    pub fn unmaximize(&self, window: Window) -> Result<()> {
        self.set_maximized(window, false)
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

    fn set_maximized(&self, window: Window, enabled: bool) -> Result<()> {
        self.require_supported(self.atoms.net_wm_state, "_NET_WM_STATE")?;
        let action = if enabled { 1 } else { 0 };
        self.send_root_message(
            window,
            self.atoms.net_wm_state,
            [
                action,
                self.atoms.net_wm_state_maximized_vert,
                self.atoms.net_wm_state_maximized_horz,
                SOURCE_PAGER,
                0,
            ],
        )
    }

    // ── internals ────────────────────────────────────────────────────────────
    fn require_supported(&self, atom: Atom, name: &'static str) -> Result<()> {
        if self.is_supported(atom)? {
            Ok(())
        } else {
            Err(Error::Unsupported(name))
        }
    }

    fn is_supported(&self, atom: Atom) -> Result<bool> {
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
        Ok(reply
            .value32()
            .is_some_and(|mut atoms| atoms.any(|supported| supported == atom)))
    }

    fn configure_window(
        &self,
        window: Window,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.x.conn().configure_window(
            window,
            &ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width)
                .height(height),
        )?;
        self.x.conn().flush()?;
        Ok(())
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

    fn window_class(&self, window: Window) -> Result<(Option<String>, Option<String>)> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                window,
                self.atoms.wm_class,
                AtomEnum::STRING,
                0,
                u32::MAX,
            )?
            .reply()?;
        Ok(parse_wm_class(&reply.value))
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

    fn window_mapped(&self, window: Window) -> Result<bool> {
        let attrs = self.x.conn().get_window_attributes(window)?.reply()?;
        Ok(attrs.map_state == MapState::VIEWABLE)
    }

    fn window_minimized(&self, window: Window) -> Result<bool> {
        const ICONIC_STATE: u32 = 3;
        Ok(self
            .property_u32(window, self.atoms.wm_state, self.atoms.wm_state)
            .is_some_and(|state| state == ICONIC_STATE))
    }

    fn window_maximized(&self, window: Window) -> Result<bool> {
        let reply = self
            .x
            .conn()
            .get_property(
                false,
                window,
                self.atoms.net_wm_state,
                AtomEnum::ATOM,
                0,
                u32::MAX,
            )?
            .reply()?;
        let states: Vec<Atom> = reply.value32().map(|it| it.collect()).unwrap_or_default();
        Ok(states.contains(&self.atoms.net_wm_state_maximized_horz)
            && states.contains(&self.atoms.net_wm_state_maximized_vert))
    }

    fn property_u32(&self, window: Window, property: Atom, ty: impl Into<Atom>) -> Option<u32> {
        self.x
            .conn()
            .get_property(false, window, property, ty.into(), 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()
            .and_then(|mut it| it.next())
    }

    fn property_window(&self, window: Window, property: Atom) -> Option<Window> {
        self.property_u32(window, property, AtomEnum::WINDOW)
            .filter(|&id| id != 0)
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

fn parse_wm_class(value: &[u8]) -> (Option<String>, Option<String>) {
    let mut parts = value
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned());
    let instance = parts.next();
    let class = parts.next();
    (instance, class)
}

#[cfg(test)]
mod tests {
    use super::parse_wm_class;

    #[test]
    fn parses_wm_class_instance_and_class() {
        let (instance, class) = parse_wm_class(b"Navigator\0firefox\0");
        assert_eq!(instance.as_deref(), Some("Navigator"));
        assert_eq!(class.as_deref(), Some("firefox"));
    }

    #[test]
    fn parses_missing_wm_class_parts() {
        let (instance, class) = parse_wm_class(b"terminal\0");
        assert_eq!(instance.as_deref(), Some("terminal"));
        assert!(class.is_none());

        let (instance, class) = parse_wm_class(b"");
        assert!(instance.is_none());
        assert!(class.is_none());
    }
}
