# 录制参数摘要显示设计

> 日期：2026-06-08 | 状态：待审批

## 背景

录制进行中时，虚线框内第二行文字 `此区域表示被录制的屏幕内容` 是纯占位文案，没有实际信息价值。用户希望将其替换为当前录制参数的摘要展示，方便在录制过程中确认当前配置。

## 改动范围

仅修改 `src/App.tsx` 第 504-511 行的虚线框内容区域。不新增组件文件、state、Tauri 命令或依赖。

## 设计

### 布局结构

```
正在录制 全屏

      画面              音频
  1920×1080         系统音频 ✓
  30 fps            麦克风 ✗
```

- **画面组**：分辨率（`{width}×{height}`）、帧率（`{fps} fps`）
- **音频组**：系统音频（✓/✗）、麦克风（✓/✗）
- 两组并排，左对齐，中间用 `gap-8` 分隔
- ✓ 用 `text-green-400`，✗ 用 `text-muted-foreground`

### 数据来源

全部来自 App 组件已有 state，无需新增任何状态或后端调用：

| 显示项   | 数据来源            | 示例值           |
| -------- | ------------------- | ---------------- |
| 分辨率   | `resolution`        | `1920×1080`      |
| 帧率     | `fps`               | `30 fps`         |
| 系统音频 | `systemAudioEnabled` | `✓` / `✗`       |
| 麦克风   | `micEnabled`        | `✓` / `✗`       |

### 视觉风格

- 组标题：`text-[10px] uppercase tracking-wide text-muted-foreground`，与 `RecordingPanel` 分组标题风格一致
- 参数值：`text-xs text-muted-foreground`
- 网格容器：`flex gap-8`
- 状态标记：✓ 为 `text-green-400`，✗ 为 `text-muted-foreground`

### 不做的事情

- 不新增 state、Tauri 命令、组件文件
- 不改变第一行 "正在录制 全屏/窗口/区域" 的显示逻辑
- 不引入新依赖

## 测试

- 前端：更新 `App.test.tsx`，验证录制状态下虚线框内显示分辨率、帧率、音频状态
- 验证 `npm test -- --run` 通过
- 验证 `npm run build` 通过
