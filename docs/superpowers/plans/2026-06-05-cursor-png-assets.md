# Cursor PNG 素材替换改造计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `cursor_overlay.rs` 中硬编码的静态 `GlyphPixel` 像素数组替换为从 PNG 文件加载的 RGBA 光标素材，提升光标渲染的视觉质量和可维护性。

**Architecture:** 在 `src-tauri/src/media/` 下新增 `cursor_assets.rs` 模块，负责启动时从 `assets/cursors/` 目录加载 PNG 文件并解码为 RGBA 像素数据。`cursor_overlay.rs` 中的 `GlyphPixel` / `CursorGlyph` 结构体替换为 PNG 驱动的 `CursorAsset` / `RgbaPixel`，渲染逻辑从 3 值枚举 alpha 混合改为标准 RGBA alpha 混合（含 RGB→YUV 转换）。

**Tech Stack:** Rust, `image` crate (PNG 解码), ffmpeg-next (YUV420P 帧操作)

---

## 文件结构

| 操作 | 文件路径 | 职责 |
|---|---|---|
| **新增** | `src-tauri/src/media/cursor_assets.rs` | PNG 加载、解码、缓存光标素材 |
| **修改** | `src-tauri/Cargo.toml` | 添加 `image` 依赖 |
| **修改** | `src-tauri/src/media/cursor_overlay.rs` | 移除 `GlyphPixel`/`CursorGlyph`，改用 `cursor_assets` 渲染 |
| **修改** | `src-tauri/src/media/mod.rs` | 注册 `cursor_assets` 模块 |
| **不变** | `src-tauri/src/core/timeline.rs` | `CursorKind` 枚举无需改动（已有 Arrow/Hand/IBeam） |
| **不变** | `src-tauri/src/platform/macos/cursor_kind.rs` | 光标检测逻辑无需改动 |

---

### Task 1: 添加 `image` 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 在 `[dependencies]` 中添加 `image` crate**

在 `Cargo.toml` 的 `[dependencies]` 段落末尾添加：

```toml
# PNG decoding for cursor assets
image = { version = "0.25", default-features = false, features = ["png"] }
```

> 使用 `default-features = false` + `features = ["png"]` 最小化依赖，只引入 PNG 解码能力。

- [ ] **Step 2: 验证依赖可解析**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过（可能有 warning，但无 error）。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore(deps): 添加 image crate 用于 PNG 光标素材解码"
```

---

### Task 2: 创建 `cursor_assets.rs` 模块

**Files:**
- Create: `src-tauri/src/media/cursor_assets.rs`

- [ ] **Step 1: 编写 `cursor_assets.rs` 骨架**

```rust
//! Cursor asset loader — loads PNG cursor images from the assets directory.
//!
//! At startup, loads all PNG files from `assets/cursors/` and caches them
//! as RGBA pixel data keyed by `CursorKind`. During export rendering,
//! the overlay compositor queries this cache to get the cursor bitmap
//! instead of using hardcoded glyph arrays.

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
        // 使用项目中实际的 assets/cursors 目录
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cursors_dir = Path::new(manifest_dir).join("assets").join("cursors");

        if !cursors_dir.exists() {
            eprintln!("跳过测试：assets/cursors 目录不存在");
            return;
        }

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

        // 验证解码后的尺寸合理
        let arrow = &assets[&CursorKind::Arrow];
        assert!(arrow.width > 0 && arrow.height > 0);
        assert_eq!(arrow.pixels.len() as u64, arrow.width as u64 * arrow.height as u64);
    }
}
```

- [ ] **Step 2: 在 `mod.rs` 中注册模块**

找到 `src-tauri/src/media/mod.rs`，添加 `pub mod cursor_assets;`。

- [ ] **Step 3: 验证编译通过**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过。

- [ ] **Step 4: 运行新模块的单元测试**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo test cursor_assets -- 2>&1 | tail -15
```

