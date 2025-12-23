use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;
use chrono::Utc;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;

/// 验证管理员权限
async fn require_admin(state: &AppState, cookies: &Cookies) -> Result<String, (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let admin_info: Option<(String, bool)> = sqlx::query_as(
        "SELECT u.id, u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    match admin_info {
        Some((user_id, true)) => Ok(user_id),
        Some((_, false)) => Err((StatusCode::FORBIDDEN, Json(json!({"error": "需要管理员权限"})))),
        None => Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "会话无效"})))),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupData {
    pub version: String,
    pub created_at: String,
    pub site_settings: Vec<SiteSettingBackup>,
    pub users: Vec<UserBackup>,
    pub user_groups: Vec<UserGroupBackup>,
    pub user_group_members: Vec<UserGroupMemberBackup>,
    pub drivers: Vec<DriverBackup>,
    pub mounts: Vec<MountBackup>,
    pub metas: Vec<MetaBackup>,
    pub shares: Vec<ShareBackup>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SiteSettingBackup {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserBackup {
    pub id: String,
    pub unique_id: String,
    pub username: String,
    pub password_hash: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub root_path: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
    pub two_factor_enabled: bool,
    pub two_factor_secret: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserGroupBackup {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_admin: bool,
    pub allow_direct_link: bool,
    pub allow_share: bool,
    pub show_hidden_files: bool,
    pub no_password_access: bool,
    pub add_offline_download: bool,
    pub create_upload: bool,
    pub rename_files: bool,
    pub move_files: bool,
    pub copy_files: bool,
    pub delete_files: bool,
    pub read_files: bool,
    pub read_compressed: bool,
    pub extract_files: bool,
    pub webdav_enabled: bool,
    pub ftp_enabled: bool,
    pub root_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserGroupMemberBackup {
    pub user_id: String,
    pub group_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DriverBackup {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub config: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MountBackup {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub mount_path: String,
    pub config: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaBackup {
    pub id: i64,
    pub path: String,
    pub password: Option<String>,
    pub hide: Option<String>,
    pub readme: Option<String>,
    pub header: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShareBackup {
    pub id: i64,
    pub path: String,
    pub password: Option<String>,
    pub expire_at: Option<String>,
    pub created_at: String,
}

/// GET /api/admin/backup - 导出所有设置
pub async fn export_backup(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    // 导出站点设置
    let site_settings: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM site_settings"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("导出站点设置失败: {}", e)}))))?;
    
    let site_settings: Vec<SiteSettingBackup> = site_settings.into_iter()
        .map(|(key, value)| SiteSettingBackup { key, value })
        .collect();
    
    // 导出用户（包括密码hash）
    let users: Vec<UserBackup> = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<String>, bool, bool, bool, Option<String>, String, String)>(
        "SELECT id, unique_id, username, password_hash, email, phone, root_path, is_admin, enabled, two_factor_enabled, two_factor_secret, created_at, updated_at FROM users"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("导出用户失败: {}", e)}))))?
    .into_iter()
    .map(|(id, unique_id, username, password_hash, email, phone, root_path, is_admin, enabled, two_factor_enabled, two_factor_secret, created_at, updated_at)| {
        UserBackup {
            id, unique_id, username, password_hash, email, phone, root_path, is_admin, enabled, two_factor_enabled, two_factor_secret, created_at, updated_at
        }
    })
    .collect();
    
    // 导出用户组 - 使用models中的UserGroup结构体
    let user_groups: Vec<UserGroupBackup> = sqlx::query_as::<_, crate::models::UserGroup>(
        "SELECT * FROM user_groups"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("导出用户组失败: {}", e)}))))?
    .into_iter()
    .map(|g| UserGroupBackup {
        id: g.id, name: g.name, description: g.description, is_admin: g.is_admin,
        allow_direct_link: g.allow_direct_link, allow_share: g.allow_share,
        show_hidden_files: g.show_hidden_files, no_password_access: g.no_password_access,
        add_offline_download: g.add_offline_download, create_upload: g.create_upload,
        rename_files: g.rename_files, move_files: g.move_files, copy_files: g.copy_files,
        delete_files: g.delete_files, read_files: g.read_files, read_compressed: g.read_compressed,
        extract_files: g.extract_files, webdav_enabled: g.webdav_enabled, ftp_enabled: g.ftp_enabled,
        root_path: g.root_path, created_at: g.created_at, updated_at: g.updated_at
    })
    .collect();
    
    // 导出用户组成员关系
    let user_group_members: Vec<(String, String)> = sqlx::query_as(
        "SELECT user_id, group_id FROM user_group_members"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("导出用户组成员失败: {}", e)}))))?;
    
    let user_group_members: Vec<UserGroupMemberBackup> = user_group_members.into_iter()
        .map(|(user_id, group_id)| UserGroupMemberBackup { user_id, group_id })
        .collect();
    
    // 导出驱动
    let drivers: Vec<DriverBackup> = sqlx::query_as::<_, (String, String, String, bool, Option<String>, String, String)>(
        "SELECT name, version, description, enabled, config, created_at, updated_at FROM drivers"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("导出驱动失败: {}", e)}))))?
    .into_iter()
    .map(|(name, version, description, enabled, config, created_at, updated_at)| {
        DriverBackup {
            name, version, description, enabled, config, created_at, updated_at
        }
    })
    .collect();
    
    // 导出挂载点
    let mounts: Vec<MountBackup> = sqlx::query_as::<_, (String, String, String, String, String, bool, String, String)>(
        "SELECT id, name, driver, mount_path, config, enabled, created_at, updated_at FROM mounts"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, name, driver, mount_path, config, enabled, created_at, updated_at)| {
        MountBackup {
            id, name, driver, mount_path, config, enabled, created_at, updated_at
        }
    })
    .collect();
    
    // 导出元信息
    let metas: Vec<MetaBackup> = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, Option<String>, Option<String>, String, String)>(
        "SELECT id, path, password, hide, readme, header, created_at, updated_at FROM metas"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, path, password, hide, readme, header, created_at, updated_at)| {
        MetaBackup {
            id, path, password, hide, readme, header, created_at, updated_at
        }
    })
    .collect();
    
    // 导出分享
    let shares: Vec<ShareBackup> = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, String)>(
        "SELECT id, path, password, expire_at, created_at FROM shares"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, path, password, expire_at, created_at)| {
        ShareBackup {
            id, path, password, expire_at, created_at
        }
    })
    .collect();
    
    let backup = BackupData {
        version: "1.0".to_string(),
        created_at: Utc::now().to_rfc3339(),
        site_settings,
        users,
        user_groups,
        user_group_members,
        drivers,
        mounts,
        metas,
        shares,
    };
    
    Ok(Json(json!({
        "code": 200,
        "data": backup
    })))
}

