# Phase 6 / W11-W12 自测清单：导出预设与本地授权

## 目标

验证 Phase 6 完成以下范围：

1. Phase 5 遗留 FFmpeg Gate 与 trim sensitivity 完整重聚合被纳入 Phase 6 前置导出流水线。
2. 16:9、9:16、1:1 三种固定预设可导出独立 playable output。
3. 导出进度、取消、失败清理可用。
4. 本地 14 天试用状态与激活状态接口可展示。
5. MVP 仍保持数据流红线：媒体帧、音频块、activity stream 不进入 React。

## Verification Summary

- [ ] `git diff --check`: 待执行
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: 待执行
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`: 待执行
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: 待执行
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml`: 待执行
- [ ] `npm run build`: 待执行
- [ ] `npm test -- --run`: 待执行
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`: 待执行 / 如本机缺 FFmpeg dev libraries 需记录阻塞原因
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets`: 待执行 / 如本机缺 FFmpeg dev libraries 需记录阻塞原因

## Phase 6A：导出流水线前置验证

- [ ] 导出预设集中定义在 Rust 侧，且只包含 `bilibili`、`douyin`、`xiaohongshu`。
- [ ] 16:9 预设固定为 1920x1080，非模板系统。
- [ ] 9:16 预设固定为 1080x1920，非模板系统。
- [ ] 1:1 预设固定为 1080x1080，非模板系统。
- [ ] 导出输出路径与原始录制路径不同。
- [ ] `outputPath` 只在文件真实存在且非空后返回。
- [ ] `outputPath` 不再被 FFmpeg Gate 伪造。
- [ ] `export_video()` 通过结构化 `TrimExportRequest` / `TrimExporter` 边界消费裁剪时间线。
- [ ] auto-trim off 时仍形成完整导出请求。
- [ ] auto-trim on 时读取并消费 `CutTimeline`。
- [ ] writer `push_video` 失败会导致最终录制/导出失败。
- [ ] writer `push_audio` 失败会导致最终录制/导出失败。
- [ ] writer `finish` 失败会导致最终录制/导出失败。
- [ ] writer 失败后仍执行 capture stop、mic stop、consumer join、mic reset。
- [ ] FFmpeg feature 启用时，`FfmpegRecordingWriter` 会生成真实原始录制 artifact。
- [ ] `stop_recording` 只在原始录制 artifact 存在、非空、可被 FFmpeg binding 检查后返回非空 `outputPath`。
- [ ] 原始录制 artifact 含视频轨、音频轨、非零时长和非零文件大小。
- [ ] `FfmpegRecordingWriter` push/finish 失败时不返回伪造 `outputPath`。
- [ ] 原始录制 artifact 路径唯一，不复用已有路径。

## Phase 6B：Trim Sensitivity 完整重聚合验证

- [ ] 录制期保存 sensitivity-independent base RMS bucket。
- [ ] base RMS bucket 默认 100ms 粒度。
- [ ] `TrimMetadata` 有 schema/version 字段。
- [ ] 旧 `audioActivity` sidecar 可兼容读取。
- [ ] High sensitivity 可从同一 base buckets 聚合出 500ms RMS window。
- [ ] Medium sensitivity 可从同一 base buckets 聚合出 750ms RMS window。
- [ ] Low sensitivity 可从同一 base buckets 聚合出 1000ms RMS window。
- [ ] 修改 Preview sensitivity 后无需重新录制即可重建 cut timeline。
- [ ] `SilenceDetectorEngine` 继续消费派生后的 `AudioActivitySample`，不直接依赖 React 或原始音频流。

## Phase 6C：FFmpeg Playable Export 验证

- [ ] FFmpeg 集成使用 Rust binding / C API。
- [ ] 未直接调用 FFmpeg CLI。
- [ ] 未拼接用户输入为命令行字符串。
- [ ] 原始录制 artifact 可播放且路径非空。
- [ ] 导出 artifact 与原始录制 artifact 分离。
- [ ] 导出不会删除或覆盖原始录制 artifact。
- [ ] auto-trim off 可导出完整 playable file。
- [ ] auto-trim on 可导出 trimmed playable file。
- [ ] cut timeline 有裁剪段时，导出时长短于原始素材。
- [ ] cut timeline no-op 时，导出时长与原始素材可解释地接近。
- [ ] Bilibili / YouTube 16:9 导出可播放。
- [ ] Douyin 9:16 导出可播放。
- [ ] Xiaohongshu 1:1 导出可播放。
- [ ] 导出视频含视频轨。
- [ ] 导出视频含音频轨。
- [ ] 导出视频尺寸通过 FFmpeg binding 检查，16:9 为 1920x1080。
- [ ] 导出视频尺寸通过 FFmpeg binding 检查，9:16 为 1080x1920。
- [ ] 导出视频尺寸通过 FFmpeg binding 检查，1:1 为 1080x1080。
- [ ] 导出时长通过 FFmpeg binding 检查，auto-trim off 与原始素材时长差异在可解释范围内。
- [ ] 导出时长通过 FFmpeg binding 检查，auto-trim on 在存在 cut 时短于原始素材。
- [ ] 音视频同步无明显漂移。
- [ ] FFmpeg 导出失败返回中文结构化错误。
- [ ] FFmpeg 导出失败不返回 fake `outputPath`。
- [ ] FFmpeg 取消后清理 partial output。
- [ ] FFmpeg 导出进度来自 exporter callback，至少包含一个 1-99 的中间进度值。

