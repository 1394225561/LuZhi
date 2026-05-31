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

## 15. FFmpeg playable export 复审（2026-05-30）

> 本节是对 commit `555b7f2c91d397b88ed821ccff4d8943aee23710` 起始的 FFmpeg playable export 实现，以及后续整改提交 `104f78abef82385833f9d507a2c9ee8ccb6a9b78` 的追加复审。

### 15.1 复审范围

用户说明：

- 已基于 `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md` 完成编码。
- 覆盖上一轮 review 中 Important 5 和 Important 6 的整改。
- 覆盖本文件上一轮 code review 的整改。
- 指定起点为 commit `555b7f2c91d397b88ed821ccff4d8943aee23710`。

本轮采用 inclusive 审查口径：

```bash
git rev-parse 555b7f2c91d397b88ed821ccff4d8943aee23710^
# f85ad4519aa2166ddafe19fcc1cd1c12ea9960ea

git diff f85ad4519aa2166ddafe19fcc1cd1c12ea9960ea..104f78abef82385833f9d507a2c9ee8ccb6a9b78
```

审查范围统计：

```text
10 files changed, 1435 insertions(+), 84 deletions(-)
```

变更文件：

- `HANDOFF.md`
- `src-tauri/src/app/export_service.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/mod.rs`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/test_support/ffmpeg_helpers.rs`
- `src-tauri/tests/ffmpeg_export.rs`
- `tests/phase-6-manual-ffmpeg-gates.md`

额外对照输入：

- `HANDOFF.md`
- `BUG.md`
- `.codex/rules/0-global.md` 到 `.codex/rules/5-docs.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`
- `reference/tasks/phase-6-ffmpeg-implementation.md`

### 15.2 总体结论

**结论：No，不建议合并为 Phase 6 完成态。**

本轮相比上一轮有实质进展：

- `FfmpegRecordingWriter` 不再是纯骨架，已能通过 FFmpeg binding 生成 MP4。
- `FfmpegTrimExporter` 不再直接返回 “实现需补齐”，已能在 synthetic source 上导出三种 preset 尺寸。
- `src-tauri/tests/ffmpeg_export.rs` 已存在，且在当前本机 FFmpeg dev 环境中可以运行。
- `export_video()` 已比上一轮更接近真实导出路径：会创建 output path、progress reporter，并调用 `ExportService`。
- `media::ffmpeg_common` 已作为生产 inspection helper 出现，方向正确。
- 产品代码未发现 `ffmpeg` / `ffprobe` CLI 调用，仍走 `ffmpeg-next` binding。
- 前端仍只接收轻量 summary/progress/path/status，没有接触媒体帧流。

但 Phase 6 export 仍有阻塞问题：

1. **产品录制路径仍没有接入 `FfmpegRecordingWriter`**。FFmpeg writer 实现存在，但 macOS 正常录制仍使用 `CountingRecordingWriter::new(None)`，导致 `export_video()` 在 FFmpeg 构建下仍拿不到 original recording artifact。
2. **非空 `CutTimeline` 的导出时间线不可信**。当前 seek 和 PTS/time-base 换算使用了错误单位，integration tests 又没有覆盖非空 cut timeline。
3. **`export_video()` 仍在 async command 内同步执行 FFmpeg 导出**，没有按计划进入 `spawn_blocking`，长导出可能阻塞 Tauri async runtime。
4. **writer 视频写入忽略 `VideoFrame.stride_bytes`**，真实 ScreenCaptureKit padded frame 可能 panic 或行错位。
5. **H.264 encoder time base 使用纳秒级 `1/1_000_000_000`**，测试输出已经出现 libx264 MB rate 超限警告，说明时间基/帧率参数不适合作为生产实现。
6. **artifact validation 仍不足**，没有严格检查 audio stream 和 duration > 0，且 duration 单位换算错误。
7. **无音频 silent AAC track、音频重采样、progress 中间值、取消后的完整清理路径**仍未完全落地。

因此，本轮可以认定为 “FFmpeg prototype / artifact smoke tests 有进展”，但不能认定为 “Phase 6 playable export 完整完成”。

### 15.3 自动化验证结果

本轮实际执行：

```bash
git diff --check f85ad4519aa2166ddafe19fcc1cd1c12ea9960ea..104f78abef82385833f9d507a2c9ee8ccb6a9b78
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
pkg-config --libs --cflags libavformat libavcodec libavutil libswscale libswresample libavfilter
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg
cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets
cargo build --manifest-path src-tauri/Cargo.toml --features ffmpeg
npm test -- --run
npm run build
```

结果：

- `git diff --check ...`: PASS。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS，199 tests。
- `pkg-config --libs --cflags libavformat libavcodec libavutil libswscale libswresample libavfilter`: PASS，本机可找到 FFmpeg dev libraries。
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`: PASS，209 unit tests + 5 integration tests。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets`: PASS with warnings。
- `cargo build --manifest-path src-tauri/Cargo.toml --features ffmpeg`: PASS with warnings。
- `npm test -- --run`: PASS，51 tests。
- `npm run build`: PASS。

重要观察：

- FFmpeg feature tests 运行期间出现多条 libx264 警告：

```text
MB rate (8160000000000) > level limit (16711680)
```

这不是测试失败，但它强烈提示当前 encoder time base / frame rate 参数不合理。结合代码中 `video_enc.set_time_base(Rational(1, 1_000_000_000))` 和 frame PTS 使用纳秒值，本轮将其列为 Important 问题。

- `cargo clippy --features ffmpeg --all-targets` 中出现新增 warnings：
  - `src/media/trim_exporter.rs` unused imports。
  - `src-tauri/tests/ffmpeg_export.rs` unused imports。
  - `src/media/trim_exporter.rs` 的 plane copy loop 有 clippy 提示。
  - `src/app/export_service.rs` `too_many_arguments` 属于结构问题但本轮不单独升级为阻塞项。

### 15.4 Strengths

1. **Important 6 从“完全骨架”推进到了可运行 prototype**
   - `src-tauri/src/media/ffmpeg_writer.rs` 已实现 H.264 + AAC + MP4 muxing 的基本路径。
   - `src-tauri/src/media/trim_exporter.rs` 已实现输入解码、视频缩放、输出编码和 muxing 的基本路径。
   - synthetic source 能被 integration tests 用作导出输入。

2. **Important 5 的文件级缺口已经补上**
   - `src-tauri/tests/ffmpeg_export.rs` 已新增。
   - 当前包含 missing source、cancel before start、full duration、Douyin、Xiaohongshu 等 artifact smoke tests。
   - 在具备 FFmpeg dev libraries 的本机环境中，feature-gated tests 可以跑通。

3. **安全边界方向正确**
   - 未发现 `std::process::Command` 调用 `ffmpeg` 或 `ffprobe`。
   - 未发现 shell 字符串拼接 FFmpeg 参数。
   - 仍通过 `ffmpeg-next` binding 工作，符合架构中 “FFmpeg binding / C API，不调用 CLI” 的约束。

4. **数据流红线仍保持**
   - 变更集中在 Rust media/app boundary。
   - React 侧没有新增音视频帧流、audio activity stream、frame diff stream 的传输。
   - 前端仍只展示 lightweight progress、summary、path、license/status。

5. **部分上一轮问题确实有整改**
   - `export_video()` 已接近接入 `ExportService` / `TrimExporter`。
   - `ExportService` 对失败路径做了 partial output cleanup。
   - `ffmpeg_common` 从 test_support 中独立出来作为生产模块，方向正确。
   - `FfmpegTrimExporter` 成功后由 command 层做 artifact validation。

### 15.5 Critical Issues

#### Critical 1: FFmpeg writer 未接入产品录制路径，正常录制后仍没有 original recording artifact

位置：

- `src-tauri/src/platform/macos_service.rs:221`
- `src-tauri/src/platform/macos_service.rs:327`
- `src-tauri/src/lib.rs:763`

现象：

- `MacRecordingService::start()` 仍固定创建：

```rust
let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));
```

- `CountingRecordingWriter::finish()` 在 `output_path: None` 时返回 `RecordingResult.output_path = None`。
- `stop()` 将 `result.output_path.clone()` 写入 `self.last_recording_output_path`。
- `export_video()` 在 FFmpeg 构建下继续要求 source path：

```rust
let source_path = source_path
    .ok_or_else(|| "没有可用的原始录制文件，请先完成一次可播放录制".to_string())
```

为什么重要：

- `FfmpegRecordingWriter` 虽然实现了，但产品主路径没有使用它。
- 用户完成一次真实录制后，仍然不会产生可导出的 original recording artifact。
- 这直接阻断 “录制 source artifact -> preset export” 的 Phase 6 核心闭环。
- 这也意味着当前 integration tests 只能证明 synthetic helper 路径有效，不能证明产品录制路径有效。

违反的计划/验收：

- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md` Success Criteria：`FfmpegRecordingWriter::finish()` 返回 `Some(output_path)` 且 source artifact 可 inspect。
- 同计划 File Structure 要求修改 `src-tauri/src/platform/macos_service.rs`，在 `ffmpeg` feature 下选择 `FfmpegRecordingWriter`。
- Phase 6B：Original recording artifact is preserved。

建议修复：

1. 在 `macos_service.rs` 增加 production writer factory，例如：
   - `#[cfg(feature = "ffmpeg")]` 创建 `FfmpegRecordingWriter::new(recording_output_path())`。
   - `#[cfg(not(feature = "ffmpeg"))]` 保持 `CountingRecordingWriter::new(None)` 或明确 gate writer。
2. FFmpeg feature 下 writer 初始化失败应 fail fast，不要静默 fallback 到 counting writer。
3. `stop()` 写入 `last_recording_output_path` 前应确认：
   - path exists；
   - file size > 0；
   - binding inspection 通过；
   - 至少有 video stream；
   - 按产品要求，有 audio stream 或已生成 silent AAC track。
4. 增加 command / service 层测试：
   - FFmpeg feature 下 stop recording 后返回 non-null `outputPath`。
   - source artifact 不被导出覆盖。
   - 非 FFmpeg 构建下 export 返回明确 gate，不伪造 output。

验收建议：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_recording_writer_creates_playable_source_artifact
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg export_video_uses_recorded_source_artifact
```

并完成手动 Gate：

- 真实录制 10 秒。
- `stop_recording` payload 中 `outputPath` 非空。
- 文件存在、非空、可播放。
- 再从 Preview 导出三种 preset。

#### Critical 2: 非空 `CutTimeline` 的 seek / PTS 换算单位错误，auto-trim export 不可信

位置：

- `src-tauri/src/media/trim_exporter.rs:359`
- `src-tauri/src/media/trim_exporter.rs:379`
- `src-tauri/src/media/trim_exporter.rs:406`
- `src-tauri/src/media/trim_exporter.rs:492`
- `src-tauri/tests/ffmpeg_export.rs:41`

现象：

1. `input.seek(seg_start_nanos, ..seg_end_nanos)` 直接把纳秒值传给 FFmpeg seek。
2. `packet.pts()` 转 “纳秒” 的公式写成：

```rust
let pkt_nanos = pkt_ts * video_time_base.0 as i64 / video_time_base.1 as i64;
```

该公式缺少 `* 1_000_000_000`，得到的是秒的有理倍数截断，不是纳秒。

3. `cut_offset_in_tb` 使用：

```rust
let cut_offset_in_tb = cumulative_cut_nanos * video_time_base.1 as i64 / video_time_base.0 as i64;
```

同样缺少从 nanos 到 time-base units 的 `1_000_000_000` 分母。

4. audio cut offset 也存在相同单位问题。
5. integration tests 没有覆盖非空 cut timeline；`src-tauri/tests/ffmpeg_export.rs` 虽然 import 了 `CutReason`、`CutSegment`、`KeepSegment`、`MediaTimestamp`，但没有实际使用。

为什么重要：

- Phase 6 不只是 “三种尺寸转码”，还要求 auto-trim on 消费 `CutTimeline` 并导出更短文件。
- 当前代码在 no-op timeline 上可能产出文件，但不能证明裁剪后的时间线正确。
- 错误时间基会导致 seek 到错误位置、保留段边界错误、输出 PTS 不连续、A/V drift 或输出 duration 不符合 cut timeline。

违反的计划/验收：

- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md` Success Criteria：Auto-trim on consumes `CutTimeline` and exports shorter playable file when cuts exist。
- 同计划要求测试 `ffmpeg_exporter_applies_cut_timeline_and_preserves_original`。
- Native Safety Gate 要求 decoded timestamps retimed monotonically after cut removal。

建议修复：

1. 在 `ffmpeg_common.rs` 中实现并统一使用 helper：

```rust
time_base_units_to_nanos(value: i64, time_base: Rational) -> AppResult<u64>
nanos_to_time_base_units(nanos: u64, time_base: Rational) -> AppResult<i64>
```

2. `input.seek()` 使用 FFmpeg seek 期望单位，不直接传 nanoseconds。
3. packet/frame timestamp 全部先规范化为 nanos，再根据 keep segment 计算 output nanos，最后转 encoder/output time base。
4. 为 `KeepSegment` 写纯函数测试：
   - 第一段 `[0s, 2s]` 输出 `[0s, 2s]`。
   - 第二段 `[6s, 8s]` 输出 `[2s, 4s]`。
   - cut gap 不进入输出 PTS。
5. 增加 artifact-level test：
   - source duration 8s。
   - cut `[2s, 6s]`。
   - output duration 约 4s。
   - original source still exists。
   - output has video/audio stream。

验收建议：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg kept_output_timestamp_second_keep_is_monotonic
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_exporter_applies_cut_timeline_and_preserves_original
```

#### Critical 3: `export_video()` 未使用 `spawn_blocking`，长时间 FFmpeg 导出可能阻塞 Tauri async runtime

位置：

- `src-tauri/src/lib.rs:806`
- `src-tauri/src/lib.rs:820`
- `src-tauri/src/lib.rs:839`

现象：

`export_video()` 是 async Tauri command，但 FFmpeg 导出在 command 内同步执行：

```rust
let export_result = {
    let mut exporter = media::trim_exporter::FfmpegTrimExporter;
    app::export_service::export_recording_with_timeline(...)
};
```

为什么重要：

- FFmpeg decode/encode/mux 是 CPU + IO 密集型长任务。
- 在 async command 内同步执行会占用 Tauri async runtime worker。
- 大文件导出时，取消命令、事件处理、其他 command 都可能被拖慢。
- 这与总体架构 “Tauri 主事件循环不做帧处理、不做编码” 和计划 “exporter 在 blocking worker 中运行” 不一致。

违反的计划/验收：

- `docs/architecture/project-architecture-and-overall-planning.md`：Tauri 主事件循环不做帧处理、不做编码。
- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`：导出 exporter 在 blocking worker 中运行。
- Native Safety Gate：`lib.rs`: export runs in blocking worker and active cancel token is cleared on every return path。

建议修复：

1. 把 FFmpeg export service 调用放入：

```rust
tauri::async_runtime::spawn_blocking(move || {
    let mut exporter = FfmpegTrimExporter;
    export_recording_with_timeline(...)
})
```

2. `AppHandle` 可以 clone 给 progress callback 发事件，但 heavy work 必须在 blocking worker。
3. join error 必须：
   - emit terminal export progress；
   - clear active cancel token；
   - return Chinese structured error。
4. 非 FFmpeg gate branch 不应进入 blocking worker。

验收建议：

- 新增测试或结构检查确保 `export_video()` 的 FFmpeg branch 走 `spawn_blocking`。
- 手动长导出时点击取消，UI 能及时响应。

### 15.6 Important Issues

#### Important 1: `FfmpegRecordingWriter::push_video()` 忽略 `VideoFrame.stride_bytes`

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:214`
- `src-tauri/src/media/ffmpeg_writer.rs:218`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:196`
- `src-tauri/src/platform/macos/screen_capture_kit.rs:221`

现象：

`screen_capture_kit.rs` 从 CVPixelBuffer 读取真实 `bytes_per_row`，并写入 `VideoFrame.stride_bytes`。这说明捕获帧可能有 row padding。

但 writer 中直接执行：

```rust
let dst = input_frame.data_mut(0);
dst[..src_data.len()].copy_from_slice(src_data);
```

问题：

- FFmpeg frame destination linesize 可能等于 `width * 4` 或有自己的 alignment。
- Source `src_data.len()` 是 `source_stride * height`。
- 当 source stride 大于 destination row bytes 时，`dst[..src_data.len()]` 可能越界 panic。
- 即使不 panic，也会把 padded bytes 当作像素连续复制，导致画面行错位。

为什么重要：

- 这是从 synthetic frame 到真实 SCK frame 的典型差异。
- tests 使用的 synthetic frame stride 等于 `width * 4`，不能覆盖真实 padded frame。
- 该问题发生在录制消费线程，可能导致录制失败或 artifact 花屏。

建议修复：

1. 像 exporter 已整改的 C1 一样逐行复制。
2. 使用 source `frame.stride_bytes` 和 destination linesize。
3. 每行只复制 `width * 4` bytes。
4. 校验：
   - `frame.pixel_format == PixelFormat::Bgra8`；
   - `frame.stride_bytes >= width * 4`；
   - `src_data.len() >= frame.stride_bytes * height`。
5. 增加 padded BGRA frame 测试：
   - width=2, height=2, stride=12；
   - 每行 8 bytes 有效像素 + 4 bytes padding；
   - writer 不 panic，输出可 inspect。

#### Important 2: H.264 encoder time base 使用纳秒导致 x264 MB rate 超限

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:73`
- `src-tauri/src/media/ffmpeg_writer.rs:90`
- `src-tauri/src/media/ffmpeg_writer.rs:222`
- `src-tauri/src/media/trim_exporter.rs:256`
- `src-tauri/src/media/trim_exporter.rs:272`

现象：

writer 使用：

```rust
video_enc.set_time_base(Rational(1, 1_000_000_000));
video_stream.set_time_base(Rational(1, 1_000_000_000));
output_frame.set_pts(Some(frame.timestamp.nanos as i64));
```

FFmpeg feature tests 虽然通过，但 libx264 输出多条：

```text
MB rate (8160000000000) > level limit (16711680)
```

为什么重要：

- x264 将 time base / PTS / frame rate 推导成不合理的超高宏块率。
- 当前短 synthetic tests 不能证明真实播放器、长视频、A/V sync 在生产条件下可靠。
- 时间基不合理会影响 duration、seek、interleaved muxing、播放器兼容性和后续裁剪。

建议修复：

1. writer 以 capture config fps 或固定 30fps 设置 encoder：
   - `set_time_base(Rational(1, fps))`
   - 设置 frame rate / average frame rate（按 ffmpeg-next 可用 API）。
2. video PTS 使用连续 frame index，或者把 media timestamp 映射到 encoder time base units。
3. exporter 也不要沿用输入 video time base 作为 encoder time base 的唯一策略，应明确 output fps/preset。
4. inspection 增加 duration 断言，避免“文件能打开但时间轴异常”被误判为成功。

#### Important 3: artifact inspection / validation 不满足计划要求

位置：

- `src-tauri/src/media/ffmpeg_common.rs:55`
- `src-tauri/src/media/ffmpeg_common.rs:73`
- `src-tauri/src/lib.rs:846`

现象：

`inspect_media_artifact()` 中：

```rust
if ictx.duration() > 0 {
    duration_nanos = ictx.duration() as u64;
}
```

FFmpeg container duration 是 AV_TIME_BASE 单位，通常是 microseconds，不是 nanoseconds；这里至少应乘以 1000。

`validate_export_artifact()` 只检查：

- file size > 0；
- has video stream；
- dimensions match expected preset。

没有检查：

- has audio stream；
- duration > 0；
- stream-level duration fallback；
- original source preservation。

为什么重要：

- Phase 6 plan 明确要求 artifact inspection 验证 dimensions、duration、streams、file size。
- 当前 validation 可能接受没有音频、duration 为 0 或时间轴异常的文件。
- 这会掩盖 writer/exporter 的时间基和 silent audio 问题。

建议修复：

1. 修正 container duration 单位：
   - `ictx.duration()` > 0 时转为 nanoseconds。
2. 如果 container duration 缺失，回退到 stream duration * stream time base。
3. `validate_export_artifact()` 至少检查：
   - non-empty；
   - video stream；
   - expected dimensions；
   - audio stream 或明确允许 silent/no-audio policy；
   - duration > 0。
4. 对 source artifact 和 export artifact 使用同一 production inspector。

#### Important 4: exporter 用 output audio stream index 判断 input packet stream

位置：

- `src-tauri/src/media/trim_exporter.rs:165`
- `src-tauri/src/media/trim_exporter.rs:276`
- `src-tauri/src/media/trim_exporter.rs:477`

现象：

代码记录了 input audio stream：

```rust
let audio_stream_info = input
    .streams()
    .find(|s| s.parameters().medium() == ff::media::Type::Audio)
    .map(|s| (s.index(), s.time_base()));
