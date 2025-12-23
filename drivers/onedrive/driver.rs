//! OneDrive OAuth driver implementation / OneDrive OAuth 驱动实现
//! 
//! Uses refresh_token OAuth authorization method / 使用refresh_token OAuth授权方式

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

/// OneDrive region configuration / OneDrive区域配置
struct HostConfig {
    oauth: &'static str,
    api: &'static str,
}

/// Region to host mapping / 区域到主机的映射
fn get_host_config(region: &str) -> HostConfig {
    match region {
        "cn" => HostConfig {
            oauth: "https://login.chinacloudapi.cn",
            api: "https://microsoftgraph.chinacloudapi.cn",
        },
        "us" => HostConfig {
            oauth: "https://login.microsoftonline.us",
            api: "https://graph.microsoft.us",
        },
        "de" => HostConfig {
            oauth: "https://login.microsoftonline.de",
            api: "https://graph.microsoft.de",
        },
        _ => HostConfig { // global
            oauth: "https://login.microsoftonline.com",
            api: "https://graph.microsoft.com",
        },
    }
}

/// OneDrive OAuth configuration / OneDrive OAuth 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneDriveConfig {
    /// Region: global, cn, us, de / 区域
    pub region: String,
    /// Whether it's SharePoint mode / 是否为SharePoint模式
    #[serde(default)]
    pub is_sharepoint: bool,
    /// Client ID / 客户端ID
    pub client_id: String,
    /// Client secret / 客户端密码
    pub client_secret: String,
    /// Redirect URI / 重定向URI
    pub redirect_uri: String,
    /// Refresh token / 刷新令牌
    pub refresh_token: String,
    /// SharePoint site ID (only needed for SharePoint mode) / SharePoint站点ID
    #[serde(default)]
    pub site_id: Option<String>,
    /// Root folder path / 根文件夹路径
    #[serde(default = "default_root")]
    pub root_folder_path: String,
    /// Chunk upload size (MB) / 分块上传大小
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u64,
    /// Custom download domain / 自定义下载域名
    #[serde(default)]
    pub custom_host: Option<String>,
    /// Show space information / 显示空间信息
    #[serde(default = "default_show_space")]
    pub show_space_info: bool,
    /// Enable frontend direct upload / 启用前端直传
    #[serde(default)]
    pub enable_direct_upload: bool,
}

fn default_root() -> String {
    "/".to_string()
}

fn default_chunk_size() -> u64 {
    5
}

fn default_show_space() -> bool {
    true
}

/// Token response / Token响应
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[allow(dead_code)]
    expires_in: u64,
}

/// Token error / Token错误
#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: String,
}

/// API error / API错误
#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    code: String,
    message: String,
}

/// OneDrive文件信息
#[derive(Debug, Deserialize)]
struct OneDriveFile {
    #[allow(dead_code)]
    id: String,
    name: String,
    size: Option<i64>,
    #[serde(rename = "lastModifiedDateTime")]
    last_modified: Option<String>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
    file: Option<FileDetail>,
    #[serde(rename = "parentReference")]
    #[allow(dead_code)]
    parent_reference: Option<ParentReference>,
}

#[derive(Debug, Deserialize)]
struct FileDetail {
    #[serde(rename = "mimeType")]
    #[allow(dead_code)]
    mime_type: String,
}

