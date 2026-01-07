//! 139云盘驱动实现 / 139Yun driver implementation
//!
//! 架构原则 / Architecture principles:
//! - 驱动只提供原语能力(Reader/Writer) / Driver only provides primitive capabilities
//! - Core控制进度、并发、断点 / Core controls progress, concurrency, resume points
//! - 永远不把文件放内存，使用流式传输 / Never load files into memory, use streaming
//! - 支持302重定向和本地代理 / Support 302 redirect and local proxy

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

use super::client::Yun139Client;
use super::types::*;
use super::util::*;
use super::writer::Yun139StreamWriter;

/// 139云盘驱动配置 / 139Yun driver config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Yun139Config {
    #[serde(default)]
    pub authorization: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub mail_cookies: String,
    #[serde(default)]
    pub root_folder_id: String,
    #[serde(default = "default_cloud_type")]
    pub cloud_type: String,
    #[serde(default)]
    pub cloud_id: String,
    #[serde(default)]
    pub user_domain_id: String,
    #[serde(default)]
    pub custom_upload_part_size: i64,
    #[serde(default = "default_true")]
    pub report_real_size: bool,
    #[serde(default)]
    pub use_large_thumbnail: bool,
    #[serde(default = "default_true")]
    pub show_space_info: bool,
}

fn default_cloud_type() -> String {
    "personal_new".to_string()
}

fn default_true() -> bool {
    true
}

/// 139云盘存储驱动 / 139Yun storage driver
pub struct Yun139Driver {
    config: Yun139Config,
    client: Arc<Yun139Client>,
    http_client: Client,
    /// 路径到ID的缓存 (path -> (id, internal_path))
    path_cache: Arc<RwLock<HashMap<String, (String, String)>>>,
    root_path: Arc<StdRwLock<String>>,
    config_changed: Arc<StdRwLock<bool>>,
    initialized: Arc<StdRwLock<bool>>,
}

