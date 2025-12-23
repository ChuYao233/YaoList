use serde::Serialize;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use chrono::Utc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use super::types::*;

/// GET /api/settings/public - 获取公开站点设置
pub async fn get_public_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    let site_title: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'site_title'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let site_description: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'site_description'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let allow_registration: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'allow_registration'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let site_announcement: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'site_announcement'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let site_icon: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'site_icon'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let robots_txt: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'robots_txt'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let preview_encrypted_audio: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'preview_encrypted_audio'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let background_image: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'background_image'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let glass_effect: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'glass_effect'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let glass_blur: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'glass_blur'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let glass_opacity: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'glass_opacity'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let default_user_group: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'default_user_group'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Proxy and download settings / 代理和下载设置
    let proxy_max_speed: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'proxy_max_speed'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let proxy_max_concurrent: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'proxy_max_concurrent'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let download_domain: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'download_domain'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let link_expiry_minutes: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'link_expiry_minutes'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(json!({
        "site_title": site_title.map(|(v,)| v).unwrap_or_else(|| "YaoList".to_string()),
        "site_description": site_description.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        "site_icon": site_icon.map(|(v,)| v).unwrap_or_else(|| "/favicon.ico".to_string()),
        "allow_registration": allow_registration.map(|(v,)| v == "true").unwrap_or(false),
        "default_user_group": default_user_group.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        "site_announcement": site_announcement.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        "robots_txt": robots_txt.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        "preview_encrypted_audio": preview_encrypted_audio.map(|(v,)| v == "true").unwrap_or(false),
        "background_image": background_image.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        "glass_effect": glass_effect.map(|(v,)| v == "true").unwrap_or(false),
        "glass_blur": glass_blur.map(|(v,)| v.parse::<i32>().unwrap_or(12)).unwrap_or(12),
        "glass_opacity": glass_opacity.map(|(v,)| v.parse::<i32>().unwrap_or(80)).unwrap_or(80),
        // Proxy settings / 代理设置 (0 = unlimited / 0表示无限制)
        "proxy_max_speed": proxy_max_speed.map(|(v,)| v.parse::<i64>().unwrap_or(0)).unwrap_or(0),
        "proxy_max_concurrent": proxy_max_concurrent.map(|(v,)| v.parse::<i32>().unwrap_or(0)).unwrap_or(0),
        // Download domain / 下载域名 (empty = use current / 空表示使用当前域名)
        "download_domain": download_domain.map(|(v,)| v).unwrap_or_else(|| "".to_string()),
        // Link expiry / 链接有效期
        "link_expiry_minutes": link_expiry_minutes.map(|(v,)| v.parse::<i32>().unwrap_or(15)).unwrap_or(15)
    })))
}

/// POST /api/settings - 更新站点设置（需要管理员权限）
pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 验证管理员权限
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let is_admin: Option<bool> = sqlx::query_scalar(
        "SELECT u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    if !is_admin.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "需要管理员权限"}))));
    }
    
    let now = Utc::now().to_rfc3339();
    
    if let Some(site_title) = req.site_title {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("site_title")
        .bind(&site_title)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(site_description) = req.site_description {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("site_description")
        .bind(&site_description)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(site_icon) = req.site_icon {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("site_icon")
        .bind(&site_icon)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(allow_registration) = req.allow_registration {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("allow_registration")
        .bind(if allow_registration { "true" } else { "false" })
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(default_user_group) = req.default_user_group {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("default_user_group")
        .bind(&default_user_group)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(site_announcement) = req.site_announcement {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("site_announcement")
        .bind(&site_announcement)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(robots_txt) = req.robots_txt {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("robots_txt")
        .bind(&robots_txt)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(preview_encrypted_audio) = req.preview_encrypted_audio {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("preview_encrypted_audio")
        .bind(if preview_encrypted_audio { "true" } else { "false" })
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(background_image) = req.background_image {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("background_image")
        .bind(&background_image)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(glass_effect) = req.glass_effect {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("glass_effect")
        .bind(if glass_effect { "true" } else { "false" })
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(glass_blur) = req.glass_blur {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("glass_blur")
        .bind(glass_blur.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(glass_opacity) = req.glass_opacity {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("glass_opacity")
        .bind(glass_opacity.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    // Proxy max speed / 代理最大速度
    if let Some(proxy_max_speed) = req.proxy_max_speed {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("proxy_max_speed")
        .bind(proxy_max_speed.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    // Proxy max concurrent / 代理最大并发数
    if let Some(proxy_max_concurrent) = req.proxy_max_concurrent {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("proxy_max_concurrent")
        .bind(proxy_max_concurrent.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    // Download domain / 下载域名
    if let Some(ref download_domain) = req.download_domain {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("download_domain")
        .bind(download_domain)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    // Link expiry minutes / 链接有效期
    if let Some(link_expiry_minutes) = req.link_expiry_minutes {
        sqlx::query(
            "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
        )
        .bind("link_expiry_minutes")
        .bind(link_expiry_minutes.to_string())
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    // Update download settings cache / 更新下载设置缓存
    if req.proxy_max_speed.is_some() || req.proxy_max_concurrent.is_some() || req.download_domain.is_some() 
        || req.link_expiry_minutes.is_some() {
        if let Some(speed) = req.proxy_max_speed {
            state.download_settings.set_max_speed(speed);
        }
        if let Some(concurrent) = req.proxy_max_concurrent {
            state.download_settings.set_max_concurrent(concurrent);
        }
        if let Some(domain) = req.download_domain {
            state.download_settings.set_download_domain(domain);
        }
        if let Some(expiry) = req.link_expiry_minutes {
            state.download_settings.set_link_expiry_minutes(expiry);
        }
        tracing::info!("Download settings cache updated: expiry={}min", 
            state.download_settings.get_link_expiry_minutes());
    }
    
    Ok(Json(json!({
        "code": 200,
        "message": "success"
    })))
}

/// GeoIP数据库状态
#[derive(Serialize)]
pub struct GeoIpStatus {
    pub loaded: bool,
    pub country_db: bool,
    pub city_db: bool,
    pub asn_db: bool,
    pub data_dir: String,
}
