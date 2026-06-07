use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_screen_capture_kit::{SCShareableContent, SCWindow};

use crate::app::error::{AppError, AppResult};
use crate::core::window::WindowInfo;

/// 获取当前可见窗口列表
pub fn list_windows() -> AppResult<Vec<WindowInfo>> {
    let content = get_shareable_content_sync()?;
    let windows = unsafe { content.windows() };
    let mut result = Vec::new();

    for i in 0..windows.count() {
        let window = unsafe { windows.objectAtIndex(i) };

        // 过滤条件
        if !should_include_window(&window) {
            continue;
        }

        let info = window_to_info(&window);
        result.push(info);
    }

    // 按应用名称排序
    result.sort_by(|a, b| a.app_name.cmp(&b.app_name).then(a.title.cmp(&b.title)));

    Ok(result)
}

/// 获取窗口缩略图（Base64 PNG）
///
/// 当前返回 None，缩略图为可选功能。
/// 后续可实现 SCScreenshotManager.captureImage。
pub fn get_window_thumbnail(_window_id: u32) -> AppResult<Option<String>> {
    Ok(None)
}

/// 同步获取 SCShareableContent
fn get_shareable_content_sync() -> AppResult<Retained<SCShareableContent>> {
    let result: Arc<Mutex<Option<AppResult<Retained<SCShareableContent>>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut objc2_foundation::NSError| {
            let outcome = if error.is_null() && !content.is_null() {
                Ok(unsafe { Retained::retain(content) }.unwrap())
            } else {
                Err(AppError::CaptureFailed {
                    reason: "获取屏幕内容失败".to_string(),
                })
            };
            *result_clone.lock().unwrap() = Some(outcome);
        },
    );

    unsafe {
        SCShareableContent::getShareableContentWithCompletionHandler(&block);
    }

    // 等待异步回调完成
    let mut attempts = 0;
    loop {
        let guard = result.lock().unwrap();
        if guard.is_some() {
            return guard.as_ref().unwrap().clone();
        }
        drop(guard);

        attempts += 1;
        if attempts > 100 {
            return Err(AppError::CaptureFailed {
                reason: "获取屏幕内容超时".to_string(),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 判断窗口是否应该包含在列表中
fn should_include_window(window: &SCWindow) -> bool {
    // 排除无标题窗口
    let title = unsafe { window.title() };
    if title.is_none() || title.unwrap().len() == 0 {
        return false;
    }

    // 排除自身应用窗口（LuZhi）
    let app_name = unsafe { window.owningApplication() }
        .map(|app| unsafe { app.applicationName() }.to_string())
        .unwrap_or_default();

    if app_name == "LuZhi" || app_name == "录智" {
        return false;
    }

    true
}

/// 将 SCWindow 转换为 WindowInfo
fn window_to_info(window: &SCWindow) -> WindowInfo {
    let title = unsafe { window.title() }
        .map(|s| s.to_string())
        .unwrap_or_default();

    let (app_name, bundle_id) = unsafe { window.owningApplication() }
        .map(|app| {
            let name = unsafe { app.applicationName() }.to_string();
            let bundle = unsafe { app.bundleIdentifier() }.to_string();
            (name, Some(bundle))
        })
        .unwrap_or_default();

    let frame = unsafe { window.frame() };
    let is_on_screen = unsafe { window.isOnScreen() };

    WindowInfo {
        window_id: unsafe { window.windowID() },
        title,
        app_name,
        bundle_id,
        is_on_screen,
        width: frame.size.width as f64,
        height: frame.size.height as f64,
        thumbnail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_info_serializes_to_json() {
        let info = WindowInfo {
            window_id: 123,
            title: "Safari".to_string(),
            app_name: "Safari".to_string(),
            bundle_id: Some("com.apple.Safari".to_string()),
            is_on_screen: true,
            width: 1920.0,
            height: 1080.0,
            thumbnail: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"windowId\":123"));
        assert!(json.contains("\"appName\":\"Safari\""));
    }
}
