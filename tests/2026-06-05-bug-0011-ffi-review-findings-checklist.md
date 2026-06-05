# BUG-0011 FFI Review Findings 自测清单

> 日期：2026-06-05
> 目标：修复 BUG-0011 第 6 轮 code review findings，重点收紧 NSCursor FFI 的 selector guard、线程边界、autorelease pool 和 C string 类型安全。

## Phase 1：Review Feedback 复核

- [x] 已复核 `HANDOFF.md` 中 BUG-0011 第 6 轮 NSCursor 方案背景。
- [x] 已复核 `BUG.md` 规则 25-29。
- [x] 已复核 `.codex/rules/0-global.md` 到 `.codex/rules/5-docs.md`。
- [x] 已确认不修改 `Cargo.toml` 依赖版本。

## Phase 2：RED 测试

- [x] 新增测试：main-thread reader 必须通过 dispatcher 执行系统 cursor 读取闭包。
- [x] 新增测试：main-thread dispatch 失败时 reader 返回 `None`，由 provider fallback Arrow。
- [x] 已先运行定向测试，并确认新测试在实现前失败。

## Phase 3：FFI 修复

- [x] ObjC selector 参数改为 `&CStr` / `c"..."`，避免非 NUL 结尾 slice 被当 C string 使用。
- [x] `objc_msgSend` 前检查 class/instance method 是否存在。
- [x] `NSCursor.currentSystemCursor` / class cursor / image / hotSpot / TIFFRepresentation 调用失败时 fail closed。
- [x] autorelease pool 改为 runtime `objc_autoreleasePoolPush/Pop`，每次读取都有释放边界。
- [x] legacy AX 的 `NSWorkspace` ObjC 调用同步使用 guarded helper。

## Phase 4：线程边界

- [x] 新增 `CursorMainThreadDispatcher`。
- [x] `MacCursorKindProvider` 构造时必须接收 main-thread dispatcher。
- [x] Tauri 启动录制链路通过 `AppHandle::run_on_main_thread` 执行 AppKit cursor 读取。
- [x] main-thread dispatch 超时/失败时 fallback Arrow，不阻塞捕获主链路。

## Phase 5：验证

- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind`：12 tests 通过。
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib`：294 tests 通过。
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --lib`：通过，剩余 32 个既有 warning。
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --check`：通过。
- [x] `npm test -- --run`：55 tests 通过。
- [x] `git diff --check`：通过。

## 人工复核建议

- [ ] 普通桌面/空白区域：导出美化光标为 Arrow。
- [ ] 链接/按钮悬停：导出美化光标为 Hand。
- [ ] 文本输入框：导出美化光标为 IBeam。
- [ ] Mission Control/桌面调度界面：macOS 显示 Arrow 时导出美化视频也为 Arrow。