### FFmpeg Artifact Evidence Log

| Scenario | Source path | Output path | Output bytes | Width x Height | Video stream | Audio stream | Source duration | Output duration | Duration delta | Original exists before/after | Inspector | Date |
|---|---|---|---:|---|---|---|---:|---:|---:|---|---|---|
| Original recording artifact |  | n/a |  |  |  |  |  | n/a | n/a | n/a |  |  |
| 16:9 auto-trim off |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| 9:16 auto-trim off |  |  |  | 1080x1920 |  |  |  |  |  |  |  |  |
| 1:1 auto-trim off |  |  |  | 1080x1080 |  |  |  |  |  |  |  |  |
| 16:9 auto-trim on |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| Cancel cleanup |  |  | n/a | n/a | n/a | n/a |  | n/a | n/a |  |  |  |

## Phase 6D：导出 UI / 进度 / 取消验证

- [ ] React 只发送导出命令与展示 summary/path/progress，不接收视频帧或音频块。
- [ ] Preview 可展示导出进度。
- [ ] Preview 进度不是只显示 0/100，后端测试已证明存在中间进度。
- [ ] Preview 可点击取消导出。
- [ ] 导出过程中按钮禁用或防重入。
- [ ] 导出成功且 `outputPath` 存在时显示“已生成可播放导出文件”。
- [ ] `outputPath` 为空时仍明确展示 FFmpeg Gate 文案。
- [ ] 导出失败时显示中文错误。
- [ ] 用户修改美化配置后旧 export summary 被清空。
- [ ] in-flight export 的旧结果不会覆盖新配置状态。

## Phase 6E：授权验证

- [ ] 首次启动可生成本地 14 天试用状态。
- [ ] UI 可显示试用剩余天数。
- [ ] 试用过期状态可展示。
- [ ] 已激活状态可展示。
- [ ] `license_status` command 存在。
- [ ] `activation_status` command 存在。
- [ ] `activate_license` command 存在且空激活码返回中文错误。
- [ ] 未实现服务端激活协议。
- [ ] 已激活状态来自 `ActivationCredentialStore` 抽象，不从普通 JSON 试用文件读取。
- [ ] 文件持久化只保存本地试用开始时间，不保存 activation entitlement。
- [ ] 本地试用 JSON 拒绝 `activatedAtSecs` 等 entitlement 字段。
- [ ] 产品默认 credential store 在服务端协议未接入前不伪造激活状态。
- [ ] 未硬编码激活私钥、服务端密钥、Sentry DSN 或 PostHog Key。
- [ ] 授权状态持久化路径不在 React 层处理。
- [ ] 授权系统未扩展成商业化后端。
- [ ] 360px、768px、桌面宽度下授权 badge 不遮挡开始录制、预览导出或返回按钮。

## Phase 6F：长录制与性能 Gate

- [ ] 10 分钟 1080p 录制后 trim metadata sidecar 大小已记录。
- [ ] 10 分钟 1080p 录制停止耗时已记录。
- [ ] 10 分钟 1080p 录制导出耗时已记录。
- [ ] 10 分钟 1080p 录制内存峰值已记录。
- [ ] 导出任务不阻塞 Tauri 主事件循环。
- [ ] ScreenCaptureKit callback 未加入 RMS、frame diff、JSON 写入或 FFmpeg 处理。
- [ ] visual diff / audio activity 仍在 consumer 或后处理路径。

## BUG.md 预防规则扫描

- [ ] 未新增 `data-tauri-drag-region="false"` 容器级 wrapper。
- [ ] 未新增 `setIgnoreCursorEvents(true)` 或类似全窗口点击穿透回归。
- [ ] 未新增 `motion.div whileTap` 作为 Button/Link 等交互元素直接父容器。
- [ ] 如使用 `whileTap`，仅直接作用于按钮自身或改用 CSS `active:`。

扫描命令：

```bash
rg -n "whileTap|data-tauri-drag-region=\\{false\\}|data-tauri-drag-region=\\\"false\\\"|setIgnoreCursorEvents|ignoreCursor" src src-tauri
```

## Native Safety Gate

- [ ] `src-tauri/src/media/ffmpeg_writer.rs` 已人工逐行审查内存安全、线程安全、资源释放。
- [ ] `src-tauri/src/media/trim_exporter.rs` 已人工逐行审查 FFmpeg context/packet/frame 生命周期。
- [ ] `src-tauri/src/platform/macos/screen_capture_kit.rs` SCK callback 未引入新的 use-after-free 或阻塞路径。
- [ ] 授权持久化实现已人工确认不硬编码敏感信息。
- [ ] 若启用系统 Keychain 或其他系统凭据 API，相关 FFI/平台调用已人工审查。

## 人工产品验收

- [ ] 录制 8-12 秒静音且静止画面，开启 auto trim 后 `cutCount >= 1`。
- [ ] 录制短暂停顿素材，确认不会误剪。
- [ ] 录制带加载动画/鼠标移动素材，确认 visual change 能阻止误剪。
- [ ] 原始素材保留，可重新导出。
- [ ] 三种预设连续导出无崩溃。
- [ ] 导出取消后可再次导出。
- [ ] 没有新增平台发布 API。
- [ ] 没有引入字幕、摘要、知识库、模板系统。
- [ ] MVP 可交付 20 个种子用户测试。