Expected: 3 个测试全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_assets.rs src-tauri/src/media/mod.rs
git commit -m "feat(cursor): 新增 cursor_assets 模块加载 PNG 光标素材"
```

---

### Task 3: 重构 `cursor_overlay.rs` —— 移除静态 Glyph 系统

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs:1-60` (glyph 数据定义区域)

- [ ] **Step 1: 替换 import 和数据结构**

将 `cursor_overlay.rs` 中的以下内容：

```rust
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
```

替换为：

```rust
use crate::app::error::{AppError, AppResult};
use crate::core::timeline::{CursorFrame, CursorKind, EffectTimeline};
use crate::media::cursor_assets::{CursorAsset, RgbaPixel};
use crate::media::export_presets::ExportScalePolicy;
```

- [ ] **Step 2: 删除全部静态 glyph 像素数据**

删除从 `// Shorthand constants for the pixel arrays.` 开始到 `IBEAM_PIXELS` 数组结束（`cursor_overlay.rs` 第 59-241 行）的所有代码。包括：
- `B`, `W`, `T` 常量
- `ARROW_GLYPH` / `ARROW_PIXELS`
- `HAND_GLYPH` / `HAND_PIXELS`
- `IBEAM_GLYPH` / `IBEAM_PIXELS`

- [ ] **Step 3: 验证编译失败（预期行为）**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo check 2>&1 | grep "error\[" | head -5
```

Expected: 多个编译错误（`GlyphPixel` undefined, `CursorGlyph` undefined 等）。

- [ ] **Step 4: Commit (中间状态，便于回滚)**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "refactor(cursor): 移除静态 GlyphPixel 像素数据（编译暂不通过）"
```

---

### Task 4: 重构 `cursor_overlay.rs` —— 改造渲染器使用 PNG 素材

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs`

- [ ] **Step 1: 改造 `CursorOverlayRenderer` 结构体**

将 `CursorOverlayRenderer` 从：

```rust
pub struct CursorOverlayRenderer {
    timeline: EffectTimeline,
    cursor_radius: f32,
    mapper: CursorCoordMapper,
}
```

改为：

```rust
pub struct CursorOverlayRenderer {
    timeline: EffectTimeline,
    cursor_radius: f32,
    mapper: CursorCoordMapper,
    /// Pre-loaded cursor assets indexed by kind.
    cursor_assets: std::collections::HashMap<CursorKind, CursorAsset>,
}
```

- [ ] **Step 2: 改造 `CursorOverlayRenderer::new()`**

在 `new()` 方法中增加 `cursor_assets` 参数：

```rust
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
```

- [ ] **Step 3: 改造 `draw_on_frame()` 中的 glyph 查找逻辑**

将 `draw_on_frame()` 中的：

```rust
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
```

替换为：

```rust
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
let draw_y = out_y_clamped - (asset.hotspot_y * scale). round() as i64;

