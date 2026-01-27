//! 迅雷云盘 StorageDriver 实现

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::{Future, StreamExt};
use md5;
use reqwest::Client;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::Region;
use s3::serde_types::Part;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, 
    ProgressCallback, SpaceInfo, StorageDriver,
};

use super::client::ThunderClient;
use super::types::*;

// ============ 配置 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThunderConfig {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub sms_code: String,
    #[serde(default = "default_root_id")]
    pub root_id: String,
    #[serde(default)]
    pub refresh_token: String, // 持久化 refresh_token，避免频繁登录
}

fn default_root_id() -> String { "".to_string() }

// ============ 驱动能力 ============

fn thunder_capability() -> Capability {
    Capability {
        can_range_read: true,
        can_append: false,
        can_direct_link: false,
        max_chunk_size: None,
        can_concurrent_upload: false,
        requires_oauth: false,
        can_multipart_upload: false,
        can_server_side_copy: true,
        can_batch_operations: true,
        max_file_size: None,
        requires_full_file_for_upload: false,
    }
}

// ============ 驱动主体 ============

pub struct ThunderDriver {
    config: ThunderConfig,
    client: ThunderClient,
    path_cache: RwLock<HashMap<String, String>>,
    initialized: RwLock<bool>,
}

impl ThunderDriver {
    pub fn new(config: ThunderConfig) -> Self {
        // 生成设备 ID
        let input = format!("{}{}", config.username, config.password);
        let device_id = format!("{:x}", md5::compute(input.as_bytes()));

        let client = ThunderClient::new(
            device_id,
            String::new(),
            String::new(),
        );

        Self {
            config,
            client,
            path_cache: RwLock::new(HashMap::new()),
            initialized: RwLock::new(false),
        }
    }

    /// 确保已登录
    async fn ensure_login(&self) -> Result<()> {
        let initialized = *self.initialized.read().await;
        if !initialized {
            // 优先使用 refresh_token 刷新
            if !self.config.refresh_token.is_empty() {
                // 先设置 token，再刷新
                self.client.set_refresh_token(&self.config.refresh_token).await;
                match self.client.refresh_token().await {
                    Ok(_) => {
                        *self.initialized.write().await = true;
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("refresh_token 失效，需要重新登录: {}", e);
                    }
                }
            }
            
            // refresh_token 不可用，使用用户名密码登录
            let sms_code = if self.config.sms_code.is_empty() {
                None
            } else {
                Some(self.config.sms_code.as_str())
            };
            self.client.login_with_sms(&self.config.username, &self.config.password, sms_code).await?;
            *self.initialized.write().await = true;
        }
        Ok(())
    }
    
    /// 获取当前 refresh_token（用于持久化）
    pub async fn get_refresh_token(&self) -> Option<String> {
        self.client.get_refresh_token().await
    }

    /// 获取文件列表
    async fn get_files(&self, folder_id: &str) -> Result<Vec<ThunderFile>> {
        let mut files = Vec::new();
        let mut page_token = String::new();

        loop {
            let url = format!(
                "{}?space=&__type=drive&refresh=true&__sync=true&parent_id={}&page_token={}&with_audit=true&limit=100&filters={{\"trashed\":{{\"eq\":false}}}}",
                FILE_API_URL, folder_id, page_token
            );

            let resp: FileList = self.client.get(&url).await?;

            files.extend(resp.files);

            if resp.next_page_token.is_empty() {
                break;
            }
            page_token = resp.next_page_token;
        }

        Ok(files)
    }

    /// 通过路径获取文件 ID
    async fn get_file_id(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.config.root_id.clone());
        }

        // 检查缓存
        {
            let cache = self.path_cache.read().await;
            if let Some(id) = cache.get(path) {
                return Ok(id.clone());
            }
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.config.root_id.clone();
        let mut current_path = String::new();

        for part in parts {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);

            // 检查缓存
            {
                let cache = self.path_cache.read().await;
                if let Some(id) = cache.get(&current_path) {
                    current_id = id.clone();
                    continue;
                }
            }

            // 获取文件列表
            let files = self.get_files(&current_id).await?;
            let found = files.iter().find(|f| f.name == part);

            if let Some(file) = found {
                current_id = file.id.clone();
                self.path_cache.write().await.insert(current_path.clone(), current_id.clone());
            } else {
                // 清除父目录缓存，可能是新上传的文件
                if current_path.contains('/') {
                    let parent = current_path.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
                    self.path_cache.write().await.remove(&parent);
                }
                return Err(anyhow!("路径不存在: /{}", current_path));
            }
        }

        Ok(current_id)
    }

    /// 获取下载链接
    async fn get_download_url(&self, file_id: &str) -> Result<String> {
        let url = format!("{}/{}?space=", FILE_API_URL, file_id);
        let file: ThunderFile = self.client.get(&url).await?;

        // 优先使用 web_content_link
        if !file.web_content_link.is_empty() {
            return Ok(file.web_content_link);
        }

        // 备用：使用媒体链接
        for media in &file.medias {
            if !media.link.url.is_empty() {
                return Ok(media.link.url.clone());
            }
        }

        Err(anyhow!("无法获取下载链接，文件可能仍在处理中"))
    }
}

