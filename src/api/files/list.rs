use std::sync::Arc;
use std::collections::HashMap;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::api::file_resolver::{MountInfo, get_all_mounts, get_matching_mounts, get_first_mount};
use yaolist_backend::utils::{fix_and_clean_path, should_hide_file};

use super::{
    FsListReq, get_virtual_files_by_path,
    get_user_context, join_user_path, get_nearest_password_meta, can_access_password,
    get_nearest_meta, is_hide_apply, get_readme, get_header, can_write,
    get_user_permissions,
};

#[derive(Debug, Deserialize)]
pub struct AdminListReq {
    pub path: Option<String>,
}

/// POST /api/fs/list - 列出目录内容
pub async fn fs_list(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsListReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path.unwrap_or_default());
    let password = req.password.clone().unwrap_or_default();
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 检查是否有读取权限（游客组禁用时无权限）
    if !perms.read_files {
        return Ok(Json(json!({
            "code": 403,
            "message": "guest_disabled"
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
    
    tracing::debug!("fs_list 用户根路径: {}, 请求路径: {}, 实际路径: {}", 
        user_ctx.root_path, req_path, path);
    
    // 获取最近的有密码的元信息（只需验证这一个密码）
    let password_meta = get_nearest_password_meta(&state, &path).await;
    
    // 检查密码访问权限（"我的附庸的附庸不是我的附庸"）
    if !can_access_password(password_meta.as_ref(), &path, &password) {
        return Ok(Json(json!({
            "code": 403,
            "message": "password is incorrect or you have no permission"
        })));
    }
    
    // 获取最近的元信息用于其他属性（readme/header/hide等）
    let meta = get_nearest_meta(&state, &path).await;
    
    // 获取隐藏规则
    let hide_patterns = meta.as_ref()
        .filter(|m| is_hide_apply(&m.path, &path, m.h_sub))
        .and_then(|m| m.hide.clone())
        .unwrap_or_default();
    
    // 分页参数：默认10/页，最高100/页
    let page = req.page.unwrap_or(1).max(1);
    let per_page = req.per_page.unwrap_or(10).clamp(1, 100);
    
    // 获取所有存储挂载点（使用file_resolver）
    let mounts = get_all_mounts(&state).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // 获取所有匹配的驱动（支持别名：多个驱动挂载同一路径）
    let matching_mounts = get_matching_mounts(&path, &mounts);
    
    if !matching_mounts.is_empty() {
        // 计算实际路径（所有驱动共用同一挂载路径）
        let mount_path = fix_and_clean_path(&matching_mounts[0].mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        tracing::debug!("Matched {} drivers, mount point: {}, actual path: {}", 
            matching_mounts.len(), mount_path, actual_path);
        
        // 从所有驱动获取文件列表并合并
        let mut all_files: HashMap<String, Value> = HashMap::new();
        let mut last_error: Option<String> = None;
        let mut has_success = false;
        
        for mount in &matching_mounts {
            if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
                match driver.list(&actual_path).await {
                    Ok(files) => {
                        has_success = true;
                        for f in files {
                            // 过滤隐藏文件
                            if !perms.show_hidden_files && should_hide_file(&f.name, &hide_patterns) {
                                continue;
                            }
                            
                            let file_json = json!({
                                "name": f.name,
                                "size": f.size,
                                "is_dir": f.is_dir,
                                "modified": f.modified.clone().unwrap_or_default(),
                                "created": ""
                            });
                            
                            // 同名文件只保留第一个（按order排序，优先级高的先处理）
                            // 不同名文件全部保留
                            all_files.entry(f.name.clone()).or_insert(file_json);
                        }
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        tracing::error!("Driver {} list failed: {}", mount.id, error_msg);
                        // 记录驱动运行时错误
                        state.storage_manager.set_driver_error(&mount.id, error_msg.clone()).await;
                        last_error = Some(error_msg);
                    }
                }
            }
        }
        
        // 如果所有驱动都失败了，返回简单错误信息（不暴露详细信息）
        if !has_success && last_error.is_some() {
            return Ok(Json(json!({
                "code": 500,
                "message": "存储驱动故障，请联系管理员",
                "data": null,
                "error_type": "DRIVER_ERROR"
            })));
        }
        
        let mut content: Vec<Value> = all_files.into_values().collect();
        
        // 合并虚拟目录
        let virtual_files = get_virtual_files_by_path(&path, &mounts);
        let existing_names: std::collections::HashSet<String> = content.iter()
            .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect();
        for vf in virtual_files {
            if let Some(name) = vf.get("name").and_then(|n| n.as_str()) {
                if !existing_names.contains(name) && (perms.show_hidden_files || !should_hide_file(name, &hide_patterns)) {
                    content.push(vf);
                }
            }
        }
        
        // 统计文件夹和文件数量
        let folder_count = content.iter().filter(|f| f.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false)).count();
        let file_count = content.len() - folder_count;
        
        // 排序处理：目录始终在前，然后按指定字段排序
        let sort_by = req.sort_by.as_deref().unwrap_or("name");
        let sort_order = req.sort_order.as_deref().unwrap_or("asc");
        let is_desc = sort_order == "desc";
        
        content.sort_by(|a, b| {
            let a_is_dir = a.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            let b_is_dir = b.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
            
            // 目录始终在前
            if a_is_dir && !b_is_dir {
                return std::cmp::Ordering::Less;
            }
            if !a_is_dir && b_is_dir {
                return std::cmp::Ordering::Greater;
            }
            
            let cmp = match sort_by {
                "modified" => {
                    let a_mod = a.get("modified").and_then(|v| v.as_str()).unwrap_or("");
                    let b_mod = b.get("modified").and_then(|v| v.as_str()).unwrap_or("");
                    a_mod.cmp(b_mod)
                }
                "size" => {
                    let a_size = a.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
                    let b_size = b.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
                    a_size.cmp(&b_size)
                }
                _ => {
                    let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    a_name.cmp(&b_name)
                }
            };
            
            if is_desc { cmp.reverse() } else { cmp }
        });
        
        // 返回所有文件名用于全选
        let all_names: Vec<String> = content.iter()
            .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect();
        
        // 分页处理
        let total = content.len();
        let start = ((page - 1) * per_page) as usize;
        let end = (start + per_page as usize).min(total);
        let paginated_content = if start < total {
            content[start..end].to_vec()
        } else {
            vec![]
        };
        
        // 获取元信息内容
        let readme = get_readme(meta.as_ref(), &path);
        let header = get_header(meta.as_ref(), &path);
        let write = perms.is_admin || perms.create_upload || can_write(meta.as_ref(), &path);
        
        return Ok(Json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "content": paginated_content,
                "total": total,
                "folder_count": folder_count,
                "file_count": file_count,
                "page": page,
                "per_page": per_page,
                "readme": readme,
                "header": header,
                "write": write,
                "provider": "Mixed",
                "all_names": all_names
            }
        })));
    }
    
    // 没有找到匹配的存储，显示虚拟目录
    let virtual_files = get_virtual_files_by_path(&path, &mounts);
    
    // 过滤隐藏的虚拟目录（有 show_hidden_files 权限的用户可以看到）
    let virtual_files: Vec<Value> = virtual_files.into_iter()
        .filter(|vf| {
            if let Some(name) = vf.get("name").and_then(|n| n.as_str()) {
                perms.show_hidden_files || !should_hide_file(name, &hide_patterns)
            } else {
                true
            }
        })
        .collect();
    
    if virtual_files.is_empty() && path != "/" {
        return Ok(Json(json!({
            "code": 404,
            "message": format!("路径不存在: {}", path),
            "data": null
        })));
    }
    
    // 统计文件夹和文件数量
    let folder_count = virtual_files.iter().filter(|f| f.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false)).count();
    let file_count = virtual_files.len() - folder_count;
    
    // 虚拟目录也需要分页
    let total = virtual_files.len();
    let start = ((page - 1) * per_page) as usize;
    let end = (start + per_page as usize).min(total);
    let paginated_content = if start < total {
        virtual_files[start..end].to_vec()
    } else {
        vec![]
    };
    
    // 获取元信息内容
    let readme = get_readme(meta.as_ref(), &path);
    let header = get_header(meta.as_ref(), &path);
    let write = perms.is_admin || perms.create_upload || can_write(meta.as_ref(), &path);
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "content": paginated_content,
            "total": total,
            "folder_count": folder_count,
            "file_count": file_count,
            "page": page,
            "per_page": per_page,
            "readme": readme,
            "header": header,
            "write": write,
            "provider": "Virtual"
        }
    })))
}


