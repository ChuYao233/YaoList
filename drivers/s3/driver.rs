//! S3驱动核心实现
//!
//! 设计原则：
//! - 只提供原语（open_reader, open_writer, list等）
//! - 分片上传，每片16MB，内存只保留2片
//! - 支持预签名URL直链（302）

use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::Region;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use s3::serde_types::Part;
use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::{FutureExt, Future};

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo, ProgressCallback};
use super::config::S3Config;

const CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB per chunk (S3最小5MB)
const MAX_BUFFER_CHUNKS: usize = 2; // channel容量2
const CONCURRENT_UPLOADS: usize = 2; // 2个并发上传

/// S3驱动
pub struct S3Driver {
    config: S3Config,
    bucket: Box<Bucket>,
}

impl S3Driver {
    /// 创建新的S3驱动实例
    pub fn new(config: S3Config) -> Result<Self> {
        let bucket = Self::create_bucket(&config)?;
        Ok(Self { config, bucket })
    }
    
    /// 创建S3 Bucket客户端
    fn create_bucket(config: &S3Config) -> Result<Box<Bucket>> {
        let credentials = Credentials::new(
            Some(&config.access_key_id),
            Some(&config.secret_access_key),
            if config.session_token.is_empty() { None } else { Some(&config.session_token) },
            None,
            None,
        ).map_err(|e| anyhow!("创建S3凭证失败: {}", e))?;
        
        let region = if config.endpoint.is_empty() {
            Region::Custom {
                region: config.region.clone(),
                endpoint: format!("https://s3.{}.amazonaws.com", config.region),
            }
        } else {
            Region::Custom {
                region: config.region.clone(),
                endpoint: config.endpoint.clone(),
            }
        };
        
        let bucket = Bucket::new(&config.bucket, region, credentials)
            .map_err(|e| anyhow!("创建S3 Bucket失败: {}", e))?;
        
        let bucket = if config.force_path_style {
            bucket.with_path_style()
        } else {
            bucket
        };
        
        Ok(bucket)
    }
    
    /// 获取完整的对象键（路径）
    fn get_object_key(&self, path: &str) -> String {
        let root = self.config.root_path.trim_matches('/');
        let path = path.trim_start_matches('/');
        
        if root.is_empty() {
            path.to_string()
        } else if path.is_empty() {
            root.to_string()
        } else {
            format!("{}/{}", root, path)
        }
    }
    
    /// 获取目录前缀
    fn get_prefix(&self, path: &str) -> String {
        let key = self.get_object_key(path);
        if key.is_empty() {
            String::new()
        } else {
            format!("{}/", key.trim_end_matches('/'))
        }
    }
    
    /// 获取占位文件名
    fn placeholder_name(&self) -> &str {
        if self.config.placeholder.is_empty() {
            ".yaolist"
        } else {
            &self.config.placeholder
        }
    }
    
    /// S3 CopyObject - 使用copy_object_internal，验证复制结果
    async fn s3_copy_object(&self, src_key: &str, dst_key: &str) -> Result<()> {
        // copy_object_internal的from参数需要URL编码（中文等非ASCII字符）
        let encoded_src = urlencoding::encode(src_key);
        
        tracing::debug!("S3 CopyObject: src_key={}, encoded={}, dst_key={}", src_key, encoded_src, dst_key);
        
        // 执行复制
        let result = self.bucket
            .copy_object_internal(&encoded_src, dst_key)
            .await
            .map_err(|e| anyhow!("S3 CopyObject失败: {}", e))?;
        
        tracing::debug!("S3 CopyObject返回: {:?}", result);
        
        // 验证新文件是否存在
        let (_, code) = self.bucket
            .head_object(dst_key)
            .await
            .map_err(|e| anyhow!("验证复制结果失败: {}", e))?;
        
        if code != 200 {
            return Err(anyhow!("S3 CopyObject后新文件不存在, head返回: {}", code));
        }
        
        tracing::debug!("S3 CopyObject成功，新文件已验证存在");
        Ok(())
    }
    
    /// 删除单个文件 - 按照OpenList实现
    async fn remove_file(&self, key: &str) -> Result<()> {
        self.bucket
            .delete_object(key)
            .await
            .map_err(|e| anyhow!("删除S3对象失败: {}", e))?;
        Ok(())
    }
    
