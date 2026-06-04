//! Cursor overlay compositor for FFmpeg export pipeline.
//!
//! Loads an `EffectTimeline` JSON and draws cursor indicators onto YUV420P
//! video frames during export. Supports both `FitWithBars` and `CenterCrop`
//! scale policies with correct coordinate mapping.
//!
//! # Coordinate mapping
//!
//! Cursor coordinates in the effect timeline are in **source video pixel
//! coordinates** (matching the decoded frame dimensions). The mapping to
//! output frame coordinates depends on the export scale policy:
//!
//! - **CenterCrop**: translate by crop origin, then scale by `out/crop` ratio.
//! - **FitWithBars**: scale by `fit/src` ratio, then translate by canvas offset.
//!
//! # Safety contract
//!
//! The `render_cursor_overlay` flag in `EffectTimeline` enforces:
//! - If `true`: raw SCK frames do NOT contain the system cursor; overlay is needed.
//! - If `false`: raw SCK frames already contain the cursor; overlay is forbidden.

use crate::app::error::{AppError, AppResult};
use crate::core::timeline::{CursorFrame, CursorKind, EffectTimeline};
use crate::media::export_presets::ExportScalePolicy;

// ---------------------------------------------------------------------------
// Cursor glyph data
// ---------------------------------------------------------------------------

/// A pixel in a cursor glyph: transparent, black with alpha, or white with alpha.
#[derive(Clone, Copy)]
enum GlyphPixel {
    Transparent,
    Black(u8),
    White(u8),
}

/// Static cursor glyph with hotspot metadata.
struct CursorGlyph {
    width: usize,
    height: usize,
    /// Hotspot X relative to glyph top-left corner.
    hotspot_x: f32,
    /// Hotspot Y relative to glyph top-left corner.
    hotspot_y: f32,
    /// Row-major pixel data.
    pixels: &'static [GlyphPixel],
}

/// Get the glyph for a given cursor kind.
fn glyph_for_kind(kind: CursorKind) -> &'static CursorGlyph {
    match kind {
        CursorKind::Arrow => &ARROW_GLYPH,
        CursorKind::Hand => &HAND_GLYPH,
        CursorKind::IBeam => &IBEAM_GLYPH,
    }
}

// Shorthand constants for the pixel arrays.
const B: GlyphPixel = GlyphPixel::Black(255);
const W: GlyphPixel = GlyphPixel::White(255);
const T: GlyphPixel = GlyphPixel::Transparent;

// Arrow: 24×24 diagonal arrow. Hotspot at tip (0,0).
// Colors: white outline (W), black fill (B) — matches macOS standard arrow.
// Row layout (24 values per row, 24 rows = 576 total):
//   Row 0:  W T T T T T T T T T T T T T T T T T T T T T T T
//   Row 1:  W W T T T T T T T T T T T T T T T T T T T T T T
//   Row 2:  W B W T T T T T T T T T T T T T T T T T T T T T
//   Row 3:  W B B W T T T T T T T T T T T T T T T T T T T T
//   Row 4:  W B B B W T T T T T T T T T T T T T T T T T T T
//   Row 5:  W B B B B W T T T T T T T T T T T T T T T T T T
//   Row 6:  W B B B B B W T T T T T T T T T T T T T T T T T
//   Row 7:  W B B B B B B W T T T T T T T T T T T T T T T T
//   Row 8:  W B B B B B B B W T T T T T T T T T T T T T T T
//   Row 9:  W B B B B B B B B W T T T T T T T T T T T T T T
//   Row 10: W B B B B B B B B B W T T T T T T T T T T T T T
//   Row 11: W B B B B B B B B B B W T T T T T T T T T T T T
//   Row 12: W B B B B B B B B B B B W T T T T T T T T T T T
//   Row 13: W B B B B B B B B B B B B W T T T T T T T T T T
//   Row 14: W B B B B B B B B B B B B B W T T T T T T T T T
//   Row 15: W B B B B B B B B B B B B B B W T T T T T T T T
//   Row 16: W B B B B B B B B B B B B B B B W T T T T T T T
//   Row 17: W B B B B B B B B B B B B B B B B W T T T T T T
//   Row 18: W B B B B B B B B B B B B B B B B B W T T T T T
//   Row 19: W B B W W W W W W W W W W W W W W W W T T T T T
//   Row 20: W B W T T T T T T T T T T T T T T T T T T T T T
//   Row 21: W W T T T T T T T T T T T T T T T T T T T T T T
//   Row 22: W T T T T T T T T T T T T T T T T T T T T T T T
//   Row 23: T T T T T T T T T T T T T T T T T T T T T T T T
static ARROW_GLYPH: CursorGlyph = CursorGlyph {
    width: 24,
    height: 24,
    hotspot_x: 0.0,
    hotspot_y: 0.0,
    pixels: &ARROW_PIXELS,
};

