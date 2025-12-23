use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;
use tracing::{debug, info, warn, error};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tempfile::TempDir;
use std::path::Path;

use crate::task::Task;
use crate::task::TaskType;
use super::extractors::extract_to_local_with_progress;

use crate::state::AppState;
use crate::models::UserPermissions;
use crate::auth::SESSION_COOKIE_NAME;
use yaolist_backend::utils::{fix_and_clean_path, is_sub_path};
use super::types::*;
use super::utils::*;


/// 根据文件名获取压缩格式
fn get_archive_format(filename: &str) -> Option<ArchiveFormat> {
    let filename = filename.to_lowercase();
    
    // 检查双扩展名
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if filename.ends_with(".tar.bz2") || filename.ends_with(".tbz2") {
        return Some(ArchiveFormat::TarBz2);
    }
    
    // 检查单扩展名
    let ext = filename.split('.').last()?;
    match ext {
        "zip" | "jar" | "war" | "apk" | "ipa" | "epub" | "zipx" => Some(ArchiveFormat::Zip),
        "tar" => Some(ArchiveFormat::Tar),
        "7z" => Some(ArchiveFormat::SevenZip),
        "gz" => {
            // 单独的 .gz 文件（非 .tar.gz）
            if filename.ends_with(".tar.gz") {
                Some(ArchiveFormat::TarGz)
            } else {
                None // 单独的 gzip 文件暂不支持
            }
        }
        _ => None,
    }
}

/// 根据路径找到最匹配的挂载点
fn get_storage_by_path<'a>(path: &str, mounts: &'a [MountInfo]) -> Option<&'a MountInfo> {
    let mut best_match: Option<&MountInfo> = None;
    let mut best_len = 0;
    
    for mount in mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        if is_sub_path(&mount_path, path) {
            if mount_path.len() > best_len {
                best_match = Some(mount);
                best_len = mount_path.len();
            }
        }
    }
    best_match
}

/// 获取用户权限
async fn get_user_permissions(state: &AppState, cookies: &Cookies) -> UserPermissions {
    let session_id = match cookies.get(SESSION_COOKIE_NAME) {
        Some(c) => {
            debug!("解压API有session cookie: {}", c.value());
            c.value().to_string()
        },
        None => {
            debug!("解压API无session cookie，使用游客权限");
            return get_guest_permissions(state).await;
        }
    };
    
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
            debug!("解压API用户权限: is_admin={}, extract_files={}", p.is_admin, p.extract_files);
            p
        },
        None => {
            warn!("解压API session有效但未找到权限数据，使用游客权限");
            get_guest_permissions(state).await
        }
    }
}

/// 获取当前用户ID（users.id 是 TEXT 类型）
async fn get_current_user_id(state: &AppState, cookies: &Cookies) -> Option<String> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)?.value().to_string();
    
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT u.id FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now')"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .ok()?;
    
    result.map(|(id,)| id)
}

