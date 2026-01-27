//! Google Drive 驱动实现
//! 
//! 支持 OAuth refresh_token 授权、在线API刷新token
//! 流式分片上传（内存占用<40MB）

use async_trait::async_trait;
use anyhow::{Result, anyhow};
use futures::TryStreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::RwLock;
use tokio_util::io::StreamReader;

use crate::storage::{
    StorageDriver, Entry, Capability, SpaceInfo,
    DriverFactory, DriverConfig, ConfigItem,
};

// ============ 配置结构 ============

/// Google Drive 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleDriveConfig {
    /// 刷新令牌（可以是token或服务账号JSON文件路径）
    pub refresh_token: String,
    /// 客户端ID
    pub client_id: String,
    /// 客户端密钥
    pub client_secret: String,
    /// 根目录ID（默认为root）
    #[serde(default = "default_root_id")]
    pub root_id: String,
    /// 排序方式
    #[serde(default)]
    pub order_by: String,
    /// 排序方向
    #[serde(default)]
    pub order_direction: String,
    /// 分片上传大小（MB）
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u64,
    /// 显示空间信息
    #[serde(default = "default_show_space")]
    pub show_space_info: bool,
}

fn default_root_id() -> String { "root".to_string() }
fn default_chunk_size() -> u64 { 5 }
fn default_show_space() -> bool { true }

// ============ API响应结构 ============

/// Token刷新响应
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    expires_in: Option<u64>,
}

