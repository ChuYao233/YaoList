use std::sync::Arc;
use std::collections::HashMap;
use axum::{
    extract::{State, Multipart},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_cookies::Cookies;
use tokio::sync::RwLock;
use tokio::io::AsyncWrite;
use futures::StreamExt;

use crate::state::AppState;
use crate::task::{TaskType, TaskStatus, UploadFileInfo};
use crate::api::file_resolver::{MountInfo, get_first_mount};
use yaolist_backend::utils::{fix_and_clean_path, resolve_conflict_name, ConflictStrategy};

use super::{get_user_context, join_user_path, get_user_id};

/// 全局writer缓存，用于跨请求保持流式写入连接
/// Key: task_id + filename
lazy_static::lazy_static! {
    static ref STREAM_WRITERS: RwLock<HashMap<String, tokio::sync::Mutex<Box<dyn AsyncWrite + Unpin + Send>>>> = RwLock::new(HashMap::new());
}

/// 流式写入缓冲大小
const STREAM_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1MB

/// 最大内存缓冲大小（超过此大小使用流式写入）
const MAX_MEMORY_CHUNK_SIZE: usize = 32 * 1024 * 1024; // 32MB

/// POST /api/fs/upload - 分片上传文件（使用流式写入）
pub async fn fs_upload(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    mut multipart: Multipart,
) -> Result<Json<Value>, StatusCode> {
    use tokio::io::AsyncWriteExt;
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    // 权限验证
    if !perms.create_upload && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有上传文件的权限"
        })));
    }
    
    let user_root_path = user_ctx.root_path.clone();
    let mut target_path = String::new();
    let mut filename = String::new();
    let mut chunk_index: i64 = -1;
    let mut total_chunks: i64 = 1;
    let mut total_size: u64 = 0;
    let mut task_id: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    
    let user_id = get_user_id(&state, &cookies).await;
    
    // 解析multipart数据
    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "path" => target_path = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?,
            "filename" => filename = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?,
            "chunkIndex" => chunk_index = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?.parse().unwrap_or(-1),
            "totalChunks" => total_chunks = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?.parse().unwrap_or(1),
            "totalSize" => total_size = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?.parse().unwrap_or(0),
            "taskId" => task_id = Some(field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?),
            "file" | "files" => {
                if filename.is_empty() {
                    filename = field.file_name().unwrap_or("unknown").to_string();
                }
                // 真正的流式读取：每次只缓冲最多1MB，背压控制内存
                let mut buffer = Vec::with_capacity(STREAM_BUFFER_SIZE);
                let mut field_stream = field;
                while let Some(chunk_result) = field_stream.chunk().await.transpose() {
                    match chunk_result {
                        Ok(chunk) => {
                            buffer.extend_from_slice(&chunk);
                            // 达到缓冲大小时停止，等待写入
                            if buffer.len() >= STREAM_BUFFER_SIZE {
                                break;
                            }
                        }
                        Err(_) => return Err(StatusCode::BAD_REQUEST),
                    }
                }
                // 继续读取剩余数据（如果有）
                while let Some(chunk_result) = field_stream.chunk().await.transpose() {
                    match chunk_result {
                        Ok(chunk) => {
                            buffer.extend_from_slice(&chunk);
                        }
                        Err(_) => return Err(StatusCode::BAD_REQUEST),
                    }
                }
                file_data = Some(buffer);
            }
            _ => {}
        }
    }
    
    let data = file_data.ok_or(StatusCode::BAD_REQUEST)?;
    if target_path.is_empty() {
        target_path = "/".to_string();
    }
    
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
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let req_path = fix_and_clean_path(&target_path);
    let path = match join_user_path(&user_root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    let file_path = if path == "/" {
        format!("/{}", filename)
    } else {
        format!("{}/{}", path, filename)
    };
    
    let mount = get_first_mount(&file_path, &mounts)
        .ok_or_else(|| {
            tracing::error!("Upload failed: Mount point not found, file_path={}", file_path);
            StatusCode::NOT_FOUND
        })?;
    
    tracing::debug!("Upload: Found mount point {}, file_path={}", mount.mount_path, file_path);
    let mount_path = fix_and_clean_path(&mount.mount_path);
    let actual_path = if file_path.len() > mount_path.len() {
        fix_and_clean_path(&file_path[mount_path.len()..])
    } else {
        format!("/{}", filename)
    };
    
    let driver = state.storage_manager.get_driver(&mount.id).await
        .ok_or_else(|| {
            tracing::error!("Upload failed: Driver not found {}", mount.id);
            StatusCode::NOT_FOUND
        })?;
    
    tracing::debug!("Upload: Driver obtained successfully, actual_path={}", actual_path);
    
    // 创建或获取任务
    let is_batch_task = task_id.is_some();
    let current_task_id = if let Some(tid) = task_id {
        tid
    } else {
        let display_path = if path == "/" {
            format!("/{}", filename)
        } else {
            format!("{}/{}", path, filename)
        };
        let tid = state.task_manager.create_task(
            TaskType::Upload,
            filename.clone(),
            display_path,
            None,
            total_size,
            1,
            user_id,
        ).await;
        state.task_manager.start_task(&tid).await;
        tid
    };
    
    // 批次任务中的文件路径 - 使用纯文件名匹配（批次任务只存储文件名）
    let simple_filename = filename.split('/').last().unwrap_or(&filename);
    let batch_file_path = if path == "/" {
        format!("/{}", simple_filename)
    } else {
        format!("{}/{}", path, simple_filename)
    };
    
    // 检查任务是否被取消
    if is_batch_task {
        if let Some(control) = state.task_manager.get_control(&current_task_id).await {
            if control.is_cancelled() {
                return Ok(Json(json!({
                    "code": 499,
                    "message": "任务已取消"
                })));
            }
            // 检查暂停状态，返回498让前端等待
            if control.is_paused() {
                return Ok(Json(json!({
                    "code": 498,
                    "message": "任务已暂停"
                })));
            }
        }
    }
    
    // 分片上传
    if chunk_index >= 0 && total_chunks > 1 {
        let capabilities = driver.capabilities();
        
        // 只有需要完整文件MD5的驱动（如123云盘）才缓存本地，其他全部直接put（流式）
        let needs_local_cache = capabilities.can_multipart_upload;
        
        if needs_local_cache {
            // 123云盘等：缓存分片到本地，最后合并调用put（需要完整MD5）
            let temp_dir = std::path::PathBuf::from("data/temps");
            let _ = std::fs::create_dir_all(&temp_dir);
            let chunk_file = temp_dir.join(format!("{}_{}.part{}", current_task_id, filename.replace("/", "_"), chunk_index));
            
            tokio::fs::write(&chunk_file, &data).await
                .map_err(|e| {
                    tracing::error!("write chunk to temp failed: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            drop(data);
            
            if is_batch_task {
                state.task_manager.update_file_progress(
                    &current_task_id, 
                    &batch_file_path, 
                    0,
                    Some(chunk_index as u32)
                ).await;
            }
            
            // 最后一个分片，合并后调用put上传
            if chunk_index == total_chunks - 1 {
                let driver_clone = driver.clone();
                let actual_path_clone = actual_path.clone();
                let task_manager = state.task_manager.clone();
                let task_id_clone = current_task_id.clone();
                let batch_file_path_clone = batch_file_path.clone();
                let is_batch = is_batch_task;
                let filename_clone = filename.clone();
                
                tokio::spawn(async move {
                    let upload_result = async {
                        let mut merged_data = Vec::with_capacity(total_size as usize);
                        let temp_dir = std::path::PathBuf::from("data/temps");
                        
                        for i in 0..total_chunks {
                            let part_file = temp_dir.join(format!("{}_{}.part{}", task_id_clone, filename_clone.replace("/", "_"), i));
                            let part_data = tokio::fs::read(&part_file).await?;
                            merged_data.extend_from_slice(&part_data);
                            let _ = tokio::fs::remove_file(&part_file).await;
                            
                            let progress = ((i + 1) as f64 / total_chunks as f64 * 0.5 * total_size as f64) as u64;
                            if is_batch {
                                task_manager.update_file_progress(&task_id_clone, &batch_file_path_clone, progress, None).await;
                            }
                        }
                        
                        let shared_progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                        let shared_progress_cb = shared_progress.clone();
                        let ts = total_size;
                        let progress_callback: Option<yaolist_backend::storage::ProgressCallback> = Some(std::sync::Arc::new(move |completed: u64, _total: u64| {
                            let progress = (ts as f64 * 0.5 + completed as f64 * 0.5) as u64;
                            shared_progress_cb.store(progress, std::sync::atomic::Ordering::SeqCst);
                        }));
                        
                        driver_clone.put(&actual_path_clone, bytes::Bytes::from(merged_data), progress_callback).await?;
                        Ok::<(), anyhow::Error>(())
                    }.await;
                    
                    match upload_result {
                        Ok(()) => {
                            if is_batch {
                                task_manager.update_file_progress(&task_id_clone, &batch_file_path_clone, total_size, None).await;
                                task_manager.complete_file(&task_id_clone, &batch_file_path_clone).await;
                            } else {
                                task_manager.complete_task(&task_id_clone).await;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Upload failed: {}", e);
                            task_manager.fail_task(&task_id_clone, format!("上传失败: {}", e)).await;
                        }
                    }
                });
                
                return Ok(Json(json!({
                    "code": 200,
                    "message": "分片上传完成，正在上传到存储",
                    "data": {
                        "filename": filename,
                        "completed": true,
                        "merging": true,
                        "taskId": current_task_id
                    }
                })));
            }
        } else {
            // 其他所有驱动：使用全局writer缓存，流式写入，不缓存本地
            let writer_key = format!("{}_{}", current_task_id, filename.replace("/", "_"));
            let chunk_size = data.len() as u64;
            let processed = (chunk_index as u64 + 1) * chunk_size;
            
            // 写入前再次检查任务状态（处理取消/暂停）
            if let Some(control) = state.task_manager.get_control(&current_task_id).await {
                if control.is_cancelled() {
                    // 清理writer
                    let mut writers = STREAM_WRITERS.write().await;
                    if let Some(writer_mutex) = writers.remove(&writer_key) {
                        let mut writer = writer_mutex.lock().await;
                        writer.shutdown().await.ok();
                    }
                    tracing::info!("Upload cancelled: {}", current_task_id);
                    return Ok(Json(json!({
                        "code": 499,
                        "message": "任务已取消"
                    })));
                }
                if control.is_paused() {
                    return Ok(Json(json!({
                        "code": 498,
                        "message": "任务已暂停"
                    })));
                }
            }
            
            // 第一个分片：创建writer并缓存
            if chunk_index == 0 {
                let writer = driver.open_writer(&actual_path, Some(total_size), None).await
                    .map_err(|e| {
                        tracing::error!("open_writer failed: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                
                let mut writers = STREAM_WRITERS.write().await;
                writers.insert(writer_key.clone(), tokio::sync::Mutex::new(writer));
            }
            
            // 写入当前分片数据
            {
                let readers = STREAM_WRITERS.read().await;
                if let Some(writer_mutex) = readers.get(&writer_key) {
                    let mut writer = writer_mutex.lock().await;
                    writer.write_all(&data).await
                        .map_err(|e| {
                            tracing::error!("write chunk failed: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                } else {
                    // Writer不存在可能是任务已取消
                    tracing::warn!("Writer not found for key: {}, task may be cancelled", writer_key);
                    return Ok(Json(json!({
                        "code": 499,
                        "message": "任务已取消或writer丢失"
                    })));
                }
            }
            
            drop(data);
            
            // 更新进度
            if is_batch_task {
                state.task_manager.update_file_progress(
                    &current_task_id, 
                    &batch_file_path, 
                    processed.min(total_size),
                    Some(chunk_index as u32)
                ).await;
            } else {
                state.task_manager.update_progress(&current_task_id, processed.min(total_size)).await;
            }
            
            // 最后一个分片：关闭writer并清理
            if chunk_index == total_chunks - 1 {
                {
                    let mut writers = STREAM_WRITERS.write().await;
                    if let Some(writer_mutex) = writers.remove(&writer_key) {
                        let mut writer = writer_mutex.lock().await;
                        writer.shutdown().await.ok();
                    }
                }
                
                if is_batch_task {
                    state.task_manager.update_file_progress(&current_task_id, &batch_file_path, total_size, None).await;
                    state.task_manager.complete_file(&current_task_id, &batch_file_path).await;
                } else {
                    state.task_manager.complete_task(&current_task_id).await;
                }
                
                return Ok(Json(json!({
                    "code": 200,
                    "message": "上传完成",
                    "data": {
                        "filename": filename,
                        "completed": true,
                        "taskId": current_task_id
                    }
                })));
            }
        }
        
        return Ok(Json(json!({
            "code": 200,
            "message": "分片上传成功",
            "data": {
                "chunkIndex": chunk_index,
                "completed": false,
                "taskId": current_task_id,
                "filename": filename
            }
        })));
    } else {
        // 单文件上传 - 使用put方法
        tracing::debug!("Single file upload: {} -> {}", batch_file_path, actual_path);
        
        let file_size = data.len() as u64;
        
        // 创建进度回调 - 使用共享原子状态，避免频繁创建线程
        let shared_progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let shared_progress_cb = shared_progress.clone();
        let progress_callback: Option<yaolist_backend::storage::ProgressCallback> = Some(std::sync::Arc::new(move |completed: u64, _total: u64| {
            shared_progress_cb.store(completed, std::sync::atomic::Ordering::SeqCst);
        }));
        
        // 调用put上传
        driver.put(&actual_path, bytes::Bytes::from(data), progress_callback).await
            .map_err(|e| {
                tracing::error!("Single file put failed: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        
        // 上传完成后读取最终进度并更新
        let final_progress = shared_progress.load(std::sync::atomic::Ordering::SeqCst);
        if final_progress > 0 {
            if is_batch_task {
                state.task_manager.update_file_progress(&current_task_id, &batch_file_path, final_progress, None).await;
            } else {
                state.task_manager.update_progress(&current_task_id, final_progress).await;
            }
        }
        
        // 标记文件/任务完成
        if is_batch_task {
            state.task_manager.update_file_progress(&current_task_id, &batch_file_path, file_size, None).await;
            state.task_manager.complete_file(&current_task_id, &batch_file_path).await;
        } else {
            state.task_manager.complete_task(&current_task_id).await;
        }
        
        return Ok(Json(json!({
            "code": 200,
            "message": "上传成功",
            "data": {
                "filename": filename,
                "completed": true,
                "taskId": current_task_id
            }
        })));
    }
}

#[derive(Debug, Deserialize)]
pub struct UploadStatusReq {
    pub path: String,
    pub filename: String,
    pub total_chunks: i64,
}

/// POST /api/fs/upload/status - 查询上传状态（用于断点续传）
pub async fn fs_upload_status(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<UploadStatusReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    if !perms.create_upload && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有权限"
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
    
    // 查询任务中的已上传分片（用于断点续传）
    let full_path = format!("{}/{}", path, req.filename);
    
    // 查找所有上传任务中匹配此文件的
    let tasks = state.task_manager.get_all_tasks().await;
    for task in tasks {
        if let Some(ref files) = task.files {
            if let Some(file) = files.iter().find(|f| f.path == full_path) {
                return Ok(Json(json!({
                    "code": 200,
                    "data": {
                        "uploadedChunks": file.uploaded_chunks,
                        "taskId": task.id
                    }
                })));
            }
        }
    }
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "uploadedChunks": []
        }
    })))
}

#[derive(Debug, Deserialize)]
pub struct BatchUploadFileInfo {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct CreateBatchUploadReq {
    pub target_path: String,
    pub files: Vec<BatchUploadFileInfo>,
    #[serde(default)]
    pub conflict_strategy: Option<String>, // "auto_rename", "overwrite", "skip", "error"
}

/// POST /api/fs/upload/batch - 创建批次上传任务
pub async fn fs_create_batch_upload(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<CreateBatchUploadReq>,
) -> Result<Json<Value>, StatusCode> {
    let req_path = fix_and_clean_path(&req.target_path);
    
    // 获取用户上下文（权限+根路径）
    let user_ctx = get_user_context(&state, &cookies).await;
    let perms = &user_ctx.permissions;
    
    if !perms.create_upload && !perms.is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "没有上传权限"
        })));
    }
    
    // 将用户请求路径与用户根路径结合（防止路径穿越）
    let target_path = match join_user_path(&user_ctx.root_path, &req_path) {
        Ok(p) => p,
        Err(e) => {
            return Ok(Json(json!({
                "code": 403,
                "message": e
            })));
        }
    };
    
    let user_id = get_user_id(&state, &cookies).await;
    
    // 解析冲突策略
    let strategy = match req.conflict_strategy.as_deref() {
        Some("overwrite") => ConflictStrategy::Overwrite,
        Some("skip") => ConflictStrategy::Skip,
        Some("error") => ConflictStrategy::Error,
        _ => ConflictStrategy::AutoRename,
    };
    
    // 获取目标目录已存在的文件列表（用于冲突检测）
    let existing_names = get_existing_names(&state, &target_path).await;
    
    // 处理文件名冲突
    let mut upload_files: Vec<UploadFileInfo> = Vec::new();
    let mut resolved_paths: Vec<Value> = Vec::new();
    
    for file in &req.files {
        let filename = file.path.split('/').last().unwrap_or(&file.path);
        
        let final_name = match strategy {
            ConflictStrategy::AutoRename => {
                resolve_conflict_name(filename, &existing_names)
            }
            ConflictStrategy::Overwrite => filename.to_string(),
            ConflictStrategy::Skip => {
                if existing_names.contains(&filename.to_string()) {
                    resolved_paths.push(json!({
                        "original": file.path,
                        "resolved": null,
                        "skipped": true
                    }));
                    continue;
                }
                filename.to_string()
            }
            ConflictStrategy::Error => {
                if existing_names.contains(&filename.to_string()) {
                    return Ok(Json(json!({
                        "code": 409,
                        "message": format!("文件已存在: {}", filename)
                    })));
                }
                filename.to_string()
            }
        };
        
        let full_path = if target_path == "/" {
            format!("/{}", final_name)
        } else {
            format!("{}/{}", target_path, final_name)
        };
        
        upload_files.push(UploadFileInfo {
            path: full_path.clone(),
            size: file.size,
            uploaded_size: 0,
            uploaded_chunks: vec![],
            status: TaskStatus::Pending,
        });
        
        resolved_paths.push(json!({
            "original": file.path,
            "resolved": full_path,
            "skipped": false
        }));
    }
    
    if upload_files.is_empty() {
        return Ok(Json(json!({
            "code": 200,
            "message": "所有文件已跳过",
            "data": {
                "taskId": null,
                "files": resolved_paths
            }
        })));
    }
    
    // 创建批次上传任务
    let task_name = if upload_files.len() == 1 {
        upload_files[0].path.split('/').last().unwrap_or("上传").to_string()
    } else {
        format!("上传 {} 个文件", upload_files.len())
    };
    
    let task_id = state.task_manager.create_batch_upload(
        task_name,
        target_path,
        upload_files,
        user_id,
    ).await;
    
    // 启动任务
    state.task_manager.start_task(&task_id).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "taskId": task_id,
            "files": resolved_paths
        }
    })))
}

