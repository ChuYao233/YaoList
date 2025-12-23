//! 123云盘开放平台驱动实现 
//! 123 Cloud Open Platform driver implementation 

use async_trait::async_trait;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::RwLock;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use chrono::{FixedOffset, NaiveDateTime, TimeZone};
use serde_json::Value;
use futures::StreamExt;
use bytes::Bytes;

use crate::storage::{StorageDriver, Entry, SpaceInfo, Capability, DriverFactory, DriverConfig, ConfigItem};
use super::api::ApiClient;
use super::types::*;
use super::upload::Pan123Writer;

/// 123云盘开放平台驱动 / 123 Cloud Open Platform driver
pub struct Pan123OpenDriver {
    /// API客户端 / API client
    client: Arc<ApiClient>,
    /// 驱动配置（与ApiClient共享）/ Driver config (shared with ApiClient)
    config: Arc<tokio::sync::Mutex<Pan123OpenConfig>>,
    /// 路径到文件ID缓存 / Path to file ID cache
    path_cache: Arc<RwLock<HashMap<String, i64>>>,
    /// 文件ID到文件信息缓存 / File ID to file info cache
    file_cache: Arc<RwLock<HashMap<i64, FileInfo>>>,
}

impl Pan123OpenDriver {
    /// 创建驱动实例 / Create driver instance
    pub fn new(config: Pan123OpenConfig) -> Self {
        // 共享 config，这样 ApiClient 刷新 token 后，driver 也能获取到
        // Shared config, so when ApiClient refreshes token, driver can also get it
        let shared_config = Arc::new(tokio::sync::Mutex::new(config));
        let client = Arc::new(ApiClient::new(shared_config.clone()));
        Self {
            client,
            config: shared_config,
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            file_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 根据路径获取文件ID / Get file ID by path
    async fn get_file_id(&self, path: &str) -> Result<i64> {
        // 根目录 / Root directory
        if path.is_empty() || path == "/" {
            return Ok(0);
        }

        let normalized = path.trim_matches('/').to_string();

        // 检查缓存 / Check cache
        {
            let cache = self.path_cache.read().await;
            if let Some(&fid) = cache.get(&normalized) {
                return Ok(fid);
            }
        }

        // 逐级查找 / Find level by level
        let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_fid: i64 = 0;
        let mut current_path = String::new();

        for part in parts {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);

            // 检查缓存 / Check cache
            {
                let cache = self.path_cache.read().await;
                if let Some(&fid) = cache.get(&current_path) {
                    current_fid = fid;
                    continue;
                }
            }

            // 列出当前目录查找 / List current directory to find
            let mut last_file_id = 0i64;
            let mut found = false;

            loop {
                let resp = self.client.get_files(current_fid, 100, last_file_id).await
                    .map_err(|e| anyhow!("{}", e))?;

                let data = match resp.data {
                    Some(d) => d,
                    None => break,
                };

                for file in &data.file_list {
                    if file.trashed == 0 && file.file_name == part {
                        current_fid = file.file_id;
                        // 更新缓存 / Update cache
                        self.path_cache.write().await.insert(current_path.clone(), current_fid);
                        self.file_cache.write().await.insert(file.file_id, file.clone());
                        found = true;
                        break;
                    }
                }

                if found || data.last_file_id == -1 {
                    break;
                }
                last_file_id = data.last_file_id;
            }

            if !found {
                return Err(anyhow!("Path not found: /{}", current_path));
            }
        }

        Ok(current_fid)
    }

    /// 获取缓存的文件信息 / Get cached file info
    async fn get_cached_file(&self, file_id: i64) -> Option<FileInfo> {
        self.file_cache.read().await.get(&file_id).cloned()
    }

    /// 转换文件信息为Entry / Convert FileInfo to Entry
    fn file_to_entry(file: &FileInfo, parent_path: &str) -> Entry {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let modified = NaiveDateTime::parse_from_str(&file.update_at, "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(|dt| tz.from_local_datetime(&dt).single())
            .map(|dt| dt.to_rfc3339());

        let path = if parent_path.is_empty() || parent_path == "/" {
            format!("/{}", file.file_name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.file_name)
        };

        Entry {
            name: file.file_name.clone(),
            path,
            is_dir: file.is_dir(),
            size: file.size as u64,
            modified,
        }
    }
}

#[async_trait]
impl StorageDriver for Pan123OpenDriver {
    fn name(&self) -> &str {
        "123云盘开放平台"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,
            max_chunk_size: Some(16 * 1024 * 1024),
            can_concurrent_upload: true,
            requires_oauth: true,
            can_multipart_upload: true,
            can_server_side_copy: true, // 支持秒传复制 / Supports instant copy
            can_batch_operations: true,
            max_file_size: None,
        }
    }

    /// 列出目录  / List directory 
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let parent_file_id = self.get_file_id(path).await?;

        let mut all_files = Vec::new();
        let mut last_file_id = 0i64;

        loop {
            let resp = self.client.get_files(parent_file_id, 100, last_file_id).await
                .map_err(|e| anyhow!("{}", e))?;

            let data = match resp.data {
                Some(d) => d,
                None => break,
            };

            // 过滤回收站文件 / Filter trashed files
            for file in &data.file_list {
                if file.trashed == 0 {
                    all_files.push(Self::file_to_entry(file, path));
                    // 更新缓存 / Update cache
                    let file_path = if path.is_empty() || path == "/" {
                        file.file_name.clone()
                    } else {
                        format!("{}/{}", path.trim_matches('/'), file.file_name)
                    };
                    self.path_cache.write().await.insert(file_path, file.file_id);
                    self.file_cache.write().await.insert(file.file_id, file.clone());
                }
            }

            if data.last_file_id == -1 {
                break;
            }
            last_file_id = data.last_file_id;
        }

        Ok(all_files)
    }

