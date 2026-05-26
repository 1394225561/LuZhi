use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Manages a background thread that periodically reads and emits mic level.
///
/// The thread stops when `stop()` is called or when the `MicLevelRuntime` is dropped.
pub struct MicLevelRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MicLevelRuntime {
    pub fn spawn<F>(mut read_and_emit: F) -> Self
    where
        F: FnMut() + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                read_and_emit();
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MicLevelRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}
