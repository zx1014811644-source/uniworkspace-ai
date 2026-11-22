//! Global hotkey for showing and hiding the main window.
//!
//! The default chord is Option+Space. The user can replace it. Registration
//! uses Carbon `RegisterEventHotKey` on the main thread. The Carbon callback
//! only enqueues the press and returns. It does not call AppKit, and it does
//! not wait on the main queue. Waiting there deadlocks when the same chord is
//! also a system shortcut, because the system handler and this process then
//! block each other on the main run loop.
//!
//! Chords that macOS already uses for Spotlight, Finder search, the input
//! source, or the emoji panel are refused before registration. A failed
//! `RegisterEventHotKey` is reported once and is not retried.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const DEFAULT_ACCELERATOR: &str = "Option+Space";
pub const SHORTCUT_FILE: &str = "shortcut.json";

/// The hotkey callback must not block. Tests lock this contract.
pub fn hotkey_callback_blocks() -> bool {
    false
}

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "shortcuts: global hotkey, active space"
    } else {
        "shortcuts: global hotkey inactive off macOS"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Space,
    Letter(char),
    Digit(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

impl Chord {
    pub fn has_command_modifier(self) -> bool {
        self.command || self.option || self.control
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellToggle {
    Hide,
    /// Pull the main window onto the Space the user is on now, then focus it.
    ShowOnActiveSpace,
}

/// Hide only when the main window is already the key window on the active
/// Space. From any other Space, or while another app is key, the same press
/// shows it on the Space the user is looking at.
pub fn shell_toggle(
    visible: bool,
    minimized: bool,
    on_active_space: bool,
    key_window: bool,
) -> ShellToggle {
    if visible && !minimized && on_active_space && key_window {
        ShellToggle::Hide
    } else {
        ShellToggle::ShowOnActiveSpace
    }
}

pub fn default_chord() -> Chord {
    Chord {
        key: Key::Space,
        command: false,
        option: true,
        control: false,
        shift: false,
    }
}

pub fn format_chord(chord: &Chord) -> String {
    let mut parts = Vec::new();
    if chord.control {
        parts.push("Control");
    }
    if chord.option {
        parts.push("Option");
    }
    if chord.shift {
        parts.push("Shift");
    }
    if chord.command {
        parts.push("Command");
    }
    parts.push(match chord.key {
        Key::Space => "Space",
        Key::Letter(letter) => return format_with_key(&parts, &letter.to_string()),
        Key::Digit(digit) => return format_with_key(&parts, &digit.to_string()),
    });
    parts.join("+")
}

fn format_with_key(parts: &[&str], key: &str) -> String {
    let mut owned: Vec<&str> = parts.to_vec();
    owned.push(key);
    owned.join("+")
}

/// `None` means the key is not a virtual key we register.
/// `Some(0)` is valid: ANSI A is virtual key code 0.
pub fn key_code(key: Key) -> Option<u32> {
    Some(match key {
        Key::Space => 0x31,
        Key::Digit('0') => 0x1D,
        Key::Digit('1') => 0x12,
        Key::Digit('2') => 0x13,
        Key::Digit('3') => 0x14,
        Key::Digit('4') => 0x15,
        Key::Digit('5') => 0x17,
        Key::Digit('6') => 0x16,
        Key::Digit('7') => 0x1A,
        Key::Digit('8') => 0x1C,
        Key::Digit('9') => 0x19,
        Key::Digit(_) => return None,
        Key::Letter('A') => 0x00,
        Key::Letter('B') => 0x0B,
        Key::Letter('C') => 0x08,
        Key::Letter('D') => 0x02,
        Key::Letter('E') => 0x0E,
        Key::Letter('F') => 0x03,
        Key::Letter('G') => 0x05,
        Key::Letter('H') => 0x04,
        Key::Letter('I') => 0x22,
        Key::Letter('J') => 0x26,
        Key::Letter('K') => 0x28,
        Key::Letter('L') => 0x25,
        Key::Letter('M') => 0x2E,
        Key::Letter('N') => 0x2D,
        Key::Letter('O') => 0x1F,
        Key::Letter('P') => 0x23,
        Key::Letter('Q') => 0x0C,
        Key::Letter('R') => 0x0F,
        Key::Letter('S') => 0x01,
        Key::Letter('T') => 0x11,
        Key::Letter('U') => 0x20,
        Key::Letter('V') => 0x09,
        Key::Letter('W') => 0x0D,
        Key::Letter('X') => 0x07,
        Key::Letter('Y') => 0x10,
        Key::Letter('Z') => 0x06,
        Key::Letter(_) => return None,
    })
}

/// Carbon modifier flags understood by `RegisterEventHotKey`.
/// Command is bit 8, Shift bit 9, Option bit 11, Control bit 12.
pub fn carbon_modifiers(chord: &Chord) -> u32 {
    let mut flags = 0;
    if chord.command {
        flags |= 1 << 8;
    }
    if chord.shift {
        flags |= 1 << 9;
    }
    if chord.option {
        flags |= 1 << 11;
    }
    if chord.control {
        flags |= 1 << 12;
    }
    flags
}

/// Exact default macOS chords that share a key with the system.
/// Registering them and then blocking in the handler deadlocks against
/// Spotlight, the input menu, or the character viewer. They are refused
/// instead of registered, and the refusal is not retried.
pub fn system_conflict(chord: &Chord) -> Option<&'static str> {
    if chord.key != Key::Space || chord.shift {
        return None;
    }
    if chord.command && chord.control && !chord.option {
        return Some(
            "Command+Control+Space 是系统的表情与符号面板。这个组合不会注册，也不会重试，避免和系统快捷键卡住。请改一个。",
        );
    }
    if chord.command && chord.option && !chord.control {
        return Some(
            "Command+Option+Space 是系统的访达搜索。这个组合不会注册，也不会重试，避免和系统快捷键卡住。请改一个。",
        );
    }
    if chord.command && !chord.option && !chord.control {
        return Some(
            "Command+Space 是系统的聚焦搜索。这个组合不会注册，也不会重试，避免和系统快捷键卡住。请改一个。",
        );
    }
    if chord.control && chord.option && !chord.command {
        return Some(
            "Control+Option+Space 是系统的输入法切换。这个组合不会注册，也不会重试，避免和系统快捷键卡住。请改一个。",
        );
    }
    if chord.control && !chord.option && !chord.command {
        return Some(
            "Control+Space 是系统的输入法切换。这个组合不会注册，也不会重试，避免和系统快捷键卡住。请改一个。",
        );
    }
    None
}

pub fn parse_chord(raw: &str) -> Result<Chord, String> {
    let text = raw.trim();
    if text.is_empty() || text.len() > 80 {
        return Err("快捷键为空或过长".into());
    }
    let mut command = false;
    let mut option = false;
    let mut control = false;
    let mut shift = false;
    let mut key = None;
    for part in text.split('+') {
        let token = part.trim();
        if token.is_empty() {
            return Err("快捷键格式不对".into());
        }
        match token.to_ascii_lowercase().as_str() {
            "command" | "cmd" | "meta" => command = true,
            "option" | "opt" | "alt" => option = true,
            "control" | "ctrl" => control = true,
            "shift" => shift = true,
            other => {
                if key.is_some() {
                    return Err("快捷键只能有一个主键".into());
                }
                key = Some(parse_key(other)?);
            }
        }
    }
    let Some(key) = key else {
        return Err("快捷键还缺主键".into());
    };
    if key_code(key).is_none() {
        return Err("这个主键不能用作全局热键".into());
    }
    Ok(Chord {
        key,
        command,
        option,
        control,
        shift,
    })
}

fn parse_key(token: &str) -> Result<Key, String> {
    if token.eq_ignore_ascii_case("space") {
        return Ok(Key::Space);
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return Err("快捷键还缺主键".into());
    };
    if chars.next().is_some() {
        return Err("主键只能是空格、一个字母或一个数字".into());
    }
    if first.is_ascii_alphabetic() {
        return Ok(Key::Letter(first.to_ascii_uppercase()));
    }
    if first.is_ascii_digit() {
        return Ok(Key::Digit(first));
    }
    Err("主键只能是空格、一个字母或一个数字".into())
}

pub fn parse_accelerator(raw: &str) -> Result<Chord, String> {
    let chord = parse_chord(raw)?;
    if !chord.has_command_modifier() {
        return Err("快捷键至少要带 Control、Option 或 Command。只按 Shift 会和打字抢键。".into());
    }
    if let Some(reason) = system_conflict(&chord) {
        return Err(reason.to_string());
    }
    Ok(chord)
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub accelerator: String,
    pub registered: bool,
    pub error: Option<String>,
    pub platform: String,
}

#[derive(Serialize, Deserialize)]
struct ShortcutFile {
    accelerator: String,
}

struct Live {
    accelerator: String,
    chord: Option<Chord>,
    registered: bool,
    error: Option<String>,
}

fn live() -> &'static Mutex<Live> {
    static LIVE: OnceLock<Mutex<Live>> = OnceLock::new();
    LIVE.get_or_init(|| {
        Mutex::new(Live {
            accelerator: DEFAULT_ACCELERATOR.to_string(),
            chord: Some(default_chord()),
            registered: false,
            error: None,
        })
    })
}

fn lock_live() -> std::sync::MutexGuard<'static, Live> {
    live().lock().unwrap_or_else(|err| err.into_inner())
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

fn status_from(live: &Live) -> ShortcutStatus {
    ShortcutStatus {
        accelerator: live.accelerator.clone(),
        registered: live.registered,
        error: live.error.clone(),
        platform: platform_name().to_string(),
    }
}

pub fn current_status() -> ShortcutStatus {
    status_from(&lock_live())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shortcut_path(dir: &Path) -> PathBuf {
    dir.join(SHORTCUT_FILE)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn read_saved(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(shortcut_path(dir)).ok()?;
    let file: ShortcutFile = serde_json::from_str(&text).ok()?;
    Some(file.accelerator)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn write_saved(dir: &Path, accelerator: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("无法保存快捷键: {err}"))?;
    let body = serde_json::to_string_pretty(&ShortcutFile {
        accelerator: accelerator.to_string(),
    })
    .map_err(|err| format!("无法保存快捷键: {err}"))?;
    std::fs::write(shortcut_path(dir), body).map_err(|err| format!("无法保存快捷键: {err}"))
}

pub fn install(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::install(app);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::{mpsc, Mutex, Once, OnceLock};

    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use tauri::{AppHandle, Manager};

    use super::{
        carbon_modifiers, current_status, default_chord, format_chord, key_code, lock_live,
        parse_chord, read_saved, shell_toggle, support_dir, system_conflict, write_saved, Chord,
        ShellToggle, ShortcutStatus,
    };

    const HOTKEY_PRESSED: u32 = 5;
    const KEYBOARD_CLASS: u32 = 0x6B65_7962;
    const PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D;
    const TYPE_HOTKEY_ID: u32 = 0x686B_6964;
    const SIGNATURE: u32 = 0x5557_4149;
    const HOTKEY_ID: u32 = 1;
    const EXISTS: i32 = -9878;

    #[repr(C)]
    struct EventHotKeyId {
        signature: u32,
        id: u32,
    }

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetApplicationEventTarget() -> *mut c_void;
        fn InstallEventHandler(
            target: *mut c_void,
            handler: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32>,
            num_types: u32,
            list: *const EventTypeSpec,
            user: *mut c_void,
            handler_ref: *mut *mut c_void,
        ) -> i32;
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            hotkey_id: EventHotKeyId,
            target: *mut c_void,
            options: u32,
            hotkey_ref: *mut *mut c_void,
        ) -> i32;
        fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
        fn GetEventParameter(
            event: *mut c_void,
            name: u32,
            desired_type: u32,
            actual_type: *mut u32,
            buffer_size: u32,
            actual_size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    fn hotkey_slot() -> &'static Mutex<usize> {
        static SLOT: Mutex<usize> = Mutex::new(0);
        &SLOT
    }

    fn sender() -> &'static OnceLock<mpsc::Sender<()>> {
        static SENDER: OnceLock<mpsc::Sender<()>> = OnceLock::new();
        &SENDER
    }

    unsafe extern "C" fn hotkey_handler(
        _next: *mut c_void,
        event: *mut c_void,
        _user: *mut c_void,
    ) -> i32 {
        // Return immediately. Do not touch AppKit and do not wait for the
        // main queue: a system shortcut handler may already be on this thread.
        let mut id = EventHotKeyId {
            signature: 0,
            id: 0,
        };
        let status = GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyId>() as u32,
            std::ptr::null_mut(),
            &mut id as *mut EventHotKeyId as *mut c_void,
        );
        if status == 0 && id.signature == SIGNATURE && id.id == HOTKEY_ID {
            if let Some(tx) = sender().get() {
                let _ = tx.send(());
            }
        }
        0
    }

    fn ensure_handler() -> Result<(), String> {
        static ONCE: Once = Once::new();
        static RESULT: AtomicI32 = AtomicI32::new(1);
        ONCE.call_once(|| {
            let spec = EventTypeSpec {
                event_class: KEYBOARD_CLASS,
                event_kind: HOTKEY_PRESSED,
            };
            let mut installed = std::ptr::null_mut();
            let status = unsafe {
                InstallEventHandler(
                    GetApplicationEventTarget(),
                    Some(hotkey_handler),
                    1,
                    &spec,
                    std::ptr::null_mut(),
                    &mut installed,
                )
            };
            RESULT.store(status, Ordering::Release);
        });
        let status = RESULT.load(Ordering::Acquire);
        if status == 0 {
            Ok(())
        } else {
            Err(format!(
                "全局热键监听没有装上（{status}）。没有重试。应用可以继续用，热键暂时不可用。"
            ))
        }
    }

    fn unregister_current() {
        let mut slot = hotkey_slot().lock().unwrap_or_else(|err| err.into_inner());
        if *slot != 0 {
            unsafe {
                UnregisterEventHotKey(*slot as *mut c_void);
            }
            *slot = 0;
        }
        let mut live = lock_live();
        live.registered = false;
    }

    fn register_chord(chord: &Chord) -> Result<(), String> {
        if let Some(reason) = system_conflict(chord) {
            return Err(reason.to_string());
        }
        if !chord.has_command_modifier() {
            return Err("快捷键至少要带 Control、Option 或 Command。".into());
        }
        let Some(code) = key_code(chord.key) else {
            return Err("这个主键不能用作全局热键".into());
        };
        ensure_handler()?;
        unregister_current();
        let mut hotkey = std::ptr::null_mut();
        let status = unsafe {
            RegisterEventHotKey(
                code,
                carbon_modifiers(chord),
                EventHotKeyId {
                    signature: SIGNATURE,
                    id: HOTKEY_ID,
                },
                GetApplicationEventTarget(),
                0,
                &mut hotkey,
            )
        };
        if status != 0 || hotkey.is_null() {
            return Err(if status == EXISTS {
                "这个快捷键已经被占用。没有注册，也不会反复重试，避免和系统快捷键卡住。请改一个。"
                    .to_string()
            } else {
                format!("没有注册这个快捷键（{status}）。没有重试。请改一个。")
            });
        }
        *hotkey_slot().lock().unwrap_or_else(|err| err.into_inner()) = hotkey as usize;
        let mut live = lock_live();
        live.chord = Some(*chord);
        live.accelerator = format_chord(chord);
        live.registered = true;
        live.error = None;
        Ok(())
    }

    fn restore_previous(previous: Option<Chord>) {
        if let Some(chord) = previous {
            if register_chord(&chord).is_err() {
                let mut live = lock_live();
                live.chord = Some(chord);
                live.accelerator = format_chord(&chord);
                live.registered = false;
            }
        }
    }

    pub(super) fn install(app: &AppHandle) {
        start_worker(app.clone());
        let saved = support_dir(app).ok().and_then(|dir| read_saved(&dir));
        let (chord, error) = match saved {
            Some(text) => match parse_chord(&text) {
                Ok(chord) => match system_conflict(&chord) {
                    Some(reason) => (chord, Some(reason.to_string())),
                    None if chord.has_command_modifier() => (chord, None),
                    None => (
                        default_chord(),
                        Some(
                            "保存的快捷键缺少 Control、Option 或 Command，已回到 Option+Space。"
                                .to_string(),
                        ),
                    ),
                },
                Err(_) => (
                    default_chord(),
                    Some("保存的快捷键无法识别，已回到 Option+Space。".to_string()),
                ),
            },
            None => (default_chord(), None),
        };
        if system_conflict(&chord).is_some() {
            let mut live = lock_live();
            live.chord = Some(chord);
            live.accelerator = format_chord(&chord);
            live.registered = false;
            live.error = error;
            return;
        }
        if let Err(err) = register_chord(&chord) {
            let mut live = lock_live();
            live.chord = Some(chord);
            live.accelerator = format_chord(&chord);
            live.registered = false;
            live.error = Some(err);
            return;
        }
        if let Some(reason) = error {
            lock_live().error = Some(reason);
        }
    }

    fn start_worker(app: AppHandle) {
        let (tx, rx) = mpsc::channel();
        if sender().set(tx).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("uniworkspace-hotkey".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let app_for_toggle = app.clone();
                    let _ = app.run_on_main_thread(move || toggle_main(&app_for_toggle));
                }
            })
            .ok();
    }

    fn toggle_main(app: &AppHandle) {
        let Some(window) = app.get_window("main") else {
            return;
        };
        let visible = window.is_visible().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        let (on_space, key) = window_flags(&window);
        match shell_toggle(visible, minimized, on_space, key) {
            ShellToggle::Hide => {
                crate::partition_mac::note_shell_visible(app, false);
                let _ = window.hide();
            }
            ShellToggle::ShowOnActiveSpace => {
                let _ = window.unminimize();
                let _ = window.show();
                reveal_on_active_space(&window);
                let _ = window.set_focus();
                crate::partition_mac::note_shell_visible(app, true);
            }
        }
    }

    fn window_flags(window: &tauri::Window) -> (bool, bool) {
        let Ok(pointer) = window.ns_window() else {
            return (false, false);
        };
        if pointer.is_null() {
            return (false, false);
        }
        let ns = pointer as *mut AnyObject;
        unsafe {
            let on_space: bool = msg_send![ns, isOnActiveSpace];
            let key: bool = msg_send![ns, isKeyWindow];
            (on_space, key)
        }
    }

    fn reveal_on_active_space(window: &tauri::Window) {
        let Ok(pointer) = window.ns_window() else {
            return;
        };
        if pointer.is_null() {
            return;
        }
        let ns = pointer as *mut AnyObject;
        const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
        const MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
        const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
        unsafe {
            let current: usize = msg_send![ns, collectionBehavior];
            let behavior =
                (current & !CAN_JOIN_ALL_SPACES) | MOVE_TO_ACTIVE_SPACE | FULL_SCREEN_AUXILIARY;
            let _: () = msg_send![ns, setCollectionBehavior: behavior];
            if let Some(class) = AnyClass::get(c"NSApplication") {
                let application: *mut AnyObject = msg_send![class, sharedApplication];
                if !application.is_null() {
                    let _: () = msg_send![application, activateIgnoringOtherApps: true];
                }
            }
            let _: () = msg_send![ns, orderFrontRegardless];
            let _: () = msg_send![ns, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
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

    pub(super) fn set_chord(app: &AppHandle, chord: Chord) -> Result<ShortcutStatus, String> {
        let app = app.clone();
        let saving = app.clone();
        on_main(&app, move || {
            let previous = {
                let live = lock_live();
                live.chord
            };
            match register_chord(&chord) {
                Ok(()) => {
                    let dir = support_dir(&saving)?;
                    write_saved(&dir, &format_chord(&chord))?;
                    Ok(current_status())
                }
                Err(err) => {
                    restore_previous(previous);
                    Err(err)
                }
            }
        })?
    }

    pub(super) fn set_capture(app: &AppHandle, active: bool) -> Result<ShortcutStatus, String> {
        on_main(app, move || {
            if active {
                unregister_current();
                let mut live = lock_live();
                live.registered = false;
                live.error = None;
            } else {
                let chord = lock_live().chord;
                if let Some(chord) = chord {
                    if let Err(err) = register_chord(&chord) {
                        let mut live = lock_live();
                        live.error = Some(err);
                        live.registered = false;
                    }
                }
            }
            Ok(current_status())
        })?
    }

    pub(super) fn restore_saved(app: &AppHandle) -> Result<ShortcutStatus, String> {
        set_capture(app, false)
    }
}

async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| format!("后台任务中断: {err}"))
}

