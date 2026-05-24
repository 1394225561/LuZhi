# UI 移植实施计划：_v0_reference → Tauri 项目

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `_v0_reference/` 中的 UI 设计稿移植到 Tauri 桌面项目，采用 Raycast 近黑中性色主题，保留 framer-motion 动画，实现录制前→录制中→预览美化的三态路由。

**Architecture:** v0 组件的功能结构为骨架，DESIGN.md 的 Raycast 风格为视觉规范，架构文档的数据流红线为接口约束。前端只负责展示与交互，录制状态由 Rust 后端驱动。

**Tech Stack:** React 19 + TypeScript + Tailwind CSS 4 + shadcn/ui (new-york) + framer-motion + lucide-react + Tauri 2.0

---

## 文件结构

```
src/
  App.tsx                          -- 修改：三态路由 + Tauri invoke 集成
  App.test.tsx                     -- 修改：适配新组件结构
  styles.css                       -- 修改：Raycast 主题 tokens
  lib/
    utils.ts                       -- 新建：cn() 工具函数
    tauri.ts                       -- 修改：扩展 invoke 封装
  components/
    ui/
      button.tsx                   -- 新建：shadcn Button
      switch.tsx                   -- 新建：shadcn Switch
      slider.tsx                   -- 新建：shadcn Slider
      select.tsx                   -- 新建：shadcn Select
    recording-panel.tsx            -- 新建：录制配置面板
    recording-status-bar.tsx       -- 新建：录制状态栏
    preview-view.tsx               -- 新建：预览美化视图
    processing-view.tsx            -- 新建：处理中视图（补充 v0 缺失）
    error-view.tsx                 -- 新建：错误状态视图（补充 v0 缺失）
```

---

## Task 1: 安装依赖与配置 shadcn/ui

**Files:**
- Modify: `package.json`
- Create: `components.json`

- [ ] **Step 1: 安装 UI 依赖**

```bash
cd /Users/root-mac/workspace_github/LuZhi
npm install class-variance-authority clsx tailwind-merge lucide-react framer-motion @radix-ui/react-switch @radix-ui/react-slider @radix-ui/react-select @radix-ui/react-slot
```

- [ ] **Step 2: 验证依赖安装**

```bash
npm ls class-variance-authority clsx tailwind-merge lucide-react framer-motion @radix-ui/react-switch @radix-ui/react-slider @radix-ui/react-select
```

Expected: 所有包显示已安装，无 peer dependency 警告。

- [ ] **Step 3: 创建 shadcn/ui 配置文件**

创建 `components.json`：

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "new-york",
  "rsc": false,
  "tsx": true,
  "tailwind": {
    "config": "",
    "css": "src/styles.css",
    "baseColor": "neutral",
    "cssVariables": true,
    "prefix": ""
  },
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils",
    "ui": "@/components/ui",
    "lib": "@/lib",
    "hooks": "@/hooks"
  },
  "iconLibrary": "lucide"
}
```

- [ ] **Step 4: 配置 TypeScript 路径别名**

修改 `tsconfig.json`，在 `compilerOptions` 中添加：

```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}
```

修改 `vite.config.ts`，添加 resolve alias：

```ts
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vitest/config'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
  },
})
```

- [ ] **Step 5: 验证路径别名生效**

```bash
npm run build
```

Expected: 构建成功，无路径解析错误。

---

## Task 2: 创建 Raycast 主题与工具函数

**Files:**
- Modify: `src/styles.css`
- Create: `src/lib/utils.ts`

- [ ] **Step 1: 创建 cn() 工具函数**

创建 `src/lib/utils.ts`：

```ts
import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
```

- [ ] **Step 2: 替换 styles.css 为 Raycast 主题**

将 `src/styles.css` 替换为以下内容（基于 DESIGN.md 的 Raycast 风格，适配 shadcn/ui 的 CSS variable 结构）：

```css
@import "tailwindcss";

@custom-variant dark (&:is(.dark *));

:root {
  --radius: 0.5rem;
}

.dark {
  /* Canvas — near-pure neutral black */
  --background: #040506;
  --foreground: #ffffff;

  /* Card / Surface Level 1 */
  --card: #07080a;
  --card-foreground: #ffffff;

  /* Popover / Surface Level 2 */
  --popover: #111214;
  --popover-foreground: #ffffff;

  /* Primary CTA — near-white on dark */
  --primary: #e6e6e6;
  --primary-foreground: #040506;

  /* Secondary surface */
  --secondary: #111214;
  --secondary-foreground: #e6e6e6;

  /* Muted */
  --muted: #1b1c1e;
  --muted-foreground: #6a6b6c;

  /* Accent */
  --accent: #1b1c1e;
  --accent-foreground: #ffffff;

  /* Destructive — Ember Red */
  --destructive: #ff6363;
  --destructive-foreground: #ffffff;

  /* Border */
  --border: #363739;
  --input: #1b1c1e;
  --ring: #6a6b6c;

  /* Sidebar */
  --sidebar: #07080a;
  --sidebar-foreground: #ffffff;
  --sidebar-primary: #e6e6e6;
  --sidebar-primary-foreground: #040506;
  --sidebar-accent: #1b1c1e;
  --sidebar-accent-foreground: #ffffff;
  --sidebar-border: #363739;
  --sidebar-ring: #6a6b6c;

  /* Custom LuZhi tokens */
  --surface: #111214;
  --surface-hover: #1b1c1e;
  --recording: #ff6363;
}