/// 获取游客权限
async fn get_guest_permissions(state: &AppState) -> UserPermissions {
    sqlx::query_as::<_, UserPermissions>(
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
    .await
    .ok()
    .flatten()
    .unwrap_or_default()
}

/// POST /api/fs/extract - 解压缩文件
/// 
/// 这是一个独立的解压缩API，只在用户明确触发时执行
/// 不会在预览压缩包时自动执行
pub async fn extract_archive(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(req): Json<ExtractRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let src_path = fix_and_clean_path(&req.src_path);
    let dst_path = fix_and_clean_path(&req.dst_path);
    
    info!("解压缩请求: {} -> {}", src_path, dst_path);
    
    // 检查权限
    let perms = get_user_permissions(&state, &cookies).await;
    
    info!("解压权限检查: extract_files={}", perms.extract_files);
    
    // 根据用户组权限判断
    if !perms.extract_files {
        warn!("解压缩权限不足: extract_files={}", perms.extract_files);
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "code": 403,
                "message": "没有解压缩权限"
            }))
        ));
    }
    
    // 检查源文件是否为支持的压缩格式
    let filename = src_path.split('/').last().unwrap_or("").to_string();
    let filename_lower = filename.to_lowercase();
    
    // 获取压缩格式
    let archive_format = get_archive_format(&filename_lower);
    if archive_format.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "code": 400,
                "message": "不支持的压缩格式，支持: zip, tar, tar.gz, tgz, tar.bz2, 7z"
            }))
        ));
    }
    let _archive_format = archive_format.unwrap();
    
    // 从 drivers 表读取挂载点信息
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": 500, "message": e.to_string() }))
    ))?;
    
    // 构建挂载点列表
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
        })
    }).collect();
    
    // 找到源文件的挂载点
    debug!("查找源文件挂载点: {}", src_path);
    let src_mount = get_storage_by_path(&src_path, &mounts).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({ "code": 404, "message": "未找到源文件的挂载点" }))
    ))?;
    debug!("源挂载点: {} -> {}", src_mount.id, src_mount.mount_path);
    
    // 找到目标目录的挂载点
    debug!("查找目标目录挂载点: {}", dst_path);
    let dst_mount = get_storage_by_path(&dst_path, &mounts).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({ "code": 404, "message": "未找到目标目录的挂载点" }))
    ))?;
    debug!("目标挂载点: {} -> {}", dst_mount.id, dst_mount.mount_path);
    
    // 获取源驱动
    debug!("获取源驱动: {}", src_mount.id);
    let _src_driver = state.storage_manager.get_driver(&src_mount.id).await
        .ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": 404, "message": "源存储驱动不存在" }))
        ))?;
    debug!("源驱动获取成功");
    
    // 计算驱动内部的实际路径（去掉挂载点前缀）
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let src_actual_path = if src_path.len() > src_mount_path.len() {
        fix_and_clean_path(&src_path[src_mount_path.len()..])
    } else {
        "/".to_string()
    };
    
    // 获取压缩文件信息（使用驱动内部路径）
    let parent_path = src_actual_path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let parent_path = if parent_path.is_empty() { "/" } else { parent_path };
    debug!("列出目录（驱动内部路径）: {}", parent_path);
    let entries = _src_driver.list(parent_path).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"code": 500, "message": format!("获取目录列表失败: {}", e)}))))?;
    debug!("目录列表获取成功, {} 个条目", entries.len());
    
    let filename = src_path.split('/').last().unwrap_or("");
    let file_entry = entries.iter().find(|e| e.name == filename)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"code": 404, "message": "源文件不存在"}))))?;
    
    // 验证是否为支持的压缩格式
    let archive_format = get_archive_format(&file_entry.name)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"code": 400, "message": "不支持的压缩格式"}))))?;
    
    // 获取目标驱动
    let _dst_driver = state.storage_manager.get_driver(&dst_mount.id).await
        .ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": 404, "message": "目标存储驱动不存在" }))
        ))?;
    
    // 计算实际路径
    let src_mount_path = fix_and_clean_path(&src_mount.mount_path);
    let src_actual_path = if src_path.len() > src_mount_path.len() {
        fix_and_clean_path(&src_path[src_mount_path.len()..])
    } else {
        "/".to_string()
    };
    
    let dst_mount_path = fix_and_clean_path(&dst_mount.mount_path);
    let dst_actual_path = if dst_path.len() > dst_mount_path.len() {
        fix_and_clean_path(&dst_path[dst_mount_path.len()..])
    } else {
        "/".to_string()
    };
    
    // 获取当前用户ID（用于WebSocket事件过滤）
    let user_id = get_current_user_id(&state, &cookies).await;
    
    // 创建解压缩任务
    let archive_name = filename.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or_else(|| filename.to_string());
    let task_name = format!("解压 {} 到 {}", filename, dst_path);
    
    let task = Task::new(
        TaskType::Extract,
        task_name,
        src_path.clone(),
        Some(dst_path.clone()),
        0, // 总大小稍后更新
        0, // 总文件数稍后更新
        user_id,
    );
    
    let task_id = task.id.clone();
    
    // 添加任务到任务管理器并启动
    state.task_manager.add_task(task.clone()).await;
    state.task_manager.start_task(&task_id).await;
    
    // 创建任务控制标志
    let control = state.task_manager.create_control(&task_id).await;
    
    // 克隆需要的变量用于异步任务
    let state_clone = state.clone();
    let src_driver_id = src_mount.id.clone();
    let dst_driver_id = dst_mount.id.clone();
    let task_id_clone = task_id.clone();
    let put_into_new_dir = req.put_into_new_dir;
    let overwrite = req.overwrite;
    let force = req.force;
    let password = req.password.clone();
    let inner_path = req.inner_path.clone();
    let encoding = req.encoding.clone();
    
    // 在后台执行解压缩
    tokio::spawn(async move {
        let result = do_extract(
            &state_clone,
            &src_driver_id,
            &dst_driver_id,
            &src_actual_path,
            &dst_actual_path,
            &archive_name,
            archive_format,
            put_into_new_dir,
            overwrite,
            force,
            &password,
            &inner_path,
            &encoding,
            &task_id_clone,
            control,
        ).await;
        
        match result {
            Ok(count) => {
                info!("解压缩完成: {} 个文件", count);
                state_clone.task_manager.complete_task(&task_id_clone).await;
            }
            Err(e) => {
                if e.contains("已取消") {
                    info!("解压缩任务已取消");
                } else {
                    error!("解压缩失败: {}", e);
                    state_clone.task_manager.fail_task(&task_id_clone, e).await;
                }
            }
        }
        // 清理控制标志
        state_clone.task_manager.remove_control(&task_id_clone).await;
    });
    
    Ok(Json(json!({
        "code": 200,
        "message": "解压缩任务已创建",
        "data": {
            "task_id": task_id
        }
    })))
}

