//! Partition ids, the local workspace file, and the crash-rebuild rule.
//!
//! Website data still lives in a persistent WKWebsiteDataStore, one per app.
//! This module decides which store is allowed, which panes the shell may open,
//! and which single pane is rebuilt when a web content process dies.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use url::Url;
use uuid::Uuid;

pub const BUNDLE_ID: &str = "com.uniworkspace.ai";
pub const WORKSPACE_VERSION: u32 = 1;
pub const WORKSPACE_FILE: &str = "workspace.json";
const MAX_CUSTOM_APPS: usize = 40;
const MAX_URL_LEN: usize = 2048;

pub struct OfficialSite {
    pub id: &'static str,
    pub url: &'static str,
}

pub const OFFICIAL_SITES: &[OfficialSite] = &[
    OfficialSite {
        id: "chatgpt",
        url: "https://chatgpt.com/",
    },
    OfficialSite {
        id: "claude",
        url: "https://claude.ai/",
    },
    OfficialSite {
        id: "gemini",
        url: "https://gemini.google.com/",
    },
    OfficialSite {
        id: "kimi",
        url: "https://www.kimi.com/",
    },
    OfficialSite {
        id: "doubao",
        url: "https://www.doubao.com/chat/",
    },
    OfficialSite {
        id: "workbuddy",
        url: "https://www.workbuddy.cn/app",
    },
    OfficialSite {
        id: "deepseek",
        url: "https://chat.deepseek.com/",
    },
];

pub const PENDING_WEB_IDS: &[&str] = &["accio-work", "cursor", "harness"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionStoreKind {
    /// `WKWebsiteDataStore.dataStoreForIdentifier`, macOS 14+.
    Identifier,
    /// Persistent directories on `_WKWebsiteDataStoreConfiguration`, macOS 13.
    Configuration,
}

pub fn official_site(app_id: &str) -> Option<&'static OfficialSite> {
    OFFICIAL_SITES.iter().find(|site| site.id == app_id)
}

pub fn store_kind_for_major(major: u32) -> PartitionStoreKind {
    if major >= 14 {
        PartitionStoreKind::Identifier
    } else {
        PartitionStoreKind::Configuration
    }
}

pub fn backend_name(kind: PartitionStoreKind) -> &'static str {
    match kind {
        PartitionStoreKind::Identifier => "WKWebsiteDataStore.dataStoreForIdentifier",
        PartitionStoreKind::Configuration => "WKWebsiteDataStoreConfiguration",
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn backend_detail(kind: PartitionStoreKind) -> &'static str {
    match kind {
        PartitionStoreKind::Identifier => {
            "macOS 14 及以上的持久命名 WKWebsiteDataStore，不是内存仓库"
        }
        PartitionStoreKind::Configuration => {
            "macOS 13 的持久命名 WKWebsiteDataStore，按分区目录保存，不是内存仓库"
        }
    }
}

/// UUID v5 of `https://uniworkspace.ai/partition/<app id>`.
/// The bytes stay the same across launches so the WebKit store can be found again.
pub fn partition_identifier(app_id: &str) -> [u8; 16] {
    let name = format!("https://uniworkspace.ai/partition/{app_id}");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).into_bytes()
}

