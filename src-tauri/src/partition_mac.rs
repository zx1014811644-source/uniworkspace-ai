//! macOS partition webviews.
//!
//! macOS 14+ uses the public persistent `WKWebsiteDataStore` identifier.
//! macOS 13 has only one public persistent store, so each app gets its own
//! directories on `_WKWebsiteDataStoreConfiguration` instead. Neither path uses
//! `nonPersistent` storage.

use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, ClassType, MainThreadMarker};
use objc2_foundation::{NSProcessInfo, NSString, NSURL};
use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
use serde::Serialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};
use url::Url;

use crate::partition::{
    authorize_panes, backend_detail, backend_name, default_workspace, identifier_string,
    isolation_failure, official_site, pane_should_freeze, parse_marker, partition_identifier,
    plan_pane_crash, read_workspace, resolve_pane_focus, site_label, storage_hint,
    store_kind_for_major, OpenPane, PaneSpec, PartitionStoreKind, ProbeReport, ProbeSample,
    RebuildGate, OFFICIAL_SITES,
};

static OAUTH_SEQ: AtomicU64 = AtomicU64::new(1);
static PROBE_SEQ: AtomicU64 = AtomicU64::new(1);
static PROBE_BUSY: AtomicBool = AtomicBool::new(false);
static PANES: LazyLock<Mutex<HashMap<String, LivePane>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static GATES: LazyLock<Mutex<HashMap<String, RebuildGate>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FOCUSED_LABEL: Mutex<String> = Mutex::new(String::new());
static SHELL_VISIBLE: AtomicBool = AtomicBool::new(true);
static NATIVE_FOCUS_AT: AtomicU64 = AtomicU64::new(0);
static FROZEN: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static VIEW_LABELS: LazyLock<Mutex<HashMap<usize, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const NATIVE_FOCUS_FRESH_MS: u128 = 800;

#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

unsafe impl Encode for NsPoint {
    const ENCODING: Encoding = Encoding::Struct("_NSPoint", &[f64::ENCODING, f64::ENCODING]);
}

unsafe impl RefEncode for NsPoint {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

#[derive(Clone)]
struct LivePane {
    app_id: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    visible: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaneNotice {
    app_id: String,
    message: String,
}

const POPUP_CLOSE_SCRIPT: &str = r#"
(() => {
  window.close = () => {
    document.title = "__uw_close__";
  };
})();
"#;

pub fn note_shell_visible(app: &AppHandle, visible: bool) {
    SHELL_VISIBLE.store(visible, Ordering::SeqCst);
    let focused = FOCUSED_LABEL
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let labels: Vec<(String, bool)> = lock_panes()
        .iter()
        .map(|(label, pane)| (label.clone(), pane.visible))
        .collect();
    for (label, pane_visible) in labels {
        let freeze = !pane_visible || pane_should_freeze(label == focused, visible);
        request_freeze(app, &label, freeze);
    }
}

fn sync_freeze(app: &AppHandle, label: &str, focused: bool) {
    let pane_visible = lock_panes()
        .get(label)
        .map(|pane| pane.visible)
        .unwrap_or(true);
    let freeze = !pane_visible || pane_should_freeze(focused, SHELL_VISIBLE.load(Ordering::SeqCst));
    request_freeze(app, label, freeze);
}

fn request_freeze(app: &AppHandle, label: &str, freeze: bool) {
    {
        let mut frozen = FROZEN.lock().unwrap_or_else(|err| err.into_inner());
        if frozen.get(label) == Some(&freeze) {
            return;
        }
        frozen.insert(label.to_string(), freeze);
    }
    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        let Some(webview) = app.get_webview(&label) else {
            FROZEN
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .remove(&label);
            return;
        };
        let _ = webview.with_webview(move |platform| {
            let desired = FROZEN
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .get(&label)
                .copied()
                .unwrap_or(freeze);
            apply_page_freeze(platform.inner(), &label, desired);
        });
    });
}

