# Phase 6 Code Review: Export Presets and Local License

> 日期：2026-05-30
> 评审范围：Phase 6 / W11-W12 导出预设与本地授权
> Git 范围：`cb4536a643c43fab0c1f8e8b2c3d8d7c75586bcb..a17382a078921cfffb212a0f304876b86008e5be`
> 用户指定起点：从 `bec70aae215512f56f441179c67fc0e40dff8901` 开始，按 inclusive 范围审查，因此 base 使用 `bec70aae^`
> 本地工作区：当前 `git status --short` 显示本 review 文件为未跟踪；除该文件外未发现其他未提交代码
> 结论：No，不建议合并或宣称 Phase 6 完整完成；本地授权边界基本成立，但导出主路径仍未完成且存在产品路径回归

## 1. 评审目标

本次评审重点回答两个问题：

1. Phase 6 是否完整完成了开发任务。
2. Phase 6 相关代码是否存在阻塞捕获主链路、内存安全、线程安全和资源释放路径异常等问题。

额外检查：

- 是否符合 `docs/architecture/project-architecture-and-overall-planning.md` 中 Phase 6 的目标：16:9 / 9:16 / 1:1 三种本地导出预设、导出进度与取消、本地 14 天试用和激活状态边界。
- 是否符合 `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md` 中的 Phase 6A-6E 成功标准。
- 是否正确承接 Phase 5 review sections 16-17 转入 Phase 6 的前置项：真实 playable FFmpeg export、原始录制 artifact、`TrimExporter` command path 接入、writer error fatal 化、录后 trim sensitivity 完整重聚合。
- 是否符合 `tests/phase-6-w11-w12-checklist.md` 的自动化项与人工 Gate。
- 是否遵守 `BUG.md` 预防规则。
- 是否遵守数据流红线：音视频帧流、frame diff stream、audio activity stream 不进入前端 JS 层。
- FFmpeg 集成是否通过 Rust binding / C API 边界，不拼接 CLI 命令字符串。
- 授权实现是否避免 hardcoded activation secret、DSN、analytics key 或生产 credential 文件伪激活。

## 2. 审查输入

本次审查读取并对照：

- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `tests/phase-6-w11-w12-checklist.md`
- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `docs/superpowers/reviews/2026-05-29-phase-5-code-review.md` sections 16-17

同时请求了独立 code reviewer subagent 交叉审查。以下结论综合本地静态审查、自动化验证和子审查结果。

## 3. 总体结论

Phase 6 目前不能认定为完整完成，也不建议合并到主线作为完成态。

已经完成且方向正确的部分：

- `ExportPreset` 独立成固定三种 MVP 预设，没有引入模板系统或平台发布 API。
- `export_paths` 能生成独立导出路径，并验证返回 `outputPath` 前文件非空。
- `TrimMetadata` 引入 schema version、base audio activity、truncation fields，方向上支持录后灵敏度重聚合。
- `ExportService` helper 建立了结构化 `TrimExportRequest` / `TrimExporter` 边界。
- `AppError` 新增导出和授权错误，并保持中文错误消息。
- `ExportProgressPayload`、`cancel_export` command、Preview 进度条和取消按钮已有 UI 骨架。
- `LicenseService` 将本地 trial marker 与 activation credential boundary 分离，没有把激活 entitlement 写进普通文件。
- React 侧仍只接收轻量 summary / progress / path / status，没有接收媒体帧或 activity stream。

主要阻塞点：

1. 默认录制仍使用 `CountingRecordingWriter::new(None)`，不会生成原始录制 artifact；但 `export_video()` 现在要求 `last_recording_output_path()`，导致正常录制后导出直接失败。
2. `export_video()` 创建了 cancel token 和 0/100 progress，但没有调用 `ExportService`，没有调用 `TrimExporter`，也没有把 token/progress 传入真实导出边界。
3. FFmpeg writer 和 FFmpeg trim exporter 仍是骨架或显式 `Err(...)`，不存在 playable original artifact 或 playable preset export。
4. Base RMS bucket 实现并非严格固定 100ms bucket，Medium 750ms 聚合语义不可靠。
5. writer `push_audio()` 在 live loop 中仍可能被吞错，不满足生产 writer 前置 fatal 化要求。
6. cancel/failure partial output cleanup 未在 `ExportService` 中兜底。
7. 文档和 checklist 有明显 overclaim：把 helper/UI/boundary 写成 completed，但真实 artifact、command wiring、playable export、cancel cleanup 和 FFmpeg feature artifact tests 都未完成。

本轮判断：

- Local license boundary：基本可接受，仍需后续系统 credential store。
- Export preset model / path helper：基本可接受。
- Export product path：不完整，并且正常录制后导出路径发生回归。
- Capture main path：未发现 ScreenCaptureKit callback 被新增导出逻辑阻塞；但生产 writer 尚未接入，consumer path 的部分错误 fatal 化仍不完整。
- Native Safety：没有发现新增 unsafe FFmpeg 实现，因为实际实现尚未存在；SCK/FFI 和未来 FFmpeg 仍需人工 Native Safety Gate。

## 4. 自动化验证结果

本次审查期间实际执行：

```bash
git diff --check cb4536a643c43fab0c1f8e8b2c3d8d7c75586bcb
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
```

结果：

