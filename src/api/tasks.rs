use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::state::AppState;
use crate::auth::SESSION_COOKIE_NAME;

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

/// GET /api/tasks - 获取当前用户的任务列表（轮询用）
pub async fn get_tasks(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, StatusCode> {
    let user_id = get_current_user_id(&state, &cookies).await;
    let mut tasks = state.task_manager.get_user_tasks(user_id).await;
    
    // 按创建时间倒序排列（最新的在前）
    tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    
    // 转换为TaskSummary格式
    let task_list: Vec<Value> = tasks.iter().map(|task| {
        json!({
            "id": task.id,
            "task_type": task.task_type,
            "status": task.status,
            "name": task.name,
            "source_path": task.source_path,
            "target_path": task.target_path,
            "total_size": task.total_size,
            "processed_size": task.processed_size,
            "total_files": task.total_files,
            "processed_files": task.processed_files,
            "progress": task.progress,
            "speed": task.speed,
            "eta_seconds": task.eta_seconds,
            "created_at": task.created_at,
            "started_at": task.started_at,
            "finished_at": task.finished_at,
            "error": task.error,
            "current_file": task.current_file
        })
    }).collect();
    
    Ok(Json(json!({
        "code": 200,
        "data": task_list
    })))
}

/// 任务列表查询参数
#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub task_type: Option<String>,
    pub status: Option<String>,
    pub user_id: Option<String>,  // 管理员可以查看指定用户的任务
}

/// POST /api/tasks/list - 获取任务列表（支持分页和筛选）
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    Json(query): Json<ListTasksQuery>,
) -> Result<Json<Value>, StatusCode> {
    let current_user_id = get_current_user_id(&state, &cookies).await;
    
    // 检查是否是管理员
    let is_admin = if let Some(ref uid) = current_user_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT is_admin FROM users WHERE id = ?"
        )
        .bind(uid)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
    } else {
        false
    };
    
    // 获取所有任务
    let all_tasks = if is_admin {
        // 管理员可以查看所有任务或指定用户的任务
        if let Some(ref filter_user_id) = query.user_id {
            state.task_manager.get_user_tasks(Some(filter_user_id.clone())).await
        } else {
            state.task_manager.get_all_tasks().await
        }
    } else {
        // 普通用户只能查看自己的任务
        state.task_manager.get_user_tasks(current_user_id.clone()).await
    };
    
    // 应用筛选
    let filtered_tasks: Vec<_> = all_tasks.into_iter()
        .filter(|t| {
            // 筛选任务类型
            if let Some(ref task_type) = query.task_type {
                let type_str = format!("{:?}", t.task_type).to_lowercase();
                if !type_str.contains(&task_type.to_lowercase()) {
                    return false;
                }
            }
            // 筛选状态
            if let Some(ref status) = query.status {
                let status_str = format!("{:?}", t.status).to_lowercase();
                if !status_str.contains(&status.to_lowercase()) {
                    return false;
                }
            }
            true
        })
        .collect();
    
    let total = filtered_tasks.len();
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let total_pages = (total as f64 / page_size as f64).ceil() as u32;
    
    // 分页
    let start = ((page - 1) * page_size) as usize;
    let paginated_tasks: Vec<_> = filtered_tasks.into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();
    
    // 获取用户名映射
    let mut user_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for task in &paginated_tasks {
        if let Some(ref uid) = task.user_id {
            if !user_names.contains_key(uid) {
                if let Ok(Some((username,))) = sqlx::query_as::<_, (String,)>(
                    "SELECT username FROM users WHERE id = ?"
                )
                .bind(uid)
                .fetch_optional(&state.db)
                .await {
                    user_names.insert(uid.clone(), username);
                }
            }
        }
    }
    
    // 构建带用户名的任务列表
    let tasks_with_users: Vec<Value> = paginated_tasks.iter().map(|task| {
        let username = task.user_id.as_ref()
            .and_then(|uid| user_names.get(uid))
            .cloned()
            .unwrap_or_else(|| "游客".to_string());
        json!({
            "id": task.id,
            "task_type": task.task_type,
            "status": task.status,
            "name": task.name,
            "source_path": task.source_path,
            "target_path": task.target_path,
            "total_size": task.total_size,
            "processed_size": task.processed_size,
            "total_files": task.total_files,
            "processed_files": task.processed_files,
            "progress": task.progress,
            "speed": task.speed,
            "eta_seconds": task.eta_seconds,
            "created_at": task.created_at,
            "started_at": task.started_at,
            "finished_at": task.finished_at,
            "error": task.error,
            "user_id": task.user_id,
            "username": username,
            "current_file": task.current_file
        })
    }).collect();
    
    Ok(Json(json!({
        "code": 200,
        "data": {
            "tasks": tasks_with_users,
            "total": total,
            "page": page,
            "page_size": page_size,
            "total_pages": total_pages,
            "is_admin": is_admin
        }
    })))
}

