//! Week 5 geometric snap for one confirmed Electron app.
//!
//! macOS cannot reparent a foreign window, so this module never calls
//! `SetParent` or `addChildWindow`. The shell punches a placeholder in one
//! pane. Accessibility removes the target window's chrome and aligns it to
//! that rectangle. A display link (or a 60fps timer if the link cannot start)
//! keeps position, size, Space, and z-order in step with the host window.
//! Detach, pane unload, and process exit put the chrome and stacking back.
//!
//! If Accessibility will not remove the chrome or will not accept the
//! placeholder rectangle, the same follow loop places the window beside the
//! host instead. Window mirroring is not implemented.
//!
//! Manual 「指定窗口」 stays a list. It does not call into this module.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::detector::{self, RawWindow};

/// Catalog id of the only desktop app this week will move.
/// The bundle id is read from `CONFIRMED_DESKTOP`, which tests pin to the catalog.
pub const SNAP_TARGET_ID: &str = "claude-desktop";
pub const FOLLOW_HZ: u64 = 60;
pub const FOLLOW_INTERVAL_NS: u64 = 1_000_000_000 / FOLLOW_HZ;
pub const FRAME_EPSILON: f64 = 0.5;
pub const SIDE_GAP: f64 = 12.0;
pub const SIDE_MIN_ROOM: f64 = 320.0;
pub const MIN_TARGET_EDGE: f64 = 200.0;
pub const RECOVERY_FILE: &str = "snap-recovery.json";

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "snap: claude-desktop geometric follow"
    } else {
        "snap: inactive off macOS"
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapMode {
    Inactive,
    Embedded,
    SideBySide,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl SnapFrame {
    pub fn as_rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapStatus {
    pub target_app_id: String,
    pub target_name: String,
    pub target_bundle_id: String,
    pub active: bool,
    pub mode: SnapMode,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChromeRecord {
    /// Accessibility attribute. Child buttons use this name and `hidden` is the
    /// original `AXHidden` value. `AXTitleBarVisible` stores the original bool
    /// of that window attribute instead.
    pub attribute: String,
    pub hidden: bool,
}

/// Original chrome, frame, and stacking, written before the first mutation.
/// A later launch applies it if the process died while a snap was active.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryFile {
    pub bundle_id: String,
    pub pid: i32,
    pub window_id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub below_window: Option<u32>,
    pub space: Option<u64>,
    pub chrome: Vec<ChromeRecord>,
    pub enhanced: Option<bool>,
    pub manual: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Accessibility rejected a write. Restore chrome and the original frame.
    AxRejected,
    /// The target window is gone. There is nothing to put back.
    TargetGone,
    /// The user minimized it. Put the chrome back and leave it minimized.
    Minimized,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FollowPlan {
    pub frame: Option<Rect>,
    pub raise_above_host: bool,
    pub move_to_space: Option<u64>,
    pub stop: Option<StopReason>,
}

pub struct FollowInput {
    pub mode: SnapMode,
    pub host: Rect,
    pub local_placeholder: Rect,
    pub current: Rect,
    pub display: Rect,
    pub host_space: Option<u64>,
    pub target_space: Option<u64>,
    pub target_above_host: bool,
    pub ax_rejected: bool,
    pub target_visible: bool,
    pub minimized: bool,
    pub space_move_failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterChromeMove {
    FollowEmbedded,
    RollBackThenBeside,
}

pub fn is_snap_target(app_id: &str) -> bool {
    app_id == SNAP_TARGET_ID
        && detector::CONFIRMED_DESKTOP
            .iter()
            .any(|app| app.id == app_id && !app.bundle_id.is_empty())
}

pub fn target_parts() -> Option<(&'static str, &'static str, &'static str)> {
    detector::CONFIRMED_DESKTOP
        .iter()
        .find(|app| app.id == SNAP_TARGET_ID)
        .map(|app| (app.id, app.name, app.bundle_id))
}

pub fn idle_status() -> SnapStatus {
    let (id, name, bundle) = target_parts().unwrap_or((SNAP_TARGET_ID, "Claude Desktop", ""));
    SnapStatus {
        target_app_id: id.to_string(),
        target_name: name.to_string(),
        target_bundle_id: bundle.to_string(),
        active: false,
        mode: SnapMode::Inactive,
        message: None,
    }
}

pub fn frame_usable(frame: &SnapFrame) -> bool {
    [frame.x, frame.y, frame.width, frame.height]
        .iter()
        .all(|value| value.is_finite())
        && frame.width >= 2.0
        && frame.height >= 2.0
        && frame.width <= 20_000.0
        && frame.height <= 20_000.0
}

/// `None` means attach may continue. A string means do not move the window.
pub fn rejection(app_id: &str, trusted: bool, running: bool, has_window: bool) -> Option<String> {
    if !is_snap_target(app_id) {
        return Some("这一周只吸附 Claude Desktop。指定窗口和其他桌面应用不会被移动。".to_string());
    }
    if !trusted {
        return Some(
            "需要辅助功能才能吸附。网页分屏可以继续使用，这个窗口没有被移动。".to_string(),
        );
    }
    if !running {
        return Some("Claude Desktop 没有在运行。".to_string());
    }
    if !has_window {
        return Some("没有找到 Claude Desktop 可移动的窗口。".to_string());
    }
    None
}

pub fn rects_close(left: Rect, right: Rect, epsilon: f64) -> bool {
    (left.x - right.x).abs() <= epsilon
        && (left.y - right.y).abs() <= epsilon
        && (left.width - right.width).abs() <= epsilon
        && (left.height - right.height).abs() <= epsilon
}

/// `host` and `local` are both in the AX coordinate system: y grows downward.
/// `local` is the placeholder's origin measured from the host window's top-left.
pub fn placeholder_rect(host: Rect, local: Rect) -> Rect {
    Rect {
        x: host.x + local.x,
        y: host.y + local.y,
        width: local.width,
        height: local.height,
    }
}

/// AppKit's screen origin is the bottom-left of the primary display, y up.
/// Accessibility and `CGWindow` bounds use the top-left, y down.
/// The follow loop reads `CGWindow` bounds, which are already in that system.
#[cfg_attr(not(test), allow(dead_code))]
pub fn appkit_to_ax(rect: Rect, primary_height: f64) -> Rect {
    Rect {
        x: rect.x,
        y: primary_height - (rect.y + rect.height),
        width: rect.width,
        height: rect.height,
    }
}

pub fn side_by_side_rect(main: Rect, display: Rect) -> Rect {
    let right_room = (display.x + display.width) - (main.x + main.width + SIDE_GAP);
    let left_room = (main.x - SIDE_GAP) - display.x;
    let height = main.height.min(display.height).max(1.0);
    let y_limit = (display.y + display.height - height).max(display.y);
    let y = main.y.clamp(display.y, y_limit);
    if right_room >= SIDE_MIN_ROOM {
        let width = main.width.min(960.0).min(right_room);
        return Rect {
            x: main.x + main.width + SIDE_GAP,
            y,
            width,
            height,
        };
    }
    if left_room >= SIDE_MIN_ROOM {
        let width = main.width.min(960.0).min(left_room);
        return Rect {
            x: main.x - SIDE_GAP - width,
            y,
            width,
            height,
        };
    }
    let width = main.width.min(display.width).max(1.0);
    Rect {
        x: (display.x + display.width - width).max(display.x),
        y,
        width,
        height,
    }
}

pub fn pick_target_window(windows: &[RawWindow], pid: i32) -> Option<u32> {
    let mut best: Option<&RawWindow> = None;
    for window in windows {
        if window.pid != pid
            || window.window_id == 0
            || !window.on_screen
            || window.layer != 0
            || window.alpha < 0.05
            || window.width < MIN_TARGET_EDGE
            || window.height < MIN_TARGET_EDGE
        {
            continue;
        }
        let replace = match best {
            None => true,
            Some(current) => {
                let area = window.width * window.height;
                let current_area = current.width * current.height;
                area > current_area
                    || ((area - current_area).abs() < f64::EPSILON
                        && window.window_id < current.window_id)
            }
        };
        if replace {
            best = Some(window);
        }
    }
    best.map(|window| window.window_id)
}

/// `order` is front-to-back, as `CGWindowListCopyWindowInfo` returns it.
pub fn target_is_above(order: &[u32], target: u32, host: u32) -> Option<bool> {
    let target_index = order.iter().position(|id| *id == target)?;
    let host_index = order.iter().position(|id| *id == host)?;
    Some(target_index < host_index)
}

pub fn window_behind(order: &[u32], target: u32) -> Option<u32> {
    let index = order.iter().position(|id| *id == target)?;
    order.get(index + 1).copied()
}

/// Title hidden, a window-level title-bar flag, or a window that already had
/// no title and no close button. Button-only changes do not count.
pub fn chrome_removed(title_hidden: bool, titlebar_hidden: bool, already_borderless: bool) -> bool {
    title_hidden || titlebar_hidden || already_borderless
}

pub fn after_chrome_and_placeholder(chrome_ok: bool, placeholder_ok: bool) -> AfterChromeMove {
    if chrome_ok && placeholder_ok {
        AfterChromeMove::FollowEmbedded
    } else {
        AfterChromeMove::RollBackThenBeside
    }
}

pub fn beside_accepted(side_ok: bool) -> bool {
    side_ok
}

/// Embedded mode keeps the chrome change. Every other outcome puts it back,
/// including a failed attach and side-by-side following.
pub fn rollback_chrome(chrome_dirty: bool, mode: Option<SnapMode>) -> bool {
    chrome_dirty && mode != Some(SnapMode::Embedded)
}

pub fn plan_follow(input: &FollowInput) -> FollowPlan {
    if input.minimized {
        return FollowPlan {
            frame: None,
            raise_above_host: false,
            move_to_space: None,
            stop: Some(StopReason::Minimized),
        };
    }
    if !input.target_visible {
        return FollowPlan {
            frame: None,
            raise_above_host: false,
            move_to_space: None,
            stop: Some(StopReason::TargetGone),
        };
    }
    if input.ax_rejected || matches!(input.mode, SnapMode::Inactive) {
        return FollowPlan {
            frame: None,
            raise_above_host: false,
            move_to_space: None,
            stop: Some(StopReason::AxRejected),
        };
    }
    let desired = match input.mode {
        SnapMode::Embedded => placeholder_rect(input.host, input.local_placeholder),
        SnapMode::SideBySide => side_by_side_rect(input.host, input.display),
        SnapMode::Inactive => unreachable!("inactive already returned"),
    };
    let frame = if rects_close(desired, input.current, FRAME_EPSILON) {
        None
    } else {
        Some(desired)
    };
    let move_to_space = match (input.host_space, input.target_space) {
        (Some(host), Some(target)) if host != target && !input.space_move_failed => Some(host),
        _ => None,
    };
    FollowPlan {
        frame,
        raise_above_host: !input.target_above_host,
        move_to_space,
        stop: None,
    }
}

pub fn embedded_message() -> String {
    "已去掉边框，对齐到这一格。点击窗口即可操作。".to_string()
}

pub fn side_by_side_message() -> String {
    "系统没有允许去掉边框或对齐到格子里，已改为在主窗口旁边跟随。边框还在。".to_string()
}

pub fn moved_rejected_message() -> String {
    "辅助功能没有允许移动这个窗口，它留在原来的位置。".to_string()
}

pub fn recovery_path(dir: &Path) -> PathBuf {
    dir.join(RECOVERY_FILE)
}

pub fn write_recovery(dir: &Path, file: &RecoveryFile) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    let body =
        serde_json::to_vec_pretty(file).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    let path = recovery_path(dir);
    let tmp = dir.join("snap-recovery.json.tmp");
    std::fs::write(&tmp, &body).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    std::fs::rename(&tmp, &path).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    Ok(())
}

pub fn read_recovery(dir: &Path) -> Option<RecoveryFile> {
    let text = std::fs::read_to_string(recovery_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn clear_recovery(dir: &Path) {
    let path = recovery_path(dir);
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    let tmp = dir.join("snap-recovery.json.tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(tmp);
    }
}

fn support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))
}

async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| format!("后台任务中断: {err}"))
}

#[tauri::command]
pub async fn snap_attach(
    app: AppHandle,
    app_id: String,
    frame: SnapFrame,
) -> Result<SnapStatus, String> {
    if !frame_usable(&frame) {
        return Err("占位区域还没有准备好。".into());
    }
    off_main(move || attach_blocking(app, app_id, frame)).await?
}

#[tauri::command]
pub async fn snap_update(app: AppHandle, frame: SnapFrame) -> Result<SnapStatus, String> {
    if !frame_usable(&frame) {
        return Err("占位区域还没有准备好。".into());
    }
    off_main(move || update_blocking(app, frame)).await?
}

#[tauri::command]
pub async fn snap_detach(app: AppHandle) -> Result<SnapStatus, String> {
    off_main(move || detach_blocking(app)).await?
}

fn attach_blocking(app: AppHandle, app_id: String, frame: SnapFrame) -> Result<SnapStatus, String> {
    #[cfg(target_os = "macos")]
    {
        crate::snap_mac::attach(&app, &app_id, frame)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_id, frame);
        Err("几何吸附只在 macOS 上执行。".into())
    }
}

