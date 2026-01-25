use serde::{Deserialize, Serialize};
use chrono::Utc;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tower_cookies::Cookies;
use futures::stream::{self, StreamExt};

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;
use super::types::*;

/// 验证管理员权限
async fn require_admin(state: &AppState, cookies: &Cookies) -> Result<(), (StatusCode, Json<Value>)> {
    let session_id = cookies.get(SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "未登录"}))))?;
    
    let is_admin: Option<bool> = sqlx::query_scalar(
        "SELECT u.is_admin FROM users u 
         JOIN sessions s ON u.id = s.user_id 
         WHERE s.id = ? AND s.expires_at > datetime('now') AND u.enabled = 1"
    )
    .bind(&session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "服务器错误"}))))?;
    
    if !is_admin.unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "需要管理员权限"}))));
    }
    Ok(())
}


/// 获取搜索用的用户权限
async fn get_search_permissions(state: &AppState, cookies: &Cookies) -> SearchUserPermissions {
    let session_id = match cookies.get(SESSION_COOKIE_NAME) {
        Some(c) => c.value().to_string(),
        None => {
            // 游客权限
            return sqlx::query_as::<_, SearchUserPermissions>(
                "SELECT show_hidden_files FROM user_groups WHERE name = '游客组'"
            )
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        }
    };
    
    sqlx::query_as::<_, SearchUserPermissions>(
        r#"SELECT MAX(g.show_hidden_files) as show_hidden_files
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
    .flatten()
    .unwrap_or_default()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchSettings {
    pub enabled: bool,
    pub auto_update_index: bool,
    pub ignore_paths: String,
    pub max_index_depth: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IndexStatus {
    pub status: String,
    pub object_count: u64,
    pub index_size: u64,
    pub last_updated: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 200,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            code: 500,
            message: message.to_string(),
            data: None,
        }
    }
}

pub async fn get_search_settings(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<ApiResponse<SearchSettings>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let result = sqlx::query_as::<_, (bool, bool, String, i32)>(
        "SELECT enabled, auto_update_index, ignore_paths, max_index_depth FROM search_settings WHERE id = 1"
    )
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some((enabled, auto_update, ignore_paths, max_depth))) => {
            Ok(Json(ApiResponse::success(SearchSettings {
                enabled,
                auto_update_index: auto_update,
                ignore_paths,
                max_index_depth: max_depth,
            })))
        }
        Ok(None) => {
            // 返回默认设置
            Ok(Json(ApiResponse::success(SearchSettings {
                enabled: false,
                auto_update_index: true,
                ignore_paths: String::new(),
                max_index_depth: 20,
            })))
        }
        Err(e) => {
            tracing::error!("Failed to get search settings: {}", e);
            Ok(Json(ApiResponse::error(&format!("获取设置失败: {}", e))))
        }
    }
}

/// 公开API：检查搜索功能是否启用
pub async fn is_search_enabled(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<bool>> {
    let result = sqlx::query_as::<_, (bool,)>(
        "SELECT enabled FROM search_settings WHERE id = 1"
    )
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some((enabled,))) => Json(ApiResponse::success(enabled)),
        Ok(None) => Json(ApiResponse::success(false)),
        Err(_) => Json(ApiResponse::success(false)),
    }
}

pub async fn update_search_settings(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(settings): Json<SearchSettings>,
) -> Result<Json<ApiResponse<SearchSettings>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let now = Utc::now().to_rfc3339();
    
    let result = sqlx::query(
        r#"
        INSERT INTO search_settings (id, enabled, auto_update_index, ignore_paths, max_index_depth, updated_at)
        VALUES (1, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            enabled = excluded.enabled,
            auto_update_index = excluded.auto_update_index,
            ignore_paths = excluded.ignore_paths,
            max_index_depth = excluded.max_index_depth,
            updated_at = excluded.updated_at
        "#
    )
    .bind(settings.enabled)
    .bind(settings.auto_update_index)
    .bind(&settings.ignore_paths)
    .bind(settings.max_index_depth)
    .bind(&now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            tracing::info!("Search settings saved: enabled={}, auto_update={}", settings.enabled, settings.auto_update_index);
            Ok(Json(ApiResponse::success(settings)))
        }
        Err(e) => {
            tracing::error!("Failed to save search settings: {}", e);
            Ok(Json(ApiResponse::error(&format!("保存失败: {}", e))))
        }
    }
}

