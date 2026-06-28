//! XDamage change feed — the dirty-rectangle stream `ai-proto` (#14) consumes
//! to build screen-diff deltas.

use std::sync::Arc;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::damage::{ConnectionExt as _, ReportLevel};
use x11rb::protocol::xproto::Rectangle;

use crate::{Result, X};

/// A live damage subscription on the root window. `poll()` drains the dirty
/// rectangles seen since the last call (non-blocking). Destroyed on drop.
pub struct DamageFeed {
    x: Arc<X>,
    damage: u32,
}

impl DamageFeed {
    pub fn new(x: Arc<X>) -> Result<Self> {
        let damage = x.conn().generate_id()?;
        x.conn()
            .damage_create(damage, x.root(), ReportLevel::DELTA_RECTANGLES)?;
        x.conn().flush()?;
        Ok(Self { x, damage })
    }

    /// Non-blocking: return the dirty rectangles queued since the last poll.
    pub fn poll(&self) -> Result<Vec<Rectangle>> {
        let mut rects = Vec::new();
        while let Some(event) = self.x.conn().poll_for_event()? {
            if let Event::DamageNotify(n) = event {
                rects.push(n.area);
            }
        }
        self.x.conn().damage_subtract(self.damage, 0u32, 0u32)?;
        self.x.conn().flush()?;
        Ok(rects)
    }
}

impl Drop for DamageFeed {
    fn drop(&mut self) {
        let _ = self.x.conn().damage_destroy(self.damage);
        let _ = self.x.conn().flush();
    }
}
