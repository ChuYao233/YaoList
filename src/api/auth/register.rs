use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use chrono::Utc;

use crate::state::AppState;
use super::types::*;

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 检查是否允许注册
    let allow_registration: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'allow_registration'"
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    let registration_enabled = allow_registration
        .map(|(v,)| v == "true")
        .unwrap_or(false);
    
    if !registration_enabled {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "注册功能已关闭"}))));
    }

    // 验证目标（邮箱或手机）
    let target = match req.verification_type.as_str() {
        "email" => {
            req.email.clone().ok_or_else(|| {
                (StatusCode::BAD_REQUEST, Json(json!({"error": "请提供邮箱地址"})))
            })?
        }
        "sms" => {
            req.phone.clone().ok_or_else(|| {
                (StatusCode::BAD_REQUEST, Json(json!({"error": "请提供手机号码"})))
            })?
        }
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "不支持的验证方式"}))));
        }
    };

    // 验证验证码
    let now = Utc::now().to_rfc3339();
    let code_result = sqlx::query_as::<_, (String, String, i32)>(
        "SELECT id, code, used FROM verification_codes WHERE target = ? AND type = ? AND expires_at > ? ORDER BY created_at DESC LIMIT 1"
    )
    .bind(&target)
    .bind(&req.verification_type)
    .bind(&now)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    match code_result {
        Some((code_id, stored_code, used)) => {
            if used == 1 {
                return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码已使用"}))));
            }
            if stored_code != req.verification_code {
                return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码错误"}))));
            }
            // 标记验证码已使用
            sqlx::query("UPDATE verification_codes SET used = 1 WHERE id = ?")
                .bind(&code_id)
                .execute(&state.db)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
        }
        None => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "验证码不存在或已过期"}))));
        }
    }

    // 检查用户名是否已存在
    let existing_user: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

    if existing_user.is_some() {
        return Err((StatusCode::CONFLICT, Json(json!({"error": "用户名已存在"}))));
    }

    // 检查邮箱是否已存在
    if let Some(email) = &req.email {
        let existing_email: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM users WHERE email = ?"
        )
        .bind(email)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

        if existing_email.is_some() {
            return Err((StatusCode::CONFLICT, Json(json!({"error": "邮箱已被注册"}))));
        }
    }

    // 检查手机是否已存在
    if let Some(phone) = &req.phone {
        let existing_phone: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM users WHERE phone = ?"
        )
        .bind(phone)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;

        if existing_phone.is_some() {
            return Err((StatusCode::CONFLICT, Json(json!({"error": "手机号已被注册"}))));
        }
    }

    // 获取默认用户组（从站点设置或使用默认值）
    let default_group_id: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'default_user_group'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let group_id = if let Some((gid,)) = default_group_id {
        gid
    } else {
        // 查找默认组
        let group: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM user_groups WHERE name = '默认组' OR name = 'default' LIMIT 1"
        )
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        group.map(|(id,)| id.to_string()).unwrap_or_else(|| "1".to_string())
    };

    // 获取用户组的root_path
    let group_root_path: Option<String> = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT root_path FROM user_groups WHERE id = ?"
    )
    .bind(&group_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .and_then(|(rp,)| rp);

    // 创建用户 - 获取下一个整数ID
    let max_id: Option<(i64,)> = sqlx::query_as("SELECT MAX(CAST(id AS INTEGER)) FROM users WHERE id GLOB '[0-9]*'")
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    let next_id = max_id.map(|(id,)| id + 1).unwrap_or(1);
    let user_id = next_id.to_string();
    let unique_id = uuid::Uuid::new_v4().to_string();
    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO users (id, unique_id, username, password_hash, email, phone, is_admin, enabled, root_path, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, ?, ?)"
    )
    .bind(&user_id)
    .bind(&unique_id)
    .bind(&req.username)
    .bind(&password_hash)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&group_root_path)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create user: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建用户失败"})))
    })?;

    // 添加用户到默认用户组
    sqlx::query(
        "INSERT INTO user_group_members (user_id, group_id, created_at) VALUES (?, ?, ?)"
    )
    .bind(&user_id)
    .bind(&group_id)
    .bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| {
        tracing::error!("Failed to add user to group: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "创建用户失败"})))
    })?;

    Ok(Json(json!({
        "code": 200,
        "message": "注册成功",
        "user_id": user_id
    })))
}

/// POST /api/auth/check-unique - 检查用户名/邮箱/手机号是否可用
pub async fn check_unique(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CheckUniqueRequest>,
) -> Json<Value> {
    let mut errors: Vec<String> = vec![];

    // 检查用户名
    if let Some(username) = &req.username {
        if username.len() < 3 {
            errors.push("用户名至少3个字符".to_string());
        } else if username.len() > 20 {
            errors.push("用户名最多20个字符".to_string());
        } else {
            // 检查用户名是否已存在（同时检查username、email、phone字段）
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM users WHERE username = ? OR email = ? OR phone = ?"
            )
            .bind(username)
            .bind(username)
            .bind(username)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

            if existing.is_some() {
                errors.push("用户名已被使用".to_string());
            }
        }
    }

    // 检查邮箱
    if let Some(email) = &req.email {
        if !email.contains('@') {
            errors.push("邮箱格式不正确".to_string());
        } else {
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM users WHERE email = ? OR username = ? OR phone = ?"
            )
            .bind(email)
            .bind(email)
            .bind(email)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

            if existing.is_some() {
                errors.push("邮箱已被使用".to_string());
            }
        }
    }

    // 检查手机号
    if let Some(phone) = &req.phone {
        if phone.len() < 10 {
            errors.push("手机号格式不正确".to_string());
        } else {
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM users WHERE phone = ? OR username = ? OR email = ?"
            )
            .bind(phone)
            .bind(phone)
            .bind(phone)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

            if existing.is_some() {
                errors.push("手机号已被使用".to_string());
            }
        }
    }

    if errors.is_empty() {
        Json(json!({
            "code": 200,
            "available": true,
            "message": "可用"
        }))
    } else {
        Json(json!({
            "code": 200,
            "available": false,
            "errors": errors
        }))
    }
}

/// GET /api/auth/registration-config - 获取注册配置（是否允许注册，支持哪些验证方式）
pub async fn get_registration_config(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    // 检查是否允许注册
    let allow_registration: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'allow_registration'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let registration_enabled = allow_registration
        .map(|(v,)| v == "true")
        .unwrap_or(false);

    // 检查邮箱是否启用
    let email_enabled: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'notification_email_enabled'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let email_available = email_enabled
        .map(|(v,)| v == "true")
        .unwrap_or(false);

    // 检查短信是否启用
    let sms_enabled: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM site_settings WHERE key = 'notification_sms_enabled'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let sms_available = sms_enabled
        .map(|(v,)| v == "true")
        .unwrap_or(false);

    Json(json!({
        "code": 200,
        "data": {
            "registration_enabled": registration_enabled,
            "email_available": email_available,
            "sms_available": sms_available
        }
    }))
}