// ============ Reader ============

const DOWNLOAD_USER_AGENT: &str = "Dalvik/2.1.0 (Linux; U; Android 12; M2004J7AC Build/SP1A.210812.016)";

struct ThunderReader {
    inner: Pin<Box<dyn AsyncRead + Send + Unpin>>,
}

impl ThunderReader {
    async fn new(url: &str, range: Option<(u64, u64)>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(DOWNLOAD_USER_AGENT)
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(0) // 禁用连接池，避免 channel closed
            .build()?;

        let mut req = client.get(url);

        if let Some((start, end)) = range {
            req = req.header("Range", format!("bytes={}-{}", start, end));
        }

        let resp = req.send().await?;
        
        if !resp.status().is_success() && resp.status().as_u16() != 206 {
            return Err(anyhow!("下载失败: {}", resp.status()));
        }

        let stream = resp.bytes_stream()
            .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
        let reader = tokio_util::io::StreamReader::new(stream);

        Ok(Self {
            inner: Box::pin(reader),
        })
    }
}

impl AsyncRead for ThunderReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

// ============ Writer (S3 分片上传，内存限制 40MB) ============

const CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB 每片
const MAX_BUFFER_CHUNKS: usize = 2;

enum ChunkData {
    Part { part_number: u32, data: Bytes },
    Complete,
}

struct PartResult {
    part_number: u32,
    etag: String,
}

/// 创建 S3 Bucket 用于迅雷上传
fn create_thunder_bucket(params: &ResumableParams) -> Result<Box<Bucket>> {
    let credentials = Credentials::new(
        Some(&params.access_key_id),
        Some(&params.access_key_secret),
        Some(&params.security_token),
        None,
        None,
    ).map_err(|e| anyhow!("创建凭证失败: {}", e))?;

    let endpoint = params.endpoint.trim_start_matches(&format!("{}.", params.bucket));
    let region = Region::Custom {
        region: "xunlei".to_string(),
        endpoint: format!("https://{}", endpoint),
    };

    let bucket = Bucket::new(&params.bucket, region, credentials)
        .map_err(|e| anyhow!("创建Bucket失败: {}", e))?;

    Ok(bucket)
}

