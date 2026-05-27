# Phase 4 / W7-W8 自测清单：光标平滑与点击放大

> 最后更新：2026-05-27 | 自动化验证全部通过，Native Safety Gate 待人工审查，FFmpeg compositor 待 Phase 6 接入

## 目标

验证光标轨迹采集、移动平均滤波、贝塞尔插值和点击放大状态机可生成稳定的 `EffectTimeline`。

## 元数据采集

- [x] 录制期能记录 `CursorSample`。（Task 8 自动测试：`recorder_keeps_samples_in_order`）
- [x] 录制期能记录 `CursorClick`。（Task 8 自动测试：`recorder_detects_left_click_down_and_up`）
- [x] 光标代码路径已使用 PTS → session origin 映射（CMSampleBuffer PTS 经 pts_origin 归一化至 SessionClock 域；extract_timestamp_nanos 返回 Option<u64>，无效 PTS 被丢弃不污染 origin）
- [ ] CMSampleBuffer PTS 与真实 macOS 录制对齐已验证（需人工 `npm run tauri dev` 检查 click timestamp 与视频动作匹配度，以及乱序/跨流 PTS 边界）
- [x] 光标元数据保存到中间录制元数据中。（Task 10 集成到 MacRecordingService.stop()）

## 算法验证

- [x] 0 帧输入返回空轨迹。（Task 3 测试：`smoothing_empty_input_returns_empty`）
- [x] 单点输入保持原位置。（Task 3 测试：`smoothing_single_point_keeps_position`）
- [x] 高频抖动输入被平滑。（Task 3 测试：`high_frequency_jitter_is_reduced`）
- [x] 快速跨区域移动不过度拉平。（Task 3 测试：`fast_cross_region_jump_is_preserved`）
- [x] 30fps 和 60fps 使用不同自适应窗口。（Task 3 测试：`adaptive_window_differs_between_30fps_and_60fps`）
- [x] 贝塞尔插值输出覆盖每个视频时间戳。（Task 4 测试：4 个插值测试）

## 点击放大验证

- [x] `Idle -> PressedExpand -> Hold -> ReleaseShrink -> Idle` 状态迁移正确。（Task 5 测试：`click_state_machine_transitions_through_expected_states`）
- [x] 点击瞬间有放大效果。（Task 5 测试：`click_effect_has_expand_and_shrink_window`）
- [x] 放大后能平滑恢复。（Task 5 测试：shrink 至 Idle）
- [x] 连续点击不会导致状态卡死。（Task 5 测试：`consecutive_clicks_do_not_leave_machine_stuck`）
- [x] 点击效果写入 `EffectTimeline`。（Task 6 测试：`engine_builds_frames_and_click_effects`）

## 导出验证

- [x] 录后处理可读取光标时间线。（Task 6 引擎构建 + Task 11 `build_cursor_effect_timeline` 命令）
- [ ] 导出视频包含光标平滑效果。（需 Phase 6 FFmpeg compositor 接入后做人工视频检查；Phase 4 自动化已验证 EffectTimeline 生成）
- [ ] 导出视频包含点击放大效果。（需 Phase 6 FFmpeg compositor 接入后做人工视频检查；Phase 4 自动化已验证 CursorClickEffect 生成）
- [x] 原始录制素材不被破坏。（原始录制使用 CountingWriter + 光标另存 sidecar）

## 性能验证

- [x] 光标算法不会阻塞捕获线程。（CursorMetadataRuntime 独立线程 + 录后 CursorEffectEngine）
- [x] 录后处理进度可上报。（`post-process-progress` 事件，stage/progress 字段）
- [x] 1080p 素材处理过程中内存无无界增长。（`CursorMetadataRecorder` 上限 120k samples）

## 测试要求

- [x] 移动平均滤波有单元测试。（5 个测试）
- [x] 贝塞尔插值有边界测试。（4 个测试，含空输入/单点/3 点/帧间隔验证）
- [x] 点击放大状态机有单元测试。（3 个测试）
- [x] `EffectTimeline` 序列化和反序列化有测试。（3 个 serde 测试 + 2 个 metadata round-trip 测试）

## 人工审查

- [x] 算法注释说明核心数学思路。（贝塞尔控制点估算、自适应窗口、跳变阈值）
- [x] 没有将光标效果实时压入捕获主链路。（录后 CursorEffectEngine，不阻塞 capture）
- [x] 没有为 V2 自动缩放提前实现复杂逻辑。

## Native Safety Gate（待人工逐行审查）

- [ ] `platform/macos/cursor_source.rs` CoreGraphics FFI：CGEventCreate 配对 CFRelease、null 检查、只读按钮状态
- [ ] `platform/macos/screen_capture_kit.rs` `showsCursor` 通过 `CaptureConfig.show_system_cursor` 受控
- [ ] 确认光标美化开启时无系统光标（双光标检查）

## Verification Summary (2026-05-27, Round 3)

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: **108 tests** PASS (+4 PTS origin tests, +4 since Round 2)
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS (21 pre-existing SCK FFI warnings)
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: **29 tests** PASS (+1 debounce strict ordering test)
