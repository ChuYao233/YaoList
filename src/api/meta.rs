use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::models::{CreateMetaRequest, Meta, UpdateMetaRequest};
use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;

/// 验证管理员权限
async fn require_admin(state: &AppState, cookies: &Cookies) -> Result<(), (StatusCode, Json<Value>)> {
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
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct ListMetasQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

pub async fn list_metas(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Query(query): Query<ListMetasQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(10).clamp(1, 100);
    let offset = (page - 1) * per_page;

    // Get total count
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM metas")
        .fetch_one(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let metas: Vec<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas ORDER BY path LIMIT ? OFFSET ?"
    )
    .bind(per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let total_pages = (total.0 as f64 / per_page as f64).ceil() as i64;

    Ok(Json(json!({
        "code": 200,
        "data": {
            "content": metas,
            "total": total.0,
            "page": page,
            "per_page": per_page,
            "total_pages": total_pages
        }
    })))
}

pub async fn get_meta(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let meta: Option<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    match meta {
        Some(m) => Ok(Json(json!({
            "code": 200,
            "data": m
        }))),
        None => Ok(Json(json!({
            "code": 404,
            "message": "Meta not found"
        })))
    }
}

pub async fn create_meta(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateMetaRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    let result = sqlx::query(
        "INSERT INTO metas (path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&req.path)
    .bind(&req.password)
    .bind(req.p_sub.unwrap_or(false))
    .bind(req.write.unwrap_or(false))
    .bind(req.w_sub.unwrap_or(false))
    .bind(&req.hide)
    .bind(req.h_sub.unwrap_or(false))
    .bind(&req.readme)
    .bind(req.r_sub.unwrap_or(false))
    .bind(&req.header)
    .bind(req.header_sub.unwrap_or(false))
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(json!({
            "code": 200,
            "message": "Meta created successfully"
        }))),
        Err(e) => {
            eprintln!("Failed to create meta: {:?}", e);
            Ok(Json(json!({
                "code": 500,
                "message": "Failed to create meta"
            })))
        }
    }
}

pub async fn update_meta(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
    Json(req): Json<UpdateMetaRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    // 获取现有的 meta
    let existing: Option<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let existing = match existing {
        Some(m) => m,
        None => return Ok(Json(json!({
            "code": 404,
            "message": "Meta not found"
        })))
    };

    let result = sqlx::query(
        "UPDATE metas SET path = ?, password = ?, p_sub = ?, write = ?, w_sub = ?, hide = ?, h_sub = ?, readme = ?, r_sub = ?, header = ?, header_sub = ?, updated_at = ? WHERE id = ?"
    )
    .bind(req.path.unwrap_or(existing.path))
    .bind(req.password.or(existing.password))
    .bind(req.p_sub.unwrap_or(existing.p_sub))
    .bind(req.write.unwrap_or(existing.write))
    .bind(req.w_sub.unwrap_or(existing.w_sub))
    .bind(req.hide.or(existing.hide))
    .bind(req.h_sub.unwrap_or(existing.h_sub))
    .bind(req.readme.or(existing.readme))
    .bind(req.r_sub.unwrap_or(existing.r_sub))
    .bind(req.header.or(existing.header))
    .bind(req.header_sub.unwrap_or(existing.header_sub))
    .bind(&now)
    .bind(id)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok(Json(json!({
            "code": 200,
            "message": "Meta updated successfully"
        }))),
        Err(e) => {
            eprintln!("Failed to update meta: {:?}", e);
            Ok(Json(json!({
                "code": 500,
                "message": "Failed to update meta"
            })))
        }
    }
}

pub async fn delete_meta(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let result = sqlx::query("DELETE FROM metas WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => Ok(Json(json!({
            "code": 200,
            "message": "Meta deleted successfully"
        }))),
        Err(e) => {
            eprintln!("Failed to delete meta: {:?}", e);
            Ok(Json(json!({
                "code": 500,
                "message": "Failed to delete meta"
            })))
        }
    }
}

// 获取指定路径的元信息（用于前台）
pub async fn get_meta_for_path(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, StatusCode> {
    let path = req.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    
    // 查找匹配的元信息
    let metas: Vec<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas ORDER BY length(path) DESC"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 找到最匹配的元信息
    let mut result_meta: Option<Meta> = None;
    let mut has_password = false;
    let password_verified = false;
    let mut can_write = false;
    let mut hide_patterns: Vec<String> = vec![];
    let mut readme: Option<String> = None;
    let mut header: Option<String> = None;

    for meta in metas {
        let meta_path = meta.path.trim_end_matches('/');
        let check_path = path.trim_end_matches('/');
        
        // 检查是否匹配
        let is_match = if meta_path == check_path {
            true
        } else if check_path.starts_with(&format!("{}/", meta_path)) {
            // 子路径匹配，检查是否应用到子文件夹
            true
        } else if meta_path == "/" || meta_path.is_empty() {
            true
        } else {
            false
        };

        if !is_match {
            continue;
        }

        let is_sub = check_path != meta_path && check_path.starts_with(&format!("{}/", meta_path));

        // 密码
        if meta.password.is_some() && !meta.password.as_ref().unwrap().is_empty() {
            if !is_sub || meta.p_sub {
                has_password = true;
            }
        }

        // 写入权限
        if meta.write {
            if !is_sub || meta.w_sub {
                can_write = true;
            }
        }

        // 隐藏
        if let Some(ref hide) = meta.hide {
            if !hide.is_empty() && (!is_sub || meta.h_sub) {
                for line in hide.lines() {
                    if !line.trim().is_empty() {
                        hide_patterns.push(line.trim().to_string());
                    }
                }
            }
        }

        // 说明
        if let Some(ref r) = meta.readme {
            if !r.is_empty() && (!is_sub || meta.r_sub) {
                readme = Some(r.clone());
            }
        }

        // 顶部说明
        if let Some(ref h) = meta.header {
            if !h.is_empty() && (!is_sub || meta.header_sub) {
                header = Some(h.clone());
            }
        }

        if result_meta.is_none() {
            result_meta = Some(meta);
        }
    }

    Ok(Json(json!({
        "code": 200,
        "data": {
            "has_password": has_password,
            "password_verified": password_verified,
            "can_write": can_write,
            "hide_patterns": hide_patterns,
            "readme": readme,
            "header": header
        }
    })))
}

// 验证路径密码
pub async fn verify_meta_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, StatusCode> {
    let path = req.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    let password = req.get("password").and_then(|v| v.as_str()).unwrap_or("");

    let metas: Vec<Meta> = sqlx::query_as(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas ORDER BY length(path) DESC"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for meta in metas {
        let meta_path = meta.path.trim_end_matches('/');
        let check_path = path.trim_end_matches('/');
        
        let is_match = if meta_path == check_path {
            true
        } else if check_path.starts_with(&format!("{}/", meta_path)) && meta.p_sub {
            true
        } else if (meta_path == "/" || meta_path.is_empty()) && meta.p_sub {
            true
        } else {
            false
        };

        if !is_match {
            continue;
        }

        if let Some(ref meta_password) = meta.password {
            if !meta_password.is_empty() {
                if meta_password == password {
                    return Ok(Json(json!({
                        "code": 200,
                        "data": {
                            "verified": true
                        }
                    })));
                } else {
                    return Ok(Json(json!({
                        "code": 200,
                        "data": {
                            "verified": false
                        }
                    })));
                }
            }
        }
    }

    // 没有找到需要密码的元信息
    Ok(Json(json!({
        "code": 200,
        "data": {
            "verified": true
        }
    })))
}
