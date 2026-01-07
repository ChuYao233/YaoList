use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use chrono::Utc;

use super::types::{TaskType, TaskStatus, TaskEvent};
use super::models::{Task, TaskSummary, TaskControl, UploadFileInfo};

/// 任务管理器（按用户隔离，支持WebSocket广播）
#[derive(Clone)]
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    controls: Arc<RwLock<HashMap<String, Arc<TaskControl>>>>,
    event_sender: broadcast::Sender<TaskEvent>,
    db: Option<sqlx::SqlitePool>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(256);
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            controls: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            db: None,
        }
    }

    /// 设置数据库连接（用于持久化）
    pub fn set_db(&mut self, db: sqlx::SqlitePool) {
        self.db = Some(db);
    }

    /// 从数据库加载任务到内存
    pub async fn load_tasks_from_db(&self) {
        if let Some(db) = &self.db {
            use sqlx::Row;
            
            let rows = sqlx::query(
                r#"SELECT id, task_type, status, name, source_path, target_path,
                   total_size, processed_size, total_files, processed_files,
                   progress, speed, eta_seconds, created_at, started_at,
                   finished_at, error, user_id, current_file, files, items, conflict_strategy FROM tasks"#
            )
            .fetch_all(db)
            .await
            .unwrap_or_default();

            let mut tasks = self.tasks.write().await;
            for row in rows {
                let id: String = row.get("id");
                let task_type_str: String = row.get("task_type");
                let status_str: String = row.get("status");
                
                let task_type = match task_type_str.as_str() {
                    "upload" => TaskType::Upload,
                    "download" => TaskType::Download,
                    "copy" => TaskType::Copy,
                    "move" => TaskType::Move,
                    "delete" => TaskType::Delete,
                    "extract" => TaskType::Extract,
                    _ => TaskType::Upload,
                };
                
                // 运行中的任务重启后标记为中断
                let status = match status_str.as_str() {
                    "pending" => TaskStatus::Pending,
                    "running" => TaskStatus::Interrupted, // 重启后运行中的任务变为中断
                    "paused" => TaskStatus::Interrupted,  // 暂停的也变为中断
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    "cancelled" => TaskStatus::Cancelled,
                    "interrupted" => TaskStatus::Interrupted,
                    _ => TaskStatus::Pending,
                };
                
                let created_at_str: String = row.get("created_at");
                let started_at_str: Option<String> = row.get("started_at");
                let finished_at_str: Option<String> = row.get("finished_at");
                let current_file: Option<String> = row.try_get("current_file").ok().flatten();
                let files_json: Option<String> = row.try_get("files").ok().flatten();
                let items_json: Option<String> = row.try_get("items").ok().flatten();
                let conflict_strategy: Option<String> = row.try_get("conflict_strategy").ok().flatten();
                
                // 解析files和items字段
                let files: Option<Vec<UploadFileInfo>> = files_json
                    .and_then(|json| serde_json::from_str(&json).ok());
                let items: Option<Vec<String>> = items_json
                    .and_then(|json| serde_json::from_str(&json).ok());
                
                let task = Task {
                    id: id.clone(),
                    task_type,
                    status,
                    name: row.get("name"),
                    source_path: row.get("source_path"),
                    target_path: row.get("target_path"),
                    total_size: row.get::<i64, _>("total_size") as u64,
                    processed_size: row.get::<i64, _>("processed_size") as u64,
                    total_files: row.get::<i64, _>("total_files") as u64,
                    processed_files: row.get::<i64, _>("processed_files") as u64,
                    progress: row.get::<f64, _>("progress") as f32,
                    speed: 0.0, // 重置速度
                    eta_seconds: None, // 重置ETA
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    started_at: started_at_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    finished_at: finished_at_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    error: row.get("error"),
                    user_id: row.get("user_id"),
                    current_file,
                    files,
                    items,
                    conflict_strategy,
                    last_saved: None,
                    last_speed_update_time: None,
                    last_speed_processed_size: 0,
                };
                tasks.insert(id, task);
            }
            tracing::info!("Loaded {} tasks from database", tasks.len());
        }
    }

    /// 订阅任务事件
    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.event_sender.subscribe()
    }

    /// 广播事件
    pub fn broadcast(&self, event: TaskEvent) {
        let _ = self.event_sender.send(event);
    }
    
    /// 添加任务
    pub async fn add_task(&self, task: Task) {
        let mut tasks = self.tasks.write().await;
        tasks.insert(task.id.clone(), task.clone());
        drop(tasks);
        self.save_task_to_db(&task).await;
        self.broadcast(TaskEvent::TaskCreated { task: TaskSummary::from(&task) });
    }
    
    /// 更新任务字段（用于复制/移动操作更新进度）
    pub async fn update_task_progress_files(&self, task_id: &str, processed_files: u64, current_file: Option<String>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.processed_files = processed_files;
            if task.total_files > 0 {
                task.progress = (processed_files as f32 / task.total_files as f32) * 100.0;
            }
            if let Some(cf) = current_file {
                task.current_file = Some(cf);
            }
            let task_clone = task.clone();
            drop(tasks);
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新任务总大小
    pub async fn update_task_total_size(&self, task_id: &str, total_size: u64) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.total_size = total_size;
            let task_clone = task.clone();
            drop(tasks);
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新任务大小和文件数（用于解压缩等任务）
    pub async fn update_task_size(&self, task_id: &str, total_size: u64, total_files: u64) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.total_size = total_size;
            task.total_files = total_files;
            task.status = TaskStatus::Running;
            task.started_at = Some(Utc::now());
            let task_clone = task.clone();
            drop(tasks);
            self.save_task_to_db(&task_clone).await;
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新当前处理的文件
    pub async fn update_current_file(&self, task_id: &str, current_file: &str) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.current_file = Some(current_file.to_string());
            let task_clone = task.clone();
            drop(tasks);
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新进度（百分比和已处理文件数）
    pub async fn update_progress_percent(&self, task_id: &str, progress: f32, processed_files: u64) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.progress = progress;
            task.processed_files = processed_files;
            
            // 计算速度和ETA
            if let Some(started_at) = &task.started_at {
                let elapsed_secs = (Utc::now() - *started_at).num_seconds() as f64;
                if elapsed_secs > 0.0 && task.total_files > 0 {
                    let files_per_sec = processed_files as f64 / elapsed_secs;
                    if files_per_sec > 0.0 {
                        let remaining = task.total_files - processed_files;
                        task.eta_seconds = Some((remaining as f64 / files_per_sec) as u64);
                    }
                }
            }
            
            let task_clone = task.clone();
            drop(tasks);
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新解压任务完整进度（用于多阶段任务：下载/解压/上传）
    pub async fn update_extract_task_progress(
        &self,
        task_id: &str,
        progress: f32,
        speed: f64,
        eta_seconds: u64,
        current_file: &str,
        processed_files: u64,
        total_files: u64,
    ) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.progress = progress;
            task.speed = speed;
            task.eta_seconds = Some(eta_seconds);
            task.current_file = Some(current_file.to_string());
            task.processed_files = processed_files;
            if total_files > 0 {
                task.total_files = total_files;
            }
            let task_clone = task.clone();
            drop(tasks);
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新复制/移动任务进度（支持多阶段：下载中/上传中）
    pub async fn update_copy_task_progress(
        &self,
        task_id: &str,
        phase: &str,  // "downloading" / "uploading"
        current_file: &str,
        _file_progress: f32,  // 当前文件进度 0-100（预留）
        processed_files: u64,
        total_files: u64,
        processed_size: u64,
        total_size: u64,
    ) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            // 状态格式：[阶段] 文件名
            let status = format!("[{}] {}", phase, current_file);
            task.current_file = Some(status);
            task.processed_files = processed_files;
            task.total_files = total_files;
            task.processed_size = processed_size;
            task.total_size = total_size;
            
            // 计算总进度
            if total_size > 0 {
                task.progress = (processed_size as f32 / total_size as f32) * 100.0;
            }
            
            // 计算瞬时速度（使用滑动窗口，基于最近3-5秒的数据）
            let now = Utc::now();
            let speed = if let Some(last_update) = task.last_speed_update_time {
                let time_diff_ms = now.signed_duration_since(last_update).num_milliseconds();
                let time_diff_secs = time_diff_ms as f64 / 1000.0;
                
                // 如果距离上次更新超过1秒，计算瞬时速度
                if time_diff_secs >= 1.0 {
                    let size_diff = processed_size.saturating_sub(task.last_speed_processed_size);
                    let instant_speed = size_diff as f64 / time_diff_secs;
                    
                    // 更新速度跟踪
                    task.last_speed_update_time = Some(now);
                    task.last_speed_processed_size = processed_size;
                    
                    // 如果瞬时速度有效，使用它；否则使用平均速度作为后备
                    if instant_speed > 0.0 {
                        instant_speed
                    } else if let Some(started_at) = &task.started_at {
                        let elapsed_secs = (now - *started_at).num_seconds() as f64;
                        if elapsed_secs > 0.0 {
                            processed_size as f64 / elapsed_secs
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    }
                } else {
                    // 时间间隔太短，保持当前速度
                    task.speed
                }
            } else {
                // 首次更新，初始化速度跟踪
                task.last_speed_update_time = Some(now);
                task.last_speed_processed_size = processed_size;
                
                // 使用平均速度
                if let Some(started_at) = &task.started_at {
                    let elapsed_secs = (now - *started_at).num_seconds() as f64;
                    if elapsed_secs > 0.0 {
                        processed_size as f64 / elapsed_secs
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            };
            
            task.speed = speed;
            
            // 计算ETA
            if task.speed > 0.0 && total_size > processed_size {
                let remaining = total_size - processed_size;
                task.eta_seconds = Some((remaining as f64 / task.speed) as u64);
            }
            
            // 定期保存到数据库（每5秒一次）
            let should_save = task.last_saved.map(|t| {
                Utc::now().signed_duration_since(t).num_seconds() >= 5
            }).unwrap_or(true);
            
            if should_save {
                task.last_saved = Some(Utc::now());
            }
            
            let task_clone = task.clone();
            drop(tasks);
            
            if should_save {
                self.save_task_to_db(&task_clone).await;
            }
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 更新任务处理进度（文件数和字节数，计算速度和ETA）
    pub async fn update_task_progress_with_size(&self, task_id: &str, processed_files: u64, processed_size: u64, current_file: Option<String>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.processed_files = processed_files;
            task.processed_size = processed_size;
            if task.total_size > 0 {
                task.progress = (processed_size as f32 / task.total_size as f32) * 100.0;
            }
            if let Some(cf) = current_file {
                task.current_file = Some(cf);
            }
            
            // 计算瞬时速度（使用滑动窗口，基于最近3-5秒的数据）
            let now = Utc::now();
            let speed = if let Some(last_update) = task.last_speed_update_time {
                let time_diff_ms = now.signed_duration_since(last_update).num_milliseconds();
                let time_diff_secs = time_diff_ms as f64 / 1000.0;
                
                // 如果距离上次更新超过1秒，计算瞬时速度
                if time_diff_secs >= 1.0 {
                    let size_diff = processed_size.saturating_sub(task.last_speed_processed_size);
                    let instant_speed = size_diff as f64 / time_diff_secs;
                    
                    // 更新速度跟踪
                    task.last_speed_update_time = Some(now);
                    task.last_speed_processed_size = processed_size;
                    
                    // 如果瞬时速度有效，使用它；否则使用平均速度作为后备
                    if instant_speed > 0.0 {
                        instant_speed
                    } else if let Some(started_at) = &task.started_at {
                        let elapsed_secs = (now - *started_at).num_seconds() as f64;
                        if elapsed_secs > 0.0 {
                            processed_size as f64 / elapsed_secs
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    }
                } else {
                    // 时间间隔太短，保持当前速度
                    task.speed
                }
            } else {
                // 首次更新，初始化速度跟踪
                task.last_speed_update_time = Some(now);
                task.last_speed_processed_size = processed_size;
                
                // 使用平均速度
                if let Some(started_at) = &task.started_at {
                    let elapsed_secs = (now - *started_at).num_seconds() as f64;
                    if elapsed_secs > 0.0 {
                        processed_size as f64 / elapsed_secs
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            };
            
            task.speed = speed;
            
            // 计算ETA
            if task.speed > 0.0 && task.total_size > processed_size {
                let remaining = task.total_size - processed_size;
                task.eta_seconds = Some((remaining as f64 / task.speed) as u64);
            }
            
            // 定期保存到数据库（每5秒一次）
            let should_save = task.last_saved.map(|t| {
                Utc::now().signed_duration_since(t).num_seconds() >= 5
            }).unwrap_or(true);
            
            if should_save {
                task.last_saved = Some(Utc::now());
            }
            
            let task_clone = task.clone();
            drop(tasks);
            
            if should_save {
                self.save_task_to_db(&task_clone).await;
            }
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }

    /// 保存任务到数据库
    async fn save_task_to_db(&self, task: &Task) {
        if let Some(db) = &self.db {
            // 序列化files和items字段为JSON（用于断点续传）
            let files_json = task.files.as_ref()
                .map(|f| serde_json::to_string(f).unwrap_or_default());
            let items_json = task.items.as_ref()
                .map(|i| serde_json::to_string(i).unwrap_or_default());
            
            let _ = sqlx::query(
                r#"INSERT OR REPLACE INTO tasks 
                   (id, task_type, status, name, source_path, target_path, 
                    total_size, processed_size, total_files, processed_files, 
                    progress, speed, eta_seconds, created_at, started_at, 
                    finished_at, error, user_id, current_file, files, items, conflict_strategy) 
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#
            )
            .bind(&task.id)
            .bind(format!("{:?}", task.task_type).to_lowercase())
            .bind(format!("{:?}", task.status).to_lowercase())
            .bind(&task.name)
            .bind(&task.source_path)
            .bind(&task.target_path)
            .bind(task.total_size as i64)
            .bind(task.processed_size as i64)
            .bind(task.total_files as i64)
            .bind(task.processed_files as i64)
            .bind(task.progress as f64)
            .bind(task.speed)
            .bind(task.eta_seconds.map(|s| s as i64))
            .bind(task.created_at.to_rfc3339())
            .bind(task.started_at.map(|t| t.to_rfc3339()))
            .bind(task.finished_at.map(|t| t.to_rfc3339()))
            .bind(&task.error)
            .bind(task.user_id.clone())
            .bind(&task.current_file)
            .bind(files_json)
            .bind(items_json)
            .bind(&task.conflict_strategy)
            .execute(db)
            .await;
        }
    }

    /// 创建任务（用户关联）
    pub async fn create_task(
        &self,
        task_type: TaskType,
        name: String,
        source_path: String,
        target_path: Option<String>,
        total_size: u64,
        total_files: u64,
        user_id: Option<String>,
    ) -> String {
        let task = Task::new(task_type, name, source_path, target_path, total_size, total_files, user_id);
        let task_id = task.id.clone();
        
        tracing::info!("Task created: {} (user_id: {:?}, type: {:?}, files: {})", task_id, task.user_id, task.task_type, total_files);
        
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task.clone());
        
        // 持久化到数据库
        self.save_task_to_db(&task).await;
        
        self.broadcast(TaskEvent::TaskCreated { task: TaskSummary::from(&task) });
        task_id
    }

    /// 开始任务
    pub async fn start_task(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Running;
            task.started_at = Some(Utc::now());
            let task_clone = task.clone();
            drop(tasks);
            self.save_task_to_db(&task_clone).await;
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
            true
        } else {
            false
        }
    }

    /// 更新任务进度（自动计算速度和ETA）
    pub async fn update_progress(&self, task_id: &str, processed_size: u64) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            // 确保进度只增不减，避免进度倒退
            let actual_processed_size = processed_size.max(task.processed_size);
            
            // 计算瞬时速度（使用滑动窗口，基于最近3-5秒的数据）
            let now = Utc::now();
            let speed = if let Some(last_update) = task.last_speed_update_time {
                let time_diff_ms = now.signed_duration_since(last_update).num_milliseconds();
                let time_diff_secs = time_diff_ms as f64 / 1000.0;
                
                // 如果距离上次更新超过1秒，计算瞬时速度
                if time_diff_secs >= 1.0 {
                    let size_diff = actual_processed_size.saturating_sub(task.last_speed_processed_size);
                    let instant_speed = size_diff as f64 / time_diff_secs;
                    
                    // 更新速度跟踪
                    task.last_speed_update_time = Some(now);
                    task.last_speed_processed_size = actual_processed_size;
                    
                    // 如果瞬时速度有效，使用它；否则使用平均速度作为后备
                    if instant_speed > 0.0 {
                        instant_speed
                    } else if let Some(started_at) = task.started_at {
                        let elapsed = now.signed_duration_since(started_at).num_milliseconds() as f64 / 1000.0;
                        if elapsed > 0.0 {
                            actual_processed_size as f64 / elapsed
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    }
                } else {
                    // 时间间隔太短，保持当前速度
                    task.speed
                }
            } else {
                // 首次更新，初始化速度跟踪
                task.last_speed_update_time = Some(now);
                task.last_speed_processed_size = actual_processed_size;
                
                // 使用平均速度
                if let Some(started_at) = task.started_at {
                    let elapsed = now.signed_duration_since(started_at).num_milliseconds() as f64 / 1000.0;
                    if elapsed > 0.0 {
                        actual_processed_size as f64 / elapsed
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            };
            
            task.processed_size = actual_processed_size;
            task.speed = speed;
            if task.total_size > 0 {
                task.progress = (processed_size as f32 / task.total_size as f32) * 100.0;
                // 计算ETA
                if speed > 0.0 {
                    let remaining = task.total_size - processed_size;
                    task.eta_seconds = Some((remaining as f64 / speed) as u64);
                }
            }
            let task_clone = task.clone();
            drop(tasks);
            self.save_task_to_db(&task_clone).await;
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }

    /// 完成任务
    pub async fn complete_task(&self, task_id: &str) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.progress = 100.0;
            task.processed_size = task.total_size;
            task.eta_seconds = Some(0);
            task.finished_at = Some(Utc::now());
            let task_clone = task.clone();
            drop(tasks);
            self.save_task_to_db(&task_clone).await;
            self.broadcast(TaskEvent::TaskCompleted { task: TaskSummary::from(&task_clone) });
        }
    }

    /// 任务失败
    pub async fn fail_task(&self, task_id: &str, error: String) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error = Some(error);
            task.finished_at = Some(Utc::now());
            let task_clone = task.clone();
            drop(tasks);
            self.save_task_to_db(&task_clone).await;
            self.broadcast(TaskEvent::TaskFailed { task: TaskSummary::from(&task_clone) });
        }
    }

    /// 取消任务（同时保存到数据库）
    pub async fn cancel_task(&self, task_id: &str) -> bool {
        // 设置取消标志
        {
            let controls = self.controls.read().await;
            if let Some(ctrl) = controls.get(task_id) {
                ctrl.cancel();
            }
        }
        
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Pending || task.status == TaskStatus::Running || task.status == TaskStatus::Paused {
                task.status = TaskStatus::Cancelled;
                task.finished_at = Some(Utc::now());
                let task_clone = task.clone();
                drop(tasks);
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskCancelled { task: TaskSummary::from(&task_clone) });
                return true;
            }
        }
        false
    }
    
    /// 暂停任务
    pub async fn pause_task(&self, task_id: &str) -> bool {
        // 设置暂停标志
        {
            let controls = self.controls.read().await;
            if let Some(ctrl) = controls.get(task_id) {
                ctrl.pause();
            }
        }
        
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Running {
                task.status = TaskStatus::Paused;
                let task_clone = task.clone();
                drop(tasks);
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
                return true;
            }
        }
        false
    }
    
    /// 继续任务
    pub async fn resume_task(&self, task_id: &str) -> bool {
        // 清除暂停标志
        {
            let controls = self.controls.read().await;
            if let Some(ctrl) = controls.get(task_id) {
                ctrl.resume();
            }
        }
        
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Paused {
                task.status = TaskStatus::Running;
                let task_clone = task.clone();
                drop(tasks);
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
                return true;
            }
        }
        false
    }
    
    /// 重新启动任务（完全重置）
    pub async fn restart_task(&self, task_id: &str) -> bool {
        self.restart_task_resume(task_id, 0).await
    }
    
    /// 重新启动任务（从断点继续，保留已处理进度）
    pub async fn restart_task_resume(&self, task_id: &str, skip_files: u64) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Failed 
                || task.status == TaskStatus::Cancelled 
                || task.status == TaskStatus::Interrupted {
                // 重置任务状态，但保留已处理进度（如果skip_files > 0）
                task.status = TaskStatus::Running;
                task.speed = 0.0;
                task.eta_seconds = None;
                task.error = None;
                task.finished_at = None;
                task.started_at = Some(Utc::now());
                
                // 如果不是从断点继续，重置进度
                if skip_files == 0 {
                    task.progress = 0.0;
                    task.processed_size = 0;
                    task.processed_files = 0;
                    task.current_file = None;
                    
                    // 重置文件状态
                    if let Some(ref mut files) = task.files {
                        for file in files.iter_mut() {
                            file.status = TaskStatus::Pending;
                            file.uploaded_size = 0;
                            file.uploaded_chunks.clear();
                        }
                    }
                }
                
                let task_clone = task.clone();
                drop(tasks);
                
                // 创建新的控制标志
                self.create_control(task_id).await;
                
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
                
                return true;
            }
        }
        false
    }
    
    /// 获取任务控制标志
    pub async fn get_control(&self, task_id: &str) -> Option<Arc<TaskControl>> {
        let controls = self.controls.read().await;
        controls.get(task_id).cloned()
    }
    
    /// 创建任务控制标志
    pub async fn create_control(&self, task_id: &str) -> Arc<TaskControl> {
        let ctrl = Arc::new(TaskControl::new());
        let mut controls = self.controls.write().await;
        controls.insert(task_id.to_string(), ctrl.clone());
        ctrl
    }
    
    /// 移除任务控制标志
    pub async fn remove_control(&self, task_id: &str) {
        let mut controls = self.controls.write().await;
        controls.remove(task_id);
    }
    
    /// 中断任务（用户刷新页面或断开连接）
    pub async fn interrupt_task(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status == TaskStatus::Running {
                task.status = TaskStatus::Interrupted;
                let task_clone = task.clone();
                drop(tasks);
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
                return true;
            }
        }
        false
    }
    
    /// 标记所有运行中的任务为中断状态（服务器启动时调用）
    pub async fn interrupt_all_running_tasks(&self) {
        let mut tasks = self.tasks.write().await;
        let mut interrupted_tasks = Vec::new();
        
        for task in tasks.values_mut() {
            if task.status == TaskStatus::Running {
                task.status = TaskStatus::Interrupted;
                interrupted_tasks.push(task.clone());
            }
        }
        
        drop(tasks);
        
        for task in interrupted_tasks {
            self.save_task_to_db(&task).await;
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task) });
        }
    }

    /// 获取任务
    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// 获取用户的所有任务
    pub async fn get_user_tasks(&self, user_id: Option<String>) -> Vec<Task> {
        let tasks = self.tasks.read().await;
        tasks.values()
            .filter(|t| t.user_id == user_id)
            .cloned()
            .collect()
    }

    /// 获取所有任务
    pub async fn get_all_tasks(&self) -> Vec<Task> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }

    /// 删除已完成的任务（同时从数据库删除）
    pub async fn clear_completed(&self, user_id: Option<String>) -> usize {
        let mut tasks = self.tasks.write().await;
        let mut removed_ids: Vec<String> = Vec::new();
        
        tasks.retain(|id, t| {
            if t.user_id != user_id {
                return true;
            }
            let keep = matches!(t.status, TaskStatus::Pending | TaskStatus::Running | TaskStatus::Interrupted);
            if !keep {
                removed_ids.push(id.clone());
            }
            keep
        });
        
        let count = removed_ids.len();
        
        // 从数据库删除
        if let Some(db) = &self.db {
            for id in &removed_ids {
                let _ = sqlx::query("DELETE FROM tasks WHERE id = ?")
                    .bind(id)
                    .execute(db)
                    .await;
            }
        }
        
        count
    }

    /// 删除指定任务（同时从数据库删除）
    pub async fn remove_task(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        let removed = tasks.remove(task_id).is_some();
        
        if removed {
            if let Some(db) = &self.db {
                let _ = sqlx::query("DELETE FROM tasks WHERE id = ?")
                    .bind(task_id)
                    .execute(db)
                    .await;
            }
        }
        
        removed
    }

    /// 清理过期任务（超过1小时的已完成/失败/取消任务）
    pub async fn cleanup_expired(&self) {
        let mut tasks = self.tasks.write().await;
        let now = Utc::now();
        tasks.retain(|_, t| {
            if let Some(finished) = t.finished_at {
                let duration = now.signed_duration_since(finished);
                duration.num_hours() < 1
            } else {
                true
            }
        });
    }
    
    /// 创建批次上传任务
    pub async fn create_batch_upload(
        &self,
        name: String,
        target_path: String,
        files: Vec<UploadFileInfo>,
        user_id: Option<String>,
    ) -> String {
        let task = Task::new_batch_upload(name, target_path, files, user_id);
        let task_id = task.id.clone();
        
        tracing::info!("Batch upload task created: {} (user_id: {:?}, files: {})", task_id, task.user_id, task.total_files);
        
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task.clone());
        
        self.save_task_to_db(&task).await;
        self.broadcast(TaskEvent::TaskCreated { task: TaskSummary::from(&task) });
        task_id
    }
    
    /// 更新批次任务中的单个文件进度（不保存数据库，只更新内存和广播）
    pub async fn update_file_progress(&self, task_id: &str, file_path: &str, uploaded_size: u64, chunk_index: Option<u32>) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.current_file = Some(file_path.to_string());
            
            if let Some(ref mut files) = task.files {
                // 尝试多种方式匹配文件路径
                let file_name = file_path.split('/').last().unwrap_or(file_path);
                let file_opt = files.iter_mut().find(|f| {
                    // 精确匹配
                    if f.path == file_path {
                        return true;
                    }
                    // 文件名匹配
                    let f_name = f.path.split('/').last().unwrap_or(&f.path);
                    if f_name == file_name {
                        return true;
                    }
                    // 路径后缀匹配
                    if f.path.ends_with(&format!("/{}", file_name)) || file_path.ends_with(&format!("/{}", f_name)) {
                        return true;
                    }
                    false
                });
                
                if let Some(file) = file_opt {
                    // 确保进度只增不减，避免进度倒退
                    if uploaded_size > file.uploaded_size {
                        file.uploaded_size = uploaded_size;
                    }
                    if let Some(chunk) = chunk_index {
                        if !file.uploaded_chunks.contains(&chunk) {
                            file.uploaded_chunks.push(chunk);
                        }
                    }
                    tracing::debug!("File progress updated: task={}, file={}, uploaded={}", task_id, file_path, file.uploaded_size);
                } else {
                    tracing::warn!("File not found: task={}, file_path={}, files={:?}", 
                        task_id, file_path, files.iter().map(|f| &f.path).collect::<Vec<_>>());
                }
                
                // 计算总进度
                let total_uploaded: u64 = files.iter().map(|f| f.uploaded_size).sum();
                task.processed_size = total_uploaded;
            }
            
            // 计算瞬时速度（使用滑动窗口，基于最近3-5秒的数据）
            let now = Utc::now();
            let speed = if let Some(last_update) = task.last_speed_update_time {
                let time_diff_ms = now.signed_duration_since(last_update).num_milliseconds();
                let time_diff_secs = time_diff_ms as f64 / 1000.0;
                
                // 如果距离上次更新超过1秒，计算瞬时速度
                if time_diff_secs >= 1.0 {
                    let size_diff = task.processed_size.saturating_sub(task.last_speed_processed_size);
                    let instant_speed = size_diff as f64 / time_diff_secs;
                    
                    // 更新速度跟踪
                    task.last_speed_update_time = Some(now);
                    task.last_speed_processed_size = task.processed_size;
                    
                    // 如果瞬时速度有效，使用它；否则使用平均速度作为后备
                    if instant_speed > 0.0 {
                        instant_speed
                    } else if let Some(started_at) = task.started_at {
                        let elapsed = now.signed_duration_since(started_at).num_milliseconds() as f64 / 1000.0;
                        if elapsed > 0.0 {
                            task.processed_size as f64 / elapsed
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    }
                } else {
                    // 时间间隔太短，保持当前速度
                    task.speed
                }
            } else {
                // 首次更新，初始化速度跟踪
                task.last_speed_update_time = Some(now);
                task.last_speed_processed_size = task.processed_size;
                
                // 使用平均速度
                if let Some(started_at) = task.started_at {
                    let elapsed = now.signed_duration_since(started_at).num_milliseconds() as f64 / 1000.0;
                    if elapsed > 0.0 {
                        task.processed_size as f64 / elapsed
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            };
            
            task.speed = speed;
            
            // 计算ETA
            if let Some(started_at) = task.started_at {
                if task.speed > 0.0 && task.total_size > task.processed_size {
                    let remaining = task.total_size - task.processed_size;
                    task.eta_seconds = Some((remaining as f64 / task.speed) as u64);
                }
            }
            
            if task.total_size > 0 {
                task.progress = (task.processed_size as f32 / task.total_size as f32) * 100.0;
            }
            
            // 定期保存到数据库（每5秒一次，避免频繁IO）
            let should_save = task.last_saved.map(|t| {
                Utc::now().signed_duration_since(t).num_seconds() >= 5
            }).unwrap_or(true);
            
            if should_save {
                task.last_saved = Some(Utc::now());
            }
            
            let task_clone = task.clone();
            drop(tasks);
            
            // 定期保存到数据库
            if should_save {
                self.save_task_to_db(&task_clone).await;
            }
            
            self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
        }
    }
    
    /// 标记批次任务中的单个文件完成
    pub async fn complete_file(&self, task_id: &str, file_path: &str) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            let mut file_was_pending = false;
            
            if let Some(ref mut files) = task.files {
                // 尝试多种方式匹配文件路径
                let file_name = file_path.split('/').last().unwrap_or(file_path);
                let file_opt = files.iter_mut().find(|f| {
                    if f.path == file_path {
                        return true;
                    }
                    let f_name = f.path.split('/').last().unwrap_or(&f.path);
                    if f_name == file_name {
                        return true;
                    }
                    if f.path.ends_with(&format!("/{}", file_name)) || file_path.ends_with(&format!("/{}", f_name)) {
                        return true;
                    }
                    false
                });
                
                if let Some(file) = file_opt {
                    // 只有未完成的文件才增加计数
                    if file.status != TaskStatus::Completed {
                        file_was_pending = true;
                        file.status = TaskStatus::Completed;
                        file.uploaded_size = file.size;
                    }
                }
                
                // 更新总进度
                let total_uploaded: u64 = files.iter().map(|f| f.uploaded_size).sum();
                task.processed_size = total_uploaded;
            }
            
            // 只有文件之前是待处理状态才增加计数
            if file_was_pending {
                task.processed_files += 1;
            }
            
            // 检查是否所有文件都完成
            let all_done = task.files.as_ref()
                .map(|files| files.iter().all(|f| f.status == TaskStatus::Completed))
                .unwrap_or(false);
            
            if all_done {
                task.status = TaskStatus::Completed;
                task.progress = 100.0;
                task.processed_size = task.total_size;
                task.finished_at = Some(Utc::now());
                task.current_file = None;
                let task_clone = task.clone();
                drop(tasks);
                self.save_task_to_db(&task_clone).await;
                self.broadcast(TaskEvent::TaskCompleted { task: TaskSummary::from(&task_clone) });
            } else {
                let task_clone = task.clone();
                drop(tasks);
                self.broadcast(TaskEvent::TaskUpdated { task: TaskSummary::from(&task_clone) });
            }
        }
    }
    
    /// 获取任务中待上传的分片（用于断点续传）
    pub async fn get_pending_chunks(&self, task_id: &str, file_path: &str) -> Option<Vec<u32>> {
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(task_id) {
            if let Some(ref files) = task.files {
                if let Some(file) = files.iter().find(|f| f.path == file_path) {
                    return Some(file.uploaded_chunks.clone());
                }
            }
        }
        None
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}
