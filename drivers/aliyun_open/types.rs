//! 阿里云盘 Open API 数据类型定义

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

// ============ API 常量 ============

pub const API_URL: &str = "https://openapi.alipan.com";

// ============ 错误响应 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrResp {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

// ============ 文件相关 ============

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AliyunFile {
    pub drive_id: String,
    pub file_id: String,
    pub parent_file_id: String,
    pub name: String,
    pub size: i64,
    pub file_extension: Option<String>,
    pub content_hash: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub file_type: String,
    pub thumbnail: Option<String>,
    pub url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    
    // 创建时使用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileList {
    pub items: Vec<AliyunFile>,
    pub next_marker: Option<String>,
}

// ============ 上传相关 ============

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PartInfo {
    pub etag: Option<String>,
    pub part_number: i32,
    pub part_size: Option<i64>,
    pub upload_url: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateFileResponse {
    pub file_id: String,
    pub upload_id: Option<String>,
    pub rapid_upload: Option<bool>,
    pub part_info_list: Option<Vec<PartInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoveOrCopyResponse {
    pub file_id: String,
}

// ============ 用户信息 ============

#[derive(Debug, Clone, Deserialize)]
pub struct DriveInfo {
    pub default_drive_id: String,
    pub resource_drive_id: String,
    pub backup_drive_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpaceInfo {
    pub personal_space_info: PersonalSpaceInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalSpaceInfo {
    pub total_size: u64,
    pub used_size: u64,
}

// ============ OAuth 相关 ============

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshTokenRequest {
    pub client_id: String,
    pub client_secret: String,
    pub grant_type: String,
    pub refresh_token: String,
}

// ============ 下载相关 ============

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadUrlResponse {
    pub url: String,
    pub streams_url: Option<serde_json::Value>, // LIVP 格式的流媒体 URL
}

// ============ 视频预览相关 ============

#[derive(Debug, Clone, Deserialize)]
pub struct VideoPreviewResponse {
    pub video_preview_play_info: VideoPreviewPlayInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoPreviewPlayInfo {
    pub category: String,
    pub meta: VideoMeta,
    pub live_transcoding_task_list: Vec<TranscodingTask>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoMeta {
    pub duration: f64,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranscodingTask {
    pub template_id: String,
    pub status: String,
    pub url: Option<String>,
}

impl AliyunFile {
    pub fn is_folder(&self) -> bool {
        self.file_type == "folder"
    }
    
    pub fn get_size(&self) -> u64 {
        self.size.max(0) as u64
    }
    
    pub fn get_modified_time(&self) -> SystemTime {
        // 解析 ISO 8601 格式时间
        chrono::DateTime::parse_from_rfc3339(&self.updated_at)
            .map(|dt| dt.into())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    }
}
