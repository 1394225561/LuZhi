# Phase 5 Code Review: Silence Trimming

> 日期：2026-05-29
> 评审范围：Phase 5 / W9-W10 空白段检测与自动裁剪
> Git 范围：`179eaa76bc070e31169b8f973b9c3a97011352b7..a9bab95a958d957124e48b7de82aea1eb8e788e1`
> 结论：With fixes，不建议直接进入下一阶段整改前视为完成
> 最新复审：见 `## 16. 三轮整改补充复审（2026-05-29）：排除 Phase 6 合并项后的 Phase 5 代码复核`

## 1. 评审目标

本次评审重点回答两个问题：

1. Phase 5 是否完整完成了开发任务。
2. Phase 5 相关代码是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常等问题。

额外检查：

- 是否符合 `docs/architecture/project-architecture-and-overall-planning.md` 中 Phase 5 的目标：音频 RMS、低分辨率帧差分、候选空白段合并、输出 `CutTimeline`、通过 FFmpeg 封装执行裁剪导出。
- 是否符合 `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md` 中的保守实现边界：Rust 侧采集 activity metadata，React 不接触音视频帧流，真实可播放 FFmpeg 裁剪导出保留为 Gate。
- 是否符合 `tests/phase-5-w9-w10-checklist.md` 中的自测项和剩余人工 Gate。
- 是否遵守 `BUG.md` 预防规则。
- 是否遵守数据流红线：音视频帧流、frame-diff stream、audio activity stream 不进入前端 JS 层。

## 2. 总体结论

Phase 5 的主体骨架已经建立：

- Rust 侧新增 `CutTimeline`、`CutSegment`、`KeepSegment`、audio/visual activity sample serde 模型。
- 新增 `SilenceDetector` trait、音频 RMS 分析、低分辨率帧差分、候选段合并和裁剪缓冲逻辑。
- 新增 trim metadata 和 cut timeline JSON sidecar 读写。
- `MacRecordingService` 在消费线程采集 trim metadata，并在停止录制后写入 sidecar。
- 新增 `build_cut_timeline` Tauri command。
- `export_video` 返回 Phase 4 cursor summary 与 Phase 5 cut summary 的组合结果。
- Preview UI 只调用 command 和接收轻量 summary/path，没有接收音视频帧或 activity stream。

但是当前不能认定 Phase 5 已完整完成。最主要阻塞点是：默认录制 writer 仍然返回 `duration_secs = 0`，trim metadata 的 `duration_nanos` 因此为 0，`SilenceDetectorEngine` 会直接返回 no-op timeline。也就是说，真实默认录制路径虽然会采集 activity samples，但无法实际生成裁剪段。

此外，当前实现还有几个和计划/checklist 不一致的点：

- 帧差分对每个视频帧都执行，没有低频抽样。
- trim metadata 的 Vec 没有 bounded 策略。
- RMS window 配置存在，但没有真正用于 500ms 到 1000ms 窗口聚合。
- 帧差分没有处理 ScreenCaptureKit 的 `bytes_per_row` stride。
- Preview 调用了 trim/export command，但没有展示返回的轻量 summary。

FFmpeg 真实可播放裁剪导出仍属于当前 plan/checklist 明确记录的 FFmpeg Gate，本次不单独作为阻塞问题；但从总架构文档的 W9-W10 目标看，它仍是 MVP 后续必须补齐的导出能力。

## 3. 自动化验证结果

本次评审期间实际执行：

```bash
git diff --check 179eaa7..HEAD
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

结果：

- `git diff --check 179eaa7..HEAD`: PASS，无输出。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，149 tests。
- `npm test -- --run`: PASS，44 tests。

注意：

- Rust 测试仍有 21 个既有 SCK/FFI warning，以及 1 个 `silence_detector.rs` 测试里的 irrefutable `if let` warning。
- `npm test` 输出多条 `Window.scrollTo()` 未实现提示，但测试通过。
- 当前测试没有覆盖默认录制路径中 `duration_nanos == 0` 导致 cut timeline no-op 的问题。

## 4. Strengths

1. 架构边界总体正确
   - 音频 RMS、帧差分、cut timeline 构建均在 Rust 侧。
   - React 只调用 `build_cut_timeline` / `export_video`，没有接触媒体帧或 activity stream。
   - `export_video` 没有伪造 playable output path，符合 FFmpeg Gate 的谨慎边界。

2. 捕获 callback 没有被直接加入裁剪逻辑
   - ScreenCaptureKit callback 仍通过 bounded channel 和 `try_send_drop_newest` 投递帧/音频。
   - Phase 5 的 activity metadata 采集发生在 consumer thread，而不是 SCK callback。
   - `build_cut_timeline` 使用 `spawn_blocking`，没有在 Tauri 主事件循环同步跑 JSON 读写和检测。

3. 资源释放顺序比早期实现更稳
   - `stop()` 会先停 cursor runtime，再停 native capture/mic，再 signal consumer，再 join。
   - sidecar 写入失败会进入 errors 收集，不会跳过 mic level reset。
   - stop 中会聚合 capture stop、mic stop、sidecar 写入错误。

4. 测试覆盖了不少纯逻辑
   - cut timeline serde、sensitivity defaults、empty timeline。
   - 静音/噪声/loud chunk 的 RMS 基础行为。
   - 静态帧、全量变化、小光标移动的 frame diff。
   - 双信号候选、短暂停顿不裁剪、长静音静止裁剪、相邻候选合并。
   - 前端 auto-trim command path 和 export flush 顺序。

## 5. Issues

### Critical 1: 默认录制路径生成的 `duration_nanos` 为 0，导致裁剪时间线永远 no-op

位置：

- `src-tauri/src/platform/macos_service.rs:475`
- `src-tauri/src/media/recording_writer.rs:58`
- `src-tauri/src/media/silence_detector.rs:178`

现象：

- `MacRecordingService::consume_frames()` 在 writer `finish()` 后用 `result.duration_secs.saturating_mul(1_000_000_000)` 填充 `TrimMetadata.duration_nanos`。
- 当前实际使用的 writer 是 `CountingRecordingWriter::new(None)`。
- `CountingRecordingWriter::finish()` 固定返回 `duration_secs: 0`。
- `SilenceDetectorEngine::analyze()` 遇到 `duration_nanos == 0` 直接返回 `CutTimeline::empty(0)`。

为什么重要：

这会让 Phase 5 的真实默认路径无法完成核心目标。即使录制期间已经收集到 `audio_activity` 和 `visual_activity`，`build_cut_timeline` 仍会得到空的 no-op timeline，无法识别长静音且低变化片段。

风险类型：

- 功能完整性风险：Phase 5 交付物“长静音且画面低变化片段可识别”在默认路径中不成立。
- 测试缺口：当前单元测试只用手工传入的非 0 duration 验证 detector，没有覆盖 recording service 产出的 trim metadata。
- 用户可见风险：用户开启自动裁剪后导出 summary 可能显示 `cutCount = 0`，误以为没有空白段。

建议修复：

1. 在 `consume_frames()` 中不要只依赖 writer duration。
2. 在当前 `CountingRecordingWriter` 仍是默认 writer 的阶段，可用已采集 activity 的最大 `end.nanos` 派生 duration，例如：
   - `visual_activity.last().map(|s| s.end.nanos)`
   - `audio_activity.last().map(|s| s.end.nanos)`
   - 两者取 max。
3. 若未来 production writer 可返回真实 duration，则可优先使用 writer duration，writer duration 为 0 时 fallback 到 observed duration。
4. 给 `MacRecordingService` 或一个可测试 helper 增加覆盖：
   - activity 样本存在、writer duration 为 0 时，trim metadata duration 应大于 0。
   - 基于该 metadata 构建 timeline 能产生 expected cut。

建议验收：

- 新增测试：`trim_metadata_duration_falls_back_to_observed_activity_when_writer_duration_zero`。
- 新增测试：默认 writer 录制路径产出的 `TrimMetadata.duration_nanos > 0`。
- 手动录制 8 秒静音静止片段，`build_cut_timeline` 返回 `cutCount >= 1`。

### Important 1: 帧差分对每个视频帧执行，不符合低频抽样要求，可能拖慢 consumer/write path

位置：

- `src-tauri/src/platform/macos_service.rs:377`
- `src-tauri/src/platform/macos_service.rs:381`
- `src-tauri/src/platform/macos_service.rs:387`
- `src-tauri/src/platform/macos_service.rs:429`

现象：

`consume_frames()` drain 每个 video frame 时都会：

1. 对 `previous_frame` 和当前 frame 执行 `frame_diff_analyzer.diff_pair(...)`。
2. push `visual_activity`。
3. clone 当前 `VideoFrame` 作为下一次 previous。
4. 最后才调用 `writer.push_video(frame)`。

为什么重要：

架构文档要求“空白检测使用低频抽样，不占用主帧流”。当前实现虽然没有阻塞 SCK callback，但它位于 consumer/write path。consumer 如果被每帧 diff 拖慢，bounded video channel 会满，SCK callback 的 `try_send_drop_newest` 会开始丢弃新帧，从而影响录制完整性。

风险类型：

- 性能风险：1080p/30fps 下每秒执行 30 次两帧缩略图构建；4K 或 60fps 时更明显。
- 捕获链路间接风险：consumer 追不上会导致 channel drop，录制帧缺失。
- 计划偏差：Phase 5 plan 明确要求 low-cost、low-frequency、bounded metadata。

建议修复：

1. 增加时间节流：例如每 250ms 或 500ms 才采一组 visual diff。
2. 对 previous sample frame 保存已 downsample 的 grayscale thumb，而不是保存完整 `VideoFrame` 并每次重算 previous thumb。
3. 将 diff 逻辑放在 writer push 之后或确保 writer push 不被 metadata 分析阻塞。
4. 记录采样率常量，例如 `VISUAL_SAMPLE_INTERVAL_NANOS`，并配套测试。

建议验收：

- 新增测试：输入 30fps / 10s frame timestamp，visual samples 数量应约等于目标低频采样率，而不是 299 个。
- 手动 1080p 录制时观察 dropped frame count，不应因 auto-trim metadata 采集明显上升。

### Important 2: trim metadata 未 bounded，长录制内存和 JSON sidecar 大小无上限

位置：

- `src-tauri/src/platform/macos_service.rs:369`
- `src-tauri/src/platform/macos_service.rs:370`
- `src-tauri/src/platform/macos_service.rs:383`
- `src-tauri/src/platform/macos_service.rs:415`
- `src-tauri/src/platform/macos_service.rs:480`
- `src-tauri/src/platform/macos_service.rs:481`

现象：

`visual_activity` 和 `audio_activity` 都是普通 `Vec`，录制期间持续增长，停止录制后整体写入 JSON。

为什么重要：

Phase 5 plan 的成功标准写明“Recording stop writes a trim metadata sidecar containing bounded audio/visual activity samples”。当前实现没有上限，也没有固定时间桶聚合策略。长录制、长会议、4K/60fps 场景下 sidecar 和内存峰值可能持续增长。

风险类型：

- 内存压力风险：不是 Rust 内存安全问题，但会造成 unbounded allocation。
- 停止录制延迟风险：stop 后 JSON pretty serialization 需要一次性处理完整 Vec。
- 磁盘/临时目录风险：sidecar 文件可随录制时长线性膨胀。

建议修复：

1. 将 activity metadata 设计为固定采样率，例如：
   - visual: 2-5 samples/sec。
   - audio: 2 samples/sec 或按 `rms_window_nanos` 输出聚合窗口。
2. 引入最大样本数或最大录制时长估算，超过后按时间桶压缩。
3. 对 sidecar 写入前的样本数量做 sanity limit，并在超限时返回结构化错误或降采样。
4. 避免 pretty JSON 用于大型 sidecar，必要时改为 compact JSON。

建议验收：

- 新增测试：10 分钟模拟输入的 metadata samples 数量低于明确上限。
- `tests/phase-5-w9-w10-checklist.md` 中的 “10 minute 1080p trim metadata and cut timeline memory-pressure check” 完成人工验证。

### Important 3: `rms_window_nanos` 只暴露配置，没有真正用于 500ms-1000ms 窗口 RMS

位置：

- `src-tauri/src/media/silence_detector.rs:18`
- `src-tauri/src/media/silence_detector.rs:22`
- `src-tauri/src/media/silence_detector.rs:37`
- `src-tauri/src/media/silence_detector.rs:276`

现象：

`AudioRmsAnalyzer::window_nanos()` 会返回 sensitivity 对应的窗口大小，但 `analyze_chunks()` 实际是对每个 `MixedAudioChunk` 独立计算 RMS。典型音频 callback 可能是 10ms 到几十 ms，而不是 500ms 到 1000ms。

为什么重要：

架构文档和 checklist 均要求使用 500ms 到 1000ms RMS 窗口。单 chunk RMS 会让短暂噪声、音频 callback 粒度、系统音频与麦克风 pairing 行为直接影响静音判断，误剪保护会变弱。

风险类型：

- 算法正确性风险：短暂低 RMS chunk 可能被当作静音候选的一部分。
- 测试误导风险：`rms_window_uses_configured_500ms_to_1000ms_window` 只验证 getter，不验证窗口聚合。
- checklist 偏差：`500ms 到 1000ms 滑动窗口可配置` 当前未真正实现。

建议修复：

1. 将 `AudioRmsAnalyzer` 改为 stateful window aggregator，按 `rms_window_nanos` 聚合后输出 `AudioActivitySample`。
2. 或提供 pure helper：输入多个 `MixedAudioChunk`，按时间窗口合并 RMS。
3. 对窗口边界、背景噪声、短暂停顿增加测试。

建议验收：

- 新增测试：多个 10ms chunk 在 500ms/750ms/1000ms 窗口内聚合成较少的 activity sample。
- 新增测试：短于窗口的瞬时静音不会直接输出长静音候选。

### Important 4: 帧差分忽略 ScreenCaptureKit stride，可能错误采样 padded BGRA frame

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:203`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:207`
- `src-tauri/src/media/silence_detector.rs:100`
- `src-tauri/src/media/silence_detector.rs:110`

现象：

SCK 捕获路径拷贝的是 `bytes_per_row * height`：

```rust
let total_bytes = bytes_per_row * height;
```

但 `VideoFrame` 只保存 `width`、`height`、`pixel_format` 和 buffer，没有保存 `bytes_per_row`。`FrameDiffAnalyzer::downsample_grayscale()` 按 `(src_y * frame.width + src_x) * 4` 推算 offset，等价于假设 frame 是 tightly packed BGRA。

为什么重要：

真实 CVPixelBuffer 常见 `bytes_per_row` 大于 `width * 4`。当存在 row padding 时，当前 offset 会从第二行开始读错位置，导致 frame diff 计算不可信。空白段检测可能误判静止或误判变化。

风险类型：

- 功能正确性风险：低变化判断可能失真。
- 平台兼容风险：不同显示器宽度、scale factor、SCK 输出格式下 padding 行为不同。
- 测试缺口：当前 frame diff 测试只用 tightly packed 小图。

建议修复：

1. 在 `VideoFrame` 增加 `bytes_per_row: usize` 或 `stride_bytes: usize`。
2. SCK 捕获时填入真实 `bytes_per_row`。
3. 测试构造 padded frame，验证采样不会跨行读错。
4. 如果不想扩展 `VideoFrame`，则捕获时重排成紧凑 BGRA buffer，但这会增加 callback 拷贝成本，需要谨慎评估。

建议验收：

- 新增测试：`frame_diff_handles_padded_rows`。
- 新增测试：不同 stride 下相同图像 diff 仍接近 0。

### Minor 1: Preview 没有展示 trim/export summary

位置：

- `src/components/preview-view.tsx:198`
- `src/components/preview-view.tsx:200`
- `src/components/preview-view.tsx:466`

现象：

Preview 调用 `exportVideo(preset)` 后只清理 `beautifyError`，没有保存或展示返回的：

- `cutCount`
- `totalCutNanos`
- `cutTimelinePath`
- `outputPath`

为什么重要：

Phase 5 plan 要求 Preview wiring 展示 lightweight trim/export summaries。当前用户无法从 UI 知道自动裁剪识别了多少段、预计裁掉多少时长，也无法区分“没有检测到空白段”和“裁剪没有实际工作”。

建议修复：

1. 增加 `exportSummary` state。
2. 成功 export 后展示轻量结果，例如“已生成裁剪时间线：N 段，预计剪除 X 秒”。
3. 如果 `outputPath` 为 null，应明确这是 FFmpeg Gate，不要让用户误以为已导出 playable file。

## 6. 捕获主链路、内存、线程与资源释放专项结论

### 6.1 捕获主链路

没有发现 Phase 5 代码直接在 ScreenCaptureKit callback 中执行 RMS 或帧差分。callback 仍通过 bounded media channel 和 `try_send_drop_newest` 投递。

但存在间接风险：帧差分在 consumer/write path 中逐帧执行，并发生在 `writer.push_video` 之前。如果该路径追不上，bounded channel 会丢弃新帧。因此当前实现不满足“低频抽样，不挤占主帧队列”的严格要求。

### 6.2 内存安全

没有发现 Rust 层面的 use-after-free、裸指针新增风险或 unsafe 新增风险。Phase 5 主要新增 safe Rust 逻辑。

需要注意的是：`FrameDiffAnalyzer` 忽略 stride 会导致错误读取同一 buffer 中的 padding/错行像素，但当前有 `bytes.len() < width * height * 4` 检查，通常不会越界；问题主要是算法正确性，不是内存安全。

### 6.3 线程安全

未发现新增明显 data race：

- `consumer_handle` 只在 service 内持有并 join。
- `mic_level` 使用 `Arc<Mutex<f64>>`。
- `build_cut_timeline` 使用 session id、metadata path 和 beautify revision 做 stale result guard。

剩余风险：

- `build_cursor_effect_timeline` 与 `build_cut_timeline` 共享 `beautify_revision`。这能防止 stale 写回，但如果用户快速切换配置，前端会收到取消错误，需要继续保持 UI 侧忽略 stale error 的逻辑。

### 6.4 资源释放路径

本轮没有发现 stop path 因 trim sidecar 写入失败而跳过 capture/mic 清理的问题。`stop()` 的资源释放顺序总体可接受。

剩余风险：

- stop 后一次性 JSON serialization 大 sidecar 可能导致停止录制耗时变长。
- 如果 consumer thread panic，`handle.join().unwrap_or(empty_output)` 会吞掉 panic 并写空 trim metadata，建议至少记录错误或进入 `RecordingFinalizeFailed`。

## 7. BUG.md 预防规则扫描

本次扫描：

```bash
rg -n "data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|whileTap|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结果：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 Phase 5 diff 新增 `motion.div whileTap` 直接包交互按钮。
- 未发现 `setIgnoreCursorEvents(true)` 回归。

