//! macOS partition webviews.
//!
//! macOS 14+ uses the public persistent `WKWebsiteDataStore` identifier.
//! macOS 13 has only one public persistent store, so each app gets its own
//! directories on `_WKWebsiteDataStoreConfiguration` instead. Neither path uses
//! `nonPersistent` storage.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, ClassType, MainThreadMarker};
use objc2_foundation::{NSProcessInfo, NSString, NSURL};
use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::partition::{
    backend_detail, backend_name, identifier_string, isolation_failure, parse_marker,
    partition_identifier, require_week2_site, site_by_id, site_label, storage_hint,
    store_kind_for_major, PartitionStoreKind, ProbeReport, ProbeSample, WEEK2_SITES,
};

static OAUTH_SEQ: AtomicU64 = AtomicU64::new(1);
static PROBE_SEQ: AtomicU64 = AtomicU64::new(1);
static PROBE_BUSY: AtomicBool = AtomicBool::new(false);

const POPUP_CLOSE_SCRIPT: &str = r#"
(() => {
  window.close = () => {
    document.title = "__uw_close__";
  };
})();
"#;

pub fn show_partition(
    app: AppHandle,
    app_id: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let site = require_week2_site(&app_id, &url)?;
    if width < 2.0 || height < 2.0 {
        return Ok(());
    }
    hide_except(&app, Some(site.id))?;
    let label = site_label(site.id);
    if let Some(webview) = app.get_webview(&label) {
        place(&webview, x, y, width, height)?;
        webview.show().map_err(|err| err.to_string())?;
        webview.set_focus().map_err(|err| err.to_string())?;
        return Ok(());
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;
    let parsed = Url::parse(site.url).map_err(|err| err.to_string())?;
    let kind = current_store_kind()?;
    let mut builder = WebviewBuilder::new(label.as_str(), WebviewUrl::External(parsed));
    builder = isolated_builder(builder, &app, site.id, kind)?;
    let popup_app = app.clone();
    let parent = label.clone();
    builder = builder
        .focused(true)
        .on_new_window(move |target, features| {
            open_oauth_window(&popup_app, &parent, target, features)
        });
    window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width, height),
        )
        .map_err(|err| err.to_string())?;
    remember_partitions(&app, kind);
    Ok(())
}

pub fn set_partition_bounds(
    app: AppHandle,
    app_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if site_by_id(&app_id).is_none() || width < 2.0 || height < 2.0 {
        return Ok(());
    }
    let Some(webview) = app.get_webview(&site_label(&app_id)) else {
        return Ok(());
    };
    place(&webview, x, y, width, height)
}

pub fn hide_partitions(app: &AppHandle) -> Result<(), String> {
    hide_except(app, None)
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
    let labels: Vec<String> = WEEK2_SITES
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
        hints: WEEK2_SITES
            .iter()
            .map(|site| format!("{}  {}", site.id, storage_hint(kind, site.id)))
            .collect(),
        samples: Mutex::new(Vec::new()),
        writes_started: AtomicBool::new(false),
    });

    for (index, site) in WEEK2_SITES.iter().enumerate() {
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
        std::thread::sleep(Duration::from_secs(20));
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
        if index >= WEEK2_SITES.len() {
            self.read_marker(0);
            return;
        }
        let app_id = WEEK2_SITES[index].id;
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
            let next = next.clone();
            let _ = next
                .app
                .run_on_main_thread(move || next.write_marker(index + 1));
        }) {
            self.fail(format!("无法写入 {app_id} 的标记: {err}"));
        }
    }

    fn read_marker(self: Arc<Self>, index: usize) {
        if index >= WEEK2_SITES.len() {
            self.complete();
            return;
        }
        let site = &WEEK2_SITES[index];
        let label = self.labels[index].clone();
        let Some(webview) = self.app.get_webview(&label) else {
            self.fail(format!("探测页 {label} 已关闭"));
            return;
        };
        let script = r#"(function(){var prefix='uw-partition=';var cookie='';var parts=(document.cookie||'').split('; ');for(var i=0;i<parts.length;i++){if(parts[i].indexOf(prefix)===0){cookie=parts[i].slice(prefix.length);}}return {local:localStorage.getItem('uw-partition')||'',cookie:cookie};})()"#;
        let app_id = site.id.to_string();
        let next = self.clone();
        if let Err(err) = webview.eval_with_callback(script, move |raw| {
            let next = next.clone();
            let app_id = app_id.clone();
            let _ = next
                .app
                .run_on_main_thread(move || match parse_marker(&raw) {
                    Ok((local_storage, cookie)) => {
                        if let Ok(mut samples) = next.samples.lock() {
                            samples.push(ProbeSample {
                                app_id: app_id.clone(),
                                identifier: identifier_string(&app_id),
                                local_storage,
                                cookie,
                            });
                        }
                        next.read_marker(index + 1);
                    }
                    Err(message) => next.fail(message),
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
                "四个分区在同一来源上写入了不同的 cookie 和 localStorage，互相读不到。".into()
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
        let _ = app.run_on_main_thread(move || {
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
) -> NewWindowResponse {
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

fn hide_except(app: &AppHandle, keep: Option<&str>) -> Result<(), String> {
    for site in WEEK2_SITES {
        if Some(site.id) == keep {
            continue;
        }
        if let Some(webview) = app.get_webview(&site_label(site.id)) {
            webview.hide().map_err(|err| err.to_string())?;
        }
    }
    Ok(())
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
    let class = AnyClass::get("_WKWebsiteDataStoreConfiguration")
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
    let sel = objc2::runtime::Sel::register(selector);
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

fn remember_partitions(app: &AppHandle, kind: PartitionStoreKind) {
    let Ok(root) = app.path().app_data_dir() else {
        return;
    };
    let dir = root.join("partitions");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let entries: Vec<_> = WEEK2_SITES
        .iter()
        .map(|site| {
            serde_json::json!({
                "appId": site.id,
                "url": site.url,
                "identifier": identifier_string(site.id),
                "persistent": true,
                "inMemory": false,
                "backend": backend_name(kind),
                "hint": storage_hint(kind, site.id),
            })
        })
        .collect();
    let Ok(body) = serde_json::to_string_pretty(&entries) else {
        return;
    };
    let _ = std::fs::write(dir.join("index.json"), body);
}
