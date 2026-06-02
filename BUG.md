# bug 备忘清单

记录已知 bug。**重要**：每条 bug 修复后，都要总结对应的**预防规则**。

## 未解决

### BUG-005: 音频捕获失败

**当前状态**：第 25 节 review 整改中 — source-aware synchronizer、source-aware contract、CPAL lazy offset、writer partial-overlap diagnostics、蓝牙 mic 释放。待真实设备验证。

**修复进展**：

1. **CPAL 配置协商**（第 20 节 R2）：`cpal_microphone.rs` 使用 `device.default_input_config()` 获取真实设备配置，不再把 UI 目标格式当作硬件 stream config。
2. **writer audio timeline merge**（第 21 节 R3）：`ffmpeg_writer.rs` 的音频时间轴合并逻辑已重写。
3. **AudioMixer::to_stereo()**（第 21 节 R4）：多声道输入（>2ch）正确截取前两个通道。
4. **RequestedAudioArtifactContract**（第 24 节 R1）：新增 `RequestedAudioContract` 结构体和 `validate_source_artifact_with_audio_contract()` / `validate_export_artifact_with_audio_contract()`。录制结束后自动解码 artifact 并检查 decoded RMS/peak，当请求了音频但 artifact 静音时返回错误。
5. **AudioSynchronizer window merger**（第 24 节 R2）：重构为固定 20ms 窗口合并器，同一窗口只输出一个 mixed chunk，彻底解决 system/mic 双写同一时间轴问题。
6. **WriterDiagnostics**（第 24 节 R3）：新增 `WriterDiagnostics` 结构体，区分 queued/appended/discarded/trimmed/encoded，`RecordingResult` 携带写入器诊断。
7. **finish() non-blocking**（第 24 节 R4）：`finish()` 改为 `try_send(Flush)` + bounded retry，不再无限阻塞。
8. **strict synthetic artifact helper**（第 24 节 R5）：新增 `create_synthetic_source_artifact_strict()`，任何 push 失败立即返回错误。音频内容测试（RMS/peak 验证）使用 strict helper，backpressure 测试保留 tolerant helper。
9. **麦克风设备选择和蓝牙兼容提示**（第 24 节 R6）：后端新增 `list_microphone_devices` 命令返回设备列表和蓝牙检测。前端新增麦克风设备选择器，对蓝牙麦克风显示 HFP profile 兼容性警告。

**待验证**：需要在真实设备上开启麦克风（特别是 `24000Hz/1ch` 设备）录制 10 秒，确认不再出现 `音频 53s` 的偏差。
**验证结果**：

1. [x] 可以录制并导出，不会出现偏差报错。
2. [x] 只录系统音频 10 秒，播放音乐，验证 source/export 都可听。
3. [x] 只录内置麦克风 10 秒，说话，验证 source/export 都可听。
4. [x] 系统音频 + 内置麦克风 10 秒，验证两路都可听。
5. [x] 录制的源视频、导出的视频，播放时，都听不见系统音频声音、麦克风的声音，但是录制中顶部胶囊状态栏上有麦克风的波动反馈。（这个问题在只录制系统音频、同时录制系统音频和麦克风，这两种情况下都存在）
6. [x] 同时开启系统音频、麦克风录制，录制过程中从耳机里听到的电脑输出的系统音频声音音质变差，断断续续。结束录制后，耳机里声音就恢复正常了。
   - 该问题暂时标记为不可抗力。
7. 新的bug， **新 bug 现象**：
   - 系统音频 + 蓝牙麦克风 10 秒，验证停止后蓝牙音质**没有**恢复。
     - [ ] 情况1：同时开启系统音频、麦克风录制（蓝牙耳机麦克风），结束录制后，没有释放麦克风进程，导致音质一直都是处于很差的状态。
     - [ ] 情况2：同时开启系统音频、麦克风录制（系统默认麦克风，此时默认麦克风貌似是蓝牙耳机麦克风，因为音质变差了），结束录制后，可以释放麦克风进程，音质恢复。

以下是**新 bug 复现**时的终端日志：

