use std::sync::Arc;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::api::file_resolver::{MountInfo, get_first_mount};
use yaolist_backend::utils::fix_and_clean_path;

use super::{get_user_context, join_user_path};

#[derive(Debug, Deserialize)]
pub struct FsMkdirReq {
    pub path: String,
}

/// POST /api/fs/mkdir - 创建目录
pub async fn fs_mkdir(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsMkdirReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.create_upload && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有创建目录的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    // 获取挂载点和实际路径
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    if let Some(mount) = get_first_mount(&path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            tracing::debug!("fs_mkdir: 调用driver.create_dir, actual_path={}", actual_path);
            match driver.create_dir(&actual_path).await {
                Ok(_) => {
                    tracing::debug!("fs_mkdir: Directory created successfully");
                    return Ok(Json(json!({
                        "code": 200,
                        "message": "success"
                    })));
                }
                Err(e) => {
                    tracing::error!("fs_mkdir: Failed to create directory: {}", e);
                    return Ok(Json(json!({
                        "code": 500,
                        "message": format!("创建目录失败: {}", e)
                    })));
                }
            }
        } else {
            tracing::error!("fs_mkdir: 找不到驱动 {}", mount.id);
        }
    } else {
        tracing::error!("fs_mkdir: Mount point not found, path={}", path);
    }
    
    Ok(Json(json!({
        "code": 404,
        "message": "路径不存在"
    })))
}

#[derive(Debug, Deserialize)]
pub struct FsWriteReq {
    pub path: String,
    pub content: String,
}

/// POST /api/fs/write - 创建/写入文件（Core层控制，调用driver原语）
pub async fn fs_write(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsWriteReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.create_upload && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有创建文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    // 获取挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    if let Some(mount) = get_first_mount(&path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            // Core 层控制写入：获取 writer 原语，写入内容
            use tokio::io::AsyncWriteExt;
            
            let content_bytes = req.content.as_bytes();
            let mut writer = driver.open_writer(&actual_path, Some(content_bytes.len() as u64), None).await
                .map_err(|e| {
                    tracing::error!("Failed to open writer: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            writer.write_all(content_bytes).await
                .map_err(|e| {
                    tracing::error!("Failed to write file: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            writer.shutdown().await
                .map_err(|e| {
                    tracing::error!("Failed to close writer: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            return Ok(Json(json!({
                "code": 200,
                "message": "success"
            })));
        }
    }
    
    Ok(Json(json!({
        "code": 404,
        "message": "路径不存在"
    })))
}

#[derive(Debug, Deserialize)]
pub struct FsRemoveReq {
    pub path: String,
}

/// POST /api/fs/remove - 删除文件或目录
pub async fn fs_remove(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsRemoveReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.delete_files && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有删除文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    if let Some(mount) = get_first_mount(&path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            return Ok(Json(json!({
                "code": 403,
                "message": "不能删除根目录"
            })));
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            let driver = driver;
            match driver.delete(&actual_path).await {
                Ok(_) => {
                    return Ok(Json(json!({
                        "code": 200,
                        "message": "success"
                    })));
                }
                Err(e) => {
                    return Ok(Json(json!({
                        "code": 500,
                        "message": format!("删除失败: {}", e)
                    })));
                }
            }
        }
    }
    
    Ok(Json(json!({
        "code": 404,
        "message": "路径不存在"
    })))
}

#[derive(Debug, Deserialize)]
pub struct FsRenameReq {
    pub path: String,
    pub name: String,
}

/// POST /api/fs/rename - 重命名文件或目录
pub async fn fs_rename(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsRenameReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.rename_files && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有重命名文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    if let Some(mount) = get_first_mount(&path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            return Ok(Json(json!({
                "code": 403,
                "message": "不能重命名根目录"
            })));
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            let driver = driver;
            match driver.rename(&actual_path, &req.name).await {
                Ok(_) => {
                    return Ok(Json(json!({
                        "code": 200,
                        "message": "success"
                    })));
                }
                Err(e) => {
                    return Ok(Json(json!({
                        "code": 500,
                        "message": format!("重命名失败: {}", e)
                    })));
                }
            }
        }
    }
    
    Ok(Json(json!({
        "code": 404,
        "message": "路径不存在"
    })))
}
