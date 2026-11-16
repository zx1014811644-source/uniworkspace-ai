//! macOS geometric snap.
//!
//! The foreign window stays a top-level window. Nothing here reparents it.
//! Accessibility hides the title chrome and writes position and size. A
//! display link, or a 60fps timer when the link cannot start, repeats that
//! write and keeps the window above the host and on the host's Space.
//! The shell window stays opaque. The placeholder is a clear `NSView` in the
//! pane plus the rectangle the shell measures for that pane.
//!
//! All Accessibility calls run on the main thread. The display-link thread
//! only schedules that work.

use std::cell::RefCell;
use std::ffi::{c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use tauri::{AppHandle, Emitter, Manager};

use crate::detector::{self, RawWindow};
use crate::snap::{
    advance_follow_health, after_chrome_and_placeholder, beside_accepted, chrome_removed,
    chrome_restore_failed_message, clear_recovery_after_target_gone, confirmed_bundle,
    embedded_message, follow_display, frame_usable, frame_write, hold_for_fullscreen_exit,
    host_missing_message, idle_status, minimized_message, moved_rejected_message,
    pick_target_window, placeholder_rect, plan_follow, read_recovery, recovery_may_clear,
    rejection, rollback_chrome, should_exit_fullscreen, side_by_side_message, side_by_side_rect,
    skip_repeated_display_link, status_for, support_dir_for, target_gone_message, target_is_above,
    target_parts, unreadable_frame_message, unreadable_window_message, window_behind,
    write_recovery, AfterChromeMove, ChromeRecord, DisplayLinkPlan, FollowHealth, FollowInput,
    RecoveryFile, Rect, SnapFrame, SnapMode, SnapStatus, StopReason, WriteOutcome, FOLLOW_EPSILON,
    FOLLOW_INTERVAL_NS,
};
use crate::window_mac;

const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;
const AX_SUCCESS: i32 = 0;
const SIDE_TIMEOUT: f32 = 1.0;
const FOLLOW_TIMEOUT: f32 = 0.08;
const RTLD_LAZY: i32 = 1;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NsSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect {
    origin: NsPoint,
    size: NsSize,
}

unsafe impl Encode for NsPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

unsafe impl Encode for NsSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

unsafe impl Encode for NsRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[NsPoint::ENCODING, NsSize::ENCODING]);
}

struct Owned(*mut c_void);

impl Owned {
    fn new(ptr: *mut c_void) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    fn get(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        release(self.0);
    }
}

struct Session {
    app: AppHandle,
    generation: u64,
    mode: SnapMode,
    app_id: String,
    name: String,
    bundle_id: String,
    pid: i32,
    window_id: u32,
    logical: Rect,
    original: RecoveryFile,
    ax_rejected: bool,
    space_failed: bool,
    attempted_space: Option<u64>,
    write_failures: u32,
    fullscreen_waits: u32,
    message: String,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = RefCell::new(None);
    static PLACEHOLDER: RefCell<*mut AnyObject> = const { RefCell::new(ptr::null_mut()) };
    static LAST_NOTE: RefCell<Option<String>> = RefCell::new(None);
}

static GENERATION: AtomicU64 = AtomicU64::new(1);
static FRAME_QUEUED: AtomicBool = AtomicBool::new(false);
static APPLYING: AtomicBool = AtomicBool::new(false);
static LINK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static TIMER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// `u32::MAX` means the link is not bound to one display.
static LINK_DISPLAY: AtomicU32 = AtomicU32::new(u32::MAX);
/// Last display a retarget was attempted for. Stops a refused display from
/// restarting the link on every vsync.
static LINK_ATTEMPTED: AtomicU32 = AtomicU32::new(u32::MAX);
const LINK_UNBOUND: u32 = u32::MAX;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: *const c_void,
        value: *mut *mut c_void,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: *mut c_void,
        attribute: *const c_void,
        value: *const c_void,
    ) -> i32;
    fn AXUIElementCopyAttributeNames(element: *mut c_void, names: *mut *mut c_void) -> i32;
    fn AXUIElementPerformAction(element: *mut c_void, action: *const c_void) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *mut c_void, timeout: f32) -> i32;
    fn AXValueCreate(kind: u32, value: *const c_void) -> *mut c_void;
    fn AXValueGetType(value: *const c_void) -> u32;
    fn AXValueGetValue(value: *const c_void, kind: u32, out: *mut c_void) -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeArrayCallBacks: u8;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const *const c_void,
        num_values: isize,
        callbacks: *const c_void,
    ) -> *mut c_void;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFNumberCreate(allocator: *const c_void, kind: i32, value: *const c_void) -> *mut c_void;
    fn CFNumberGetValue(number: *const c_void, kind: i32, value: *mut c_void) -> u8;
    fn CFRelease(value: *const c_void);
    fn CFRetain(value: *const c_void) -> *const c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CgRect;
    fn CGMainDisplayID() -> u32;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(link: *mut *mut c_void) -> i32;
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, link: *mut *mut c_void) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        link: *mut c_void,
        callback: extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_void,
            u64,
            *mut u64,
            *mut c_void,
        ) -> i32,
        user: *mut c_void,
    ) -> i32;
    fn CVDisplayLinkStart(link: *mut c_void) -> i32;
    fn CVDisplayLinkStop(link: *mut c_void) -> i32;
    fn CVDisplayLinkRelease(link: *mut c_void);
}

#[link(name = "System", kind = "dylib")]
extern "C" {
    fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    /// `dispatch_get_main_queue()` is a macro for this object in current SDKs.
    static _dispatch_main_q: u8;
    fn dispatch_async_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_source_create(
        kind: *const c_void,
        handle: usize,
        mask: usize,
        queue: *mut c_void,
    ) -> *mut c_void;
    fn dispatch_source_set_timer(source: *mut c_void, start: u64, interval: u64, leeway: u64);
    fn dispatch_source_set_event_handler_f(
        source: *mut c_void,
        handler: extern "C" fn(*mut c_void),
    );
    fn dispatch_resume(object: *mut c_void);
    fn dispatch_source_cancel(source: *mut c_void);
    fn dispatch_release(object: *mut c_void);
    static _dispatch_source_type_timer: u8;
    fn kill(pid: i32, sig: i32) -> i32;
}

type GetWindowFn = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type ConnectionFn = unsafe extern "C" fn() -> i32;
type OrderFn = unsafe extern "C" fn(i32, u32, i32, u32) -> i32;
type SpacesFn = unsafe extern "C" fn(i32, i32, *const c_void) -> *mut c_void;
type MoveSpaceFn = unsafe extern "C" fn(i32, *const c_void, u64);