impl Yun139Driver {
    /// 创建新驱动实例 / Create new driver instance
    pub fn new(config: Yun139Config) -> Self {
        let cloud_type = CloudType::from_str(&config.cloud_type);
        let client = Arc::new(Yun139Client::new(cloud_type, config.cloud_id.clone()));
        
        if !config.authorization.is_empty() {
            client.init_token(&config.authorization);
        }
        
        if !config.user_domain_id.is_empty() {
            client.set_user_domain_id(&config.user_domain_id);
        }

        Self {
            config,
            client,
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::limited(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            root_path: Arc::new(StdRwLock::new(String::new())),
            config_changed: Arc::new(StdRwLock::new(false)),
            initialized: Arc::new(StdRwLock::new(false)),
        }
    }

    /// 确保已初始化 / Ensure initialized
    async fn ensure_init(&self) -> Result<()> {
        {
            let initialized = self.initialized.read().unwrap();
            if *initialized {
                return Ok(());
            }
        }
        self.init().await?;
        *self.initialized.write().unwrap() = true;
        Ok(())
    }

    /// 初始化 / Initialize
    async fn init(&self) -> Result<()> {
        if let Err(e) = self.client.refresh_token().await {
            tracing::warn!("[139] 刷新令牌失败: {}", e);
        }
        
        let resp = self.client.query_route_policy().await?;
        for policy in &resp.data.route_policy_list {
            if policy.mod_name == "personal" && !policy.https_url.is_empty() {
                self.client.set_personal_cloud_host(&policy.https_url);
                break;
            }
        }
        
        let cloud_type = CloudType::from_str(&self.config.cloud_type);
        let root_id = if self.config.root_folder_id.is_empty() {
            match cloud_type {
                CloudType::PersonalNew => "/".to_string(),
                CloudType::Personal => "root".to_string(),
                CloudType::Group => self.config.cloud_id.clone(),
                CloudType::Family => {
                    if let Ok((_, _, path)) = self.client.family_get_files("").await {
                        path
                    } else {
                        String::new()
                    }
                }
            }
        } else {
            self.config.root_folder_id.clone()
        };
        
        *self.root_path.write().unwrap() = root_id;
        *self.config_changed.write().unwrap() = true;
        
        Ok(())
    }

    fn cloud_type(&self) -> CloudType {
        CloudType::from_str(&self.config.cloud_type)
    }

    fn get_root_id(&self) -> String {
        let root_path = self.root_path.read().unwrap();
        if root_path.is_empty() {
            if self.config.root_folder_id.is_empty() {
                match self.cloud_type() {
                    CloudType::PersonalNew => "/".to_string(),
                    CloudType::Personal => "root".to_string(),
                    _ => self.config.cloud_id.clone(),
                }
            } else {
                self.config.root_folder_id.clone()
            }
        } else {
            root_path.clone()
        }
    }

    /// 通过路径获取ID / Get ID by path
    async fn get_id_by_path(&self, path: &str) -> Result<(String, String)> {
        let path = fix_path(path);
        
        if path == "/" {
            return Ok((self.get_root_id(), String::new()));
        }
        
        {
            let cache = self.path_cache.read().await;
            if let Some((id, internal_path)) = cache.get(&path) {
                return Ok((id.clone(), internal_path.clone()));
            }
        }
        
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        let mut current_id = self.get_root_id();
        let mut current_internal_path = String::new();
        
        for (i, part) in parts.iter().enumerate() {
            let items = self.list_items(&current_id, &current_internal_path).await?;
            
            let mut found = false;
            for item in items {
                if item.0 == *part {
                    current_id = item.1.clone();
                    current_internal_path = item.2.clone();
                    found = true;
                    
                    let full_path = format!("/{}", parts[..=i].join("/"));
                    let mut cache = self.path_cache.write().await;
                    cache.insert(full_path, (current_id.clone(), current_internal_path.clone()));
                    break;
                }
            }
            
            if !found {
                return Err(anyhow!("路径不存在: {}", path));
            }
        }
        
        Ok((current_id, current_internal_path))
    }

    /// 列出项目 (返回 name, id, internal_path, is_dir, size, modified)
    async fn list_items(&self, id: &str, _parent_path: &str) -> Result<Vec<(String, String, String, bool, u64, String)>> {
        let mut items = Vec::new();
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                let files = self.client.personal_get_files(id).await?;
                for file in files {
                    let is_dir = file.file_type == "folder";
                    items.push((
                        file.name,
                        file.file_id,
                        String::new(),
                        is_dir,
                        if is_dir { 0 } else { file.size as u64 },
                        file.updated_at,
                    ));
                }
            }
            CloudType::Personal => {
                let files = self.client.get_files(id).await?;
                for (content, catalog) in files {
                    if !catalog.catalog_id.is_empty() {
                        items.push((
                            catalog.catalog_name,
                            catalog.catalog_id,
                            String::new(),
                            true,
                            0,
                            catalog.update_time,
                        ));
                    } else {
                        items.push((
                            content.content_name,
                            content.content_id,
                            String::new(),
                            false,
                            content.content_size as u64,
                            content.update_time,
                        ));
                    }
                }
            }
            CloudType::Family => {
                let (contents, catalogs, path) = self.client.family_get_files(id).await?;
                
                if id == self.get_root_id() {
                    *self.root_path.write().unwrap() = path.clone();
                }
                
                for catalog in catalogs {
                    items.push((
                        catalog.catalog_name,
                        catalog.catalog_id,
                        path.clone(),
                        true,
                        0,
                        catalog.last_update_time,
                    ));
                }
                
                for content in contents {
                    items.push((
                        content.content_name,
                        content.content_id,
                        path.clone(),
                        false,
                        content.content_size as u64,
                        content.last_update_time,
                    ));
                }
            }
            CloudType::Group => {
                let root_id = self.get_root_id();
                let (contents, catalogs, path) = self.client.group_get_files(id, &root_id).await?;
                
                if id == root_id {
                    *self.root_path.write().unwrap() = path.clone();
                }
                
                for catalog in catalogs {
                    items.push((
                        catalog.catalog.catalog_name,
                        catalog.catalog.catalog_id,
                        catalog.path,
                        true,
                        0,
                        catalog.catalog.update_time,
                    ));
                }
                
                for content in contents {
                    items.push((
                        content.content_name,
                        content.content_id,
                        path.clone(),
                        false,
                        content.content_size as u64,
                        content.update_time,
                    ));
                }
            }
        }
        