#[rustfmt::skip]
static ARROW_PIXELS: [GlyphPixel; 576] = [
    // Row 0
    W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 1
    W, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 2
    W, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 3
    W, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 4
    W, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 5
    W, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 6
    W, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 7
    W, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 8
    W, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 9
    W, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 10
    W, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 11
    W, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 12
    W, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T, T,
    // Row 13
    W, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T, T,
    // Row 14
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T, T,
    // Row 15
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T, T,
    // Row 16
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T, T,
    // Row 17
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T, T,
    // Row 18
    W, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, W, T, T, T, T, T,
    // Row 19
    W, B, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, T, T, T, T, T,
    // Row 20
    W, B, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 21
    W, W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 22
    W, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    // Row 23
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
];

// Hand: 24×24 open hand silhouette. Hotspot at fingertip (12, 4).
static HAND_GLYPH: CursorGlyph = CursorGlyph {
    width: 24,
    height: 24,
    hotspot_x: 12.0,
    hotspot_y: 4.0,
    pixels: &HAND_PIXELS,
};

static HAND_PIXELS: [GlyphPixel; 576] = [
    T, T, T, T, T, T, T, T, B, B, B, B, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, B,
    W, W, W, W, B, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, B, W, W, W, W, B, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, B, W, W, W, W, W, W, B, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, B, W, W, W, W, W, W, B, T, T, T, T, T, T, T, T, T, T, T, T, T, B, B, B, B, W,
    W, W, W, W, W, B, B, B, T, T, T, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W,
    B, T, T, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T,
    T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T, B, W, W, W, W, W, W,
    W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
    W, B, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T,
    T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T, B, W, W, W, W, W, W,
    W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
    W, B, T, T, T, T, T, T, T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T,
    T, T, B, W, W, W, W, W, W, W, W, W, W, W, W, W, B, T, T, T, T, T, T, T, T, T, T, B, B, B, B, B,
    B, B, B, B, B, B, B, B, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,
];

// IBeam: 16×24 I-beam text cursor. Hotspot at center (8, 12).
// Colors: white outline (W), black body (B) — matches macOS standard I-beam.
static IBEAM_GLYPH: CursorGlyph = CursorGlyph {
    width: 16,
    height: 24,
    hotspot_x: 8.0,
    hotspot_y: 12.0,
    pixels: &IBEAM_PIXELS,
};

#[rustfmt::skip]
static IBEAM_PIXELS: [GlyphPixel; 384] = [
    // Row 0: top serif
    T, T, T, T, W, W, W, W, W, W, T, T, T, T, T, T,
    // Row 1: top cap
    T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T,
    // Row 2: top transition
    T, T, T, W, W, B, B, W, W, T, T, T, T, T, T, T,
    // Row 3
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 4
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 5
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 6
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 7
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 8
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 9
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 10
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 11
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 12
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 13
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 14
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 15
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 16
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 17
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 18
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 19
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 20: bottom transition
    T, T, T, T, T, W, B, B, W, T, T, T, T, T, T, T,
    // Row 21: bottom serifs
    T, T, T, T, W, W, B, B, W, W, T, T, T, T, T, T,
    // Row 22: bottom cap
    T, T, T, W, B, B, B, B, B, B, W, T, T, T, T, T,
    // Row 23: bottom serif
    T, T, T, W, W, W, W, W, W, W, W, T, T, T, T, T,
];