// Draw the cursor asset with RGBA alpha blending on Y/U/V planes.
Self::draw_rgba_cursor(
    frame, w as i64, h as i64, linesize, draw_x, draw_y, scaled_w, scaled_h, asset, scale,
);
```

- [ ] **Step 4: 替换 `draw_glyph_i64` 为 `draw_rgba_cursor`**

删除旧的 `draw_glyph_i64` 方法，替换为新的 RGBA 渲染方法：

```rust
/// Draw a cursor asset onto YUV420P frames with RGBA alpha blending.
///
/// For each pixel in the scaled cursor asset, performs standard
/// "over" alpha compositing on the Y, U, and V planes simultaneously.
/// RGB→YUV conversion uses BT.601 coefficients.
#[allow(clippy::too_many_arguments)]
fn draw_rgba_cursor(
    frame: &mut ffmpeg_next::util::frame::Video,
    frame_w: i64,
    frame_h: i64,
    _linesize: usize,
    draw_x: i64,
    draw_y: i64,
    scaled_w: i64,
    scaled_h: i64,
    asset: &CursorAsset,
    scale: f32,
) {
    let y_data = frame.data_mut(0);
    let u_data = frame.data_mut(1);
    let v_data = frame.data_mut(2);
    let y_linesize = frame.stride(0);
    let u_linesize = frame.stride(1);
    let v_linesize = frame.stride(2);

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

            let alpha = pixel.a as f32 / 255.0;
            let inv_alpha = 1.0 - alpha;

            // RGB → YUV (BT.601)
            let r = pixel.r as f32;
            let g = pixel.g as f32;
            let b = pixel.b as f32;
            let y_val = 0.299 * r + 0.587 * g + 0.114 * b;
            let u_val = -0.169 * r - 0.331 * g + 0.500 * b + 128.0;
            let v_val = 0.500 * r - 0.419 * g - 0.081 * b + 128.0;

            // Blend Y plane
            let py_usize = py as usize;
            let px_usize = px as usize;
            let y_offset = py_usize * y_linesize + px_usize;
            if let Some(existing) = y_data.get(y_offset) {
                let blended = (y_val * alpha + *existing as f32 * inv_alpha).round() as u8;
                y_data[y_offset] = blended;
            }

            // Blend U/V planes (half resolution for YUV420P)
            let uv_y = py_usize / 2;
            let uv_x = px_usize / 2;
            let u_offset = uv_y * u_linesize + uv_x;
            let v_offset = uv_y * v_linesize + uv_x;
            if let Some(existing_u) = u_data.get(u_offset) {
                let blended_u = (u_val * alpha + *existing_u as f32 * inv_alpha).round() as u8;
                u_data[u_offset] = blended_u;
            }
            if let Some(existing_v) = v_data.get(v_offset) {
                let blended_v = (v_val * alpha + *existing_v as f32 * inv_alpha).round() as u8;
                v_data[v_offset] = blended_v;
            }
        }
    }
}
```

- [ ] **Step 5: 验证编译通过**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过（可能有 warning）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "feat(cursor): 改造 overlay 渲染器使用 PNG RGBA 素材"
```

---

### Task 5: 更新所有 `CursorOverlayRenderer::new()` 调用点

**Files:**
- Modify: `src-tauri/src/media/trim_exporter.rs` (调用 `CursorOverlayRenderer::new` 的位置)

- [ ] **Step 1: 查找所有调用点**

```bash
cd /Users/root-mac/workspace_github/LuZhi && grep -rn "CursorOverlayRenderer::new" src-tauri/
```

Expected: 找到 `trim_exporter.rs` 中的调用。

- [ ] **Step 2: 在调用点加载光标素材并传入**

在 `trim_exporter.rs` 中找到 `CursorOverlayRenderer::new(...)` 调用，在其上方添加素材加载逻辑：

```rust
// 加载光标 PNG 素材
let cursor_assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("assets")
    .join("cursors");
let cursor_assets = crate::media::cursor_assets::load_cursor_assets(&cursor_assets_dir);
```

然后在 `CursorOverlayRenderer::new(...)` 调用中追加 `cursor_assets` 参数。

- [ ] **Step 3: 验证编译通过**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo check 2>&1 | tail -5
```

Expected: 编译通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/media/trim_exporter.rs
git commit -m "feat(cursor): 在导出管线中传入 PNG 光标素材"
```

---

### Task 6: 更新现有测试

**Files:**
- Modify: `src-tauri/src/media/cursor_overlay.rs` (测试模块)

- [ ] **Step 1: 创建测试辅助函数**

在 `cursor_overlay.rs` 的 `#[cfg(test)] mod tests` 中，添加创建空素材 map 的辅助函数：

```rust
fn empty_cursor_assets() -> std::collections::HashMap<CursorKind, CursorAsset> {
    std::collections::HashMap::new()
}

/// Create a minimal test cursor asset (4x4 solid white with full alpha).
fn test_cursor_asset(kind: CursorKind) -> CursorAsset {
    let (hotspot_x, hotspot_y) = match kind {
        CursorKind::Arrow => (0.0, 0.0),
        CursorKind::Hand => (2.0, 1.0),
        CursorKind::IBeam => (2.0, 2.0),
    };
    CursorAsset {
        width: 4,
        height: 4,
        hotspot_x,
        hotspot_y,
        pixels: vec![
            RgbaPixel { r: 255, g: 255, b: 255, a: 255 };
            16
        ],
    }
}

fn test_cursor_assets_all() -> std::collections::HashMap<CursorKind, CursorAsset> {
    let mut map = std::collections::HashMap::new();
    map.insert(CursorKind::Arrow, test_cursor_asset(CursorKind::Arrow));
    map.insert(CursorKind::Hand, test_cursor_asset(CursorKind::Hand));
    map.insert(CursorKind::IBeam, test_cursor_asset(CursorKind::IBeam));
    map
}
```

