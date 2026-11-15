//! Desktop detection for confirmed Bundle IDs, manual window listing, and
//! localhost port status.
//!
//! Matching uses the Bundle ID already stored in `catalog/apps.json`. Process
//! names only break ties. Nothing in here moves, follows, or reparents a window.
//! Ports are probed with a TCP connect on 127.0.0.1 and then dropped; no HTTP
//! body is sent.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::partition::BUNDLE_ID;

/// Leave enough room under the 2 second sidebar deadline for one missed poll
/// plus the port probe.
pub const SCAN_INTERVAL_MS: u64 = 1000;
pub const PORT_TIMEOUT_MS: u64 = 200;
pub const MAX_LISTED_WINDOWS: usize = 200;
pub const A11Y_FILE: &str = "a11y.json";
pub const COMFYUI_OPEN_URL: &str = "http://127.0.0.1:8188/";

pub struct ConfirmedDesktop {
    pub id: &'static str,
    pub name: &'static str,
    pub bundle_id: &'static str,
    pub process_names: &'static [&'static str],
}

pub const CONFIRMED_DESKTOP: &[ConfirmedDesktop] = &[
    ConfirmedDesktop {
        id: "chatgpt-desktop",
        name: "ChatGPT Desktop",
        bundle_id: "com.openai.codex",
        process_names: &["ChatGPT"],
    },
    ConfirmedDesktop {
        id: "claude-desktop",
        name: "Claude Desktop",
        bundle_id: "com.anthropic.claudefordesktop",
        process_names: &["Claude", "Claude Helper"],
    },
    ConfirmedDesktop {
        id: "lm-studio",
        name: "LM Studio",
        bundle_id: "ai.elementlabs.lmstudio",
        process_names: &["LM Studio"],
    },
    ConfirmedDesktop {
        id: "gemini-desktop",
        name: "Gemini Desktop",
        bundle_id: "com.google.GeminiMacOS",
        process_names: &["Gemini"],
    },
    ConfirmedDesktop {
        id: "kimi-desktop",
        name: "Kimi Desktop",
        bundle_id: "com.moonshot.kimichat",
        process_names: &["Kimi"],
    },
    ConfirmedDesktop {
        id: "doubao-desktop",
        name: "豆包 Desktop",
        bundle_id: "com.bot.pc.doubao",
        process_names: &["Doubao"],
    },
    ConfirmedDesktop {
        id: "cursor-desktop",
        name: "Cursor Desktop",
        bundle_id: "com.todesktop.230313mzl4w4u92",
        process_names: &["Cursor"],
    },
];

struct LocalService {
    id: &'static str,
    name: &'static str,
    port: u16,
    open_url: Option<&'static str>,
}

const LOCAL_SERVICES: &[LocalService] = &[
    LocalService {
        id: "comfyui",
        name: "ComfyUI",
        port: 8188,
        open_url: Some(COMFYUI_OPEN_URL),
    },
    LocalService {
        id: "ollama",
        name: "Ollama",
        port: 11434,
        open_url: None,
    },
    LocalService {
        id: "lmstudio-local",
        name: "LM Studio 本地服务",
        port: 1234,
        open_url: None,
    },
];