struct Sky {
    connection: ConnectionFn,
    order: Option<OrderFn>,
    spaces: Option<SpacesFn>,
    move_space: Option<MoveSpaceFn>,
}

pub(crate) fn attach(
    app: &AppHandle,
    app_id: &str,
    frame: SnapFrame,
) -> Result<SnapStatus, String> {
    let app = app.clone();
    let hopped = app.clone();
    let app_id = app_id.to_string();
    on_main(&hopped, move || attach_here(app, app_id, frame))?
}

pub(crate) fn update(app: &AppHandle, frame: SnapFrame) -> Result<SnapStatus, String> {
    let app = app.clone();
    let hopped = app.clone();
    on_main(&hopped, move || {
        SESSION.with(|slot| {
            if let Some(session) = slot.borrow_mut().as_mut() {
                session.logical = frame.as_rect();
            }
        });
        if SESSION.with(|slot| slot.borrow().is_some()) {
            if let Some(height) = host_height(&app) {
                sync_placeholder(&app, frame.as_rect(), height);
            }
            apply_follow();
        }
        current_status()
    })
}

pub(crate) fn detach(app: &AppHandle) -> Result<SnapStatus, String> {
    let app = app.clone();
    let hopped = app.clone();
    on_main(&hopped, move || {
        let identity = session_identity();
        restore_active(&app, true)?;
        let mut status = identity;
        status.active = false;
        status.mode = SnapMode::Inactive;
        status.message = Some("已还原边框和层级。".to_string());
        LAST_NOTE.with(|note| note.replace(None));
        Ok(status)
    })?
}

pub(crate) fn recover(app: &AppHandle) {
    let app = app.clone();
    let hopped = app.clone();
    let _ = hopped.run_on_main_thread(move || {
        let _ = apply_recovery_file(&app);
    });
}

pub(crate) fn restore_for_exit() {
    stop_sources();
    GENERATION.fetch_add(1, Ordering::AcqRel);
    let session = SESSION.with(|slot| slot.borrow_mut().take());
    if let Some(session) = session {
        let host = host_window_id(&session.app);
        let restored = apply_saved(&session.original, true, host);
        if restored {
            if let Ok(dir) = support_dir_for(&session.app) {
                crate::snap::clear_recovery(&dir);
            }
        }
    }
    remove_placeholder();
}

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

fn attach_here(app: AppHandle, app_id: String, frame: SnapFrame) -> Result<SnapStatus, String> {
    if !frame_usable(&frame) {
        return Err("占位区域还没有准备好。".into());
    }
    if SESSION.with(|slot| slot.borrow().is_some()) {
        restore_active(&app, true)?;
    }
    let _ = apply_recovery_file(&app);

    let trusted = window_mac::trusted_on_caller_thread();
    let snapshot = window_mac::snapshot_on_caller_thread()?;
    let running = detector::detect_confirmed(&snapshot.processes)
        .into_iter()
        .find(|item| item.app_id == app_id);
    let pid = running.as_ref().map(|item| item.pid).unwrap_or(0);
    let window_id = if pid > 0 {
        pick_target_window(&snapshot.windows, pid).unwrap_or(0)
    } else {
        0
    };
    if let Some(message) = rejection(&app_id, trusted, pid > 0, window_id != 0) {
        return Err(message);
    }
    let (_, name, bundle) = target_parts(&app_id)
        .ok_or_else(|| "这个应用不在已确认的桌面目录里，没有移动它。".to_string())?;
    let name = name.to_string();
    let bundle = bundle.to_string();
    let host = host_rect(&snapshot.windows, &app).ok_or_else(|| host_missing_message(&name))?;
    let local = frame.as_rect();
    let placeholder = placeholder_rect(host, local);
    let display = display_containing(host.x + host.width / 2.0, host.y + host.height / 2.0);

    let originals = with_window(pid, window_id, SIDE_TIMEOUT, |application, window| {
        read_originals(application, window)
    })
    .ok_or_else(|| unreadable_window_message(&name))?;
    let Some(original_frame) = originals.frame else {
        revert_flags(pid, window_id, &originals);
        return Err(unreadable_frame_message(&name));
    };
    let order: Vec<u32> = snapshot
        .windows
        .iter()
        .map(|window| window.window_id)
        .collect();
    let recovery = RecoveryFile {
        bundle_id: bundle.clone(),
        pid,
        window_id,
        x: original_frame.x,
        y: original_frame.y,
        width: original_frame.width,
        height: original_frame.height,
        below_window: window_behind(&order, window_id),
        space: spaces_of(window_id).into_iter().next(),
        chrome: originals.chrome.clone(),
        enhanced: originals.enhanced,
        manual: originals.manual,
        fullscreen: originals.fullscreen,
    };
    let dir = support_dir_for(&app)?;
    if let Err(err) = write_recovery(&dir, &recovery) {
        revert_flags(pid, window_id, &originals);
        return Err(err);
    }

    let moved = with_window(pid, window_id, SIDE_TIMEOUT, |application, window| {
        if originals.enhanced != Some(true) {
            let _ = set_app_bool(application, "AXEnhancedUserInterface", true);
        }
        if originals.manual != Some(true) {
            let _ = set_app_bool(application, "AXManualAccessibility", true);
        }
        if should_exit_fullscreen(originals.fullscreen == Some(true)) {
            let _ = exit_fullscreen(window);
        }
        hide_chrome(window, &originals)
    });
    let (removed, dirty) = moved.unwrap_or((false, false));
    let mut placeholder_ok = false;
    if removed {
        placeholder_ok = with_window(pid, window_id, SIDE_TIMEOUT, |_application, window| {
            set_frame(window, placeholder)
        })
        .unwrap_or(false);
    }

    let mode = match after_chrome_and_placeholder(removed, placeholder_ok) {
        AfterChromeMove::FollowEmbedded => SnapMode::Embedded,
        AfterChromeMove::RollBackThenBeside => {
            let _ = with_window(pid, window_id, SIDE_TIMEOUT, |application, window| {
                restore_records(application, window, &recovery);
            });
            if placeholder_ok {
                let _ = with_window(pid, window_id, SIDE_TIMEOUT, |_application, window| {
                    set_frame(window, original_frame)
                });
            }
            let side = side_by_side_rect(host, display);
            let side_ok = with_window(pid, window_id, SIDE_TIMEOUT, |_application, window| {
                set_frame(window, side)
            })
            .unwrap_or(false);
            if !beside_accepted(side_ok) {
                let restored = with_window(pid, window_id, SIDE_TIMEOUT, |application, window| {
                    let chrome_ok = restore_records(application, window, &recovery);
                    let frame_ok = set_frame(window, original_frame);
                    if let Some(fullscreen) = recovery.fullscreen {
                        let _ = set_fullscreen(window, fullscreen);
                    }
                    recovery_may_clear(chrome_ok, true, frame_ok)
                })
                .unwrap_or(false);
                if let Some(below) = recovery.below_window {
                    let _ = order_above(window_id, below);
                }
                if restored {
                    crate::snap::clear_recovery(&dir);
                }
                remove_placeholder();
                return Err(if restored {
                    moved_rejected_message()
                } else {
                    chrome_restore_failed_message(&name)
                });
            }
            SnapMode::SideBySide
        }
    };

    // Side-by-side must not keep a title-bar change. Embedded keeps it.
    if rollback_chrome(dirty, Some(mode)) {
        let _ = with_window(pid, window_id, SIDE_TIMEOUT, |application, window| {
            restore_records(application, window, &recovery);
        });
    }

    let message = match mode {
        SnapMode::Embedded => embedded_message(),
        SnapMode::SideBySide => side_by_side_message(),
        SnapMode::Inactive => embedded_message(),
    };
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            app: app.clone(),
            generation,
            mode,
            app_id: app_id.clone(),
            name: name.clone(),
            bundle_id: bundle.clone(),
            pid,
            window_id,
            logical: local,
            original: recovery,
            ax_rejected: false,
            space_failed: false,
            attempted_space: None,
            write_failures: 0,
            fullscreen_waits: 0,
            message: message.clone(),
        });
    });
    sync_placeholder(&app, local, host.height);
    if let Err(err) = start_follow() {
        let _ = restore_active(&app, true);
        return Err(err);
    }
    // One immediate pass so the window is in place before the first vsync.
    apply_follow();
    if SESSION.with(|slot| slot.borrow().is_none()) {
        return Err(LAST_NOTE
            .with(|note| note.borrow().clone())
            .unwrap_or_else(moved_rejected_message));
    }
    Ok(current_status())
}