```
麦克风配置协商: 请求 48000Hz/2ch, 设备实际 24000Hz/1ch
[libx264 @ 0x8fa094000] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x8fa094000] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
警告: 最终排空发现未配对的系统音频块
警告: 最终排空发现未配对的系统音频块
[aac @ 0x8fa094e00] Qavg: 3247.884
[libx264 @ 0x8fa094000] frame I:2     Avg QP:13.00  size:409994
[libx264 @ 0x8fa094000] frame P:290   Avg QP: 2.01  size: 17771
[libx264 @ 0x8fa094000] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x8fa094000] mb P  I16..4:  0.6%  0.0%  0.0%  P16..4: 20.7%  0.0%  0.0%  0.0%  0.0%    skip:78.7%
[libx264 @ 0x8fa094000] final ratefactor: 6.76
[libx264 @ 0x8fa094000] coded y,uvDC,uvAC intra: 38.9% 23.5% 21.7% inter: 8.3% 3.8% 3.5%
[libx264 @ 0x8fa094000] i16 v,h,dc,p: 55% 40%  2%  3%
[libx264 @ 0x8fa094000] i8c dc,h,v,p: 68% 20% 10%  2%
[libx264 @ 0x8fa094000] kb/s:4762.88
录制音频诊断: RecordingDiagnostics { requested_system_audio: true, requested_microphone: true, microphone_device: None, system_chunks_received: 499, mic_chunks_received: 493, system_chunks_dropped: 0, mic_chunks_dropped: 0, mixed_chunks_queued: 904, writer_push_audio_failures: 0, system_rms_max: 0.017147802, mic_rms_max: 0.09349334, mixed_rms_max: 0.088926174, generated_silent_track: false, paired_window_count: 88, system_only_window_count: 411, mic_only_window_count: 405 }
录制音频 contract 验证通过: RMS=0.006991, peak=0.039398, samples=972800
录制音频诊断摘要: RecordingDiagnostics { requested_system_audio: true, requested_microphone: true, microphone_device: None, system_chunks_received: 499, mic_chunks_received: 493, system_chunks_dropped: 0, mic_chunks_dropped: 0, mixed_chunks_queued: 904, writer_push_audio_failures: 0, system_rms_max: 0.017147802, mic_rms_max: 0.09349334, mixed_rms_max: 0.088926174, generated_silent_track: false, paired_window_count: 88, system_only_window_count: 411, mic_only_window_count: 405 }
写入器诊断摘要: WriterDiagnostics { audio_chunks_received: 904, audio_chunks_appended: 499, audio_chunks_discarded_full_overlap: 405, audio_chunks_trimmed_partial_overlap: 0, audio_real_frames_appended: 479040, audio_silence_frames_padded: 3840, audio_real_rms_max_before_encode: 0.014637499, aac_frames_encoded: 474, silent_aac_frames_encoded: 0, generated_silent_track: false, video_queue_full_count: 0, audio_queue_full_count: 0 }
[libx264 @ 0x8fa096300] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x8fa096300] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0x8fa096a00] Qavg: 2443.667
[libx264 @ 0x8fa096300] frame I:2     Avg QP:13.00  size:408898
[libx264 @ 0x8fa096300] frame P:288   Avg QP: 2.18  size: 13739
[libx264 @ 0x8fa096300] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x8fa096300] mb P  I16..4:  0.6%  0.0%  0.0%  P16..4: 14.9%  0.0%  0.0%  0.0%  0.0%    skip:84.5%
[libx264 @ 0x8fa096300] final ratefactor: 5.49
[libx264 @ 0x8fa096300] coded y,uvDC,uvAC intra: 39.9% 23.1% 21.3% inter: 5.8% 2.7% 2.3%
[libx264 @ 0x8fa096300] i16 v,h,dc,p: 52% 43%  3%  3%
[libx264 @ 0x8fa096300] i8c dc,h,v,p: 67% 21% 10%  2%
[libx264 @ 0x8fa096300] kb/s:3832.41
```

**现象**：

- 录制前开启 `麦克风`，点击开始录制
- `录制中界面` 没有报错，但是点击`结束录制`，会出现报错：
  - `录制写入器完成失败: 写入录制文件失败：录制视频/音频时长偏差过大：视频 10033ms，音频 53397ms，偏差 43363ms`

以下是终端日志：