        Ok(items)
    }

    /// 个人版新API上传 / Personal new API upload
    async fn upload_personal_new(
        &self,
        parent_id: &str,
        file_name: &str,
        data: bytes::Bytes,
        size: i64,
        progress: Option<ProgressCallback>,
    ) -> Result<()> {
        use sha2::{Sha256, Digest};
        
        // 计算SHA256哈希
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let hash = format!("{:x}", hasher.finalize()).to_uppercase();
        
        // 计算分片大小
        let part_size = get_part_size(size, self.config.custom_upload_part_size);
        let part_count = if size > part_size { (size + part_size - 1) / part_size } else { 1 };
        
        // 生成分片信息
        let mut part_infos = Vec::new();
        for i in 0..part_count.min(100) {
            let start = i * part_size;
            let byte_size = (size - start).min(part_size);
            part_infos.push(PartInfo {
                part_number: i + 1,
                part_size: byte_size,
                parallel_hash_ctx: ParallelHashCtx { part_offset: start },
            });
        }
        
        // 创建上传任务
        let resp = self.client.personal_create_upload(
            parent_id,
            file_name,
            size,
            &hash,
            part_infos,
        ).await?;
        
        // 秒传成功
        if resp.data.exist || resp.data.rapid_upload {
            if let Some(ref p) = progress {
                p(size as u64, size as u64);
            }
            return Ok(());
        }
        
        // 上传分片
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        
        let mut uploaded = 0u64;
        for part_info in &resp.data.part_infos {
            let part_num = part_info.part_number as i64;
            let part_idx = (part_num - 1) as usize;
            let part_start = (part_idx as i64 * part_size) as usize;
            let part_end = ((part_idx as i64 + 1) * part_size).min(size) as usize;
            
            if part_end <= data.len() {
                let chunk = &data[part_start..part_end];
                
                let resp = http_client
                    .put(&part_info.upload_url)
                    .header("Content-Type", "application/octet-stream")
                    .header("Content-Length", chunk.len().to_string())
                    .body(chunk.to_vec())
                    .send()
                    .await?;
                
                if !resp.status().is_success() {
                    return Err(anyhow!("上传分片 {} 失败: {}", part_num, resp.status()));
                }
                
                uploaded += chunk.len() as u64;
                if let Some(ref p) = progress {
                    p(uploaded, size as u64);
                }
            }
        }
        
        // 完成上传
        self.client.personal_complete_upload(
            &resp.data.file_id,
            &resp.data.upload_id,
            &hash,
        ).await?;
        
        Ok(())
    }

    /// 旧版API上传 / Legacy API upload
    async fn upload_legacy(
        &self,
        parent_id: &str,
        parent_internal_path: &str,
        file_name: &str,
        data: bytes::Bytes,
        size: i64,
        progress: Option<ProgressCallback>,
    ) -> Result<()> {
        let upload_path = if parent_internal_path.is_empty() {
            join_path(&self.root_path.read().unwrap(), parent_id)
        } else {
            join_path(parent_internal_path, parent_id)
        };
        
        let report_size = if self.config.report_real_size { size } else { 0 };
        let resp = self.client.get_upload_url(
            parent_id,
            file_name,
            report_size,
            Some(&upload_path),
        ).await?;
        
        if resp.data.result.result_code != "0" {
            return Err(anyhow!("获取上传URL失败: {:?}", resp.data.result.result_desc));
        }
        
        let upload_url = &resp.data.upload_result.redirection_url;
        let upload_task_id = &resp.data.upload_result.upload_task_id;
        
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        
        let part_size = get_part_size(size, self.config.custom_upload_part_size);
        let part_count = if size > part_size { (size + part_size - 1) / part_size } else { 1 };
        
        let mut uploaded = 0u64;
        for i in 0..part_count {
            let start = i * part_size;
            let end = ((i + 1) * part_size).min(size);
            let chunk_size = (end - start) as usize;
            
            if (start as usize) + chunk_size <= data.len() {
                let chunk = &data[start as usize..(start as usize) + chunk_size];
                
                let resp = http_client
                    .post(upload_url)
                    .header("Content-Type", format!("text/plain;name={}", unicode_escape(file_name)))
                    .header("contentSize", size.to_string())
                    .header("range", format!("bytes={}-{}", start, end - 1))
                    .header("uploadtaskID", upload_task_id)
                    .header("rangeType", "0")
                    .body(chunk.to_vec())
                    .send()
                    .await?;
                
                if !resp.status().is_success() {
                    return Err(anyhow!("上传分片失败: {}", resp.status()));
                }
                
                uploaded += chunk.len() as u64;
                if let Some(ref p) = progress {
                    p(uploaded, size as u64);
                }
            }
        }
        
        Ok(())
    }
}

