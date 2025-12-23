use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use chrono::Utc;
use serde::Deserialize;

use crate::state::AppState;
use crate::api::file_resolver::{get_all_mounts, get_matching_mounts, get_first_mount};
use crate::api::files::{create_download_token, create_download_token_with_user};
use crate::api::stats;
use super::types::*;
use rand::Rng;

fn generate_short_id(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// ========== 分享访问API（公开） ==========

#[derive(Debug, Deserialize)]
pub struct VerifyShareRequest {
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ShareFileRequest {
    pub sub_path: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub sort_by: Option<String>,    // name, modified, size
    pub sort_order: Option<String>, // asc, desc
}

/// GET /api/s/:short_id/info - 获取分享信息（公开）
pub async fn get_share_info(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let share: Option<ShareWithCreator> = sqlx::query_as(
        "SELECT s.id, s.user_id, s.short_id, s.path, s.name, s.is_dir, s.password, s.expires_at, s.max_access_count, s.access_count, s.enabled, s.created_at, s.updated_at, u.username as creator_name
         FROM shares s
         LEFT JOIN users u ON s.user_id = u.id
         WHERE s.short_id = ?"
    )
    .bind(&short_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let share = share.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"code": "NOT_FOUND", "message": "分享不存在或已被删除"}))))?;
    
    // 检查是否启用
    if !share.enabled {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "DISABLED", "message": "分享已被禁用"}))));
    }
    
    // 检查是否过期
    if let Some(ref expires) = share.expires_at {
        let is_expired = if let Ok(expires_time) = chrono::DateTime::parse_from_rfc3339(expires) {
            expires_time < chrono::Utc::now()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M:%S") {
            expires_time < chrono::Utc::now().naive_utc()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M") {
            expires_time < chrono::Utc::now().naive_utc()
        } else {
            false
        };
        
        if is_expired {
            return Err((StatusCode::GONE, Json(json!({"code": "EXPIRED", "message": "分享已过期"}))));
        }
    }
    
    // 检查访问次数
    if let Some(max_count) = share.max_access_count {
        if share.access_count >= max_count {
            return Err((StatusCode::GONE, Json(json!({"code": "EXHAUSTED", "message": "分享访问次数已达上限"}))));
        }
    }
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "name": share.name,
            "is_dir": share.is_dir,
            "has_password": share.password.is_some(),
            "creator_name": share.creator_name.unwrap_or_else(|| "游客".to_string()),
            "created_at": share.created_at,
            "expires_at": share.expires_at
        }
    })))
}

/// POST /api/s/:short_id/verify - 验证分享密码
pub async fn verify_share(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Json(req): Json<VerifyShareRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let share: Option<Share> = sqlx::query_as(
        "SELECT id, user_id, short_id, path, name, is_dir, password, expires_at, max_access_count, access_count, enabled, created_at, updated_at
         FROM shares WHERE short_id = ?"
    )
    .bind(&short_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let share = share.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"code": "NOT_FOUND", "message": "分享不存在"}))))?;
    
    // 检查状态
    if !share.enabled {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "DISABLED", "message": "分享已被禁用"}))));
    }
    
    // 检查密码
    if let Some(ref pwd) = share.password {
        let input_pwd = req.password.unwrap_or_default();
        if input_pwd != *pwd {
            return Err((StatusCode::FORBIDDEN, Json(json!({"code": "WRONG_PASSWORD", "message": "提取码错误"}))));
        }
    }
    
    // 注意：访问次数在下载时增加，而不是在验证时
    
    // 生成访问令牌（有效期1小时）
    let token = generate_short_id(32);
    let expires = Utc::now() + chrono::Duration::hours(1);
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "token": token,
            "expires_at": expires.to_rfc3339(),
            "path": share.path,
            "name": share.name,
            "is_dir": share.is_dir
        }
    })))
}