struct Originals {
    frame: Option<Rect>,
    chrome: Vec<ChromeRecord>,
    enhanced: Option<bool>,
    manual: Option<bool>,
    fullscreen: Option<bool>,
    already_borderless: bool,
}

fn read_originals(application: *mut c_void, window: *mut c_void) -> Originals {
    let enhanced = read_app_bool(application, "AXEnhancedUserInterface");
    let manual = read_app_bool(application, "AXManualAccessibility");
    if enhanced != Some(true) {
        let _ = set_app_bool(application, "AXEnhancedUserInterface", true);
    }
    if manual != Some(true) {
        let _ = set_app_bool(application, "AXManualAccessibility", true);
    }
    let names = attribute_names(window);
    let mut chrome = Vec::new();
    for name in [
        "AXCloseButton",
        "AXMinimizeButton",
        "AXZoomButton",
        "AXFullScreenButton",
        "AXTitleUIElement",
    ] {
        if let Some(element) = copy_attr(window, name) {
            let hidden = copy_attr(element, "AXHidden")
                .and_then(|value| {
                    let hidden = read_bool(value);
                    release(value);
                    hidden
                })
                .unwrap_or(false);
            chrome.push(ChromeRecord {
                attribute: name.to_string(),
                hidden,
            });
            release(element);
        }
    }
    if names.iter().any(|name| name == "AXTitleBarVisible") {
        if let Some(value) = copy_attr(window, "AXTitleBarVisible") {
            if let Some(visible) = read_bool(value) {
                chrome.push(ChromeRecord {
                    attribute: "AXTitleBarVisible".to_string(),
                    hidden: visible,
                });
            }
            release(value);
        }
    }
    let already_borderless = !names.is_empty()
        && !names.iter().any(|name| {
            name == "AXTitleUIElement" || name == "AXCloseButton" || name == "AXTitleBarVisible"
        });
    Originals {
        frame: read_frame(window),
        chrome,
        enhanced,
        manual,
        fullscreen: read_fullscreen(window),
        already_borderless,
    }
}

fn revert_flags(pid: i32, window_id: u32, originals: &Originals) {
    let _ = with_window(pid, window_id, SIDE_TIMEOUT, |application, _window| {
        if let Some(value) = originals.enhanced {
            let _ = set_app_bool(application, "AXEnhancedUserInterface", value);
        }
        if let Some(value) = originals.manual {
            let _ = set_app_bool(application, "AXManualAccessibility", value);
        }
    });
}

fn hide_chrome(window: *mut c_void, originals: &Originals) -> (bool, bool) {
    if originals.already_borderless {
        return (true, false);
    }
    let mut dirty = false;
    let mut title_hidden = false;
    let mut titlebar_hidden = false;
    for record in &originals.chrome {
        if record.attribute == "AXTitleBarVisible" {
            if set_app_bool(window, "AXTitleBarVisible", false) {
                if let Some(value) = copy_attr(window, "AXTitleBarVisible") {
                    if read_bool(value) == Some(false) {
                        titlebar_hidden = true;
                        dirty = true;
                    }
                    release(value);
                }
            }
            continue;
        }
        let Some(element) = copy_attr(window, &record.attribute) else {
            continue;
        };
        if !record.hidden {
            if let Some(number) = ns_bool(true) {
                if set_attr(element, "AXHidden", number) {
                    if let Some(value) = copy_attr(element, "AXHidden") {
                        if read_bool(value) == Some(true) {
                            dirty = true;
                            if record.attribute == "AXTitleUIElement" {
                                title_hidden = true;
                            }
                        }
                        release(value);
                    }
                }
            }
        } else if record.attribute == "AXTitleUIElement" {
            title_hidden = true;
        }
        release(element);
    }
    let removed = chrome_removed(title_hidden, titlebar_hidden, originals.already_borderless);
    (removed, dirty)
}

fn restore_records(application: *mut c_void, window: *mut c_void, saved: &RecoveryFile) -> bool {
    let mut ok = true;
    for record in &saved.chrome {
        if record.attribute == "AXTitleBarVisible" {
            if !set_app_bool(window, "AXTitleBarVisible", record.hidden) {
                ok = false;
            }
            continue;
        }
        let Some(element) = copy_attr(window, &record.attribute) else {
            ok = false;
            continue;
        };
        match ns_bool(record.hidden) {
            Some(number) if set_attr(element, "AXHidden", number) => {}
            _ => ok = false,
        }
        release(element);
    }
    if let Some(value) = saved.enhanced {
        if !set_app_bool(application, "AXEnhancedUserInterface", value) {
            ok = false;
        }
    }
    if let Some(value) = saved.manual {
        if !set_app_bool(application, "AXManualAccessibility", value) {
            ok = false;
        }
    }
    ok
}

