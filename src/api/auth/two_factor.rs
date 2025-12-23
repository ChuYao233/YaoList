use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;
use chrono::Utc;
use totp_rs::{Algorithm, TOTP, Secret};

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use super::types::*;

pub async fn setup_2fa(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.username, u.two_factor_secret FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id, username, _existing_secret) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;

    // 生成新的密钥
    let secret = Secret::Raw(rand::random::<[u8; 20]>().to_vec());
    let secret_base32 = secret.to_encoded().to_string();
    
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_bytes().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "生成密钥失败"}))))?,
        Some("YaoList".to_string()),
        username.clone(),
    ).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建TOTP失败"}))))?;

    // 生成二维码URL
    let qr_url = totp.get_url();
    
    // 生成二维码图片(base64)
    let qr_code = totp.get_qr_base64()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "生成二维码失败"}))))?;

    // 临时存储密钥（未激活状态）
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET two_factor_secret = ?, updated_at = ? WHERE id = ?")
        .bind(&secret_base32)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "保存密钥失败"}))))?;

    Ok(Json(json!({
        "code": 200,
        "secret": secret_base32,
        "qr_code": format!("data:image/png;base64,{}", qr_code),
        "qr_url": qr_url
    })))
}

/// POST /api/auth/2fa/enable - 验证并启用2FA
pub async fn enable_2fa(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<Enable2FARequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.two_factor_secret FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id, secret) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;
    let secret = secret.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "请先设置2FA"}))))?;

    // 验证TOTP码
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret).to_bytes()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "密钥解析失败"}))))?,
        Some("YaoList".to_string()),
        "user".to_string(),
    ).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建TOTP失败"}))))?;

    if !totp.check_current(&req.totp_code).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "验证失败"}))))? {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误"}))));
    }

    // 启用2FA
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET two_factor_enabled = 1, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "启用2FA失败"}))))?;

    Ok(Json(json!({
        "code": 200,
        "message": "两步验证已启用"
    })))
}

/// POST /api/auth/2fa/disable - 禁用2FA
pub async fn disable_2fa(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<Verify2FARequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?
        .value()
        .to_string();

    let user: Option<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT u.id, u.two_factor_secret, u.two_factor_enabled FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let (user_id, secret, enabled) = user.ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"}))))?;
    
    if !enabled {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "2FA未启用"}))));
    }

    let secret = secret.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "2FA配置错误"}))))?;

    // 验证TOTP码
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret).to_bytes()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "密钥解析失败"}))))?,
        Some("YaoList".to_string()),
        "user".to_string(),
    ).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建TOTP失败"}))))?;

    if !totp.check_current(&req.totp_code).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "验证失败"}))))? {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误"}))));
    }

    // 禁用2FA
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE users SET two_factor_enabled = 0, two_factor_secret = NULL, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "禁用2FA失败"}))))?;

    Ok(Json(json!({
        "code": 200,
        "message": "两步验证已关闭"
    })))
}