#[derive(Debug, Deserialize)]
struct ParentReference {
    #[serde(rename = "driveId")]
    #[allow(dead_code)]
    drive_id: String,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
struct FilesResponse {
    value: Vec<OneDriveFile>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

/// OneDrive驱动能力配置
fn onedrive_capability() -> Capability {
    Capability {
        can_range_read: true,
        can_append: false,
        can_direct_link: true,  // OneDrive支持302重定向
        max_chunk_size: Some(60 * 1024 * 1024), // 60MB
        can_concurrent_upload: false,
        requires_oauth: true,
        can_multipart_upload: false, // OneDrive不需要缓存完整文件，直接流式写入
        can_server_side_copy: true,
        can_batch_operations: false,
        max_file_size: Some(250 * 1024 * 1024 * 1024), // 250GB
    }
}

/// 上传会话响应
#[derive(Debug, Deserialize)]
struct UploadSessionResponse {
    #[serde(rename = "uploadUrl")]
    upload_url: String,
}

/// Drive配额信息
#[derive(Debug, Deserialize)]
struct DriveQuota {
    total: u64,
    used: u64,
    remaining: u64,
}

/// Drive响应
#[derive(Debug, Deserialize)]
struct DriveResponse {
    quota: DriveQuota,
}

/// OneDrive OAuth 驱动
pub struct OneDriveDriver {
    config: OneDriveConfig,
    client: Client,
    access_token: Arc<RwLock<Option<String>>>,
    refresh_token: Arc<RwLock<String>>,
}

/// OneDrive写入器 - 流式分片上传（固定内存占用）
pub struct OneDriveWriter {
    /// 固定大小缓冲区（只保存一个分片）
    buffer: Vec<u8>,
    /// 分片大小（字节）
    chunk_size_bytes: u64,
    /// 已上传的字节数
    uploaded_bytes: u64,
    /// 文件总大小（用于Content-Range）
    total_size: u64,
    /// 上传会话URL（大文件上传用）
    upload_session_url: Option<String>,
    /// 是否已初始化会话
    session_initialized: bool,
    path: String,
    client: Client,
    access_token: String,
    api_base: String,
    is_sharepoint: bool,
    site_id: Option<String>,
    closed: bool,
    /// 上传过程中的错误
    error: Option<String>,
}

impl OneDriveWriter {
    fn new(
        path: String,
        size_hint: Option<u64>,
        client: Client,
        access_token: String,
        api_base: String,
        is_sharepoint: bool,
        site_id: Option<String>,
        chunk_size: u64,
    ) -> Self {
        let chunk_size_bytes = chunk_size * 1024 * 1024; // MB to bytes
        Self {
            buffer: Vec::with_capacity(chunk_size_bytes as usize),
            chunk_size_bytes,
            uploaded_bytes: 0,
            total_size: size_hint.unwrap_or(0),
            upload_session_url: None,
            session_initialized: false,
            path,
            client,
            access_token,
            api_base,
            is_sharepoint,
            site_id,
            closed: false,
            error: None,
        }
    }

    fn get_meta_url(&self, path: &str) -> String {
        let clean_path = path.trim_start_matches('/').trim_end_matches('/');
        let encoded_path = if clean_path.is_empty() {
            String::new()
        } else {
            clean_path.split('/')
                .map(|segment| urlencoding::encode(segment).to_string())
                .collect::<Vec<_>>()
                .join("/")
        };

        if self.is_sharepoint {
            if let Some(ref site_id) = self.site_id {
                if encoded_path.is_empty() {
                    format!("{}/v1.0/sites/{}/drive/root", self.api_base, site_id)
                } else {
                    format!("{}/v1.0/sites/{}/drive/root:/{}:", self.api_base, site_id, encoded_path)
                }
            } else {
                if encoded_path.is_empty() {
                    format!("{}/v1.0/me/drive/root", self.api_base)
                } else {
                    format!("{}/v1.0/me/drive/root:/{}:", self.api_base, encoded_path)
                }
            }
        } else {
            if encoded_path.is_empty() {
                format!("{}/v1.0/me/drive/root", self.api_base)
            } else {
                format!("{}/v1.0/me/drive/root:/{}:", self.api_base, encoded_path)
            }
        }
    }

    /// 创建上传会话（大文件用）
    async fn create_upload_session(&mut self) -> std::io::Result<()> {
        let session_url = format!("{}/createUploadSession", self.get_meta_url(&self.path));
        
        let session_body = serde_json::json!({
            "item": {
                "@microsoft.graph.conflictBehavior": "replace"
            }
        });

        let response = self.client
            .post(&session_url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&session_body)
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

        let session: UploadSessionResponse = response.json().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        self.upload_session_url = Some(session.upload_url);
        self.session_initialized = true;
        Ok(())
    }

