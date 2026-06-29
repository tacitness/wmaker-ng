//! Manual smoke test for wmng-x11 — run against an ISOLATED display only
//! (it synthesizes real input):
//!
//!   xvfb-run -a -s "-screen 0 1024x768x24" cargo run -p wmng-x11 --example smoke
//!
//! Not a `#[test]` on purpose: it needs a live X server, which CI does not have.

use std::sync::Arc;

use wmng_x11::X;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let x = Arc::new(X::connect()?);
    let (w, h) = x.dimensions();
    println!(
        "[connect] {w}x{h} depth={} bpp={} shm={}",
        x.root_depth(),
        x.bytes_per_pixel(),
        x.shm_available()
    );

    x.move_pointer(100, 120)?;
    x.click(1)?;
    x.type_text("hi")?;
    println!("[input] moved + clicked + typed");

    let mut cap = x.capture()?;
    let frame = cap.frame()?;
    println!(
        "[capture] {}x{} depth={} bytes={}",
        frame.width,
        frame.height,
        frame.depth,
        frame.bytes().len()
    );

    let feed = x.damage_feed()?;
    // Generate some activity, then drain.
    x.move_pointer(200, 200)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    println!("[damage] {} dirty rect(s)", feed.poll()?.len());

    match x.cursor_image() {
        Ok(c) => println!("[cursor] {}x{}", c.width, c.height),
        Err(e) => println!("[cursor] unavailable: {e}"),
    }

    println!("\nSMOKE OK");
    Ok(())
}