- `git diff --check cb4536a643c43fab0c1f8e8b2c3d8d7c75586bcb`: PASS，无输出。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，198 tests。
- `npm test -- --run`: PASS，51 tests。
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`: BLOCKED / FAIL，环境缺少 `pkg-config`。

FFmpeg feature 失败细节：

```text
failed to run custom build command for `ffmpeg-sys-next v7.1.3`
Could not run `pkg-config --libs --cflags libavutil`
The pkg-config command could not be found.
```

注意：

- 第一次在 sandbox 内运行 FFmpeg feature test 时，Cargo registry cache 读取被 sandbox 拒绝；按权限流程使用已获批的 `cargo test` prefix 重新运行后，实际阻塞变为缺少 `pkg-config`。
- 该结果只能证明当前环境无法完成 FFmpeg feature verification，不能证明 FFmpeg feature 代码正确。
- 普通 Rust 测试输出中出现与 Phase 6 相关的新 warning，包括 `cut_timeline`、`effect_timeline_path` 未使用；这与 `export_video()` 构建但不消费结构化导出请求一致。

## 5. Strengths

1. Export preset 范围克制
   - `src-tauri/src/media/export_presets.rs` 只提供 Bilibili / Douyin / Xiaohongshu 三种固定 preset。
   - Unknown preset 会被拒绝，没有滑向模板系统或平台发布 API。

2. 数据流红线保持住了
   - `src/lib/tauri.ts` 只新增 `ExportProgressPayload`、`LicenseStatus` 等轻量类型。
   - `PreviewView` 只展示 summary/progress/path/error，没有接收音视频帧、frame diff sample、audio activity sample 或 raw cursor stream。
   - Rust 侧 trim metadata 仍作为 sidecar 读写，不穿过前端 JS。

3. FFmpeg CLI 禁令未被违反
   - 未发现 `std::process::Command` 调用 FFmpeg。
   - 未发现 `ffmpeg` / `ffprobe` CLI 字符串拼接。
   - `FfmpegTrimExporter` 的接口仍是结构化 request。

4. Local license 设计边界清晰
   - 本地 file store 只保存 `trialStartedAtSecs`。
   - `LocalTrialState` 使用 `deny_unknown_fields`，测试覆盖了不能写入 `activatedAtSecs`。
   - product 默认使用 `NoopActivationCredentialStore`，没有伪造激活状态。
   - `activate_license()` 对非空 code 返回“服务端激活协议未接入”，未硬编码私钥或激活码。

5. Phase 5 的几个前置整改方向已有局部实现
   - writer push/finish error 开始进入 consumer output errors。
   - activity truncation fields 已加入 metadata。
   - base audio activity 已从 windowed RMS 改为独立结构。

6. BUG.md 预防规则未发现回归
   - 未发现产品代码新增 `data-tauri-drag-region="false"` wrapper。
   - 未发现 `setIgnoreCursorEvents(true)` / `ignoreCursor` 回归。
   - `src/components/recording-panel.tsx` 中的 `motion.button whileTap` 直接作用于按钮本身，不是 BUG-003 禁止的 `motion.div whileTap` 作为交互元素直接父容器。

## 6. Issues

### Critical 1: 正常录制后导出直接失败，Phase 6 export product path 回归

位置：

- `src-tauri/src/platform/macos_service.rs:221`
- `src-tauri/src/platform/macos_service.rs:327`
- `src-tauri/src/lib.rs:735`

现象：

- `MacRecordingService::start()` 仍然创建 `CountingRecordingWriter::new(None)`。
- `CountingRecordingWriter::finish()` 因 `output_path: None` 返回 `RecordingResult.output_path = None`。
- `stop()` 把这个 `None` 写入 `self.last_recording_output_path`。
- `export_video()` 现在强制读取 `service.last_recording_output_path()`，没有则返回：
  - `没有可用的原始录制文件，请先完成一次可播放录制`

为什么重要：

这是产品路径级别的回归。Phase 5 阶段 `export_video()` 虽然仍是 FFmpeg Gate，但至少能构建 cursor/cut summary 并返回 `outputPath: None`。Phase 6 改动后，默认非-FFmpeg 构建、正常录制后点击导出会因为没有 source artifact 直接失败，无法到达 gated summary。

违反的计划/验收：

- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md` Phase 6A 要求 `export_video()` forms a structured export request/plan。
- Task 6A 明确写着：没有可靠 original recording artifact，Phase 6 不能 claim export preset complete。
- `tests/phase-6-w11-w12-checklist.md` 中 “Original Recording Artifact” 被写为 completed，但产品默认路径并没有 artifact。

风险类型：

- 功能完整性风险：用户无法从正常录制进入导出流程。
- 交互回归风险：Preview UI 展示导出按钮，但 command path 必然失败。
- 后续整改风险：如果只看 helper tests，容易误判 export service/source validation 已接入产品路径。

建议修复：

1. 二选一明确产品边界：
   - 若 Phase 6 必须完成 playable export：先实现 production recording writer，确保 `stop_recording` 返回真实、非空、可播放 original artifact，再让 `export_video()` 强制 source path。
   - 若当前仍是 boundary-only/gated phase：不要在 product command 中强制要求不存在的 source artifact；应返回明确 FFmpeg Gate summary，并把 docs/checklist 改为未完成。