    /// 打开读取器 / Open reader 
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let file_id = self.get_file_id(path).await?;
        let config = self.config.lock().await;

        // 获取下载链接 / Get download URL
        let url = if config.direct_link {
            let resp = self.client.get_direct_link(file_id).await
                .map_err(|e| anyhow!("{}", e))?;
            let data = resp.data.ok_or_else(|| anyhow!("No direct link data"))?;

            if config.direct_link_private_key.is_empty() {
                data.url
            } else {
                let uid = self.client.get_uid().await.map_err(|e| anyhow!("{}", e))?;
                let duration = std::time::Duration::from_secs(config.direct_link_valid_duration as u64 * 60);
                self.client.sign_url(&data.url, &config.direct_link_private_key, uid, duration)
                    .map_err(|e| anyhow!("{}", e))?
            }
        } else {
            let resp = self.client.get_download_info(file_id).await
                .map_err(|e| anyhow!("{}", e))?;
            resp.data.ok_or_else(|| anyhow!("No download info"))?.download_url
        };
        drop(config);

        // 创建HTTP请求 / Create HTTP request
        let client = reqwest::Client::new();
        let mut req = client.get(&url);

        if let Some(r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end.saturating_sub(1)));
        }

        let resp = req.send().await?;
        let stream = resp.bytes_stream();

        Ok(Box::new(StreamReader::new(stream)))
    }

    /// 打开写入器 (流式分片上传) / Open writer (streaming chunked upload)
    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let size = size_hint.ok_or_else(|| anyhow!("File size is required for 123 cloud upload"))?;

        // 解析父目录和文件名 / Parse parent directory and filename
        let (parent_path, filename) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };

        let parent_file_id = self.get_file_id(parent_path).await?;

        let upload_thread = self.config.lock().await.upload_thread.max(1).min(32) as usize;
        let writer = Pan123Writer::new(
            self.client.clone(),
            parent_file_id,
            filename,
            size as i64,
            upload_thread,
            progress,
        ).map_err(|e| anyhow!("Failed to create upload writer: {}", e))?;

        Ok(Box::new(writer))
    }

    /// Put完整文件 - 123云盘需要完整文件MD5，自己处理分片上传
    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<()> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use reqwest::multipart::{Form, Part};
        
        let size = data.len() as i64;
        
        // 解析父目录和文件名
        let (parent_path, filename) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };
        
        let parent_file_id = self.get_file_id(parent_path).await?;
        let upload_thread = self.config.lock().await.upload_thread.max(1).min(32) as usize;
        
        // 计算整体MD5
        let etag = format!("{:x}", md5::compute(&data));
        
        // 1. 创建上传任务
        let create_resp = self.client.create_upload(
            parent_file_id,
            filename,
            &etag,
            size,
            2,
            false,
        ).await.map_err(|e| anyhow!("{}", e))?;
        
        let upload_data = create_resp.data.ok_or_else(|| anyhow!("No upload create data"))?;
        
        // 2. 检查秒传
        if upload_data.reuse {
            tracing::info!("123云盘秒传成功: {}", filename);
            if let Some(ref cb) = progress {
                cb(size as u64, size as u64);
            }
            return Ok(());
        }
        
        if upload_data.servers.is_empty() {
            return Err(anyhow!("No upload servers available"));
        }
        
        let upload_domain = upload_data.servers[0].clone();
        let preupload_id = upload_data.preupload_id.clone();
        let slice_size = upload_data.slice_size as usize;
        let access_token = self.client.get_config().await.access_token;
        
        let file_size = data.len();
        let total_slices = (file_size + slice_size - 1) / slice_size;
        
        // 进度跟踪
        let completed_slices = Arc::new(AtomicU64::new(0));
        let error_flag = Arc::new(AtomicBool::new(false));
        let data = Arc::new(data);
        
        // 3. 并发分片上传
        let semaphore = Arc::new(tokio::sync::Semaphore::new(upload_thread));
        let mut handles = Vec::new();
        
        for slice_no in 1..=total_slices {
            if error_flag.load(Ordering::SeqCst) {
                break;
            }
            
            let permit = semaphore.clone().acquire_owned().await
                .map_err(|e| anyhow!("Failed to acquire semaphore: {}", e))?;
            
            let data = data.clone();
            let upload_domain = upload_domain.clone();
            let preupload_id = preupload_id.clone();
            let access_token = access_token.clone();
            let filename = filename.to_string();
            let completed_slices = completed_slices.clone();
            let error_flag = error_flag.clone();
            let progress = progress.clone();
            let total_size = file_size as u64;
            
            let handle = tokio::spawn(async move {
                let _permit = permit;
                
                // 从内存中取分片
                let offset = (slice_no - 1) * slice_size;
                let end = std::cmp::min(offset + slice_size, file_size);
                let slice_data = data[offset..end].to_vec();
                let slice_md5 = format!("{:x}", md5::compute(&slice_data));
                
                // 3次重试
                let mut last_error = String::new();
                
                for attempt in 0..3 {
                    if error_flag.load(Ordering::SeqCst) {
                        return Err(anyhow!("Cancelled due to other slice failure"));
                    }
                    
                    let part = Part::bytes(slice_data.clone())
                        .file_name(format!("{}.part{}", filename, slice_no))
                        .mime_str("application/octet-stream")
                        .map_err(|e| anyhow!("Failed to create part: {}", e))?;
                    
                    let form = Form::new()
                        .text("preuploadID", preupload_id.clone())
                        .text("sliceNo", slice_no.to_string())
                        .text("sliceMD5", slice_md5.clone())
                        .part("slice", part);
                    
                    let upload_url = format!("{}/upload/v2/file/slice", upload_domain);
                    let http_client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(300))
                        .build()
                        .map_err(|e| anyhow!("Failed to create client: {}", e))?;
                    
                    match http_client
                        .post(&upload_url)
                        .header("Authorization", format!("Bearer {}", access_token))
                        .header("Platform", "open_platform")
                        .multipart(form)
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            let status = resp.status();
                            if status.as_u16() == 200 {
                                let resp_text = resp.text().await
                                    .map_err(|e| anyhow!("Read response failed: {}", e))?;
                                let resp_body: super::types::SliceUploadResponse = serde_json::from_str(&resp_text)
                                    .map_err(|e| anyhow!("Parse response failed: {} - {}", e, resp_text))?;
                                if resp_body.base.code == 0 {
                                    let completed = completed_slices.fetch_add(1, Ordering::SeqCst) + 1;
                                    let progress_bytes = ((completed as f64 / total_slices as f64) * total_size as f64) as u64;
                                    tracing::debug!("123云盘分片 {}/{} 上传成功, 进度 {}/{}", slice_no, total_slices, progress_bytes, total_size);
                                    if let Some(ref cb) = progress {
                                        cb(progress_bytes, total_size);
                                    }
                                    return Ok(());
                                }
                                last_error = format!("slice {} failed: {}", slice_no, resp_body.base.message);
                            } else {
                                let resp_text = resp.text().await.unwrap_or_default();
                                last_error = format!("slice {} failed, status: {} - {}", slice_no, status, resp_text);
                            }
                        }
                        Err(e) => {
                            last_error = format!("Request failed: {}", e);
                        }
                    }
                    
                    if attempt < 2 {
                        let delay = std::time::Duration::from_secs(1 << attempt);
                        tracing::warn!("123云盘分片 {} 失败，{}秒后重试: {}", slice_no, delay.as_secs(), last_error);
                        tokio::time::sleep(delay).await;
                    }
                }
                
                error_flag.store(true, Ordering::SeqCst);
                Err(anyhow!("Slice {} failed after 3 retries: {}", slice_no, last_error))
            });
            
            handles.push(handle);
        }
        
        // 等待所有任务完成
        let mut first_error: Option<anyhow::Error> = None;
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(anyhow!("Task panicked: {}", e));
                    }
                }
            }
        }
        
        if let Some(err) = first_error {
            return Err(err);
        }
        
        // 4. 完成上传
        for i in 0..60 {
            match self.client.upload_complete(&preupload_id).await {
                Ok(resp) => {
                    if let Some(complete_data) = resp.data {
                        if complete_data.completed && complete_data.file_id != 0 {
                            tracing::info!("123云盘上传完成: {} (file_id: {})", filename, complete_data.file_id);
                            if let Some(ref cb) = progress {
                                cb(size as u64, size as u64);
                            }
                            // 更新缓存
                            let file_path = if parent_path.is_empty() {
                                filename.to_string()
                            } else {
                                format!("{}/{}", parent_path.trim_matches('/'), filename)
                            };
                            self.path_cache.write().await.insert(file_path, complete_data.file_id);
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    if !e.contains("20103") && i >= 10 {
                        return Err(anyhow!("{}", e));
                    }
                    tracing::debug!("123云盘校验中 ({}/60): {}", i + 1, e);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        
        Err(anyhow!("Upload complete timeout after 60 seconds"))
    }
    
    /// 删除 / Delete 
    async fn delete(&self, path: &str) -> Result<()> {
        let file_id = self.get_file_id(path).await?;
        self.client.trash(file_id).await.map_err(|e| anyhow!("{}", e))?;

        // 清理缓存 / Clear cache
        let normalized = path.trim_matches('/').to_string();
        self.path_cache.write().await.remove(&normalized);
        self.file_cache.write().await.remove(&file_id);

        Ok(())
    }

    /// 创建目录 / Create directory 
    async fn create_dir(&self, path: &str) -> Result<()> {
        let parts: Vec<&str> = path.trim_matches('/').rsplitn(2, '/').collect();
        let (name, parent_path) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (parts[0], "")
        };

        let parent_file_id = self.get_file_id(parent_path).await?;
        self.client.mkdir(parent_file_id, name).await.map_err(|e| anyhow!("{}", e))
    }

    /// 重命名 / Rename 
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let file_id = self.get_file_id(old_path).await?;
        self.client.rename(file_id, new_name).await.map_err(|e| anyhow!("{}", e))?;

        // 清理缓存 / Clear cache
        let normalized = old_path.trim_matches('/').to_string();
        self.path_cache.write().await.remove(&normalized);

        Ok(())
    }

    /// 移动 / Move 
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let file_id = self.get_file_id(old_path).await?;

        // 解析目标父目录 / Parse target parent directory
        let new_parent = if let Some(pos) = new_path.rfind('/') {
            &new_path[..pos]
        } else {
            ""
        };
        let to_parent_file_id = self.get_file_id(new_parent).await?;

        self.client.move_file(file_id, to_parent_file_id).await.map_err(|e| anyhow!("{}", e))?;

        // 清理缓存 / Clear cache
        let normalized = old_path.trim_matches('/').to_string();
        self.path_cache.write().await.remove(&normalized);

        Ok(())
    }

    /// 复制  / Copy 
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let file_id = self.get_file_id(old_path).await?;
        let file_info = self.get_cached_file(file_id).await
            .ok_or_else(|| anyhow!("File info not cached, please list directory first"))?;

        // 解析目标父目录 / Parse target parent directory
        let new_parent = if let Some(pos) = new_path.rfind('/') {
            &new_path[..pos]
        } else {
            ""
        };
        let to_parent_file_id = self.get_file_id(new_parent).await?;

        // 尝试秒传复制 / Try instant copy
        let create_resp = self.client.create_upload(
            to_parent_file_id,
            &file_info.file_name,
            &file_info.etag,
            file_info.size,
            2, // 覆盖 / Overwrite
            false,
        ).await.map_err(|e| anyhow!("{}", e))?;

        let data = create_resp.data.ok_or_else(|| anyhow!("No create response data"))?;
        if data.reuse {
            return Ok(());
        }

        Err(anyhow!("Copy not supported (instant upload failed), use download + upload instead"))
    }

    /// 获取直链 / Get direct link
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        let file_id = self.get_file_id(path).await?;
        let config = self.config.lock().await;

        if config.direct_link {
            let resp = self.client.get_direct_link(file_id).await
                .map_err(|e| anyhow!("{}", e))?;
            Ok(resp.data.map(|d| d.url))
        } else {
            let resp = self.client.get_download_info(file_id).await
                .map_err(|e| anyhow!("{}", e))?;
            Ok(resp.data.map(|d| d.download_url))
        }
    }

    /// 获取空间信息  / Get space info 
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        match self.client.get_user_info().await {
            Ok(resp) => {
                if let Some(data) = resp.data {
                    let total = data.space_permanent + data.space_temp;
                    let used = data.space_used;
                    let free = total.saturating_sub(used);
                    Ok(Some(SpaceInfo { used, total, free }))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get 123 cloud space info: {}", e);
                Ok(None)
            }
        }
    }

    fn show_space_in_frontend(&self) -> bool {
        true
    }
    
    /// 获取更新后的配置（包含刷新后的token）/ Get updated config (with refreshed token)
    fn get_updated_config(&self) -> Option<serde_json::Value> {
        // 使用 try_lock 同步读取共享的 config
        // Use try_lock to synchronously read shared config
        match self.config.try_lock() {
            Ok(config) => serde_json::to_value(config.clone()).ok(),
            Err(_) => None,
        }
    }
}