说明：

- `src/components/recording-panel.tsx` 中存在 `motion.button whileTap`，这是 `whileTap` 直接放在按钮元素本身，不属于 BUG-003 预防规则中的“`motion.div whileTap` 作为交互按钮直接父容器”问题。

## 8. Phase 5 完成度对照

| 项目 | 当前状态 | 结论 |
| --- | --- | --- |
| `CutTimeline` / segment / activity sample 模型 | 已实现 | 通过 |
| `SilenceDetector` trait | 已实现 | 通过 |
| 音频 RMS | 已实现基础 RMS，但没有 500ms-1000ms window | 需整改 |
| 低分辨率帧差分 | 已实现，但忽略 stride，且不是低频采样 | 需整改 |
| 候选空白段合并 | 已实现并有测试 | 基本通过 |
| trim metadata sidecar | 已实现，但 metadata 不 bounded，duration 为 0 | 需整改 |
| 录制消费线程采集 metadata | 已接入 consumer thread | 有性能风险 |
| `build_cut_timeline` command | 已实现 | 依赖 duration 修复 |
| `export_video` 返回 combined summary | 已实现 | 通过 |
| Preview command wiring | 已实现调用和错误展示 | summary 展示缺失 |
| React 不接触媒体流 | 未发现违规 | 通过 |
| 原始素材保留 | 当前未删除原始素材 | 通过 |
| FFmpeg playable trimmed output | Gate，未实现 | 不计入本轮 blocker，但仍需后续完成 |

## 9. 建议整改顺序

1. 先修 Critical：让 trim metadata duration 在默认 writer 路径中变为真实非 0，并补 recording-path 测试。
2. 修 RMS window：把 `rms_window_nanos` 从配置展示变成实际窗口聚合逻辑。
3. 修 visual sampling：实现低频采样和 bounded visual metadata。
4. 修 stride：扩展 `VideoFrame` 或将 SCK buffer 重排为紧凑 BGRA，并补 padded row 测试。
5. 修 metadata bounded：给 audio/visual activity 设置明确采样率和上限，并补长录制模拟测试。
6. 补 Preview summary：展示裁剪段数、预计剪除时长和 FFmpeg Gate 状态。
7. 最后跑全量自动化验证，并做 10 分钟 1080p 手动内存压力检查。

## 10. 整改后复验清单

建议整改完成后至少执行：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
git diff --check 179eaa7..HEAD
```

建议新增或更新的测试：

- `trim_metadata_duration_falls_back_to_observed_activity_when_writer_duration_zero`
- `build_cut_timeline_generates_cuts_from_recording_service_metadata`
- `audio_rms_aggregates_chunks_by_configured_window`
- `audio_rms_short_pause_does_not_form_long_candidate`
- `frame_diff_handles_padded_rows`
- `visual_activity_sampling_is_low_frequency`
- `trim_metadata_samples_are_bounded_for_long_recording`
- `preview_displays_export_trim_summary`

建议人工验证：

- 录制 8-12 秒静音且画面静止素材，`build_cut_timeline` 产生至少 1 个 cut。
- 录制含加载动画/终端输出/鼠标移动素材，低变化判断不会误剪。
- 录制 10 分钟 1080p，观察 trim metadata 样本数、sidecar 文件大小、停止录制耗时和内存峰值。
- 确认原始录制素材/sidecar 没有被裁剪流程删除。
- FFmpeg Gate 仍明确展示，不伪造 playable output。

## 11. Open Questions

1. 在 production FFmpeg writer 未接入前，Phase 5 是否接受用 observed activity timestamp 作为 recording duration 的权威来源？
2. visual activity 的目标采样率应定为 2 fps、4 fps 还是与录制 fps 比例相关？
3. trim metadata 的 bounded 策略更偏向“固定采样率全量保留”还是“超过上限后按时间桶压缩”？
4. `VideoFrame` 是否应该现在扩展 stride 字段，还是等 production encoder 接入时一起处理 frame layout？

## 12. Ready To Merge?

**结论：No / With fixes.**

Phase 5 的结构方向正确，但默认录制路径当前无法生成真实 cut timeline，且低频/窗口/bounded/stride 几个关键实现细节未达到计划与 checklist 要求。建议完成本 review 中 Critical 和 Important 整改后，再进行复审。

## 13. 首轮整改复审（2026-05-29）

> 复审范围：首轮 code review 整改后的 Phase 5 相关代码改动
> 复审基线：`HEAD` = `a9bab95 fix: Phase 5 code review 整改`
> 复审对象：当前 working tree diff
> 结论：No Critical，仍建议修复 3 个 Important 后再视为 Phase 5 整改完成

### 13.1 复审输入

本轮复审对照以下文件：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-5-w9-w10-checklist.md`
- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md`
- `BUG.md`

本轮 working tree diff 涉及：

- `src-tauri/src/core/frame.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/silence_detector.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src/components/preview-view.tsx`

复审重点：

1. 首轮 6 个问题是否真正修复。
2. Phase 5 是否仍满足计划和 checklist。
3. 是否引入阻塞捕获主链路、内存安全、线程安全、资源释放异常。
4. 是否违反 `BUG.md` 预防规则。

说明：按 `superpowers:requesting-code-review` 流程尝试启动独立 subagent reviewer 两次，但均因服务端 `502 Bad Gateway` 失败。本节结论来自本地代码审查与自动化验证。

### 13.2 自动化验证结果

本轮复审实际执行：

```bash
git diff --check HEAD
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

结果：

- `git diff --check HEAD`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，153 tests。
- `npm test -- --run`: PASS，44 tests。

测试输出备注：

- Rust 仍有既有 SCK/FFI warning。
- 新增 1 条 `silence_detector.rs` 测试中的 irrefutable `if let` warning，属于测试代码风格问题，不影响功能。
- Vitest 仍输出 `Window.scrollTo()` 未实现提示，测试通过。

### 13.3 首轮问题整改确认

| 首轮问题 | 当前整改状态 | 复审结论 |
| --- | --- | --- |
| 默认 writer duration 为 0 导致 cut timeline no-op | `macos_service.rs` 增加 writer duration 为 0 时用 observed activity end timestamp fallback | 基本修复，但超长录制触发 sample cap 后仍有 duration 截断风险，见 Important 2 |
| frame diff 每帧执行 | `VISUAL_SAMPLE_INTERVAL_NANOS = 250ms`，约 4fps 低频采样 | 基本修复 |
| activity Vec 无界增长 | 增加 `MAX_VISUAL_SAMPLES` / `MAX_AUDIO_SAMPLES` | 基本修复，但 final drain 有一处 cap 不一致，见 Minor 1 |
| `rms_window_nanos` 未实际聚合 | `AudioRmsAnalyzer` 改为 stateful window aggregator | 方向正确，但超过窗口时未保留 remainder，见 Important 1 |
| frame diff 忽略 SCK stride | `VideoFrame` 增加 `stride_bytes`，SCK 传入 `bytes_per_row`，diff 使用 stride | 基本修复，但缺少 stride 下界校验，见 Important 3 |
| Preview 不展示 trim/export summary | `PreviewView` 保存并展示 `ExportSummary` | 基本修复，但配置变化后旧 summary 未清空，见 Minor 2 |

### 13.4 Strengths

1. 首轮 Critical 已被正面处理
   - `CountingRecordingWriter` 仍返回 `duration_secs = 0` 的阶段，`MacRecordingService` 已不再直接把 trim metadata duration 写成 0。
   - `duration_nanos` fallback 到 observed activity end timestamp 后，默认路径可以生成非 0 cut timeline。

2. 捕获主链路风险明显下降
   - 帧差分从每帧执行改为约 4fps 低频采样。
   - activity 采集仍在 consumer thread，不在 ScreenCaptureKit callback。
   - React 仍只接收 command summary/path，没有接收音视频帧、frame-diff stream 或 audio activity stream。

3. RMS 逻辑从单 chunk RMS 变成 stateful aggregation
   - 新增 `push_chunk()` / `flush()`，能跨多个小音频 chunk 聚合。
   - 新增测试覆盖多个 10ms chunk 聚合到一个 window 的行为。

4. stride 修复方向正确
   - `VideoFrame` 增加 `stride_bytes`。
   - `screen_capture_kit.rs` 将 `CVPixelBufferGetBytesPerRow` 写入 `VideoFrame`。
   - `FrameDiffAnalyzer` 使用 `src_y * stride + src_x * 4` 读取 padded row。

5. Preview summary 已补齐用户可见反馈
   - 导出成功后显示裁剪段数、预计剪除秒数。
   - 当 `outputPath` 为空时明确展示 FFmpeg 编码器 Gate，不伪造可播放文件。

### 13.5 Issues

#### Critical

未发现新的 Critical 阻塞项。

本轮没有发现会立即导致数据丢失、主链路必现阻塞、默认裁剪完全不可用、React 接收媒体帧流、或明显资源泄漏的改动。

#### Important 1: RMS window 聚合会吞并超过窗口的余量，窗口语义仍不精确

位置：

- `src-tauri/src/media/silence_detector.rs:50`
- `src-tauri/src/media/silence_detector.rs:52`
- `src-tauri/src/media/silence_detector.rs:67`
- `src-tauri/src/media/silence_detector.rs:74`

现象：

`AudioRmsAnalyzer::push_chunk()` 在累计样本达到 `rms_window_nanos` 后，会把当前 `sample_buffer` 的全部内容作为一个 activity sample 输出，然后直接 `clear()`。

这意味着：

- 如果单个 chunk 本身大于窗口，比如 1s chunk 配 750ms window，会输出 1s sample，而不是 750ms sample + 250ms remainder。
- 如果多个小 chunk 累计超过窗口，比如 760ms，会输出 760ms sample，10ms remainder 被吞进上一窗口。
- 输出不是严格的 500ms/750ms/1000ms fixed window，也不是真正 sliding window。

为什么重要：

Phase 5 checklist 写的是“500ms 到 1000ms 滑动窗口可配置”。当前实现已经比首轮好很多，但窗口边界仍不精确。对 silence trimming 来说，窗口边界会影响候选段长度、merge gap、buffer 后的最终 cut start/end，尤其在音频 callback 粒度较大或系统/mic 混音输出 chunk 较大时，可能造成裁剪时间线偏移。

风险类型：

- 算法正确性风险。
- 测试覆盖不足：现有 `audio_rms_aggregates_small_chunks_by_window` 只断言 80 个 10ms chunk 输出 1 个 sample，没有断言 sample 时长必须等于 configured window，也没有断言 remainder 被保留。

建议修复：

1. 按 `rms_window_nanos` 精确切窗：
   - 计算每个窗口需要的 samples 数。
   - buffer 超过窗口时只消费窗口长度。
   - 剩余 samples 留在 `sample_buffer` 中进入下一窗口。
2. `push_chunk()` 可以返回 `Vec<AudioActivitySample>`，因为一个大 chunk 可能产生多个 window。
3. 如果继续只返回 `Option<AudioActivitySample>`，需要循环调用内部 drain，并在 consumer thread 取出所有 ready samples。
4. 明确命名为 fixed window 或 sliding window；如果不做重叠窗口，不要在 checklist/注释中称为 sliding window。

建议补充测试：

- 1s chunk + 750ms window，应输出 750ms sample，flush 后输出 250ms remainder。
- 1.6s chunk + 750ms window，应输出 2 个完整 window，flush 后输出 100ms remainder。
- 80 个 10ms chunk + 750ms window，首个 sample 的 `end - start` 应等于 750ms。

#### Important 2: duration fallback 受 sample cap 影响，超长录制可能截断 `duration_nanos`

位置：

- `src-tauri/src/platform/macos_service.rs:371`
- `src-tauri/src/platform/macos_service.rs:372`
- `src-tauri/src/platform/macos_service.rs:396`
- `src-tauri/src/platform/macos_service.rs:433`
- `src-tauri/src/platform/macos_service.rs:517`
- `src-tauri/src/platform/macos_service.rs:521`

现象：

本轮为防止 unbounded metadata 增加了 `MAX_VISUAL_SAMPLES` 和 `MAX_AUDIO_SAMPLES`。但 `duration_nanos` fallback 仍从 `audio_activity.last().end` 和 `visual_activity.last().end` 推导。

当长录制超过样本上限后：

- 新 activity sample 不再 push 进 Vec。
- `audio_activity.last()` / `visual_activity.last()` 停在 cap 达到时的时间点。
- 如果 writer duration 仍为 0，则 metadata `duration_nanos` 会被截断到 cap 达到时，而不是录制实际结束时。

为什么重要：

首轮 Critical 的核心修复是“默认 writer duration 为 0 时仍能生成有效 duration”。当前修复对正常长度录制有效，但和本轮新增 bounded 策略组合后，在超长录制中会重新出现 duration 不准确。虽然 cap 注释按约 10 小时估算，短期 MVP 不一定常遇到，但这是同一条关键路径上的设计漏洞。

