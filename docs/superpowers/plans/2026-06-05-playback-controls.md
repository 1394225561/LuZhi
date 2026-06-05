# 播放控件完整功能实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将美化界面左侧主预览区域底部的播放控件从伪代码升级为完整可用的视频播放控制功能。

**Architecture:** 使用原生 HTMLVideoElement API 直接控制视频播放，通过 useRef 获取视频引用，监听原生事件同步状态，移除浏览器原生 controls。

**Tech Stack:** React, TypeScript, HTMLVideoElement API, @radix-ui/react-slider, lucide-react

---

## 文件结构

| 文件 | 职责 |
|------|------|
| `src/components/preview-view.tsx` | 核心修改：添加 videoRef、事件监听、控件交互逻辑、移除 controls |
| `src/components/ui/slider.tsx` | 无需修改（已支持 onValueCommit 透传） |
| `src/App.test.tsx` | 新增播放控件相关测试 |

## 关键依赖

- `@radix-ui/react-slider`：原生支持 `onValueCommit` 事件，Slider 组件通过 `...props` 透传
- `lucide-react`：需新增 `VolumeX` 图标导入（当前只有 `Volume2`）

---

### Task 1: Foundation — videoRef、状态变更、事件监听、移除原生 controls

**Files:**
- Modify: `src/components/preview-view.tsx:1-12,69-72,303-310`

- [ ] **Step 1: 添加 VolumeX 导入**

修改 `src/components/preview-view.tsx` 第 1-12 行的导入，添加 `VolumeX`：

```tsx
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  Maximize2,
  MousePointer2,
  Sparkles,
  Scissors,
  Download,
} from 'lucide-react'
```

- [ ] **Step 2: 添加 videoRef 和 isSeekingRef，修改初始状态**

修改 `src/components/preview-view.tsx` 第 68-72 行：

```tsx
export function PreviewView({ onBack, recordingResult, licenseStatus }: PreviewViewProps) {
  const videoRef = useRef<HTMLVideoElement>(null)
  const videoContainerRef = useRef<HTMLDivElement>(null)
  const isSeekingRef = useRef(false)
  const [isPlaying, setIsPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [volume, setVolume] = useState(80)
  const [isMuted, setIsMuted] = useState(false)
```

- [ ] **Step 3: 添加视频事件监听 useEffect**

在 `src/components/preview-view.tsx` 中，现有 `useEffect` 块之后（约第 122 行后）添加：

```tsx
  // Sync video element events with React state
  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const onLoadedMetadata = () => setDuration(video.duration)
    const onTimeUpdate = () => {
      if (!isSeekingRef.current) {
        setCurrentTime(video.currentTime)
      }
    }
    const onPlay = () => setIsPlaying(true)
    const onPause = () => setIsPlaying(false)
    const onEnded = () => setIsPlaying(false)

    video.addEventListener('loadedmetadata', onLoadedMetadata)
    video.addEventListener('timeupdate', onTimeUpdate)
    video.addEventListener('play', onPlay)
    video.addEventListener('pause', onPause)
    video.addEventListener('ended', onEnded)

    return () => {
      video.removeEventListener('loadedmetadata', onLoadedMetadata)
      video.removeEventListener('timeupdate', onTimeUpdate)
      video.removeEventListener('play', onPlay)
      video.removeEventListener('pause', onPause)
      video.removeEventListener('ended', onEnded)
    }
  }, [])
```

- [ ] **Step 4: 修改 video 元素 — 移除 controls，添加 ref 和 playsInline**

修改 `src/components/preview-view.tsx` 第 303-310 行：

```tsx
          <div className="flex-1 bg-black/50 flex items-center justify-center relative" ref={videoContainerRef}>
            {recordingResult?.outputPath ? (
              <video
                ref={videoRef}
                src={convertFileSrc(recordingResult.outputPath)}
                playsInline
                className="w-full h-full object-contain"
              />
            ) : (
```

- [ ] **Step 5: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 6: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件基础 — videoRef、事件监听、移除原生 controls"
```

---

### Task 2: 播放/暂停、后退/前进控件

**Files:**
- Modify: `src/components/preview-view.tsx:352-371`

- [ ] **Step 1: 添加播放/暂停处理函数**

在 `src/components/preview-view.tsx` 中，`handleBack` 函数之后添加：

```tsx
  const handlePlayPause = () => {
    const video = videoRef.current
    if (!video) return
    if (video.paused) {
      void video.play()
    } else {
      video.pause()
    }
  }

  const handleSkipBack = () => {
    const video = videoRef.current
    if (!video) return
    video.currentTime = Math.max(0, video.currentTime - 10)
  }

  const handleSkipForward = () => {
    const video = videoRef.current
    if (!video) return
    video.currentTime = Math.min(video.duration, video.currentTime + 10)
  }
