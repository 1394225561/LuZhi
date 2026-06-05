# LuZhi 项目交接文档

> 最后更新：2026-06-05 | BUG-0011 NSCursor 方案人工验证通过；FFI review findings 已修复并通过自动化验证。
>
> 更新本文件时，**必须**保持”项目概述 → 完整开发计划 → 工作任务记录（按**时间倒序**，并且只保留最近的 7 条记录） → 冬眠记录（按**时间倒序**，并且只保留最近的 7 条记录）”的结构顺序。

## 项目概述

LuZhi 是中国版 Screen Studio，核心聚焦“录屏 + AI 自动美化 + 一键导出”。MVP 坚持极致单点，不做字幕、摘要、知识库、模板系统、团队协作和平台发布 API。

当前技术栈约束：

- 桌面框架：Tauri 2.0
- 前端：React + TypeScript + Tailwind + shadcn/ui
- Rust 底层：ScreenCaptureKit、DXGI、WASAPI、cpal、FFmpeg binding
- 数据流红线：音视频帧流不得经过前端 JS 层

---

## 完整开发计划

已落地主架构与计划文档：

- `docs/architecture/project-architecture-and-overall-planning.md`

关键决策：

1. MVP 采用“macOS 先打穿的分层流水线架构”。
2. 4K 作为架构预留能力，不作为首版唯一硬验收；MVP 主验收为 1080p 稳定录制、录后美化和导出。
3. Windows 通过同一组 Rust Trait 在 W3-W8 逐步追平，不单独开业务管线。
4. 光标美化和空白裁剪在 MVP 阶段以录后处理为主，避免阻塞捕获线程。
5. 授权系统只做本地试用状态和激活状态接口，不展开服务端激活协议。

W1-W12 Phase：

- Phase 1 / W1-W2：脚手架与 macOS 录制闭环
- Phase 2 / W3-W4：Windows 捕获预研与双音频链路
- Phase 3 / W5-W6：录制 UI 与前后端联调
- Phase 4 / W7-W8：光标平滑与点击放大
- Phase 5 / W9-W10：空白段检测与自动裁剪
- Phase 6 / W11-W12：导出预设与本地授权

### 系统架构设计

输入文件：

- `reference/tasks/architecture-task.md`
- `docs/PRD/LuZhi_PRD_final_version.md`
- `.codex/rules/0-global.md`
- `.codex/rules/1-coding-style.md`
- `.codex/rules/2-testing.md`
- `.codex/rules/3-git-commit.md`
- `.codex/rules/4-security.md`
- `.codex/rules/5-docs.md`
- `BUG.md`

已完成：

- 明确进程与线程模型。
- 明确 React / Rust App Logic / Native Modules 三层边界。
- 明确跨平台 Trait 抽象：`ScreenCapture`、`AudioCapture`、`CursorProcessor`、`SilenceDetector`、`MediaEncoder`。
- 明确核心数据流、Ring Buffer、时间戳对齐、音频混音和 FFmpeg 封装原则。
- 明确光标平滑、点击放大、空白段检测和 4K 降级策略。
- 明确 W1-W12 MVP 执行计划、风险点和前后端联调节奏。
- 为每个 Phase 生成自测清单到 `tests/` 目录。

后续入口：

1. 人工审阅 `docs/architecture/project-architecture-and-overall-planning.md`。
2. 确认无调整后，进入详细实施计划编写。
3. 启动 W1-W2 时，先按 `tests/phase-1-w1-w2-checklist.md` 建立验收闭环。

### 已知问题阻塞点

1. [x] **CoreMedia 链接错误阻塞 `npm run tauri dev`**（已修复：2026-05-25）
   - **症状**：`cargo build --bin luzhi` 报 `_CMFormatDescriptionGetStreamBasicDescription` 符号未定义
   - **根因**：使用了错误的符号名 `CMFormatDescriptionGetStreamBasicDescription`，正确符号为 `CMAudioFormatDescriptionGetStreamBasicDescription`
   - **修复**：Task 1 已替换为正确的 CoreMedia 音频格式符号，`cargo build` 通过

---

## 工作任务记录

### 2026-06-05：BUG-0011 第 6 轮 FFI Code Review Findings 修复

输入文件：

- 用户反馈：第 6 轮人工验证效果没问题
- Code review findings：重点修复 NSCursor FFI selector guard、AppKit 线程边界、autorelease 生命周期、C string 类型安全和 clippy 近邻告警
- `BUG.md` BUG-0011 预防规则 25-29

已完成：

1. 新增 `CursorMainThreadDispatcher`，生产录制链路通过 Tauri `AppHandle::run_on_main_thread` 执行 AppKit cursor 读取
2. `MacCursorKindProvider` 构造时必须接收 main-thread dispatcher，避免生产路径在 cursor metadata 后台线程直接调用 AppKit
3. ObjC 消息发送前增加 class/instance method guard，缺失 selector 时 fail closed
4. selector 参数改为 `&CStr` / `c"..."`，移除裸 `&[u8]` C string 边界
5. autorelease pool 改为 `objc_autoreleasePoolPush/Pop`
6. legacy AX 的 `NSWorkspace` ObjC 调用同步使用 guarded helper
7. 新增 main-thread reader 调度成功/失败测试
8. 清理本轮 review 指出的 `cursor_source.rs` / `cursor_metadata_runtime.rs` unused import 告警
9. 更新 BUG.md 和本轮自测清单

当前验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind` **12 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` **294 tests** 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib` 通过；剩余 **32 个既有 warning**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `npm test -- --run` **55 tests** 通过
- `git diff --check` 通过

改动文件：

- **修改**: `src-tauri/src/platform/macos/cursor_kind.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `BUG.md`, `HANDOFF.md`
- **新增**: `tests/2026-06-05-bug-0011-ffi-review-findings-checklist.md`

人工复核建议：

1. 普通桌面/空白区域：导出美化光标为 Arrow
2. WebView/浏览器按钮或链接：导出美化光标为 Hand
3. 文本输入框：导出美化光标为 IBeam
4. 四指上划进入 Mission Control：macOS 显示 Arrow，导出美化视频也显示 Arrow

### 2026-06-05：BUG-0011 第 6 轮整改（NSCursor 真实系统光标来源）

输入文件：

- 用户反馈：第 5 轮后导出视频仍无法正确切换光标形态
- 用户反馈：四指上划进入 Mission Control 时，macOS 显示 Arrow，但导出美化视频变成 Hand
- `HANDOFF.md` 冬眠记录：AX 方案无法穿透 WebView，下一步尝试 NSCursor API

已完成：

1. `MacCursorKindProvider` 生产主路径改为读取 `NSCursor.currentSystemCursor`
2. 新增系统光标映射：Arrow / PointingHand / IBeam
3. 系统光标匹配支持对象指针、`isEqual:`、hot spot + TIFF 图像数据
4. 未知或读取失败 fallback Arrow，避免 AX false positive 误画 Hand
5. AppKit 轮询增加 `NSAutoreleasePool`
6. 移除前端“需要辅助功能权限才能识别手形/文本光标”的误导提示
7. 新增回归测试覆盖 Mission Control false positive：系统 Arrow 必须保持 Arrow
8. 新增本轮计划与自测清单
9. 更新 BUG.md

当前验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind` **10 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` **292 tests** 通过
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `npm test -- --run` **55 tests** 通过
- Objective-C runtime selector check 通过：`currentSystemCursor` / `arrowCursor` / `pointingHandCursor` / `IBeamCursor` 均存在

改动文件：

- **修改**: `src-tauri/src/platform/macos/cursor_kind.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/app/permission_service.rs`, `src-tauri/src/app/events.rs`, `src/lib/tauri.ts`, `src/App.tsx`, `BUG.md`, `HANDOFF.md`
- **新增**: `docs/superpowers/plans/2026-06-05-bug-0011-nscursor-provider.md`, `tests/2026-06-05-bug-0011-nscursor-provider-checklist.md`

人工验证门禁：

1. 普通桌面/空白区域：导出美化光标为 Arrow
2. WebView/浏览器按钮或链接：导出美化光标为 Hand
3. 文本输入框：导出美化光标为 IBeam
4. 四指上划进入 Mission Control：macOS 显示 Arrow，导出美化视频也显示 Arrow
5. 未授权 Accessibility：仍能根据系统当前光标识别 Arrow/Hand/IBeam

### 2026-06-04：BUG-0011 方案B（前台应用 AX 查询）实现与测试

输入文件：

- 用户终端日志：cursor-kind-classify 始终返回 `role_chain=["AXMenuBar", "AXApplication"]`

已完成：

1. 添加 `AXUIElementCreateApplication` FFI 绑定
2. 添加 `get_frontmost_app_pid()` — 通过 `NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier` 获取前台应用 PID
3. `query_cursor_kind()` 改为先查询前台应用 AX 元素，失败后 fallback 到系统级查询
4. 添加 `used_frontmost_app` 字段到 `AxClassificationLog`，日志显示 `src=front` 或 `src=system`
5. 更新 BUG.md

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib` **288 tests** 通过
- `cargo fmt --check` 通过

**测试结论**：方案B 已实现但 WebView (WKWebView) 内容对 AX hit-test 不透明。
- `src=front` 查询成功（前台应用 PID 正确获取）
- 但 `role_chain` 始终返回 `["AXMenuBar", "AXApplication"]` 或 `["AXGroup", "AXScrollArea", "AXApplication"]`
- AX API 无法穿透 WebView 的内部 AX 层级，无法检测到链接、按钮、输入框等交互元素

改动文件：

- **修改**: `src-tauri/src/platform/macos/cursor_kind.rs`, `BUG.md`

下一步：尝试方案A（NSCursor API）

---

### 2026-06-04：BUG-0010/0011 第五轮整改（光标漂移 + AX 坐标修复）

输入文件：

- 用户终端日志：cursor-kind-classify 始终返回 `role_chain=["AXMenuBar", "AXApplication"]`
- 用户确认：漂移仅在开启平滑/贝塞尔时出现；光标在所有情况下都是箭头

已完成：

**BUG-0010（漂移）**：
1. 移动平均替换为 EMA（alpha=0.4）— 停止后 ~2-3 帧收敛
2. Catmull-Rom Bezier 替换为单调三次 Hermite 插值（Fritsch-Carlson）— 数学保证不过冲
3. 新增 3 个漂移回归测试
4. 清理未使用的 `distance_between` 函数

**BUG-0011（光标分类）**：
5. `MacCursorKindProvider` 新增 Y 轴翻转 — CGEvent top-down → AX bottom-up
6. 通过 `CGDisplayBounds(CGMainDisplayID())` 获取显示器高度
7. 更新 BUG.md 和 HANDOFF.md

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --lib` **288 tests** 通过
- `npm test -- --run` **55 tests** 通过
- `cargo fmt --check` 通过

改动文件：

- **修改**: `src-tauri/src/media/cursor_engine.rs`, `BUG.md`, `HANDOFF.md`

人工验证门禁（第五轮）：

1. BUG-0010 smooth：大浮动向左移动后静止，光标水平方向不向左偏移
2. BUG-0010 smooth：大浮动向右移动后静止，光标水平方向不向右偏移
3. BUG-0010 smooth：多次方向变化后静止，光标无累积漂移

---

### 2026-06-04：BUG-0010/0011 第四轮整改完成

输入文件：

- `docs/superpowers/reviews/2026-06-04-bug-0010-0011-fourth-round-code-review-root-cause-and-fix-plan.md`
- `docs/superpowers/plans/2026-06-04-bug-0010-0011-fourth-round-rectification.md`

已完成（12 个 Task）：

1. 合并 AX 全局计数器到 metadata — `cursor_kind_diagnostics_merged()` 确保 failure/fallback 真实落盘
2. 新增 MediaTimelineDiagnostics 和 CursorTimingDiagnostics 结构体
3. 新增 Accessibility 权限检查 — `AXIsProcessTrusted()`，前端未授权时显示提示
4. CursorKindProvider 注入 MacCursorSource — 删除重复 TTL，统一 provider 路径
5. AX 分类限频采样日志 — role chain、result code、query duration，每秒最多 2 条
6. 收集 MediaTimelineDiagnostics 数据 — 首帧 PTS origin、actual buffer size、cursor timing
7. trim_exporter overlay timestamp 显式减去 source video PTS origin
8. 新增 raw positioning 验收模式 — 关闭 smoothing/Bezier/magnification，仅渲染 glyph
9. 录制时记录 Accessibility 权限状态到 CursorTimingDiagnostics
10. 删除废弃的 AXUIElementCreateApplication FFI 声明
11. Arrow glyph 添加尾部把柄 — 匹配 macOS 原生箭头光标外形
12. 更新 BUG.md 第四轮整改状态和预防规则

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **285 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **350 unit + 10 integration tests** 通过
- `npm test -- --run` **55 tests** 通过
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过

改动文件：