风险类型：

- 长录制 cut timeline duration 不正确。
- keeps/cuts 只覆盖录制前半段，后半段消失在 timeline duration 之外。
- 未来如果 cap 调小做压力控制，该问题会更容易触发。

建议修复：

1. 在 consumer thread 单独维护 `latest_observed_media_nanos`：
   - 每个 video frame 到达时更新为 `max(latest, frame.timestamp.nanos)`。
   - 每个 mixed audio chunk 到达时用 chunk duration 更新为 `max(latest, chunk_end_nanos)`。
   - 该变量不受 `MAX_*_SAMPLES` 限制。
2. `writer_duration_nanos > 0` 时仍优先用 writer duration。
3. writer duration 为 0 时 fallback 到 `latest_observed_media_nanos`，而不是 `activity.last()`。
4. 如果完全没有媒体输入，再退回 0。

建议补充测试：

- 模拟 sample cap 达到后继续输入更晚 timestamp，metadata duration 应等于最新媒体 timestamp。
- activity Vec 保持 cap，上述 duration 仍覆盖全录制时长。

#### Important 3: stride 修复缺少下界校验，异常 `VideoFrame` 可导致 diff panic

位置：

- `src-tauri/src/media/silence_detector.rs:151`
- `src-tauri/src/media/silence_detector.rs:157`
- `src-tauri/src/media/silence_detector.rs:167`
- `src-tauri/src/platform/macos_service.rs:395`
- `src-tauri/src/platform/macos_service.rs:457`

现象：

`FrameDiffAnalyzer::downsample_grayscale()` 当前检查：

- `bytes.len() >= stride * height`

但没有检查：

- `stride >= width * 4`

如果某个 `VideoFrame` 带入异常 stride，例如 `width = 100`、`stride = 16`、`bytes.len() = stride * height`，长度检查能通过。但读取最后一列附近像素时，`offset = src_y * stride + src_x * 4` 会越过该行 stride 边界，甚至越过整个 buffer，从而 panic。

真实 ScreenCaptureKit 的 `bytes_per_row` 正常情况下应大于等于 `width * 4`，所以这不是当前 macOS happy path 的高概率问题。但 `VideoFrame` 已是 core 公共结构，未来 mock、Windows、encoder 或测试输入都可能构造异常 frame。

为什么重要：

该 panic 发生在 consumer thread 的 frame diff 路径中。当前 `stop()` 使用 `handle.join().unwrap_or(empty_output)`，consumer thread panic 会被吞掉并写空 trim metadata。用户看到的是自动裁剪失效，而不是明确错误。

风险类型：

- 线程 panic 风险。
- 资源释放路径可继续执行，但错误被吞掉，定位困难。
- 算法输入契约不完整。

建议修复：

1. 在 `downsample_grayscale()` 中增加：
   - `let min_stride = frame.width as usize * 4;`
   - `if stride < min_stride { return Err("帧 stride 小于行像素字节数".to_string()); }`
2. 对 `stride * height` 使用 `checked_mul`，避免极端输入整数溢出。
3. 在 `macos_service.rs` 中遇到 `diff_pair` Err 时可计数或 debug log，避免静默吞掉大量格式错误。
4. 更进一步：consumer thread panic 时不要 `unwrap_or(empty_output)` 静默降级，至少记录错误并进入 `RecordingFinalizeFailed`。

建议补充测试：

- `frame_diff_rejects_stride_smaller_than_width_bytes`
- `frame_diff_rejects_stride_height_overflow`
- consumer thread panic/error 时 stop 返回可解释错误或至少记录错误。

#### Minor 1: final drain 中 audio sample push 没有复用 `MAX_AUDIO_SAMPLES`

位置：

- `src-tauri/src/platform/macos_service.rs:481`
- `src-tauri/src/platform/macos_service.rs:484`
- `src-tauri/src/platform/macos_service.rs:485`

现象：

主循环中 `rms_analyzer.push_chunk()` 输出 sample 后会检查：

```rust
if audio_activity.len() < MAX_AUDIO_SAMPLES {
    audio_activity.push(sample);
}
```

但 final drain 中直接：

```rust
if let Some(sample) = rms_analyzer.push_chunk(&mixed) {
    audio_activity.push(sample);
}
```

为什么重要：

实际 final drain 受 channel/synchronizer 队列规模限制，一般不会造成无界增长。但这破坏了“所有 metadata push 都受 cap 约束”的一致性，也会让后续维护者误以为 final drain 不需要 cap。

建议修复：

- 抽出小 helper，例如 `push_bounded_audio_sample(&mut audio_activity, sample)`。
- 或直接在 final drain 补同样的 `len() < MAX_AUDIO_SAMPLES` 检查。

#### Minor 2: Preview summary 在配置变化后不会清空，可能显示过期结果

位置：

- `src/components/preview-view.tsx:73`
- `src/components/preview-view.tsx:200`
- `src/components/preview-view.tsx:205`
- `src/components/preview-view.tsx:476`

现象：

导出成功后 `exportSummary` 会保留展示。用户随后修改 `autoTrimSilences` 或 `trimSensitivity` 时，旧 summary 仍会显示，直到下一次导出成功或页面重建。

为什么重要：

这不是底层正确性问题，但对用户会造成误导：界面显示的“已生成裁剪时间线 X 段”可能对应上一组配置，而不是当前开关/灵敏度。

建议修复：

- 在 `handleBeautifyChange()` 或相关配置变更路径中 `setExportSummary(null)`。
- 或在 summary 旁展示其对应的 preset/config revision，但当前阶段清空更简单。

建议补充测试：

- 导出成功显示 summary 后，切换 auto trim 或 sensitivity，summary 应消失。

### 13.6 捕获主链路专项结论

本轮整改后，首轮最主要的主链路风险已经下降：

- frame diff 不再每帧执行，采样间隔约 250ms。
- SCK callback 仍只做 buffer copy 和 bounded channel `try_send_drop_newest`，没有新增裁剪计算。
- `build_cut_timeline` 仍使用 `spawn_blocking`，不会阻塞 Tauri 主事件循环。

剩余关注点：

- `FrameDiffAnalyzer` 仍在 consumer/write path 中运行，虽然频率降低，但如果后续 thumb size 或算法复杂度增加，仍可能影响 consumer drain 速度。
- 当前 diff 发生在 `writer.push_video(frame)` 之前。若未来 writer 变成生产 encoder，应考虑让主写入优先于 metadata 分析，或将 visual analysis 下沉到独立低优先级任务。

结论：当前未发现阻塞捕获 callback 的问题；consumer path 风险已从 Critical/Important 降到可控，但仍建议保留性能压力 Gate。

### 13.7 内存安全与内存压力专项结论

内存安全：

- 本轮 `stride_bytes` 传递没有新增 unsafe。
- ScreenCaptureKit 仍在 unlock 前 copy CVPixelBuffer 数据到 owned buffer，未发现 use-after-free。
- 新增逻辑主要是 safe Rust。

内存压力：

- `MAX_VISUAL_SAMPLES` / `MAX_AUDIO_SAMPLES` 已解决首轮无界 Vec 的主要问题。
- 但样本上限目前只是丢弃超限后的新样本，没有做时间桶压缩；超长录制的后半段 activity 会缺失。
- `TrimMetadataWriter` 仍使用 `serde_json::to_string_pretty()` 一次性生成完整 JSON，长录制 stop path 仍可能有停止耗时和临时内存峰值问题。

结论：Rust 内存安全未发现阻塞问题；长录制内存峰值与 sidecar 策略仍需要人工压力验证。

### 13.8 线程安全与资源释放专项结论

线程安全：

- `AudioRmsAnalyzer`、`FrameDiffAnalyzer` 都只在 consumer thread 内使用，没有共享 mutable state。
- `mic_level` 仍通过 `Arc<Mutex<f64>>` 共享。
- `build_cut_timeline` 仍用 session id、metadata path、beautify revision 防止 stale async writeback。

资源释放：

- stop path 仍按 cursor runtime、native capture、mic capture、consumer join、sidecar write、状态机收尾的顺序处理。
- trim sidecar 写失败不会跳过 mic reset 或 capture stop。

剩余风险：

- consumer thread panic 仍会被 `handle.join().unwrap_or(empty_output)` 吞掉。结合 Important 3，建议后续把 consumer panic 纳入 `RecordingFinalizeFailed`。

### 13.9 BUG.md 预防规则扫描

本轮扫描命令：

```bash
rg "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" -n src src-tauri
```

结果：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 `setIgnoreCursorEvents(true)` 回归。
- `src/components/recording-panel.tsx` 存在 `motion.button whileTap`，但这是 `whileTap` 直接作用在按钮元素本身，不是 BUG-003 中的 `motion.div whileTap` 作为 Button 直接父级。

结论：未发现 Phase 5 整改引入 BUG.md 预防规则回归。

### 13.10 Phase 5 完成度复核

| 项目 | 首轮状态 | 本轮整改后状态 | 复审结论 |
| --- | --- | --- | --- |
| 默认路径 cut timeline 可生成 | duration 为 0，no-op | observed activity fallback | 基本通过，需修超长录制 cap duration 风险 |
| 音频 RMS window | 单 chunk RMS | stateful aggregation | 方向通过，需精确切窗和 remainder |
| 低频 visual sampling | 每帧 diff | 250ms 采样 | 通过 |
| activity metadata bounded | 无界 Vec | 样本数 cap | 基本通过，需处理 cap 后 duration 与后半段 activity 缺失策略 |
| SCK stride | 忽略 stride | `stride_bytes` + `bytes_per_row` | 基本通过，需补 stride 下界校验 |
| Preview summary | 不展示 | 已展示 export summary | 基本通过，需配置变更清空旧 summary |
| React 数据流红线 | 未违规 | 未违规 | 通过 |
| 捕获 callback 不被阻塞 | 未直接阻塞 | 未直接阻塞 | 通过 |
| FFmpeg playable export | Gate | 仍为 Gate | 不计入本轮 blocker，但 Phase 6/FFmpeg Gate 必须继续 |

### 13.11 建议整改顺序

1. 修 `FrameDiffAnalyzer` stride 下界和 checked overflow，避免异常输入 panic。
2. 修 duration fallback：新增不受 metadata cap 影响的 `latest_observed_media_nanos`。
3. 修 `AudioRmsAnalyzer` 精确窗口切分和 remainder 保留。
4. 补 final drain audio cap 一致性。
5. Preview 配置变更时清空 `exportSummary`。
6. 补齐上述 regression tests。
7. 再跑完整验证和 10 分钟 1080p 手动压力 Gate。

### 13.12 建议新增测试

Rust：

- `audio_rms_emits_exact_configured_window_and_keeps_remainder`
- `audio_rms_large_chunk_emits_multiple_windows`
- `trim_metadata_duration_uses_latest_media_timestamp_after_sample_cap`
- `frame_diff_rejects_stride_smaller_than_width_bytes`
- `frame_diff_rejects_stride_height_overflow`
- `final_audio_drain_respects_metadata_cap`

前端：

- `preview_clears_export_summary_when_trim_config_changes`
- `preview_displays_export_trim_summary_after_successful_export`

人工：

- 8-12 秒静音静止素材：应产生至少 1 个 cut。
- 含加载动画、终端输出、鼠标移动素材：不应被误判为完全静止。
- 10 分钟 1080p 录制：记录 metadata 样本数、sidecar 大小、停止录制耗时、内存峰值。
- FFmpeg Gate：继续确认不伪造 playable output path。

### 13.13 Ready To Merge?

**结论：With fixes。**

本轮整改已经解决首轮 Critical 和大部分 Important 问题，Phase 5 的默认裁剪链路从“不可产生 cut”推进到了“可产生 cut timeline”。但 RMS 窗口精度、超长录制 duration fallback、stride 输入校验仍是应修问题，建议在进入下一阶段前完成。

## 14. 二轮整改复审（2026-05-29）

> 复审范围：`## 13. 首轮整改复审（2026-05-29）` 后的二轮整改代码
> 复审基线：`HEAD` = `a9bab95 fix: Phase 5 code review 整改`
> 复审对象：当前 working tree diff
> 结论：No Critical，仍建议修复 4 个 Important 后再视为 Phase 5 整改完成

### 14.1 复审输入

本轮复审对照以下文件：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-5-w9-w10-checklist.md`
- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md`
- `BUG.md`
- `HANDOFF.md`

本轮 working tree diff 涉及：

- `src-tauri/src/core/frame.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/silence_detector.rs`
- `src-tauri/src/media/trim_metadata.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src/App.test.tsx`
- `src/components/preview-view.tsx`

复审重点：

1. 第 13 节列出的 3 个 Important 和 2 个 Minor 是否真正修复。
2. Phase 5 是否仍满足计划、checklist 和架构红线。
3. 是否引入阻塞捕获主链路、内存安全、线程安全、资源释放异常。
4. 是否违反 `BUG.md` 预防规则。

说明：本轮按 `superpowers:requesting-code-review` 流程启动独立 code reviewer subagent，并同步进行本地代码审查。下文结论已合并独立 reviewer 和本地复核结果。

### 14.2 自动化验证结果

