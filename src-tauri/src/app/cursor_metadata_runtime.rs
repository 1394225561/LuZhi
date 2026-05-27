use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::app::error::AppResult;
use crate::core::clock::SessionClock;
use crate::core::frame::MediaTimestamp;
use crate::core::timeline::{ClickPhase, CursorClick, CursorSample, MouseButton};
use crate::media::recording_metadata::RecordingMetadata;

const DEFAULT_MAX_CURSOR_SAMPLES: usize = 120_000;

/// Snapshot of the current cursor position and button states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorSnapshot {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub right_down: bool,
    pub middle_down: bool,
}

/// Source that can poll the current cursor state.
pub trait CursorSnapshotSource: Send + 'static {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot>;
}

/// Pure recorder that converts snapshots into timeline metadata.
pub struct CursorMetadataRecorder {
    fps: u32,
    max_samples: usize,
    samples: Vec<CursorSample>,
    clicks: Vec<CursorClick>,
    previous_snapshot: Option<CursorSnapshot>,
}

impl CursorMetadataRecorder {
    pub fn new(fps: u32) -> Self {
        Self::with_max_samples(fps, DEFAULT_MAX_CURSOR_SAMPLES)
    }

    pub fn with_max_samples(fps: u32, max_samples: usize) -> Self {
        Self {
            fps,
            max_samples,
            samples: Vec::new(),
            clicks: Vec::new(),
            previous_snapshot: None,
        }
    }

    pub fn record_snapshot(&mut self, timestamp: MediaTimestamp, snapshot: CursorSnapshot) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }

        self.samples.push(CursorSample {
            timestamp,
            x: snapshot.x,
            y: snapshot.y,
        });

        if let Some(previous) = self.previous_snapshot {
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Left,
                previous.left_down,
                snapshot.left_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Right,
                previous.right_down,
                snapshot.right_down,
            );
            self.record_button_transition(
                timestamp,
                snapshot,
                MouseButton::Middle,
                previous.middle_down,
                snapshot.middle_down,
            );
        }

        self.previous_snapshot = Some(snapshot);
    }

    fn record_button_transition(
        &mut self,
        timestamp: MediaTimestamp,
        snapshot: CursorSnapshot,
        button: MouseButton,
        was_down: bool,
        is_down: bool,
    ) {
        if was_down == is_down {
            return;
        }

        self.clicks.push(CursorClick {
            timestamp,
            button,
            phase: if is_down { ClickPhase::Down } else { ClickPhase::Up },
            x: snapshot.x,
            y: snapshot.y,
        });
    }

    pub fn finish(self, duration_nanos: u64) -> RecordingMetadata {
        RecordingMetadata {
            fps: self.fps,
            duration_nanos,
            cursor_samples: self.samples,
            cursor_clicks: self.clicks,
        }
    }
}

/// Background cursor metadata runtime. It polls at capture fps and stores only metadata.
pub struct CursorMetadataRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<RecordingMetadata>>,
}

impl CursorMetadataRuntime {
    pub fn spawn<S>(mut source: S, fps: u32, session_clock: Arc<SessionClock>) -> Self
    where
        S: CursorSnapshotSource,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let interval = Duration::from_nanos(1_000_000_000u64 / fps.max(1) as u64);

        let handle = thread::spawn(move || {
            let mut recorder = CursorMetadataRecorder::new(fps.max(1));
            while !thread_stop.load(Ordering::Relaxed) {
                if let Ok(snapshot) = source.snapshot() {
                    recorder.record_snapshot(
                        MediaTimestamp::from_nanos(session_clock.elapsed_nanos()),
                        snapshot,
                    );
                }
                thread::sleep(interval);
            }

            recorder.finish(session_clock.elapsed_nanos())
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) -> Option<RecordingMetadata> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .and_then(|handle| handle.join().ok())
    }
}

impl Drop for CursorMetadataRuntime {
    fn drop(&mut self) {
        if self.handle.is_some() {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::frame::MediaTimestamp;
    use crate::core::timeline::{ClickPhase, MouseButton};

    #[test]
    fn recorder_keeps_samples_in_order() {
        let mut recorder = CursorMetadataRecorder::new(30);

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 1.0,
                y: 2.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 3.0,
                y: 4.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );

        let metadata = recorder.finish(66_666_666);

        assert_eq!(metadata.fps, 30);
        assert_eq!(metadata.cursor_samples.len(), 2);
        assert_eq!(metadata.cursor_samples[1].x, 3.0);
    }

    #[test]
    fn recorder_detects_left_click_down_and_up() {
        let mut recorder = CursorMetadataRecorder::new(60);

        recorder.record_snapshot(
            MediaTimestamp::from_nanos(0),
            CursorSnapshot {
                x: 10.0,
                y: 20.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(16_666_666),
            CursorSnapshot {
                x: 11.0,
                y: 21.0,
                left_down: true,
                right_down: false,
                middle_down: false,
            },
        );
        recorder.record_snapshot(
            MediaTimestamp::from_nanos(33_333_333),
            CursorSnapshot {
                x: 12.0,
                y: 22.0,
                left_down: false,
                right_down: false,
                middle_down: false,
            },
        );

        let metadata = recorder.finish(50_000_000);

        assert_eq!(metadata.cursor_clicks.len(), 2);
        assert_eq!(metadata.cursor_clicks[0].button, MouseButton::Left);
        assert_eq!(metadata.cursor_clicks[0].phase, ClickPhase::Down);
        assert_eq!(metadata.cursor_clicks[1].phase, ClickPhase::Up);
    }

    #[test]
    fn recorder_caps_sample_count_to_bound_memory() {
        let mut recorder = CursorMetadataRecorder::with_max_samples(30, 3);

        for i in 0..10 {
            recorder.record_snapshot(
                MediaTimestamp::from_nanos(i * 33_333_333),
                CursorSnapshot {
                    x: i as f32,
                    y: i as f32,
                    left_down: false,
                    right_down: false,
                    middle_down: false,
                },
            );
        }

        let metadata = recorder.finish(333_333_333);

        assert_eq!(metadata.cursor_samples.len(), 3);
        assert_eq!(metadata.cursor_samples[0].x, 7.0);
    }
}
