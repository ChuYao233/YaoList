use serde::Deserialize;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::path::PathBuf;
use chrono::Utc;
use tokio::io::AsyncWriteExt;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use yaolist_backend::geoip::get_geoip_manager;
use super::types::*;

/// GET /api/settings/geoip/status - 获取GeoIP数据库状态
pub async fn get_geoip_status(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
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
    let data_dir = std::env::current_dir()
        .map(|p| p.join("data"))
        .unwrap_or_else(|_| PathBuf::from("data"));
    
    let country_exists = data_dir.join("GeoLite2-Country.mmdb").exists();
    let city_exists = data_dir.join("GeoLite2-City.mmdb").exists();
    let asn_exists = data_dir.join("GeoLite2-ASN.mmdb").exists();
    
    let manager = get_geoip_manager();
    let loaded = manager.read().is_loaded();
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "loaded": loaded,
            "country_db": country_exists,
            "city_db": city_exists,
            "asn_db": asn_exists,
            "data_dir": data_dir.to_string_lossy()
        }
    })))
}

#[derive(Deserialize)]
pub struct DownloadGeoIpRequest {
    pub url: String,
    pub db_type: String, // "country", "city", "asn"
}

/// POST /api/settings/geoip/download - 下载GeoIP数据库
pub async fn download_geoip_db(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<DownloadGeoIpRequest>,
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
    let data_dir = std::env::current_dir()
        .map(|p| p.join("data"))
        .unwrap_or_else(|_| PathBuf::from("data"));
    
    // 确保data目录存在
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        return Ok(Json(json!({
            "code": 500,
            "message": format!("创建目录失败: {}", e)
        })));
    }
    
    let filename = match req.db_type.as_str() {
        "country" => "GeoLite2-Country.mmdb",
        "city" => "GeoLite2-City.mmdb",
        "asn" => "GeoLite2-ASN.mmdb",
        _ => return Ok(Json(json!({
            "code": 400,
            "message": "无效的数据库类型"
        }))),
    };
    
    let dest_path = data_dir.join(filename);
    
    // 下载文件
    tracing::info!("开始下载GeoIP数据库: {} -> {:?}", req.url, dest_path);
    
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build() 
    {
        Ok(c) => c,
        Err(e) => return Ok(Json(json!({
            "code": 500,
            "message": format!("创建HTTP客户端失败: {}", e)
        }))),
    };
    
    let response = match client.get(&req.url).send().await {
        Ok(r) => r,
        Err(e) => return Ok(Json(json!({
            "code": 500,
            "message": format!("下载失败: {}", e)
        }))),
    };
    
    if !response.status().is_success() {
        return Ok(Json(json!({
            "code": 500,
            "message": format!("下载失败: HTTP {}", response.status())
        })));
    }
    
    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => return Ok(Json(json!({
            "code": 500,
            "message": format!("读取响应失败: {}", e)
        }))),
    };
    
    // 检查是否为gzip压缩（.gz文件）
    let final_bytes = if req.url.ends_with(".gz") {
        use flate2::read::GzDecoder;
        use std::io::Read;
        
        let mut decoder = GzDecoder::new(&bytes[..]);
        let mut decompressed = Vec::new();
        if let Err(e) = decoder.read_to_end(&mut decompressed) {
            return Ok(Json(json!({
                "code": 500,
                "message": format!("解压失败: {}", e)
            })));
        }
        decompressed
    } else {
        bytes.to_vec()
    };
    
    // 写入文件
    let mut file = match tokio::fs::File::create(&dest_path).await {
        Ok(f) => f,
        Err(e) => return Ok(Json(json!({
            "code": 500,
            "message": format!("创建文件失败: {}", e)
        }))),
    };
    
    if let Err(e) = file.write_all(&final_bytes).await {
        return Ok(Json(json!({
            "code": 500,
            "message": format!("写入文件失败: {}", e)
        })));
    }
    
    tracing::info!("GeoIP数据库下载完成: {:?}", dest_path);
    
    // 重新加载GeoIP管理器
    let manager = get_geoip_manager();
    let mut mgr = manager.write();
    if let Err(e) = mgr.load_from_dir(&data_dir) {
        tracing::warn!("Failed to reload GeoIP database: {}", e);
    }
    
    Ok(Json(json!({
        "code": 200,
        "message": "下载成功"
    })))
}