本轮复审实际执行：

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
```

结果：

- `git diff --check HEAD`: PASS。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，159 tests。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS，无 error。
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS。
- `npm run build`: PASS。
- `npm test -- --run`: PASS，45 tests。

测试和构建输出备注：

- Rust 仍有既有 SCK/FFI warning。
- 新增 1 条 `silence_detector.rs` 测试代码中的 irrefutable `if let` warning，见 Minor 1。
- Vitest 仍输出多条 `Window.scrollTo()` 未实现提示，测试通过。

### 14.3 第 13 节问题整改确认

| 第 13 节问题 | 当前整改状态 | 二轮复审结论 |
| --- | --- | --- |
| RMS window 聚合吞并超过窗口的余量 | `AudioRmsAnalyzer::push_chunk()` 改为返回 `Vec<AudioActivitySample>`，按配置窗口精确 drain，`flush()` 保留并输出 remainder | 核心逻辑基本修复；但窗口配置在录制期仍硬编码 Medium，见 Important 3 |
| duration fallback 受 sample cap 影响 | `MacRecordingService::consume_frames()` 新增 `latest_observed_media_nanos`，独立于 `MAX_*_SAMPLES` 更新 | 实现方向正确；但测试只验证表达式，没有覆盖 cap 场景，见 Important 4 |
| stride 修复缺少下界校验 | `FrameDiffAnalyzer::downsample_grayscale()` 增加 `stride >= width * 4` 和 `checked_mul(stride, height)` 校验 | 基本修复 |
| final drain audio sample push 没有复用 cap | final drain 和主循环都使用 `audio_activity.len() < MAX_AUDIO_SAMPLES` 保护 | 基本修复；仍建议抽 helper 提升可测性 |
| Preview summary 配置变化后不清空 | `handleBeautifyChange()` 调用 `setExportSummary(null)`，新增前端测试 | 静态配置变更场景已修复；但 in-flight export resolve 仍可写回旧 summary，见 Important 1 |

### 14.4 Strengths

1. 第 13 节核心整改大多已经落地
   - `AudioRmsAnalyzer` 不再把超过窗口的所有样本吞进一个 sample，而是按窗口精确切分，并通过 `flush()` 输出剩余短窗口。
   - `FrameDiffAnalyzer` 已补上 stride 下界和 `stride * height` 溢出校验，异常 `VideoFrame` 不再直接 panic。
   - `latest_observed_media_nanos` 独立于 capped activity Vec 更新，长录制在实现层面不再只依赖 `activity.last()` 推导 duration。
   - final drain 中的 audio sample push 已补 `MAX_AUDIO_SAMPLES` 检查。
   - Preview 在用户改变美化配置时会清空旧 export summary。

2. 捕获主链路边界保持正确
   - ScreenCaptureKit callback 仍只做 CVPixelBuffer copy 和 bounded channel `try_send_drop_newest`。
   - RMS 和 frame diff 仍在 Rust consumer thread 侧，不进入 React。
   - frame diff 仍保持约 250ms 一次的低频采样，没有回退到逐帧 diff。

3. React 数据流红线未被破坏
   - `src/lib/tauri.ts` 只暴露 `CutTimelineSummary` / `ExportSummary` 等轻量结构。
   - Preview 只调用 `buildCutTimeline()` / `exportVideo()`，没有接收 audio chunks、video frames、frame-diff samples 或 audio activity stream。
   - UI summary 显示的是段数、预计剪除时长和 FFmpeg Gate 状态，不包含媒体帧数据。

4. FFmpeg Gate 边界仍然谨慎
   - `export_video` 仍返回 `output_path: None`，没有伪造可播放文件。
   - `trim_exporter.rs` 保持结构化 `TrimExportRequest` 边界，没有拼接 FFmpeg CLI 字符串。
   - 原始素材/sidecar 没有被裁剪流程删除。

### 14.5 Issues

#### Critical

未发现新的 Critical 阻塞项。

本轮没有发现立即导致数据丢失、默认裁剪链路完全不可用、React 接收媒体帧流、捕获 callback 被裁剪逻辑阻塞、或明显 unsafe/资源释放路径破坏的问题。

#### Important 1: in-flight export resolve 后仍可能写回过期 summary

位置：

- `src/components/preview-view.tsx:164`
- `src/components/preview-view.tsx:201`
- `src/components/preview-view.tsx:204`
- `src/components/preview-view.tsx:206`

现象：

本轮修复在 `handleBeautifyChange()` 中调用了 `setExportSummary(null)`。这能处理“导出成功后用户再改配置”的静态场景。

但还有一个异步竞态：

1. 用户用配置 A 点击导出。
2. `exportVideo(preset)` 仍在进行中。
3. 用户切换 `autoTrimSilences` 或 `trimSensitivity` 到配置 B。
4. `handleBeautifyChange()` 清空旧 summary。
5. 配置 A 的旧 `exportVideo()` Promise 之后 resolve。
6. `handleExport()` 无条件 `setExportSummary(summary)`，旧 summary 会重新出现在配置 B 下。

为什么重要：

第 13 节 Minor 2 的目标是避免 Preview 展示过期 summary。当前修复只覆盖同步/静态配置变化，未覆盖真实 UI 中常见的 in-flight export race。用户可能看到“已生成裁剪时间线 X 段”但该结果属于上一组配置，尤其在 FFmpeg/导出耗时变长后更容易触发。

风险类型：

- 用户可见误导。
- Preview 展示状态与当前 trim config 不一致。
- 后续接入真实 FFmpeg 导出后，该 race 可能变得更常见。

建议修复：

1. 增加一个独立 revision ref，例如 `exportSummaryRevisionRef` 或复用/扩展现有 `buildSeqRef`。
2. `handleBeautifyChange()` 中递增该 revision，并清空 `exportSummary`。
3. `handleExport()` 开始时捕获当前 revision。
4. `exportVideo()` resolve 后，仅当 revision 仍匹配时才 `setExportSummary(summary)`。
5. 如果 revision 已变化，静默忽略该旧 export summary。

建议补充测试：

- `preview_ignores_export_summary_when_config_changes_before_export_resolves`
  - 构造 deferred `export_video`。
  - 点击导出后不 resolve。
  - 切换 auto trim 或 sensitivity。
  - resolve 旧 export。
  - 断言页面不显示“已生成裁剪时间线”。

#### Important 2: `AudioRmsAnalyzer` 使用合成连续时间轴，音频丢块或 timestamp gap 会导致 RMS sample 时间漂移

位置：

- `src-tauri/src/media/silence_detector.rs:44`
- `src-tauri/src/media/silence_detector.rs:48`
- `src-tauri/src/media/silence_detector.rs:51`
- `src-tauri/src/media/silence_detector.rs:68`
- `src-tauri/src/media/silence_detector.rs:75`

现象：

`AudioRmsAnalyzer` 第一次收到 chunk 时把 `window_start_nanos` 初始化为 `chunk.timestamp.nanos`。之后无论后续 chunk 的 timestamp 是多少，聚合器都只按 `self.window_start_nanos + rms_window_nanos` 推进窗口。

这隐含假设：

- 所有 mixed audio chunks 连续到达。
- 中间没有 channel drop。
- 没有 audio synchronizer age-out、系统音频/麦克风单源切换导致的明显 timestamp gap。
- chunk timestamp 与聚合器内部样本累计时长永远一致。

一旦 bounded audio channel 丢 chunk，或 mixed chunk timestamp 出现间隔，聚合器会把后续实际更晚的音频样本标到较早的 synthetic window 上。

为什么重要：

Phase 5 的 cut candidate 依赖音频静音窗口与 visual frame-diff 窗口的时间重叠。音频 sample 时间漂移会导致：

- 真实静音段和画面静止段错位，漏剪。
- 非静音段被标到静止窗口附近，误剪风险上升。
- 长录制中漂移逐步累积，cut timeline 可解释性下降。

风险类型：

- 算法正确性风险。
- 压力场景下的时间戳对齐风险。
- 与架构文档“音频同步必须基于时间戳对齐”存在潜在偏差。

建议修复：

1. 在 `push_chunk()` 中计算当前 chunk 的真实开始时间和根据 buffer 推导出的 expected start。
2. 当 `chunk.timestamp.nanos` 与 expected start 的差值超过阈值（例如 1-2 个 audio callback 或 50ms）时，显式处理 discontinuity：
   - 保守方案 A：先 `flush()` 当前 remainder，再把 `window_start_nanos` reset 到当前 chunk timestamp。
   - 保守方案 B：把 gap 视为静音 padding 写入 buffer，使时间轴保持真实连续。
   - 更精确方案 C：维护 timestamped sample spans，切窗时按真实时间边界消费。
3. 将策略写入注释，明确是 fixed window，不要称为 sliding window，除非后续实现重叠滑窗。

建议补充测试：

- `audio_rms_resets_or_pads_on_timestamp_gap`
  - 第一个 chunk 覆盖 0-250ms。
  - 第二个 chunk timestamp 跳到 2s。
  - 输出 sample 不应被标记到 250ms-1000ms 这类合成早期窗口。
- `audio_rms_preserves_timestamps_after_dropped_chunk`
  - 模拟缺失中间 chunk 后继续输入。
  - 断言输出 RMS sample start/end 与真实 chunk timestamp 策略一致。

#### Important 3: 录制期 RMS window 仍硬编码 Medium，Preview 的 trim sensitivity 不能真正配置 500ms-1000ms RMS 窗口

位置：

- `src-tauri/src/platform/macos_service.rs:370`
- `src-tauri/src/platform/macos_service.rs:371`
- `src-tauri/src/lib.rs:705`
- `src-tauri/src/lib.rs:710`
- `src-tauri/src/lib.rs:711`

现象：

`MacRecordingService::consume_frames()` 创建录制期 RMS analyzer 时固定使用：

```rust
TrimConfig::from_sensitivity(crate::core::cut::TrimSensitivity::Medium)
```

用户在 Preview 中选择 Low/Medium/High 后，`build_cut_timeline()` 确实会解析当前 `trim_sensitivity`，但此时录制期 `audio_activity` 已经按 Medium 的 750ms window 写进 metadata sidecar。

因此当前行为实际是：

- detector 阈值、`min_candidate_nanos`、buffer 等使用当前 sensitivity。
- RMS activity sample 的窗口粒度永远是 Medium 750ms。

为什么重要：

架构文档和 checklist 要求“500ms 到 1000ms 滑动窗口可配置”。当前实现的 `AudioRmsAnalyzer` 支持不同窗口，但生产录制路径没有把用户配置传进去。用户选择 High 预期 500ms 采样、Low 预期 1000ms 采样，实际 metadata 仍是 750ms。

风险类型：

- 功能完整性风险：配置项没有完整生效。
- checklist 偏差：自动化测试只覆盖 analyzer 能按不同 config 工作，没有覆盖 recording path 使用用户 config。
- 用户调参可解释性下降：灵敏度变化只改变后处理阈值，不改变 RMS 采样窗口。

建议修复：

可选方案：

1. 录制开始时将当前 `trim_sensitivity` 传给 consumer thread：
   - `MacRecordingService::start()` 已接收 `BeautifyConfigSnapshot`。
   - 可以从 snapshot 解析 `trim_sensitivity`，把 `TrimConfig` 或 `TrimSensitivity` 传入 `consume_frames()`。
   - 注意：如果用户录后在 Preview 调整 sensitivity，录制期 metadata 无法重新按新窗口生成。

2. 采集 sensitivity-independent 的基础窗口：
   - 录制期固定采集更细粒度 RMS，例如 500ms 或更低成本基础桶。
   - `build_cut_timeline()` 根据当前 sensitivity 在 post-process 阶段重新聚合/合并基础 activity。
   - 这更符合 Preview 中反复调整 Low/Medium/High 并重建 cut timeline 的交互预期。

3. 在当前 Phase 保守落地：
   - 明确将 recording metadata 的 RMS window 固定为 500ms 基础桶。
   - Low/Medium 在 detector 阶段通过连续窗口长度和阈值保守控制。
   - 更新注释和 checklist，避免声称录制期窗口随 Preview sensitivity 改变。

建议补充测试：

- `recording_trim_config_uses_beautify_snapshot_sensitivity`，如果采用方案 1。
- `build_cut_timeline_reaggregates_base_audio_activity_by_sensitivity`，如果采用方案 2。
- 前端或 Rust command 测试：切换 sensitivity 后 rebuild cut timeline 的输入/输出策略可解释。

#### Important 4: cap / final drain 回归测试仍偏浅，不能证明第 13 节指定风险不会回归

位置：

- `src-tauri/src/platform/macos_service.rs:592`
- `src-tauri/src/platform/macos_service.rs:598`
- `src-tauri/src/platform/macos_service.rs:612`

现象：

本轮新增的 `duration_fallback_uses_latest_media_timestamp()` 和 `duration_prefers_writer_when_nonzero()` 只是复制了最终表达式：

```rust
if writer_duration_nanos > 0 {
    writer_duration_nanos
} else {
    latest_observed_media_nanos
}
```

它们没有覆盖第 13 节建议的真实回归场景：

- `MAX_AUDIO_SAMPLES` / `MAX_VISUAL_SAMPLES` 达到上限后，继续输入更晚 media timestamp。
- `audio_activity` / `visual_activity` Vec 保持 cap。
- `duration_nanos` 仍等于最新媒体 timestamp，而不是 capped Vec 的最后一项。
- final drain 中 audio sample push 仍受 cap 约束。

为什么重要：

第 13 节 Important 2 和 Minor 1 都属于“bounded metadata 与 duration/final drain 组合后产生的边界问题”。当前测试没有驱动这些边界路径，将来维护时即使重新引入 `activity.last()` 或 final drain 无 cap，测试仍可能通过。

风险类型：

- 测试有效性风险。
- 长录制 metadata correctness 回归风险。
- 未来调整 cap 或抽样率时缺少防线。

建议修复：

1. 抽出小 helper，便于无线程单测：
   - `fn push_bounded_audio_sample(samples: &mut Vec<AudioActivitySample>, sample, max) -> bool`
   - `fn push_bounded_visual_sample(...) -> bool`
   - `fn choose_duration_nanos(writer_duration_nanos, latest_observed_media_nanos) -> u64`
2. 或建立 test-only harness，模拟 `consume_frames()` 的核心 sample push 和 duration fallback。
3. 测试必须证明“cap 已满后 duration 仍继续前进”，而不是只证明最后 if/else。

建议补充测试：

- `trim_metadata_duration_uses_latest_media_timestamp_after_sample_cap`
  - 构造 capped activity Vec。
  - 模拟更晚 video/audio timestamp 更新 `latest_observed_media_nanos`。
  - 断言 duration 使用最新 timestamp。
- `final_audio_drain_respects_metadata_cap`
  - audio activity 已到 cap。
  - final flush 或 final drain 产生 sample。
  - 断言 Vec 长度不超过 cap。
- `bounded_activity_push_reports_dropped_sample`
  - 如果 helper 返回 bool，可验证超 cap 样本被丢弃且不 panic。

#### Minor 1: 新增测试代码仍有 irrefutable `if let` warning

位置：

- `src-tauri/src/media/silence_detector.rs:547`

现象：

测试中：

```rust
if let FrameBuffer::Owned(ref mut buf) = &mut second.buffer {
    ...
}
```

但 `FrameBuffer` 当前只有 `Owned` 一个变体，该 pattern 永远匹配，Rust/clippy 输出 irrefutable `if let` warning。

为什么重要：

这不是功能问题，但会增加测试和 clippy 输出噪声，让后续真正的新 warning 更不显眼。

建议修复：

改为直接绑定：

```rust
let FrameBuffer::Owned(ref mut buf) = &mut second.buffer;
```

或者如果未来确实会增加其他 buffer 变体，则在此处显式 `match` 并处理非 Owned 分支。

### 14.6 捕获主链路专项结论

本轮未发现裁剪逻辑进入 ScreenCaptureKit callback。

确认点：

- `screen_capture_kit.rs` callback 仍在 unlock 前 copy pixel buffer，构造 `VideoFrame` 后通过 bounded channel 投递。
- `FrameDiffAnalyzer` 仍只在 `MacRecordingService::consume_frames()` consumer thread 中运行。
- visual diff 采样间隔仍是 `VISUAL_SAMPLE_INTERVAL_NANOS = 250_000_000`，约 4fps。
- `build_cut_timeline()` 仍使用 `spawn_blocking`，不会在 Tauri 主事件循环同步读 JSON / 跑 detector。

剩余关注：

- visual diff 仍发生在 `writer.push_video(frame)` 之前。当前 `CountingRecordingWriter` 成本很低，问题不明显。未来生产 FFmpeg writer 接入后，建议让主写入优先于 metadata 分析，或把 visual analysis 放到独立低优先级路径。
- `AudioRmsAnalyzer` 的 timestamp gap 问题属于媒体时间轴正确性风险，不是 callback 阻塞风险。

结论：当前未发现捕获 callback 被阻塞的问题；consumer path 性能风险保持可控，但 10 分钟 1080p 压力 Gate 仍需要保留。

### 14.7 内存安全与内存压力专项结论

内存安全：

- 本轮未新增 unsafe。
- `stride_bytes` 校验已经降低异常 frame 输入导致 panic 的风险。
- SCK 仍在 `CVPixelBufferUnlockBaseAddress` 前复制到 owned buffer，未发现 use-after-free。

内存压力：

- `MAX_VISUAL_SAMPLES` / `MAX_AUDIO_SAMPLES` 限制仍在。
- final drain 已补 audio cap。
- `TrimMetadataWriter` 仍使用 `serde_json::to_string_pretty()` 一次性构建完整 JSON。10 小时上限注释下样本量仍可控，但停止录制时的临时内存峰值和耗时仍需人工压力验证。
- 当前 cap 策略是“超过上限后丢弃后续 activity”，没有时间桶压缩；超长录制后半段 activity 可能缺失。由于 duration 已独立更新，timeline duration 不会被截断，但后半段无法生成 cut candidate。

结论：Rust 内存安全未发现阻塞问题；长录制 sidecar 体积、停止耗时、后半段 activity 缺失策略仍是后续 Gate。

### 14.8 线程安全与资源释放专项结论

线程安全：

- `AudioRmsAnalyzer`、`FrameDiffAnalyzer` 仍只在 consumer thread 内使用。
- `latest_observed_media_nanos` 是 consumer thread 局部变量，没有共享并发写。
- `build_cut_timeline()` 仍用 session id、trim metadata path、beautify revision 防止 stale async writeback。

资源释放：

- stop path 仍先停 cursor runtime，再停 native capture / mic capture，再 signal consumer、join、写 sidecar、更新状态机。
- trim sidecar 写入失败不会跳过 mic reset 或 capture stop。
- consumer thread panic 现在会 `eprintln!` 记录，而不是完全静默 `unwrap_or(empty_output)`；但仍会降级为空 metadata，不会把 panic 作为 `RecordingFinalizeFailed` 返回。

剩余建议：

- 后续可把 consumer panic 转成 errors push 到 `RecordingFinalizeFailed`，避免用户只看到“自动裁剪没有结果”而无明确失败信号。

### 14.9 BUG.md 预防规则扫描

本轮扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结果：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 `setIgnoreCursorEvents(true)` 回归。
- `src/components/recording-panel.tsx` 中仍存在 `motion.button whileTap`，这是 `whileTap` 直接作用在按钮自身，不属于 BUG-003 的 `motion.div whileTap` 作为交互按钮直接父级问题。

结论：未发现 Phase 5 二轮整改引入 BUG.md 预防规则回归。

### 14.10 Phase 5 完成度复核

| 项目 | 第 13 节状态 | 二轮整改后状态 | 复审结论 |
| --- | --- | --- | --- |
| 默认路径 cut timeline 可生成 | 基本通过，需修 cap duration 风险 | `latest_observed_media_nanos` 独立于 cap | 实现基本通过，测试需加强 |
| 音频 RMS window 精确切分 | 需修 exact window 和 remainder | 已按窗口 drain，保留 remainder | 核心逻辑通过，但 timestamp gap 和生产配置仍需修 |
| 低频 visual sampling | 通过 | 仍保持约 4fps | 通过 |
| activity metadata bounded | 基本通过，final drain cap 需补 | final drain 已补 cap | 基本通过，需补有效回归测试 |
| SCK stride | 需补 stride 下界校验 | 已补 stride 下界和 overflow 校验 | 通过 |
| Preview summary | 配置变化清空旧 summary | 静态配置变化已清空 | 需修 in-flight export race |
| React 数据流红线 | 未违规 | 未违规 | 通过 |
| 捕获 callback 不被阻塞 | 通过 | 仍通过 | 通过 |
| FFmpeg playable export | Gate | 仍为 Gate | 不计入本轮 blocker，但 Phase 6/FFmpeg Gate 必须继续 |

### 14.11 建议整改顺序

1. 修 Preview in-flight export summary race：增加 revision guard，并补 deferred Promise 测试。
2. 修 `AudioRmsAnalyzer` timestamp gap 策略：遇到 chunk timestamp discontinuity 时 reset、pad silence 或维护 timestamped spans，并补测试。
3. 修录制期 RMS sensitivity 生效问题：确定采用“录制期传入 snapshot sensitivity”还是“采集基础窗口、post-process 重新聚合”，并补对应测试。
4. 加强 cap / final drain regression tests：不要只测 if/else 表达式，要覆盖 cap 已满后 duration 仍前进、final drain 不突破 cap。
5. 清理 `silence_detector.rs` 测试中的 irrefutable `if let` warning。
6. 再跑完整验证命令，并保留 10 分钟 1080p 人工压力 Gate。

### 14.12 建议新增测试

Rust：

- `audio_rms_resets_or_pads_on_timestamp_gap`
- `audio_rms_preserves_timestamps_after_dropped_chunk`
- `recording_trim_config_uses_beautify_snapshot_sensitivity`（若采用录制期 sensitivity 方案）
- `build_cut_timeline_reaggregates_base_audio_activity_by_sensitivity`（若采用 post-process 重新聚合方案）
- `trim_metadata_duration_uses_latest_media_timestamp_after_sample_cap`
- `final_audio_drain_respects_metadata_cap`
- `bounded_activity_push_reports_dropped_sample`

前端：

- `preview_ignores_export_summary_when_config_changes_before_export_resolves`
- 保留现有 `clears export summary when beautify config changes`

人工：

- 8-12 秒静音静止素材：应产生至少 1 个 cut。
- 含加载动画、终端输出、鼠标移动素材：不应被误判为完全静止。
- 10 分钟 1080p 录制：记录 metadata 样本数、sidecar 大小、停止录制耗时、内存峰值。
- FFmpeg Gate：继续确认不伪造 playable output path。

### 14.13 Open Questions

1. RMS timestamp gap 采用哪种策略更符合产品预期：gap reset、gap 填静音，还是 timestamped spans 精确切窗？
2. Preview 中调整 trim sensitivity 是否必须无须重新录制即可改变 RMS window？如果是，应优先采用“基础窗口 + post-process 重新聚合”。
3. metadata cap 超限后的策略是否接受“后半段 activity 缺失但 duration 正确”，还是需要时间桶压缩以覆盖完整录制时长？
4. consumer thread panic 是否应该在 Phase 5 就升级为 `RecordingFinalizeFailed`，还是放到 FFmpeg/录制稳定性专项整改？

### 14.14 Ready To Merge?

**结论：With fixes。**

本轮整改已经把第 13 节中的主要实现问题推进到可用状态：RMS exact window、stride 校验、cap-independent duration、final drain cap 和静态 summary 清空都已落地，自动化验证也全部通过。但 in-flight export summary race、RMS timestamp gap、录制期 sensitivity 未真正影响 RMS window，以及 cap/final drain 测试有效性不足仍是应修问题。建议完成本节 4 个 Important 后，再认定 Phase 5 二轮整改完成。

## 15. 三轮整改复审（2026-05-29）

> 复审范围：`## 14. 二轮整改复审（2026-05-29）` 后的三轮整改代码
> 复审基线：`HEAD` = `a9bab95 fix: Phase 5 code review 整改`
> 复审对象：当前 working tree diff
> 结论：No Critical；Phase 5 裁剪时间线与导出边界合同基本完成，但仍有 1 个 Important 收尾风险和 FFmpeg Gate 未完成

