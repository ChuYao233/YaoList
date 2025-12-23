use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::{Cookies, Cookie};
use chrono::Utc;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use super::types::*;

/// GET /api/auth/me - 获取当前用户信息
pub async fn get_current_user(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user: Option<(String, String, Option<String>, Option<String>, bool, String, i64, i64)> = sqlx::query_as(
        "SELECT u.id, u.username, u.email, u.phone, u.two_factor_enabled, u.created_at, u.total_requests, u.total_traffic 
         FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let user = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;

    Ok(Json(json!({
        "id": user.0,
        "username": user.1,
        "email": user.2,
        "phone": user.3,
        "two_factor_enabled": user.4,
        "created_at": user.5,
        "total_requests": user.6,
        "total_traffic": user.7
    })))
}
/// POST /api/auth/update-email - 更新邮箱
pub async fn update_email(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<UpdateEmailRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user_id: Option<(String,)> = sqlx::query_as(
        "SELECT u.id FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id,) = user_id.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;

    // 验证验证码（从数据库verification_codes表中验证）
    let code_record: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM verification_codes WHERE target = ? AND code = ? AND type = 'email' AND used = 0 AND expires_at > datetime('now')"
    )
    .bind(&req.email)
    .bind(&req.verification_code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (code_id,) = code_record.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误或已过期"}))))?;

    // 标记验证码已使用
    sqlx::query("UPDATE verification_codes SET used = 1 WHERE id = ?")
        .bind(&code_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    // 检查邮箱是否已被使用（作为邮箱）
    let existing_email: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ? AND id != ?")
        .bind(&req.email)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if existing_email.is_some() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "该邮箱已被其他账号使用"}))));
    }

    // 检查邮箱是否被用作其他用户的手机号（互相唯一）
    let existing_as_phone: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE phone = ? AND id != ?")
        .bind(&req.email)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if existing_as_phone.is_some() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "该邮箱已被其他账号作为手机号使用"}))));
    }

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET email = ?, updated_at = ? WHERE id = ?")
        .bind(&req.email)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新邮箱失败"}))))?;

    Ok(Json(json!({
        "code": 200,
        "message": "邮箱更新成功"
    })))
}

/// POST /api/auth/update-phone - 更新手机号
pub async fn update_phone(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<UpdatePhoneRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user_id: Option<(String,)> = sqlx::query_as(
        "SELECT u.id FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id,) = user_id.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;

    // 验证验证码（从数据库verification_codes表中验证）
    let code_record: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM verification_codes WHERE target = ? AND code = ? AND type = 'sms' AND used = 0 AND expires_at > datetime('now')"
    )
    .bind(&req.phone)
    .bind(&req.verification_code)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (code_id,) = code_record.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误或已过期"}))))?;

    // 标记验证码已使用
    sqlx::query("UPDATE verification_codes SET used = 1 WHERE id = ?")
        .bind(&code_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    // 检查手机号是否已被使用（作为手机号或邮箱）
    let existing_phone: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE phone = ? AND id != ?")
        .bind(&req.phone)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if existing_phone.is_some() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "该手机号已被其他账号使用"}))));
    }

    // 检查手机号是否被用作其他用户的邮箱（互相唯一）
    let existing_as_email: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ? AND id != ?")
        .bind(&req.phone)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if existing_as_email.is_some() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "该手机号已被其他账号作为邮箱使用"}))));
    }

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET phone = ?, updated_at = ? WHERE id = ?")
        .bind(&req.phone)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新手机号失败"}))))?;

    // 删除该用户所有session，强制退出登录
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&user_id)
        .execute(&state.db)
        .await
        .ok();

    // 清除当前session cookie
    let mut cookie = Cookie::new(SESSION_COOKIE_NAME, "");
    cookie.set_path("/");
    cookie.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
    cookies.remove(cookie);

    Ok(Json(json!({
        "code": 200,
        "message": "手机号更新成功",
        "logout": true
    })))
}