2. 不允许把 `CountingRecordingWriter` 伪造成 source artifact。
3. `last_recording_output_path` 只有在 source file exists + non-empty + inspected playable 后才能设置为可导出 source。
4. 增加 command-level 测试：
   - 默认 writer 无 output path 时，`export_video()` 行为应符合明确 gate 策略，而不是 UI 看似可导出实际必失败。
   - FFmpeg writer enabled 且 source artifact valid 时，`export_video()` 才进入真实 exporter。

建议验收：

- 非-FFmpeg 构建：导出按钮返回稳定、明确的 FFmpeg Gate 错误或 summary，不出现“请先完成一次可播放录制”的死路状态。
- FFmpeg 构建：`stop_recording` 返回非空 `outputPath`，文件存在、非空、可播放且含视频/音频 stream。
- `tests/phase-6-w11-w12-checklist.md` 填入 original recording artifact 证据。

### Critical 2: `export_video()` 未接入 `ExportService` / `TrimExporter`，取消和真实进度都是 UI-only

位置：

- `src-tauri/src/lib.rs:670`
- `src-tauri/src/lib.rs:692`
- `src-tauri/src/lib.rs:706`
- `src-tauri/src/lib.rs:730`
- `src-tauri/src/lib.rs:742`
- `src-tauri/src/lib.rs:751`

现象：

- `export_video()` 创建了 `cancel_token` 并保存到 `state.export_cancel_token`。
- `cancel_export()` 会把 token 设为 true。
- 但 `export_video()` 从未检查该 token。
- `export_video()` 从未创建 `ExportProgressReporter`。
- `export_video()` 从未调用 `export_recording_with_timeline()`。
- `export_video()` 从未调用 `FfmpegTrimExporter` 或 `MockTrimExporter`。
- `cut_timeline` 和 `effect_timeline_path` 被构建/读取后没有被消费，普通测试也出现 unused variable warning。
- 最终 progress 直接从 0 跳到 100。
- cancel token 在 build cursor/cut timeline、source lookup 等多个 `?` 返回之前已经写入 state；任一早退都会跳过末尾清理块，也不会发出 terminal error progress event。

为什么重要：

Phase 6C 的核心要求不是“UI 上有进度条和取消按钮”，而是：

- progress must be emitted by Rust exporter callbacks and include intermediate values。
- export can be cancelled。
- cancel/failure paths clean partial output files。

当前实现只满足 UI 观感，不满足后端语义。用户点击取消最多影响一个无人读取的 atomic bool，不能停止 cursor timeline build、cut timeline build，也不能停止 exporter，因为 exporter 根本没被调用。

风险类型：

- 功能完整性风险：取消按钮无实际效果。
- 误导性进度风险：0/100 不是 exporter callback progress。
- 状态一致性风险：早期错误可能让 `export_cancel_token` 残留为 active，UI/后端状态都可能被后续取消或导出请求污染。
- 线程/资源风险：未来接入真实 exporter 时，如果继续绕开 `ExportService`，partial output cleanup、source preservation、cancel token 都容易散落在 command 内。

建议修复：

1. 将 `export_video()` 拆成明确阶段：
   - setup cancel token
   - emit 0%
   - build cursor timeline
   - check cancel
   - build or load cut timeline
   - check cancel
   - build `TrimExportRequest`
   - spawn_blocking 调用 `export_recording_with_timeline()`
   - exporter progress callback emit intermediate 1-99
   - success emit 100 with outputPath
   - error/cancel emit cancellable false + error
   - finally/scope guard clear token only if token belongs to this export，覆盖 missing source、timeline build failure、exporter error/cancel 等所有早退路径
2. `cancel_export()` 应对没有 active export 的情况保持 idempotent，但 active export 必须实际响应。
3. 并发 export 需要防护：
   - 当前 `export_cancel_token` 只有一个 slot，第二次 export 会覆盖第一次 token。
   - UI 禁用了按钮，但后端 command 仍应拒绝并发 export 或按 sequence/token ownership 清理。
4. 增加 command/helper tests：
   - exporter callback report 8/47/91 时，event 中能观察到中间进度。
   - cancel token 在 exporter 前已 true 时返回 `ExportCancelled`。
   - cancel 后 token 清理，partial output 删除。
   - missing source / cursor timeline failure / cut timeline failure 均清理 active token，并发出包含 error 的 terminal `export-progress` event。

建议验收：

- Preview 点击导出后至少收到一个 1-99 的 `export-progress` event，来源于 exporter callback。
- 点击取消后 command 返回取消错误或取消 summary，并且 `export-progress` 包含 error。
- 真实导出 partial output 被删除。

### Important 1: Base RMS bucket 不是严格固定 100ms bucket，Medium 750ms 重聚合不可靠

位置：

- `src-tauri/src/media/trim_audio_activity.rs:49`
- `src-tauri/src/media/trim_audio_activity.rs:77`
- `src-tauri/src/media/trim_audio_activity.rs:101`
- `src-tauri/src/media/trim_audio_activity.rs:106`

现象：

