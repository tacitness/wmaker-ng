//! The screen-diff protocol — the genuinely novel piece (PLAN §6).
//!
//! Grounded in X primitives, not reinvented: **XDamage** yields exact dirty
//! rectangles per frame and **XShm** gives fast pixel access; the invention is
//! the *encoding* that makes deltas cheap for a model. Frame model (video-codec
//! analogy):
//!
//! - **Keyframe (I-frame):** full screen capture/description — the baseline.
//! - **Delta (P-frame):** `{rect, content}` list from XDamage since last frame.
//! - **Re-baseline:** every N seconds, or when cumulative damage exceeds a
//!   threshold.
//! - **Semantic tier (later):** structured deltas via EWMH + AT-SPI, pixel crop
//!   as fallback.
//!
//! Pixel-rect tier ships first (Week 3, PLAN §8).

use std::time::{Duration, Instant};

use serde::Serialize;
use wmng_x11::Frame;
use x11rb::protocol::xproto::Rectangle as XRectangle;

/// Re-baseline tuning for the pixel-rect protocol.
#[derive(Debug, Clone)]
pub struct DiffConfig {
    /// Force a keyframe after this much wall-clock time.
    pub keyframe_interval: Duration,
    /// Force a keyframe once dirty area exceeds this share of the screen.
    pub max_dirty_ratio: f32,
    /// Emit at most this many dirty regions before coalescing to a keyframe.
    pub max_regions: usize,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            keyframe_interval: Duration::from_secs(10),
            max_dirty_ratio: 0.35,
            max_regions: 16,
        }
    }
}

/// Stateful encoder for full-screen keyframes plus XDamage-backed deltas.
#[derive(Debug, Clone)]
pub struct DiffEncoder {
    config: DiffConfig,
    last_keyframe: Option<Instant>,
}

impl DiffEncoder {
    pub fn new(config: DiffConfig) -> Self {
        Self {
            config,
            last_keyframe: None,
        }
    }

    /// Encode a full frame and reset the rebaseline timer.
    pub fn keyframe(&mut self, frame: &Frame<'_>) -> Result<ScreenUpdate, Error> {
        self.last_keyframe = Some(Instant::now());
        let rect = Rect {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        };
        Ok(ScreenUpdate {
            kind: UpdateKind::Keyframe,
            width: frame.width,
            height: frame.height,
            dirty_area: rect.area(),
            rebaseline_reason: Some(RebaselineReason::Initial),
            regions: vec![encode_region(frame, rect)?],
        })
    }

    /// Mark the current client view as baselined without encoding a keyframe.
    ///
    /// MCP clients that already consumed a full screenshot can call this before
    /// requesting raw deltas, avoiding an immediate full-frame raw payload.
    pub fn note_keyframe(&mut self) {
        self.last_keyframe = Some(Instant::now());
    }

    pub fn has_keyframe(&self) -> bool {
        self.last_keyframe.is_some()
    }

