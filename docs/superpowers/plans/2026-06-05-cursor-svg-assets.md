# Cursor SVG 素材替换计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将光标素材格式从 PNG 替换为 SVG，启动时预光栅化为位图缓存，提升光标在任意缩放下的显示质量。

**Architecture:** 替换 `image` 依赖为 `resvg`（纯 Rust SVG 渲染器）。`cursor_assets.rs` 改为加载 SVG 文件，在启动时按固定倍率（如 4x）光栅化为 RGBA 位图。热点坐标通过 `cursors.toml` 侧文件配置。导出渲染管线（`cursor_overlay.rs`）无需改动——它消费的仍是 `CursorAsset { pixels, width, height, ... }`。

**Tech Stack:** Rust, `resvg` + `usvg` + `tiny-skia`（SVG 光栅化）, `toml`（热点配置解析）

---

## 文件结构

| 操作 | 文件路径 | 职责 |
|---|---|---|
| **修改** | `src-tauri/Cargo.toml` | 替换 `image` 为 `resvg` + `toml` |
| **修改** | `src-tauri/src/media/cursor_assets.rs` | SVG 加载 + 光栅化逻辑 |
| **替换** | `src-tauri/assets/cursors/*.png` → `*.svg` | SVG 素材文件 |
| **新增** | `src-tauri/assets/cursors/cursors.toml` | 热点坐标配置 |
| **不变** | `src-tauri/src/media/cursor_overlay.rs` | 消费 `CursorAsset`，无需改动 |

---

### Task 1: 替换依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 替换 `image` 为 `resvg` + `toml`**

将 Cargo.toml 中的：

```toml
# PNG decoding for cursor assets
image = { version = "0.25", default-features = false, features = ["png"] }
```

替换为：

```toml
# SVG rasterization for cursor assets
resvg = "0.44"
toml = "0.8"
```

> `resvg` 自带 `usvg` 和 `tiny-skia`，无需单独声明。纯 Rust，无 C 依赖。

