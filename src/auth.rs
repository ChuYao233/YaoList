use axum::http::{HeaderMap, StatusCode};
use sqlx::SqlitePool;

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

// 获取用户认证信息
async fn get_user_auth(headers: &HeaderMap, pool: &SqlitePool) -> AuthResult {
    // 1. 尝试从请求头获取用户名
    if let Some(username) = headers.get("x-username").and_then(|v| v.to_str().ok()) {
        // 查询已登录用户信息
        match get_user_info(username, pool).await {
            Ok((permissions, user_path)) => {
                return AuthResult::Authenticated(UserPermissions {
                    username: username.to_string(),
                    permissions,
                    user_path,
                });
            }
            Err(sqlx::Error::RowNotFound) => {
                // 用户不存在，返回未授权错误
                return AuthResult::Unauthorized(format!("用户 {} 不存在或已禁用", username));
            }
            Err(_) => {
                // 其他错误（如数据库错误），返回通用错误
                return AuthResult::Unauthorized("验证失败，请稍后重试".to_string());
            }
        }
    }

    // 2. 未登录，使用游客权限
    get_guest_permissions(pool).await
}

// 获取游客权限
async fn get_guest_permissions(pool: &SqlitePool) -> AuthResult {
    // 查询游客账户信息
    match sqlx::query!(
        "SELECT permissions, enabled, user_path FROM users WHERE username = 'guest'"
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(guest)) => {
            // 游客账户存在
            if guest.enabled {
                AuthResult::Guest(UserPermissions {
                    username: "guest".to_string(),
                    permissions: guest.permissions as i32,
                    user_path: guest.user_path,
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

// 获取用户信息
async fn get_user_info(username: &str, pool: &SqlitePool) -> Result<(i32, String), sqlx::Error> {
    match sqlx::query!(
        "SELECT permissions, user_path FROM users WHERE username = ? AND enabled = true",
        username
    )
    .fetch_optional(pool)
    .await?
    {
        Some(user) => Ok((user.permissions as i32, user.user_path)),
        None => Err(sqlx::Error::RowNotFound),
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