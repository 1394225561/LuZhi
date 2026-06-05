# 播放控件完整功能设计

> 日期：2026-06-05 | 状态：设计通过

## 1. 问题描述

美化界面左侧主预览区域底部的播放控件当前是伪代码：
- `currentTime` 硬编码为 45，`duration` 硬编码为 180
- 播放/暂停按钮仅切换 `isPlaying` 状态，不实际控制视频
- SkipBack/SkipForward 按钮无 onClick 处理
- 进度条拖拽只更新状态，不 seek 视频
- 音量滑块只更新状态，不控制视频音量
- 全屏按钮无功能
- `<video>` 标签使用原生 `controls`，与自定义控件重叠

## 2. 方案选择

采用方案 A：原生 Video API 直接控制。

理由：单一播放场景，直接使用 `HTMLVideoElement` API 最简洁，无额外依赖，符合 YAGNI 原则。

## 3. 详细设计

### 3.1 状态与引用变更

```tsx
// 新增
const videoRef = useRef<HTMLVideoElement>(null)
const isSeekingRef = useRef(false)  // 进度条拖拽防抖

// 修改
const [currentTime, setCurrentTime] = useState(0)   // 原 45 → 0
const [duration, setDuration] = useState(0)           // 原 180 → 0
const [isMuted, setIsMuted] = useState(false)         // 新增静音状态
```

### 3.2 视频事件监听

```tsx
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

### 3.3 控件交互逻辑

| 控件 | 实现 |
|------|------|
| 播放/暂停 | `video.play()` / `video.pause()` |
| 后退 10s | `video.currentTime = Math.max(0, currentTime - 10)` |
| 前进 10s | `video.currentTime = Math.min(duration, currentTime + 10)` |
| 进度条拖拽 | `onValueChange` → `video.currentTime = seekTarget`；`isSeekingRef` 防抖 |
| 音量滑块 | `video.volume = value / 100`；`video.muted = value === 0` |
| 静音切换 | 点击音量图标 → `video.muted = !video.muted`；同步 `isMuted` 状态 |
| 全屏 | 对视频容器调用 `requestFullscreen()` / `exitFullscreen()` |

### 3.4 视频元素修改

```tsx
// 修改前
<video
  src={convertFileSrc(recordingResult.outputPath)}
  controls
  className="w-full h-full object-contain"
/>

// 修改后
<video
  ref={videoRef}
  src={convertFileSrc(recordingResult.outputPath)}
  playsInline
  className="w-full h-full object-contain"
/>
```

移除 `controls` 属性，添加 `ref` 和 `playsInline`。

### 3.5 进度条拖拽防抖

```tsx
// onValueChange 时
isSeekingRef.current = true
setCurrentTime(value[0])

// onValueCommit 时（Slider 需支持 onValueCommit）
isSeekingRef.current = false
if (videoRef.current) {
  videoRef.current.currentTime = value[0]
}
```

注意：当前 Slider 组件基于 `@radix-ui/react-slider`，Radix Slider 原生支持 `onValueCommit` 事件（松手时触发），无需修改 Slider 组件。

### 3.6 音量控制

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
  if (videoRef.current) {
    const newMuted = !videoRef.current.muted
    videoRef.current.muted = newMuted
    setIsMuted(newMuted)
  }
}
```

音量图标：`isMuted || volume === 0` 时显示 `VolumeX`，否则显示 `Volume2`。

### 3.7 全屏实现

```tsx
const videoContainerRef = useRef<HTMLDivElement>(null)

const handleFullscreen = () => {
  if (!videoContainerRef.current) return
  if (document.fullscreenElement) {
    document.exitFullscreen()
  } else {
    videoContainerRef.current.requestFullscreen()
  }
}
```

### 3.8 格式化函数

已有 `formatTime` 函数，无需修改。

### 3.9 无视频源时的禁用

当 `!recordingResult?.outputPath` 时，所有播放控件按钮应 `disabled`。

## 4. 修改文件清单

| 文件 | 修改内容 |
|------|----------|
| `src/components/preview-view.tsx` | 核心修改：videoRef、事件监听、控件交互逻辑、移除 controls |

仅修改 1 个文件，不涉及后端改动。

## 5. 测试策略

- 前端测试：验证播放/暂停按钮切换、进度条拖拽、音量控制、静音切换
- Mock：Mock `HTMLVideoElement` 的 `play()`/`pause()` 方法
- 手动验证：录制一段视频 → 进入预览 → 验证所有控件功能

## 6. 验证清单

1. 播放按钮点击 → 视频开始播放，图标变为 Pause
2. 暂停按钮点击 → 视频暂停，图标变为 Play
3. 后退按钮 → 视频后退 10 秒
4. 前进按钮 → 视频前进 10 秒
5. 进度条拖拽 → 视频 seek 到目标位置，松手前不跳动
6. 音量滑块 → 视频音量实时变化
7. 静音图标点击 → 视频静音/取消静音
8. 全屏按钮 → 视频容器进入全屏
9. 播放结束 → 自动暂停在最后一帧
10. 无录制文件 → 控件按钮禁用
