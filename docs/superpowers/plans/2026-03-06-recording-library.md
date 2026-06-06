# Recording Library & Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a recording history library with auto-registration on stop, sidebar UI for browsing past recordings, and file-picker import with strict validation — enabling users to re-enter the beautify/export workflow for any past recording.

**Architecture:** A new `RecordingLibrary` module manages a JSON index file (`recordings-index.json`) in the app data directory. On recording stop, the result is auto-registered. The idle view gains a collapsible sidebar listing past recordings, plus an import button that validates file name + companion metadata files before registering.

**Tech Stack:** Rust (serde_json, std::fs), Tauri commands, React + TypeScript + Tailwind (shadcn/ui Button, existing components)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src-tauri/src/app/recording_library.rs` | **Create** | Index CRUD, import validation, startup repair |
| `src-tauri/src/app/error.rs` | Modify | Add 3 new `AppError` variants |
| `src-tauri/src/app/mod.rs` | Modify | Register `recording_library` module |
| `src-tauri/src/lib.rs` | Modify | Add `library` to `AppState`, 4 Tauri commands, startup repair call |
| `src-tauri/src/platform/macos_service.rs` | Modify | Auto-register after `stop()` |
| `src/lib/tauri.ts` | Modify | Add `LibraryEntrySummary` type + 4 API functions |
| `src/components/recording-sidebar.tsx` | **Create** | Sidebar with history list, import button, delete |
| `src/App.tsx` | Modify | Add sidebar state, `handleSelectRecording`, integrate sidebar in idle view |

---

### Task 1: Backend Error Types

**Files:**
- Modify: `src-tauri/src/app/error.rs`

- [ ] **Step 1: Add new error variants**

Add these three variants to the `AppError` enum (after `LicenseFailed`):

```rust
ImportFailed { reason: String },
RecordingNotFound(String),
IndexCorrupted { reason: String },
```

- [ ] **Step 2: Add Display arms**

In the `Display` impl (after the `LicenseFailed` arm), add:

```rust
Self::ImportFailed { reason } => write!(f, "导入失败: {reason}"),
Self::RecordingNotFound(id) => write!(f, "录制未找到: {id}"),
Self::IndexCorrupted { reason } => write!(f, "索引损坏: {reason}"),
```

- [ ] **Step 3: Add test cases**

Add to the existing test module (after the `license_failed_chinese_message` test):

```rust
#[test]
fn import_failed_chinese_message() {
    let err = AppError::ImportFailed {
        reason: "非本应用录制的文件".to_string(),
    };
    assert_eq!(err.to_string(), "导入失败: 非本应用录制的文件");
}

#[test]
fn recording_not_found_chinese_message() {
    let err = AppError::RecordingNotFound("rec-123".to_string());
    assert_eq!(err.to_string(), "录制未找到: rec-123");
}