### 15.1 复审输入

本轮复审对照以下文件：

- `HANDOFF.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-5-w9-w10-checklist.md`
- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md`
- `BUG.md`
- `.codex/rules/0-global.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/4-security.md`

本轮 working tree diff 涉及：

- `src-tauri/src/core/frame.rs`
- `src-tauri/src/media/recording_writer.rs`
- `src-tauri/src/media/silence_detector.rs`
- `src-tauri/src/media/trim_metadata.rs`
- `src-tauri/src/platform/macos/screen_capture_kit.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src/App.test.tsx`
- `src/components/preview-view.tsx`

复审重点：

1. 第 14 节列出的 4 个 Important 和 1 个 Minor 是否真正修复。
2. Phase 5 是否完整完成“空白段检测与自动裁剪”的开发任务。
3. 是否存在阻塞捕获主链路、内存安全、线程安全、资源释放路径异常。
4. 是否遵守 `BUG.md` 中拖拽、点击、点击穿透相关预防规则。

本轮按 `superpowers:requesting-code-review` 口径启动了独立 reviewer，并合并其结论与本地代码审查结果。

### 15.2 自动化验证结果

本轮复审实际执行：

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结果：

- `git diff --check HEAD`: PASS。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，166 tests。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS，无 error；仍有 warning，见 Minor 1。
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS。
- `npm run build`: PASS。
- `npm test -- --run`: PASS，46 tests。
- `BUG.md` 预防规则扫描：未发现禁止模式回归；命中项均为可接受的 `motion.button whileTap` 或测试断言。

输出备注：

- Rust 仍有既有 SCK/FFI naming / unused warning。
- 本轮 `clippy` 额外提示 `macos_service.rs` 的 `too_many_arguments` 与测试中的 `clone_on_copy`，不影响通过，但建议清理。
- Vitest 仍输出 `Window.scrollTo()` 未实现提示，测试通过。

### 15.3 第 14 节问题整改确认

| 第 14 节问题 | 当前整改状态 | 三轮复审结论 |
| --- | --- | --- |
| in-flight export resolve 后写回过期 summary | `PreviewView` 增加 `exportRevisionRef`；配置变化递增 revision；旧 export resolve 后不再 `setExportSummary`；新增 deferred Promise 前端测试 | 修复 |
| `AudioRmsAnalyzer` 合成连续时间轴导致 timestamp gap 漂移 | `push_chunk()` 增加 forward gap 检测，gap 超过一个 RMS window 时 flush remainder 并 reset 到新 chunk timestamp；新增 dropped chunk / timestamp gap 测试 | 基本修复；当前策略是 gap reset，不做静音 padding |
| 录制期 RMS window 硬编码 Medium | `MacRecordingService::start()` 从 `BeautifyConfigSnapshot` 提取 `trim_sensitivity` 并传入 consumer；consumer 按 snapshot 创建 `TrimConfig` | 修复录制期硬编码问题；录后再次调整 sensitivity 是否应重新聚合 RMS 仍是产品语义问题，见 15.13 |
| cap / final drain 回归测试偏浅 | 新增 bounded push helper、`choose_duration_nanos()` helper 和 cap/final drain 测试；final drain 使用同一 bounded helper | 基本修复 |
| irrefutable `if let` warning | 测试代码已改为直接 `let FrameBuffer::Owned(...) = ...` 绑定 | 修复 |

### 15.4 Strengths

1. 第 14 节核心整改已闭合
   - Preview 的旧 export summary 不会在配置变化后异步回填。
   - RMS 窗口已经支持 exact window、remainder、forward timestamp gap reset。
   - 录制期不再固定 Medium，而是使用录制开始时的 `BeautifyConfigSnapshot.trim_sensitivity`。
   - cap 与 final drain push 已统一走 bounded helper。

2. 捕获主链路仍保持隔离
   - ScreenCaptureKit callback 没有执行 RMS、帧差分、JSON 写入或 Tauri 事件发送。
   - callback 仍通过 bounded media channel 投递帧/音频。
   - visual diff 仍在 consumer thread 中约 4fps 低频执行。
   - `build_cut_timeline()` 仍通过 `spawn_blocking` 执行 JSON 读写和 detector 分析。

3. React 数据流红线未被破坏
   - 前端只接收 `ExportSummary` / `CutTimelineSummary` 等轻量 summary。
   - 未发现 audio chunk、video frame、frame-diff sample、audio activity stream 进入前端 JS 层。
   - `src/lib/tauri.ts` 暴露的是结构化 command boundary，没有媒体流 API。

4. 内存压力风险比首轮显著下降
   - visual/audio activity Vec 均有样本上限。
   - duration fallback 独立于 capped Vec 更新。
   - stride 校验已覆盖 padded row、stride 下界和 overflow。

5. 测试覆盖持续补强
   - Rust 从本文件第 14 节的 159 tests 增加到 166 tests。
   - 前端从 45 tests 增加到 46 tests。
   - 新增测试覆盖 export summary race、RMS timestamp gap、bounded final drain 等高风险回归点。

### 15.5 Issues

#### Critical

未发现新的 Critical 阻塞项。

本轮没有发现立即导致数据丢失、捕获 callback 被阻塞、React 接收媒体帧流、明显 unsafe 内存问题、或默认裁剪时间线完全不可用的问题。

#### Important 1: consumer thread panic 会被降级为空结果，`stop()` 仍可能返回成功

位置：

- `src-tauri/src/platform/macos_service.rs:272`
- `src-tauri/src/platform/macos_service.rs:275`
- `src-tauri/src/platform/macos_service.rs:277`
- `src-tauri/src/platform/macos_service.rs:306`
- `src-tauri/src/platform/macos_service.rs:333`

现象：

`MacRecordingService::stop()` join consumer thread 时，如果 consumer panic：

```rust
Err(panic) => {
    eprintln!("录制消费线程异常终止: {panic:?}");
    empty_output
}
```

随后代码仍会继续写一个空的 trim metadata sidecar。只要 cursor metadata 写入、trim metadata 写入、capture stop 和 mic stop 都没有返回错误，`errors` 仍为空，状态机会进入 `completed`，`stop()` 返回 `Ok(result)`。

为什么重要：

这属于资源释放 / 收尾路径异常被吞掉。用户可能看到录制完成，但：

- `frame_count = 0`
- `mixed_audio_chunk_count = 0`
- `trim_metadata.duration_nanos = 0`
- 自动裁剪没有结果

这会把“录制消费线程崩溃”伪装成“正常完成但没有可裁剪内容”，排查成本很高。它不一定是当前 happy path 的高概率问题，但一旦 frame diff、writer、audio synchronizer 或未来 FFmpeg writer panic，当前路径会隐藏真正故障。

风险类型：

- 资源释放路径异常被吞掉。
- 用户可见的 false success。
- 录制/裁剪问题定位困难。

建议修复：

1. 在 join 失败时把错误加入 `errors`：
   - `errors.push(format!("录制消费线程异常终止: {panic_msg}"))`
2. 保留现有清理顺序：
   - cursor runtime stop
   - native capture stop
   - mic stop
   - signal consumer
   - join
   - mic reset
   - state machine fail / finalize
3. 可以选择仍尝试写 cursor metadata；trim metadata 对 panic case 建议不要写空 sidecar，或写入后也必须返回 `RecordingFinalizeFailed`。
4. 新增测试或 helper 验证：
   - consumer join panic 时 `stop()` 最终返回 `RecordingFinalizeFailed`。
   - 即使 consumer panic，mic level reset 和 capture/mic stop error 聚合仍执行。

#### Important 2: 真实可播放 FFmpeg 裁剪导出仍未完成，只能认定 Phase 5 code contract 完成

位置：

- `src-tauri/src/lib.rs:631`
- `src-tauri/src/lib.rs:641`
- `tests/phase-5-w9-w10-checklist.md:42`
- `tests/phase-5-w9-w10-checklist.md:43`
- `tests/phase-5-w9-w10-checklist.md:44`

现象：

`export_video()` 当前会构建 cursor effect timeline，并在 `auto_trim_silences` 开启时构建 cut timeline，但最终仍返回：

```rust
output_path: None
```

Phase 5 checklist 中以下项仍是未完成 Gate：

- FFmpeg 封装能消费 `CutTimeline`
- 裁剪后视频可播放
- 音视频同步未明显漂移

为什么重要：

从本轮 Phase 5 plan 的边界看，真实 playable output 是明确保留的 FFmpeg Gate，因此这不是本轮代码实现的回归，也不应该要求当前代码伪造输出路径。但从总架构文档 W9-W10 “通过 FFmpeg 封装执行裁剪导出”的完整产品目标看，Phase 5 还不能被描述为“真实裁剪导出已完成”。

建议处理：

1. 当前阶段结论应写成：
   - “裁剪检测、`CutTimeline`、sidecar、Preview/export command boundary 已基本完成。”
   - “真实可播放 FFmpeg 裁剪导出仍为 Gate，未完成。”
2. 如果要把 Phase 5 视为完全完成，需要后续接入生产 FFmpeg encoder/muxer，并做可播放文件、音视频同步、原始素材保留验证。
3. 不要在 FFmpeg 未接入前返回假的 `outputPath`。

### 15.6 Minor Issues

#### Minor 1: 本轮新增 clippy warning 应清理，避免掩盖后续真实 warning

位置：

- `src-tauri/src/platform/macos_service.rs:361`
- `src-tauri/src/platform/macos_service.rs:681`
- `src-tauri/src/platform/macos_service.rs:682`
- `src-tauri/src/platform/macos_service.rs:683`
- `src-tauri/src/platform/macos_service.rs:704`
- `src-tauri/src/platform/macos_service.rs:709`

现象：

`cargo clippy --all-targets` 通过，但新增了与本轮 diff 相关的 warning：

- `consume_frames()` 参数 8 个，触发 `clippy::too_many_arguments`。
- 测试中对 `AudioActivitySample` / `FrameDiffSample` 使用 `.clone()`，但这些类型实现了 `Copy`，触发 `clippy::clone_on_copy`。

为什么重要：

项目目前允许 warnings 存在，但新增 warning 会让既有 SCK/FFI warning 噪声继续扩大。未来如果 CI 改为 `-D warnings`，这些会变成阻塞。

建议修复：

1. 对测试中的 `.clone()` 直接移除。
2. `consume_frames()` 可接受短期保留；若要清理，可把 consumer 入参收敛成小 struct，例如 `RecordingConsumerInputs`，避免继续拉长函数签名。

#### Minor 2: `tests/phase-5-w9-w10-checklist.md` 验证摘要已过期

位置：

- `tests/phase-5-w9-w10-checklist.md:9`
- `tests/phase-5-w9-w10-checklist.md:10`
- `tests/phase-5-w9-w10-checklist.md:14`

现象：

Checklist 仍记录：

- Rust tests: 149
- Frontend tests: 44

本轮实际验证为：

- Rust tests: 166
- Frontend tests: 46

建议修复：

- 更新 checklist 的 Verification Summary。
- 保留 FFmpeg Gate、10 分钟 1080p 内存压力、原始素材人工确认等未完成项，不要误标为完成。

#### Minor 3: 录后修改 trim sensitivity 是否应重算 RMS window 仍需产品确认

位置：

- `src-tauri/src/platform/macos_service.rs:376`
- `src-tauri/src/platform/macos_service.rs:378`
- `src-tauri/src/lib.rs:705`
- `src-tauri/src/lib.rs:710`

现象：

本轮已经修复“录制期 RMS window 固定 Medium”的问题：录制开始时会读取 `BeautifyConfigSnapshot.trim_sensitivity`。但 Preview 允许用户在录制完成后继续调整 `trimSensitivity`，此时：

- `build_cut_timeline()` 会使用当前 sensitivity 的阈值、候选时长和 buffer。
- 但 trim metadata 中的 audio RMS samples 已经按录制开始时的 window 生成，无法按新的 500ms/750ms/1000ms 重新切窗。

判断：

这不是当前代码的直接 bug，因为第 14 节给出的可选方案之一就是“录制期传入 snapshot sensitivity”，本轮实现选择了这条路径。但产品语义上需要明确：Preview 中的 sensitivity 是“影响后处理阈值”，还是“完整影响 RMS window 并支持录后反复重建”。

建议：

- 如果接受当前方案，在 UI/文档/checklist 中明确 RMS window 取录制开始时配置。
- 如果希望录后调整完全生效，后续应改为采集更细粒度基础 RMS bucket，并在 `build_cut_timeline()` 按当前 sensitivity 重新聚合。

### 15.7 捕获主链路专项结论

未发现 Phase 5 三轮整改把裁剪逻辑放入 ScreenCaptureKit callback。

确认点：

- `screen_capture_kit.rs` callback 仍只做必要的 CVPixelBuffer copy、构造 `VideoFrame`、bounded channel 投递。
- `AudioRmsAnalyzer` 与 `FrameDiffAnalyzer` 只在 `MacRecordingService::consume_frames()` consumer thread 中使用。
- visual diff 仍按 `VISUAL_SAMPLE_INTERVAL_NANOS = 250_000_000` 低频采样。
- `build_cut_timeline()` 仍通过 `spawn_blocking` 后台执行。

剩余关注：

- visual diff 仍在 `writer.push_video(frame)` 之前执行。当前 `CountingRecordingWriter` 成本低，风险可控；未来接入生产 FFmpeg writer 后，建议让主写入优先，或把 visual metadata 分析拆到独立低优先级路径。
- 10 分钟 1080p 压力 Gate 仍需人工验证。

结论：未发现阻塞捕获 callback 的问题；consumer path 的性能风险目前可控，但仍需真实素材压力测试。

### 15.8 内存安全与内存压力专项结论

内存安全：

- 本轮未新增 unsafe。
- `VideoFrame.stride_bytes` 已传递真实 SCK `bytes_per_row`。
- `FrameDiffAnalyzer` 已校验 stride 下界和 `stride * height` overflow。
- SCK callback 仍在 `CVPixelBufferUnlockBaseAddress` 前复制到 owned buffer，未发现 use-after-free。

内存压力：

- `MAX_VISUAL_SAMPLES` / `MAX_AUDIO_SAMPLES` 已限制 activity Vec 增长。
- `latest_observed_media_nanos` 独立于 capped Vec，避免长录制 duration 被 cap 截断。
- `TrimMetadataWriter` 仍用 `serde_json::to_string_pretty()` 一次性构造 JSON；在长录制场景仍需要人工测 sidecar 体积、停止耗时和内存峰值。

结论：未发现 Rust 内存安全阻塞项；长录制内存压力仍是人工 Gate。

### 15.9 线程安全与资源释放专项结论

线程安全：

- `AudioRmsAnalyzer`、`FrameDiffAnalyzer`、`latest_observed_media_nanos` 都是 consumer thread 局部状态。
- `mic_level` 仍通过 `Arc<Mutex<f64>>` 共享。
- `build_cut_timeline()` / `build_cursor_effect_timeline()` 仍使用 session id、metadata path、beautify revision 做 stale result guard。
- Preview 侧新增 `exportRevisionRef`，解决旧 export Promise resolve 后写回旧 summary 的竞态。

资源释放：

- stop path 仍按 cursor runtime、native capture、mic capture、signal consumer、join、sidecar 写入、mic reset、状态机收尾顺序执行。
- trim sidecar 写失败不会跳过 mic reset 或 capture/mic stop error 聚合。

剩余风险：

- consumer thread panic 当前只 `eprintln!` 并降级为空结果，未进入 `RecordingFinalizeFailed`。这是本轮唯一 Important 资源释放/收尾路径问题。

### 15.10 BUG.md 预防规则扫描

本轮扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结果：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 `setIgnoreCursorEvents(true)` / `ignoreCursor` 回归。
- `src/components/recording-panel.tsx` 中存在 `motion.button whileTap`，这是 `whileTap` 直接作用在按钮自身，不属于 BUG-003 中的 `motion.div whileTap` 作为交互按钮直接父级问题。
- `src/App.test.tsx` 命中的是断言不存在 `data-tauri-drag-region="false"` 的测试代码，非产品代码回归。

结论：未发现 Phase 5 三轮整改引入 `BUG.md` 预防规则回归。

### 15.11 Phase 5 完成度复核

| 项目 | 当前状态 | 复审结论 |
| --- | --- | --- |
| `CutTimeline` / activity sample 模型 | 已实现 | 通过 |
| 音频 RMS window | exact window、remainder、forward gap reset、录制期 sensitivity | 基本通过 |
| 低分辨率帧差分 | 64x36 灰度缩略图、约 4fps 低频采样、stride-aware | 通过 |
| 候选空白段合并和 buffer | 已实现并有测试 | 通过 |
| metadata bounded | audio/visual samples 有 cap，duration 独立更新 | 基本通过 |
| 默认 writer duration 为 0 fallback | `latest_observed_media_nanos` fallback | 通过 |
| Preview summary | 展示 export summary，配置变化清空，旧 export resolve 不回填 | 通过 |
| React 不接触媒体流 | 未发现违规 | 通过 |
| 捕获 callback 不阻塞 | 未发现违规 | 通过 |
| 原始素材保留 | 当前流程不删除原始素材 | 通过，但仍建议人工确认 |
| FFmpeg playable trimmed output | `outputPath: None`，Gate 未完成 | 未完成，不能称为真实裁剪导出完成 |
| consumer panic 收尾 | panic 降级 empty output，可能 false success | 需整改 |

### 15.12 建议整改顺序

1. 修 consumer thread panic false success：
   - join panic 时 push error，最终返回 `RecordingFinalizeFailed`。
   - 保持 cleanup 顺序不变。
2. 清理新增 clippy warning：
   - 移除 test 中 `clone_on_copy`。
   - 视情况把 `consume_frames()` 入参收敛成 struct。
3. 更新 `tests/phase-5-w9-w10-checklist.md` 的验证摘要：
   - Rust 166 tests。
   - Frontend 46 tests。
   - 保留 FFmpeg / 长录制压力 / 原始素材人工确认 Gate。
4. 明确 trim sensitivity 录后调整语义：
   - 当前方案是录制期 snapshot window + 后处理阈值。
   - 如果产品要求录后完整重算 RMS window，后续改成基础 RMS bucket + post-process reaggregation。
5. FFmpeg Gate 保持显式：
   - 未接入生产 encoder/muxer 前继续返回 `outputPath: None`。
   - 不伪造 playable file。

### 15.13 建议新增或调整测试

Rust：

- `stop_returns_finalize_error_when_consumer_thread_panics`
- `consumer_panic_still_resets_mic_level_and_collects_stop_errors`
- 可选：`recording_trim_metadata_records_rms_window_source`，用于明确 snapshot sensitivity 语义

前端：

- 现有 `ignores export summary when config changes before export resolves` 已覆盖关键 race，无需新增。

人工：

- 8-12 秒静音且画面静止素材：应产生至少 1 个 cut。
- 含加载动画、终端输出、鼠标移动素材：不应误判为完全静止。
- 10 分钟 1080p 录制：记录 metadata 样本数、sidecar 大小、停止录制耗时、内存峰值。
- FFmpeg Gate：继续确认不伪造 playable output path。
- 原始素材保留：确认裁剪流程不会删除 raw recording artifact。

### 15.14 Open Questions

1. Preview 中录后调整 `trimSensitivity` 是否必须完整改变 RMS window？如果必须，当前 snapshot 方案还不够，需要基础 bucket + post-process reaggregation。
2. consumer thread panic 是否在 Phase 5 必须作为阻塞错误返回？本轮建议是“必须”，因为否则会产生 false success。
3. metadata cap 超限后是否接受“后续 activity 被丢弃但 duration 正确”？如果产品要支持超长录制自动裁剪，需要时间桶压缩覆盖完整时长。
4. FFmpeg Gate 是继续留到 Phase 6，还是作为 Phase 5 完整完成前的必修项？

### 15.15 Ready To Merge?

**结论：With fixes / Code contract mostly ready, product goal gated。**

Phase 5 的裁剪检测基础、metadata sidecar、`CutTimeline` 生成、Preview/export command boundary、React 数据流红线、低频采样、bounded metadata、RMS window、stride safety 和 export summary race 都已经基本闭合。

仍不建议直接把 Phase 5 描述为“完整产品能力完成”，原因有两个：

1. consumer thread panic 当前可能被伪装成成功完成，这是应修的 Important 收尾路径问题。
2. 真实可播放 FFmpeg 裁剪导出仍是明确 Gate，当前只能说 cut timeline 与 export boundary 完成，不能说 playable trimmed export 完成。

## 16. 三轮整改补充复审（2026-05-29）：排除 Phase 6 合并项后的 Phase 5 代码复核

> 复审范围：第 15 节三轮整改后的当前 working tree
> 额外判断：`docs/superpowers/plans/2026-05-29-phase-5-ffmpeg-trim-completion.md` 是否可与总规划 Phase 6 一起执行
> 本节排除项：第 15 节 Important 2（FFmpeg Gate）与 Minor 3（录后 trim sensitivity 完整重聚合语义）
> 结论：可以并入 Phase 6；排除上述两个产品/阶段决策项后，Phase 5 其余 code contract 未发现 Critical 或 Important 阻塞

### 16.1 Phase 6 合并判断

可以把 `2026-05-29-phase-5-ffmpeg-trim-completion.md` 中的 FFmpeg 可播放导出与 trim sensitivity 完整重聚合计划，和 `docs/architecture/project-architecture-and-overall-planning.md` 的 Phase 6 一起执行。

判断依据：

- 总规划中 Phase 5 的依赖关系明确写着“Phase 6 三种导出预设消费裁剪时间线”。
- Phase 6 的研发重点包含 16:9、9:16、1:1 三种导出预设、导出进度与取消；真实 FFmpeg encoder/muxer、原始素材保留、裁剪时间线消费、本地输出路径校验，本质上属于导出流水线能力。
- 第 15 节 Important 2 的本质不是“当前代码 bug”，而是“Phase 5 是否必须在本阶段完成真实 playable trimmed export”的产品边界问题。
- 第 15 节 Minor 3 的本质也不是当前代码缺陷，而是 Preview 灵敏度是否必须支持录后完整重聚合 RMS window 的产品语义问题。

合并执行的约束：

1. 不建议把它们混成一个巨大 Phase 6 diff。应把 FFmpeg 原始录制、可播放导出、cut timeline 消费、outputPath 校验、基础 RMS bucket / post-process reaggregation 作为 Phase 6 的导出流水线前置子阶段。
2. 在该子阶段完成前，Phase 6 不应宣称“三种格式导出成功”。
3. 继续保持当前安全边界：Rust 侧处理媒体，React 只发配置/导出命令和接收 summary/path。
4. FFmpeg 集成必须走 Rust binding / C API 边界，不拼接 CLI 字符串。
5. 不得删除或覆盖原始 recording artifact；导出必须产出独立 playable output。
6. `outputPath` 只能在真实文件存在且非空后返回，不允许伪造路径。

因此，本轮 Phase 5 复审先忽略第 15 节 Important 2 和 Minor 3，把它们转为 Phase 6 前置交付项；Phase 5 只按“空白检测、bounded metadata、cut timeline、Preview/export command boundary、数据流红线、资源收尾”来判定。

### 16.2 自动化验证结果

本轮复审确认的验证结果：

```bash
git diff --check HEAD
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结果：

