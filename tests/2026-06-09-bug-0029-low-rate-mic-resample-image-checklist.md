# 2026-06-09 BUG-0029 低采样率麦克风升采样镜像音自测清单

## 自动化验证

- [x] RED: `cargo test --manifest-path src-tauri/Cargo.toml --lib denoised_low_rate_mic_resample_does_not_leave_image_tone -- --nocapture`
  - 修复前失败：`fundamental=0.23332335, image=0.06200983`
- [x] GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib denoised_low_rate_mic_resample_does_not_leave_image_tone -- --nocapture`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_synchronizer -- --nocapture`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg denoised_low_rate_mic_resample_does_not_leave_image_tone -- --nocapture`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --features ffmpeg ffmpeg_writer_records_non_silent_mixed_24khz_mono_mic -- --nocapture`
- [x] `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs src-tauri/src/media/audio_synchronizer.rs`
- [x] `git diff --check`

## 人工复核建议

- [ ] 同时开启系统音频和麦克风，开启麦克风降噪，正常说话 10 秒，确认说话时不再出现规律性高频滋滋或咔哒声。
- [ ] 观察终端中的“麦克风配置协商”日志；若设备实际采样率低于 48kHz，重点复核本次抗镜像修复效果。
- [ ] 麦克风静音 10 秒，确认静音段仍保持干净。
- [ ] 系统音频单独播放并录制，确认系统音频路径未被麦克风降噪链路影响。