```
     Running `target/debug/luzhi`
麦克风配置协商: 请求 48000Hz/2ch, 设备实际 24000Hz/1ch
[libx264 @ 0x8cbccc700] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x8cbccc700] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0x8cbccce00] Qavg: 50300.516
[libx264 @ 0x8cbccc700] frame I:2     Avg QP:12.50  size:269829
[libx264 @ 0x8cbccc700] frame P:281   Avg QP: 1.81  size: 33703
[libx264 @ 0x8cbccc700] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x8cbccc700] mb P  I16..4:  4.0%  0.0%  0.0%  P16..4: 16.3%  0.0%  0.0%  0.0%  0.0%    skip:79.7%
[libx264 @ 0x8cbccc700] final ratefactor: 7.00
[libx264 @ 0x8cbccc700] coded y,uvDC,uvAC intra: 36.8% 20.2% 19.6% inter: 7.6% 3.7% 3.3%
[libx264 @ 0x8cbccc700] i16 v,h,dc,p: 62% 34%  2%  2%
[libx264 @ 0x8cbccc700] i8c dc,h,v,p: 76% 15%  8%  1%
[libx264 @ 0x8cbccc700] kb/s:7955.15
录制写入器完成失败: 写入录制文件失败：录制视频/音频时长偏差过大：视频 10033ms，音频 53397ms，偏差 43363ms
```

---

**预防规则**：

- 麦克风设备 stream config 必须来自设备 default/supported config，UI 目标格式只能作为 mixer output target
- writer 对音频 timestamp 的处理必须同时覆盖 first-gap、middle-gap、tail-gap、overlap、out-of-order 五种情况
- `MixedAudioChunk.samples` 的布局必须与 `channels` 元数据一致；任何 downmix/truncate/resample 后都必须用测试验证 sample length 与 duration
- writer `audio_pts` 必须作为单调递增的编码器 PTS 计数器，不能与 timeline cursor 混用
- writer partial-overlap chunk append 后必须立即进入 AAC drain loop，不能跳过 drain 直接 continue（否则音频 buffer 累积到 finish 阶段）
- AudioMixer 入口必须校验 `channels > 0`、`sample_rate > 0`、`samples.len() % channels == 0`，不信任底层音频 chunk metadata
- "请求录制音频源"必须和"实际写入非静音音频内容"建立可验证 contract；不能只检查 audio stream 是否存在
- 麦克风 UI 电平只能作为 capture-side indicator，不能作为 recording artifact 成功证据
- system/mic synchronizer 必须 source-aware，同一时间窗口只输出一个 mixed chunk，不能让先到的单源 chunk 占用时间轴并吞掉后到的另一源
- capture channel drop count 必须进入 diagnostics；音频 drop 不能完全静默
- silent AAC track 只能用于"没有请求音频"的录屏兼容；用户请求音频时 silent track 必须触发 warning/error
- artifact validation 必须包含 audio RMS/peak 检查，不能只依赖 stream presence 和 duration
- FFmpeg writer queue 必须使用 non-blocking send，避免 capture consumer thread 被编码压力阻塞导致音频丢包
- consumer loop 必须使用 bounded batch 处理视频帧，避免音频被无限 drain video 饿死
- writer diagnostics 必须区分 queued（进入队列）和 encoded（实际编码进 AAC），不能用 queued 数冒充 encoded 数
- `finish()` 必须使用 bounded wait 策略（try_send + retry），不能无限阻塞等待编码队列
- 蓝牙耳机麦克风可能触发 macOS HFP profile 切换，建议用户选择内置麦克风作为输入设备

**新增预防规则（2026-06-02 Section 25 review）**：

- `AudioSynchronizer` 不能只按 chunk 起始 timestamp 归桶；真实音频 chunk 必须按 sample frame 切分到固定时间窗口
- 双源录制时 live watermark 不能由快的一路单独推进；在两个请求源都 active 时必须以慢源或 source-aware timeout 策略决定发射
- requested-audio contract 必须 source-aware；aggregate decoded RMS/peak 只能证明 artifact 非全静音，不能证明每个请求源都存在
- 当 capture-side 某请求源 RMS 非零但 writer/source-aware diagnostics 显示该源被大量 overlap discard 时，必须视为录制失败或至少阻断 BUG 关闭
- CPAL 麦克风 timestamp 不能在 stream build 时固定 offset；首帧 callback 或设备 timestamp 才能作为输入流真实起点
- 蓝牙麦克风 UI warning 不能替代资源释放验证；显式蓝牙设备 stop 后必须验证 stream drop 与音质恢复

---

## 已解决

### BUG-009: 选择系统默认麦克风会导致录制失败

**当前状态**：已修复（第 25 节整改）。