/// 执行实际的解压缩操作
/// 
/// 架构原则：
/// - 所有文件操作通过 StorageDriver 接口（open_reader, open_writer, create_dir）
/// - Core 层负责进度/暂停/取消逻辑
/// - Driver 只提供原语能力
async fn do_extract(
    state: &AppState,
    src_driver_id: &str,
    dst_driver_id: &str,
    src_path: &str,
    dst_path: &str,
    archive_name: &str,
    archive_format: ArchiveFormat,
    put_into_new_dir: bool,
    overwrite: bool,
    force: bool,
    _password: &Option<String>,
    inner_path: &Option<String>,
    encoding: &str,
    task_id: &str,
    control: std::sync::Arc<crate::task::TaskControl>,
) -> Result<u64, String> {
    let src_driver = state.storage_manager.get_driver(src_driver_id).await
        .ok_or("源驱动不可用")?;
    let dst_driver = state.storage_manager.get_driver(dst_driver_id).await
        .ok_or("目标驱动不可用")?;
    
    let base_dst_path = if put_into_new_dir {
        format!("{}/{}", dst_path.trim_end_matches('/'), archive_name)
    } else {
        dst_path.to_string()
    };
    
    // 获取源文件大小（通过 driver.list）
    let parent_path = src_path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let file_name = src_path.split('/').last().unwrap_or("");
    let entries_list = src_driver.list(parent_path).await
        .map_err(|e| format!("获取文件信息失败: {}", e))?;
    let file_entry = entries_list.iter().find(|e| e.name == file_name)
        .ok_or("源文件不存在")?;
    let file_size = file_entry.size;
    
    state.task_manager.update_task_size(task_id, file_size, 0).await;
    
    let inner_path_str = inner_path.as_ref().map(|s| s.trim_matches('/')).unwrap_or("");
    
    // 统一流程：通过 Driver 接口读取压缩包数据
    // ZIP/7Z 需要 Seek，所以先读取到内存或临时文件
    do_extract_via_driver(
        &src_driver, &dst_driver, src_path, &base_dst_path,
        archive_format, put_into_new_dir, overwrite, force, inner_path_str, encoding,
        file_size, state, task_id, control
    ).await
}

