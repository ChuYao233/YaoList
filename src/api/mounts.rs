use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;
use tower_cookies::Cookies;

use crate::{models::{CreateMountRequest, UpdateMountRequest, Mount}, state::AppState, auth::SESSION_COOKIE_NAME};

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

pub async fn list_mounts(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let mounts = sqlx::query_as::<_, Mount>("SELECT * FROM mounts ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    Ok(Json(json!({
        "mounts": mounts
    })))
}

pub async fn create_mount(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateMountRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let config_str = serde_json::to_string(&req.config)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error": "配置格式错误"}))))?;
    
    sqlx::query(
        "INSERT INTO mounts (id, name, driver, mount_path, config, enabled, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, 1, ?, ?)"
    )
    .bind(&id)
    .bind(&req.name)
    .bind(&req.driver)
    .bind(&req.mount_path)
    .bind(&config_str)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    Ok(Json(json!({
        "id": id,
        "message": "挂载点创建成功"
    })))
}

pub async fn get_mount(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let mount = sqlx::query_as::<_, Mount>("SELECT * FROM mounts WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"error": "挂载点不存在"}))))?;
    
    Ok(Json(json!({
        "mount": mount
    })))
}

pub async fn update_mount(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
    Json(req): Json<UpdateMountRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now();
    
    if let Some(name) = req.name {
        sqlx::query("UPDATE mounts SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(now.to_rfc3339())
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(config) = req.config {
        let config_str = serde_json::to_string(&config)
            .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error": "配置格式错误"}))))?;
        sqlx::query("UPDATE mounts SET config = ?, updated_at = ? WHERE id = ?")
            .bind(&config_str)
            .bind(now.to_rfc3339())
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    if let Some(enabled) = req.enabled {
        sqlx::query("UPDATE mounts SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled as i32)
            .bind(now.to_rfc3339())
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }
    
    Ok(Json(json!({
        "message": "挂载点更新成功"
    })))
}

pub async fn delete_mount(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    sqlx::query("DELETE FROM mounts WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    Ok(Json(json!({
        "message": "挂载点删除成功"
    })))
}
