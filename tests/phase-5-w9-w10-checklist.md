# Phase 5 / W9-W10 自测清单：空白段检测与自动裁剪

## 目标

验证音频 RMS、低分辨率帧差分和裁剪时间线可保守识别长空白段，并通过 FFmpeg 封装完成裁剪导出。

## Verification Summary (2026-05-29, Phase 5)

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 149 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS, 21 pre-existing SCK FFI warnings
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: PASS, 44 tests

## 音频 RMS 验证

- [x] 500ms 到 1000ms 滑动窗口可配置。
- [x] 静音段能被识别。
- [x] 短暂停顿不会默认进入裁剪候选。
- [x] 背景噪声存在时阈值表现可解释。

## 帧差分验证

- [x] 帧差分使用低分辨率灰度图。
- [x] 完全静止画面变化率接近 0。
- [x] 加载动画不会被误判为完全静止。
- [x] 鼠标移动能贡献画面变化信号。
- [x] 抽样频率不会影响主录制帧流。

## 裁剪策略验证

- [x] 只有音频静音和画面低变化同时满足才进入候选。
- [x] 小于 2 秒静默不裁剪。
- [x] 超过 5 到 8 秒低变化片段进入候选。
- [x] 候选段前后保留 300 到 500ms 缓冲。
- [x] 相邻候选段可正确合并。
- [x] 输出 `CutTimeline` 可解释。

## 导出验证

- [ ] FFmpeg 封装能消费 `CutTimeline`。（FFmpeg Gate：需生产编码器接入）
- [ ] 裁剪后视频可播放。（FFmpeg Gate）
- [ ] 音视频同步未明显漂移。（FFmpeg Gate）
- [x] 原始素材保留。
- [x] 取消裁剪后可按原素材重新导出。

## 测试要求

- [x] RMS 检测有单元测试。
- [x] 帧差分有边界测试。
- [x] 裁剪候选合并有单元测试。
- [x] 裁剪时间线空输入、重叠输入、连续输入均有测试。

## 人工审查

- [ ] 裁剪默认策略足够保守。
- [ ] 没有直接删除原始素材。
- [ ] FFmpeg 调用未拼接用户输入字符串。

## Remaining Manual Gates

- [ ] Real playable FFmpeg trimmed export once production encoder/muxer is connected.
- [ ] 10 minute 1080p trim metadata and cut timeline memory-pressure check.
- [ ] Manual review that original recording artifacts are preserved.
- [ ] BUG.md prevention scan for drag/whileTap/click-through regressions.