**根因**：`FfmpegRecordingWriter` 音频 timeline gap 分支实现错误。当 `target_sample > audio_timeline_cursor` 时，writer 只补齐 gap silence 并推进 cursor 到 chunk 起点，没有追加当前 audio chunk 的真实 samples，也没有把 cursor 推进到 chunk 末尾。真实设备录制的首个音频 chunk 通常带有非零 timestamp（因为 CPAL callback 有延迟），因此大量非静音 PCM 被替换为静音 AAC frame。

**修复内容**：

1. **writer gap 分支修复**：gap 分支补齐静音后必须继续 append 当前 chunk 的真实 PCM 样本，cursor 推进到 chunk 结束位置。提取 `append_audio_chunk_to_timeline()` helper 统一处理 gap/overlap/contiguous 三种情况。
2. **WriterDiagnostics 语义修正**：新增 `audio_real_frames_appended`、`audio_silence_frames_padded`、`audio_real_rms_max_before_encode`、`silent_aac_frames_encoded`、`generated_silent_track` 字段，区分真实 PCM append 与 silence padding。
3. **AudioSynchronizer per-source metadata**：`AudioWindow` 改为 `SourceWindowBuffer` 结构，system 和 mic 各自保留 `sample_rate/channels`，避免 48kHz/2ch system 与 48kHz/1ch mic 被套用同一份 metadata。
4. **silent track diagnostics**：`generated_silent_track` 从 writer diagnostics 直接获取，不再通过 `mixed_audio_chunk_count == 0` 推断。

**预防规则**：

1. writer 处理 audio gap 时，padding silence 后必须继续 append 当前真实 chunk；gap padding 不能替代 chunk append。
2. 音频 timeline 单元测试不能只检查 duration，还必须检查 decoded RMS/peak。
3. writer diagnostics 必须区分 real PCM append 与 silence padding。
4. `aac_frames_encoded > 0` 不能作为"artifact 有声"的证据，只能说明 AAC encoder 输出了 frame。
5. synchronizer window 必须保留 per-source metadata，不能把 system/mic 两路 PCM 套用同一份 sample_rate/channels。

---

### BUG-004: 导出视频无法播放（视频 PTS 被压缩到 0.03s）

**现象**：

- 导出视频只有封面第一帧有画面，从第二帧开始黑屏
- 原 20 秒视频导出后变成 2 分多钟
- `ffprobe` 显示 export video stream duration 仅 `0.032552s`，而 audio/container 约 `19s`

**根因**（两层叠加）：

1. **exporter packet rescale 空操作**：`trim_exporter.rs` 中 `enc_pkt.rescale_ts(video_enc_tb, video_enc_tb)` 等于没有转换。`write_header()` 后 MP4 muxer 可能把 output stream time base 改为 `1/15360`，但 PTS `0,1,2...` 仍被当作 `1/30` 单位写入，实际被解释为 `1/15360` 单位 → 501 帧仅 `501/15360 ≈ 0.03s`。
2. **source writer PTS 模型与真实时间脱节**：writer 使用固定帧序号 `0,1,2...` 作为 PTS，但 `duration_secs` 来自最后一帧的真实 timestamp。当帧率不稳定或有丢帧时，source video duration 远短于 audio/container duration。

**修复**：

1. `trim_exporter.rs`：`write_header()` 后读取 muxer 真实 output stream time base，所有 encoded packet 使用 `rescale_ts(enc_tb, out_tb)` 正确转换。
2. `ffmpeg_writer.rs`：视频 PTS 改为基于真实 frame timestamp 转换到 encoder time base，保证 monotonicity。
3. `ffmpeg_common.rs`：`MediaArtifactInspection` 增加 `video_duration_nanos` / `audio_duration_nanos` 字段，validation 检查 video/audio drift。

**预防规则**：

- FFmpeg muxer 写包必须使用 `write_header()` 后的真实 output stream time base，不能假设 encoder time base 等于 muxer time base
- artifact validation 必须检查 per-stream duration（video 和 audio），不能只看 container duration
- writer 不得只用 frame count 伪造真实录制时间轴，必须处理 frame sparsity/drop
- playable export manual gate 必须包含 ffprobe stream-level duration/fps 检查

---

### BUG-006: 系统音频稀疏时间轴导致 source writer duration drift

**现象**：

