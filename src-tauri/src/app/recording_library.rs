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
                    reason: format!("缺少配套文件：{}", p.display()),
                });
            }
        }

        // Validate metadata is parseable.
        let _meta = crate::media::recording_metadata::RecordingMetadataWriter::read_metadata(
            &cursor_metadata_path,
        )
        .map_err(|e| AppError::ImportFailed {
            reason: format!("元数据文件损坏：{e}"),
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