#[test]
fn index_corrupted_chinese_message() {
    let err = AppError::IndexCorrupted {
        reason: "JSON 解析失败".to_string(),
    };
    assert_eq!(err.to_string(), "索引损坏: JSON 解析失败");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib app::error`
Expected: All error tests pass (including new ones).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/error.rs
git commit -m "feat(app): 新增录制库相关错误类型"
```

---

### Task 2: RecordingLibrary Core — Data Structures & Index I/O

**Files:**
- Create: `src-tauri/src/app/recording_library.rs`
- Modify: `src-tauri/src/app/mod.rs`

- [ ] **Step 1: Create module file with data structures**

Create `src-tauri/src/app/recording_library.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};

const INDEX_VERSION: u32 = 1;

/// Persisted index file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordingIndex {
    version: u32,
    entries: Vec<LibraryEntry>,
}

/// A single recording entry in the library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub id: String,
    pub created_at: u64,
    pub duration_secs: f64,
    pub video_path: PathBuf,
    pub cursor_metadata_path: PathBuf,
    pub effect_timeline_path: PathBuf,
    pub trim_metadata_path: PathBuf,
    pub cut_timeline_path: PathBuf,
}

/// Lightweight summary sent to the frontend (no file paths).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntrySummary {
    pub id: String,
    pub created_at: u64,
    pub duration_secs: f64,
}

/// Full context for re-entering the beautify workflow.
pub struct RecordingContext {
    pub entry: LibraryEntry,
    pub metadata_json: String,
    pub effect_timeline_json: String,
    pub cut_timeline_json: String,
}

/// Payload serialized to the frontend for `get_recording_context`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingContextPayload {
    pub video_path: String,
    pub cursor_metadata_path: String,
    pub effect_timeline_path: String,
    pub trim_metadata_path: String,
    pub cut_timeline_path: String,
    pub metadata_json: String,
    pub effect_timeline_json: String,
    pub cut_timeline_json: String,
}

pub struct RecordingLibrary {
    index_path: PathBuf,
    index: RecordingIndex,
}
```

- [ ] **Step 2: Implement index load/save**

Add to `RecordingLibrary`:

```rust
impl RecordingLibrary {
    /// Load existing index from disk, or create a fresh empty index.
    pub fn new(app_data_dir: &Path) -> Self {
        let index_path = app_data_dir.join("recordings-index.json");
        let index = Self::load_index(&index_path).unwrap_or(RecordingIndex {
            version: INDEX_VERSION,
            entries: Vec::new(),
        });
        Self { index_path, index }
    }

    fn load_index(path: &Path) -> Option<RecordingIndex> {
        let content = fs::read_to_string(path).ok()?;
        let idx: RecordingIndex = serde_json::from_str(&content).ok()?;
        if idx.version != INDEX_VERSION {
            return None;
        }
        Some(idx)
    }

    fn save_index(&self) -> AppResult<()> {
        if let Some(parent) = self.index_path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::IndexCorrupted {
                reason: format!("创建索引目录失败: {e}"),
            })?;
        }
        let json = serde_json::to_string_pretty(&self.index).map_err(|e| AppError::IndexCorrupted {
            reason: format!("序列化索引失败: {e}"),
        })?;
        fs::write(&self.index_path, json).map_err(|e| AppError::IndexCorrupted {
            reason: format!("写入索引文件失败: {e}"),
        })?;
        Ok(())
    }

    fn find_entry(&self, id: &str) -> AppResult<&LibraryEntry> {
        self.index
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| AppError::RecordingNotFound(id.to_string()))
    }
}
```

- [ ] **Step 3: Register module in mod.rs**

Add to `src-tauri/src/app/mod.rs`:

```rust
pub mod recording_library;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/recording_library.rs src-tauri/src/app/mod.rs
git commit -m "feat(app): 新建 RecordingLibrary 模块，实现索引数据结构与 I/O"
```

---

### Task 3: RecordingLibrary — list, register, delete

**Files:**
- Modify: `src-tauri/src/app/recording_library.rs`

- [ ] **Step 1: Implement `list`**

```rust
impl RecordingLibrary {
    /// Return summaries of all entries for the sidebar.
    pub fn list(&self) -> Vec<LibraryEntrySummary> {
        self.index
            .entries
            .iter()
            .map(|e| LibraryEntrySummary {
                id: e.id.clone(),
                created_at: e.created_at,
                duration_secs: e.duration_secs,
            })
            .collect()
    }
}
```

- [ ] **Step 2: Implement `register`**

```rust
impl RecordingLibrary {
    /// Register a completed recording into the library index.
    pub fn register(
        &mut self,
        id: &str,
        created_at: u64,
        duration_secs: f64,
        video_path: &str,
        cursor_metadata_path: Option<&str>,
        effect_timeline_path: Option<&str>,
        trim_metadata_path: Option<&str>,
        cut_timeline_path: Option<&str>,
    ) -> AppResult<()> {
        // All 5 files must exist for a valid entry.
        let vp = PathBuf::from(video_path);
        if !vp.exists() {
            return Err(AppError::ImportFailed {
                reason: "录制视频文件不存在".to_string(),
            });
        }

        let cmp = cursor_metadata_path
            .map(PathBuf::from)
            .ok_or_else(|| AppError::ImportFailed {
                reason: "光标元数据路径缺失".to_string(),
            })?;
        let etp = effect_timeline_path
            .map(PathBuf::from)
            .ok_or_else(|| AppError::ImportFailed {
                reason: "效果时间线路径缺失".to_string(),
            })?;
        let tmp = trim_metadata_path
            .map(PathBuf::from)
            .ok_or_else(|| AppError::ImportFailed {
                reason: "裁剪元数据路径缺失".to_string(),
            })?;
        let ctp = cut_timeline_path
            .map(PathBuf::from)
            .ok_or_else(|| AppError::ImportFailed {
                reason: "裁剪时间线路径缺失".to_string(),
            })?;

        for p in [&cmp, &etp, &tmp, &ctp] {
            if !p.exists() {
                return Err(AppError::ImportFailed {
                    reason: format!("缺少配套文件: {}", p.display()),
                });
            }
        }

        // Duplicate check.
        if self.index.entries.iter().any(|e| e.id == id) {
            return Ok(()); // Already registered, idempotent.
        }

        let entry = LibraryEntry {
            id: id.to_string(),
            created_at,
            duration_secs,
            video_path: vp,
            cursor_metadata_path: cmp,
            effect_timeline_path: etp,
            trim_metadata_path: tmp,
            cut_timeline_path: ctp,
        };

        self.index.entries.push(entry);
        self.save_index()
    }
}
```

- [ ] **Step 3: Implement `delete`**

```rust
impl RecordingLibrary {
    /// Remove an entry from the index and optionally delete its files.
    pub fn delete(&mut self, id: &str, delete_files: bool) -> AppResult<()> {
        let entry = self.find_entry(id)?.clone();

        if delete_files {
            let files = [
                &entry.video_path,
                &entry.cursor_metadata_path,
                &entry.effect_timeline_path,
                &entry.trim_metadata_path,
                &entry.cut_timeline_path,
            ];
            for file in &files {
                let _ = fs::remove_file(file);
            }
        }

        self.index.entries.retain(|e| e.id != id);
        self.save_index()
    }
}
```

- [ ] **Step 4: Implement `get_recording`**

```rust
impl RecordingLibrary {
    /// Load full context for re-entering the beautify workflow.
    pub fn get_recording(&self, id: &str) -> AppResult<RecordingContext> {
        let entry = self.find_entry(id)?.clone();

        let metadata_json = fs::read_to_string(&entry.cursor_metadata_path).map_err(|e| {
            AppError::ImportFailed {
                reason: format!("读取光标元数据失败: {e}"),
            }
        })?;

        let effect_timeline_json =
            fs::read_to_string(&entry.effect_timeline_path).map_err(|e| {
                AppError::ImportFailed {
                    reason: format!("读取效果时间线失败: {e}"),
                }
            })?;

        let cut_timeline_json =
            fs::read_to_string(&entry.cut_timeline_path).map_err(|e| {
                AppError::ImportFailed {
                    reason: format!("读取裁剪时间线失败: {e}"),
                }
            })?;

        Ok(RecordingContext {
            entry,
            metadata_json,
            effect_timeline_json,
            cut_timeline_json,
        })
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/app/recording_library.rs
git commit -m "feat(app): 实现 RecordingLibrary 的 list/register/delete/get_recording"
```

---

### Task 4: RecordingLibrary — Import with Validation

**Files:**
- Modify: `src-tauri/src/app/recording_library.rs`

- [ ] **Step 1: Implement filename parser**

```rust
impl RecordingLibrary {
    /// Parse "recording-{millis}-{seq}" from a file stem.
    fn parse_recording_filename(path: &Path) -> AppResult<(u64, u64)> {
        let stem = path
            .file_stem()
            .and_then(|f| f.to_str())
            .ok_or_else(|| AppError::ImportFailed {
                reason: "无效的文件名".to_string(),
            })?;

        let parts: Vec<&str> = stem.split('-').collect();
        if parts.len() < 3 || parts[0] != "recording" {
            return Err(AppError::ImportFailed {
                reason: "非本应用录制的文件".to_string(),
            });
        }

        let millis = parts[1].parse::<u64>().map_err(|_| AppError::ImportFailed {
            reason: "文件名格式错误".to_string(),
        })?;
        let seq = parts[2].parse::<u64>().map_err(|_| AppError::ImportFailed {
            reason: "文件名格式错误".to_string(),
        })?;

        Ok((millis, seq))
    }
}
```

- [ ] **Step 2: Implement `import`**

```rust
impl RecordingLibrary {
    /// Validate and import an external MP4 file produced by this app.
    pub fn import(&mut self, video_path: &Path) -> AppResult<LibraryEntrySummary> {
        let (millis, seq) = Self::parse_recording_filename(video_path)?;

        let dir = video_path.parent().unwrap_or(Path::new("."));
        let cursor_metadata_path = dir.join(format!("cursor-metadata-{millis}-{seq}.json"));
        let effect_timeline_path = dir.join(format!("cursor-effects-{millis}-{seq}.json"));
        let trim_metadata_path = dir.join(format!("trim-metadata-{millis}-{seq}.json"));
        let cut_timeline_path = dir.join(format!("cut-timeline-{millis}-{seq}.json"));

        let companion_paths = [
            &cursor_metadata_path,
            &effect_timeline_path,
            &trim_metadata_path,
            &cut_timeline_path,
        ];
        for p in &companion_paths {
            if !p.exists() {
                return Err(AppError::ImportFailed {
                    reason: format!("缺少配套文件: {}", p.display()),
                });
            }
        }

        // Validate metadata is parseable.
        let _meta = crate::media::recording_metadata::RecordingMetadataWriter::read_metadata(
            &cursor_metadata_path,
        )
        .map_err(|e| AppError::ImportFailed {
            reason: format!("元数据文件损坏: {e}"),
        })?;

        let id = format!("rec-{millis}-{seq}");

        // Duplicate check.
        if self.index.entries.iter().any(|e| e.id == id) {
            return Err(AppError::ImportFailed {
                reason: "该录制已在历史记录中".to_string(),
            });
        }

        let duration_secs = _meta.duration_nanos as f64 / 1_000_000_000.0;

        let entry = LibraryEntry {
            id: id.clone(),
            created_at: millis,
            duration_secs,
            video_path: video_path.to_path_buf(),
            cursor_metadata_path,
            effect_timeline_path,
            trim_metadata_path,
            cut_timeline_path,
        };

        self.index.entries.push(entry.clone());
        self.save_index()?;

        Ok(LibraryEntrySummary {
            id: entry.id,
            created_at: entry.created_at,
            duration_secs: entry.duration_secs,
        })
    }
}
```

- [ ] **Step 3: Implement `repair_on_startup`**

```rust
impl RecordingLibrary {
    /// Remove entries whose files no longer exist.
    pub fn repair_on_startup(&mut self) {
        let before = self.index.entries.len();
        self.index.entries.retain(|entry| {
            entry.video_path.exists()
                && entry.cursor_metadata_path.exists()
                && entry.effect_timeline_path.exists()
                && entry.trim_metadata_path.exists()
                && entry.cut_timeline_path.exists()
        });
        let removed = before - self.index.entries.len();
        if removed > 0 {
            log::warn!("启动时移除 {removed} 条无效索引条目");
            let _ = self.save_index();
        }
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/app/recording_library.rs
git commit -m "feat(app): 实现 RecordingLibrary 导入校验与启动修复"
```

---

### Task 5: RecordingLibrary Unit Tests

**Files:**
- Modify: `src-tauri/src/app/recording_library.rs`

- [ ] **Step 1: Add test module**

Append to `recording_library.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luzhi-lib-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn new_library_creates_empty_index() {
        let dir = temp_dir();
        let lib = RecordingLibrary::new(&dir);
        assert!(lib.list().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn list_returns_empty_for_fresh_library() {
        let dir = temp_dir();
        let lib = RecordingLibrary::new(&dir);
        assert_eq!(lib.list().len(), 0);
        cleanup(&dir);
    }

    #[test]
    fn parse_recording_filename_valid() {
        let path = Path::new("/tmp/luzhi-recordings/recording-1717000000000-0.mp4");
        let (millis, seq) = RecordingLibrary::parse_recording_filename(path).unwrap();
        assert_eq!(millis, 1717000000000);
        assert_eq!(seq, 0);
    }

    #[test]
    fn parse_recording_filename_invalid_prefix() {
        let path = Path::new("/tmp/video-123-0.mp4");
        assert!(RecordingLibrary::parse_recording_filename(path).is_err());
    }

    #[test]
    fn parse_recording_filename_invalid_number() {
        let path = Path::new("/tmp/recording-abc-0.mp4");
        assert!(RecordingLibrary::parse_recording_filename(path).is_err());
    }

    #[test]
    fn delete_removes_entry_from_index() {
        let dir = temp_dir();
        // Create dummy files.
        let video = dir.join("recording-1000-0.mp4");
        let cm = dir.join("cursor-metadata-1000-0.json");
        let et = dir.join("cursor-effects-1000-0.json");
        let tm = dir.join("trim-metadata-1000-0.json");
        let ct = dir.join("cut-timeline-1000-0.json");
        for f in [&video, &cm, &et, &tm, &ct] {
            fs::write(f, "{}").unwrap();
        }

        let mut lib = RecordingLibrary::new(&dir);
        lib.register("rec-1000-0", 1000, 10.0,
            video.to_str().unwrap(),
            Some(cm.to_str().unwrap()),
            Some(et.to_str().unwrap()),
            Some(tm.to_str().unwrap()),
            Some(ct.to_str().unwrap()),
        ).unwrap();

        assert_eq!(lib.list().len(), 1);

        lib.delete("rec-1000-0", false).unwrap();
        assert!(lib.list().is_empty());

        cleanup(&dir);
    }

    #[test]
    fn get_recording_not_found_returns_error() {
        let dir = temp_dir();
        let lib = RecordingLibrary::new(&dir);
        let result = lib.get_recording("nonexistent");
        assert!(matches!(result, Err(AppError::RecordingNotFound(_))));
        cleanup(&dir);
    }

    #[test]
    fn repair_on_startup_removes_missing_files() {
        let dir = temp_dir();
        let mut lib = RecordingLibrary::new(&dir);

        // Manually insert an entry with non-existent files.
        lib.index.entries.push(LibraryEntry {
            id: "rec-ghost-0".to_string(),
            created_at: 0,
            duration_secs: 1.0,
            video_path: dir.join("nonexistent.mp4"),
            cursor_metadata_path: dir.join("nonexistent.json"),
            effect_timeline_path: dir.join("nonexistent2.json"),
            trim_metadata_path: dir.join("nonexistent3.json"),
            cut_timeline_path: dir.join("nonexistent4.json"),
        });
        lib.save_index().unwrap();

        lib.repair_on_startup();
        assert!(lib.list().is_empty());

        cleanup(&dir);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib app::recording_library`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/app/recording_library.rs
git commit -m "test(app): RecordingLibrary 单元测试"
```

---

### Task 6: Tauri Commands & AppState Integration

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `library` field to `AppState`**

In the `AppState` struct, add after the last field:

```rust
library: Arc<Mutex<RecordingLibrary>>,
```

In `Default::default()`, add after the last field initialization (use temp dir as placeholder, will be re-initialized in `setup()`):

```rust
library: Arc::new(Mutex::new(RecordingLibrary::new(
    &std::env::temp_dir().join("luzhi-recordings"),
))),
```

Add an `init_library` method to `AppState`:

```rust
impl AppState {
    fn init_library(&self, app_data_dir: &Path) {
        let mut lib = self.library.lock().expect("library lock poisoned");
        *lib = RecordingLibrary::new(app_data_dir);
        lib.repair_on_startup();
    }
}
```

- [ ] **Step 2: Add 4 Tauri commands**

Add these commands to `lib.rs` (near the other commands):

```rust
#[tauri::command]
fn list_recordings(state: tauri::State<'_, AppState>) -> Result<Vec<LibraryEntrySummary>, String> {
    let library = state.library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
    Ok(library.list())
}

#[tauri::command]
fn get_recording_context(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<RecordingContextPayload, String> {
    let library = state.library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
    let ctx = library.get_recording(&id).map_err(|e| e.to_string())?;
    Ok(RecordingContextPayload {
        video_path: ctx.entry.video_path.to_string_lossy().to_string(),
        cursor_metadata_path: ctx.entry.cursor_metadata_path.to_string_lossy().to_string(),
        effect_timeline_path: ctx.entry.effect_timeline_path.to_string_lossy().to_string(),
        trim_metadata_path: ctx.entry.trim_metadata_path.to_string_lossy().to_string(),
        cut_timeline_path: ctx.entry.cut_timeline_path.to_string_lossy().to_string(),
        metadata_json: ctx.metadata_json,
        effect_timeline_json: ctx.effect_timeline_json,
        cut_timeline_json: ctx.cut_timeline_json,
    })
}

#[tauri::command]
fn import_recording(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<LibraryEntrySummary, String> {
    let mut library = state.library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
    let entry = library.import(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(entry)
}

#[tauri::command]
fn delete_recording(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut library = state.library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
    library.delete(&id, true).map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 3: Register commands in invoke_handler**

Add to the `generate_handler!` macro:

```rust
list_recordings,
get_recording_context,
import_recording,
delete_recording,
```

- [ ] **Step 4: Initialize library in `run()`**

Add a `.setup()` hook before `.run()` in the existing `run()` function:

```rust
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            // ... all existing commands ...
            list_recordings,
            get_recording_context,
            import_recording,
            delete_recording,
        ])
        .setup(|app| {
            // Initialize recording library with proper app data dir.
            if let Ok(data_dir) = app.path().app_data_dir() {
                let state = app.state::<AppState>();
                state.init_library(&data_dir);
            }
            Ok(())
        })
        .run(tauri::generate_context())?;

    Ok(())
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(app): 注册录制库 Tauri 命令并集成到 AppState"
```

---

### Task 7: Auto-Register on Recording Stop

**Files:**
- Modify: `src-tauri/src/platform/macos_service.rs`
- Modify: `src-tauri/src/lib.rs` (stop_recording command)

- [ ] **Step 1: Add library parameter to stop_recording**

Modify the `stop_recording` Tauri command to also register the result:

```rust
#[tauri::command]
fn stop_recording(state: tauri::State<'_, AppState>) -> Result<StopRecordingResponse, String> {
    let response = {
        let mut svc = state.service.lock().map_err(|_| "录制服务锁已损坏".to_string())?;
        svc.stop().map_err(|e| e.to_string())?
    };

    // Auto-register to library on successful stop.
    if !response.failed {
        if let (Some(ref video_path), Some(ref cursor_path)) =
            (&response.result.output_path, &response.result.cursor_metadata_path)
        {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let mut library = state.library.lock().map_err(|_| "录制库锁已损坏".to_string())?;
            let register_result = library.register(
                &format!("rec-{now_ms}-{}", response.result.frame_count),
                now_ms,
                response.result.duration_secs as f64,
                video_path,
                Some(cursor_path),
                response.result.effect_timeline_path.as_deref(),
                response.result.trim_metadata_path.as_deref(),
                response.result.cut_timeline_path.as_deref(),
            );
            if let Err(e) = register_result {
                log::warn!("自动注册录制到历史库失败: {e}");
                // Non-fatal: recording still succeeded.
            }
        }
    }

    Ok(response)
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(app): 录制停止时自动注册到历史库"
```

---

### Task 8: Frontend API Functions & Types

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add types**

Add after the existing `LicenseStatus` type:

```typescript
/** Lightweight summary of a recording in the library. */
export type LibraryEntrySummary = {
  id: string
  createdAt: number
  durationSecs: number
}

/** Full context for re-entering the beautify workflow. */
export type RecordingContextPayload = {
  videoPath: string
  cursorMetadataPath: string
  effectTimelinePath: string
  trimMetadataPath: string
  cutTimelinePath: string
  metadataJson: string
  effectTimelineJson: string
  cutTimelineJson: string
}
```

- [ ] **Step 2: Add API functions**

Add after the existing `activateLicense` function:

```typescript
export async function listRecordings(): Promise<LibraryEntrySummary[]> {
  return invoke<LibraryEntrySummary[]>('list_recordings')
}

export async function getRecordingContext(id: string): Promise<RecordingContextPayload> {
  return invoke<RecordingContextPayload>('get_recording_context', { id })
}

export async function importRecording(path: string): Promise<LibraryEntrySummary> {
  return invoke<LibraryEntrySummary>('import_recording', { path })
}

export async function deleteRecording(id: string): Promise<void> {
  return invoke('delete_recording', { id })
}
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `npm run build`
Expected: No TypeScript errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(ui): 新增录制库 API 函数和类型定义"
```

---

### Task 9: Recording Sidebar Component

**Files:**
- Create: `src/components/recording-sidebar.tsx`

- [ ] **Step 1: Create the sidebar component**

```tsx
import { useState, useEffect, useCallback } from 'react'
import { History, X, Upload, Trash2, ChevronRight, Loader2, Film } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  listRecordings,
  deleteRecording,
  importRecording,
  type LibraryEntrySummary,
} from '@/lib/tauri'

interface RecordingSidebarProps {
  isOpen: boolean
  onToggle: () => void
  onSelectRecording: (id: string) => void
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

function formatDate(timestamp: number): string {
  const date = new Date(timestamp)
  return date.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function RecordingSidebar({
  isOpen,
  onToggle,
  onSelectRecording,
}: RecordingSidebarProps) {
  const [recordings, setRecordings] = useState<LibraryEntrySummary[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [importError, setImportError] = useState<string | null>(null)

  const loadRecordings = useCallback(async () => {
    setIsLoading(true)
    try {
      const list = await listRecordings()
      // Sort by createdAt descending (newest first).
      setRecordings(list.sort((a, b) => b.createdAt - a.createdAt))
    } catch (e) {
      console.error('加载历史录制失败:', e)
    } finally {
      setIsLoading(false)
    }
  }, [])

  useEffect(() => {
    if (isOpen) {
      void loadRecordings()
    }
  }, [isOpen, loadRecordings])

  const handleImport = async () => {
    setImportError(null)
    try {
      // Dynamically import tauri plugin for file dialog.
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        multiple: false,
        filters: [{ name: '视频文件', extensions: ['mp4'] }],
      })
      if (!selected) return

      await importRecording(selected)
      await loadRecordings()
    } catch (e) {
      const msg = String(e)
      setImportError(msg)
      setTimeout(() => setImportError(null), 5000)
    }
  }

  const handleDelete = async (id: string) => {
    setDeletingId(id)
    try {
      await deleteRecording(id)
      setRecordings((prev) => prev.filter((r) => r.id !== id))
    } catch (e) {
      console.error('删除录制失败:', e)
    } finally {
      setDeletingId(null)
    }
  }

  return (
    <>
      {/* Toggle button — always visible */}
      <button
        onClick={onToggle}
        className={cn(
          'fixed top-1/2 -translate-y-1/2 z-40 bg-card border border-border/50 rounded-l-lg p-2',
          'hover:bg-secondary transition-all duration-200',
          isOpen ? 'right-80' : 'right-0',
        )}
        title="历史录制"
      >
        <History className="w-4 h-4" />
      </button>

      {/* Sidebar panel */}
      <div
        className={cn(
          'fixed right-0 top-0 h-full bg-card border-l border-border/50',
          'transition-all duration-300 ease-in-out z-30 flex flex-col',
          isOpen ? 'w-80' : 'w-0 overflow-hidden',
        )}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-border/50 shrink-0">
          <h3 className="font-semibold text-sm">历史录制</h3>
          <div className="flex items-center gap-1">
            <Button variant="ghost" size="sm" onClick={handleImport} title="导入录制">
              <Upload className="w-4 h-4" />
            </Button>
            <Button variant="ghost" size="sm" onClick={onToggle}>
              <X className="w-4 h-4" />
            </Button>
          </div>
        </div>

        {/* Error banner */}
        {importError && (
          <div className="mx-3 mt-2 p-2 rounded-lg bg-destructive/10 border border-destructive/20 text-xs text-destructive">
            {importError}
          </div>
        )}

        {/* List */}
        <div className="flex-1 overflow-y-auto p-2">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          ) : recordings.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
              <Film className="w-8 h-8 mb-2 opacity-40" />
              <p className="text-xs">暂无历史录制</p>
            </div>
          ) : (
            recordings.map((rec) => (
              <div
                key={rec.id}
                className="group relative bg-secondary/50 rounded-lg p-3 mb-2 hover:bg-secondary cursor-pointer"
                onClick={() => onSelectRecording(rec.id)}
              >
                <div className="flex justify-between items-start">
                  <div>
                    <p className="text-sm font-medium">{formatDate(rec.createdAt)}</p>
                    <p className="text-xs text-muted-foreground">
                      时长: {formatDuration(rec.durationSecs)}
                    </p>
                  </div>
                  <ChevronRight className="w-4 h-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </div>

                {/* Delete button */}
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    void handleDelete(rec.id)
                  }}
                  disabled={deletingId === rec.id}
                  className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive transition-all p-1"
                  title="删除"
                >
                  {deletingId === rec.id ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Trash2 className="w-3 h-3" />
                  )}
                </button>
              </div>
            ))
          )}
        </div>
      </div>
    </>
  )
}
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `npm run build`
Expected: No TypeScript errors. Note: `@tauri-apps/plugin-dialog` may need to be installed.

- [ ] **Step 3: Install dialog plugin if needed**

Run: `npm install @tauri-apps/plugin-dialog`
Then add to `src-tauri/Cargo.toml`:
```toml
tauri-plugin-dialog = "2"
```
And in `lib.rs` `run()`:
```rust
.plugin(tauri_plugin_dialog::init())
```

- [ ] **Step 4: Commit**

```bash
git add src/components/recording-sidebar.tsx package.json
git commit -m "feat(ui): 新增历史录制侧边栏组件"
```

---

### Task 10: App.tsx Integration

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add imports and state**

Add to imports:

```typescript
import { RecordingSidebar } from '@/components/recording-sidebar'
import { getRecordingContext } from '@/lib/tauri'
```

Add state in the `App` component (after `licenseStatus`):

```typescript
const [sidebarOpen, setSidebarOpen] = useState(false)
```

- [ ] **Step 2: Add `handleSelectRecording` callback**

Add after `handleRetry`:

```typescript
const handleSelectRecording = useCallback(async (id: string) => {
  try {
    const ctx = await getRecordingContext(id)
    // Build a RecordingResult with paths from the library context.
    // Diagnostics are zeroed since this is a re-open, not a fresh recording.
    const result = {
      durationSecs: 0,
      frameCount: 0,
      mixedAudioChunkCount: 0,
      outputPath: ctx.videoPath,
      cursorMetadataPath: ctx.cursorMetadataPath,
      effectTimelinePath: ctx.effectTimelinePath,
      trimMetadataPath: ctx.trimMetadataPath,
      cutTimelinePath: ctx.cutTimelinePath,
      writerDiagnostics: {
        audioChunksReceived: 0,
        audioChunksAppended: 0,
        audioChunksDiscardedFullOverlap: 0,
        audioChunksTrimmedPartialOverlap: 0,
        audioRealFramesAppended: 0,
        audioSilenceFramesPadded: 0,
        audioRealRmsMaxBeforeEncode: 0,
        aacFramesEncoded: 0,
        silentAacFramesEncoded: 0,
        generatedSilentTrack: false,
        videoQueueFullCount: 0,
        audioQueueFullCount: 0,
        systemChunksReceivedByWriter: 0,
        micChunksReceivedByWriter: 0,
      },
      diagnostics: {
        requestedSystemAudio: false,
        requestedMicrophone: false,
        microphoneDevice: null,
        systemChunksReceived: 0,
        micChunksReceived: 0,
        systemChunksDropped: 0,
        micChunksDropped: 0,
        mixedChunksQueued: 0,
        writerPushAudioFailures: 0,
        systemRmsMax: 0,
        micRmsMax: 0,
        mixedRmsMax: 0,
        generatedSilentTrack: false,
        pairedWindowCount: 0,
        systemOnlyWindowCount: 0,
        micOnlyWindowCount: 0,
        sourceTimeoutWindowCount: 0,
        systemRmsMaxBeforeWriter: 0,
        micRmsMaxBeforeWriter: 0,
        systemWindowsBeforeWriter: 0,
        micWindowsBeforeWriter: 0,
        systemFramesBeforeWriter: 0,
        micFramesBeforeWriter: 0,
        micStopDiagnostics: null,
      },
      finalizationErrors: [],
    } as RecordingResult
    setRecordingResult(result)
    setAppState('preview')
  } catch (e) {
    setErrorMessage(String(e))
    setAppState('failed')
  }
}, [])
```

- [ ] **Step 3: Integrate sidebar in idle view**

Modify the idle render block. Replace the existing container div:

```tsx
if (appState === 'idle') {
  return (
    <div className="min-h-screen flex" data-luzhi-drag-region="surface">
      {/* Main content area */}
      <div className="flex-1 flex items-center justify-center p-8">
        <div className="relative">
          <RecordingPanel
            recordingMode={recordingMode}
            setRecordingMode={setRecordingMode}
            systemAudioEnabled={systemAudioEnabled}
            setSystemAudioEnabled={setSystemAudioEnabled}
            micEnabled={micEnabled}
            setMicEnabled={setMicEnabled}
            micDevice={micDevice}
            setMicDevice={setMicDevice}
            micVolume={micVolume}
            denoiseEnabled={denoiseEnabled}
            onDenoiseChange={setDenoiseEnabled}
            onStartRecording={handleStartRecording}
            resolution={resolution}
            setResolution={setResolution}
            fps={fps}
            setFps={setFps}
          />
          {/* Permission warnings */}
          {permissions.screenRecording === 'denied' && (
            <div className="mt-4 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              <p>屏幕录制权限未授权，请在系统设置中开启</p>
            </div>
          )}
          {permissions.microphone === 'denied' && (
            <div className="mt-2 p-3 rounded-xl bg-destructive/10 border border-destructive/20 text-sm text-destructive">
              <p>麦克风权限未授权，请在系统设置中开启</p>
            </div>
          )}
          {permissions.screenRecording === 'notDetermined' && (
            <div className="mt-4 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-sm text-amber-400">
              <p>需要屏幕录制权限才能录制，请在启动录制时授权</p>
            </div>
          )}
          {permissions.microphone === 'notDetermined' && micEnabled && (
            <div className="mt-2 p-3 rounded-xl bg-amber-500/10 border border-amber-500/20 text-sm text-amber-400">
              <p>需要麦克风权限才能录制音频，请在启动录制时授权</p>
            </div>
          )}
          <div className="flex w-full items-start justify-end mt-4">
            <LicenseStatus status={licenseStatus} />
          </div>
        </div>
      </div>

      {/* History sidebar */}
      <RecordingSidebar
        isOpen={sidebarOpen}
        onToggle={() => setSidebarOpen(!sidebarOpen)}
        onSelectRecording={(id) => void handleSelectRecording(id)}
      />
    </div>
  )
}
```

- [ ] **Step 4: Verify TypeScript compiles**

Run: `npm run build`
Expected: No TypeScript errors.

- [ ] **Step 5: Run existing frontend tests**

Run: `npm test -- --run`
Expected: All tests pass. If tests mock `invoke`, add mocks for the 4 new commands.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/components/recording-sidebar.tsx
git commit -m "feat(ui): 在空闲界面集成历史录制侧边栏"
```

---

### Task 11: Full Build & Integration Test

**Files:** None (verification only)

- [ ] **Step 1: Run Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: All tests pass.

- [ ] **Step 2: Run frontend tests**

Run: `npm test -- --run`
Expected: All tests pass.

- [ ] **Step 3: Run full build**

Run: `npm run build`
Expected: Build succeeds without errors.

- [ ] **Step 4: Run Rust linter**

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
Expected: No warnings.

- [ ] **Step 5: Manual verification checklist**

Test the following scenarios:
1. Start a recording → stop → verify it appears in sidebar
2. Click sidebar item → verify preview loads with correct video
3. Click import → select a valid recording file → verify it appears in sidebar
4. Click import → select an invalid file → verify error message
5. Delete a recording → verify it's removed from sidebar and files deleted
6. Restart app → verify sidebar still shows previous recordings

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: 录制历史库与导入功能完整实现"
```