- 只开启系统音频，并且系统存在音频播放时（比如打开音乐播放器播放音乐），点击开始录制
- `录制中界面` 报错：
  - `完成录制文件失败：录制写入器完成失败：写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0x713711c00] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0x713711c00] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0x713712300] Qavg: 993.605
[libx264 @ 0x713711c00] frame I:3     Avg QP: 9.00  size:392678
[libx264 @ 0x713711c00] frame P:676   Avg QP: 1.82  size: 23728
[libx264 @ 0x713711c00] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0x713711c00] mb P  I16..4:  2.3%  0.0%  0.0%  P16..4: 17.8%  0.0%  0.0%  0.0%  0.0%    skip:79.8%
[libx264 @ 0x713711c00] final ratefactor: 6.86
[libx264 @ 0x713711c00] coded y,uvDC,uvAC intra: 34.7% 18.1% 17.0% inter: 7.8% 2.6% 2.4%
[libx264 @ 0x713711c00] i16 v,h,dc,p: 59% 38%  2%  2%
[libx264 @ 0x713711c00] i8c dc,h,v,p: 77% 14%  8%  1%
[libx264 @ 0x713711c00] kb/s:6085.99
录制写入器完成失败: 写入录制文件失败：录制视频/音频时长偏差过大：视频 22600ms，音频 12821ms，偏差 9779ms
```

**根因**：

系统音频捕获天然可能是稀疏的：用户开始录制后几秒才播放声音，中途暂停播放，或结束前没有声音。同步器和 mixer 已经保留 timestamp，但 writer 把 timestamp 丢了，最终 AAC stream duration 只等于"有声样本总长度"，不是"录制时间轴长度"。

**预防规则**：

- 音频输入 chunk 必须携带真实 sample_rate/channels/timestamp；统一输出格式只能在 mixer/writer timeline 层完成
- writer 必须尊重 mixed audio timestamp；对前导 gap、中间 gap、尾部 gap 写入 silence，不能把稀疏音频压缩成连续短音轨

---

### BUG-007: 导出的视频没有美化（默认美化开启时无光标）

**现象**：

- 默认美化开启（`cursor_magnification=true, cursor_smoothing=true`）时，录制会隐藏系统光标
- FFmpeg exporter 未应用 `effect_timeline_path` 绘制光标效果
- 导出视频无光标、无美化，违背 MVP "录屏 + AI 自动美化 + 一键导出" 核心目标

**根因**：

1. `build_cursor_effect_timeline()` 失败时静默降级为 `effect_timeline_path: None`
2. `FfmpegTrimExporter::export()` 从未读取 `effect_timeline_path` JSON
3. 没有 cursor overlay compositor 在导出帧上绘制光标

**修复**：

1. 新增 `cursor_overlay.rs` 模块：`CursorOverlayRenderer` 加载 EffectTimeline，映射坐标到输出帧空间（支持 FitWithBars 和 CenterCrop），在 YUV420P 帧上绘制光标（白点 + 暗边框 + 点击放大环）
2. `trim_exporter.rs`：导出时加载 effect timeline 并在每帧编码前叠加 cursor overlay
3. `lib.rs`：当美化开启（`cursor_magnification || cursor_smoothing`）时，cursor timeline 构建失败必须阻断导出，不再静默降级
4. `ffmpeg_writer.rs`：重构为 worker-backed 架构，编码在独立线程执行，bounded channel 提供背压

**预防规则**：

- 当 raw system cursor hidden 时，exporter 必须应用 cursor effect timeline，或导出必须失败，不得静默产出无光标视频
- cursor effect timeline 不能只作为 request 字段存在，exporter 必须实际读取并应用
- 默认美化开关、capture `show_system_cursor`、export compositor 三者必须作为端到端 contract 测试
- UI 不能把"可播放导出成功"文案等同于"美化导出成功"

---

### BUG-008: cursor overlay scale/radius 数值溢出导致 export panic

**现象**：

- 如果当前系统没有音频输出时，录制能成功，但是`美化界面`导出会报错：
  - `导出失败，请重试或检查录制素材。`
- 终端日志如下：