#[derive(Debug, Deserialize)]
pub struct GetTaskReq {
    pub task_id: String,
}

/// POST /api/tasks/get - 获取单个任务
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GetTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(task) = state.task_manager.get_task(&req.task_id).await {
        Ok(Json(json!({
            "code": 200,
            "data": task
        })))
    } else {
        Ok(Json(json!({
            "code": 404,
            "message": "任务不存在"
        })))
    }
}

#[derive(Debug, Deserialize)]
pub struct CancelTaskReq {
    pub task_id: String,
}

/// POST /api/tasks/cancel - 取消任务
pub async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CancelTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if state.task_manager.cancel_task(&req.task_id).await {
        Ok(Json(json!({
            "code": 200,
            "message": "任务已取消"
        })))
    } else {
        Ok(Json(json!({
            "code": 400,
            "message": "无法取消任务"
        })))
    }
}

/// POST /api/tasks/pause - 暂停任务
pub async fn pause_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CancelTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if state.task_manager.pause_task(&req.task_id).await {
        Ok(Json(json!({
            "code": 200,
            "message": "任务已暂停"
        })))
    } else {
        Ok(Json(json!({
            "code": 400,
            "message": "无法暂停任务"
        })))
    }
}

/// POST /api/tasks/resume - 继续任务
pub async fn resume_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CancelTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if state.task_manager.resume_task(&req.task_id).await {
        Ok(Json(json!({
            "code": 200,
            "message": "任务已继续"
        })))
    } else {
        Ok(Json(json!({
            "code": 400,
            "message": "无法继续任务"
        })))
    }
}

/// POST /api/tasks/clear - 清除已完成的任务
pub async fn clear_completed(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, StatusCode> {
    let user_id = get_current_user_id(&state, &cookies).await;
    let count = state.task_manager.clear_completed(user_id).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": format!("已清除 {} 个任务", count),
        "data": {
            "cleared": count
        }
    })))
}

/// POST /api/tasks/clear_all - 管理员清除所有已完成的任务
pub async fn clear_all_completed(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> Result<Json<Value>, StatusCode> {
    let user_id = get_current_user_id(&state, &cookies).await;
    
    // 检查是否是管理员
    let is_admin = if let Some(ref uid) = user_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT is_admin FROM users WHERE id = ?"
        )
        .bind(uid)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
    } else {
        false
    };
    
    if !is_admin {
        return Ok(Json(json!({
            "code": 403,
            "message": "需要管理员权限"
        })));
    }
    
    // 清除所有用户的已完成任务
    let count = state.task_manager.clear_completed(None).await;
    
    Ok(Json(json!({
        "code": 200,
        "message": format!("已清除 {} 个任务", count),
        "data": {
            "cleared": count
        }
    })))
}

#[derive(Debug, Deserialize)]
pub struct RemoveTaskReq {
    pub task_id: String,
}

/// POST /api/tasks/remove - 删除任务
pub async fn remove_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoveTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if state.task_manager.remove_task(&req.task_id).await {
        Ok(Json(json!({
            "code": 200,
            "message": "任务已删除"
        })))
    } else {
        Ok(Json(json!({
            "code": 404,
            "message": "任务不存在"
        })))
    }
}

#[derive(Debug, Deserialize)]
pub struct RetryTaskReq {
    pub task_id: String,
}

