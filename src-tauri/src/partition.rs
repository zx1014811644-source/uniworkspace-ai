//! Stable per-app partition ids and the isolation verdict.
//!
//! Website data itself lives in a persistent WKWebsiteDataStore. This module
//! only decides which store, and whether a probe proved the stores are separate.

use serde::Serialize;
use uuid::Uuid;

pub const BUNDLE_ID: &str = "com.uniworkspace.ai";

pub struct Week2Site {
    pub id: &'static str,
    pub url: &'static str,
}

pub const WEEK2_SITES: &[Week2Site] = &[
    Week2Site {
        id: "chatgpt",
        url: "https://chatgpt.com/",
    },
    Week2Site {
        id: "claude",
        url: "https://claude.ai/",
    },
    Week2Site {
        id: "gemini",
        url: "https://gemini.google.com/",
    },
    Week2Site {
        id: "kimi",
        url: "https://www.kimi.com/",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionStoreKind {
    /// `WKWebsiteDataStore.dataStoreForIdentifier`, macOS 14+.
    Identifier,
    /// Persistent directories on `_WKWebsiteDataStoreConfiguration`, macOS 13.
    Configuration,
}

pub fn site_by_id(app_id: &str) -> Option<&'static Week2Site> {
    WEEK2_SITES.iter().find(|site| site.id == app_id)
}

pub fn require_week2_site(app_id: &str, url: &str) -> Result<&'static Week2Site, String> {
    let Some(site) = site_by_id(app_id) else {
        return Err("这个应用本周还不会在内容区打开".into());
    };
    if url != site.url {
        return Err(format!("网址必须是目录里的 {}", site.url));
    }
    Ok(site)
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
    if samples.len() != WEEK2_SITES.len() {
        return Some(format!(
            "应有 {} 个分区样本，实际 {}",
            WEEK2_SITES.len(),
            samples.len()
        ));
    }
    for site in WEEK2_SITES {
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
    use std::collections::HashSet;

    #[test]
    fn partition_identifiers_are_stable_distinct_and_not_nil() {
        let expected = [
            ("chatgpt", "8bf5300b-868c-58d0-8193-87da08195fe8"),
            ("claude", "a5f67569-b0cb-5727-b6cd-7411e6f94697"),
            ("gemini", "30ed5c01-da80-56eb-9442-09f0fd2b086c"),
            ("kimi", "38f3ff38-1357-5fe3-9663-57b408ac24e3"),
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
    fn week2_allowlist_matches_the_catalog_and_stops_there() {
        let catalog = include_str!("../../catalog/apps.json");
        let config = include_str!("../tauri.conf.json");
        assert!(config.contains(&format!("\"identifier\": \"{BUNDLE_ID}\"")));
        for site in WEEK2_SITES {
            assert!(catalog.contains(&format!("\"id\": \"{}\"", site.id)));
            assert!(
                catalog.contains(&format!("\"webUrl\": \"{}\"", site.url)),
                "{} url drifted",
                site.id
            );
            assert_eq!(require_week2_site(site.id, site.url).unwrap().url, site.url);
            assert!(require_week2_site(site.id, "https://example.com/").is_err());
        }
        for app_id in [
            "doubao",
            "workbuddy",
            "accio-work",
            "cursor",
            "deepseek",
            "harness",
        ] {
            assert!(
                site_by_id(app_id).is_none(),
                "{app_id} is not a Week 2 embed"
            );
        }
    }

    #[test]
    fn isolation_verdict_accepts_four_private_markers() {
        let samples: Vec<_> = WEEK2_SITES
            .iter()
            .map(|site| sample(site.id, site.id, site.id))
            .collect();
        assert_eq!(isolation_failure(&samples), None);
    }

    #[test]
    fn isolation_verdict_rejects_shared_local_storage_and_cookies() {
        let leaked: Vec<_> = WEEK2_SITES
            .iter()
            .map(|site| sample(site.id, "kimi", "kimi"))
            .collect();
        let message = isolation_failure(&leaked).expect("shared store must fail");
        assert!(message.contains("chatgpt"));
        assert!(message.contains("localStorage"));

        let mut cookie_leak: Vec<_> = WEEK2_SITES
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
pub fn show_partition(
    app: tauri::AppHandle,
    app_id: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::show_partition(app, app_id, url, x, y, width, height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_id, url, x, y, width, height);
        Err("分区 WebView 只在 macOS 上打开".into())
    }
}

#[tauri::command]
pub fn set_partition_bounds(
    app: tauri::AppHandle,
    app_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::partition_mac::set_partition_bounds(app, app_id, x, y, width, height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_id, x, y, width, height);
        Ok(())
    }
}

#[tauri::command]
pub fn hide_partitions(app: tauri::AppHandle) -> Result<(), String> {
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
pub async fn probe_partitions(app: tauri::AppHandle) -> Result<ProbeReport, String> {
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