fn apply_page_freeze(view: *mut std::ffi::c_void, label: &str, freeze: bool) {
    if view.is_null() {
        return;
    }
    VIEW_LABELS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(view as usize, label.to_string());
    let view = view as *mut AnyObject;
    let page_sel = if freeze {
        objc2::runtime::Sel::register(c"_suspendPage:")
    } else {
        objc2::runtime::Sel::register(c"_resumePage:")
    };
    let page_ok: bool = unsafe { msg_send![view, respondsToSelector: page_sel] };
    if page_ok {
        let block = RcBlock::new(|_ok: Bool| {});
        unsafe {
            if freeze {
                let _: () = msg_send![view, _suspendPage: &*block];
            } else {
                let _: () = msg_send![view, _resumePage: &*block];
            }
        }
    }
    let media_sel = if freeze {
        objc2::runtime::Sel::register(c"_suspendAllMediaPlayback")
    } else {
        objc2::runtime::Sel::register(c"_resumeAllMediaPlayback")
    };
    let media_ok: bool = unsafe { msg_send![view, respondsToSelector: media_sel] };
    if media_ok {
        unsafe {
            if freeze {
                let _: () = msg_send![view, _suspendAllMediaPlayback];
            } else {
                let _: () = msg_send![view, _resumeAllMediaPlayback];
            }
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn wk_ancestor(mut view: *mut AnyObject) -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"WKWebView")?;
    let mut depth = 0;
    while !view.is_null() && depth < 32 {
        let is_webview: bool = unsafe { msg_send![view, isKindOfClass: class] };
        if is_webview {
            return Some(view);
        }
        view = unsafe { msg_send![view, superview] };
        depth += 1;
    }
    None
}

fn activate_clicked_partition(app: &AppHandle, event: *mut AnyObject) {
    let window: *mut AnyObject = unsafe { msg_send![event, window] };
    if window.is_null() {
        return;
    }
    let point: NsPoint = unsafe { msg_send![event, locationInWindow] };
    let content: *mut AnyObject = unsafe { msg_send![window, contentView] };
    if content.is_null() {
        return;
    }
    let hit: *mut AnyObject = unsafe { msg_send![content, hitTest: point] };
    let Some(webview) = wk_ancestor(hit) else {
        return;
    };
    let label = VIEW_LABELS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(&(webview as usize))
        .cloned();
    let Some(label) = label else {
        return;
    };
    let already = focused_label_is(&label);
    let frozen = FROZEN
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(&label)
        .copied()
        .unwrap_or(false);
    if already && !frozen {
        return;
    }
    let previous = {
        let mut focused = FOCUSED_LABEL.lock().unwrap_or_else(|err| err.into_inner());
        let previous = focused.clone();
        *focused = label.clone();
        previous
    };
    NATIVE_FOCUS_AT.store(
        u64::try_from(now_ms()).unwrap_or(u64::MAX),
        Ordering::Release,
    );
    if !previous.is_empty() && previous != label {
        request_freeze(app, &previous, true);
    }
    request_freeze(
        app,
        &label,
        pane_should_freeze(true, SHELL_VISIBLE.load(Ordering::Acquire)),
    );
    if let Some(app_id) = label.strip_prefix("partition-") {
        let _ = app.emit(
            "pane-activated",
            PaneNotice {
                app_id: app_id.to_string(),
                message: String::new(),
            },
        );
    }
}

pub fn install_activation_monitor(app: &AppHandle) {
    static HOST: OnceLock<AppHandle> = OnceLock::new();
    static ONCE: std::sync::Once = std::sync::Once::new();
    let _ = HOST.set(app.clone());
    ONCE.call_once(|| {
        let block = RcBlock::new(|event: *mut AnyObject| -> *mut AnyObject {
            if !event.is_null() {
                if let Some(host) = HOST.get() {
                    activate_clicked_partition(host, event);
                }
            }
            event
        });
        let block = Box::leak(Box::new(block));
        if let Some(class) = AnyClass::get(c"NSEvent") {
            let mask: u64 = 1 << 1;
            let handler = RcBlock::as_ptr(block);
            let _: *mut AnyObject = unsafe {
                msg_send![
                    class,
                    addLocalMonitorForEventsMatchingMask: mask,
                    handler: handler
                ]
            };
        }
    });
}

pub fn sync_panes(app: AppHandle, panes: Vec<PaneSpec>) -> Result<(), String> {
    let workspace = read_workspace(&app)?;
    let opened = authorize_panes(&panes, &workspace)?;
    let kind = current_store_kind()?;
    let requested = opened
        .iter()
        .find(|pane| pane.focus)
        .map(|pane| site_label(&pane.app_id));
    let native = FOCUSED_LABEL
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let age = now_ms().saturating_sub(u128::from(NATIVE_FOCUS_AT.load(Ordering::Acquire)));
    let chosen = resolve_pane_focus(requested.as_deref(), &native, age, NATIVE_FOCUS_FRESH_MS);
    let mut errors = Vec::new();
    let mut visible_labels = Vec::new();
    for pane in &opened {
        let mut pane = pane.clone();
        let label = site_label(&pane.app_id);
        pane.focus = chosen.as_deref() == Some(label.as_str());
        visible_labels.push(label);
        if let Err(err) = ensure_visible(&app, &pane, kind) {
            errors.push(err);
        }
    }
    hide_unlisted(&app, &visible_labels)?;
    write_partition_index(&app);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

pub fn focus_partition(app: &AppHandle, app_id: &str) -> Result<(), String> {
    let label = site_label(app_id);
    let Some(webview) = app.get_webview(&label) else {
        return Ok(());
    };
    webview.show().map_err(|err| err.to_string())?;
    focus_once(app, &webview, &label)?;
    sync_freeze(app, &label, true);
    Ok(())
}

pub fn hide_partitions(app: &AppHandle) -> Result<(), String> {
    hide_unlisted(app, &[])
}

pub fn handle_content_process_terminated(webview: &tauri::Webview) {
    let label = webview.label().to_string();
    if !label.starts_with("partition-") {
        let _ = webview.reload();
        return;
    }
    let app = webview.app_handle().clone();
    std::thread::spawn(move || {
        let app_for_task = app.clone();
        let _ = app.run_on_main_thread(move || rebuild_crashed_pane(&app_for_task, &label));
    });
}

fn ensure_visible(
    app: &AppHandle,
    pane: &OpenPane,
    kind: PartitionStoreKind,
) -> Result<(), String> {
    let label = site_label(&pane.app_id);
    {
        let mut panes = lock_panes();
        panes.insert(
            label.clone(),
            LivePane {
                app_id: pane.app_id.clone(),
                url: pane.url.clone(),
                x: pane.x,
                y: pane.y,
                width: pane.width,
                height: pane.height,
                visible: true,
            },
        );
    }
    if let Some(webview) = app.get_webview(&label) {
        place(&webview, pane.x, pane.y, pane.width, pane.height)?;
        webview.show().map_err(|err| err.to_string())?;
        if pane.focus {
            focus_once(app, &webview, &label)?;
        }
        sync_freeze(app, &label, pane.focus);
        return Ok(());
    }
    reset_rebuild_gate(&label);
    create_child(app, &label, pane, kind)?;
    if pane.focus {
        if let Some(webview) = app.get_webview(&label) {
            focus_once(app, &webview, &label)?;
        }
    }
    sync_freeze(app, &label, pane.focus);
    Ok(())
}

fn create_child(
    app: &AppHandle,
    label: &str,
    pane: &OpenPane,
    kind: PartitionStoreKind,
) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;
    let parsed = Url::parse(&pane.url).map_err(|err| err.to_string())?;
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(parsed));
    builder = isolated_builder(builder, app, &pane.app_id, kind)?;
    let popup_app = app.clone();
    let parent = label.to_string();
    builder = builder
        .focused(false)
        .on_new_window(move |target, features| {
            open_oauth_window(&popup_app, &parent, target, features)
        });
    window
        .add_child(
            builder,
            LogicalPosition::new(pane.x, pane.y),
            LogicalSize::new(pane.width, pane.height),
        )
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn rebuild_crashed_pane(app: &AppHandle, crashed_label: &str) {
    let open_labels: Vec<String> = lock_panes().keys().cloned().collect();
    let plan = plan_pane_crash(crashed_label, &open_labels);
    if plan.rebuild != vec![crashed_label.to_string()] {
        return;
    }
    let Some(runtime) = lock_panes().get(crashed_label).cloned() else {
        return;
    };
    if !allow_rebuild(crashed_label) {
        let _ = close_label(app, crashed_label);
        emit_pane(
            app,
            "pane-rebuild-failed",
            &runtime.app_id,
            "这一格连续崩溃，已停止重建。再从侧栏点一次可以重试。",
        );
        return;
    }
    let _ = close_label(app, crashed_label);
    if app.get_webview(crashed_label).is_some() {
        if let Some(webview) = app.get_webview(crashed_label) {
            if webview.reload().is_ok() {
                let focused = focused_label_is(crashed_label);
                sync_freeze(app, crashed_label, focused && runtime.visible);
                emit_pane(
                    app,
                    "pane-rebuilt",
                    &runtime.app_id,
                    "这一格的页面已重新载入，其他格子没有动。",
                );
                return;
            }
        }
        emit_pane(
            app,
            "pane-rebuild-failed",
            &runtime.app_id,
            "这一格崩溃后没能重建。",
        );
        return;
    }
    let kind = match current_store_kind() {
        Ok(kind) => kind,
        Err(message) => {
            emit_pane(app, "pane-rebuild-failed", &runtime.app_id, &message);
            return;
        }
    };
    let pane = OpenPane {
        app_id: runtime.app_id.clone(),
        url: runtime.url.clone(),
        x: runtime.x,
        y: runtime.y,
        width: runtime.width,
        height: runtime.height,
        focus: false,
    };
    if let Err(message) = create_child(app, crashed_label, &pane, kind) {
        emit_pane(app, "pane-rebuild-failed", &runtime.app_id, &message);
        return;
    }
    if !runtime.visible {
        if let Some(webview) = app.get_webview(crashed_label) {
            let _ = webview.hide();
        }
        request_freeze(app, crashed_label, true);
    } else {
        let focused = focused_label_is(crashed_label);
        if focused {
            if let Some(webview) = app.get_webview(crashed_label) {
                let _ = webview.set_focus();
            }
        }
        sync_freeze(app, crashed_label, focused);
    }
    emit_pane(
        app,
        "pane-rebuilt",
        &runtime.app_id,
        "这一格已按原来的分区重新打开，其他格子没有动。",
    );
}

fn hide_unlisted(app: &AppHandle, keep: &[String]) -> Result<(), String> {
    let workspace = read_workspace(app).unwrap_or_else(|_| default_workspace());
    let labels: Vec<String> = lock_panes().keys().cloned().collect();
    for label in labels {
        if keep.iter().any(|item| item == &label) {
            continue;
        }
        let app_id = lock_panes().get(&label).map(|pane| pane.app_id.clone());
        let known = app_id.as_deref().is_some_and(|id| {
            official_site(id).is_some() || workspace.custom_apps.iter().any(|item| item.id == id)
        });
        if !known {
            let _ = close_label(app, &label);
            lock_panes().remove(&label);
            continue;
        }
        if let Some(webview) = app.get_webview(&label) {
            request_freeze(app, &label, true);
            webview.hide().map_err(|err| err.to_string())?;
        } else {
            request_freeze(app, &label, true);
        }
        if let Some(pane) = lock_panes().get_mut(&label) {
            pane.visible = false;
        }
    }
    Ok(())
}

fn close_label(app: &AppHandle, label: &str) -> Result<(), String> {
    FROZEN
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(label);
    VIEW_LABELS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .retain(|_, value| value != label);
    let Some(webview) = app.get_webview(label) else {
        return Ok(());
    };
    webview.close().map_err(|err| err.to_string())
}

fn focused_label_is(label: &str) -> bool {
    FOCUSED_LABEL
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_str()
        == label
}

fn focus_once(app: &AppHandle, webview: &tauri::Webview, label: &str) -> Result<(), String> {
    let previous = {
        let mut focused = FOCUSED_LABEL.lock().unwrap_or_else(|err| err.into_inner());
        if *focused == label {
            return Ok(());
        }
        let previous = focused.clone();
        webview.set_focus().map_err(|err| err.to_string())?;
        *focused = label.to_string();
        NATIVE_FOCUS_AT.store(0, Ordering::Release);
        previous
    };
    if !previous.is_empty() && previous != label {
        request_freeze(app, &previous, true);
    }
    Ok(())
}

fn reset_rebuild_gate(label: &str) {
    let mut gates = GATES.lock().unwrap_or_else(|err| err.into_inner());
    gates.remove(label);
}

fn allow_rebuild(label: &str) -> bool {
    let mut gates = GATES.lock().unwrap_or_else(|err| err.into_inner());
    let gate = gates
        .entry(label.to_string())
        .or_insert_with(|| RebuildGate::new(3, 10_000));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    gate.allow(now)
}

fn emit_pane(app: &AppHandle, event: &str, app_id: &str, message: &str) {
    let _ = app.emit(
        event,
        PaneNotice {
            app_id: app_id.to_string(),
            message: message.to_string(),
        },
    );
}

fn lock_panes() -> std::sync::MutexGuard<'static, HashMap<String, LivePane>> {
    PANES.lock().unwrap_or_else(|err| err.into_inner())
}