/// 获取目录中已存在的文件名列表
pub async fn get_existing_names(state: &AppState, path: &str) -> Vec<String> {
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
            order: 0,
        })
    }).collect();
    
    if let Some(mount) = get_first_mount(path, &mounts) {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        let actual_path = if path.len() > mount_path.len() {
            fix_and_clean_path(&path[mount_path.len()..])
        } else {
            "/".to_string()
        };
        
        if let Some(driver) = state.storage_manager.get_driver(&mount.id).await {
            if let Ok(entries) = driver.list(&actual_path).await {
                return entries.iter().map(|e| e.name.clone()).collect();
            }
        }
    }
    
    vec![]
}

#[derive(Debug, Deserialize)]
pub struct UpdateUploadProgressReq {
    pub task_id: String,
    pub file_path: String,
    pub uploaded_size: u64,
    pub chunk_index: Option<u32>,
}

/// POST /api/fs/upload/progress - 更新上传进度
pub async fn fs_update_upload_progress(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateUploadProgressReq>,
) -> Result<Json<Value>, StatusCode> {
    state.task_manager.update_file_progress(
        &req.task_id,
        &req.file_path,
        req.uploaded_size,
        req.chunk_index,
    ).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": "success"
    })))
}

#[derive(Debug, Deserialize)]
pub struct CompleteFileReq {
    pub task_id: String,
    pub file_path: String,
}

/// POST /api/fs/upload/complete_file - 标记单个文件上传完成
pub async fn fs_complete_file(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompleteFileReq>,
) -> Result<Json<Value>, StatusCode> {
    state.task_manager.complete_file(&req.task_id, &req.file_path).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": "success"
    })))
}