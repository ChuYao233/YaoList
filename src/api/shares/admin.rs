use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;
use chrono::Utc;
use rand::Rng;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use super::types::*;

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

/// 获取用户ID
async fn get_user_id(state: &AppState, cookies: &Cookies) -> Option<String> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)?.value().to_string();
    
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT u.id FROM users u 
         INNER JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .ok()?;
    
    result.map(|(id,)| id)
}

/// GET /api/shares - 获取分享列表
pub async fn list_shares(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Query(query): Query<ListSharesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let user: Option<(String, bool)> = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (user_id, is_admin) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话已过期"}))))?;
    
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(10).min(100).max(1);
    let offset = (page - 1) * per_page;
    
    let search_pattern = query.search.as_ref().map(|s| format!("%{}%", s));
    
    // 查询分享并关联用户名
    let base_query = if is_admin {
        if search_pattern.is_some() {
            "SELECT s.id, s.user_id, s.short_id, s.path, s.name, s.is_dir, s.password, s.expires_at, s.max_access_count, s.access_count, s.enabled, s.created_at, s.updated_at, u.username as creator_name
             FROM shares s
             LEFT JOIN users u ON s.user_id = u.id
             WHERE s.path LIKE ? OR s.name LIKE ? OR s.short_id LIKE ?
             ORDER BY s.created_at DESC LIMIT ? OFFSET ?"
        } else {
            "SELECT s.id, s.user_id, s.short_id, s.path, s.name, s.is_dir, s.password, s.expires_at, s.max_access_count, s.access_count, s.enabled, s.created_at, s.updated_at, u.username as creator_name
             FROM shares s
             LEFT JOIN users u ON s.user_id = u.id
             ORDER BY s.created_at DESC LIMIT ? OFFSET ?"
        }
    } else {
        if search_pattern.is_some() {
            "SELECT s.id, s.user_id, s.short_id, s.path, s.name, s.is_dir, s.password, s.expires_at, s.max_access_count, s.access_count, s.enabled, s.created_at, s.updated_at, u.username as creator_name
             FROM shares s
             LEFT JOIN users u ON s.user_id = u.id
             WHERE s.user_id = ? AND (s.path LIKE ? OR s.name LIKE ? OR s.short_id LIKE ?)
             ORDER BY s.created_at DESC LIMIT ? OFFSET ?"
        } else {
            "SELECT s.id, s.user_id, s.short_id, s.path, s.name, s.is_dir, s.password, s.expires_at, s.max_access_count, s.access_count, s.enabled, s.created_at, s.updated_at, u.username as creator_name
             FROM shares s
             LEFT JOIN users u ON s.user_id = u.id
             WHERE s.user_id = ?
             ORDER BY s.created_at DESC LIMIT ? OFFSET ?"
        }
    };

    let shares: Vec<ShareWithCreator> = if is_admin {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_as(base_query)
                .bind(pattern)
                .bind(pattern)
                .bind(pattern)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_as(base_query)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    } else {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_as(base_query)
                .bind(&user_id)
                .bind(pattern)
                .bind(pattern)
                .bind(pattern)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_as(base_query)
                .bind(&user_id)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    };
    
    // 获取总数
    let count_query = if is_admin {
        if search_pattern.is_some() {
            "SELECT COUNT(*) FROM shares WHERE path LIKE ? OR name LIKE ? OR short_id LIKE ?"
        } else {
            "SELECT COUNT(*) FROM shares"
        }
    } else {
        if search_pattern.is_some() {
            "SELECT COUNT(*) FROM shares WHERE user_id = ? AND (path LIKE ? OR name LIKE ? OR short_id LIKE ?)"
        } else {
            "SELECT COUNT(*) FROM shares WHERE user_id = ?"
        }
    };
    
    let total: i64 = if is_admin {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_scalar(count_query)
                .bind(pattern)
                .bind(pattern)
                .bind(pattern)
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_scalar(count_query)
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    } else {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_scalar(count_query)
                .bind(&user_id)
                .bind(pattern)
                .bind(pattern)
                .bind(pattern)
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_scalar(count_query)
                .bind(&user_id)
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    };
    
    // 隐藏密码内容，只返回是否有密码
    let shares_response: Vec<Value> = shares.iter().map(|s| {
        json!({
            "id": s.id,
            "user_id": s.user_id,
            "short_id": s.short_id,
            "path": s.path,
            "name": s.name,
            "is_dir": s.is_dir,
            "has_password": s.password.is_some(),
            "expires_at": s.expires_at,
            "max_access_count": s.max_access_count,
            "access_count": s.access_count,
            "enabled": s.enabled,
            "created_at": s.created_at,
            "updated_at": s.updated_at,
            "creator_name": s.creator_name
        })
    }).collect();
    
    Ok(Json(json!({
        "data": shares_response,
        "total": total,
        "page": page,
        "per_page": per_page
    })))
}