/// Renders cursor overlay onto YUV420P video frames during export.
pub struct CursorOverlayRenderer {
    timeline: EffectTimeline,
    cursor_radius: f32,
    mapper: CursorCoordMapper,
}

/// Maps cursor coordinates from source video space to output frame space.
///
/// Two variants handle the two export scale policies:
/// - **CenterCrop**: `(src - crop_origin) * scale → output`.
/// - **FitWithBars**: `src * scale + offset → output`.
enum CursorCoordMapper {
    CenterCrop {
        crop_x: f32,
        crop_y: f32,
        crop_w: f32,
        crop_h: f32,
        scale_x: f32,
        scale_y: f32,
    },
    FitWithBars {
        scale_x: f32,
        scale_y: f32,
        offset_x: f32,
        offset_y: f32,
    },
}

impl CursorCoordMapper {
    fn new(
        policy: ExportScalePolicy,
        src_w: u32,
        src_h: u32,
        out_w: u32,
        out_h: u32,
        crop_origin: Option<(u32, u32)>,
        fit_dims: Option<(u32, u32)>,
    ) -> Self {
        match policy {
            ExportScalePolicy::CenterCrop => {
                let (cx, cy) = crop_origin.unwrap_or((0, 0));
                let src_ratio = src_w as f64 / src_h as f64;
                let out_ratio = out_w as f64 / out_h as f64;
                let (crop_w, crop_h) = if src_ratio > out_ratio {
                    ((src_h as f64 * out_ratio).round() as f32, src_h as f32)
                } else {
                    (src_w as f32, (src_w as f64 / out_ratio).round() as f32)
                };
                let crop_w = crop_w.max(1.0);
                let crop_h = crop_h.max(1.0);
                Self::CenterCrop {
                    crop_x: cx as f32,
                    crop_y: cy as f32,
                    crop_w,
                    crop_h,
                    scale_x: out_w as f32 / crop_w,
                    scale_y: out_h as f32 / crop_h,
                }
            }
            ExportScalePolicy::FitWithBars => {
                let (fw, fh) = fit_dims.unwrap_or((out_w, out_h));
                let scale_x = fw as f32 / src_w.max(1) as f32;
                let scale_y = fh as f32 / src_h.max(1) as f32;
                Self::FitWithBars {
                    scale_x,
                    scale_y,
                    offset_x: (out_w - fw) as f32 / 2.0,
                    offset_y: (out_h - fh) as f32 / 2.0,
                }
            }
        }
    }

    /// Map cursor coordinates from source video space to output frame space.
    /// Returns `None` if the cursor falls outside the visible output region.
    fn map(&self, src_x: f32, src_y: f32) -> Option<(f32, f32)> {
        match *self {
            Self::CenterCrop {
                crop_x,
                crop_y,
                crop_w,
                crop_h,
                scale_x,
                scale_y,
            } => {
                let rel_x = src_x - crop_x;
                let rel_y = src_y - crop_y;
                if rel_x < 0.0 || rel_y < 0.0 || rel_x > crop_w || rel_y > crop_h {
                    return None;
                }
                Some((rel_x * scale_x, rel_y * scale_y))
            }
            Self::FitWithBars {
                scale_x,
                scale_y,
                offset_x,
                offset_y,
            } => {
                let out_x = src_x * scale_x + offset_x;
                let out_y = src_y * scale_y + offset_y;
                Some((out_x, out_y))
            }
        }
    }
}

