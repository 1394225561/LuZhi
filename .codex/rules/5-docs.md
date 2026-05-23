# 项目辅助

## 1. 文档结构、注释口径

### 1.1 项目文档结构

```text
/docs
  /PRD                - 项目产品需求文档
  /architecture      - 架构设计（三层架构图、数据流）
  /api                - Tauri Command 接口文档
  /platform-diff      - macOS/Windows 差异化记录
README.md             - 项目启动、构建、打包说明
```

### 1.2 注释口径

- **自解释优先** ：良好的命名胜过注释，禁止无意义注释（如 `// set count to 0` ）。
- **算法必注释** ：贝塞尔曲线计算、帧差分算法、音频 RMS 阈值计算等核心数学逻辑， **必须** 附带思路解释或参考链接。
- **平台差异必注释** ：在封装 `ScreenCaptureKit` 和 `DXGI` 的适配层代码处， **必须** 注释说明该平台特有的行为或限制。
- **公共接口必注释** ：Rust 的 `pub` 函数、Trait 定义，TS 的 Props 接口，必须编写标准 Doc Comments。

---
