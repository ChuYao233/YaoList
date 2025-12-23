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
use yaolist_backend::utils::{fix_and_clean_path, resolve_conflict_name, ConflictStrategy};

use super::{get_user_context, join_user_path, get_user_id, get_existing_names};

/// 跨驱动复制：Core 层控制，调用 driver 原语
/// 支持 FTP→Local→OneDrive→夸克 等任意驱动组合
async fn cross_driver_copy(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    dst_path: &str,
) -> anyhow::Result<()> {
    // 检查源是文件还是目录
    let parent_path = src_path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let filename = src_path.split('/').last().unwrap_or("");
    
    let entries = src_driver.list(parent_path).await?;
    let is_dir = entries.iter()
        .find(|e| e.name == filename)
        .map(|e| e.is_dir)
        .unwrap_or(false);
    
    if is_dir {
        // 递归复制目录
        cross_driver_copy_dir(src_driver, dst_driver, src_path, dst_path).await
    } else {
        // 复制单个文件
        cross_driver_copy_file(src_driver, dst_driver, src_path, dst_path).await
    }
}

/// 跨驱动复制单个文件（使用大缓冲区提高速度）
async fn cross_driver_copy_file(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    dst_path: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // 获取文件大小
    let file_name = src_path.split('/').last().unwrap_or(src_path);
    let file_size = src_driver.list(src_path.rsplit_once('/').map(|(p, _)| p).unwrap_or("/"))
        .await
        .ok()
        .and_then(|entries| entries.iter().find(|e| e.name == file_name).map(|e| e.size))
        .unwrap_or(0);
    
    let mut reader = src_driver.open_reader(src_path, None).await?;
    let mut writer = dst_driver.open_writer(dst_path, Some(file_size), None).await?;
    
    // 使用 32MB 缓冲区支持10Gbps高速传输
    let mut buffer = vec![0u8; 32 * 1024 * 1024];
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n]).await?;
    }
    writer.shutdown().await?;
    
    Ok(())
}

/// 跨驱动复制单个文件（带详细状态：下载中/上传中）
async fn cross_driver_copy_file_with_progress(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    dst_path: &str,
    task_manager: &crate::task::TaskManager,
    task_id: &str,
    base_processed_size: u64,
    processed_files: u64,
    total_files: u64,
    total_task_size: u64,
) -> anyhow::Result<()> {
    use std::time::Instant;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // 获取任务控制
    let control = task_manager.get_control(task_id).await;
    let file_name = src_path.split('/').last().unwrap_or(src_path);
    let start_time = Instant::now();
    
    // 获取文件大小
    let file_size = src_driver.list(src_path.rsplit_once('/').map(|(p, _)| p).unwrap_or("/"))
        .await
        .ok()
        .and_then(|entries| entries.iter().find(|e| e.name == file_name).map(|e| e.size))
        .unwrap_or(0);
    
    // 阶段1：下载（从源存储读取）
    task_manager.update_copy_task_progress(
        task_id, "下载中", file_name, 0.0,
        processed_files, total_files, base_processed_size, total_task_size
    ).await;
    
    let mut reader = src_driver.open_reader(src_path, None).await?;
    
    // 阶段2：上传（写入目标存储）
    task_manager.update_copy_task_progress(
        task_id, "上传中", file_name, 0.0,
        processed_files, total_files, base_processed_size, total_task_size
    ).await;
    
    // 传递file_size作为size_hint，驱动需要知道总大小才能正确分片上传
    let mut writer = dst_driver.open_writer(dst_path, Some(file_size), None).await?;
    
    // 使用32MB缓冲区复制
    let mut buffer = vec![0u8; 32 * 1024 * 1024];
    let mut total = 0u64;
    let mut last_update = std::time::Instant::now();
    
    loop {
        // 检查取消
        if let Some(ref ctrl) = control {
            if ctrl.is_cancelled() {
                anyhow::bail!("任务已取消");
            }
            // 检查暂停
            while ctrl.is_paused() {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if ctrl.is_cancelled() {
                    anyhow::bail!("任务已取消");
                }
            }
        }
        
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n]).await?;
        total += n as u64;
        
        // 每100ms更新一次进度
        if last_update.elapsed().as_millis() >= 100 {
            let file_progress = if file_size > 0 { (total as f32 / file_size as f32) * 100.0 } else { 0.0 };
            let current_size = base_processed_size + total;
            task_manager.update_copy_task_progress(
                task_id, "传输中", file_name, file_progress,
                processed_files, total_files, current_size, total_task_size
            ).await;
            last_update = std::time::Instant::now();
        }
    }
    
    // 关键：调用shutdown触发驱动完成上传
    writer.shutdown().await?;
    
    // 输出性能统计
    let elapsed = start_time.elapsed().as_secs_f64();
    if elapsed > 0.0 {
        let speed_mbps = (total as f64 / elapsed) / (1024.0 * 1024.0);
        tracing::info!("File copy completed: {} -> {}, {} MB in {:.2}s, speed: {:.2} MB/s", 
            src_path, dst_path, total / 1024 / 1024, elapsed, speed_mbps);
    }
    
    Ok(())
}