pub async fn probe_partitions(app: AppHandle) -> Result<ProbeReport, String> {
    let (tx, rx) = mpsc::channel();
    let app_for_probe = app.clone();
    app.run_on_main_thread(move || start_probe(app_for_probe, tx))
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|_| "隔离检查中断".to_string())?
    })
    .await
    .map_err(|err| err.to_string())?
}

fn start_probe(app: AppHandle, tx: Sender<Result<ProbeReport, String>>) {
    if PROBE_BUSY.swap(true, Ordering::SeqCst) {
        let _ = tx.send(Err("上一次隔离检查还在进行".into()));
        return;
    }
    if let Err((tx, message)) = assemble_probe(app, tx) {
        PROBE_BUSY.store(false, Ordering::SeqCst);
        let _ = tx.send(Err(message));
    }
}

fn assemble_probe(
    app: AppHandle,
    tx: Sender<Result<ProbeReport, String>>,
) -> Result<(), (Sender<Result<ProbeReport, String>>, String)> {
    let server = match start_probe_server() {
        Ok(server) => server,
        Err(message) => return Err((tx, message)),
    };
    let kind = match current_store_kind() {
        Ok(kind) => kind,
        Err(message) => {
            server.stop.store(true, Ordering::Relaxed);
            return Err((tx, message));
        }
    };
    let window = match app.get_window("main") {
        Some(window) => window,
        None => {
            server.stop.store(true, Ordering::Relaxed);
            return Err((tx, "找不到主窗口".into()));
        }
    };
    let page = match Url::parse(&format!("http://127.0.0.1:{}/isolation.html", server.port)) {
        Ok(page) => page,
        Err(err) => {
            server.stop.store(true, Ordering::Relaxed);
            return Err((tx, err.to_string()));
        }
    };
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let labels: Vec<String> = OFFICIAL_SITES
        .iter()
        .map(|site| format!("probe-{seq}-{}", site.id))
        .collect();
    let probe = Arc::new(LiveProbe {
        app: app.clone(),
        tx: Mutex::new(Some(tx)),
        pending: Mutex::new(labels.clone()),
        labels: labels.clone(),
        stop_server: server.stop.clone(),
        backend: backend_detail(kind).to_string(),
        hints: OFFICIAL_SITES
            .iter()
            .map(|site| format!("{}  {}", site.id, storage_hint(kind, site.id)))
            .collect(),
        samples: Mutex::new(Vec::new()),
        writes_started: AtomicBool::new(false),
    });

    for (index, site) in OFFICIAL_SITES.iter().enumerate() {
        let label = labels[index].clone();
        let loaded = probe.clone();
        let loaded_label = label.clone();
        let builder = match isolated_builder(
            WebviewBuilder::new(label.as_str(), WebviewUrl::External(page.clone())).focused(false),
            &app,
            site.id,
            kind,
        ) {
            Ok(builder) => builder,
            Err(message) => {
                probe.fail(message);
                return Ok(());
            }
        };
        let builder = builder.on_page_load(move |_webview, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            if !payload.url().as_str().contains("127.0.0.1") {
                return;
            }
            loaded.mark_loaded(&loaded_label);
        });
        if let Err(err) = window.add_child(
            builder,
            LogicalPosition::new(12_000.0, 12_000.0),
            LogicalSize::new(800.0, 600.0),
        ) {
            probe.fail(format!("无法创建探测页: {err}"));
            return Ok(());
        }
    }

    let timeout = probe.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        timeout.fail("隔离检查超时。本机需要允许应用访问 127.0.0.1。".into());
    });
    Ok(())
}

