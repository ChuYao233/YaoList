//! 123云盘开放平台API类型定义
//! 123 Cloud Open Platform API type definitions

use serde::{Deserialize, Serialize};

/// 驱动配置 / Driver configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pan123OpenConfig {
    /// 客户端ID (从开发者平台获取)
    /// Client ID (obtained from developer platform)
    pub client_id: String,

    /// 客户端密钥 (从开发者平台获取)
    /// Client secret (obtained from developer platform)
    pub client_secret: String,

    /// 访问令牌 (自动刷新)
    /// Access token (auto-refreshed)
    #[serde(default)]
    pub access_token: String,

    /// 刷新令牌 (OAuth2模式)
    /// Refresh token (OAuth2 mode)
    #[serde(default)]
    pub refresh_token: String,

    /// 上传并发线程数 (1-32)
    /// Upload thread count (1-32)
    #[serde(default = "default_upload_thread")]
    pub upload_thread: u32,

    /// 是否使用直链下载
    /// Whether to use direct link for download
    #[serde(default)]
    pub direct_link: bool,

    /// 直链私钥 (用于URL鉴权)
    /// Direct link private key (for URL authentication)
    #[serde(default)]
    pub direct_link_private_key: String,

    /// 直链有效期 (分钟)
    /// Direct link validity duration (minutes)
    #[serde(default = "default_direct_link_duration")]
    pub direct_link_valid_duration: i64,
}

fn default_upload_thread() -> u32 {
    3
}

fn default_direct_link_duration() -> i64 {
    30
}

impl Default for Pan123OpenConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            access_token: String::new(),
            refresh_token: String::new(),
            upload_thread: 3,
            direct_link: false,
            direct_link_private_key: String::new(),
            direct_link_valid_duration: 30,
        }
    }
}

// ============ API 响应结构体 / API Response Structures ============

/// 基础响应结构 / Base response structure
#[derive(Debug, Clone, Deserialize)]
pub struct BaseResponse {
    /// 响应码 (0表示成功) / Response code (0 means success)
    pub code: i32,
    /// 错误消息 / Error message
    #[serde(default)]
    pub message: String,
}

/// 访问令牌响应 / Access token response
#[derive(Debug, Clone, Deserialize)]
pub struct AccessTokenResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<AccessTokenData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccessTokenData {
    /// 访问令牌 / Access token
    #[serde(rename = "accessToken")]
    pub access_token: String,
    /// 过期时间 / Expiration time
    #[serde(rename = "expiredAt")]
    pub expired_at: String,
}

/// OAuth2刷新令牌响应 / OAuth2 refresh token response
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub refresh_token: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub token_type: String,
}

/// 用户信息响应 / User info response
#[derive(Debug, Clone, Deserialize)]
pub struct UserInfoResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<UserInfoData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfoData {
    /// 用户ID / User ID
    pub uid: u64,
    /// 已使用空间 (字节) / Used space (bytes)
    #[serde(rename = "spaceUsed")]
    pub space_used: u64,
    /// 永久空间 (字节) / Permanent space (bytes)
    #[serde(rename = "spacePermanent")]
    pub space_permanent: u64,
    /// 临时空间 (字节) / Temporary space (bytes)
    #[serde(rename = "spaceTemp")]
    pub space_temp: u64,
}

/// 文件信息 / File information
#[derive(Debug, Clone, Deserialize)]
pub struct FileInfo {
    /// 文件名 / File name
    #[serde(rename = "filename")]
    pub file_name: String,
    /// 文件大小 (字节) / File size (bytes)
    pub size: i64,
    /// 创建时间 / Creation time
    #[serde(rename = "createAt")]
    pub create_at: String,
    /// 更新时间 / Update time
    #[serde(rename = "updateAt")]
    pub update_at: String,
    /// 文件ID / File ID
    #[serde(rename = "fileId")]
    pub file_id: i64,
    /// 类型 (1=目录, 2=文件) / Type (1=directory, 2=file)
    #[serde(rename = "type")]
    pub file_type: i32,
    /// 文件MD5 / File MD5
    #[serde(default)]
    pub etag: String,
    /// S3 Key标识 / S3 Key flag
    #[serde(rename = "s3KeyFlag", default)]
    pub s3_key_flag: String,
    /// 父目录ID / Parent directory ID
    #[serde(rename = "parentFileId")]
    pub parent_file_id: i64,
    /// 文件类别 / File category
    #[serde(default)]
    pub category: i32,
    /// 状态 / Status
    #[serde(default)]
    pub status: i32,
    /// 是否在回收站 (0=否, 1=是) / Trashed (0=no, 1=yes)
    #[serde(default)]
    pub trashed: i32,
}

impl FileInfo {
    /// 是否为目录 / Whether it is a directory
    pub fn is_dir(&self) -> bool {
        self.file_type == 1
    }
}

/// 文件列表响应 / File list response
#[derive(Debug, Clone, Deserialize)]
pub struct FileListResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<FileListData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileListData {
    /// 最后一个文件ID (用于分页, -1表示无更多)
    /// Last file ID (for pagination, -1 means no more)
    #[serde(rename = "lastFileId")]
    pub last_file_id: i64,
    /// 文件列表 / File list
    #[serde(rename = "fileList", default)]
    pub file_list: Vec<FileInfo>,
}

