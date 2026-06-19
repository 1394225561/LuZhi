use crate::app::error::{AppError, AppResult};
use crate::core::window::{WindowInfo, WindowRecordingState};

/// Raw window information collected during enumeration.
#[derive(Clone, Debug)]
struct RawWindowInfo {
    hwnd: isize,
    title: String,
    app_name: String,
    visible: bool,
    minimized: bool,
    width: f64,
    height: f64,
}

fn should_include_window(raw: &RawWindowInfo) -> bool {
    raw.visible
        && !raw.title.trim().is_empty()
        && raw.width >= 1.0
        && raw.height >= 1.0
        && raw.app_name != "LuZhi"
        && raw.app_name != "录智"
}

pub fn list_windows() -> AppResult<Vec<WindowInfo>> {
    #[cfg(target_os = "windows")]
    {
        windows_list_windows()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(AppError::NativeCaptureUnavailable {
            reason: "Windows 窗口枚举仅支持 Windows",
        })
    }
}

#[cfg(target_os = "windows")]
fn windows_list_windows() -> AppResult<Vec<WindowInfo>> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible,
    };

    use std::sync::Mutex;

    let raw_windows: Mutex<Vec<RawWindowInfo>> = Mutex::new(Vec::new());

    unsafe extern "system" fn enum_callback(
        hwnd: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        let visible = IsWindowVisible(hwnd).as_bool();
        let minimized = IsIconic(hwnd).as_bool();

        let title_len = GetWindowTextLengthW(hwnd);
        let title = if title_len > 0 {
            let mut buf = vec![0u16; (title_len + 1) as usize];
            let written = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..written as usize])
        } else {
            String::new()
        };

        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let width = (rect.right - rect.left) as f64;
        let height = (rect.bottom - rect.top) as f64;

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let app_name = get_process_name(pid).unwrap_or_default();

        let raw = RawWindowInfo {
            hwnd: hwnd.0 as isize,
            title,
            app_name,
            visible,
            minimized,
            width,
            height,
        };

        if should_include_window(&raw) {
            // Access the Vec through the raw pointer passed via LPARAM.
            let windows_ptr = lparam.0 as *mut Mutex<Vec<RawWindowInfo>>;
            if let Ok(mut windows) = (*windows_ptr).lock() {
                windows.push(raw);
            }
        }

        windows::core::BOOL::from(true)
    }

    let raw_windows_ptr = &raw_windows as *const Mutex<Vec<RawWindowInfo>>;

    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            windows::Win32::Foundation::LPARAM(raw_windows_ptr as isize),
        );
    }

    let raw_windows = raw_windows.into_inner().unwrap_or_default();

    Ok(raw_windows
        .into_iter()
        .map(|raw| WindowInfo {
            window_id: raw.hwnd as u32,
            title: raw.title,
            app_name: raw.app_name,
            bundle_id: None,
            is_on_screen: !raw.minimized,
            width: raw.width,
            height: raw.height,
            thumbnail: None,
        })
        .collect())
}

#[cfg(target_os = "windows")]
fn get_process_name(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let pwstr = windows::core::PWSTR(buf.as_mut_ptr());
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, pwstr, &mut size);
        let _ = CloseHandle(handle);

        if ok.is_ok() {
            let path = String::from_utf16_lossy(&buf[..size as usize]);
            std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_empty_and_hidden_windows() {
        assert!(!should_include_window(&RawWindowInfo {
            hwnd: 1,
            title: "".to_string(),
            app_name: "App".to_string(),
            visible: true,
            minimized: false,
            width: 100.0,
            height: 100.0,
        }));

        assert!(!should_include_window(&RawWindowInfo {
            hwnd: 1,
            title: "Hidden".to_string(),
            app_name: "App".to_string(),
            visible: false,
            minimized: false,
            width: 100.0,
            height: 100.0,
        }));
    }

    #[test]
    fn filters_zero_size_windows() {
        assert!(!should_include_window(&RawWindowInfo {
            hwnd: 1,
            title: "Zero".to_string(),
            app_name: "App".to_string(),
            visible: true,
            minimized: false,
            width: 0.0,
            height: 100.0,
        }));
    }

    #[test]
    fn filters_luzhi_windows() {
        assert!(!should_include_window(&RawWindowInfo {
            hwnd: 1,
            title: "LuZhi".to_string(),
            app_name: "LuZhi".to_string(),
            visible: true,
            minimized: false,
            width: 800.0,
            height: 600.0,
        }));
    }

    #[test]
    fn includes_normal_windows() {
        assert!(should_include_window(&RawWindowInfo {
            hwnd: 1,
            title: "Notepad".to_string(),
            app_name: "Notepad".to_string(),
            visible: true,
            minimized: false,
            width: 800.0,
            height: 600.0,
        }));
    }
}