- **修改**: `src-tauri/src/platform/macos/cursor_kind.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/platform/macos/permissions.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/app/permission_service.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/media/recording_metadata.rs`, `src-tauri/src/media/cursor_engine.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src-tauri/src/media/trim_exporter.rs`, `src-tauri/src/core/timeline.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/App.tsx`, `src/App.test.tsx`, `BUG.md`, `HANDOFF.md`

人工验证门禁（第四轮）：

1. BUG-0010 raw：smoothing off + magnification off，静止四角和中心，overlay 与源位置重合
2. BUG-0010 raw：水平匀速移动，overlay 不持续向右/向左漂
3. BUG-0010 raw：多段移动，overlay 不出现累计漂移
4. BUG-0011 Arrow：普通桌面导出光标为黑底白边箭头（带把柄）
5. BUG-0011 Hand：Accessibility 授权后，悬停按钮/链接导出光标为白底黑边手形
6. BUG-0011 IBeam：Accessibility 授权后，悬停文本框导出光标为黑底白边 I-beam
7. BUG-0011 未授权：未授权 Accessibility 时，UI 显示提示，metadata 中 ax_query_failure_count > 0
8. BUG-0011 Diagnostics：metadata 中 AX failure/fallback/kind distribution 可见

---

### 2026-06-04：BUG-0010/0011 第三轮整改完成

输入文件：

- `docs/superpowers/reviews/2026-06-04-bug-0010-0011-third-round-code-review-root-cause-and-fix-plan.md`
- `docs/superpowers/plans/2026-06-04-bug-0010-0011-third-round-rectification.md`

已完成（12 个 Task）：

1. 新增慢 snapshot 失败测试锁定 BUG-0010 timestamp 错位
2. timestamp 移到 snapshot() 调用前 — 避免 AX 查询耗时污染坐标时间戳
3. AX 查询限频 10Hz + snapshot duration 记录 + captured_at 时间戳
4. 新增 CursorKindProvider trait + CursorKindDiagnostics + mock 测试
5. AX provider 改用 AXUIElementCreateSystemWide — 修复跨应用 hit-test
6. 扩展 CursorKind 分类 — parent chain 回溯 3 层 + AXPress action 检查
7. CursorKindDiagnostics 写入 RecordingMetadata + stop 时结构化日志
8. 首帧 CVPixelBuffer 实际尺寸诊断日志
9. 补 Hand/IBeam rendered Y plane 测试 + cursor_frame_with_kind helper
10. 补 terminal export error UI 测试 — beautifyError 显示与 isExporting 清理
11. 更新 BUG.md 第三轮整改状态和预防规则
12. 更新 HANDOFF.md 工作记录与完整回归

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **272 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **334 unit + 10 integration tests** 通过
- `npm test -- --run` **55 tests** 通过
- `cargo fmt --check` 通过
- `npm run build` 通过

改动文件：

- **修改**: `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/platform/macos/cursor_kind.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/media/recording_metadata.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src-tauri/src/lib.rs`, `src/App.test.tsx`, `BUG.md`, `HANDOFF.md`

人工验证门禁（第三轮）：

1. BUG-0010 静止：cursor 放四角，导出 overlay 与目标点一致
2. BUG-0010 动态：快速水平移动 cursor，导出 overlay 不再随移动方向左/右漂
3. BUG-0010 Retina：Retina 显示器四角和中心
4. BUG-0011 Arrow：普通桌面导出光标为黑底白边箭头
5. BUG-0011 Hand：悬停按钮/链接导出光标为白底黑边手形
6. BUG-0011 IBeam：悬停文本框导出光标为黑底白边 I-beam
7. BUG-0011 Diagnostics：关闭 Accessibility 权限时 diagnostics 显示 failure count

---

### 2026-06-04：BUG-0010/0011 第二轮整改完成

输入文件：

- `docs/superpowers/reviews/2026-06-04-bug-0010-0011-second-round-code-review-root-cause-and-fix-plan.md`
- `docs/superpowers/plans/2026-06-04-bug-0010-0011-second-round-rectification.md`

已完成（11 个 Task）：

1. 新增 mapper 四角和负 origin 测试锁定 BUG-0010 Y 轴错误（7 个新测试）
2. 移除无条件 Y 轴翻转 — `CGEventGetLocation` 与 SCK 使用同一 top-down 坐标系
3. 添加 SCDisplay 几何诊断日志用于人工确认坐标系
4. CursorSnapshot 携带 kind 字段，record_snapshot 使用真实 kind
5. 实现 macOS CursorKind provider — Accessibility hit-test 识别 Arrow/Hand/IBeam
6. CursorClick 坐标归一化 — 与 CursorSample 使用同一 source video 坐标空间
7. 修正 Arrow/IBeam glyph 颜色 — 黑底白边符合验收要求
8. 新增像素级颜色契约测试 — Arrow/Hand/IBeam 颜色和渲染验证
9. terminal export error 写入 beautifyError 确保错误显示
10. 清理 cursor_overlay.rs 死代码 — 删除未使用 radius 变量和 draw_circle_i64
11. BUG.md 第二轮整改状态和预防规则更新

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **271 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **333 unit + 10 integration tests** 通过
- `npm test -- --run` **54 tests** 通过
- `cargo fmt --check` 通过
- `npm run build` 通过

改动文件：

- **新增**: `src-tauri/src/platform/macos/cursor_kind.rs`
- **修改**: `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/platform/macos/cursor_source.rs`, `src-tauri/src/platform/macos/mod.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src/components/preview-view.tsx`, `BUG.md`, `HANDOFF.md`

人工验证门禁（第二轮）：

1. BUG-0010 1080p：鼠标移动到四角和下半屏，导出 overlay 精确匹配源光标位置
2. BUG-0010 Retina：Retina 显示器四角和中心，导出 overlay 不偏移
3. BUG-0011 Arrow：普通桌面导出光标为黑底白边箭头
4. BUG-0011 Hand：悬停按钮/链接导出光标为白底黑边手形
5. BUG-0011 IBeam：悬停文本框导出光标为黑底白边 I-beam
6. BUG-0011 Hotspot：点击放大中心在目标 hotspot，不在 glyph 中心
7. BUG-0012 Error：导出失败时 UI 显示错误详情，isExporting 清理

---

### 2026-06-04：BUG-0010/0011/0012 实施完成

输入文件：

- `docs/superpowers/plans/2026-06-03-bug-0010-0011-0012-fix-plan.md`
- `BUG.md`

已完成（16 个 Task，3 个 Phase）：

**Phase 1（Task 1-7）：BUG-0010 光标坐标归一化**

1. `CaptureGeometry` 结构体添加到 `timeline.rs`
2. `RecordingMetadata` 新增 `capture_geometry` 字段，`CursorMetadataRecorder` 更新
3. 从 `SCDisplay` 读取显示几何信息（⚠️ FFI 需人工审查）
4. `CaptureGeometry` 从屏幕捕获传递到光标元数据运行时
5. `CursorCoordinateMapper` 实现坐标归一化（全局屏幕坐标→源视频像素坐标）
6. overlay renderer 验证（无代码变更）
7. BUG.md 更新 BUG-0010 状态和预防规则

**Phase 2（Task 8-12）：BUG-0011 CursorKind 和 Glyph 渲染**

8. `CursorKind` 枚举（Arrow/Hand/IBeam）添加到 `timeline.rs`
9. `CursorSample`/`CursorFrame` 扩展 `kind` 字段，`CursorMetadataRecorder` 写入默认 Arrow
10. `CursorEffectEngine` 保留 `kind` 通过平滑和插值
11. glyph 渲染替换白色圆点（静态 bitmask：24×24 arrow、24×24 hand、16×24 ibeam）
12. BUG.md 更新 BUG-0011 状态和预防规则

**Phase 3（Task 13-16）：BUG-0012 导出进度粒度升级**

13. `FfmpegTrimExporter` 进度从 segment 级升级为 frame/time 级
14. 导出准备阶段进度（0→5→10%）和范围映射（1-99→10-99）
15. 前端 terminal error 显示
16. BUG.md 更新 BUG-0012 状态和预防规则

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **263 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **321 unit + 10 integration tests** 通过
- `npm test -- --run` **54 tests** 通过
- `cargo fmt --check` 通过
- `npm run build` 通过

改动文件：

- **修改**: `src-tauri/src/core/timeline.rs`, `src-tauri/src/media/recording_metadata.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/media/cursor_engine.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src-tauri/src/media/trim_exporter.rs`, `src-tauri/src/lib.rs`, `src/components/preview-view.tsx`, `BUG.md`, `HANDOFF.md`

人工验证门禁：

1. BUG-0010：1080p 主显示器 — 鼠标移动到四角和中心，导出 overlay 精确匹配
2. BUG-0010：Retina 显示器 — 验证 point/pixel scale 不产生偏移
3. BUG-0011：普通桌面 — 导出光标为黑底白边箭头
4. BUG-0011：悬停按钮/链接 — 导出光标为白底黑边手形
5. BUG-0011：悬停文本框 — 导出光标为黑底白边 I-beam
6. BUG-0011：点击放大中心必须在目标 hotspot，不在 glyph 中心
7. BUG-0012：导出 10 秒视频无裁剪 — 进度从 0% 持续增长到 100%
8. BUG-0012：取消导出 — 进度 UI 清理，不显示成功状态

---

### 2026-06-03：BUG-0010/0011/0012 实施计划编写

输入文件：

- `docs/superpowers/reviews/2026-06-03-bug-0010-0011-0012-root-cause-and-fix-plan.md`
- `reference/tasks/0-bug-fix-task.md`
- `BUG.md`

已完成：

- 完成三条未解决 bug 的根因分析和详细实施计划
- 计划保存至 `docs/superpowers/plans/2026-06-03-bug-0010-0011-0012-fix-plan.md`

三条 bug 根因：

1. **BUG-0010（光标偏移）**：`MacCursorSource.snapshot()` 返回 macOS 全局屏幕坐标（`CGEventGetLocation`），但 `CursorOverlayRenderer` 假设坐标已在源视频像素空间。缺少 display origin/contentRect/pointPixelScale/stream size 的坐标归一化。
2. **BUG-0011（光标不好看）**：`CursorSample`/`CursorFrame` 没有 `kind` 字段，`CursorOverlayRenderer` 固定画白色圆点。需要 `CursorKind` 枚举（Arrow/Hand/IBeam）和 glyph 渲染替换。
3. **BUG-0012（进度不优雅）**：`FfmpegTrimExporter` 按 keep segment 上报进度（`(seg_idx+1)*99/total_keeps`），自动裁剪关闭时只有一个 keep segment，表现为 0% 停很久然后直接完成。

推荐修复顺序：BUG-0010 → BUG-0011 → BUG-0012（坐标正确性优先于形态美化，形态美化优先于体验优化）。

计划结构（16 个 Task，3 个 Phase）：

- **Phase 1（Task 1-7）**：BUG-0010 — 新增 `CaptureGeometry`、`CursorCoordinateMapper`，从 SCDisplay 读取显示几何信息，在 `CursorMetadataRecorder` 中归一化坐标
- **Phase 2（Task 8-12）**：BUG-0011 — 新增 `CursorKind` 枚举，扩展 `CursorSample`/`CursorFrame`，保留 kind 通过引擎管线，用 glyph 渲染替换圆点
- **Phase 3（Task 13-16）**：BUG-0012 — frame/time 级进度上报、准备阶段进度（0→5→10%）、范围映射、前端 terminal error 显示

关键待确认项：

1. SCDisplay 的 `pointPixelScale` 不在 `SCDisplay` 上，可能需要通过 `SCShareableContentInfo` 获取，或通过 `stream_width / display.frame().width` 推算
2. `CGEventGetLocation()` 的 Y 轴方向需实测确认（计划假设 bottom-up，可能需要 flip）
3. 手形和 I-beam 的像素级 glyph 数据需要设计（arrow 24×24 完整 bitmap 已在计划中提供）
4. Task 3 涉及 `objc2-screen-capture-kit` FFI 调用读取 SCDisplay 几何属性，**必须人工审查内存安全**

下一步：

1. 用户审阅计划，选择执行方式（Subagent-Driven 或 Inline）
2. 按 Phase 1 → Phase 2 → Phase 3 顺序执行
3. Phase 1 Task 3 完成后，需在真实设备上验证 `SCDisplay.frame()` 返回值的坐标系

改动文件：

- **新增**: `docs/superpowers/plans/2026-06-03-bug-0010-0011-0012-fix-plan.md`
- **新增**: `docs/superpowers/reviews/2026-06-03-bug-0010-0011-0012-root-cause-and-fix-plan.md`
- **新增**: `reference/tasks/0-bug-fix-task.md`
- **修改**: `BUG.md`（新增 BUG-0010/0011/0012 未解决条目）

---

### 2026-06-03：Phase 6 fourth follow-up code review 整改（结构化 stop 响应、生产 timeout 测试、类型契约对齐）

输入文件：

- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-third-follow-up-rectification-code-review-findings.md`
- `docs/superpowers/plans/2026-06-03-phase-6-bug-005-fourth-follow-up-rectification-code-review-rectification.md`

本轮修复（3 个 Issue）：

1. **Important 1: 结构化 stop 响应**（`StopRecordingResponse`）：新增 `StopRecordingResponse { result, failed }` 结构体；`drive_state_machine()` 有 errors 时 `failed=true`，前端进入 failed UI 展示具体 `finalizationErrors`，不再把 hard failure 当 warning 仅 `console.warn`。
2. **Important 2: 生产 timeout 分支回归测试**：提取 `join_worker_with_timeout()` 和 `receive_consumer_output_with_timeout()` 可测试 helper；新增 3 个回归测试（`join_worker_timeout_returns_without_joining_parked_worker`、`join_consumer_timeout_detaches_parked_consumer`、`join_consumer_timeout_records_error_and_preserves_empty_output`），使用 parked thread + never-send channel 触发真实 timeout 分支。
3. **Minor 1: finalizationErrors 类型契约对齐**：Rust 移除 `skip_serializing_if = "Vec::is_empty"`，JSON 始终包含 `finalizationErrors`（空数组），与 TypeScript 必填类型一致。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **253 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **311 tests** 通过
- `npm test -- --run` **53 tests** 通过
- `npm run build` 通过
- `cargo fmt --check` 通过

改动文件：

- **修改**: `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/App.tsx`, `src/App.test.tsx`, `BUG.md`, `HANDOFF.md`

---

### 2026-06-03：Phase 6 second follow-up code review 整改（前端构建修复、before-writer 类型、失败路径 diagnostics、timeout 测试）

输入文件：

- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-second-follow-up-rectification-code-review-findings.md`
- `docs/superpowers/plans/2026-06-03-phase-6-bug-005-second-follow-up-rectification-code-review-rectification.md`

本轮修复（5 个 Phase）：

1. **Phase A: 前端构建 blocker 修复**（Critical 1）：`App.tsx` 的 `setRecordingResult()` 透传 `writerDiagnostics` 和 `diagnostics` 字段；`App.test.tsx` 3 处 stop mock 补齐完整 diagnostics 结构；event mock 签名对齐 Tauri `Event<T>` 类型。
2. **Phase B: TS RecordingDiagnostics 补齐 before-writer 字段**（Important 2）：`src/lib/tauri.ts` 新增 6 个 before-writer 字段（`systemRmsMaxBeforeWriter` 等），与 Rust serde shape 完全对齐。
3. **Phase C: 失败路径保留结构化 diagnostics**（Important 1 + Minor 1）：`RecordingResult` 新增 `finalization_errors` 字段（`serde skip_serializing_if = "Vec::is_empty"`）；`drive_state_machine()` 有 errors 时写入 `result.finalization_errors` 而非丢弃 result；前端类型同步更新；`record_source_contribution()` trait 注释修正。
4. **Phase D: timeout 分支直接回归测试**（Important 3）：新增 `recv_timeout_returns_quickly_on_never_send_channel`（writer 侧）、`recv_timeout_consumer_returns_quickly_on_never_send_channel`（consumer 侧）、`detached_thread_does_not_block_on_drop`、`consumer_timeout_fallback_preserves_diagnostics_slot`。
5. **Phase E: 文档与门禁收口**（Important 4）：HANDOFF.md 验证结果更新、BUG.md 预防规则补充。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **251 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **308 unit + 10 integration tests** 通过
- `npm run build` 通过
- `npm test -- --run` **52 tests** 通过

改动文件：

- **修改**: `src/App.tsx`, `src/App.test.tsx`, `src/lib/tauri.ts`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `BUG.md`, `HANDOFF.md`

---

### 2026-06-03：Phase 6 follow-up code review 整改（第二轮）（per-source counter timing、diagnostics 暴露、timeout 测试）

输入文件：

- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-follow-up-rectification-code-review.md`
- `docs/superpowers/plans/2026-06-03-phase-6-bug-005-follow-up-rectification-code-review-rectification.md`

本轮修复（4 个 Phase）：

1. **Phase A: per-source writer counter 时序修正**（Important 1）：`record_source_contribution()` 从 `push_audio()` 前移到 `Ok` 分支；push_audio 失败时 per-source counters 不再虚假递增；新增 2 个回归测试。
2. **Phase B: RecordingResult 暴露 RecordingDiagnostics**（Important 2）：`RecordingResult` 新增 `diagnostics` 字段（含 `mic_stop_diagnostics`）；`drive_state_machine()` 将 capture diagnostics 写入 result；前端 TypeScript 类型同步更新；新增序列化 camelCase 测试。
3. **Phase C: timeout 分支回归测试**（Minor 1）：`empty_consumer_output()` 方法提取并验证默认值安全；`extract_panic_message` 覆盖 &str/String/unknown payload 测试。
4. **Phase D: 文档清理**（Minor 2）：review 文档尾部空白清理；BUG.md 补充预防规则 32-33；HANDOFF.md 更新。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **247 tests** 通过
- `npm test -- --run` **52 tests** 通过
- `cargo fmt --check` 通过

改动文件：

- **修改**: `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src/lib/tauri.ts`, `BUG.md`, `HANDOFF.md`
- **清理**: `docs/superpowers/reviews/*.md`（尾部空白）

---

### 2026-06-03：Phase 6 follow-up code review 整改（bounded finalize、drop ratio、mic diagnostics、per-source writer）

输入文件：

- `docs/superpowers/reviews/2026-06-03-phase-6-bug-005-current-rectification-code-review.md`
- `docs/superpowers/plans/2026-06-03-phase-6-bug-005-follow-up-rectification.md`

本轮修复（6 个 Task）：

1. **Task 1: FFmpeg writer timeout 不再无界 join**（Critical 1）：`join_worker()` timeout 分支移除 `handle.join()`，直接返回 `RecordingWriteFailed` 错误；Disconnected 分支保留 join（worker 已退出）；新增 `extract_panic_message` helper 和 timeout 测试。
2. **Task 2: Consumer timeout 不再无界 join**（Critical 2）：`join_consumer()` timeout 分支移除 `handle.join()`，使用 `empty_output` 继续 cleanup；fallback 分支也移除无界 join，记录状态不一致错误。
3. **Task 3: Drop ratio 分母修正**（Important 1）：system/mic drop ratio 分母从 `received` 改为 `received + dropped`（attempted total）；日志中的"总计"改为 attempted total；新增 9.09% pass 和 10.71% fail 边界测试。
4. **Task 4: 蓝牙 mic stop diagnostics 结构化返回**（Important 2）：新增 `stop_with_diagnostics()` 方法返回 `CpalMicrophoneStopDiagnostics`；`macos_service` 使用 `stop_with_diagnostics()` 并在重建 capture 前保留 diagnostics；`RecordingDiagnostics` 新增 `mic_stop_diagnostics` 字段。
5. **Task 5: Writer per-source diagnostics**（Important 3）：`WriterDiagnostics` 新增 `system_chunks_received_by_writer` / `mic_chunks_received_by_writer`；`RecordingWriter` trait 新增 `record_source_contribution()` 方法；`consume_frames()` 在每次 `push_audio` 前记录 per-source 贡献；`validate_source_aware_audio_contract()` 新增 per-source writer 检查。
6. **Task 6: 文档修复与完整回归**：cargo fmt 修复、drop 日志"音频通道"改为"媒体通道"、RAII guard Drop 注释修正、BUG.md 补充预防规则 28-31。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **243 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **297 unit + 10 integration tests** 通过
- `npm test -- --run` **52 tests** 通过
- `cargo fmt --check` 通过

改动文件：

- **修改**: `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/core/media_channel.rs`, `BUG.md`, `HANDOFF.md`

---

### 2026-06-02：Phase 6 code review 整改（drop source logging、RAII guard、stop 测试补充）

输入文件：

- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md`
- `docs/superpowers/plans/2026-06-02-phase-6-bug-005-code-review-rectification.md`

本轮修复（6 个 Task）：

1. **Task 1: Media Channel source-aware drop logging**：`MediaSender` 新增 `source: &'static str` 字段；`bounded_media_channel` 签名增加 source 参数；`try_send_drop_newest()` 在 drop 时打印 source 标识日志（首条 + 每 100 条）；所有调用点更新为传入 "video"/"system"/"mic" 标识。
2. **Task 2: CPAL callback drop logging**：`cpal_microphone.rs` 移除 `let _ = sink.try_send_drop_newest(chunk)` 的静默忽略，改为直接调用（drop 日志由 MediaSender 层处理）。
3. **Task 3: RecordingFinalizeGuard RAII 结构体**：新增 `RecordingFinalizeGuard` 结构体，将 `stop()` 的 9 个清理步骤重构为 `stop_captures()` → `join_consumer()` → `write_sidecars()` → `reset_mic()` → `collect_errors()` → `drive_state_machine()` 链式调用；`Drop` impl 确保 panic 时 stop_flag 和 mic_level 仍被重置。
4. **Task 4: Stop-During-Startup 集成测试**：新增 `consume_frames_respects_stop_flag_set_before_start`（stop_flag 在 consumer 启动前设置）和 `consume_frames_drains_queued_data_on_stop`（active processing 中 stop 后完整 drain）两个测试。
5. **Task 5: Drop Ratio Hard Fail 测试**：新增 `consume_frames_fails_on_high_audio_drop_ratio` 测试，验证 20% drop ratio > 10% 阈值时触发 hard fail。
6. **Task 6: BUG.md 预防规则补充**：新增规则 24-27，覆盖 source-aware drop logging、CPAL callback drop、RAII guard、stop-during-startup 测试。

验证结果：

- `cargo test -p luzhi` **239 tests** 通过
- `cargo clippy` 通过（既有 warnings）

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/core/media_channel.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/app/recording_service.rs`

---

### 2026-06-02：Phase 6 整改后 code review 整改（CPAL lazy offset、drop 分层、bounded finalize、蓝牙 stop diagnostics）

输入文件：

- `docs/superpowers/reviews/2026-06-02-phase-6-bug-005-post-rectification-code-review.md`
- `docs/superpowers/plans/2026-06-02-phase-6-bug-005-post-rectification-code-review-rectification.md`

本轮修复（5 个 Phase）：

1. **Phase B CPAL lazy offset sentinel**（Important 2）：`AudioSampleClock` 新增 `offset_initialized: AtomicBool`；`initialize_offset()` 使用 `AtomicBool` CAS 做 first-call-wins，不再用 `0` 做 sentinel；新增 2 个 zero-offset 回归测试。
2. **Phase C drop warning/error 分层**（Important 3）：system/mic chunk drop 降级为 `eprintln` warning + drop ratio 日志，不再直接进入 errors；新增 10% drop ratio hard fail 阈值；新增 small-drop pass 测试。
3. **Phase D source-aware discard 对称化**（Important 4）：确认 writer discard ratio 检查已对称覆盖 requested system 和 mic（已有代码）；新增 system discard 和 mic discard 测试验证。
4. **Phase A 真正 bounded finalize**（Important 1）：writer worker result 通过 `mpsc::channel` 返回，`join_worker` 改为 `recv_timeout(10s)`；consumer thread result 通过 `mpsc::channel` 返回，`stop` 改为 `recv_timeout(15s)`；超时后尝试 `handle.join()` 获取 panic 信息。
5. **Phase E 蓝牙 mic stop diagnostics**（Minor 1）：新增 `CpalMicrophoneStopDiagnostics` 结构体；`stop()` 记录 pause/drop/wait/callbacks_after_stop；callback 中 `running=false` 时递增计数；BUG.md 补充预防规则 21-23。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **289 unit + 10 integration tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **236 tests** 通过
- `npm test -- --run` **52 tests** 通过
- `cargo fmt --check` 通过
- `cargo clippy` 通过（既有 warnings）

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/core/clock.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/platform/macos_service.rs`

---

### 2026-06-02：Phase 6 第 10 节 code review 整改（source-aware before-writer diagnostics、audible contract、蓝牙 mic pause、synchronizer grace、writer bounded join）

输入文件：

- `docs/superpowers/reviews/2026-06-01-phase-6-bug-005-code-review.md` 第 10 节
- `docs/superpowers/plans/2026-06-02-phase-6-bug-005-rectification.md`

本轮修复（7 个 Task）：

1. **Task 1 AudioSynchronizer source start grace**：`AudioSynchronizerConfig` 新增 `source_start_grace_nanos`（默认 500ms）；`calculate_watermark()` 返回 `(u64, bool)` timeout mode；双源 requested 但仅一路 seen 时在 grace 内不 emit；grace 超时或 source stall 后 emit 标记 `emitted_due_to_timeout=true`；新增 `first_system_seen_nanos`/`first_mic_seen_nanos` 跟踪首次 seen 时间。
2. **Task 2 RecordingDiagnostics before-writer fields**：新增 `system_windows_before_writer`/`mic_windows_before_writer`/`system_frames_before_writer`/`mic_frames_before_writer`；`validate_source_aware_audio_contract()` 增加 before-writer windows/RMS 检查。
3. **Task 3 audible_min_rms enforcement**：`validate_source_artifact_with_audio_contract()` 和 `validate_export_artifact_with_audio_contract()` 新增 Level 2 audible 检查：RMS < `audible_min_rms`（0.015）时 fail。
4. **Task 4 writer bounded join**：`FfmpegRecordingWriter.tx` 改为 `Option<SyncSender>`；`finish()` 在 `join_worker()` 前 `self.tx.take()` drop sender；新增慢 join 日志（>10s warning）。
5. **Task 5 drain loop diagnostics write-back**：`consume_frames()` 的 live drain 和 final drain 在 `writer.push_audio()` 前写入 per-source before-writer RMS/frames/windows 到 `RecordingDiagnostics`。
6. **Task 6 蓝牙 mic stop lifecycle**：`CpalMicrophoneCapture::stop()` 新增显式 `pause()` 调用并记录结果；wait 从 200ms 增加到 300ms；`MacRecordingService::stop()` 仅在 `last_requested_microphone` 时执行 mic stop；stop 后重建 `CpalMicrophoneCapture::new()`。
7. **Task 7 export contract integration**：`export_video()` 修复 `unwrap()` 为 `map_err`；contract 已使用 `..Default::default()` 包含 `audible_min_rms=0.015`。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **279 unit + 10 integration tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **229 tests** 通过（无 ffmpeg 无回归）
- `npm test -- --run` **52 tests** 通过
- `cargo fmt --check` 通过
- `cargo clippy` 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/audio_synchronizer.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/lib.rs`

---

### 2026-06-02：Phase 6 第 25 节 code review 整改（source-aware synchronizer、source-aware contract、CPAL lazy offset、writer partial-overlap diagnostics、蓝牙 mic 释放）

输入文件：

- `docs/superpowers/reviews/2026-06-01-phase-6-bug-005-code-review.md` 第 25 节

本轮修复（8 个 Task）：

1. **Task 1 失败测试锁定**：新增 3 个失败测试锁定 Critical 1/2 缺陷（dual-source offset、long-chunk splitting、fast-source watermark）。
2. **Task 2 AudioSynchronizer 重构**：重构为 source-aware fixed-window sample merger。watermark 使用 `min(system_ts, mic_ts)` 防止快源单独推进；chunk 按 sample frame 切分到 20ms 窗口；source stall timeout 防止无限阻塞；`SynchronizedAudioChunk` 新增 `system_frames`/`mic_frames`/`emitted_due_to_timeout`。
3. **Task 3 consumer 线程接入**：`macos_service.rs` 传入 `AudioSynchronizerConfig`（requested sources）；`drain_mixed()` 返回 `SynchronizedAudioChunk`，consumer 用 `synced.mixed` 推入 writer；`RecordingDiagnostics` 新增 `source_timeout_window_count`/`system_rms_max_before_writer`/`mic_rms_max_before_writer`。
4. **Task 4 writer partial-overlap diagnostics**：`TimelineAppendResult` 新增 `trimmed_partial_overlap`/`trimmed_frames`；partial-overlap 分支正确设置标志；encoder_worker 递增 `audio_chunks_trimmed_partial_overlap`。
5. **Task 5 source-aware contract**：`RequestedAudioContract` 新增 `audible_min_rms=0.015`；新增 `validate_source_aware_audio_contract()` 检查每个请求源的 window emission 和 discard ratio；consumer thread 在 artifact contract 后调用。
6. **Task 6 CPAL lazy offset**：`AudioSampleClock.session_offset_nanos` 改为 `AtomicU64`；新增 `initialize_offset()` 在首次回调时用 CAS 设置 offset（`callback_now - buffer_duration`）；`cpal_microphone.rs` 不再在 stream build 时调用 `with_session_clock()`。
7. **Task 7 导出 contract + 蓝牙 mic 释放**：`export_video()` 使用 `validate_export_artifact_with_audio_contract` 代替 plain validation；`MacRecordingService` 新增 `last_requested_system_audio`/`last_requested_microphone`；stop 顺序改为 mic first；`CpalMicrophoneCapture::stop()` 显式 drop stream + 200ms bounded wait。
8. **Task 8 BUG.md + 回归**：更新 BUG-005 状态和 6 条新增预防规则。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **273 unit + 10 integration tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过
- `npm test -- --run` **52 tests** 通过
- `cargo fmt --check` 通过
- `cargo clippy` 通过（既有 warnings）

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/audio_synchronizer.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/core/clock.rs`, `src-tauri/src/lib.rs`

---

### 2026-06-01：Phase 6 第 24 节 code review 整改（RequestedAudioContract、window merger、WriterDiagnostics、finish non-blocking、strict helper、蓝牙提示）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 24 节

本轮修复（6 个 Phase）：

1. **R1 RequestedAudioArtifactContract**（Critical 1）：新增 `RequestedAudioContract` 结构体和 `validate_source_artifact_with_audio_contract()` / `validate_export_artifact_with_audio_contract()`。当用户请求了音频源时，validation 解码 artifact 的 AAC 流并检查 decoded RMS/peak 是否超过阈值（min_rms=0.003, min_peak=0.02）。未请求音频时允许 silent AAC track。`macos_service.rs` consumer thread 在 writer.finish() 后自动执行 contract validation。
2. **R2 AudioSynchronizer window merger**（Critical 2）：将 AudioSynchronizer 从 per-chunk 配对重构为固定 20ms 窗口合并器。system/mic chunk 按 timestamp 分配到窗口，每个窗口最多输出一个 `SynchronizedAudioChunk`。watermark-based live drain 确保窗口在 hold window（40ms）后才输出。彻底解决 `mixed_chunks_written = system_chunks_received + mic_chunks_received` 的双写问题。
3. **R3 WriterDiagnostics**（Critical 3）：新增 `WriterDiagnostics` 结构体，区分 `audio_chunks_received`/`audio_chunks_appended`/`audio_chunks_discarded_full_overlap`/`audio_chunks_trimmed_partial_overlap`/`aac_frames_encoded`。`RecordingResult` 携带写入器诊断。`RecordingDiagnostics.mixed_chunks_written` 重命名为 `mixed_chunks_queued` 以明确语义。
4. **R4 finish() non-blocking**（Important 2）：`finish()` 从 blocking `send(Flush)` 改为 `try_send(Flush)` + bounded retry（100次 × 10ms = 1秒上限），不再无限阻塞录制停止路径。
5. **R5 strict synthetic artifact helper**（Important 3）：新增 `create_synthetic_source_artifact_strict()`，任何 push 失败立即返回错误。音频内容测试（RMS/peak 验证）使用 strict helper，backpressure 测试保留 tolerant helper。
6. **R6 麦克风设备选择和蓝牙兼容提示**（Important 4）：后端新增 `list_microphone_devices` 命令返回设备列表和蓝牙检测。前端新增麦克风设备选择器，对蓝牙麦克风显示 HFP profile 兼容性警告。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **263 unit + 10 integration tests** 通过
- `npm test -- --run` **52 tests** 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/lib.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/audio_synchronizer.rs`, `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/test_support/ffmpeg_helpers.rs`, `src/lib/tauri.ts`, `src/App.tsx`, `src/components/recording-panel.tsx`

---

### 2026-06-01：Phase 6 第 23 节 code review 整改（audio diagnostics、synchronizer、RMS inspection、non-blocking queue）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 23 节

本轮修复（7 个 Phase）：

1. **R1 audio diagnostics 和 requested-audio contract**（Critical 1）：新增 `RecordingDiagnostics` 结构体跟踪 `requested_system_audio`/`requested_microphone`、system/mic chunks received/dropped、mixed chunks written、writer push_audio failures、RMS max、generated silent track。consumer thread 在 diagnostics 中记录所有音频源 metrics，当请求的音频源无 chunk 或被 drop 时输出明确警告。
2. **R2 AudioSynchronizer drain_final 和 source-aware output**（Critical 2）：新增 `drain_final()` 方法在停止录制时 flush all remaining system/mic chunks，不再按 MAX_HOLD_NANOS 保留。新增 `SynchronizedAudioChunk` 结构体携带 `has_system`/`has_mic`/`system_rms`/`mic_rms` 元数据。consumer thread 使用 `drain_final()` 替代 `drain_mixed()` 作为最终排空路径。
3. **R3 FFmpeg artifact audio RMS/peak inspection**（Important 4）：`MediaArtifactInspection` 新增 `audio_sample_count`/`audio_rms`/`audio_peak`/`audio_sample_rate`/`audio_channels` 字段。新增 `decode_audio_stats()` 函数解码 AAC 流计算 RMS 和 peak。新增 `inspect_media_artifact_with_audio_stats()` 便捷函数。
4. **R4 真实 24kHz/1ch mixer -> writer integration test**（Important 3）：新增 `ffmpeg_writer_records_non_silent_mixed_24khz_mono_mic` 测试，构造 `AudioChunk { sample_rate: 24000, channels: 1 }` 经 `SimpleAudioMixer::mix()` 后推入 writer，验证 mixed sample layout、A/V drift、decoded audio RMS。
5. **R5 writer queue non-blocking send 和 audio starvation 修复**（Important 1）：`FfmpegRecordingWriter` 的 `push_video()`/`push_audio()` 从 blocking `send()` 改为 non-blocking `try_send()`，queue full 时返回结构化错误。consumer loop 使用 bounded batch（MAX_VIDEO_BATCH_PER_ITERATION=10）处理视频帧，避免音频被无限 drain video 饿死。queue capacity 从 25 调整为 64。
6. **R6 蓝牙耳机麦克风兼容性**（Important 2）：已在 `docs/platform-diff/macos-compatibility.md` 记录为已知限制。UI 设备选择需后续实现。
7. **R7 真实设备 manual gate 收口**：已在测试中添加 audio RMS inspection 和 24kHz mixer integration test，真实设备验证仍需人工完成。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **212 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **254 unit + 10 integration tests** 通过
- `npm test -- --run` **52 tests** 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/media/audio_synchronizer.rs`, `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/test_support/ffmpeg_helpers.rs`

---

### 2026-06-01：Phase 6 第 25 节 code review 整改（BUG-009 writer gap 修复、per-source metadata、diagnostics 语义修正）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 25 节

**BUG-009 根因**：`FfmpegRecordingWriter` 音频 timeline gap 分支实现错误。当 `target_sample > audio_timeline_cursor` 时，writer 只补齐 gap silence 并推进 cursor 到 chunk 起点，没有追加当前 audio chunk 的真实 samples。真实设备录制的首个音频 chunk 通常带有非零 timestamp，因此大量非静音 PCM 被替换为静音 AAC frame。

本轮修复（6 个 Phase）：

1. **R1 WriterDiagnostics 扩展**：新增 `audio_real_frames_appended`、`audio_silence_frames_padded`、`audio_real_rms_max_before_encode`、`silent_aac_frames_encoded`、`generated_silent_track` 字段，区分真实 PCM append 与 silence padding。
2. **R2 BUG-009 writer gap 分支修复**：提取 `append_audio_chunk_to_timeline()` helper，gap 分支补齐静音后必须继续 append 当前 chunk 的真实 PCM 样本，cursor 推进到 chunk 结束位置。新增 `compute_chunk_rms()` helper 计算真实 PCM RMS。
3. **R3 silent track diagnostics**：`generated_silent_track` 从 writer diagnostics 直接获取，不再通过 `mixed_audio_chunk_count == 0` 推断。
4. **R4 macos_service.rs 更新**：`diagnostics.generated_silent_track = result.writer_diagnostics.generated_silent_track`。
5. **R5 AudioSynchronizer per-source metadata**：`AudioWindow` 改为 `SourceWindowBuffer` 结构，system 和 mic 各自保留 `sample_rate/channels`，避免 48kHz/2ch system 与 48kHz/1ch mic 被套用同一份 metadata。
6. **R6 新增测试**：`ffmpeg_writer_preserves_non_silent_audio_after_leading_gap`、`ffmpeg_writer_preserves_non_silent_audio_after_middle_gap`、`audio_synchronizer_preserves_mic_mono_metadata_when_system_arrives_first`、`audio_synchronizer_preserves_system_stereo_metadata_when_mic_arrives_first`。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **266 unit + 10 integration tests** 通过
- `npm test -- --run` **52 tests** 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/media/audio_synchronizer.rs`, `src-tauri/src/platform/macos_service.rs`

---

### 2026-06-01：Phase 6 第 22 节 code review 整改（partial-overlap drain、mixer defense、test matrix）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 22 节

本轮修复（6 个 Phase）：

1. **R1 partial-overlap drain bug 修复**（Critical 1）：`ffmpeg_writer.rs` 提取 `drain_audio_sample_buffer()` helper，partial-overlap chunk append 后立即进入 AAC drain loop，不再跳过。flush 阶段复用同一 helper。
2. **R2 BUG-005 测试矩阵补强**（Important 2）：新增 `ffmpeg_writer_drains_partial_overlap_audio_before_finish`（~500 轻微 overlap chunks，A/V drift < 1s）和 `ffmpeg_writer_handles_out_of_order_audio_chunks` 测试。既有 overlap 测试增加 duration 非膨胀断言。
3. **R3 AudioMixer 输入 metadata 防御**（Important 1）：新增 `validate_audio_chunk()` 在 `passthrough()`/`mix_two()` 入口校验 `channels > 0`、`sample_rate > 0`、`samples.len() % channels == 0`。新增 4 个测试覆盖零通道、零采样率、样本数不匹配场景。
4. **R4 cursor overlay integration tests**（Important 3）：新增 `ffmpeg_exporter_accepts_render_cursor_overlay_false_noop_timeline` 和 `ffmpeg_exporter_rejects_required_overlay_with_empty_timeline` 集成测试。
5. **R5 terminal progress test**（Important 3）：前端新增 `clears exporting state on terminal progress with cancellable=false` 测试，验证 no-FFmpeg gate 的 `cancellable=false` 事件清除导出状态。
6. **R6 test helper artifact contract**（Minor 1）：`create_synthetic_source_artifact()` 新增 output_path 校验和 `inspect_media_artifact()` 自证 video/audio stream 和非零 duration。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **206 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **245 unit + 10 integration tests** 通过
- `npm test -- --run` **52 tests** 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/media/audio_mixer.rs`, `src-tauri/src/test_support/ffmpeg_helpers.rs`, `src-tauri/tests/ffmpeg_export.rs`, `src/App.test.tsx`

---

### 2026-06-01：Phase 6 第 21 节 code review 整改（BUG-005/008 修复、no-op export、terminal progress）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 21 节

本轮修复（7 个 Phase）：

1. **R1 文档状态收口**：更新 BUG.md BUG-005 状态为"部分修复-待真实设备验证"，补充修复进展和预防规则。更新 HANDOFF.md 状态。
2. **R2 writer audio timeline 测试**：新增 5 个 BUG-005 相关测试：leading gap padding、overlap trimming、full overlap discard、24kHz mic after mixer A/V drift、multiple chunk inflation prevention。
3. **R3 writer audio timeline merge 修复**（BUG-005 核心修复）：`ffmpeg_writer.rs` 音频时间轴合并逻辑重写——首个 chunk 正确补前导静音、overlap 裁剪/丢弃、`audio_timeline_cursor` 作为唯一时间轴游标、`audio_pts` 作为单调递增编码器 PTS 计数器、tail padding 使用 video end + one frame duration。修复 flush 阶段处理多帧 buffer 的逻辑。
4. **R4 AudioMixer::to_stereo() 多声道修复**：`src_channels > 2` 时按 frame 截取前两个通道，`src_channels == 0` 返回空输出。
5. **R5 cursor overlay 数值安全修复**（BUG-008 完全闭环）：`cursor_overlay.rs` 对 mapped coordinates 做 finite check、viewport clamp、i64 bounding box arithmetic。新增 3 个 BUG-008 测试。
6. **R6 raw cursor visible no-op export 修复**：`trim_exporter.rs` 区分 `render_cursor_overlay=false`（no-op）和 overlay required but failed（fatal）。raw cursor 已可见时基础导出不再被误阻断。
7. **R7 no-FFmpeg terminal progress 修复**：`lib.rs` export handler 对 `Ok(output_path=None)` 也 emit terminal `export-progress`（`cancellable=false`），UI 不再残留 exporting 状态。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **199 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **243 unit + 8 integration tests** 通过
- `npm test -- --run` **51 tests** 通过
- `npm run build` 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src-tauri/src/media/trim_exporter.rs`, `src-tauri/src/media/audio_mixer.rs`, `src-tauri/src/lib.rs`

---

### 2026-05-31：Phase 6 第 20 节 code review 整改（BUG-005/006/008 修复）

输入文件：

- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 第 20 节

本轮修复（6 个 Phase）：

1. **R1 BUG.md housekeeping**：修正重复 BUG-005 编号（已解决区改为 BUG-007），拆分 BUG-006 为两个独立条目（BUG-006 系统音频稀疏时间轴、BUG-008 cursor overlay 数值溢出），更新 HANDOFF.md 状态。
2. **R2 麦克风 CPAL 配置协商**（BUG-005）：`cpal_microphone.rs` 不再使用前端请求的 48kHz/stereo 直接构建输入流，改为使用 `device.default_input_config()` 的实际设备配置。`AudioMixer` 负责下游重采样。增加配置协商诊断日志。
3. **R3 writer 音频 timestamp 与 silence padding**（BUG-006 现象一）：`EncoderMessage::Audio` 携带 `timestamp_nanos`，worker 维护 48kHz audio timeline cursor。对前导 gap、中间 gap（稀疏系统音频）和尾部 gap 写入 silence。修复 ffmpeg_common.rs 重复注释。
4. **R4 cursor overlay 数值安全**（BUG-006 现象二 / BUG-008）：`draw_on_frame` 对 `x/y/scale` 做 finite check，scale clamp 到 `0.25..=4.0`，radius clamp 到 `4..min(frame/2, 256)`。`draw_circle`/`draw_circle_outline` 距离计算改用 `i64` 防止溢出。
5. **R5 trim 后 cursor overlay 时间轴**：`draw_on_frame` 参数从 output PTS 改为 source timestamp nanos。trim_exporter 传入 `raw_pts` 转换的 source nanos，避免 auto-trim 后 cursor 查询错位。
6. **R6 no-FFmpeg gate、terminal progress 与 UI 状态收口**：no-FFmpeg branch 不再调用 MockTrimExporter，返回明确 gate 错误。failure/cancel 路径 emit terminal `export-progress`（`cancellable: false` + error payload）。

验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml` **199 tests** 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **228 unit + 8 integration tests** 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` 通过（既有 macOS FFI warnings）
- `npm test -- --run` **51 tests** 通过
- `npm run build` 通过

改动文件：

- **修改**: `BUG.md`, `HANDOFF.md`, `src-tauri/src/platform/macos/cpal_microphone.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/src/media/cursor_overlay.rs`, `src-tauri/src/media/trim_exporter.rs`, `src-tauri/src/lib.rs`

---

### 2026-05-30：Phase 6 FFmpeg 可播放导出实现

输入文件：

- `docs/superpowers/plans/2026-05-30-phase-6-ffmpeg-playable-export.md`

本轮完成（4 Tasks）：

1. **FfmpegRecordingWriter 完整编码**：BGRA→YUV420P（swscale）+ H.264（libx264 ultrafast）+ AAC + MP4 muxing。支持动态分辨率输入、惰性 scaler 初始化、flush 编码器、写入 trailer。
2. **FfmpegTrimExporter 完整导出**：输入解码→裁剪段 seek→视频缩放→音频重采样→编码→muxing。支持三种预设分辨率（1920×1080/1080×1920/1080×1080）、cancel token、progress 回调。
3. **集成测试** `tests/ffmpeg_export.rs`：5 个 artifact-level 测试（全时长导出、三种预设分辨率、取消清理、缺失源文件）。
4. **手动测试清单** `tests/phase-6-manual-ffmpeg-gates.md`：6 组 Gate 测试（1080p 压力、A/V 同步、原始保留、取消清理、预设尺寸、无 FFmpeg Gate）。

额外修复：

- `test_support/ffmpeg_helpers.rs`：修复 `stride_bytes` 类型（u32→usize）和 `codecpar`→`parameters()` API 兼容性。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg` **214 tests** 通过（209 unit + 5 integration）
- `cargo clippy --manifest-path src-tauri/Cargo.toml --features ffmpeg --all-targets` 通过（warnings only）
- `cargo build --manifest-path src-tauri/Cargo.toml --features ffmpeg` 通过
- `npm run build` 通过
- `npm test -- --run` **51 tests** 通过

改动文件：

- **修改**: `src-tauri/src/media/ffmpeg_writer.rs`（完整实现）, `src-tauri/src/media/trim_exporter.rs`（完整实现）, `src-tauri/src/test_support/ffmpeg_helpers.rs`（API 兼容修复）
- **新增**: `src-tauri/src/media/ffmpeg_common.rs`, `src-tauri/tests/ffmpeg_export.rs`, `tests/phase-6-manual-ffmpeg-gates.md`

Code Review 整改（2026-05-30）：

**整改背景**：Code Review 于 commit `555b7f2` 后执行，发现 3 Critical + 7 Important 问题。以下整改在 review 之后进行：

1. **C1 修复**：exporter 视频帧复制改为逐行复制，正确处理 linesize（stride）padding，避免 padded 格式下产生乱码。
2. **C3 修复**：`export_video()` 在导出成功后调用 `validate_export_artifact()` 校验输出文件（流存在、分辨率匹配），不通过则清理并报错。
3. **I1 修复**：新增 `media/ffmpeg_common.rs` 生产模块，将 `inspect_media_artifact` 和 `validate_export_artifact` 从 test_support 移入生产代码。
4. **I2 修复**：writer 音频 PTS 按实际采样率换算到 48kHz 时间基，防止 A/V 漂移。
5. **I3 修复**：exporter 多段裁剪时 PTS 连续映射——跟踪累计裁剪时长，减去间隙，输出时间戳连续。
6. **I4 修复**：非 EOF 数据包读取错误不再静默吞掉，改为 `eprintln!` 警告后跳过。
7. **I7 修复**：exporter flush 阶段补上 `rescale_ts` 调用。

**未修复（需人工评估）**：
- **C2**：exporter 音频未做格式重采样（当前仅兼容 writer 产出的 F32P@48kHz，外部 MP4 需加 SwrContext）。
- **I5**：无音频录制时未生成静音 AAC 轨道（需 finish() 中补充静音帧逻辑）。
- **I6**：集成测试未覆盖裁剪时间线导出（CutTimeline 非空场景）。
- **M1-M5**：Minor 代码质量改进。

**Native Safety 审查待办**：
- `trim_exporter.rs` 中的 `unsafe { frame::Frame::empty() }` 和逐行 plane 复制
- `ffmpeg_writer.rs` 中的 `SendScaler` unsafe impl Send
- `ffmpeg_common.rs` 中的 `unsafe { &*params.as_ptr() }` 访问 AVCodecParameters

---

### 2026-05-30：Phase 6 导出预设与本地授权边界部分完成

输入文件：

- `docs/superpowers/plans/2026-05-29-phase-6-export-presets-local-license.md`
- `tests/phase-6-w11-w12-checklist.md`
- `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md`

本轮完成（边界/helper/UI 骨架）：

1. `media/export_presets.rs` 建立固定三种导出预设（Bilibili 16:9、抖音 9:16、小红书 1:1）。
2. `media/export_paths.rs` 生成独立输出路径并验证非空输出。
3. `media/trim_audio_activity.rs` 实现灵敏度无关的 100ms 基础 RMS 桶，支持录后灵敏度重聚合。
4. `app/export_service.rs` 建立结构化导出服务边界，验证源文件、生成请求、调用导出器。
5. `ExportProgressPayload` 事件和 `cancel_export` 命令接入 UI 进度条与取消按钮。
6. `media/original_recording_artifact.rs` 源文件验证器和 `ffmpeg_test_support.rs` 测试辅助。
7. `FfmpegTrimExporter` 更新为使用新错误类型和验证逻辑（实际转码需 Native Safety 审查）。
8. `app/license_service.rs` 实现本地 14 天试用与激活状态接口，前端展示试用状态。

Code Review 整改（2026-05-30）：

**整改背景**：Code Review 文件 `docs/superpowers/reviews/2026-05-30-phase-6-code-review.md` 于 commit `8e050f0` 时生成。后续 commits `a17382a` 和 `e2a6ae2` 包含格式化和锁合并等重构，但未包含行为修复。以下整改在 review 之后进行：

1. **Critical 1 修复**：`export_video()` 在非 FFmpeg 构建中不再因缺失 source artifact 陷入死路，返回明确 FFmpeg Gate 错误。
2. **Critical 2 修复**：`export_video()` 接入 `ExportService` 和 `TrimExporter`，cancel token 和 progress callback 进入 exporter boundary。
3. **Important 2 修复**：live-loop `push_audio` 错误不再被吞掉，与 final drain 保持一致进入 `errors`。新增 `consume_frames_writer_push_audio_failure_records_error` 测试验证。
4. **Important 3 修复**：`ExportService` 在 cancel/failure/validation error 时清理 partial output 文件。
5. **Important 1 修复**：Base RMS bucket 边界改为严格 `[start, start+100ms)`，exact-boundary sample 进入下一个 bucket。
6. **Minor 1 修复**：`MAX_AUDIO_SAMPLES` 注释更正为 `~2h @ 10/sec (100ms base buckets)`。
7. **Minor 3 修复**：清理 unused variable warnings。

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **198 tests** 通过（+5 相比 Phase 5）
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 通过（warnings only）
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` **51 tests** 通过（+5 相比 Phase 5）

改动文件：

- **新增**: `src-tauri/src/media/export_presets.rs`, `src-tauri/src/media/export_paths.rs`, `src-tauri/src/media/trim_audio_activity.rs`, `src-tauri/src/media/original_recording_artifact.rs`, `src-tauri/src/media/ffmpeg_test_support.rs`, `src-tauri/src/app/export_service.rs`, `src-tauri/src/app/license_service.rs`, `src/components/license-status.tsx`
- **修改**: `src-tauri/src/media/mod.rs`, `src-tauri/src/media/trim_exporter.rs`, `src-tauri/src/media/trim_metadata.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/error.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/components/preview-view.tsx`, `src/App.tsx`, `src/App.test.tsx`, `tests/phase-6-w11-w12-checklist.md`

**Phase 6 当前状态（Code Review 结论）**：

- ✅ **已完成**：固定三种 export presets、export path helper、source artifact validator、local 14-day trial、activation status boundary、no activation secret hardcoded。
- ⚠️ **边界/helper 通过，产品路径未接入**：ExportService helper、ExportProgressPayload、cancel_export command。
- ❌ **未完成（阻塞 Phase 6 完成）**：
  - 默认录制仍使用 `CountingRecordingWriter::new(None)`，无原始录制 artifact。
  - `export_video()` 已接入 ExportService，但 FFmpeg exporter 仍是骨架。
  - FFmpeg writer 不写文件，FFmpeg exporter 永远返回 "实现需补齐"。
  - `src-tauri/tests/ffmpeg_export.rs` 不存在。
  - Manual FFmpeg Gates 全部未完成。
  - 文档和 checklist 有明显 overclaim。

**剩余待完成（阻塞 Phase 6 export 完成）**：

1. 完成 original recording artifact writer（FFmpeg feature + Native Safety review）。
2. 完成 playable preset export（三种 preset 产出可播放 output）。
3. 完成 FFmpeg artifact integration tests。
4. 完成 manual FFmpeg gates（1080p 10-minute pressure、A/V sync、original preservation、cancel cleanup）。
5. 完成 Native Safety Gate review。
6. 更新 `tests/phase-6-w11-w12-checklist.md` 反映实际状态。

---

### 2026-05-29：Phase 5 空白段检测与自动裁剪实现

输入文件：

- `docs/superpowers/plans/2026-05-29-phase-5-silence-trimming.md`
- `tests/phase-5-w9-w10-checklist.md`

本轮完成（10 Tasks）：

1. `core/cut.rs` 建立 `CutTimeline` / `CutSegment` / `KeepSegment` / activity sample serde 模型。
2. `media/silence_detector.rs` 实现音频 RMS、低分辨率帧差分、保守候选合并和裁剪缓冲策略。
3. `media/trim_metadata.rs` 建立 trim metadata 与 cut timeline JSON sidecar。
4. `MacRecordingService` 消费线程采集 bounded trim metadata，停止录制后写入 sidecar。
5. Tauri 命令 `build_cut_timeline` 和 `export_video` 接入裁剪时间线边界。
6. Preview UI 接入 auto-trim command path，前端仍不接触音视频帧或 activity stream。

验证结果：

- `cargo fmt --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **149 tests** 通过（+22 相比 Phase 4）
- `cargo clippy --all-targets` 无 error（21 pre-existing SCK FFI warnings）
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **44 tests** 通过（+2 相比 Phase 4）

改动文件：

- **新增**: `src-tauri/src/core/cut.rs`, `src-tauri/src/media/silence_detector.rs`, `src-tauri/src/media/trim_metadata.rs`, `src-tauri/src/media/trim_exporter.rs`
- **修改**: `src-tauri/src/core/mod.rs`, `src-tauri/src/core/processor.rs`, `src-tauri/src/media/mod.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/media/ffmpeg_writer.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/app/error.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/components/preview-view.tsx`, `src/App.tsx`, `src/App.test.tsx`, `tests/phase-5-w9-w10-checklist.md`

剩余待完成（不阻塞 Phase 5 code contract，但需关注）：

- 真实可播放裁剪导出需生产 FFmpeg encoder/muxer 接入后人工验收。
- 长录制 trim metadata / cut timeline 内存峰值压力测试。
- 人工确认原始素材保留与重新导出路径。

---

### 2026-05-27：Phase 4 光标平滑与点击放大实现

输入文件：

- `docs/superpowers/plans/2026-05-27-phase-4-cursor-effects.md`
- `tests/phase-4-w7-w8-checklist.md`

本轮完成（15 Tasks）：

1. `core/timeline.rs` 建立 `CursorSample` / `CursorClick` / `EffectTimeline` serde 模型。
2. `media/cursor_engine.rs` 实现移动平均、贝塞尔插值、点击放大状态机、CursorEffectEngine 组合。
3. `app/cursor_metadata_runtime.rs` 和 `platform/macos/cursor_source.rs` 建立录制期光标元数据采集。
4. `MacRecordingService` 停止录制后写入光标元数据边车文件。
5. Tauri 命令（`set_beautify_config`、`build_cursor_effect_timeline`、`export_video`）和预览页接入光标美化配置与时间线生成。
6. `CaptureConfig.show_system_cursor` 控制 SCK 原始光标绘制（美化开启时隐藏系统光标防双光标）。

验证结果：

- `cargo fmt --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **127 tests** 通过（+51 相比 Phase 3）
- `cargo clippy --all-targets` 无 error（21 pre-existing SCK FFI warnings）
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **42 tests** 通过（+19 相比 Phase 3）

改动文件：

- **新增**: `src-tauri/src/core/timeline.rs`, `src-tauri/src/core/processor.rs`, `src-tauri/src/media/cursor_engine.rs`, `src-tauri/src/media/recording_metadata.rs`, `src-tauri/src/app/cursor_metadata_runtime.rs`, `src-tauri/src/platform/macos/cursor_source.rs`
- **修改**: `src-tauri/src/core/frame.rs`, `src-tauri/src/core/mod.rs`, `src-tauri/src/core/config.rs`, `src-tauri/src/media/mod.rs`, `src-tauri/src/media/recording_writer.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/error.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/platform/macos/mod.rs`, `src-tauri/src/platform/macos/screen_capture_kit.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`, `src/components/preview-view.tsx`, `src/App.tsx`, `src/App.test.tsx`, `tests/phase-4-w7-w8-checklist.md`

剩余待完成（不阻塞合并判断但需关注）：

- `npm run tauri dev` 手动验证光标元数据、点击采集、时间线 JSON 和双光标策略。
- `platform/macos/cursor_source.rs` CoreGraphics FFI Native Safety Gate 人工逐行审查。
- Phase 6 接入生产 FFmpeg compositor 后做真实视频光标效果人工验收。

---

### 2026-05-27：Phase 3 Round 3 整改（复审后的剩余问题修复）

输入文件：

- `docs/superpowers/plans/2026-05-26-phase-3-code-review-remediation.md` Section 11

本轮修复（5 个 Phase）：

1. **R3-B**：`MicLevelRuntime` 旧 runtime 清理提升到 `mic_enabled` 分支之前，确保新 session 开始前无条件清理
2. **R3-C**：修复 `audio_clock_with_session_offset_starts_at_elapsed_time` flaky 测试（增加 1ms sleep）
3. **R3-A**：录制态麦克风电平可见闭环 — `RecordingStatusBar` 新增 `micEnabled`/`micVolume` props 和 5 段电平指示条，补充 `data-level` 测试
4. **R3-D**：文档与 checklist 状态同步
5. **R3-E**：`PreviewView` 导出卡片移除 `motion.div whileTap`，改用 CSS `active:scale-[0.98]`

验证结果：

- `cargo fmt --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` **76 tests** 通过（flaky 已修复）
- `cargo clippy --all-targets` 无 error（21 pre-existing warnings）
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **23 tests** 通过

改动文件：

- **修改**: `src-tauri/src/lib.rs`, `src-tauri/src/core/clock.rs`, `src/components/recording-status-bar.tsx`, `src/App.tsx`, `src/App.test.tsx`, `src/components/preview-view.tsx`, `tests/phase-3-code-review-remediation-checklist.md`, `tests/phase-3-w5-w6-checklist.md`, `HANDOFF.md`

剩余待完成（不阻塞合并判断但需关注）：

- `npm run tauri dev` 手动验证（录制态 mic 电平可见性、快速点击稳定性、麦克风断开不崩溃）
- `permissions.rs` FFI Native Safety Gate 人工逐行审查
- 架构红线人工复核
- BUG.md 预防规则人工复核

---

### 2026-05-26：Phase 3 Code Review 首版整改 (Round 2)

输入文件：

- `docs/superpowers/plans/2026-05-26-phase-3-code-review-remediation.md`
- `tests/phase-3-code-review-remediation-checklist.md`

Round 2 完成项（7 个 Phase）：

1. **Phase A**：前端开始录制防重入 — `App.tsx` 增加 `isStartingRef` 守卫
2. **Phase B**：前端停止录制防重入 — `App.tsx` 增加 `isStoppingRef` 守卫
3. **Phase C**：`MicLevelRuntime` 生命周期管理 — 新建 `app/mic_level_runtime.rs`，实现 stop/join/Drop
4. **Phase D**：麦克风关闭与电平重置 — `capture_microphone == false` 时不启常驻 mic runtime；start/stop 中重置 `mic_level`
5. **Phase E**：权限测试隔离 — 提取 `map_av_authorization_status()`、`map_screen_preflight()` 纯函数
6. **Phase F**：测试补齐 — `MicLevelPayload` 序列化测试 + mic-level 事件测试 + 双击防重入测试
7. **Phase G**：`MicLevelDetector` 测试清理 — 重命名测试、清理注释

Round 2 复审发现的问题（已进入 Round 3 修复）：
- 防重入锁完全依赖事件清理，事件丢失可能造成 UI/Rust 脱节
- 录制态 UI 无 mic-level 可见展示入口
- `start_recording` 旧 `MicLevelRuntime` 清理只在 mic_enabled 分支执行
- Rust 时钟测试 flaky
- HANDOFF/checklist 状态与代码事实不一致

验证结果：

- `cargo fmt --check` 通过
- `cargo test` **76 tests** 通过（1 flaky 需复跑）
- `cargo clippy --all-targets` 无 error
- `cargo build` 通过
- `npm run build` 通过
- `npm test -- --run` **21 tests** 通过（Round 2 结束时实际为 21，HANDOFF 曾误写为 17）

改动文件：

- **新增**: `src-tauri/src/app/mic_level_runtime.rs`
- **修改**: `src/App.tsx`, `src/App.test.tsx`, `src-tauri/src/lib.rs`, `src-tauri/src/app/mod.rs`, `src-tauri/src/platform/macos/permissions.rs`, `src-tauri/src/platform/macos_service.rs`, `src-tauri/src/app/events.rs`, `src-tauri/src/media/mic_level.rs`

---

### 2026-05-26：Phase 3 录制 UI 与前后端联调完成

输入文件：

- `docs/architecture/project-architecture-and-overall-planning.md`
- `docs/superpowers/plans/` (本 Phase 计划在 `.claude/plans/` 中)
- `tests/phase-3-w5-w6-checklist.md`

已完成（9 个 Task 全部完成）：

1. **Task 1**：`CaptureMode` 枚举新增 `Window`/`Area` 变体 + `mode_from_str()` 解析方法
2. **Task 2**：`MacPermissionProbe` 接入真实 macOS 权限 API（`CGPreflightScreenCaptureAccess` + `AVCaptureDevice.authorizationStatusForMediaType:`）
3. **Task 3**：`MicLevelDetector` — 滑动窗口 RMS 电平计算器（`media/mic_level.rs`）
4. **Task 4**：消费线程集成 `MicLevelDetector` + `mic-level` 事件（100ms 间隔）推送
5. **Task 5**：`RecordingPanel` 新增分辨率/FPS 下拉 + 窗口/区域模式"即将推出"提示 + 按钮禁用
6. **Task 6**：`App.tsx` 替换 mic 随机数模拟为真实 `mic-level` 事件 + 状态转换加固 + 权限变更重检
7. **Task 7**：`lib/tauri.ts` 补充 `MicLevelPayload` 类型
8. **Task 8**：测试扩展（69 Rust + 14 前端 测试全部通过）
9. **Task 9**：手动验证清单更新到 `tests/phase-3-w5-w6-checklist.md`

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 69 tests 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error（21 个 FFI naming warning 可接受）
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 14 tests 通过

关键决策：

- **窗口/区域模式前端传参、后端预留**：`CaptureMode` 枚举已扩展，`set_capture_mode` 接受三种模式参数，`start_recording` 对非全屏返回中文错误
- **真实 macOS 权限 API**：`CGPreflightScreenCaptureAccess()` + `AVCaptureDevice.authorizationStatusForMediaType:` 通过 FFI/objc2 调用，状态映射 0→NotDetermined, 1→Denied, 2→Denied, 3→Granted
- **MicLevelDetector 100ms 推送**：独立线程每 100ms 从 `Arc<Mutex<f64>>` 读取 RMS 值并通过 Tauri event 推送，替换前端 setInterval 模拟
- **权限变更重检**：`handleBackToIdle` 中重新调用 `fetchRecordingPermissions()`
- **状态转换加固**：所有 handler 增加当前状态检查守卫（`appState !== 'idle'` 直接 return）

后续入口：

1. `npm run tauri dev` 执行 Phase 3 手动验证清单（7 项）
2. 人工审查 `permissions.rs` 中的 FFI 调用（Native Safety Gate）
3. 进入 Phase 4（W7-W8）光标平滑与点击放大

---

### 2026-05-25：Phase 1/2 录制流水线整改计划执行完成

输入文件：

- `docs/superpowers/plans/2026-05-25-phase-1-2-recording-pipeline-remediation.md`
- `tests/phase-1-2-remediation-checklist.md`

已完成（15 个 Task 全部完成）：

1. **Task 1**：修复 CoreMedia 音频格式符号 `CMAudioFormatDescriptionGetStreamBasicDescription`
2. **Task 2**：录制回调改为有界非阻塞媒体队列 `MediaSender`/`MediaReceiver`
3. **Task 3**：ScreenCaptureKit 和 cpal 时间戳归一化（`TimestampNormalizer` + `AudioSampleClock`）
4. **Task 4**：音频缓冲解析增加 PCM 格式判断和动态 `AudioBufferList` 分配
5. **Task 5**：Tauri 录制命令迁移到 `spawn_blocking`，`TickRuntime` 可取消
6. **Task 6**：`stopCaptureWithCompletionHandler` 超时返回 `CaptureStopTimeout` 错误
7. **Task 7**：`AudioSynchronizer` 封装 `SimpleAudioMixer`，接入录制链路
8. **Task 8**：`RecordingWriter` trait 抽象 + `CountingRecordingWriter` 测试实现
9. **Task 9**：FFmpeg 生产写入器骨架（feature-gated `ffmpeg-next`）
10. **Task 10**：前端/后端命令参数对齐（`setCaptureMode`/`setAudioConfig` payload）
11. **Task 11**：权限探测改用 `PermissionService` + `MacPermissionProbe`
12. **Task 12**：移除区域级 `data-tauri-drag-region={false}` 标记
13. **Task 13**：Windows stub 编译边界修复（`CaptureConfig` 导入路径 + `compile_error!`）
14. **Task 14**：自动化验证矩阵（fmt/clippy/test/build）
15. **Task 15**：手动验收清单 + HANDOFF 更新

验证结果：

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过
- `cargo test --manifest-path src-tauri/Cargo.toml` 43 tests 通过
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` 无 error（FFI 命名 warning 可接受）
- `cargo build --manifest-path src-tauri/Cargo.toml` 通过
- `npm run build` 通过
- `npm test -- --run` 6 tests 通过

关键决策：

- **有界非阻塞队列**：`try_send_drop_newest` 策略，队列满时丢弃最新帧，避免捕获回调阻塞
- **会话相对时间戳**：第一个时间戳归零，解决跨源时间戳不一致问题
- **动态 AudioBufferList 分配**：两步查询模式，避免固定大小缓冲区溢出
- **FFmpeg feature gate**：`ffmpeg-next` 通过 Cargo feature 控制，开发环境可不安装

后续入口：

1. 进入 Phase 3 前，人工复审 ScreenCaptureKit/cpal unsafe 和资源释放路径
2. Windows DXGI/WASAPI 在 Windows 测试机上执行可行性验证
3. FFmpeg 写入器完整实现（编码 + 封装）

---

### 2026-05-24：端到端录制流水线连接

输入文件：

- `.claude/plans/docs-architecture-project-architecture-jaunty-horizon.md`

**根因：** `npm run tauri dev` 验证发现数据流在 5 层全部断裂：前端未调用 Tauri 命令、Tauri 命令仅切换状态机未调用录制服务、AppState 不持有 RecordingService、帧无消费者、预览界面为占位文字。

已完成（7 个 Task 全部完成）：

1. **Task 1**：`MacRecordingService` 具体结构体（`platform/macos_service.rs`），封装 `MacScreenCapture` + `CpalMicrophoneCapture` + 状态机 + 混音器 + drain 线程
2. **Task 2**：`AppState` 重构为持有 `Mutex<MacRecordingService>`（替代裸状态机）
3. **Task 3**：所有 Tauri 命令更新为调用真实 `MacRecordingService`（start/stop/pause/resume）
4. **Task 4**：帧消费线程（drain 模式，防止 channel 阻塞捕获线程）+ `recording-tick` 事件发射
5. **Task 5**：前端 `App.tsx` 替换 TODO 注释为真实 Tauri invoke 调用 + 事件监听
6. **Task 6**：`stop_recording` 返回 `RecordingResult`，`PreviewView` 显示录制统计
7. **Task 7**：验证（fmt/clippy/test/build 全通过）

验证结果：

- `cargo fmt --check` 通过
- `cargo clippy` 无错误
- `cargo test` 34/34 通过
- `cargo build --lib` 通过
- `npm run build` 通过
- `npm run test` 4/4 通过

关键决策：

- **`start_combined()` 方法**：`MacScreenCapture` 新增 `start_combined(config, video_sink, audio_sink)` 绕过 trait 方法歧义，原子化设置两个 sink
- **Drain 模式帧消费**：后台线程以 10ms 间隔 `try_recv()` 消费帧，计数器记录帧数，不做编码
- **`recording-tick` 线程**：独立线程每秒 emit 事件到前端，驱动计时器显示
- **`RecordingResult`**：`stop_recording` 返回帧数和时长，`output_path: None`（FFmpeg 编码未实现）

已知问题：

- **CoreMedia 链接错误**：`CMFormatDescriptionGetStreamBasicDescription` 符号在当前系统上无法解析（CoreMedia.framework 符号链接损坏），导致 `cargo build --bin luzhi` 失败。`cargo build --lib` 和 `cargo test` 正常。需修复系统 CoreMedia 框架或改用替代 API。

后续入口：

1. 修复 CoreMedia 链接问题后，`npm run tauri dev` 端到端验证
2. 进入 FFmpeg 集成实现视频编码和文件写入
3. 实现 `recording-tick` 精确计时（基于 SCStream 时间戳）

---

### 2026-05-24：Phase 2 完成（W3-W4 双音频链路）

输入文件：

- `docs/superpowers/plans/2026-05-24-phase-2-windows-capture-research-dual-audio.md`
- `tests/phase-2-w3-w4-checklist.md`

已完成（10 个 Task 全部完成）：

1. **Task 1**：`AudioChunk`、`MixedAudioChunk` 数据类型 + `AudioCapture` Trait + `AudioConfig`、`AudioDevice`、`AudioCapabilities`
2. **Task 2**：`AppError` 新增 `AudioCaptureFailed`、`AudioDeviceNotFound`、`AudioMixFailed` 三个变体
3. **Task 3**：`MacScreenCapture` 真实实现（SCStream 音视频统一捕获，`define_class!` 宏 + FFI 绑定）
4. **Task 4**：`CpalMicrophoneCapture` 实现（cpal 麦克风采集，`SendStream` 解决 `!Send` 问题）
5. **Task 5**：`SimpleAudioMixer` 实现（线性插值重采样 + 时间戳对齐 + 等权混音 + 硬限幅）
6. **Task 6**：`RecordingService<C: ScreenCapture, A: AudioCapture>` 双泛型重构，含 rollback 逻辑
7. **Task 7**：Windows `DxgiCapture` + `WasapiLoopback` Trait stub 骨架（`#[cfg(target_os = "windows")]`）
8. **Task 8**：Tauri 命令扩展（start/stop/pause/resume/set_capture_mode/set_audio_config）+ 状态机 Paused 状态 + `recording-state-changed` 事件
9. **Task 9**：音频单元测试补充（34 个测试全部通过）
10. **Task 10**：Phase 2 验证与交接

验证结果：

- `cargo fmt --check` 通过
- `cargo clippy` 无错误（FFI 相关 warning 可接受）
- `cargo test` 34/34 通过
- `npm run build` 通过
- `npm run test` 4/4 通过

关键决策：

- **SCStream 统一音视频**：ScreenCaptureKit 的 SCStream 同时输出视频帧和系统音频，无需分离
- **SendSCStream wrapper**：objc2 的 SCStream 是 `!Send`，用 unsafe newtype 解决（基于原子引用计数）
- **SendStream wrapper**：cpal 的 Stream 也是 `!Send`，同样用 unsafe newtype 解决
- **`objc2::define_class!` 宏**：必须使用完整路径调用，不能通过 `use` 导入
- **双泛型 RecordingService**：`<C: ScreenCapture, A: AudioCapture>` 分离视频捕获和麦克风捕获
- **实时混音**：录制期间实时混合系统音频和麦克风（非录后混音）

后续入口：

1. `npm run tauri dev` 验证端到端音视频录制
2. 进入 Phase 3（W5-W6）录制 UI 与前后端联调
3. ScreenCaptureKit FFI 代码需人工逐行审查内存安全

---

### 2026-05-24：BUG-001 & BUG-002 深度修复

输入文件：

- `BUG.md`（原记录 + 补充更新）
- `src-tauri/Cargo.toml`（添加 macos-private-api）
- `src-tauri/tauri.conf.json`（启用 macOSPrivateApi + acceptFirstMouse）
- `src-tauri/capabilities/default.json`（添加拖拽权限）
- `src/App.tsx`（程序化拖拽 handler + 拖拽区域标记）
- `src/components/`（全部 5 个视图组件的拖拽区域配置）
- `src/styles.css`（html 透明 + body 去背景）
- `.cargo/registry/.../tauri-2.11.2/src/window/scripts/drag.js`（Tauri 源码审查）

已完成：

- BUG-001 修复（4 轮迭代）：
  - CSS 层：`styles.css` body 移除 `bg-background`，html 添加 `background-color: transparent`
  - WKWebView 层：`Cargo.toml` 启用 `macos-private-api` feature，`tauri.conf.json` 启用 `macOSPrivateApi: true`
  - 视觉伪影：移除 RecordingPanel/StatusBar 的 `shadow-2xl shadow-black/30`、`backdrop-blur-xl`，移除 idle 装饰渐变 div
- BUG-002 修复（4 轮迭代）：
  - ACL 权限：`capabilities/default.json` 添加 `core:window:allow-start-dragging`
  - macOS 首次点击：`tauri.conf.json` 添加 `acceptFirstMouse: true`
  - 拖拽区域：全部 5 个视图（idle/recording/preview/processing/error）添加 `data-tauri-drag-region="deep"`
  - 程序化拖拽：`App.tsx` 自定义 `mousedown` handler，调用 `getCurrentWindow().startDragging()`，绕过 drag.js 的 macOS 焦点窗口限制
  - 交互元素排除：handler 用 `closest('button,input,select,textarea,a,[role=...],[contenteditable],[tabindex]')` 精确排除
- BUG-003 补充修复：`recording-status-bar.tsx` pause/stop 按钮移除 `motion.div whileTap` 包裹
- `BUG.md` 更新：BUG-001/002 完整根因分析 + 预防规则，新增延期-001（点击穿透）
- Tauri 源码审查：drag.js 注入机制、`isDragRegion` 逻辑、macOS `performWindowDragWithEvent:` 调用链

验证结果：

- `npm run build` 通过
- `npm run test` 通过（4/4）
- `cargo build` 通过（clean）

关键决策：

- **放弃声明式 `data-tauri-drag-region`，采用程序化 `startDragging()` API**：声明式机制在 macOS 上受焦点窗口限制（tauri#11605），对浮动面板 app 不可靠
- **不检查 `{false}` 值**：交互元素选择器已足够，`{false}` 排除反会阻止面板内容区拖拽
- **动态 `import('@tauri-apps/api/window')`**：避免测试环境顶层导入报错

后续入口：

1. `npm run tauri dev` 验证所有视图拖拽和透明效果
2. 进入 Phase 3 (W5-W6) 前后端联调

---

### 2026-05-24：UI 移植（v0 → Tauri）

输入文件：

- `reference/ui/ui_spec.md`
- `docs/architecture/project-architecture-and-overall-planning.md`
- `_v0_reference/`（Next.js 设计稿）

已完成：

- 依赖安装：class-variance-authority, clsx, tailwind-merge, lucide-react, framer-motion, @radix-ui/react-switch/slider/select/slot
- shadcn/ui 配置：components.json, 路径别名 `@/*` → `src/*`
- Raycast 主题：CSS 自定义属性 + Tailwind v4 `@theme inline` 映射
- shadcn/ui 组件：Button, Switch, Slider, Select
- 业务组件：RecordingPanel, RecordingStatusBar, PreviewView, ProcessingView, ErrorView
- Tauri invoke 封装：完整 Commands/Events 类型定义和函数
- App.tsx 五态路由：idle/recording/preview/processing/failed
- 前端测试：4 个测试覆盖核心交互
- BUG 修复：3 个（透明窗口、拖拽区域、按钮点击）
- 移植规范文档：`reference/ui/ui-migration-spec.md`

验证结果：

- `npm run build` 通过（JS 464 KB, CSS 115 KB）
- `npm run test` 通过（4/4）
- `cargo test` 通过（14/14）

后续入口：

1. `npm run tauri dev` 验证桌面端效果
2. 将 console.log 占位替换为真实 Tauri invoke
3. 进入 Phase 3 (W5-W6) 前后端联调

---

### 2026-05-23：Phase 1 实施计划

已生成实施计划：

- `docs/superpowers/plans/2026-05-23-phase-1-macos-recording-foundation.md`

执行范围：

- 仅覆盖 Phase 1 / W1-W2：脚手架与 macOS 录制闭环。
- 不覆盖 Windows、音频混音、光标美化、空白裁剪、导出预设和授权系统。

执行要求：

1. 按任务顺序执行，每个任务完成后提交一次。
2. 执行前确认 `src-tauri/Cargo.toml` 依赖版本经过人工审查。
3. ScreenCaptureKit 原生回调、线程切换、buffer 生命周期和 unsafe/FFI 代码必须人工逐行审查。
4. Phase 1 完成后逐项执行 `tests/phase-1-w1-w2-checklist.md`。

---

## 冬眠记录

### 2026-06-04：BUG-0011 方案B 实现完成、准备尝试方案A 冬眠

#### 1. 当前任务上下文

BUG-0011 光标类型始终为 Arrow。已尝试方案B（增强 AX 分类），包括前台应用 AX 查询，但 WebView 内容对 AX hit-test 不透明。准备尝试方案A（NSCursor API）。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- 方案B 实现：添加 `AXUIElementCreateApplication` + `get_frontmost_app_pid()` + 前台应用优先查询
- 测试结论：`src=front` 查询成功，但 `role_chain` 始终返回 `AXMenuBar`/`AXGroup`，无法穿透 WebView
- 诊断日志显示 `arrow=480 hand=0 ibeam=0`，确认 AX API 对 WebView 内容无效
- Git 已提交（`2a6f814`），未 push

#### 3. 中断时的处置决策

方案B 代码已提交，状态稳定。决定尝试方案A（NSCursor API），冬眠保存现场。

#### 4. 架构与关键决策

- **AX API 无法穿透 WebView**：`AXUIElementCopyElementAtPosition` 无论系统级还是应用级查询，都无法获取 WebView 内部 DOM 元素的 AX role
- **前台应用 PID 方案**：通过 `NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier` 获取 PID，再用 `AXUIElementCreateApplication(pid)` 查询。技术上可行，但 WebView 内容不暴露交互元素
- **方案A（NSCursor API）**：下一步调研 `[NSCursor currentCursor]` 在后台线程查询实时光标类型的可行性

#### 5. 立即执行清单

1. 调研 `[NSCursor currentCursor]` API 的线程安全性和实时性
2. 如果可行，实现 NSCursor-based CursorKindProvider
3. 验证：在 Tauri 应用中悬停链接/按钮/输入框，检查光标类型是否正确切换
4. 验证完成后移除 diagnostic logging

#### 6. 当前报错/阻碍

无编译报错。核心阻碍是 AX API 对 WebView 内容的固有限制。

---

### 2026-06-03：BUG-0010/0011/0012 实施计划编写完成冬眠

#### 1. 当前任务上下文

完成 BUG-0010（光标偏移）、BUG-0011（光标不好看）、BUG-0012（导出进度不优雅）三条未解决 bug 的根因分析和详细实施计划编写。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- 完成三条 bug 的静态链路审查和根因定位（无代码修改）
- 编写 16 个 Task 的详细实施计划，保存至 `docs/superpowers/plans/2026-06-03-bug-0010-0011-0012-fix-plan.md`
- 计划包含完整代码、测试、验证命令、commit 消息
- 更新 BUG.md 新增 BUG-0010/0011/0012 未解决条目
- Git 已提交（`556ad79`），未 push

#### 3. 中断时的处置决策

休眠触发时，计划编写刚完成，未开始任何代码修改。选择"直接冬眠"：无需回滚或额外收尾，状态完全稳定。

#### 4. 架构与关键决策

- **坐标归一化在 recorder 层完成**：`CursorMetadataRuntime` 接收 `CaptureGeometry`，`CursorMetadataRecorder` 内部持有 `CursorCoordinateMapper`，在 `record_snapshot()` 时将全局屏幕坐标归一化为源视频像素坐标。`CursorOverlayRenderer` 不变。
- **CursorKind 使用 `#[serde(default)]`**：旧 timeline JSON 没有 `kind` 字段时默认为 `Arrow`，保证向后兼容。
- **glyph 渲染使用静态 bitmask**：不引入图片解码依赖，arrow/hand/ibeam 的像素数据编译为 Rust 静态数组。
- **导出进度分段**：准备阶段占 0-10%，FFmpeg 导出占 10-99%，validation 成功后报 100%。

#### 5. 立即执行清单

1. 用户审阅计划，选择执行方式（Subagent-Driven 或 Inline）
2. 按 Phase 1 → Phase 2 → Phase 3 顺序执行 16 个 Task
3. Phase 1 Task 3 完成后，在真实设备上验证 `SCDisplay.frame()` 返回值

#### 6. 当前报错/阻碍

无阻塞报错。

待确认项：

- SCDisplay 的 `pointPixelScale` 不在 `SCDisplay` 上，需确认获取方式
- `CGEventGetLocation()` Y 轴方向需实测
- 手形和 I-beam 的 glyph 像素数据需设计

---

### 2026-05-24：端到端流水线连接完成冬眠

#### 1. 当前任务上下文

修复 `npm run tauri dev` 端到端验证发现的数据流断裂问题（5 层全部断裂）。当前在 `feat/architecture-planning` 分支。

#### 2. 已完成进度

- `MacRecordingService` 具体结构体（`platform/macos_service.rs`），封装 SCStream + cpal + 状态机 + drain 线程
- `MacScreenCapture::start_combined()` 原子化设置 video + audio sink
- `AppState` 重构为 `Mutex<MacRecordingService>`（替代裸状态机）
- 所有 6 个 Tauri 命令连接到真实服务 + `recording-tick` 事件发射
- 前端 `App.tsx` 替换 TODO 为真实 invoke + 事件监听
- `PreviewView` 接收 `RecordingResult` 显示录制统计
- 全部验证通过：`cargo fmt`、`cargo clippy`、`cargo test` 34/34、`npm run build`、`npm run test` 4/4

#### 3. 中断时的处置决策

休眠触发时，所有 7 个 Task 已完成，代码处于稳定态。用户要求将 CoreMedia 链接错误记录到"已知问题阻塞点"后执行冬眠。选择"直接冬眠"：无需回滚或额外收尾。

#### 4. 架构与关键决策

- **`MacRecordingService` 替代泛型 `RecordingService<C, A>`**：macOS 的 SCStream 是统一流，需要同一个 `MacScreenCapture` 对象同时处理视频和系统音频，泛型设计无法满足
- **`start_combined()` 方法**：绕过 `ScreenCapture`/`AudioCapture` trait 方法歧义（两个 trait 都有 `start()`/`stop()`），原子化设置两个 sink
- **Drain 模式帧消费**：后台线程 10ms 间隔 `try_recv()` 消费帧，防止 channel 阻塞 SCStream 回调线程。不做编码
- **`recording-tick` 线程**：独立线程每秒 emit 事件到前端，驱动计时器

#### 5. 立即执行清单

1. 修复 CoreMedia 链接错误（见"已知问题阻塞点"第 1 项的 3 个方案）
2. `npm run tauri dev` 端到端验证完整录制流程
3. 进入 FFmpeg 集成实现视频编码和文件写入

#### 6. 当前报错/阻碍

**CoreMedia 链接错误**（阻塞 `npm run tauri dev`）：

```
Undefined symbols for architecture arm64:
  "_CMFormatDescriptionGetStreamBasicDescription", referenced from:
    luzhi_lib::platform::macos::screen_capture_kit::cmformat_description_get_stream_basic_description
ld: symbol(s) not found for architecture arm64
```

根因：`/System/Library/Frameworks/CoreMedia.framework/CoreMedia` 是断开的符号链接。详见"已知问题阻塞点"第 1 项。

---

### 2026-05-24：BUG-001 & BUG-002 修复完成冬眠

#### 1. 当前任务上下文

修复 Tauri 2.0 浮动面板窗口的 BUG-001（800x600 不透明背景）和 BUG-002（窗口无法拖动）。当前在 `feat/architecture-planning` 分支，最新提交 `ecbdb69`。

#### 2. 已完成进度

- BUG-001 三层透明架构修复（NSWindow + WKWebView + CSS）
- BUG-002 程序化拖拽方案（自定义 mousedown handler + `getCurrentWindow().startDragging()`）
- 5 个视图全部支持拖拽（idle/recording/preview/processing/error）
- 交互元素正确排除（按钮、滑块、开关等不触发拖拽）
- CSS 阴影伪影清除
- BUG.md 预防规则完整更新
- 全部验证通过：`npm run build`、`npm run test` 4/4、`cargo build`

#### 3. 中断时的处置决策

休眠触发时，用户已验证拖拽功能正常，所有修改处于稳定完成态。选择"快速收尾"：提交现场并更新 HANDOFF.md。

#### 4. 架构与关键决策

- **程序化拖拽 > 声明式拖拽**：Tauri 2 的 `data-tauri-drag-region` 在 macOS 上受焦点窗口限制，不可靠。自定义 mousedown handler + `startDragging()` API 是更健壮的方案
- **交互元素白名单 > `{false}` 排除**：用 `closest('button,input,select,...')` 精确排除交互元素，不做区域级 `{false}` 排除
- **三层透明**：NSWindow (`transparent: true`) + WKWebView (`macos-private-api`) + CSS (html/body transparent) 缺一不可
- **ACL 权限是硬门控**：`core:window:allow-start-dragging` 不在 `core:default` 内，必须显式授予

#### 5. 立即执行清单

1. `npm run tauri dev` 最终验证桌面端效果
2. 将 console.log 占位替换为真实 Tauri invoke
3. 录制计时器和麦克风音量替换为 Tauri event 监听
4. 进入 Phase 3 (W5-W6) 前后端联调

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知待办事项：

- 延期-001：透明区域鼠标点击不穿透（需原生 macOS 代码，记录在 BUG.md）
- PreviewView 和 App.tsx 中有 console.log 占位
- 录制计时器和麦克风音量需替换为 Tauri event 监听
- 权限检测需连接真实系统权限 API

---

### 2026-05-24：UI 移植完成冬眠

#### 1. 当前任务上下文

完成 `_v0_reference/` UI 设计稿到 Tauri 2.0 项目的完整移植。当前在 `feat/architecture-planning` 分支，最新提交 `7f4ee1a`。

#### 2. 已完成进度

- 10 个 Task 全部完成（依赖→主题→组件→封装→业务组件→路由→测试）
- 修复 3 个 BUG（透明窗口背景、拖拽区域缺失、按钮点击拦截）
- 创建 `reference/ui/ui-migration-spec.md` 移植规范文档（389 行）
- 编译通过，4 个前端测试通过

#### 3. 中断时的处置决策

休眠触发时，工作处于稳定完成态：

- 所有 UI 组件已创建并验证
- 最后一个操作是创建移植规范文档并更新索引
- 无进行中的代码修改，无需回滚
- 选择"快速收尾"：提交现场并生成交接文档

#### 4. 架构与关键决策

- 主题选择 Raycast 近黑中性色（`#040506` canvas），非 v0 原始紫色系
- 窗口配置 `decorations: false` + `transparent: true` 实现无边框透明
- `motion.div` 的 `whileTap` 不能作为可交互元素的父容器（会拦截点击）
- 拖拽区域需要 `data-tauri-drag-region`，可交互元素需要 `{false}` 排除
- `<html>` 必须添加 `class="dark"` 才能激活 CSS 主题变量

#### 5. 立即执行清单

1. 运行 `npm run tauri dev` 验证桌面端效果
2. 检查浮动面板是否正确显示（无背景方框）
3. 检查窗口拖拽和按钮点击是否正常
4. 如有问题，参考 `reference/ui/ui-migration-spec.md` 第 7 章排查

#### 6. 当前报错/阻碍

当前无阻塞报错。

已知待办事项：

- PreviewView 和 App.tsx 中有 console.log 占位，需替换为真实 Tauri invoke
- 录制计时器和麦克风音量需替换为 Tauri event 监听
- 权限检测需连接真实系统权限 API

---

### 2026-05-24：Phase 1 worktree 合并与清理

- 将 `feat/phase-1-macos-recording` 合并到 `feat/architecture-planning`。
- 清理 `.worktrees/phase-1-macos-recording` worktree。
- Phase 1 Task 1-10 全部完成，14 Rust tests + 2 frontend tests 通过。
- 下一步：人工审查 ScreenCaptureKit 边界后激活真实实现，进入 Phase 2。
