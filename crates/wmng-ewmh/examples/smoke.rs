//! Manual smoke test for wmng-ewmh. Run against an isolated display; with a
//! real WM running you'll see managed windows, without one the calls simply
//! return empty / no-op (proving protocol correctness):
//!
//!   xvfb-run -a -s "-screen 0 1024x768x24" cargo run -p wmng-ewmh --example smoke

use wmng_ewmh::{Ewmh, TileSlot};
use wmng_x11::X;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, PropMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let x = X::connect()?;
    let conn = x.conn();
    let root = x.root();

    // Create + map a titled window so there's something to manage.
    let win = conn.generate_id()?;
    conn.create_window(
        0, // COPY_FROM_PARENT depth
        win,
        root,
        0,
        0,
        200,
        150,
        0,
        WindowClass::COPY_FROM_PARENT,
        0, // COPY_FROM_PARENT visual
        &CreateWindowAux::new(),
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"smoke-window",
    )?;
    conn.map_window(win)?;
    conn.flush()?;
    // Give a window manager (if one is running) a moment to manage the window.
    std::thread::sleep(std::time::Duration::from_millis(400));

    let ewmh = Ewmh::new(&x)?;
    let windows = ewmh.list_windows()?;
    println!("[list_windows] {} managed", windows.len());
    for w in &windows {
        println!(
            "  0x{:x} {:>4}x{:<4} '{}'",
            w.id, w.width, w.height, w.title
        );
    }
    println!("[active_window] {:?}", ewmh.active_window()?);

    // These no-op without a WM listening, but must not error.
    ewmh.focus(win)?;
    ewmh.move_resize(win, 50, 60, 320, 240)?;
    ewmh.tile(win, TileSlot::Left)?;
    println!("[requests] focus + move_resize + tile sent OK");

    println!("\nSMOKE OK");
    Ok(())
}