/// 递归计算文件夹大小
async fn calculate_dir_size(
    driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    path: &str,
) -> anyhow::Result<u64> {
    let entries = driver.list(path).await?;
    let mut total = 0u64;
    
    for entry in entries {
        if entry.is_dir {
            let sub_path = format!("{}/{}", path.trim_end_matches('/'), entry.name);
            total += Box::pin(calculate_dir_size(driver, &sub_path)).await?;
        } else {
            total += entry.size;
        }
    }
    
    Ok(total)
}

/// 跨驱动递归复制目录
async fn cross_driver_copy_dir(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    dst_path: &str,
) -> anyhow::Result<()> {
    // 在目标创建目录
    dst_driver.create_dir(dst_path).await?;
    
    // 列出源目录内容
    let entries = src_driver.list(src_path).await?;
    
    for entry in entries {
        let new_src = format!("{}/{}", src_path.trim_end_matches('/'), entry.name);
        let new_dst = format!("{}/{}", dst_path.trim_end_matches('/'), entry.name);
        
        if entry.is_dir {
            // 递归复制子目录
            Box::pin(cross_driver_copy_dir(src_driver, dst_driver, &new_src, &new_dst)).await?;
        } else {
            // 复制文件
            cross_driver_copy_file(src_driver, dst_driver, &new_src, &new_dst).await?;
        }
    }
    
    Ok(())
}

/// 跨驱动递归复制目录（带进度更新）
async fn cross_driver_copy_dir_with_progress(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    dst_path: &str,
    task_manager: &crate::task::TaskManager,
    task_id: &str,
    base_processed_size: &mut u64,
    processed_files: u64,
    total_files: u64,
    total_size: u64,
) -> anyhow::Result<()> {
    // 在目标创建目录
    dst_driver.create_dir(dst_path).await?;
    
    // 列出源目录内容
    let entries = src_driver.list(src_path).await?;
    
    for entry in entries {
        let new_src = format!("{}/{}", src_path.trim_end_matches('/'), entry.name);
        let new_dst = format!("{}/{}", dst_path.trim_end_matches('/'), entry.name);
        
        if entry.is_dir {
            // 递归复制子目录
            Box::pin(cross_driver_copy_dir_with_progress(
                src_driver,
                dst_driver,
                &new_src,
                &new_dst,
                task_manager,
                task_id,
                base_processed_size,
                processed_files,
                total_files,
                total_size,
            )).await?;
        } else {
            // 复制文件并更新进度
            cross_driver_copy_file_with_progress(
                src_driver,
                dst_driver,
                &new_src,
                &new_dst,
                task_manager,
                task_id,
                *base_processed_size,
                processed_files,
                total_files,
                total_size,
            ).await?;
            *base_processed_size += entry.size;
        }
    }
    
    Ok(())
}

/// 生成安全的随机令牌
fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// 生成短一点的签名用于直链
fn generate_sign() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}