fn apply_follow() {
    if APPLYING.swap(true, Ordering::AcqRel) {
        return;
    }
    apply_follow_inner();
    APPLYING.store(false, Ordering::Release);
}

fn apply_follow_inner() {
    let gen = GENERATION.load(Ordering::Acquire);
    let Some((app, mode, pid, window_id, logical, ax_rejected, space_failed, health)) = SESSION
        .with(|slot| {
            let session = slot.borrow();
            let session = session.as_ref()?;
            if session.generation != gen {
                return None;
            }
            Some((
                session.app.clone(),
                session.mode,
                session.pid,
                session.window_id,
                session.logical,
                session.ax_rejected,
                session.space_failed,
                FollowHealth {
                    write_failures: session.write_failures,
                    fullscreen_waits: session.fullscreen_waits,
                },
            ))
        })
    else {
        return;
    };
    if ax_rejected {
        let _ = finish_stopped(&app, StopReason::AxRejected);
        return;
    }

    let windows = window_mac::windows_on_caller_thread();
    let order: Vec<u32> = windows.iter().map(|window| window.window_id).collect();
    let Some(host) = host_rect(&windows, &app) else {
        return;
    };
    ensure_display_link(display_id_containing(
        host.x + host.width / 2.0,
        host.y + host.height / 2.0,
    ));
    sync_placeholder(&app, logical, host.height);
    let host_id = host_window_id(&app);
    let current = bounds_of(&windows, window_id);
    let (alive, minimized, current) = if let Some(bounds) = current {
        (true, false, bounds)
    } else {
        match probe_window(pid, window_id) {
            Probe::Missing => (false, false, logical),
            Probe::Minimized => (true, true, logical),
            Probe::Alive(frame) => (true, false, frame),
        }
    };
    let above = match host_id {
        Some(host_id) => target_is_above(&order, window_id, host_id).unwrap_or(false),
        None => true,
    };
    let display = display_containing(host.x + host.width / 2.0, host.y + host.height / 2.0);
    let plan = plan_follow(&FollowInput {
        mode,
        host,
        local_placeholder: logical,
        current,
        display,
        host_spaces: host_id.map(spaces_of).unwrap_or_default(),
        target_spaces: if alive && process_alive(pid) {
            spaces_of(window_id)
        } else {
            Vec::new()
        },
        target_above_host: above,
        ax_rejected,
        target_visible: alive,
        target_process_alive: process_alive(pid),
        minimized,
        space_move_failed: space_failed,
    });
    if let Some(reason) = plan.stop {
        let _ = finish_stopped(&app, reason);
        return;
    }
    // SkyLight's space move returns nothing, so each target Space is attempted once.
    // A later Space change gets a new attempt. A stuck call does not repeat every frame.
    let attempted = SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|session| session.attempted_space)
    });
    if plan.move_to_space.is_none() {
        if attempted.is_some() {
            SESSION.with(|slot| {
                if let Some(session) = slot.borrow_mut().as_mut() {
                    if session.generation == gen {
                        session.attempted_space = None;
                    }
                }
            });
        }
    } else if let Some(space) = plan.move_to_space {
        if attempted != Some(space) {
            move_to_space(window_id, space);
            SESSION.with(|slot| {
                if let Some(session) = slot.borrow_mut().as_mut() {
                    if session.generation == gen {
                        session.attempted_space = Some(space);
                    }
                }
            });
        }
    }
    if GENERATION.load(Ordering::Acquire) != gen {
        return;
    }
    if let Some(frame) = plan.frame {
        let outcome = with_window(pid, window_id, FOLLOW_TIMEOUT, |_application, window| {
            if hold_for_fullscreen_exit(read_fullscreen(window) == Some(true)) {
                let _ = set_fullscreen(window, false);
                return WriteOutcome::WaitingForFullscreen;
            }
            let write = frame_write(current, frame, FOLLOW_EPSILON);
            let size_ok = match write.size {
                Some((width, height)) => set_size(window, width, height),
                None => true,
            };
            let position_ok = match write.position {
                Some((x, y)) => set_point(window, x, y),
                None => true,
            };
            if size_ok && position_ok {
                WriteOutcome::Wrote
            } else {
                WriteOutcome::Failed
            }
        })
        .unwrap_or(WriteOutcome::Failed);
        let (next, stop) = advance_follow_health(health, outcome);
        store_health(gen, next);
        if outcome == WriteOutcome::Failed && matches!(probe_window(pid, window_id), Probe::Missing)
        {
            let _ = finish_stopped(&app, StopReason::TargetGone);
            return;
        }
        if stop {
            let _ = finish_stopped(&app, StopReason::AxRejected);
            return;
        }
    } else {
        store_health(
            gen,
            FollowHealth {
                write_failures: 0,
                fullscreen_waits: 0,
            },
        );
    }
    if GENERATION.load(Ordering::Acquire) != gen {
        return;
    }
    if plan.raise_above_host {
        if let Some(host_id) = host_id {
            if !order_above(window_id, host_id) {
                let _ = ax_raise(pid, window_id);
            }
        }
    }
}

fn finish_stopped(app: &AppHandle, reason: StopReason) -> Result<(), String> {
    let identity = session_identity();
    let name = if identity.target_name.is_empty() {
        "这个应用".to_string()
    } else {
        identity.target_name.clone()
    };
    let note = match reason {
        StopReason::TargetGone => target_gone_message(&name),
        StopReason::Minimized => minimized_message(&name),
        StopReason::AxRejected => moved_rejected_message(),
    };
    if matches!(reason, StopReason::TargetGone) {
        stop_sources();
        GENERATION.fetch_add(1, Ordering::AcqRel);
        let session = SESSION.with(|slot| slot.borrow_mut().take());
        if let Some(session) = session {
            let host = host_window_id(&session.app);
            let restored = apply_saved(&session.original, true, host);
            let ax_alive = !matches!(probe_window(session.pid, session.window_id), Probe::Missing);
            if clear_recovery_after_target_gone(ax_alive, restored) {
                if let Ok(dir) = support_dir_for(&session.app) {
                    crate::snap::clear_recovery(&dir);
                }
            }
        }
        remove_placeholder();
        publish_stop(app, &identity, note);
        return Ok(());
    }
    let restore_frame = !matches!(reason, StopReason::Minimized);
    match restore_active(app, restore_frame) {
        Ok(()) => {
            publish_stop(app, &identity, note);
            Ok(())
        }
        Err(err) => {
            publish_stop(app, &identity, err.clone());
            Err(err)
        }
    }
}

fn publish_stop(app: &AppHandle, identity: &SnapStatus, message: String) {
    LAST_NOTE.with(|slot| slot.replace(Some(message.clone())));
    let mut status = identity.clone();
    status.active = false;
    status.mode = SnapMode::Inactive;
    status.message = Some(message);
    let _ = app.emit("snap-ended", &status);
}

