//! PikPak data type definitions / PikPak数据类型定义

use serde::{Deserialize, Serialize};

/// API error response / API错误响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrResp {
    #[serde(default, rename = "error_code")]
    pub error_code: i64,
    #[serde(default, rename = "error")]
    pub error: String,
    #[serde(default, rename = "error_description")]
    pub error_description: String,
}

impl ErrResp {
    pub fn is_error(&self) -> bool {
        self.error_code != 0 || !self.error.is_empty() || !self.error_description.is_empty()
    }

    pub fn error_message(&self) -> String {
        if self.error_code != 0 {
            return format!(
                "ErrorCode: {}, Error: {}, Description: {}",
                self.error_code, self.error, self.error_description
            );
        }
        if !self.error.is_empty() {
            return format!("Error: {}, Description: {}", self.error, self.error_description);
        }
        "Unknown error".to_string()
    }
}

/// File list response / 文件列表响应
#[derive(Debug, Deserialize, Default)]
pub struct FilesResp {
    #[serde(default)]
    pub files: Vec<PikPakFile>,
    #[serde(default, rename = "next_page_token")]
    pub next_page_token: String,
}

/// PikPak file info / PikPak文件信息
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PikPakFile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub created_time: String,
    #[serde(default)]
    pub modified_time: String,
    #[serde(default)]
    pub thumbnail_link: String,
    #[serde(default)]
    pub web_content_link: String,
    #[serde(default)]
    pub medias: Vec<Media>,
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub trashed: bool,
}

impl PikPakFile {
    pub fn is_dir(&self) -> bool {
        self.kind == "drive#folder"
    }

    pub fn get_size(&self) -> u64 {
        self.size.parse().unwrap_or(0)
    }
}

/// Media info (for video streaming) / 媒体信息
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Media {
    #[serde(default)]
    pub media_id: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default)]
    pub video: VideoInfo,
    #[serde(default)]
    pub link: MediaLink,
    #[serde(default)]
    pub need_more_quota: bool,
    #[serde(default)]
    pub redirect_link: String,
    #[serde(default)]
    pub icon_link: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub is_origin: bool,
    #[serde(default)]
    pub resolution_name: String,
    #[serde(default)]
    pub is_visible: bool,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VideoInfo {
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub width: i32,
    #[serde(default)]
    pub duration: i32,
    #[serde(default)]
    pub bit_rate: i32,
    #[serde(default)]
    pub frame_rate: i32,
    #[serde(default)]
    pub video_codec: String,
    #[serde(default)]
    pub audio_codec: String,
    #[serde(default)]
    pub video_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MediaLink {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub expire: String,
}

/// Login response / 登录响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoginResp {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub sub: String,
    #[serde(flatten)]
    pub error: ErrResp,
}

/// Captcha token request / 验证码令牌请求
#[derive(Debug, Clone, Serialize)]
pub struct CaptchaTokenRequest {
    pub action: String,
    pub captcha_token: String,
    pub client_id: String,
    pub device_id: String,
    pub meta: std::collections::HashMap<String, String>,
    pub redirect_uri: String,
}

/// Captcha token response / 验证码令牌响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CaptchaTokenResp {
    #[serde(default)]
    pub captcha_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub url: String,
}

/// Upload task response / 上传任务响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadTaskResp {
    #[serde(default)]
    pub upload_type: String,
    #[serde(default)]
    pub resumable: Option<ResumableInfo>,
    #[serde(default)]
    pub file: PikPakFile,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResumableInfo {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub params: S3Params,
    #[serde(default)]
    pub provider: String,
}

/// S3 upload params / S3上传参数
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct S3Params {
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub access_key_secret: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub expiration: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub security_token: String,
}

/// About/quota response / 容量信息响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AboutResp {
    #[serde(default)]
    pub quota: QuotaInfo,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub user_type: i32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QuotaInfo {
    #[serde(default)]
    pub limit: String,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub usage_in_trash: String,
    #[serde(default)]
    pub is_unlimited: bool,
    #[serde(default)]
    pub complimentary: String,
}

/// Offline download response / 离线下载响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OfflineDownloadResp {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub task: OfflineTask,
    #[serde(default)]
    pub upload_type: String,
}

/// Offline task / 离线任务
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OfflineTask {
    #[serde(default)]
    pub callback: String,
    #[serde(default)]
    pub created_time: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size: String,
    #[serde(default)]
    pub icon_link: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub progress: i64,
    #[serde(default)]
    pub space: String,
    #[serde(default)]
    pub status_size: i64,
    #[serde(default)]
    pub statuses: Vec<String>,
    #[serde(default)]
    pub third_task_id: String,
    #[serde(default, rename = "type")]
    pub task_type: String,
    #[serde(default)]
    pub updated_time: String,
    #[serde(default)]
    pub user_id: String,
}

/// Offline task list response / 离线任务列表响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OfflineListResp {
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub next_page_token: String,
    #[serde(default)]
    pub tasks: Vec<OfflineTask>,
}

/// Batch operation response / 批量操作响应
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BatchResp {
    #[serde(default)]
    pub task_id: String,
}

/// Token info (internal state) / 令牌信息(内部状态)
#[derive(Debug, Clone, Default)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub captcha_token: String,
    pub device_id: String,
}
