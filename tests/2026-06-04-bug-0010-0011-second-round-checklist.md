# BUG-0010 / BUG-0011 第二轮修复自测清单

> 日期：2026-06-04
> 范围：BUG-0010 光标 Y 轴偏移、BUG-0011 CursorKind/光标 glyph 未达验收
> 目标：下一轮修复必须用本清单验证，不能只依赖中心点或“有像素变化”测试。

## Phase 1: BUG-0010 坐标归一化修正

- [ ] 新增 mapper 回归测试：top-left 原点坐标映射到 source video top-left。
- [ ] 新增 mapper 回归测试：bottom-right 坐标映射到 source video bottom-right。
- [ ] 新增 mapper 回归测试：非零 display origin 下四角映射正确。
- [ ] 新增 mapper 回归测试：负坐标外接显示器 origin 下四角映射正确。
- [ ] 新增 mapper 回归测试：Retina point 尺寸到 stream pixel 尺寸缩放正确。
- [ ] 新增 mapper 回归测试：如果启用 Y flip，top/bottom 测试必须能证明 flip 是被显式选择的。
- [ ] 修复后检查 `CursorCoordinateMapper` 不得默认无条件翻转 Y 轴。
- [ ] 修复后检查 `CaptureGeometry` 字段来源与 `CGEventGetLocation()` 坐标空间一致。
- [ ] 修复后执行：`cargo test --manifest-path src-tauri/Cargo.toml cursor_mapper`
- [ ] 人工验证：1080p 主显示器四角、中心、下半屏移动，导出 overlay 与源 cursor 对齐。
- [ ] 人工验证：Retina 显示器四角、中心、下半屏移动，导出 overlay 与源 cursor 对齐。

## Phase 2: BUG-0011 CursorKind 识别

- [ ] `CursorSnapshot` 或等效结构携带 `CursorKind`，不能只携带 x/y/buttons。
- [ ] macOS kind 识别失败时 fallback 为 `Arrow`，并记录诊断计数，不阻塞录制。
- [ ] target-aware 查询不得在 ScreenCaptureKit callback 中执行。
- [ ] target-aware 查询需要限频或移动阈值，避免高频 AX 查询拖慢录制。
- [ ] 新增 fake source 测试：录制 Arrow/Hand/IBeam 三类样本时 metadata 保存对应 kind。
- [ ] 新增 engine 测试：平滑和插值后 Arrow/Hand/IBeam kind 不丢失。
- [ ] 修复后执行：`cargo test --manifest-path src-tauri/Cargo.toml cursor_engine`
- [ ] 人工验证：普通桌面导出为 Arrow。
- [ ] 人工验证：悬停按钮/链接导出为 Hand。
- [ ] 人工验证：悬停文本框导出为 IBeam。

## Phase 3: BUG-0011 glyph 与 hotspot 修正

- [ ] Arrow glyph 必须是黑色填充、白色描边，hotspot 在箭头尖。
- [ ] Hand glyph 必须是白色填充、黑色描边，hotspot 在食指尖。
- [ ] IBeam glyph 必须是黑色主体、白色描边，hotspot 在中线中心或平台约定点。
- [ ] 新增像素级测试：Arrow 内部像素接近黑色 Y 值，描边像素接近白色 Y 值。
- [ ] 新增像素级测试：Hand 内部像素接近白色 Y 值，描边像素接近黑色 Y 值。
- [ ] 新增像素级测试：IBeam 主体像素接近黑色 Y 值，描边像素接近白色 Y 值。
- [ ] 新增 hotspot 测试：click ring 以 hotspot 为圆心，不以 glyph 中心为圆心。
- [ ] 修复后执行：`cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay`
- [ ] 人工验证：点击放大中心在目标 hotspot，不在 glyph 视觉中心。

## Phase 4: 回归和文档门禁

- [ ] BUG-0010/0011 人工验证结果写回 `BUG.md`。
- [ ] 更新 `HANDOFF.md`，记录第二轮修复范围、验证命令和人工门禁结果。
- [ ] 检查 `BUG.md` 预防规则：坐标空间一致、hotspot 对齐、target-aware fallback、glyph 像素测试均已覆盖。
- [ ] 执行完整回归：`cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] 执行完整 FFmpeg 回归：`cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`
- [ ] 执行前端回归：`npm test -- --run`
- [ ] 执行构建门禁：`npm run build`
