# BUG-0010/0011 第四轮整改自测清单

## 自动化测试

### Rust 单元测试

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 全部通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` 全部通过
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- [ ] `npm test -- --run` 全部通过
- [ ] `npm run build` 通过

### 新增测试用例

- [ ] `finish_merges_ax_failure_counts_into_metadata` — AX 全局计数合并到 metadata
- [ ] `recording_metadata_serializes_media_timeline_diagnostics` — MediaTimelineDiagnostics 序列化
- [ ] `mac_cursor_source_uses_injected_kind_provider` — provider 注入生效
- [ ] `mac_cursor_source_preserves_position_timestamp_with_slow_provider` — 慢 provider 不污染 timestamp
- [ ] `effect_timeline_raw_positioning_disables_smoothing` — raw mode 关闭 smoothing
- [ ] `overlay_uses_source_pts_origin_when_input_pts_is_non_zero` — PTS origin mapping
- [ ] `arrow_glyph_has_tail_handle_and_hotspot_still_at_tip` — Arrow 把柄形状

## 人工验证门禁

### BUG-0010 raw positioning（前置：smoothing off, magnification off）

- [ ] 静止桌面四角和中心 — overlay 与源位置重合
- [ ] 水平从左到右匀速移动 — overlay 不持续向右/向左漂
- [ ] 水平从右到左匀速移动 — overlay 不持续向右/向左漂
- [ ] 多段移动（左到右、停顿、右到左、停顿、再左到右）— 不出现累计漂移
- [ ] Retina 显示器 — overlay 不偏移
- [ ] metadata 中 video/cursor first/last timestamp 差异在预期范围内

### BUG-0011 target-aware kind（前置：Accessibility granted）

- [ ] 普通桌面 — 导出光标为黑底白边箭头（带把柄）
- [ ] Tauri 按钮 — 导出光标为白底黑边手形
- [ ] 浏览器链接 — 导出光标为白底黑边手形
- [ ] 原生文本框 — 导出光标为黑底白边 I-beam
- [ ] Tauri/Electron input — 导出光标为黑底白边 I-beam
- [ ] metadata 中 Hand/IBeam count > 0
- [ ] metadata 中 AX failure/fallback/kind distribution 可见

### BUG-0011 未授权 Accessibility

- [ ] 未授权 Accessibility 时，UI 显示明确提示
- [ ] 未授权时录制，metadata 中 `accessibility_permission_at_start=notdetermined`
- [ ] 未授权时所有光标为 Arrow，metadata 中 `ax_query_failure_count > 0`

### BUG-0011 glyph 外形

- [ ] Arrow 黑底白边，有尾部把柄，hotspot 在箭头尖端
- [ ] Hand 白底黑边，外形短而清晰
- [ ] IBeam 黑底白边，hotspot 居中

## 诊断可见性

- [ ] stop 时打印完整结构化摘要：video first/last、cursor first/last、AX permission、AX failure/fallback、kind distribution
- [ ] metadata sidecar JSON 包含 `media_timeline_diagnostics` 字段
- [ ] metadata sidecar JSON 包含 `cursor_timing_diagnostics` 字段
- [ ] metadata sidecar JSON 的 `cursor_kind_diagnostics.ax_query_failure_count` 不再永远为 0