struct ProbeServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

fn start_probe_server() -> Result<ProbeServer, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|err| format!("无法打开隔离检查服务: {err}"))?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    listener
        .set_nonblocking(true)
        .map_err(|err| err.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    std::thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = write_probe_page(&mut stream);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(ProbeServer { port, stop })
}

fn write_probe_page(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut buf = [0_u8; 1024];
    let _ = stream.read(&mut buf);
    let body = b"<!doctype html><meta charset=\"utf-8\"><title>uw-probe</title>";
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

struct LiveProbe {
    app: AppHandle,
    tx: Mutex<Option<Sender<Result<ProbeReport, String>>>>,
    pending: Mutex<Vec<String>>,
    labels: Vec<String>,
    stop_server: Arc<AtomicBool>,
    backend: String,
    hints: Vec<String>,
    samples: Mutex<Vec<ProbeSample>>,
    writes_started: AtomicBool,
}

impl Drop for LiveProbe {
    fn drop(&mut self) {
        self.stop_server.store(true, Ordering::Relaxed);
    }
}

impl LiveProbe {
    fn mark_loaded(self: &Arc<Self>, label: &str) {
        let ready = {
            let mut pending = match self.pending.lock() {
                Ok(pending) => pending,
                Err(_) => return,
            };
            pending.retain(|item| item != label);
            pending.is_empty()
        };
        if ready && !self.writes_started.swap(true, Ordering::SeqCst) {
            self.clone().write_marker(0);
        }
    }

    fn write_marker(self: Arc<Self>, index: usize) {
        if index >= OFFICIAL_SITES.len() {
            self.read_marker(0);
            return;
        }
        let app_id = OFFICIAL_SITES[index].id;
        let label = self.labels[index].clone();
        let Some(webview) = self.app.get_webview(&label) else {
            self.fail(format!("探测页 {label} 已关闭"));
            return;
        };
        let script = format!(
            "(function(){{localStorage.setItem('uw-partition','{app_id}');document.cookie='uw-partition={app_id}; path=/; SameSite=Lax';return true;}})()"
        );
        let next = self.clone();
        if let Err(err) = webview.eval_with_callback(script, move |_| {
            let step = next.clone();
            let hop = step.clone();
            let _ = hop
                .app
                .run_on_main_thread(move || step.write_marker(index + 1));
        }) {
            self.fail(format!("无法写入 {app_id} 的标记: {err}"));
        }
    }

    fn read_marker(self: Arc<Self>, index: usize) {
        if index >= OFFICIAL_SITES.len() {
            self.complete();
            return;
        }
        let site = &OFFICIAL_SITES[index];
        let label = self.labels[index].clone();
        let Some(webview) = self.app.get_webview(&label) else {
            self.fail(format!("探测页 {label} 已关闭"));
            return;
        };
        let script = r#"(function(){var prefix='uw-partition=';var cookie='';var parts=(document.cookie||'').split('; ');for(var i=0;i<parts.length;i++){if(parts[i].indexOf(prefix)===0){cookie=parts[i].slice(prefix.length);}}return {local:localStorage.getItem('uw-partition')||'',cookie:cookie};})()"#;
        let app_id = site.id.to_string();
        let next = self.clone();
        let callback_id = app_id.clone();
        if let Err(err) = webview.eval_with_callback(script, move |raw| {
            let step = next.clone();
            let hop = step.clone();
            let app_id = callback_id.clone();
            let _ = hop
                .app
                .run_on_main_thread(move || match parse_marker(&raw) {
                    Ok((local_storage, cookie)) => {
                        if let Ok(mut samples) = step.samples.lock() {
                            samples.push(ProbeSample {
                                app_id: app_id.clone(),
                                identifier: identifier_string(&app_id),
                                local_storage,
                                cookie,
                            });
                        }
                        step.read_marker(index + 1);
                    }
                    Err(message) => step.fail(message),
                });
        }) {
            self.fail(format!("无法读取 {app_id} 的标记: {err}"));
        }
    }

    fn complete(self: &Arc<Self>) {
        let samples = self
            .samples
            .lock()
            .map(|samples| samples.clone())
            .unwrap_or_default();
        let failure = isolation_failure(&samples);
        let report = ProbeReport {
            isolated: failure.is_none(),
            backend: self.backend.clone(),
            detail: failure.unwrap_or_else(|| {
                "已确认的网页分区在同一来源上写入了不同的 cookie 和 localStorage，互相读不到。"
                    .into()
            }),
            hints: self.hints.clone(),
            samples,
        };
        self.finish(Ok(report));
    }

    fn fail(self: &Arc<Self>, message: String) {
        self.finish(Err(message));
    }

    fn finish(self: &Arc<Self>, result: Result<ProbeReport, String>) {
        let sender = self.tx.lock().ok().and_then(|mut slot| slot.take());
        let Some(sender) = sender else {
            return;
        };
        self.stop_server.store(true, Ordering::Relaxed);
        PROBE_BUSY.store(false, Ordering::SeqCst);
        let app = self.app.clone();
        let labels = self.labels.clone();
        let hop = app.clone();
        let _ = hop.run_on_main_thread(move || {
            for label in labels {
                if let Some(webview) = app.get_webview(&label) {
                    let _ = webview.close();
                }
            }
        });
        let _ = sender.send(result);
    }
}

fn open_oauth_window(
    app: &AppHandle,
    parent_label: &str,
    url: Url,
    features: tauri::webview::NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    if !matches!(url.scheme(), "https" | "http" | "about") {
        return NewWindowResponse::Deny;
    }
    let seq = OAUTH_SEQ.fetch_add(1, Ordering::Relaxed);
    let label = format!("oauth-{parent_label}-{seq}");
    let mut builder = WebviewWindowBuilder::new(app, label.as_str(), WebviewUrl::External(url))
        .title("登录")
        .inner_size(520.0, 720.0)
        .center()
        .focused(true)
        .initialization_script(POPUP_CLOSE_SCRIPT)
        .on_document_title_changed(|window, title| {
            if title == "__uw_close__" {
                let _ = window.close();
            } else if !title.is_empty() {
                let _ = window.set_title(&title);
            }
        })
        .window_features(features);
    let nested_app = app.clone();
    let nested_parent = label.clone();
    builder = builder.on_new_window(move |target, features| {
        open_oauth_window(&nested_app, &nested_parent, target, features)
    });
    match builder.build() {
        Ok(window) => {
            if let Ok(size) = window.inner_size() {
                if size.width < 80 || size.height < 80 {
                    let _ = window.set_size(LogicalSize::new(520.0, 720.0));
                }
            }
            NewWindowResponse::Create { window }
        }
        Err(err) => {
            eprintln!("oauth window failed: {err}");
            NewWindowResponse::Deny
        }
    }
}

fn place(webview: &tauri::Webview, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    webview
        .set_position(LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    webview
        .set_size(LogicalSize::new(width, height))
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn isolated_builder(
    builder: WebviewBuilder<tauri::Wry>,
    app: &AppHandle,
    app_id: &str,
    kind: PartitionStoreKind,
) -> Result<WebviewBuilder<tauri::Wry>, String> {
    match kind {
        PartitionStoreKind::Identifier => {
            Ok(builder.data_store_identifier(partition_identifier(app_id)))
        }
        PartitionStoreKind::Configuration => {
            let config = spi_configuration(app, app_id)?;
            Ok(builder.with_webview_configuration(config))
        }
    }
}

fn current_store_kind() -> Result<PartitionStoreKind, String> {
    Ok(store_kind_for_major(macos_major()?))
}

fn macos_major() -> Result<u32, String> {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    u32::try_from(version.majorVersion).map_err(|_| "无法读取 macOS 版本".to_string())
}

fn spi_configuration(
    app: &AppHandle,
    app_id: &str,
) -> Result<Retained<WKWebViewConfiguration>, String> {
    let mtm = MainThreadMarker::new().ok_or("分区数据仓库必须在主线程创建")?;
    let root = partition_root(app, app_id)?;
    let config_object = spi_store_configuration(&root)?;
    let store = spi_store(&config_object)?;
    if !unsafe { store.isPersistent() } {
        return Err("macOS 13 分区仓库不是持久的，已停止打开，避免改用内存仓库".into());
    }
    let web_config = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe { web_config.setWebsiteDataStore(&store) };
    Ok(web_config)
}

fn partition_root(app: &AppHandle, app_id: &str) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))?;
    let root = base.join("partitions").join(app_id);
    std::fs::create_dir_all(&root).map_err(|err| format!("无法创建分区目录: {err}"))?;
    Ok(root)
}

