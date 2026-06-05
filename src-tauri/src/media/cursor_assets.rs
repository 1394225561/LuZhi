//! Cursor asset loader — loads PNG cursor images from the assets directory.
//!
//! At startup, loads all PNG files from `assets/cursors/` and caches them
//! as RGBA pixel data keyed by `CursorKind`. During export rendering,
//! the overlay compositor queries this cache to get the cursor bitmap
//! instead of using hardcoded glyph arrays.
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

/// Hotspot coordinates for each cursor kind.
///
/// These match the macOS system cursor hotspots:
/// - Arrow: tip at (0, 0) — top-left corner
/// - Hand: fingertip at ~(12, 4)
/// - IBeam: center at ~(8, 12)
fn default_hotspot(kind: CursorKind) -> (f32, f32) {
    match kind {
        CursorKind::Arrow => (0.0, 0.0),
        CursorKind::Hand => (12.0, 4.0),
        CursorKind::IBeam => (8.0, 12.0),
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

/// Load all cursor PNG assets from the given directory.
///
/// Returns a map from `CursorKind` to decoded `CursorAsset`.
/// Skips files that don't match known cursor filenames or fail to decode.
pub fn load_cursor_assets(assets_dir: &Path) -> HashMap<CursorKind, CursorAsset> {
    let mut map = HashMap::new();

    let entries = match std::fs::read_dir(assets_dir) {
        Ok(e) => e,
        Err(_) => return map,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "png" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(kind) = filename_to_kind(stem) else {
            continue;
        };

        match load_png_asset(&path, kind) {
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

/// Load a single PNG file and convert to `CursorAsset`.
fn load_png_asset(path: &Path, kind: CursorKind) -> Result<CursorAsset, image::ImageError> {
    let img = image::open(path)?.to_rgba8();
    let (width, height) = img.dimensions();
    let (hotspot_x, hotspot_y) = default_hotspot(kind);

    let pixels: Vec<RgbaPixel> = img
        .pixels()
        .map(|p| RgbaPixel {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
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
        assert_eq!(default_hotspot(CursorKind::Arrow), (0.0, 0.0));
        assert_eq!(default_hotspot(CursorKind::Hand), (12.0, 4.0));
        assert_eq!(default_hotspot(CursorKind::IBeam), (8.0, 12.0));
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
            "assets/cursors 目录不存在，请确保 PNG 光标素材已提交到仓库"
        );

        let assets = load_cursor_assets(&cursors_dir);
        assert!(
            assets.contains_key(&CursorKind::Arrow),
            "应加载到 arrow-cursor.png"
        );
        assert!(
            assets.contains_key(&CursorKind::Hand),
            "应加载到 hand-cursor.png"
        );
        assert!(
            assets.contains_key(&CursorKind::IBeam),
            "应加载到 ibeam-cursor.png"
        );

        let arrow = &assets[&CursorKind::Arrow];
        assert!(arrow.width > 0 && arrow.height > 0);
        assert_eq!(arrow.pixels.len() as u64, arrow.width as u64 * arrow.height as u64);
    }
}