/// 分片上传任务
async fn multipart_upload(
    bucket: Box<Bucket>,
    key: String,
    mut rx: mpsc::Receiver<ChunkData>,
) -> Result<()> {
    use futures::{FutureExt, stream::FuturesUnordered};

    let init_response = bucket
        .initiate_multipart_upload(&key, "application/octet-stream")
        .await
        .map_err(|e| anyhow!("初始化分片上传失败: {}", e))?;

    let upload_id = init_response.upload_id;
    let bucket = Arc::new(bucket);

    let mut completed_parts: Vec<Part> = Vec::new();
    let mut pending_tasks: FuturesUnordered<tokio::task::JoinHandle<Result<PartResult>>> = FuturesUnordered::new();

    loop {
        // 处理已完成的任务
        while let Some(result) = pending_tasks.next().now_or_never().flatten() {
            let part_result = result.map_err(|e| anyhow!("任务失败: {}", e))??;
            completed_parts.push(Part {
                part_number: part_result.part_number,
                etag: part_result.etag,
            });
        }

        // 接收新分片
        if pending_tasks.len() < 2 {
            match rx.try_recv() {
                Ok(ChunkData::Part { part_number, data }) => {
                    let bucket = bucket.clone();
                    let key = key.clone();
                    let upload_id = upload_id.clone();
                    let data_vec = data.to_vec();

                    let task = tokio::spawn(async move {
                        let response = bucket
                            .put_multipart_chunk(data_vec, &key, part_number, &upload_id, "application/octet-stream")
                            .await
                            .map_err(|e| anyhow!("上传分片失败: {}", e))?;
                        Ok(PartResult { part_number, etag: response.etag })
                    });
                    pending_tasks.push(task);
                }
                Ok(ChunkData::Complete) => break,
                Err(mpsc::error::TryRecvError::Empty) => {
                    if pending_tasks.is_empty() {
                        match rx.recv().await {
                            Some(ChunkData::Part { part_number, data }) => {
                                let bucket = bucket.clone();
                                let key = key.clone();
                                let upload_id = upload_id.clone();
                                let data_vec = data.to_vec();

                                let task = tokio::spawn(async move {
                                    let response = bucket
                                        .put_multipart_chunk(data_vec, &key, part_number, &upload_id, "application/octet-stream")
                                        .await
                                        .map_err(|e| anyhow!("上传分片失败: {}", e))?;
                                    Ok(PartResult { part_number, etag: response.etag })
                                });
                                pending_tasks.push(task);
                            }
                            Some(ChunkData::Complete) | None => break,
                        }
                    } else {
                        if let Some(result) = pending_tasks.next().await {
                            let part_result = result.map_err(|e| anyhow!("任务失败: {}", e))??;
                            completed_parts.push(Part {
                                part_number: part_result.part_number,
                                etag: part_result.etag,
                            });
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        } else {
            if let Some(result) = pending_tasks.next().await {
                let part_result = result.map_err(|e| anyhow!("任务失败: {}", e))??;
                completed_parts.push(Part {
                    part_number: part_result.part_number,
                    etag: part_result.etag,
                });
            }
        }
    }

    // 等待剩余任务
    while let Some(result) = pending_tasks.next().await {
        let part_result = result.map_err(|e| anyhow!("任务失败: {}", e))??;
        completed_parts.push(Part {
            part_number: part_result.part_number,
            etag: part_result.etag,
        });
    }

    // 完成上传
    if completed_parts.is_empty() {
        let _ = bucket.abort_upload(&key, &upload_id).await;
    } else {
        completed_parts.sort_by_key(|p| p.part_number);
        bucket
            .complete_multipart_upload(&key, &upload_id, completed_parts)
            .await
            .map_err(|e| anyhow!("完成上传失败: {}", e))?;
    }

    Ok(())
}

struct ThunderWriter {
    tx: Option<mpsc::Sender<ChunkData>>,
    result_rx: Option<oneshot::Receiver<Result<(), String>>>,
    buffer: BytesMut,
    part_number: u32,
    pending_chunk: Option<(u32, Bytes)>,
    total_size: u64,
    uploaded_bytes: u64,
    progress: Option<ProgressCallback>,
    last_progress_time: std::time::Instant,
}

impl ThunderWriter {
    fn new(params: ResumableParams, size: u64, progress: Option<ProgressCallback>) -> Result<Self> {
        let bucket = create_thunder_bucket(&params)?;
        let key = params.key.clone();

        let (tx, rx) = mpsc::channel::<ChunkData>(MAX_BUFFER_CHUNKS);
        let (result_tx, result_rx) = oneshot::channel::<Result<(), String>>();

        tokio::spawn(async move {
            let result = multipart_upload(bucket, key, rx).await;
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        });

        Ok(Self {
            tx: Some(tx),
            result_rx: Some(result_rx),
            buffer: BytesMut::with_capacity(CHUNK_SIZE),
            part_number: 1,
            pending_chunk: None,
            total_size: size,
            uploaded_bytes: 0,
            progress,
            last_progress_time: std::time::Instant::now(),
        })
    }

    fn report_progress(&mut self, force: bool) {
        if let Some(ref cb) = self.progress {
            let now = std::time::Instant::now();
            if force || now.duration_since(self.last_progress_time).as_millis() >= 1000 {
                // 进度 = 已上传字节 + 缓冲区字节
                let current = self.uploaded_bytes + self.buffer.len() as u64;
                cb(current, self.total_size);
                self.last_progress_time = now;
            }
        }
    }
}

impl AsyncWrite for ThunderWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        // 如果有待发送的分片，先发送
        if let Some((part_number, data)) = this.pending_chunk.take() {
            if let Some(ref tx) = this.tx {
                match tx.try_send(ChunkData::Part { part_number, data: data.clone() }) {
                    Ok(()) => {
                        this.uploaded_bytes += CHUNK_SIZE as u64;
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        this.pending_chunk = Some((part_number, data));
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "上传通道已关闭",
                        )));
                    }
                }
            }
        }

        // 计算可接受数据量
        let space = CHUNK_SIZE.saturating_sub(this.buffer.len());
        let accept = buf.len().min(space.max(1));
        this.buffer.extend_from_slice(&buf[..accept]);
        
        // 每秒更新进度（基于缓冲的数据）
        this.report_progress(false);

        // buffer满了，发送分片
        if this.buffer.len() >= CHUNK_SIZE {
            let chunk = this.buffer.split_to(CHUNK_SIZE).freeze();
            let part_number = this.part_number;
            this.part_number += 1;

            if let Some(ref tx) = this.tx {
                match tx.try_send(ChunkData::Part { part_number, data: chunk.clone() }) {
                    Ok(()) => {
                        this.uploaded_bytes += CHUNK_SIZE as u64;
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        this.pending_chunk = Some((part_number, chunk));
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "上传通道已关闭",
                        )));
                    }
                }
            }
        }

        Poll::Ready(Ok(accept))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // 发送剩余数据
        if !this.buffer.is_empty() || this.pending_chunk.is_some() {
            // 先处理 pending_chunk
            if let Some((part_number, data)) = this.pending_chunk.take() {
                if let Some(ref tx) = this.tx {
                    match tx.try_send(ChunkData::Part { part_number, data: data.clone() }) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            this.pending_chunk = Some((part_number, data));
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Err(_) => {}
                    }
                }
            }

            // 发送 buffer 中剩余数据
            if !this.buffer.is_empty() {
                let mut remaining = std::mem::take(&mut this.buffer);
                // 迅雷不支持0字节
                if remaining.is_empty() && this.uploaded_bytes == 0 {
                    remaining.extend_from_slice(&[0u8]);
                }
                let chunk = remaining.freeze();
                let part_number = this.part_number;

                if let Some(ref tx) = this.tx {
                    match tx.try_send(ChunkData::Part { part_number, data: chunk.clone() }) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            this.buffer = BytesMut::from(chunk.as_ref());
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Err(_) => {}
                    }
                }
            }
        }

        // 发送完成信号
        if let Some(tx) = this.tx.take() {
            let _ = tx.try_send(ChunkData::Complete);
        }

        // 等待上传结果
        if let Some(mut rx) = this.result_rx.take() {
            match Pin::new(&mut rx).poll(cx) {
                Poll::Ready(Ok(Ok(()))) => Poll::Ready(Ok(())),
                Poll::Ready(Ok(Err(e))) => Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
                Poll::Ready(Err(_)) => Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "上传任务异常"))),
                Poll::Pending => {
                    this.result_rx = Some(rx);
                    Poll::Pending
                }
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

