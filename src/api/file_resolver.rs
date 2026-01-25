//! 文件路径解析与驱动选择器
//! 
//! 提供通用的文件操作辅助功能：
//! - 路径解析（用户根路径+路径穿越检查）
//! - 驱动查找（多驱动合并支持）
//! - 负载均衡选择（302优先+轮询）

use std::collections::HashMap;
use tokio::sync::RwLock;
use once_cell::sync::Lazy;
use serde_json::Value;
use tower_cookies::Cookies;
use yaolist_backend::utils::fix_and_clean_path;

use crate::state::AppState;
use crate::models::UserPermissions;
use crate::auth::SESSION_COOKIE_NAME;

/// 基于路径的轮询计数器（每个路径独立计数，避免前端重复请求干扰）
static PATH_COUNTERS: Lazy<RwLock<HashMap<String, u64>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// 获取路径的下一个计数值
async fn get_next_counter_for_path(path: &str) -> u64 {
    let mut counters = PATH_COUNTERS.write().await;
    let counter = counters.entry(path.to_string()).or_insert(0);
    let current = *counter;
    *counter = counter.wrapping_add(1);
    current
}

/// 挂载点信息
#[derive(Debug, Clone)]
pub struct MountInfo {
    pub id: String,
    pub mount_path: String,
    pub order: i32,
}

/// 驱动匹配结果（包含驱动和302能力）
#[derive(Debug, Clone)]
pub struct DriverMatch {
    pub mount: MountInfo,
    pub can_direct_link: bool,
    pub actual_path: String,
}

/// 用户上下文
#[derive(Debug, Clone)]
pub struct UserContext {
    pub permissions: UserPermissions,
    pub root_path: String,
    pub is_guest: bool,
}

impl Default for UserContext {
    fn default() -> Self {
        Self {
            permissions: UserPermissions::default(),
            root_path: "/".to_string(),
            is_guest: true,
        }
    }
}

/// 获取用户上下文（权限+根路径）
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
        },
    }
}

/// 获取游客权限
async fn get_guest_permissions(state: &AppState) -> UserPermissions {
    let result = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, bool, bool, bool, bool)>(
        r#"SELECT 
            read_files, create_upload, rename_files, move_files, copy_files,
            delete_files, allow_direct_link, allow_share, is_admin, show_hidden_files, extract_files
        FROM user_groups WHERE name = '游客组'"#
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    match result {
        Some((read_files, create_upload, rename_files, move_files, copy_files,
              delete_files, allow_direct_link, allow_share, is_admin, show_hidden_files, extract_files)) => {
            UserPermissions {
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
            }
        },
        None => UserPermissions::default(),
    }
}

/// 获取游客用户的根路径（优先从 guest 用户获取，其次从游客组获取）
async fn get_guest_root_path(state: &AppState) -> String {
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

/// 将用户请求路径与用户根路径结合（防止路径穿越）
pub fn join_user_path(user_root: &str, req_path: &str) -> Result<String, &'static str> {
    let user_root = fix_and_clean_path(user_root);
    let req_path = fix_and_clean_path(req_path);
    
    // 如果用户根路径是 /，直接返回请求路径
    if user_root == "/" {
        return Ok(req_path);
    }
    
    // 组合路径
    let combined = if req_path == "/" {
        user_root.clone()
    } else {
        format!("{}{}", user_root.trim_end_matches('/'), req_path)
    };
    
    let combined = fix_and_clean_path(&combined);
    
    // 确保组合后的路径仍在用户根路径内
    if !combined.starts_with(&user_root) && combined != user_root {
        return Err("路径越权");
    }
    
    Ok(combined)
}

/// 从数据库获取所有启用的挂载点
pub async fn get_all_mounts(state: &AppState) -> Result<Vec<MountInfo>, sqlx::Error> {
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        let order = config.get("order").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order,
        })
    }).collect();
    
    Ok(mounts)
}

/// 检查路径是否是子路径
fn is_sub_path(parent: &str, child: &str) -> bool {
    let parent = fix_and_clean_path(parent);
    let child = fix_and_clean_path(child);
    
    if parent == "/" {
        return true;
    }
    
    child == parent || child.starts_with(&format!("{}/", parent))
}