fn spi_store_configuration(root: &Path) -> Result<Retained<AnyObject>, String> {
    let class = AnyClass::get(c"_WKWebsiteDataStoreConfiguration")
        .ok_or("系统没有 _WKWebsiteDataStoreConfiguration，不能在 macOS 13 上分开持久分区")?;
    let allocated: *mut AnyObject = unsafe { msg_send![class, alloc] };
    let initialized: *mut AnyObject = unsafe { msg_send![allocated, init] };
    let config = unsafe { Retained::from_raw(initialized) }.ok_or("无法创建分区数据仓库配置")?;
    assign_required(
        &config,
        "_setWebStorageDirectory:",
        &root.join("web-storage"),
        true,
    )?;
    assign_required(
        &config,
        "_setIndexedDBDatabaseDirectory:",
        &root.join("indexeddb"),
        true,
    )?;
    assign_required(
        &config,
        "_setCookieStorageFile:",
        &root.join("cookies").join("Cookies.binarycookies"),
        false,
    )?;
    assign_optional(
        &config,
        "setGeneralStorageDirectory:",
        &root.join("general"),
        true,
    )?;
    assign_optional(
        &config,
        "setNetworkCacheDirectory:",
        &root.join("network-cache"),
        true,
    )?;
    assign_optional(
        &config,
        "_setCacheStorageDirectory:",
        &root.join("cache-storage"),
        true,
    )?;
    assign_optional(
        &config,
        "_setServiceWorkerRegistrationDirectory:",
        &root.join("service-workers"),
        true,
    )?;
    assign_optional(
        &config,
        "_setResourceLoadStatisticsDirectory:",
        &root.join("resource-load-statistics"),
        true,
    )?;
    Ok(config)
}

