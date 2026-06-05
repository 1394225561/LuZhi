//! Cursor asset loader — loads SVG cursor images from the assets directory.
//!
//! At startup, loads all SVG files from `assets/cursors/`, rasterizes them
//! via `resvg`, and caches the result as RGBA pixel data keyed by `CursorKind`.
//! Hotspot coordinates are read from `cursors.toml` in the same directory.
//!
//! During export rendering, the overlay compositor queries this cache to get
//! the cursor bitmap instead of using hardcoded glyph arrays.
//!
//! ## Coordinate Space
//!
//! Cursor bitmaps are always provided in **logical (point) coordinates**.
//! The compositor multiplies by `scale_factor` to map to pixel coordinates.
//!
//! ## References
//!
//! - TigerBeetle pixel compositing blog post: <https://tigerbeetle.com/blog/2026-04-17-rendering-a-cursor/>

use crate::core::timeline::CursorKind;
use std::collections::HashMap;
use std::path::Path;

/// An RGBA pixel (pre-multiplied alpha is NOT used — straight alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaPixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// A loaded cursor asset with dimensions and hotspot metadata.
#[derive(Clone, Debug)]
pub struct CursorAsset {
    pub width: u32,
    pub height: u32,
    /// Hotspot X relative to top-left corner.
    pub hotspot_x: f32,
    /// Hotspot Y relative to top-left corner.
    pub hotspot_y: f32,
    /// Row-major RGBA pixel data, length = width * height.
    pub pixels: Vec<RgbaPixel>,
}

/// Fallback hotspot coordinates when `cursors.toml` is missing or incomplete.
///
/// These match the macOS system cursor hotspots:
/// - Arrow: tip at (4, 2)
/// - Hand: fingertip at ~(14, 4)
/// - IBeam: center at ~(8, 16)
fn default_hotspot(kind: CursorKind) -> (f32, f32) {
    match kind {
        CursorKind::Arrow => (4.0, 2.0),
        CursorKind::Hand => (14.0, 4.0),
        CursorKind::IBeam => (8.0, 16.0),
    }
}

/// Map from filename (without extension) to CursorKind.
fn filename_to_kind(name: &str) -> Option<CursorKind> {
    match name {
        "arrow-cursor" => Some(CursorKind::Arrow),
        "hand-cursor" => Some(CursorKind::Hand),
        "ibeam-cursor" => Some(CursorKind::IBeam),
        _ => None,
    }
}

/// Hotspot config entry parsed from `cursors.toml`.
#[derive(serde::Deserialize)]
struct HotspotEntry {
    hotspot_x: Option<f32>,
    hotspot_y: Option<f32>,
}

/// Top-level structure of `cursors.toml`.
#[derive(serde::Deserialize, Default)]
struct HotspotConfig {
    #[serde(rename = "arrow-cursor")]
    arrow_cursor: Option<HotspotEntry>,
    #[serde(rename = "hand-cursor")]
    hand_cursor: Option<HotspotEntry>,
    #[serde(rename = "ibeam-cursor")]
    ibeam_cursor: Option<HotspotEntry>,
}

/// Load hotspot overrides from `cursors.toml` in the assets directory.
///
/// Returns `None` if the file doesn't exist or fails to parse.
fn load_hotspot_config(assets_dir: &Path) -> Option<HotspotConfig> {
    let toml_path = assets_dir.join("cursors.toml");
    let content = std::fs::read_to_string(&toml_path).ok()?;
    toml::from_str(&content).ok()
}

/// Resolve hotspot coordinates for a given cursor kind.
///
/// Uses the TOML config if available, otherwise falls back to defaults.
fn resolve_hotspot(kind: CursorKind, config: &Option<HotspotConfig>) -> (f32, f32) {
    let entry = match config {
        Some(cfg) => match kind {
            CursorKind::Arrow => cfg.arrow_cursor.as_ref(),
            CursorKind::Hand => cfg.hand_cursor.as_ref(),
            CursorKind::IBeam => cfg.ibeam_cursor.as_ref(),
        },
        None => None,
    };

    match entry {
        Some(e) => (
            e.hotspot_x.unwrap_or(default_hotspot(kind).0),
            e.hotspot_y.unwrap_or(default_hotspot(kind).1),
        ),
        None => default_hotspot(kind),
    }
}