#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub data: BackupData,
    pub override_existing: bool,
}

/// POST /api/admin/restore - 导入所有设置
pub async fn import_backup(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<RestoreRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let admin_id = require_admin(&state, &cookies).await?;
    
    let mut results = Vec::new();
    let now = Utc::now().to_rfc3339();
    
    // 获取当前admin的密码hash（导入后保持不变）
    let admin_password_hash: Option<(String,)> = sqlx::query_as(
        "SELECT password_hash FROM users WHERE id = ?"
    )
    .bind(&admin_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    
    // 恢复站点设置
    for setting in &req.data.site_settings {
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO site_settings (key, value, updated_at) VALUES (?, ?, ?) 
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
            )
            .bind(&setting.key)
            .bind(&setting.value)
            .bind(&now)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO site_settings (key, value, updated_at) VALUES (?, ?, ?)"
            )
            .bind(&setting.key)
            .bind(&setting.value)
            .bind(&now)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "setting", "key": setting.key, "status": "success"})),
            Err(e) => results.push(json!({"type": "setting", "key": setting.key, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复用户组
    for group in &req.data.user_groups {
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO user_groups (id, name, description, is_admin, allow_direct_link, allow_share, show_hidden_files, no_password_access, add_offline_download, create_upload, rename_files, move_files, copy_files, delete_files, read_files, read_compressed, extract_files, webdav_enabled, ftp_enabled, root_path, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, is_admin = excluded.is_admin, allow_direct_link = excluded.allow_direct_link, allow_share = excluded.allow_share, show_hidden_files = excluded.show_hidden_files, no_password_access = excluded.no_password_access, add_offline_download = excluded.add_offline_download, create_upload = excluded.create_upload, rename_files = excluded.rename_files, move_files = excluded.move_files, copy_files = excluded.copy_files, delete_files = excluded.delete_files, read_files = excluded.read_files, read_compressed = excluded.read_compressed, extract_files = excluded.extract_files, webdav_enabled = excluded.webdav_enabled, ftp_enabled = excluded.ftp_enabled, root_path = excluded.root_path, updated_at = excluded.updated_at"
            )
            .bind(group.id)
            .bind(&group.name)
            .bind(&group.description)
            .bind(group.is_admin)
            .bind(group.allow_direct_link)
            .bind(group.allow_share)
            .bind(group.show_hidden_files)
            .bind(group.no_password_access)
            .bind(group.add_offline_download)
            .bind(group.create_upload)
            .bind(group.rename_files)
            .bind(group.move_files)
            .bind(group.copy_files)
            .bind(group.delete_files)
            .bind(group.read_files)
            .bind(group.read_compressed)
            .bind(group.extract_files)
            .bind(group.webdav_enabled)
            .bind(group.ftp_enabled)
            .bind(&group.root_path)
            .bind(&group.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO user_groups (id, name, description, is_admin, allow_direct_link, allow_share, show_hidden_files, no_password_access, add_offline_download, create_upload, rename_files, move_files, copy_files, delete_files, read_files, read_compressed, extract_files, webdav_enabled, ftp_enabled, root_path, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(group.id)
            .bind(&group.name)
            .bind(&group.description)
            .bind(group.is_admin)
            .bind(group.allow_direct_link)
            .bind(group.allow_share)
            .bind(group.show_hidden_files)
            .bind(group.no_password_access)
            .bind(group.add_offline_download)
            .bind(group.create_upload)
            .bind(group.rename_files)
            .bind(group.move_files)
            .bind(group.copy_files)
            .bind(group.delete_files)
            .bind(group.read_files)
            .bind(group.read_compressed)
            .bind(group.extract_files)
            .bind(group.webdav_enabled)
            .bind(group.ftp_enabled)
            .bind(&group.root_path)
            .bind(&group.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "user_group", "name": group.name, "status": "success"})),
            Err(e) => results.push(json!({"type": "user_group", "name": group.name, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复用户（admin密码保持不变）
    for user in &req.data.users {
        // 如果是当前admin用户，跳过密码更新
        let password_hash = if user.id == admin_id {
            admin_password_hash.as_ref().map(|(h,)| h.clone()).unwrap_or_else(|| user.password_hash.clone())
        } else {
            user.password_hash.clone()
        };
        
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO users (id, unique_id, username, password_hash, email, phone, root_path, is_admin, enabled, two_factor_enabled, two_factor_secret, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET unique_id = excluded.unique_id, username = excluded.username, password_hash = excluded.password_hash, email = excluded.email, phone = excluded.phone, root_path = excluded.root_path, is_admin = excluded.is_admin, enabled = excluded.enabled, two_factor_enabled = excluded.two_factor_enabled, two_factor_secret = excluded.two_factor_secret, updated_at = excluded.updated_at"
            )
            .bind(&user.id)
            .bind(&user.unique_id)
            .bind(&user.username)
            .bind(&password_hash)
            .bind(&user.email)
            .bind(&user.phone)
            .bind(&user.root_path)
            .bind(user.is_admin)
            .bind(user.enabled)
            .bind(user.two_factor_enabled)
            .bind(&user.two_factor_secret)
            .bind(&user.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO users (id, unique_id, username, password_hash, email, phone, root_path, is_admin, enabled, two_factor_enabled, two_factor_secret, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&user.id)
            .bind(&user.unique_id)
            .bind(&user.username)
            .bind(&password_hash)
            .bind(&user.email)
            .bind(&user.phone)
            .bind(&user.root_path)
            .bind(user.is_admin)
            .bind(user.enabled)
            .bind(user.two_factor_enabled)
            .bind(&user.two_factor_secret)
            .bind(&user.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "user", "username": user.username, "status": "success"})),
            Err(e) => results.push(json!({"type": "user", "username": user.username, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复用户组成员关系
    for member in &req.data.user_group_members {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO user_group_members (user_id, group_id, created_at) VALUES (?, ?, ?)"
        )
        .bind(&member.user_id)
        .bind(&member.group_id)
        .bind(&now)
        .execute(&state.db)
        .await;
        
        match result {
            Ok(_) => results.push(json!({"type": "user_group_member", "user_id": member.user_id, "group_id": member.group_id, "status": "success"})),
            Err(e) => results.push(json!({"type": "user_group_member", "user_id": member.user_id, "group_id": member.group_id, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复驱动
    for driver in &req.data.drivers {
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO drivers (name, version, description, enabled, config, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(name) DO UPDATE SET version = excluded.version, description = excluded.description, enabled = excluded.enabled, config = excluded.config, updated_at = excluded.updated_at"
            )
            .bind(&driver.name)
            .bind(&driver.version)
            .bind(&driver.description)
            .bind(driver.enabled)
            .bind(&driver.config)
            .bind(&driver.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO drivers (name, version, description, enabled, config, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&driver.name)
            .bind(&driver.version)
            .bind(&driver.description)
            .bind(driver.enabled)
            .bind(&driver.config)
            .bind(&driver.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "driver", "name": driver.name, "status": "success"})),
            Err(e) => results.push(json!({"type": "driver", "name": driver.name, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复元信息
    for meta in &req.data.metas {
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO metas (id, path, password, hide, readme, header, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET path = excluded.path, password = excluded.password, hide = excluded.hide, readme = excluded.readme, header = excluded.header, updated_at = excluded.updated_at"
            )
            .bind(meta.id)
            .bind(&meta.path)
            .bind(&meta.password)
            .bind(&meta.hide)
            .bind(&meta.readme)
            .bind(&meta.header)
            .bind(&meta.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO metas (id, path, password, hide, readme, header, created_at, updated_at) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(meta.id)
            .bind(&meta.path)
            .bind(&meta.password)
            .bind(&meta.hide)
            .bind(&meta.readme)
            .bind(&meta.header)
            .bind(&meta.created_at)
            .bind(&now)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "meta", "path": meta.path, "status": "success"})),
            Err(e) => results.push(json!({"type": "meta", "path": meta.path, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 恢复分享
    for share in &req.data.shares {
        let result = if req.override_existing {
            sqlx::query(
                "INSERT INTO shares (id, path, password, expire_at, created_at) 
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET path = excluded.path, password = excluded.password, expire_at = excluded.expire_at"
            )
            .bind(share.id)
            .bind(&share.path)
            .bind(&share.password)
            .bind(&share.expire_at)
            .bind(&share.created_at)
            .execute(&state.db)
            .await
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO shares (id, path, password, expire_at, created_at) 
                 VALUES (?, ?, ?, ?, ?)"
            )
            .bind(share.id)
            .bind(&share.path)
            .bind(&share.password)
            .bind(&share.expire_at)
            .bind(&share.created_at)
            .execute(&state.db)
            .await
        };
        
        match result {
            Ok(_) => results.push(json!({"type": "share", "path": share.path, "status": "success"})),
            Err(e) => results.push(json!({"type": "share", "path": share.path, "status": "error", "message": e.to_string()})),
        }
    }
    
    // 统计结果
    let success_count = results.iter().filter(|r| r["status"] == "success").count();
    let error_count = results.iter().filter(|r| r["status"] == "error").count();
    
    Ok(Json(json!({
        "code": 200,
        "message": format!("恢复完成：成功 {} 项，失败 {} 项", success_count, error_count),
        "data": {
            "results": results,
            "success_count": success_count,
            "error_count": error_count
        }
    })))
}