    /// 上传一个分片
    async fn upload_chunk(&mut self, chunk: &[u8], is_last: bool) -> std::io::Result<()> {
        let start = self.uploaded_bytes;
        let end = start + chunk.len() as u64;
        
        // 如果是最后一个分片且还没初始化会话，说明是小文件，直接上传
        if is_last && !self.session_initialized && self.uploaded_bytes == 0 {
            return self.upload_small(chunk).await;
        }
        
        // 确保会话已创建
        if !self.session_initialized {
            self.create_upload_session().await?;
        }
        
        let upload_url = self.upload_session_url.as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "上传会话未创建"))?;
        
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
        if !status.is_success() && status.as_u16() != 202 {
            let text = response.text().await.unwrap_or_default();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("分片上传失败: HTTP {} - {}", status, text)
            ));
        }
        
        self.uploaded_bytes = end;
        tracing::debug!("OneDrive分片上传: path={}, range={}, uploaded={}/{}", 
            self.path, content_range, self.uploaded_bytes, self.total_size);
        
        Ok(())
    }

    /// 小文件直接上传 (≤4MB)
    async fn upload_small(&self, data: &[u8]) -> std::io::Result<()> {
        let url = format!("{}/content", self.get_meta_url(&self.path));
        
        let response = self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", data.len().to_string())
            .body(data.to_vec())
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

    /// 刷新缓冲区 - 上传当前buffer中的数据
    async fn flush_buffer(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        
        let chunk = std::mem::take(&mut self.buffer);
        self.upload_chunk(&chunk, false).await?;
        self.buffer = Vec::with_capacity(self.chunk_size_bytes as usize);
        Ok(())
    }
}

impl AsyncWrite for OneDriveWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // 检查是否有之前的错误
        if let Some(ref err) = self.error {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone())));
        }
        
        self.buffer.extend_from_slice(buf);
        
        // 当buffer达到分片大小时，立即上传
        if self.buffer.len() >= self.chunk_size_bytes as usize {
            let chunk = std::mem::take(&mut self.buffer);
            let path = self.path.clone();
            let client = self.client.clone();
            let access_token = self.access_token.clone();
            let api_base = self.api_base.clone();
            let is_sharepoint = self.is_sharepoint;
            let site_id = self.site_id.clone();
            let upload_session_url = self.upload_session_url.clone();
            let session_initialized = self.session_initialized;
            let uploaded_bytes = self.uploaded_bytes;
            let total_size = self.total_size;
            let chunk_size_bytes = self.chunk_size_bytes;
            
            // 同步上传分片
            let result = std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut writer = OneDriveWriter {
                        buffer: Vec::new(),
                        chunk_size_bytes,
                        uploaded_bytes,
                        total_size,
                        upload_session_url,
                        session_initialized,
                        path,
                        client,
                        access_token,
                        api_base,
                        is_sharepoint,
                        site_id,
                        closed: false,
                        error: None,
                    };
                    writer.upload_chunk(&chunk, false).await?;
                    Ok::<_, std::io::Error>((writer.uploaded_bytes, writer.upload_session_url, writer.session_initialized))
                })
            }).join().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "上传线程panic"))?;
            
            match result {
                Ok((new_uploaded, new_url, new_init)) => {
                    self.uploaded_bytes = new_uploaded;
                    self.upload_session_url = new_url;
                    self.session_initialized = new_init;
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
        
        // 检查是否有之前的错误
        if let Some(ref err) = self.error {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone())));
        }
        
        let chunk = std::mem::take(&mut self.buffer);
        let path = self.path.clone();
        let client = self.client.clone();
        let access_token = self.access_token.clone();
        let api_base = self.api_base.clone();
        let is_sharepoint = self.is_sharepoint;
        let site_id = self.site_id.clone();
        let upload_session_url = self.upload_session_url.clone();
        let session_initialized = self.session_initialized;
        let uploaded_bytes = self.uploaded_bytes;
        let total_size = if self.total_size == 0 { self.uploaded_bytes + chunk.len() as u64 } else { self.total_size };
        let chunk_size_bytes = self.chunk_size_bytes;
        
        self.closed = true;
        
        if chunk.is_empty() && self.uploaded_bytes == 0 {
            return Poll::Ready(Ok(()));
        }
        
        tracing::info!("OneDrive Writer shutdown: path={}, remaining={}, total_uploaded={}", 
            path, chunk.len(), uploaded_bytes);
        
        // 上传最后一个分片
        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut writer = OneDriveWriter {
                    buffer: Vec::new(),
                    chunk_size_bytes,
                    uploaded_bytes,
                    total_size,
                    upload_session_url,
                    session_initialized,
                    path,
                    client,
                    access_token,
                    api_base,
                    is_sharepoint,
                    site_id,
                    closed: true,
                    error: None,
                };
                if !chunk.is_empty() {
                    writer.upload_chunk(&chunk, true).await?;
                }
                Ok::<_, std::io::Error>(())
            })
        }).join().map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "上传线程panic"))?;
        
        Poll::Ready(result)
    }
}