- `BaseAudioActivityAnalyzer` 用 `bucket_start_nanos.get_or_insert(frame_nanos)` 让第一帧 timestamp 成为 bucket 起点。
- 判断条件是 `frame_nanos > bucket_start + BASE_RMS_BUCKET_NANOS`，因此 exact-boundary sample 会落入前一个 bucket。
- `take_bucket()` 固定把 end 写成 `start + 100ms`，即使 sample 实际覆盖超过 100ms。
- 测试为了适配这一行为，把 `base_analyzer_emits_100ms_buckets` 改成了 20 个 10Hz sample 只输出 9 个 bucket，每个 bucket 2 个 sample。这不是“固定 100ms bucket”的语义。
- `aggregate_base_audio_activity()` 按 bucket 整体归入 RMS window，不拆 bucket；Medium `750ms` 不是 `100ms` 的整数倍，会产生窗口标签和实际样本覆盖不一致。

为什么重要：

Phase 6 承接 Phase 5 的关键产品语义是：录后切换 Low/Medium/High 时，RMS window 也能完整重聚合。当前实现能改变 window label，但 sample coverage 不严格，尤其 Medium 750ms 场景会静默偏差。

风险类型：

- 功能完整性风险：Preview sensitivity 看似完整重算，实际 RMS window 不精确。
- 用户可解释性风险：边界静音/短噪声素材在不同 sensitivity 下结果可能不可预期。
- 测试误导风险：测试名称仍说 100ms buckets，但断言接受了跨 boundary 样本。

建议修复：

1. 将 base bucket 定义为严格 `[bucket_start, bucket_start + BASE_RMS_BUCKET_NANOS)`。
2. boundary 判断使用 `>=`，exact-boundary sample 进入下一个 bucket。
3. 如果输入 chunk timestamp 不连续，应决定是 gap reset 还是补空 bucket，并记录测试。
4. Medium 750ms 支持有两种选择：
   - 将 base bucket 改为 50ms，因为 500/750/1000 都可整除。
   - 保持 100ms，但 aggregation 必须按时间比例拆分 bucket 的 `sum_squares` / `sample_count`。
5. 更新测试名称和断言，避免“bucket label 100ms、实际 sample coverage 200ms”的情况。

建议验收：

- 1kHz 下 100 个 sample exactly 0-99ms，不 emit；第 100ms sample 进入下个 bucket。
- 10 个 100ms base bucket 聚合 High 得到两个 500ms window。
- 15 个 50ms base bucket 或 split 后聚合 Medium 得到严格 750ms window。
- 同一 base metadata 切 High/Medium/Low 的 `AudioActivitySample` window duration 分别精确符合配置。

### Important 2: Live loop 中 `writer.push_audio()` 错误仍会被吞掉

位置：

- `src-tauri/src/platform/macos_service.rs:499`

现象：

在 consumer live loop 中：

```rust
if let Err(e) = writer.push_audio(mixed) {
    eprintln!("写入混音音频失败: {e}");
}
```

但 final drain 中的同类错误会进入 `errors.push(msg)`。

为什么重要：

Phase 5 review section 17 已明确要求：生产 writer 接入前，writer push/finish error 必须 fatal 化。Phase 6 部分修复了 video push、final audio push、finish，但漏掉 live loop audio push。真实 encoder/muxer 在录制期间写音频失败时，仍可能最终返回 success。

风险类型：

- 数据完整性风险：视频有、音频缺失或音频写入失败时仍可能显示录制完成。
- 导出污染风险：后续 export 使用的 original artifact 可能不完整。
- 错误定位风险：用户只看到导出失败，根因是录制期音频写入早已失败但被吞掉。

建议修复：

1. live loop `push_audio` 错误与 final drain 保持一致：
   - 构造 `msg = format!("写入混音音频失败: {e}")`
   - `eprintln!("{msg}")`
   - `errors.push(msg)`
2. 新增测试覆盖 `FailingRecordingWriter::new(false, true, false)`，并确保 `consume_frames()` 返回 errors。
3. stop path 已经 `errors.extend(consumer_output.errors)`，修复后会自然进入 `RecordingFinalizeFailed`。

建议验收：

- fake writer live-loop `push_audio` failure 导致 `stop()` 或 consumer output error。
- writer failure 后 capture stop、mic stop、consumer join、mic level reset 仍执行。

### Important 3: `ExportService` 未清理 cancel/failure/validation 失败的 partial output

位置：

- `src-tauri/src/app/export_service.rs:39`
- `src-tauri/src/app/export_service.rs:47`
- `src-tauri/src/app/export_service.rs:57`

现象：

`export_recording_with_timeline()` 会算出 `output_path`，然后调用 exporter，最后 `validate_non_empty_output()`。但以下情况都不会清理 partial output：

- exporter 创建了 partial output 后返回 `Err(AppError::ExportCancelled)`。
- exporter 创建了 partial output 后返回其他 error。
- exporter 返回 `Ok`，但 `validate_non_empty_output()` 失败。

为什么重要：

Phase 6C 明确要求 cancel and failure paths clean partial output files。partial MP4 如果残留，后续可能被误认为有效导出，也会浪费磁盘空间。等 FFmpeg 真接入后，取消和失败最容易留下损坏容器或半写入文件。

建议修复：

1. 在 `export_recording_with_timeline()` 中统一兜底：
   - 先 clone planned output path。
   - `match exporter.export(request)`。
   - `Err(error)` 时 best-effort `remove_file(planned_output_path)` 后返回 error。
   - `Ok(result)` 后 validation 失败也删除 `result.output_path`。
