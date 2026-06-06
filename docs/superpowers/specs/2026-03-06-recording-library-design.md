# 录制历史库与导入功能设计

> 版本：v1.0
> 日期：2026-03-06
> 状态：设计完成，待审查

## 1. 概述

### 1.1 需求背景

当前问题：
- 录制完成后进入预览/美化界面，如果用户离开（返回空闲状态），就无法再对刚才录制的视频进行处理
- 架构文档明确要求"保留原始录制文件：用于回退、重新裁剪、重新导出"

### 1.2 功能目标

1. **回退与重新处理**：用户离开预览界面后，可以随时返回继续美化和导出
2. **历史录制列表**：在空闲界面显示历史录制，支持快速访问
3. **导入功能**：支持导入本应用之前录制的视频文件

### 1.3 设计决策

| 维度 | 决定 |
|------|------|
| 功能范围 | 完整重新美化（光标、裁剪、导出都可调整） |
| 文件生命周期 | 永久保留，直到手动删除 |
| 入口 | 侧边栏"最近录制"列表 + 文件选择器"导入"按钮 |
| 校验 | 严格模式（必须有完整配套元数据） |
| 侧边栏位置 | 录制面板右侧，可折叠 |
| 默认状态 | 收起 |
| 导出后行为 | 停留在预览界面，用户主动返回才回到 idle |

---

## 2. 数据结构

### 2.1 索引文件

存储位置：`~/Library/Application Support/com.luzhi.app/recordings-index.json`

```json
{
  "version": 1,
  "entries": [
    {
      "id": "rec-1717000000000-0",
      "createdAt": 1717000000000,
      "durationSecs": 125.5,
      "videoPath": "/tmp/luzhi-recordings/recording-1717000000000-0.mp4",
      "cursorMetadataPath": "/tmp/luzhi-recordings/cursor-metadata-1717000000000-0.json",
      "effectTimelinePath": "/tmp/luzhi-recordings/cursor-effects-1717000000000-0.json",
      "trimMetadataPath": "/tmp/luzhi-recordings/trim-metadata-1717000000000-0.json",
      "cutTimelinePath": "/tmp/luzhi-recordings/cut-timeline-1717000000000-0.json"
    }
  ]
}
```

### 2.2 Rust 结构体

```rust
// recording_library.rs

/// 索引文件结构
#[derive(Serialize, Deserialize)]
struct RecordingIndex {
    version: u32,
    entries: Vec<LibraryEntry>,
}

/// 单条历史录制记录
#[derive(Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub id: String,                    // "rec-{millis}-{seq}"
    pub created_at: u64,               // 毫秒时间戳
    pub duration_secs: f64,            // 录制时长
    pub video_path: PathBuf,           // 原始 MP4 路径
    pub cursor_metadata_path: PathBuf, // 光标元数据路径
    pub effect_timeline_path: PathBuf, // 效果时间线路径
    pub trim_metadata_path: PathBuf,   // 裁剪元数据路径
    pub cut_timeline_path: PathBuf,    // 裁剪时间线路径
}

/// 返回给前端的摘要（不含完整路径）
#[derive(Serialize)]
pub struct LibraryEntrySummary {
    pub id: String,
    pub created_at: u64,
    pub duration_secs: f64,
}

/// 导入时的完整上下文
pub struct RecordingContext {
    pub entry: LibraryEntry,
    pub metadata: RecordingMetadata,
    pub effect_timeline: EffectTimeline,
    pub cut_timeline: CutTimeline,
}
```

### 2.3 前端类型

```typescript
// tauri.ts
export type LibraryEntrySummary = {
  id: string
  createdAt: number
  durationSecs: number
}
```

---

## 3. 后端模块设计

### 3.1 模块位置

新增文件：`src-tauri/src/app/recording_library.rs`

### 3.2 核心 API

```rust
pub struct RecordingLibrary {
    index_path: PathBuf,
    index: RecordingIndex,
}

impl RecordingLibrary {
    /// 初始化：加载或创建索引文件
    pub fn new(app_data_dir: &Path) -> Self;

    /// 录制完成后自动注册
    pub fn register(&mut self, result: &RecordingResult) -> AppResult<()>;

    /// 获取列表摘要（给侧边栏用）
    pub fn list(&self) -> Vec<LibraryEntrySummary>;

    /// 获取完整上下文（进入美化界面时用）
    pub fn get_recording(&self, id: &str) -> AppResult<RecordingContext>;

    /// 校验并导入外部文件
    pub fn import(&mut self, video_path: &Path) -> AppResult<LibraryEntry>;

    /// 删除录制
    pub fn delete(&mut self, id: &str, delete_files: bool) -> AppResult<()>;

    /// 启动时修复索引
    pub fn repair_on_startup(&mut self);
}
```

