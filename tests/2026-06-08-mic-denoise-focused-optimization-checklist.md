# 麦克风降噪聚焦优化自测清单

## Phase 0: Baseline

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 基线通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 基线通过
- [x] `git status --short` 已确认没有误碰无关文件

## Phase 1: 100Hz 麦克风高通

- [x] `denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline` 修改前失败
- [x] `denoise_chain_attenuates_30hz_hum_beyond_80hz_baseline` 修改后通过
- [x] `denoise_chain_preserves_200hz_low_voice_after_stronger_highpass` 通过
- [x] `denoise_chain_preserves_speech_like_low_voice_after_stronger_highpass` 通过
- [x] `denoise_chain_suppresses_residual_buzz_when_voice_opens_gate` 修改前失败
- [x] `denoise_chain_suppresses_residual_buzz_when_voice_opens_gate` 修改后通过
- [x] `denoise_chain_preserves_voice_components_near_new_notches` 通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise -- --nocapture` 通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过

## Phase 2: 透明软限幅器

- [x] `soft_limiter_preserves_samples_below_knee` 修改前因 `soft_limit_samples` 不存在而编译失败
- [x] `soft_limiter_preserves_samples_below_knee` 修改后通过
- [x] `soft_limiter_compresses_peaks_without_hard_plateau` 通过
- [x] `soft_limiter_limits_extreme_values_near_full_scale` 通过
- [x] `mixer_uses_soft_limiter_for_single_source_peaks` 通过
- [x] `mixer_soft_limits_full_scale_mix_without_hard_clipping` 通过
- [x] `simple_mixer_rejects_non_finite_samples` 修改前失败
- [x] `simple_mixer_rejects_non_finite_samples` 修改后通过
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer -- --nocapture` 通过

## Final Regression

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过
- [x] `rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过
- [x] `git diff --check` 通过

## Manual Verification

- [ ] 有线耳机麦克风开启降噪，静音环境录制 10 秒，低频嗡声比修复前更弱
- [ ] 有线耳机麦克风开启降噪，正常说话 10 秒，开头不吞字，尾音不过早切断
- [ ] 有线耳机麦克风开启降噪，从静音到开始说话，确认语音产生瞬间不再出现明显“嗡”一下或低声“滋滋声”
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受麦克风 denoise 影响
- [ ] 播放较大音量系统音频并说话，确认录制结果没有硬削波的撕裂感