impl OneDriveDriver {
    /// 创建新的驱动实例
    pub fn new(config: OneDriveConfig) -> Self {
        let refresh_token = config.refresh_token.clone();
        Self {
            config,
            client: Client::new(),
            access_token: Arc::new(RwLock::new(None)),
            refresh_token: Arc::new(RwLock::new(refresh_token)),
        }
    }

    /// 获取API URL
    fn get_meta_url(&self, path: &str) -> String {
        let host = get_host_config(&self.config.region);
        let clean_path = path.trim_start_matches('/').trim_end_matches('/');
        
        // URL编码路径（不编码斜杠）
        let encoded_path = if clean_path.is_empty() {
            String::new()
        } else {
            clean_path.split('/')
                .map(|segment| urlencoding::encode(segment).to_string())
                .collect::<Vec<_>>()
                .join("/")
        };

        if self.config.is_sharepoint {
            if let Some(ref site_id) = self.config.site_id {
                if encoded_path.is_empty() {
                    format!("{}/v1.0/sites/{}/drive/root", host.api, site_id)
                } else {
                    format!("{}/v1.0/sites/{}/drive/root:/{}:", host.api, site_id, encoded_path)
                }
            } else {
                // 没有site_id时使用me/drive
                if encoded_path.is_empty() {
                    format!("{}/v1.0/me/drive/root", host.api)
                } else {
                    format!("{}/v1.0/me/drive/root:/{}:", host.api, encoded_path)
                }
            }
        } else {
            if encoded_path.is_empty() {
                format!("{}/v1.0/me/drive/root", host.api)
            } else {
                format!("{}/v1.0/me/drive/root:/{}:", host.api, encoded_path)
            }
        }
    }

    /// 获取访问令牌
    async fn get_access_token(&self) -> Result<String> {
        // 先检查缓存的token
        {
            let token = self.access_token.read().await;
            if let Some(ref t) = *token {
                return Ok(t.clone());
            }
        }
        
        // 获取新token
        self.do_refresh_token().await
    }