#[derive(Debug, Deserialize)]
pub struct FsMoveReq {
    pub src_dir: String,
    pub dst_dir: String,
    pub names: Vec<String>,
    #[serde(default)]
    pub conflict_strategy: Option<String>, // "overwrite", "skip", "auto_rename"
}

/// POST /api/fs/move - 移动文件或目录（创建任务异步执行）
pub async fn fs_move(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsMoveReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_src_dir = fix_and_clean_path(&req.src_dir);
    let req_dst_dir = fix_and_clean_path(&req.dst_dir);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.move_files && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有移动文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let src_dir = match join_user_path(&user_ctx.root_path, &req_src_dir) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    let dst_dir = match join_user_path(&user_ctx.root_path, &req_dst_dir) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    let user_id = get_user_id(&state, &cookies).await;
    let names = req.names.clone();
    
    // 解析冲突策略
    let strategy_str = req.conflict_strategy.clone().unwrap_or_else(|| "auto_rename".to_string());
    let strategy = match strategy_str.as_str() {
        "overwrite" => ConflictStrategy::Overwrite,
        "skip" => ConflictStrategy::Skip,
        _ => ConflictStrategy::AutoRename,
    };
    
    // 创建移动任务（保存执行上下文用于断点续传）
    let task_name = if names.len() == 1 {
        format!("移动 {}", names[0])
    } else {
        format!("移动 {} 个项目", names.len())
    };
    
    let task = crate::task::Task::new_copy_move(
        crate::task::TaskType::Move,
        task_name,
        src_dir.clone(),
        dst_dir.clone(),
        names.clone(),
        strategy_str.clone(),
        user_id,
    );
    let task_id = task.id.clone();
    
    state.task_manager.add_task(task).await;
    state.task_manager.start_task(&task_id).await;
    
    // 异步执行移动操作
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    
    tokio::spawn(async move {
        let result = execute_move_operation(&state_clone, &src_dir, &dst_dir, &names, &task_id_clone, strategy).await;
        
        match result {
            Ok(()) => {
                state_clone.task_manager.complete_task(&task_id_clone).await;
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("cancelled") || err_msg.contains("取消") {
                    state_clone.task_manager.cancel_task(&task_id_clone).await;
                } else {
                    state_clone.task_manager.fail_task(&task_id_clone, err_msg).await;
                }
            }
        }
    });
    
    Ok(Json(json!({
        "code": 200,
        "message": "移动任务已创建",
        "data": {
            "taskId": task_id
        }
    })))
}