/// Token错误
#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// API错误
#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    code: i32,
    message: String,
    #[allow(dead_code)]
    errors: Option<Vec<ApiErrorItem>>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorItem {
    #[allow(dead_code)]
    domain: Option<String>,
    #[allow(dead_code)]
    reason: Option<String>,
    #[allow(dead_code)]
    message: Option<String>,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
struct FilesResponse {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    files: Vec<GoogleFile>,
}

/// Google Drive文件
#[derive(Debug, Deserialize)]
struct GoogleFile {
    id: String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    #[serde(rename = "modifiedTime")]
    modified_time: Option<String>,
    #[serde(rename = "createdTime")]
    #[allow(dead_code)]
    created_time: Option<String>,
    size: Option<String>,
    #[serde(rename = "thumbnailLink")]
    #[allow(dead_code)]
    thumbnail_link: Option<String>,
    #[serde(rename = "shortcutDetails")]
    shortcut_details: Option<ShortcutDetails>,
    #[serde(rename = "md5Checksum")]
    #[allow(dead_code)]
    md5_checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ShortcutDetails {
    #[serde(rename = "targetId")]
    target_id: String,
    #[serde(rename = "targetMimeType")]
    target_mime_type: String,
}

/// About响应（存储配额）
#[derive(Debug, Deserialize)]
struct AboutResponse {
    #[serde(rename = "storageQuota")]
    storage_quota: StorageQuota,
}

#[derive(Debug, Deserialize)]
struct StorageQuota {
    limit: Option<String>,
    usage: String,
    #[serde(rename = "usageInDrive")]
    #[allow(dead_code)]
    usage_in_drive: Option<String>,
}

// ============ 常量 ============

const FILES_LIST_FIELDS: &str = "files(id,name,mimeType,size,modifiedTime,createdTime,thumbnailLink,shortcutDetails,md5Checksum),nextPageToken";
const FILE_INFO_FIELDS: &str = "id,name,mimeType,size,md5Checksum";

// ============ 驱动能力 ============

fn google_drive_capability() -> Capability {
    Capability {
        can_range_read: true,
        can_append: false,
        can_direct_link: true,
        max_chunk_size: Some(256 * 1024 * 1024), // 256MB
        can_concurrent_upload: false,
        requires_oauth: true,
        can_multipart_upload: true,
        can_server_side_copy: false,
        can_batch_operations: false,
        max_file_size: Some(5 * 1024 * 1024 * 1024 * 1024), // 5TB
        requires_full_file_for_upload: false,
    }
}

// ============ 驱动主体 ============

/// Google Drive 驱动
pub struct GoogleDriveDriver {
    config: GoogleDriveConfig,
    client: Client,
    access_token: Arc<RwLock<Option<String>>>,
    refresh_token: Arc<RwLock<String>>,
    /// 文件ID缓存 (path -> id)
    path_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl GoogleDriveDriver {
    /// 创建新的驱动实例
    pub fn new(config: GoogleDriveConfig) -> Self {
        let refresh_token = config.refresh_token.clone();
        let root_id = config.root_id.clone();
        
        let mut path_cache = HashMap::new();
        path_cache.insert("/".to_string(), root_id);
        
        Self {
            config,
            client: Client::new(),
            access_token: Arc::new(RwLock::new(None)),
            refresh_token: Arc::new(RwLock::new(refresh_token)),
            path_cache: Arc::new(RwLock::new(path_cache)),
        }
    }

    /// 获取访问令牌
    async fn get_access_token(&self) -> Result<String> {
        {
            let token = self.access_token.read().await;
            if let Some(ref t) = *token {
                return Ok(t.clone());
            }
        }
        self.do_refresh_token().await
    }

    /// 刷新访问令牌
    async fn do_refresh_token(&self) -> Result<String> {
        if self.config.client_id.is_empty() || self.config.client_secret.is_empty() {
            return Err(anyhow!("未配置client_id或client_secret"));
        }

        let current_refresh = self.refresh_token.read().await.clone();
        
        let mut params = HashMap::new();
        params.insert("client_id", self.config.client_id.as_str());
        params.insert("client_secret", self.config.client_secret.as_str());
        params.insert("refresh_token", current_refresh.as_str());
        params.insert("grant_type", "refresh_token");

        let response = self.client
            .post("https://www.googleapis.com/oauth2/v4/token")
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let token_resp: TokenResponse = response.json().await?;
            {
                let mut at = self.access_token.write().await;
                *at = Some(token_resp.access_token.clone());
            }
            Ok(token_resp.access_token)
        } else {
            let error: TokenError = response.json().await
                .unwrap_or_else(|_| TokenError {
                    error: "unknown".to_string(),
                    error_description: Some("解析错误响应失败".to_string()),
                });
            Err(anyhow!("Token刷新失败: {}", error.error_description.unwrap_or(error.error)))
        }
    }

    /// 发起API请求
    async fn request(&self, url: &str, method: reqwest::Method, body: Option<Value>) -> Result<reqwest::Response> {
        let token = self.get_access_token().await?;
        
        let mut request = self.client
            .request(method.clone(), url)
            .header("Authorization", format!("Bearer {}", token))
            .query(&[
                ("includeItemsFromAllDrives", "true"),
                ("supportsAllDrives", "true"),
            ]);

        if let Some(ref b) = body {
            request = request.json(b);
        }

        let response = request.send().await?;

        if response.status() == 401 {
            // Token过期，刷新后重试
            {
                let mut at = self.access_token.write().await;
                *at = None;
            }
            let new_token = self.do_refresh_token().await?;
            
            let mut request = self.client
                .request(method, url)
                .header("Authorization", format!("Bearer {}", new_token))
                .query(&[
                    ("includeItemsFromAllDrives", "true"),
                    ("supportsAllDrives", "true"),
                ]);

            if let Some(b) = body {
                request = request.json(&b);
            }

            return Ok(request.send().await?);
        }

        Ok(response)
    }

    /// 获取文件ID（从路径）
    async fn get_file_id(&self, path: &str) -> Result<String> {
        let normalized = if path.is_empty() || path == "/" {
            "/".to_string()
        } else {
            format!("/{}", path.trim_matches('/'))
        };

        // 检查缓存
        {
            let cache = self.path_cache.read().await;
            if let Some(id) = cache.get(&normalized) {
                return Ok(id.clone());
            }
        }

        // 逐级查找
        let parts: Vec<&str> = normalized.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.config.root_id.clone();
        let mut current_path = String::new();

        for part in parts {
            current_path = format!("{}/{}", current_path, part);
            
            // 检查缓存
            {
                let cache = self.path_cache.read().await;
                if let Some(id) = cache.get(&current_path) {
                    current_id = id.clone();
                    continue;
                }
            }

            // 查找子项
            let url = format!(
                "https://www.googleapis.com/drive/v3/files?q='{}'+in+parents+and+name='{}'+and+trashed=false&fields=files(id,name)&pageSize=1",
                current_id,
                urlencoding::encode(part)
            );

            let response = self.request(&url, reqwest::Method::GET, None).await?;
            
            if !response.status().is_success() {
                return Err(anyhow!("查找文件失败: {}", path));
            }

            let files: FilesResponse = response.json().await?;
            if files.files.is_empty() {
                return Err(anyhow!("文件不存在: {}", path));
            }

            current_id = files.files[0].id.clone();
            
            // 更新缓存
            {
                let mut cache = self.path_cache.write().await;
                cache.insert(current_path.clone(), current_id.clone());
            }
        }

        Ok(current_id)
    }

    /// 获取目录下的文件列表
    async fn get_files(&self, parent_id: &str) -> Result<Vec<GoogleFile>> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let order_by = if self.config.order_by.is_empty() {
                "folder,name,modifiedTime desc".to_string()
            } else {
                format!("{} {}", self.config.order_by, self.config.order_direction)
            };

            let mut url = format!(
                "https://www.googleapis.com/drive/v3/files?orderBy={}&fields={}&pageSize=1000&q='{}'+in+parents+and+trashed=false",
                urlencoding::encode(&order_by),
                urlencoding::encode(FILES_LIST_FIELDS),
                parent_id
            );

            if let Some(ref token) = page_token {
                url = format!("{}&pageToken={}", url, token);
            }

            let response = self.request(&url, reqwest::Method::GET, None).await?;
            
            if !response.status().is_success() {
                let text = response.text().await.unwrap_or_default();
                return Err(anyhow!("获取文件列表失败: {}", text));
            }

            let files_resp: FilesResponse = response.json().await?;
            all_files.extend(files_resp.files);
            
            page_token = files_resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(all_files)
    }

    /// 转换为Entry
    fn file_to_entry(&self, file: GoogleFile, parent_path: &str) -> Entry {
        let is_dir = file.mime_type == "application/vnd.google-apps.folder";
        let size = file.size.and_then(|s| s.parse().ok()).unwrap_or(0);
        
        let path = if parent_path == "/" {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.name)
        };

        // 处理快捷方式
        let final_id = if file.mime_type == "application/vnd.google-apps.shortcut" {
            if let Some(ref details) = file.shortcut_details {
                details.target_id.clone()
            } else {
                file.id
            }
        } else {
            file.id
        };

        // 更新缓存
        let path_cache = self.path_cache.clone();
        let path_clone = path.clone();
        tokio::spawn(async move {
            let mut cache = path_cache.write().await;
            cache.insert(path_clone, final_id);
        });

        Entry {
            name: file.name,
            path,
            is_dir,
            size,
            modified: file.modified_time,
        }
    }

    /// 获取存储配额信息
    async fn get_about(&self) -> Result<AboutResponse> {
        let url = "https://www.googleapis.com/drive/v3/about?fields=storageQuota";
        let response = self.request(url, reqwest::Method::GET, None).await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("获取存储信息失败"));
        }

        Ok(response.json().await?)
    }
}