- `git diff --check HEAD`: PASS。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，169 tests。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS。
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS。
- `npm run build`: PASS。
- `npm test -- --run`: PASS，46 tests。
- `BUG.md` 预防规则扫描：未发现禁止模式回归。

输出备注：

- `clippy` / `build` 仍有 SCK/FFI 相关 warning，例如 CoreMedia/CoreVideo FFI 命名、未使用的 FFI binding、SCK 相关 `unsafe` 提示等；本轮未发现新增的 `clone_on_copy` warning。
- `consume_frames()` 的参数数量目前用 `#[allow(clippy::too_many_arguments)]` 显式压制。可以后续清理为小 struct，但不是 Phase 5 阻塞项。
- Vitest 仍会输出 `Window.scrollTo()` 未实现提示；测试通过，属于测试环境噪声。

### 16.3 第 15 节非延期项整改复核

| 第 15 节问题 | 当前状态 | 本轮结论 |
| --- | --- | --- |
| Important 1：consumer thread panic 被降级为空结果，`stop()` 可能返回成功 | `stop()` join panic 时提取 panic message、写入 `errors`，并标记 `consumer_panicked`；panic 时跳过空 trim sidecar；最终走 `RecordingFinalizeFailed` | 已修复 |
| Important 2：真实可播放 FFmpeg 裁剪导出未完成 | 转入 Phase 6 导出流水线前置子阶段 | 本轮不作为 Phase 5 code blocker |
| Minor 1：新增 clippy warning | 当前未见第 15 节提到的 `clone_on_copy` warning；`too_many_arguments` 已显式 allow | 不阻塞 |
| Minor 2：checklist 验证摘要过期 | `tests/phase-5-w9-w10-checklist.md` 已更新为 Rust 169 tests、frontend 46 tests | 已修复 |
| Minor 3：录后修改 trim sensitivity 是否完整重算 RMS window | 转入 Phase 6 产品语义与实现计划 | 本轮不作为 Phase 5 code blocker |

consumer panic 修复重点：

- `src-tauri/src/platform/macos_service.rs:272`-`280`：`handle.join()` panic 后提取 message，写入 `errors`，返回 `(empty_output, true)`。
- `src-tauri/src/platform/macos_service.rs:309`-`323`：consumer panic 时不写空 trim metadata sidecar，避免误导后处理链路。
- `src-tauri/src/platform/macos_service.rs:326`-`337`：mic level reset、capture stop / mic stop error collection 仍会执行。
- `src-tauri/src/platform/macos_service.rs:350`-`354`：有 errors 时进入 `RecordingFinalizeFailed`，不再 false success。