fn session_identity() -> SnapStatus {
    SESSION.with(|slot| match slot.borrow().as_ref() {
        Some(session) => status_for(&session.app_id, &session.name, &session.bundle_id),
        None => idle_status(),
    })
}

fn store_health(generation: u64, health: FollowHealth) {
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            if session.generation == generation {
                session.write_failures = health.write_failures;
                session.fullscreen_waits = health.fullscreen_waits;
            }
        }
    });
}

fn restore_active(app: &AppHandle, restore_frame: bool) -> Result<(), String> {
    stop_sources();
    GENERATION.fetch_add(1, Ordering::AcqRel);
    let session = SESSION.with(|slot| slot.borrow_mut().take());
    if let Some(session) = session {
        let host = host_window_id(&session.app);
        let name = session.name.clone();
        let restored = apply_saved(&session.original, restore_frame, host);
        remove_placeholder();
        if restored {
            if let Ok(dir) = support_dir_for(&session.app) {
                crate::snap::clear_recovery(&dir);
            }
            return Ok(());
        }
        return Err(chrome_restore_failed_message(&name));
    }
    let _ = apply_recovery_file(app);
    remove_placeholder();
    Ok(())
}

fn apply_saved(saved: &RecoveryFile, restore_frame: bool, host_window: Option<u32>) -> bool {
    let frame = Rect {
        x: saved.x,
        y: saved.y,
        width: saved.width,
        height: saved.height,
    };
    let wrote = with_window(
        saved.pid,
        saved.window_id,
        SIDE_TIMEOUT,
        |application, window| {
            let chrome_ok = restore_records(application, window, saved);
            let frame_ok = if restore_frame {
                set_frame(window, frame)
            } else {
                true
            };
            if let Some(fullscreen) = saved.fullscreen {
                let _ = set_fullscreen(window, fullscreen);
            }
            recovery_may_clear(chrome_ok, restore_frame, frame_ok)
        },
    )
    .unwrap_or(false);
    if !wrote {
        return false;
    }
    if restore_frame {
        if let Some(space) = saved.space {
            if !spaces_of(saved.window_id).contains(&space) {
                let _ = move_to_space(saved.window_id, space);
            }
        }
    }
    let stacked = match saved.below_window {
        Some(below) => order_above(saved.window_id, below),
        None => false,
    };
    if !stacked {
        if let Some(host_window) = host_window {
            // The recorded window behind this one is gone. Put the shell back
            // above the target so a restored window is not left covering it.
            let _ = order_above(host_window, saved.window_id);
        }
    }
    true
}

fn apply_recovery_file(app: &AppHandle) -> Result<(), String> {
    let dir = support_dir_for(app)?;
    let Some(saved) = read_recovery(&dir) else {
        return Ok(());
    };
    if !confirmed_bundle(&saved.bundle_id) {
        crate::snap::clear_recovery(&dir);
        return Ok(());
    }
    if !window_mac::trusted_on_caller_thread() {
        return Ok(());
    }
    let snapshot = window_mac::snapshot_on_caller_thread()?;
    let same_process = snapshot
        .processes
        .iter()
        .any(|process| process.pid == saved.pid && process.bundle_id == saved.bundle_id);
    if !same_process {
        crate::snap::clear_recovery(&dir);
        return Ok(());
    }
    if !snapshot
        .windows
        .iter()
        .any(|window| window.window_id == saved.window_id)
        && matches!(probe_window(saved.pid, saved.window_id), Probe::Missing)
    {
        crate::snap::clear_recovery(&dir);
        return Ok(());
    }
    if apply_saved(&saved, true, host_window_id(app)) {
        crate::snap::clear_recovery(&dir);
    }
    Ok(())
}

fn current_status() -> SnapStatus {
    SESSION.with(|slot| match slot.borrow().as_ref() {
        Some(session) => {
            let mut status = status_for(&session.app_id, &session.name, &session.bundle_id);
            status.active = true;
            status.mode = session.mode;
            status.message = Some(session.message.clone());
            status
        }
        None => {
            let mut status = idle_status();
            status.message = LAST_NOTE.with(|note| note.borrow().clone());
            status
        }
    })
}

#[derive(Clone, Copy)]
enum Probe {
    Missing,
    Minimized,
    Alive(Rect),
}

fn probe_window(pid: i32, window_id: u32) -> Probe {
    with_window(pid, window_id, FOLLOW_TIMEOUT, |_application, window| {
        let minimized = copy_attr(window, "AXMinimized")
            .and_then(|value| {
                let minimized = read_bool(value);
                release(value);
                minimized
            })
            .unwrap_or(false);
        if minimized {
            Probe::Minimized
        } else {
            Probe::Alive(read_frame(window).unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }))
        }
    })
    .unwrap_or(Probe::Missing)
}

fn with_window<T>(
    pid: i32,
    window_id: u32,
    timeout: f32,
    body: impl FnOnce(*mut c_void, *mut c_void) -> T,
) -> Option<T> {
    let application = Owned::new(unsafe { AXUIElementCreateApplication(pid) })?;
    unsafe { AXUIElementSetMessagingTimeout(application.get(), timeout) };
    let windows = copy_attr(application.get(), "AXWindows")?;
    let count = unsafe { CFArrayGetCount(windows) };
    let mut found = ptr::null_mut();
    for index in 0..count {
        let item = unsafe { CFArrayGetValueAtIndex(windows, index) } as *mut c_void;
        if ax_window_id(item) == Some(window_id) {
            found = unsafe { CFRetain(item) } as *mut c_void;
            break;
        }
    }
    if found.is_null() {
        if let Some(bounds) = cg_bounds(window_id) {
            for index in 0..count {
                let item = unsafe { CFArrayGetValueAtIndex(windows, index) } as *mut c_void;
                if let Some(frame) = read_frame(item) {
                    if rects_near(frame, bounds) {
                        found = unsafe { CFRetain(item) } as *mut c_void;
                        break;
                    }
                }
            }
        }
    }
    release(windows);
    let window = Owned::new(found)?;
    unsafe { AXUIElementSetMessagingTimeout(window.get(), timeout) };
    Some(body(application.get(), window.get()))
}

fn cg_bounds(window_id: u32) -> Option<Rect> {
    bounds_of(&window_mac::windows_on_caller_thread(), window_id)
}

fn bounds_of(windows: &[RawWindow], window_id: u32) -> Option<Rect> {
    let window = windows.iter().find(|item| item.window_id == window_id)?;
    if window.width < 2.0 || window.height < 2.0 {
        return None;
    }
    Some(Rect {
        x: window.x,
        y: window.y,
        width: window.width,
        height: window.height,
    })
}