/// 执行移动操作
async fn execute_move_operation(
    state: &AppState,
    src_dir: &str,
    dst_dir: &str,
    names: &[String],
    task_id: &str,
    strategy: ConflictStrategy,
) -> anyhow::Result<()> {
    // 获取所有挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    let src_mount = get_first_mount(src_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("源目录不存在"))?;
    let dst_mount = get_first_mount(dst_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("目标目录不存在"))?;
    
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let dst_mount_path = fix_and_clean_path(&dst_mount.mount_path);
    
    // 计算总大小并更新任务
    let src_driver = state.storage_manager.get_driver(&src_mount.id).await
        .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
    let mut total_size = 0u64;
    for name in names {
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                if entry.is_dir {
                    // 递归计算文件夹大小
                    if let Ok(size) = calculate_dir_size(&src_driver, &src_actual).await {
                        total_size += size;
                    }
                } else {
                    total_size += entry.size;
                }
            }
        }
    }
    
    // 更新任务总大小
    state.task_manager.update_task_total_size(task_id, total_size).await;
    
    let existing_names = get_existing_names(state, dst_dir).await;
    let mut processed = 0u64;
    let mut processed_size = 0u64;
    
    for name in names {
        // 根据冲突策略处理文件名
        let final_name = match strategy {
            ConflictStrategy::Overwrite => name.clone(),
            ConflictStrategy::Skip => {
                if existing_names.contains(name) {
                    processed += 1;
                    continue; // 跳过已存在的文件
                }
                name.clone()
            }
            _ => resolve_conflict_name(name, &existing_names),
        };
        
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        
        let dst_file_path = if dst_dir == "/" {
            format!("/{}", final_name)
        } else {
            format!("{}/{}", dst_dir, final_name)
        };
        
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        let dst_actual = if dst_file_path.len() > dst_mount_path.len() {
            fix_and_clean_path(&dst_file_path[dst_mount_path.len()..])
        } else {
            format!("/{}", final_name)
        };
        
        // 获取文件大小
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        let mut file_size = 0u64;
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                file_size = entry.size;
            }
        }
        
        // 更新当前文件（在开始处理前）
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
        
        if src_mount.id == dst_mount.id {
            if let Some(driver) = state.storage_manager.get_driver(&src_mount.id).await {
                driver.move_item(&src_actual, &dst_actual).await?;
            }
        } else {
            let src_driver = state.storage_manager.get_driver(&src_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
            let dst_driver = state.storage_manager.get_driver(&dst_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("目标驱动不存在"))?;
            
            // 判断是文件还是目录
            let is_dir = {
                let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
                let filename = src_actual.split('/').last().unwrap_or("");
                if let Ok(entries) = src_driver.list(parent).await {
                    entries.iter().find(|e| e.name == filename).map(|e| e.is_dir).unwrap_or(false)
                } else {
                    false
                }
            };
            
            let total_files = names.len() as u64;
            
            if is_dir {
                // 复制目录（带进度更新）
                cross_driver_copy_dir_with_progress(
                    &src_driver,
                    &dst_driver,
                    &src_actual,
                    &dst_actual,
                    &state.task_manager,
                    task_id,
                    &mut processed_size,
                    processed,
                    total_files,
                    total_size,
                ).await?;
            } else {
                // 使用带进度更新的复制（每秒更新一次）
                cross_driver_copy_file_with_progress(
                    &src_driver,
                    &dst_driver,
                    &src_actual,
                    &dst_actual,
                    &state.task_manager,
                    task_id,
                    processed_size,
                    processed,
                    total_files,
                    total_size,
                ).await?;
            }
            
            src_driver.delete(&src_actual).await?;
        }
        
        processed += 1;
        processed_size += file_size;
        
        // 更新进度（在完成后）
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
    }
    
    Ok(())
}