    /// Encode the changed regions, falling back to a keyframe when the delta is
    /// too old or too large to stay cheap.
    pub fn changed_regions(
        &mut self,
        frame: &Frame<'_>,
        dirty: &[XRectangle],
    ) -> Result<ScreenUpdate, Error> {
        let rects = coalesce_rects(
            normalize_rects(frame.width, frame.height, dirty),
            self.config.max_regions,
        );
        let dirty_area = rects
            .iter()
            .fold(0u32, |total, rect| total.saturating_add(rect.area()));
        if self.last_keyframe.is_none() {
            return self.keyframe(frame);
        }
        if rects.is_empty() {
            return Ok(ScreenUpdate {
                kind: UpdateKind::Delta,
                width: frame.width,
                height: frame.height,
                dirty_area: 0,
                rebaseline_reason: None,
                regions: Vec::new(),
            });
        }
        if let Some(reason) = self.rebaseline_reason(frame, dirty_area) {
            let mut update = self.keyframe(frame)?;
            update.rebaseline_reason = Some(reason);
            return Ok(update);
        }
        Ok(ScreenUpdate {
            kind: UpdateKind::Delta,
            width: frame.width,
            height: frame.height,
            dirty_area,
            rebaseline_reason: None,
            regions: rects
                .into_iter()
                .map(|rect| encode_region(frame, rect))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    /// Return raw X11 pixel crops for the changed regions. This avoids PNG
    /// deflate and RGBA conversion, so it is intended for low-latency local
    /// consumers that can handle native `ZPixmap` bytes.
    pub fn changed_regions_raw(
        &mut self,
        frame: &Frame<'_>,
        dirty: &[XRectangle],
    ) -> Result<RawScreenUpdate, Error> {
        let rects = coalesce_rects(
            normalize_rects(frame.width, frame.height, dirty),
            self.config.max_regions,
        );
        let dirty_area = rects
            .iter()
            .fold(0u32, |total, rect| total.saturating_add(rect.area()));

        if self.last_keyframe.is_none() {
            return Ok(RawScreenUpdate {
                kind: UpdateKind::Keyframe,
                width: frame.width,
                height: frame.height,
                bytes_per_pixel: frame.bytes_per_pixel,
                dirty_area: frame.width as u32 * frame.height as u32,
                rebaseline_reason: Some(RebaselineReason::Initial),
                needs_keyframe: true,
                regions: Vec::new(),
            });
        }

        if rects.is_empty() {
            return Ok(RawScreenUpdate {
                kind: UpdateKind::Delta,
                width: frame.width,
                height: frame.height,
                bytes_per_pixel: frame.bytes_per_pixel,
                dirty_area: 0,
                rebaseline_reason: None,
                needs_keyframe: false,
                regions: Vec::new(),
            });
        }

        if let Some(reason) = self.rebaseline_reason(frame, dirty_area) {
            return Ok(RawScreenUpdate {
                kind: UpdateKind::Keyframe,
                width: frame.width,
                height: frame.height,
                bytes_per_pixel: frame.bytes_per_pixel,
                dirty_area: frame.width as u32 * frame.height as u32,
                rebaseline_reason: Some(reason),
                needs_keyframe: true,
                regions: Vec::new(),
            });
        }

        Ok(RawScreenUpdate {
            kind: UpdateKind::Delta,
            width: frame.width,
            height: frame.height,
            bytes_per_pixel: frame.bytes_per_pixel,
            dirty_area,
            rebaseline_reason: None,
            needs_keyframe: false,
            regions: rects
                .into_iter()
                .map(|rect| encode_region_raw(frame, rect))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    fn rebaseline_reason(&self, frame: &Frame<'_>, dirty_area: u32) -> Option<RebaselineReason> {
        if self
            .last_keyframe
            .is_some_and(|last| last.elapsed() >= self.config.keyframe_interval)
        {
            return Some(RebaselineReason::Interval);
        }
        let screen_area = frame.width as f32 * frame.height as f32;
        if screen_area > 0.0 && dirty_area as f32 / screen_area >= self.config.max_dirty_ratio {
            return Some(RebaselineReason::Area);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    Keyframe,
    Delta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RebaselineReason {
    Initial,
    Interval,
    Area,
    RegionCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EncodedRegion {
    pub rect: Rect,
    #[serde(skip_serializing)]
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenUpdate {
    pub kind: UpdateKind,
    pub width: u16,
    pub height: u16,
    pub dirty_area: u32,
    pub rebaseline_reason: Option<RebaselineReason>,
    pub regions: Vec<EncodedRegion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawEncodedRegion {
    pub rect: Rect,
    pub stride: u32,
    #[serde(skip_serializing)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawScreenUpdate {
    pub kind: UpdateKind,
    pub width: u16,
    pub height: u16,
    pub bytes_per_pixel: u8,
    pub dirty_area: u32,
    pub rebaseline_reason: Option<RebaselineReason>,
    pub needs_keyframe: bool,
    pub regions: Vec<RawEncodedRegion>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported X pixmap bytes-per-pixel: {0}")]
    UnsupportedBpp(u8),

    #[error("PNG encoding failed: {0}")]
    Png(#[from] png::EncodingError),
}

fn normalize_rects(width: u16, height: u16, dirty: &[XRectangle]) -> Vec<Rect> {
    let mut rects: Vec<Rect> = dirty
        .iter()
        .filter_map(|r| clip_rect(width, height, r))
        .collect();
    rects.sort_by_key(|r| (r.y, r.x, r.height, r.width));
    rects.dedup();
    rects
}

fn coalesce_rects(rects: Vec<Rect>, max_regions: usize) -> Vec<Rect> {
    if rects.len() <= max_regions {
        return rects;
    }
    let Some(first) = rects.first().copied() else {
        return rects;
    };
    let (mut x0, mut y0, mut x1, mut y1) = (
        first.x,
        first.y,
        first.x + first.width,
        first.y + first.height,
    );
    for rect in rects.iter().skip(1) {
        x0 = x0.min(rect.x);
        y0 = y0.min(rect.y);
        x1 = x1.max(rect.x + rect.width);
        y1 = y1.max(rect.y + rect.height);
    }
    vec![Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }]
}

fn clip_rect(width: u16, height: u16, r: &XRectangle) -> Option<Rect> {
    let x0 = (r.x as i32).clamp(0, width as i32) as u16;
    let y0 = (r.y as i32).clamp(0, height as i32) as u16;
    let x1 = (r.x as i32 + r.width as i32).clamp(0, width as i32) as u16;
    let y1 = (r.y as i32 + r.height as i32).clamp(0, height as i32) as u16;
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn encode_region(frame: &Frame<'_>, rect: Rect) -> Result<EncodedRegion, Error> {
    Ok(EncodedRegion {
        rect,
        png: encode_rect_png(
            frame.bytes(),
            frame.width,
            frame.height,
            frame.bytes_per_pixel,
            rect,
        )?,
    })
}

fn encode_region_raw(frame: &Frame<'_>, rect: Rect) -> Result<RawEncodedRegion, Error> {
    let bpp = frame.bytes_per_pixel as usize;
    if bpp < 3 {
        return Err(Error::UnsupportedBpp(frame.bytes_per_pixel));
    }

    let frame_width = frame.width as usize;
    let rect_x = rect.x as usize;
    let rect_y = rect.y as usize;
    let rect_w = rect.width as usize;
    let rect_h = rect.height as usize;
    let frame_stride = frame_width * bpp;
    let region_stride = rect_w * bpp;
    let mut bytes = Vec::with_capacity(region_stride * rect_h);

    for y in rect_y..rect_y + rect_h {
        let start = y * frame_stride + rect_x * bpp;
        let end = start + region_stride;
        bytes.extend_from_slice(&frame.bytes()[start..end]);
    }

    Ok(RawEncodedRegion {
        rect,
        stride: region_stride as u32,
        bytes,
    })
}

pub fn encode_full_png(frame: &Frame<'_>) -> Result<Vec<u8>, Error> {
    encode_rect_png(
        frame.bytes(),
        frame.width,
        frame.height,
        frame.bytes_per_pixel,
        Rect {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        },
    )
}

fn encode_rect_png(
    src: &[u8],
    frame_width: u16,
    frame_height: u16,
    bpp: u8,
    rect: Rect,
) -> Result<Vec<u8>, Error> {
    let bpp = bpp as usize;
    if bpp < 3 {
        return Err(Error::UnsupportedBpp(bpp as u8));
    }
    let frame_width = frame_width as usize;
    let frame_height = frame_height as usize;
    let rect_x = rect.x as usize;
    let rect_y = rect.y as usize;
    let rect_w = rect.width as usize;
    let rect_h = rect.height as usize;
    let stride = frame_width * bpp;
    let mut rgba = Vec::with_capacity(rect_w * rect_h * 4);
    for y in rect_y..(rect_y + rect_h).min(frame_height) {
        let start = y * stride + rect_x * bpp;
        let end = start + rect_w * bpp;
        for px in src[start..end].chunks_exact(bpp) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, rect.width as u32, rect.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&rgba)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_dirty_rectangles_to_screen() {
        let rects = normalize_rects(
            100,
            80,
            &[XRectangle {
                x: -5,
                y: 70,
                width: 20,
                height: 20,
            }],
        );

        assert_eq!(
            rects,
            vec![Rect {
                x: 0,
                y: 70,
                width: 15,
                height: 10,
            }]
        );
    }

    #[test]
    fn drops_empty_or_offscreen_rectangles() {
        let rects = normalize_rects(
            100,
            80,
            &[XRectangle {
                x: 120,
                y: 90,
                width: 20,
                height: 20,
            }],
        );

        assert!(rects.is_empty());
    }

    #[test]
    fn coalesces_too_many_rectangles_to_bounding_region() {
        let rects = coalesce_rects(
            vec![
                Rect {
                    x: 10,
                    y: 10,
                    width: 5,
                    height: 5,
                },
                Rect {
                    x: 30,
                    y: 15,
                    width: 10,
                    height: 10,
                },
                Rect {
                    x: 20,
                    y: 50,
                    width: 5,
                    height: 5,
                },
            ],
            2,
        );

        assert_eq!(
            rects,
            vec![Rect {
                x: 10,
                y: 10,
                width: 30,
                height: 45,
            }]
        );
    }
}
