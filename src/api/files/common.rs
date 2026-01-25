use std::collections::HashMap;
use tokio::sync::RwLock;
use chrono::Utc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::models::{Meta, UserPermissions};
use crate::auth::SESSION_COOKIE_NAME;
use crate::api::file_resolver::UserContext;
use yaolist_backend::utils::{fix_and_clean_path, is_sub_path};

/// 生成安全的随机令牌
pub fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// 获取最近的有密码的元信息（向上查找，找到有密码的就停止）
/// "我的附庸的附庸不是我的附庸" - 只需验证最近的密码
pub async fn get_nearest_password_meta(state: &AppState, path: &str) -> Option<Meta> {
    let path = fix_and_clean_path(path);
    
    // 先尝试精确匹配
    if let Ok(Some(meta)) = sqlx::query_as::<_, Meta>(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas WHERE path = ?"
    )
    .bind(&path)
    .fetch_optional(&state.db)
    .await {
        // 如果当前路径有密码，直接返回
        if meta.password.is_some() && !meta.password.as_ref().unwrap().is_empty() {
            tracing::debug!("get_nearest_password_meta: 找到有密码的元信息 path={}", path);
            return Some(meta);
        }
    }
    
    // 递归向上查找父目录
    if path == "/" {
        tracing::debug!("get_nearest_password_meta: 到达根目录，未找到有密码的元信息");
        return None;
    }
    
    let parent = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    let parent = if parent.is_empty() { "/".to_string() } else { parent };
    
    tracing::debug!("get_nearest_password_meta: 向上查找 {} -> {}", path, parent);
    Box::pin(get_nearest_password_meta(state, &parent)).await
}

/// 获取最近的元信息（向上查找父目录，用于获取其他属性如readme/header）
pub async fn get_nearest_meta(state: &AppState, path: &str) -> Option<Meta> {
    let path = fix_and_clean_path(path);
    
    // 先尝试精确匹配
    if let Ok(Some(meta)) = sqlx::query_as::<_, Meta>(
        "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas WHERE path = ?"
    )
    .bind(&path)
    .fetch_optional(&state.db)
    .await {
        tracing::debug!("get_nearest_meta: 精确匹配到 path={}", path);
        return Some(meta);
    }
    
    // 递归向上查找父目录
    if path == "/" {
        tracing::debug!("get_nearest_meta: 到达根目录，未找到元信息");
        return None;
    }
    
    let parent = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    let parent = if parent.is_empty() { "/".to_string() } else { parent };
    
    tracing::debug!("get_nearest_meta: 向上查找 {} -> {}", path, parent);
    Box::pin(get_nearest_meta(state, &parent)).await
}

/// 检查路径是否匹配元信息的应用范围
pub fn is_meta_apply(meta_path: &str, req_path: &str, apply_sub: bool) -> bool {
    let meta_path = fix_and_clean_path(meta_path);
    let req_path = fix_and_clean_path(req_path);
    
    if meta_path == req_path {
        return true;
    }
    
    if apply_sub && is_sub_path(&meta_path, &req_path) {
        return true;
    }
    
    false
}

/// 检查隐藏规则是否应用到指定路径
/// 隐藏规则始终应用到该目录下的直接文件，h_sub 控制是否应用到子目录的文件
pub fn is_hide_apply(meta_path: &str, req_path: &str, h_sub: bool) -> bool {
    let meta_path = fix_and_clean_path(meta_path);
    let req_path = fix_and_clean_path(req_path);
    
    // 相同路径
    if meta_path == req_path {
        return true;
    }
    
    // 检查是否是子路径
    if !is_sub_path(&meta_path, &req_path) {
        return false;
    }
    
    // 计算相对路径深度
    let relative = req_path.strip_prefix(&meta_path).unwrap_or(&req_path);
    let relative = relative.trim_start_matches('/');
    let depth = relative.matches('/').count();
    
    // depth == 0 表示直接子文件/目录，始终应用
    // depth > 0 表示子目录中的文件，需要 h_sub 为 true
    depth == 0 || h_sub
}


/// 检查密码是否应用到指定路径时间比较密码（防止时间攻击）
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        // 即使长度不同也要执行比较以保持恒定时间
        let _ = a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        return false;
    }
    let result = a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    result == 0
}

/// 检查是否可以访问路径（密码验证）
/// password_meta: 最近的有密码的元信息（只需验证这一个）
/// "我的附庸的附庸不是我的附庸" - 嵌套密码时只验证最近的
pub fn can_access_password(password_meta: Option<&Meta>, req_path: &str, password: &str) -> bool {
    let meta = match password_meta {
        Some(m) => m,
        None => {
            tracing::debug!("can_access: 没有有密码的元信息，允许访问");
            return true;
        }
    };
    
    tracing::debug!("can_access: 找到最近的有密码元信息 path={}, p_sub={}", meta.path, meta.p_sub);
    
    // 检查密码是否应用到当前路径
    if !is_meta_apply(&meta.path, req_path, meta.p_sub) {
        tracing::debug!("can_access: 密码不应用到当前路径 meta_path={}, req_path={}, p_sub={}", 
            meta.path, req_path, meta.p_sub);
        return true;
    }
    
    // 验证密码（使用恒定时间比较）
    let result = meta.password.as_ref().map(|p| constant_time_compare(p, password)).unwrap_or(false);
    tracing::debug!("can_access: 密码验证结果={}, 输入密码长度={}", result, password.len());
    result
}

