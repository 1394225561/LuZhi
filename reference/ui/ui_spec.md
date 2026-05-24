# 界面编码准则

你现在是 LuZhi 项目的首席前端架构师。LuZhi 是一个基于 Tauri 2.0 + Vite + React + TS + Tailwind 的跨平台桌面应用。

我已经将 UI 设计师生成的参考代码放在了 `_v0_reference/` 目录下。这是一个基于 Next.js App Router 的完整项目，包含了我们所需的所有 UI 组件和样式。

**你的任务不是运行这个 Next.js 项目，而是将其中的 UI 代码“翻译”并“移植”到我们的 Tauri (Vite) 项目中。**

是否已完成移植改造的规划？
**移植计划与规范**：[`文件索引地址`]

**重要**：如果上述移植计划与规范对于项目功能的涵盖不完整，请**严格**参考移植计划与规范的 UI 风格，**辅助**参考 "DESIGN.md" 风格，进行前端界面的补充编码。
**重要**：应用界面上的文字必须为中文。

---

请按照以下步骤执行：

### 第一步：分析参考项目

1. 查看 `_v0_reference/app/` 目录，了解页面的整体结构。
2. 查看 `_v0_reference/components/` 目录，找出核心 UI 组件（如录制控制面板、预览播放器、AI美化侧边栏、导出面板等）。
3. 查看 `_v0_reference/lib/utils.ts` 和 `hooks/`，了解使用的工具函数。
4. 查看 `_v0_reference/components/ui/`，确认使用了哪些 shadcn/ui 组件。

### 第二步：环境准备与依赖安装

1. 检查我们 Tauri 项目 (根目录的 package.json) 是否已安装 shadcn/ui 所需的依赖（如 tailwindcss, class-variance-authority, clsx, tailwind-merge, lucide-react 等），如果没有，请安装它们。
2. 将 `_v0_reference/lib/utils.ts` 中的 `cn` 函数复制到我们项目的 `src/lib/utils.ts` 中。
3. 检查 `_v0_reference/components/ui/` 中用到的 shadcn 组件（如 Button, Slider, Switch, Card 等），使用 `npx shadcn-ui@latest add [component]` 命令在我们的项目中初始化这些基础组件。**不要直接复制 v0 的 ui 组件代码**，以免版本冲突，用 shadcn 官方方式安装即可。

### 第三步：代码移植与重构（关键步骤）

这是最重要的环节，将 Next.js 代码转化为 Vite + Tauri 桌面端代码。请注意以下翻译规则：

1. **路由转换**：忽略 Next.js 的 `app/page.tsx` 路由逻辑。将页面内容提取到我们 Tauri 项目的 `src/App.tsx` 中，通过 React 的状态变量（如 `recording`, `previewing`）来切换不同的视图状态。
2. **去除 Next.js 特性**：
   - 删除所有 `"use client"` 指令。
   - 删除所有 `next/image` 引用，改用标准的 `<img>` 标签。
   - 删除所有 `next/link` 引用，改用 `<a>` 或 `<button>`。
3. **组件提取**：将 `_v0_reference/components/` 中的业务组件（非 ui 基础组件）复制到我们的 `src/components/` 目录下。重新调整内部的 import 路径，确保它们指向我们项目的 `@/components/ui/` 和 `@/lib/utils`。
4. **桌面端适配**：
   - 为顶部拖拽区域添加 `data-tauri-drag-region` 属性。
   - 确保窗口大小符合桌面端逻辑（如悬浮小窗、固定比例的预览窗口）。
   - 去除网页端默认的选中、右键菜单行为。
5. **数据解耦**：将组件内写死的 Mock 数据提取出来，通过 Props 传递或使用 React State 管理，为后续调用 Tauri Rust 后端预留接口。
6. **数据与视图分离**：UI 组件只负责渲染和交互状态，不要写死逻辑。所有状态（如录制时间、光标放大倍数）通过 React state 管理。未来需要调用 Tauri 的 Rust 后端（通过 `@tauri-apps/api` 的 invoke），请现在就预留好异步调用的接口函数（可以先 console.log 模拟）。

### 严格边界限制（来自 PRD）

在移植过程中，严禁引入以下 MVP 阶段砍掉的功能 UI：

- 背景模糊/纯色替换
- 自动缩放关键区域
- 字幕编辑
- 模板系统

---

请先执行第一步，分析 `_v0_reference/` 的结构，告诉我你找到了哪些核心业务组件，以及你准备如何拆分和移植它们。不要急于修改代码，先输出你的移植计划。
将最终完整的移植计划保存到 `reference/ui/` 目录下，并索引到当前文件的**移植计划与规范**部分，后续所有的界面编码严格遵守该移植计划与规范。
最终实现一个完整的录制前 -> 录制中 -> 预览美化的状态切换路由（不需要 react-router，用状态机切换 View 即可）。

---
