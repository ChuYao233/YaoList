//! PikPak driver implementation / PikPak驱动实现
//!
//! Architecture principles / 架构原则:
//! - Driver only provides primitive capabilities (Reader/Writer) / 驱动只提供原语能力
//! - Core controls progress, concurrency, resume points / Core控制进度、并发、断点
//! - Never load files into memory, use streaming / 永远不把文件放内存，使用流式传输
//! - Support 302 redirect and local proxy / 支持302重定向和本地代理

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::sync::{Arc, RwLock as StdRwLock};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::RwLock;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry,
    ProgressCallback, SpaceInfo, StorageDriver,
};

use super::client::{PikPakClient, Platform};
use super::types::*;
use super::util::*;
use super::writer::PikPakStreamWriter;

/// PikPak driver configuration / PikPak驱动配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PikPakConfig {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default)]
    pub captcha_token: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default = "default_root_folder_id")]
    pub root_folder_id: String,
    #[serde(default = "default_true")]
    pub disable_media_link: bool,
    #[serde(default = "default_true")]
    pub show_space_info: bool,
}

fn default_platform() -> String { "web".to_string() }
fn default_root_folder_id() -> String { "".to_string() }
fn default_true() -> bool { true }

impl Default for PikPakConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            refresh_token: String::new(),
            platform: default_platform(),
            captcha_token: String::new(),
            device_id: String::new(),
            root_folder_id: default_root_folder_id(),
            disable_media_link: true,
            show_space_info: true,
        }
    }
}

/// PikPak storage driver / PikPak存储驱动
pub struct PikPakDriver {
    config: PikPakConfig,
    client: PikPakClient,
    http_client: Client,
    path_cache: Arc<RwLock<HashMap<String, String>>>,
    config_changed: Arc<StdRwLock<bool>>,
    initialized: Arc<StdRwLock<bool>>,
}