// ============ StorageDriver 实现 ============

#[async_trait]
impl StorageDriver for ThunderDriver {
    fn name(&self) -> &str { "迅雷云盘" }
    fn version(&self) -> &str { "1.0.0" }
    fn capabilities(&self) -> Capability { thunder_capability() }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_login().await?;
        let folder_id = self.get_file_id(path).await?;
        let files = self.get_files(&folder_id).await?;
        let base = path.trim_end_matches('/');

        let mut items = Vec::new();
        for f in files {
            let file_path = if base.is_empty() {
                format!("/{}", f.name)
            } else {
                format!("{}/{}", base, f.name)
            };

            // 缓存路径
            self.path_cache.write().await.insert(
                file_path.trim_start_matches('/').to_string(),
                f.id.clone(),
            );

            items.push(Entry {
                name: f.name.clone(),
                path: file_path,
                size: f.get_size(),
                is_dir: f.is_dir(),
                modified: if f.modified_time.is_empty() { None } else { Some(f.modified_time.clone()) },
            });
        }

        Ok(items)
    }

    async fn open_reader(&self, path: &str, range: Option<Range<u64>>) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_login().await?;
        let file_id = self.get_file_id(path).await?;
        let url = self.get_download_url(&file_id).await?;
        let range = range.map(|r| (r.start, r.end.saturating_sub(1)));
        Ok(Box::new(ThunderReader::new(&url, range).await?))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        self.ensure_login().await?;
        let path = path.trim_matches('/');
        let (parent_path, file_name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };

        let parent_id = if parent_path.is_empty() {
            self.config.root_id.clone()
        } else {
            self.get_file_id(parent_path).await?
        };

        // 迅雷不支持0字节文件，最小1字节
        let size = size_hint.unwrap_or(0).max(1);

        // 创建上传任务
        let body = serde_json::json!({
            "kind": FILE_KIND,
            "parent_id": parent_id,
            "name": file_name,
            "size": size,
            "hash": "",
            "upload_type": UPLOAD_TYPE_RESUMABLE,
            "space": ""
        });

        let resp: UploadTaskResponse = self.client.post(FILE_API_URL, body).await?;

        if resp.upload_type != UPLOAD_TYPE_RESUMABLE {
            return Err(anyhow!("不支持的上传类型: {}", resp.upload_type));
        }

        let params = resp.resumable
            .and_then(|r| r.params)
            .ok_or_else(|| anyhow!("未获取到上传参数"))?;

        let writer = ThunderWriter::new(params, size, progress)?;
        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_login().await?;
        let file_id = self.get_file_id(path).await?;
        let url = format!("{}/{}/trash?space=", FILE_API_URL, file_id);
        
        let _: Value = self.client.request(&url, reqwest::Method::PATCH, Some(serde_json::json!({}))).await?;

        // 清除缓存
        self.path_cache.write().await.remove(path.trim_matches('/'));
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_login().await?;
        let path = path.trim_matches('/');
        let (parent_path, dir_name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };

        let parent_id = if parent_path.is_empty() {
            self.config.root_id.clone()
        } else {
            self.get_file_id(parent_path).await?
        };

        let body = serde_json::json!({
            "kind": FOLDER_KIND,
            "name": dir_name,
            "parent_id": parent_id,
            "space": ""
        });

        let _: Value = self.client.post(FILE_API_URL, body).await?;
        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        self.ensure_login().await?;
        let file_id = self.get_file_id(path).await?;
        let url = format!("{}/{}?space=", FILE_API_URL, file_id);

        let body = serde_json::json!({
            "name": new_name,
            "space": ""
        });

        let _: Value = self.client.request(&url, reqwest::Method::PATCH, Some(body)).await?;

        // 清除缓存
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_login().await?;
        let file_id = self.get_file_id(old_path).await?;
        let new_parent_path = std::path::Path::new(new_path.trim_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let new_parent_id = if new_parent_path.is_empty() {
            self.config.root_id.clone()
        } else {
            self.get_file_id(&new_parent_path).await?
        };

        let url = format!("{}:batchMove", FILE_API_URL);
        let body = serde_json::json!({
            "to": { "parent_id": new_parent_id },
            "ids": [file_id],
            "space": ""
        });

        let _: Value = self.client.post(&url, body).await?;

        // 清除缓存
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn copy_item(&self, src_path: &str, dest_path: &str) -> Result<()> {
        self.ensure_login().await?;
        let file_id = self.get_file_id(src_path).await?;
        let dest_parent_path = std::path::Path::new(dest_path.trim_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let dest_parent_id = if dest_parent_path.is_empty() {
            self.config.root_id.clone()
        } else {
            self.get_file_id(&dest_parent_path).await?
        };

        let url = format!("{}:batchCopy", FILE_API_URL);
        let body = serde_json::json!({
            "to": { "parent_id": dest_parent_id },
            "ids": [file_id],
            "space": ""
        });

        let _: Value = self.client.post(&url, body).await?;
        Ok(())
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // 迅雷链接需要 User-Agent，不适合直链
        Ok(None)
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        // 迅雷 API 暂不支持获取空间信息
        Ok(None)
    }
}

// ============ 驱动工厂 ============

pub struct ThunderDriverFactory;

impl DriverFactory for ThunderDriverFactory {
    fn driver_type(&self) -> &'static str { "thunder" }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "迅雷云盘".to_string(),
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
                .title("手机号")
                .required(),
            ConfigItem::new("password", "string")
                .title("密码")
                .required(),
            ConfigItem::new("send_sms", "action")
                .title("发送验证码")
                .link("/api/driver/thunder/send_sms"),
            ConfigItem::new("sms_code", "string")
                .title("短信验证码"),
            ConfigItem::new("root_id", "string")
                .title("根目录ID")
                .default("")
                .help("留空表示根目录"),
            ConfigItem::new("refresh_token", "string")
                .title("Refresh Token")
                .help("登录成功后自动保存，用于免密登录"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: ThunderConfig = serde_json::from_value(config)?;
        Ok(Box::new(ThunderDriver::new(config)))
    }
}