- [ ] **Step 2: 验证依赖可解析**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore(deps): 替换 image 为 resvg + toml，支持 SVG 光标素材"
```

---

### Task 2: 创建 SVG 素材和热点配置

**Files:**
- Replace: `src-tauri/assets/cursors/arrow-cursor.png` → `arrow-cursor.svg`
- Replace: `src-tauri/assets/cursors/hand-cursor.png` → `hand-cursor.svg`
- Replace: `src-tauri/assets/cursors/ibeam-cursor.png` → `ibeam-cursor.svg`
- Create: `src-tauri/assets/cursors/cursors.toml`

- [ ] **Step 1: 删除旧 PNG 文件**

```bash
rm src-tauri/assets/cursors/*.png
```

- [ ] **Step 2: 创建占位 SVG 文件**

> 用户后续会替换为设计稿。以下为功能占位，确保管线可跑通。

`arrow-cursor.svg`:
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
  <path d="M4 2 L4 26 L10 20 L16 28 L20 26 L14 18 L22 18 Z"
        fill="black" stroke="white" stroke-width="1.5" stroke-linejoin="round"/>
</svg>
```

`hand-cursor.svg`:
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
  <path d="M12 4 C12 2 14 2 14 4 L14 16 L16 4 C16 2 18 2 18 4 L18 16 L20 6 C20 4 22 4 22 6 L22 18 L24 12 C24 10 26 10 26 12 L26 22 C26 28 20 30 14 30 C8 30 6 26 6 22 L6 14 C6 12 8 12 8 14 L8 18 L10 16 L10 6 C10 4 12 4 12 6 Z"
        fill="white" stroke="black" stroke-width="1"/>
</svg>
```

`ibeam-cursor.svg`:
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="32" viewBox="0 0 16 32">
  <rect x="6" y="2" width="4" height="28" rx="1" fill="black"/>
  <rect x="3" y="1" width="10" height="4" rx="1" fill="black" stroke="white" stroke-width="1"/>
  <rect x="3" y="27" width="10" height="4" rx="1" fill="black" stroke="white" stroke-width="1"/>
</svg>
```

- [ ] **Step 3: 创建热点配置文件**

`cursors.toml`:
```toml
# 光标热点坐标配置
# hotspot_x/y 为相对于 SVG 视口左上角的像素坐标

[arrow-cursor]
hotspot_x = 4.0
hotspot_y = 2.0

[hand-cursor]
hotspot_x = 14.0
hotspot_y = 4.0

[ibeam-cursor]
hotspot_x = 8.0
hotspot_y = 16.0
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/assets/cursors/
git commit -m "feat(cursor): 替换 PNG 为 SVG 光标素材和热点配置"
```

---

### Task 3: 改造 `cursor_assets.rs` — SVG 加载和光栅化

**Files:**
- Modify: `src-tauri/src/media/cursor_assets.rs`

- [ ] **Step 1: 替换 import 和模块文档**

将整个文件内容替换为：

```rust
//! Cursor asset loader — loads SVG cursor images and rasterizes them at startup.
//!
//! At startup, loads all SVG files from `assets/cursors/`, rasterizes them
//! to RGBA bitmaps at a fixed scale factor, and caches the result keyed
//! by `CursorKind`. Hotspot coordinates are read from `cursors.toml`.
//!
//! ## Rasterization
//!
//! SVGs are rasterized at 4× their intrinsic size (viewBox dimensions)
//! to ensure crisp rendering at typical video export resolutions.
//! The rasterized bitmap is stored as `CursorAsset.pixels` (RGBA, row-major).
//!
//! ## Coordinate Space
//!
//! Hotspot coordinates are in **rasterized pixel coordinates** (after scaling).

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
    /// Hotspot X relative to top-left corner (in rasterized pixel coords).
    pub hotspot_x: f32,
    /// Hotspot Y relative to top-left corner (in rasterized pixel coords).
    pub hotspot_y: f32,
    /// Row-major RGBA pixel data, length = width * height.
    pub pixels: Vec<RgbaPixel>,
}

/// Rasterization scale factor applied to SVG intrinsic dimensions.
const RASTER_SCALE: f32 = 4.0;

/// Hotspot configuration entry from `cursors.toml`.
#[derive(serde::Deserialize)]
struct HotspotConfig {
    hotspot_x: f32,
    hotspot_y: f32,
}

/// Top-level TOML structure: filename → hotspot config.
#[derive(serde::Deserialize)]
struct CursorsToml {
    #[serde(rename = "arrow-cursor")]
    arrow_cursor: Option<HotspotConfig>,
    #[serde(rename = "hand-cursor")]
    hand_cursor: Option<HotspotConfig>,
    #[serde(rename = "ibeam-cursor")]
    ibeam_cursor: Option<HotspotConfig>,
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

/// Load hotspot config from `cursors.toml` in the assets directory.
fn load_hotspot_config(assets_dir: &Path) -> HashMap<CursorKind, (f32, f32)> {
    let toml_path = assets_dir.join("cursors.toml");
    let content = match std::fs::read_to_string(&toml_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let config: CursorsToml = match toml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[cursor-assets] 解析 cursors.toml 失败: {e}");
            return HashMap::new();
        }
    };

    let mut map = HashMap::new();
    if let Some(cfg) = config.arrow_cursor {
        map.insert(CursorKind::Arrow, (cfg.hotspot_x, cfg.hotspot_y));
    }
    if let Some(cfg) = config.hand_cursor {
        map.insert(CursorKind::Hand, (cfg.hotspot_x, cfg.hotspot_y));
    }
    if let Some(cfg) = config.ibeam_cursor {
        map.insert(CursorKind::IBeam, (cfg.hotspot_x, cfg.hotspot_y));
    }
    map
}

/// Load all cursor SVG assets from the given directory.
///
/// For each `.svg` file with a known filename, rasterizes at `RASTER_SCALE`
/// and uses hotspot from `cursors.toml` (falls back to (0, 0) if missing).
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

        let (hotspot_x, hotspot_y) = hotspot_config.get(&kind).copied().unwrap_or((0.0, 0.0));

        match load_svg_asset(&path, kind, hotspot_x * RASTER_SCALE, hotspot_y * RASTER_SCALE) {
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

/// Rasterize a single SVG file to `CursorAsset`.
///
/// The SVG is rendered at `intrinsic_size × RASTER_SCALE` pixels.
fn load_svg_asset(
    path: &Path,
    _kind: CursorKind,
    hotspot_x: f32,
    hotspot_y: f32,
) -> Result<CursorAsset, String> {
    let svg_data = std::fs::read(path).map_err(|e| format!("读取 SVG 文件失败: {e}"))?;

    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&svg_data, &opt).map_err(|e| format!("解析 SVG 失败: {e}"))?;

    let svg_size = tree.size();
    let width = (svg_size.width() * RASTER_SCALE).round() as u32;
    let height = (svg_size.height() * RASTER_SCALE).round() as u32;

    if width == 0 || height == 0 {
        return Err("SVG 光栅化尺寸为零".to_string());
    }

    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or("创建像素图失败")?;

    let scale = resvg::Transform::from_scale(RASTER_SCALE, RASTER_SCALE);
    resvg::render(&tree, scale, &mut pixmap.as_mut());

    let pixels: Vec<RgbaPixel> = pixmap
        .data()
        .chunks_exact(4)
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
    fn rasterized_svg_has_non_zero_dimensions() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = Path::new(manifest_dir).join("assets").join("cursors");

        if !cursors_dir.exists() {
            return;
        }

        let assets = load_cursor_assets(&cursors_dir);
        for (kind, asset) in &assets {
            assert!(
                asset.width > 0 && asset.height > 0,
                "{kind:?} 光栅化尺寸为零"
            );
            assert!(
                asset.pixels.iter().any(|p| p.a > 0),
                "{kind:?} 光栅化结果全透明"
            );
        }
    }

    #[test]
    fn hotspot_config_from_toml_is_applied() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = Path::new(manifest_dir).join("assets").join("cursors");

        if !cursors_dir.exists() {
            return;
        }

        let assets = load_cursor_assets(&cursors_dir);
        if let Some(arrow) = assets.get(&CursorKind::Arrow) {
            // Arrow hotspot should be (4, 2) in SVG coords × 4x scale = (16, 8)
            assert!(
                arrow.hotspot_x > 0.0 || arrow.hotspot_y > 0.0,
                "arrow 热点应从 cursors.toml 加载"
            );
        }
    }
}
```

- [ ] **Step 2: 验证编译通过**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过。

- [ ] **Step 3: 运行测试**

```bash
cd src-tauri && cargo test cursor_assets 2>&1 | tail -15
```

Expected: 4 个测试全部 PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/cursor_assets.rs
git commit -m "feat(cursor): 改造 cursor_assets 支持 SVG 加载和光栅化"
```

---

### Task 4: 验证全量测试和集成

**Files:**
- No file changes

- [ ] **Step 1: 运行全量测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -10
```

Expected: 所有测试 PASS。

- [ ] **Step 2: 验证 cursor_overlay 无需改动**

确认 `cursor_overlay.rs` 中没有任何对 `image` crate 或 PNG 格式的直接引用——它只消费 `CursorAsset` 结构体。

```bash
grep -n "image\|png\|PNG" src-tauri/src/media/cursor_overlay.rs || echo "无 PNG 相关引用"
```

Expected: 无输出。

- [ ] **Step 3: 代码审查 checklist**

- [ ] `resvg` 渲染路径无 `unwrap()`（使用 `map_err` + `?`）
- [ ] `cursors.toml` 解析失败时优雅降级（热点默认为 0,0）
- [ ] SVG 光栅化尺寸为零时返回错误而非 panic
- [ ] `RASTER_SCALE` 常量有文档注释
- [ ] 遵循 BUG.md 中所有预防规则

- [ ] **Step 4: Commit (最终)**

```bash
git add -A
git commit -m "feat(cursor): 完成 SVG 光标素材替换 PNG 改造"
```

---

## 验收标准

1. `assets/cursors/` 目录下为 `.svg` 文件 + `cursors.toml`，无 `.png` 文件
2. `Cargo.toml` 中无 `image` 依赖，有 `resvg` + `toml` 依赖
3. `cursor_assets.rs` 使用 `resvg` 光栅化 SVG，热点从 TOML 读取
4. `cursor_overlay.rs` 无需任何改动（仍消费 `CursorAsset`）
5. 所有测试通过
6. SVG 光栅化在启动时完成，不在导出渲染热路径中