impl PikPakDriver {
    /// Create new driver instance (sync) / 创建新驱动实例(同步)
    pub fn new(config: PikPakConfig) -> Self {
        let platform = Platform::from_str(&config.platform);
        let client = PikPakClient::new(platform);
        
        let device_id = if config.device_id.is_empty() {
            md5_hash(&format!("{}{}", config.username, config.password))
        } else {
            config.device_id.clone()
        };
        
        // 同步初始化token信息 (现在是同步方法)
        client.init_token(&device_id, &config.refresh_token, &config.captcha_token);
        
        Self {
            config,
            client,
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::limited(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            config_changed: Arc::new(StdRwLock::new(false)),
            initialized: Arc::new(StdRwLock::new(false)),
        }
    }

    /// Ensure driver is initialized / 确保驱动已初始化
    async fn ensure_init(&self) -> Result<()> {
        {
            let initialized = self.initialized.read().unwrap();
            if *initialized {
                return Ok(());
            }
        }
        self.authenticate().await?;
        *self.initialized.write().unwrap() = true;
        Ok(())
    }

    async fn authenticate(&self) -> Result<()> {
        if !self.config.refresh_token.is_empty() {
            match self.client.refresh_token(&self.config.refresh_token).await {
                Ok(_) => {
                    let info = self.client.get_token_info();
                    self.client.refresh_captcha_token_at_login(
                        &get_action("GET", api::FILES_URL),
                        &info.user_id,
                    ).await?;
                    *self.config_changed.write().unwrap() = true;
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Refresh token failed, trying password login: {}", e);
                }
            }
        }
        
        if !self.config.username.is_empty() && !self.config.password.is_empty() {
            self.client.login(&self.config.username, &self.config.password).await?;
            let info = self.client.get_token_info();
            self.client.refresh_captcha_token_at_login(
                &get_action("GET", api::FILES_URL),
                &info.user_id,
            ).await?;
            *self.config_changed.write().unwrap() = true;
            return Ok(());
        }
        
        Err(anyhow!("No valid credentials / 没有有效的认证信息"))
    }

    async fn get_folder_id(&self, path: &str) -> Result<String> {
        let path = fix_path(path);
        
        if path == "/" {
            return Ok(self.config.root_folder_id.clone());
        }
        
        {
            let cache = self.path_cache.read().await;
            if let Some(id) = cache.get(&path) {
                return Ok(id.clone());
            }
        }
        
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.config.root_folder_id.clone();
        let mut current_path = String::new();
        
        for part in parts {
            current_path = format!("{}/{}", current_path, part);
            
            {
                let cache = self.path_cache.read().await;
                if let Some(id) = cache.get(&current_path) {
                    current_id = id.clone();
                    continue;
                }
            }
            
            let files = self.list_files(&current_id).await?;
            let found = files.iter().find(|f| f.name == part && f.is_dir());
            
            match found {
                Some(f) => {
                    current_id = f.id.clone();
                    let mut cache = self.path_cache.write().await;
                    cache.insert(current_path.clone(), current_id.clone());
                }
                None => {
                    return Err(anyhow!("Folder not found / 文件夹不存在: {}", current_path));
                }
            }
        }
        
        Ok(current_id)
    }

    async fn get_file(&self, path: &str) -> Result<PikPakFile> {
        let path = fix_path(path);
        let parent_path = get_parent_path(&path);
        let file_name = get_file_name(&path);
        
        let parent_id = self.get_folder_id(&parent_path).await?;
        let files = self.list_files(&parent_id).await?;
        
        files.into_iter()
            .find(|f| f.name == file_name)
            .ok_or_else(|| anyhow!("File not found / 文件不存在: {}", path))
    }

    async fn list_files(&self, folder_id: &str) -> Result<Vec<PikPakFile>> {
        let mut all_files = Vec::new();
        let mut page_token = String::new();
        
        loop {
            let query = vec![
                ("parent_id", folder_id.to_string()),
                ("thumbnail_size", "SIZE_LARGE".to_string()),
                ("with_audit", "true".to_string()),
                ("limit", "100".to_string()),
                ("filters", r#"{"phase":{"eq":"PHASE_TYPE_COMPLETE"},"trashed":{"eq":false}}"#.to_string()),
                ("page_token", page_token.clone()),
            ];
            
            let resp: FilesResp = self.client.get(api::FILES_URL, Some(query)).await?;
            all_files.extend(resp.files);
            
            if resp.next_page_token.is_empty() {
                break;
            }
            page_token = resp.next_page_token;
        }
        
        Ok(all_files)
    }
}

#[async_trait]
impl StorageDriver for PikPakDriver {
    fn name(&self) -> &str {
        "PikPak"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,
            max_chunk_size: Some(10 * 1024 * 1024 * 1024),
            can_concurrent_upload: true,
            requires_oauth: false,
            can_multipart_upload: true,
            can_server_side_copy: true,
            can_batch_operations: true,
            max_file_size: None,
            requires_full_file_for_upload: false, // PikPak支持分片上传
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_init().await?;
        let path = fix_path(path);
        let folder_id = self.get_folder_id(&path).await?;
        let files = self.list_files(&folder_id).await?;
        
        let entries: Vec<Entry> = files.iter().map(|f| {
            let file_path = if path == "/" {
                format!("/{}", f.name)
            } else {
                format!("{}/{}", path, f.name)
            };
            
            Entry {
                name: f.name.clone(),
                path: file_path,
                is_dir: f.is_dir(),
                size: f.get_size(),
                modified: parse_datetime(&f.modified_time),
            }
        }).collect();
        
        Ok(entries)
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_init().await?;
        let file = self.get_file(path).await?;
        let url = self.client.get_download_url(&file.id, self.config.disable_media_link).await?;
        
        let mut req = self.http_client.get(&url);
        
        if let Some(r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }
        
        let resp = req.send().await?;
        
        if !resp.status().is_success() && resp.status().as_u16() != 206 {
            return Err(anyhow!("Download failed / 下载失败: {} - {}", resp.status(), url));
        }
        
        let stream = resp.bytes_stream();
        let reader = StreamReader::new(stream);
        
        Ok(Box::new(reader))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        self.ensure_init().await?;
        let path = fix_path(path);
        let parent_path = get_parent_path(&path);
        let file_name = get_file_name(&path);
        
        let parent_id = self.get_folder_id(&parent_path).await?;
        
        // PikPak要求文件大小大于0，空文件使用特殊处理：上传1字节的占位内容
        let file_size = match size_hint {
            Some(0) | None => 1, // 空文件使用1字节
            Some(s) => s,
        };
        
        let body = json!({
            "kind": "drive#file",
            "name": file_name,
            "size": file_size,
            "hash": "",
            "upload_type": "UPLOAD_TYPE_RESUMABLE",
            "objProvider": {"provider": "UPLOAD_TYPE_UNKNOWN"},
            "parent_id": parent_id,
            "folder_type": "NORMAL",
        });
        
        let resp: UploadTaskResp = self.client.post(api::FILES_URL, body).await?;
        
        if resp.resumable.is_none() {
            return Ok(Box::new(NoOpWriter));
        }
        
        let params = resp.resumable.unwrap().params;
        // 对于空文件，创建特殊的writer
        let actual_size = size_hint.unwrap_or(0);
        let writer = if actual_size == 0 {
            PikPakStreamWriter::new_empty_file(params)
        } else {
            PikPakStreamWriter::new(params, size_hint, progress)
        };
        
        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let file = self.get_file(path).await?;
        
        let body = json!({ "ids": [file.id] });
        let _: Value = self.client.post(&format!("{}:batchTrash", api::FILES_URL), body).await?;
        
        let path = fix_path(path);
        let mut cache = self.path_cache.write().await;
        cache.remove(&path);
        
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let path = fix_path(path);
        let parent_path = get_parent_path(&path);
        let dir_name = get_file_name(&path);
        
        let parent_id = self.get_folder_id(&parent_path).await?;
        
        let body = json!({
            "kind": "drive#folder",
            "parent_id": parent_id,
            "name": dir_name,
        });
        
        let resp: PikPakFile = self.client.post(api::FILES_URL, body).await?;
        
        let mut cache = self.path_cache.write().await;
        cache.insert(path, resp.id);
        
        Ok(())
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        self.ensure_init().await?;
        let file = self.get_file(old_path).await?;
        
        let body = json!({ "name": new_name });
        let url = format!("{}/{}", api::FILES_URL, file.id);
        let _: Value = self.client.patch(&url, body).await?;
        
        let old_path = fix_path(old_path);
        let mut cache = self.path_cache.write().await;
        cache.remove(&old_path);
        
        Ok(())
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_init().await?;
        let file = self.get_file(old_path).await?;
        let new_parent_path = get_parent_path(new_path);
        let new_parent_id = self.get_folder_id(&new_parent_path).await?;
        
        let body = json!({
            "ids": [file.id],
            "to": { "parent_id": new_parent_id },
        });
        
        let _: Value = self.client.post(&format!("{}:batchMove", api::FILES_URL), body).await?;
        
        let old_path = fix_path(old_path);
        let mut cache = self.path_cache.write().await;
        cache.remove(&old_path);
        
        Ok(())
    }

    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_init().await?;
        let file = self.get_file(old_path).await?;
        let new_parent_path = get_parent_path(new_path);
        let new_parent_id = self.get_folder_id(&new_parent_path).await?;
        
        let body = json!({
            "ids": [file.id],
            "to": { "parent_id": new_parent_id },
        });
        
        let _: Value = self.client.post(&format!("{}:batchCopy", api::FILES_URL), body).await?;
        
        Ok(())
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        self.ensure_init().await?;
        let file = self.get_file(path).await?;
        let url = self.client.get_download_url(&file.id, self.config.disable_media_link).await?;
        Ok(Some(url))
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        if !self.config.show_space_info {
            return Ok(None);
        }
        self.ensure_init().await?;
        let resp: AboutResp = self.client.get(api::ABOUT_URL, None).await?;
        
        let total: u64 = resp.quota.limit.parse().unwrap_or(0);
        let used: u64 = resp.quota.usage.parse().unwrap_or(0);
        
        Ok(Some(SpaceInfo {
            total,
            used,
            free: total.saturating_sub(used),
        }))
    }

    fn show_space_in_frontend(&self) -> bool {
        self.config.show_space_info
    }

    fn get_updated_config(&self) -> Option<Value> {
        let changed = *self.config_changed.read().unwrap();
        
        if !changed {
            return None;
        }
        
        let token_info = self.client.get_token_info();
        
        let mut config = self.config.clone();
        config.refresh_token = token_info.refresh_token;
        config.captcha_token = token_info.captcha_token;
        config.device_id = token_info.device_id;
        
        serde_json::to_value(config).ok()
    }
}

/// Streaming reader from HTTP response / 从HTTP响应创建流式读取器
struct StreamReader {
    stream: Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: bytes::Bytes,
    offset: usize,
}

impl StreamReader {
    fn new<S>(stream: S) -> Self
    where
        S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            stream: Box::pin(stream),
            buffer: bytes::Bytes::new(),
            offset: 0,
        }
    }
}

