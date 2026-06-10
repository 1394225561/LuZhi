# BUG-0027 高频电流滋滋声回归自测清单

## Root Cause

- [x] 复盘人工反馈：上一轮优化后电流“滋滋声”更频繁、更严重
- [x] 确认上一轮 BUG-0026 测试只覆盖 `240Hz/480Hz` 低频窄带 buzz
- [x] 确认现有链路缺少“语音开门 + 高频宽带 hiss”回归

## TDD

- [x] RED：`denoise_chain_suppresses_high_frequency_hiss_when_voice_opens_gate` 修改前失败
- [x] GREEN：`denoise_chain_suppresses_high_frequency_hiss_when_voice_opens_gate` 修改后通过
- [x] `denoise_chain_preserves_voice_band_signal` 覆盖 4kHz 清晰度保真
- [x] `denoise_chain_preserves_quiet_voice_near_noise_gate` 通过
- [x] `denoise_chain_does_not_hold_back_voice_after_quiet_noise` 通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 通过

## Final Regression

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- [x] `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- [x] `git diff --check` 通过
- [x] 确认未修改 `Cargo.toml`、`Cargo.lock`、`src-tauri/src/core/media_channel.rs`

## Manual Verification

- [ ] 有线耳机麦克风开启降噪，静音 5 秒后开始说话，确认语音进入时没有更频繁的电流滋滋声
- [ ] 持续正常说话 10 秒，确认高频滋滋声弱于上一轮
- [ ] 听 4kHz 附近清晰度体感，确认人声没有明显发闷
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受麦克风 denoise 影响