/// 执行移动操作（从断点继续）
pub async fn execute_move_operation_resume(
    state: &AppState,
    src_dir: &str,
    dst_dir: &str,
    names: &[String],
    task_id: &str,
    strategy_str: &str,
    skip_files: u64,
) -> anyhow::Result<()> {
    let strategy = match strategy_str {
        "overwrite" => ConflictStrategy::Overwrite,
        "skip" => ConflictStrategy::Skip,
        _ => ConflictStrategy::AutoRename,
    };
    
    // 检查任务是否被取消
    if let Some(ctrl) = state.task_manager.get_control(task_id).await {
        if ctrl.is_cancelled() {
            return Err(anyhow::anyhow!("任务已取消"));
        }
    }
    
    // 获取所有挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    let src_mount = get_first_mount(src_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("源目录不存在"))?;
    let dst_mount = get_first_mount(dst_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("目标目录不存在"))?;
    
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let dst_mount_path = fix_and_clean_path(&dst_mount.mount_path);
    
    let src_driver = state.storage_manager.get_driver(&src_mount.id).await
        .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
    
    let existing_names = get_existing_names(state, dst_dir).await;
    let mut processed = skip_files;
    let mut processed_size = 0u64;
    
    // 从断点继续：跳过已处理的文件
    for (idx, name) in names.iter().enumerate() {
        if (idx as u64) < skip_files {
            continue; // 跳过已处理的文件
        }
        
        // 检查任务是否被取消
        if let Some(ctrl) = state.task_manager.get_control(task_id).await {
            if ctrl.is_cancelled() {
                return Err(anyhow::anyhow!("任务已取消"));
            }
            // 暂停等待
            while ctrl.is_paused() {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if ctrl.is_cancelled() {
                    return Err(anyhow::anyhow!("任务已取消"));
                }
            }
        }
        
        // 根据冲突策略处理文件名
        let final_name = match strategy {
            ConflictStrategy::Overwrite => name.clone(),
            ConflictStrategy::Skip => {
                if existing_names.contains(name) {
                    processed += 1;
                    continue;
                }
                name.clone()
            }
            _ => resolve_conflict_name(name, &existing_names),
        };
        
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        
        let dst_file_path = if dst_dir == "/" {
            format!("/{}", final_name)
        } else {
            format!("{}/{}", dst_dir, final_name)
        };
        
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        let dst_actual = if dst_file_path.len() > dst_mount_path.len() {
            fix_and_clean_path(&dst_file_path[dst_mount_path.len()..])
        } else {
            format!("/{}", final_name)
        };
        
        // 获取文件大小
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        let mut file_size = 0u64;
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                file_size = entry.size;
            }
        }
        
        // 更新当前文件
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
        
        if src_mount.id == dst_mount.id {
            if let Some(driver) = state.storage_manager.get_driver(&src_mount.id).await {
                driver.move_item(&src_actual, &dst_actual).await?;
            }
        } else {
            let src_driver = state.storage_manager.get_driver(&src_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
            let dst_driver = state.storage_manager.get_driver(&dst_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("目标驱动不存在"))?;
            
            let is_dir = {
                let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
                let filename = src_actual.split('/').last().unwrap_or("");
                if let Ok(entries) = src_driver.list(parent).await {
                    entries.iter().find(|e| e.name == filename).map(|e| e.is_dir).unwrap_or(false)
                } else {
                    false
                }
            };
            
            let total_files = names.len() as u64;
            let total_size = 0u64; // 恢复时不重新计算总大小
            
            if is_dir {
                cross_driver_copy_dir_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, &mut processed_size, processed,
                    total_files, total_size,
                ).await?;
            } else {
                cross_driver_copy_file_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, processed_size, processed,
                    total_files, total_size,
                ).await?;
            }
            
            src_driver.delete(&src_actual).await?;
        }
        
        processed += 1;
        processed_size += file_size;
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
    }
    
    Ok(())
}

/// 执行复制操作（从断点继续）
pub async fn execute_copy_operation_resume(
    state: &AppState,
    src_dir: &str,
    dst_dir: &str,
    names: &[String],
    task_id: &str,
    strategy_str: &str,
    skip_files: u64,
) -> anyhow::Result<()> {
    let strategy = match strategy_str {
        "overwrite" => ConflictStrategy::Overwrite,
        "skip" => ConflictStrategy::Skip,
        _ => ConflictStrategy::AutoRename,
    };
    
    // 检查任务是否被取消
    if let Some(ctrl) = state.task_manager.get_control(task_id).await {
        if ctrl.is_cancelled() {
            return Err(anyhow::anyhow!("任务已取消"));
        }
    }
    
    // 获取所有挂载点
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    let src_mount = get_first_mount(src_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("源目录不存在"))?;
    let dst_mount = get_first_mount(dst_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("目标目录不存在"))?;
    
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let dst_mount_path = fix_and_clean_path(&dst_mount.mount_path);
    
    let src_driver = state.storage_manager.get_driver(&src_mount.id).await
        .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
    
    let existing_names = get_existing_names(state, dst_dir).await;
    let mut processed = skip_files;
    let mut processed_size = 0u64;
    
    // 从断点继续：跳过已处理的文件
    for (idx, name) in names.iter().enumerate() {
        if (idx as u64) < skip_files {
            continue;
        }
        
        // 检查任务是否被取消
        if let Some(ctrl) = state.task_manager.get_control(task_id).await {
            if ctrl.is_cancelled() {
                return Err(anyhow::anyhow!("任务已取消"));
            }
            while ctrl.is_paused() {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if ctrl.is_cancelled() {
                    return Err(anyhow::anyhow!("任务已取消"));
                }
            }
        }
        
        let final_name = match strategy {
            ConflictStrategy::Overwrite => name.clone(),
            ConflictStrategy::Skip => {
                if existing_names.contains(name) {
                    processed += 1;
                    continue;
                }
                name.clone()
            }
            _ => resolve_conflict_name(name, &existing_names),
        };
        
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        
        let dst_file_path = if dst_dir == "/" {
            format!("/{}", final_name)
        } else {
            format!("{}/{}", dst_dir, final_name)
        };
        
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        let dst_actual = if dst_file_path.len() > dst_mount_path.len() {
            fix_and_clean_path(&dst_file_path[dst_mount_path.len()..])
        } else {
            format!("/{}", final_name)
        };
        
        // 获取文件大小
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        let mut file_size = 0u64;
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                file_size = entry.size;
            }
        }
        
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
        
        let is_dir = {
            let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Ok(entries) = src_driver.list(parent).await {
                entries.iter().find(|e| e.name == filename).map(|e| e.is_dir).unwrap_or(false)
            } else {
                false
            }
        };
        
        if src_mount.id == dst_mount.id {
            if let Some(driver) = state.storage_manager.get_driver(&src_mount.id).await {
                driver.copy_item(&src_actual, &dst_actual).await?;
            }
        } else {
            let src_driver = state.storage_manager.get_driver(&src_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
            let dst_driver = state.storage_manager.get_driver(&dst_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("目标驱动不存在"))?;
            
            let total_files = names.len() as u64;
            let total_size = 0u64;
            
            if is_dir {
                cross_driver_copy_dir_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, &mut processed_size, processed,
                    total_files, total_size,
                ).await?;
            } else {
                cross_driver_copy_file_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, processed_size, processed,
                    total_files, total_size,
                ).await?;
            }
        }
        
        processed += 1;
        processed_size += file_size;
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
    }
    
    Ok(())
}