fn assign_required(
    config: &AnyObject,
    selector: &str,
    path: &Path,
    directory: bool,
) -> Result<(), String> {
    if !assign_optional(config, selector, path, directory)? {
        return Err(format!(
            "系统数据仓库配置没有 {selector}，不能把这个分区做成持久仓库"
        ));
    }
    Ok(())
}

fn assign_optional(
    config: &AnyObject,
    selector: &str,
    path: &Path,
    directory: bool,
) -> Result<bool, String> {
    let name = CString::new(selector).map_err(|_| format!("选择子无效: {selector}"))?;
    let sel = objc2::runtime::Sel::register(&name);
    let responds: bool = unsafe { msg_send![config, respondsToSelector: sel] };
    if !responds {
        return Ok(false);
    }
    if directory {
        std::fs::create_dir_all(path)
            .map_err(|err| format!("无法创建 {}: {err}", path.display()))?;
    } else if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("无法创建 {}: {err}", parent.display()))?;
    }
    let text = path.to_string_lossy();
    let ns_path = NSString::from_str(&text);
    let url = NSURL::fileURLWithPath(&ns_path);
    unsafe {
        set_config_url(config, selector, &url);
    }
    Ok(true)
}

unsafe fn set_config_url(config: &AnyObject, selector: &str, url: &NSURL) {
    match selector {
        "_setWebStorageDirectory:" => {
            let _: () = msg_send![config, _setWebStorageDirectory: url];
        }
        "_setIndexedDBDatabaseDirectory:" => {
            let _: () = msg_send![config, _setIndexedDBDatabaseDirectory: url];
        }
        "_setCookieStorageFile:" => {
            let _: () = msg_send![config, _setCookieStorageFile: url];
        }
        "setGeneralStorageDirectory:" => {
            let _: () = msg_send![config, setGeneralStorageDirectory: url];
        }
        "setNetworkCacheDirectory:" => {
            let _: () = msg_send![config, setNetworkCacheDirectory: url];
        }
        "_setCacheStorageDirectory:" => {
            let _: () = msg_send![config, _setCacheStorageDirectory: url];
        }
        "_setServiceWorkerRegistrationDirectory:" => {
            let _: () = msg_send![config, _setServiceWorkerRegistrationDirectory: url];
        }
        "_setResourceLoadStatisticsDirectory:" => {
            let _: () = msg_send![config, _setResourceLoadStatisticsDirectory: url];
        }
        _ => {}
    }
}

