# git 规范

## 1. 提交信息、分支、PR 口径

### 1.1 提交信息规范

遵循 Conventional Commits：

`<type>(<scope>): <subject>`

- [type]: feat, fix, refactor, perf, chore, docs, test
- [scope]: record, cursor, export, audio, ui, auth, core
- [subject]部分**必须**要用中文书写

示例：

- `feat(record): 为Windows实现DXGI桌面复制`
- `fix(cursor): 修正高频运动中的贝塞尔插值抖动`

### 1.2 分支策略

- `main`：生产稳定分支，仅接受 PR 合入。
- `dev`：开发集成分支。
- `feat/xxx` 或 `fix/xxx`：功能/修复分支，从 `dev` 检出，完成后提合回 `dev`。

### 1.3 PR 口径

- 描述必须包含：变更内容、关联里程碑（如 W5-6）、测试情况。
- AI 生成的 PR 必须在标题前缀标注 `[AI]`，并在描述中说明 AI 生成的核心逻辑及人工验证结果。

### 1.4 行为准则

- **不允许**主动 push、merge。commit 后，可以提醒我进行 push、merge 等重要操作。
- **git push 口径**：代码必须通过本地编译且无 ESLint Error，单测通过后，由人类手动 Push 或合并。

---