```

但 packet dispatch 时使用：

```rust
} else if Some(pkt_stream) == audio_out_idx {
```

`audio_out_idx` 是 output stream index，不是 input audio stream index。

为什么重要：

- 当输入 stream index 和输出 stream index 恰好一样时，tests 可能通过。
- 一旦输入文件 stream 排序不同，例如 audio/video 顺序、额外 metadata/subtitle stream，音频 packets 会被跳过。
- 导出文件可能缺音频或 A/V sync 错。

建议修复：

1. 单独保存：

```rust
let input_audio_stream_index = audio_stream_info.map(|(idx, _)| idx);
```

2. packet dispatch 使用 input stream index：

```rust
} else if Some(pkt_stream) == input_audio_stream_index {
```

3. 增加测试或 fixture 覆盖 input stream order 不同的场景。

#### Important 5: 音频重采样和 silent AAC track 仍未完成

位置：

- `src-tauri/src/media/trim_exporter.rs:276`
- `src-tauri/src/media/trim_exporter.rs:499`
- `src-tauri/src/media/ffmpeg_writer.rs:268`
- `HANDOFF.md:127`

现状：

- `HANDOFF.md` 已记录 C2 未修复：exporter 音频未做格式重采样。
- `HANDOFF.md` 已记录 I5 未修复：无音频录制时未生成 silent AAC track。
- 但同一段文档前文又写 “音频重采样” 和 “完整导出”，表述存在 overclaim。

为什么重要：

- `FfmpegTrimExporter` 创建了 AAC encoder，要求 F32 planar stereo 48kHz。
- 输入音频 decoder 输出不保证就是该格式。
- writer 当前测试 helper 会生成音频，所以 no-audio path 未被 artifact tests 覆盖。
- 产品要求 “无系统音频且无麦克风时，writer/exporter 必须生成 silent AAC track，保持 MP4 artifact 始终有 audio stream”。

建议修复：

1. exporter 接入 `SwrContext`：
   - input sample format/channel layout/sample rate -> F32 planar stereo 48kHz。
2. writer finish 时如果没有收到音频 chunk，按 video duration 补 silent AAC frames。
3. exporter 输入无 audio stream 时，也生成 silent AAC track。
4. tests：
   - `ffmpeg_recording_writer_without_audio_generates_silent_track`
   - `ffmpeg_exporter_source_without_audio_generates_silent_track`
   - validation 检查 `has_audio_stream == true`。

#### Important 6: `export_video()` 安装 cancel token 后多条早退路径不清理 token

位置：

- `src-tauri/src/lib.rs:673`
- `src-tauri/src/lib.rs:695`
- `src-tauri/src/lib.rs:703`
- `src-tauri/src/lib.rs:763`
- `src-tauri/src/lib.rs:768`
- `src-tauri/src/lib.rs:788`

现象：

`export_video()` 先安装 active cancel token，然后执行多个可能 `?` 早退的步骤：

- `build_cursor_effect_timeline(...)`
- `build_cut_timeline(...)`
- lock service
- FFmpeg 构建下 missing source
- read cut timeline
- read trim metadata
- export output path generation

目前只有部分非 FFmpeg source missing branch 和最终 match 分支清理 token。

为什么重要：

- 早退后 `state.export_cancel_token` 可能残留。
- 下一次 `cancel_export()` 可能操作旧 token。
- UI 可能收不到 terminal `export-progress`，保持 exporting 状态。
- 这类状态 bug 在失败/取消路径上最容易漏测。

建议修复：

1. 引入 helper：

```rust
fn clear_export_cancel_token(state: &AppState, token: &Arc<AtomicBool>)
```

只清理当前 export 拥有的 token，避免并发 export 互相覆盖。

2. 引入 terminal progress helper：

```rust
fn emit_export_terminal_progress(app, preset_id, output_path, error)
```

3. 安装 token 后，所有 early return 都必须：
   - emit terminal progress；
   - clear token；
   - return error/summary。
4. 后端拒绝并发 export，不能只依赖 UI disabled。

#### Important 7: progress callback 不满足 “至少一个 1..99 中间值” 要求

位置：

- `src-tauri/src/media/trim_exporter.rs:533`
- `src-tauri/src/media/trim_exporter.rs:535`
- `src-tauri/tests/ffmpeg_export.rs:41`

现象：

progress 当前按 segment 完成度上报：

```rust
let pct = ((seg_idx + 1) * 100 / total_keeps).min(100) as u8;
progress.report(pct);
```

当 no-op timeline 只有一个 keep segment 时，第一次 report 就是 100。

为什么重要：

- Phase 6C 要求 progress includes intermediate values, not just 0/100 UI state。
- 长导出时 UI 仍可能只看到 0 -> 100。
- 当前 integration tests 没捕获 progress callback 序列。

建议修复：

1. 按 processed kept duration、packet count 或 estimated input timestamp 周期性上报。
2. clamp 为 `1..99`，最终成功路径再 report 100。
3. 增加 test：

```rust
ffmpeg_exporter_reports_intermediate_progress
```

断言 callback 中存在 `progress > 0 && progress < 100`，最后一个值为 100。

### 15.7 Minor / Advisory

#### Minor 1: FFmpeg integration tests 覆盖面不足且有 unused imports

位置：

- `src-tauri/tests/ffmpeg_export.rs:12`
- `src-tauri/tests/ffmpeg_export.rs:13`
- `src-tauri/tests/ffmpeg_export.rs:15`

现象：

`CutReason`、`CutSegment`、`KeepSegment`、`MediaTimestamp`、`TrimExportRequest` 被 import，但没有使用。

这反映出 plan 中要求的以下测试尚未落地：

- source artifact writer test；
- video-before-audio / bounded queue；
- no-audio silent track；
- same input/output rejection；
- all fixed presets in one test；
- cut timeline duration shortening；
- original preservation；
- progress callback；
- no FFmpeg/ffprobe CLI static scan。

建议：

- 未使用 imports 不只是 style 问题，应通过补足测试场景消掉。
- 如果短期不补测试，至少删除 unused imports，避免 clippy warnings 干扰真正风险。

#### Minor 2: `tests/phase-6-w11-w12-checklist.md` 状态已过期

位置：

- `tests/phase-6-w11-w12-checklist.md:12`
- `tests/phase-6-w11-w12-checklist.md:49`

现象：

文件仍写：

```text
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg: BLOCKED
```

但本轮本机验证已经通过 `pkg-config` 和 FFmpeg feature tests。

建议：

- 更新 Verification Summary：区分 “当前本机自动化已通过” 与 “manual gates / Native Safety 未完成”。
- FFmpeg Artifact Evidence table 应填真实 evidence，或明确保持空表代表 manual gate 未执行。

#### Minor 3: 手动 Gate 中使用 `ffprobe` 需注明仅限人工验证

位置：

- `tests/phase-6-manual-ffmpeg-gates.md:65`

现象：

手动 Gate 写 “用 `ffprobe` 验证”。生产代码禁止调用 FFmpeg CLI，这不等同于人工手动不能用 `ffprobe`，但建议避免误读。

建议：

- 改成 “人工本地可使用 VLC/IINA/ffprobe 辅助验证；产品代码和自动化测试仍必须使用 binding inspection，不得 shell out”。

### 15.8 Native Safety 专项复核

本轮新增/涉及的 Native Safety 风险点：

1. `src-tauri/src/media/ffmpeg_writer.rs:18`
   - `unsafe impl Send for SendScaler {}`
   - 当前 public methods 需要 `&mut self`，实际单线程使用，但 FFmpeg scaler 是否可跨线程移动仍需人工确认。
   - 若后续实现 worker-backed writer，应避免 scaler 在多个线程共享；只允许 move 到单一 worker thread。

2. `src-tauri/src/media/trim_exporter.rs:398` 和 `src-tauri/src/media/trim_exporter.rs:485`
   - `unsafe { frame::Frame::empty() }`
   - 需要确认 ffmpeg-next receive_frame 对 empty Frame 的初始化、drop、reuse 安全性。

3. `src-tauri/src/media/trim_exporter.rs:422`
   - 手动从 `decoded.as_ptr()` 读取 AVFrame data/linesize 并逐行 copy。
   - 当前已有 null check，但还需要确认：
     - negative linesize；
     - plane height 与 data buffer 实际大小；
     - packed / planar pixel formats；
     - `copy_per_row` 不应复制超出目标有效 row bytes；
     - decoder frame lifetime 在 copy 期间有效。

4. `src-tauri/src/media/ffmpeg_common.rs:44`
   - `unsafe { &*params.as_ptr() }` 读取 AVCodecParameters width/height。
   - 只读 plain integer，风险较低，但需要确认指针非空和 lifetime 绑定在 `params` 活跃期内。

5. `src-tauri/src/media/ffmpeg_writer.rs:218`
   - 目前不是 unsafe，但对 slice 边界的假设不成立时会 panic。
   - 应作为 Native Safety review 的 “真实 SCK padded frame” 专项检查。

Native Safety Gate 不通过前，不建议将 FFmpeg feature 作为默认产品路径发布。

### 15.9 数据流、安全与 BUG.md 复核

本轮未发现以下回归：

- 未发现 `std::process::Command` 调用 `ffmpeg` 或 `ffprobe`。
- 未发现 FFmpeg CLI 参数拼接用户输入。
- 未发现音视频帧流进入 React / TypeScript 层。
- 未发现 hardcoded activation private key、Sentry DSN、PostHog key 或生产 secret。
- 未发现新增平台发布 API、模板系统、字幕、摘要、团队协作等 MVP 禁区功能。
- 未发现 BUG.md 中透明窗口、拖拽、framer-motion `whileTap` 相关预防规则的新回归。

仍需关注：

- `test_support` 在 `feature = "ffmpeg"` 时会被编入 lib：`src-tauri/src/test_support/mod.rs` 使用 `#[cfg(any(test, feature = "ffmpeg"))]`。当前主要用于 tests/helper，但后续应避免产品路径依赖 `crate::test_support`。
- `tests/phase-6-manual-ffmpeg-gates.md` 使用 `ffprobe` 只能作为人工验证工具，不得进入生产代码或自动化 gate 的产品逻辑。

### 15.10 与上一轮 review 的整改对照

上一轮 Critical / Important 对照：

| 上一轮问题 | 本轮状态 | 说明 |
|---|---|---|
| Critical 1: 正常录制后导出失败 / 无 source artifact | **未完成** | Writer 实现存在，但 `macos_service.rs` 仍未接入，产品路径仍无 source artifact。 |
| Critical 2: `export_video()` 未接入 ExportService/TrimExporter | **部分完成** | 已调用 ExportService/Exporter，但未 spawn_blocking，早退清理不完整，FFmpeg 构建下仍受 source artifact 阻塞。 |
| Important 1: Base RMS bucket 语义 | **上一轮后已声称修复，本轮未重点复审** | 本轮主要关注 FFmpeg playable export。 |
| Important 2: live loop push_audio 吞错 | **已修复方向可接受** | 已有 `consume_frames_writer_push_audio_failure_records_error`。 |
| Important 3: partial output cleanup | **部分完成** | `ExportService` 有 cleanup，但 command 早退和 blocking join 路径仍需兜底。 |
| Important 4: docs/checklist overclaim | **仍存在** | HANDOFF 写 “音频重采样/完整导出”，但 C2/I5/I6 仍未修。Checklist 状态也过期。 |
| Important 5: 缺少 FFmpeg artifact integration tests | **部分完成** | 文件已新增且 smoke tests 通过，但 cut timeline、silent track、progress、source writer、original preservation 等计划测试缺失。 |
| Important 6: FFmpeg writer/exporter 骨架 | **部分完成** | 不再是骨架，但仍是 prototype，产品接入、time base、stride、audio resample、cut timeline 语义未达生产验收。 |

### 15.11 建议整改顺序

#### Phase F1: 先接通产品 source artifact

目标：

- FFmpeg feature 下，真实录制能生成 original recording artifact。
- `export_video()` 不再因为 `last_recording_output_path == None` 阻塞。

任务：

1. 在 `macos_service.rs` 增加 writer factory。
2. FFmpeg feature 下使用 `FfmpegRecordingWriter`。
3. 生成独立 original recording path。
4. finish 后 inspect source artifact。
5. fail/cancel 时清理 partial source artifact。
6. 补产品路径测试和手动 Gate。

#### Phase F2: 修正 writer 基础媒体语义

目标：

- 真实 SCK frame 不 panic、不花屏。
- H.264 时间基和 duration 合理。
- 无音频录制也有 audio stream。

任务：

1. writer `push_video()` 按 stride 逐行复制。
2. writer video time base 改成 fps / frame index 模型。
3. writer duration 不从最后一个 raw nanos 粗略除秒，应来自 artifact inspection 或准确 timestamp。
4. no-audio path 生成 silent AAC track。
5. 增加 padded frame、no audio、duration tests。

#### Phase F3: 修正 exporter cut timeline / timestamp 核心算法

目标：

- auto-trim on 真正使用 keep segments。
- 多段输出 PTS 单调连续。
- output duration 符合 total kept duration。

任务：

1. `ffmpeg_common.rs` 增加 time-base helper。
2. 修正 seek 单位。
3. 修正 packet/frame PTS nanos 转换。
4. 修正 video/audio cut offset。
5. 修正 input audio stream index 判断。
6. 增加 pure timestamp tests + artifact-level cut test。

#### Phase F4: 音频格式与 progress/cancel 收口

目标：

- 输入音频格式不依赖 writer 恰好产出 F32P@48kHz。
- progress 有真实中间值。
- cancel 和所有失败路径都清理状态。

任务：

1. exporter 接 SwrContext。
2. source no-audio 时生成 silent AAC。
3. progress 按 processed duration 上报 1..99。
4. command FFmpeg branch 进入 `spawn_blocking`。
5. 安装 cancel token 后所有早退路径统一清理。
6. 后端拒绝并发 export。

#### Phase F5: 文档和 Gate 对齐真实状态

目标：

- 不再 overclaim。
- 自动化、manual、Native Safety 状态清晰。

任务：

1. 更新 `HANDOFF.md`：把 “完整导出/音频重采样” 改成实际状态。
2. 更新 `tests/phase-6-w11-w12-checklist.md`：FFmpeg feature tests 当前本机可跑，但 manual gates / Native Safety 未完成。
3. 填写 FFmpeg Artifact Evidence table。
4. 手动执行 10 分钟 1080p、A/V sync、原始素材保留、取消清理。
5. 完成人工 Native Safety review。

### 15.12 建议补充测试清单

Rust / FFmpeg feature:

- `ffmpeg_recording_writer_creates_playable_source_artifact`
- `ffmpeg_recording_writer_handles_padded_bgra_stride`
- `ffmpeg_recording_writer_without_audio_generates_silent_track`
- `ffmpeg_recording_writer_uses_sane_video_time_base`
- `ffmpeg_exporter_rejects_same_input_output_path`
- `ffmpeg_exporter_exports_all_fixed_presets_from_synthetic_source`
- `ffmpeg_exporter_applies_cut_timeline_and_preserves_original`
- `ffmpeg_exporter_reports_intermediate_progress`
- `ffmpeg_exporter_cancel_after_progress_removes_partial_output_and_preserves_original`
- `ffmpeg_exporter_source_without_audio_generates_silent_track`
- `ffmpeg_exporter_handles_input_audio_stream_index_not_equal_output_index`
- `kept_output_timestamp_second_keep_is_monotonic`
- `packet_timestamps_are_rescaled_before_muxing`
- `inspect_media_artifact_converts_container_duration_to_nanos`
- `validate_export_artifact_rejects_missing_audio_or_zero_duration`
- `rust_code_does_not_shell_out_to_ffmpeg_or_ffprobe`

Rust / command-service:

- `export_video_ffmpeg_branch_runs_in_blocking_worker`（可通过结构测试或拆 helper 测试）
- `export_video_missing_source_clears_cancel_token_and_emits_terminal_progress`
- `export_video_cut_timeline_read_failure_clears_cancel_token`
- `export_video_output_path_failure_clears_cancel_token`
- `export_video_rejects_concurrent_export`
- `cancel_export_is_idempotent_without_active_export`

Manual:

- 真实录制 10 秒后 `stop_recording.outputPath` 非空、存在、可播放。
- 1080p 10 分钟录制 + Bilibili preset 导出。
- Douyin / Xiaohongshu preset 输出尺寸。
- Auto-trim off 输出时长约等于 source。
- Auto-trim on 输出时长小于 source，且 cut gap 消失。
- A/V sync 偏移小于 100ms。
- 取消导出后无 partial output。
- 原始 source artifact 导出前后 path 和 size 不变。

### 15.13 Ready To Merge / 下一步判断

**Ready to merge: No。**

技术判断：

- 当前 FFmpeg writer/exporter 已经从骨架推进到可运行 smoke prototype，但还没有达到 Phase 6 playable export 的生产闭环。
- 最致命的是产品录制路径仍未产生 source artifact；因此即使 exporter synthetic tests 通过，用户从正常录制进入导出的主路径仍不可用。
- 其次是 cut timeline / time base / stride / audio resample 这些媒体语义问题，均属于后续越晚修越难排查的底层问题。
- Native Safety Gate 尚未完成，不能把 FFmpeg feature 作为默认产品路径发布。

建议下一步：

1. 先修 Critical 1，让真实录制产生 source artifact。
2. 再修 Critical 2 和 Important 1/2/3，确保 “能播放” 不只是短 synthetic 文件，而是时间轴、裁剪和真实帧布局都正确。
3. 最后处理 `spawn_blocking`、cancel/progress、silent audio、docs/checklist。

修复完成前，建议对外状态表述为：

> Phase 6 local license boundary 和 export preset/preflight 已完成；FFmpeg playable export 已有 binding prototype 与 smoke tests，但 product path、cut timeline、time-base、audio/silent track、Native Safety 和 manual gates 仍未完成。

## 16. Phase 6 FFmpeg playable export 再复审（2026-05-30，commit `bec70aae` / 当前 HEAD + worktree）

### 16.1 结论摘要

**Ready to merge: No。**

本轮复审相对第 15 节有一个重要变化：Phase 6 FFmpeg 路径已经继续推进，当前工作区中 `macos_service.rs` 已在 `feature = "ffmpeg"` 时接入 `FfmpegRecordingWriter`，`export_video()` 也已经把 FFmpeg 转码放进 `spawn_blocking`。因此第 15 节中 “产品路径仍无 source artifact / 未 spawn_blocking” 的部分结论已经被后续代码推进覆盖。

但当前仍不能合并为 Phase 6 完成，原因是导出核心媒体语义仍存在 Critical 级问题：音频 PTS/裁剪段处理不正确，导出预设声明的 crop/fit 策略没有真正实现，cursor timeline 失败仍会阻断基础可播放导出，cursor effect timeline 虽被传入但未被 exporter 应用。writer 侧还存在同步编码压力和真实录制时间基过于理想化的问题。

| 项目 | 本轮判断 |
|---|---|
| 指定 commit | `bec70aae215512f56f441179c67fc0e40dff8901` |
| 指定 commit 实际内容 | docs/checklist only：只改 `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md` 和 `tests/phase-6-w11-w12-checklist.md` |
| 代码复审实际范围 | 当前 `HEAD` + dirty worktree 中 Phase 6 相关 Rust/TS/测试改动 |
| 自动化验证 | Rust/TS 测试与 build 通过；FFmpeg feature 测试通过但输出 AAC 时间戳警告 |
| BUG.md 预防规则 | 本轮未发现新增违反项 |
| 合并判断 | 不建议合并，需先修 Critical / Important |

### 16.2 本轮有效范围

用户给定的 commit `bec70aae215512f56f441179c67fc0e40dff8901` 的 parent 是 `cb4536a643c43fab0c1f8e8b2c3d8d7c75586bcb`。该 commit 本身没有代码变更，只更新了计划和 checklist：

- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `tests/phase-6-w11-w12-checklist.md`

因此本轮真正需要整改的代码风险来自当前分支/工作区中 Phase 6 后续实现，重点文件如下：

- `src-tauri/src/lib.rs`
- `src-tauri/src/media/export_paths.rs`
- `src-tauri/src/media/export_presets.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/tests/ffmpeg_export.rs`
- `src/App.tsx`
- `src/App.test.tsx`
- `src/components/preview-view.tsx`
- `src/lib/tauri.ts`
- `tests/phase-6-manual-ffmpeg-gates.md`
- `tests/phase-6-w11-w12-checklist.md`

复审时工作区已有未提交改动。本文档第 16 节是追加记录，不回滚、不覆盖第 15 节历史结论。

### 16.3 本轮验证记录

本轮审查记录中的验证命令和结果：

| 命令 | 结果 | 备注 |
|---|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 | Rust formatting clean |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 | 199 tests passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` | 通过 | 219 unit tests + 6 integration tests passed；但输出 AAC 时间戳警告 |
| `npm test -- --run` | 通过 | 51 tests passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` | 通过，有 warnings | warnings 见 16.8 |
| `npm run build` | 通过 | 前端 build 通过 |
| `git diff --check cb4536a643c43fab0c1f8e8b2c3d8d7c75586bcb` | 通过 | 未发现 whitespace error |

关键异常信号：

```text
[aac] Queue input is backward in time
```

该警告在 `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` 中重复出现。虽然测试最终通过，但这个警告直接指向音频 encoder 收到非单调或倒退的输入时间戳。对 Phase 6 playable export 来说，这不是可忽略噪音，而是时间轴正确性风险。

### 16.4 Critical 1: FFmpeg exporter 音频 PTS / 裁剪段处理仍然错误

位置：

- `src-tauri/src/media/trim_exporter.rs:417`
- `src-tauri/src/media/trim_exporter.rs:560`
- `src-tauri/src/media/trim_exporter.rs:590`
- `src-tauri/src/media/trim_exporter.rs:591`
- `src-tauri/src/media/trim_exporter.rs:593`

现象：

1. `trim_exporter.rs:417` 只用 video packet 判断是否超过当前 keep segment：

   ```rust
   if pkt_stream == video_stream_index && pkt_ts > seg_end_in_vtb {
       break;
   }
   ```

   audio packet / decoded audio frame 没有按 segment 边界独立判断、skip 或 slice。

2. `trim_exporter.rs:560` 从 `resampled.pts().unwrap_or(0)` 读取音频 PTS：

   ```rust
   let raw_audio_pts = resampled.pts().unwrap_or(0);
   ```

   但 resampled frame 经常没有继承 decoded input frame 的 PTS，`unwrap_or(0)` 会把多个音频帧压到同一个原始 PTS 起点。

3. `trim_exporter.rs:590-591` 只把输出 PTS clamp 到 `last_audio_out_pts`，然后把 `last_audio_out_pts` 设成同一个值：

   ```rust
   let out_pts = out_pts.max(last_audio_out_pts);
   last_audio_out_pts = out_pts;
   ```

   这里没有按 `resampled.samples()` 或 encoder frame sample count 前进。因此多个音频帧可能以相同 PTS 进入 AAC encoder。

4. `trim_exporter.rs:593` 最终把这个不前进或非单调的 PTS 写回音频 frame：

   ```rust
   resampled.set_pts(Some(out_pts));
   ```

影响：

- AAC encoder 已实际报出 `Queue input is backward in time`。
- 导出文件可能出现音频包 PTS 非单调、A/V drift、音频时长与视频时长不一致。
- 多段 cut timeline 下，音频可能泄漏被裁掉区间的内容，或者在 segment 边界错位。
- 现有测试只看 artifact 尺寸、存在性或粗粒度 duration，不能证明音频时间轴正确。

建议修复：

1. 音频输入 PTS 应优先来自 decoded input audio frame，而不是 resampled frame 的默认 PTS。
2. 对每个 keep segment，audio packet / decoded audio frame 也必须按 segment start/end 判断。
3. 如果音频 frame 跨越 keep 边界，要么 slice 到边界，要么明确采用保守 skip 策略，并在测试中锁定语义。
4. 输出音频 PTS 应使用一个独立的 `next_audio_out_pts` 递增游标；每送入一帧后按 sample count 前进。
5. PTS 映射应统一为：input audio PTS → 扣除已裁剪 gap → 转换到 encoder audio time base → 写入 encoder frame。
6. 增加 integration test 检查导出后的 audio packet/frame PTS 单调递增，并覆盖多段 keep segments。
7. 增加“被裁掉区间有明显音频内容”的 synthetic fixture，验证导出音频中该内容不存在。

### 16.5 Critical 2: 导出预设声明的 crop/fit policy 没有被实现

位置：

- `src-tauri/src/media/export_presets.rs:26`
- `src-tauri/src/media/trim_exporter.rs:340`

现象：

`ExportPresetSpec` 明确定义了 `scale_policy`：

```rust
pub scale_policy: ExportScalePolicy,
```

其中：

- Bilibili: `FitWithBars`
- Douyin: `CenterCrop`
- Xiaohongshu: `CenterCrop`

但 exporter 实际创建 scaler 时只把 source dimensions 直接缩放到 preset dimensions：

```rust
software::scaling::Context::get(
    src_pix_fmt,
    src_width,
    src_height,
    Pixel::YUV420P,
    out_w,
    out_h,
    software::scaling::Flags::BILINEAR,
)
```

这会把 16:9 输入直接拉伸成 9:16 或 1:1，而不是 center crop；也不会为 FitWithBars 生成 letterbox/pillarbox。

影响：

- 抖音 9:16、小红书 1:1 导出会严重变形。
- 当前测试只验证输出 width/height，无法发现视觉几何被拉伸。
- 这违背 `export_presets.rs` 中固定预设的语义，也会让用户误以为已经获得平台适配导出。

建议修复：

1. 在 exporter 中按 `ExportScalePolicy` 分支实现：
   - `FitWithBars`：保持源画面比例，缩放后居中贴到目标 canvas，剩余区域补黑或设计指定背景。
   - `CenterCrop`：保持比例放大到覆盖目标 canvas，再从中心裁剪。
2. 把 crop/fit 数学拆成纯函数测试，避免直接在 FFmpeg frame 操作里难以验证。
3. 增加 synthetic visual geometry fixture，例如左右不同颜色、中心十字线、圆形/方形标记，导出后抽样验证没有横向/纵向拉伸。
4. 对 Bilibili/Douyin/Xiaohongshu 三个 preset 都做 artifact-level 检查，不只检查尺寸。

### 16.6 Important 1: 基础可播放导出仍被 cursor timeline 生成失败阻断

位置：

- `src-tauri/src/lib.rs:698`
- `src-tauri/src/app/events.rs:94`

现象：

`export_video()` 的核心流程一开始就强制生成 cursor effect timeline：

```rust
let cursor = build_cursor_effect_timeline(app.clone(), state.clone()).await?;
```

同时 `ExportSummaryPayload` 中 `effect_timeline_path` 仍是非 nullable `String`：

```rust
pub effect_timeline_path: String,
```

这与 `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md` 中的目标不一致：基础 playable export 不应因为 cursor metadata / effect timeline 不可用而失败；不可用时应返回 `effectTimelinePath: null`，并把 `None` 传给 exporter。

影响：

- 用户只想导出一个可播放 MP4，但 cursor metadata 缺失或生成失败时会直接失败。
- FFmpeg exporter 本身已把 `effect_timeline_path` 设计成 `Option<PathBuf>`，command payload 仍强制非空，会造成边界语义不一致。
- 失败后如果继续读取 `service.last_effect_timeline_path()`，还可能误用旧录制的 stale timeline。

建议修复：

1. `build_cursor_effect_timeline()` 失败不应阻断基础导出；除非用户明确选择“必须应用 cursor 美化”。
2. `ExportSummaryPayload.effect_timeline_path` 改为 `Option<String>`，前端类型同步改成 nullable。
3. `export_video()` 内部 cursor summary 改成 optional，frame/click count 在不可用时返回 0 或 `None`，按 UI 需要确定。
4. 失败时不要回退读取 `service.last_effect_timeline_path()` 的旧值。
5. 增加测试：cursor metadata 不存在/损坏时，FFmpeg export 仍成功，payload 中 `effectTimelinePath` 为 `null`。

### 16.7 Important 2: Cursor effect timeline 被传入 exporter，但没有被应用

位置：

- `src-tauri/src/media/trim_exporter.rs:52`
- `src-tauri/src/media/trim_exporter.rs:119`

现象：

`TrimExportRequest` 已包含：

```rust
pub effect_timeline_path: Option<PathBuf>,
```

但 `FfmpegTrimExporter::export()` 没有读取或解析该路径，也没有把 Phase 4 的 cursor magnification、smoothing、click overlay 合成到输出视频。

影响：

- 当前产物更准确地说是 “preset playable export”，不是完整的 “AI 美化导出”。
- 如果 UI/文档宣称导出包含光标美化，会造成 overclaim。
- 后续补 compositor 时会触碰 video frame processing 主路径，需要单独 Native Safety / performance review。

建议修复：

1. 产品层先明确 Phase 6 当前交付口径：
   - 若本阶段只要求 playable preset export，应在 checklist 和 UI 文案中避免声称已导出 cursor effects。
   - 若本阶段要求 AI 美化导出，则必须实现 effect timeline compositor。
2. 如果实现 compositor，建议先建立纯 Rust timeline parser + frame overlay 接口，再接入 FFmpeg video encode loop。
3. 增加测试：传入 effect timeline 后，导出视频可观察到 overlay/cursor effect；或在未支持时返回明确 unsupported 状态，不静默忽略。

### 16.8 Important 3: Recording writer 在 consumer drain thread 内同步编码，压力风险仍未关闭

位置：

- `src-tauri/src/platform/macos_service.rs:221`
- `src-tauri/src/media/ffmpeg_writer.rs:267`

现象：

`macos_service.rs` 在 `feature = "ffmpeg"` 时已经接入：

```rust
Box::new(crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(output_path)?)
```

这是相对第 15 节的推进。但 `FfmpegRecordingWriter::push_video()` 内部仍同步执行 frame copy、scaling、encoding、muxing。也就是说，consumer thread 从 bounded media channels drain 帧时，会被 FFmpeg 编码耗时拖慢。

影响：

- 捕获 callback 没有直接执行 FFmpeg，这是正确方向；但 consumer 如果追不上，bounded channel 仍可能堆积、丢帧或造成音视频不同步。
- 1080p 长时间录制、4K 预留、系统音频+麦克风混音同时存在时，风险会放大。
- 计划中提到的 “internal encoding worker + byte-budgeted bounded queue” 还没有落地。

建议修复：

1. 把 writer 设计成 worker-backed writer：consumer thread 只做轻量 enqueue，编码/muxing 在独立 worker 中执行。
2. 队列应以字节预算或 frame budget 限流，且记录 drop/late frame telemetry。
3. 明确 backpressure 策略：宁可可观测地降级/丢帧，也不要静默拖垮录制链路。
4. 在该设计完成前，不建议默认启用 FFmpeg writer 作为产品录制主路径。
5. Manual Gate 必须覆盖 1080p 10 分钟压力录制、CPU 压力下录制、系统音频+麦克风同时开启。

### 16.9 Important 4: Writer/export timing 对真实录制过于理想化

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:68`
- `src-tauri/src/media/ffmpeg_writer.rs:81`
- `src-tauri/src/media/ffmpeg_writer.rs:82`
- `src-tauri/src/media/ffmpeg_writer.rs:314`
- `src-tauri/src/media/ffmpeg_writer.rs:315`
- `src-tauri/src/media/ffmpeg_writer.rs:316`
- `src-tauri/src/media/ffmpeg_writer.rs:317`

现象：

writer 侧仍有固定假设：

```rust
let video_fps = 30u32;
video_enc.set_width(1920);
video_enc.set_height(1080);
output_frame.set_pts(Some(self.video_frame_index));
self.video_frame_index += 1;
```

这等价于把所有输入都当作稳定 30fps、固定 1920x1080 输出，并用 frame count 直接生成 PTS。

影响：

- 如果真实捕获帧率不是 30fps，或存在 dropped frames，输出 duration / A/V sync 会偏。
- 如果捕获配置不是 1920x1080，当前是强制缩放到 1080p source artifact；这可以是 MVP 决策，但必须在产品和测试里明确。
- `video_duration_nanos` 来自最后一帧 timestamp，但 encoded video PTS 来自 frame count，两者不是同一个时钟来源。

建议修复：

1. 明确产品约束：MVP 是否强制 source artifact 为 1080p30。
2. 如果强制 1080p30，应在 capture config、writer config、测试和 checklist 中显式验证。
3. 如果不强制，应把 capture timestamps 映射到 encoder PTS，并处理 dropped/late frame。
4. `RecordingResult.duration_nanos` 建议来自最终 artifact inspection 或统一时钟，而不是混用 raw timestamp 与 frame-count PTS。
5. 增加非 30fps / dropped frame synthetic tests。

### 16.10 Minor: Clippy warnings 需要在下一轮整改中清理

本轮 `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` 通过，但仍有 warnings。与 Phase 6 新代码直接相关的包括：

- `src/media/trim_exporter.rs:122`：unused import `time_base_units_to_nanos`
- `src/platform/macos_service.rs:22`：unused import `CountingRecordingWriter`
- `src/media/trim_exporter.rs:230-232`：`audio_dec.is_some()` 后仍有 unnecessary unwrap
- `src/media/trim_exporter.rs`：needless range loop / manual checked division / manual clamp
- `src-tauri/tests/ffmpeg_export.rs:217`：unused variable `result`

此外还有既有 warning：

- `src/app/license_service.rs:135`：`mut self` unnecessary
- `src/app/export_service.rs:12`：`too_many_arguments`
- macOS / SCK 相关既有 warnings

建议：

- Phase 6 新增 warnings 应随整改清理，避免掩盖后续真正的媒体链路风险。
- `too_many_arguments` 可以暂缓，但如果继续扩展 export 参数，建议引入 request struct，避免 command/service/exporter 三层签名继续膨胀。

### 16.11 BUG.md、数据流与安全复核

本轮未发现 BUG.md 预防规则新增回归：

- 未发现新增 `data-tauri-drag-region="false"` 容器级拖拽排除。
- 未发现 `setIgnoreCursorEvents(true)` 相关回归。
- `src/components/recording-panel.tsx` 中存在 `motion.button whileTap`，不属于 BUG-003 禁止的 “`motion.div whileTap` 直接包裹交互元素” 模式。

本轮未发现以下安全/架构红线回归：

- 未发现生产代码通过 `std::process::Command` shell out 到 `ffmpeg` / `ffprobe`。
- 未发现 FFmpeg CLI 参数拼接用户输入。
- 未发现原始音视频帧流进入 React / TypeScript 层。
- 未发现 hardcoded activation secrets。
- 未发现新增字幕、摘要、团队协作、平台发布 API 等 MVP 禁区功能。

仍需注意：

- 手动 Gate 中可以使用 VLC/IINA/ffprobe 辅助人工验收，但生产代码和自动化核心逻辑仍应使用 FFmpeg binding inspection，不得 shell out。
- `test_support` 在 `feature = "ffmpeg"` 下被编入 lib，应避免产品路径依赖 `crate::test_support`。

### 16.12 与第 15 节的状态对照

| 第 15 节问题 | 当前状态 | 本轮判断 |
|---|---|---|
| 产品路径仍无 source artifact | 已推进 | `macos_service.rs` 在 `feature = "ffmpeg"` 时接入 `FfmpegRecordingWriter`，但 writer 压力和时间基仍需整改 |
| `export_video()` 未 `spawn_blocking` | 已推进 | `src-tauri/src/lib.rs:791-810` 已使用 `spawn_blocking` |
| FFmpeg exporter smoke prototype | 已推进但未达生产 | tests 可通过，但音频 PTS、audio cut、crop/fit、cursor compositor 仍阻塞 |
| cut timeline / time base 风险 | 仍存在 | 尤其是 audio PTS 与 audio segment boundary |
| Native Safety Gate 未完成 | 仍存在 | FFmpeg frame/scaler/unsafe 仍需人工专项审查 |
| docs/checklist overclaim | 仍需收口 | 应把 “playable preset export prototype” 与 “完整 AI 美化导出” 区分清楚 |

### 16.13 建议整改顺序

#### R1: 先修 exporter 音频时间轴和音频裁剪边界

目标：

- AAC encoder 不再输出 `Queue input is backward in time`。
- audio packet/frame PTS 单调递增。
- 多段 keep segment 下，音频与视频使用同一裁剪语义。

任务：

1. decoded audio frame PTS 作为输入 PTS 来源。
2. 建立 `next_audio_out_pts`，按 sample count 前进。
3. audio packet / decoded frame 按 segment start/end skip 或 slice。
4. 修正 cut gap 映射到 audio encoder time base 的算法。
5. 增加 monotonic audio PTS 和 cut-range audio exclusion 测试。

#### R2: 实现 export preset 的 FitWithBars / CenterCrop

目标：

- 三个固定 preset 输出不仅尺寸正确，视觉比例也正确。

任务：

1. 抽出 crop/fit 纯函数。
2. Bilibili 使用 FitWithBars。
3. Douyin / Xiaohongshu 使用 CenterCrop。
4. 增加 synthetic geometry 测试，验证没有拉伸。

#### R3: 让基础 playable export 不依赖 cursor timeline 成功

目标：

- cursor metadata 缺失时仍能导出 MP4。
- payload 与 Rust request 的 optional 语义一致。

任务：

1. `ExportSummaryPayload.effect_timeline_path` 改为 `Option<String>`。
2. `build_cursor_effect_timeline()` 失败降级为 `None`。
3. 不复用 stale `last_effect_timeline_path()`。
4. 前端类型和 UI 状态同步 nullable。
5. 增加 cursor timeline unavailable 的 command/service 测试。

#### R4: 明确并处理 cursor effect compositor 交付口径

目标：

- 不静默忽略 `effect_timeline_path`。

任务：

1. 若本阶段不实现 compositor，更新 docs/checklist/UI 说明：当前是 playable preset export，不包含 cursor effect compositing。
2. 若本阶段必须实现，则补 timeline parser、video overlay/compositor、artifact-level visual tests。

#### R5: 降低 writer 对录制 consumer thread 的编码压力

目标：

- 长时间录制时 consumer 不被同步 encode/muxing 拖垮。

任务：

1. 引入 worker-backed writer。
2. 增加 bounded byte/frame queue。
3. 明确 backpressure/drop 策略并记录 telemetry。
4. 通过 1080p 10 分钟压力 Gate 后再默认启用。

#### R6: 收口测试、warnings、文档状态

目标：

- 自动化测试能覆盖真实风险，文档不 overclaim。

任务：

1. 清理 Phase 6 新增 clippy warnings。
2. 更新 `tests/phase-6-w11-w12-checklist.md`：区分自动化已通过、manual gates 未完成、Native Safety 未完成。
3. 更新 HANDOFF / plan 中关于 “完整导出 / AI 美化导出” 的状态表述。
4. 完成人工 Native Safety review。

### 16.14 建议补充测试清单

Rust / FFmpeg feature：

- `ffmpeg_exporter_audio_pts_are_monotonic`
- `ffmpeg_exporter_audio_pts_advance_by_sample_count`
- `ffmpeg_exporter_excludes_audio_from_cut_ranges`
- `ffmpeg_exporter_handles_audio_frame_crossing_keep_boundary`
- `ffmpeg_exporter_cut_timeline_keeps_audio_and_video_in_sync`
- `ffmpeg_exporter_douyin_center_crop_does_not_stretch_geometry`
- `ffmpeg_exporter_xiaohongshu_center_crop_does_not_stretch_geometry`
- `ffmpeg_exporter_bilibili_fit_with_bars_preserves_aspect_ratio`
- `export_video_succeeds_when_cursor_timeline_is_unavailable`
- `export_video_returns_null_effect_timeline_path_when_cursor_metadata_missing`
- `ffmpeg_exporter_with_effect_timeline_applies_cursor_overlay_or_returns_unsupported`
- `ffmpeg_writer_worker_queue_does_not_block_consumer_under_synthetic_load`
- `ffmpeg_writer_non_30fps_or_dropped_frames_duration_is_correct`

Rust / static and service checks：

- `rust_code_does_not_shell_out_to_ffmpeg_or_ffprobe`
- `phase6_code_does_not_depend_on_test_support_in_product_path`
- `export_summary_payload_effect_timeline_path_is_nullable`
- `export_video_does_not_reuse_stale_effect_timeline_path`

Manual Gates：

- 1080p 10 分钟录制，系统音频+麦克风开启，录制结束后 source artifact 可播放。
- 同一 source artifact 导出 Bilibili、Douyin、小红书三个 preset，视觉比例不拉伸。
- 有明显静音/非静音切换的录制，auto-trim 后视频和音频同时裁掉目标区间。
- A/V sync 人工检查偏移小于 100ms。
- cursor metadata 缺失或损坏时，基础 MP4 仍可导出。
- 取消导出后 partial output 被删除，original source artifact 保留。
- FFmpeg feature 开关关闭时，UI 给出清晰 Gate 状态，不误报导出成功。

### 16.15 当前建议对外状态表述

建议在整改前统一使用下面口径，避免 overclaim：

> Phase 6 本地授权边界、固定导出预设配置、FFmpeg binding writer/exporter smoke path 已推进；当前 Rust/TS 自动化和 build 可通过。但 FFmpeg playable export 仍有音频 PTS/裁剪、预设 crop/fit、cursor timeline 降级、cursor effect compositor、writer 压力与 Native Safety Gate 未完成，暂不建议合并为 Phase 6 完成。

## 17. Phase 6 FFmpeg playable export 整改后复审与运行问题定位（2026-05-31，HEAD `8986b4a` + worktree）

### 17.1 结论摘要

**Ready to merge: No。**

本轮针对用户反馈“已完成第 16 节整改任务”后的当前代码再次复审，并同步定位运行时现象：

- `npm run tauri dev` 后开始录制的录制中界面看不到画面、听不到声音。
- 停止录制后的预览/美化界面看不到画面、听不到声音。
- 美化界面显示 `视频编码尚未实现（FFmpeg 集成待完成）`。

本轮结论分三层：

1. **第 16 节部分问题已有推进**：
   - `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` 本轮通过。
   - 第 16 节记录过的 AAC `Queue input is backward in time` 警告本轮未再出现。
   - `export_video()` 的 FFmpeg 分支仍在 `spawn_blocking` 中执行。
   - cursor timeline 构建失败不再直接 `?` 传播成命令失败。
   - 三种 preset 的尺寸 smoke tests 可通过。
2. **运行时反馈的直接根因已经明确**：
   - `npm run tauri dev` 默认不启用 `ffmpeg` feature。
   - `src-tauri/Cargo.toml` 中 `default = []`，所以 dev app 走非 FFmpeg 分支。
   - `macos_service.rs` 在非 FFmpeg 构建中使用 `CountingRecordingWriter::new(None)`，停止录制返回 `outputPath: null`。
   - `preview-view.tsx` 在 `recordingResult.outputPath` 为空时渲染 fallback 文案，因此显示 `视频编码尚未实现（FFmpeg 集成待完成）`。
3. **即使启用 FFmpeg feature，Phase 6 仍不能作为完成态合并**：
   - 前端播放路径没有正确使用 Tauri `convertFileSrc()`，CSP/asset protocol 也未允许视频媒体加载。
   - writer 返回 source artifact 前未做真实可播放校验。
   - exporter seek time base、audio cut boundary、decoder flush、source/export audio validation 仍存在生产风险。
   - cursor effect timeline 仍未被 exporter 应用。
   - FFmpeg writer 仍同步压在录制 consumer thread 内。
   - Native Safety 与 manual gates 仍未完成。

### 17.2 本轮有效范围

本轮复审基于：

| 项目 | 内容 |
|---|---|
| 目标章节 | `## 16. Phase 6 FFmpeg playable export 再复审（2026-05-30，commit bec70aae / 当前 HEAD + worktree）` 的整改结果 |
| 当前 HEAD | `8986b4a8e7a059ecb19ac8527102749eca467ac1` |
| 当前分支 | `feat/architecture-planning` |
| Dirty worktree | 仅 `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 有未提交文档改动 |
| 复审方式 | 本地逐文件复核 + 独立 reviewer subagent 复核 + 自动化命令复核 |

重点文件：

- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lib.rs`
- `src-tauri/src/app/export_service.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/platform/macos_service.rs`
- `src-tauri/tests/ffmpeg_export.rs`
- `src/App.tsx`
- `src/components/preview-view.tsx`
- `src/lib/tauri.ts`
- `tests/phase-6-w11-w12-checklist.md`

### 17.3 本轮验证记录

本轮执行过的验证命令和结果：

| 命令 | 结果 | 备注 |
|---|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 | Rust formatting clean |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 | 199 tests passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` | 通过 | 219 unit tests + 6 integration tests passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` | 通过，有 warnings | 见 17.11 |
| `npm test -- --run` | 通过 | 51 tests passed |
| `npm run build` | 通过 | TypeScript + Vite build 通过 |
| `git diff --check` | 通过 | 无 whitespace error |

本轮 FFmpeg feature 测试输出里没有再观察到：

```text
[aac] Queue input is backward in time
```

但仍有 FFmpeg/libx264/AAC 常规编码日志，以及 clippy/test warning，例如：

```text
warning: unused variable: `result`
   --> tests/ffmpeg_export.rs:217:9
```

### 17.4 Critical 1: 默认 `npm run tauri dev` 未启用 FFmpeg，产品路径仍没有录制 artifact

位置：

- `src-tauri/Cargo.toml:42-44`
- `src-tauri/src/platform/macos_service.rs:223-233`
- `src/components/preview-view.tsx:297-313`

现象：

`src-tauri/Cargo.toml`：

```toml
[features]
default = []
ffmpeg = ["dep:ffmpeg-next"]
```

`macos_service.rs`：

```rust
#[cfg(feature = "ffmpeg")]
let writer: Box<dyn RecordingWriter> = {
    let output_path = crate::media::export_paths::original_recording_path();
    Box::new(crate::media::ffmpeg_writer::FfmpegRecordingWriter::new(output_path)?)
};
#[cfg(not(feature = "ffmpeg"))]
let writer: Box<dyn RecordingWriter> = Box::new(CountingRecordingWriter::new(None));
```

`preview-view.tsx`：

```tsx
{recordingResult?.outputPath ? (
  <video src={`asset://localhost/${recordingResult.outputPath}`} controls />
) : (
  <p className="text-xs opacity-40 mt-2">视频编码尚未实现（FFmpeg 集成待完成）</p>
)}
```

`npm run tauri dev` 不会自动传入 `--features ffmpeg`。因此当前 dev app 默认仍然使用 `CountingRecordingWriter::new(None)`，`stop_recording()` 返回的 `RecordingResult.outputPath` 是 `null`，前端进入 fallback 文案。

影响：

- 这是用户反馈“美化界面显示 FFmpeg 待完成”的直接根因。
- 默认 dev/product 路径不能产出 source artifact，也就无法进行可播放预览和真实导出。
- 当前文档若宣称“Phase 6 playable export 已在产品路径完成”，仍属于 overclaim。

建议修复：

1. 立即可用的开发运行方式：

   ```bash
   npm run tauri -- dev --features ffmpeg
   ```

2. 建议新增显式脚本，避免开发时误用默认无 FFmpeg 构建：

   ```json
   {
     "scripts": {
       "tauri:dev": "tauri dev",
       "tauri:dev:ffmpeg": "tauri dev --features ffmpeg"
     }
   }
   ```

3. UI 上区分两种状态：
   - 无 FFmpeg feature：明确显示“当前构建未启用 FFmpeg，无法生成可播放文件”。
   - FFmpeg feature 已启用但编码失败：显示具体编码/校验错误。
4. 在 Native Safety Gate 通过前，不建议把 `ffmpeg` 加入 `default` feature。
5. 如果暂时保持默认无 FFmpeg，Phase 6 状态应写为“FFmpeg feature build 下 smoke path 可运行，默认 dev/product 未启用”。

### 17.5 Critical 2: 即使有 `outputPath`，前端 video 播放路径也未正确接入 Tauri asset protocol

位置：

- `src/components/preview-view.tsx:297-300`
- `src-tauri/tauri.conf.json:24-25`

现象：

当前前端直接拼接：

```tsx
src={`asset://localhost/${recordingResult.outputPath}`}
```

但 Tauri 2 的推荐路径是使用：

```ts
import { convertFileSrc } from '@tauri-apps/api/core'
const url = convertFileSrc(recordingResult.outputPath)
```

同时当前 CSP 只有：

```json
"csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self' asset: https://asset.localhost; font-src 'self'"
```

这里没有 `media-src`，也没有在 `app.security.assetProtocol` 中启用 asset protocol 和 scope。

影响：

- 即使 `npm run tauri -- dev --features ffmpeg` 成功生成 MP4，`<video>` 仍可能被 URL 格式、CSP 或 asset scope 拦截。
- 这会表现为“有 outputPath，但预览仍黑屏/无声”。
- 该问题与 FFmpeg writer/exporter 是否产出真实文件相互独立，必须单独修。

建议修复：

1. 前端新增 helper：

   ```ts
   import { convertFileSrc } from '@tauri-apps/api/core'

   export function mediaFileSrc(path: string): string {
     return convertFileSrc(path)
   }
   ```

2. `preview-view.tsx` 使用转换后的 URL：

   ```tsx
   const sourceUrl = recordingResult?.outputPath
     ? convertFileSrc(recordingResult.outputPath)
     : null
   ```

3. `tauri.conf.json` 增加 media CSP 和 asset protocol scope。scope 应尽量限制到录制目录，例如 temp 下 `luzhi-recordings/**`，不要开放整个文件系统：

   ```json
   "security": {
     "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self' asset: http://asset.localhost https://asset.localhost; media-src 'self' asset: http://asset.localhost https://asset.localhost; font-src 'self'",
     "assetProtocol": {
       "enable": true,
       "scope": ["$TEMP/luzhi-recordings/**"]
     }
   }
   ```

4. 增加前端测试，验证有 `outputPath` 时调用 `convertFileSrc()`，而不是手写 `asset://localhost/...`。
5. 手动 Gate 增加：FFmpeg feature 构建下停止录制后 `<video>` 能播放、有声音、控制条能 scrub。

### 17.6 Critical 3: `FfmpegRecordingWriter::finish()` 返回 source artifact 前未校验真实可播放

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:404-435`
- `src-tauri/src/media/ffmpeg_common.rs:157-180`
- `src-tauri/src/app/export_service.rs:23-37`

现象：

`FfmpegRecordingWriter::finish()` 当前写 trailer 后直接返回：

```rust
Ok(RecordingResult {
    duration_secs,
    frame_count: self.frame_count,
    mixed_audio_chunk_count: self.mixed_audio_chunk_count,
    output_path: Some(self.output_path.to_string_lossy().to_string()),
    ...
})
```

但没有调用 `validate_source_artifact()` 或等价校验。`ExportService` 也只校验 source file 存在且非空：

```rust
if !input_path.exists() { ... }
if input_path.metadata().map(|metadata| metadata.len()).unwrap_or(0) == 0 { ... }
```

影响：

- 零帧、坏容器、缺视频流、缺音频流、零时长等 artifact 可能被当作有效 source。
- UI 会拿到 `outputPath` 并尝试播放，exporter 也会尝试以此作为输入。
- 这违反计划中“`outputPath` 只在真实非空可播放文件存在时返回”的约束。

建议修复：

1. `FfmpegRecordingWriter::finish()` 在返回前调用：

   ```rust
   let inspection = crate::media::ffmpeg_common::validate_source_artifact(&self.output_path)?;
   ```

2. `validate_source_artifact()` 应至少要求：
   - file size > 0
   - has video stream
   - has audio stream 或明确允许 silent AAC 生成失败时返回错误
   - duration > 0
3. 对 `frame_count == 0` 的 writer finish：
   - 要么返回 `output_path: None`
   - 要么返回 `RecordingWriteFailed`
   - 不应返回可播放 source path
4. `ExportService` 在 FFmpeg feature 下可以进一步调用 source artifact inspection，而不只是 exists/non-empty。
5. 增加测试：
   - `ffmpeg_writer_zero_frame_does_not_return_playable_output_path`
   - `ffmpeg_writer_finish_validates_source_artifact`
   - `export_service_rejects_source_without_video_stream`
   - `export_service_rejects_source_without_audio_stream`

### 17.7 Critical 4: `trim_exporter` seek 使用了错误 time base，长视频/多段裁剪可能偏位

位置：

- `src-tauri/src/media/trim_exporter.rs:445-497`
- `ffmpeg-next 7.1.0 src/format/context/input.rs:124`

现象：

当前 exporter 先把 segment 边界转换到 video stream time base：

```rust
let seg_start_in_vtb =
    nanos_to_time_base_units(seg_start_nanos as u64, video_time_base).unwrap_or(0);
let seg_end_in_vtb =
    nanos_to_time_base_units(seg_end_nanos as u64, video_time_base).unwrap_or(0);

input.seek(seg_start_in_vtb, ..seg_end_in_vtb)?;
```

但 ffmpeg-next 的 `Input::seek()` 内部实现是：

```rust
avformat_seek_file(
    self.as_mut_ptr(),
    -1,
    range.start().cloned().unwrap_or(i64::MIN),
    ts,
    range.end().cloned().unwrap_or(i64::MAX),
    0,
)
```

`stream_index = -1` 时，seek timestamp 使用 AV_TIME_BASE 单位，而不是 video stream time base。

影响：

- 例如 video time base 是 `1/30` 时，2 秒会被转换成 60；但 AV_TIME_BASE 中 2 秒应是 2,000,000。
- 对短 synthetic fixture，packet skip 可能掩盖问题；对真实长录制，seek 可能落到错误位置，导致大量解码/跳过、裁剪不准或格式相关行为。
- 多段 keep segment 的性能与正确性都会受影响。

建议修复：

1. seek 参数改为 AV_TIME_BASE 单位：

   ```rust
   const AV_TIME_BASE: i64 = 1_000_000;
   let seg_start_avtb = seg_start_nanos / 1_000;
   let seg_end_avtb = seg_end_nanos / 1_000;
   input.seek(seg_start_avtb, ..seg_end_avtb)?;
   ```

2. seek 后 flush decoder/resampler 状态：
   - video decoder flush
   - audio decoder flush
   - audio resampler flush/drain
3. 保留 packet/frame 层 start/end 判断，seek 只负责定位到附近。
4. 增加长 source 多段 cut test，确保第二段 keep 不从错误位置开始。
5. 如果 ffmpeg-next 暴露 stream-indexed seek API，可优先使用对应 stream 的 time base，并明确封装 helper，避免混用。

### 17.8 Critical 5: 音频裁剪仍未覆盖 decoded frame 跨 keep 边界场景

位置：

- `src-tauri/src/media/trim_exporter.rs:512-530`
- `src-tauri/src/media/trim_exporter.rs:791-838`

现象：

当前 packet 层有 segment 边界判断：

```rust
if pkt_stream == video_stream_index && pkt_ts > seg_end_in_vtb { break; }
if pkt_stream == input_audio_stream_index.unwrap_or(usize::MAX) && pkt_ts > seg_end_in_atb { break; }
if pkt_stream == video_stream_index && pkt_ts < seg_start_in_vtb { continue; }
if pkt_stream == input_audio_stream_index.unwrap_or(usize::MAX) && pkt_ts < seg_start_in_atb { continue; }
```

但 decoded audio frame 可能覆盖一个时间范围，而不是一个点。当前逻辑没有根据 decoded frame 的 `[start, end)` 与 keep segment 的交集来 slice 或 drop，只是：

```rust
let num_samples = resampled.samples() as i64;
resampled.set_pts(Some(next_audio_out_pts));
next_audio_out_pts += num_samples;
audio_encoder.send_frame(&resampled)?;
```

影响：

- cut 区间边缘的音频可能泄漏到输出中。
- 多段 keep 下，decoder/resampler buffer 可能携带上一段尾部数据进入下一段。
- 现有 integration test 只验证总时长变短、输出有流、有尺寸，无法证明“被裁掉的音频内容不存在”。

建议修复：

1. 对 decoded audio frame 计算输入时间范围：
   - start = decoded PTS in input audio time base
   - duration = samples / sample_rate
   - end = start + duration
2. 对 frame 与 keep segment 做交集：
   - 完全在 keep 外：drop
   - 完全在 keep 内：encode
   - 跨边界：slice 到 keep 内，或保守 drop 并在测试中锁定语义
3. 每个 segment seek 后 flush decoder/resampler，避免跨段 buffer 泄漏。
4. 增加 synthetic fixture：cut 区间中放明显音频 tone，导出后检测该 tone 不存在。
5. 增加 packet/frame PTS 单调检查：
   - audio packet DTS/PTS monotonic
   - audio frame PTS 按 sample count 前进
   - video/audio kept duration 差值在可接受范围内

### 17.9 Important 1: cursor timeline 失败降级后仍可能复用 stale effect timeline

位置：

- `src-tauri/src/lib.rs:701-707`
- `src-tauri/src/lib.rs:723-731`

现象：

`export_video()` 当前会把 cursor timeline 构建失败降级成：

```rust
let cursor = build_cursor_effect_timeline(app.clone(), state.clone())
    .await
    .unwrap_or(CursorEffectSummaryPayload {
        frame_count: 0,
        click_effect_count: 0,
        effect_timeline_path: None,
    });
```

但后续传给 exporter 的 effect path 不是来自 `cursor.effect_timeline_path`，而是：

```rust
let effect = service.last_effect_timeline_path().map(PathBuf::from);
```

影响：

- 当前录制的 cursor build 失败时，仍可能传入上一次成功 build 留下的旧 timeline。
- `ExportSummaryPayload.effect_timeline_path` 可能是 `None`，但实际 exporter request 带了 stale path，边界语义不一致。

建议修复：

1. exporter request 使用当前 cursor summary：

   ```rust
   let effect_timeline = cursor
       .effect_timeline_path
       .as_ref()
       .map(PathBuf::from);
   ```

2. cursor build 失败时清理或忽略 service 中的 stale `last_effect_timeline_path`。
3. 增加测试：
   - `export_video_does_not_reuse_stale_effect_timeline_path`
   - `export_video_returns_null_effect_timeline_path_when_cursor_build_fails`

### 17.10 Important 2: `effect_timeline_path` 仍未被 FFmpeg exporter 应用

位置：

- `src-tauri/src/media/trim_exporter.rs:52`
- `src-tauri/src/media/trim_exporter.rs:120-956`

现象：

`TrimExportRequest` 已有：

```rust
pub effect_timeline_path: Option<PathBuf>,
```

但 `FfmpegTrimExporter::export()` 没有解析该 JSON，也没有在 video frame 上合成 cursor magnification、smoothing、click overlay。

影响：

- 当前产物最多是“playable preset export”，不是完整“AI 美化导出”。
- UI 中“光标放大 / 光标平滑”看起来会影响导出，但导出结果不会体现这些 cursor effects。

建议修复：

1. 若 Phase 6 只要求 playable preset export：
   - 文档、checklist、UI 文案明确“当前导出不包含 cursor effect compositing”。
   - 不要在完成态描述中写“完整 AI 美化导出”。
2. 若 Phase 6 必须包含 cursor effect：
   - 实现 effect timeline parser。
   - 在 exporter video loop 中做 overlay/compositor。
   - 增加 visual artifact test，验证 overlay 可观察。
   - 单独做 Native Safety / performance review。

### 17.11 Important 3: export/source validation 未强制音频流

位置：

- `src-tauri/src/media/ffmpeg_common.rs:123-155`
- `src-tauri/src/media/ffmpeg_common.rs:157-180`

现象：

`validate_export_artifact()` 当前校验：

- file size > 0
- has video stream
- dimensions match expected preset
- duration > 0

但没有强制：

- has audio stream

计划和实现意图都写了“无音频录制时生成 silent AAC track，保持 MP4 artifact 始终有 audio stream”。如果 validation 不检查音频流，这个保证可能悄悄失效。

建议修复：

1. `validate_export_artifact()` 增加：

   ```rust
   if !inspection.has_audio_stream {
       return Err(AppError::ExportFailed {
           reason: "导出文件缺少音频流".to_string(),
       });
   }
   ```

2. `validate_source_artifact()` 同样按产品要求决定是否强制 audio stream。
3. 增加测试：
   - `validate_export_artifact_rejects_missing_audio_stream`
   - `ffmpeg_writer_without_audio_generates_silent_track`
   - `ffmpeg_exporter_source_without_audio_generates_silent_track`

### 17.12 Important 4: writer 对非 48kHz 输入只改 PTS，不重采样 samples

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:366-401`

现象：

当前 `push_audio()`：

```rust
let pts_increment = if chunk.sample_rate > 0 && chunk.sample_rate != 48000 {
    (num_frames as i64 * 48000) / chunk.sample_rate as i64
} else {
    num_frames as i64
};
self.audio_pts += pts_increment;
```

这里把 PTS 递增换算到了 48kHz，但实际 samples 没有重采样，仍按原 sample data 直接送给 48kHz AAC encoder。

需要注意：当前 `SimpleAudioMixer` 设计上会输出 48kHz stereo，因此真实产品路径通常不会触发非 48kHz mixed chunk。但 writer trait 是公共边界，测试 helper 里也会生成 44.1kHz chunk，因此这里仍是潜在 bug。

建议修复：

1. 最小修复：writer boundary 明确只接受 48kHz stereo mixed audio：

   ```rust
   if chunk.sample_rate != 48_000 || chunk.channels != 2 {
       return Err(AppError::RecordingWriteFailed {
           reason: "FFmpeg writer 仅接受 48kHz stereo mixed audio".to_string(),
       });
   }
   ```

2. 或补 `SwrContext`，像 exporter 一样把任意输入重采样到 F32P stereo 48kHz。
3. 测试 helper 应使用 48kHz stereo，避免用测试数据掩盖真实边界。

### 17.13 Important 5: FFmpeg writer 仍同步运行在录制 consumer thread

位置：

- `src-tauri/src/platform/macos_service.rs:234-245`
- `src-tauri/src/media/ffmpeg_writer.rs:271-401`

现象：

FFmpeg writer 虽然没有在 SCK callback 中直接编码，但它在 `consume_frames()` 所在线程中同步执行：

- BGRA copy
- swscale
- H.264 encode
- AAC encode
- mux write

影响：

- consumer thread 被编码拖慢时，bounded media channel 会开始堆积或丢帧。
- 1080p 长录制、4K 预留、系统音频+麦克风同时开启、CPU 压力下风险更明显。
- 计划中提到的 worker-backed bounded queue 仍未完成。

建议修复：

1. `FfmpegRecordingWriter` 改成 worker-backed：
   - public `push_video/push_audio` 只做轻量 enqueue。
   - FFmpeg encode/mux 在独立 worker thread 中执行。
2. 队列必须有 byte budget 或 frame budget。
3. backpressure 策略需要可观测：
   - drop late frames
   - 记录 dropped count
   - 返回 fatal error 或降级状态
4. Native Safety Gate 前，不建议让 FFmpeg writer 成为默认产品构建。
5. Manual Gate 必须覆盖 1080p 10 分钟录制压力。

### 17.14 Minor: 本轮仍有 Phase 6 新 warning 与 checklist 状态不一致

本轮 `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` 通过但仍有 warnings。

与 Phase 6 新代码直接相关：

- `src/app/export_service.rs:12`：`too_many_arguments`
- `src/app/license_service.rs:168`：manual `div_ceil`
- `src/media/trim_exporter.rs:603`、`:671`、`:731`、`:736`：manual checked division
- `src-tauri/tests/ffmpeg_export.rs:217`：unused variable `result`

既有或平台相关 warning：

- `src/platform/macos/screen_capture_kit.rs` 多个 FFI 命名/unused warning
- `src/platform/macos_service.rs:400` private interface warning
- `src/platform/macos/cpal_microphone.rs:20` `SendStream` field never read

建议：

1. 先清理 Phase 6 新增 warnings，避免后续媒体链路 warning 被淹没。
2. `export_service::export_recording_with_timeline()` 参数继续增长时，引入 request struct。
3. `tests/phase-6-w11-w12-checklist.md` 仍需更新为真实状态：
   - FFmpeg feature 自动化 smoke tests 本机已通过。
   - manual gates 未完成。
   - Native Safety 未完成。
   - 默认 dev/product 未启用 FFmpeg。

### 17.15 运行问题定位：为什么现在看不到画面/听不到声音

#### 17.15.1 录制中界面没有画面/声音

位置：

- `src/App.tsx:312-337`

当前 recording state 渲染的是占位 UI：

```tsx
<div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-border/30 flex items-center justify-center">
  <div className="text-center text-muted-foreground">
    <p className="text-sm mb-1">正在录制...</p>
    <p className="text-xs opacity-60">此区域表示被录制的屏幕内容</p>
  </div>
</div>
```

所以“开始录制后看不到实时画面/听不到实时声音”不是 FFmpeg exporter 的直接问题，而是 live preview 功能尚未实现。

同时项目架构红线明确：原始音视频帧流不得进入 React/TypeScript 层。因此不能简单把 SCK frames/audio samples 传给前端来预览。若要做实时预览，需要另行设计 native preview path，例如：

- Rust/native 侧生成低频轻量 preview thumbnail。
- 或使用原生渲染 layer / 单独 preview surface。
- 或只显示录制状态、计时、麦克风电平，不承诺实时视频预览。

#### 17.15.2 停止录制后美化界面显示 FFmpeg 待完成

直接链路：

1. 用户运行 `npm run tauri dev`。
2. Tauri dev 未带 `--features ffmpeg`。
3. `Cargo.toml default = []`。
4. `macos_service.rs` 使用 `CountingRecordingWriter::new(None)`。
5. `stop_recording()` 返回 `RecordingResult.outputPath = None`。
6. `App.tsx` 把 `outputPath` 存成 `null`。
7. `preview-view.tsx` 看到 `recordingResult?.outputPath` 为空，渲染 fallback：

   ```text
   视频编码尚未实现（FFmpeg 集成待完成）
   ```

#### 17.15.3 即使用 FFmpeg feature 后仍可能黑屏/无声

如果改用：

```bash
npm run tauri -- dev --features ffmpeg
```

仍需同时修：

1. `preview-view.tsx` 使用 `convertFileSrc()`。
2. `tauri.conf.json` 启用 asset protocol 和 media CSP。
3. writer finish 前校验 artifact 可播放。
4. 手动打开生成的 `/tmp/luzhi-recordings/*.mp4` 验证 VLC/IINA/QuickTime 可播放。

### 17.16 BUG.md、架构红线与安全复核

本轮未发现新增违反 BUG.md 预防规则：

- 未发现新增透明区域点击穿透相关改动。
- 未发现新增全窗口 `setIgnoreCursorEvents(true)` 回归。
- 未发现用容器级 `{false}` 阻断拖拽区域。
- 未发现 `motion.div whileTap` 直接包裹 Button 的 BUG-003 模式新增回归。

本轮未发现以下架构/安全红线回归：

- 未发现生产代码调用 `std::process::Command` 执行 `ffmpeg` / `ffprobe`。
- 未发现 FFmpeg CLI 参数拼接用户输入。
- 未发现原始音视频帧流进入 React / TypeScript 层。
- 未发现 hardcoded activation secret / private key。
- 未发现字幕、摘要、模板系统、团队协作、平台发布 API 等 MVP 禁区功能。

仍需注意：

- 手动 Gate 中可使用 VLC/IINA/ffprobe 辅助人工验收，但生产代码和自动化核心逻辑应继续使用 FFmpeg binding inspection。
- `test_support` 在 `feature = "ffmpeg"` 下会被编入 lib，应避免任何产品路径依赖 `crate::test_support`。
- FFmpeg unsafe / frame plane copy / scaler / resampler / encoder flush 仍需 Native Safety 专项审查。

### 17.17 建议整改顺序

#### R0: 先修运行入口和播放路径

目标：

- 用户能明确知道当前构建是否启用 FFmpeg。
- FFmpeg feature 构建下，停止录制后的 preview `<video>` 能加载本地 MP4。

任务：

1. 新增 `tauri:dev:ffmpeg` 脚本或文档说明 `npm run tauri -- dev --features ffmpeg`。
2. 非 FFmpeg 构建 UI 显示明确 Gate 信息，不使用“编码尚未实现”这种容易误导的历史文案。
3. `preview-view.tsx` 改用 `convertFileSrc()`.
4. `tauri.conf.json` 增加 `media-src` 和 asset protocol scope。
5. 增加前端测试和手动播放 Gate。

#### R1: 收紧 source/export artifact validation

目标：

- `outputPath` 只在真实可播放 artifact 存在时返回。

任务：

1. writer finish 调用 `validate_source_artifact()`。
2. validation 要求 audio stream。
3. zero-frame writer 不得返回 playable output path。
4. ExportService 在 FFmpeg feature 下拒绝坏 source。

#### R2: 修正 exporter seek time base 和 decoder flush

目标：

- 多段裁剪定位正确，不依赖 packet skip 掩盖 seek 偏位。

任务：

1. `Input::seek()` 改用 AV_TIME_BASE 单位。
2. 每段 seek 后 flush video/audio decoder。
3. flush/drain resampler，防止上一段音频进入下一段。
4. 增加长视频多段 keep 测试。

#### R3: 完整处理 audio frame cut boundary

目标：

- 被 cut 区间的音频不会泄漏。
- audio PTS 单调且按 samples 前进。

任务：

1. 按 decoded frame range 判断 keep/cut。
2. 跨边界 frame slice 或保守 drop。
3. 增加 tone fixture 检查 cut-range audio exclusion。
4. 增加 audio/video duration sync 测试。

#### R4: 明确 cursor effect 交付口径

目标：

- 不静默忽略 `effect_timeline_path`。

任务：

1. 若暂不实现 compositor，更新 UI/docs/checklist 为“可播放 preset 导出，不含光标视觉重绘”。
2. 若实现 compositor，单独做 timeline parser、overlay、visual artifact tests 和 Native Safety review。

#### R5: 录制 writer worker 化

目标：

- FFmpeg 编码压力不拖垮录制 consumer。

任务：

1. `FfmpegRecordingWriter` 改为 worker-backed。
2. byte/frame budgeted queue。
3. 明确 drop/backpressure/telemetry。
4. 完成 1080p 10 分钟压力 Gate 后再讨论默认启用 FFmpeg。

#### R6: 文档、checklist、warnings 收口

目标：

- 后续整改优先级不会被过时文档误导。

任务：

1. 更新 `tests/phase-6-w11-w12-checklist.md` 当前状态。
2. 更新 `HANDOFF.md`：默认 dev 未启用 FFmpeg、manual gates 未完成、Native Safety 未完成。
3. 清理 Phase 6 新增 clippy warnings。
4. 保留第 15/16/17 节历史差异，避免把旧风险误标成已完成。

### 17.18 建议补充测试清单

Rust / command & feature gate：

- `non_ffmpeg_gate_summary_never_fakes_output`
- `default_dev_build_uses_non_ffmpeg_gate_or_explicit_warning`
- `export_video_without_ffmpeg_never_instantiates_mock_exporter_in_product_path`
- `export_video_does_not_reuse_stale_effect_timeline_path`

Rust / writer：

- `ffmpeg_writer_finish_validates_source_artifact`
- `ffmpeg_writer_zero_frame_does_not_return_output_path`
- `ffmpeg_writer_rejects_non_48khz_audio_or_resamples_it`
- `ffmpeg_writer_without_audio_generates_valid_silent_aac_track`
- `ffmpeg_writer_worker_queue_does_not_block_consumer_under_synthetic_load`

Rust / exporter：

- `ffmpeg_exporter_seek_uses_av_time_base_for_input_seek`
- `ffmpeg_exporter_flushes_decoders_after_segment_seek`
- `ffmpeg_exporter_audio_pts_are_monotonic`
- `ffmpeg_exporter_audio_frame_crossing_keep_boundary_is_sliced_or_dropped`
- `ffmpeg_exporter_excludes_audio_from_cut_ranges`
- `ffmpeg_exporter_cut_timeline_keeps_audio_and_video_in_sync`
- `validate_export_artifact_rejects_missing_audio_stream`
- `ffmpeg_exporter_douyin_center_crop_does_not_stretch_geometry`
- `ffmpeg_exporter_xiaohongshu_center_crop_does_not_stretch_geometry`
- `ffmpeg_exporter_bilibili_fit_with_bars_preserves_aspect_ratio`

Frontend：

- `preview_uses_convert_file_src_for_recording_output`
- `preview_shows_ffmpeg_feature_gate_copy_when_output_path_missing`
- `preview_video_renders_when_recording_result_has_output_path`
- `export_success_message_includes_output_path_only_when_present`

Manual Gates：

- 默认 `npm run tauri dev`：录制后 UI 显示明确 FFmpeg feature gate，不误报可播放导出。
- `npm run tauri -- dev --features ffmpeg`：录制后生成 `/tmp/luzhi-recordings/*.mp4`。
- 录制后预览 `<video>` 能播放画面和声音。
- 原始 source artifact 用 VLC/IINA/QuickTime 可播放。
- 三种 preset 输出尺寸正确且视觉比例不拉伸。
- 有明显静音/非静音切换的录制，auto-trim 后音频和视频同时裁掉目标区间。
- 取消导出后 partial output 删除，source artifact 保留。
- 1080p 10 分钟录制压力，系统音频+麦克风开启，A/V sync 偏移小于 100ms。
- Native Safety Gate 完成并记录审查结论。

### 17.19 当前建议对外状态表述

建议整改前统一使用下面口径：

> Phase 6 已完成本地授权边界、固定导出预设配置，以及 FFmpeg feature 构建下的 writer/exporter smoke path；自动化 Rust/TS/build 当前可通过。但默认 `npm run tauri dev` 未启用 FFmpeg，因此不会生成可播放录制 artifact。停止录制后预览显示 “FFmpeg 集成待完成” 的直接原因是 `outputPath = null`。即使启用 FFmpeg feature，前端本地视频播放还需补 `convertFileSrc()`、asset protocol scope 与 `media-src` CSP。writer/exporter 仍需补 source/export 可播放校验、seek time base 修正、audio cut boundary、cursor compositor 交付口径、worker-backed writer、manual gates 与 Native Safety review。因此暂不建议合并为 Phase 6 完成态。

## 18. Phase 6 FFmpeg playable export 第 17 节整改后复审（2026-05-31，HEAD `8986b4a` + worktree）

### 18.1 结论摘要

**Ready to merge: No。**

本轮针对用户反馈“已完成 `## 17. Phase 6 FFmpeg playable export 整改后复审与运行问题定位` 章节的整改任务”后的当前 worktree 进行复审。第 17 节中若干运行入口和播放路径问题已有明显推进，但 Phase 6 FFmpeg export 仍存在会直接导致 BUG-004 的 Critical 媒体时间戳问题。

本轮最关键的新证据来自真实本地 artifact：

- source: `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/recording-1780214406410-0.mp4`
- export: `/private/var/folders/h8/2vjd0nlx79q4m4q97xcdgh080000gn/T/luzhi-recordings/recording-1780214406410-0-bilibili-export-1.mp4`

`ffprobe` 结果显示：

| 文件 | 视频时长 | 视频帧数 | 视频 `time_base` | 音频时长 | 容器时长 | 结论 |
|---|---:|---:|---|---:|---:|---|
| source | `16.700000s` | `502` | `1/15360` | `19.733000s` | `19.733000s` | source 已存在 A/V duration drift |
| export | `0.032552s` | `501` | `1/15360` | `19.008000s` | `19.008000s` | export 视频 PTS 被压缩到约 0.03s，直接解释黑屏/第一帧现象 |

export video packet 旁证：

```text
0.000000,0.000000,0.000065,K__
0.000065,0.000065,0.000065,___
0.000130,0.000130,0.000065,___
```

source video packet 对照：

```text
0.000000,0.000000,0.033333,K__
0.033333,0.033333,0.033333,___
0.066667,0.066667,0.033333,___
```

这说明当前 export 的每帧 duration 是 `1/15360s`，而不是预期的 `1/30s`。

### 18.2 本轮有效范围

本轮复审覆盖第 17 节整改后的当前 dirty worktree，重点文件：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lib.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/test_support/ffmpeg_helpers.rs`
- `src-tauri/tests/ffmpeg_export.rs`
- `src/App.test.tsx`
- `src/components/preview-view.tsx`
- `BUG.md`

本轮未修改代码，只追加审查结论。

### 18.3 本轮验证记录

| 命令 | 结果 | 备注 |
|---|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | **失败** | 新增/整改 Rust 代码未 rustfmt |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 | `199` tests passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` | 通过 | `220` unit tests + `8` integration tests passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` | 通过，有 warnings | 仍有 Phase 6 新 warning |
| `cargo build --manifest-path src-tauri/Cargo.toml --features ffmpeg` | 通过，有 warnings | build 可过但不是 clean |
| `npm test -- --run` | 通过 | `51` tests passed |
| `npm run build` | 通过 | Vite/TypeScript build 通过 |
| `git diff --check` | 通过 | 无 whitespace error |
| `ffprobe` 检查真实 source/export artifact | 暴露 Critical 问题 | export 视频流仅 `0.032552s` |

`cargo fmt --check` 失败示例：

- `src-tauri/src/lib.rs`：`effect_timeline` 可被 rustfmt 压成一行。
- `src-tauri/src/media/ffmpeg_common.rs`：`ffmpeg_error_to_recording_error()` match arm 格式不符合 rustfmt。
- `src-tauri/src/media/ffmpeg_writer.rs`：多处链式调用和 resize/extend 格式不符合 rustfmt。

`clippy --features ffmpeg --all-targets` 中和 Phase 6 直接相关的新增 warning：

- `src/media/trim_exporter.rs:462`：`AV_TIME_BASE_I64` unused。
- `src/app/export_service.rs:12`：`too_many_arguments`。
- `src/media/trim_exporter.rs:626/694/754/759`：manual checked division。
- `src-tauri/tests/ffmpeg_export.rs:217/282/346`：unused `result`。
- `src-tauri/tests/ffmpeg_export.rs:305/368`：manual `abs_diff`。

### 18.4 已确认推进项

#### 已推进 1：FFmpeg dev 入口显式化

位置：

- `package.json`

当前新增：

```json
"tauri:dev": "tauri dev",
"tauri:dev:ffmpeg": "tauri dev --features ffmpeg"
```

评价：

- 这能解决第 17 节里“开发时容易误用默认非 FFmpeg 构建”的直接使用问题。
- 保持 `ffmpeg` 不进 default feature 仍是合理的，因为 Native Safety/manual gates 尚未完成。

仍需注意：

- 文档/checklist/HANDOFF 仍应明确默认 `npm run tauri dev` 不产出可播放 source artifact。

#### 已推进 2：Tauri asset protocol 播放路径基本接上

位置：

- `src/components/preview-view.tsx`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src/App.test.tsx`

当前新增：

- `preview-view.tsx` 使用 `convertFileSrc(recordingResult.outputPath)`。
- `Cargo.toml` 给 `tauri` 增加 `protocol-asset` feature。
- `tauri.conf.json` 增加 `media-src` 和 `assetProtocol.scope = ["$TEMP/luzhi-recordings/**"]`。
- 前端 test mock 了 `convertFileSrc()`。

评价：

- 方向正确，符合 Tauri 2 API 文档要求。
- scope 限制在 `$TEMP/luzhi-recordings/**`，没有开放整个文件系统，安全口径可接受。

仍需补充：

- 手动 Gate：`npm run tauri:dev:ffmpeg` 下停止录制后 `<video>` 能播放真实 source artifact。
- 前端测试目前只 mock `convertFileSrc`，没有显式断言它被调用。建议加 `preview_uses_convert_file_src_for_recording_output`。

#### 已推进 3：source/export artifact validation 增加音频流要求

位置：

- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/lib.rs`

当前行为：

- `validate_export_artifact()` 要求 audio stream。
- `validate_source_artifact()` 要求 audio stream。
- `FfmpegRecordingWriter::finish()` 在返回 `output_path` 前调用 `validate_source_artifact()`。
- `export_video()` 成功后调用 `validate_export_artifact()`。

评价：

- 这修复了第 17 节 Important 3 的一部分。
- 但当前 validation 仍不足以拦截 BUG-004 的真实坏文件，见 Critical 3。

#### 已推进 4：stale effect timeline 复用风险已有修正

位置：

- `src-tauri/src/lib.rs`

当前逻辑：

```rust
let effect_timeline = cursor.effect_timeline_path.as_ref().map(PathBuf::from);
```

评价：

- 已不再从 `service.last_effect_timeline_path()` 读取旧路径，避免 cursor build 失败后复用上一轮 stale timeline。
- 这修复了第 17 节 Important 1 的主要问题。

仍需注意：

- `effect_timeline_path` 仍未被 FFmpeg exporter 应用，导出结果仍不包含 cursor overlay/compositing。

#### 已推进 5：writer 对非 48kHz 输入不再静默改 PTS

位置：

- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/test_support/ffmpeg_helpers.rs`

当前行为：

- `push_audio()` 拒绝 `sample_rate != 48_000`。
- 测试 helper 改为生成 `48_000Hz stereo` audio chunk。

评价：

- 这避免了“samples 未重采样但 PTS 被换算到 48kHz”的旧问题。
- 但 writer 仍未严格 enforce stereo，见 Important 2。

### 18.5 Critical 1: export 视频 packet 未 rescale 到 muxer stream time base，直接导致黑屏/第一帧问题

位置：

- `src-tauri/src/media/trim_exporter.rs:803-807`
- `src-tauri/src/media/trim_exporter.rs:940-945`

当前代码：

```rust
while video_encoder.receive_packet(&mut enc_pkt).is_ok() {
    enc_pkt.set_stream(video_out_idx);
    enc_pkt.rescale_ts(video_enc_tb, video_enc_tb);
    enc_pkt.write_interleaved(&mut output)?;
}
```

问题：

- `video_enc_tb` 是 encoder time base，当前设为 `Rational(1, out_fps)`，即 `1/30`。
- `write_header()` 之后，FFmpeg muxer 可能把 output stream time base 改成 MP4 常用 time scale，例如真实 artifact 里是 `1/15360`。
- 当前 `rescale_ts(video_enc_tb, video_enc_tb)` 等于没有转换。
- 因此 packet PTS/duration `0,1,2...` 被 muxer 当成 `1/15360` 单位写入。
- 结果是 501 帧视频总时长只有 `501 / 15360 ≈ 0.0326s`。

真实 artifact 证据：

```json
{
  "streams": [
    {
      "codec_type": "video",
      "time_base": "1/15360",
      "duration": "0.032552",
      "nb_frames": "501"
    },
    {
      "codec_type": "audio",
      "time_base": "1/48000",
      "duration": "19.008000",
      "nb_frames": "892"
    }
  ],
  "format": {
    "duration": "19.008000"
  }
}
```

影响：

- 这是 BUG-004 “只有封面第一帧有画面，从第二帧开始黑屏”的直接代码根因。
- 播放器按容器/audio duration 播放约 19 秒，但视频帧在 0.03 秒内全部耗尽，后续只能停留/黑屏。
- 这也解释了 `ffprobe` 里 export `avg_frame_rate` 异常高的问题。

建议修复：

1. `output.write_header()` 后读取真正的 muxer stream time base：

   ```rust
   let video_out_tb = output.stream(video_out_idx).unwrap().time_base();
   let audio_out_tb = output.stream(audio_out_idx).unwrap().time_base();
   ```

2. 所有 video encoded packet 写入前使用：

   ```rust
   enc_pkt.rescale_ts(video_enc_tb, video_out_tb);
   ```

3. 所有 audio encoded packet 写入前使用：

   ```rust
   enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb);
   ```

   当前 audio output stream time base 多数情况下仍是 `1/48000`，但也不应假设永远不变。

4. `ffmpeg_writer.rs` 同理建议在 `write_header()` 后缓存真实 stream time base，避免未来 muxer 改写后踩同类问题。

建议补测：

- `ffmpeg_exporter_video_stream_duration_matches_container_duration`
- `ffmpeg_exporter_video_packet_duration_is_about_1_over_fps`
- `validate_export_artifact_rejects_video_audio_duration_drift`
- integration test 对真实 output 执行 stream-level inspection：
  - video duration > `expected * 0.9`
  - abs(video duration - audio duration) < `100ms` 或产品认可阈值
  - video avg fps 在 `30 ± 1` 范围

### 18.6 Critical 2: source writer 仍按固定帧序号编码 PTS，真实录制 source 已出现 A/V duration drift

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:328-334`
- `src-tauri/src/media/ffmpeg_writer.rs:512-516`
- `src-tauri/src/platform/macos_service.rs:622-624`

当前代码：

```rust
self.video_duration_nanos = frame.timestamp.nanos;

let mut output_frame = frame::Video::new(Pixel::YUV420P, 1920, 1080);
output_frame.set_pts(Some(self.video_frame_index));
self.video_frame_index += 1;
```

问题：

- writer 的 `duration_secs` 使用最后一个 frame timestamp。
- 但 encoded video PTS 完全忽略真实 `frame.timestamp`，只按 `0,1,2...` 固定 30fps 递增。
- 真实录制如果由于 SCK callback 抖动、consumer thread 编码压力、channel drop 或系统负载导致视频帧数不足，source video duration 会变短。

真实 source artifact 证据：

```json
{
  "streams": [
    {
      "codec_type": "video",
      "duration": "16.700000",
      "nb_frames": "502"
    },
    {
      "codec_type": "audio",
      "duration": "19.733000",
      "nb_frames": "926"
    }
  ],
  "format": {
    "duration": "19.733000"
  }
}
```

影响：

- 即使修复 exporter rescale，source 已存在约 3 秒 A/V drift。
- auto-trim off 时 `CutTimeline::empty(duration_nanos)` 可能使用 trim metadata 的 `19s`，但 source video 实际只有 `16.7s`。
- 后续 exporter 以 source video stream 为主处理时，末尾画面不足、音频仍继续，会造成黑屏/停帧/音画不同步。

建议修复方向：

方案 A：按真实 timestamp 写 video PTS。

```rust
let pts = nanos_to_time_base_units(frame.timestamp.nanos, Rational(1, 30))?;
output_frame.set_pts(Some(pts.max(last_video_pts + 1)));
```

但该方案会在输入帧间隔大于 1/30 时产生时间间隔，播放器可正常表现停帧/低帧率。

方案 B：录制 writer 维持 CFR 30fps，补齐重复帧。

- 对真实 frame timestamp 做 30fps timeline quantize。
- 如果当前 frame 与上次 frame 间隔超过 1 帧，重复上一帧补齐中间 PTS。
- 这样 source video duration 与音频/容器时长一致，更适合 Screen Studio 类录屏。

方案 C：先保守拒绝 drift 明显的 source。

- `finish()` 后检查 video/audio duration drift。
- drift 超过阈值时返回 `RecordingWriteFailed`，不返回 `output_path`。
- 这不能修复录制，但能阻止坏 artifact 进入 preview/export。

建议最小整改组合：

1. 先修 exporter packet rescale，解决 export 视频被压缩到 0.03s。
2. 再给 writer source validation 增加 A/V drift 检查，阻止坏 source。
3. 再决定 writer 是采用 VFR timestamp PTS 还是 CFR duplicate-frame 策略。

建议补测：

- `ffmpeg_writer_video_duration_matches_last_frame_timestamp`
- `ffmpeg_writer_source_video_audio_duration_drift_under_threshold`
- `ffmpeg_writer_dropped_or_sparse_frames_do_not_create_short_video_track`
- `source_artifact_validation_rejects_video_audio_drift`

### 18.7 Critical 3: artifact validation 仍会放过当前真实坏 export

位置：

- `src-tauri/src/media/ffmpeg_common.rs:124-160`
- `src-tauri/src/media/ffmpeg_common.rs:163-192`
- `src-tauri/src/lib.rs:841-852`

当前 validation 检查：

- file size > 0
- has video stream
- has audio stream
- dimensions match expected preset
- `inspection.duration_nanos > 0`

问题：

- `inspect_media_artifact()` 目前只返回单个 `duration_nanos`，优先使用 container duration。
- 当前真实坏 export 的 container duration 来自 audio，仍是 `19.008s`。
- video stream duration 只有 `0.032552s`，但 validation 看不到这个字段。

影响：

- `export_video()` 会认为坏 export 是有效可播放文件。
- UI 会显示“已生成可播放导出文件”。
- 这与用户实际看到的 BUG-004 完全一致。

建议修复：

扩展 `MediaArtifactInspection`：

```rust
pub struct MediaArtifactInspection {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_nanos: u64,
    pub video_duration_nanos: u64,
    pub audio_duration_nanos: u64,
    pub video_frame_count: Option<u64>,
    pub video_avg_fps: Option<f64>,
    pub has_video_stream: bool,
    pub has_audio_stream: bool,
}
```

`validate_export_artifact()` 至少增加：

- video stream duration > 0
- audio stream duration > 0
- `abs(video_duration_nanos - audio_duration_nanos) <= 100_000_000` 或暂定 `<= 500_000_000`
- video duration 与 container duration 差值在阈值内
- 对 30fps preset，video frame count 与 video duration 推导的 fps 在合理范围内

`validate_source_artifact()` 至少增加：

- source video/audio duration drift 阈值
- 若无真实音频输入但生成 silent AAC，则 silent audio duration 也要覆盖 video duration

建议补测：

- `validate_export_artifact_rejects_short_video_long_audio`
- `validate_source_artifact_rejects_audio_video_duration_drift`
- `validate_export_artifact_rejects_unrealistic_video_fps`

### 18.8 Important 1: audio cut boundary 仍未完整处理 decoded frame 跨 keep/cut 边界

位置：

- `src-tauri/src/media/trim_exporter.rs:535-553`
- `src-tauri/src/media/trim_exporter.rs:814-861`

当前逻辑：

- packet 层用 `pkt.pts()` 与 segment start/end 比较。
- decoded audio frame 进入 resampler 后整体编码。
- 没有计算 decoded audio frame 的 `[start, end)` 与 keep segment 的交集。

影响：

- 如果一个 decoded audio frame 跨越 keep/cut 边界，cut 区间音频可能泄漏到输出。
- 如果 packet PTS 在 keep 内但 frame 尾部进入 cut，当前会整帧保留。
- 如果 packet PTS 在 cut 内但 frame 尾部进入 keep，当前会整帧丢弃。

建议修复：

1. 对 decoded audio frame 计算输入时间范围：

   ```text
   frame_start = decoded_audio.pts in input audio time_base
   frame_duration = samples / sample_rate
   frame_end = frame_start + frame_duration
   ```

2. 与当前 keep segment 做交集：
   - 完全在 keep 外：drop
   - 完全在 keep 内：encode
   - 跨边界：slice 到 keep 内，或保守 drop 并在产品口径中明确

3. resampler buffer 需要在 segment 边界确定性 drain/flush，并防止上一段尾部被带入下一段。

建议补测：

- `ffmpeg_exporter_audio_frame_crossing_keep_start_is_sliced_or_dropped`
- `ffmpeg_exporter_audio_frame_crossing_keep_end_is_sliced_or_dropped`
- `ffmpeg_exporter_excludes_tone_inside_cut_range`
- `ffmpeg_exporter_audio_pts_monotonic_after_multi_segment_cut`

### 18.9 Important 2: writer 声称只接受 stereo，但实际只校验 `channels >= 1`

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:379-398`
- `src-tauri/src/media/ffmpeg_writer.rs:416-430`
- `src-tauri/src/media/ffmpeg_writer.rs:475-486`

当前代码：

```rust
if chunk.channels < 1 {
    return Err(...);
}
self.audio_channels = chunk.channels.max(1) as usize;
...
for ch in 0..self.audio_channels {
    let plane = audio_frame.plane_mut::<f32>(ch);
    ...
}
```

问题：

- AAC encoder frame 是 `ChannelLayout::STEREO`，但 `self.audio_channels` 可以是 1、2、3...
- mono 输入时只写 plane 0，plane 1 保持默认/未明确填充。
- `channels > 2` 时可能访问不存在的 plane 并 panic。

当前产品路径通常经 `SimpleAudioMixer` 输出 `48kHz stereo`，但 writer 是公共边界，应该明确 enforce。

建议修复：

```rust
if chunk.channels != 2 {
    return Err(AppError::RecordingWriteFailed {
        reason: format!("FFmpeg writer 仅接受 stereo 音频，实际通道数 {}", chunk.channels),
    });
}
if chunk.samples.len() % 2 != 0 {
    return Err(AppError::RecordingWriteFailed {
        reason: "FFmpeg writer 收到的 stereo 音频样本数不是 2 的倍数".to_string(),
    });
}
```

建议补测：

- `ffmpeg_writer_rejects_mono_audio`
- `ffmpeg_writer_rejects_more_than_stereo_audio`
- `ffmpeg_writer_rejects_odd_interleaved_sample_count`

### 18.10 Important 3: decoder flush after seek 被显式跳过，当前注释理由不足以关闭风险

位置：

- `src-tauri/src/media/trim_exporter.rs:499-520`

当前注释说：

- 正确模式是 seek 后 flush decoder。
- 但由于 encoder buffered frames / PTS monotonicity 问题，当前故意省略。
- 假设 `max_b_frames=0` 且 demuxer seek 到 keyframe，decoder 可以处理。

问题：

- 这是以“避免 PTS 问题”为理由跳过 decoder seek hygiene，而不是修复 PTS 问题。
- 对多段 cut，decoder 内部参考帧/缓冲状态是否跨 segment 干扰仍未被测试覆盖。
- 当前真实 BUG 已经证明 PTS/muxer time base 仍有系统性问题，因此不能用“避免 PTS monotonicity 违例”作为长期理由。

建议修复：

1. 先修 Critical 1 的 muxer time base rescale。
2. 再尝试 seek 后 flush video/audio decoder。
3. 对 multi-segment cut 增加视觉与时间戳检查。
4. 如果 flush 确实导致 encoder monotonicity 问题，应重置 per-segment timestamp mapping，而不是保留 decoder stale state。

建议补测：

- `ffmpeg_exporter_flushes_video_decoder_after_segment_seek`
- `ffmpeg_exporter_flushes_audio_decoder_after_segment_seek`
- `ffmpeg_exporter_second_keep_segment_starts_near_requested_time`

### 18.11 Important 4: integration tests 通过但没有覆盖真实失败模式

位置：

- `src-tauri/tests/ffmpeg_export.rs`
- `src-tauri/src/test_support/ffmpeg_helpers.rs`

当前测试新增：

- full-duration export
- three preset dimensions
- missing source
- cancel before start
- cut timeline shorter output
- cut duration tolerance
- start keep segment

问题：

1. synthetic source 是由当前 `FfmpegRecordingWriter` 生成的短 fixture，且 video/audio 都是规则数据，不模拟真实录制中的 frame sparsity/drop。
2. 测试只用 container/media artifact duration，未拆分 video/audio stream duration。
3. 测试没有检查 video packet duration/fps，因此 export 视频 `0.03s` 这类问题可漏过。
4. 新增测试里多个 `let result = ...` 未使用，clippy 已报警。

建议补测：

- 用现有 synthetic helper 生成一个“稀疏视频 + 连续音频”的 source，验证 source validation 拒绝或 writer 补帧。
- 用 artifact inspector 检查 stream-level duration。
- 用 packet-level inspection 检查 video packet duration。
- 集成测试不要只断言 `output.exists()`、`has_video_stream`、`has_audio_stream`。

### 18.12 Important 5: `effect_timeline_path` 仍未被 FFmpeg exporter 应用

位置：

- `src-tauri/src/media/trim_exporter.rs:52`
- `src-tauri/src/media/trim_exporter.rs:120-980`

当前状态：

- `TrimExportRequest` 仍有 `effect_timeline_path`。
- `FfmpegTrimExporter::export()` 仍未读取该 JSON，也未进行 cursor magnification/smoothing/click overlay compositing。

影响：

- 当前导出仍是“可播放 preset export”，不是“包含光标视觉美化的导出”。
- 真实 source 播放里“没有光标”的现象也需要区分：
  - 如果 `CaptureConfig.show_system_cursor = false`，source 本身不会有系统光标。
  - 如果 exporter 未应用 effect timeline，导出也不会补绘光标。

建议：

- 若 Phase 6 不交付 cursor compositor，文档、checklist、UI 文案必须明确“不包含光标视觉重绘”。
- 若 Phase 6 要交付完整 AI 美化导出，需要单独实现 timeline parser + compositor + visual artifact tests + Native Safety review。

### 18.13 Important 6: FFmpeg writer 仍同步运行在录制 consumer thread

位置：

- `src-tauri/src/platform/macos_service.rs:223-245`
- `src-tauri/src/platform/macos_service.rs:439-521`
- `src-tauri/src/media/ffmpeg_writer.rs:283-528`

当前状态：

- `FfmpegRecordingWriter` 仍在 `consume_frames()` consumer thread 内同步做：
  - BGRA row copy
  - swscale
  - H.264 encode
  - AAC encode
  - mux write

影响：

- 编码变慢会拖慢 consumer thread，导致 media channel 堆积/丢帧。
- source artifact 已出现 video duration 短于 audio/container duration，虽然不能仅凭这一点断言全部由同步 writer 导致，但该架构风险仍未关闭。
- 1080p 10 分钟压力 Gate 未完成前，不建议默认启用 FFmpeg writer。

建议：

- worker-backed writer + byte/frame budgeted queue。
- 明确 drop/backpressure 策略。
- 记录 dropped frame count 并进入 RecordingResult / metadata。
- 压力测试覆盖 1080p 10 分钟、系统音频+麦克风开启。

### 18.14 Minor: BUG.md 有格式化噪声和预防规则缺口

位置：

- `BUG.md`

本轮 `BUG.md` 新增了 BUG-004，非常有价值。但 diff 中也包含大量既有 table/空行格式化改动，和本轮 Phase 6 审查目标无关。

建议：

- 保留 BUG-004 现象、日志、真实 artifact 证据。
- 避免无关 Markdown table 格式 churn，减少 review 噪声。
- BUG-004 后续修复后必须补“预防规则”，建议至少包括：
  - FFmpeg muxer 写包必须使用 `write_header()` 后的真实 output stream time base。
  - artifact validation 必须检查 per-stream duration，而不是只看 container duration。
  - writer 不得只用 frame count 伪造真实录制时间轴，必须处理 frame sparsity/drop。
  - playable export manual gate 必须包含 ffprobe stream-level duration/fps 检查。

### 18.15 BUG.md、架构红线与安全复核

本轮未发现以下红线回归：

- 未发现生产代码调用 `std::process::Command` 执行 `ffmpeg` / `ffprobe`。
- 未发现 FFmpeg CLI 参数拼接用户输入。
- 未发现原始音视频帧流进入 React / TypeScript 层。
- 未发现 hardcoded activation secret / private key。
- 未发现字幕、摘要、模板系统、团队协作、平台发布 API 等 MVP 禁区功能。
- 未发现新增透明窗口、拖拽、`whileTap` 包裹 Button 等 BUG-001/002/003 预防规则回归。

仍需注意：

- 本轮使用 `ffprobe` 是人工审查辅助，不是生产路径依赖。
- 自动化核心逻辑仍应使用 FFmpeg binding inspection。
- Native Safety Gate 尚未完成，尤其是 `trim_exporter.rs` 中 unsafe plane copy、decoder/scaler/resampler/encoder ownership、seek/flush/drop 路径。

### 18.16 建议整改顺序

#### R0: 先修 export packet time base

目标：

- 解决 BUG-004 中 export 视频只有第一帧/黑屏的直接根因。

任务：

1. `output.write_header()` 后读取 `video_out_tb` / `audio_out_tb`。
2. video packet 全部 `rescale_ts(video_enc_tb, video_out_tb)`。
3. audio packet 全部 `rescale_ts(audio_enc_tb, audio_out_tb)`。
4. 集成测试检查 video stream duration、packet duration、fps。

#### R1: 强化 artifact validation

目标：

- 当前真实坏 export 不能再通过 validation。

任务：

1. `MediaArtifactInspection` 增加 stream-level durations。
2. validate export/source 检查 video/audio drift。
3. validate export 检查 video fps/packet duration sanity。
4. `export_video()` 成功前必须跑增强 validation。

#### R2: 修 source writer 时间轴

目标：

- source artifact 的 video duration 与 audio/container duration 一致或在阈值内。

任务：

1. 决定 VFR timestamp PTS 或 CFR duplicate-frame 策略。
2. 如果继续固定 30fps，必须补重复帧。
3. writer finish 后拒绝 A/V drift 明显的 source。
4. 记录/暴露 dropped frame count。

#### R3: audio cut boundary 和 decoder flush

目标：

- auto-trim 后音频不泄漏、PTS 单调、segment seek 正确。

任务：

1. decoded audio frame 按 `[start,end)` 和 keep segment 求交集。
2. 跨边界 frame slice 或明确保守 drop。
3. 修复 PTS 后重新评估 seek 后 decoder flush。
4. 增加 tone fixture 测试。

#### R4: writer 音频边界与同步编码压力

目标：

- writer trait boundary 清晰，录制链路不被编码压力拖垮。

任务：

1. `push_audio()` 强制 `sample_rate == 48_000 && channels == 2`。
2. 检查 stereo samples len 必须是 2 的倍数。
3. 后续 worker-backed writer。
4. 1080p 10 分钟压力 Gate。

#### R5: 文档/checklist/warnings 收口

目标：

- 整改状态不被过时文档或 warning 噪声误导。

任务：

1. 先跑 `cargo fmt`。
2. 清理 Phase 6 新增 clippy warnings。
3. 更新 `tests/phase-6-w11-w12-checklist.md` 真实状态。
4. 更新 `HANDOFF.md`：记录 BUG-004、当前 FFmpeg feature path 状态、manual gates/Native Safety 未完成。

### 18.17 建议补充测试清单

Rust / artifact validation：

- `validate_export_artifact_rejects_short_video_long_audio`
- `validate_source_artifact_rejects_video_audio_duration_drift`
- `validate_export_artifact_rejects_unrealistic_video_fps`
- `inspect_media_artifact_reports_stream_level_durations`

Rust / exporter：

- `ffmpeg_exporter_rescales_video_packets_to_muxer_time_base`
- `ffmpeg_exporter_video_stream_duration_matches_audio_duration`
- `ffmpeg_exporter_video_packet_duration_is_about_1_over_fps`
- `ffmpeg_exporter_audio_packets_rescale_to_muxer_time_base`
- `ffmpeg_exporter_multi_segment_cut_keeps_video_audio_in_sync`
- `ffmpeg_exporter_excludes_tone_inside_cut_range`

Rust / writer：

- `ffmpeg_writer_source_video_duration_matches_recording_duration`
- `ffmpeg_writer_sparse_frames_do_not_create_short_video_track`
- `ffmpeg_writer_rejects_mono_audio`
- `ffmpeg_writer_rejects_more_than_stereo_audio`
- `ffmpeg_writer_rejects_odd_interleaved_sample_count`
- `ffmpeg_writer_reports_or_handles_dropped_frames`

Frontend：

- `preview_uses_convert_file_src_for_recording_output`
- `preview_shows_ffmpeg_feature_gate_copy_when_output_path_missing`
- `export_success_message_includes_output_path_only_when_validation_passed`

Manual Gates：

- `npm run tauri:dev:ffmpeg` 录制约 20 秒，source 用 QuickTime/IINA/VLC 可播放完整 20 秒。
- 导出 Bilibili preset 后，export 视频 stream duration 与 audio stream duration 差值小于 100ms。
- `ffprobe` 检查 export video packet duration 约 `0.033333s`。
- auto-trim 多段导出后，第二段画面/声音从正确位置开始。
- 含明显音频 tone 的 cut 区间在导出中不可听见。
- source artifact 保留，cancel export 删除 partial output。

### 18.18 当前建议对外状态表述

建议本轮整改前统一使用下面口径：

> Phase 6 的 FFmpeg feature 构建已能跑通 writer/exporter smoke tests，开发入口、Tauri asset 播放路径、音频流 validation、stale cursor timeline 等第 17 节部分问题已有推进。但当前真实导出 artifact 显示视频 stream 只有约 0.03 秒而 audio/container 约 19 秒，根因是 `trim_exporter` 写出 encoded video packet 时没有 rescale 到 `write_header()` 后 muxer 的真实 output stream time base。这会直接造成导出视频第一帧后黑屏/无画面。source writer 也仍按固定帧序号生成视频 PTS，真实 source 已出现 video/audio duration drift。当前 validation 只看 container duration，会放过这类坏文件。因此 Phase 6 仍不能作为完成态合并，需先修 packet time base、stream-level artifact validation、source writer 时间轴和 audio cut boundary，再完成 manual gates 与 Native Safety review。

## 19. Phase 6 FFmpeg playable export 第 18 节整改后复审（2026-05-31，HEAD `8986b4a` + worktree）

### 19.1 结论摘要

本轮针对用户反馈“已完成 `## 18. Phase 6 FFmpeg playable export 第 17 节整改后复审` 章节的整改任务”后的当前 dirty worktree 进行复审。

结论：

- 第 18 节中 BUG-004 的直接根因已经有实质性整改：
  - exporter encoded packet 已改为在 `write_header()` 后读取 muxer 真实 output stream time base，并使用 `rescale_ts(enc_tb, out_tb)` 写包。
  - writer 视频 PTS 已从固定 frame index 改为基于真实 frame timestamp 映射到 encoder time base。
  - artifact validation 已增加 video/audio stream-level duration drift 检查，能拦截“container/audio 很长但 video stream 极短”的坏文件。
- 自动化验证状态明显好转：
  - 默认 Rust tests 通过。
  - `--features ffmpeg` tests 通过，且新增 cut timeline duration 相关 integration tests。
  - 前端 tests 和 build 通过。
- 但 Phase 6 仍不能标记完成，核心原因不是 BUG-004，而是新的产品级 blocker：
  - **Critical：默认美化开启时，录制会隐藏系统光标，但 FFmpeg exporter 仍没有应用 `effect_timeline_path` 绘制光标效果，导致导出视频没有光标、没有美化。该问题已进入 `BUG.md` 的 BUG-005。**
- 仍需作为 Phase 6 完成前阻塞项处理：
  - FFmpeg writer 仍同步运行在录制 consumer thread，worker-backed writer / bounded queue 风险未关闭。
  - audio cut boundary 对 decoded frame 跨 keep/cut 边界仍是已知未完成点。
  - `npm run tauri:dev` 默认不启用 `ffmpeg` feature，manual gate 必须明确使用 `npm run tauri:dev:ffmpeg`。
  - Native Safety Gate 和 1080p 10 分钟手动压力 Gate 仍未完成。

合并判断：

| 项目 | 当前判断 |
| --- | --- |
| BUG-004 可播放导出 | 主要代码根因已修，自动化验证通过，但仍需真实 artifact manual gate 确认 |
| 三种 preset 可播放导出 | FFmpeg integration tests 通过 |
| Phase 6 “导出包含美化” | **未完成，BUG-005 阻塞** |
| 默认产品路径完成态 | **不建议声明完成** |
| Native Safety | 未完成 |

### 19.2 本轮有效范围

本轮复审基于当前 worktree 状态，重点文件：

- `BUG.md`
- `HANDOFF.md`
- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lib.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/test_support/ffmpeg_helpers.rs`
- `src-tauri/tests/ffmpeg_export.rs`
- `src/components/preview-view.tsx`
- `src/App.test.tsx`

本轮没有逐帧人工播放真实录屏 artifact，也没有执行 GUI 录制和 10 分钟压力 gate；这些仍归入 Manual Gates。

### 19.3 本轮验证记录

本轮复审执行了以下命令：

| 命令 | 结果 | 备注 |
| --- | --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 通过 | 第 18 节 rustfmt 失败已修复 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 | 199 tests passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` | 通过 | 220 unit tests + 8 integration tests passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` | 通过（warnings only） | 仍有新增/相关 warnings，见 19.9 |
| `npm test -- --run` | 通过 | 51 tests passed |
| `npm run build` | 通过 | Vite build passed |

`cargo clippy` 仍有 warning：

- `src/app/export_service.rs`: `too_many_arguments`
- `src/media/trim_exporter.rs`: several `manual_checked_ops`
- `src-tauri/tests/ffmpeg_export.rs`: two `manual_abs_diff`
- 既有 macOS FFI 命名/unused warnings 仍存在

这些 warnings 不阻塞编译，但 Phase 6 收尾前建议清理新增 warning，避免掩盖后续 Native Safety 风险。

### 19.4 已确认推进项

#### 19.4.1 BUG-004: exporter packet time base 已修

位置：

- `src-tauri/src/media/trim_exporter.rs:406-414`
- `src-tauri/src/media/trim_exporter.rs:810-814`
- `src-tauri/src/media/trim_exporter.rs:864-868`
- `src-tauri/src/media/trim_exporter.rs:953-971`

确认：

- `output.write_header()` 后重新读取：
  - `video_out_tb = output.stream(video_out_idx).unwrap().time_base()`
  - `audio_out_tb = output.stream(audio_out_idx).unwrap().time_base()`
- video packet 写入前使用：
  - `enc_pkt.rescale_ts(video_enc_tb, video_out_tb)`
- audio packet 写入前使用：
  - `enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb)`

这直接修复第 18 节 Critical 1，也是 BUG-004 “视频 stream 被压缩到 0.03s”的主要根因。

#### 19.4.2 BUG-004: writer 视频 PTS 已改为真实 timestamp 映射

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:331-344`

确认：

- writer 不再简单使用 `video_frame_index` 作为 PTS。
- 当前 PTS 计算：
  - `pts_in_enc_tb = frame.timestamp.nanos * 30 / 1_000_000_000`
  - 再通过 `max(last_video_pts + 1)` 保证单调。
- 这比第 18 节中的固定 frame count PTS 模型更符合真实录制时间轴。

仍需注意：

- 如果真实捕获非常稀疏，当前实现会生成 VFR-like sparse PTS，而不是补重复帧。
- 这能让 stream duration 接近真实时间，但是否符合产品期望的 CFR 30fps 视觉流畅度，仍需 manual gate 和后续策略确认。

#### 19.4.3 artifact validation 已能拦截 video/audio duration drift

位置：

- `src-tauri/src/media/ffmpeg_common.rs:8-20`
- `src-tauri/src/media/ffmpeg_common.rs:77-147`
- `src-tauri/src/media/ffmpeg_common.rs:156-223`
- `src-tauri/src/media/ffmpeg_common.rs:231-277`

确认：

- `MediaArtifactInspection` 已新增：
  - `video_duration_nanos`
  - `audio_duration_nanos`
  - `video_frame_count`
  - `video_avg_fps`
- `validate_export_artifact()` 检查：
  - 文件非空
  - video stream 存在
  - audio stream 存在
  - 分辨率匹配
  - container duration 非零
  - video stream duration 非零
  - audio stream duration 非零
  - video/audio drift 不超过 500ms
- `validate_source_artifact()` 检查 source video/audio drift 不超过 1s。

这修复第 18 节 Critical 3 的主要风险：坏 artifact 不能再只靠 container duration 通过。

#### 19.4.4 writer 音频输入边界已更严格

位置：

- `src-tauri/src/media/ffmpeg_writer.rs:389-458`

确认：

- `push_audio()` 已强制：
  - `sample_rate == 48_000`
  - `channels == 2`
  - stereo interleaved samples len 必须是 2 的倍数
- AAC frame size 1024 sample buffering 已补齐。
- 无音频时 `finish()` 会生成 silent AAC track。

这修复第 18 节 Important 2 的主要问题。

#### 19.4.5 Tauri asset 播放路径已接入标准 API

位置：

- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src/components/preview-view.tsx:14`
- `src/components/preview-view.tsx:298-303`

确认：

- Tauri 增加 `protocol-asset` feature。
- `assetProtocol` 已启用，scope 为 `$TEMP/luzhi-recordings/**`。
- 前端 preview 不再手拼 `asset://localhost/${path}`，改用 `convertFileSrc(recordingResult.outputPath)`。
- `media-src` CSP 已包含 asset / asset.localhost。

这修复第 17 节 Critical 2 的主要播放入口问题。

注意：

- `Cargo.toml` 没有修改核心依赖版本，只新增 Tauri feature；仍建议由人工确认该 feature 变更符合团队“核心依赖版本由人类负责”的边界。

#### 19.4.6 stale cursor timeline 复用问题已收敛

位置：

- `src-tauri/src/lib.rs:721-734`

确认：

- `export_video()` 不再从 `service.last_effect_timeline_path()` 读取可能过期的 effect timeline。
- 当前只使用本次 `build_cursor_effect_timeline()` 返回的 `cursor.effect_timeline_path`。

这修复第 17 节 Important 1 的主要 stale path 风险。

### 19.5 Critical 1: 默认美化开启时导出视频仍没有光标/没有美化（BUG-005）

位置：

- `BUG.md`
- `src-tauri/src/lib.rs:152-172`
- `src-tauri/src/lib.rs:699-707`
- `src-tauri/src/lib.rs:721-734`
- `src-tauri/src/media/trim_exporter.rs:47-55`
- `src-tauri/src/media/trim_exporter.rs:120-991`

现象：

- `BUG.md` 已新增未解决 BUG-005：导出的视频没有美化，录制原视频看不到光标，导出也看不到光标。

代码证据：

1. `start_recording()` 会根据当前美化配置重新计算：

   ```rust
   let expected_show_system_cursor =
       !(beautify_config.cursor_magnification || beautify_config.cursor_smoothing);
   config.show_system_cursor = expected_show_system_cursor;
   ```

   当前前端默认美化开关为开启状态：

   - `cursorMagnification = true`
   - `cursorSmoothing = true`

   因此默认录制会隐藏系统光标。source artifact 本身没有原始系统光标，这是预期行为。

2. `export_video()` 会尝试构建 cursor effect timeline，但失败时会降级为：

   ```rust
   CursorEffectSummaryPayload {
       frame_count: 0,
       click_effect_count: 0,
       effect_timeline_path: None,
   }
   ```

   对“只要基础 playable export”的目标来说，这种降级曾经合理；但在默认 raw cursor 被隐藏的产品路径下，这会静默产出“无光标、无美化”的导出。

3. `TrimExportRequest` 虽然包含 `effect_timeline_path`，但 `FfmpegTrimExporter::export()` 没有读取该 JSON，也没有进行 cursor overlay compositing。

影响：

- 当前 FFmpeg 导出完成的是“可播放 preset export”，不是“包含光标放大/平滑/点击效果的美化导出”。
- 这会直接违背 MVP 核心“录屏 + AI 自动美化 + 一键导出”。
- 这是当前 Phase 6 完成态最大的阻塞项。

整改方向需要先做产品/架构决策：

方案 A：Phase 6 完整实现 cursor compositor。

- exporter 读取 `effect_timeline_path`。
- 将 effect timeline 的 cursor 坐标映射到导出画布：
  - FitWithBars 需要考虑缩放比例和黑边 offset。
  - CenterCrop 需要考虑 crop origin、scale ratio 和坐标裁剪。
- 在视频 frame scale 后或 scale 前叠加 cursor overlay，需要明确坐标空间。
- 支持 cursor smoothing、magnification、click effect。
- 加入 visual artifact tests 或 decoded frame pixel tests。
- 进入 Native Safety review。

方案 B：暂不交付 compositor，但避免无光标坏体验。

- 在 cursor compositor 完成前，不要默认隐藏系统光标。
- 或在 raw cursor hidden 且 effect timeline 不可用/未被 exporter 应用时，让 export 明确失败，不要静默导出无光标视频。
- UI、checklist、HANDOFF 必须明确“当前 playable export 不包含光标视觉重绘”。

建议：

- 如果 Phase 6 要按 MVP 完成口径验收，应走方案 A。
- 如果先保 playable export，不应把 Phase 6 宣称为“美化导出完成”，并且默认录制不应产出无光标 artifact。

建议补测：

- `export_video_default_beautify_hidden_raw_cursor_requires_effect_timeline`
- `ffmpeg_exporter_applies_cursor_effect_timeline_to_output_frames`
- `ffmpeg_exporter_maps_cursor_coordinates_for_fit_with_bars`
- `ffmpeg_exporter_maps_cursor_coordinates_for_center_crop`
- `ffmpeg_exporter_missing_effect_timeline_fails_when_raw_cursor_hidden`
- Manual Gate：默认美化开启录制 10-20 秒，导出视频中必须能看到光标轨迹/点击效果。

### 19.6 Important 1: FFmpeg writer 仍同步运行在录制 consumer thread

位置：

- `src-tauri/src/platform/macos_service.rs:400-521`
- `src-tauri/src/platform/macos_service.rs:526-598`
- `src-tauri/src/media/ffmpeg_writer.rs:287-530`

当前状态：

- `consume_frames()` 消费线程中直接调用：
  - `writer.push_video(frame)`
  - `writer.push_audio(mixed)`
- `FfmpegRecordingWriter::push_video()` 同步完成：
  - BGRA row copy
  - swscale
  - H.264 `send_frame`
  - packet receive/write
- `FfmpegRecordingWriter::push_audio()` 同步完成：
  - interleaved buffer
  - planar conversion
  - AAC `send_frame`
  - packet receive/write

影响：

- 当前编码压力不会直接阻塞 ScreenCaptureKit callback，但会拖慢 consumer thread。
- consumer thread 跟不上时，bounded media channel 会堆积并丢帧/丢音频。
- 这与 Phase 6 计划中的 worker-backed writer + byte-budgeted bounded queue 尚未对齐。
- 当前 FFmpeg integration tests 多为短 synthetic artifact，不能证明 1080p 长录制稳定性。

建议整改：

1. 将 `FfmpegRecordingWriter` 改为轻量 front writer + encoding worker。
2. front writer 只做必要校验和 bounded enqueue，不做 swscale/x264/AAC/mux write。
3. queue 使用 byte/frame budget，不允许无界积压。
4. 明确队列满时策略：
   - 保守方案：返回 fatal error，停止录制并提示编码跟不上。
   - 或记录 dropped frame，但必须让 source artifact 时间轴和 metadata 可解释。
5. `finish()` join worker exactly once。
6. 失败/取消路径删除 partial source artifact。

建议补测：

- `ffmpeg_writer_push_video_does_not_encode_on_capture_consumer_thread`
- `ffmpeg_writer_queue_rejects_when_byte_budget_exceeded`
- `ffmpeg_writer_finish_joins_worker_and_surfaces_worker_error`
- Manual Gate：1080p 10 分钟、系统音频+麦克风开启，检查 source/export 可播放、A/V drift、dropped frame 计数。

### 19.7 Important 2: audio cut boundary 仍未完整处理 decoded frame 跨 keep/cut 边界

位置：

- `src-tauri/src/media/trim_exporter.rs:821-828`

当前代码已经明确注释：

```rust
// NOTE: Decoded audio frames may span across keep/cut
// boundaries. Packet-level PTS filtering handles most
// cases, but edge-case leakage at boundaries is possible.
```

影响：

- auto-trim 导出时，音频 frame 如果跨越 keep/cut 边界，当前 packet-level filtering 可能保留 cut 区间内的一小段音频，或丢失 keep 边界附近音频。
- 对“静音裁剪”场景可能不明显，但对有明显 tone、人声、点击声的边界会产生可听泄漏。
- 第 18 节 Important 1 仍未彻底关闭。

建议整改：

1. 对 decoded audio frame 计算真实 `[frame_start, frame_end)`。
2. 与当前 keep segment 求交集。
3. 对跨边界 frame 做 sample-level slice。
4. 重设 sliced frame PTS，继续由 `next_audio_out_pts` 保证输出连续。
5. 对 resampler flush 的输出也按 segment 边界处理。

建议补测：

- `ffmpeg_exporter_slices_audio_frame_crossing_keep_start`
- `ffmpeg_exporter_slices_audio_frame_crossing_keep_end`
- `ffmpeg_exporter_excludes_audible_tone_inside_cut_range`
- Manual Gate：构造明显 tone 区间，auto-trim 后导出中 cut 区间 tone 不可听见。

### 19.8 Important 3: 默认开发入口仍不启用 FFmpeg feature，容易误判 product path

位置：

- `package.json`
- `src/components/preview-view.tsx:313-315`
- `src-tauri/src/lib.rs:736-753`

当前状态：

- 新增脚本：
  - `npm run tauri:dev`
  - `npm run tauri:dev:ffmpeg`
- 但默认 `tauri:dev` 仍不启用 `ffmpeg` feature。
- 非 FFmpeg 构建中，`export_video()` 在没有 source artifact 时返回明确 Gate 结果，`output_path: None`。
- Preview 文案已提示“请使用 npm run tauri:dev:ffmpeg”。

判断：

- 这符合计划中“FFmpeg feature 默认仍关闭，Native Safety Gate 通过后才允许产品构建启用”的约束。
- 但对实际整改/验收来说，极易出现用默认 dev command 测试后误判“导出仍不可用”。

建议：

- `tests/phase-6-w11-w12-checklist.md` 和 HANDOFF 必须明确：
  - playable export manual gate 必须使用 `npm run tauri:dev:ffmpeg`。
  - 默认 `npm run tauri:dev` 只验证 no-FFmpeg Gate，不验证 playable export。
- 若后续切到产品默认启用 FFmpeg，必须先完成 Native Safety Gate 和 10 分钟压力 Gate。

建议补测：

- Frontend test 覆盖 no-FFmpeg Gate 文案。
- Manual checklist 分开记录：
  - no-FFmpeg build behavior
  - FFmpeg feature build behavior

### 19.9 Important 4: Native Safety 和新增 warnings 仍需收口

位置：

- `src-tauri/src/media/trim_exporter.rs`
- `src-tauri/src/media/ffmpeg_writer.rs`
- `src-tauri/src/media/ffmpeg_common.rs`
- `src-tauri/tests/ffmpeg_export.rs`

当前状态：

- `cargo clippy --features ffmpeg --all-targets` 通过，但仍有 warnings。
- `trim_exporter.rs` 中仍有多处 unsafe plane copy。
- `ffmpeg_common.rs` 中仍有 `unsafe { &*params.as_ptr() }`。
- `ffmpeg_writer.rs` 中仍有 `unsafe impl Send for SendScaler`。

新增/相关 warnings：

- `src/app/export_service.rs`: `too_many_arguments`
- `src/media/trim_exporter.rs`: `manual_checked_ops`
- `src-tauri/tests/ffmpeg_export.rs`: `manual_abs_diff`

建议：

- 对 Phase 6 新增 warnings 做清理或显式 `allow`，不要让 Native Safety review 在 warning 噪声里进行。
- unsafe block 需要逐处写明：
  - 源/目标 buffer 长度边界。
  - plane width/height/linesize 的关系。
  - crop offset 不越界。
  - FFmpeg wrapper ownership/drop 顺序。
  - scaler/resampler/encoder/decoder flush 和 drop 路径。

建议补测：

- odd crop origin / odd fit offset 下的 chroma plane copy。
- padded linesize source frame 的 crop/fit 输出。
- exporter error/cancel 路径是否释放并删除 partial output。

### 19.10 BUG.md、架构红线与安全复核

BUG.md 复核：

- BUG-004 已补充现象、根因、修复和预防规则，当前代码与预防规则方向一致：
  - muxer 写包使用 `write_header()` 后真实 output stream time base。
  - artifact validation 检查 per-stream duration。
  - writer 不再只用 frame count 伪造真实时间轴。
- BUG-005 已记录为未解决：
  - 当前 review 确认它是 Phase 6 完成态 blocker。
  - BUG-005 修复后也必须补预防规则。

建议 BUG-005 修复后新增预防规则：

- 当 raw system cursor hidden 时，exporter 必须应用 cursor effect timeline，或导出必须失败，不得静默产出无光标视频。
- cursor effect timeline 不能只作为 request 字段存在，必须有 artifact-level visual validation。
- 默认美化开关、capture `show_system_cursor`、export compositor 三者必须作为一条端到端 contract 测试。
- UI 不能把“可播放导出成功”文案等同于“美化导出成功”。

架构红线复核：

- 未发现生产代码调用 `std::process::Command` 执行 `ffmpeg` / `ffprobe`。
- 未发现 FFmpeg CLI 参数拼接用户输入。
- 未发现音视频帧流进入 React / TypeScript 层。
- 未发现新增 hardcoded activation secret / private key。
- 未发现字幕、摘要、模板系统、团队协作、平台发布 API 等 MVP 禁区功能。
- 未发现 BUG-001/002/003 相关透明窗口、拖拽、`whileTap` 包裹 Button 的新回归。
- `Cargo.toml` 未修改核心依赖版本；新增 `tauri` feature `protocol-asset` 需要人工确认，但不属于版本 bump。

### 19.11 Phase 6 要求对照表

| 要求 | 当前状态 | 结论 |
| --- | --- | --- |
| fixed export presets | 三种 preset tests 通过 | 已推进 |
| playable FFmpeg export | `--features ffmpeg` integration tests 通过 | 已推进，需 manual gate |
| BUG-004 黑屏/0.03s | packet time base、writer PTS、validation 已修 | 代码层主要根因已修 |
| source artifact 保留 | integration tests 有覆盖，manual 未完成 | 部分完成 |
| cancel cleanup | cancel-before-start 和 exporter loop 有覆盖，mid-export manual 未完成 | 部分完成 |
| no-FFmpeg Gate | command 和 UI 文案已推进 | 已推进 |
| export progress | callback/event 有，真实长导出中间值 manual 未完成 | 部分完成 |
| audio resample | exporter 有 SwrContext，writer 强制 48k stereo | 已推进 |
| silent audio track | writer/exporter 均有生成逻辑 | 已推进 |
| audio cut boundary | 代码注释承认 edge leakage | 未完成 |
| cursor beautification export | `effect_timeline_path` 未被 exporter 应用 | **未完成，Critical** |
| worker-backed writer | 仍同步编码 | 未完成 |
| Native Safety Gate | 未见完成记录 | 未完成 |
| 1080p 10 分钟压力 Gate | 未见完成记录 | 未完成 |

### 19.12 建议整改顺序

#### R0: 先关闭 BUG-005 的产品决策

目标：

- 避免继续产出“source 无系统光标，export 也无光标”的 artifact。

建议选择其一：

1. 立即实现 cursor compositor，并把 Phase 6 作为“美化导出”验收。
2. 暂不实现 compositor，但在完成前保持 raw system cursor visible，或者在 raw cursor hidden 且 effect timeline 未应用时让 export 失败。

不建议继续保持当前状态：默认隐藏系统光标 + exporter 不绘制 effect timeline + export 成功。

#### R1: 如果选择实现 compositor，先做最小垂直闭环

任务：

1. 读取 `effect_timeline_path` JSON。
2. 只先支持最小 cursor overlay：
   - 固定基础光标图形或简单圆点。
   - 映射到 Bilibili 16:9 输出。
3. 加 tests 确认导出帧中 cursor 像素存在。
4. 再扩展：
   - Douyin / Xiaohongshu coordinate mapping。
   - magnification。
   - click effect。
   - smoothing path。

#### R2: 修 audio cut boundary

任务：

1. decoded audio frame 与 keep segment 做 sample-level intersection。
2. slice 跨边界 frame。
3. 增加 audible tone fixture。

#### R3: worker-backed writer

任务：

1. front writer bounded enqueue。
2. encoder worker 负责 swscale/x264/AAC/mux。
3. queue budget 和 backpressure 策略。
4. worker error 进入 `finish()` 和 stop result。

#### R4: Manual Gates / Native Safety

任务：

1. `npm run tauri:dev:ffmpeg` 真实录制 1080p 10 分钟。
2. 检查 source 和 export：
   - QuickTime/IINA/VLC 可播放。
   - video/audio stream duration drift。
   - packet duration/fps。
   - cursor visual effect。
   - auto-trim 边界。
   - cancel cleanup。
3. Native Safety review 覆盖 unsafe plane copy、FFmpeg context ownership、encoder/decoder flush/drop、worker queue/thread join。

#### R5: 文档与 warnings 收口

任务：

1. 清理 Phase 6 新增 clippy warnings。
2. 更新 `tests/phase-6-w11-w12-checklist.md`：
   - BUG-004 fixed evidence。
   - BUG-005 pending/fixed evidence。
   - no-FFmpeg Gate 和 FFmpeg feature Gate 分开。
3. 更新 `HANDOFF.md`：
   - Phase 6 当前真实状态。
   - Manual Gates 和 Native Safety 仍未完成项。
4. BUG-005 修复后补 BUG.md 预防规则。

### 19.13 建议补充测试清单

Rust / cursor export：

- `export_video_hidden_raw_cursor_requires_effect_timeline`
- `ffmpeg_exporter_rejects_hidden_raw_cursor_without_effect_timeline`
- `ffmpeg_exporter_draws_cursor_overlay_on_bilibili_output`
- `ffmpeg_exporter_maps_cursor_overlay_for_douyin_center_crop`
- `ffmpeg_exporter_maps_cursor_overlay_for_xiaohongshu_center_crop`
- `ffmpeg_exporter_click_effect_changes_pixels_near_click_timestamp`

Rust / writer：

- `ffmpeg_writer_enqueue_is_nonblocking_under_slow_encoder`
- `ffmpeg_writer_queue_budget_failure_is_fatal`
- `ffmpeg_writer_finish_surfaces_worker_error`
- `ffmpeg_writer_preserves_av_duration_with_sparse_frames`

Rust / audio trim：

- `ffmpeg_exporter_slices_audio_frame_crossing_keep_start`
- `ffmpeg_exporter_slices_audio_frame_crossing_keep_end`
- `ffmpeg_exporter_excludes_tone_inside_cut_range`

Rust / validation：

- `validate_export_artifact_rejects_hidden_cursor_without_overlay_metadata`
- `inspect_media_artifact_reports_stream_level_durations`
- `validate_export_artifact_rejects_video_audio_duration_drift`

Frontend：

- `preview_uses_convert_file_src_for_recording_output`
- `preview_shows_no_ffmpeg_gate_copy_without_output_path`
- `preview_does_not_label_export_as_beautified_when_effect_timeline_missing`
- `export_success_message_distinguishes_playable_export_from_beautified_export`

Manual Gates：

- `npm run tauri:dev`：确认 no-FFmpeg Gate 文案和 `outputPath: None`。
- `npm run tauri:dev:ffmpeg`：录制 20 秒，source 可播放。
- 默认美化开启：导出中必须看到光标和点击效果。
- 导出 Bilibili / Douyin / Xiaohongshu：尺寸正确、光标位置映射正确。
- auto-trim 多段导出：第二段画面/声音从正确位置开始。
- 含 audible tone 的 cut 区间：导出中不可听见。
- 1080p 10 分钟压力：source/export A/V drift 在阈值内。
- cancel export：partial output 删除，source 保留。

### 19.14 当前建议对外状态表述

建议整改前统一使用下面口径：

> Phase 6 的 FFmpeg feature 构建已修复 BUG-004 的主要代码根因：export packet time base 已按 muxer 真实 stream time base rescale，writer 视频 PTS 已基于真实 timestamp，artifact validation 已检查 video/audio stream-level duration drift。当前 `cargo test --features ffmpeg`、默认 Rust tests、前端 tests 和 build 均通过，三种 preset playable export 的自动化测试也通过。但 Phase 6 仍不能声明完成：默认美化开启时录制会隐藏系统光标，而 FFmpeg exporter 仍未应用 `effect_timeline_path` 绘制光标效果，导致导出视频没有光标/没有美化（BUG-005）。此外 writer 仍同步运行在录制 consumer thread，audio cut boundary、Native Safety Gate 和 1080p 10 分钟 manual gates 仍未完成。下一轮应优先关闭 BUG-005 的产品/架构决策，并实现 cursor compositor 或临时恢复 raw cursor 可见，随后再处理 worker-backed writer、audio boundary 和 manual gates。

## 20. Phase 6 FFmpeg playable export 第 19 节整改后复审与 BUG-005/BUG-006 定位（2026-05-31，HEAD `8986b4a` + worktree）

### 20.1 审查范围与总体结论

本节基于 `HEAD 8986b4a` 加当前 dirty worktree，对第 19 节整改后的 Phase 6 FFmpeg playable export 相关改动做复审。重点范围：

- 文档：`docs/architecture/project-architecture-and-overall-planning.md`、`docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`、`docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`、本 review 文件第 18/19 节、`BUG.md`。
- 前端录制入口：`src/App.tsx`。
- macOS 音频采集：`src-tauri/src/platform/macos/cpal_microphone.rs`、系统音频捕获链路。
- 音频同步与混音：`src-tauri/src/media/audio_synchronizer.rs`、`src-tauri/src/media/audio_mixer.rs`。
- FFmpeg writer 与 artifact validation：`src-tauri/src/media/ffmpeg_writer.rs`、`src-tauri/src/media/ffmpeg_common.rs`。
- FFmpeg trim/export：`src-tauri/src/media/trim_exporter.rs`、`src-tauri/src/media/cursor_overlay.rs`。
- Tauri export command 与进度/feature gate：`src-tauri/src/lib.rs`。

总体结论：第 19 节中针对 BUG-004 的核心整改方向正确，当前代码已经把 export packet rescale 改为使用 `write_header()` 后的真实 output stream time base，并把 source writer 的视频 PTS 从固定帧序号改为基于真实 frame timestamp。这解决了“导出视频流被压缩到 0.03s”这一主根因。

但 Phase 6 仍不能声明完成，也不建议进入合并或对外发布状态。当前至少还有 4 个阻断问题：

1. `BUG.md` 新增的麦克风失败 BUG-005 仍未修复，根因是前端固定请求 48kHz stereo，后端 CPAL 直接拿该配置构建输入流，没有和真实设备能力协商。
2. `BUG.md` BUG-006 现象一仍未修复，根因是 writer 丢弃 `MixedAudioChunk.timestamp`，把系统音频当作连续音频写入，无法表达系统音频开始晚、间歇性输出或尾部静音。
3. `BUG.md` BUG-006 现象二仍未修复，根因是 cursor overlay 的 `scale/radius/x/y` 没有有限值和范围保护，debug 构建下 `i32` 乘法溢出 panic。
4. 新增 cursor overlay 在 auto-trim 多段导出下存在时间轴错位风险，exporter 用裁剪后的 output PTS 查询 source cursor timeline，剪掉前段后后续光标会按错误时间取样。

### 20.2 已验证命令

本轮复审期间已执行并通过的 focused verification：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cursor_overlay
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg ffmpeg_writer_without_audio_generates_silent_track
cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg cut_export_duration_within_tolerance
```

说明：

- 上述命令均通过，但仍有既有 macOS FFI/private interface 等 warning。
- 上述测试只能证明当前新增的局部路径可跑通，不能覆盖真实麦克风设备协商、系统音频稀疏时间轴、cursor timeline 非法数值、auto-trim 多段 cursor retiming 和 1080p 10 分钟压力场景。
- 本节是 review 文档追加，不代表业务代码已经修复 BUG-005/BUG-006。

### 20.3 Positive Findings

- BUG-004 的 exporter time base 修复是正确方向。`trim_exporter.rs:562-566`、`trim_exporter.rs:889-893` 使用 `enc_pkt.rescale_ts(audio_enc_tb, audio_out_tb)` / `enc_pkt.rescale_ts(video_enc_tb, video_out_tb)`，而 `audio_out_tb` / `video_out_tb` 来自 `write_header()` 后 muxer 的真实 stream time base，避免再次把 encoder time base 假定为 muxer time base。
- writer 视频 PTS 已改为真实 timestamp 模型。`ffmpeg_writer.rs:16-22` 在 `EncoderMessage::Video` 里传入 `timestamp_nanos`，`ffmpeg_writer.rs:112-118` 从 `VideoFrameRef.timestamp.nanos` 入队，避免继续用单纯 frame count 伪造录制时间轴。
- validation 已扩大到 stream-level duration。`ffmpeg_common.rs:192-210` 检查 video stream duration、audio stream duration 以及 video/audio drift，能够捕获 BUG-004 类型的“container 看起来有时长，但 video stream 实际极短”的坏文件。
- 第 19 节要求的 cursor compositor 已经有第一版实现：`cursor_overlay.rs` 可读取 `EffectTimeline`，并在 `trim_exporter.rs:878-880` 编码前叠加光标。这关闭了“exporter 完全不读 `effect_timeline_path`”这个方向性缺口，但实现仍有下面的数值安全和时间轴问题。

### 20.4 Critical 1：麦克风输入流配置未协商，导致 BUG-005

现象对应 `BUG.md` 未解决区 BUG-005：

- 开启麦克风后开始录制，界面报错：`音频捕获失败：构建麦克风输入流失败：The requested stream configuration is not supported by the device.`

证据：

- 前端录制入口固定发送 48kHz stereo：`src/App.tsx:149-155`。
- macOS CPAL microphone 读取 `default_input_config()` 后，仍优先使用请求里的 `config.sample_rate` / `config.channels`：`cpal_microphone.rs:87-110`。
- 之后直接用该 `StreamConfig` 调 `device.build_input_stream(...)`：`cpal_microphone.rs:230-263`。如果真实设备默认只支持 44.1kHz、单声道、或某个特定 channel layout，该调用就会失败。

根因：

当前链路把“业务希望的混音输出格式 48kHz stereo”和“设备输入流必须支持的硬件格式”混为一谈。`AudioMixer` 已经在 `audio_mixer.rs:57-68` 将任意输入 resample 到 48kHz stereo，因此 CPAL 输入端不应该强制要求设备按 48kHz stereo 打开。

整改方案：

1. 在 macOS CPAL microphone 端实现配置协商：
   - 优先使用 `device.default_input_config().config()` 作为实际 `StreamConfig`。
   - 如果用户选择了设备或将来 UI 需要指定采样率/通道数，只能从 `supported_input_configs()` 中选择真实支持的配置，不能盲传 UI 请求值。
   - 保留 `supported_config.sample_format()`，并保证 sample format 与实际 stream config 同源。
2. `AudioChunk` 继续记录真实 `sample_rate` / `channels`。后续 `AudioMixer` 负责 resample 和 stereo 转换，不要在采集层伪装格式。
3. 错误信息要把 requested config、default config、supported config 摘要打印出来，便于真实设备排查。
4. 增加一个纯函数或小模块封装设备配置选择逻辑，便于单测覆盖“不支持 48kHz stereo 时回退默认配置”的路径。

建议补充测试：

- `cpal_microphone_uses_default_supported_config_when_requested_config_unsupported`
- `cpal_microphone_records_actual_sample_rate_and_channels_in_audio_chunk`
- `microphone_capture_error_includes_requested_and_supported_config`

修复验收：

- 在用户当前失败设备上，开启麦克风可以开始录制。
- source 文件 QuickTime/IINA/VLC 可播放。
- `ffprobe` 显示 audio stream 存在，video/audio duration drift 在阈值内。

### 20.5 Critical 2：writer 丢弃音频时间戳，导致 BUG-006 系统音频时长漂移

现象对应 `BUG.md` BUG-006 现象一：

- 只开启系统音频，系统存在实际播放声音时，停止录制失败。
- 报错为：`录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms`。

证据：

- `audio_synchronizer.rs:71-88` 按 system chunk timestamp 输出混音结果。
- `audio_mixer.rs:63-67` 在 passthrough 场景保留 `MixedAudioChunk.timestamp`。
- 但 writer 入队消息只保留 samples：`ffmpeg_writer.rs:23-25`。
- `push_audio()` 明确丢弃 timestamp，只发送 `chunk.samples.to_vec()`：`ffmpeg_writer.rs:150-153`。
- worker 写 AAC 时使用连续 `audio_pts += 1024`：`ffmpeg_writer.rs:377-395`。这表示 writer 假设“收到的音频 chunk 从 0 开始连续不断”，无法表达前导静音、系统音频间歇空洞、尾部静音。
- 当前静音兜底只覆盖 `mixed_audio_chunk_count == 0` 的完全无音频场景。只要系统捕获到过一些 chunk，就不会按视频时长补齐真实音频 timeline。

根因：

系统音频捕获天然可能是稀疏的：用户开始录制后几秒才播放声音，中途暂停播放，或结束前没有声音。同步器和 mixer 已经保留 timestamp，但 writer 把 timestamp 丢了，最终 AAC stream duration 只等于“有声样本总长度”，不是“录制时间轴长度”。第 19 节新增的 stream-level duration validation 正好把这个真实 bug 暴露出来。

整改方案：

1. 扩展 `EncoderMessage::Audio`：
   - `timestamp_nanos: u64`
   - `sample_rate: u32`
   - `channels: u16`
   - `samples: Vec<f32>`
2. worker 维护 audio timeline cursor，单位建议使用 48kHz stereo 的 frame/sample 位置：
   - `target_sample = timestamp_nanos * 48000 / 1_000_000_000`
   - 如果 `target_sample > audio_cursor`，插入 silence 填补前导或中间 gap。
   - 如果 `target_sample < audio_cursor`，按 overlap 策略丢弃已过期样本或裁剪重叠部分，避免 PTS 回退。
   - 每写出 1024 samples 更新 `audio_cursor`，AAC PTS 使用该 timeline cursor。
3. finish 时根据 video duration 或 last video timestamp 补齐尾部 silence：
   - 如果已有音频但 audio duration 小于 video duration，需要补尾部 silence。
   - 如果完全无音频，保留当前 silent track 兜底，但也应基于 video duration 生成足够长的 silent AAC。
4. validation 继续保留，作为防回归保护。

建议补充测试：

- `ffmpeg_writer_pads_leading_gap_for_sparse_audio`
- `ffmpeg_writer_pads_middle_gap_for_interrupted_system_audio`
- `ffmpeg_writer_pads_audio_tail_to_video_duration`
- `ffmpeg_writer_preserves_av_duration_with_system_audio_gaps`
- `ffmpeg_writer_trims_overlapping_or_late_audio_without_pts_regression`

修复验收：

- 只开系统音频，录制开始后延迟播放音乐，不应在停止时失败。
- 中途暂停系统声音再恢复，不应产生 video/audio duration drift。
- source 文件和 export 文件都应有接近视频时长的 audio stream，允许阈值内 drift。

### 20.6 Critical 3：cursor overlay 未限制 scale/radius，导致 BUG-006 导出 panic

现象对应 `BUG.md` BUG-006 现象二：

- 当前系统没有音频输出时，录制能成功，但美化界面导出失败。
- 终端 panic：`thread 'tokio-rt-worker' panicked at src/media/cursor_overlay.rs:263:30: attempt to multiply with overflow`。

证据：

- `cursor_overlay.rs:184-185` 将 output PTS 转成 timeline timestamp。
- `cursor_overlay.rs:192-195` 直接使用 timeline 中的 cursor `x/y` 映射结果。
- `cursor_overlay.rs:208` 用 `self.cursor_radius * cursor_frame.scale` 得到 radius，没有检查 `scale` 是否 finite，也没有上限 clamp。
- `cursor_overlay.rs:251` 计算 `r2 = radius * radius`，`cursor_overlay.rs:263` 计算 `dx * dx + dy * dy`，`cursor_overlay.rs:286-299` outline 路径也有同类 unchecked `i32` 乘法。

根因：

cursor effect timeline 来自录制时的外部输入和动画计算，不应被视为可信数值。`scale` 可能异常大、非有限值、或由错误时间轴插值得到极端值。当前 rasterizer 用 `i32` 做平方，debug 构建会 panic，release 构建则可能溢出后产生错误绘制。

整改方案：

1. 在进入映射和绘制前校验 timeline frame：
   - `x.is_finite() && y.is_finite() && scale.is_finite()`。
   - `scale` clamp 到明确范围，例如 `0.25..=4.0` 或结合设计规格确定。
2. radius clamp：
   - 最小值保留当前 `4px`。
   - 最大值建议限制到 `min(frame_w, frame_h) / 2` 且再加一个绝对上限，例如 `256px`，避免大半屏扫描。
3. 距离计算改用 `i64` 或 saturating arithmetic：
   - `let dx = x as i64 - cx_i as i64`
   - `let dist2 = dx * dx + dy * dy`
   - `r2` / `inner_r2` 同样用 `i64`。
4. 对 off-canvas 坐标提前 return 或 clamp bounding box，避免 `frame_h - 1` 等边界在异常尺寸下产生二次问题。
5. export worker 不应因为 cursor overlay 单帧异常 panic；应跳过异常 overlay frame 或返回可诊断 `ExportFailed`。

建议补充测试：

- `cursor_overlay_clamps_huge_scale_without_panic`
- `cursor_overlay_skips_non_finite_timeline_values`
- `cursor_overlay_handles_off_canvas_coordinates_without_overflow`
- `cursor_overlay_draw_circle_uses_wide_integer_distance_math`

修复验收：

- 用包含巨大 `scale`、`NaN/inf`、极端 off-canvas 坐标的 effect timeline 导出，不 panic。
- 正常 timeline 下 cursor 仍可见，点击放大环仍显示。

### 20.7 Critical 4：裁剪导出后 cursor overlay 使用 output time 查询 source timeline

现象：

- 这是 review 中发现的潜在端到端 correctness bug，未必已经被 `BUG.md` 单独记录。
- 当 auto-trim 或手动剪辑删除中间片段后，第二段及后续视频帧的 cursor overlay 可能取错时间点的 cursor frame。

证据：

- `trim_exporter.rs:652-675` 把 source raw PTS 扣除 `cumulative_cut_nanos`，生成连续的 output PTS。
- `trim_exporter.rs:878-880` 调 `overlay.draw_on_frame(&mut output_frame, out_pts, out_fps)`。
- `cursor_overlay.rs:170-185` 明确把 `output_pts` 当作 `1/fps` 单位，转换为 nanoseconds 后查询 `EffectTimeline`。
- 但 `EffectTimeline` 的 frame timestamp 是原始录制 source timeline，而不是删除 cut gap 后的 output timeline。

根因：

video/audio 在 trim exporter 中被 retime 到连续 output timeline，但 cursor timeline 没有同步 retime。对第一段通常看不出问题；从第二段开始，`out_pts` 比 source timestamp 少了已删除片段时长，所以 overlay 会查询过早的 cursor frame。

整改方案有两种，建议选 A：

- 方案 A：exporter 传 source timestamp 给 overlay。
  - 在解码帧处保留 `raw_pts` 对应的 source timestamp nanos。
  - `draw_on_frame` 改为接收 `source_timestamp_nanos`，不要在 overlay 内从 output PTS 反推。
  - 该方案最直接，cursor timeline 继续保持 source-time 语义。
- 方案 B：在 export 开始前把 cursor timeline 按 `CutTimeline` retime 成 output timeline。
  - 删除 cut 区间内 cursor frames。
  - 对保留区间的 frames 扣除累计 cut duration。
  - 复杂度更高，但如果未来有 output-time 特效，可能更统一。

建议补充测试：

- `ffmpeg_exporter_uses_source_time_for_cursor_overlay_after_cut_gap`
- `cursor_timeline_retiming_drops_frames_inside_cut_range`
- `cursor_overlay_position_after_second_kept_segment_matches_source_cursor`

修复验收：

- 构造 cursor 在 cut 前后位置明显不同的测试素材。
- 导出多段 keep segment 后，第二段光标位置必须对应 source timestamp，而不是 output timestamp。

### 20.8 Important Issues

1. writer 已 worker-backed，但 producer 仍可能阻塞录制 consumer thread。
   - `ffmpeg_writer.rs:50` 使用 `mpsc::sync_channel`。
   - `ffmpeg_writer.rs:110-112` 注释称“Non-blocking enqueue”，但实际 `SyncSender::send()` 在队列满时会阻塞。
   - `ffmpeg_writer.rs:150-153` audio 入队同样会阻塞。
   - `ffmpeg_writer.rs:785-803` 测试名和注释也在验证 blocking backpressure。
   - 这比第 18 节时同步编码已有改善，但仍不满足“录制 consumer thread 不被 encoder 反压长时间阻塞”的捕获链路安全要求。
   - 建议后续改为 `try_send` 加显式 `QueueFull` 策略，或前置 drop policy/环形队列，并把丢帧/丢音频统计纳入 RecordingResult。

2. no-FFmpeg branch 仍会返回 MockTrimExporter 结果，与 gate-only 目标不一致。
   - `lib.rs:827-841` 在 `#[cfg(not(feature = "ffmpeg"))]` 下仍调用 `MockTrimExporter::new()` 并传入 `Some(output_path)`。
   - 第 19 节计划要求 no-FFmpeg 构建只能给出 gate 文案，不能让 UI 误以为产生了真实 playable export。
   - 建议 no-FFmpeg branch 返回明确 gate result，`output_path: None`，前端提示“需要 FFmpeg feature 构建才能导出”。

3. export 失败或取消后没有统一 terminal progress event。
   - `lib.rs:857-869` 只在成功后 emit `progress: 100, cancellable: false, output_path: Some(...)`。
   - `lib.rs:885-897` 虽然清理了 cancel token，但对 failure/cancel 没有 emit `cancellable: false` 和 error payload。
   - 如果 UI 依赖 progress event 收尾，失败路径可能残留可取消状态或无法展示具体错误。
   - 建议在 `do_export.await` 后统一 match result，成功/失败/取消都 emit terminal `export-progress`。

4. seek 后 decoder flush 仍是 Native Safety / correctness 待确认项。
   - `trim_exporter.rs:581-587` seek 后显式说明 omit decoder flush。
   - 注释解释了避免 PTS monotonicity 问题的原因，但仍需要更强证据：多段 cut 边界、非关键帧起点、B-frame/不同 GOP 素材、画面首帧污染风险。
   - 建议不要仅靠注释关闭该风险，需要补集成测试和 Native Safety review；如保留 omit 策略，应记录适用条件和不变量。

### 20.9 Minor / 文档问题

- `BUG.md` 当前存在重复编号：未解决区 `BUG-005: 音频捕获失败`，已解决区也有 `BUG-005: 导出的视频没有美化（默认美化开启时无光标）`。建议立即重编号，避免后续整改和 review 引用混乱。可将已解决的 cursor beautification 问题改为 `BUG-004` 后续子项或 `BUG-007`，把未解决麦克风问题保留为当前 BUG-005；也可以统一按时间顺序重新编号，但要更新所有 review 引用。
- `ffmpeg_common.rs:192-193` 有重复/截断注释：`// Stream-level duration checks to catch PT` 和下一行完整注释重复。建议清理为一行，避免后续读者误判为未完成编辑。
- `ffmpeg_writer.rs:110` 注释“Non-blocking enqueue”与 `send()` 实际阻塞行为不一致。即使暂不修实现，也应先改注释，避免 review 和维护人员误判捕获链路安全性。

### 20.10 BUG.md 预防规则复核

按项目约定，本轮 code review 额外检查了 `BUG.md` 中所有预防规则的遵循情况：

- BUG-004 已解决区预防规则基本已落实：
  - muxer 写包使用 `write_header()` 后的真实 output stream time base。
  - validation 已检查 stream-level duration。
  - writer video PTS 已不再只用 frame count。
  - 仍需 manual gate 继续覆盖 ffprobe duration/fps。
- 已解决区 cursor beautification 的预防规则部分落实：
  - exporter 已实际读取并应用 effect timeline。
  - raw cursor hidden 时不再完全静默产出无光标视频。
  - 但当前 overlay 仍缺少数值安全和 trim retiming 保护，因此还不能认为该规则完整闭环。
- 未解决 BUG-005/BUG-006 尚未有修复后的预防规则。建议修复时写入以下规则：
  - 麦克风采集不得把 UI 目标格式直接作为 CPAL device stream config；必须从设备 supported/default config 协商得到实际输入格式。
  - 音频输入 chunk 必须携带真实 sample_rate/channels/timestamp；统一输出格式只能在 mixer/writer timeline 层完成。
  - writer 必须尊重 mixed audio timestamp；对前导 gap、中间 gap、尾部 gap 写入 silence，不能把稀疏音频压缩成连续短音轨。
  - cursor/effect timeline 来自外部输入，所有 `x/y/scale/timestamp` 参与 rasterization 前必须 finite check 和范围 clamp。
  - overlay 距离计算不得依赖 debug/release 不同行为；平方和半径计算必须使用足够宽的整数类型或 saturating arithmetic。
  - trim/export 中所有 source-time metadata，包括 cursor timeline、click effect、未来字幕/高亮，都必须显式声明并测试 source timeline 到 output timeline 的转换策略。

### 20.11 建议整改 Phase

#### R1：BUG.md housekeeping 与当前状态澄清

任务：

1. 修正 `BUG.md` 重复 BUG-005 编号。
2. 把 BUG-006 拆成两个可独立验证的条目：
   - 系统音频稀疏时间轴导致 source writer duration drift。
   - cursor overlay 数值溢出导致 export panic。
3. 在 `HANDOFF.md` 标注 Phase 6 当前真实状态：BUG-004 主路径已修，BUG-005/BUG-006 未修，Phase 6 不可声明完成。

验证：

- review 文档、BUG.md、HANDOFF.md 对 BUG 编号一致。
- 后续整改 issue/测试名不再引用冲突编号。

#### R2：修复麦克风 CPAL 配置协商

任务：

1. 抽出配置选择逻辑，输入 requested config、default config、supported configs，输出实际 stream config。
2. CPAL input stream 使用设备实际支持的 sample_rate/channels/sample_format。
3. `AudioChunk` 保留实际采集格式，交给 mixer resample 到 48kHz stereo。
4. 增加诊断日志和错误信息。

验证：

- 单测覆盖 unsupported requested config fallback。
- 真实设备 manual gate：开启麦克风开始录制不失败。

#### R3：修复 writer 音频 timestamp 与 silence padding

任务：

1. `EncoderMessage::Audio` 携带 timestamp/sample_rate/channels/samples。
2. worker 维护 48kHz stereo audio timeline cursor。
3. 对前导、中间、尾部 gap 写入 silence。
4. 完全无音频路径继续生成 silent audio track，但长度必须按 video duration。
5. 保留 stream-level validation。

验证：

- sparse audio 单测和集成测试通过。
- 只开系统音频，延迟播放/间歇播放/尾部无声均不触发 duration drift。

#### R4：修复 cursor overlay 数值安全

任务：

1. 对 timeline frame 的 `x/y/scale` 做 finite check。
2. 对 scale/radius 做明确 clamp。
3. `draw_circle` / `draw_circle_outline` 使用 `i64` 距离计算。
4. 对异常 timeline frame 选择 skip 或返回结构化 export error，不能 panic。

验证：

- 巨大 scale、NaN/inf、off-canvas 坐标测试不 panic。
- 正常 cursor overlay 像素测试仍通过。

#### R5：修复 trim 后 cursor overlay 时间轴

任务：

1. 明确 `EffectTimeline` 的 timestamp 语义是 source time。
2. exporter 给 overlay 传 source timestamp，或在 export 前把 cursor timeline retime 到 output time。
3. click effect 和未来 effect metadata 采用同一时间轴策略。

验证：

- 多段 cut 后第二段 cursor 位置正确。
- cut gap 前后 cursor 位置差异明显的 fixture 能防回归。

#### R6：no-FFmpeg gate、terminal progress 与 UI 状态收口

任务：

1. no-FFmpeg branch 不再调用 MockTrimExporter 产出 output path。
2. `ExportSummaryPayload.output_path` 在 no-FFmpeg gate 下为 `None`。
3. success/failure/cancel 都 emit terminal `export-progress`，且 `cancellable: false`。
4. 前端 preview 文案区分“需要 FFmpeg 构建”和“导出成功”。

验证：

- `npm run tauri:dev` 默认构建下点击导出不会显示成功文件路径。
- cancel/failure UI 不残留可取消状态。

#### R7：writer backpressure 与 Native Safety gate

任务：

1. 将 producer 入队阻塞风险收口，明确 drop/backpressure 策略。
2. 记录 dropped frames/audio chunks，并进入 RecordingResult 或诊断日志。
3. Native Safety review 覆盖：
   - unsafe plane copy 边界。
   - FFmpeg context ownership/drop。
   - encoder/decoder flush。
   - worker thread join 和 error propagation。
   - capture consumer thread 不被长时间阻塞。

验证：

- 慢 encoder 或小队列测试不会无限阻塞 capture consumer。
- 1080p 10 分钟压力录制 source/export 都可播放，A/V drift 在阈值内。

### 20.12 建议补充测试与 Manual Gates

Rust / CPAL microphone：

- `cpal_microphone_uses_default_supported_config_when_requested_config_unsupported`
- `cpal_microphone_records_actual_sample_rate_and_channels_in_audio_chunk`
- `microphone_capture_error_includes_requested_and_supported_config`

Rust / writer sparse audio：

- `ffmpeg_writer_pads_leading_gap_for_sparse_audio`
- `ffmpeg_writer_pads_middle_gap_for_interrupted_system_audio`
- `ffmpeg_writer_pads_audio_tail_to_video_duration`
- `ffmpeg_writer_preserves_av_duration_with_system_audio_gaps`
- `ffmpeg_writer_trims_overlapping_or_late_audio_without_pts_regression`

Rust / cursor overlay safety：

- `cursor_overlay_clamps_huge_scale_without_panic`
- `cursor_overlay_skips_non_finite_timeline_values`
- `cursor_overlay_handles_off_canvas_coordinates_without_overflow`
- `cursor_overlay_draw_circle_uses_wide_integer_distance_math`

Rust / trim cursor retiming：

- `ffmpeg_exporter_uses_source_time_for_cursor_overlay_after_cut_gap`
- `cursor_timeline_retiming_drops_frames_inside_cut_range`
- `cursor_overlay_position_after_second_kept_segment_matches_source_cursor`

Rust / export command：

- `export_video_non_ffmpeg_returns_gate_without_output_path`
- `export_video_failure_emits_terminal_progress_error`
- `export_video_cancel_emits_terminal_progress_cancelled`

Manual Gates：

- `npm run tauri:dev:ffmpeg`，开启麦克风，在当前失败设备上开始录制，不出现 CPAL unsupported config。
- 只开启系统音频，录制开始后延迟播放音乐，停止录制 source 成功生成且可播放。
- 只开启系统音频，中途暂停/恢复播放，source 和 export 的 audio duration 接近 video duration。
- 当前系统无音频输出，默认美化开启，录制后导出不 panic。
- 默认美化开启，auto-trim 多段导出，第二段及后续 cursor 位置与 source timeline 对齐。
- no-FFmpeg 默认构建点击导出，只显示 feature gate，不返回 output path。
- export 失败和 cancel 后 UI 收到 terminal progress，取消按钮/状态正确收尾。
- 1080p 10 分钟压力：source/export QuickTime、IINA、VLC 可播放，ffprobe stream-level duration/fps 在阈值内。

### 20.13 当前建议对外状态表述

建议本轮整改前统一使用下面口径：

> Phase 6 的 FFmpeg playable export 已修复 BUG-004 的主要 time base 根因，并通过 focused cargo tests 覆盖 cursor overlay、无音频 silent track 和 cut export duration。但第 19 节整改后，真实录制又暴露出新的阻断问题：麦克风开启时 CPAL 输入配置未协商导致采集失败；系统音频为稀疏时间轴时 writer 丢弃 timestamp，导致 audio stream duration 明显短于 video stream；默认美化导出时 cursor overlay 对异常 scale/radius 缺少保护，可能在 debug 构建 panic；此外 auto-trim 后 cursor overlay 仍存在 source time 与 output time 混用风险。因此 Phase 6 仍不能声明完成。下一轮应按 R1 到 R5 优先关闭 BUG.md 中未解决项，再处理 no-FFmpeg gate、terminal progress、backpressure 和 Native Safety/manual gates。