- [ ] **Step 2: 更新所有 `CursorOverlayRenderer::new()` 测试调用**

将测试中所有：

```rust
CursorOverlayRenderer::new(
    timeline,
    1920, 1080, 1920, 1080,
    ExportScalePolicy::FitWithBars,
    None,
    Some((1920, 1080)),
)
```

替换为：

```rust
CursorOverlayRenderer::new(
    timeline,
    1920, 1080, 1920, 1080,
    ExportScalePolicy::FitWithBars,
    None,
    Some((1920, 1080)),
    test_cursor_assets_all(),
)
```

对 `renderer_returns_none_when_overlay_disabled` 和 `renderer_returns_none_when_no_frames` 测试使用 `empty_cursor_assets()`。

- [ ] **Step 3: 更新 Y 平面断言为 YUV 断言**

由于新渲染器同时写入 Y/U/V 平面，部分测试的断言需要调整。例如 `rendered_arrow_has_correct_y_plane_values` 中：

- 白色光标像素的 Y 值断言仍有效（Y > 200）
- 黑色光标像素的 Y 值断言需要更新（新渲染器的白色素材 → Y≈235）

> 注意：由于测试用的是 4x4 纯白素材（而非旧的黑白箭头 glyph），`fit_with_bars_identity_mapping` 等测试的断言需要相应调整——白色素材渲染后 Y 平面应为高值。

- [ ] **Step 4: 运行全部光标相关测试**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo test cursor_overlay -- 2>&1 | tail -20
```

Expected: 所有测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media/cursor_overlay.rs
git commit -m "test(cursor): 更新 overlay 测试适配 PNG 素材渲染"
```

---

### Task 7: 使用真实 PNG 素材进行集成验证

**Files:**
- Test: 手动验证导出效果

- [ ] **Step 1: 运行全量测试确保无回归**

```bash
cd /Users/root-mac/workspace_github/LuZhi/src-tauri && cargo test 2>&1 | tail -10
```

Expected: 所有测试 PASS。

- [ ] **Step 2: 检查 PNG 素材尺寸一致性**

```bash
cd /Users/root-mac/workspace_github/LuZhi && file src-tauri/assets/cursors/*.png
```

Expected: 确认三个 PNG 文件的尺寸。如果尺寸不统一，记录到 HOTSPOT 配置中。

- [ ] **Step 3: 代码审查 checklist**

- [ ] `cursor_assets.rs` 的 `load_cursor_assets` 在 PNG 读取失败时不会 panic
- [ ] `draw_rgba_cursor` 中 alpha=0 的像素被跳过（性能优化）
- [ ] UV 平面的半分辨率坐标正确（`/ 2`）
- [ ] `CursorOverlayRenderer::new` 的签名变更已同步到所有调用点
- [ ] 旧的 `GlyphPixel` / `CursorGlyph` 类型已完全移除，无残留引用
- [ ] 遵循 BUG.md 中所有预防规则

- [ ] **Step 4: Commit (最终)**

```bash
git add -A
git commit -m "feat(cursor): 完成 PNG 光标素材替换静态 glyph 改造"
```

---

## 验收标准

1. `cursor_overlay.rs` 中不再包含任何 `GlyphPixel`、`CursorGlyph` 或静态像素数组
2. 光标 PNG 素材在运行时从 `assets/cursors/` 目录加载
3. 渲染时使用标准 RGBA alpha 混合，支持彩色光标（含 U/V 平面写入）
4. 所有现有测试通过（适配后）
5. `cargo clippy` 无新增 warning
