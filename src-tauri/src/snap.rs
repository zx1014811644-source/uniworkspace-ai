//! Geometric snap for every desktop app whose Bundle ID is confirmed.
//!
//! macOS cannot reparent a foreign window, so this module never calls
//! `SetParent` or `addChildWindow`. The shell punches a placeholder in one
//! pane. Accessibility removes the target window's chrome and aligns it to
//! that rectangle. A display link (or a 60fps timer if the link cannot start)
//! keeps position, size, Space, and z-order in step with the host window.
//! Detach, pane unload, host exit, and the target quitting first put the
//! chrome and stacking back. A borderless window must not be left behind.
//!
//! If Accessibility will not remove the chrome or will not accept the
//! placeholder rectangle, the same follow loop places the window beside the
//! host instead. Window mirroring is not implemented.
//!
//! One confirmed app is snapped at a time. Attaching another restores the
//! previous window first. Manual 「指定窗口」 stays a list. It does not call
//! into this module. Apps with no confirmed Bundle ID stay Web-only.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::detector::{self, RawWindow};

pub const FOLLOW_HZ: u64 = 60;
pub const FOLLOW_INTERVAL_NS: u64 = 1_000_000_000 / FOLLOW_HZ;
/// Sub-pixel disagreement between Accessibility and the window list.
/// Follow ignores anything inside this band so the window is not rewritten
/// every vsync. A slight gap while dragging is acceptable.
pub const FOLLOW_EPSILON: f64 = 2.0;
/// Failed position writes in a row before follow gives up and restores chrome.
pub const FOLLOW_WRITE_BUDGET: u32 = 45;
/// Display frames to wait while a fullscreen window is leaving fullscreen.
pub const FULLSCREEN_EXIT_BUDGET: u32 = 90;
pub const SIDE_GAP: f64 = 12.0;
pub const SIDE_MIN_ROOM: f64 = 320.0;
pub const MIN_TARGET_EDGE: f64 = 200.0;
pub const RECOVERY_FILE: &str = "snap-recovery.json";
/// Week 6 accepts one snapped native window. Week 7 does not raise this.
pub const NATIVE_SNAP_LIMIT: usize = 1;
/// A later week may follow more than one native window, and must not pass this.
pub const NATIVE_SNAP_CEILING: usize = 2;

pub fn can_start_another_native_snap(active: usize) -> bool {
    active < NATIVE_SNAP_LIMIT
}

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "snap: confirmed desktop geometric follow"
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
    /// `AXFullScreen` before the first mutation. `None` means it was not readable.
    #[serde(default)]
    pub fullscreen: Option<bool>,
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
    /// Every Space the host window currently belongs to. A window on a Space
    /// edge is listed on more than one. Empty means the Space is unknown.
    pub host_spaces: Vec<u64>,
    pub target_spaces: Vec<u64>,
    pub target_above_host: bool,
    pub ax_rejected: bool,
    pub target_visible: bool,
    pub target_process_alive: bool,
    pub minimized: bool,
    pub space_move_failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterChromeMove {
    FollowEmbedded,
    RollBackThenBeside,
}

pub fn is_snap_target(app_id: &str) -> bool {
    target_parts(app_id).is_some()
}

pub fn target_parts(app_id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    detector::CONFIRMED_DESKTOP
        .iter()
        .find(|app| app.id == app_id && !app.bundle_id.is_empty())
        .map(|app| (app.id, app.name, app.bundle_id))
}

pub fn confirmed_bundle(bundle_id: &str) -> bool {
    !bundle_id.is_empty()
        && detector::CONFIRMED_DESKTOP
            .iter()
            .any(|app| app.bundle_id == bundle_id)
}

pub fn idle_status() -> SnapStatus {
    SnapStatus {
        target_app_id: String::new(),
        target_name: String::new(),
        target_bundle_id: String::new(),
        active: false,
        mode: SnapMode::Inactive,
        message: None,
    }
}

