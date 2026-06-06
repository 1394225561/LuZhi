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
                reason: format!("创建索引目录失败：{e}"),
            })?;
        }
        let json =
            serde_json::to_string_pretty(&self.index).map_err(|e| AppError::IndexCorrupted {
                reason: format!("序列化索引失败：{e}"),
            })?;
        fs::write(&self.index_path, json).map_err(|e| AppError::IndexCorrupted {
            reason: format!("写入索引文件失败：{e}"),
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
                    reason: format!("缺少配套文件：{}", p.display()),
                });
            }
        }

        // Duplicate check.
        if self.index.entries.iter().any(|e| e.id == id) {
            return Ok(());
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

    /// Load full context for re-entering the beautify workflow.
    pub fn get_recording(&self, id: &str) -> AppResult<RecordingContext> {
        let entry = self.find_entry(id)?.clone();

        let metadata_json = fs::read_to_string(&entry.cursor_metadata_path).map_err(|e| {
            AppError::ImportFailed {
                reason: format!("读取光标元数据失败：{e}"),
            }
        })?;

        let effect_timeline_json =
            fs::read_to_string(&entry.effect_timeline_path).map_err(|e| {
                AppError::ImportFailed {
                    reason: format!("读取效果时间线失败：{e}"),
                }
            })?;

        let cut_timeline_json =
            fs::read_to_string(&entry.cut_timeline_path).map_err(|e| {
                AppError::ImportFailed {
                    reason: format!("读取裁剪时间线失败：{e}"),
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