fn host_window_id(app: &AppHandle) -> Option<u32> {
    let window = app.get_window("main")?;
    let pointer = window.ns_window().ok()?;
    if pointer.is_null() {
        return None;
    }
    let ns = pointer as *mut AnyObject;
    let number: isize = unsafe { msg_send![ns, windowNumber] };
    u32::try_from(number).ok().filter(|id| *id > 0)
}

fn host_rect(windows: &[RawWindow], app: &AppHandle) -> Option<Rect> {
    if let Some(id) = host_window_id(app) {
        if let Some(rect) = bounds_of(windows, id) {
            return Some(rect);
        }
    }
    let own = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    windows
        .iter()
        .filter(|window| window.pid == own && window.layer == 0 && window.width >= 200.0)
        .max_by(|left, right| {
            let area = |item: &RawWindow| item.width * item.height;
            area(left)
                .partial_cmp(&area(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|window| Rect {
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
        })
}

fn host_height(app: &AppHandle) -> Option<f64> {
    let windows = window_mac::windows_on_caller_thread();
    host_rect(&windows, app).map(|rect| rect.height)
}

fn rects_near(left: Rect, right: Rect) -> bool {
    (left.x - right.x).abs() <= 4.0
        && (left.y - right.y).abs() <= 4.0
        && (left.width - right.width).abs() <= 4.0
        && (left.height - right.height).abs() <= 4.0
}

fn process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // `kill(pid, 0)` checks the process without signaling it. EPERM still means
    // the process exists.
    let alive = unsafe { kill(pid, 0) == 0 };
    alive || std::io::Error::last_os_error().raw_os_error() == Some(1)
}

fn read_fullscreen(window: *mut c_void) -> Option<bool> {
    read_app_bool(window, "AXFullScreen")
}

fn set_fullscreen(window: *mut c_void, value: bool) -> bool {
    set_app_bool(window, "AXFullScreen", value)
}

fn exit_fullscreen(window: *mut c_void) -> bool {
    if !should_exit_fullscreen(read_fullscreen(window) == Some(true)) {
        return true;
    }
    if !set_fullscreen(window, false) {
        return false;
    }
    for _ in 0..10 {
        if read_fullscreen(window) != Some(true) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    read_fullscreen(window) != Some(true)
}

fn display_id_containing(x: f64, y: f64) -> Option<u32> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return None;
    }
    for id in ids.into_iter().take(count as usize) {
        let bounds = unsafe { CGDisplayBounds(id) };
        let rect = Rect {
            x: bounds.origin.x,
            y: bounds.origin.y,
            width: bounds.size.width,
            height: bounds.size.height,
        };
        if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
            return Some(id);
        }
    }
    None
}

fn display_containing(x: f64, y: f64) -> Rect {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
    if err == 0 {
        for id in ids.into_iter().take(count as usize) {
            let bounds = unsafe { CGDisplayBounds(id) };
            let rect = Rect {
                x: bounds.origin.x,
                y: bounds.origin.y,
                width: bounds.size.width,
                height: bounds.size.height,
            };
            if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
                return rect;
            }
        }
    }
    let primary = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    if primary.size.width > 1.0 && primary.size.height > 1.0 {
        return Rect {
            x: primary.origin.x,
            y: primary.origin.y,
            width: primary.size.width,
            height: primary.size.height,
        };
    }
    Rect {
        x: 0.0,
        y: 0.0,
        width: 20_000.0,
        height: 20_000.0,
    }
}

fn read_frame(window: *mut c_void) -> Option<Rect> {
    let position = copy_attr(window, "AXPosition")?;
    let point = read_point(position);
    release(position);
    let (x, y) = point?;
    let size = copy_attr(window, "AXSize")?;
    let dimensions = read_size(size);
    release(size);
    let (width, height) = dimensions?;
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn set_frame(window: *mut c_void, frame: Rect) -> bool {
    set_size(window, frame.width, frame.height) && set_point(window, frame.x, frame.y)
}

fn read_point(value: *mut c_void) -> Option<(f64, f64)> {
    if value.is_null() || unsafe { AXValueGetType(value) } != AX_POINT {
        return None;
    }
    let mut point = CgPoint { x: 0.0, y: 0.0 };
    let ok = unsafe { AXValueGetValue(value, AX_POINT, &mut point as *mut CgPoint as *mut c_void) };
    if ok == 0 {
        None
    } else {
        Some((point.x, point.y))
    }
}

fn read_size(value: *mut c_void) -> Option<(f64, f64)> {
    if value.is_null() || unsafe { AXValueGetType(value) } != AX_SIZE {
        return None;
    }
    let mut size = CgSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = unsafe { AXValueGetValue(value, AX_SIZE, &mut size as *mut CgSize as *mut c_void) };
    if ok == 0 {
        None
    } else {
        Some((size.width, size.height))
    }
}

fn set_point(window: *mut c_void, x: f64, y: f64) -> bool {
    let point = CgPoint { x, y };
    let value = unsafe { AXValueCreate(AX_POINT, &point as *const CgPoint as *const c_void) };
    if value.is_null() {
        return false;
    }
    let ok = set_attr(window, "AXPosition", value);
    release(value);
    ok
}

fn set_size(window: *mut c_void, width: f64, height: f64) -> bool {
    let size = CgSize { width, height };
    let value = unsafe { AXValueCreate(AX_SIZE, &size as *const CgSize as *const c_void) };
    if value.is_null() {
        return false;
    }
    let ok = set_attr(window, "AXSize", value);
    release(value);
    ok
}

fn copy_attr(element: *mut c_void, name: &str) -> Option<*mut c_void> {
    let text = NSString::from_str(name);
    let mut value = ptr::null_mut();
    let err = unsafe {
        AXUIElementCopyAttributeValue(
            element,
            Retained::as_ptr(&text) as *const c_void,
            &mut value,
        )
    };
    if err != AX_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

fn set_attr(element: *mut c_void, name: &str, value: *const c_void) -> bool {
    let text = NSString::from_str(name);
    let err = unsafe {
        AXUIElementSetAttributeValue(element, Retained::as_ptr(&text) as *const c_void, value)
    };
    err == AX_SUCCESS
}

fn attribute_names(element: *mut c_void) -> Vec<String> {
    let mut names = ptr::null_mut();
    let err = unsafe { AXUIElementCopyAttributeNames(element, &mut names) };
    if err != AX_SUCCESS || names.is_null() {
        return Vec::new();
    }
    let count = unsafe { CFArrayGetCount(names) };
    let mut out = Vec::new();
    for index in 0..count {
        let item = unsafe { CFArrayGetValueAtIndex(names, index) } as *mut AnyObject;
        if let Some(text) = ax_string(item) {
            out.push(text);
        }
    }
    release(names);
    out
}

fn read_app_bool(element: *mut c_void, name: &str) -> Option<bool> {
    let value = copy_attr(element, name)?;
    let parsed = read_bool(value);
    release(value);
    parsed
}

fn set_app_bool(element: *mut c_void, name: &str, value: bool) -> bool {
    let Some(number) = ns_bool(value) else {
        return false;
    };
    set_attr(element, name, number)
}

fn ns_bool(value: bool) -> Option<*const c_void> {
    let class = AnyClass::get(c"NSNumber")?;
    let number: *mut AnyObject = unsafe { msg_send![class, numberWithBool: value] };
    if number.is_null() {
        None
    } else {
        Some(number.cast())
    }
}

fn read_bool(value: *mut c_void) -> Option<bool> {
    let object = value as *mut AnyObject;
    if object.is_null() || !is_class(object, c"NSNumber") {
        return None;
    }
    let parsed: bool = unsafe { msg_send![object, boolValue] };
    Some(parsed)
}

fn ax_string(object: *mut AnyObject) -> Option<String> {
    if object.is_null() {
        return None;
    }
    let bytes: *const i8 = unsafe { msg_send![object, UTF8String] };
    if bytes.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn is_class(object: *mut AnyObject, name: &CStr) -> bool {
    let Some(class) = AnyClass::get(name) else {
        return false;
    };
    let yes: bool = unsafe { msg_send![object, isKindOfClass: class] };
    yes
}

fn ax_window_id(element: *mut c_void) -> Option<u32> {
    let func = *window_id_fn()?;
    let mut id = 0u32;
    let err = unsafe { func(element, &mut id) };
    if err == AX_SUCCESS && id != 0 {
        Some(id)
    } else {
        None
    }
}

fn window_id_fn() -> Option<&'static GetWindowFn> {
    static FN: OnceLock<Option<GetWindowFn>> = OnceLock::new();
    FN.get_or_init(|| {
        let symbol = unsafe { dlsym(RTLD_DEFAULT, c"_AXUIElementGetWindow".as_ptr()) };
        if symbol.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute(symbol) })
        }
    })
    .as_ref()
}