    /// 递归删除目录 - 按照OpenList实现
    async fn remove_dir(&self, path: &str) -> Result<()> {
        let prefix = self.get_prefix(path);
        
        let results = self.bucket
            .list(prefix, None)
            .await
            .map_err(|e| anyhow!("列出S3对象失败: {}", e))?;
        
        for result in results {
            for obj in result.contents {
                self.bucket
                    .delete_object(&obj.key)
                    .await
                    .map_err(|e| anyhow!("删除S3对象失败: {}", e))?;
            }
        }
        
        // 删除占位文件
        let placeholder_key = format!("{}/{}", self.get_object_key(path).trim_end_matches('/'), self.placeholder_name());
        let _ = self.bucket.delete_object(&placeholder_key).await;
        
        Ok(())
    }
}

#[async_trait]
impl StorageDriver for S3Driver {
    fn name(&self) -> &str {
        "S3"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true, // 支持预签名URL
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: true,
            can_batch_operations: true,
            max_file_size: None,
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let prefix = self.get_prefix(path);
        let placeholder = self.placeholder_name();
        
        let results = self.bucket
            .list(prefix.clone(), Some("/".to_string()))
            .await
            .map_err(|e| anyhow!("列出S3对象失败: {}", e))?;
        
        let mut entries = Vec::new();
        
        for result in results {
            // 处理目录（公共前缀）
            for cp in result.common_prefixes.unwrap_or_default() {
                let prefix_str = cp.prefix;
                let name = prefix_str
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                
                if !name.is_empty() && name != placeholder {
                    entries.push(Entry {
                        name,
                        path: String::new(),
                        size: 0,
                        is_dir: true,
                        modified: None,
                    });
                }
            }
            
            // 处理文件
            for obj in result.contents {
                if obj.key.ends_with('/') {
                    continue;
                }
                
                let name = obj.key.rsplit('/').next().unwrap_or(&obj.key).to_string();
                
                if name == placeholder {
                    continue;
                }
                
                let size = obj.size as u64;
                
                entries.push(Entry {
                    name,
                    path: String::new(),
                    size,
                    is_dir: false,
                    modified: Some(obj.last_modified.clone()),
                });
            }
        }
        
        Ok(entries)
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let key = self.get_object_key(path);
        
        // 流式获取对象
        let response = if let Some(r) = range {
            self.bucket
                .get_object_range(&key, r.start, Some(r.end))
                .await
                .map_err(|e| anyhow!("获取S3对象失败: {}", e))?
        } else {
            self.bucket
                .get_object(&key)
                .await
                .map_err(|e| anyhow!("获取S3对象失败: {}", e))?
        };
        
        // rust-s3返回完整响应，封装为AsyncRead
        let data = response.bytes().to_vec();
        Ok(Box::new(std::io::Cursor::new(data)))
    }
    
    async fn open_writer(
        &self,
        path: &str,
        _size_hint: Option<u64>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let key = self.get_object_key(path);
        let bucket = self.bucket.clone();
        
        // 使用有限容量的channel实现背压，内存只保留2片
        let (tx, rx) = mpsc::channel::<ChunkData>(MAX_BUFFER_CHUNKS);
        let (result_tx, result_rx) = oneshot::channel::<Result<(), String>>();
        
        // 后台任务：分片上传
        tokio::spawn(async move {
            let result = multipart_upload(bucket, key, rx).await;
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        });
        
        Ok(Box::new(S3Writer {
            tx: Some(tx),
            result_rx: Some(result_rx),
            buffer: BytesMut::with_capacity(CHUNK_SIZE),
            part_number: 1,
            pending_chunk: None,
            shutdown_state: ShutdownState::NotStarted,
        }))
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        // 按照OpenList: Remove方法
        // 先尝试作为文件删除
        let key = self.get_object_key(path);
        let _ = self.remove_file(&key).await;
        
        // 递归删除目录内容
        self.remove_dir(path).await
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        let key = format!("{}/{}", self.get_object_key(path).trim_end_matches('/'), self.placeholder_name());
        
        self.bucket
            .put_object(&key, &[])
            .await
            .map_err(|e| anyhow!("创建S3目录失败: {}", e))?;
        
        Ok(())
    }
    
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let old_key = self.get_object_key(old_path);
        
        let parent = if let Some(pos) = old_path.trim_matches('/').rfind('/') {
            &old_path.trim_matches('/')[..pos]
        } else {
            ""
        };
        