/// POST /api/share/:short_id/files - 获取分享文件列表（公开，无需认证）
pub async fn get_share_files(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Json(req): Json<ShareFileRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("get_share_files: short_id={}, sub_path={:?}", short_id, req.sub_path);
    
    let share: Option<Share> = sqlx::query_as(
        "SELECT id, user_id, short_id, path, name, is_dir, password, expires_at, max_access_count, access_count, enabled, created_at, updated_at
         FROM shares WHERE short_id = ?"
    )
    .bind(&short_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("get_share_files: Failed to query share: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
    })?;
    
    let share = share.ok_or_else(|| {
        tracing::warn!("get_share_files: 分享不存在 short_id={}", short_id);
        (StatusCode::NOT_FOUND, Json(json!({"code": "NOT_FOUND", "message": "分享不存在"})))
    })?;
    
    tracing::debug!("get_share_files: 找到分享 path={}, is_dir={}", share.path, share.is_dir);
    
    if !share.enabled {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "DISABLED", "message": "分享已被禁用"}))));
    }
    
    // 获取所有存储挂载点（使用file_resolver）
    let mounts = get_all_mounts(&state).await
        .map_err(|e| {
            tracing::error!("get_share_files: Failed to query drivers: {:?}", e);
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"})))
        })?;
    
    tracing::debug!("get_share_files: 找到 {} 个驱动", mounts.len());
    
    let base_path = yaolist_backend::utils::fix_and_clean_path(&share.path);
    tracing::debug!("get_share_files: base_path={}", base_path);
    
    // 如果是单文件分享，直接返回文件信息
    if !share.is_dir {
        tracing::debug!("get_share_files: Single file share mode");
        
        // 获取父目录路径
        let parent_path = std::path::Path::new(&base_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let parent_path = if parent_path.is_empty() { "/".to_string() } else { parent_path };
        
        let mount = get_first_mount(&parent_path, &mounts).ok_or_else(|| {
            tracing::error!("get_share_files: 未找到驱动 parent_path={}", parent_path);
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"})))
        })?;
        
        let mount_path = yaolist_backend::utils::fix_and_clean_path(&mount.mount_path);
        let relative_parent = if parent_path.len() > mount_path.len() {
            yaolist_backend::utils::fix_and_clean_path(&parent_path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        let driver = state.storage_manager.get_driver(&mount.id).await
            .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"}))))?;
        
        // 列出父目录找到文件信息
        let entries = driver.list(&relative_parent).await
            .map_err(|e| {
                tracing::error!("Share listing failed: {}", e);
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"})))
            })?;
        
        let file_entry = entries.iter().find(|e| e.name == share.name);
        
        let files: Vec<Value> = if let Some(entry) = file_entry {
            vec![json!({
                "name": entry.name,
                "size": entry.size,
                "is_dir": entry.is_dir,
                "modified": entry.modified
            })]
        } else {
            vec![json!({
                "name": share.name,
                "size": 0,
                "is_dir": false,
                "modified": null
            })]
        };
        
        return Ok(Json(json!({
            "code": 200,
            "data": {
                "files": files,
                "path": ""
            }
        })));
    }
    
    // 目录分享：构建实际路径
    let actual_path = if let Some(ref sub) = req.sub_path {
        let sub = sub.trim_start_matches('/');
        if sub.is_empty() {
            base_path.clone()
        } else {
            format!("{}/{}", base_path, sub)
        }
    } else {
        base_path.clone()
    };
    
    // 安全检查：确保不能访问分享路径之外的文件
    let actual_clean = yaolist_backend::utils::fix_and_clean_path(&actual_path);
    if !actual_clean.starts_with(&base_path) && actual_clean != base_path {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "FORBIDDEN", "message": "无权访问此路径"}))));
    }
    
    tracing::debug!("get_share_files: 目录分享模式, actual_clean={}", actual_clean);
    let mount = get_first_mount(&actual_clean, &mounts).ok_or_else(|| {
        tracing::error!("get_share_files: 目录分享未找到驱动 actual_clean={}", actual_clean);
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"})))
    })?;
    
    let mount_path = yaolist_backend::utils::fix_and_clean_path(&mount.mount_path);
    let relative_path = if actual_clean.len() > mount_path.len() {
        yaolist_backend::utils::fix_and_clean_path(&actual_clean[mount_path.len()..])
    } else {
        "/".to_string()
    };
    
    let driver = state.storage_manager.get_driver(&mount.id).await
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"}))))?;
    
    let mut entries = driver.list(&relative_path).await
        .map_err(|e| {
            tracing::error!("Share directory listing failed: {}", e);
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"code": "DRIVER_ERROR", "message": "存储驱动故障"})))
        })?;
    
    // 排序处理：目录始终在前，然后按指定字段排序
    let sort_by = req.sort_by.as_deref().unwrap_or("name");
    let sort_order = req.sort_order.as_deref().unwrap_or("asc");
    let is_desc = sort_order == "desc";
    
    entries.sort_by(|a, b| {
        // 目录始终在前
        if a.is_dir && !b.is_dir {
            return std::cmp::Ordering::Less;
        }
        if !a.is_dir && b.is_dir {
            return std::cmp::Ordering::Greater;
        }
        
        let cmp = match sort_by {
            "modified" => a.modified.cmp(&b.modified),
            "size" => a.size.cmp(&b.size),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        
        if is_desc { cmp.reverse() } else { cmp }
    });
    
    // 分页处理
    let total = entries.len();
    let page = req.page.unwrap_or(1).max(1) as usize;
    let per_page = req.per_page.unwrap_or(50).clamp(1, 100) as usize;
    let start = (page - 1) * per_page;
    let end = (start + per_page).min(total);
    
    let paged_entries = if start < total {
        &entries[start..end]
    } else {
        &entries[0..0]
    };
    
    let files: Vec<Value> = paged_entries.iter().map(|e| {
        json!({
            "name": e.name,
            "size": e.size,
            "is_dir": e.is_dir,
            "modified": e.modified
        })
    }).collect();
    
    // 返回所有文件名用于全选
    let all_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "files": files,
            "total": total,
            "page": page,
            "per_page": per_page,
            "path": req.sub_path.unwrap_or_default(),
            "all_names": all_names
        }
    })))
}