/// Load all cursor SVG assets from the given directory.
///
/// Returns a map from `CursorKind` to rasterized `CursorAsset`.
/// Skips files that don't match known cursor filenames or fail to rasterize.
pub fn load_cursor_assets(assets_dir: &Path) -> HashMap<CursorKind, CursorAsset> {
    let mut map = HashMap::new();

    let hotspot_config = load_hotspot_config(assets_dir);

    let entries = match std::fs::read_dir(assets_dir) {
        Ok(e) => e,
        Err(_) => return map,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "svg" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(kind) = filename_to_kind(stem) else {
            continue;
        };

        let (hotspot_x, hotspot_y) = resolve_hotspot(kind, &hotspot_config);

        match load_svg_asset(&path, hotspot_x, hotspot_y) {
            Ok(asset) => {
                map.insert(kind, asset);
            }
            Err(e) => {
                eprintln!(
                    "[cursor-assets] 加载光标素材失败: {} — {}",
                    path.display(),
                    e
                );
            }
        }
    }

    map
}

/// Load a single SVG file, rasterize it via resvg, and convert to `CursorAsset`.
///
/// The SVG is rendered at its native `width`/`height` attributes (no scaling).
fn load_svg_asset(
    path: &Path,
    hotspot_x: f32,
    hotspot_y: f32,
) -> Result<CursorAsset, Box<dyn std::error::Error>> {
    let svg_data = std::fs::read(path)?;
    let tree = resvg::usvg::Tree::from_data(&svg_data, &resvg::usvg::Options::default())?;

    let size = tree.size();
    let width = size.width() as u32;
    let height = size.height() as u32;

    if width == 0 || height == 0 {
        return Err("SVG has zero dimensions".into());
    }

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or("failed to create pixmap")?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    let pixels: Vec<RgbaPixel> = pixmap
        .pixels()
        .iter()
        .map(|p| RgbaPixel {
            r: p.red(),
            g: p.green(),
            b: p.blue(),
            a: p.alpha(),
        })
        .collect();

    Ok(CursorAsset {
        width,
        height,
        hotspot_x,
        hotspot_y,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotspots_are_correct() {
        assert_eq!(default_hotspot(CursorKind::Arrow), (4.0, 2.0));
        assert_eq!(default_hotspot(CursorKind::Hand), (14.0, 4.0));
        assert_eq!(default_hotspot(CursorKind::IBeam), (8.0, 16.0));
    }

    #[test]
    fn filename_to_kind_maps_known_files() {
        assert_eq!(filename_to_kind("arrow-cursor"), Some(CursorKind::Arrow));
        assert_eq!(filename_to_kind("hand-cursor"), Some(CursorKind::Hand));
        assert_eq!(filename_to_kind("ibeam-cursor"), Some(CursorKind::IBeam));
        assert_eq!(filename_to_kind("unknown"), None);
    }

    #[test]
    fn load_from_real_assets_directory() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = Path::new(manifest_dir).join("assets").join("cursors");

        assert!(
            cursors_dir.exists(),
            "assets/cursors 目录不存在，请确保 SVG 光标素材已提交到仓库"
        );

        let assets = load_cursor_assets(&cursors_dir);
        assert!(
            assets.contains_key(&CursorKind::Arrow),
            "应加载到 arrow-cursor.svg"
        );
        assert!(
            assets.contains_key(&CursorKind::Hand),
            "应加载到 hand-cursor.svg"
        );
        assert!(
            assets.contains_key(&CursorKind::IBeam),
            "应加载到 ibeam-cursor.svg"
        );

        let arrow = &assets[&CursorKind::Arrow];
        assert!(arrow.width > 0 && arrow.height > 0);
        assert_eq!(
            arrow.pixels.len() as u64,
            arrow.width as u64 * arrow.height as u64
        );
    }

    #[test]
    fn hotspot_config_from_toml_overrides_defaults() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = Path::new(manifest_dir).join("assets").join("cursors");

        let assets = load_cursor_assets(&cursors_dir);
        let arrow = &assets[&CursorKind::Arrow];
        // cursors.toml defines arrow hotspot at (4.0, 2.0)
        assert_eq!(arrow.hotspot_x, 4.0);
        assert_eq!(arrow.hotspot_y, 2.0);
    }
}
