# 测试要求

## 1. 测试要求、测试覆盖率、Mock 策略

### 1.1 覆盖率目标

- Rust 核心逻辑（状态机、光标平滑算法、空白段检测算法）： **大于 85%**
- React UI 组件： **大于 60%**

### 1.2 Rust 测试策略

- 单元测试：紧挨业务代码编写 `#[cfg(test)] mod tests` 。
- 算法测试：光标贝塞尔插值、帧差分检测必须包含边界条件测试（如：0帧输入、极高频抖动输入、纯静态画面）。
- Mock 策略：录制引擎（ScreenCaptureKit/DXGI）和 FFmpeg 必须通过 Trait 抽象，测试时使用 Mock 实现， **禁止在测试环境调用真实系统录屏 API** 。

### 1.3 前端测试策略

- 框架：Vitest + React Testing Library。
- Mock 策略：必须 Mock Tauri 的 `invoke` 方法，隔离前后端依赖。
- E2E：暂不强制要求 E2E 测试，但核心录制起停流程需有手动验证 Checklist。

---