### 16.4 Findings

#### Critical

未发现 Critical 阻塞项。

本轮未发现会立即导致数据丢失、捕获 callback 被阻塞、React 接收媒体帧流、明显 Rust 内存安全问题、默认 cut timeline 完全不可用、或 stop path false success 的问题。

#### Important

排除第 15 节 Important 2（FFmpeg Gate）和 Minor 3（trim sensitivity 录后完整重聚合）后，未发现 Phase 5 其余 code contract 的 Important 阻塞项。

#### Advisory 1: Writer 错误在当前 mock writer 阶段可接受，但必须作为 Phase 6 生产 writer 前置整改

位置：

- `src-tauri/src/platform/macos_service.rs:432`-`434`
- `src-tauri/src/platform/macos_service.rs:473`-`475`
- `src-tauri/src/platform/macos_service.rs:500`-`502`
- `src-tauri/src/platform/macos_service.rs:526`-`528`
- `src-tauri/src/platform/macos_service.rs:539`-`548`

现状：

- `writer.push_video()` / `writer.push_audio()` 失败时仅 `eprintln!`。
- `writer.finish()` 失败时 `unwrap_or(RecordingResult { ... })` fallback 为空结果。
- 当前默认 `CountingRecordingWriter` 基本不会失败，所以这不是本轮 Phase 5 code contract 的实际阻塞。

为什么仍需记录：

Phase 6 接入生产 FFmpeg writer 后，这条路径必须改为 fatal。否则真实 encoder/muxer 写入失败、封装失败、磁盘失败可能被降级为“录制完成但无输出/无音视频”，形成 false success。

Phase 6 前置要求：

1. `push_video()` / `push_audio()` 错误需要进入 consumer output error collection。
2. `finish()` 错误必须返回 `RecordingFinalizeFailed` 或结构化 export/write error。
3. producer/capture stop cleanup 仍需保持执行，不能因为 writer error 跳过资源释放。
4. 增加失败 writer 测试，覆盖 video push、audio push、finish 三类失败。

#### Advisory 2: 长录制压力仍需人工 Gate，不建议只凭单元测试关闭

位置：

- `src-tauri/src/platform/macos_service.rs:389`-`400`
- `src-tauri/src/media/trim_metadata.rs:22`-`35`
- `tests/phase-5-w9-w10-checklist.md:42`-`44`

现状：

- visual activity 约 4fps 采样，并有 `MAX_VISUAL_SAMPLES`。
- audio activity 有 `MAX_AUDIO_SAMPLES`。
- `latest_observed_media_nanos` 独立于 capped Vec，duration 不会因 cap 截断。
- `TrimMetadataWriter` 仍使用 `serde_json::to_string_pretty()` 一次性构造 JSON。

判断：

这已经满足 Phase 5 “bounded metadata” 的代码 contract，但 10 分钟 1080p 实录的 sidecar 大小、停止录制耗时、内存峰值仍需人工验证。该 Gate 不应和 FFmpeg Gate 混淆，也不应在未跑真实素材前标为完成。

#### Advisory 3: Native Safety Gate 仍需人工审查

位置：

- `src-tauri/src/platform/macos/screen_capture_kit.rs:188`-`224`

现状：

- SCK callback 在 `CVPixelBufferUnlockBaseAddress` 前 copy 到 owned `Arc<[u8]>`，避免 unlock 后访问 base address。
- `VideoFrame.stride_bytes` 使用真实 `bytes_per_row`。
- `FrameDiffAnalyzer` 已校验 `stride >= width * 4` 和 `stride * height` overflow。

判断：

本轮未发现新增 unsafe 或 use-after-free 风险。但 AGENTS.md 明确要求跨平台底层 API / SCK FFI 需要人工逐行审查内存安全与并发安全；该 Gate 仍保留。

### 16.5 捕获主链路专项结论

未发现 Phase 5 剩余代码阻塞捕获主链路。

确认点：

- ScreenCaptureKit callback 仍只做必要的 CVPixelBuffer lock/copy/unlock、构造 `VideoFrame`、bounded channel 投递。
- RMS 分析、frame diff、metadata Vec push 都在 `MacRecordingService::consume_frames()` consumer thread，不在 callback。
- visual diff 仍按 `VISUAL_SAMPLE_INTERVAL_NANOS = 250_000_000` 低频采样，不是逐帧 diff。
- `build_cut_timeline()` 使用 `spawn_blocking`，不会在 Tauri 主事件循环同步读 JSON 或跑 detector。
- React 不接收 audio chunk、video frame、frame-diff sample 或 audio activity stream。

剩余关注：

- 当前 visual diff 仍发生在 `writer.push_video(frame)` 前。对 `CountingRecordingWriter` 风险很低；Phase 6 接入生产 encoder 后，建议重新评估“主写入优先于 metadata 分析”或拆独立低优先级分析路径。

### 16.6 内存安全、线程安全与资源释放专项结论

内存安全：

- 本轮 Phase 5 剩余 diff 主要是 safe Rust。
- SCK frame buffer 在 unlock 前复制为 owned buffer；未发现 unlock 后悬垂引用。
- `FrameDiffAnalyzer` 已 stride-aware，并拒绝 stride 过小和乘法溢出。
- 未发现新增裸指针生命周期风险。

内存压力：

- audio/visual activity Vec 有 cap。
- duration 使用 `latest_observed_media_nanos`，独立于 cap。
- JSON sidecar 仍是一次性 pretty serialization，长录制压力需人工 Gate。
- cap 超限后的策略是丢弃后续 activity，但保留 duration；如果未来要支持超长录制完整自动裁剪，建议改为时间桶压缩，而不是简单 drop。

线程安全：

- `AudioRmsAnalyzer`、`FrameDiffAnalyzer`、`latest_observed_media_nanos` 都是 consumer thread 局部状态。
- `mic_level` 仍通过 `Arc<Mutex<f64>>` 共享。
- `build_cursor_effect_timeline()` / `build_cut_timeline()` 使用 session id、metadata path、beautify revision stale guard。
- Preview 使用 `exportRevisionRef` 避免旧 export Promise 在配置变化后回填过期 summary。

资源释放：

- `stop()` 顺序仍是 cursor runtime stop、native capture stop、mic stop、signal consumer、join、sidecar 写入、mic reset、state machine terminal。
- consumer panic 已进入 `errors`，最终返回 `RecordingFinalizeFailed`，不再 false success。
- consumer panic 时跳过空 trim sidecar，避免生成误导性 metadata。
- capture/mic stop error 仍会聚合，不会被 trim sidecar 路径覆盖。

### 16.7 BUG.md 预防规则扫描

扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结论：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 `setIgnoreCursorEvents(true)` / `ignoreCursor` 回归。
- `src/components/recording-panel.tsx` 中的 `motion.button whileTap` 是动画直接作用于按钮自身，不是 BUG-003 禁止的 `motion.div whileTap` 作为交互元素直接父容器。
- `src/App.test.tsx` 命中的是断言禁止模式不存在的测试代码，不是产品代码回归。

### 16.8 Phase 5 完成度复核

| 项目 | 当前状态 | 本轮结论 |
| --- | --- | --- |
| `CutTimeline` / `CutSegment` / `KeepSegment` / activity sample 模型 | 已实现并有 serde 测试 | 通过 |
| 音频 RMS window | exact window、remainder、forward gap reset、录制期 snapshot sensitivity | Phase 5 code contract 通过 |
| 低分辨率帧差分 | 64x36 灰度缩略图、约 4fps 低频采样、stride-aware | 通过 |
| 候选空白段合并和 buffer | 已实现并覆盖长静音/短暂停顿/相邻候选合并 | 通过 |
| trim metadata sidecar | 有 bounded audio/visual samples，duration 独立更新 | 通过，保留长录制人工 Gate |
| `build_cut_timeline` command | 读 sidecar、spawn_blocking、stale guard、写 cut timeline | 通过 |
| Preview export summary | 展示 cut count / total cut seconds / FFmpeg Gate，配置变化清空，旧 export 不回填 | 通过 |
| React 数据流红线 | 未发现媒体帧或 activity stream 进入前端 | 通过 |
| 捕获 callback 不阻塞 | 未发现 RMS/frame diff/JSON 写入进入 callback | 通过 |
| 原始素材保留 | 当前流程不删除原始素材 | 代码层通过，仍建议人工确认 |
| consumer panic 收尾 | panic 进入 `RecordingFinalizeFailed`，不写空 trim sidecar | 通过 |
| FFmpeg playable trimmed output | 转 Phase 6 前置导出流水线 | 本轮不作为 Phase 5 blocker |
| 录后 sensitivity 完整重聚合 RMS window | 转 Phase 6 产品语义与实现计划 | 本轮不作为 Phase 5 blocker |

### 16.9 Phase 6 前置执行建议

如果将 `2026-05-29-phase-5-ffmpeg-trim-completion.md` 合并进 Phase 6，建议 Phase 6 先拆出以下前置子阶段，再做三种预设和授权：

1. 生产录制 artifact writer：产出真实原始素材路径，writer push/finish 错误必须 fatal。
2. export path helper：稳定生成独立输出路径，校验 output file exists + non-empty 后才返回 `outputPath`。
3. FFmpeg trim exporter：结构化消费 `CutTimeline`，不拼 CLI 字符串，不覆盖原始素材。
4. auto-trim off export：即使关闭自动裁剪，也能从原始素材导出完整 playable file。
5. auto-trim on export：消费 cut timeline 并产出 playable trimmed file。
6. trim sensitivity 完整语义：录制期保存 sensitivity-independent base RMS bucket，`build_cut_timeline()` 按当前 sensitivity 重聚合。
7. 手动 Gate：可播放性、音视频同步、原始素材保留、取消/失败清理、10 分钟 1080p 压力、Native Safety Gate。

### 16.10 Ready To Merge / 进入 Phase 6 判断

**结论：Phase 5 剩余代码可以按 code contract 完成处理；可以进入 Phase 6，但必须把 FFmpeg Gate 与 trim sensitivity 完整重聚合作为 Phase 6 前置导出流水线任务。**

本轮排除第 15 节 Important 2 和 Minor 3 后，未发现 Phase 5 其他功能的 Critical / Important 阻塞项。

仍需保留的未完成 Gate：

- 真实可播放 FFmpeg trimmed export。
- 录后 sensitivity 完整重聚合 RMS window。
- 10 分钟 1080p trim metadata / cut timeline 内存压力人工验证。
- 原始素材保留与重新导出路径人工确认。
- SCK/FFI Native Safety Gate 人工逐行审查。

## 17. 四轮补充复审（2026-05-29）：Phase 5 相关功能整改清单

> 本节记录 2026-05-29 针对 Phase 5 空白段检测与自动裁剪相关功能的补充 code review。审查范围覆盖 `179eaa76bc070e31169b8f973b9c3a97011352b7..a9bab95a958d957124e48b7de82aea1eb8e788e1`，并额外包含当前 working tree 中尚未提交的 Phase 5 整改 diff。

### 17.1 审查输入

本轮重点对照以下输入：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-5-w9-w10-checklist.md`
- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `docs/superpowers/plans/2026-05-29-phase-5-ffmpeg-trim-completion.md`
- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md` 到 `.codex/rules/5-docs.md`

本轮额外复核重点：

- React 不得接收 audio/video frame、frame diff sample、audio activity stream。
- ScreenCaptureKit callback / 捕获主链路不得被空白检测、JSON 写入或导出任务阻塞。
- 原始录制素材不得被删除或覆盖。
- FFmpeg 必须通过结构化 binding / C API 边界，不得拼 CLI 字符串，不得伪造 playable output。
- BUG.md 预防规则：不得新增 `data-tauri-drag-region="false"` wrapper、不得新增 `motion.div whileTap` 作为交互元素直接父容器、不得新增 `setIgnoreCursorEvents(true)` 类回归。
- AI 生成的视频帧处理、光标渲染、裁剪元数据采集路径需要重点看性能、线程安全、资源释放和完整性。

### 17.2 本轮结论

未发现 Critical 问题。

Phase 5 的核心骨架仍然成立：

- `CutTimeline`、`CutSegment`、`KeepSegment`、`AudioActivitySample`、`FrameDiffSample` 等模型已建立。
- 音频 RMS、低分辨率帧差分、候选段合并、前后 buffer、cut timeline 写入均在 Rust 侧。
- 录制期 trim metadata 采集发生在 consumer thread，不在 SCK callback。
- `build_cut_timeline()` 使用 `spawn_blocking`，不会在 Tauri 主事件循环同步跑 JSON 读写和 detector。
- Preview UI 只调用 Tauri command，并展示 lightweight summary / path。
- 当前没有发现媒体帧流或 activity stream 进入 React。
- 当前没有拼接 FFmpeg CLI 字符串，也没有伪造 playable `outputPath`。

但本轮仍发现 3 个 Important 和 1 个 Minor / Advisory，需要作为整改清单保留。尤其是录后 sensitivity 语义、export boundary 实际接入、writer error fatal 化，建议在 Phase 6 生产导出接入前优先处理。

### 17.3 自动化验证结果

本轮实际执行：

```bash
git diff --check
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
```

结果：

- `git diff --check`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，169 tests。
- `npm test -- --run`: PASS，46 tests。

输出备注：

- Rust 仍有 21 个既有 SCK/FFI warning，主要是 CoreMedia/CoreVideo FFI 命名、未使用 binding、SCK unsafe 提示等。
- Vitest 仍会输出 `Window.scrollTo()` 未实现提示；测试通过，属于测试环境噪声。
- 本轮请求了独立 reviewer subagent，但该 subagent 在等待窗口内未返回，已关闭；以下结论来自本地静态审查与自动化验证。

### 17.4 Strengths

1. **架构边界整体正确**
   - `src/lib/tauri.ts` 只暴露 `buildCutTimeline()` / `exportVideo()` 等 command wrapper。
   - `PreviewView` 只处理 `CutTimelineSummary` / `ExportSummary`，没有接收原始媒体帧或 activity stream。
   - `TrimMetadata` 写入发生在 Rust 侧 sidecar，不穿过前端。

2. **捕获 callback 未被 Phase 5 逻辑污染**
   - SCK callback 仍只做 timestamp 提取、CVPixelBuffer copy、AudioBufferList 转换、bounded channel 投递。
   - RMS、frame diff、metadata Vec push 都在 `MacRecordingService::consume_frames()`。
   - JSON sidecar 写入在 stop 后执行，不在 callback。

3. **保守裁剪策略基本可解释**
   - `SilenceDetectorEngine` 要求 audio silence 与 visual stillness 同时满足。
   - 小于阈值的短暂停顿不会进入 cut。
   - candidate merge、buffer、empty timeline 等路径已有单元测试。

4. **近期整改已经降低多项风险**
   - trim metadata duration 不再只依赖 `CountingRecordingWriter.duration_secs == 0`，而是 fallback 到 `latest_observed_media_nanos`。
   - visual diff 已做 250ms 低频采样，不再逐帧 diff。
   - activity Vec 有 cap，duration 独立于 cap。
   - frame diff 已支持 `stride_bytes`，并校验 stride 过小和乘法溢出。
   - consumer thread panic 不再降级为空结果成功返回。

### 17.5 Issues

#### Critical

未发现 Critical。

本轮未发现立即导致数据丢失、捕获 callback 被阻塞、React 接收媒体流、明显 use-after-free、默认 cut timeline 完全不可用、或 stop path false success 的问题。

#### Important 1: Trim sensitivity 只做了部分录后重建，RMS window 仍被录制时配置锁定

位置：