fn ax_raise(pid: i32, window_id: u32) -> bool {
    with_window(pid, window_id, FOLLOW_TIMEOUT, |_application, window| {
        let action = NSString::from_str("AXRaise");
        unsafe {
            AXUIElementPerformAction(window, Retained::as_ptr(&action) as *const c_void)
                == AX_SUCCESS
        }
    })
    .unwrap_or(false)
}

fn sky() -> Option<&'static Sky> {
    static SKY: OnceLock<Option<Sky>> = OnceLock::new();
    SKY.get_or_init(|| {
        let handle = unsafe {
            dlopen(
                c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                RTLD_LAZY,
            )
        };
        if handle.is_null() {
            return None;
        }
        let connection = unsafe { dlsym(handle, c"SLSMainConnectionID".as_ptr()) };
        if connection.is_null() {
            return None;
        }
        let order = unsafe { dlsym(handle, c"SLSOrderWindow".as_ptr()) };
        let spaces = unsafe { dlsym(handle, c"SLSCopySpacesForWindows".as_ptr()) };
        let move_space = unsafe { dlsym(handle, c"SLSMoveWindowsToManagedSpace".as_ptr()) };
        Some(Sky {
            connection: unsafe { std::mem::transmute(connection) },
            order: if order.is_null() {
                None
            } else {
                Some(unsafe { std::mem::transmute(order) })
            },
            spaces: if spaces.is_null() {
                None
            } else {
                Some(unsafe { std::mem::transmute(spaces) })
            },
            move_space: if move_space.is_null() {
                None
            } else {
                Some(unsafe { std::mem::transmute(move_space) })
            },
        })
    })
    .as_ref()
}

fn order_above(window_id: u32, other: u32) -> bool {
    let sky = match sky() {
        Some(sky) => sky,
        None => return false,
    };
    let Some(order) = sky.order else {
        return false;
    };
    let cid = unsafe { (sky.connection)() };
    let result = unsafe { order(cid, window_id, 1, other) };
    result == 0
}

fn spaces_of(window_id: u32) -> Vec<u64> {
    let Some(sky) = sky() else {
        return Vec::new();
    };
    let Some(spaces_fn) = sky.spaces else {
        return Vec::new();
    };
    let Some(array) = window_array(window_id) else {
        return Vec::new();
    };
    let cid = unsafe { (sky.connection)() };
    let spaces = unsafe { spaces_fn(cid, 0x7, array.get()) };
    if spaces.is_null() {
        return Vec::new();
    }
    let count = unsafe { CFArrayGetCount(spaces) };
    let mut found = Vec::new();
    for index in 0..count {
        let number = unsafe { CFArrayGetValueAtIndex(spaces, index) };
        let mut value = 0i64;
        if unsafe { CFNumberGetValue(number, 4, &mut value as *mut i64 as *mut c_void) } != 0
            && value >= 0
        {
            let space = value as u64;
            if !found.contains(&space) {
                found.push(space);
            }
        }
    }
    release(spaces);
    found
}

fn move_to_space(window_id: u32, space: u64) {
    let Some(sky) = sky() else {
        return;
    };
    let Some(func) = sky.move_space else {
        return;
    };
    let cid = unsafe { (sky.connection)() };
    let Some(array) = window_array(window_id) else {
        return;
    };
    unsafe { func(cid, array.get(), space) };
}

fn window_array(window_id: u32) -> Option<Owned> {
    let value = i64::from(window_id);
    let number = unsafe { CFNumberCreate(ptr::null(), 4, &value as *const i64 as *const c_void) };
    if number.is_null() {
        return None;
    }
    let values = [number as *const c_void];
    let array = unsafe {
        CFArrayCreate(
            ptr::null(),
            values.as_ptr(),
            1,
            std::ptr::addr_of!(kCFTypeArrayCallBacks).cast(),
        )
    };
    release(number);
    Owned::new(array)
}

fn sync_placeholder(app: &AppHandle, local: Rect, window_height: f64) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    if pointer.is_null() {
        return;
    }
    let ns = pointer as *mut AnyObject;
    let view = placeholder_view(ns);
    if view.is_null() {
        return;
    }
    let rect = NsRect {
        origin: NsPoint {
            x: local.x,
            y: window_height - local.y - local.height,
        },
        size: NsSize {
            width: local.width,
            height: local.height,
        },
    };
    let _: () = unsafe { msg_send![view, setFrame: rect] };
}