pub fn status() -> &'static str {
    "detector: bundle-id scan, window list, local port status"
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedProcess {
    pub pid: i32,
    pub bundle_id: String,
    pub process_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RawWindow {
    pub window_id: u32,
    pub pid: i32,
    pub owner_name: String,
    pub title: String,
    pub layer: i32,
    pub on_screen: bool,
    pub alpha: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct MacSnapshot {
    pub processes: Vec<ObservedProcess>,
    pub windows: Vec<RawWindow>,
    pub trusted: bool,
    pub window_note: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedDesktop {
    pub app_id: String,
    pub name: String,
    pub bundle_id: String,
    pub process_name: String,
    pub pid: i32,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListedWindow {
    pub window_id: u32,
    pub pid: i32,
    pub owner_name: String,
    pub title: String,
    pub bundle_id: Option<String>,
    pub catalog_app_id: Option<String>,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub address: String,
    pub running: bool,
    pub open_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopScan {
    pub interval_ms: u64,
    pub platform: String,
    pub trusted: bool,
    pub apps: Vec<DetectedDesktop>,
    pub services: Vec<ServiceStatus>,
    pub windows: Vec<ListedWindow>,
    pub window_note: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum A11yChoice {
    Pending,
    Granted,
    WebOnly,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct A11yStatus {
    pub trusted: bool,
    pub choice: A11yChoice,
    pub platform: String,
    pub blocks_workbench: bool,
}

#[derive(Serialize, Deserialize)]
struct ChoiceFile {
    choice: A11yChoice,
}

pub fn detect_confirmed(observed: &[ObservedProcess]) -> Vec<DetectedDesktop> {
    let mut found = Vec::new();
    for target in CONFIRMED_DESKTOP {
        let mut matches: Vec<&ObservedProcess> = observed
            .iter()
            .filter(|process| process.pid > 0 && process.bundle_id == target.bundle_id)
            .collect();
        if matches.is_empty() {
            continue;
        }
        matches.sort_by_key(|process| {
            let named = if target
                .process_names
                .iter()
                .any(|name| *name == process.process_name)
            {
                0
            } else {
                1
            };
            (named, process.pid)
        });
        let chosen = matches[0];
        found.push(DetectedDesktop {
            app_id: target.id.to_string(),
            name: target.name.to_string(),
            bundle_id: target.bundle_id.to_string(),
            process_name: chosen.process_name.clone(),
            pid: chosen.pid,
        });
    }
    found
}

pub fn window_label(owner: &str, title: &str, window_id: u32) -> String {
    let owner = owner.trim();
    let title = title.trim();
    if owner.is_empty() && title.is_empty() {
        format!("窗口 {window_id}")
    } else if title.is_empty() || title == owner {
        owner.to_string()
    } else if owner.is_empty() {
        title.to_string()
    } else {
        format!("{owner} · {title}")
    }
}

pub fn selectable_windows(
    raw: &[RawWindow],
    processes: &[ObservedProcess],
    own_pid: i32,
) -> (Vec<ListedWindow>, bool) {
    let bundle_by_pid: HashMap<i32, String> = processes
        .iter()
        .filter(|process| !process.bundle_id.is_empty())
        .map(|process| (process.pid, process.bundle_id.clone()))
        .collect();
    let mut listed = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for window in raw {
        if !seen.insert(window.window_id) || window.window_id == 0 || window.pid <= 0 {
            continue;
        }
        if window.pid == own_pid || !window.on_screen || window.layer != 0 || window.alpha < 0.05 {
            continue;
        }
        if window.width >= 0.0
            && window.height >= 0.0
            && (window.width < 40.0 || window.height < 40.0)
        {
            continue;
        }
        let owner = window.owner_name.trim();
        if owner.is_empty() || owner.starts_with("UniWorkspace") {
            continue;
        }
        let bundle = bundle_by_pid.get(&window.pid).cloned().unwrap_or_default();
        if bundle == BUNDLE_ID || bundle.starts_with(&format!("{BUNDLE_ID}.")) {
            continue;
        }
        let bundle_id = if bundle.is_empty() {
            None
        } else {
            Some(bundle)
        };
        let catalog_app_id = bundle_id
            .as_deref()
            .and_then(catalog_id_for_bundle)
            .map(str::to_string);
        listed.push(ListedWindow {
            window_id: window.window_id,
            pid: window.pid,
            owner_name: owner.to_string(),
            title: window.title.trim().to_string(),
            label: window_label(owner, &window.title, window.window_id),
            bundle_id,
            catalog_app_id,
        });
    }
    listed.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then(left.window_id.cmp(&right.window_id))
    });
    let truncated = listed.len() > MAX_LISTED_WINDOWS;
    listed.truncate(MAX_LISTED_WINDOWS);
    (listed, truncated)
}

fn catalog_id_for_bundle(bundle_id: &str) -> Option<&'static str> {
    CONFIRMED_DESKTOP
        .iter()
        .find(|app| app.bundle_id == bundle_id)
        .map(|app| app.id)
}

pub fn tcp_open(port: u16, timeout: Duration) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&address, timeout).is_ok()
}

pub fn open_ports(ports: &[u16], timeout: Duration) -> Vec<u16> {
    let mut found = Vec::new();
    std::thread::scope(|scope| {
        let mut guards = Vec::with_capacity(ports.len());
        for port in ports {
            let port = *port;
            guards.push(scope.spawn(move || tcp_open(port, timeout).then_some(port)));
        }
        for guard in guards {
            if let Ok(Some(port)) = guard.join() {
                found.push(port);
            }
        }
    });
    found
}

pub fn service_status(open: &[u16]) -> Vec<ServiceStatus> {
    LOCAL_SERVICES
        .iter()
        .map(|service| ServiceStatus {
            id: service.id.to_string(),
            name: service.name.to_string(),
            port: service.port,
            address: format!("127.0.0.1:{}", service.port),
            running: open.contains(&service.port),
            open_url: if service.id == "comfyui" {
                service.open_url.map(str::to_string)
            } else {
                None
            },
        })
        .collect()
}

pub fn parse_choice(text: &str) -> A11yChoice {
    serde_json::from_str::<ChoiceFile>(text)
        .map(|file| file.choice)
        .unwrap_or(A11yChoice::Pending)
}

pub fn load_choice(dir: &Path) -> A11yChoice {
    let Ok(text) = std::fs::read_to_string(dir.join(A11Y_FILE)) else {
        return A11yChoice::Pending;
    };
    parse_choice(&text)
}

pub fn store_choice(dir: &Path, choice: A11yChoice) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("无法创建应用数据目录: {err}"))?;
    let body = serde_json::to_string_pretty(&ChoiceFile { choice })
        .map_err(|err| format!("无法保存辅助功能选择: {err}"))?;
    std::fs::write(dir.join(A11Y_FILE), body)
        .map_err(|err| format!("无法保存辅助功能选择: {err}"))?;
    Ok(())
}

