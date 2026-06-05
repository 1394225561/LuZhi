# BUG-0011 NSCursor Provider 自测清单

> 日期：2026-06-05
> 目标：让美化导出视频的光标形态跟随 macOS 当前真实显示的系统光标，修复 Mission Control 中被错误渲染为 Hand 的问题。

## Phase 1：计划与边界

- [x] 已读取 `HANDOFF.md`，确认上一轮结论为 AX 无法穿透 WebView，下一步为 NSCursor API。
- [x] 已读取 `BUG.md`，确认 BUG-0011_8 仍未通过人工验证。
- [x] 已读取 `.codex/rules/0-global.md` 到 `.codex/rules/5-docs.md`。
- [x] 已确认本轮不修改 `Cargo.toml` 核心依赖版本。
- [x] 已确认导出 glyph、timeline schema、录制主链路不做无关重构。

## Phase 2：RED 测试

- [x] 新增测试：NSCursor Arrow 映射为 `CursorKind::Arrow`。
- [x] 新增测试：NSCursor pointing hand 映射为 `CursorKind::Hand`。
- [x] 新增测试：NSCursor IBeam 映射为 `CursorKind::IBeam`。
- [x] 新增测试：未知系统光标不能误判为 Hand。
- [x] 新增测试：系统 Arrow 优先级高于 AX 推断 Hand，用于覆盖 Mission Control false positive。
- [x] 已运行定向 Rust 测试，并确认新增测试在实现前失败。

## Phase 3：实现

- [x] `MacCursorKindProvider` 优先读取 `[NSCursor currentSystemCursor]`。
- [x] 读取 NSCursor 时使用 autorelease pool，避免高频轮询造成 autorelease 对象堆积。
- [x] 只识别 Arrow / pointingHand / IBeam 三类系统光标。
- [x] 未知或读取失败时 fallback 为 Arrow。
- [x] AX role inference 不再作为生产主来源发出 Hand/IBeam。
- [x] 前端不再提示“必须辅助功能权限才能识别手形/文本光标”。

## Phase 4：验证

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind` 通过。
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过。
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过。
- [x] `npm test -- --run` 通过。

## 人工验证门禁

- [ ] 普通桌面或空白区域：导出美化光标为黑底白边 Arrow。
- [ ] 链接/按钮悬停：导出美化光标为 Hand。
- [ ] 文本输入框悬停：导出美化光标为 IBeam。
- [ ] 四指上划调出 Mission Control：macOS 显示 Arrow，导出美化视频也显示 Arrow。
- [ ] 未授予 Accessibility 权限时，仍能根据系统当前光标识别 Arrow/Hand/IBeam。