pub async fn get_index_status(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<ApiResponse<IndexStatus>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    let progress = state.index_state.get_progress();
    
    // 检查是否有任何存储的索引数据库
    let driver_dbs = yaolist_backend::search::DbIndex::list_driver_dbs();
    let has_index = !driver_dbs.is_empty();
    
    // 获取所有存储的索引统计
    let index_size = yaolist_backend::search::DbIndex::get_all_driver_db_size();
    
    // 统计所有存储的文件和目录数
    let mut total_files: u64 = 0;
    let mut total_dirs: u64 = 0;
    let mut latest_updated: Option<i64> = None;
    
    for driver_id in &driver_dbs {
        if let Ok(db_index) = yaolist_backend::search::DbIndex::new_for_driver(driver_id).await {
            let stats = db_index.get_stats().await;
            total_files += stats.file_count;
            total_dirs += stats.dir_count;
            if let Some(ts) = stats.last_updated {
                latest_updated = Some(latest_updated.map_or(ts, |prev| prev.max(ts)));
            }
            db_index.close().await;
        }
    }
    
    let stats = yaolist_backend::search::db_index::IndexStats {
        file_count: total_files,
        dir_count: total_dirs,
        total_size: 0,
        last_updated: latest_updated,
    };
    
    let status = if progress.is_running {
        "indexing"
    } else if progress.error.is_some() {
        "error"
    } else if !has_index {
        "not_built"
    } else {
        "idle"
    };

    // 构建中使用实时进度，否则使用保存的统计
    let object_count = if progress.is_running {
        progress.object_count
    } else {
        stats.file_count + stats.dir_count
    };

    let last_updated = if progress.is_running {
        None // 构建中不显示更新时间
    } else {
        stats.last_updated.map(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        })
    };

    let index_status = IndexStatus {
        status: status.to_string(),
        object_count,
        index_size, // 数据库文件大小
        last_updated,
        error_message: progress.error,
    };
    Ok(Json(ApiResponse::success(index_status)))
}

/// 供其他模块调用的索引重建函数
pub async fn trigger_rebuild_index(state: Arc<AppState>) -> Result<(), String> {
    // 检查是否已经在运行
    if state.index_state.is_running() {
        return Err("索引正在构建中".to_string());
    }
    
    do_rebuild_index(state).await
}

pub async fn rebuild_index(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    // 检查是否已经在运行
    if state.index_state.is_running() {
        return Ok(Json(ApiResponse::error("索引正在构建中")));
    }
    
    match do_rebuild_index(state).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Ok(Json(ApiResponse::error(&e))),
    }
}

async fn do_rebuild_index(state: Arc<AppState>) -> Result<(), String> {
    tracing::info!("开始构建索引");
    
    let drivers_result = sqlx::query_as::<_, (String, String)>(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await;

    let drivers = match drivers_result {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to get driver list: {}", e);
            return Err(format!("获取驱动列表失败: {}", e));
        }
    };

    // 解析驱动配置，提取 mount_path
    let mut driver_mounts: Vec<(String, String)> = Vec::new();
    for (driver_name, config_str) in drivers {
        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&config_str) {
            let mount_path = config.get("mount_path")
                .and_then(|v| v.as_str())
                .unwrap_or("/")
                .to_string();
            driver_mounts.push((driver_name, mount_path));
        }
    }

    if driver_mounts.is_empty() {
        tracing::warn!("没有启用的驱动");
        return Err("没有启用的驱动".to_string());
    }

    // 启动后台索引任务
    state.index_state.start();
    let state_clone = state.clone();
    
    tokio::spawn(async move {
        // 删除所有旧的存储索引数据库
        yaolist_backend::search::DbIndex::delete_all_driver_db_files();
        
        let total_files = Arc::new(AtomicU64::new(0));
        let total_dirs = Arc::new(AtomicU64::new(0));
        
        // 并发索引：每个存储一个独立数据库，最多8个并发
        let max_concurrent = std::cmp::min(driver_mounts.len(), 8);
        tracing::info!("开始并发索引，存储数量: {}, 并发数: {}", driver_mounts.len(), max_concurrent);
        
        stream::iter(driver_mounts)
            .for_each_concurrent(max_concurrent, |(driver_id, mount_path)| {
                let state_ref = state_clone.clone();
                let total_files_ref = total_files.clone();
                let total_dirs_ref = total_dirs.clone();
                
                async move {
                    if state_ref.index_state.is_cancelled() {
                        tracing::info!("Indexing cancelled for driver: {}", driver_id);
                        return;
                    }
                    
                    tracing::info!("Indexing driver: {} (mount point: {})", driver_id, mount_path);
                    
                    // 为每个存储创建独立的数据库
                    let db_index = match yaolist_backend::search::DbIndex::new_for_driver(&driver_id).await {
                        Ok(idx) => Arc::new(idx),
                        Err(e) => {
                            tracing::error!("Failed to create search database for driver {}: {}", driver_id, e);
                            return;
                        }
                    };
                    
                    // 初始化表结构
                    if let Err(e) = db_index.init().await {
                        tracing::error!("Failed to init index tables for driver {}: {}", driver_id, e);
                        return;
                    }
                    
                    // 使用独立数据库索引
                    match index_directory_to_db(&state_ref, &db_index, &driver_id, &mount_path, "/", 0, 20).await {
                        Ok((files, dirs)) => {
                            total_files_ref.fetch_add(files, Ordering::SeqCst);
                            total_dirs_ref.fetch_add(dirs, Ordering::SeqCst);
                            
                            // 保存该存储的索引更新时间
                            if let Err(e) = db_index.set_last_updated().await {
                                tracing::warn!("Failed to save index update time for driver {}: {}", driver_id, e);
                            }
                            
                            tracing::info!("Driver {} indexing completed, {} files/{} directories", driver_id, files, dirs);
                        }
                        Err(e) => {
                            tracing::error!("Failed to index driver {}: {}", driver_id, e);
                        }
                    }
                    
                    // 关闭数据库连接
                    db_index.close().await;
                }
            })
            .await;
        
        let total_files = total_files.load(Ordering::SeqCst);
        let total_dirs = total_dirs.load(Ordering::SeqCst);
        
        tracing::info!("Indexing completed, {} files/{} directories indexed", total_files, total_dirs);
        state_clone.index_state.finish(None);
    });

    Ok(())
}

