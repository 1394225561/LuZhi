use std::sync::Arc;

use crate::app::cursor_metadata_runtime::{CursorSnapshot, CursorSnapshotSource};
use crate::app::error::{AppError, AppResult};
use crate::core::clock::SessionClock;
use crate::core::timeline::CursorKind;

pub struct WindowsCursorSource {
    session_clock: Arc<SessionClock>,
}

impl WindowsCursorSource {
    pub fn new(session_clock: Arc<SessionClock>) -> Self {
        Self { session_clock }
    }
}

impl CursorSnapshotSource for WindowsCursorSource {
    fn snapshot(&mut self) -> AppResult<CursorSnapshot> {
        #[cfg(target_os = "windows")]
        {
            windows_cursor_snapshot(&self.session_clock)
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(AppError::CursorProcessingFailed {
                reason: "Windows 光标采集仅支持 Windows".to_string(),
            })
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_cursor_snapshot(session_clock: &SessionClock) -> AppResult<CursorSnapshot> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    // Virtual key codes for mouse buttons (not gated behind WindowsAndMessaging feature).
    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    const VK_MBUTTON: i32 = 0x04;

    let start = std::time::Instant::now();
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.map_err(|e| AppError::CursorProcessingFailed {
        reason: format!("读取 Windows 鼠标位置失败: {e}"),
    })?;

    let captured_at_nanos = session_clock.elapsed_nanos();
    let left_down = unsafe { (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0 };
    let right_down = unsafe { (GetAsyncKeyState(VK_RBUTTON) as u16 & 0x8000) != 0 };
    let middle_down = unsafe { (GetAsyncKeyState(VK_MBUTTON) as u16 & 0x8000) != 0 };

    Ok(CursorSnapshot {
        x: point.x as f32,
        y: point.y as f32,
        left_down,
        right_down,
        middle_down,
        kind: CursorKind::Arrow,
        captured_at_nanos,
        snapshot_duration_nanos: start.elapsed().as_nanos() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cursor_source_constructs_with_session_clock() {
        let source = WindowsCursorSource::new(Arc::new(SessionClock::new()));
        let _ = source;
    }
}