impl CursorOverlayRenderer {
    /// Create a new renderer from an effect timeline and export geometry.
    #[allow(clippy::too_many_arguments)]
    ///
    /// Returns `None` if `timeline.render_cursor_overlay` is `false` or the
    /// timeline has no frames (nothing to draw).
    pub fn new(
        timeline: EffectTimeline,
        src_w: u32,
        src_h: u32,
        out_w: u32,
        out_h: u32,
        scale_policy: ExportScalePolicy,
        crop_origin: Option<(u32, u32)>,
        fit_dims: Option<(u32, u32)>,
    ) -> Option<Self> {
        if !timeline.render_cursor_overlay || timeline.frames.is_empty() {
            return None;
        }

        let mapper = CursorCoordMapper::new(
            scale_policy,
            src_w,
            src_h,
            out_w,
            out_h,
            crop_origin,
            fit_dims,
        );

        Some(Self {
            timeline,
            cursor_radius: 12.0,
            mapper,
        })
    }

    /// Draw cursor overlay onto the output frame at the given source timestamp.
    ///
    /// `source_timestamp_nanos` is the timestamp in the **original source
    /// timeline** (nanoseconds from recording start). This must NOT be the
    /// output timeline PTS, because after auto-trim cuts the output PTS no
    /// longer corresponds to the source cursor timeline.
    pub fn draw_on_frame(
        &self,
        frame: &mut ffmpeg_next::util::frame::Video,
        source_timestamp_nanos: u64,
        _fps: u32,
    ) {
        if self.timeline.frames.is_empty() {
            return;
        }

        let timestamp_nanos = source_timestamp_nanos;

        let cursor_frame = match self.find_cursor_frame(timestamp_nanos) {
            Some(f) => f,
            None => return,
        };

        // Validate timeline frame values: skip non-finite or out-of-range.
        if !cursor_frame.x.is_finite()
            || !cursor_frame.y.is_finite()
            || !cursor_frame.scale.is_finite()
        {
            return;
        }

        // Clamp scale to a reasonable range to prevent extreme radius values.
        let clamped_scale = cursor_frame.scale.clamp(0.25, 4.0);

        let (out_x, out_y) = match self.mapper.map(cursor_frame.x, cursor_frame.y) {
            Some(p) => p,
            None => return, // Cursor outside visible region.
        };

        // BUG-008: Validate mapped coordinates are finite before drawing.
        // Huge finite values after mapping can still cause i32 overflow in
        // bounding box arithmetic. Skip the frame rather than panic.
        if !out_x.is_finite() || !out_y.is_finite() {
            return;
        }

        // Draw on Y plane (luma) only. YUV420P Y=0 is black, Y=235 is white.
        // Read immutable dimensions before mutable data borrow.
        let w = frame.width() as i32;
        let h = frame.height() as i32;
        if w <= 0 || h <= 0 {
            return;
        }
        let data = frame.data_mut(0);
        let linesize = if h > 0 {
            data.len() / h as usize
        } else {
            return;
        };

        // Clamp radius: minimum 4px, maximum 256px or half the smallest dimension.
        let max_radius = (w.min(h) / 2).min(256);
        let radius = ((self.cursor_radius * clamped_scale).round() as i32)
            .max(4)
            .min(max_radius);

        // BUG-008: Clamp mapped coordinates to a safe drawing range.
        // Allow margin for partially-visible cursors at edges.
        let margin = max_radius as i64;
        let out_x_clamped = (out_x.round() as i64).clamp(-margin, w as i64 + margin);
        let out_y_clamped = (out_y.round() as i64).clamp(-margin, h as i64 + margin);

        // Get the glyph for the current cursor kind.
        let glyph = glyph_for_kind(cursor_frame.kind);

        // Scale glyph dimensions.
        let scale = clamped_scale;
        let scaled_w = ((glyph.width as f32 * scale).round() as i64).max(1);
        let scaled_h = ((glyph.height as f32 * scale).round() as i64).max(1);

        // Calculate glyph top-left position: output position minus hotspot offset.
        let draw_x = out_x_clamped - (glyph.hotspot_x * scale).round() as i64;
        let draw_y = out_y_clamped - (glyph.hotspot_y * scale).round() as i64;

        // Draw the glyph with alpha blending on Y plane.
        Self::draw_glyph_i64(
            data, w as i64, h as i64, linesize, draw_x, draw_y, scaled_w, scaled_h, glyph, scale,
        );

        // Click magnification ring: still drawn around hotspot.
        if clamped_scale > 1.05 {
            let ring_radius =
                ((self.cursor_radius * clamped_scale * 1.4).round() as i64).min(max_radius as i64);
            Self::draw_circle_outline_i64(
                data,
                w as i64,
                h as i64,
                linesize,
                out_x_clamped,
                out_y_clamped,
                ring_radius,
                180,
            );
        }
    }