fn spi_store(config: &AnyObject) -> Result<Retained<WKWebsiteDataStore>, String> {
    let allocated: *mut AnyObject = unsafe { msg_send![WKWebsiteDataStore::class(), alloc] };
    let initialized: *mut WKWebsiteDataStore =
        unsafe { msg_send![allocated, _initWithConfiguration: config] };
    unsafe { Retained::from_raw(initialized) }
        .ok_or_else(|| "macOS 13 无法按目录创建持久 WKWebsiteDataStore".into())
}

pub fn write_partition_index(app: &AppHandle) {
    let Ok(kind) = current_store_kind() else {
        return;
    };
    let Ok(workspace) = read_workspace(app) else {
        return;
    };
    let Ok(root) = app.path().app_data_dir() else {
        return;
    };
    let dir = root.join("partitions");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut entries = Vec::new();
    for site in OFFICIAL_SITES {
        entries.push(index_entry(site.id, site.url, kind));
    }
    for custom in &workspace.custom_apps {
        entries.push(index_entry(&custom.id, &custom.url, kind));
    }
    let Ok(body) = serde_json::to_string_pretty(&entries) else {
        return;
    };
    let _ = std::fs::write(dir.join("index.json"), body);
}

fn index_entry(app_id: &str, url: &str, kind: PartitionStoreKind) -> serde_json::Value {
    serde_json::json!({
        "appId": app_id,
        "url": url,
        "identifier": identifier_string(app_id),
        "persistent": true,
        "inMemory": false,
        "backend": backend_name(kind),
        "hint": storage_hint(kind, app_id),
    })
}
