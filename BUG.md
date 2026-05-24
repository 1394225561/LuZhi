# bug 备忘清单

记录已知 bug。**重要**：每条 bug 修复后，都要总结对应的**预防规则**。

## 未解决

（暂无）

---

## 已解决

### BUG-001: 录制面板不是浮动小组件，有800x600背景方框

**现象**：主录制控制面板应为浮动小组件，但显示时有一个800x600的不透明背景方框。

**根因**：App.tsx 的 idle 状态容器使用了 `bg-background`（#040506 不透明色），填满了整个 Tauri 窗口。虽然 tauri.conf.json 配置了 `transparent: true`，但 CSS 背景色覆盖了透明效果。

**修复**：移除 idle 和 recording 状态容器的 `bg-background` class，让 Tauri 窗口的透明配置生效。

**预防规则**：
- Tauri 无边框透明窗口（`decorations: false` + `transparent: true`）下，前端容器不能使用不透明背景色，否则会覆盖窗口透明效果。
- 浮动 UI 组件应只在组件自身设置背景，而非全屏容器。

---

### BUG-002: 窗口无法拖动

**现象**：在 idle 状态下，窗口无法通过鼠标拖动移动。

**根因**：idle 状态的根容器缺少 `data-tauri-drag-region` 属性。只有 recording 状态的头部 div 有此属性。

**修复**：在 idle 状态的根容器添加 `data-tauri-drag-region`，并在内部面板容器添加 `data-tauri-drag-region={false}` 以确保按钮等交互元素不被拖拽区域拦截。

**预防规则**：
- Tauri 无边框窗口的每个状态视图都需要有 `data-tauri-drag-region` 拖拽区域。
- 拖拽区域内的可交互元素（按钮、输入框等）需要设置 `data-tauri-drag-region={false}` 来排除自身，否则点击事件会被拖拽逻辑拦截。

---

### BUG-003: 点击"开始录制"无法切换到录制状态

**现象**：点击"开始录制"按钮后，界面没有切换到录制状态栏（迷你播放器）。

**根因**：Button 被包裹在 `motion.div` 的 `whileTap={{ scale: 0.99 }}` 中。framer-motion 的 whileTap 会捕获指针事件来实现缩放动画，阻止了点击事件到达内部的 Button 元素。

**修复**：移除 Button 外层的 `motion.div` 包裹，改用 CSS `active:scale-[0.99]` 实现按压反馈效果。

**预防规则**：
- framer-motion 的 `whileTap` 会拦截指针事件，不要将其作为可交互元素（Button、Link 等）的直接父容器。
- 如需在可交互元素上添加按压动画，优先使用 CSS `active:` 伪类，或将 `whileTap` 直接放在元素本身上（如 `motion.button`）。

---