/// POST /api/tasks/retry - 重试/恢复中断的任务
pub async fn retry_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RetryTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(task) = state.task_manager.get_task(&req.task_id).await {
        // 只能重试失败、取消或中断的任务
        if task.status == crate::task::TaskStatus::Failed 
            || task.status == crate::task::TaskStatus::Cancelled 
            || task.status == crate::task::TaskStatus::Interrupted {
            
            // 返回任务详情，包括已上传的文件信息
            let pending_files: Vec<Value> = if let Some(files) = &task.files {
                files.iter()
                    .filter(|f| f.status != crate::task::TaskStatus::Completed)
                    .map(|f| json!({
                        "path": f.path,
                        "size": f.size,
                        "uploaded_size": f.uploaded_size,
                        "uploaded_chunks": f.uploaded_chunks
                    }))
                    .collect()
            } else {
                vec![]
            };
            
            Ok(Json(json!({
                "code": 200,
                "message": "请选择对应文件继续上传",
                "data": {
                    "task_id": task.id,
                    "task_type": format!("{:?}", task.task_type).to_lowercase(),
                    "target_path": task.target_path,
                    "total_files": task.total_files,
                    "processed_files": task.processed_files,
                    "total_size": task.total_size,
                    "processed_size": task.processed_size,
                    "pending_files": pending_files,
                    "needFileSelection": task.task_type == crate::task::TaskType::Upload
                }
            })))
        } else {
            Ok(Json(json!({
                "code": 400,
                "message": "只能恢复失败、已取消或已中断的任务"
            })))
        }
    } else {
        Ok(Json(json!({
            "code": 404,
            "message": "任务不存在"
        })))
    }
}

/// POST /api/tasks/restart - 重新启动任务（复制/移动/解压等非上传任务）
pub async fn restart_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RetryTaskReq>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(task) = state.task_manager.get_task(&req.task_id).await {
        // 只能重启失败、取消或中断的任务
        if task.status != crate::task::TaskStatus::Failed 
            && task.status != crate::task::TaskStatus::Cancelled 
            && task.status != crate::task::TaskStatus::Interrupted {
            return Ok(Json(json!({
                "code": 400,
                "message": "只能重启失败、已取消或已中断的任务"
            })));
        }
        
        // 上传任务需要用户重新选择文件，不能直接重启
        if task.task_type == crate::task::TaskType::Upload {
            return Ok(Json(json!({
                "code": 400,
                "message": "上传任务请使用'继续上传'功能"
            })));
        }
        
        // 获取任务执行上下文
        let items = task.items.clone().unwrap_or_default();
        let conflict_strategy = task.conflict_strategy.clone().unwrap_or_else(|| "auto_rename".to_string());
        let source_path = task.source_path.clone();
        let target_path = task.target_path.clone().unwrap_or_default();
        let task_type = task.task_type.clone();
        let task_id = task.id.clone();
        let processed_files = task.processed_files;
        
        if items.is_empty() {
            return Ok(Json(json!({
                "code": 400,
                "message": "任务上下文丢失，无法重启"
            })));
        }
        
        // 重置任务状态（保留已处理进度）
        state.task_manager.restart_task_resume(&task_id, processed_files).await;
        
        // 克隆task_id用于返回
        let task_id_return = task_id.clone();
        
        // 异步重新执行任务
        let state_clone = state.clone();
        tokio::spawn(async move {
            let result = match task_type {
                crate::task::TaskType::Move => {
                    crate::api::files::execute_move_operation_resume(
                        &state_clone, &source_path, &target_path, &items, &task_id, 
                        &conflict_strategy, processed_files
                    ).await
                }
                crate::task::TaskType::Copy => {
                    crate::api::files::execute_copy_operation_resume(
                        &state_clone, &source_path, &target_path, &items, &task_id,
                        &conflict_strategy, processed_files
                    ).await
                }
                _ => {
                    Err(anyhow::anyhow!("不支持重启此类型任务"))
                }
            };
            
            match result {
                Ok(()) => {
                    state_clone.task_manager.complete_task(&task_id).await;
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("cancelled") || err_msg.contains("取消") {
                        state_clone.task_manager.cancel_task(&task_id).await;
                    } else {
                        state_clone.task_manager.fail_task(&task_id, err_msg).await;
                    }
                }
            }
        });
        
        Ok(Json(json!({
            "code": 200,
            "message": "任务已重新启动",
            "data": {
                "task_id": task_id_return
            }
        })))
    } else {
        Ok(Json(json!({
            "code": 404,
            "message": "任务不存在"
        })))
    }
}