/// 
/// 流程（全部本地缓存，不读入内存）：
/// 1. 通过 src_driver.open_reader() 流式下载到临时文件
/// 2. 从临时文件解压到临时目录
/// 3. 通过 dst_driver.open_writer() 上传解压后的文件
async fn do_extract_via_driver(
    src_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    dst_driver: &std::sync::Arc<Box<dyn yaolist_backend::storage::StorageDriver>>,
    src_path: &str,
    base_dst_path: &str,
    archive_format: ArchiveFormat,
    put_into_new_dir: bool,
    overwrite: bool,
    force: bool,
    inner_path: &str,
    encoding: &str,
    file_size: u64,
    state: &AppState,
    task_id: &str,
    control: std::sync::Arc<crate::task::TaskControl>,
) -> Result<u64, String> {
    // 创建临时目录
    let temp_dir = TempDir::new().map_err(|e| format!("创建临时目录失败: {}", e))?;
    let temp_archive = temp_dir.path().join("archive");
    let temp_extract = temp_dir.path().join("out");
    std::fs::create_dir_all(&temp_extract).map_err(|e| format!("创建解压目录失败: {}", e))?;
    
    // 检查磁盘空间（预留 2.5 倍）
    let available = fs2::available_space(temp_dir.path()).unwrap_or(0);
    let required = (file_size as f64 * 2.5) as u64;
    if available < required && !force {
        return Err(format!("DISK_SPACE_WARNING:磁盘空间可能不足: 可用 {}MB，建议 {}MB。可强制继续。", 
            available / 1024 / 1024, required / 1024 / 1024));
    }
    
    // 记录开始时间用于ETA计算
    let start_time = std::time::Instant::now();
    
    // ========== 阶段1: 下载文件到本地 (0-30%) ==========
    // 先发送初始状态
    update_extract_progress(state, task_id, 0.0, 0.0, 0, 
        &format!("下载中... (0/{})", format_size(file_size)), 0, 0).await;
    
    let mut reader = src_driver.open_reader(src_path, None).await
        .map_err(|e| format!("打开压缩包失败: {}", e))?;
    let mut temp_file = tokio::fs::File::create(&temp_archive).await
        .map_err(|e| format!("创建临时文件失败: {}", e))?;
    
    let mut downloaded = 0u64;
    let mut buf = vec![0u8; 1024 * 1024]; // 1MB buffer
    let mut last_update = std::time::Instant::now();
    let mut last_downloaded = 0u64;
    
    loop {
        // 检查取消
        if control.is_cancelled() {
            return Err("任务已取消".to_string());
        }
        // 检查暂停
        while control.is_paused() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if control.is_cancelled() {
                return Err("任务已取消".to_string());
            }
        }
        
        let n = reader.read(&mut buf).await.map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 { break; }
        
        temp_file.write_all(&buf[..n]).await.map_err(|e| format!("写入临时文件失败: {}", e))?;
        downloaded += n as u64;
        
        // 每1秒更新一次进度
        let now = std::time::Instant::now();
        if now.duration_since(last_update).as_millis() >= 1000 {
            let elapsed_ms = now.duration_since(last_update).as_millis() as f64;
            let bytes_delta = downloaded - last_downloaded;
            let speed = (bytes_delta as f64 / elapsed_ms) * 1000.0; // bytes/sec
            
            let progress = (downloaded as f32 / file_size as f32) * 30.0;
            let total_elapsed = start_time.elapsed().as_secs_f64();
            let eta = if progress > 0.0 {
                ((total_elapsed / progress as f64) * (100.0 - progress as f64)) as u64
            } else { 0 };
            
            // 格式化下载速度显示
            let speed_str = format_speed(speed);
            let status = format!("下载中... {} ({}/{})", speed_str, 
                format_size(downloaded), format_size(file_size));
            
            update_extract_progress(state, task_id, progress, speed, eta, &status, 0, 0).await;
            
            last_update = now;
            last_downloaded = downloaded;
        }
    }
    temp_file.shutdown().await.ok();
    
    // 下载完成状态
    let download_elapsed = start_time.elapsed().as_secs_f64();
    update_extract_progress(state, task_id, 30.0, 0.0, 0, 
        &format!("下载完成 ({:.1}s)", download_elapsed), 0, 0).await;
    
    // ========== 阶段2: 解压缩 (30-60%) ==========
    let extract_start = std::time::Instant::now();
    update_extract_progress(state, task_id, 30.0, 0.0, 0, "解压缩中...", 0, 0).await;
    
    // 使用 channel 传递解压进度（非阻塞）
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<(u64, u64, String)>(1000);
    
    let arc_path = temp_archive.clone();
    let ext_path = temp_extract.clone();
    let fmt = archive_format;
    let inner = inner_path.to_string();
    let enc = encoding.to_string();
    let ow = overwrite;
    let ctrl = control.clone();
    
    // 启动解压任务
    let extract_handle = tokio::task::spawn_blocking(move || {
        extract_to_local_with_progress(&arc_path, &ext_path, fmt, &inner, &enc, ow, &ctrl, progress_tx)
    });
    
    // 异步接收进度更新（Core 层处理进度）
    // 注意：需要克隆 Arc 而不是借用，因为 tokio::spawn 需要 'static 生命周期
    let task_manager = state.task_manager.clone();
    let task_id_for_progress = task_id.to_string();
    let start_time_clone = start_time;
    let progress_handle = tokio::spawn(async move {
        while let Some((processed, total, _current_file)) = progress_rx.recv().await {
            let extract_progress = if total > 0 { processed as f32 / total as f32 } else { 0.0 };
            let total_progress = 30.0 + extract_progress * 30.0; // 30-60%
            
            let total_elapsed = start_time_clone.elapsed().as_secs_f64();
            let eta = if total_progress > 0.0 {
                ((total_elapsed / total_progress as f64) * (100.0 - total_progress as f64)) as u64
            } else { 0 };
            
            let status = format!("解压缩中... ({}/{})", processed, total);
            task_manager.update_extract_task_progress(
                &task_id_for_progress, total_progress, 0.0, eta, &status, processed, total
            ).await;
        }
    });
    
    // 等待解压完成
    let extract_result = extract_handle.await.map_err(|e| format!("解压任务失败: {}", e))?;
    // 等待进度更新完成
    let _ = progress_handle.await;
    
    let file_count = extract_result?;
    let extract_elapsed = extract_start.elapsed().as_secs_f64();
    let total_elapsed = start_time.elapsed().as_secs_f64();
    let eta = if total_elapsed > 0.0 {
        ((total_elapsed / 60.0) * 40.0) as u64
    } else { 0 };
    
    let status = format!("解压完成: {} 个文件 ({:.1}s)", file_count, extract_elapsed);
    update_extract_progress(state, task_id, 60.0, 0.0, eta, &status, file_count, file_count).await;
    
    // ========== 阶段3: 上传到目标 (60-100%) ==========
    
    if put_into_new_dir {
        dst_driver.create_dir(base_dst_path).await.ok();
    }
    
    // 收集所有文件用于上传
    let all_files = collect_local_files(&temp_extract, "")?;
    let total_files_to_upload = all_files.len() as u64;
    
    let mut uploaded_count = 0u64;
    let mut uploaded_bytes = 0u64;
    let mut last_update = std::time::Instant::now();
    let mut last_bytes = 0u64;
    
    for (local_path, relative_path) in &all_files {
        // 检查取消/暂停
        if control.is_cancelled() { return Err("任务已取消".to_string()); }
        while control.is_paused() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if control.is_cancelled() { return Err("任务已取消".to_string()); }
        }
        
        let remote_path = format!("{}/{}", base_dst_path.trim_end_matches('/'), relative_path);
        
        if local_path.is_dir() {
            dst_driver.create_dir(&remote_path).await.ok();
        } else {
            // 确保父目录存在
            if let Some(parent) = std::path::Path::new(&remote_path).parent() {
                let parent_str = parent.to_string_lossy().replace('\\', "/");
                if !parent_str.is_empty() && parent_str != "/" && parent_str != base_dst_path {
                    dst_driver.create_dir(&parent_str).await.ok();
                }
            }
            
            // 读取并上传
            let content = tokio::fs::read(local_path).await.map_err(|e| e.to_string())?;
            let file_size = content.len() as u64;
            
            let mut writer = dst_driver.open_writer(&remote_path, Some(file_size), None).await
                .map_err(|e| format!("创建文件失败: {}", e))?;
            writer.write_all(&content).await.map_err(|e| format!("上传失败: {}", e))?;
            writer.shutdown().await.ok();
            
            uploaded_count += 1;
            uploaded_bytes += file_size;
        }
        
        // 更新进度（每1秒）
        let now = std::time::Instant::now();
        if now.duration_since(last_update).as_millis() >= 1000 || uploaded_count == total_files_to_upload {
            let elapsed_ms = now.duration_since(last_update).as_millis().max(1) as f64;
            let bytes_delta = uploaded_bytes - last_bytes;
            let speed = (bytes_delta as f64 / elapsed_ms) * 1000.0;
            
            let upload_progress = uploaded_count as f32 / total_files_to_upload as f32;
            let total_progress = 60.0 + upload_progress * 40.0;
            
            let total_elapsed = start_time.elapsed().as_secs_f64();
            let eta = if total_progress > 0.0 {
                ((total_elapsed / total_progress as f64) * (100.0 - total_progress as f64)) as u64
            } else { 0 };
            
            let speed_str = format_speed(speed);
            let status = format!("上传中... {} ({}/{})", speed_str, uploaded_count, total_files_to_upload);
            
            update_extract_progress(state, task_id, total_progress, speed, eta, &status, 
                uploaded_count, total_files_to_upload).await;
            
            last_update = now;
            last_bytes = uploaded_bytes;
        }
    }
    
    Ok(uploaded_count)
}