2. exporter 内部仍应清理自身 partial output；service 兜底是第二道防线。
3. 增加测试：
   - exporter writes file then returns error，service removes file。
   - exporter writes empty file and returns Ok，validation fails and file removed。
   - cancel error removes partial output。

建议验收：

- `ExportCancelled` 后 output path 不存在。
- `ExportFailed` 后 output path 不存在。
- original input path 仍存在。

### Important 4: Phase 6 docs/checklist 明显 overclaim，容易误导后续整改优先级

位置：

- `HANDOFF.md:3`
- `HANDOFF.md:91`
- `HANDOFF.md:116`
- `tests/phase-6-w11-w12-checklist.md:79`
- `tests/phase-6-w11-w12-checklist.md:90`

现象：

当前文档写法包括：

- `Phase 6 导出预设与本地授权实现完成`
- `本轮完成（8 Tasks）`
- `Export Progress & Cancel`
- `Original Recording Artifact`
- `FFmpeg Export Gate`
- `剩余待完成（不阻塞 Phase 6 contract，但需关注）`

但实际代码：

- 没有默认 original recording artifact。
- `export_video()` 没有接入 `ExportService` / `TrimExporter`。
- cancel token 没有被消费。
- progress 不是 exporter callback。
- FFmpeg writer 不写文件。
- FFmpeg exporter 永远返回 “实现需补齐”。
- `src-tauri/tests/ffmpeg_export.rs` 不存在。
- Manual FFmpeg Gates 全部未完成。

为什么重要：

这和用户本轮额外重点“仔细审核 Phase 6 是否完整完成开发任务”直接相关。当前最大风险不是只差几个小 bug，而是文档把 preflight/helper/UI 骨架写成 Phase 6 完成态，后续整改可能被错误降级为“非阻塞关注项”。

建议修复：

1. 将 Phase 6 状态改为：
   - “Phase 6A 部分 preflight 完成”
   - “Local trial / activation status boundary 完成”
   - “Playable export、original artifact、command wiring、exporter progress/cancel cleanup 未完成”
2. `HANDOFF.md` 的“剩余待完成（不阻塞 Phase 6 contract）”应改为“阻塞 Phase 6 export 完成”。
3. `tests/phase-6-w11-w12-checklist.md` 的 Implementation Notes 应拆分：
   - Completed
   - Boundary only
   - Blocked / Must fix
4. 不要把 manual FFmpeg gates 放到“future phases”，因为它们属于 Phase 6 plan 自身的 Phase 6B / 6C 成功标准。

建议验收：

- 文档中不再出现“Phase 6 完成”描述，除非真实 export gates 通过。
- 每个 blocked gate 都能对应到具体修复任务和验证命令。

### Important 5: 缺少计划要求的 FFmpeg artifact-level integration tests

位置：

- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md:108`
- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md:2289`
- 当前仓库无 `src-tauri/tests/ffmpeg_export.rs`

现象：

Phase 6 plan 明确要求创建 `src-tauri/tests/ffmpeg_export.rs`，覆盖：

- missing source rejection
- all fixed presets export
- cut timeline applies and preserves original
- cancel removes partial output and preserves original
- artifact inspection: dimensions, duration, streams, file size
- intermediate progress callback

当前没有该 integration test 文件。`ffmpeg_test_support.rs` 存在，但没有 artifact-level test 使用它验证真实 exporter。

额外注意：`create_synthetic_source_artifact()` 当前调用 `FfmpegRecordingWriter` 后直接 `writer.finish()?; Ok(())`，但 `FfmpegRecordingWriter::finish()` 明确返回 `output_path: None` 且不创建文件。因此该 helper 在 FFmpeg feature 可编译时也可能“成功返回但没有真实 source artifact”，不满足 plan 对 synthetic media artifact 的要求。

为什么重要：

在 FFmpeg dev libraries 当前不可用的环境中，保留 feature-gated integration tests 更重要。没有测试文件，后续环境具备 `pkg-config` / FFmpeg 后也无法自动证明 export gate 已关闭。若 synthetic helper 本身不校验真实产物，后续测试还可能误把“writer skeleton 正常返回”当成可用源文件。

建议修复：

1. 新增 `src-tauri/tests/ffmpeg_export.rs`，按 plan 中的场景写 feature-gated tests。
2. 若当前 FFmpeg implementation 尚未完成，测试可以先存在并失败于 exporter skeleton；这能准确表达 Task 6 未完成。
3. checklist 记录当前失败/阻塞原因：缺少 `pkg-config`，以及 exporter implementation 未完成。
4. `create_synthetic_source_artifact()` 必须在返回 `Ok(())` 前验证文件存在、非空、可被 binding inspection 打开且含视频/音频 stream；在 writer 仍是 skeleton 时，应返回明确 gated error。

建议验收：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_rejects_missing_source`
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source`
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original`
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original`

