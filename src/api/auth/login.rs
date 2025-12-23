use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_cookies::{Cookies, Cookie};
use chrono::Utc;
use captcha::Captcha;
use captcha::filters::Noise;
use base64::prelude::*;
use totp_rs::{Algorithm, TOTP, Secret};

use crate::state::AppState;
use crate::auth::{SESSION_COOKIE_NAME, create_session};
use crate::models::{User, UserInfo, UserPermissions};
use super::types::*;

/// 生成验证码
pub async fn generate_captcha(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CaptchaResponse>, StatusCode> {
    let mut captcha = Captcha::new();
    captcha
        .add_chars(4)
        .apply_filter(Noise::new(0.2))
        .view(120, 40);
    
    let code = captcha.chars_as_string();
    let png_data = captcha.as_png().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let base64_image = format!("data:image/png;base64,{}", BASE64_STANDARD.encode(&png_data));
    
    let captcha_id = uuid::Uuid::new_v4().to_string();
    state.login_security.store_captcha(captcha_id.clone(), code);
    
    Ok(Json(CaptchaResponse {
        captcha_id,
        captcha_image: base64_image,
    }))
}

/// 检查是否需要验证码（基于IP）
pub async fn check_need_captcha(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Json<Value> {
    let ip = addr.ip().to_string();
    let needs = state.login_security.needs_captcha_by_ip(&ip);
    Json(json!({ "need_captcha": needs }))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    cookies: Cookies,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let ip = addr.ip().to_string();
    
    // 检查IP是否被封禁
    if state.login_security.is_ip_blocked(&ip) {
        return Err((StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "error": "登录失败次数过多，请30分钟后再试",
            "blocked": true
        }))));
    }
    
    // 基于IP检查是否需要验证码（不能被绕过）
    if state.login_security.needs_captcha_by_ip(&ip) {
        match (&req.captcha_id, &req.captcha_code) {
            (Some(id), Some(code)) if !id.is_empty() && !code.is_empty() => {
                if !state.login_security.verify_captcha(id, code) {
                    return Err((StatusCode::BAD_REQUEST, Json(json!({
                        "error": "验证码错误",
                        "need_captcha": true
                    }))));
                }
            }
            _ => {
                return Err((StatusCode::BAD_REQUEST, Json(json!({
                    "error": "请输入验证码",
                    "need_captcha": true
                }))));
            }
        }
    }
    
    // 支持用户名/邮箱/手机号登录
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE (username = ? OR email = ? OR phone = ?) AND enabled = 1"
    )
    .bind(&req.username)
    .bind(&req.username)
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?
    .ok_or_else(|| {
        state.login_security.record_failure(&ip, &req.username);
        (StatusCode::UNAUTHORIZED, Json(json!({
            "error": "账号或密码错误",
            "need_captcha": state.login_security.needs_captcha_by_ip(&ip)
        })))
    })?;

    let valid = bcrypt::verify(&req.password, &user.password_hash)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if !valid {
        state.login_security.record_failure(&ip, &req.username);
        return Err((StatusCode::UNAUTHORIZED, Json(json!({
            "error": "账号或密码错误",
            "need_captcha": state.login_security.needs_captcha_by_ip(&ip)
        }))));
    }
    
    // 检查是否启用了2FA
    if user.two_factor_enabled {
        match &req.totp_code {
            Some(code) if !code.is_empty() => {
                // 验证TOTP码
                let secret = user.two_factor_secret.as_ref()
                    .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "2FA配置错误"}))))?;
                
                let totp = TOTP::new(
                    Algorithm::SHA1,
                    6,
                    1,
                    30,
                    Secret::Encoded(secret.clone()).to_bytes()
                        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "密钥解析失败"}))))?,
                    Some("YaoList".to_string()),
                    user.username.clone(),
                ).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建TOTP失败"}))))?;

                if !totp.check_current(code).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "验证失败"}))))? {
                    return Err((StatusCode::BAD_REQUEST, Json(json!({
                        "error": "两步验证码错误",
                        "need_2fa": true
                    }))));
                }
            }
            _ => {
                // 需要2FA但未提供验证码
                return Err((StatusCode::BAD_REQUEST, Json(json!({
                    "error": "请输入两步验证码",
                    "need_2fa": true
                }))));
            }
        }
    }
    
    // 登录成功，清除失败记录
    state.login_security.clear_failure(&ip, &req.username);

    let session = create_session(&user.id);
    let now = Utc::now().to_rfc3339();
    
    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&session.id)
    .bind(&session.user_id)
    .bind(session.expires_at.to_rfc3339())
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    // 更新用户最后登录时间
    sqlx::query("UPDATE users SET last_login = ? WHERE id = ?")
        .bind(&now)
        .bind(&user.id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    let mut cookie = Cookie::new(SESSION_COOKIE_NAME, session.id);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookies.add(cookie);

    Ok(Json(json!({
        "user": UserInfo {
            id: user.id,
            username: user.username,
            email: user.email,
            is_admin: user.is_admin,
        }
    })))
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, StatusCode> {
    if let Some(cookie) = cookies.get(SESSION_COOKIE_NAME) {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(cookie.value())
            .execute(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    
    // 必须设置相同的 path 才能正确删除 cookie
    let mut removal_cookie = Cookie::new(SESSION_COOKIE_NAME, "");
    removal_cookie.set_path("/");
    cookies.remove(removal_cookie);
    
    Ok(Json(json!({"message": "已退出登录"})))
}

/// 获取当前权限（登录用户返回用户权限，未登录返回游客权限）
/// 这个接口不会返回401，总是返回权限信息
pub async fn permissions(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, StatusCode> {
    // 检查是否有有效session
    if let Some(session_cookie) = cookies.get(SESSION_COOKIE_NAME) {
        // 尝试获取登录用户信息
        let user = sqlx::query_as::<_, User>(
            r#"SELECT u.* FROM users u 
               INNER JOIN sessions s ON u.id = s.user_id 
               WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"#
        )
        .bind(session_cookie.value())
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        
        if let Some(user) = user {
            // 已登录用户，获取用户权限
            let permissions = sqlx::query_as::<_, UserPermissions>(
                r#"SELECT 
                    MAX(g.read_files) as read_files,
                    MAX(g.create_upload) as create_upload,
                    MAX(g.rename_files) as rename_files,
                    MAX(g.move_files) as move_files,
                    MAX(g.copy_files) as copy_files,
                    MAX(g.delete_files) as delete_files,
                    MAX(g.allow_direct_link) as allow_direct_link,
                    MAX(g.allow_share) as allow_share,
                    MAX(g.is_admin) as is_admin,
                    MAX(g.show_hidden_files) as show_hidden_files,
                    MAX(g.extract_files) as extract_files
                FROM user_groups g
                INNER JOIN user_group_members ugm ON CAST(g.id AS TEXT) = ugm.group_id
                WHERE ugm.user_id = ?"#
            )
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            
            return Ok(Json(json!({
                "is_guest": false,
                "user": UserInfo {
                    id: user.id,
                    username: user.username,
                    email: user.email,
                    is_admin: user.is_admin,
                },
                "permissions": permissions
            })));
        }
    }
    
    // 未登录或session无效，返回游客权限
    let (guest_disabled, guest_permissions) = get_guest_permissions(&state).await;
    
    Ok(Json(json!({
        "is_guest": true,
        "guest_disabled": guest_disabled,
        "user": null,
        "permissions": guest_permissions
    })))
}

/// 获取游客组权限，返回 (guest_disabled, permissions)
async fn get_guest_permissions(state: &AppState) -> (bool, UserPermissions) {
    // 首先检查游客用户是否启用
    let guest_enabled: Option<(bool,)> = sqlx::query_as(
        "SELECT enabled FROM users WHERE username = 'guest'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    if let Some((enabled,)) = guest_enabled {
        if !enabled {
            return (true, UserPermissions::default());  // 游客被禁用
        }
    }
    
    // 查找"游客组"的权限
    let permissions = sqlx::query_as::<_, UserPermissions>(
        r#"SELECT 
            read_files,
            create_upload,
            rename_files,
            move_files,
            copy_files,
            delete_files,
            allow_direct_link,
            allow_share,
            is_admin,
            show_hidden_files,
            extract_files
        FROM user_groups
        WHERE name = '游客组'"#
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    
    (false, permissions)
}