### 3.3 Tauri 命令

```rust
#[tauri::command]
fn list_recordings(state: State<AppState>) -> Result<Vec<LibraryEntrySummary>, String>;

#[tauri::command]
fn import_recording(path: String, state: State<AppState>) -> Result<LibraryEntrySummary, String>;

#[tauri::command]
fn delete_recording(id: String, state: State<AppState>) -> Result<(), String>;

#[tauri::command]
fn get_recording_context(id: String, state: State<AppState>) -> Result<RecordingContextPayload, String>;
```

### 3.4 自动注册

在 `MacRecordingService::stop()` 完成后调用：

```rust
fn stop(&mut self) -> AppResult<StopRecordingResponse> {
    // ... 现有的停止逻辑 ...

    // 新增：注册到历史库
    let mut library = self.library.lock()?;
    library.register(&result)?;

    Ok(response)
}
```

### 3.5 导入校验流程

```
用户选择 .mp4 文件
    ↓
解析文件名 "recording-{millis}-{seq}.mp4"
    ↓
在同目录下查找四个元数据文件：
  - cursor-metadata-{millis}-{seq}.json
  - cursor-effects-{millis}-{seq}.json
  - trim-metadata-{millis}-{seq}.json
  - cut-timeline-{millis}-{seq}.json
    ↓
验证所有文件存在
    ↓
验证元数据可解析
    ↓
检查是否已在索引中
    ↓
注册到索引
```

### 3.6 索引修复

应用启动时检查索引一致性：

```rust
pub fn repair_on_startup(&mut self) {
    // 移除指向不存在文件的条目
    self.index.entries.retain(|entry| {
        entry.video_path.exists()
            && entry.cursor_metadata_path.exists()
            && entry.effect_timeline_path.exists()
            && entry.trim_metadata_path.exists()
            && entry.cut_timeline_path.exists()
    });
    let _ = self.save_index();
}
```

---

## 4. 前端组件设计

### 4.1 组件结构

```
src/
├── components/
│   ├── recording-panel.tsx        # 现有录制面板（不变）
│   ├── recording-sidebar.tsx      # 新增：历史录制侧边栏
│   ├── recording-history-item.tsx # 新增：单条历史记录卡片
│   └── import-button.tsx          # 新增：导入按钮
├── lib/
│   └── tauri.ts                   # 新增 API 函数
```

### 4.2 侧边栏组件

```tsx
interface RecordingSidebarProps {
  isOpen: boolean
  onToggle: () => void
  onSelectRecording: (id: string) => void
}

export function RecordingSidebar({ isOpen, onToggle, onSelectRecording }: RecordingSidebarProps) {
  const [recordings, setRecordings] = useState<LibraryEntrySummary[]>([])

  useEffect(() => {
    if (isOpen) {
      listRecordings().then(setRecordings)
    }
  }, [isOpen])

  // ... 渲染逻辑
}
```

### 4.3 App.tsx 集成

```tsx
export default function App() {
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [recordingResult, setRecordingResult] = useState<RecordingResult | null>(null)

  const handleSelectRecording = useCallback(async (id: string) => {
    const context = await getRecordingContext(id)
    setRecordingResult({
      outputPath: context.videoPath,
      cursorMetadataPath: context.cursorMetadataPath,
      effectTimelinePath: context.effectTimelinePath,
      trimMetadataPath: context.trimMetadataPath,
      cutTimelinePath: context.cutTimelinePath,
    })
    setAppState('preview')
  }, [])

  // Idle state
  if (appState === 'idle') {
    return (
      <div className="min-h-screen flex">
        <div className="flex-1 flex items-center justify-center p-8">
          <RecordingPanel ... />
        </div>
        <RecordingSidebar
          isOpen={sidebarOpen}
          onToggle={() => setSidebarOpen(!sidebarOpen)}
          onSelectRecording={handleSelectRecording}
        />
      </div>
    )
  }
}
```

---

## 5. 状态转换流程

### 5.1 状态图