```

- [ ] **Step 2: 连接播放/暂停按钮 onClick**

修改 `src/components/preview-view.tsx` 第 357-368 行，将 `onClick={() => setIsPlaying(!isPlaying)}` 改为 `onClick={handlePlayPause}`：

```tsx
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10 rounded-full bg-surface hover:bg-surface-hover text-foreground"
                  onClick={handlePlayPause}
                >
```

- [ ] **Step 3: 连接 SkipBack 按钮 onClick**

修改 `src/components/preview-view.tsx` 第 354-356 行：

```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipBack}>
                  <SkipBack className="w-4 h-4" />
                </Button>
```

- [ ] **Step 4: 连接 SkipForward 按钮 onClick**

修改 `src/components/preview-view.tsx` 第 369-371 行：

```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipForward}>
                  <SkipForward className="w-4 h-4" />
                </Button>
```

- [ ] **Step 5: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 6: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件 — 播放/暂停、后退/前进 10 秒"
```

---

### Task 3: 进度条拖拽 seek（onValueCommit）

**Files:**
- Modify: `src/components/preview-view.tsx:337-349`

- [ ] **Step 1: 修改进度条 Slider — 添加 onValueCommit 实现松手 seek**

修改 `src/components/preview-view.tsx` 第 337-349 行：

```tsx
            <div className="mb-3">
              <Slider
                value={[currentTime]}
                max={duration || 1}
                step={0.1}
                onValueChange={(value) => {
                  isSeekingRef.current = true
                  setCurrentTime(value[0])
                }}
                onValueCommit={(value) => {
                  isSeekingRef.current = false
                  if (videoRef.current) {
                    videoRef.current.currentTime = value[0]
                  }
                }}
                className="w-full"
              />
              <div className="flex justify-between text-xs text-muted-foreground mt-1 font-mono">
                <span>{formatTime(currentTime)}</span>
                <span>{formatTime(duration)}</span>
              </div>
            </div>
```

关键变更：
- `max` 从 `duration` 改为 `duration || 1`（避免 duration=0 时 Slider max=0 导致除零）
- `step` 从 `1` 改为 `0.1`（支持亚秒级 seek 精度）
- 添加 `onValueChange` 设置 `isSeekingRef.current = true`
- 添加 `onValueCommit` 在松手时执行 seek

- [ ] **Step 2: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 3: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件 — 进度条拖拽 seek（onValueCommit）"
```

---

### Task 4: 音量控制与静音切换

**Files:**
- Modify: `src/components/preview-view.tsx:374-388`

- [ ] **Step 1: 添加音量和静音处理函数**

在 `src/components/preview-view.tsx` 中，`handleSkipForward` 函数之后添加：

```tsx
  const handleVolumeChange = (value: number[]) => {
    setVolume(value[0])
    if (videoRef.current) {
      videoRef.current.volume = value[0] / 100
      videoRef.current.muted = value[0] === 0
      setIsMuted(value[0] === 0)
    }
  }

  const handleMuteToggle = () => {
    const video = videoRef.current
    if (!video) return
    const newMuted = !video.muted
    video.muted = newMuted
    setIsMuted(newMuted)
  }
```

- [ ] **Step 2: 连接音量 Slider 的 onValueChange**

修改 `src/components/preview-view.tsx` 第 374-388 行：

```tsx
              <div className="flex items-center gap-3">
                <div className="flex items-center gap-2">
                  <button onClick={handleMuteToggle} className="text-muted-foreground hover:text-foreground transition-colors">
                    {isMuted || volume === 0 ? (
                      <VolumeX className="w-4 h-4" />
                    ) : (
                      <Volume2 className="w-4 h-4" />
                    )}
                  </button>
                  <Slider
                    value={[volume]}
                    max={100}
                    step={1}
                    onValueChange={handleVolumeChange}
                    className="w-24"
                  />
                </div>
```

关键变更：
- `Volume2` 图标替换为条件渲染：`isMuted || volume === 0` 时显示 `VolumeX`，否则显示 `Volume2`
- 图标改为可点击的 `<button>` 触发 `handleMuteToggle`
- Slider 的 `onValueChange` 连接到 `handleVolumeChange`

- [ ] **Step 3: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 4: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件 — 音量控制与静音切换"
```

---

### Task 5: 全屏功能

**Files:**
- Modify: `src/components/preview-view.tsx:385-387`

- [ ] **Step 1: 添加全屏处理函数**

在 `src/components/preview-view.tsx` 中，`handleMuteToggle` 函数之后添加：