    /// Binary search for the cursor frame closest to `timestamp_nanos`.
    fn find_cursor_frame(&self, timestamp_nanos: u64) -> Option<&CursorFrame> {
        let frames = &self.timeline.frames;
        if frames.is_empty() {
            return None;
        }
        // Find the frame with the largest timestamp <= target.
        let idx = match frames.binary_search_by_key(&timestamp_nanos, |f| f.timestamp.nanos) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        frames.get(idx)
    }

    /// Draw a filled circle on the Y plane using i64 coordinates.
    ///
    /// BUG-008: Uses i64 throughout to prevent overflow on extreme coordinates
    /// after coordinate mapping. This is the primary drawing function used by
    /// `draw_on_frame()` after clamping.
    #[allow(clippy::too_many_arguments)]
    fn draw_circle_i64(
        data: &mut [u8],
        frame_w: i64,
        frame_h: i64,
        linesize: usize,
        cx: i64,
        cy: i64,
        radius: i64,
        value: u8,
    ) {
        let r2 = radius * radius;

        let y_start = (cy - radius).max(0) as usize;
        let y_end = ((cy + radius).min(frame_h - 1)) as usize;
        let x_start = (cx - radius).max(0) as usize;
        let x_end = ((cx + radius).min(frame_w - 1)) as usize;

        for y in y_start..=y_end {
            let row_offset = y * linesize;
            for x in x_start..=x_end {
                let dx = x as i64 - cx;
                let dy = y as i64 - cy;
                if dx * dx + dy * dy <= r2 {
                    if let Some(pixel) = data.get_mut(row_offset + x) {
                        *pixel = value;
                    }
                }
            }
        }
    }

    /// Draw a circle outline (1px wide) on the Y plane using i64 coordinates.
    ///
    /// BUG-008: Uses i64 throughout to prevent overflow on extreme coordinates.
    #[allow(clippy::too_many_arguments)]
    fn draw_circle_outline_i64(
        data: &mut [u8],
        frame_w: i64,
        frame_h: i64,
        linesize: usize,
        cx: i64,
        cy: i64,
        radius: i64,
        value: u8,
    ) {
        let r2 = radius * radius;
        let inner_r = (radius - 1).max(0);
        let inner_r2 = inner_r * inner_r;

        let y_start = (cy - radius).max(0) as usize;
        let y_end = ((cy + radius).min(frame_h - 1)) as usize;
        let x_start = (cx - radius).max(0) as usize;
        let x_end = ((cx + radius).min(frame_w - 1)) as usize;

        for y in y_start..=y_end {
            let row_offset = y * linesize;
            for x in x_start..=x_end {
                let dx = x as i64 - cx;
                let dy = y as i64 - cy;
                let dist2 = dx * dx + dy * dy;
                if dist2 <= r2 && dist2 >= inner_r2 {
                    if let Some(pixel) = data.get_mut(row_offset + x) {
                        *pixel = value;
                    }
                }
            }
        }
    }

