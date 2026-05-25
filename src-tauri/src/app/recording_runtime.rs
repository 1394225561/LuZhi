use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Manages a background thread that emits periodic tick events.
///
/// The tick thread stops when `stop()` is called or when the `TickRuntime` is dropped.
pub struct TickRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TickRuntime {
    pub fn spawn<F>(mut emit: F) -> Self
    where
        F: FnMut(u64) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let started_at = Instant::now();

        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                emit(started_at.elapsed().as_secs());
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

impl Drop for TickRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}