pub fn status_for(id: &str, name: &str, bundle: &str) -> SnapStatus {
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
/// `web_only` is the in-app choice. It blocks moves even when Accessibility
/// is already granted, so 「仅使用网页」 cannot drag a desktop window.
pub fn rejection(
    app_id: &str,
    trusted: bool,
    web_only: bool,
    running: bool,
    has_window: bool,
) -> Option<String> {
    let Some((_, name, _)) = target_parts(app_id) else {
        return Some(
            "只有目录里已确认 Bundle ID 的桌面应用可以吸附。指定窗口和其他应用不会被移动。"
                .to_string(),
        );
    };
    if web_only {
        return Some(format!(
            "当前是仅使用网页。需要辅助功能才会吸附。网页分屏可以继续使用，{name} 没有被移动。"
        ));
    }
    if !trusted {
        return Some(format!(
            "需要辅助功能才能吸附。网页分屏可以继续使用，{name} 没有被移动。"
        ));
    }
    if !running {
        return Some(format!("{name} 没有在运行。"));
    }
    if !has_window {
        return Some(format!("没有找到 {name} 可移动的窗口。"));
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

/// The recovery file may be deleted only after every chrome write that still
/// matters has succeeded. A frame write is required only when this restore is
/// supposed to move the window.
pub fn recovery_may_clear(chrome_ok: bool, frame_requested: bool, frame_ok: bool) -> bool {
    chrome_ok && (!frame_requested || frame_ok)
}

/// A failed Accessibility write does not fail restore when the live value is
/// already the saved one. Side-by-side follow often rewrites read-only chrome
/// that was never changed. A value that is still different, or that cannot be
/// read, still blocks clearing the recovery file.
pub fn chrome_write_blocks_restore(saved: bool, current: Option<bool>, write_ok: bool) -> bool {
    if write_ok {
        return false;
    }
    !matches!(current, Some(value) if value == saved)
}

pub fn recovery_blocked_message() -> &'static str {
    "上一个窗口的边框还没还原，这次没有开始新的吸附，也没有覆盖还原记录。"
}

/// A fullscreen target has to leave fullscreen before its frame can match a pane.
pub fn should_exit_fullscreen(is_fullscreen: bool) -> bool {
    is_fullscreen
}

/// While fullscreen is still exiting, skip the frame write for this vsync.
pub fn hold_for_fullscreen_exit(is_fullscreen: bool) -> bool {
    is_fullscreen
}

/// Move only after the Space change has committed.
/// Sharing any Space means the window is still on the edge between them.
/// An unknown target Space is not spammed with moves.
pub fn space_move_needed(host: &[u64], target: &[u64], failed: bool) -> Option<u64> {
    if failed || host.is_empty() || target.is_empty() {
        return None;
    }
    if target.iter().any(|space| host.contains(space)) {
        return None;
    }
    host.first().copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayLinkPlan {
    Keep,
    Retarget(u32),
}

/// Bind the display link to the display under the host. An unknown display
/// keeps whatever link is already running.
pub fn follow_display(host_display: Option<u32>, link_display: Option<u32>) -> DisplayLinkPlan {
    match host_display {
        Some(host) if link_display == Some(host) => DisplayLinkPlan::Keep,
        Some(host) => DisplayLinkPlan::Retarget(host),
        None => DisplayLinkPlan::Keep,
    }
}

/// A display that already failed to create a link must not be retried on every
/// vsync once follow has a link or the 60fps timer. A different display gets
/// one new attempt. If neither source is running, try again.
pub fn skip_repeated_display_link(
    attempted: Option<u32>,
    display: u32,
    link_running: bool,
    timer_running: bool,
) -> bool {
    attempted == Some(display) && (link_running || timer_running)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameWrite {
    pub position: Option<(f64, f64)>,
    pub size: Option<(f64, f64)>,
}

/// Size and position are separate Accessibility writes. A drag that only
/// changes the origin must not also set the size, which makes Electron
/// windows relayout and jitter.
pub fn frame_write(current: Rect, desired: Rect, epsilon: f64) -> FrameWrite {
    let position =
        if (current.x - desired.x).abs() > epsilon || (current.y - desired.y).abs() > epsilon {
            Some((desired.x, desired.y))
        } else {
            None
        };
    let size = if (current.width - desired.width).abs() > epsilon
        || (current.height - desired.height).abs() > epsilon
    {
        Some((desired.width, desired.height))
    } else {
        None
    };
    FrameWrite { position, size }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Wrote,
    WaitingForFullscreen,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowHealth {
    pub write_failures: u32,
    pub fullscreen_waits: u32,
}

pub fn advance_follow_health(health: FollowHealth, outcome: WriteOutcome) -> (FollowHealth, bool) {
    match outcome {
        WriteOutcome::Wrote => (
            FollowHealth {
                write_failures: 0,
                fullscreen_waits: 0,
            },
            false,
        ),
        WriteOutcome::WaitingForFullscreen => {
            let fullscreen_waits = health.fullscreen_waits.saturating_add(1);
            let stop = fullscreen_waits >= FULLSCREEN_EXIT_BUDGET;
            (
                FollowHealth {
                    write_failures: 0,
                    fullscreen_waits,
                },
                stop,
            )
        }
        WriteOutcome::Failed => {
            let write_failures = health.write_failures.saturating_add(1);
            let stop = write_failures >= FOLLOW_WRITE_BUDGET;
            (
                FollowHealth {
                    write_failures,
                    fullscreen_waits: health.fullscreen_waits,
                },
                stop,
            )
        }
    }
}

/// Delete the recovery file after the target disappears only when chrome was
/// restored, or when no Accessibility window remains to be left borderless.
pub fn clear_recovery_after_target_gone(ax_window_alive: bool, chrome_restored: bool) -> bool {
    !ax_window_alive || chrome_restored
}

pub fn unreadable_window_message(name: &str) -> String {
    format!("读不到 {name} 的窗口，没有移动它。")
}

pub fn unreadable_frame_message(name: &str) -> String {
    format!("读不到 {name} 的窗口位置，没有移动它。")
}

pub fn host_missing_message(name: &str) -> String {
    format!("找不到主窗口，没有移动 {name}。")
}

pub fn chrome_restore_failed_message(name: &str) -> String {
    format!("没能还原 {name} 的边框。下次启动会再试。")
}

pub fn target_gone_message(name: &str) -> String {
    format!("{name} 已退出或窗口已关闭，吸附结束。")
}

pub fn minimized_message(name: &str) -> String {
    format!("{name} 已最小化，边框已还原，吸附结束。")
}

pub fn plan_follow(input: &FollowInput) -> FollowPlan {
    if !input.target_process_alive || !input.target_visible {
        return FollowPlan {
            frame: None,
            raise_above_host: false,
            move_to_space: None,
            stop: Some(StopReason::TargetGone),
        };
    }
    if input.minimized {
        return FollowPlan {
            frame: None,
            raise_above_host: false,
            move_to_space: None,
            stop: Some(StopReason::Minimized),
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
    let frame = if rects_close(desired, input.current, FOLLOW_EPSILON) {
        None
    } else {
        Some(desired)
    };
    FollowPlan {
        frame,
        raise_above_host: !input.target_above_host,
        move_to_space: space_move_needed(
            &input.host_spaces,
            &input.target_spaces,
            input.space_move_failed,
        ),
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
    let path = recovery_path(dir);
    if path.exists() {
        return Err(recovery_blocked_message().to_string());
    }
    let body =
        serde_json::to_vec_pretty(file).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    let tmp = dir.join("snap-recovery.json.tmp");
    std::fs::write(&tmp, &body).map_err(|err| format!("无法记录吸附还原信息: {err}"))?;
    if path.exists() {
        let _ = std::fs::remove_file(&tmp);
        return Err(recovery_blocked_message().to_string());
    }
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
            host_spaces: vec![1],
            target_spaces: vec![1],
            target_above_host: true,
            ax_rejected: false,
            target_visible: true,
            target_process_alive: true,
            minimized: false,
            space_move_failed: false,
        }
    }

    #[test]
    fn every_confirmed_desktop_app_can_snap_and_pending_ids_cannot() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/apps.json")).unwrap();
        let apps = catalog["apps"].as_array().unwrap();
        let expected = [
            ("chatgpt-desktop", "com.openai.codex", "ChatGPT Desktop"),
            (
                "claude-desktop",
                "com.anthropic.claudefordesktop",
                "Claude Desktop",
            ),
            ("lm-studio", "ai.elementlabs.lmstudio", "LM Studio"),
            ("gemini-desktop", "com.google.GeminiMacOS", "Gemini Desktop"),
            ("kimi-desktop", "com.moonshot.kimichat", "Kimi Desktop"),
            ("doubao-desktop", "com.bot.pc.doubao", "豆包 Desktop"),
            (
                "cursor-desktop",
                "com.todesktop.230313mzl4w4u92",
                "Cursor Desktop",
            ),
        ];
        assert_eq!(expected.len(), detector::CONFIRMED_DESKTOP.len());
        for (id, bundle, name) in expected {
            let app = apps.iter().find(|app| app["id"] == id).unwrap();
            assert_eq!(app["bundleId"], bundle, "{id}");
            assert_eq!(app["bundleIdStatus"], "confirmed", "{id}");
            assert_eq!(app["kind"], "native", "{id}");
            assert_eq!(app["name"], name, "{id}");
            let (found_id, found_name, found_bundle) = target_parts(id).unwrap();
            assert_eq!(found_id, id);
            assert_eq!(found_name, name);
            assert_eq!(found_bundle, bundle);
            assert!(is_snap_target(id), "{id}");
            assert!(confirmed_bundle(bundle), "{bundle}");
            assert!(rejection(id, true, false, true, true).is_none(), "{id}");
        }
        for id in [
            "cursor",
            "claude",
            "chatgpt",
            "workbuddy-desktop",
            "harness-desktop",
            "accio-work-desktop",
            "deepseek",
        ] {
            assert!(!is_snap_target(id), "{id}");
            assert!(rejection(id, true, false, true, true).is_some(), "{id}");
        }
        for id in ["workbuddy-desktop", "harness-desktop", "accio-work-desktop"] {
            let app = apps.iter().find(|app| app["id"] == id).unwrap();
            assert!(app["bundleId"].is_null(), "{id}");
            assert_eq!(app["bundleIdStatus"], "pending", "{id}");
        }
        for id in ["accio-work", "cursor", "harness"] {
            let app = apps.iter().find(|app| app["id"] == id).unwrap();
            assert!(app["webUrl"].is_null(), "{id}");
            assert_eq!(app["webStatus"], "pending", "{id}");
        }
        assert!(detector::CONFIRMED_DESKTOP.iter().all(|app| {
            app.id == "cursor-desktop" || app.bundle_id != "com.todesktop.230313mzl4w4u92"
        }));
        let cursor_web = apps.iter().find(|app| app["id"] == "cursor").unwrap();
        assert!(cursor_web["bundleId"].is_null());
        assert!(!confirmed_bundle("com.tencent.workbuddy.mac"));
        assert!(!confirmed_bundle("com.deepseek.dsh"));
        assert!(!confirmed_bundle(""));
    }

    #[test]
    fn refusal_running_and_foreign_windows_do_not_move() {
        let refused = rejection("claude-desktop", false, false, true, true).unwrap();
        assert!(refused.contains("网页分屏"));
        assert!(refused.contains("没有被移动"));
        assert!(refused.contains("需要辅助功能"));
        assert!(refused.contains("Claude Desktop"));
        assert!(!refused.contains("仅使用网页"));
        let web_only = rejection("claude-desktop", true, true, true, true).unwrap();
        assert!(web_only.contains("仅使用网页"));
        assert!(web_only.contains("没有被移动"));
        assert!(web_only.contains("需要辅助功能"));
        assert!(web_only.contains("网页分屏"));
        let stopped = rejection("lm-studio", true, false, false, true).unwrap();
        assert!(stopped.contains("没有在运行"));
        assert!(stopped.contains("LM Studio"));
        assert!(rejection("chatgpt-desktop", true, false, true, false)
            .unwrap()
            .contains("没有找到"));
        let other = rejection("workbuddy-desktop", true, false, true, true).unwrap();
        assert!(other.contains("指定窗口"));
        assert!(other.contains("不会被移动"));
        assert!(rejection("harness-desktop", true, false, true, true).is_some());
        assert!(rejection("cursor-desktop", true, false, true, true).is_none());
        assert!(rejection("claude-desktop", true, false, true, true).is_none());
        assert_eq!(NATIVE_SNAP_LIMIT, 1);
        assert_eq!(NATIVE_SNAP_CEILING, 2);
        assert!(NATIVE_SNAP_LIMIT <= NATIVE_SNAP_CEILING);
        assert!(can_start_another_native_snap(0));
        assert!(!can_start_another_native_snap(NATIVE_SNAP_LIMIT));
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
        assert!(
            !chrome_write_blocks_restore(false, Some(false), false),
            "an unchanged read-only value is already restored"
        );
        assert!(!chrome_write_blocks_restore(true, Some(true), true));
        assert!(chrome_write_blocks_restore(false, Some(true), false));
        assert!(chrome_write_blocks_restore(false, None, false));
        assert!(recovery_may_clear(true, true, true));
        assert!(
            !recovery_may_clear(false, false, true),
            "a real chrome failure still keeps the recovery file"
        );
        assert!(!recovery_may_clear(false, true, true));
        assert!(!recovery_may_clear(false, false, true));
        assert!(!recovery_may_clear(true, true, false));
        assert!(recovery_may_clear(true, false, false));
        assert!(recovery_may_clear(true, true, true));
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
        input.target_spaces = vec![4];
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
        input.target_process_alive = false;
        assert_eq!(plan_follow(&input).stop, Some(StopReason::TargetGone));
        input.target_process_alive = true;
        input.minimized = true;
        assert_eq!(plan_follow(&input).stop, Some(StopReason::Minimized));
        assert!(plan_follow(&input).frame.is_none());

        input.minimized = false;
        input.current = placeholder_rect(input.host, input.local_placeholder);
        input.current.x += 1.0;
        assert!(
            plan_follow(&input).frame.is_none(),
            "a 1px disagreement stays inside the follow gap"
        );
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
            fullscreen: Some(false),
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
    fn a_new_snap_does_not_overwrite_an_unrestored_recovery_file() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("uniworkspace-snap-keep-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        let original = RecoveryFile {
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
            fullscreen: Some(false),
        };
        write_recovery(&dir, &original).unwrap();
        let replacement = RecoveryFile {
            bundle_id: "com.openai.codex".into(),
            pid: 90,
            window_id: 11,
            x: 1.0,
            y: 1.0,
            width: 400.0,
            height: 300.0,
            below_window: None,
            space: None,
            chrome: vec![ChromeRecord {
                attribute: "AXCloseButton".into(),
                hidden: true,
            }],
            enhanced: None,
            manual: None,
            fullscreen: None,
        };
        let err = write_recovery(&dir, &replacement).unwrap_err();
        assert!(err.contains("没有覆盖"));
        assert_eq!(read_recovery(&dir).unwrap().chrome, original.chrome);
        assert_eq!(
            read_recovery(&dir).unwrap().bundle_id,
            "com.anthropic.claudefordesktop"
        );
        assert!(!recovery_may_clear(false, true, true));
        assert!(read_recovery(&dir).is_some());
        clear_recovery(&dir);
        write_recovery(&dir, &replacement).unwrap();
        assert_eq!(read_recovery(&dir).unwrap().bundle_id, "com.openai.codex");
        clear_recovery(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_recovery_without_fullscreen_still_loads() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("uniworkspace-snap-legacy-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            recovery_path(&dir),
            r#"{
              "bundleId": "com.anthropic.claudefordesktop",
              "pid": 7,
              "windowId": 9,
              "x": 1.0,
              "y": 2.0,
              "width": 300.0,
              "height": 200.0,
              "belowWindow": null,
              "space": 3,
              "chrome": [],
              "enhanced": null,
              "manual": null
            }"#,
        )
        .unwrap();
        let file = read_recovery(&dir).unwrap();
        assert_eq!(file.bundle_id, "com.anthropic.claudefordesktop");
        assert!(file.fullscreen.is_none());
        clear_recovery(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn space_edges_wait_until_the_windows_no_longer_share_a_space() {
        assert_eq!(space_move_needed(&[1, 2], &[2], false), None);
        assert_eq!(space_move_needed(&[2], &[1, 2], false), None);
        assert_eq!(space_move_needed(&[4], &[1], false), Some(4));
        assert_eq!(space_move_needed(&[4], &[1], true), None);
        assert_eq!(space_move_needed(&[], &[1], false), None);
        assert_eq!(space_move_needed(&[4], &[], false), None);
        assert_eq!(space_move_needed(&[8, 9], &[8], false), None);
    }

    #[test]
    fn fullscreen_exit_and_follow_health_do_not_drop_a_window_early() {
        assert!(should_exit_fullscreen(true));
        assert!(!should_exit_fullscreen(false));
        assert!(hold_for_fullscreen_exit(true));
        assert!(!hold_for_fullscreen_exit(false));
        let health = FollowHealth {
            write_failures: 0,
            fullscreen_waits: 0,
        };
        let (waiting, stop) = advance_follow_health(health, WriteOutcome::WaitingForFullscreen);
        assert!(!stop);
        assert_eq!(waiting.fullscreen_waits, 1);
        assert_eq!(waiting.write_failures, 0);
        let mut health = waiting;
        let mut stopped = false;
        for _ in 0..(FULLSCREEN_EXIT_BUDGET - 1) {
            let (next, stop) = advance_follow_health(health, WriteOutcome::WaitingForFullscreen);
            health = next;
            stopped = stop;
        }
        assert!(stopped);
        let (failed, stop) = advance_follow_health(
            FollowHealth {
                write_failures: FOLLOW_WRITE_BUDGET - 2,
                fullscreen_waits: 4,
            },
            WriteOutcome::Failed,
        );
        assert!(!stop);
        assert_eq!(failed.write_failures, FOLLOW_WRITE_BUDGET - 1);
        let (_, stop) = advance_follow_health(failed, WriteOutcome::Failed);
        assert!(stop);
        let (reset, stop) = advance_follow_health(failed, WriteOutcome::Wrote);
        assert!(!stop);
        assert_eq!(reset.write_failures, 0);
        assert_eq!(reset.fullscreen_waits, 0);
    }

    #[test]
    fn follow_gap_skips_a_size_write_when_only_the_origin_moved() {
        let current = rect(10.0, 20.0, 400.0, 300.0);
        let moved = rect(18.0, 20.0, 400.0, 300.0);
        let write = frame_write(current, moved, FOLLOW_EPSILON);
        assert_eq!(write.position, Some((18.0, 20.0)));
        assert!(write.size.is_none());
        let jitter = frame_write(current, rect(11.0, 20.5, 401.0, 300.0), FOLLOW_EPSILON);
        assert!(jitter.position.is_none());
        assert!(jitter.size.is_none());
        let resized = frame_write(current, rect(10.0, 20.0, 640.0, 300.0), FOLLOW_EPSILON);
        assert!(resized.position.is_none());
        assert_eq!(resized.size, Some((640.0, 300.0)));
    }

    #[test]
    fn display_link_follows_the_host_display_and_keeps_a_match() {
        assert_eq!(follow_display(Some(3), Some(3)), DisplayLinkPlan::Keep);
        assert_eq!(
            follow_display(Some(5), Some(3)),
            DisplayLinkPlan::Retarget(5)
        );
        assert_eq!(follow_display(Some(5), None), DisplayLinkPlan::Retarget(5));
        assert_eq!(follow_display(None, Some(3)), DisplayLinkPlan::Keep);
        assert_eq!(follow_display(None, None), DisplayLinkPlan::Keep);
        assert!(skip_repeated_display_link(Some(5), 5, false, true));
        assert!(skip_repeated_display_link(Some(5), 5, true, false));
        assert!(!skip_repeated_display_link(Some(5), 5, false, false));
        assert!(!skip_repeated_display_link(Some(5), 8, false, true));
        assert!(!skip_repeated_display_link(None, 5, false, true));
    }

    #[test]
    fn a_quit_keeps_recovery_when_the_borderless_window_is_still_there() {
        assert!(!clear_recovery_after_target_gone(true, false));
        assert!(clear_recovery_after_target_gone(true, true));
        assert!(clear_recovery_after_target_gone(false, false));
        assert!(clear_recovery_after_target_gone(false, true));
        assert!(target_gone_message("ChatGPT Desktop").contains("ChatGPT Desktop"));
        assert!(target_gone_message("LM Studio").contains("吸附结束"));
        assert!(minimized_message("Gemini Desktop").contains("边框已还原"));
        assert!(chrome_restore_failed_message("Kimi Desktop").contains("下次启动会再试"));
        assert!(unreadable_window_message("豆包 Desktop").contains("没有移动"));
        assert!(host_missing_message("Cursor Desktop").contains("没有移动"));
        assert!(status().contains("snap"));
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