```
     Running `target/debug/luzhi`
[libx264 @ 0xab32e4700] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e4700] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit
[aac @ 0xab32e4e00] Qavg: 65536.000
[libx264 @ 0xab32e4700] frame I:2     Avg QP:17.00  size:288984
[libx264 @ 0xab32e4700] frame P:384   Avg QP: 5.91  size: 33077
[libx264 @ 0xab32e4700] mb I  I16..4: 100.0%  0.0%  0.0%
[libx264 @ 0xab32e4700] mb P  I16..4:  4.2%  0.0%  0.0%  P16..4: 17.9%  0.0%  0.0%  0.0%  0.0%    skip:77.8%
[libx264 @ 0xab32e4700] final ratefactor: 12.31
[libx264 @ 0xab32e4700] coded y,uvDC,uvAC intra: 40.9% 17.2% 14.0% inter: 9.2% 2.6% 2.2%
[libx264 @ 0xab32e4700] i16 v,h,dc,p: 52% 43%  3%  2%
[libx264 @ 0xab32e4700] i8c dc,h,v,p: 78% 14%  7%  1%
[libx264 @ 0xab32e4700] kb/s:7811.50
[libx264 @ 0xab32e6680] using cpu capabilities: ARMv8 NEON DotProd
[libx264 @ 0xab32e6680] profile Constrained Baseline, level 4.0, 4:2:0, 8-bit

thread 'tokio-rt-worker' (7976776) panicked at src/media/cursor_overlay.rs:263:30:
attempt to multiply with overflow
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
[aac @ 0xab32e6300] Qavg: nan
[aac @ 0xab32e6300] 1 frames left in the queue on closing
[libx264 @ 0xab32e6680] final ratefactor: 20.64
```

**根因**：

cursor effect timeline 来自录制时的外部输入和动画计算，不应被视为可信数值。`scale` 可能异常大、非有限值、或由错误时间轴插值得到极端值。当前 rasterizer 用 `i32` 做平方，debug 构建会 panic，release 构建则可能溢出后产生错误绘制。

**预防规则**：

- cursor/effect timeline 来自外部输入，所有 `x/y/scale/timestamp` 参与 rasterization 前必须 finite check 和范围 clamp
- overlay 距离计算不得依赖 debug/release 不同行为；平方和半径计算必须使用足够宽的整数类型或 saturating arithmetic

---

### BUG-001: 录制面板不是浮动小组件，有800x600背景方框

**现象**：主录制控制面板应为浮动小组件，但显示时有一个800x600的不透明背景方框。修复 body CSS 后背景从深色变为白色，但方框仍然存在。

**根因**：Tauri 透明窗口需要**三层独立**都透明，缺一不可：