    /// 刷新访问令牌
    async fn do_refresh_token(&self) -> Result<String> {
        let host = get_host_config(&self.config.region);
        let url = format!("{}/common/oauth2/v2.0/token", host.oauth);

        let current_refresh_token = self.refresh_token.read().await.clone();

        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("client_id", &self.config.client_id);
        params.insert("client_secret", &self.config.client_secret);
        params.insert("redirect_uri", &self.config.redirect_uri);
        params.insert("refresh_token", &current_refresh_token);

        let response = self.client
            .post(&url)
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let token_resp: TokenResponse = response.json().await?;
            
            // 更新access_token
            {
                let mut access_token = self.access_token.write().await;
                *access_token = Some(token_resp.access_token.clone());
            }
            
            // 更新refresh_token
            {
                let mut refresh_token = self.refresh_token.write().await;
                *refresh_token = token_resp.refresh_token;
            }
            
            Ok(token_resp.access_token)
        } else {
            let error: TokenError = response.json().await
                .unwrap_or_else(|_| TokenError {
                    error: "unknown".to_string(),
                    error_description: "Failed to parse error response".to_string(),
                });
            Err(anyhow!("Token刷新失败: {}", error.error_description))
        }
    }

    /// 发起API请求
    async fn request<T>(&self, url: &str, method: reqwest::Method) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let token = self.get_access_token().await?;
        
        let response = self.client
            .request(method.clone(), url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else if response.status() == 401 {
            // Token过期，刷新后重试
            let new_token = self.do_refresh_token().await?;
            let response = self.client
                .request(method, url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send()
                .await?;

            if response.status().is_success() {
                Ok(response.json().await?)
            } else {
                let error: ApiError = response.json().await?;
                Err(anyhow!("API错误: {}", error.error.message))
            }
        } else {
            let error: ApiError = response.json().await
                .unwrap_or_else(|_| ApiError {
                    error: ApiErrorDetail {
                        code: "unknown".to_string(),
                        message: "Failed to parse error response".to_string(),
                    },
                });
            Err(anyhow!("API错误: {}", error.error.message))
        }
    }

    /// 获取文件列表
    async fn get_files(&self, path: &str) -> Result<Vec<OneDriveFile>> {
        let mut all_files = Vec::new();
        let base_url = format!("{}/children", self.get_meta_url(path));
        let mut next_link = Some(format!(
            "{}?$top=1000&$select=id,name,size,lastModifiedDateTime,@microsoft.graph.downloadUrl,file,parentReference",
            base_url
        ));

        while let Some(url) = next_link {
            let response: FilesResponse = self.request(&url, reqwest::Method::GET).await?;
            all_files.extend(response.value);
            next_link = response.next_link;
        }

        Ok(all_files)
    }

    /// 获取单个文件信息
    async fn get_file(&self, path: &str) -> Result<OneDriveFile> {
        let url = self.get_meta_url(path);
        self.request(&url, reqwest::Method::GET).await
    }

    /// 转换为Entry
    fn file_to_entry(&self, file: OneDriveFile, parent_path: &str) -> Entry {
        let is_dir = file.file.is_none();
        let size = file.size.unwrap_or(0) as u64;
        
        let path = if parent_path == "/" {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.name)
        };

        Entry {
            name: file.name,
            path,
            is_dir,
            size,
            modified: file.last_modified,
        }
    }
    
    /// 获取Drive信息（包含配额）
    async fn get_drive(&self) -> Result<DriveResponse> {
        let host = get_host_config(&self.config.region);
        let url = if self.config.is_sharepoint {
            if let Some(ref site_id) = self.config.site_id {
                format!("{}/v1.0/sites/{}/drive", host.api, site_id)
            } else {
                format!("{}/v1.0/me/drive", host.api)
            }
        } else {
            format!("{}/v1.0/me/drive", host.api)
        };
        
        self.request(&url, reqwest::Method::GET).await
    }
}

// ============ StorageDriver trait 实现 ============

#[async_trait]
impl StorageDriver for OneDriveDriver {
    fn name(&self) -> &str {
        "onedrive"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        onedrive_capability()
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let files = self.get_files(path).await?;
        Ok(files.into_iter()
            .map(|f| self.file_to_entry(f, path))
            .collect())
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let file = self.get_file(path).await?;
        let download_url = file.download_url
            .ok_or_else(|| anyhow!("文件没有下载链接"))?;

        let final_url = if let Some(ref custom_host) = self.config.custom_host {
            let mut parsed = reqwest::Url::parse(&download_url)?;
            parsed.set_host(Some(custom_host))?;
            parsed.to_string()
        } else {
            download_url
        };

        let mut request = self.client.get(&final_url);
        if let Some(ref r) = range {
            request = request.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }

        // 流式传输：将响应体转换为AsyncRead，不加载到内存
        let stream = response.bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = StreamReader::new(stream);
        Ok(Box::new(reader))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        _progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let token = self.get_access_token().await?;
        let host = get_host_config(&self.config.region);
        
        let writer = OneDriveWriter::new(
            path.to_string(),
            size_hint,
            self.client.clone(),
            token,
            host.api.to_string(),
            self.config.is_sharepoint,
            self.config.site_id.clone(),
            self.config.chunk_size,
        );
        
        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.get_meta_url(path);
        let token = self.get_access_token().await?;
        
        let response = self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() || response.status() == 204 {
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

        let url = format!("{}/children", self.get_meta_url(&parent_path));
        let token = self.get_access_token().await?;
        
        let body = serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("创建目录失败"))
        }
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let url = self.get_meta_url(old_path);
        let token = self.get_access_token().await?;
        