/// 流式读取器 / Streaming reader
struct StreamReader {
    stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    current_chunk: Option<bytes::Bytes>,
    offset: usize,
}

impl AsyncRead for StreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            let offset = self.offset;
            if let Some(ref chunk) = self.current_chunk {
                let remaining = &chunk[offset..];
                if !remaining.is_empty() {
                    let to_copy = remaining.len().min(buf.remaining());
                    buf.put_slice(&remaining[..to_copy]);
                    self.offset += to_copy;
                    return Poll::Ready(Ok(()));
                }
            }
            
            if self.current_chunk.is_some() {
                self.current_chunk = None;
                self.offset = 0;
            }

            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.current_chunk = Some(chunk);
                    self.offset = 0;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )));
                }
                Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

/// 空操作写入器 / No-op writer
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

#[async_trait]
impl StorageDriver for Yun139Driver {
    fn name(&self) -> &str {
        "139云盘"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false, // 天翼云支持流式写入
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_init().await?;
        let (id, internal_path) = self.get_id_by_path(path).await?;
        let items = self.list_items(&id, &internal_path).await?;
        
        let base_path = fix_path(path);
        let mut entries = Vec::new();
        
        for (name, _id, _internal_path, is_dir, size, modified) in items {
            let entry_path = if base_path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", base_path, name)
            };
            
            entries.push(Entry {
                name,
                path: entry_path,
                is_dir,
                size,
                modified: Some(modified),
            });
        }
        
        Ok(entries)
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_init().await?;
        let (id, internal_path) = self.get_id_by_path(path).await?;
        
        let url = match self.cloud_type() {
            CloudType::PersonalNew => self.client.personal_get_link(&id).await?,
            CloudType::Personal => self.client.get_link(&id).await?,
            CloudType::Family => self.client.family_get_link(&id, &internal_path).await?,
            CloudType::Group => self.client.group_get_link(&id, &internal_path).await?,
        };
        
        let mut req = self.http_client.get(&url);
        
