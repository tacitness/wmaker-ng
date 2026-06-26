//! Screen capture: MIT-SHM shared-memory path (fast, zero-copy reads) with a
//! plain `GetImage` fallback when the extension is unavailable.

use std::os::raw::c_void;

use x11rb::connection::Connection;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

use crate::{Error, Result, X};

/// One captured frame. `bytes` is `Z_PIXMAP` data, `bytes_per_pixel`-packed,
/// row-major, `width * height * bytes_per_pixel` long.
pub struct Frame<'a> {
    pub width: u16,
    pub height: u16,
    pub depth: u8,
    pub bytes_per_pixel: u8,
    data: &'a [u8],
}

impl Frame<'_> {
    /// Raw pixel bytes for this frame.
    pub fn bytes(&self) -> &[u8] {
        self.data
    }
}

/// A reusable capturer over the root window. On the SHM path it attaches one
/// shared segment up front and reuses it for every `frame()`; on drop it
/// detaches and releases the segment.
pub struct Capture<'x> {
    x: &'x X,
    width: u16,
    height: u16,
    depth: u8,
    bpp: u8,
    shm: Option<ShmState>,
    buf: Vec<u8>,
}

struct ShmState {
    addr: *mut c_void,
    seg: u32,
    size: usize,
}

impl<'x> Capture<'x> {
    pub(crate) fn new(x: &'x X) -> Result<Self> {
        let (width, height) = x.dimensions();
        let mut cap = Capture {
            x,
            width,
            height,
            depth: x.root_depth(),
            bpp: x.bytes_per_pixel(),
            shm: None,
            buf: Vec::new(),
        };
        if x.shm_available() {
            // Best-effort: fall back to GetImage if the SHM setup fails.
            cap.shm = cap.init_shm().ok();
        }
        Ok(cap)
    }

    fn init_shm(&self) -> Result<ShmState> {
        let size = self.width as usize * self.height as usize * self.bpp as usize;
        // SAFETY: standard System V shared-memory dance for MIT-SHM. The
        // segment is marked IPC_RMID immediately after attach so it cannot leak
        // past the last detach, even on crash.
        let id = unsafe { libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600) };
        if id < 0 {
            return Err(Error::Shm("shmget"));
        }
        let addr = unsafe { libc::shmat(id, std::ptr::null(), 0) };
        if addr as isize == -1 {
            // SAFETY: id is a valid segment we just created.
            unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
            return Err(Error::Shm("shmat"));
        }
        let seg = self.x.conn().generate_id()?;
        self.x.conn().shm_attach(seg, id as u32, false)?;
        self.x.conn().flush()?;
        // SAFETY: mark for deletion now; the kernel frees it once both we and
        // the X server detach.
        unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
        Ok(ShmState { addr, seg, size })
    }

    /// Capture the current root-window contents.
    pub fn frame(&mut self) -> Result<Frame<'_>> {
        let conn = self.x.conn();
        let root = self.x.root();
        if let Some(shm) = &self.shm {
            conn.shm_get_image(
                root,
                0,
                0,
                self.width,
                self.height,
                !0,
                ImageFormat::Z_PIXMAP.into(),
                shm.seg,
                0,
            )?
            .reply()?;
            // SAFETY: the server just wrote `size` bytes into our attached
            // segment; the mapping outlives this borrow (dropped with Capture).
            let data = unsafe { std::slice::from_raw_parts(shm.addr as *const u8, shm.size) };
            Ok(Frame {
                width: self.width,
                height: self.height,
                depth: self.depth,
                bytes_per_pixel: self.bpp,
                data,
            })
        } else {
            let img = conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    root,
                    0,
                    0,
                    self.width,
                    self.height,
                    !0,
                )?
                .reply()?;
            self.buf = img.data;
            Ok(Frame {
                width: self.width,
                height: self.height,
                depth: self.depth,
                bytes_per_pixel: self.bpp,
                data: &self.buf,
            })
        }
    }
}

impl Drop for Capture<'_> {
    fn drop(&mut self) {
        if let Some(shm) = &self.shm {
            let _ = self.x.conn().shm_detach(shm.seg);
            let _ = self.x.conn().flush();
            // SAFETY: addr came from shmat; detach our mapping (IPC_RMID was
            // already set, so the segment is freed here).
            unsafe { libc::shmdt(shm.addr) };
        }
    }
}