### Important 6: `FfmpegRecordingWriter` / `FfmpegTrimExporter` 仍是骨架，但提交和文档命名容易误导

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:31`
- `src-tauri/src/media/ffmpeg_writer.rs:44`
- `src-tauri/src/media/trim_exporter.rs:120`
- `src-tauri/src/media/trim_exporter.rs:139`
- commit `7eb7815 feat(record): 产出可导出的原始录制文件`
- commit `cad4215 feat(export): 接入可播放 FFmpeg 导出`

现象：

- `FfmpegRecordingWriter::push_video()` 只递增计数。
- `FfmpegRecordingWriter::push_audio()` 只递增计数。
- `FfmpegRecordingWriter::finish()` 不创建文件，并显式 `output_path: None`。
- `FfmpegTrimExporter::export()` 做少量 validation 后直接返回 `ExportFailed`，提示 FFmpeg 导出实现待补齐。

为什么重要：

代码注释本身较诚实，但 commit 和 handoff 语义容易让人误以为“可导出原始录制文件”和“可播放 FFmpeg 导出”已经接入。后续如果只看 commit log/checklist，很容易误判阶段状态。

建议修复：

1. 若保留骨架：rename commit/document wording 或在 review/remediation 中明确“boundary only”。
2. 若要完成 Phase 6：按 Task 6A/6 继续实现真实 writer/exporter，并通过 Native Safety review。
3. 不要把 skeleton writer 切进 default product path。

建议验收：

- `FfmpegRecordingWriter::finish()` 返回 `Some(output_path)` 前，文件必须存在、非空、含视频/音频 stream、duration > 0。
- `FfmpegTrimExporter::export()` 返回 `Ok` 前，输出文件必须通过 binding inspection。

## 7. Minor / Advisory

### Minor 1: `MAX_AUDIO_SAMPLES` 注释和真实覆盖时长不一致

位置：

- `src-tauri/src/platform/macos_service.rs:410`

现象：

`MAX_AUDIO_SAMPLES = 72_000` 注释为 `~10h @ 2/sec (750ms window)`，但当前存的是 100ms base buckets，理论采样率约 10/sec，因此 72,000 个样本约覆盖 2 小时，不是 10 小时。

为什么重要：

这不是立即功能 bug，但会误导长录制压力评估和 cap 策略。

建议修复：

- 如果目标是 10 小时 base bucket，cap 应约为 360,000。
- 如果目标是更保守内存 cap，注释应改为 “~2h @ 10/sec base 100ms buckets”。
- checklist 中 10-minute pressure gate 不受该 cap 影响，但长会议场景会受影响。

### Minor 2: `PreviewView` progress bar 使用 inline style

位置：

- `src/components/preview-view.tsx:527`

现象：

进度条宽度使用：

```tsx
style={{ width: `${Math.max(0, Math.min(exportProgress.progress, 100))}%` }}
```

项目 React 规则禁止静态 inline style。这里属于动态宽度，计划文档里也曾允许该行作为例外，因此不作为阻塞项。

建议：

- 若要完全遵守风格规则，可改为 CSS variable 或通过 shadcn progress component 封装。
- 不建议在修 Critical/Important 前优先处理。

### Minor 3: 新增 warnings 应在整改后清理

位置：

- `src-tauri/src/lib.rs:707`
- `src-tauri/src/lib.rs:730`
- `src-tauri/src/platform/macos_service.rs:396`
- `src-tauri/src/media/ffmpeg_test_support.rs:1`
- `src-tauri/src/app/license_service.rs:135`

现象：

普通 `cargo test` 通过，但有 Phase 6 相关 unused/private-interface warning：

- `cut_timeline` unused
- `effect_timeline_path` unused
- `trim_sensitivity_str` unused
- `ffmpeg_test_support` 非 feature build 下 unused imports
- `LicenseService::status(mut self)` unnecessary `mut`

为什么重要：

单独看不是阻塞，但 `cut_timeline` / `effect_timeline_path` unused 正好暴露了导出边界未接入产品路径。整改 Critical 2 后这两项应自然消失。

建议：

- 不要只通过 `_cut_timeline` 静默压制；应真正消费到 `TrimExportRequest`。
- `trim_sensitivity_str` 已无用途，应移除参数或改成明确兼容注释。

## 8. 捕获主链路专项复核

本轮未发现新增代码直接阻塞 ScreenCaptureKit callback。

确认点：

- SCK callback 未新增 RMS、JSON 写入、FFmpeg 转码或 export 逻辑。
- Base RMS bucket、frame diff、audio mix 仍在 `MacRecordingService::consume_frames()` consumer thread。
- `build_cursor_effect_timeline()` / `build_cut_timeline()` 使用 `spawn_blocking`。
- `export_video()` 当前没有真实 FFmpeg work，因此还没有把转码塞入 Tauri 主事件循环的问题。

剩余风险：

1. 生产 writer 尚未接入，因此还不能证明 writer push/finish 不会拖慢 consumer。
2. 当前 visual diff 仍发生在 `writer.push_video(frame)` 前；虽然低频采样已降低风险，但真实 writer 接入后建议重新评估“写盘/编码优先于 metadata 分析”的顺序。
3. live-loop audio writer error 仍未 fatal，见 Important 2。
4. 长录制 sidecar 仍使用 `serde_json::to_string_pretty()` 一次性构造 JSON，10 分钟压力 Gate 未完成。

## 9. 内存安全、线程安全与资源释放专项复核

### 内存安全

- 本轮新增核心 export/license/preflight 代码主要是 safe Rust。
- 未发现新增 unsafe FFmpeg resource lifetime，因为 FFmpeg writer/exporter 尚未真实实现。
- `ffmpeg_test_support` 在 feature 下使用 `ffmpeg_next::format::input()` 做 inspection，方向符合 binding/C API 要求。
- SCK/FFI 原有 Native Safety Gate 仍需人工逐行审查。

结论：

- 当前没有发现新增 use-after-free 或裸指针生命周期问题。
- 不能给 FFmpeg Native Safety 背书，因为生产实现不存在。

### 线程安全

- `export_cancel_token` 使用 `Arc<Mutex<Option<Arc<AtomicBool>>>>`，基本线程安全。
- 但没有 export ownership / sequence guard，多个后端 `export_video` command 并发时可能互相覆盖 token。
- Preview UI disabled 不能替代后端并发防护。
- `ExportProgressReporter` callback 是 `Arc<dyn Fn(u8) + Send + Sync>`，接口方向可接受。

建议：

- `AppState` 增加 `export_in_flight` 或用 sequence token，后端拒绝并发 export。
- clear cancel token 时确认仍是当前 export 的 token，避免清掉后续 export 的 token。

### 资源释放

已改善：

- consumer panic 会进入 `RecordingFinalizeFailed`。
- writer video push/final audio push/finish errors 部分进入 `errors`。

仍需修：

- live-loop audio push error 未进入 `errors`。
- export partial output cleanup 缺失。
- cancel token 没有实际消费。
- FFmpeg writer/exporter 真实资源释放路径尚未实现，需 Native Safety review。

## 10. 数据流与安全规则复核

### React 数据流红线

未发现违规。

确认点：

- TypeScript 类型只包括 `RecordingResult`、`CursorEffectSummary`、`CutTimelineSummary`、`ExportSummary`、`ExportProgressPayload`、`LicenseStatus` 等轻量结构。
- 没有 `VideoFrame`、`AudioChunk`、`AudioActivitySample`、`FrameDiffSample` 类型进入 `src/lib/tauri.ts`。
- Preview UI 没有接收 activity stream。

### FFmpeg CLI 禁令

未发现违规。

扫描命中：

- `trim_exporter.rs` 注释说明 no CLI。
- 未发现 `std::process::Command` 或 shell out 到 ffmpeg/ffprobe。

### 授权与敏感信息

未发现 hardcoded private key、activation secret、Sentry DSN、PostHog key。

注意：

- 当前 production activation store 是 `NoopActivationCredentialStore`，不会返回 activated。
- 后续实现激活持久化必须使用 macOS Keychain / Windows Credential Manager / 等效 OS credential store，并走 Native Safety review。

### BUG.md 预防规则

扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

结论：

- 未发现产品代码新增 `data-tauri-drag-region="false"` wrapper。
- 未发现 `setIgnoreCursorEvents(true)` / `ignoreCursor`。
- `src/components/recording-panel.tsx` 中的 `motion.button whileTap` 可接受，不是 BUG-003 的 forbidden wrapper。
- `src/App.test.tsx` 中命中 `data-tauri-drag-region="false"` 是测试断言，不是产品代码。

## 11. Phase 6 完成度复核

| 项目 | 当前状态 | 本轮结论 |
| --- | --- | --- |
| 固定三种 export presets | 已实现并测试固定尺寸/unknown reject | 通过 |
| export path helper | 已实现独立路径和 non-empty validation | 通过 |
| source artifact validator | helper 已实现 | helper 通过，产品路径未接入 |
| production original recording artifact | 默认仍 `CountingRecordingWriter::new(None)` | 未完成，Critical |
| writer error fatal 化 | video/final audio/finish 部分处理 | live audio 漏掉，Important |
| sensitivity-independent base RMS | 已引入 base sample | bucket/window 语义需修，Important |
| `export_video()` 形成 structured request | 读取了 source/cut，但未调用 `ExportService` / `TrimExporter` | 未完成，Critical |
| playable FFmpeg export | `FfmpegTrimExporter` 返回 explicit Err | 未完成 |
| export progress from exporter callback | UI + event payload 有，product path 仅 0/100 | 未完成，Critical |
| export cancel | command/token 有，token 未被消费 | 未完成，Critical |
| cancel/failure partial output cleanup | exporter/service 未兜底 | 未完成，Important |
| FFmpeg artifact integration tests | plan 要求存在，仓库无 `src-tauri/tests/ffmpeg_export.rs` | 未完成，Important |
| local 14-day trial | 已实现 file trial marker | 通过 |
| activation status boundary | Noop product store + Memory test store | 通过 |
| no activation secret hardcoded | 未发现 | 通过 |
| license UI badge | 已接入 idle/preview | 自动测试通过，manual overlap gate 未完成 |
| manual FFmpeg gates | checklist 全未勾选 | 未完成 |
| Native Safety Gate | checklist BLOCKED | 未完成 |

## 12. 建议整改顺序

### Phase R1: 修复当前导出路径回归

目标：

- 非-FFmpeg product path 不再因为缺失 source artifact 陷入不可导出死路。
- 文档不再 overclaim。

建议任务：

1. 明确 Phase 6 当前是 boundary-only 还是必须 playable export。
2. 若 boundary-only：`export_video()` 在没有 source artifact 时返回明确 FFmpeg Gate summary/error，不要求“先完成一次可播放录制”。
3. 若 playable export：先完成 R3 的 original artifact writer，再保留 source requirement。
4. 同步更新 `HANDOFF.md` / checklist，把 export 完成态改为 blocked。

验证：

- 普通录制后点击导出，不出现死路错误。
- `npm test -- --run` 覆盖 UI error/summary。

### Phase R2: 接入 `ExportService` command path

目标：

- `export_video()` 真正形成并消费 `TrimExportRequest`。
- cancel token/progress reporter 进入 exporter boundary。

建议任务：

1. `export_video()` 调用 `export_recording_with_timeline()`。
2. 在非-FFmpeg build 中返回明确 “FFmpeg 导出未启用” 或 Gate summary，但不要伪造 outputPath。
3. 加入 cancel check。
4. 加入 exporter callback progress event。
5. 增加后端并发 export guard。

验证：

- Rust test：export service forwards intermediate progress。
- Rust test：cancel before exporter returns `ExportCancelled`。
- Rust test：auto-trim off creates no-op/full timeline request。
- Rust test：auto-trim on consumes cut timeline request。

### Phase R3: 完成 original recording artifact writer

目标：

- `stop_recording` 在 FFmpeg feature + Native Safety review 后返回真实 source artifact。

建议任务：

1. 实现 `FfmpegRecordingWriter` 真正 mux video/audio。
2. writer push/finish error 全部 fatal。
3. partial source artifact failure cleanup。
4. feature-gated tests inspect file size、duration、video stream、audio stream。
5. Native Safety review 后才切默认 FFmpeg writer。

验证：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact`
- 手动 `stop_recording` 返回 non-null `outputPath`。

