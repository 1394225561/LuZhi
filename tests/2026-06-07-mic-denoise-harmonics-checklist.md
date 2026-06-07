# 2026-06-07 麦克风降噪谐波优化自测清单

## 范围

- 仅优化开启“降噪（去除电流声）”时的麦克风处理链路。
- 不新增 UI 配置，不修改 `Cargo.toml` 依赖版本。
- 系统音频不应被降噪链路处理。

## 自动验证

- [x] RED：新增合成工频谐波测试在实现前失败，失败信息为 `highpass=0.04565383, denoised=0.04565383`。
- [x] RED：新增系统单源保护测试在旧行为下失败，失败信息为 `System-only audio should not be denoised, got avg 0.000012978191`。
- [x] RED：Code Review 后新增麦克风通道数 mono→stereo 测试在旧行为下失败，失败信息为 `index out of bounds: the len is 1 but the index is 1`。
- [x] GREEN：`cargo test --manifest-path src-tauri/Cargo.toml --lib audio_denoise` 通过（10 tests，包含 composite 工频谐波逐频点衰减与多频点人声保真）。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib audio_mixer` 通过（21 tests）。
- [x] 回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib` 通过（341 tests）。
- [x] 格式：`rustfmt --check --edition 2021 src-tauri/src/media/audio_denoise.rs src-tauri/src/media/audio_mixer.rs` 通过。

## 已知非本轮问题

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 会在 `src-tauri/src/app/error.rs`、`src-tauri/src/app/recording_library.rs` 等既有未触碰文件上报告格式差异；本轮未顺手格式化无关文件。

## 人工验证建议

- [ ] 有线耳机麦克风开启降噪录制 10 秒静音环境，回放确认轻微电流声进一步减弱。
- [ ] 同一设备关闭降噪录制，作为对照确认降噪开关仍有效。
- [ ] 开启降噪录制正常说话，确认人声明显可懂、无明显闷声或抽吸感。
- [ ] 同时录制系统音频和麦克风，确认系统音频音质不受影响。
