# 代码风格

## 1. 代码风格、命名、格式

### 1.1 通用规范

- 格式化工具优先：Rust 使用 `rustfmt` + `clippy` ，前端使用 `Prettier` + `ESLint` 。
- 缩进：Rust 4 空格，TS/React 2 空格。
- 禁止提交任何 `TODO` 、 `FIXME` 以外的临时代码或被注释的无用代码块。

### 1.2 Rust 规范（后端/核心逻辑）

- 命名：类型/结构体 `UpperCamelCase` ，函数/变量 `snake_case` ，常量 `SCREAMING_SNAKE_CASE` 。
- 错误处理： **禁止随意 `unwrap()` 或 `expect()`** ，必须使用 `Result<T, E>` 传递错误，应用级统一错误枚举。
- 所有权：严格遵循所有权机制，避免不必要的 `clone()` ，录制管线中帧数据传递必须使用 `Arc` + 引用计数。

### 1.3 TypeScript/React 规范（前端 UI）

- 命名：组件 `PascalCase` ，工具函数/钩子 `camelCase` ，自定义钩子 `useXxx` 。
- 组件范式：优先函数式组件 + Hooks，禁止使用 Class 组件。
- 样式：统一使用 Tailwind CSS，禁止内联样式 `style={{}}` ，禁止创建独立 CSS 文件（全局 tailwind.css 除外）。
- 状态管理：UI 状态使用 `zustand` 或 React Context， **禁止将录制状态机等核心逻辑放在前端** ，前端仅作为展示和指令发送方。

---