### Phase R4: 完成 playable preset export

目标：

- 三种 preset 真正产出可播放 output。
- auto-trim on/off 行为都符合 plan。

建议任务：

1. 新增 `src-tauri/tests/ffmpeg_export.rs`。
2. 实现 `FfmpegTrimExporter`。
3. 使用 `request.cut_timeline.keeps`。
4. 实现 fixed scale/crop policy。
5. progress callback 来源于 processed duration / timestamp。
6. cancel/failure cleanup partial output。
7. binding inspection 后才返回 `outputPath`。

验证：

- FFmpeg feature integration tests 全过。
- checklist artifact evidence table 填完整。

### Phase R5: 修复 RMS bucket 语义

目标：

- 录后 sensitivity 完整重聚合是真的，不只是近似 label。

建议任务：

1. 修改 base bucket boundary。
2. 解决 750ms window 与 100ms base bucket 不整除问题。
3. 补 legacy sidecar 行为说明。

验证：

- High/Medium/Low window duration 精确测试。
- 录后切 sensitivity 不需重录即可改变 timeline。

### Phase R6: 收尾文档与 manual gates

目标：

- docs 与真实状态一致。
- 不再 overclaim。

建议任务：

1. 更新 `HANDOFF.md`。
2. 更新 `tests/phase-6-w11-w12-checklist.md`。
3. 完成 BUG.md prevention scan。
4. 完成 10-minute 1080p pressure。
5. 完成 Native Safety Gate。