        let new_path = if parent.is_empty() {
            format!("/{}", new_name)
        } else {
            format!("/{}/{}", parent, new_name)
        };
        let new_key = self.get_object_key(&new_path);
        
        tracing::debug!("S3重命名: old_key={}, new_key={}", old_key, new_key);
        
        // 使用自己实现的copy_object
        self.s3_copy_object(&old_key, &new_key).await?;
        
        tracing::debug!("S3复制成功，删除原文件: {}", old_key);
        
        // 删除原对象
        self.bucket
            .delete_object(&old_key)
            .await
            .map_err(|e| anyhow!("删除S3原对象失败: {}", e))?;
        
        tracing::debug!("S3重命名完成");
        Ok(())
    }
    
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_key = self.get_object_key(old_path);
        let new_key = self.get_object_key(new_path);
        
        tracing::debug!("S3移动: old_key={}, new_key={}", old_key, new_key);
        
        // 使用自己实现的copy_object
        self.s3_copy_object(&old_key, &new_key).await?;
        
        // 删除原对象
        self.bucket
            .delete_object(&old_key)
            .await
            .map_err(|e| anyhow!("删除S3原对象失败: {}", e))?;
        
        tracing::debug!("S3移动完成");
        Ok(())
    }
    
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_key = self.get_object_key(old_path);
        let new_key = self.get_object_key(new_path);
        
        tracing::debug!("S3复制: old_key={}, new_key={}", old_key, new_key);
        
        // 使用服务端CopyObject API
        self.s3_copy_object(&old_key, &new_key).await?;
        
        tracing::debug!("S3复制完成");
        Ok(())
    }
    
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        let key = self.get_object_key(path);
        
        // 如果有自定义域名
        if !self.config.custom_host.is_empty() {
            let host = self.config.custom_host.trim_end_matches('/');
            return Ok(Some(format!("{}/{}", host, key)));
        }
        
        // 生成预签名URL
        let expire_secs = (self.config.sign_url_expire.max(1) as u64) * 3600;
        
        let url = self.bucket
            .presign_get(&key, expire_secs as u32, None)
            .await
            .map_err(|e| anyhow!("生成预签名URL失败: {}", e))?;
        
        Ok(Some(url))
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        Ok(None)
    }
}

/// 分片数据
enum ChunkData {
    Part { part_number: u32, data: Bytes },
    Complete,
}

/// 上传单个分片的结果
struct PartResult {
    part_number: u32,
    etag: String,
}