impl AsyncRead for StreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.buffer.len() {
            let remaining = &self.buffer[self.offset..];
            let to_copy = std::cmp::min(remaining.len(), buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.offset += to_copy;
            return Poll::Ready(Ok(()));
        }

        use futures::StreamExt;
        match self.stream.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.buffer = chunk;
                self.offset = 0;
                let to_copy = std::cmp::min(self.buffer.len(), buf.remaining());
                buf.put_slice(&self.buffer[..to_copy]);
                self.offset = to_copy;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
            }
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// No-op writer for instant upload / 秒传时使用的空写入器
struct NoOpWriter;

impl AsyncWrite for NoOpWriter {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// PikPak driver factory / PikPak驱动工厂
pub struct PikPakDriverFactory;

impl DriverFactory for PikPakDriverFactory {
    fn driver_type(&self) -> &'static str {
        "PikPak"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "PikPak".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("username", "string")
                .title("Username / 用户名")
                .help("PikPak账号用户名(邮箱/手机号)"),
            ConfigItem::new("password", "password")
                .title("Password / 密码")
                .help("PikPak账号密码"),
            ConfigItem::new("refresh_token", "string")
                .title("Refresh Token")
                .help("使用refresh_token认证(如果提供了用户名密码则可选)"),
            ConfigItem::new("platform", "select")
                .title("平台")
                .default("web")
                .options("android,web,pc")
                .help("客户端平台类型"),
            ConfigItem::new("root_folder_id", "string")
                .title("根文件夹ID")
                .default("")
                .help("根文件夹ID，留空表示根目录"),
            ConfigItem::new("disable_media_link", "bool")
                .title("禁用媒体链接")
                .default("true")
                .help(" 禁用视频文件的转码媒体链接"),
            ConfigItem::new("show_space_info", "bool")
                .title("显示空间信息")
                .default("true")
                .help("在前端显示存储空间信息"),
            ConfigItem::new("captcha_token", "string")
                .title("验证码令牌")
                .help("验证码令牌(自动管理)"),
            ConfigItem::new("device_id", "string")
                .title("设备ID")
                .help("设备ID(留空自动生成)"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let cfg: PikPakConfig = serde_json::from_value(config)?;
        Ok(Box::new(PikPakDriver::new(cfg)))
    }
}
