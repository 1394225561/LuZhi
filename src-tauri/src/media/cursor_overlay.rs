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
use crate::media::cursor_assets::CursorAsset;
use crate::media::export_presets::ExportScalePolicy;

/// Renders cursor overlay onto YUV420P video frames during export.
pub struct CursorOverlayRenderer {
    timeline: EffectTimeline,
    cursor_radius: f32,
    mapper: CursorCoordMapper,
    /// Pre-loaded cursor assets indexed by kind.
    cursor_assets: std::collections::HashMap<CursorKind, CursorAsset>,
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
        cursor_assets: std::collections::HashMap<CursorKind, CursorAsset>,
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
            cursor_assets,
        })
    }

    /// Get the source video PTS origin in nanoseconds.
    /// Used by trim_exporter to align decoded PTS with cursor timeline.
    pub fn source_pts_origin_nanos(&self) -> u64 {
        self.timeline.source_pts_origin_nanos
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

        // Read immutable dimensions before mutable data borrow.
        let w = frame.width() as i32;
        let h = frame.height() as i32;
        if w <= 0 || h <= 0 {
            return;
        }

        // Clamp radius: minimum 4px, maximum 256px or half the smallest dimension.
        let max_radius = (w.min(h) / 2).min(256);

        // BUG-008: Clamp mapped coordinates to a safe drawing range.
        // Allow margin for partially-visible cursors at edges.
        let margin = max_radius as i64;
        let out_x_clamped = (out_x.round() as i64).clamp(-margin, w as i64 + margin);
        let out_y_clamped = (out_y.round() as i64).clamp(-margin, h as i64 + margin);

        // Get the cursor asset for the current kind.
        let asset = match self.cursor_assets.get(&cursor_frame.kind) {
            Some(a) => a,
            None => return, // No asset loaded for this kind.
        };

        // Scale asset dimensions.
        let scale = clamped_scale;
        let scaled_w = ((asset.width as f32 * scale).round() as i64).max(1);
        let scaled_h = ((asset.height as f32 * scale).round() as i64).max(1);

        // Calculate asset top-left position: output position minus hotspot offset.
        let draw_x = out_x_clamped - (asset.hotspot_x * scale).round() as i64;
        let draw_y = out_y_clamped - (asset.hotspot_y * scale).round() as i64;

        // Draw the cursor asset with RGBA alpha blending on Y/U/V planes.
        Self::draw_rgba_cursor(
            frame, w as i64, h as i64, draw_x, draw_y, scaled_w, scaled_h, asset, scale,
            cursor_frame.opacity,
        );

        // Click magnification ring: still drawn around hotspot on Y plane only.
        if clamped_scale > 1.05 {
            let ring_radius =
                ((self.cursor_radius * clamped_scale * 1.4).round() as i64).min(max_radius as i64);
            let y_linesize = frame.stride(0);
            let data = frame.data_mut(0);
            Self::draw_circle_outline_i64(
                data,
                w as i64,
                h as i64,
                y_linesize,
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

    /// Draw a cursor asset onto YUV420P frames with RGBA alpha blending.
    ///
    /// For each pixel in the scaled cursor asset, performs standard
    /// "over" alpha compositing on the Y, U, and V planes sequentially.
    /// RGB-to-YUV conversion uses BT.601 coefficients.
    ///
    /// To avoid simultaneous mutable borrows of multiple frame planes,
    /// blend ops are collected first, then applied in three separate passes
    /// (Y, U, V).
    #[allow(clippy::too_many_arguments)]
    fn draw_rgba_cursor(
        frame: &mut ffmpeg_next::util::frame::Video,
        frame_w: i64,
        frame_h: i64,
        draw_x: i64,
        draw_y: i64,
        scaled_w: i64,
        scaled_h: i64,
        asset: &CursorAsset,
        scale: f32,
        opacity: f32,
    ) {
        // Collect blending operations to avoid simultaneous mutable borrows
        // of Y, U, V plane data. Each op: (py, px, y_val, u_val, v_val, alpha).
        let mut blend_ops: Vec<(usize, usize, f32, f32, f32, f32)> =
            Vec::with_capacity((scaled_w * scaled_h) as usize);

        for sy in 0..scaled_h {
            let gy = (sy as f32 / scale).floor() as u32;
            if gy >= asset.height {
                continue;
            }
            let py = draw_y + sy;
            if py < 0 || py >= frame_h {
                continue;
            }

            for sx in 0..scaled_w {
                let gx = (sx as f32 / scale).floor() as u32;
                if gx >= asset.width {
                    continue;
                }
                let px = draw_x + sx;
                if px < 0 || px >= frame_w {
                    continue;
                }

                let pixel = &asset.pixels[(gy * asset.width + gx) as usize];
                if pixel.a == 0 {
                    continue; // Fully transparent.
                }

                let alpha = (pixel.a as f32 / 255.0) * opacity;

                // NOTE: YUV420P chroma subsampling — each UV cell covers a 2x2 luma block.
                // When multiple semi-transparent cursor pixels map to the same UV cell,
                // the last pixel processed overwrites the previous blend. This produces
                // minor color fringing at cursor edges, which is acceptable for overlay use.

                // RGB to YUV (BT.601)
                let r = pixel.r as f32;
                let g = pixel.g as f32;
                let b = pixel.b as f32;
                let y_val = 0.299 * r + 0.587 * g + 0.114 * b;
                let u_val = -0.169 * r - 0.331 * g + 0.500 * b + 128.0;
                let v_val = 0.500 * r - 0.419 * g - 0.081 * b + 128.0;

                blend_ops.push((py as usize, px as usize, y_val, u_val, v_val, alpha));
            }
        }

        if blend_ops.is_empty() {
            return;
        }

        // Pass 1: blend Y plane.
        {
            let y_linesize = frame.stride(0);
            let y_data = frame.data_mut(0);
            for &(py, px, y_val, _, _, alpha) in &blend_ops {
                let inv_alpha = 1.0 - alpha;
                let y_offset = py * y_linesize + px;
                if let Some(existing) = y_data.get(y_offset) {
                    let blended = (y_val * alpha + *existing as f32 * inv_alpha).round() as u8;
                    y_data[y_offset] = blended;
                }
            }
        }

        // Pass 2: blend U plane (half resolution for YUV420P).
        {
            let u_linesize = frame.stride(1);
            let u_data = frame.data_mut(1);
            for &(py, px, _, u_val, _, alpha) in &blend_ops {
                let inv_alpha = 1.0 - alpha;
                let uv_y = py / 2;
                let uv_x = px / 2;
                let u_offset = uv_y * u_linesize + uv_x;
                if let Some(existing_u) = u_data.get(u_offset) {
                    let blended_u =
                        (u_val * alpha + *existing_u as f32 * inv_alpha).round() as u8;
                    u_data[u_offset] = blended_u;
                }
            }
        }

        // Pass 3: blend V plane (half resolution for YUV420P).
        {
            let v_linesize = frame.stride(2);
            let v_data = frame.data_mut(2);
            for &(py, px, _, _, v_val, alpha) in &blend_ops {
                let inv_alpha = 1.0 - alpha;
                let uv_y = py / 2;
                let uv_x = px / 2;
                let v_offset = uv_y * v_linesize + uv_x;
                if let Some(existing_v) = v_data.get(v_offset) {
                    let blended_v =
                        (v_val * alpha + *existing_v as f32 * inv_alpha).round() as u8;
                    v_data[v_offset] = blended_v;
                }
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
    use crate::media::cursor_assets::{CursorAsset, RgbaPixel};

    fn make_timeline(frames: Vec<CursorFrame>, render: bool) -> EffectTimeline {
        EffectTimeline {
            fps: 30,
            duration_nanos: 1_000_000_000,
            frames,
            click_effects: Vec::new(),
            raw_system_cursor_visible: !render,
            render_cursor_overlay: render,
            source_pts_origin_nanos: 0,
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

    fn cursor_frame_with_kind(nanos: u64, x: f32, y: f32, kind: CursorKind) -> CursorFrame {
        CursorFrame {
            timestamp: MediaTimestamp::from_nanos(nanos),
            x,
            y,
            scale: 1.0,
            opacity: 1.0,
            kind,
        }
    }

    /// Create a simple 8x8 opaque white test asset for a given kind.
    fn make_test_asset(kind: CursorKind) -> CursorAsset {
        let (hotspot_x, hotspot_y) = match kind {
            CursorKind::Arrow => (0.0, 0.0),
            CursorKind::Hand => (4.0, 2.0),
            CursorKind::IBeam => (4.0, 4.0),
        };
        let pixels = vec![
            RgbaPixel {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            };
            64
        ];
        CursorAsset {
            width: 8,
            height: 8,
            hotspot_x,
            hotspot_y,
            pixels,
        }
    }

    /// Build a cursor_assets HashMap with all three kinds.
    fn make_test_assets() -> std::collections::HashMap<CursorKind, CursorAsset> {
        let mut map = std::collections::HashMap::new();
        map.insert(CursorKind::Arrow, make_test_asset(CursorKind::Arrow));
        map.insert(CursorKind::Hand, make_test_asset(CursorKind::Hand));
        map.insert(CursorKind::IBeam, make_test_asset(CursorKind::IBeam));
        map
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
            make_test_assets(),
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
            make_test_assets(),
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
            make_test_assets(),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        // Verify cursor asset is visible near cursor position.
        let y_plane = frame.data(0);
        let linesize = y_plane.len() / 1080;
        let near_center = 540 * linesize + 960;
        let has_visible = y_plane[near_center..near_center + 8]
            .iter()
            .any(|&b| b > 0);
        assert!(
            has_visible,
            "cursor asset should be visible near cursor position"
        );
    }

    #[test]
    fn center_crop_mapping() {
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
            make_test_assets(),
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
            make_test_assets(),
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
            make_test_assets(),
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
            scale: 2.0,
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
            make_test_assets(),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        let y_plane = frame.data(0);
        let ring_radius = (12.0_f32 * 2.0 * 1.4) as i32;
        let linesize = y_plane.len() / 1080;
        let ring_y: usize = 540;
        let ring_x: usize = 960 + ring_radius as usize;
        let offset = ring_y * linesize + ring_x;
        assert!(
            y_plane[offset] > 100,
            "ring pixel at ({ring_x}, {ring_y}) should be visible, got {}",
            y_plane[offset]
        );
    }

    #[test]
    fn cursor_overlay_skips_huge_finite_coordinates_without_panic() {
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
            make_test_assets(),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
    }

    #[test]
    fn cursor_overlay_handles_extreme_negative_coordinates_without_panic() {
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
            make_test_assets(),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
    }

    #[test]
    fn cursor_overlay_skips_mapped_infinite_coordinates() {
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
            make_test_assets(),
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);
        let y_plane = frame.data(0);
        assert!(
            y_plane.iter().all(|&b| b == 0),
            "infinite cursor should not draw"
        );
    }

    #[test]
    fn rendered_arrow_rgba_blends_yuv_planes() {
        // Use the real PNG assets for this test.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = std::path::Path::new(manifest_dir).join("assets").join("cursors");
        let assets = crate::media::cursor_assets::load_cursor_assets(&cursors_dir);

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
            assets,
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        renderer.draw_on_frame(&mut frame, 0, 30);

        // Y plane should have visible pixels near cursor.
        let y_plane = frame.data(0);
        let linesize = frame.stride(0);
        let tip_offset = 540 * linesize + 960;
        let has_visible = y_plane[tip_offset..tip_offset + 10]
            .iter()
            .any(|&b| b > 0);
        assert!(
            has_visible,
            "arrow cursor should be visible on Y plane near cursor position"
        );
    }

    #[test]
    fn missing_asset_for_kind_is_noop() {
        // Provide an empty assets map — no assets for any kind.
        let empty_assets: std::collections::HashMap<CursorKind, CursorAsset> =
            std::collections::HashMap::new();

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
            empty_assets,
        )
        .unwrap();

        let mut frame = ffmpeg_next::util::frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            1920,
            1080,
        );
        // Should not panic, frame should be untouched.
        renderer.draw_on_frame(&mut frame, 0, 30);
        let y_plane = frame.data(0);
        assert!(
            y_plane.iter().all(|&b| b == 0),
            "no asset should result in no drawing"
        );
    }
}