// ============ 流式上传Writer ============

/// Google Drive 写入器 - 流式分片上传
pub struct GoogleDriveWriter {
    /// 固定大小缓冲区（只保存一个分片）
    buffer: Vec<u8>,
    /// 分片大小（字节）
    chunk_size_bytes: u64,
    /// 已上传的字节数
    uploaded_bytes: u64,
    /// 已写入缓冲区的字节数（包含未上传的）
    buffered_bytes: u64,
    /// 文件总大小
    total_size: u64,
    /// Resumable上传URL
    upload_url: Option<String>,
    /// 是否已初始化
    initialized: bool,
    /// 父目录ID
    parent_id: String,
    /// 文件名
    file_name: String,
    /// HTTP客户端
    client: Client,
    /// 访问令牌
    access_token: String,
    /// 是否已关闭
    closed: bool,
    /// 错误信息
    error: Option<String>,
    /// 进度回调
    progress_callback: Option<crate::storage::ProgressCallback>,
    /// 上次报告进度的时间
    last_progress_time: std::time::Instant,
}

impl GoogleDriveWriter {
    fn new(
        parent_id: String,
        file_name: String,
        size_hint: Option<u64>,
        client: Client,
        access_token: String,
        chunk_size: u64,
        progress_callback: Option<crate::storage::ProgressCallback>,
    ) -> Self {
        let chunk_size_bytes = chunk_size * 1024 * 1024;
        Self {
            buffer: Vec::with_capacity(chunk_size_bytes as usize),
            chunk_size_bytes,
            uploaded_bytes: 0,
            buffered_bytes: 0,
            total_size: size_hint.unwrap_or(0),
            upload_url: None,
            initialized: false,
            parent_id,
            file_name,
            client,
            access_token,
            closed: false,
            error: None,
            progress_callback,
            last_progress_time: std::time::Instant::now(),
        }
    }