/// 分片上传后台任务 - 并发上传，控制内存
async fn multipart_upload(
    bucket: Box<Bucket>,
    key: String,
    mut rx: mpsc::Receiver<ChunkData>,
) -> Result<()> {
    // 初始化分片上传
    let init_response = bucket
        .initiate_multipart_upload(&key, "application/octet-stream")
        .await
        .map_err(|e| anyhow!("初始化分片上传失败: {}", e))?;
    
    let upload_id = init_response.upload_id;
    let bucket = Arc::new(bucket);
    
    tracing::debug!("S3分片上传开始: key={}, upload_id={}", key, upload_id);
    
    let mut completed_parts: Vec<Part> = Vec::new();
    let mut pending_tasks: FuturesUnordered<tokio::task::JoinHandle<Result<PartResult>>> = FuturesUnordered::new();
    
    loop {
        // 先处理已完成的任务，释放内存
        while let Some(result) = pending_tasks.next().now_or_never().flatten() {
            let join_result: std::result::Result<Result<PartResult>, _> = result;
            let part_result = join_result.map_err(|e| anyhow!("任务执行失败: {}", e))??;
            completed_parts.push(Part {
                part_number: part_result.part_number,
                etag: part_result.etag,
            });
        }
        
        // 如果并发数未满，尝试接收新分片
        if pending_tasks.len() < CONCURRENT_UPLOADS {
            match rx.try_recv() {
                Ok(ChunkData::Part { part_number, data }) => {
                    let bucket = bucket.clone();
                    let key = key.clone();
                    let upload_id = upload_id.clone();
                    
                    // 立即消费data，避免保持引用
                    let data_vec = data.to_vec();
                    drop(data); // 显式释放Bytes
                    
                    let task = tokio::spawn(async move {
                        tracing::debug!("S3上传分片: key={}, part={}, size={}", key, part_number, data_vec.len());
                        
                        let response = bucket
                            .put_multipart_chunk(
                                data_vec,
                                &key,
                                part_number,
                                &upload_id,
                                "application/octet-stream",
                            )
                            .await
                            .map_err(|e| anyhow!("上传分片失败: part={}, error={}", part_number, e))?;
                        
                        Ok(PartResult {
                            part_number,
                            etag: response.etag,
                        })
                    });
                    
                    pending_tasks.push(task);
                }
                Ok(ChunkData::Complete) => {
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // channel空了，等待任务完成或新数据
                    if pending_tasks.is_empty() {
                        // 没有进行中的任务，阻塞等待新数据
                        match rx.recv().await {
                            Some(ChunkData::Part { part_number, data }) => {
                                let bucket = bucket.clone();
                                let key = key.clone();
                                let upload_id = upload_id.clone();
                                let data_vec = data.to_vec();
                                drop(data);
                                
                                let task = tokio::spawn(async move {
                                    tracing::debug!("S3上传分片: key={}, part={}, size={}", key, part_number, data_vec.len());
                                    let response = bucket
                                        .put_multipart_chunk(data_vec, &key, part_number, &upload_id, "application/octet-stream")
                                        .await
                                        .map_err(|e| anyhow!("上传分片失败: part={}, error={}", part_number, e))?;
                                    Ok(PartResult { part_number, etag: response.etag })
                                });
                                pending_tasks.push(task);
                            }
                            Some(ChunkData::Complete) | None => break,
                        }
                    } else {
                        // 有进行中的任务，等待一个完成
                        if let Some(result) = pending_tasks.next().await {
                            let join_result: std::result::Result<Result<PartResult>, _> = result;
                            let part_result = join_result.map_err(|e| anyhow!("任务执行失败: {}", e))??;
                            completed_parts.push(Part {
                                part_number: part_result.part_number,
                                etag: part_result.etag,
                            });
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            }
        } else {
            // 并发数已满，等待一个任务完成
            if let Some(result) = pending_tasks.next().await {
                let join_result: std::result::Result<Result<PartResult>, _> = result;
                let part_result = join_result.map_err(|e| anyhow!("任务执行失败: {}", e))??;
                completed_parts.push(Part {
                    part_number: part_result.part_number,
                    etag: part_result.etag,
                });
            }
        }
    }
    
    // 等待所有剩余任务完成
    while let Some(result) = pending_tasks.next().await {
        let join_result: std::result::Result<Result<PartResult>, _> = result;
        let part_result = join_result.map_err(|e| anyhow!("任务执行失败: {}", e))??;
        completed_parts.push(Part {
            part_number: part_result.part_number,
            etag: part_result.etag,
        });
    }
    
    // 完成分片上传
    if completed_parts.is_empty() {
        let _ = bucket.abort_upload(&key, &upload_id).await;
        tracing::debug!("S3分片上传取消（无数据）: key={}", key);
    } else {
        completed_parts.sort_by_key(|p| p.part_number);
        
        bucket
            .complete_multipart_upload(&key, &upload_id, completed_parts)
            .await
            .map_err(|e| anyhow!("完成分片上传失败: {}", e))?;
        
        tracing::debug!("S3分片上传完成: key={}", key);
    }
    
    Ok(())
}

/// S3写入器 - 分片上传，每片16MB，内存只保留2片
struct S3Writer {
    tx: Option<mpsc::Sender<ChunkData>>,
    result_rx: Option<oneshot::Receiver<Result<(), String>>>,
    buffer: BytesMut,
    part_number: u32,
    pending_chunk: Option<(u32, Bytes)>, // 待发送的分片
    shutdown_state: ShutdownState,
}

#[derive(Clone, Copy, PartialEq)]
enum ShutdownState {
    NotStarted,
    SendingRemainder,
    SendingComplete,
    Done,
}

impl AsyncWrite for S3Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        
        // 如果有待发送的分片，必须先发送完，不接受新数据
        if let Some((part_number, data)) = this.pending_chunk.take() {
            if let Some(ref tx) = this.tx {
                match tx.try_send(ChunkData::Part { part_number, data: data.clone() }) {
                    Ok(()) => {
                        // 发送成功，继续处理
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        this.pending_chunk = Some((part_number, data));
                        cx.waker().wake_by_ref();
                        return Poll::Pending; // 不接受新数据
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "S3上传通道已关闭",
                        )));
                    }
                }
            }
        }
        
        // 计算可以接受的数据量：只接受能填满一个分片的数据
        let space_in_buffer = CHUNK_SIZE.saturating_sub(this.buffer.len());
        let bytes_to_accept = buf.len().min(space_in_buffer.max(1)); // 至少接受1字节避免死锁
        
        // 添加有限的数据到缓冲区
        this.buffer.extend_from_slice(&buf[..bytes_to_accept]);
        
        // 如果缓冲区达到分片大小，发送分片
        if this.buffer.len() >= CHUNK_SIZE {
            let chunk_data = this.buffer.split_to(CHUNK_SIZE).freeze();
            let part_number = this.part_number;
            this.part_number += 1;
            
            if let Some(ref tx) = this.tx {
                match tx.try_send(ChunkData::Part { part_number, data: chunk_data.clone() }) {
                    Ok(()) => {
                        // 发送成功
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // channel满了，保存待发送分片
                        this.pending_chunk = Some((part_number, chunk_data));
                        cx.waker().wake_by_ref();
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "S3上传通道已关闭",
                        )));
                    }
                }
            }
        }
        
        Poll::Ready(Ok(bytes_to_accept))
    }
    
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        
        loop {
            // 每次循环开始都先检查并发送pending_chunk
            if let Some((part_number, data)) = this.pending_chunk.take() {
                if let Some(ref tx) = this.tx {
                    match tx.try_send(ChunkData::Part { part_number, data: data.clone() }) {
                        Ok(()) => {
                            // 发送成功，继续处理
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            this.pending_chunk = Some((part_number, data));
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Err(_) => {
                            // channel关闭，忽略错误继续
                        }
                    }
                }
            }
            
            match this.shutdown_state {
                ShutdownState::NotStarted => {
                    this.shutdown_state = ShutdownState::SendingRemainder;
                }
                ShutdownState::SendingRemainder => {
                    // 发送剩余数据
                    if !this.buffer.is_empty() {
                        let chunk_data = this.buffer.split().freeze();
                        let part_number = this.part_number;
                        this.part_number += 1;
                        
                        tracing::debug!("S3 shutdown: 发送剩余分片 part={}, size={}", part_number, chunk_data.len());
                        
                        if let Some(ref tx) = this.tx {
                            match tx.try_send(ChunkData::Part { part_number, data: chunk_data.clone() }) {
                                Ok(()) => {
                                    tracing::debug!("S3 shutdown: 剩余分片发送成功");
                                }
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    tracing::debug!("S3 shutdown: channel满，等待");
                                    this.pending_chunk = Some((part_number, chunk_data));
                                    cx.waker().wake_by_ref();
                                    return Poll::Pending;
                                }
                                Err(_) => {
                                    tracing::warn!("S3 shutdown: channel关闭");
                                }
                            }
                        }
                    }
                    // 确保没有pending_chunk才切换状态
                    if this.pending_chunk.is_none() {
                        this.shutdown_state = ShutdownState::SendingComplete;
                    } else {
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }
                ShutdownState::SendingComplete => {
                    // 发送完成信号
                    if let Some(tx) = this.tx.take() {
                        match tx.try_send(ChunkData::Complete) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                this.tx = Some(tx);
                                cx.waker().wake_by_ref();
                                return Poll::Pending;
                            }
                            Err(_) => {}
                        }
                    }
                    this.shutdown_state = ShutdownState::Done;
                }
                ShutdownState::Done => {
                    // 等待后台任务完成
                    if let Some(ref mut result_rx) = this.result_rx {
                        match Pin::new(result_rx).poll(cx) {
                            Poll::Ready(Ok(Ok(()))) => {
                                this.result_rx = None;
                                return Poll::Ready(Ok(()));
                            }
                            Poll::Ready(Ok(Err(e))) => {
                                this.result_rx = None;
                                return Poll::Ready(Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    format!("S3上传失败: {}", e),
                                )));
                            }
                            Poll::Ready(Err(_)) => {
                                this.result_rx = None;
                                return Poll::Ready(Err(std::io::Error::new(
                                    std::io::ErrorKind::BrokenPipe,
                                    "S3上传任务异常终止",
                                )));
                            }
                            Poll::Pending => {
                                return Poll::Pending;
                            }
                        }
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
    }
}
