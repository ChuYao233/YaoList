use serde::{Deserialize, Serialize};

/// Site settings update request / 站点设置更新请求
#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateSettingsRequest {
    pub site_title: Option<String>,
    pub site_description: Option<String>,
    pub site_icon: Option<String>,
    pub allow_registration: Option<bool>,
    pub default_user_group: Option<String>,
    pub site_announcement: Option<String>,
    pub robots_txt: Option<String>,
    pub preview_encrypted_audio: Option<bool>,
    pub background_image: Option<String>,
    pub glass_effect: Option<bool>,
    pub glass_blur: Option<i32>,
    pub glass_opacity: Option<i32>,
    /// Proxy max speed in bytes/sec, 0 or null means unlimited
    pub proxy_max_speed: Option<i64>,
    /// Proxy max concurrent connections, 0 or null means unlimited
    pub proxy_max_concurrent: Option<i32>,
    /// Download domain for direct links/downloads/shares
    pub download_domain: Option<String>,
    /// Download link expiry in minutes (default 15)
    pub link_expiry_minutes: Option<i32>,
}

/// GeoIP配置请求
#[derive(Debug, Deserialize)]
pub struct GeoIpConfigRequest {
    pub enabled: bool,
    pub url: String,
    pub update_interval: String,
}

/// GeoIP数据库状态
#[derive(Debug, Serialize)]
pub struct GeoIpStatus {
    pub loaded: bool,
    pub country_db: bool,
    pub city_db: bool,
    pub asn_db: bool,
    pub data_dir: String,
}
