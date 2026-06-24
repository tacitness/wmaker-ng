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
//! Pixel-rect tier ships first (Week 3, PLAN §8); no behavior yet.

/// Placeholder marker until the keyframe/delta codec is implemented.
pub const SCAFFOLD: &str = "ai-proto";