@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-destructive-foreground: var(--destructive-foreground);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-sidebar: var(--sidebar);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-primary: var(--sidebar-primary);
  --color-sidebar-primary-foreground: var(--sidebar-primary-foreground);
  --color-sidebar-accent: var(--sidebar-accent);
  --color-sidebar-accent-foreground: var(--sidebar-accent-foreground);
  --color-sidebar-border: var(--sidebar-border);
  --color-sidebar-ring: var(--sidebar-ring);
  --color-surface: var(--surface);
  --color-surface-hover: var(--surface-hover);
  --color-recording: var(--recording);
  --radius-sm: calc(var(--radius) - 4px);
  --radius-md: calc(var(--radius) - 2px);
  --radius-lg: var(--radius);
  --radius-xl: calc(var(--radius) + 4px);
  --font-sans: 'Inter', ui-sans-serif, system-ui, -apple-system, sans-serif;
  --font-mono: 'GeistMono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

@layer base {
  * {
    @apply border-border outline-ring/50;
  }
  body {
    @apply bg-background text-foreground;
  }
}

/* Recording indicator pulse animation */
@keyframes pulse-recording {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.animate-pulse-recording {
  animation: pulse-recording 1.5s ease-in-out infinite;
}
```

- [ ] **Step 3: 验证主题生效**

```bash
npm run dev
```

在浏览器中打开 http://localhost:1420，确认页面背景为近黑色 (#040506)，文字为白色。

---

## Task 3: 安装 shadcn/ui 基础组件

**Files:**
- Create: `src/components/ui/button.tsx`
- Create: `src/components/ui/switch.tsx`
- Create: `src/components/ui/slider.tsx`
- Create: `src/components/ui/select.tsx`

- [ ] **Step 1: 创建 Button 组件**

创建 `src/components/ui/button.tsx`：

```tsx
import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '@/lib/utils'

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground hover:bg-primary/90',
        destructive:
          'bg-destructive text-white hover:bg-destructive/90 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 dark:bg-destructive/60',
        outline:
          'border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/50',
        secondary:
          'bg-secondary text-secondary-foreground hover:bg-secondary/80',
        ghost:
          'hover:bg-accent hover:text-accent-foreground dark:hover:bg-accent/50',
        link: 'text-primary underline-offset-4 hover:underline',
      },
      size: {
        default: 'h-9 px-4 py-2 has-[>svg]:px-3',
        sm: 'h-8 rounded-md gap-1.5 px-3 has-[>svg]:px-2.5',
        lg: 'h-10 rounded-md px-6 has-[>svg]:px-4',
        icon: 'size-9',
        'icon-sm': 'size-8',
        'icon-lg': 'size-10',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot : 'button'

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
```

- [ ] **Step 2: 创建 Switch 组件**

创建 `src/components/ui/switch.tsx`：

```tsx
import * as React from 'react'
import * as SwitchPrimitive from '@radix-ui/react-switch'

import { cn } from '@/lib/utils'

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        'peer data-[state=checked]:bg-primary data-[state=unchecked]:bg-input focus-visible:border-ring focus-visible:ring-ring/50 dark:data-[state=unchecked]:bg-input/80 inline-flex h-[1.15rem] w-8 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="bg-background dark:data-[state=unchecked]:bg-foreground dark:data-[state=checked]:bg-primary-foreground pointer-events-none block size-4 rounded-full ring-0 transition-transform data-[state=checked]:translate-x-[calc(100%-2px)] data-[state=unchecked]:translate-x-0"
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
```

- [ ] **Step 3: 创建 Slider 组件**

创建 `src/components/ui/slider.tsx`：

```tsx
import * as React from 'react'
import * as SliderPrimitive from '@radix-ui/react-slider'

import { cn } from '@/lib/utils'

function Slider({
  className,
  defaultValue,
  value,
  min = 0,
  max = 100,
  ...props
}: React.ComponentProps<typeof SliderPrimitive.Root>) {
  const _values = React.useMemo(
    () =>
      Array.isArray(value)
        ? value
        : Array.isArray(defaultValue)
          ? defaultValue
          : [min, max],
    [value, defaultValue, min, max],
  )

  return (
    <SliderPrimitive.Root
      data-slot="slider"
      defaultValue={defaultValue}
      value={value}
      min={min}
      max={max}
      className={cn(
        'relative flex w-full touch-none items-center select-none data-[disabled]:opacity-50 data-[orientation=vertical]:h-full data-[orientation=vertical]:min-h-44 data-[orientation=vertical]:w-auto data-[orientation=vertical]:flex-col',
        className,
      )}
      {...props}
    >
      <SliderPrimitive.Track
        data-slot="slider-track"
        className="bg-muted relative grow overflow-hidden rounded-full data-[orientation=horizontal]:h-1.5 data-[orientation=horizontal]:w-full data-[orientation=vertical]:h-full data-[orientation=vertical]:w-1.5"
      >
        <SliderPrimitive.Range
          data-slot="slider-range"
          className="bg-primary absolute data-[orientation=horizontal]:h-full data-[orientation=vertical]:w-full"
        />
      </SliderPrimitive.Track>
      {Array.from({ length: _values.length }, (_, index) => (
        <SliderPrimitive.Thumb
          data-slot="slider-thumb"
          key={index}
          className="border-primary ring-ring/50 block size-4 shrink-0 rounded-full border bg-white shadow-sm transition-[color,box-shadow] hover:ring-4 focus-visible:ring-4 focus-visible:outline-hidden disabled:pointer-events-none disabled:opacity-50"
        />
      ))}
    </SliderPrimitive.Root>
  )
}

export { Slider }
```

- [ ] **Step 4: 创建 Select 组件**

创建 `src/components/ui/select.tsx`（内容与 `_v0_reference/components/ui/select.tsx` 完全一致，仅修改 import 路径 `@/lib/utils`）。

- [ ] **Step 5: 验证组件可导入**

在 `src/App.tsx` 顶部临时添加测试 import，运行 `npm run build` 确认无编译错误。

---

## Task 4: 扩展 Tauri invoke 封装

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 扩展 tauri.ts 类型和函数**

将 `src/lib/tauri.ts` 替换为：

```ts
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// ─── 类型定义 ───

export type RecordingState = 'idle' | 'recording' | 'paused' | 'processing' | 'completed' | 'failed'

export type RecordingStatus = {
  state: RecordingState
  canStart: boolean
}

export type RecordingPermissions = {
  screenRecording: 'granted' | 'denied' | 'notDetermined' | 'unknown'
  microphone: 'granted' | 'denied' | 'notDetermined' | 'unknown'
}

export type CaptureMode = 'fullscreen' | 'window' | 'area'

export type AudioConfig = {
  systemAudio: boolean
  microphone: boolean
}

export type BeautifyConfig = {
  cursorMagnification: boolean
  magnificationFactor: number
  cursorSmoothing: boolean
  autoTrimSilences: boolean
  trimSensitivity: 'low' | 'medium' | 'high'
}

export type ExportPreset = 'bilibili' | 'douyin' | 'xiaohongshu'

// ─── Tauri Commands ───

export async function fetchRecordingStatus(): Promise<RecordingStatus> {
  return invoke<RecordingStatus>('recording_status')
}

export async function fetchRecordingPermissions(): Promise<RecordingPermissions> {
  return invoke<RecordingPermissions>('recording_permissions')
}

export async function startRecording(): Promise<void> {
  return invoke('start_recording')
}

export async function pauseRecording(): Promise<void> {
  return invoke('pause_recording')
}

export async function resumeRecording(): Promise<void> {
  return invoke('resume_recording')
}

export async function stopRecording(): Promise<void> {
  return invoke('stop_recording')
}

export async function setCaptureMode(mode: CaptureMode): Promise<void> {
  return invoke('set_capture_mode', { mode })
}

export async function setAudioConfig(config: AudioConfig): Promise<void> {
  return invoke('set_audio_config', { config })
}

export async function setBeautifyConfig(config: BeautifyConfig): Promise<void> {
  return invoke('set_beautify_config', { config })
}

export async function exportVideo(preset: ExportPreset): Promise<void> {
  return invoke('export_video', { preset })
}

// ─── Tauri Events ───

export function onRecordingTick(callback: (elapsed: number) => void): Promise<UnlistenFn> {
  return listen<{ elapsed: number }>('recording-tick', (event) => {
    callback(event.payload.elapsed)
  })
}

export function onMicLevel(callback: (level: number) => void): Promise<UnlistenFn> {
  return listen<{ level: number }>('mic-level', (event) => {
    callback(event.payload.level)
  })
}

export function onRecordingStateChanged(callback: (status: RecordingStatus) => void): Promise<UnlistenFn> {
  return listen<RecordingStatus>('recording-state-changed', (event) => {
    callback(event.payload)
  })
}
```

- [ ] **Step 2: 验证编译通过**

```bash
npm run build
```

Expected: 构建成功。新增的函数目前不会被调用，但类型定义必须正确。

---

## Task 5: 创建 RecordingPanel 组件

**Files:**
- Create: `src/components/recording-panel.tsx`

- [ ] **Step 1: 创建 RecordingPanel**

创建 `src/components/recording-panel.tsx`。从 `_v0_reference/components/recording-panel.tsx` 移植，做以下修改：

1. 删除 `"use client"` 指令
2. 将 `@/components/ui/button` 改为 `@/components/ui/button`（路径不变，但确保 alias 生效）
3. 将 `@/lib/utils` 改为 `@/lib/utils`
4. 将 `framer-motion` 的 `motion` 改为标准 import
5. 将主题色从紫色系改为 Raycast 中性色系：
   - `bg-primary/15` → `bg-surface`（surface token）
   - `border-primary/40` → `border-graphite-500`
   - `text-primary` → `text-foreground`
   - `bg-primary`（按钮）→ `bg-primary`（ash-50 近白色）
   - `shadow-primary/25` → `shadow-black/30`

```tsx
import { Monitor, AppWindow, Square, Volume2, Mic, Circle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { motion } from 'framer-motion'

interface RecordingPanelProps {
  recordingMode: 'fullscreen' | 'window' | 'area'
  setRecordingMode: (mode: 'fullscreen' | 'window' | 'area') => void
  systemAudioEnabled: boolean
  setSystemAudioEnabled: (enabled: boolean) => void
  micEnabled: boolean
  setMicEnabled: (enabled: boolean) => void
  micVolume: number
  onStartRecording: () => void
}

export function RecordingPanel({
  recordingMode,
  setRecordingMode,
  systemAudioEnabled,
  setSystemAudioEnabled,
  micEnabled,
  setMicEnabled,
  micVolume,
  onStartRecording,
}: RecordingPanelProps) {
  const modes = [
    { id: 'fullscreen' as const, icon: Monitor, label: '全屏' },
    { id: 'window' as const, icon: AppWindow, label: '窗口' },
    { id: 'area' as const, icon: Square, label: '区域' },
  ]

  return (
    <motion.div
      initial={{ opacity: 0, y: 20, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="bg-card/95 backdrop-blur-xl border border-border/50 rounded-2xl p-5 shadow-2xl shadow-black/30 w-[320px]"
    >
      {/* Header */}
      <div className="flex items-center justify-between mb-5">
        <div className="flex items-center gap-2">
          <div className="w-8 h-8 rounded-lg bg-surface flex items-center justify-center">
            <Circle className="w-4 h-4 text-foreground" />
          </div>
          <span className="font-semibold text-foreground">录制</span>
        </div>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground font-mono">
          <span className="px-2 py-0.5 rounded bg-secondary">1080p</span>
          <span className="px-2 py-0.5 rounded bg-secondary">60fps</span>
        </div>
      </div>

      {/* Mode Selection */}
      <div className="mb-5">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">录制模式</p>
        <div className="flex gap-2">
          {modes.map((mode) => (
            <motion.button
              key={mode.id}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.98 }}
              onClick={() => setRecordingMode(mode.id)}
              className={cn(
                'flex-1 flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl transition-all duration-200',
                recordingMode === mode.id
                  ? 'bg-surface border border-border text-foreground'
                  : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
              )}
            >
              <mode.icon className="w-5 h-5" />
              <span className="text-xs font-medium">{mode.label}</span>
            </motion.button>
          ))}
        </div>
      </div>

      {/* Audio Controls */}
      <div className="mb-6">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">音频设置</p>
        <div className="flex gap-2">
          {/* System Audio */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setSystemAudioEnabled(!systemAudioEnabled)}
            className={cn(
              'flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200',
              systemAudioEnabled
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
            )}
          >
            <Volume2 className="w-4 h-4" />
            <span className="text-xs font-medium">系统音频</span>
          </motion.button>

          {/* Microphone with volume indicator */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setMicEnabled(!micEnabled)}
            className={cn(
              'flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200 relative overflow-hidden',
              micEnabled
                ? 'bg-surface border border-border text-foreground'
                : 'bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground',
            )}
          >
            <Mic className="w-4 h-4" />
            <span className="text-xs font-medium">麦克风</span>
            {micEnabled && (
              <div className="absolute bottom-1 left-1/2 -translate-x-1/2 flex gap-0.5">
                {[...Array(5)].map((_, i) => (
                  <motion.div
                    key={i}
                    initial={{ height: 2 }}
                    animate={{ height: i < Math.ceil(micVolume / 20) ? 6 + i * 2 : 2 }}
                    transition={{ duration: 0.1 }}
                    className="w-1 rounded-full bg-muted-foreground/60"
                  />
                ))}
              </div>
            )}
          </motion.button>
        </div>
      </div>

      {/* Start Recording Button */}
      <motion.div whileHover={{ scale: 1.01 }} whileTap={{ scale: 0.99 }}>
        <Button
          onClick={onStartRecording}
          className="w-full h-12 rounded-xl bg-primary hover:bg-primary/90 text-primary-foreground font-semibold text-base shadow-lg shadow-black/30 transition-all duration-200"
        >
          <Circle className="w-4 h-4 mr-2 fill-current" />
          开始录制
        </Button>
      </motion.div>
    </motion.div>
  )
}
```

- [ ] **Step 2: 验证编译**

```bash
npm run build
```

Expected: 构建成功。

---

## Task 6: 创建 RecordingStatusBar 组件

**Files:**
- Create: `src/components/recording-status-bar.tsx`

- [ ] **Step 1: 创建 RecordingStatusBar**

从 `_v0_reference/components/recording-status-bar.tsx` 移植，主题色改为 Raycast 风格：

```tsx
import { Pause, Square, Circle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { motion } from 'framer-motion'

interface RecordingStatusBarProps {
  elapsedTime: number
  isPaused: boolean
  onPause: () => void
  onStop: () => void
}

function formatTime(seconds: number): string {
  const hrs = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60
  return `${hrs.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
}

export function RecordingStatusBar({
  elapsedTime,
  isPaused,
  onPause,
  onStop,
}: RecordingStatusBarProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: -20, scale: 0.9 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -20, scale: 0.9 }}
      transition={{ duration: 0.25, ease: 'easeOut' }}
      className="bg-card/95 backdrop-blur-xl border border-border/50 rounded-full px-4 py-2 shadow-2xl shadow-black/30 flex items-center gap-4"
    >
      {/* Recording Indicator */}
      <div className="flex items-center gap-2">
        <motion.div
          animate={isPaused ? {} : { scale: [1, 1.2, 1] }}
          transition={{ duration: 1.5, repeat: Infinity, ease: 'easeInOut' }}
          className="relative"
        >
          <Circle
            className={`w-3 h-3 fill-current ${
              isPaused ? 'text-muted-foreground' : 'text-recording animate-pulse-recording'
            }`}
          />
          {!isPaused && (
            <motion.div
              initial={{ scale: 1, opacity: 0.6 }}
              animate={{ scale: 2, opacity: 0 }}
              transition={{ duration: 1.5, repeat: Infinity, ease: 'easeOut' }}
              className="absolute inset-0 rounded-full bg-recording"
            />
          )}
        </motion.div>
        <span className="text-xs text-muted-foreground uppercase tracking-wider">
          {isPaused ? '暂停' : '录制中'}
        </span>
      </div>

      {/* Timer */}
      <div className="font-mono text-lg font-semibold text-foreground tabular-nums min-w-[80px] text-center">
        {formatTime(elapsedTime)}
      </div>

      {/* Controls */}
      <div className="flex items-center gap-1.5">
        <motion.div whileHover={{ scale: 1.1 }} whileTap={{ scale: 0.95 }}>
          <Button
            variant="ghost"
            size="icon"
            onClick={onPause}
            className="h-8 w-8 rounded-full hover:bg-secondary"
          >
            {isPaused ? (
              <Circle className="w-4 h-4 fill-primary text-primary" />
            ) : (
              <Pause className="w-4 h-4 text-foreground" />
            )}
          </Button>
        </motion.div>
        <motion.div whileHover={{ scale: 1.1 }} whileTap={{ scale: 0.95 }}>
          <Button
            variant="ghost"
            size="icon"
            onClick={onStop}
            className="h-8 w-8 rounded-full hover:bg-destructive/20 text-destructive hover:text-destructive"
          >
            <Square className="w-4 h-4 fill-current" />
          </Button>
        </motion.div>
      </div>
    </motion.div>
  )
}
```

- [ ] **Step 2: 验证编译**

```bash
npm run build
```

---

## Task 7: 创建 PreviewView 组件

**Files:**
- Create: `src/components/preview-view.tsx`

- [ ] **Step 1: 创建 PreviewView**

从 `_v0_reference/components/preview-view.tsx` 移植，主题色改为 Raycast 风格。关键修改：

1. 删除 `"use client"`
2. 紫色系 `text-primary` → `text-foreground` 或 `text-muted-foreground`
3. `bg-primary/10` → `bg-surface`
4. `bg-primary/20` → `bg-surface-hover`
5. 导出预设卡片的渐变色改为中性色渐变
6. 预留 Tauri invoke 接口（console.log 模拟）

```tsx
import { useState } from 'react'
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  Maximize2,
  MousePointer2,
  Sparkles,
  Scissors,
  Download,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Slider } from '@/components/ui/slider'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { motion } from 'framer-motion'
import type { BeautifyConfig, ExportPreset } from '@/lib/tauri'

interface PreviewViewProps {
  onBack: () => void
}

export function PreviewView({ onBack }: PreviewViewProps) {
  const [isPlaying, setIsPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(45)
  const [duration] = useState(180)
  const [volume, setVolume] = useState(80)

  // AI Beautification settings
  const [cursorMagnification, setCursorMagnification] = useState(true)
  const [magnificationFactor, setMagnificationFactor] = useState([2])
  const [cursorSmoothing, setCursorSmoothing] = useState(true)
  const [autoTrimSilences, setAutoTrimSilences] = useState(false)
  const [trimSensitivity, setTrimSensitivity] = useState<'low' | 'medium' | 'high'>('medium')

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = Math.floor(seconds % 60)
    return `${mins}:${secs.toString().padStart(2, '0')}`
  }

  // 预留 Tauri invoke 接口
  const handleBeautifyChange = (config: Partial<BeautifyConfig>) => {
    console.log('set_beautify_config', config)
    // TODO: invoke('set_beautify_config', { config })
  }

  const handleExport = (preset: ExportPreset) => {
    console.log('export_video', preset)
    // TODO: invoke('export_video', { preset })
  }

  const exportPresets: Array<{
    id: ExportPreset
    name: string
    specs: string
  }> = [
    { id: 'bilibili', name: 'Bilibili', specs: '16:9 · 1080p' },
    { id: 'douyin', name: '抖音', specs: '9:16 · 竖屏' },
    { id: 'xiaohongshu', name: '小红书', specs: '1:1 · 方形' },
  ]

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="flex h-screen bg-background"
    >
      {/* Main Preview Area */}
      <div className="flex-1 flex flex-col p-6">
        {/* Header */}
        <div className="flex items-center justify-between mb-4" data-tauri-drag-region>
          <button
            onClick={onBack}
            className="text-muted-foreground hover:text-foreground transition-colors text-sm flex items-center gap-2"
          >
            <SkipBack className="w-4 h-4" />
            返回录制
          </button>
          <h1 className="text-lg font-semibold text-foreground">预览与美化</h1>
          <div className="w-20" />
        </div>

        {/* Video Preview */}
        <div className="flex-1 bg-card rounded-2xl border border-border/50 overflow-hidden flex flex-col">
          {/* Video Area */}
          <div className="flex-1 bg-black/50 flex items-center justify-center relative">
            <div className="text-muted-foreground text-sm">视频预览区域</div>
            {cursorMagnification && (
              <motion.div
                initial={{ scale: 1 }}
                animate={{ scale: 1.1 }}
                transition={{ duration: 0.5, repeat: Infinity, repeatType: 'reverse' }}
                className="absolute bottom-1/3 right-1/3 w-6 h-6 rounded-full border-2 border-muted-foreground/50 bg-muted-foreground/10"
              />
            )}
          </div>

          {/* Video Controls */}
          <div className="p-4 border-t border-border/50 bg-card/50 backdrop-blur-sm">
            {/* Progress Bar */}
            <div className="mb-3">
              <Slider
                value={[currentTime]}
                max={duration}
                step={1}
                onValueChange={(value) => setCurrentTime(value[0])}
                className="w-full"
              />
              <div className="flex justify-between text-xs text-muted-foreground mt-1 font-mono">
                <span>{formatTime(currentTime)}</span>
                <span>{formatTime(duration)}</span>
              </div>
            </div>

            {/* Playback Controls */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
                  <SkipBack className="w-4 h-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10 rounded-full bg-surface hover:bg-surface-hover text-foreground"
                  onClick={() => setIsPlaying(!isPlaying)}
                >
                  {isPlaying ? (
                    <Pause className="w-5 h-5" />
                  ) : (
                    <Play className="w-5 h-5 ml-0.5" />
                  )}
                </Button>
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
                  <SkipForward className="w-4 h-4" />
                </Button>
              </div>

              <div className="flex items-center gap-3">
                <div className="flex items-center gap-2">
                  <Volume2 className="w-4 h-4 text-muted-foreground" />
                  <Slider
                    value={[volume]}
                    max={100}
                    step={1}
                    onValueChange={(value) => setVolume(value[0])}
                    className="w-24"
                  />
                </div>
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full">
                  <Maximize2 className="w-4 h-4" />
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Right Sidebar */}
      <div className="w-[340px] border-l border-border/50 bg-card/30 p-5 overflow-y-auto">
        {/* AI Beautification Section */}
        <div className="mb-8">
          <div className="flex items-center gap-2 mb-4">
            <Sparkles className="w-4 h-4 text-foreground" />
            <h2 className="text-sm font-semibold text-foreground uppercase tracking-wide">
              AI 美化
            </h2>
          </div>

          <div className="space-y-5">
            {/* Cursor Magnification */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <MousePointer2 className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">光标放大</span>
                </div>
                <Switch
                  checked={cursorMagnification}
                  onCheckedChange={(checked) => {
                    setCursorMagnification(checked)
                    handleBeautifyChange({ cursorMagnification: checked })
                  }}
                />
              </div>
              {cursorMagnification && (
                <motion.div
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  className="pt-2 border-t border-border/50"
                >
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-xs text-muted-foreground">放大倍数</span>
                    <span className="text-xs font-mono text-foreground">
                      {magnificationFactor[0]}x
                    </span>
                  </div>
                  <Slider
                    value={magnificationFactor}
                    min={1}
                    max={3}
                    step={0.5}
                    onValueChange={(value) => {
                      setMagnificationFactor(value)
                      handleBeautifyChange({ magnificationFactor: value[0] })
                    }}
                    className="w-full"
                  />
                </motion.div>
              )}
            </div>

            {/* Cursor Smoothing */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <MousePointer2 className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">光标平滑</span>
                </div>
                <Switch
                  checked={cursorSmoothing}
                  onCheckedChange={(checked) => {
                    setCursorSmoothing(checked)
                    handleBeautifyChange({ cursorSmoothing: checked })
                  }}
                />
              </div>
            </div>

            {/* Auto-trim Silences */}
            <div className="bg-secondary/30 rounded-xl p-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Scissors className="w-4 h-4 text-muted-foreground" />
                  <span className="text-sm text-foreground">自动剪除静音</span>
                </div>
                <Switch
                  checked={autoTrimSilences}
                  onCheckedChange={(checked) => {
                    setAutoTrimSilences(checked)
                    handleBeautifyChange({ autoTrimSilences: checked })
                  }}
                />
              </div>
              {autoTrimSilences && (
                <motion.div
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  className="pt-2 border-t border-border/50"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-muted-foreground">灵敏度</span>
                    <Select
                      value={trimSensitivity}
                      onValueChange={(value: 'low' | 'medium' | 'high') => {
                        setTrimSensitivity(value)
                        handleBeautifyChange({ trimSensitivity: value })
                      }}
                    >
                      <SelectTrigger className="w-24 h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="low">低</SelectItem>
                        <SelectItem value="medium">中</SelectItem>
                        <SelectItem value="high">高</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </motion.div>
              )}
            </div>
          </div>
        </div>

        {/* Export Section */}
        <div>
          <div className="flex items-center gap-2 mb-4">
            <Download className="w-4 h-4 text-foreground" />
            <h2 className="text-sm font-semibold text-foreground uppercase tracking-wide">
              导出
            </h2>
          </div>

          <div className="space-y-3">
            {exportPresets.map((preset) => (
              <motion.div
                key={preset.id}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.98 }}
                className={cn(
                  'relative overflow-hidden rounded-xl border border-border/50 p-4 cursor-pointer transition-all duration-200',
                  'bg-secondary/30 hover:border-border',
                )}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <p className="text-sm font-medium text-foreground">{preset.name}</p>
                    <p className="text-xs text-muted-foreground">{preset.specs}</p>
                  </div>
                  <Button
                    size="sm"
                    className="h-8 px-4 rounded-lg bg-surface hover:bg-surface-hover text-foreground border border-border/50"
                    onClick={() => handleExport(preset.id)}
                  >
                    导出
                  </Button>
                </div>
              </motion.div>
            ))}
          </div>
        </div>
      </div>
    </motion.div>
  )
}
```

- [ ] **Step 2: 验证编译**

```bash
npm run build
```

---

## Task 8: 创建补充视图组件（Processing + Error）

**Files:**
- Create: `src/components/processing-view.tsx`
- Create: `src/components/error-view.tsx`

- [ ] **Step 1: 创建 ProcessingView**

创建 `src/components/processing-view.tsx`：

```tsx
import { motion } from 'framer-motion'
import { Loader2 } from 'lucide-react'

interface ProcessingViewProps {
  message?: string
}

export function ProcessingView({ message = '正在处理录制内容...' }: ProcessingViewProps) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="min-h-screen flex items-center justify-center bg-background"
    >
      <div className="flex flex-col items-center gap-4">
        <motion.div
          animate={{ rotate: 360 }}
          transition={{ duration: 1, repeat: Infinity, ease: 'linear' }}
        >
          <Loader2 className="w-8 h-8 text-muted-foreground" />
        </motion.div>
        <p className="text-sm text-muted-foreground">{message}</p>
      </div>
    </motion.div>
  )
}
```

- [ ] **Step 2: 创建 ErrorView**

创建 `src/components/error-view.tsx`：

```tsx
import { motion } from 'framer-motion'
import { AlertCircle, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface ErrorViewProps {
  message: string
  onRetry?: () => void
  onBack?: () => void
}

export function ErrorView({ message, onRetry, onBack }: ErrorViewProps) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="min-h-screen flex items-center justify-center bg-background"
    >
      <div className="flex flex-col items-center gap-4 max-w-sm text-center">
        <AlertCircle className="w-10 h-10 text-destructive" />
        <h2 className="text-lg font-semibold text-foreground">录制失败</h2>
        <p className="text-sm text-muted-foreground">{message}</p>
        <div className="flex gap-3 mt-2">
          {onRetry && (
            <Button variant="outline" onClick={onRetry}>
              <RotateCcw className="w-4 h-4 mr-2" />
              重试
            </Button>
          )}
          {onBack && (
            <Button variant="ghost" onClick={onBack}>
              返回
            </Button>
          )}
        </div>
      </div>
    </motion.div>
  )
}
```

- [ ] **Step 3: 验证编译**

```bash
npm run build
```

---

## Task 9: 重写 App.tsx — 三态路由与 Tauri 集成

**Files:**
- Modify: `src/App.tsx`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: 重写 App.tsx**

将 `src/App.tsx` 替换为以下内容。核心变化：
1. 三态路由：idle/recording/preview（补充 processing/failed）
2. 录制面板状态由 props 传递，预留 Tauri invoke
3. 录制中窗口添加 `data-tauri-drag-region`
4. 桌面端禁用右键菜单和文本选中

```tsx
import { useCallback, useEffect, useState } from 'react'
import { AnimatePresence } from 'framer-motion'
import { RecordingPanel } from '@/components/recording-panel'
import { RecordingStatusBar } from '@/components/recording-status-bar'
import { PreviewView } from '@/components/preview-view'
import { ProcessingView } from '@/components/processing-view'
import { ErrorView } from '@/components/error-view'
import {
  fetchRecordingStatus,
  fetchRecordingPermissions,
  type RecordingPermissions,
  type RecordingStatus,
} from '@/lib/tauri'

type AppState = 'idle' | 'recording' | 'preview' | 'processing' | 'failed'

export default function App() {
  const [appState, setAppState] = useState<AppState>('idle')
  const [recordingMode, setRecordingMode] = useState<'fullscreen' | 'window' | 'area'>('fullscreen')
  const [systemAudioEnabled, setSystemAudioEnabled] = useState(true)
  const [micEnabled, setMicEnabled] = useState(false)
  const [micVolume, setMicVolume] = useState(60)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [isPaused, setIsPaused] = useState(false)
  const [permissions, setPermissions] = useState<RecordingPermissions>({
    screenRecording: 'unknown',
    microphone: 'unknown',
  })
  const [errorMessage, setErrorMessage] = useState('')

  // 从 Tauri 后端获取初始状态
  useEffect(() => {
    void fetchRecordingStatus().then((status: RecordingStatus) => {
      if (status.state === 'recording') setAppState('recording')
      else if (status.state === 'processing') setAppState('processing')
      else if (status.state === 'completed') setAppState('preview')
      else if (status.state === 'failed') {
        setAppState('failed')
        setErrorMessage('录制过程中发生错误')
      }
    })
    void fetchRecordingPermissions().then(setPermissions)
  }, [])

  // 模拟麦克风音量变化（后续替换为 Tauri event）
  useEffect(() => {
    if (!micEnabled) return
    const interval = setInterval(() => {
      setMicVolume(Math.floor(Math.random() * 60) + 20)
    }, 200)
    return () => clearInterval(interval)
  }, [micEnabled])

  // 录制计时器（后续替换为 Tauri event 'recording-tick'）
  useEffect(() => {
    if (appState !== 'recording' || isPaused) return
    const interval = setInterval(() => {
      setElapsedTime((prev) => prev + 1)
    }, 1000)
    return () => clearInterval(interval)
  }, [appState, isPaused])

  const handleStartRecording = useCallback(() => {
    console.log('start_recording', { recordingMode, systemAudioEnabled, micEnabled })
    // TODO: await invoke('start_recording')
    setAppState('recording')
    setElapsedTime(0)
    setIsPaused(false)
  }, [recordingMode, systemAudioEnabled, micEnabled])

  const handlePauseRecording = useCallback(() => {
    console.log(isPaused ? 'resume_recording' : 'pause_recording')
    // TODO: await invoke(isPaused ? 'resume_recording' : 'pause_recording')
    setIsPaused((prev) => !prev)
  }, [isPaused])

  const handleStopRecording = useCallback(() => {
    console.log('stop_recording')
    // TODO: await invoke('stop_recording')
    setAppState('preview')
  }, [])

  const handleBackToIdle = useCallback(() => {
    setAppState('idle')
    setElapsedTime(0)
    setIsPaused(false)
    setErrorMessage('')
  }, [])

  const handleRetry = useCallback(() => {
    setAppState('idle')
    setErrorMessage('')
  }, [])

  // 禁用桌面端右键菜单和文本选中
  useEffect(() => {
    const handler = (e: Event) => e.preventDefault()
    document.addEventListener('contextmenu', handler)
    document.addEventListener('selectstart', handler)
    return () => {
      document.removeEventListener('contextmenu', handler)
      document.removeEventListener('selectstart', handler)
    }
  }, [])

  // Idle state
  if (appState === 'idle') {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background p-8">
        <div className="relative">
          <div className="absolute inset-0 -z-10 rounded-3xl bg-gradient-to-br from-muted/5 via-transparent to-muted/5 blur-3xl scale-150" />
          <RecordingPanel
            recordingMode={recordingMode}
            setRecordingMode={setRecordingMode}
            systemAudioEnabled={systemAudioEnabled}
            setSystemAudioEnabled={setSystemAudioEnabled}
            micEnabled={micEnabled}
            setMicEnabled={setMicEnabled}
            micVolume={micVolume}
            onStartRecording={handleStartRecording}
          />
          {/* 权限提示 */}
          {(permissions.screenRecording === 'denied' || permissions.microphone === 'denied') && (
            <div className="mt-4 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              {permissions.screenRecording === 'denied' && <p>屏幕录制权限未授权，请在系统设置中开启</p>}
              {permissions.microphone === 'denied' && <p>麦克风权限未授权，请在系统设置中开启</p>}
            </div>
          )}
        </div>
      </div>
    )
  }

  // Recording state
  if (appState === 'recording') {
    return (
      <div className="min-h-screen flex flex-col items-center justify-between bg-background p-8">
        <div className="pt-4" data-tauri-drag-region>
          <AnimatePresence>
            <RecordingStatusBar
              elapsedTime={elapsedTime}
              isPaused={isPaused}
              onPause={handlePauseRecording}
              onStop={handleStopRecording}
            />
          </AnimatePresence>
        </div>
        <div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-border/30 flex items-center justify-center">
          <div className="text-center text-muted-foreground">
            <p className="text-sm mb-1">
              正在录制 {recordingMode === 'fullscreen' ? '全屏' : recordingMode === 'window' ? '窗口' : '区域'}
            </p>
            <p className="text-xs opacity-60">此区域表示被录制的屏幕内容</p>
          </div>
        </div>
        <div className="h-12" />
      </div>
    )
  }

  // Processing state
  if (appState === 'processing') {
    return <ProcessingView />
  }

  // Failed state
  if (appState === 'failed') {
    return (
      <ErrorView
        message={errorMessage}
        onRetry={handleRetry}
        onBack={handleBackToIdle}
      />
    )
  }

  // Preview state
  return <PreviewView onBack={handleBackToIdle} />
}
```

- [ ] **Step 2: 更新 tauri.conf.json 窗口配置**

修改 `src-tauri/tauri.conf.json` 中的窗口配置，添加 `decorations: false`（无边框窗口，用于自定义标题栏拖拽区域）：

```json
{
  "app": {
    "windows": [
      {
        "title": "录智",
        "width": 800,
        "height": 600,
        "decorations": false,
        "transparent": true
      }
    ]
  }
}
```

- [ ] **Step 3: 验证编译**

```bash
npm run build
```

---

## Task 10: 更新测试

**Files:**
- Modify: `src/App.test.tsx`

- [ ] **Step 1: 更新 App 测试**

将 `src/App.test.tsx` 替换为适配新组件结构的测试：

```tsx
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'unknown',
          microphone: 'unknown',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })
  })

  it('renders Chinese recording controls in idle state', async () => {
    render(<App />)

    expect(await screen.findByText('录制')).toBeInTheDocument()
    expect(screen.getByText('全屏')).toBeInTheDocument()
    expect(screen.getByText('窗口')).toBeInTheDocument()
    expect(screen.getByText('区域')).toBeInTheDocument()
    expect(screen.getByText('开始录制')).toBeInTheDocument()
  })

  it('renders audio settings', async () => {
    render(<App />)

    expect(await screen.findByText('系统音频')).toBeInTheDocument()
    expect(screen.getByText('麦克风')).toBeInTheDocument()
  })

  it('requests status and permissions from Tauri', async () => {
    render(<App />)

    await screen.findByText('录制')

    expect(invokeMock).toHaveBeenCalledWith('recording_status')
    expect(invokeMock).toHaveBeenCalledWith('recording_permissions')
  })

  it('shows permission warning when denied', async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === 'recording_status') {
        return Promise.resolve({ state: 'idle', canStart: true })
      }
      if (command === 'recording_permissions') {
        return Promise.resolve({
          screenRecording: 'denied',
          microphone: 'denied',
        })
      }
      return Promise.reject(new Error(`unexpected command ${command}`))
    })

    render(<App />)

    expect(await screen.findByText('屏幕录制权限未授权，请在系统设置中开启')).toBeInTheDocument()
    expect(screen.getByText('麦克风权限未授权，请在系统设置中开启')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: 运行测试**

```bash
npm run test
```

Expected: 所有 4 个测试通过。

- [ ] **Step 3: 提交**

```bash
git add -A
git commit -m "feat(ui): 移植 v0 设计稿到 Tauri 项目，实现三态路由与 Raycast 主题"
```

---

## 验证清单

完成所有 Task 后，执行以下验证：

1. **编译验证**：`npm run build` 无错误
2. **测试验证**：`npm run test` 全部通过
3. **主题验证**：`npm run dev` 打开浏览器，确认背景为近黑色 (#040506)，文字为白色
4. **状态路由验证**：
   - 空闲态：显示录制配置面板（模式选择 + 音频开关 + 开始按钮）
   - 点击"开始录制"→ 录制态：显示悬浮状态栏（计时器 + 暂停/停止）
   - 点击停止 → 预览态：显示视频预览 + AI 美化侧边栏 + 导出面板
5. **权限提示验证**：当权限为 denied 时，显示中文权限引导
6. **桌面端验证**：右键菜单被禁用，文本不可选中
