//! macOS window bridge.
//!
//! Week 4 reads running apps and on-screen windows, and it can ask for the
//! Accessibility permission when the user starts that step. It still does not
//! move, resize, follow, or strip another window. Those calls stay out of this
//! file until geometric snapping is actually implemented.
//!
//! Reading the on-screen list uses `CGWindowListCopyWindowInfo`, which does not
//! prompt. `AXIsProcessTrusted` only queries. The prompt option is used from the
//! onboarding command, not while the shell is starting.

#[cfg(target_os = "macos")]
use tauri::AppHandle;

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "window_mac: list windows and a11y prompt; snap placeholder"
    } else {
        "window_mac: placeholder, inactive off macOS"
    }
}

// Week 5 geometric snap, not implemented:
//
// The content placeholder would be an NSView frame in this window, the same
// bounds the partition webviews already receive from the shell. It stays an
// opaque window. Moving the other app would use AXUIElementSetAttributeValue
// for position and size, then a display link only while a snap is active.
// AX coordinates are top-left; AppKit screen coordinates are bottom-left.
// None of that runs this week. Do not add an observer that writes frames.
//
// See docs/week2-a11y-placeholder-research.md and docs/week4-detection-a11y.md.

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_void, CStr};
    use std::sync::mpsc;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;
    use tauri::AppHandle;

    use crate::detector::{MacSnapshot, ObservedProcess, RawWindow};

    const ONSCREEN_EXCLUDING_DESKTOP: u32 = 1 | (1 << 4);

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut AnyObject;
    }

    pub(super) fn observe(app: &AppHandle) -> Result<MacSnapshot, String> {
        on_main(app, collect_snapshot)?
    }

    pub(super) fn trusted(app: &AppHandle) -> bool {
        on_main(app, || unsafe { AXIsProcessTrusted() != 0 }).unwrap_or(false)
    }

    pub(super) fn prompt(app: &AppHandle) -> Result<bool, String> {
        on_main(app, prompt_on_main)?
    }

    pub(super) fn open_settings(app: &AppHandle) -> Result<(), String> {
        on_main(app, open_settings_on_main)?
    }

    /// The caller must not already be the main thread. `run_on_main_thread` only
    /// queues the closure, so waiting here from the main thread would deadlock.
    fn on_main<T: Send + 'static>(
        app: &AppHandle,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(work());
        })
        .map_err(|err| format!("无法切到主线程: {err}"))?;
        rx.recv().map_err(|_| "主线程没有返回".to_string())
    }

    fn collect_snapshot() -> Result<MacSnapshot, String> {
        let processes = collect_processes()?;
        let (windows, window_note) = collect_windows();
        Ok(MacSnapshot {
            processes,
            windows,
            trusted: unsafe { AXIsProcessTrusted() != 0 },
            window_note,
        })
    }

    fn collect_processes() -> Result<Vec<ObservedProcess>, String> {
        let Some(class) = AnyClass::get(c"NSWorkspace") else {
            return Err("系统没有 NSWorkspace".into());
        };
        let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
        if workspace.is_null() {
            return Err("无法读取正在运行的应用".into());
        }
        let apps: *mut AnyObject = unsafe { msg_send![workspace, runningApplications] };
        if apps.is_null() {
            return Err("无法读取正在运行的应用".into());
        }
        let count: usize = unsafe { msg_send![apps, count] };
        let mut processes = Vec::new();
        for index in 0..count.min(2_000) {
            let app: *mut AnyObject = unsafe { msg_send![apps, objectAtIndex: index] };
            if app.is_null() {
                continue;
            }
            let pid: i32 = unsafe { msg_send![app, processIdentifier] };
            let bundle_obj: *mut AnyObject = unsafe { msg_send![app, bundleIdentifier] };
            let name_obj: *mut AnyObject = unsafe { msg_send![app, localizedName] };
            processes.push(ObservedProcess {
                pid,
                bundle_id: string_from(bundle_obj).unwrap_or_default(),
                process_name: string_from(name_obj).unwrap_or_default(),
            });
        }
        Ok(processes)
    }

    fn collect_windows() -> (Vec<RawWindow>, Option<String>) {
        let raw = unsafe { CGWindowListCopyWindowInfo(ONSCREEN_EXCLUDING_DESKTOP, 0) };
        if raw.is_null() {
            return (Vec::new(), Some("系统没有返回可见窗口。".into()));
        }
        let Some(array) = (unsafe { Retained::from_raw(raw) }) else {
            return (Vec::new(), Some("系统没有返回可见窗口。".into()));
        };
        let count: usize = unsafe { msg_send![&*array, count] };
        let mut windows = Vec::new();
        for index in 0..count.min(4_000) {
            let item: *mut AnyObject = unsafe { msg_send![&*array, objectAtIndex: index] };
            if let Some(window) = parse_window(item) {
                windows.push(window);
            }
        }
        (windows, None)
    }

    fn parse_window(item: *mut AnyObject) -> Option<RawWindow> {
        if item.is_null() {
            return None;
        }
        let window_id = u32::try_from(number_i64(item, "kCGWindowNumber")?).ok()?;
        if window_id == 0 {
            return None;
        }
        let pid = i32::try_from(number_i64(item, "kCGWindowOwnerPID")?).ok()?;
        let layer = i32::try_from(number_i64(item, "kCGWindowLayer")?).ok()?;
        let (width, height) =
            bounds_size(object_for(item, "kCGWindowBounds")).unwrap_or((-1.0, -1.0));
        Some(RawWindow {
            window_id,
            pid,
            owner_name: string_key(item, "kCGWindowOwnerName").unwrap_or_default(),
            title: string_key(item, "kCGWindowName").unwrap_or_default(),
            layer,
            on_screen: bool_key(item, "kCGWindowIsOnscreen").unwrap_or(true),
            alpha: f64_key(item, "kCGWindowAlpha").unwrap_or(1.0),
            width,
            height,
        })
    }

    fn prompt_on_main() -> Result<bool, String> {
        let Some(number_class) = AnyClass::get(c"NSNumber") else {
            return Err("无法请求辅助功能授权".into());
        };
        let Some(dict_class) = AnyClass::get(c"NSDictionary") else {
            return Err("无法请求辅助功能授权".into());
        };
        let value: *mut AnyObject = unsafe { msg_send![number_class, numberWithBool: true] };
        if value.is_null() {
            return Err("无法请求辅助功能授权".into());
        }
        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let options: *mut AnyObject =
            unsafe { msg_send![dict_class, dictionaryWithObject: value, forKey: &*key] };
        if options.is_null() {
            return Err("无法请求辅助功能授权".into());
        }
        let trusted = unsafe { AXIsProcessTrustedWithOptions(options.cast::<c_void>()) } != 0;
        Ok(trusted)
    }

    fn open_settings_on_main() -> Result<(), String> {
        let Some(class) = AnyClass::get(c"NSWorkspace") else {
            return Err("打不开系统设置里的辅助功能页面".into());
        };
        let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
        if workspace.is_null() {
            return Err("打不开系统设置里的辅助功能页面".into());
        }
        let urls = [
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility",
        ];
        if urls.iter().any(|url| open_one(workspace, url)) {
            Ok(())
        } else {
            Err("打不开系统设置里的辅助功能页面".into())
        }
    }

    fn open_one(workspace: *mut AnyObject, url: &str) -> bool {
        let Some(url_class) = AnyClass::get(c"NSURL") else {
            return false;
        };
        let text = NSString::from_str(url);
        let nsurl: *mut AnyObject = unsafe { msg_send![url_class, URLWithString: &*text] };
        if nsurl.is_null() {
            return false;
        }
        let opened: bool = unsafe { msg_send![workspace, openURL: nsurl] };
        opened
    }

    fn object_for(dict: *mut AnyObject, key: &str) -> *mut AnyObject {
        let name = NSString::from_str(key);
        unsafe { msg_send![dict, objectForKey: &*name] }
    }

    fn is_class(obj: *mut AnyObject, name: &CStr) -> bool {
        let Some(class) = AnyClass::get(name) else {
            return false;
        };
        let yes: bool = unsafe { msg_send![obj, isKindOfClass: class] };
        yes
    }

    fn string_from(obj: *mut AnyObject) -> Option<String> {
        if obj.is_null() || !is_class(obj, c"NSString") {
            return None;
        }
        let bytes: *const i8 = unsafe { msg_send![obj, UTF8String] };
        if bytes.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(bytes) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn string_key(dict: *mut AnyObject, key: &str) -> Option<String> {
        string_from(object_for(dict, key))
    }

    fn number_i64(dict: *mut AnyObject, key: &str) -> Option<i64> {
        let obj = object_for(dict, key);
        if obj.is_null() || !is_class(obj, c"NSNumber") {
            return None;
        }
        let value: i64 = unsafe { msg_send![obj, longLongValue] };
        Some(value)
    }

    fn f64_key(dict: *mut AnyObject, key: &str) -> Option<f64> {
        let obj = object_for(dict, key);
        if obj.is_null() || !is_class(obj, c"NSNumber") {
            return None;
        }
        let value: f64 = unsafe { msg_send![obj, doubleValue] };
        Some(value)
    }

    fn bool_key(dict: *mut AnyObject, key: &str) -> Option<bool> {
        let obj = object_for(dict, key);
        if obj.is_null() || !is_class(obj, c"NSNumber") {
            return None;
        }
        let value: bool = unsafe { msg_send![obj, boolValue] };
        Some(value)
    }

    fn bounds_size(bounds: *mut AnyObject) -> Option<(f64, f64)> {
        if bounds.is_null() || !is_class(bounds, c"NSDictionary") {
            return None;
        }
        let width = f64_key(bounds, "Width")?;
        let height = f64_key(bounds, "Height")?;
        Some((width, height))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn observe(app: &AppHandle) -> Result<crate::detector::MacSnapshot, String> {
    macos::observe(app)
}

#[cfg(target_os = "macos")]
pub(crate) fn trusted(app: &AppHandle) -> bool {
    macos::trusted(app)
}

#[cfg(target_os = "macos")]
pub(crate) fn prompt(app: &AppHandle) -> Result<bool, String> {
    macos::prompt(app)
}

#[cfg(target_os = "macos")]
pub(crate) fn open_settings(app: &AppHandle) -> Result<(), String> {
    macos::open_settings(app)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_placeholder_and_does_not_claim_snap_is_active() {
        let text = super::status();
        assert!(text.contains("placeholder"));
        assert!(text.contains("snap") || text.contains("inactive"));
    }
}