pub fn identifier_string(app_id: &str) -> String {
    Uuid::from_bytes(partition_identifier(app_id)).to_string()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn storage_hint(kind: PartitionStoreKind, app_id: &str) -> String {
    match kind {
        PartitionStoreKind::Identifier => format!(
            "~/Library/WebKit/{BUNDLE_ID}/WebsiteDataStore/{}",
            identifier_string(app_id)
        ),
        PartitionStoreKind::Configuration => {
            format!("~/Library/Application Support/{BUNDLE_ID}/partitions/{app_id}")
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn site_label(app_id: &str) -> String {
    format!("partition-{app_id}")
}

pub fn visible_count(layout: &str) -> Option<usize> {
    match layout {
        "single" => Some(1),
        "split" => Some(2),
        "grid" => Some(4),
        _ => None,
    }
}

pub fn is_custom_id(app_id: &str) -> bool {
    let Some(rest) = app_id.strip_prefix("custom-") else {
        return false;
    };
    Uuid::parse_str(rest).is_ok()
}

pub fn normalize_http_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_URL_LEN {
        return Err("链接为空或过长".into());
    }
    let url = Url::parse(trimmed).map_err(|_| "链接格式不对".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("只接受 http 或 https 链接".into());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("链接缺少主机名".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("链接里不要带账号密码".into());
    }
    Ok(url.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomAppRecord {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub version: u32,
    pub layout: String,
    pub slots: Vec<Option<String>>,
    pub focused_pane: usize,
    pub custom_apps: Vec<CustomAppRecord>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct PaneSpec {
    pub app_id: String,
    pub url: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub focus: bool,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct OpenPane {
    pub app_id: String,
    pub url: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub focus: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneCrashPlan {
    pub rebuild: Vec<String>,
    pub leave_running: Vec<String>,
}

pub struct RebuildGate {
    limit: usize,
    window_ms: u128,
    stamps_ms: Vec<u128>,
}

impl RebuildGate {
    pub fn new(limit: usize, window_ms: u128) -> Self {
        Self {
            limit,
            window_ms,
            stamps_ms: Vec::new(),
        }
    }

    pub fn allow(&mut self, now_ms: u128) -> bool {
        let window = self.window_ms;
        self.stamps_ms
            .retain(|stamp| now_ms.saturating_sub(*stamp) < window);
        if self.stamps_ms.len() >= self.limit {
            return false;
        }
        self.stamps_ms.push(now_ms);
        true
    }
}

pub fn default_workspace() -> WorkspaceFile {
    WorkspaceFile {
        version: WORKSPACE_VERSION,
        layout: "single".into(),
        slots: vec![None, None, None, None],
        focused_pane: 0,
        custom_apps: Vec::new(),
    }
}

pub fn normalize_workspace(mut file: WorkspaceFile) -> Result<WorkspaceFile, String> {
    if file.version != WORKSPACE_VERSION {
        return Err("布局配置版本无法识别".into());
    }
    let visible =
        visible_count(&file.layout).ok_or_else(|| "布局只能是单窗、1:1 或田字格".to_string())?;
    if file.slots.len() != 4 {
        return Err("布局需要记住 4 个格子".into());
    }
    if file.focused_pane >= visible {
        return Err("当前布局里没有这一格".into());
    }
    if file.custom_apps.len() > MAX_CUSTOM_APPS {
        return Err("自定义链接太多".into());
    }

    let mut custom_ids = HashSet::new();
    let mut custom_urls = HashSet::new();
    for app in &mut file.custom_apps {
        let id = app.id.trim().to_string();
        if !is_custom_id(&id) {
            return Err("自定义链接的分区 id 无效".into());
        }
        if !custom_ids.insert(id.clone()) {
            return Err("自定义链接的分区 id 重复".into());
        }
        let url = normalize_http_url(&app.url)?;
        if !custom_urls.insert(url.clone()) {
            return Err("这个自定义链接已经添加过".into());
        }
        let name = app.name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err("自定义链接的名称无效".into());
        }
        app.id = id;
        app.url = url;
        app.name = name.to_string();
    }

    let mut seen_slots = HashSet::new();
    for slot in &file.slots {
        let Some(app_id) = slot else {
            continue;
        };
        if !seen_slots.insert(app_id.clone()) {
            return Err("同一个网页不能放进两格".into());
        }
        if official_site(app_id).is_none() && !file.custom_apps.iter().any(|app| app.id == *app_id)
        {
            if PENDING_WEB_IDS.contains(&app_id.as_str()) {
                return Err("待确认的应用不能放进分屏".into());
            }
            return Err(format!("格子里的应用无法打开: {app_id}"));
        }
    }
    Ok(file)
}

pub fn load_workspace_text(text: &str) -> Result<WorkspaceFile, String> {
    let parsed: WorkspaceFile =
        serde_json::from_str(text).map_err(|err| format!("布局配置无法解析: {err}"))?;
    normalize_workspace(parsed)
}

pub fn resolve_embed(
    app_id: &str,
    url: &str,
    custom_apps: &[CustomAppRecord],
) -> Result<(String, String), String> {
    if let Some(site) = official_site(app_id) {
        if url != site.url {
            return Err(format!("网址必须是目录里的 {}", site.url));
        }
        return Ok((site.id.to_string(), site.url.to_string()));
    }
    if PENDING_WEB_IDS.contains(&app_id) {
        return Err("这个应用没有稳定的官方网页入口，不会打开".into());
    }
    if is_custom_id(app_id) {
        let Some(record) = custom_apps.iter().find(|app| app.id == app_id) else {
            return Err("自定义链接还没有写入本地配置".into());
        };
        if url != record.url {
            return Err("自定义链接和本地配置不一致".into());
        }
        return Ok((record.id.clone(), record.url.clone()));
    }
    Err("这个应用不会在网页分区里打开".into())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn authorize_panes(
    specs: &[PaneSpec],
    workspace: &WorkspaceFile,
) -> Result<Vec<OpenPane>, String> {
    if specs.len() > 4 {
        return Err("一次最多打开 4 个网页".into());
    }
    let mut opened = Vec::new();
    let mut seen = HashSet::new();
    for spec in specs {
        if !spec.width.is_finite()
            || !spec.height.is_finite()
            || !spec.x.is_finite()
            || !spec.y.is_finite()
        {
            return Err("分屏位置无效".into());
        }
        if spec.width < 2.0 || spec.height < 2.0 {
            continue;
        }
        let (app_id, url) = resolve_embed(&spec.app_id, &spec.url, &workspace.custom_apps)?;
        if !seen.insert(app_id.clone()) {
            return Err("同一个网页不能放进两格".into());
        }
        opened.push(OpenPane {
            app_id,
            url,
            x: spec.x,
            y: spec.y,
            width: spec.width,
            height: spec.height,
            focus: spec.focus,
        });
    }
    Ok(opened)
}

/// A crashed partition webview is the only pane that gets rebuilt.
/// The shell, login popups, and every other open pane stay as they are.
pub fn plan_pane_crash(crashed_label: &str, open_labels: &[String]) -> PaneCrashPlan {
    let mut rebuild = Vec::new();
    let mut leave_running = Vec::new();
    let crashed_is_pane = crashed_label.starts_with("partition-");
    for label in open_labels {
        if crashed_is_pane && label == crashed_label {
            rebuild.push(label.clone());
        } else {
            leave_running.push(label.clone());
        }
    }
    PaneCrashPlan {
        rebuild,
        leave_running,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSample {
    pub app_id: String,
    pub identifier: String,
    pub local_storage: String,
    pub cookie: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub isolated: bool,
    pub backend: String,
    pub detail: String,
    pub hints: Vec<String>,
    pub samples: Vec<ProbeSample>,
}

pub fn isolation_failure(samples: &[ProbeSample]) -> Option<String> {
    if samples.len() != OFFICIAL_SITES.len() {
        return Some(format!(
            "应有 {} 个分区样本，实际 {}",
            OFFICIAL_SITES.len(),
            samples.len()
        ));
    }
    for site in OFFICIAL_SITES {
        if !samples.iter().any(|sample| sample.app_id == site.id) {
            return Some(format!("缺少 {} 的样本", site.id));
        }
    }
    for sample in samples {
        if sample.local_storage != sample.app_id {
            return Some(format!(
                "{} 的 localStorage 是「{}」，分区之间串了数据",
                sample.app_id, sample.local_storage
            ));
        }
        if sample.cookie != sample.app_id {
            return Some(format!(
                "{} 的 cookie 是「{}」，分区之间串了数据",
                sample.app_id, sample.cookie
            ));
        }
    }
    None
}

pub fn parse_marker(raw: &str) -> Result<(String, String), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("探测页没有返回标记".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| format!("无法解析探测结果 {raw}: {err}"))?;
    let value = if let serde_json::Value::String(inner) = &value {
        serde_json::from_str(inner).map_err(|err| format!("无法解析探测结果 {inner}: {err}"))?
    } else {
        value
    };
    let local = value
        .get("local")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let cookie = value
        .get("cookie")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    Ok((local, cookie))
}

fn support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))?;
    std::fs::create_dir_all(&dir).map_err(|err| format!("无法创建应用数据目录: {err}"))?;
    Ok(dir)
}

pub fn workspace_path(dir: &Path) -> PathBuf {
    dir.join(WORKSPACE_FILE)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn read_workspace(app: &AppHandle) -> Result<WorkspaceFile, String> {
    let path = workspace_path(&support_dir(app)?);
    if !path.exists() {
        return Ok(default_workspace());
    }
    let text = std::fs::read_to_string(&path).map_err(|err| format!("无法读取布局配置: {err}"))?;
    match load_workspace_text(&text) {
        Ok(file) => Ok(file),
        Err(err) => {
            let backup = path.with_extension("json.bak");
            let _ = std::fs::rename(&path, backup);
            Err(err)
        }
    }
}

fn write_workspace(app: &AppHandle, file: &WorkspaceFile) -> Result<(), String> {
    let path = workspace_path(&support_dir(app)?);
    let body = serde_json::to_string_pretty(file).map_err(|err| err.to_string())?;
    std::fs::write(&path, body).map_err(|err| format!("无法保存布局配置: {err}"))?;
    Ok(())
}

fn sample(app_id: &str, local_storage: &str, cookie: &str) -> ProbeSample {
    ProbeSample {
        app_id: app_id.to_string(),
        identifier: identifier_string(app_id),
        local_storage: local_storage.to_string(),
        cookie: cookie.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_identifiers_are_stable_distinct_and_not_nil() {
        let expected = [
            ("chatgpt", "8bf5300b-868c-58d0-8193-87da08195fe8"),
            ("claude", "a5f67569-b0cb-5727-b6cd-7411e6f94697"),
            ("gemini", "30ed5c01-da80-56eb-9442-09f0fd2b086c"),
            ("kimi", "38f3ff38-1357-5fe3-9663-57b408ac24e3"),
            ("doubao", "490f5018-e8c7-5baa-87fc-a53db7141d59"),
            ("workbuddy", "f3e31651-703c-5193-99ff-9bec98f668dc"),
            ("deepseek", "7c863b01-27e3-5d60-b58d-76fc647b84cf"),
        ];
        let mut seen = HashSet::new();
        for (app_id, text) in expected {
            let bytes = partition_identifier(app_id);
            assert_ne!(bytes, [0; 16], "{app_id} must not use the nil UUID");
            assert_eq!(bytes[6] >> 4, 5, "{app_id} version nibble");
            assert_eq!(bytes[8] & 0b1100_0000, 0b1000_0000, "{app_id} variant");
            assert_eq!(identifier_string(app_id), text);
            assert_eq!(partition_identifier(app_id), bytes);
            assert!(seen.insert(bytes), "{app_id} identifier collided");
        }
        let custom = "custom-11111111-1111-4111-8111-111111111111";
        assert!(
            seen.insert(partition_identifier(custom)),
            "custom partition collided with a catalog app"
        );
    }

    #[test]
    fn macos_13_does_not_share_the_macos_14_code_path() {
        assert_eq!(store_kind_for_major(13), PartitionStoreKind::Configuration);
        assert_eq!(store_kind_for_major(14), PartitionStoreKind::Identifier);
        assert_eq!(store_kind_for_major(15), PartitionStoreKind::Identifier);
        assert!(!backend_name(PartitionStoreKind::Identifier)
            .to_ascii_lowercase()
            .contains("nonpersistent"));
        assert!(!backend_name(PartitionStoreKind::Configuration)
            .to_ascii_lowercase()
            .contains("memory"));
    }

    #[test]
    fn catalog_contract_keeps_confirmed_urls_and_pending_nulls() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../catalog/apps.json")).unwrap();
        let config = include_str!("../tauri.conf.json");
        assert!(config.contains(&format!("\"identifier\": \"{BUNDLE_ID}\"")));
        let apps = catalog["apps"].as_array().unwrap();
        let by_id = |id: &str| {
            apps.iter()
                .find(|app| app["id"] == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        };
        for site in OFFICIAL_SITES {
            let app = by_id(site.id);
            assert_eq!(app["kind"], "web");
            assert_eq!(app["webStatus"], "official");
            assert_eq!(app["webUrl"], site.url, "{}", site.id);
            assert_eq!(
                resolve_embed(site.id, site.url, &[]).unwrap(),
                (site.id.to_string(), site.url.to_string())
            );
            assert!(resolve_embed(site.id, "https://example.com/", &[]).is_err());
        }
        for id in PENDING_WEB_IDS {
            let app = by_id(id);
            assert_eq!(app["kind"], "web");
            assert_eq!(app["webStatus"], "pending");
            assert!(app["webUrl"].is_null(), "{id}");
            assert!(
                app["notes"]
                    .as_str()
                    .unwrap()
                    .contains("does not open a WebView"),
                "{id} notes"
            );
            assert!(resolve_embed(id, "https://example.com/", &[]).is_err());
        }
        assert!(by_id("workbuddy-desktop")["bundleId"].is_null());
        assert!(by_id("harness-desktop")["bundleId"].is_null());
        assert!(by_id("accio-work-desktop")["bundleId"].is_null());
        assert_eq!(by_id("cursor")["webStatus"], "pending");
        assert!(by_id("cursor")["webUrl"].is_null());
        assert_eq!(
            by_id("cursor-desktop")["bundleId"],
            "com.todesktop.230313mzl4w4u92"
        );
        assert_eq!(visible_count("single"), Some(1));
        assert_eq!(visible_count("split"), Some(2));
        assert_eq!(visible_count("grid"), Some(4));
    }

    #[test]
    fn workspace_accepts_a_grid_and_rejects_pending_or_bad_urls() {
        let mut file = default_workspace();
        file.layout = "grid".into();
        file.focused_pane = 3;
        file.slots = vec![
            Some("chatgpt".into()),
            Some("doubao".into()),
            Some("deepseek".into()),
            Some("workbuddy".into()),
        ];
        assert_eq!(normalize_workspace(file).unwrap().layout, "grid");

        let mut pending = default_workspace();
        pending.slots[0] = Some("cursor".into());
        assert!(normalize_workspace(pending).unwrap_err().contains("待确认"));

        let mut harness = default_workspace();
        harness.slots[0] = Some("harness".into());
        assert!(normalize_workspace(harness).is_err());

        let custom = CustomAppRecord {
            id: "custom-11111111-1111-4111-8111-111111111111".into(),
            name: "Example".into(),
            url: "https://example.com/path".into(),
        };
        let mut with_custom = default_workspace();
        with_custom.layout = "split".into();
        with_custom.slots[0] = Some(custom.id.clone());
        with_custom.slots[1] = Some("kimi".into());
        with_custom.custom_apps = vec![custom.clone()];
        let saved = normalize_workspace(with_custom).unwrap();
        assert_eq!(saved.custom_apps[0].url, "https://example.com/path");
        assert!(resolve_embed(&custom.id, &saved.custom_apps[0].url, &saved.custom_apps).is_ok());
        assert!(
            resolve_embed(&custom.id, "https://example.com/other", &saved.custom_apps).is_err()
        );

        assert!(normalize_http_url("javascript:alert(1)").is_err());
        assert!(normalize_http_url("file:///tmp/x").is_err());
        assert!(normalize_http_url("https://user:pw@example.com/").is_err());
        assert_eq!(
            normalize_http_url(" https://example.com ").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn one_crashed_pane_is_rebuilt_and_the_others_stay() {
        let open = vec![
            "partition-chatgpt".into(),
            "partition-doubao".into(),
            "partition-deepseek".into(),
            "partition-custom-11111111-1111-4111-8111-111111111111".into(),
        ];
        let plan = plan_pane_crash("partition-doubao", &open);
        assert_eq!(plan.rebuild, vec!["partition-doubao".to_string()]);
        assert_eq!(plan.leave_running.len(), 3);
        assert!(!plan
            .leave_running
            .iter()
            .any(|label| label == "partition-doubao"));

        let shell = plan_pane_crash("main", &open);
        assert!(shell.rebuild.is_empty());
        assert_eq!(shell.leave_running, open);

        let popup = plan_pane_crash("oauth-partition-chatgpt-1", &open);
        assert!(popup.rebuild.is_empty());
        assert_eq!(popup.leave_running, open);
    }

    #[test]
    fn rebuild_gate_stops_a_tight_loop() {
        let mut gate = RebuildGate::new(3, 10_000);
        assert!(gate.allow(1_000));
        assert!(gate.allow(1_100));
        assert!(gate.allow(1_200));
        assert!(!gate.allow(1_300));
        assert!(gate.allow(11_200));
    }

    #[test]
    fn isolation_verdict_accepts_private_markers_for_every_confirmed_site() {
        let samples: Vec<_> = OFFICIAL_SITES
            .iter()
            .map(|site| sample(site.id, site.id, site.id))
            .collect();
        assert_eq!(isolation_failure(&samples), None);
        assert!(isolation_failure(&samples[..4]).unwrap().contains("应有"));
    }

    #[test]
    fn isolation_verdict_rejects_shared_local_storage_and_cookies() {
        let leaked: Vec<_> = OFFICIAL_SITES
            .iter()
            .map(|site| sample(site.id, "kimi", "kimi"))
            .collect();
        let message = isolation_failure(&leaked).expect("shared store must fail");
        assert!(message.contains("chatgpt"));
        assert!(message.contains("localStorage"));

        let mut cookie_leak: Vec<_> = OFFICIAL_SITES
            .iter()
            .map(|site| sample(site.id, site.id, site.id))
            .collect();
        cookie_leak[1].cookie = "chatgpt".into();
        let message = isolation_failure(&cookie_leak).expect("cookie leak must fail");
        assert!(message.contains("cookie"));
        assert!(message.contains("claude"));
    }

    #[test]
    fn marker_parser_accepts_object_and_encoded_object() {
        let (local, cookie) = parse_marker(r#"{"local":"chatgpt","cookie":"chatgpt"}"#).unwrap();
        assert_eq!(local, "chatgpt");
        assert_eq!(cookie, "chatgpt");
        let encoded = serde_json::to_string(&serde_json::json!({
            "local": "claude",
            "cookie": "claude"
        }))
        .unwrap();
        let wrapped = serde_json::to_string(&encoded).unwrap();
        let (local, cookie) = parse_marker(&wrapped).unwrap();
        assert_eq!(local, "claude");
        assert_eq!(cookie, "claude");
        assert!(parse_marker("").is_err());
        assert!(parse_marker("null").is_ok());
    }
}

#[tauri::command]
pub fn load_workspace(app: AppHandle) -> Result<WorkspaceFile, String> {
    read_workspace(&app)
}

#[tauri::command]
pub fn save_workspace(app: AppHandle, workspace: WorkspaceFile) -> Result<WorkspaceFile, String> {
    let file = normalize_workspace(workspace)?;
    write_workspace(&app, &file)?;
    #[cfg(target_os = "macos")]
    crate::partition_mac::write_partition_index(&app);
    Ok(file)
}

#[tauri::command]
pub fn sync_panes(app: AppHandle, panes: Vec<PaneSpec>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::sync_panes(app, panes)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, panes);
        Err("分区 WebView 只在 macOS 上打开".into())
    }
}

#[tauri::command]
pub fn focus_partition(app: AppHandle, app_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::focus_partition(&app, &app_id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_id);
        Ok(())
    }
}

#[tauri::command]
pub fn hide_partitions(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::hide_partitions(&app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
pub async fn probe_partitions(app: AppHandle) -> Result<ProbeReport, String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::probe_partitions(app).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("隔离检查只在 macOS 上运行".into())
    }
}