- `src-tauri/src/platform/macos_service.rs:187`-`189`
- `src-tauri/src/platform/macos_service.rs:381`-`388`
- `src-tauri/src/lib.rs:705`-`711`
- `docs/superpowers/plans/2026-05-29-phase-5-ffmpeg-trim-completion.md` 的 “Decision 2: Trim Sensitivity Semantics”

现象：

`MacRecordingService::start()` 在录制开始时从 `BeautifyConfigSnapshot` 取出 `trim_sensitivity`，随后 `consume_frames()` 用这个 sensitivity 构造 `AudioRmsAnalyzer`：

```rust
let sensitivity = TrimSensitivity::from_str(trim_sensitivity_str)
    .unwrap_or(TrimSensitivity::Medium);
let mut rms_analyzer = AudioRmsAnalyzer::new(TrimConfig::from_sensitivity(sensitivity));
```

这意味着 `audio_activity` sidecar 中保存的是“已经按录制时 sensitivity window 聚合后的 RMS sample”。后续 `build_cut_timeline()` 会读取当前 Preview 配置：

```rust
let sensitivity: TrimSensitivity = config.trim_sensitivity.parse()?;
let trim_config = TrimConfig::from_sensitivity(sensitivity);
```

但此时只能用当前 sensitivity 改变 threshold、min duration、buffer 等检测参数；无法重新生成 500ms / 750ms / 1000ms RMS window，因为原始 audio activity 已经被录制时窗口聚合过。

为什么重要：

- Preview UI 语义上像是“录后调参”，用户会预期 Low / Medium / High 可以完整改变检测策略。
- 现有实现会产生隐藏语义：如果录制时是 Medium，那么录后切 High 只能改变阈值等参数，不能真的使用 High 的 500ms RMS window。
- 这与 `2026-05-29-phase-5-ffmpeg-trim-completion.md` 中 “Changing sensitivity after recording must rebuild the cut timeline from the same original metadata” 的产品决定不一致。

风险：

- 用户修改灵敏度后 cut timeline 看似重建，实际 audio RMS 粒度没有完整重建。
- 对边界素材（短静默、短噪声、间歇性讲话）可能出现不可解释结果。
- 未来如果把 sensitivity 文案做强，会形成产品行为与实现不一致。

建议修复：

1. 录制期保存 sensitivity-independent base audio activity，例如 100ms RMS bucket：
   - `start`
   - `end`
   - `sum_squares`
   - `sample_count`
2. `TrimMetadata` 增加 version/schema 字段，区分旧 `audioActivity` 与新 base bucket。
3. `build_cut_timeline()` 按当前 `TrimConfig.rms_window_nanos` 将 base buckets 重新聚合为 500ms / 750ms / 1000ms window。
4. `SilenceDetectorEngine` 继续只消费派生后的 `AudioActivitySample`，保持纯逻辑边界。
5. 兼容旧 sidecar：旧 `audioActivity` 可 fallback 使用，但应标记为 legacy / partially rebuildable。

建议测试：

- base bucket 100ms 输入，High 聚合为 500ms，Medium 聚合为 750ms，Low 聚合为 1000ms。
- 同一组 base bucket 在不同 sensitivity 下生成不同 RMS window 数量。
- 修改 Preview sensitivity 后调用 `build_cut_timeline()`，不需要重新录制即可生成不同 cut timeline。
- legacy `audioActivity` sidecar 能读，但不会声称完整重建 RMS window。

#### Important 2: `export_video` 尚未真正消费结构化 `TrimExporter` 边界

位置：

- `src-tauri/src/media/trim_exporter.rs:31`-`47`
- `src-tauri/src/lib.rs:609`-`642`
- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md` Success criteria

现象：

`trim_exporter.rs` 已定义结构化导出边界：

- `TrimExportRequest`
- `TrimExportResult`
- `TrimExporter`
- `MockTrimExporter`
- feature-gated `FfmpegTrimExporter`

但当前 `export_video()` 的实际 command path 只是：

1. parse preset。
2. 调用 `build_cursor_effect_timeline()`。
3. 如果 `auto_trim_silences` 为 true，调用 `build_cut_timeline()`。
4. 返回 `ExportSummaryPayload { output_path: None }`。

没有创建 `TrimExportRequest`，没有读取或传递 `CutTimeline` 给 `TrimExporter`，也没有通过 mock / stub exporter 形成“export command consumes cut timeline through structured boundary”的闭环。

为什么重要：

- Phase 5 plan 的 success criteria 写的是：`export_video` consumes both cursor effect and cut timeline contracts and returns one combined export summary。
- 当前“构建 cut timeline”与“导出边界消费 cut timeline”还差一层。
- `trim_exporter.rs` 的安全边界目前只被 isolated unit test 覆盖，不能证明 Tauri command 层会使用这个结构化边界。

风险：

- 后续接 production FFmpeg 时，容易绕过 `TrimExporter` 直接在 `export_video()` 内拼流程。
- 当前代码容易让人误以为 export boundary 已接入，但实际上只接入了 timeline summary。
- checklist 中 “FFmpeg 封装能消费 `CutTimeline`” 保持 unchecked 是正确的，但 code contract 文案需要更精确。

建议修复：

短期（不完成 playable FFmpeg）：

1. 把 `export_video()` 拆到一个 `ExportService` / helper，显式表达 Phase 5 当前状态：
   - build cursor timeline
   - build/read cut timeline
   - assemble structured export intent
   - return `outputPath: None` with FFmpeg Gate reason
2. 或添加一个 `PreparedExportRequest` / `ExportPlan`，让 command path 明确生成包含 `CutTimeline` 路径或内容的结构化 plan。
3. 增加 Rust 测试：auto trim enabled 时，export helper 会读取 cut timeline 并形成 request/plan；auto trim disabled 时 request/plan 使用 empty/no-op timeline。

中期（Phase 6 前置）：

1. 有真实 `input_path` 后，`export_video()` 创建 `TrimExportRequest`。
2. feature-gated FFmpeg exporter 通过 binding/C API 消费 request。
3. 只有 output file exists 且 non-empty 时才返回 `outputPath: Some(...)`。
4. 禁止覆盖或删除原始素材。

建议测试：

- `export_video` auto-trim on：必须先有 cut timeline，再形成 export request/plan。
- `export_video` auto-trim off：仍形成 no-op cut timeline 或 full keep plan。
- unknown preset 仍返回错误。
- no source recording artifact 时，不返回 fake output path。

#### Important 3: writer push/finish 错误仍会被吞掉，生产 writer 接入前必须 fatal 化

位置：

- `src-tauri/src/platform/macos_service.rs:432`-`434`
- `src-tauri/src/platform/macos_service.rs:473`-`475`
- `src-tauri/src/platform/macos_service.rs:500`-`502`
- `src-tauri/src/platform/macos_service.rs:526`-`528`
- `src-tauri/src/platform/macos_service.rs:539`-`548`

现象：

`consume_frames()` 中：

- `writer.push_video(frame)` 失败只 `eprintln!`。
- `writer.push_audio(mixed)` 失败只 `eprintln!`。
- `writer.finish()` 失败用 `unwrap_or(RecordingResult { ... empty ... })` fallback。

当前默认 writer 是 `CountingRecordingWriter`，几乎不会失败，所以这个问题在 Phase 5 mock writer 阶段不一定触发。但一旦 Phase 6 接入生产 FFmpeg writer，push/finish error 就会变成真实风险。

为什么重要：

- writer 写入失败、编码失败、封装失败、磁盘不足、权限失败都应是录制/导出失败。
- 当前实现可能把 writer failure 降级为“录制完成 + 空 output + 可能仍有 trim metadata”，形成 false success。
- 这会污染后续 export：没有可靠原始素材时，后处理仍可能继续。

风险：

- 用户以为录制完成，实际原始素材不可用。
- trim/cursor sidecar 与真实媒体 artifact 不一致。
- 生产 FFmpeg writer 接入后，错误定位困难。

建议修复：

1. `RecordingConsumerOutput` 增加 `errors: Vec<String>` 或 `result: AppResult<RecordingResult>`。
2. `push_video()` / `push_audio()` 失败时记录错误；根据策略可继续 drain channel，但最终 stop 必须返回失败。
3. `finish()` 失败必须进入 `RecordingFinalizeFailed`。
4. `stop()` 仍需保持资源释放顺序：capture stop、mic stop、consumer join、runtime cleanup、mic reset 都不能因为 writer error 被跳过。
5. consumer panic 与 writer error 的错误聚合需要共存。

建议测试：

- fake writer `push_video` fails：`stop()` 最终返回 `RecordingFinalizeFailed`。
- fake writer `push_audio` fails：`stop()` 最终返回 `RecordingFinalizeFailed`。
- fake writer `finish` fails：`stop()` 最终返回 `RecordingFinalizeFailed`。
- writer failure 后仍 reset mic level、stop capture、join consumer。

#### Minor / Advisory 1: activity sample cap 超限后静默丢弃后续分析覆盖

位置：

- `src-tauri/src/platform/macos_service.rs:391`-`397`
- `src-tauri/src/platform/macos_service.rs:576`-`604`

现象：

Phase 5 为避免长录制 metadata 无界增长，新增了：

- `MAX_VISUAL_SAMPLES`
- `MAX_AUDIO_SAMPLES`
- `push_bounded_audio_sample()`
- `push_bounded_visual_sample()`

cap 达到后，新 sample 会直接 drop，duration 仍通过 `latest_observed_media_nanos` 保持准确。

为什么重要：

这满足 bounded metadata 的基本要求，但如果录制超过 cap 覆盖范围，cut timeline 可能只分析前半段或前 N 小时，后续内容不会被分析。当前 sidecar 没有记录 `truncated` / `dropped_count`，Preview summary 也无法提示“后半段未分析”。

建议修复：

1. `TrimMetadata` 增加：
   - `audioActivityDroppedCount`
   - `visualActivityDroppedCount`
   - `activityTruncated`
   - `activityCoverageEndNanos`
2. `build_cut_timeline()` 如果检测到 truncation，可在 summary 或 event 中返回中文提示。
3. 更优策略是超限后按时间桶压缩/降采样，而不是简单 drop。

建议测试：

- cap 达到后 dropped count 增加。
- metadata 标记 truncation。
- duration 仍使用 latest observed media nanos。
- build summary 能区分“未检测到空白段”和“后续片段未完全分析”。

### 17.6 捕获主链路专项结论

本轮未发现 Phase 5 逻辑直接阻塞 ScreenCaptureKit callback。

确认点：

- SCK callback 没有执行 RMS、frame diff、JSON serialization、cut timeline build。
- `try_send_drop_newest` 仍是 bounded channel 投递，不等待 consumer。
- frame diff 在 consumer thread 且按 `VISUAL_SAMPLE_INTERVAL_NANOS = 250_000_000` 低频采样。
- cut timeline build 在 `spawn_blocking` 中执行。

剩余关注：

- visual diff 当前仍发生在 `writer.push_video(frame)` 前。对 `CountingRecordingWriter` 风险很低；生产 writer 接入后建议重新评估顺序，优先保证写盘/编码路径。
- JSON sidecar 仍是 stop 后一次性 pretty serialization；长录制停止耗时需要人工 Gate。

### 17.7 React 数据流红线复核

未发现 React 接收媒体帧或 activity stream。

确认点：

- `src/lib/tauri.ts` 只定义：
  - `RecordingResult`
  - `CursorEffectSummary`
  - `CutTimelineSummary`
  - `ExportSummary`
- `PreviewView` 只调用：
  - `getBeautifyConfig()`
  - `setBeautifyConfig()`
  - `buildCursorEffectTimeline()`
  - `buildCutTimeline()`
  - `exportVideo()`
- `App.tsx` stop 后只保存 path / count / duration 等轻量字段。
- 未发现 `AudioActivitySample` / `FrameDiffSample` / `VideoFrame` / `AudioChunk` 进入 TS 类型层。

结论：

符合数据流红线：音视频帧流和 activity stream 不经过前端 JS 层。

### 17.8 BUG.md 预防规则扫描

扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结论：

- 未发现 `data-tauri-drag-region="false"` wrapper 回归。
- 未发现 `setIgnoreCursorEvents(true)` / `ignoreCursor` 回归。
- `src/components/recording-panel.tsx` 中存在 `motion.button whileTap`，但动画直接作用于按钮自身，不是 BUG-003 禁止的 `motion.div whileTap` 作为交互元素直接父容器。
- `src/App.test.tsx` 中命中的 `data-tauri-drag-region="false"` 是断言禁止模式不存在的测试代码，不是产品代码回归。

### 17.9 与第 16 节结论的关系

第 16 节是在“排除 Phase 6 合并项后的 Phase 5 code contract”口径下给出结论。本节采用更宽的“Phase 5 相关功能进入生产导出前整改清单”口径，因此重新把以下内容列为 Important：

- 录后 sensitivity 完整重聚合 RMS window。
- `export_video` 实际消费结构化 trim export boundary。
- 生产 writer 接入前 writer error fatal 化。

这不否定第 16 节对 Phase 5 已有骨架的判断；它的作用是给后续整改提供更明确的阻塞优先级。

### 17.10 建议整改顺序

推荐顺序：

1. **Writer error fatal 化**
   - 先把 recording artifact 的可靠性守住。
   - 没有可靠原始素材，后续 FFmpeg export 和 trim 都没有意义。

2. **Export boundary 接入 command path**
   - 即使暂不生成 playable output，也要让 command path 形成结构化 export plan/request。
   - 避免 Phase 6 接生产 FFmpeg 时绕开已有安全边界。

3. **Sensitivity-independent base RMS buckets**
   - 让 Preview sensitivity 成为真正的录后调参。
   - 这个改动涉及 metadata schema，建议在 FFmpeg 前置阶段尽早做。

4. **Activity truncation 可观测化**
   - 不一定阻塞短期 MVP，但应避免超长录制时 summary 误导用户。

5. **人工 Gate**
   - 10 分钟 1080p trim metadata / cut timeline memory pressure。
   - 原始素材保留与重新导出路径。
   - SCK/FFI Native Safety Gate。
   - FFmpeg trimmed export playable / A/V sync。

### 17.11 整改后复验清单

自动化复验：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo build --manifest-path src-tauri/Cargo.toml
npm run build
npm test -- --run
git diff --check
```

建议新增/补强测试：

- writer `push_video` / `push_audio` / `finish` failure 都会导致 `stop()` 返回失败。
- writer failure 后资源释放仍完整执行。
- export helper 在 auto-trim on 时读取/消费 cut timeline。
- export helper 在 auto-trim off 时生成 no-op/full keep export plan。
- base RMS bucket 可按不同 sensitivity 聚合为不同 window。
- 录后修改 sensitivity 不需要重新录制即可改变 cut timeline。
- activity sample cap 达到后记录 dropped/truncated 状态。

人工复验：

- 录制 8-12 秒静音且静止画面，开启 auto trim 后 `cutCount >= 1`。
- 录制短暂停顿素材，确认不会误剪。
- 录制带加载动画/鼠标移动素材，确认 visual change 能阻止误剪。
- 10 分钟 1080p 录制，检查 stop 耗时、sidecar 大小、内存峰值。
- FFmpeg Gate 完成后，检查裁剪后视频可播放且音视频同步无明显漂移。
- 确认原始素材保留，导出不会覆盖或删除原始录制 artifact。

### 17.12 Ready To Merge / 进入下一步判断

**结论：With fixes。**

当前 Phase 5 骨架和自动化测试状态良好，没有 Critical blocker；但若按“Phase 5 相关功能进入生产导出前可整改”口径，建议至少先处理 writer error fatal 化和 export boundary command path 接入，再继续扩大 FFmpeg 生产导出范围。

未处理前可以继续做计划拆解和小范围重构，但不建议把真实 FFmpeg writer/exporter 接入到当前 error-handling 语义上。