    /// Draw a cursor glyph onto the Y plane with nearest-neighbor scaling.
    ///
    /// Each glyph pixel is scaled by `scale` and drawn at the corresponding
    /// position. Alpha blending is done on the Y plane: existing pixel value
    /// is blended with the glyph's target value based on the glyph alpha.
    #[allow(clippy::too_many_arguments)]
    fn draw_glyph_i64(
        data: &mut [u8],
        frame_w: i64,
        frame_h: i64,
        linesize: usize,
        draw_x: i64,
        draw_y: i64,
        scaled_w: i64,
        scaled_h: i64,
        glyph: &CursorGlyph,
        scale: f32,
    ) {
        for sy in 0..scaled_h {
            let gy = (sy as f32 / scale).floor() as usize;
            if gy >= glyph.height {
                continue;
            }
            let py = draw_y + sy;
            if py < 0 || py >= frame_h {
                continue;
            }

            for sx in 0..scaled_w {
                let gx = (sx as f32 / scale).floor() as usize;
                if gx >= glyph.width {
                    continue;
                }
                let px = draw_x + sx;
                if px < 0 || px >= frame_w {
                    continue;
                }

                let glyph_pixel = &glyph.pixels[gy * glyph.width + gx];
                let target_y: u8 = match glyph_pixel {
                    GlyphPixel::Transparent => continue,
                    GlyphPixel::Black(a) => {
                        // Blend toward black (Y=16)
                        let a_f = *a as f32 / 255.0;
                        let existing = data[(py as usize) * linesize + px as usize] as f32;
                        (existing * (1.0 - a_f) + 16.0 * a_f).round() as u8
                    }
                    GlyphPixel::White(a) => {
                        // Blend toward white (Y=235)
                        let a_f = *a as f32 / 255.0;
                        let existing = data[(py as usize) * linesize + px as usize] as f32;
                        (existing * (1.0 - a_f) + 235.0 * a_f).round() as u8
                    }
                };
                data[(py as usize) * linesize + px as usize] = target_y;
            }
        }
    }
}