/// 获取 readme 内容
pub fn get_readme(meta: Option<&Meta>, req_path: &str) -> String {
    match meta {
        Some(m) if is_meta_apply(&m.path, req_path, m.r_sub) => {
            m.readme.clone().unwrap_or_default()
        }
        _ => String::new()
    }
}

/// 获取 header 内容
pub fn get_header(meta: Option<&Meta>, req_path: &str) -> String {
    match meta {
        Some(m) if is_meta_apply(&m.path, req_path, m.header_sub) => {
            m.header.clone().unwrap_or_default()
        }
        _ => String::new()
    }
}

/// 检查是否有写入权限
pub fn can_write(meta: Option<&Meta>, req_path: &str) -> bool {
    match meta {
        Some(m) if m.write && is_meta_apply(&m.path, req_path, m.w_sub) => true,
        _ => false
    }
}

/// 获取当前用户ID（users.id 是 TEXT 类型）
/// 如果用户未登录，返回游客的用户ID
pub async fn get_user_id(state: &AppState, cookies: &Cookies) -> Option<String> {
    // 尝试从session获取登录用户ID
    if let Some(session_cookie) = cookies.get(SESSION_COOKIE_NAME) {
        let session_id = session_cookie.value().to_string();
        let result: Option<(String,)> = sqlx::query_as(
            "SELECT u.id FROM users u 
             JOIN sessions s ON u.id = s.user_id 
             WHERE s.id = ? AND s.expires_at > datetime('now')"
        )
        .bind(&session_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        
        if let Some((id,)) = result {
            return Some(id);
        }
    }
    
    // 未登录时返回游客用户ID
    let guest_id: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM users WHERE username = 'guest' AND enabled = 1"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    guest_id.map(|(id,)| id)
}

/// 获取游客组权限（未登录用户）
pub async fn get_guest_permissions(state: &AppState) -> UserPermissions {
    // 首先检查游客用户是否启用
    let guest_enabled: Option<(bool,)> = sqlx::query_as(
        "SELECT enabled FROM users WHERE username = 'guest'"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    // 如果游客用户未启用，返回默认权限（read_files=false会触发guest_disabled）
    if let Some((enabled,)) = guest_enabled {
        if !enabled {
            tracing::info!("游客用户已禁用");
            return UserPermissions::default();
        }
    }
    
    // 直接查找"游客组"的权限
    let result = sqlx::query_as::<_, UserPermissions>(
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
    .await;
    
    match &result {
        Ok(Some(perms)) => {
            tracing::info!("游客权限: read_files={}", perms.read_files);
        }
        Ok(None) => {
            tracing::warn!("Guest group not found");
        }
        Err(e) => {
            tracing::error!("Guest permission query failed: {:?}", e);
        }
    }
    
    result.ok().flatten().unwrap_or_default()
}


/// 将用户请求路径与用户根路径结合（防止路径穿越攻击）
pub fn join_user_path(base_path: &str, req_path: &str) -> Result<String, String> {
    // 检测是否有相对路径攻击
    let has_relative = req_path.contains("..");
    let clean_req = fix_and_clean_path(req_path);
    
    // 如果清理后仍然包含..说明路径穿越攻击
    if has_relative && clean_req.contains("..") {
        return Err("路径不合法".to_string());
    }
    
    let clean_base = fix_and_clean_path(base_path);
    
    // 如果根路径是/，直接返回清理后的请求路径
    if clean_base == "/" {
        return Ok(clean_req);
    }
    
    // 拼接路径
    let full_path = if clean_req == "/" {
        clean_base.clone()
    } else {
        format!("{}{}", clean_base.trim_end_matches('/'), clean_req)
    };
    
    Ok(fix_and_clean_path(&full_path))
}

/// 获取当前用户上下文（权限+根路径）
/// 如果用户没有设置根路径，则使用用户组的根路径
pub async fn get_user_context(state: &AppState, cookies: &Cookies) -> UserContext {
    let session_id = match cookies.get(SESSION_COOKIE_NAME) {
        Some(c) => c.value().to_string(),
        None => {
            // 未登录时使用游客权限和根路径
            let guest_perms = get_guest_permissions(state).await;
            let guest_root = get_guest_root_path(state).await;
            return UserContext {
                permissions: guest_perms,
                root_path: guest_root,
                is_guest: true,
            };
        }
    };
    
    // 查询用户权限、用户根路径和用户组根路径
    // 如果用户没有设置根路径(NULL或空)，则使用用户组的根路径
    let result = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, bool, bool, bool, bool, Option<String>, Option<String>)>(
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
            MAX(g.extract_files) as extract_files,
            u.root_path as user_root_path,
            MAX(g.root_path) as group_root_path
        FROM users u
        INNER JOIN sessions s ON u.id = s.user_id
        INNER JOIN user_group_members ugm ON u.id = ugm.user_id
        INNER JOIN user_groups g ON CAST(g.id AS TEXT) = ugm.group_id
        WHERE s.id = ? AND s.expires_at > datetime('now')
        GROUP BY u.id"#
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    match result {
        Some((read_files, create_upload, rename_files, move_files, copy_files, 
              delete_files, allow_direct_link, allow_share, is_admin, show_hidden_files, 
              extract_files, user_root_path, group_root_path)) => {
            // 优先使用用户根路径，如果没有则使用用户组根路径
            let root_path = user_root_path
                .filter(|p| !p.is_empty() && p != "/")
                .or(group_root_path)
                .unwrap_or_else(|| "/".to_string());
            
            UserContext {
                permissions: UserPermissions {
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
                    extract_files,
                },
                root_path,
                is_guest: false,
            }
        },
        None => {
            // session无效，使用游客权限
            let guest_perms = get_guest_permissions(state).await;
            let guest_root = get_guest_root_path(state).await;
            UserContext {
                permissions: guest_perms,
                root_path: guest_root,
                is_guest: true,
            }
        }
    }
}

/// 获取游客用户的根路径（优先从 guest 用户获取，其次从游客组获取）
pub async fn get_guest_root_path(state: &AppState) -> String {
    // 优先从 guest 用户获取根路径
    let user_root = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT root_path FROM users WHERE username = 'guest' AND enabled = 1 LIMIT 1"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    if let Some((Some(root),)) = user_root {
        if !root.is_empty() && root != "/" {
            tracing::debug!("游客根路径(用户): {}", root);
            return root;
        }
    }
    
    // 其次从游客组获取根路径
    let group_root = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT root_path FROM user_groups WHERE name = '游客组' LIMIT 1"
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    let root = group_root.and_then(|(r,)| r).unwrap_or_else(|| "/".to_string());
    tracing::debug!("游客根路径(组): {}", root);
    root
}

/// 获取当前用户权限
pub async fn get_user_permissions(state: &AppState, cookies: &Cookies) -> UserPermissions {
    let session_id = match cookies.get(SESSION_COOKIE_NAME) {
        Some(c) => {
            tracing::info!("有session cookie: {}", c.value());
            c.value().to_string()
        },
        // 未登录时使用游客权限
        None => {
            tracing::info!("无session cookie，使用游客权限");
            return get_guest_permissions(state).await;
        }
    };
    
    // 已登录用户的权限
    let perms = sqlx::query_as::<_, UserPermissions>(
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
        FROM users u
        INNER JOIN sessions s ON u.id = s.user_id
        INNER JOIN user_group_members ugm ON u.id = ugm.user_id
        INNER JOIN user_groups g ON CAST(g.id AS TEXT) = ugm.group_id
        WHERE s.id = ? AND s.expires_at > datetime('now')"#
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    // 如果查询失败（session无效），使用游客权限
    match perms {
        Some(p) => {
            tracing::info!("已登录用户权限: read_files={}, create_upload={}, delete_files={}, is_admin={}", 
                p.read_files, p.create_upload, p.delete_files, p.is_admin);
            p
        },
        None => {
            tracing::warn!("用户session有效但未找到权限数据，使用游客权限");
            get_guest_permissions(state).await
        }
    }
}

// 下载令牌存储（内存缓存，SQLite持久化直链）
lazy_static::lazy_static! {
    pub static ref DOWNLOAD_TOKENS: RwLock<HashMap<String, DownloadToken>> = RwLock::new(HashMap::new());
}

#[derive(Clone)]
pub struct DownloadToken {
    pub path: String,
    pub driver_id: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub can_direct_link: bool,
    pub file_size: Option<u64>,
    pub user_id: Option<String>,  // 用于流量统计
}

/// Create a download token and return the token string / 创建下载令牌并返回令牌字符串
pub async fn create_download_token(
    path: String,
    driver_id: String,
    expires_at: chrono::DateTime<Utc>,
    can_direct_link: bool,
    file_size: Option<u64>,
) -> String {
    create_download_token_with_user(path, driver_id, expires_at, can_direct_link, file_size, None).await
}

/// Create a download token with user_id for traffic stats / 创建带用户ID的下载令牌（用于流量统计）
pub async fn create_download_token_with_user(
    path: String,
    driver_id: String,
    expires_at: chrono::DateTime<Utc>,
    can_direct_link: bool,
    file_size: Option<u64>,
    user_id: Option<String>,
) -> String {
    let token = generate_token();
    let download_token = DownloadToken {
        path,
        driver_id,
        expires_at,
        can_direct_link,
        file_size,
        user_id,
    };
    
    let mut tokens = DOWNLOAD_TOKENS.write().await;
    tokens.insert(token.clone(), download_token);
    token
}