/// POST /api/fs/copy - 复制文件或目录（创建任务异步执行）
pub async fn fs_copy(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<FsMoveReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_src_dir = fix_and_clean_path(&req.src_dir);
    let req_dst_dir = fix_and_clean_path(&req.dst_dir);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.copy_files && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有复制文件的权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let src_dir = match join_user_path(&user_ctx.root_path, &req_src_dir) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    let dst_dir = match join_user_path(&user_ctx.root_path, &req_dst_dir) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    let user_id = get_user_id(&state, &cookies).await;
    let names = req.names.clone();
    
    // 解析冲突策略
    let strategy_str = req.conflict_strategy.clone().unwrap_or_else(|| "auto_rename".to_string());
    let strategy = match strategy_str.as_str() {
        "overwrite" => ConflictStrategy::Overwrite,
        "skip" => ConflictStrategy::Skip,
        _ => ConflictStrategy::AutoRename,
    };
    
    // 创建复制任务
    let task_name = if names.len() == 1 {
        format!("复制 {}", names[0])
    } else {
        format!("复制 {} 个项目", names.len())
    };
    
    let task = crate::task::Task::new_copy_move(
        crate::task::TaskType::Copy,
        task_name,
        src_dir.clone(),
        dst_dir.clone(),
        names.clone(),
        strategy_str.clone(),
        user_id,
    );
    let task_id = task.id.clone();
    
    state.task_manager.add_task(task).await;
    state.task_manager.start_task(&task_id).await;
    
    // 创建任务控制标志
    let control = state.task_manager.create_control(&task_id).await;
    
    // 异步执行复制操作
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    
    tokio::spawn(async move {
        let result = execute_copy_operation(&state_clone, &src_dir, &dst_dir, &names, &task_id_clone, strategy, control.clone()).await;
        
        match result {
            Ok(()) => {
                state_clone.task_manager.complete_task(&task_id_clone).await;
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("cancelled") || err_msg.contains("取消") {
                    state_clone.task_manager.cancel_task(&task_id_clone).await;
                } else {
                    state_clone.task_manager.fail_task(&task_id_clone, err_msg).await;
                }
            }
        }
        state_clone.task_manager.remove_control(&task_id_clone).await;
    });
    
    Ok(Json(json!({
        "code": 200,
        "message": "复制任务已创建",
        "data": {
            "taskId": task_id
        }
    })))
}