fn placeholder_view(ns_window: *mut AnyObject) -> *mut AnyObject {
    let existing = PLACEHOLDER.with(|slot| *slot.borrow());
    if !existing.is_null() {
        return existing;
    }
    let Some(class) = AnyClass::get(c"NSView") else {
        return ptr::null_mut();
    };
    let allocated: *mut AnyObject = unsafe { msg_send![class, alloc] };
    if allocated.is_null() {
        return ptr::null_mut();
    }
    let view: *mut AnyObject = unsafe { msg_send![allocated, init] };
    if view.is_null() {
        return ptr::null_mut();
    }
    let _: () = unsafe { msg_send![view, setWantsLayer: true] };
    if let Some(color_class) = AnyClass::get(c"NSColor") {
        let clear: *mut AnyObject = unsafe { msg_send![color_class, clearColor] };
        let layer: *mut AnyObject = unsafe { msg_send![view, layer] };
        if !clear.is_null() && !layer.is_null() {
            let cg: *mut AnyObject = unsafe { msg_send![clear, CGColor] };
            if !cg.is_null() {
                let _: () = unsafe { msg_send![layer, setBackgroundColor: cg] };
            }
        }
    }
    let content: *mut AnyObject = unsafe { msg_send![ns_window, contentView] };
    if content.is_null() {
        let _: () = unsafe { msg_send![view, release] };
        return ptr::null_mut();
    }
    let _: () = unsafe { msg_send![content, addSubview: view] };
    PLACEHOLDER.with(|slot| *slot.borrow_mut() = view);
    view
}

fn remove_placeholder() {
    let view = PLACEHOLDER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let view = *slot;
        *slot = ptr::null_mut();
        view
    });
    if view.is_null() {
        return;
    }
    let _: () = unsafe { msg_send![view, removeFromSuperview] };
    let _: () = unsafe { msg_send![view, release] };
}

fn start_follow() -> Result<(), String> {
    stop_sources();
    if start_display_link(None).is_ok() {
        return Ok(());
    }
    start_timer().map_err(|_| "无法启动窗口跟随，已把窗口放回原来的位置。".to_string())
}

fn start_display_link(display: Option<u32>) -> Result<(), ()> {
    let mut link = ptr::null_mut();
    let created = match display {
        Some(display) => unsafe { CVDisplayLinkCreateWithCGDisplay(display, &mut link) },
        None => unsafe { CVDisplayLinkCreateWithActiveCGDisplays(&mut link) },
    };
    if created != 0 || link.is_null() {
        return Err(());
    }
    let callback = unsafe { CVDisplayLinkSetOutputCallback(link, on_display, ptr::null_mut()) };
    if callback != 0 {
        unsafe { CVDisplayLinkRelease(link) };
        return Err(());
    }
    let started = unsafe { CVDisplayLinkStart(link) };
    if started != 0 {
        unsafe { CVDisplayLinkRelease(link) };
        return Err(());
    }
    LINK.store(link, Ordering::Release);
    LINK_DISPLAY.store(display.unwrap_or(LINK_UNBOUND), Ordering::Release);
    Ok(())
}

fn ensure_display_link(display: Option<u32>) {
    let link_display = if LINK.load(Ordering::Acquire).is_null() {
        None
    } else {
        let bound = LINK_DISPLAY.load(Ordering::Acquire);
        if bound == LINK_UNBOUND {
            None
        } else {
            Some(bound)
        }
    };
    let DisplayLinkPlan::Retarget(display) = follow_display(display, link_display) else {
        return;
    };
    let attempted = LINK_ATTEMPTED.load(Ordering::Acquire);
    let attempted = if attempted == LINK_UNBOUND {
        None
    } else {
        Some(attempted)
    };
    if skip_repeated_display_link(
        attempted,
        display,
        !LINK.load(Ordering::Acquire).is_null(),
        !TIMER.load(Ordering::Acquire).is_null(),
    ) {
        return;
    }
    LINK_ATTEMPTED.store(display, Ordering::Release);
    let previous = LINK.swap(ptr::null_mut(), Ordering::AcqRel);
    if start_display_link(Some(display)).is_ok() {
        release_link(previous);
        return;
    }
    if !previous.is_null() {
        LINK.store(previous, Ordering::Release);
        return;
    }
    if start_display_link(None).is_err() && TIMER.load(Ordering::Acquire).is_null() {
        let _ = start_timer();
    }
}

fn release_link(link: *mut c_void) {
    if link.is_null() {
        return;
    }
    unsafe {
        CVDisplayLinkStop(link);
        CVDisplayLinkRelease(link);
    }
}

fn start_timer() -> Result<(), ()> {
    let queue = main_queue();
    let timer = unsafe {
        dispatch_source_create(
            std::ptr::addr_of!(_dispatch_source_type_timer).cast(),
            0,
            0,
            queue,
        )
    };
    if timer.is_null() {
        return Err(());
    }
    unsafe {
        dispatch_source_set_timer(timer, 0, FOLLOW_INTERVAL_NS, 1_000_000);
        dispatch_source_set_event_handler_f(timer, on_timer);
        dispatch_resume(timer);
    }
    TIMER.store(timer, Ordering::Release);
    Ok(())
}

fn stop_sources() {
    LINK_DISPLAY.store(LINK_UNBOUND, Ordering::Release);
    LINK_ATTEMPTED.store(LINK_UNBOUND, Ordering::Release);
    let link = LINK.swap(ptr::null_mut(), Ordering::AcqRel);
    release_link(link);
    let timer = TIMER.swap(ptr::null_mut(), Ordering::AcqRel);
    if !timer.is_null() {
        unsafe {
            dispatch_source_cancel(timer);
            dispatch_release(timer);
        }
    }
}

extern "C" fn on_display(
    _link: *mut c_void,
    _now: *const c_void,
    _output: *const c_void,
    _flags_in: u64,
    _flags_out: *mut u64,
    _context: *mut c_void,
) -> i32 {
    queue_apply();
    0
}

extern "C" fn on_timer(_context: *mut c_void) {
    queue_apply();
}

fn queue_apply() {
    if FRAME_QUEUED.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe { dispatch_async_f(main_queue(), ptr::null_mut(), apply_dispatched) };
}

fn main_queue() -> *mut c_void {
    std::ptr::addr_of!(_dispatch_main_q).cast_mut().cast()
}

extern "C" fn apply_dispatched(_context: *mut c_void) {
    FRAME_QUEUED.store(false, Ordering::Release);
    apply_follow();
}

fn release(value: *const c_void) {
    if !value.is_null() {
        unsafe { CFRelease(value) };
    }
}