```tsx
  const handleFullscreen = () => {
    if (!videoContainerRef.current) return
    if (document.fullscreenElement) {
      void document.exitFullscreen()
    } else {
      void videoContainerRef.current.requestFullscreen()
    }
  }
```

- [ ] **Step 2: 连接全屏按钮 onClick**

修改 `src/components/preview-view.tsx` 第 385-387 行：

```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleFullscreen}>
                  <Maximize2 className="w-4 h-4" />
                </Button>
```

- [ ] **Step 3: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 4: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件 — 全屏功能"
```

---

### Task 6: 无视频源时禁用控件

**Files:**
- Modify: `src/components/preview-view.tsx:354,357,369,385`

- [ ] **Step 1: 为所有播放控件按钮添加 disabled 条件**

修改 `src/components/preview-view.tsx` 中的 4 个按钮，添加 `disabled={!recordingResult?.outputPath}`：

SkipBack 按钮：
```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipBack} disabled={!recordingResult?.outputPath}>
```

Play/Pause 按钮：
```tsx
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-10 w-10 rounded-full bg-surface hover:bg-surface-hover text-foreground"
                  onClick={handlePlayPause}
                  disabled={!recordingResult?.outputPath}
                >
```

SkipForward 按钮：
```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleSkipForward} disabled={!recordingResult?.outputPath}>
```

全屏按钮：
```tsx
                <Button variant="ghost" size="icon" className="h-8 w-8 rounded-full" onClick={handleFullscreen} disabled={!recordingResult?.outputPath}>
```

- [ ] **Step 2: 验证编译通过**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 3: 提交**

```bash
git add src/components/preview-view.tsx
git commit -m "feat(ui): 播放控件 — 无视频源时禁用所有控件"
```

---

### Task 7: 前端测试

**Files:**
- Modify: `src/App.test.tsx`

- [ ] **Step 1: 添加播放控件测试 — 播放/暂停按钮切换**

在 `src/App.test.tsx` 中添加测试：

```tsx
  test('play/pause button calls video.play() and video.pause()', async () => {
    // Setup: navigate to preview state with recording result
    const mockPlay = vi.fn().mockResolvedValue(undefined)
    const mockPause = vi.fn()
    
    // Mock HTMLVideoElement
    Object.defineProperty(HTMLVideoElement.prototype, 'play', { value: mockPlay, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'pause', { value: mockPause, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'paused', { value: true, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'duration', { value: 100, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'currentTime', { value: 0, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'volume', { value: 0.8, writable: true })
    Object.defineProperty(HTMLVideoElement.prototype, 'muted', { value: false, writable: true })

    // ... render and navigate to preview state
    // ... click play button
    // ... expect mockPlay toHaveBeenCalled
    // ... click pause button
    // ... expect mockPause toHaveBeenCalled
  })
```

- [ ] **Step 2: 添加进度条 seek 测试**

```tsx
  test('progress bar seek updates video.currentTime on commit', async () => {
    // Setup: navigate to preview state
    // Mock video element with currentTime setter spy
    // ... drag progress bar
    // ... expect video.currentTime to be updated
  })
```

- [ ] **Step 3: 添加音量控制测试**

```tsx
  test('volume slider updates video volume', async () => {
    // Setup: navigate to preview state
    // ... change volume slider
    // ... expect video.volume to be updated
  })
```

- [ ] **Step 4: 运行测试验证**

Run: `npm test -- --run`
Expected: All tests PASS

- [ ] **Step 5: 提交**

```bash
git add src/App.test.tsx
git commit -m "test(ui): 播放控件前端测试"
```

---

### Task 8: 完整验证与回归

- [ ] **Step 1: 运行完整测试套件**

Run: `npm test -- --run`
Expected: All tests PASS

- [ ] **Step 2: 运行构建验证**

Run: `npm run build`
Expected: BUILD SUCCESS

- [ ] **Step 3: 运行 lint 检查**

Run: `npx eslint src/components/preview-view.tsx`
Expected: No errors

- [ ] **Step 4: 手动验证清单**

在 `npm run tauri dev` 环境下验证：
1. 播放按钮点击 → 视频开始播放，图标变为 Pause
2. 暂停按钮点击 → 视频暂停，图标变为 Play
3. 后退按钮 → 视频后退 10 秒
4. 前进按钮 → 视频前进 10 秒
5. 进度条拖拽 → 视频 seek 到目标位置，松手前不跳动
6. 音量滑块 → 视频音量实时变化
7. 静音图标点击 → 视频静音/取消静音，图标切换
8. 全屏按钮 → 视频容器进入全屏
9. 播放结束 → 自动暂停在最后一帧
10. 无录制文件 → 控件按钮禁用

- [ ] **Step 5: 最终提交**

```bash
git add -A
git commit -m "feat(ui): 播放控件完整功能实现完成"
```