        if let Some(r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end.saturating_sub(1)));
        }
        
        let resp = req.send().await?;
        
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("下载失败: {}", resp.status()));
        }
        
        let stream = resp.bytes_stream();
        let reader = StreamReader {
            stream: Box::pin(stream),
            current_chunk: None,
            offset: 0,
        };
        
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
        
        let (parent_id, parent_internal_path) = self.get_id_by_path(&parent_path).await?;
        
        let size = size_hint.unwrap_or(0) as i64;
        
        // 139云盘不支持空文件，上传1字节占位符
        let actual_size = if size == 0 { 1 } else { size };
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                let writer = Yun139StreamWriter::new_personal(
                    self.client.clone(),
                    parent_id,
                    file_name,
                    size_hint,
                    self.config.custom_upload_part_size,
                    progress,
                );
                Ok(Box::new(writer))
            }
            _ => {
                let upload_path = if parent_internal_path.is_empty() {
                    join_path(&self.root_path.read().unwrap(), &parent_id)
                } else {
                    join_path(&parent_internal_path, &parent_id)
                };
                
                let report_size = if self.config.report_real_size { size } else { 0 };
                let resp = self.client.get_upload_url(
                    &parent_id,
                    &file_name,
                    report_size,
                    Some(&upload_path),
                ).await?;
                
                if resp.data.result.result_code != "0" {
                    return Err(anyhow!("获取上传URL失败: {:?}", resp.data.result.result_desc));
                }
                
                let writer = Yun139StreamWriter::new_legacy(
                    resp.data.upload_result.redirection_url,
                    resp.data.upload_result.upload_task_id,
                    file_name,
                    size,
                    self.config.custom_upload_part_size,
                    progress,
                );
                Ok(Box::new(writer))
            }
        }
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let (id, internal_path) = self.get_id_by_path(path).await?;
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                self.client.personal_delete(vec![id]).await?;
            }
            _ => {
                let entries = self.list(path).await?;
                let is_dir = entries.is_empty() || self.list(&fix_path(path)).await.is_ok();
                let content_ids = if is_dir { vec![] } else { vec![id.clone()] };
                let catalog_ids = if is_dir { vec![id] } else { vec![] };
                self.client.delete(content_ids, catalog_ids, &internal_path).await?;
            }
        }
        
        let mut cache = self.path_cache.write().await;
        cache.remove(path);
        
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let path = fix_path(path);
        let parent_path = get_parent_path(&path);
        let dir_name = get_file_name(&path);
        
        let (parent_id, _) = self.get_id_by_path(&parent_path).await?;
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                self.client.personal_create_folder(&parent_id, &dir_name).await?;
            }
            _ => {
                self.client.create_folder(&parent_id, &dir_name).await?;
            }
        }
        
        Ok(())
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        self.ensure_init().await?;
        let (id, _) = self.get_id_by_path(old_path).await?;
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                self.client.personal_rename(&id, new_name).await?;
            }
            _ => {
                return Err(anyhow!("此云盘类型暂不支持重命名"));
            }
        }
        
        let mut cache = self.path_cache.write().await;
        cache.remove(old_path);
        
        Ok(())
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_init().await?;
        let (id, _) = self.get_id_by_path(old_path).await?;
        let new_parent_path = get_parent_path(new_path);
        let (new_parent_id, _) = self.get_id_by_path(&new_parent_path).await?;
        
        match self.cloud_type() {
            CloudType::PersonalNew => {
                self.client.personal_move(vec![id], &new_parent_id).await?;
            }
            _ => {
                return Err(anyhow!("此云盘类型暂不支持移动"));
            }
        }
        
        let mut cache = self.path_cache.write().await;
        cache.remove(old_path);
        
        Ok(())
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        self.ensure_init().await?;
        let (id, internal_path) = self.get_id_by_path(path).await?;
        
        let url = match self.cloud_type() {
            CloudType::PersonalNew => self.client.personal_get_link(&id).await?,
            CloudType::Personal => self.client.get_link(&id).await?,
            CloudType::Family => self.client.family_get_link(&id, &internal_path).await?,
            CloudType::Group => self.client.group_get_link(&id, &internal_path).await?,
        };
        
        Ok(Some(url))
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        self.ensure_init().await?;
        
        let user_domain_id = self.client.get_token_info().user_domain_id;
        if user_domain_id.is_empty() {
            return Ok(None);
        }
        
        let (total, used) = self.client.get_disk_info().await?;
        
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
        config.authorization = token_info.authorization;
        config.user_domain_id = token_info.user_domain_id;
        
        let root_path = self.root_path.read().unwrap();
        if !root_path.is_empty() {
            config.root_folder_id = root_path.clone();
        }
        
        serde_json::to_value(config).ok()
    }
}

/// 139云盘驱动工厂 / 139Yun driver factory
pub struct Yun139DriverFactory;

impl DriverFactory for Yun139DriverFactory {
    fn driver_type(&self) -> &'static str {
        "139yun"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "中国移动云盘(139云)".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("authorization", "text")
                .title("Authorization")
                .required()
                .help("Base64编码的认证信息"),
            ConfigItem::new("username", "string")
                .title("用户名")
                .help("手机号"),
            ConfigItem::new("password", "string")
                .title("密码")
                .help("登录密码"),
            ConfigItem::new("mail_cookies", "text")
                .title("邮箱Cookies")
                .help("mail.139.com的Cookies"),
            ConfigItem::new("cloud_type", "select")
                .title("云盘类型")
                .default("personal_new")
                .options("personal_new,personal,family,group")
                .help("选择云盘类型"),
            ConfigItem::new("cloud_id", "string")
                .title("云ID")
                .help("家庭云/群组云ID"),
            ConfigItem::new("root_folder_id", "string")
                .title("根目录ID")
                .help("根目录ID，留空使用默认"),
            ConfigItem::new("user_domain_id", "string")
                .title("用户域ID")
                .help("ud_id，用于显示容量"),
            ConfigItem::new("custom_upload_part_size", "number")
                .title("自定义分片大小")
                .default("0")
                .help("0表示自动"),
            ConfigItem::new("show_space_info", "bool")
                .title("显示容量信息")
                .default("true")
                .help("在前端显示容量"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: Yun139Config = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(Yun139Driver::new(config)))
    }
}