    /// 报告进度（每秒最多一次）
    fn report_progress(&mut self, force: bool) {
        if let Some(ref callback) = self.progress_callback {
            let now = std::time::Instant::now();
            if force || now.duration_since(self.last_progress_time).as_millis() >= 1000 {
                // 报告已写入缓冲区的字节数，这样前端能看到实时进度
                callback(self.buffered_bytes, self.total_size);
                self.last_progress_time = now;
            }
        }
    }

    /// 创建Resumable上传会话
    async fn create_upload_session(&mut self) -> std::io::Result<()> {
        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&supportsAllDrives=true";
        
        let metadata = serde_json::json!({
            "name": self.file_name,
            "parents": [self.parent_id]
        });

        let response = self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("X-Upload-Content-Type", "application/octet-stream")
            .header("X-Upload-Content-Length", self.total_size.to_string())
            .json(&metadata)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("创建上传会话失败: {}", text)
            ));
        }

        let location = response.headers()
            .get("location")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::Other,
                "未获取到上传URL"
            ))?;

        self.upload_url = Some(location.to_string());
        self.initialized = true;
        Ok(())
    }

    /// 上传一个分片
    async fn upload_chunk(&mut self, chunk: &[u8], is_last: bool) -> std::io::Result<()> {
        // 小文件直接上传
        if is_last && !self.initialized && self.uploaded_bytes == 0 {
            return self.upload_small(chunk).await;
        }

        // 确保会话已创建
        if !self.initialized {
            self.create_upload_session().await?;
        }

        let upload_url = self.upload_url.as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "上传会话未创建"))?;

        let start = self.uploaded_bytes;
        let end = start + chunk.len() as u64;
        let content_range = format!("bytes {}-{}/{}", start, end - 1, self.total_size);

        let response = self.client
            .put(upload_url)
            .header("Content-Range", &content_range)
            .header("Content-Length", chunk.len())
            .body(chunk.to_vec())
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let status = response.status();
        // 308 Resume Incomplete 或 200/201 表示成功
        if !status.is_success() && status.as_u16() != 308 {
            let text = response.text().await.unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("分片上传失败: HTTP {} - {}", status, text)
            ));
        }

        self.uploaded_bytes = end;
        tracing::debug!("Google Drive分片上传: range={}, uploaded={}/{}", 
            content_range, self.uploaded_bytes, self.total_size);

        Ok(())
    }

    /// 小文件直接上传
    async fn upload_small(&self, data: &[u8]) -> std::io::Result<()> {
        let url = format!(
            "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&supportsAllDrives=true"
        );

        let metadata = serde_json::json!({
            "name": self.file_name,
            "parents": [self.parent_id]
        });

        // 使用multipart上传
        let boundary = "boundary_yaolist_google_drive";
        let mut body = Vec::new();
        
        // Metadata part
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body.extend_from_slice(serde_json::to_string(&metadata).unwrap().as_bytes());
        body.extend_from_slice(b"\r\n");
        
        // File part
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}--", boundary).as_bytes());

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", format!("multipart/related; boundary={}", boundary))
            .body(body)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("上传失败: HTTP {} - {}", status, text)
            ))
        }
    }
}