| 层        | 机制                        | 问题                                                 |
| --------- | --------------------------- | ---------------------------------------------------- |
| NSWindow  | `"transparent": true`       | ✅ 已配置                                            |
| WKWebView | `macos-private-api` feature | ❌ 缺失：macOS WKWebView 独立于 CSS 绘制默认白色背景 |
| CSS       | html/body 无背景色          | ❌ body `bg-background` (#040506) + html 未设透明    |

第一轮修复只解决了 CSS 层 body 的问题，但 WKWebView 的白色背景随即暴露。之前 body `#040506` 覆盖了 WKWebView 的白色，使人误以为只有 CSS 问题。

**修复**：

1. `Cargo.toml`：tauri 添加 `macos-private-api` feature → 通过 KVC 禁用 WKWebView 的 `drawsBackground`
2. `styles.css`：body 移除 `bg-background` + html 添加 `background-color: transparent`
3. 需要全屏背景的视图（processing、error、preview）已在各自根 div 显式设置 `bg-background`

**预防规则**：

- Tauri macOS 透明窗口 = NSWindow + WKWebView + CSS，三层缺一不可
- 排查透明窗口问题必须逐层验证，不能只看 CSS
- `macos-private-api` feature 是 macOS 平台上实现真正透明 WKWebView 的必要条件

---

### BUG-002: 窗口无法拖动

**现象**：所有状态下窗口都无法通过鼠标拖动移动。

**根因**（多层叠加，逐层剥离后才暴露下一层）：

| 层             | 问题                                                                | 详情                                                                                                                                                                                        |
| -------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| ACL 权限       | `capabilities/default.json` 缺少 `core:window:allow-start-dragging` | Tauri 2 的 drag.js 在 mousedown 时通过 IPC 调用 Rust 命令 `plugin:window                                                                                                                    | start_dragging`，没有此权限 IPC 被静默拒绝。**这是拖拽完全不工作的致命原因** |
| 拖拽区域标记   | bare `data-tauri-drag-region` 渲染为 `"true"`                       | React 将裸属性渲染为 `"true"`，Tauri drag.js 对 `"true"` 的处理是仅元素自身直接点击触发（`el === composedPath[0]`），子元素区域不触发。面板覆盖大部分区域，实际可拖拽区只剩 `p-8` 32px 窄边 |
| macOS API 限制 | `performWindowDragWithEvent:` 需窗口为焦点窗口                      | 即使权限和属性都正确，底层 macOS API 在窗口未获焦点时静默失败（tauri#11605）。声明式机制对浮动面板窗口不可靠                                                                                |
| 排除逻辑过严   | `{false}` 排除 + `attr === 'false'` 检查双重拦截                    | 面板包裹在 `{false}` div 中，`closest('[data-tauri-drag-region]')` 最先匹配到 `{false}` 父元素，handler 拒绝拖拽。面板内容区（非按钮空白处）完全无法拖拽                                    |

**修复**（迭代 4 轮，最终方案：程序化拖拽替代声明式）：

1. **`src-tauri/capabilities/default.json`**：添加 `core:window:allow-start-dragging` 权限 — 解禁 IPC 调用
2. **`src-tauri/tauri.conf.json`**：添加 `"acceptFirstMouse": true` — macOS 首次点击可同时聚焦+拖拽
3. **`src/App.tsx`**：实现程序化拖拽 handler，直接调用 `getCurrentWindow().startDragging()`，绕过不可靠的声明式 drag.js
   - 添加 `data-tauri-drag-region="deep"` 到所有外层容器（idle、recording、preview、processing、error）
   - 拖拽 handler 逻辑：
     - **允许拖拽**：点击在 `[data-tauri-drag-region]` 区域内且不是交互元素
     - **阻止拖拽**：点击目标为 `button, input, select, textarea, a` 或 `role` 为 `button/link/menuitem/tab/checkbox/radio/slider/switch` 或 `contenteditable` / 非 `-1` 的 `tabindex`
   - 使用动态 `import('@tauri-apps/api/window')` 避免测试环境顶层导入报错
   - **不检查 `{false}` 值** — 交互元素检查已足够，`{false}` 标记仅保留供 Tauri 内置 drag.js 作 fallback

**预防规则**：

- **Tauri 2 窗口拖拽必须授予 ACL 权限**：`core:window:allow-start-dragging` 不在 `core:default` 范围内，必须显式添加
- **声明式 `data-tauri-drag-region` 不可靠**：受 macOS 焦点窗口限制，对浮动面板类 app 应优先使用程序化 `startDragging()` API
- **不要用 `{false}` 做区域级拖拽排除**：用交互元素选择器（`closest('button, input, ...')`）精确排除，而非用 `{false}` 阻止整个面板区域。`{false}` 应该是交互元素自身的标记，不是容器的标记
- **每个状态视图都需要拖拽区域**：包括 processing、error 等过渡状态
- **交互元素白名单要覆盖完整**：button, input, select, textarea, a, [role="button"], [role="slider"], [role="switch"], [contenteditable], [tabindex] 等

---

### BUG-003: 点击"开始录制"无法切换到录制状态

**现象**：点击"开始录制"按钮后，界面没有切换到录制状态栏（迷你播放器）。

**根因**：Button 被包裹在 `motion.div` 的 `whileTap={{ scale: 0.99 }}` 中。framer-motion 的 whileTap 会捕获指针事件来实现缩放动画，阻止了点击事件到达内部的 Button 元素。

**修复**：移除 Button 外层的 `motion.div` 包裹，改用 CSS `active:scale-[0.99]` 实现按压反馈效果。

**预防规则**：

- framer-motion 的 `whileTap` 会拦截指针事件，不要将其作为可交互元素（Button、Link 等）的直接父容器。
- 如需在可交互元素上添加按压动画，优先使用 CSS `active:` 伪类，或将 `whileTap` 直接放在元素本身上（如 `motion.button`）。

---

## 延期解决

### 延期-001: 透明区域鼠标点击不穿透

**现象**：窗口视觉透明区域的鼠标点击不会穿透到被覆盖的应用（桌面、其他窗口），点击透明区域不会激活后方应用。

**原因**：Tauri 2 的 `setIgnoreCursorEvents(true)` 是全窗口级开关，开启后整个窗口（包括面板按钮）都无法交互。无像素级点击穿透支持。

**计划方案**（后期实现）：

- 方案 A：自定义 NSWindow `hitTest:` 重写，透明像素处返回 nil（需原生 macOS 代码）
- 方案 B：主窗口忽略鼠标事件 + 独立子窗口承载控制面板
- 方案 C：使用窗口 shape mask 裁剪到面板区域

---