/// 获取所有匹配该路径的驱动（用于别名/负载均衡）
/// 只返回相同挂载路径的驱动（最长匹配）
pub fn get_matching_mounts<'a>(path: &str, mounts: &'a [MountInfo]) -> Vec<&'a MountInfo> {
    let path = fix_and_clean_path(path);
    let mut best_len = 0;
    
    // 先找到最长匹配长度
    for mount in mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        if is_sub_path(&mount_path, &path) && mount_path.len() > best_len {
            best_len = mount_path.len();
        }
    }
    
    // 返回所有具有最长匹配长度的驱动（按order排序）
    let mut result: Vec<&MountInfo> = mounts.iter()
        .filter(|mount| {
            let mount_path = fix_and_clean_path(&mount.mount_path);
            is_sub_path(&mount_path, &path) && mount_path.len() == best_len
        })
        .collect();
    
    result.sort_by_key(|m| m.order);
    result
}

/// 获取第一个匹配的驱动（写操作用）
pub fn get_first_mount<'a>(path: &str, mounts: &'a [MountInfo]) -> Option<&'a MountInfo> {
    get_matching_mounts(path, mounts).into_iter().next()
}

/// 查找包含指定文件的所有驱动（带302能力标记）
pub async fn find_file_drivers(
    state: &AppState,
    path: &str,
    mounts: &[MountInfo],
) -> Vec<DriverMatch> {
    let path = fix_and_clean_path(path);
    let parent_path = path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let parent_path = if parent_path.is_empty() { "/" } else { parent_path };
    let filename = path.split('/').last().unwrap_or("");
    
    let matching_mounts = get_matching_mounts(&parent_path, mounts);
    let mut results = Vec::new();
    
    for mount in matching_mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_parent = if parent_path.len() > mount_path.len() {
            fix_and_clean_path(&parent_path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            // 检查文件是否存在
            if let Ok(files) = driver.list(&actual_parent).await {
                if files.iter().any(|f| f.name == filename) {
                    let actual_path = format!("{}/{}", actual_parent.trim_end_matches('/'), filename);
                    let actual_path = fix_and_clean_path(&actual_path);
                    
                    // 获取驱动的302能力
                    let can_direct_link = driver.capabilities().can_direct_link;
                    
                    results.push(DriverMatch {
                        mount: mount.clone(),
                        can_direct_link,
                        actual_path,
                    });
                }
            }
        }
    }
    
    // 确保结果按order排序，保证顺序稳定
    results.sort_by_key(|r| r.mount.order);
    results
}

/// 多源聚合：默认驱动选择（302优先+轮询）
/// 
/// 策略：
/// 1. 有302能力的驱动优先
/// 2. 多个302驱动之间平均轮询
/// 3. 没有302驱动时，在所有驱动之间轮询
/// 
/// 注意：此函数是同步的，使用传入的计数器值
pub fn select_driver_default_with_counter(drivers: &[DriverMatch], counter: u64) -> Option<&DriverMatch> {
    if drivers.is_empty() {
        return None;
    }
    
    // 分离302驱动和普通驱动
    let redirect_drivers: Vec<_> = drivers.iter().filter(|d| d.can_direct_link).collect();
    
    // 优先使用302驱动
    let candidates: Vec<_> = if !redirect_drivers.is_empty() {
        redirect_drivers
    } else {
        drivers.iter().collect()
    };
    
    if candidates.is_empty() {
        return None;
    }
    
    // 轮询选择
    let index = (counter as usize) % candidates.len();
    
    tracing::debug!("多源聚合轮询: counter={}, candidates={}, index={}, 选择驱动={}",
        counter, candidates.len(), index, candidates[index].mount.id);
    
    Some(candidates[index])
}

/// 计算驱动内部的实际路径
pub fn calculate_internal_path(mount_path: &str, full_path: &str) -> String {
    let mount_path = fix_and_clean_path(mount_path);
    let full_path = fix_and_clean_path(full_path);
    
    if mount_path == "/" {
        return full_path;
    }
    
    if full_path.len() > mount_path.len() {
        fix_and_clean_path(&full_path[mount_path.len()..])
    } else {
        "/".to_string()
    }
}

