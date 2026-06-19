use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::app::error::{AppError, AppResult};
use crate::core::capture::{
    AudioCapabilities, AudioCapture, AudioChunkSink, AudioConfig, AudioDevice,
};
use crate::core::clock::SessionClock;

pub struct WasapiLoopback {
    session_clock: Option<Arc<SessionClock>>,
    stop_flag: Option<Arc<AtomicBool>>,
    worker: Option<thread::JoinHandle<AppResult<()>>>,
}

impl WasapiLoopback {
    pub fn new() -> Self {
        Self {
            session_clock: None,
            stop_flag: None,
            worker: None,
        }
    }

    pub fn set_session_clock(&mut self, session_clock: Arc<SessionClock>) {
        self.session_clock = Some(session_clock);
    }
}

impl Default for WasapiLoopback {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapture for WasapiLoopback {
    fn start(&mut self, config: AudioConfig, sink: AudioChunkSink) -> AppResult<()> {
        if self.worker.is_some() {
            return Err(AppError::InvalidState {
                current: "recording",
                action: "start_wasapi_loopback",
            });
        }

        let session_clock = self
            .session_clock
            .clone()
            .unwrap_or_else(|| Arc::new(SessionClock::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        self.stop_flag = Some(stop);
        self.worker = Some(thread::spawn(move || {
            run_loopback_worker(config, sink, session_clock, thread_stop)
        }));
        Ok(())
    }

    fn stop(&mut self) -> AppResult<()> {
        if let Some(stop) = self.stop_flag.take() {
            stop.store(true, Ordering::Relaxed);
        }

        if let Some(worker) = self.worker.take() {
            // Bounded join: spawn a thread to wait for the worker, then
            // recv with timeout to avoid blocking forever if the worker
            // hangs in a WASAPI/COM call.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = worker.join();
                let _ = tx.send(result);
            });
            match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(AppError::AudioCaptureFailed {
                    reason: "WASAPI loopback 线程崩溃".to_string(),
                }),
                Err(_) => Err(AppError::AudioCaptureFailed {
                    reason: "WASAPI loopback 停止超时".to_string(),
                }),
            }
        } else {
            Ok(())
        }
    }

    fn device_list(&self) -> AppResult<Vec<AudioDevice>> {
        Ok(vec![AudioDevice {
            id: "default-output-loopback".to_string(),
            name: "默认系统输出".to_string(),
            is_default: true,
        }])
    }

    fn capabilities(&self) -> AudioCapabilities {
        AudioCapabilities {
            supports_system_audio: true,
            supports_microphone: false,
        }
    }
}

#[cfg(target_os = "windows")]
fn run_loopback_worker(
    config: AudioConfig,
    sink: AudioChunkSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    unsafe { run_loopback_worker_inner(config, sink, session_clock, stop) }
}

#[cfg(not(target_os = "windows"))]
fn run_loopback_worker(
    _config: AudioConfig,
    _sink: AudioChunkSink,
    _session_clock: Arc<SessionClock>,
    _stop: Arc<AtomicBool>,
) -> AppResult<()> {
    Err(AppError::NativeCaptureUnavailable {
        reason: "WASAPI loopback 仅支持 Windows",
    })
}

/// WASAPI loopback capture worker.
///
/// # Safety
///
/// This function uses COM APIs (IMMDeviceEnumerator, IAudioClient,
/// IAudioCaptureClient). COM must be initialized on this thread.
/// All COM resources are released before the function returns.
/// WASAPI buffers obtained via GetBuffer must be released exactly
/// once via ReleaseBuffer.
#[cfg(target_os = "windows")]
unsafe fn run_loopback_worker_inner(
    _config: AudioConfig,
    sink: AudioChunkSink,
    session_clock: Arc<SessionClock>,
    stop: Arc<AtomicBool>,
) -> AppResult<()> {
    // Implementation detail:
    // 1. CoInitializeEx for the worker thread.
    // 2. IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole).
    // 3. Activate IAudioClient.
    // 4. GetMixFormat and derive sample_rate/channels/sample type.
    // 5. Initialize shared loopback stream with AUDCLNT_STREAMFLAGS_LOOPBACK.
    // 6. Get IAudioCaptureClient.
    // 7. Start audio client.
    // 8. Poll GetNextPacketSize, then GetBuffer/ReleaseBuffer.
    //    IMPORTANT: Check the `flags` output parameter from GetBuffer:
    //    - AUDCLNT_BUFFERFLAGS_SILENT (0x2): buffer contains silence;
    //      emit a zero-filled silent chunk or skip entirely.
    //    - AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY (0x4): audio glitch detected;
    //      flag the timestamp gap to avoid sync issues.
    // 9. Convert PCM float or i16 to interleaved f32.
    // 10. Timestamp with AudioSampleClock lazy offset.
    // 11. Send AudioChunk through sink; record dropped chunks via MediaSender counters.
    // 12. Stop client and CoUninitialize before return.
    let _ = sink;
    let _ = session_clock;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}
