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
    /// Optional: missing when cursor tracking was disabled or write failed.
    pub cursor_metadata_path: Option<PathBuf>,
    /// Optional: missing when beautify has not been built yet.
    pub effect_timeline_path: Option<PathBuf>,
    /// Optional: only created when auto-trim is used.
    pub trim_metadata_path: Option<PathBuf>,
    /// Optional: only created when auto-trim is used.
    pub cut_timeline_path: Option<PathBuf>,
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
        match serde_json::from_str::<RecordingIndex>(&content) {
            Ok(idx) if idx.version == INDEX_VERSION => Some(idx),
            Ok(_) => {
                log::warn!("索引版本不匹配，将重建索引");
                None
            }
            Err(e) => {
                log::warn!("索引文件解析失败：{e}，将重建索引");
                None
            }
        }
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
    ///
    /// Only `video_path` is strictly required. All companion file paths
    /// (`cursor_metadata_path`, `effect_timeline_path`, `trim_metadata_path`,
    /// `cut_timeline_path`) are optional — stored as `None` when the file
    /// does not exist or was not produced.
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

        // Optional companion files: use if exists, ignore if missing.
        let cmp = cursor_metadata_path.map(PathBuf::from).filter(|p| p.exists());
        let etp = effect_timeline_path.map(PathBuf::from).filter(|p| p.exists());
        let tmp = trim_metadata_path.map(PathBuf::from).filter(|p| p.exists());
        let ctp = cut_timeline_path.map(PathBuf::from).filter(|p| p.exists());

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
            Self::safe_remove_file(&entry.video_path);
            if let Some(ref p) = entry.cursor_metadata_path {
                Self::safe_remove_file(p);
            }
            if let Some(ref p) = entry.effect_timeline_path {
                Self::safe_remove_file(p);
            }
            if let Some(ref p) = entry.trim_metadata_path {
                Self::safe_remove_file(p);
            }
            if let Some(ref p) = entry.cut_timeline_path {
                Self::safe_remove_file(p);
            }
        }

        self.index.entries.retain(|e| e.id != id);
        self.save_index()
    }

    /// Delete a file only if it lives under the app's recordings directory.
    /// Prevents path-traversal attacks via a tampered `recordings-index.json`.
    fn safe_remove_file(path: &Path) {
        let recordings_dir = std::env::temp_dir().join("luzhi-recordings");
        // Resolve symlinks before checking the prefix, so a symlink pointing
        // outside the directory is rejected.
        match path.canonicalize() {
            Ok(canonical) if canonical.starts_with(&recordings_dir) => {
                let _ = fs::remove_file(canonical);
            }
            _ => {
                log::warn!("拒绝删除录制目录外的文件：{}", path.display());
            }
        }
    }

    /// Load full context for re-entering the beautify workflow.
    pub fn get_recording(&self, id: &str) -> AppResult<RecordingContext> {
        let entry = self.find_entry(id)?.clone();

        let metadata_json = match entry.cursor_metadata_path {
            Some(ref p) => fs::read_to_string(p).map_err(|e| AppError::ImportFailed {
                reason: format!("读取光标元数据失败：{e}"),
            })?,
            None => String::new(),
        };

        let effect_timeline_json = match entry.effect_timeline_path {
            Some(ref p) => fs::read_to_string(p).map_err(|e| AppError::ImportFailed {
                reason: format!("读取效果时间线失败：{e}"),
            })?,
            None => String::new(),
        };

        let cut_timeline_json = match entry.cut_timeline_path {
            Some(ref p) => fs::read_to_string(p).map_err(|e| AppError::ImportFailed {
                reason: format!("读取裁剪时间线失败：{e}"),
            })?,
            None => String::new(),
        };

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
        if parts.len() != 3 || parts[0] != "recording" {
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

    /// Search the directory for a companion file matching the given prefix and
    /// sequence number. Companion files are named:
    ///   `{prefix}-{millis}-{seq}.json`
    /// where `millis` may differ from the video file's timestamp because
    /// metadata files are created at recording *start* while the video file
    /// is created at recording *stop*.
    ///
    /// If multiple files match (e.g., from different sessions with the same
    /// seq after an app restart), returns the most recent one.
    fn find_companion_file(
        dir: &Path,
        prefix: &str,
        seq: u64,
        extension: &str,
    ) -> Option<PathBuf> {
        let entries = fs::read_dir(dir).ok()?;
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|ext| ext.to_str()) == Some(extension)
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|stem| {
                            stem.starts_with(prefix)
                                && stem.ends_with(&format!("-{seq}"))
                                && stem.len() > prefix.len() + format!("-{seq}").len()
                        })
                        .unwrap_or(false)
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Sort by filename descending → most recent (highest millis) first.
        candidates.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        candidates.into_iter().next()
    }

    /// Validate and import an external MP4 file produced by this app.
    pub fn import(&mut self, video_path: &Path) -> AppResult<LibraryEntrySummary> {
        if video_path.extension().and_then(|e| e.to_str()) != Some("mp4") {
            return Err(AppError::ImportFailed {
                reason: "仅支持导入 .mp4 文件".to_string(),
            });
        }

        let (millis, seq) = Self::parse_recording_filename(video_path)?;
        let dir = video_path.parent().unwrap_or(Path::new("."));

        // Find companion files by prefix pattern. Metadata files have different
        // timestamps than the video file (created at recording start vs stop).
        let cursor_metadata_path =
            Self::find_companion_file(dir, "cursor-metadata", seq, "json").ok_or_else(|| {
                AppError::ImportFailed {
                    reason: "缺少配套文件：cursor-metadata".to_string(),
                }
            })?;

        let effect_timeline_path =
            Self::find_companion_file(dir, "cursor-effects", seq, "json").ok_or_else(|| {
                AppError::ImportFailed {
                    reason: "缺少配套文件：cursor-effects".to_string(),
                }
            })?;

        // Optional companion files — only exist after auto-trim is used.
        let trim_metadata_path =
            Self::find_companion_file(dir, "trim-metadata", seq, "json");
        let cut_timeline_path =
            Self::find_companion_file(dir, "cut-timeline", seq, "json");

        // Validate required metadata is parseable.
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
            cursor_metadata_path: Some(cursor_metadata_path),
            effect_timeline_path: Some(effect_timeline_path),
            trim_metadata_path,
            cut_timeline_path,
        };

        let summary = LibraryEntrySummary {
            id: entry.id.clone(),
            created_at: entry.created_at,
            duration_secs: entry.duration_secs,
        };

        self.index.entries.push(entry);
        self.save_index()?;

        Ok(summary)
    }

    /// Remove entries whose video file no longer exist.
    /// Companion files (cursor metadata, effect timeline, trim, cut) are all
    /// optional — their absence does not invalidate the entry.
    pub fn repair_on_startup(&mut self) {
        let before = self.index.entries.len();
        self.index.entries.retain(|entry| entry.video_path.exists());
        let removed = before - self.index.entries.len();
        if removed > 0 {
            log::warn!("启动时移除 {removed} 条无效索引条目");
            if self.save_index().is_err() {
                // Rollback: reload from disk to restore consistency.
                if let Some(idx) = Self::load_index(&self.index_path) {
                    self.index = idx;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("luzhi-lib-test-{}-{}", std::process::id(), nanos));
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
            cursor_metadata_path: Some(dir.join("nonexistent.json")),
            effect_timeline_path: Some(dir.join("nonexistent2.json")),
            trim_metadata_path: Some(dir.join("nonexistent3.json")),
            cut_timeline_path: Some(dir.join("nonexistent4.json")),
        });
        lib.save_index().unwrap();

        lib.repair_on_startup();
        assert!(lib.list().is_empty());

        cleanup(&dir);
    }
}
