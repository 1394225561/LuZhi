# 2026-06-07 BUG-0020 窗口录制退出音频 Contract 自测清单

## 自动化验证

- [x] RED: `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract_allows_quiet_dual_source_when_source_verified` 修复前失败，失败点为 aggregate RMS/peak 低于 Level 1 阈值
- [x] GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract_allows_quiet_dual_source_when_source_verified`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract_source_verified_still_rejects_zero_audio`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg source_artifact_audio_contract`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg requested_audio_contract`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [x] `npm test -- --run`
- [x] `npm run build`
- [x] `cargo build --manifest-path src-tauri/Cargo.toml`
- [x] `rustfmt --check src-tauri/src/media/ffmpeg_common.rs src-tauri/src/platform/macos_service.rs`
- [x] `git diff --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg` 未全绿：本轮音频测试均通过，但 `media::cursor_overlay::tests::rendered_arrow_rgba_blends_yuv_planes` 单独运行仍失败，失败断言为 `arrow cursor should be visible on Y plane near cursor position`，与本轮音频 contract 改动无直接调用关系

## 重点人工验证

- [ ] 窗口录制同时开启系统音频和麦克风，系统无播放声音，麦克风低音量输入，通过“退出”结束应用；录制不应进入 failed UI
- [ ] 同一场景通过 `Command + Q` 结束应用；录制结果应保留，日志可出现低音量 warning，但不能出现 hard finalize failure
- [ ] 同一场景提高麦克风音量；录制和导出均可播放音频
- [ ] 麦克风权限关闭或设备无 chunk 输入时，source-aware contract 仍应报告失败
- [ ] 系统音频单独开启且系统静音时，BUG-0013 行为保持：录制允许静音音轨

## Review 防线

- [ ] `allow_quiet_when_source_verified` 默认值保持 `false`
- [ ] source-aware contract 必须先于依赖其结果的 quiet bypass 执行
- [ ] quiet bypass 只允许 decoded RMS/peak 非零的音频通过；全零音频仍失败
- [ ] export artifact contract 未因本修复默认放宽
- [ ] 退出/Cmd+Q 问题定位必须以 diagnostics 为准，不能把 artifact validation false positive 误判成 writer cleanup 失败