```
                                    ┌──────────────────┐
                                    │                  │
                    ┌───────────────┤     idle         │◄─────────────────┐
                    │               │                  │                  │
                    │               └────────┬─────────┘                  │
                    │                        │                            │
                    │    点击"开始录制"       │ 选择历史录制               │ 点击"返回录制"
                    │                        ▼                            │
                    │               ┌──────────────────┐                  │
                    │               │                  │                  │
                    │               │    recording     │                  │
                    │               │                  │                  │
                    │               └────────┬─────────┘                  │
                    │                        │                            │
                    │    停止录制             │                            │
                    │                        ▼                            │
                    │               ┌──────────────────┐                  │
                    │               │                  │                  │
                    │               │     preview      │──────────────────┘
                    │               │                  │
                    │               └────────┬─────────┘
                    │                        │
                    │    点击"导出"           │
                    │                        ▼
                    │               ┌──────────────────┐
                    │               │                  │
                    └───────────────│   processing     │
                                    │                  │
                                    └──────────────────┘
```

### 5.2 关键流程

**流程 A：录制 → 美化 → 返回空闲**
1. 用户点击"开始录制" → `appState = 'recording'`
2. 用户点击"停止" → 后端 `stop()` → 写入元数据 → 自动注册到 Library → `appState = 'preview'`
3. 用户在预览界面美化、导出...
4. 用户点击"返回录制" → `appState = 'idle'` → 元数据文件保留在磁盘，Library 中仍有记录
5. 用户再次打开侧边栏 → `listRecordings()` → 显示刚才的录制 → 点击可重新进入美化界面

**流程 B：导入外部录制**
1. 用户打开侧边栏，点击"导入" → 打开文件选择器
2. 用户选择 .mp4 文件 → 后端校验文件名 + 元数据 → 注册到 Library
3. 用户点击刚导入的录制 → `getRecordingContext(id)` → `appState = 'preview'`

**流程 C：删除历史录制**
1. 用户悬停某条记录，点击删除图标 → 确认对话框
2. 用户确认 → 删除所有关联文件 → 从索引移除 → 列表刷新

---

## 6. 错误处理

### 6.1 新增错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("导入失败: {reason}")]
    ImportFailed { reason: String },

    #[error("录制未找到: {0}")]
    RecordingNotFound(String),

    #[error("索引损坏: {reason}")]
    IndexCorrupted { reason: String },
}
```

### 6.2 边界情况处理

| 场景 | 处理方式 |
|------|----------|
| 索引文件损坏 | 重建索引：扫描目录，解析文件名重新注册 |
| 元数据文件丢失 | 从索引移除该条目，提示用户"录制文件不完整" |
| 视频文件丢失 | 从索引移除该条目，提示用户"录制文件已删除" |
| 导入重复文件 | 提示"该录制已在历史记录中" |
| 导入文件名格式错误 | 提示"非本应用录制的文件" |
| 导入缺少元数据 | 提示"缺少配套文件: xxx.json" |

---

## 7. 实现计划

### 7.1 新增/修改文件

| 模块 | 操作 | 关键内容 |
|------|------|----------|
| `recording_library.rs` | **新增** | 索引管理、注册、导入、删除、修复 |
| `recording_library_test.rs` | **新增** | 单元测试 |
| `lib.rs` | 修改 | 新增 4 个 Tauri 命令 |
| `macos_service.rs` | 修改 | 录制完成时自动注册 |
| `error.rs` | 修改 | 新增错误类型 |
| `tauri.ts` | 修改 | 新增 API 函数和类型 |
| `recording-sidebar.tsx` | **新增** | 侧边栏组件 |
| `recording-history-item.tsx` | **新增** | 历史记录卡片 |
| `import-button.tsx` | **新增** | 导入按钮组件 |
| `App.tsx` | 修改 | 集成侧边栏，修改状态转换 |

### 7.2 开发顺序

1. 后端：`recording_library.rs` + 测试
2. 后端：Tauri 命令 + 集成到 `macos_service.rs`
3. 前端：API 函数 + 类型定义
4. 前端：侧边栏组件
5. 前端：导入功能
6. 集成测试

---

## 8. 测试策略

### 8.1 后端单元测试

- 索引的创建、加载、保存
- 注册录制
- 导入校验（正常流程 + 各种错误情况）
- 删除录制
- 索引修复

### 8.2 前端测试

- 侧边栏展开/收起
- 列表加载和显示
- 导入流程（mock Tauri 命令）
- 错误提示显示

---

## 9. 待确认事项

无。