/// POST /api/admin/fs/list - 管理后台专用目录列表（不受密码/隐藏限制）
/// 仅管理员可访问
pub async fn admin_fs_list(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<AdminListReq>,
) -> Result<Json<Value>, StatusCode> {
    let path = fix_and_clean_path(&req.path.unwrap_or_default());
    
    // 验证管理员权限
    let perms = get_user_permissions(&state, &cookies).await;
    if !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "需要管理员权限"
        })));
    }
    
    // 从数据库获取所有存储挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // 构建挂载点列表
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
    
    // 尝试找到最长匹配的存储
    if let Some(mount) = get_first_mount(&path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            match driver.list(&actual_path).await {
                Ok(files) => {
                    let virtual_files = get_virtual_files_by_path(&path, &mounts);
                    
                    // 不过滤隐藏文件
                    let mut content: Vec<Value> = files.iter()
                        .map(|f| {
                            json!({
                                "name": f.name,
                                "size": f.size,
                                "is_dir": f.is_dir,
                                "modified": f.modified.clone().unwrap_or_default(),
                            })
                        }).collect();
                    
                    // 合并虚拟目录
                    let existing_names: std::collections::HashSet<String> = content.iter()
                        .filter_map(|f| f.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                        .collect();
                    for vf in virtual_files {
                        if let Some(name) = vf.get("name").and_then(|n| n.as_str()) {
                            if !existing_names.contains(name) {
                                content.push(vf);
                            }
                        }
                    }
                    
                    return Ok(Json(json!({
                        "code": 200,
                        "message": "success",
                        "data": {
                            "content": content
                        }
                    })));
                }
                Err(e) => {
                    return Ok(Json(json!({
                        "code": 500,
                        "message": format!("列出文件失败: {}", e)
                    })));
                }
            }
        }
    }
    
    // 没有找到匹配的存储，显示虚拟目录
    let virtual_files = get_virtual_files_by_path(&path, &mounts);
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "content": virtual_files
        }
    })))
}