fn update_blocking(app: AppHandle, frame: SnapFrame) -> Result<SnapStatus, String> {
    #[cfg(target_os = "macos")]
    {
        crate::snap_mac::update(&app, frame)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, frame);
        Ok(idle_status())
    }
}

fn detach_blocking(app: AppHandle) -> Result<SnapStatus, String> {
    #[cfg(target_os = "macos")]
    {
        crate::snap_mac::detach(&app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(idle_status())
    }
}

pub fn recover_orphans(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    crate::snap_mac::recover(app);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

pub fn restore_for_exit() {
    #[cfg(target_os = "macos")]
    crate::snap_mac::restore_for_exit();
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn support_dir_for(app: &AppHandle) -> Result<PathBuf, String> {
    support_dir(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn raw(id: u32, pid: i32, width: f64, height: f64) -> RawWindow {
        RawWindow {
            window_id: id,
            pid,
            owner_name: "Claude".into(),
            title: String::new(),
            layer: 0,
            on_screen: true,
            alpha: 1.0,
            x: 10.0,
            y: 20.0,
            width,
            height,
        }
    }

    fn follow_input(mode: SnapMode) -> FollowInput {
        FollowInput {
            mode,
            host: rect(100.0, 80.0, 800.0, 600.0),
            local_placeholder: rect(240.0, 48.0, 520.0, 500.0),
            current: rect(0.0, 0.0, 100.0, 100.0),
            display: rect(0.0, 0.0, 1440.0, 900.0),
            host_space: Some(1),
            target_space: Some(1),
            target_above_host: true,
            ax_rejected: false,
            target_visible: true,
            minimized: false,
            space_move_failed: false,
        }
    }

    #[test]
    fn only_claude_desktop_is_the_week5_snap_target() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/apps.json")).unwrap();
        let apps = catalog["apps"].as_array().unwrap();
        let claude = apps
            .iter()
            .find(|app| app["id"] == "claude-desktop")
            .unwrap();
        assert_eq!(claude["bundleId"], "com.anthropic.claudefordesktop");
        assert_eq!(claude["bundleIdStatus"], "confirmed");
        assert_eq!(claude["kind"], "native");
        let (id, name, bundle) = target_parts().unwrap();
        assert_eq!(id, SNAP_TARGET_ID);
        assert_eq!(id, "claude-desktop");
        assert_eq!(name, claude["name"].as_str().unwrap());
        assert_eq!(bundle, "com.anthropic.claudefordesktop");
        assert!(is_snap_target("claude-desktop"));
        for id in [
            "chatgpt-desktop",
            "lm-studio",
            "gemini-desktop",
            "kimi-desktop",
            "doubao-desktop",
            "cursor-desktop",
            "cursor",
            "claude",
            "workbuddy-desktop",
            "harness-desktop",
            "accio-work-desktop",
        ] {
            assert!(!is_snap_target(id), "{id}");
        }
        assert!(detector::CONFIRMED_DESKTOP
            .iter()
            .all(|app| app.id == "cursor-desktop"
                || app.bundle_id != "com.todesktop.230313mzl4w4u92"));
        let cursor_web = apps.iter().find(|app| app["id"] == "cursor").unwrap();
        assert!(cursor_web["bundleId"].is_null());
        assert!(cursor_web["webUrl"].is_null());
    }

    #[test]
    fn refusal_running_and_foreign_windows_do_not_move() {
        let refused = rejection("claude-desktop", false, true, true).unwrap();
        assert!(refused.contains("网页分屏"));
        assert!(refused.contains("没有被移动"));
        assert!(rejection("claude-desktop", true, false, true)
            .unwrap()
            .contains("没有在运行"));
        assert!(rejection("claude-desktop", true, true, false)
            .unwrap()
            .contains("没有找到"));
        let other = rejection("cursor-desktop", true, true, true).unwrap();
        assert!(other.contains("指定窗口"));
        assert!(other.contains("不会被移动"));
        assert!(rejection("workbuddy-desktop", true, true, true).is_some());
        assert!(rejection("claude-desktop", true, true, true).is_none());
    }

    #[test]
    fn placeholder_uses_host_top_left_and_appkit_flips_y() {
        let host = rect(100.0, 40.0, 900.0, 700.0);
        let local = rect(200.0, 36.0, 640.0, 600.0);
        assert_eq!(
            placeholder_rect(host, local),
            rect(300.0, 76.0, 640.0, 600.0)
        );
        let primary = 900.0;
        let appkit = rect(100.0, 200.0, 400.0, 300.0);
        assert_eq!(
            appkit_to_ax(appkit, primary),
            rect(100.0, 400.0, 400.0, 300.0)
        );
        let above = rect(0.0, 900.0, 1440.0, 900.0);
        assert_eq!(appkit_to_ax(above, primary).y, -900.0);
        let left = rect(-1280.0, 0.0, 1280.0, 800.0);
        assert_eq!(appkit_to_ax(left, primary).x, -1280.0);
    }

    #[test]
    fn side_by_side_prefers_the_right_and_flips_when_there_is_no_room() {
        let display = rect(0.0, 0.0, 1440.0, 900.0);
        let narrow = side_by_side_rect(rect(100.0, 80.0, 500.0, 600.0), display);
        assert_eq!(narrow.x, 100.0 + 500.0 + SIDE_GAP);
        assert_eq!(narrow.y, 80.0);
        assert_eq!(narrow.width, 500.0);
        assert_eq!(narrow.height, 600.0);
        let wide = side_by_side_rect(rect(700.0, 50.0, 700.0, 600.0), display);
        assert!(
            wide.x < 700.0,
            "place on the left when the right side is short"
        );
        assert!(wide.x >= display.x);
        assert!(wide.x + wide.width <= 700.0);
        let cramped = side_by_side_rect(rect(10.0, 10.0, 1400.0, 800.0), display);
        assert!(cramped.x >= display.x);
        assert!(cramped.x + cramped.width <= display.x + display.width + 0.1);
    }

    #[test]
    fn target_window_is_the_largest_normal_window_of_that_pid() {
        let windows = vec![
            raw(1, 9, 300.0, 300.0),
            {
                let mut tiny = raw(2, 9, 80.0, 400.0);
                tiny.width = 80.0;
                tiny
            },
            {
                let mut hidden = raw(3, 9, 900.0, 700.0);
                hidden.on_screen = false;
                hidden
            },
            {
                let mut menu = raw(4, 9, 900.0, 700.0);
                menu.layer = 25;
                menu
            },
            raw(5, 4, 1200.0, 800.0),
            raw(6, 9, 800.0, 600.0),
        ];
        assert_eq!(pick_target_window(&windows, 9), Some(6));
        assert_eq!(pick_target_window(&windows, 4), Some(5));
        assert_eq!(pick_target_window(&windows, 3), None);
    }

    #[test]
    fn z_order_and_the_window_behind_follow_front_to_back() {
        let order = [8, 3, 5, 1];
        assert_eq!(target_is_above(&order, 3, 5), Some(true));
        assert_eq!(target_is_above(&order, 5, 3), Some(false));
        assert_eq!(target_is_above(&order, 9, 3), None);
        assert_eq!(window_behind(&order, 3), Some(5));
        assert_eq!(window_behind(&order, 1), None);
    }

    #[test]
    fn chrome_removal_and_rollback_leave_no_borderless_side_window() {
        assert!(chrome_removed(true, false, false));
        assert!(chrome_removed(false, true, false));
        assert!(chrome_removed(false, false, true));
        assert!(!chrome_removed(false, false, false));
        assert_eq!(
            after_chrome_and_placeholder(true, true),
            AfterChromeMove::FollowEmbedded
        );
        assert_eq!(
            after_chrome_and_placeholder(true, false),
            AfterChromeMove::RollBackThenBeside
        );
        assert_eq!(
            after_chrome_and_placeholder(false, true),
            AfterChromeMove::RollBackThenBeside
        );
        assert!(rollback_chrome(true, Some(SnapMode::SideBySide)));
        assert!(rollback_chrome(true, None));
        assert!(!rollback_chrome(true, Some(SnapMode::Embedded)));
        assert!(!rollback_chrome(false, Some(SnapMode::SideBySide)));
        assert!(!beside_accepted(false));
        assert!(beside_accepted(true));
    }

    #[test]
    fn follow_writes_only_when_geometry_space_or_z_order_changed() {
        let mut input = follow_input(SnapMode::Embedded);
        input.current = placeholder_rect(input.host, input.local_placeholder);
        let steady = plan_follow(&input);
        assert!(steady.frame.is_none());
        assert!(!steady.raise_above_host);
        assert!(steady.move_to_space.is_none());
        assert!(steady.stop.is_none());

        input.current.x -= 40.0;
        let moved = plan_follow(&input);
        assert_eq!(
            moved.frame,
            Some(placeholder_rect(input.host, input.local_placeholder))
        );

        input.target_above_host = false;
        input.target_space = Some(4);
        let stacked = plan_follow(&input);
        assert!(stacked.raise_above_host);
        assert_eq!(stacked.move_to_space, Some(1));

        input.space_move_failed = true;
        assert!(plan_follow(&input).move_to_space.is_none());

        input.ax_rejected = true;
        assert_eq!(plan_follow(&input).stop, Some(StopReason::AxRejected));
        input.ax_rejected = false;
        input.target_visible = false;
        assert_eq!(plan_follow(&input).stop, Some(StopReason::TargetGone));
        input.target_visible = true;
        input.minimized = true;
        assert_eq!(plan_follow(&input).stop, Some(StopReason::Minimized));
        assert!(plan_follow(&input).frame.is_none());
    }

    #[test]
    fn side_by_side_follow_tracks_the_host_not_the_placeholder() {
        let mut input = follow_input(SnapMode::SideBySide);
        let desired = side_by_side_rect(input.host, input.display);
        input.current = desired;
        assert!(plan_follow(&input).frame.is_none());
        input.host.x += 25.0;
        let plan = plan_follow(&input);
        assert_eq!(
            plan.frame,
            Some(side_by_side_rect(input.host, input.display))
        );
        assert_ne!(
            plan.frame,
            Some(placeholder_rect(input.host, input.local_placeholder))
        );
    }

    #[test]
    fn messages_distinguish_embed_follow_and_refusal() {
        assert!(embedded_message().contains("边框"));
        assert!(side_by_side_message().contains("旁边跟随"));
        assert!(moved_rejected_message().contains("原来的位置"));
    }

    #[test]
    fn follow_interval_targets_sixty_hertz() {
        assert_eq!(FOLLOW_HZ, 60);
        assert_eq!(FOLLOW_INTERVAL_NS, 16_666_666);
        assert!(status().contains("snap"));
    }

    #[test]
    fn recovery_file_roundtrips_and_a_broken_file_is_ignored() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("uniworkspace-snap-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_recovery(&dir).is_none());
        let file = RecoveryFile {
            bundle_id: "com.anthropic.claudefordesktop".into(),
            pid: 42,
            window_id: 7,
            x: 12.0,
            y: 24.0,
            width: 800.0,
            height: 600.0,
            below_window: Some(3),
            space: Some(9),
            chrome: vec![ChromeRecord {
                attribute: "AXTitleUIElement".into(),
                hidden: false,
            }],
            enhanced: Some(false),
            manual: None,
        };
        write_recovery(&dir, &file).unwrap();
        let text = std::fs::read_to_string(recovery_path(&dir)).unwrap();
        assert!(text.contains("com.anthropic.claudefordesktop"));
        assert!(!text.contains("com.tencent.workbuddy"));
        assert!(!text.contains("com.todesktop"));
        assert_eq!(read_recovery(&dir), Some(file));
        std::fs::write(recovery_path(&dir), "{").unwrap();
        assert!(read_recovery(&dir).is_none());
        clear_recovery(&dir);
        assert!(read_recovery(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unusable_frames_are_rejected() {
        assert!(frame_usable(&SnapFrame {
            x: 1.0,
            y: 2.0,
            width: 100.0,
            height: 80.0,
        }));
        assert!(!frame_usable(&SnapFrame {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 80.0,
        }));
        assert!(!frame_usable(&SnapFrame {
            x: f64::NAN,
            y: 0.0,
            width: 100.0,
            height: 80.0,
        }));
    }
}