/// GET /api/share/:short_id/download/:filename - 生成临时下载链接
/// Generate temporary download link for shared file
pub async fn get_share_download(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((short_id, filename)): Path<(String, String)>,
    Query(query): Query<ShareFileRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let share: Option<Share> = sqlx::query_as(
        "SELECT id, user_id, short_id, path, name, is_dir, password, expires_at, max_access_count, access_count, enabled, created_at, updated_at
         FROM shares WHERE short_id = ?"
    )
    .bind(&short_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let share = share.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"code": "NOT_FOUND", "message": "分享不存在"}))))?;
    
    if !share.enabled {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "DISABLED", "message": "分享已被禁用"}))));
    }
    
    // 检查是否过期
    if let Some(ref expires) = share.expires_at {
        let is_expired = if let Ok(expires_time) = chrono::DateTime::parse_from_rfc3339(expires) {
            expires_time < chrono::Utc::now()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M:%S") {
            expires_time < chrono::Utc::now().naive_utc()
        } else if let Ok(expires_time) = chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%dT%H:%M") {
            expires_time < chrono::Utc::now().naive_utc()
        } else {
            false
        };
        if is_expired {
            return Err((StatusCode::GONE, Json(json!({"code": "EXPIRED", "message": "分享已过期"}))));
        }
    }
    
    // 检查访问次数
    if let Some(max_count) = share.max_access_count {
        if share.access_count >= max_count {
            return Err((StatusCode::GONE, Json(json!({"code": "EXHAUSTED", "message": "分享访问次数已达上限"}))));
        }
    }
    
    // 增加访问次数（下载时计数）
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE shares SET access_count = access_count + 1, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(share.id)
        .execute(&state.db)
        .await
        .ok();
    
    // 安全检查：验证文件名是否在分享范围内
    let base_path = yaolist_backend::utils::fix_and_clean_path(&share.path);
    let file_path = if share.is_dir {
        if let Some(ref sub) = query.sub_path {
            let sub = sub.trim_start_matches('/');
            if sub.is_empty() {
                format!("{}/{}", base_path, filename)
            } else {
                format!("{}/{}/{}", base_path, sub, filename)
            }
        } else {
            format!("{}/{}", base_path, filename)
        }
    } else {
        // 单文件分享，只能下载这个文件
        if filename != share.name {
            return Err((StatusCode::FORBIDDEN, Json(json!({"code": "FORBIDDEN", "message": "无权下载此文件"}))));
        }
        base_path.clone()
    };
    
    // 安全检查：确保路径在分享范围内
    let file_path_clean = yaolist_backend::utils::fix_and_clean_path(&file_path);
    if share.is_dir && !file_path_clean.starts_with(&base_path) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"code": "FORBIDDEN", "message": "无权下载此文件"}))));
    }
    
    // Get scheme from X-Forwarded-Proto header / 从反代请求头获取协议
    let scheme = headers.get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    // Find driver and internal path for this file / 查找文件对应的驱动和内部路径
    let mounts = get_all_mounts(&state).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let matching_mounts = get_matching_mounts(&file_path_clean, &mounts);
    if matching_mounts.is_empty() {
        return Err((StatusCode::NOT_FOUND, Json(json!({"code": "MOUNT_NOT_FOUND", "message": "挂载点不存在"}))));
    }
    
    // Find driver with the file / 查找包含该文件的驱动
    let parent_path = file_path_clean.rsplitn(2, '/').nth(1).unwrap_or("/");
    let parent_path = if parent_path.is_empty() { "/" } else { parent_path };
    let file_name = file_path_clean.split('/').last().unwrap_or("");
    
    let mut found_driver_id = None;
    let mut found_internal_path = String::new();
    let mut found_file_size = None;
    let mut can_direct_link = false;
    
    for mount in &matching_mounts {
        let mount_path = yaolist_backend::utils::fix_and_clean_path(&mount.mount_path);
        let actual_parent = if parent_path.len() > mount_path.len() {
            yaolist_backend::utils::fix_and_clean_path(&parent_path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            if let Ok(files) = driver.list(&actual_parent).await {
                if let Some(file) = files.iter().find(|f| f.name == file_name && !f.is_dir) {
                    found_internal_path = format!("{}/{}", actual_parent.trim_end_matches('/'), file_name);
                    found_internal_path = yaolist_backend::utils::fix_and_clean_path(&found_internal_path);
                    found_driver_id = Some(mount.id.clone());
                    found_file_size = Some(file.size);
                    // Let fs_download decide if direct link is available / 让 fs_download 决定是否使用直链
                    can_direct_link = true;
                    break;
                }
            }
        }
    }
    
    let driver_id = found_driver_id.ok_or_else(|| 
        (StatusCode::NOT_FOUND, Json(json!({"code": "FILE_NOT_FOUND", "message": "文件不存在"}))))?;
    
    // Use configured link expiry / 使用配置的链接有效期
    let expiry_minutes = state.download_settings.get_link_expiry_minutes() as i64;
    let expires_at = Utc::now() + chrono::Duration::minutes(expiry_minutes);
    
    // Create download token with user_id for traffic stats / 创建带用户ID的下载令牌（用于流量统计）
    // 流量统计在实际下载时进行：302统计整个文件，本地中转统计实际传输
    let token = create_download_token_with_user(
        found_internal_path,
        driver_id,
        expires_at,
        can_direct_link,
        found_file_size,
        share.user_id.clone(),
    ).await;
    
    // Build download URL with configured domain / 使用配置的下载域名生成下载链接
    let download_path = format!("/download/{}", token);
    let download_url = state.download_settings.build_download_url(&download_path, scheme);
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "url": download_url,
            "expires_at": expires_at.to_rfc3339()
        }
    })))
}