impl AsyncWrite for GoogleDriveWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Some(ref err) = self.error {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone())));
        }

        self.buffer.extend_from_slice(buf);
        self.buffered_bytes += buf.len() as u64;
        
        // 每秒报告一次进度
        self.report_progress(false);

        // 当buffer达到分片大小时，立即上传
        if self.buffer.len() >= self.chunk_size_bytes as usize {
            let chunk = std::mem::take(&mut self.buffer);
            let client = self.client.clone();
            let access_token = self.access_token.clone();
            let parent_id = self.parent_id.clone();
            let file_name = self.file_name.clone();
            let upload_url = self.upload_url.clone();
            let initialized = self.initialized;
            let uploaded_bytes = self.uploaded_bytes;
            let total_size = self.total_size;
            let chunk_size_bytes = self.chunk_size_bytes;

            let result = std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut writer = GoogleDriveWriter {
                        buffer: Vec::new(),
                        chunk_size_bytes,
                        uploaded_bytes,
                        buffered_bytes: 0,
                        total_size,
                        upload_url,
                        initialized,
                        parent_id,
                        file_name,
                        client,
                        access_token,
                        closed: false,
                        error: None,
                        progress_callback: None,
                        last_progress_time: std::time::Instant::now(),
                    };
                    writer.upload_chunk(&chunk, false).await?;
                    Ok::<_, std::io::Error>((writer.uploaded_bytes, writer.upload_url, writer.initialized))
                })
            }).join().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "上传线程panic"))?;

            match result {
                Ok((new_uploaded, new_url, new_init)) => {
                    self.uploaded_bytes = new_uploaded;
                    self.upload_url = new_url;
                    self.initialized = new_init;
                    self.buffer = Vec::with_capacity(self.chunk_size_bytes as usize);
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    return Poll::Ready(Err(e));
                }
            }
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }

        if let Some(ref err) = self.error {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone())));
        }

        let chunk = std::mem::take(&mut self.buffer);
        let client = self.client.clone();
        let access_token = self.access_token.clone();
        let parent_id = self.parent_id.clone();
        let file_name = self.file_name.clone();
        let upload_url = self.upload_url.clone();
        let initialized = self.initialized;
        let uploaded_bytes = self.uploaded_bytes;
        let total_size = if self.total_size == 0 { 
            self.uploaded_bytes + chunk.len() as u64 
        } else { 
            self.total_size 
        };
        let chunk_size_bytes = self.chunk_size_bytes;

        self.closed = true;

        tracing::info!("Google Drive Writer shutdown: remaining={}, total_uploaded={}", 
            chunk.len(), uploaded_bytes);

        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut writer = GoogleDriveWriter {
                    buffer: Vec::new(),
                    chunk_size_bytes,
                    uploaded_bytes,
                    buffered_bytes: 0,
                    total_size,
                    upload_url,
                    initialized,
                    parent_id,
                    file_name,
                    client,
                    access_token,
                    closed: true,
                    error: None,
                    progress_callback: None,
                    last_progress_time: std::time::Instant::now(),
                };
                // 即使是空文件也需要上传（创建）
                writer.upload_chunk(&chunk, true).await?;
                Ok::<_, std::io::Error>(writer.uploaded_bytes)
            })
        }).join().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "上传线程panic"))?;

        match &result {
            Ok(final_bytes) => {
                self.uploaded_bytes = *final_bytes;
                self.buffered_bytes = *final_bytes;
                // 报告最终进度
                self.report_progress(true);
            }
            _ => {}
        }

        Poll::Ready(result.map(|_| ()))
    }
}

