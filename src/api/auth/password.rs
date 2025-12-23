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

/// POST /api/auth/forgot-password - 发送密码重置验证码
pub async fn forgot_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForgotPasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 验证图形验证码
    if !state.login_security.verify_captcha(&req.captcha_id, &req.captcha_code) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误或已过期"}))));
    }

    // 查找用户
    let user: Option<(String, String)> = if req.target_type == "email" {
        sqlx::query_as("SELECT id, username FROM users WHERE email = ? AND enabled = 1")
            .bind(&req.target)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
    } else {
        sqlx::query_as("SELECT id, username FROM users WHERE phone = ? AND enabled = 1")
            .bind(&req.target)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
    };

    if user.is_none() {
        // 为安全起见，不提示用户是否存在
        return Ok(Json(json!({
            "code": 200,
            "message": "如果该账号存在，验证码将发送到对应的邮箱/手机"
        })));
    }

    let (user_id, _username) = user.unwrap();

    // 生成6位验证码
    let code: String = (0..6)
        .map(|_| rand::random::<u8>() % 10)
        .map(|n| char::from_digit(n as u32, 10).unwrap())
        .collect();

    // 存储重置码（有效期10分钟）
    let reset_key = format!("{}:{}", req.target_type, req.target);
    state.login_security.store_reset_code(reset_key, user_id.clone(), code.clone());

    // 加载通知设置
    let settings = crate::api::notification::load_notification_settings(&state).await;

    // 发送验证码
    if req.target_type == "email" {
        if !settings.email_enabled {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "邮箱通知未启用"}))));
        }
        let subject = "YaoList 密码重置验证码";
        let body = format!(r#"
            <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
                <h2 style="color: #333;">密码重置</h2>
                <p>您正在重置密码，验证码是：</p>
                <div style="background: #f5f5f5; padding: 20px; border-radius: 8px; text-align: center; margin: 20px 0;">
                    <span style="font-size: 32px; font-weight: bold; letter-spacing: 8px;">{}</span>
                </div>
                <p style="color: #999;">验证码有效期为10分钟，请勿泄露给他人。</p>
            </div>
        "#, code);
        
        if let Err(e) = crate::api::notification::send_smtp_email(&settings, &req.target, subject, &body).await {
            tracing::error!("Failed to send email: {}", e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "发送验证码失败"}))));
        }
    } else {
        if !settings.sms_enabled {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "短信通知未启用"}))));
        }
        // 使用融合认证接口，验证码由阿里云生成
        match crate::api::notification::send_aliyun_sms(&settings, &req.target, "").await {
            Ok(sms_code) => {
                // 用阿里云返回的验证码更新存储
                let reset_key = format!("{}:{}", req.target_type, req.target);
                state.login_security.store_reset_code(reset_key, user_id.clone(), sms_code);
            }
            Err(e) => {
                tracing::error!("Failed to send SMS: {}", e);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "发送验证码失败"}))));
            }
        }
    }

    Ok(Json(json!({
        "code": 200,
        "message": "验证码已发送"
    })))
}

/// POST /api/auth/reset-password - 重置密码
pub async fn reset_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 验证密码长度
    if req.new_password.len() < 6 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "密码长度至少6位"}))));
    }

    // 验证重置码
    let reset_key = format!("{}:{}", req.target_type, req.target);
    let user_id = state.login_security.verify_reset_code(&reset_key, &req.verification_code)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误或已过期"}))))?;

    // 更新密码并关闭2FA（找回密码后自动关闭2FA）
    let password_hash = bcrypt::hash(&req.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    let now = Utc::now().to_rfc3339();

    sqlx::query("UPDATE users SET password_hash = ?, two_factor_enabled = 0, two_factor_secret = NULL, updated_at = ? WHERE id = ?")
        .bind(&password_hash)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新密码失败"}))))?;

    Ok(Json(json!({
        "code": 200,
        "message": "密码重置成功，两步验证已自动关闭"
    })))
}

/// POST /api/auth/change-password - 修改密码（已登录用户）
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user: Option<(String, String)> = sqlx::query_as(
        "SELECT u.id, u.password_hash FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id, password_hash) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;

    let valid = bcrypt::verify(&req.current_password, &password_hash)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if !valid {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "当前密码错误"}))));
    }

    if req.new_password.len() < 6 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "新密码长度至少6位"}))));
    }

    let new_hash = bcrypt::hash(&req.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    let now = Utc::now().to_rfc3339();

    sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "更新密码失败"}))))?;

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
        "message": "密码修改成功",
        "logout": true
    })))
}