/// 使用数据库索引目录
async fn index_directory_to_db(
    state: &Arc<AppState>,
    db_index: &Arc<yaolist_backend::search::DbIndex>,
    driver_id: &str,
    mount_path: &str,
    path: &str,
    depth: i32,
    max_depth: i32,
) -> Result<(u64, u64), String> {
    if depth > max_depth {
        return Ok((0, 0));
    }

    if state.index_state.is_cancelled() {
        return Ok((0, 0));
    }

    // 获取驱动
    let driver = match state.storage_manager.get_driver(driver_id).await {
        Some(d) => d,
        None => return Err(format!("驱动不存在: {}", driver_id)),
    };

    // 列出文件
    let files = match driver.list(path).await {
        Ok(f) => f,
        Err(e) => {
            return Err(format!("列出目录失败: {}", e));
        }
    };

    let mut file_count = 0u64;
    let mut dir_count = 0u64;
    let mut batch: Vec<(String, String, bool, i64, i64)> = Vec::with_capacity(2000);

    for file in files {
        if state.index_state.is_cancelled() {
            // 写入剩余批次
            if !batch.is_empty() {
                let _ = db_index.insert_batch(&batch).await;
            }
            return Ok((file_count, dir_count));
        }

        let file_path = if path == "/" {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", path, file.name)
        };
        
        let full_file_path = format!("{}{}", mount_path.trim_end_matches('/'), file_path);

        let modified_ts = file.modified
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);
        
        batch.push((
            full_file_path.clone(),
            file.name.clone(),
            file.is_dir,
            file.size as i64,
            modified_ts,
        ));

        if file.is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
        }
        state.index_state.increment();

        // 批量写入（每2000条）
        if batch.len() >= 2000 {
            if let Err(e) = db_index.insert_batch(&batch).await {
                tracing::warn!("批量写入索引失败: {}", e);
            }
            batch.clear();
        }

        // 如果是目录，递归索引
        if file.is_dir {
            match Box::pin(index_directory_to_db(state, db_index, driver_id, mount_path, &file_path, depth + 1, max_depth)).await {
                Ok((sub_files, sub_dirs)) => {
                    file_count += sub_files;
                    dir_count += sub_dirs;
                }
                Err(e) => tracing::warn!("Failed to index subdirectory {}: {}", file_path, e),
            }
        }
    }

    // 写入剩余批次
    if !batch.is_empty() {
        if let Err(e) = db_index.insert_batch(&batch).await {
            tracing::warn!("Batch index write failed: {}", e);
        }
    }

    Ok((file_count, dir_count))
}

pub async fn clear_index(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    if state.index_state.is_running() {
        return Ok(Json(ApiResponse::error("索引正在构建中，请先停止")));
    }

    // 删除所有存储的索引数据库文件
    yaolist_backend::search::DbIndex::delete_all_driver_db_files();
    
    // 同时删除旧的单一数据库文件（如果存在）
    yaolist_backend::search::DbIndex::delete_db_files();
    
    tracing::info!("索引已清除");
    Ok(Json(ApiResponse::success(())))
}

pub async fn stop_indexing(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<Value>)> {
    require_admin(&state, &cookies).await?;
    
    if !state.index_state.is_running() {
        return Ok(Json(ApiResponse::error("没有正在运行的索引任务")));
    }
    
    state.index_state.cancel();
    tracing::info!("索引任务已请求停止");
    Ok(Json(ApiResponse::success(())))
}