impl std::fmt::Debug for Pan123OpenDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pan123OpenDriver").finish()
    }
}

// ============ 流式读取器 / Stream Reader ============

use futures::Stream;

/// 将 bytes stream 转换为 AsyncRead / Convert bytes stream to AsyncRead
struct StreamReader {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: Vec<u8>,
    pos: usize,
}

impl StreamReader {
    fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            stream: Box::pin(stream),
            buffer: Vec::new(),
            pos: 0,
        }
    }
}

impl AsyncRead for StreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.pos < self.buffer.len() {
            let remaining = &self.buffer[self.pos..];
            let to_copy = std::cmp::min(remaining.len(), buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.pos += to_copy;
            return Poll::Ready(Ok(()));
        }

        match self.stream.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                self.buffer = bytes.to_vec();
                self.pos = 0;
                let to_copy = std::cmp::min(self.buffer.len(), buf.remaining());
                buf.put_slice(&self.buffer[..to_copy]);
                self.pos = to_copy;
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

// ============ 驱动工厂 / Driver Factory ============

/// 123云盘开放平台驱动工厂 / 123 Cloud Open Platform driver factory
pub struct Pan123OpenDriverFactory;

impl DriverFactory for Pan123OpenDriverFactory {
    fn driver_type(&self) -> &'static str {
        "123_open"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "123云盘开放平台".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false, // 支持流式上传 / Supports streaming upload
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("client_id", "string")
                .title("客户端ID")
                .required()
                .help("从123云盘开发者平台获取 "),
            ConfigItem::new("client_secret", "string")
                .title("客户端密钥")
                .required()
                .help("从123云盘开发者平台获取"),
            ConfigItem::new("access_token", "string")
                .title("访问令牌")
                .help("可选，自动刷新"),
            ConfigItem::new("refresh_token", "string")
                .title("刷新令牌")
                .help("OAuth2模式使用"),
            ConfigItem::new("upload_thread", "number")
                .title("上传线程数")
                .default("3")
                .help("1-32"),
            ConfigItem::new("direct_link", "bool")
                .title("使用直链")
                .default("false"),
            ConfigItem::new("direct_link_private_key", "string")
                .title("直链私钥")
                .help("URL鉴权私钥"),
            ConfigItem::new("direct_link_valid_duration", "number")
                .title("直链有效期(分钟)")
                .default("30"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let pan123_config: Pan123OpenConfig = serde_json::from_value(config)?;
        let driver = Pan123OpenDriver::new(pan123_config);
        Ok(Box::new(driver))
    }
}
