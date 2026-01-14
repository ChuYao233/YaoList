use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;
use tower_cookies::Cookies;

use tower_cookies::Cookie;

use crate::{
    models::{CreateUserRequest, UpdateUserRequest, User, UserGroup},
    state::AppState,
    auth::SESSION_COOKIE_NAME,
};

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
pub struct PaginationQuery {
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size")]
    page_size: i64,
    #[serde(default)]
    search: Option<String>,
}

fn default_page() -> i64 { 1 }
fn default_page_size() -> i64 { 10 }

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    let offset = (params.page - 1) * params.page_size;
    
    let (users, total) = if let Some(search) = params.search {
        let search_pattern = format!("%{}%", search);
        let users = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username LIKE ? OR email LIKE ? OR phone LIKE ? OR unique_id LIKE ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(params.page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
        
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE username LIKE ? OR email LIKE ? OR phone LIKE ? OR unique_id LIKE ?"
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(&search_pattern)
        .fetch_one(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
        
        (users, total)
    } else {
        let users = sqlx::query_as::<_, User>(
            "SELECT * FROM users ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(params.page_size)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
        
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
        
        (users, total)
    };

    Ok(Json(json!({
        "users": users,
        "total": total,
        "page": params.page,
        "page_size": params.page_size,
        "total_pages": (total as f64 / params.page_size as f64).ceil() as i64
    })))
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    // 获取下一个整数ID（与注册逻辑保持一致）
    let max_id: Option<(i64,)> = sqlx::query_as("SELECT MAX(CAST(id AS INTEGER)) FROM users WHERE id GLOB '[0-9]*'")
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    let next_id = max_id.map(|(id,)| id + 1).unwrap_or(1);
    let id = next_id.to_string();
    let unique_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    
    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    sqlx::query(
        "INSERT INTO users (id, unique_id, username, password_hash, email, phone, is_admin, enabled, two_factor_enabled, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?, 0, 1, 0, ?, ?)"
    )
    .bind(&id)
    .bind(&unique_id)
    .bind(&req.username)
    .bind(&password_hash)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    for group_id in req.group_ids {
        sqlx::query(
            "INSERT INTO user_group_members (user_id, group_id, created_at) VALUES (?, ?, ?)"
        )
        .bind(&id)
        .bind(&group_id)
        .bind(&now)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    }

    Ok(Json(json!({
        "id": id,
        "message": "用户创建成功"
    })))
}

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
        .ok_or((StatusCode::NOT_FOUND, Json(json!({"error": "用户不存在"}))))?;

    let groups = sqlx::query_as::<_, UserGroup>(
        "SELECT g.* FROM user_groups g 
         INNER JOIN user_group_members ugm ON g.id = ugm.group_id 
         WHERE ugm.user_id = ?"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    Ok(Json(json!({
        "user": user,
        "groups": groups
    })))
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
    body: String,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    tracing::debug!("update_user called with id: {}, body: {}", id, body);
    
    let req: UpdateUserRequest = serde_json::from_str(&body)
        .map_err(|e| {
            tracing::error!("Failed to parse UpdateUserRequest: {}", e);
            (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": format!("JSON parse error: {}", e)})))
        })?;
    
    let now = Utc::now().to_rfc3339();
    
    if let Some(username) = &req.username {
        sqlx::query("UPDATE users SET username = ?, updated_at = ? WHERE id = ?")
            .bind(username)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    if let Some(email) = &req.email {
        sqlx::query("UPDATE users SET email = ?, updated_at = ? WHERE id = ?")
            .bind(email)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    if let Some(phone) = &req.phone {
        sqlx::query("UPDATE users SET phone = ?, updated_at = ? WHERE id = ?")
            .bind(phone)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    if let Some(root_path) = &req.root_path {
        sqlx::query("UPDATE users SET root_path = ?, updated_at = ? WHERE id = ?")
            .bind(root_path)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    // 跟踪是否修改了自己的密码
    let mut self_password_changed = false;
    
    if let Some(password) = &req.password {
        let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(&password_hash)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        
        // 检查是否修改的是当前登录用户自己的密码
        if let Some(session_cookie) = cookies.get(SESSION_COOKIE_NAME) {
            let session_id = session_cookie.value().to_string();
            let current_user: Option<(String,)> = sqlx::query_as(
                "SELECT user_id FROM sessions WHERE id = ?"
            )
            .bind(&session_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            
            if let Some((current_user_id,)) = current_user {
                if current_user_id == id {
                    self_password_changed = true;
                    // 删除该用户所有session
                    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                        .bind(&id)
                        .execute(&state.db)
                        .await
                        .ok();
                }
            }
        }
    }
    
    if let Some(enabled) = req.enabled {
        sqlx::query("UPDATE users SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled as i32)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    if let Some(two_factor) = req.two_factor_enabled {
        sqlx::query("UPDATE users SET two_factor_enabled = ?, updated_at = ? WHERE id = ?")
            .bind(two_factor as i32)
            .bind(&now)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    }
    
    if let Some(group_ids) = &req.group_ids {
        sqlx::query("DELETE FROM user_group_members WHERE user_id = ?")
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        
        for group_id in group_ids {
            sqlx::query(
                "INSERT INTO user_group_members (user_id, group_id, created_at) VALUES (?, ?, ?)"
            )
            .bind(&id)
            .bind(group_id)
            .bind(&now)
            .execute(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        }
    }
    
    // 如果修改了自己的密码，清除cookie并返回logout标志
    if self_password_changed {
        let mut cookie = Cookie::new(SESSION_COOKIE_NAME, "");
        cookie.set_path("/");
        cookie.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
        cookies.remove(cookie);
        
        return Ok(Json(json!({
            "message": "用户更新成功",
            "logout": true
        })));
    }
    
    Ok(Json(json!({
        "message": "用户更新成功"
    })))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    Ok(Json(json!({
        "message": "用户删除成功"
    })))
}