// ============ StorageDriver trait 实现 ============

#[async_trait]
impl StorageDriver for GoogleDriveDriver {
    fn name(&self) -> &str {
        "google_drive"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        google_drive_capability()
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let file_id = self.get_file_id(path).await?;
        let files = self.get_files(&file_id).await?;
        Ok(files.into_iter()
            .map(|f| self.file_to_entry(f, path))
            .collect())
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let file_id = self.get_file_id(path).await?;
        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?alt=media&acknowledgeAbuse=true",
            file_id
        );

        let token = self.get_access_token().await?;
        let mut request = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token));

        if let Some(ref r) = range {
            request = request.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }

        let stream = response.bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = StreamReader::new(stream);
        Ok(Box::new(reader))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let file_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的文件路径"))?;

        let parent_id = self.get_file_id(&parent_path).await?;
        let token = self.get_access_token().await?;

        let writer = GoogleDriveWriter::new(
            parent_id,
            file_name,
            size_hint,
            self.client.clone(),
            token,
            self.config.chunk_size,
            progress,
        );

        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let file_id = self.get_file_id(path).await?;
        let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
        
        let response = self.request(&url, reqwest::Method::DELETE, None).await?;
        
        if response.status().is_success() || response.status() == 204 {
            // 清除缓存
            let normalized = format!("/{}", path.trim_matches('/'));
            let mut cache = self.path_cache.write().await;
            cache.remove(&normalized);
            Ok(())
        } else {
            Err(anyhow!("删除失败"))
        }
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let folder_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的路径"))?;

        let parent_id = self.get_file_id(&parent_path).await?;

        let body = serde_json::json!({
            "name": folder_name,
            "parents": [parent_id],
            "mimeType": "application/vnd.google-apps.folder"
        });

        let response = self.request(
            "https://www.googleapis.com/drive/v3/files",
            reqwest::Method::POST,
            Some(body)
        ).await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("创建目录失败"))
        }
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let file_id = self.get_file_id(old_path).await?;
        let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);

        let body = serde_json::json!({ "name": new_name });
        let response = self.request(&url, reqwest::Method::PATCH, Some(body)).await?;

        if response.status().is_success() {
            // 更新缓存
            let old_normalized = format!("/{}", old_path.trim_matches('/'));
            let parent = std::path::Path::new(&old_normalized)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string());
            let new_normalized = if parent == "/" {
                format!("/{}", new_name)
            } else {
                format!("{}/{}", parent, new_name)
            };

            let mut cache = self.path_cache.write().await;
            if let Some(id) = cache.remove(&old_normalized) {
                cache.insert(new_normalized, id);
            }
            Ok(())
        } else {
            Err(anyhow!("重命名失败"))
        }
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let file_id = self.get_file_id(old_path).await?;
        
        let new_parent = std::path::Path::new(new_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        
        let new_name = std::path::Path::new(new_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的目标路径"))?;

        let new_parent_id = self.get_file_id(&new_parent).await?;
        
        // 获取当前父目录
        let old_parent = std::path::Path::new(old_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let old_parent_id = self.get_file_id(&old_parent).await?;

        let url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?addParents={}&removeParents={}",
            file_id, new_parent_id, old_parent_id
        );

        let body = serde_json::json!({ "name": new_name });
        let response = self.request(&url, reqwest::Method::PATCH, Some(body)).await?;

        if response.status().is_success() {
            // 更新缓存
            let old_normalized = format!("/{}", old_path.trim_matches('/'));
            let new_normalized = format!("/{}", new_path.trim_matches('/'));
            
            let mut cache = self.path_cache.write().await;
            if let Some(id) = cache.remove(&old_normalized) {
                cache.insert(new_normalized, id);
            }
            Ok(())
        } else {
            Err(anyhow!("移动失败"))
        }
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // Google Drive API 不支持浏览器直接访问（会被检测为自动化请求）
        // 返回 None 让下载通过后端代理进行
        Ok(None)
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        match self.get_about().await {
            Ok(about) => {
                let used: u64 = about.storage_quota.usage.parse().unwrap_or(0);
                let total: u64 = about.storage_quota.limit
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0);
                let free = if total > used { total - used } else { 0 };
                
                Ok(Some(SpaceInfo { used, total, free }))
            }
            Err(e) => {
                tracing::warn!("获取Google Drive空间信息失败: {}", e);
                Ok(None)
            }
        }
    }

    fn show_space_in_frontend(&self) -> bool {
        self.config.show_space_info
    }

    fn get_updated_config(&self) -> Option<serde_json::Value> {
        // 返回更新后的refresh_token
        let rt = tokio::runtime::Handle::try_current();
        if let Ok(handle) = rt {
            let refresh_token = self.refresh_token.clone();
            let config = self.config.clone();
            
            let new_token = handle.block_on(async {
                refresh_token.read().await.clone()
            });
            
            if new_token != config.refresh_token {
                let mut updated = serde_json::to_value(&config).ok()?;
                updated["refresh_token"] = serde_json::Value::String(new_token);
                return Some(updated);
            }
        }
        None
    }
}