/// POST /api/settings/geoip/reload - 重新加载GeoIP数据库
pub async fn reload_geoip_db(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
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
    let data_dir = std::env::current_dir()
        .map(|p| p.join("data"))
        .unwrap_or_else(|_| PathBuf::from("data"));
    
    let manager = get_geoip_manager();
    let mut mgr = manager.write();
    if let Err(e) = mgr.load_from_dir(&data_dir) {
        return Ok(Json(json!({
            "code": 500,
            "message": format!("加载失败: {}", e)
        })));
    }
    
    Ok(Json(json!({
        "code": 200,
        "message": "加载成功"
    })))
}

#[derive(Deserialize)]
pub struct GeoIpConfigRequest {
    pub enabled: bool,
    pub url: String,
    pub update_interval: String, // "daily", "weekly", "monthly"
}

/// GET /api/settings/geoip/config - 获取GeoIP更新配置
pub async fn get_geoip_config(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
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
    let enabled: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'geoip_auto_update'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let url: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'geoip_update_url'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let interval: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'geoip_update_interval'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let last_update: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'geoip_last_update'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "enabled": enabled.map(|(v,)| v == "true").unwrap_or(false),
            "url": url.map(|(v,)| v).unwrap_or_else(|| "https://git.io/GeoLite2-Country.mmdb".to_string()),
            "update_interval": interval.map(|(v,)| v).unwrap_or_else(|| "weekly".to_string()),
            "last_update": last_update.map(|(v,)| v)
        }
    })))
}

/// POST /api/settings/geoip/config - 保存GeoIP更新配置
pub async fn save_geoip_config(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<GeoIpConfigRequest>,
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
    
    tracing::info!("Saving GeoIP config: enabled={}, url={}, interval={}", req.enabled, req.url, req.update_interval);
    
    if let Err(e) = sqlx::query(
        "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
    )
    .bind("geoip_auto_update")
    .bind(if req.enabled { "true" } else { "false" })
    .bind(&now)
    .execute(&state.db)
    .await {
        tracing::error!("Failed to save geoip_auto_update: {}", e);
        return Ok(Json(json!({
            "code": 500,
            "message": format!("保存失败: {}", e)
        })));
    }
    
    if let Err(e) = sqlx::query(
        "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
    )
    .bind("geoip_update_url")
    .bind(&req.url)
    .bind(&now)
    .execute(&state.db)
    .await {
        tracing::error!("Failed to save geoip_update_url: {}", e);
        return Ok(Json(json!({
            "code": 500,
            "message": format!("保存失败: {}", e)
        })));
    }
    
    if let Err(e) = sqlx::query(
        "INSERT OR REPLACE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
    )
    .bind("geoip_update_interval")
    .bind(&req.update_interval)
    .bind(&now)
    .execute(&state.db)
    .await {
        tracing::error!("Failed to save geoip_update_interval: {}", e);
        return Ok(Json(json!({
            "code": 500,
            "message": format!("保存失败: {}", e)
        })));
    }
    
    tracing::info!("GeoIP config saved successfully");
    
    Ok(Json(json!({
        "code": 200,
        "message": "保存成功"
    })))
}

/// GET /api/settings/version - 获取版本信息
pub async fn get_version_info() -> Json<Value> {
    Json(json!({
        "code": 200,
        "data": {
            "backend_version": env!("CARGO_PKG_VERSION"),
            "build_time": env!("BUILD_TIME"),
        }
    }))
}
