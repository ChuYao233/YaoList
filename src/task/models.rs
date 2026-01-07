use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use super::types::{TaskType, TaskStatus};

/// 任务控制标志（用于取消/暂停）
#[derive(Debug)]
pub struct TaskControl {
    pub cancelled: AtomicBool,
    pub paused: AtomicBool,
}

impl TaskControl {
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        }
    }
    
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
    
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
}

/// 轻量级任务信息（用于WebSocket推送，不包含files详情）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub name: String,
    pub source_path: String,
    pub target_path: Option<String>,
    pub total_size: u64,
    pub processed_size: u64,
    pub total_files: u64,
    pub processed_files: u64,
    pub progress: f32,
    pub speed: f64,
    pub eta_seconds: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub user_id: Option<String>,
    pub current_file: Option<String>,
}

impl From<&Task> for TaskSummary {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            task_type: task.task_type.clone(),
            status: task.status.clone(),
            name: task.name.clone(),
            source_path: task.source_path.clone(),
            target_path: task.target_path.clone(),
            total_size: task.total_size,
            processed_size: task.processed_size,
            total_files: task.total_files,
            processed_files: task.processed_files,
            progress: task.progress,
            speed: task.speed,
            eta_seconds: task.eta_seconds,
            created_at: task.created_at,
            started_at: task.started_at,
            finished_at: task.finished_at,
            error: task.error.clone(),
            user_id: task.user_id.clone(),
            current_file: task.current_file.clone(),
        }
    }
}

/// 批次上传中单个文件的状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadFileInfo {
    pub path: String,
    pub size: u64,
    pub uploaded_size: u64,
    pub uploaded_chunks: Vec<u32>,  // 已上传的分片索引（用于断点续传）
    pub status: TaskStatus,
}

/// 任务信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub name: String,
    pub source_path: String,
    pub target_path: Option<String>,
    pub total_size: u64,
    pub processed_size: u64,
    pub total_files: u64,       // 总文件数
    pub processed_files: u64,   // 已处理文件数
    pub progress: f32,
    pub speed: f64,
    pub eta_seconds: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub user_id: Option<String>,
    pub current_file: Option<String>,       // 当前正在处理的文件
    pub files: Option<Vec<UploadFileInfo>>, // 批次上传的文件列表（用于断点续传）
    pub items: Option<Vec<String>>,         // 待处理的项目列表（复制/移动用）
    pub conflict_strategy: Option<String>,  // 冲突策略
    #[serde(skip)]
    pub last_saved: Option<DateTime<Utc>>,  // 上次保存时间（不序列化）
    #[serde(skip)]
    pub last_speed_update_time: Option<DateTime<Utc>>,  // 上次速度更新时间（不序列化）
    #[serde(skip)]
    pub last_speed_processed_size: u64,  // 上次速度更新时的已处理大小（不序列化）
}

impl Task {
    pub fn new(
        task_type: TaskType,
        name: String,
        source_path: String,
        target_path: Option<String>,
        total_size: u64,
        total_files: u64,
        user_id: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_type,
            status: TaskStatus::Pending,
            name,
            source_path,
            target_path,
            total_size,
            processed_size: 0,
            total_files,
            processed_files: 0,
            progress: 0.0,
            speed: 0.0,
            eta_seconds: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            error: None,
            user_id,
            current_file: None,
            files: None,
            items: None,
            conflict_strategy: None,
            last_saved: None,
            last_speed_update_time: None,
            last_speed_processed_size: 0,
        }
    }
    
    /// 创建批次上传任务
    pub fn new_batch_upload(
        name: String,
        target_path: String,
        files: Vec<UploadFileInfo>,
        user_id: Option<String>,
    ) -> Self {
        let total_size: u64 = files.iter().map(|f| f.size).sum();
        let total_files = files.len() as u64;
        
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_type: TaskType::Upload,
            status: TaskStatus::Pending,
            name,
            source_path: String::new(),
            target_path: Some(target_path),
            total_size,
            processed_size: 0,
            total_files,
            processed_files: 0,
            progress: 0.0,
            speed: 0.0,
            eta_seconds: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            error: None,
            user_id,
            current_file: files.first().map(|f| f.path.clone()),
            files: Some(files),
            items: None,
            conflict_strategy: None,
            last_saved: None,
            last_speed_update_time: None,
            last_speed_processed_size: 0,
        }
    }
    
    /// 创建复制/移动任务
    pub fn new_copy_move(
        task_type: TaskType,
        name: String,
        source_path: String,
        target_path: String,
        items: Vec<String>,
        conflict_strategy: String,
        user_id: Option<String>,
    ) -> Self {
        let total_files = items.len() as u64;
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_type,
            status: TaskStatus::Pending,
            name,
            source_path,
            target_path: Some(target_path),
            total_size: 0,
            processed_size: 0,
            total_files,
            processed_files: 0,
            progress: 0.0,
            speed: 0.0,
            eta_seconds: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            error: None,
            user_id,
            current_file: None,
            files: None,
            items: Some(items),
            conflict_strategy: Some(conflict_strategy),
            last_saved: None,
            last_speed_update_time: None,
            last_speed_processed_size: 0,
        }
    }
}