/// 执行复制操作
async fn execute_copy_operation(
    state: &AppState,
    src_dir: &str,
    dst_dir: &str,
    names: &[String],
    task_id: &str,
    strategy: ConflictStrategy,
    _control: std::sync::Arc<crate::task::TaskControl>,
) -> anyhow::Result<()> {
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await?;
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    let src_mount = get_first_mount(src_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("源目录不存在"))?;
    let dst_mount = get_first_mount(dst_dir, &mounts)
        .ok_or_else(|| anyhow::anyhow!("目标目录不存在"))?;
    
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let dst_mount_path = fix_and_clean_path(&dst_mount.mount_path);
    
    let src_driver = state.storage_manager.get_driver(&src_mount.id).await
        .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
    
    let mut total_size = 0u64;
    for name in names {
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                if entry.is_dir {
                    if let Ok(size) = calculate_dir_size(&src_driver, &src_actual).await {
                        total_size += size;
                    }
                } else {
                    total_size += entry.size;
                }
            }
        }
    }
    
    state.task_manager.update_task_total_size(task_id, total_size).await;
    
    let existing_names = get_existing_names(state, dst_dir).await;
    let mut processed = 0u64;
    let mut processed_size = 0u64;
    
    for name in names {
        let final_name = match strategy {
            ConflictStrategy::Overwrite => name.clone(),
            ConflictStrategy::Skip => {
                if existing_names.contains(name) {
                    processed += 1;
                    continue;
                }
                name.clone()
            }
            _ => resolve_conflict_name(name, &existing_names),
        };
        
        let src_file_path = if src_dir == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", src_dir, name)
        };
        
        let dst_file_path = if dst_dir == "/" {
            format!("/{}", final_name)
        } else {
            format!("{}/{}", dst_dir, final_name)
        };
        
        let src_actual = if src_file_path.len() > src_mount_path.len() {
            fix_and_clean_path(&src_file_path[src_mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        let dst_actual = if dst_file_path.len() > dst_mount_path.len() {
            fix_and_clean_path(&dst_file_path[dst_mount_path.len()..])
        } else {
            format!("/{}", final_name)
        };
        
        let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
        let mut file_size = 0u64;
        if let Ok(entries) = src_driver.list(parent).await {
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Some(entry) = entries.iter().find(|e| e.name == filename) {
                file_size = entry.size;
            }
        }
        
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
        
        let is_dir = {
            let parent = src_actual.rsplitn(2, '/').nth(1).unwrap_or("/");
            let filename = src_actual.split('/').last().unwrap_or("");
            if let Ok(entries) = src_driver.list(parent).await {
                entries.iter().find(|e| e.name == filename).map(|e| e.is_dir).unwrap_or(false)
            } else {
                false
            }
        };
        
        if src_mount.id == dst_mount.id {
            if let Some(driver) = state.storage_manager.get_driver(&src_mount.id).await {
                driver.copy_item(&src_actual, &dst_actual).await?;
            }
        } else {
            let src_driver = state.storage_manager.get_driver(&src_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("源驱动不存在"))?;
            let dst_driver = state.storage_manager.get_driver(&dst_mount.id).await
                .ok_or_else(|| anyhow::anyhow!("目标驱动不存在"))?;
            
            let total_files = names.len() as u64;
            
            if is_dir {
                cross_driver_copy_dir_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, &mut processed_size,
                    processed, total_files, total_size,
                ).await?;
            } else {
                cross_driver_copy_file_with_progress(
                    &src_driver, &dst_driver, &src_actual, &dst_actual,
                    &state.task_manager, task_id, processed_size,
                    processed, total_files, total_size,
                ).await?;
            }
        }
        
        processed += 1;
        processed_size += file_size;
        state.task_manager.update_task_progress_with_size(task_id, processed, processed_size, Some(name.clone())).await;
    }
    
    Ok(())
}