## 13. 建议新增测试清单

Rust：

- `export_video_without_source_in_non_ffmpeg_build_returns_explicit_gate_error`
- `export_video_auto_trim_off_builds_noop_cut_timeline_request`
- `export_video_auto_trim_on_consumes_cut_timeline_request`
- `export_service_removes_partial_output_on_export_error`
- `export_service_removes_empty_output_on_validation_error`
- `export_service_removes_partial_output_on_cancel`
- `export_service_forwards_intermediate_progress_from_exporter`
- `consume_frames_writer_push_audio_failure_records_error`
- `base_audio_bucket_exact_boundary_starts_new_bucket`
- `aggregate_base_audio_activity_medium_window_is_exact_750ms`
- `ffmpeg_recording_writer_creates_playable_source_artifact`
- `ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source`
- `ffmpeg_exporter_applies_cut_timeline_and_preserves_original`
- `ffmpeg_exporter_cancel_removes_partial_output_and_preserves_original`

Frontend：

- cancel button disabled/hidden after cancel event。
- export progress shows intermediate callback value。
- export error from `export-progress.error` is displayed or at least does not leave stuck exporting state。
- non-FFmpeg gate summary/error copy is shown consistently。
- license badge layout manual check remains in checklist。

Manual：

- 1080p 10-minute recording/export pressure。
- Bilibili 16:9 playable output with video/audio。
- Douyin 9:16 playable output with video/audio。
- Xiaohongshu 1:1 playable output with video/audio。
- Auto-trim off exports full source。
- Auto-trim on consumes cut timeline and output duration is shorter。
- Original artifact still exists after export。
- Cancel removes partial output。
- Native Safety review covers `ffmpeg_writer.rs`、`trim_exporter.rs`、SCK callback、credential persistence。

## 14. Ready To Merge / 下一步判断

**结论：No。**

本地授权边界和一部分 export preflight helper 可以保留，但 Phase 6 export 不能视为完成。当前最需要先修的是：

1. 正常录制后导出直接失败的 product path 回归。
2. `export_video()` 未接入 `ExportService` / `TrimExporter` / cancel / callback progress。
3. writer live audio error 吞错。
4. partial output cleanup。
5. RMS bucket 语义。
6. docs/checklist overclaim。

修复前建议不要继续扩大 FFmpeg 生产实现的范围，也不要把当前状态写成 Phase 6 完成。更稳妥的表述是：Phase 6 的 local license boundary 和 export preflight 部分完成；playable export、original artifact、cancel/progress 的真实后端语义仍是阻塞项。