pub fn apply_choice(choice: A11yChoice, trusted: bool) -> Result<A11yChoice, String> {
    match choice {
        A11yChoice::WebOnly => Ok(A11yChoice::WebOnly),
        A11yChoice::Pending => Ok(A11yChoice::Pending),
        A11yChoice::Granted if trusted => Ok(A11yChoice::Granted),
        A11yChoice::Granted => Err("系统里还没有打开辅助功能。网页工作台可以继续使用。".into()),
    }
}

/// Refusal, delay, and a missing permission all leave the web workbench usable.
pub fn workbench_blocked(_choice: A11yChoice, _trusted: bool) -> bool {
    false
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

fn support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))
}

fn choice_of(app: &AppHandle) -> A11yChoice {
    support_dir(app)
        .map(|dir| load_choice(&dir))
        .unwrap_or(A11yChoice::Pending)
}

fn current_trusted(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::window_mac::trusted(app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

fn status_now(app: &AppHandle, trusted: bool) -> A11yStatus {
    let choice = choice_of(app);
    A11yStatus {
        trusted,
        choice,
        platform: platform_name().to_string(),
        blocks_workbench: workbench_blocked(choice, trusted),
    }
}

static SCAN_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        SCAN_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn scan_once(app: &AppHandle) -> Result<DesktopScan, String> {
    let ports: Vec<u16> = LOCAL_SERVICES.iter().map(|service| service.port).collect();
    let services = service_status(&open_ports(&ports, Duration::from_millis(PORT_TIMEOUT_MS)));
    #[cfg(target_os = "macos")]
    {
        let snapshot = crate::window_mac::observe(app)?;
        let own_pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
        let (windows, truncated) =
            selectable_windows(&snapshot.windows, &snapshot.processes, own_pid);
        let mut window_note = snapshot.window_note;
        if truncated {
            let extra = format!("可见窗口较多，侧栏只列出前 {MAX_LISTED_WINDOWS} 个。");
            window_note = Some(match window_note {
                Some(existing) => format!("{existing}{extra}"),
                None => extra,
            });
        }
        return Ok(DesktopScan {
            interval_ms: SCAN_INTERVAL_MS,
            platform: platform_name().to_string(),
            trusted: snapshot.trusted,
            apps: detect_confirmed(&snapshot.processes),
            services,
            windows,
            window_note,
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(DesktopScan {
            interval_ms: SCAN_INTERVAL_MS,
            platform: platform_name().to_string(),
            trusted: false,
            apps: Vec::new(),
            services,
            windows: Vec::new(),
            window_note: Some("桌面窗口只在 macOS 上列出".into()),
        })
    }
}

#[tauri::command]
pub fn desktop_scan(app: AppHandle) -> Result<DesktopScan, String> {
    if SCAN_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("上一次检测还在进行".into());
    }
    let _guard = BusyGuard;
    scan_once(&app)
}

#[tauri::command]
pub fn a11y_status(app: AppHandle) -> A11yStatus {
    status_now(&app, current_trusted(&app))
}

#[tauri::command]
pub fn a11y_request(app: AppHandle) -> Result<A11yStatus, String> {
    #[cfg(target_os = "macos")]
    let trusted = crate::window_mac::prompt(&app)?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &app;
        return Err("辅助功能授权只在 macOS 上发起".into());
    }
    #[cfg(target_os = "macos")]
    {
        Ok(status_now(&app, trusted))
    }
}

#[tauri::command]
pub fn a11y_open_settings(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::window_mac::open_settings(&app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("系统设置只在 macOS 上打开".into())
    }
}

#[tauri::command]
pub fn a11y_choose(app: AppHandle, choice: A11yChoice) -> Result<A11yStatus, String> {
    let trusted = current_trusted(&app);
    let choice = apply_choice(choice, trusted)?;
    store_choice(&support_dir(&app)?, choice)?;
    Ok(status_now(&app, trusted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn process(pid: i32, bundle_id: &str, process_name: &str) -> ObservedProcess {
        ObservedProcess {
            pid,
            bundle_id: bundle_id.to_string(),
            process_name: process_name.to_string(),
        }
    }

    fn window(id: u32, pid: i32, owner: &str) -> RawWindow {
        RawWindow {
            window_id: id,
            pid,
            owner_name: owner.to_string(),
            title: String::new(),
            layer: 0,
            on_screen: true,
            alpha: 1.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn reports_detection_without_claiming_snap() {
        let text = status();
        assert!(text.contains("bundle-id"));
        assert!(!text.contains("snap"));
    }

    #[test]
    fn scan_interval_stays_inside_the_two_second_deadline() {
        assert!(SCAN_INTERVAL_MS + PORT_TIMEOUT_MS * 2 < 2_000);
        assert_eq!(SCAN_INTERVAL_MS, 1_000);
    }

    #[test]
    fn catalog_detection_uses_only_confirmed_bundle_ids() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/apps.json")).unwrap();
        let apps = catalog["apps"].as_array().unwrap();
        let mut confirmed = Vec::new();
        for app in apps {
            let id = app["id"].as_str().unwrap();
            if matches!(id, "accio-work" | "cursor" | "harness") {
                assert!(app["webUrl"].is_null(), "{id}");
                assert_eq!(app["webStatus"], "pending");
            }
            if app["kind"] != "native" {
                continue;
            }
            if app["bundleIdStatus"] == "confirmed" {
                let bundle = app["bundleId"].as_str().expect(id);
                confirmed.push(id.to_string());
                let target = CONFIRMED_DESKTOP
                    .iter()
                    .find(|item| item.id == id)
                    .unwrap_or_else(|| panic!("missing scan target {id}"));
                assert_eq!(target.bundle_id, bundle);
                assert_eq!(target.name, app["name"].as_str().unwrap());
                let names: Vec<_> = app["processNames"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert_eq!(names, target.process_names);
            } else {
                assert!(app["bundleId"].is_null(), "{id}");
                assert!(CONFIRMED_DESKTOP.iter().all(|item| item.id != id));
            }
        }
        assert_eq!(confirmed.len(), CONFIRMED_DESKTOP.len());
        assert_eq!(confirmed.len(), 7);
        for bundle in [
            "com.tencent.workbuddy.mac",
            "com.workbuddy.workbuddy",
            "com.workbuddy.workbuddy-ai",
            "com.deepseek.dsh",
            "com.openai.chat",
        ] {
            assert!(CONFIRMED_DESKTOP
                .iter()
                .all(|item| item.bundle_id != bundle));
        }
    }

    #[test]
    fn bundle_id_matches_and_process_name_alone_does_not() {
        let observed = vec![
            process(40, "com.openai.codex", "ChatGPT"),
            process(41, "com.openai.chat", "ChatGPT"),
            process(42, "com.openai.Codex", "ChatGPT"),
            process(7, "", "WorkBuddy"),
            process(8, "com.tencent.workbuddy.mac", "WorkBuddy"),
            process(9, "com.deepseek.dsh", "DeepSeek Harness"),
            process(10, "", "Cursor"),
            process(11, "com.todesktop.230313mzl4w4u92", "Cursor Helper"),
            process(3, "com.anthropic.claudefordesktop", "Other"),
            process(12, "com.anthropic.claudefordesktop", "Claude"),
        ];
        let found = detect_confirmed(&observed);
        let ids: Vec<_> = found.iter().map(|app| app.app_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["chatgpt-desktop", "claude-desktop", "cursor-desktop"]
        );
        assert_eq!(found[0].pid, 40);
        assert_eq!(found[1].pid, 12, "prefer the catalog process name");
        assert_eq!(found[1].process_name, "Claude");
        assert_eq!(found[2].process_name, "Cursor Helper");
        assert!(found.iter().all(|app| app.bundle_id.contains('.')));
        assert!(detect_confirmed(&[]).is_empty());
    }

    #[test]
    fn selectable_windows_keep_foreign_windows_and_drop_our_own() {
        let raw = vec![
            window(1, 10, "Finder"),
            window(1, 10, "Finder"),
            window(2, 4, "UniWorkspace AI"),
            window(3, 99, "TextEdit"),
            {
                let mut hidden = window(4, 10, "Finder");
                hidden.on_screen = false;
                hidden
            },
            {
                let mut menu = window(5, 10, "Finder");
                menu.layer = 25;
                menu
            },
            {
                let mut tiny = window(6, 10, "Finder");
                tiny.width = 12.0;
                tiny
            },
            {
                let mut faded = window(7, 10, "Finder");
                faded.alpha = 0.0;
                faded
            },
        ];
        let processes = vec![
            process(10, "com.apple.finder", "Finder"),
            process(4, BUNDLE_ID, "UniWorkspace AI"),
            process(99, "", "TextEdit"),
        ];
        let (listed, truncated) = selectable_windows(&raw, &processes, 4);
        assert!(!truncated);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].owner_name, "Finder");
        assert!(listed[0].catalog_app_id.is_none());
        assert_eq!(listed[0].bundle_id.as_deref(), Some("com.apple.finder"));
        assert_eq!(listed[1].owner_name, "TextEdit");
        assert!(listed[1].bundle_id.is_none());
        assert!(listed.iter().all(|item| item.pid != 4));
    }

    #[test]
    fn a_confirmed_window_is_labeled_but_not_given_a_new_bundle_id() {
        let raw = vec![{
            let mut item = window(8, 20, "Cursor");
            item.title = "main.rs".into();
            item
        }];
        let processes = vec![process(20, "com.todesktop.230313mzl4w4u92", "Cursor")];
        let (listed, _) = selectable_windows(&raw, &processes, 1);
        assert_eq!(listed[0].catalog_app_id.as_deref(), Some("cursor-desktop"));
        assert_eq!(listed[0].label, "Cursor · main.rs");
        assert_eq!(window_label("Notes", "", 3), "Notes");
    }

    #[test]
    fn local_services_are_status_only_and_comfyui_is_the_root_webview() {
        let statuses = service_status(&[8188]);
        assert_eq!(statuses.len(), 3);
        assert_eq!(
            statuses
                .iter()
                .map(|service| service.port)
                .collect::<Vec<_>>(),
            vec![8188, 11434, 1234]
        );
        assert!(statuses
            .iter()
            .all(|service| service.address.starts_with("127.0.0.1:")));
        assert!(statuses
            .iter()
            .all(|service| !service.address.contains("localhost")));
        assert!(statuses[0].running);
        assert!(!statuses[1].running);
        assert!(!statuses[2].running);
        assert_eq!(statuses[0].open_url.as_deref(), Some(COMFYUI_OPEN_URL));
        assert!(statuses[1].open_url.is_none());
        assert!(statuses[2].open_url.is_none());
        let parsed = url::Url::parse(COMFYUI_OPEN_URL).unwrap();
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(8188));
        assert_eq!(parsed.path(), "/");
        assert!(parsed.query().is_none());
        assert_eq!(
            crate::partition::normalize_http_url(COMFYUI_OPEN_URL).unwrap(),
            COMFYUI_OPEN_URL
        );
        let none = service_status(&[]);
        assert!(none.iter().all(|service| !service.running));
    }

    #[test]
    fn tcp_probe_sees_a_listener_and_a_closed_port() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_open(port, Duration::from_millis(200)));
        drop(listener);
        assert!(!tcp_open(port, Duration::from_millis(200)));
    }

    #[test]
    fn refusal_does_not_block_the_web_workbench() {
        for trusted in [false, true] {
            for choice in [
                A11yChoice::Pending,
                A11yChoice::Granted,
                A11yChoice::WebOnly,
            ] {
                assert!(!workbench_blocked(choice, trusted));
            }
        }
        assert_eq!(
            apply_choice(A11yChoice::WebOnly, false).unwrap(),
            A11yChoice::WebOnly
        );
        assert!(apply_choice(A11yChoice::Granted, false)
            .unwrap_err()
            .contains("网页工作台"));
        assert_eq!(
            apply_choice(A11yChoice::Granted, true).unwrap(),
            A11yChoice::Granted
        );
        let site = &crate::partition::OFFICIAL_SITES[0];
        assert!(crate::partition::resolve_embed(site.id, site.url, &[]).is_ok());
    }

    #[test]
    fn choice_file_roundtrips_and_a_broken_file_stays_pending() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("uniworkspace-a11y-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_choice(&dir), A11yChoice::Pending);
        store_choice(&dir, A11yChoice::WebOnly).unwrap();
        assert_eq!(load_choice(&dir), A11yChoice::WebOnly);
        let text = std::fs::read_to_string(dir.join(A11Y_FILE)).unwrap();
        assert!(text.contains("webOnly"));
        assert!(!text.contains("bundleId"));
        std::fs::write(dir.join(A11Y_FILE), "{").unwrap();
        assert_eq!(load_choice(&dir), A11yChoice::Pending);
        assert_eq!(parse_choice(r#"{"choice":"granted"}"#), A11yChoice::Granted);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