/// 递归收集本地目录中的所有文件
fn collect_local_files(dir: &Path, prefix: &str) -> Result<Vec<(std::path::PathBuf, String)>, String> {
    let mut files = Vec::new();
    
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
        
        if path.is_dir() {
            files.push((path.clone(), relative.clone()));
            files.extend(collect_local_files(&path, &relative)?);
        } else {
            files.push((path, relative));
        }
    }
    
    Ok(files)
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2}GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

/// 格式化速度
fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0}B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1}KB/s", bytes_per_sec / 1024.0)
    } else if bytes_per_sec < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1}MB/s", bytes_per_sec / 1024.0 / 1024.0)
    } else {
        format!("{:.2}GB/s", bytes_per_sec / 1024.0 / 1024.0 / 1024.0)
    }
}

/// 更新解压任务进度（通过 TaskManager 公共方法）
async fn update_extract_progress(
    state: &AppState,
    task_id: &str,
    progress: f32,
    speed: f64,
    eta: u64,
    current_file: &str,
    processed_files: u64,
    total_files: u64,
) {
    debug!("解压进度更新: task={}, progress={:.1}%, status={}", task_id, progress, current_file);
    state.task_manager.update_extract_task_progress(
        task_id, progress, speed, eta, current_file, processed_files, total_files
    ).await;
}