/// 下载时选中的驱动信息
#[derive(Debug, Clone)]
pub struct SelectedDriver {
    pub driver_id: String,
    pub internal_path: String,
    pub can_direct_link: bool,
}

/// 为下载/预览选择驱动（完整的多源聚合逻辑）
/// 
/// 此函数封装了完整的多源聚合流程：
/// 1. 检查是否有配置的负载均衡组
/// 2. 如果有，使用配置的负载均衡策略（加权轮询/IP哈希/地区分流）
/// 3. 如果没有，使用默认的302优先+轮询策略
/// 4. 返回选中的驱动ID和内部路径
pub async fn select_driver_for_download(
    state: &AppState,
    file_path: &str,
) -> Option<SelectedDriver> {
    select_driver_for_download_with_ip(state, file_path, None).await
}

/// 为下载/预览选择驱动（带客户端IP，用于IP哈希和地区分流）
pub async fn select_driver_for_download_with_ip(
    state: &AppState,
    file_path: &str,
    client_ip: Option<std::net::IpAddr>,
) -> Option<SelectedDriver> {
    // 获取所有挂载点
    let mounts = match get_all_mounts(state).await {
        Ok(m) => m,
        Err(_) => return None,
    };
    
    // 查找包含该文件的所有驱动
    let drivers = find_file_drivers(state, file_path, &mounts).await;
    
    if drivers.is_empty() {
        tracing::debug!("select_driver_for_download: No driver found containing file path={}", file_path);
        return None;
    }
    
    tracing::debug!("select_driver_for_download: Found {} drivers containing file path={}", drivers.len(), file_path);
    
    // 获取挂载路径（使用第一个驱动的挂载路径）
    let _mount_path = &drivers[0].mount.mount_path;
    let file_name = file_path.split('/').last().unwrap_or("");
    
    // 检查是否有配置的负载均衡组（通过挂载路径查找）
    // 首先查找命名组中是否有包含这些驱动的组
    let groups = state.load_balance.get_all_groups().await;
    for group in &groups {
        if group.enabled {
            // 检查组中的驱动是否与当前驱动匹配
            let group_driver_ids: std::collections::HashSet<_> = group.drivers.iter()
                .map(|d| d.driver_id.as_str())
                .collect();
            
            let current_driver_ids: std::collections::HashSet<_> = drivers.iter()
                .map(|d| d.mount.id.as_str())
                .collect();
            
            // 如果有交集，使用这个负载均衡组
            if !group_driver_ids.is_disjoint(&current_driver_ids) {
                tracing::debug!("select_driver_for_download: 使用负载均衡组 {} 模式={:?}", 
                    group.name, group.mode);
                
                // 使用负载均衡管理器选择驱动
                if let Some(selected) = state.load_balance.select_from_group(
                    &group.name, 
                    client_ip, 
                    file_name
                ).await {
                    // 找到对应的DriverMatch获取internal_path
                    if let Some(driver_match) = drivers.iter().find(|d| d.mount.id == selected.driver_id) {
                        tracing::debug!("select_driver_for_download: 负载均衡选择驱动 id={}", selected.driver_id);
                        return Some(SelectedDriver {
                            driver_id: selected.driver_id,
                            internal_path: driver_match.actual_path.clone(),
                            can_direct_link: driver_match.can_direct_link,
                        });
                    }
                }
            }
        }
    }
    
    // 没有配置负载均衡组，使用默认的302优先+轮询策略
    tracing::debug!("select_driver_for_download: 使用默认轮询策略");
    let counter = get_next_counter_for_path(file_path).await;
    let selected = select_driver_default_with_counter(&drivers, counter)?;
    
    tracing::debug!("select_driver_for_download: 选择驱动 id={}, can_direct_link={}, internal_path={}",
        selected.mount.id, selected.can_direct_link, selected.actual_path);
    
    Some(SelectedDriver {
        driver_id: selected.mount.id.clone(),
        internal_path: selected.actual_path.clone(),
        can_direct_link: selected.can_direct_link,
    })
}