/// Load an `EffectTimeline` from a JSON file path.
pub fn load_effect_timeline(path: &std::path::Path) -> AppResult<EffectTimeline> {
    let json = std::fs::read_to_string(path).map_err(|e| AppError::ExportFailed {
        reason: format!("读取光标效果时间线失败: {e}"),
    })?;
    serde_json::from_str(&json).map_err(|e| AppError::ExportFailed {
        reason: format!("解析光标效果时间线失败: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{CursorFrame, CursorKind, EffectTimeline};

    fn make_timeline(frames: Vec<CursorFrame>, render: bool) -> EffectTimeline {
        EffectTimeline {
            fps: 30,
            duration_nanos: 1_000_000_000,
            frames,
            click_effects: Vec::new(),
            raw_system_cursor_visible: !render,
            render_cursor_overlay: render,
        }
    }

    fn cursor_frame(nanos: u64, x: f32, y: f32) -> CursorFrame {
        CursorFrame {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
            scale: 1.0,
            opacity: 1.0,
            kind: CursorKind::Arrow,
        }
    }

    #[test]
    fn renderer_returns_none_when_overlay_disabled() {
        let timeline = make_timeline(vec![cursor_frame(0, 100.0, 100.0)], false);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        );
        assert!(renderer.is_none());
    }

    #[test]
    fn renderer_returns_none_when_no_frames() {
        let timeline = make_timeline(vec![], true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        );
        assert!(renderer.is_none());
    }

    #[test]
    fn fit_with_bars_identity_mapping() {
        let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0)], true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        // Cursor at (960, 540) maps to (960, 540) with identity fit.
        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        // Verify arrow glyph is visible near cursor position (hotspot at tip).
        let y_plane = frame.data(0);
        let linesize = y_plane.len() / 1080;
        // Arrow hotspot is at (0,0), so glyph draws starting at cursor position.
        // The arrow tip pixel is black (Y=16). Check nearby pixels for glyph presence.
        let near_center = 540 * linesize + 960;
        let has_visible = y_plane[near_center..near_center + 50]
            .iter()
            .any(|&b| b > 0);
        assert!(
            has_visible,
            "arrow glyph should be visible near cursor position"
        );
    }

    #[test]
    fn center_crop_mapping() {
        // Source 1920x1080, output 1080x1920 (Douyin portrait).
        let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0)], true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1080,
            1920,
            ExportScalePolicy::CenterCrop,
            Some((657, 0)),
            None,
        );
        assert!(renderer.is_some());
    }

    #[test]
    fn center_crop_cursor_outside_region() {
        let timeline = make_timeline(vec![cursor_frame(0, 10.0, 10.0)], true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1080,
            1920,
            ExportScalePolicy::CenterCrop,
            Some((657, 0)),
            None,
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1080,
            1920,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        let y_plane = frame.data(0);
        assert!(y_plane.iter().all(|&b| b == 0));
    }

    #[test]
    fn find_cursor_frame_binary_search() {
        let frames = vec![
            cursor_frame(0, 0.0, 0.0),
            cursor_frame(33_333_333, 10.0, 10.0),
            cursor_frame(66_666_666, 20.0, 20.0),
            cursor_frame(100_000_000, 30.0, 30.0),
        ];
        let timeline = make_timeline(frames, true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        let f = renderer.find_cursor_frame(33_333_333).unwrap();
        assert!((f.x - 10.0).abs() < 0.01);

        let f = renderer.find_cursor_frame(50_000_000).unwrap();
        assert!((f.x - 10.0).abs() < 0.01);

        let f = renderer.find_cursor_frame(0).unwrap();
        assert!((f.x - 0.0).abs() < 0.01);

        let f = renderer.find_cursor_frame(200_000_000).unwrap();
        assert!((f.x - 30.0).abs() < 0.01);
    }

    #[test]
    fn cursor_with_click_scale_draws_ring() {
        let frames = vec![CursorFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            x: 960.0,
            y: 540.0,
            scale: 2.0, // Click magnification active.
            opacity: 0.35,
            kind: CursorKind::Arrow,
        }];
        let timeline = make_timeline(frames, true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        // The ring is drawn at radius * scale * 1.4 ≈ 12 * 2 * 1.4 ≈ 33.
        // Check a pixel on the ring outline (at ring_radius distance from center).
        let y_plane = frame.data(0);
        let ring_radius = (12.0_f32 * 2.0 * 1.4) as i32; // ≈ 33
                                                         // Find the actual linesize by checking how many bytes per row.
                                                         // YUV420P linesize may differ from width due to alignment.
        let linesize = y_plane.len() / 1080;
        let ring_y: usize = 540;
        let ring_x: usize = 960 + ring_radius as usize;
        let offset = ring_y * linesize + ring_x;
        // The ring outline (1px) should have the ring value (180).
        assert!(
            y_plane[offset] > 100,
            "ring pixel at ({ring_x}, {ring_y}) should be visible, got {}",
            y_plane[offset]
        );
    }

    #[test]
    fn cursor_overlay_skips_huge_finite_coordinates_without_panic() {
        // BUG-008: Huge finite coordinates after mapping must not panic.
        // The mapper returns (out_x, out_y) which could be very large finite values.
        let frames = vec![CursorFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            x: 999_999.0,
            y: 999_999.0,
            scale: 1.0,
            opacity: 1.0,
            kind: CursorKind::Arrow,
        }];
        let timeline = make_timeline(frames, true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        // Should not panic — cursor is drawn at clamped position or skipped.
        renderer.draw_on_frame(&mut frame, 0, 30);
    }

    #[test]
    fn cursor_overlay_handles_extreme_negative_coordinates_without_panic() {
        // BUG-008: Extreme negative coordinates must not cause underflow.
        let frames = vec![CursorFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            x: -999_999.0,
            y: -999_999.0,
            scale: 1.0,
            opacity: 1.0,
            kind: CursorKind::Arrow,
        }];
        let timeline = make_timeline(frames, true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        // Should not panic.
        renderer.draw_on_frame(&mut frame, 0, 30);
    }

    #[test]
    fn cursor_overlay_skips_mapped_infinite_coordinates() {
        // BUG-008: If source coordinates produce infinite mapped values, skip.
        let frames = vec![CursorFrame {
            timestamp: MediaTimestamp::from_nanos(0),
            x: f32::INFINITY,
            y: 540.0,
            scale: 1.0,
            opacity: 1.0,
            kind: CursorKind::Arrow,
        }];
        let timeline = make_timeline(frames, true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920,
            1080,
            1920,
            1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        // Frame should be untouched (all zeros).
        let y_plane = frame.data(0);
        assert!(
            y_plane.iter().all(|&b| b == 0),
            "infinite cursor should not draw"
        );
    }

    #[test]
    fn arrow_glyph_has_white_outline_and_black_fill() {
        let glyph = &ARROW_GLYPH;
        // Arrow tip pixel (0,0) should be white outline.
        let tip = &glyph.pixels[0];
        assert!(
            matches!(tip, GlyphPixel::White(_)),
            "arrow tip should be white outline"
        );
        // Interior pixel (row 10, col 5 — well inside the arrow body) should be black fill.
        let interior = &glyph.pixels[10 * glyph.width + 5];
        assert!(
            matches!(interior, GlyphPixel::Black(_)),
            "arrow interior should be black fill"
        );
    }

    #[test]
    fn hand_glyph_has_white_fill_and_black_outline() {
        let glyph = &HAND_GLYPH;
        // Find a white fill pixel (interior of hand).
        let has_white = glyph.pixels.iter().any(|p| matches!(p, GlyphPixel::White(_)));
        assert!(has_white, "hand should have white fill pixels");
        // Find a black outline pixel.
        let has_black = glyph.pixels.iter().any(|p| matches!(p, GlyphPixel::Black(_)));
        assert!(has_black, "hand should have black outline pixels");
    }

    #[test]
    fn ibeam_glyph_has_black_body_and_white_outline() {
        let glyph = &IBEAM_GLYPH;
        // Top bar edge should be white outline.
        let top_edge = &glyph.pixels[4]; // Row 0, col 4.
        assert!(
            matches!(top_edge, GlyphPixel::White(_)),
            "ibeam top edge should be white outline"
        );
        // Vertical stem should be black body.
        let stem = &glyph.pixels[5 * glyph.width + 7]; // Row 5, col 7 (center of stem).
        assert!(
            matches!(stem, GlyphPixel::Black(_)),
            "ibeam stem should be black body"
        );
    }

    #[test]
    fn rendered_arrow_has_correct_y_plane_values() {
        let timeline = make_timeline(vec![cursor_frame(0, 960.0, 540.0)], true);
        let renderer = CursorOverlayRenderer::new(
            timeline,
            1920, 1080, 1920, 1080,
            ExportScalePolicy::FitWithBars,
            None,
            Some((1920, 1080)),
        ).unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920, 1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);

        let y_plane = frame.data(0);
        let linesize = y_plane.len() / 1080;

        // Arrow tip at (960, 540) should be white outline (Y≈235).
        let tip_offset = 540 * linesize + 960;
        assert!(
            y_plane[tip_offset] > 200,
            "arrow tip should be white (Y>200), got {}",
            y_plane[tip_offset]
        );

        // Arrow interior (e.g., 960+5, 540+10 — row 10 has wide interior) should be black fill (Y≈16).
        let interior_offset = (540 + 10) * linesize + (960 + 5);
        assert!(
            y_plane[interior_offset] < 50,
            "arrow interior should be black (Y<50), got {}",
            y_plane[interior_offset]
        );
    }
}