/// POST /api/fs/get - 获取文件/目录信息
pub async fn fs_get(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsListReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path.unwrap_or_default());
    let password = req.password.clone().unwrap_or_default();
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
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
    
    tracing::debug!("fs_get 用户根路径: {}, 请求路径: {}, 实际路径: {}", 
        user_ctx.root_path, req_path, path);
    
    // 检查是否有读取权限（游客组禁用时无权限）
    if !perms.read_files {
        return Ok(Json(json!({
            "code": 403,
            "message": "guest_disabled"
        })));
    }
    
    // 获取最近的有密码的元信息（只需验证这一个密码）
    let password_meta = get_nearest_password_meta(&state, &path).await;
    
    // 检查密码访问权限（"我的附庸的附庸不是我的附庸"）
    if !can_access_password(password_meta.as_ref(), &path, &password) {
        return Ok(Json(json!({
            "code": 403,
            "message": "password is incorrect or you have no permission"
        })));
    }
    
    // 获取最近的元信息用于其他属性（readme/header/hide等）
    let meta = get_nearest_meta(&state, &path).await;
    
    // 获取隐藏规则
    let hide_patterns = meta.as_ref()
        .filter(|m| is_hide_apply(&m.path, &path, m.h_sub))
        .and_then(|m| m.hide.clone())
        .unwrap_or_default();
    
    // 检查文件是否被隐藏（没有 show_hidden_files 权限时）
    let filename = path.split('/').last().unwrap_or("");
    if !perms.show_hidden_files && should_hide_file(filename, &hide_patterns) {
        return Ok(Json(json!({
            "code": 404,
            "message": "文件不存在"
        })));
    }
    
    // 分别处理目录和文件获取元信息内容
    let readme = get_readme(meta.as_ref(), &path);
    let header = get_header(meta.as_ref(), &path);
    
    // 根路径一定是目录
    if path == "/" {
        return Ok(Json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "name": "/",
                "size": 0,
                "is_dir": true,
                "modified": "",
                "created": "",
                "readme": readme,
                "header": header,
                "provider": "Virtual"
            }
        })));
    }
    
    // 从数据库获取所有存储挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // 构建挂载点列表
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
    
    // 检查路径是否是某个挂载点本身
    for mount in &mounts {
        if fix_and_clean_path(&mount.mount_path) == path {
            return Ok(Json(json!({
                "code": 200,
                "message": "success",
                "data": {
                    "name": path.split('/').last().unwrap_or(""),
                    "size": 0,
                    "is_dir": true,
                    "modified": "",
                    "created": "",
                    "readme": readme,
                    "header": header,
                    "provider": "Local"
                }
            })));
        }
    }
    
    // 检查是否是虚拟目录（挂载点的父路径）
    let virtual_dirs = get_virtual_files_by_path(&path, &mounts);
    if !virtual_dirs.is_empty() {
        return Ok(Json(json!({
            "code": 200,
            "message": "success",
            "data": {
                "name": path.split('/').last().unwrap_or(""),
                "size": 0,
                "is_dir": true,
                "modified": "",
                "created": "",
                "readme": readme,
                "header": header,
                "provider": "Virtual"
            }
        })));
    }
    
    // 获取父目录路径和文件名
    let parent_path = path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let parent_path = if parent_path.is_empty() { "/" } else { parent_path };
    let filename = path.split('/').last().unwrap_or("");
    
    // 获取所有匹配的驱动（支持别名：多个驱动挂载同一路径）
    let matching_mounts = get_matching_mounts(&parent_path, &mounts);
    
    // 在所有驱动中查找文件
    for mount in &matching_mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_parent = if parent_path.len() > mount_path.len() {
            fix_and_clean_path(&parent_path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        // 获取驱动并列出父目录
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            match driver.list(&actual_parent).await {
                Ok(files) => {
                    // 在文件列表中查找目标文件
                    for file in files {
                        if file.name == filename {
                            return Ok(Json(json!({
                                "code": 200,
                                "message": "success",
                                "data": {
                                    "name": file.name,
                                    "size": file.size,
                                    "is_dir": file.is_dir,
                                    "modified": file.modified.unwrap_or_default(),
                                    "created": "",
                                    "readme": readme,
                                    "header": header,
                                    "provider": "Local"
                                }
                            })));
                        }
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    tracing::error!("Failed to get file info (driver={}): {}", mount.id, error_msg);
                    // 记录驱动运行时错误
                    state.storage_manager.set_driver_error(&mount.id, error_msg.clone()).await;
                    // 返回简单错误信息（不暴露详细信息）
                    return Ok(Json(json!({
                        "code": 500,
                        "message": "存储驱动故障，请联系管理员",
                        "data": null,
                        "error_type": "DRIVER_ERROR"
                    })));
                }
            }
        }
    }
    
    // 未找到文件
    Ok(Json(json!({
        "code": 404,
        "message": format!("文件不存在: {}", path),
        "data": null,
        "error_type": "FILE_NOT_FOUND"
    })))
}
