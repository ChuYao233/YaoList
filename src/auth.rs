use axum::http::{HeaderMap, StatusCode};
use sqlx::SqlitePool;
use serde::{Deserialize, Serialize};

// 用户权限结构
#[derive(Debug)]
pub struct UserPermissions {
    pub username: String,
    pub permissions: i32,
    pub user_path: String,
}

// 权限验证结果
pub enum AuthResult {
    Authenticated(UserPermissions),
    Guest(UserPermissions),
    Unauthorized(String),
}

// Session 结构
#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub username: String,
    pub permissions: i32,
    pub user_path: String,
    pub expires_at: i64, // Unix timestamp
}

// 验证用户权限
pub async fn verify_permissions(
    headers: &HeaderMap,
    pool: &SqlitePool,
    required_permission: i32,
) -> Result<UserPermissions, (StatusCode, String)> {
    match get_user_auth(headers, pool).await {
        AuthResult::Authenticated(user_perms) | AuthResult::Guest(user_perms) => {
            // 检查是否有所需权限
            if user_perms.permissions & required_permission != 0 {
                Ok(user_perms)
            } else {
                Err((StatusCode::FORBIDDEN, "没有足够的权限执行此操作".to_string()))
            }
        },
        AuthResult::Unauthorized(msg) => {
            Err((StatusCode::UNAUTHORIZED, msg))
        }
    }
}

// 从Cookie中提取session token
pub fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    headers.get("cookie")
        .and_then(|cookie_header| cookie_header.to_str().ok())
        .and_then(|cookie_str| {
            // 解析Cookie字符串，查找session_token
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some((key, value)) = cookie.split_once('=') {
                    if key.trim() == "session_token" {
                        return Some(value.trim().to_string());
                    }
                }
            }
            None
        })
}

// 验证session token
async fn verify_session_token(token: &str, pool: &SqlitePool) -> Result<Session, String> {
    // 从数据库中查询session
    match sqlx::query_as::<_, (String, i32, String, i64)>(
        "SELECT username, permissions, user_path, expires_at FROM user_sessions WHERE token = ? AND expires_at > ?"
    )
    .bind(token)
    .bind(chrono::Utc::now().timestamp())
    .fetch_optional(pool)
    .await
    {
        Ok(Some((username, permissions, user_path, expires_at))) => {
            // 检查用户是否仍然启用
            match sqlx::query_as::<_, (bool,)>(
                "SELECT enabled FROM users WHERE username = ?"
            )
            .bind(&username)
            .fetch_optional(pool)
            .await
            {
                Ok(Some((enabled,))) if enabled => {
                    Ok(Session {
                        username,
                        permissions,
                        user_path,
                        expires_at,
                    })
                }
                _ => Err("用户已被禁用".to_string())
            }
        }
        Ok(None) => Err("无效的session或已过期".to_string()),
        Err(_) => Err("验证session失败".to_string()),
    }
}

// 获取用户认证信息
async fn get_user_auth(headers: &HeaderMap, pool: &SqlitePool) -> AuthResult {
    // 1. 尝试从Cookie中获取session token
    if let Some(token) = extract_session_token(headers) {
        match verify_session_token(&token, pool).await {
            Ok(session) => {
                return AuthResult::Authenticated(UserPermissions {
                    username: session.username,
                    permissions: session.permissions,
                    user_path: session.user_path,
                });
            }
            Err(msg) => {
                // Session无效，继续尝试游客权限
                println!("Session验证失败: {}", msg);
            }
        }
    }

    // 2. 未登录或session无效，使用游客权限
    get_guest_permissions(pool).await
}

// 获取游客权限
async fn get_guest_permissions(pool: &SqlitePool) -> AuthResult {
    // 查询游客账户信息
    match sqlx::query_as::<_, (i32, bool, String)>(
        "SELECT permissions, enabled, user_path FROM users WHERE username = 'guest'"
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some((permissions, enabled, user_path))) => {
            // 游客账户存在
            if enabled {
                AuthResult::Guest(UserPermissions {
                    username: "guest".to_string(),
                    permissions,
                    user_path,
                })
            } else {
                // 游客账户已禁用，没有任何权限
                AuthResult::Unauthorized("游客访问已禁用".to_string())
            }
        }
        _ => {
            // 游客账户不存在
            AuthResult::Unauthorized("游客账户不存在".to_string())
        }
    }
}

// 创建session
pub async fn create_session(username: &str, pool: &SqlitePool) -> Result<String, String> {
    // 生成随机token
    use rand::Rng;
    let token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();

    // 获取用户信息
    let user = match sqlx::query_as::<_, (i32, String)>(
        "SELECT permissions, user_path FROM users WHERE username = ? AND enabled = true"
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(user)) => user,
        Ok(None) => return Err("用户不存在或已禁用".to_string()),
        Err(_) => return Err("查询用户信息失败".to_string()),
    };

    // 设置过期时间（7天）
    let expires_at = chrono::Utc::now().timestamp() + 7 * 24 * 60 * 60;

    // 删除该用户的旧session
    let _ = sqlx::query(
        "DELETE FROM user_sessions WHERE username = ?"
    )
    .bind(username)
    .execute(pool)
    .await;

    // 插入新session
    match sqlx::query(
        "INSERT INTO user_sessions (token, username, permissions, user_path, expires_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&token)
    .bind(username)
    .bind(user.0)
    .bind(&user.1)
    .bind(expires_at)
    .execute(pool)
    .await
    {
        Ok(_) => Ok(token),
        Err(_) => Err("创建session失败".to_string()),
    }
}

// 删除session（登出）
pub async fn delete_session(token: &str, pool: &SqlitePool) -> Result<(), String> {
    match sqlx::query(
        "DELETE FROM user_sessions WHERE token = ?"
    )
    .bind(token)
    .execute(pool)
    .await
    {
        Ok(_) => Ok(()),
        Err(_) => Err("删除session失败".to_string()),
    }
}

// 删除特定用户的所有session（用户配置被修改时调用）
pub async fn delete_user_sessions(username: &str, pool: &SqlitePool) -> Result<(), String> {
    match sqlx::query(
        "DELETE FROM user_sessions WHERE username = ?"
    )
    .bind(username)
    .execute(pool)
    .await
    {
        Ok(result) => {
            let deleted_count = result.rows_affected();
            if deleted_count > 0 {
                println!("🔄 已清除用户 {} 的 {} 个session", username, deleted_count);
            }
            Ok(())
        },
        Err(_) => Err("删除用户session失败".to_string()),
    }
}

// 检查用户是否为管理员
pub async fn is_admin(headers: &HeaderMap, pool: &SqlitePool) -> bool {
    match get_user_auth(headers, pool).await {
        AuthResult::Authenticated(user_perms) => {
            // 检查是否具有所有权限
            user_perms.permissions == -1 // 所有位都为1
        },
        _ => false,
    }
}

// 获取用户信息（包括游客）
pub async fn get_current_user(headers: &HeaderMap, pool: &SqlitePool) -> Option<UserPermissions> {
    match get_user_auth(headers, pool).await {
        AuthResult::Authenticated(user_perms) | AuthResult::Guest(user_perms) => Some(user_perms),
        AuthResult::Unauthorized(_) => None,
    }
} 