/// POST /api/shares - 创建分享
pub async fn create_share(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateShareRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = get_user_id(&state, &cookies).await;
    
    let path = req.path.trim();
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "路径不能为空"}))));
    }
    
    let name = path.split('/').last().unwrap_or("share").to_string();
    
    // 检查路径是否为目录（简单检查：路径不含扩展名或以/结尾）
    let is_dir = !name.contains('.') || path.ends_with('/');
    
    let short_id = generate_short_id(8);
    let now = Utc::now().to_rfc3339();
    
    sqlx::query(
        "INSERT INTO shares (user_id, short_id, path, name, is_dir, password, expires_at, max_access_count, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)"
    )
    .bind(&user_id)
    .bind(&short_id)
    .bind(path)
    .bind(&name)
    .bind(is_dir)
    .bind(&req.password)
    .bind(&req.expires_at)
    .bind(&req.max_access_count)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create share: {:?}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建分享失败"})))
    })?;
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "short_id": short_id,
            "url": format!("/share/{}", short_id)
        }
    })))
}

/// POST /api/shares/:id - 更新分享
pub async fn update_share(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
    Json(req): Json<UpdateShareRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let user: Option<(String, bool)> = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (user_id, is_admin) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话已过期"}))))?;
    
    // 检查分享是否存在且属于当前用户
    let share_user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM shares WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let share_user_id = share_user_id.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "分享不存在"}))))?;
    
    if !is_admin && share_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权编辑此分享"}))));
    }
    
    let now = Utc::now().to_rfc3339();
    
    sqlx::query(
        "UPDATE shares SET 
         password = ?,
         expires_at = ?,
         max_access_count = ?,
         enabled = ?,
         updated_at = ?
         WHERE id = ?"
    )
    .bind(&req.password)
    .bind(&req.expires_at)
    .bind(&req.max_access_count)
    .bind(&req.enabled.unwrap_or(true))
    .bind(&now)
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "code": 200,
        "message": "更新成功"
    })))
}

/// DELETE /api/shares/:id - 删除分享
pub async fn delete_share(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let user: Option<(String, bool)> = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (user_id, is_admin) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话已过期"}))))?;
    
    // 检查分享是否存在且属于当前用户
    let share_user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM shares WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let share_user_id = share_user_id.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "分享不存在"}))))?;
    
    if !is_admin && share_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除此分享"}))));
    }
    
    sqlx::query("DELETE FROM shares WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "code": 200,
        "message": "删除成功"
    })))
}

/// POST /api/shares/:id/toggle - 切换分享状态
pub async fn toggle_share(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let user: Option<(String, bool)> = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (user_id, is_admin) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话已过期"}))))?;
    
    // 检查分享是否存在且属于当前用户
    let share: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT user_id, enabled FROM shares WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (share_user_id, current_enabled) = share.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "分享不存在"}))))?;
    
    if !is_admin && share_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作此分享"}))));
    }
    
    let new_enabled = !current_enabled;
    let now = Utc::now().to_rfc3339();
    
    sqlx::query("UPDATE shares SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(new_enabled)
        .bind(&now)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "code": 200,
        "message": if new_enabled { "已启用" } else { "已禁用" },
        "enabled": new_enabled
    })))
}
