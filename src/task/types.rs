use serde::{Deserialize, Serialize};

use super::models::TaskSummary;

/// 任务类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Upload,
    Download,
    Copy,
    Move,
    Delete,
    Extract,
}

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

/// 任务事件（用于WebSocket推送）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    TaskCreated { task: TaskSummary },
    TaskUpdated { task: TaskSummary },
    TaskCompleted { task: TaskSummary },
    TaskFailed { task: TaskSummary },
    TaskCancelled { task: TaskSummary },
}