// ============ DriverFactory 实现 ============

pub struct GoogleDriveDriverFactory;

impl DriverFactory for GoogleDriveDriverFactory {
    fn driver_type(&self) -> &'static str {
        "GoogleDrive"
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let gd_config: GoogleDriveConfig = serde_json::from_value(config)?;
        Ok(Box::new(GoogleDriveDriver::new(gd_config)))
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "Google Drive".to_string(),
            local_sort: true,
            only_proxy: true, // Google Drive API需要代理
            no_cache: false,
            no_upload: false,
            default_root: Some("root".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("client_id", "string")
                .title("客户端ID")
                .required()
                .help("Google Cloud Console 创建的 OAuth 2.0 客户端 ID"),
            ConfigItem::new("client_secret", "string")
                .title("客户端密钥")
                .required()
                .help("Google Cloud Console 创建的 OAuth 2.0 客户端密钥"),
            ConfigItem::new("get_token", "oauth")
                .title("获取刷新令牌")
                .link("https://accounts.google.com/o/oauth2/v2/auth?client_id={client_id}&redirect_uri={redirect_uri}&response_type=code&scope=https://www.googleapis.com/auth/drive&access_type=offline&prompt=consent")
                .help("点击按钮跳转到 Google 授权页面。需要先在 Google Cloud Console 的 OAuth 客户端设置中，将 {当前域名}/api/oauth/google/callback 添加到「已获授权的重定向 URI」"),
            ConfigItem::new("refresh_token", "string")
                .title("刷新令牌")
                .required()
                .help("OAuth 授权后获取的刷新令牌"),
            ConfigItem::new("root_id", "string")
                .title("根目录ID")
                .default("root")
                .help("根目录的文件ID，默认为root"),
            ConfigItem::new("order_by", "string")
                .title("排序字段")
                .help("例如: folder,name,modifiedTime"),
            ConfigItem::new("order_direction", "select")
                .title("排序方向")
                .options("asc:升序,desc:降序")
                .default("asc"),
            ConfigItem::new("chunk_size", "number")
                .title("分片大小")
                .default("5")
                .help("上传分片大小(MB)，建议5-10"),
            ConfigItem::new("show_space_info", "bool")
                .title("显示空间信息")
                .default("true"),
        ]
    }
}
