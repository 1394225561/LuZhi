# BUG-0028 说话时电流滋滋声回归自测清单

## Root Cause

- [x] 复盘人工反馈：无麦克风输入时没有电流滋滋声，一旦开始说话就伴随滋滋声
- [x] 确认该现象不符合静音段底噪压不住，更符合语音相关噪声或处理链路非线性失真
- [x] 定位 `audio_mixer.rs` 的逐样本 soft-knee limiter 会对合法满幅语音做 waveshaping

## TDD

- [x] RED：`limiter_does_not_add_harmonic_fizz_to_loud_voice` 修改前失败，`fundamental=0.99419343, fizz=0.013985186`
- [x] GREEN：`limiter_does_not_add_harmonic_fizz_to_loud_voice` 修改后通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过

## Final Regression

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- [x] `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- [x] `git diff --check` 通过
- [x] 确认未修改 `Cargo.toml`、`Cargo.lock`、`src-tauri/src/core/media_channel.rs`

## Manual Verification

- [ ] 有线耳机麦克风开启降噪，静音 5 秒后开始说话，确认说话时不再伴随明显电流滋滋声
- [ ] 正常音量连续说话 10 秒，确认高频滋滋声弱于上一轮
- [ ] 稍大音量说话但不吼叫，确认没有新增破音、齿音刺耳或电流感
- [ ] 同时录制系统音频和麦克风，确认系统音频仍不受麦克风 denoise 影响
