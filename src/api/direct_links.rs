use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use chrono::Utc;
use tower_cookies::Cookies;
use crate::auth::SESSION_COOKIE_NAME;
use crate::state::AppState;
use rand::Rng;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DirectLink {
    pub id: i64,
    pub user_id: Option<String>,
    pub sign: String,
    pub path: String,
    pub filename: String,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub access_count: i64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DirectLinkWithCreator {
    pub id: i64,
    pub user_id: Option<String>,
    pub sign: String,
    pub path: String,
    pub filename: String,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub access_count: i64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub creator_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDirectLinkRequest {
    pub path: String,
    pub filename: String,
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDirectLinkRequest {
    pub expires_at: Option<String>,
    pub max_access_count: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListDirectLinksQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
}

fn generate_sign(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub async fn list_direct_links(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Query(query): Query<ListDirectLinksQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户登录
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
    
    // 查询直链并关联用户名
    let base_query = if is_admin {
        if search_pattern.is_some() {
            "SELECT d.id, d.user_id, d.sign, d.path, d.filename, d.expires_at, d.max_access_count, d.access_count, d.enabled, d.created_at, d.updated_at, u.username as creator_name
             FROM direct_links d
             LEFT JOIN users u ON d.user_id = u.id
             WHERE d.path LIKE ? OR d.filename LIKE ? OR d.sign LIKE ?
             ORDER BY d.created_at DESC LIMIT ? OFFSET ?"
        } else {
            "SELECT d.id, d.user_id, d.sign, d.path, d.filename, d.expires_at, d.max_access_count, d.access_count, d.enabled, d.created_at, d.updated_at, u.username as creator_name
             FROM direct_links d
             LEFT JOIN users u ON d.user_id = u.id
             ORDER BY d.created_at DESC LIMIT ? OFFSET ?"
        }
    } else {
        if search_pattern.is_some() {
            "SELECT d.id, d.user_id, d.sign, d.path, d.filename, d.expires_at, d.max_access_count, d.access_count, d.enabled, d.created_at, d.updated_at, u.username as creator_name
             FROM direct_links d
             LEFT JOIN users u ON d.user_id = u.id
             WHERE d.user_id = ? AND (d.path LIKE ? OR d.filename LIKE ? OR d.sign LIKE ?)
             ORDER BY d.created_at DESC LIMIT ? OFFSET ?"
        } else {
            "SELECT d.id, d.user_id, d.sign, d.path, d.filename, d.expires_at, d.max_access_count, d.access_count, d.enabled, d.created_at, d.updated_at, u.username as creator_name
             FROM direct_links d
             LEFT JOIN users u ON d.user_id = u.id
             WHERE d.user_id = ?
             ORDER BY d.created_at DESC LIMIT ? OFFSET ?"
        }
    };

    let links: Vec<DirectLinkWithCreator> = if is_admin {
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

    let total: i64 = if is_admin {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM direct_links WHERE path LIKE ? OR filename LIKE ? OR sign LIKE ?"
            )
            .bind(pattern)
            .bind(pattern)
            .bind(pattern)
            .fetch_one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_scalar("SELECT COUNT(*) FROM direct_links")
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    } else {
        if let Some(ref pattern) = search_pattern {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM direct_links WHERE user_id = ? AND (path LIKE ? OR filename LIKE ? OR sign LIKE ?)"
            )
            .bind(&user_id)
            .bind(pattern)
            .bind(pattern)
            .bind(pattern)
            .fetch_one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        } else {
            sqlx::query_scalar("SELECT COUNT(*) FROM direct_links WHERE user_id = ?")
                .bind(&user_id)
                .fetch_one(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
        }
    };
    
    let total_pages = (total as f64 / per_page as f64).ceil() as i64;
    
    Ok(Json(json!({
        "links": links,
        "total": total,
        "page": page,
        "per_page": per_page,
        "total_pages": total_pages
    })))
}

pub async fn create_direct_link(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateDirectLinkRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户登录
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let user_id: Option<String> = sqlx::query_scalar(
        "SELECT u.id FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let user_id = user_id.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话已过期"}))))?;
    
    // 检查用户是否有创建直链的权限
    let has_permission: bool = sqlx::query_scalar(
        "SELECT COALESCE(MAX(g.allow_direct_link), 0) FROM user_groups g
         JOIN user_group_members m ON g.id = m.group_id
         WHERE m.user_id = ?"
    )
    .bind(&user_id)
    .fetch_one(&state.db)
    .await
    .map(|v: i32| v > 0)
    .unwrap_or(false);
    
    // 检查是否是管理员
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_one(&state.db)
        .await
        .map(|v: i32| v > 0)
        .unwrap_or(false);
    
    if !has_permission && !is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "没有创建直链的权限"}))));
    }
    
    let sign = generate_sign(16);
    let now = Utc::now().to_rfc3339();
    let enabled = req.enabled.unwrap_or(true);
    
    sqlx::query(
        "INSERT INTO direct_links (user_id, sign, path, filename, expires_at, max_access_count, access_count, enabled, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"
    )
    .bind(&user_id)
    .bind(&sign)
    .bind(&req.path)
    .bind(&req.filename)
    .bind(&req.expires_at)
    .bind(&req.max_access_count)
    .bind(enabled)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let link: DirectLink = sqlx::query_as(
        "SELECT id, user_id, sign, path, filename, expires_at, max_access_count, access_count, enabled, created_at, updated_at 
         FROM direct_links WHERE sign = ?"
    )
    .bind(&sign)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "message": "创建成功",
        "link": link
    })))
}

pub async fn update_direct_link(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDirectLinkRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户登录
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
    
    // 检查直链是否存在且属于当前用户（管理员可以编辑所有）
    let link_user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM direct_links WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let link_user_id = link_user_id.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "直链不存在"}))))?;
    
    if !is_admin && link_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权编辑此直链"}))));
    }
    
    let now = Utc::now().to_rfc3339();
    
    // 直接更新所有字段（允许设置为null）
    sqlx::query(
        "UPDATE direct_links SET 
         expires_at = ?,
         max_access_count = ?,
         enabled = ?,
         updated_at = ?
         WHERE id = ?"
    )
    .bind(&req.expires_at)
    .bind(&req.max_access_count)
    .bind(&req.enabled.unwrap_or(true))
    .bind(&now)
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let link: DirectLink = sqlx::query_as(
        "SELECT id, user_id, sign, path, filename, expires_at, max_access_count, access_count, enabled, created_at, updated_at 
         FROM direct_links WHERE id = ?"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "message": "更新成功",
        "link": link
    })))
}

pub async fn delete_direct_link(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户登录
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
    
    // 检查直链是否存在且属于当前用户
    let link_user_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT user_id FROM direct_links WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let link_user_id = link_user_id.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "直链不存在"}))))?;
    
    if !is_admin && link_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除此直链"}))));
    }
    
    sqlx::query("DELETE FROM direct_links WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({"message": "删除成功"})))
}

pub async fn toggle_direct_link(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证用户登录
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
    
    // 检查直链是否存在且属于当前用户
    let link: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT user_id, enabled FROM direct_links WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    let (link_user_id, current_enabled) = link.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "直链不存在"}))))?;
    
    if !is_admin && link_user_id.as_ref() != Some(&user_id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作此直链"}))));
    }
    
    let now = Utc::now().to_rfc3339();
    let new_enabled = !current_enabled;
    
    sqlx::query("UPDATE direct_links SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(new_enabled)
        .bind(&now)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    
    Ok(Json(json!({
        "message": if new_enabled { "已启用" } else { "已禁用" },
        "enabled": new_enabled
    })))
}