/// 下载信息响应 / Download info response
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfoResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<DownloadInfoData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfoData {
    /// 下载链接 / Download URL
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
}

/// 直链响应 / Direct link response
#[derive(Debug, Clone, Deserialize)]
pub struct DirectLinkResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<DirectLinkData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DirectLinkData {
    /// 直链URL / Direct link URL
    pub url: String,
}

/// 创建文件响应 (V2) / Create file response (V2)
#[derive(Debug, Clone, Deserialize)]
pub struct UploadCreateResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<UploadCreateData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadCreateData {
    /// 文件ID / File ID
    #[serde(rename = "fileID")]
    pub file_id: i64,
    /// 预上传ID / Pre-upload ID
    #[serde(rename = "preuploadID")]
    pub preupload_id: String,
    /// 是否秒传成功 / Whether instant upload succeeded
    #[serde(default)]
    pub reuse: bool,
    /// 分片大小 / Slice size
    #[serde(rename = "sliceSize")]
    pub slice_size: i64,
    /// 上传服务器列表 / Upload server list
    #[serde(default)]
    pub servers: Vec<String>,
}

/// 上传完成响应 (V2) / Upload complete response (V2)
#[derive(Debug, Clone, Deserialize)]
pub struct UploadCompleteResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<UploadCompleteData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadCompleteData {
    /// 是否完成 / Whether completed
    #[serde(default)]
    pub completed: bool,
    /// 文件ID / File ID
    #[serde(rename = "fileID")]
    pub file_id: i64,
}

/// 离线下载响应 / Offline download response
#[derive(Debug, Clone, Deserialize)]
pub struct OfflineDownloadResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<OfflineDownloadData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfflineDownloadData {
    /// 任务ID / Task ID
    #[serde(rename = "taskID")]
    pub task_id: i32,
}

/// 离线下载进度响应 / Offline download progress response
#[derive(Debug, Clone, Deserialize)]
pub struct OfflineDownloadProgressResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
    pub data: Option<OfflineDownloadProgressData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfflineDownloadProgressData {
    /// 进度 (0-100) / Progress (0-100)
    pub process: f64,
    /// 状态 / Status
    pub status: i32,
}

/// 分片上传响应 / Slice upload response
#[derive(Debug, Clone, Deserialize)]
pub struct SliceUploadResponse {
    #[serde(flatten)]
    pub base: BaseResponse,
}

// ============ API 请求结构体 / API Request Structures ============

/// 创建文件请求 / Create file request
#[derive(Debug, Clone, Serialize)]
pub struct CreateFileRequest {
    /// 父目录ID / Parent directory ID
    #[serde(rename = "parentFileId")]
    pub parent_file_id: i64,
    /// 文件名 / File name
    pub filename: String,
    /// 文件MD5 (小写) / File MD5 (lowercase)
    pub etag: String,
    /// 文件大小 / File size
    pub size: i64,
    /// 重复处理 (1=跳过, 2=覆盖) / Duplicate handling (1=skip, 2=overwrite)
    pub duplicate: i32,
    /// 是否包含目录 / Whether contains directory
    #[serde(rename = "containDir")]
    pub contain_dir: bool,
}

/// 创建目录请求 / Create directory request
#[derive(Debug, Clone, Serialize)]
pub struct CreateDirRequest {
    /// 父目录ID / Parent directory ID
    #[serde(rename = "parentID")]
    pub parent_id: String,
    /// 目录名 / Directory name
    pub name: String,
}

/// 移动文件请求 / Move file request
#[derive(Debug, Clone, Serialize)]
pub struct MoveFileRequest {
    /// 文件ID列表 / File ID list
    #[serde(rename = "fileIDs")]
    pub file_ids: Vec<i64>,
    /// 目标父目录ID / Target parent directory ID
    #[serde(rename = "toParentFileID")]
    pub to_parent_file_id: i64,
}

/// 重命名请求 / Rename request
#[derive(Debug, Clone, Serialize)]
pub struct RenameRequest {
    /// 文件ID / File ID
    #[serde(rename = "fileId")]
    pub file_id: i64,
    /// 新文件名 / New file name
    #[serde(rename = "fileName")]
    pub file_name: String,
}

/// 删除文件请求 / Delete file request
#[derive(Debug, Clone, Serialize)]
pub struct TrashRequest {
    /// 文件ID列表 / File ID list
    #[serde(rename = "fileIDs")]
    pub file_ids: Vec<i64>,
}

/// 上传完成请求 / Upload complete request
#[derive(Debug, Clone, Serialize)]
pub struct UploadCompleteRequest {
    /// 预上传ID / Pre-upload ID
    #[serde(rename = "preuploadID")]
    pub preupload_id: String,
}

/// 获取访问令牌请求 / Get access token request
#[derive(Debug, Clone, Serialize)]
pub struct GetAccessTokenRequest {
    /// 客户端ID / Client ID
    #[serde(rename = "clientID")]
    pub client_id: String,
    /// 客户端密钥 / Client secret
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
}

/// 离线下载请求 / Offline download request
#[derive(Debug, Clone, Serialize)]
pub struct OfflineDownloadRequest {
    /// 下载URL / Download URL
    pub url: String,
    /// 目标目录ID / Target directory ID
    #[serde(rename = "dirID")]
    pub dir_id: String,
    /// 回调URL (可选) / Callback URL (optional)
    #[serde(rename = "callBackUrl", skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}