        let body = serde_json::json!({ "name": new_name });

        let response = self.client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("重命名失败"))
        }
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let url = self.get_meta_url(old_path);
        let token = self.get_access_token().await?;
        
        let new_parent = std::path::Path::new(new_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        
        let new_name = std::path::Path::new(new_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的目标路径"))?;

        let parent_path = if new_parent == "/" {
            "/drive/root".to_string()
        } else {
            format!("/drive/root:/{}", new_parent.trim_start_matches('/'))
        };

        let body = serde_json::json!({
            "parentReference": { "path": parent_path },
            "name": new_name
        });

        let response = self.client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("移动失败"))
        }
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        let file = self.get_file(path).await?;
        
        if let Some(download_url) = file.download_url {
            let final_url = if let Some(ref custom_host) = self.config.custom_host {
                let mut parsed = reqwest::Url::parse(&download_url)?;
                parsed.set_host(Some(custom_host))?;
                parsed.to_string()
            } else {
                download_url
            };
            Ok(Some(final_url))
        } else {
            Ok(None)
        }
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        match self.get_drive().await {
            Ok(drive) => {
                Ok(Some(SpaceInfo {
                    used: drive.quota.used,
                    total: drive.quota.total,
                    free: drive.quota.remaining,
                }))
            }
            Err(e) => {
                tracing::warn!("获取OneDrive空间信息失败: {}", e);
                Ok(None)
            }
        }
    }
    
    fn show_space_in_frontend(&self) -> bool {
        self.config.show_space_info
    }
}

// ============ DriverFactory 实现 ============

pub struct OneDriveDriverFactory;

impl DriverFactory for OneDriveDriverFactory {
    fn driver_type(&self) -> &'static str {
        "OneDrive"
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let od_config: OneDriveConfig = serde_json::from_value(config)?;
        Ok(Box::new(OneDriveDriver::new(od_config)))
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "OneDrive".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("region", "select")
                .title("地区")
                .options("global:国际版,cn:中国版(世纪互联),us:美国政府版,de:德国版")
                .default("global")
                .required(),
            ConfigItem::new("is_sharepoint", "bool")
                .title("SharePoint模式")
                .default("false"),
            ConfigItem::new("client_id", "string")
                .title("客户端 ID")
                .required(),
            ConfigItem::new("client_secret", "string")
                .title("客户端密钥")
                .required(),
            ConfigItem::new("redirect_uri", "string")
                .title("回调地址")
                .default("http://localhost:3000/api/onedrive/callback")
                .required(),
            ConfigItem::new("refresh_token", "string")
                .title("刷新令牌")
                .required(),
            ConfigItem::new("site_id", "string")
                .title("站点 ID")
                .help("SharePoint站点ID（仅SharePoint模式需要）"),
            ConfigItem::new("root_folder_path", "string")
                .title("根文件夹路径")
                .default("/"),
            ConfigItem::new("chunk_size", "number")
                .title("分片大小")
                .default("5")
                .help("上传分片大小(MB)"),
            ConfigItem::new("custom_host", "string")
                .title("自定义主机")
                .help("自定义加速下载链接"),
            ConfigItem::new("show_space_info", "bool")
                .title("显示空间信息")
                .default("true"),
            ConfigItem::new("enable_direct_upload", "bool")
                .title("启用前端直传")
                .default("false")
                .help("允许不经服务器直接上传到OneDrive"),
        ]
    }
}