async fn restore_after_rejected_capture(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let _ = off_main(move || macos::restore_saved(&app)).await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

#[tauri::command]
pub fn shortcut_status() -> ShortcutStatus {
    current_status()
}

#[tauri::command]
pub async fn shortcut_set(app: AppHandle, accelerator: String) -> Result<ShortcutStatus, String> {
    let chord = match parse_accelerator(&accelerator) {
        Ok(chord) => chord,
        Err(message) => {
            // Capture already unregistered the working hotkey. A rejected
            // chord never reaches registration, so put the previous one back.
            restore_after_rejected_capture(&app).await;
            return Err(message);
        }
    };
    #[cfg(target_os = "macos")]
    {
        off_main(move || macos::set_chord(&app, chord)).await?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, chord);
        Err("全局热键只在 macOS 上注册".into())
    }
}

#[tauri::command]
pub async fn shortcut_capture(app: AppHandle, active: bool) -> Result<ShortcutStatus, String> {
    #[cfg(target_os = "macos")]
    {
        off_main(move || macos::set_capture(&app, active)).await?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, active);
        Err("全局热键只在 macOS 上注册".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_real_hotkey_instead_of_the_placeholder() {
        let text = status();
        assert!(text.contains("hotkey"));
        assert!(!text.contains("placeholder"));
        assert!(!hotkey_callback_blocks());
    }

    #[test]
    fn default_is_option_space_and_can_be_rewritten() {
        let chord = parse_accelerator("Option+Space").unwrap();
        assert_eq!(chord, default_chord());
        assert_eq!(format_chord(&chord), "Option+Space");
        assert_eq!(key_code(chord.key), Some(0x31));
        assert_eq!(carbon_modifiers(&chord), 1 << 11);
        assert!(system_conflict(&chord).is_none());
        let changed = parse_accelerator("ctrl+shift+k").unwrap();
        assert!(changed.control && changed.shift && !changed.option && !changed.command);
        assert_eq!(changed.key, Key::Letter('K'));
        assert_eq!(format_chord(&changed), "Control+Shift+K");
        assert_eq!(key_code(changed.key), Some(0x28));
        let with_a = parse_accelerator("Control+Shift+A").unwrap();
        assert_eq!(with_a.key, Key::Letter('A'));
        assert_eq!(key_code(with_a.key), Some(0x00));
        assert_eq!(carbon_modifiers(&changed), (1 << 12) | (1 << 9));
    }

    #[test]
    fn system_space_chords_are_refused_and_not_retried() {
        for raw in [
            "Command+Space",
            "Command+Option+Space",
            "Control+Space",
            "Control+Option+Space",
            "Command+Control+Space",
            "cmd+space",
        ] {
            let err = parse_accelerator(raw).unwrap_err();
            assert!(err.contains("不会注册"), "{raw}: {err}");
            assert!(
                err.contains("不会重试") || err.contains("也不会重试"),
                "{raw}: {err}"
            );
            assert!(err.contains("系统"), "{raw}: {err}");
        }
        assert!(parse_accelerator("Shift+Space").is_err());
        assert!(parse_accelerator("Space").is_err());
        assert!(parse_accelerator("Option+A+B").is_err());
        assert!(system_conflict(&parse_chord("Option+Shift+Space").unwrap()).is_none());
        assert!(parse_accelerator("Option+Shift+Space").is_ok());
    }

    #[test]
    fn show_from_another_space_and_hide_only_when_already_key() {
        assert_eq!(shell_toggle(true, false, true, true), ShellToggle::Hide);
        assert_eq!(
            shell_toggle(true, false, false, true),
            ShellToggle::ShowOnActiveSpace
        );
        assert_eq!(
            shell_toggle(false, false, false, false),
            ShellToggle::ShowOnActiveSpace
        );
        assert_eq!(
            shell_toggle(true, true, true, true),
            ShellToggle::ShowOnActiveSpace
        );
        assert_eq!(
            shell_toggle(true, false, true, false),
            ShellToggle::ShowOnActiveSpace
        );
    }

    #[test]
    fn compatibility_checklist_keeps_pending_catalog_empty() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/apps.json")).unwrap();
        let compat: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/compatibility.json")).unwrap();
        assert_eq!(compat["nativeSnapLimit"], 1);
        assert_eq!(compat["nativeSnapCeiling"], 2);
        assert!(
            compat["nativeSnapLimit"].as_u64().unwrap()
                <= compat["nativeSnapCeiling"].as_u64().unwrap()
        );
        assert!(compat.get("webUrl").is_none());
        assert!(compat.get("bundleId").is_none());
        assert!(compat["openP0"].as_array().unwrap().is_empty());
        let apps = catalog["apps"].as_array().unwrap();
        let find = |id: &str| apps.iter().find(|app| app["id"] == id).unwrap();
        for id in compat["confirmedWeb"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            let app = find(id);
            assert_eq!(app["kind"], "web", "{id}");
            assert_eq!(app["webStatus"], "official", "{id}");
            assert!(
                app["webUrl"].as_str().unwrap().starts_with("https://"),
                "{id}"
            );
        }
        for id in compat["confirmedDesktop"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            let app = find(id);
            assert_eq!(app["kind"], "native", "{id}");
            assert_eq!(app["bundleIdStatus"], "confirmed", "{id}");
            assert!(app["bundleId"].as_str().unwrap().contains('.'), "{id}");
        }
        for id in compat["pendingDoNotOpen"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            let app = find(id);
            assert!(app["webUrl"].is_null(), "{id}");
            assert_eq!(app["webStatus"], "pending", "{id}");
        }
        for id in compat["pendingBundle"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            let app = find(id);
            assert!(app["bundleId"].is_null(), "{id}");
            assert_eq!(app["bundleIdStatus"], "pending", "{id}");
        }
        let cursor = find("cursor-desktop");
        assert_eq!(cursor["bundleId"], "com.todesktop.230313mzl4w4u92");
        assert!(find("cursor")["bundleId"].is_null());
        assert!(find("cursor")["webUrl"].is_null());
    }
}
