# 2026-06-08 BUG-0024 有线耳机麦克风动态底噪抑制自测清单

## 范围

- 仅继续优化开启“降噪（去除电流声）”时的麦克风处理链路。
- 不新增 UI 配置，不修改 `Cargo.toml` 依赖版本。
- 系统音频不应被降噪链路处理。
- 不触碰 CPAL 采集线程、ScreenCaptureKit/FFI 或前端录制流程。

## 自动验证

- [x] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_reduces_low_level_broadband_noise_floor` 旧行为下失败，失败信息为 `input=0.005095413, denoised=0.005094919`。
- [x] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_quiet_voice_near_noise_gate` 旧门限下失败，失败信息为 `input=0.016970206, denoised=0.011312481`。
- [x] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_does_not_hold_back_voice_after_quiet_noise` 旧门限下失败，失败信息为 `input=0.1272791, denoised=0.09398388`。
- [x] RED：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_quiet_voice_tail_after_normal_speech` 旧门限下失败，失败信息为 `input=0.016970742, denoised=0.011902361`。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_reduces_low_level_broadband_noise_floor` 通过。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_quiet_voice_near_noise_gate` 通过。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_does_not_hold_back_voice_after_quiet_noise` 通过。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_quiet_voice_tail_after_normal_speech` 通过。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib denoise_chain_preserves_voice_band_signal` 通过。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib mixer_dynamic_suppressor_does_not_filter_low_level_system_only_audio` 通过。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise` 通过（14 tests）。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer` 通过（22 tests）。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过（348 tests）。
- [x] 格式：`rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过。
- [x] 空白：`git diff --check` 通过。

## 人工验证建议

- [ ] 有线耳机麦克风开启降噪，静音环境录制 10 秒，回放确认轻微电流底噪进一步降低。
- [ ] 同一设备关闭降噪录制 10 秒，作为对照确认底噪差异明显。
- [ ] 开启降噪后正常说话 10 秒，确认开头不被吞字，尾音不过早被切掉。
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受影响。
