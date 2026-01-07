

//! OneDrive App streaming upload Writer - async upload, no disk cache / OneDrive App 流式上传Writer
//! 
//! Design principles / 设计原则：
//! - Frontend sends chunks (max 20MB) to backend memory / 前端传分片（最大20MB）到后端内存
//! - Backend uploads chunk to OD while receiving next chunk / 后端上传到OD同时接收下一个分片
//! - Memory keeps at most 2 chunks: one uploading, one receiving / 内存最多保留2个分片：一个上传中，一个接收中
//! - No disk cache, all in memory / 不落盘，全部在内存

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio::sync::Mutex;
use tokio::time::sleep;
use std::sync::{atomic::{AtomicU64, AtomicBool, Ordering}, mpsc};
use std::time::Duration;

use crate::storage::ProgressCallback;
use super::api::OneDriveAppApi;

/// Upload task / 上传任务
struct UploadTask {
    chunk: Vec<u8>,
    start: u64,
    is_last: bool,
}

/// OneDrive App streaming upload Writer / OneDrive App 流式上传Writer
pub struct OneDriveAppWriter {
    /// Current chunk buffer (receiving next chunk) / 当前分片缓冲区（正在接收下一个分片）
    buffer: Vec<u8>,
    /// Chunk size in bytes / 分片大小（字节）
    chunk_size_bytes: u64,
    /// File path / 文件路径
    path: String,
    /// API client / API客户端
    api: Arc<OneDriveAppApi>,
    /// Total uploaded bytes / 已上传字节数（已成功上传到服务器）
    uploaded_bytes: Arc<AtomicU64>,
    /// Total written bytes / 已写入字节数（包括buffer中未上传的数据）
    written_bytes: Arc<AtomicU64>,
    /// Total sent bytes to upload channel / 已发送到上传通道的字节数
    sent_bytes: Arc<AtomicU64>,
    /// File total size (for Content-Range) / 文件总大小（用于Content-Range）
    total_size: u64,
    /// Upload session URL (for large files) / 上传会话URL（大文件用）
    upload_session_url: Arc<Mutex<Option<String>>>,
    /// Whether session is initialized / 是否已初始化会话
    session_initialized: Arc<AtomicBool>,
    /// Whether writer is closed / 是否已关闭
    closed: Arc<AtomicBool>,
    /// Upload error / 上传错误
    error: Arc<std::sync::Mutex<Option<String>>>,
    /// Progress callback / 进度回调
    progress: Option<ProgressCallback>,
    /// Channel sender for upload tasks / 上传任务通道发送端
    task_tx: Option<mpsc::SyncSender<UploadTask>>,
    /// Background upload task handle / 后台上传任务句柄
    task_handle: Option<std::thread::JoinHandle<()>>,
    /// Whether first chunk has been sent / 是否已发送第一个分片
    first_chunk_sent: bool,
    /// Progress update task handle / 进度更新任务句柄
    progress_handle: Option<tokio::task::JoinHandle<()>>,
}

impl OneDriveAppWriter {
    pub fn new(
        path: String,
        size_hint: Option<u64>,
        api: Arc<OneDriveAppApi>,
        progress: Option<ProgressCallback>,
    ) -> Self {
        let config = api.get_config();
        let chunk_size_mb = config.chunk_size;
        let chunk_size_bytes = chunk_size_mb * 1024 * 1024; // MB to bytes
        let total_size = size_hint.unwrap_or(0);
        
        let uploaded_bytes = Arc::new(AtomicU64::new(0));
        let written_bytes = Arc::new(AtomicU64::new(0));
        let sent_bytes = Arc::new(AtomicU64::new(0));
        let upload_session_url = Arc::new(Mutex::new(None));
        let session_initialized = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let error = Arc::new(std::sync::Mutex::new(None));
        
        // 创建有界上传任务通道（容量为2：一个正在上传，一个在接收）
        // 这样可以限制内存中的分片数量
        let (tx, rx) = mpsc::sync_channel::<UploadTask>(2);
        
        // 启动后台上传线程
        let api_clone = api.clone();
        let path_clone = path.clone();
        let uploaded_bytes_clone = uploaded_bytes.clone();
        let upload_session_url_clone = upload_session_url.clone();
        let session_initialized_clone = session_initialized.clone();
        let error_clone = error.clone();
        let progress_clone = progress.clone();
        let total_size_clone = total_size;
        
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                upload_worker(
                    rx,
                    api_clone,
                    path_clone,
                    uploaded_bytes_clone,
                    upload_session_url_clone,
                    session_initialized_clone,
                    error_clone,
                    progress_clone,
                    total_size_clone,
                ).await;
            });
        });
        
        // 启动定期进度更新任务（每秒更新一次）
        let written_bytes_progress = written_bytes.clone();
        let uploaded_bytes_progress = uploaded_bytes.clone();
        let progress_progress = progress.clone();
        let closed_progress = closed.clone();
        let total_size_progress = total_size;
        let progress_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                
                // 如果已关闭，停止更新
                if closed_progress.load(Ordering::SeqCst) {
                    break;
                }
                
                // 获取当前已写入字节数（包括buffer中未上传的数据）和已上传字节数
                let written = written_bytes_progress.load(Ordering::SeqCst);
                let uploaded = uploaded_bytes_progress.load(Ordering::SeqCst);
                
                // 使用两者的最大值作为当前进度（这样可以看到包括buffer中的数据和已上传的数据）
                let current_progress = written.max(uploaded);
                let total = if total_size_progress == 0 { current_progress } else { total_size_progress };
                
                // 调用进度回调
                if let Some(ref cb) = progress_progress {
                    cb(current_progress, total);
                }
            }
        });
        
        Self {
            buffer: Vec::with_capacity(chunk_size_bytes as usize),
            chunk_size_bytes,
            path,
            api,
            uploaded_bytes,
            written_bytes,
            sent_bytes,
            total_size,
            upload_session_url,
            session_initialized,
            closed,
            error,
            progress,
            task_tx: Some(tx),
            task_handle: Some(handle),
            first_chunk_sent: false,
            progress_handle: Some(progress_handle),
        }
    }

    /// Send chunk to background upload / 发送分片到后台上传
    fn send_chunk(&mut self, chunk: Vec<u8>, is_last: bool) -> std::io::Result<()> {
        // 允许发送空分片，特别是当 is_last 为 true 时（用于创建空文件）
        // 如果 chunk 为空且不是最后一个分片，则跳过
        if chunk.is_empty() && !is_last {
            return Ok(());
        }

        // 使用 sent_bytes 而不是 uploaded_bytes，因为 uploaded_bytes 是异步更新的
        // 这样可以确保分片的 start 位置是连续的
        let start = self.sent_bytes.load(Ordering::SeqCst);
        let chunk_len = chunk.len() as u64;
        let end = start + chunk_len;
        
        tracing::trace!(
            "OneDrive App发送分片: path={}, range={}-{}, is_last={}, sent_bytes={}, chunk_len={}",
            self.path, start, end - 1, is_last, start, chunk_len
        );
        
        if let Some(ref tx) = self.task_tx {
            tx.send(UploadTask {
                chunk,
                start,
                is_last,
            }).map_err(|_| std::io::Error::new(
                std::io::ErrorKind::Other,
                "发送上传任务失败"
            ))?;
            
            // 更新已发送字节数（空分片时 chunk_len 为 0，不会影响计数）
            self.sent_bytes.fetch_add(chunk_len, Ordering::SeqCst);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "上传通道已关闭"
            ));
        }

        Ok(())
    }

    /// Check error / 检查错误
    fn check_error(&self) -> std::io::Result<()> {
        let error_guard = self.error.lock().unwrap();
        if let Some(ref err) = *error_guard {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone()));
        }
        Ok(())
    }
}

/// Background upload worker / 后台上传工作线程
async fn upload_worker(
    rx: std::sync::mpsc::Receiver<UploadTask>,
    api: Arc<OneDriveAppApi>,
    path: String,
    uploaded_bytes: Arc<AtomicU64>,
    upload_session_url: Arc<Mutex<Option<String>>>,
    session_initialized: Arc<AtomicBool>,
    error: Arc<std::sync::Mutex<Option<String>>>,
    progress: Option<ProgressCallback>,
    total_size: u64,
) {
    // 重试配置
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 500;

    while let Ok(task) = rx.recv() {
        // 检查是否已关闭
        // 这里不需要检查，因为即使关闭了也要完成当前任务
        
        // 如果是最后一个分片且还没初始化会话，说明是小文件，直接上传
        if task.is_last && !session_initialized.load(Ordering::SeqCst) && task.start == 0 {
            let chunk_len = task.chunk.len() as u64;
            let mut last_err: Option<String> = None;

            for attempt in 0..=MAX_RETRIES {
                match api.upload_small_file(&path, task.chunk.clone()).await {
                    Ok(()) => {
                        let total = chunk_len;
                        uploaded_bytes.store(total, Ordering::SeqCst);
                        
                        if let Some(ref cb) = progress {
                            cb(total, total);
                        }
                        
                        tracing::debug!(
                            "OneDrive App小文件直接上传成功: path={}, size={}, retries={}",
                            path,
                            total,
                            attempt
                        );
                        return;
                    }
                    Err(e) => {
                        let err_msg = format!("上传失败(第{}次尝试): {}", attempt + 1, e);
                        tracing::warn!("{}", err_msg);
                        last_err = Some(err_msg);

                        if attempt == MAX_RETRIES {
                            let mut error_guard = error.lock().unwrap();
                            *error_guard = last_err.clone();
                            tracing::error!(
                                "OneDrive App小文件上传最终失败: path={}, size={}, retries={}",
                                path,
                                chunk_len,
                                MAX_RETRIES + 1
                            );
                            return;
                        } else {
                            let delay = BASE_DELAY_MS * 2u64.saturating_pow(attempt);
                            sleep(Duration::from_millis(delay)).await;
                        }
                    }
                }
            }
            // 正常情况下不会到这里
            return;
        }

        // 确保会话已创建
        if !session_initialized.load(Ordering::SeqCst) {
            let mut last_err: Option<String> = None;

            for attempt in 0..=MAX_RETRIES {
                match api.create_upload_session(&path).await {
                    Ok(upload_url) => {
                        let mut session_url = upload_session_url.lock().await;
                        *session_url = Some(upload_url);
                        session_initialized.store(true, Ordering::SeqCst);
                        tracing::info!(
                            "OneDrive App创建上传会话成功: path={}, retries={}",
                            path,
                            attempt
                        );
                        break;
                    }
                    Err(e) => {
                        let err_msg = format!("创建上传会话失败(第{}次尝试): {}", attempt + 1, e);
                        tracing::warn!("{}", err_msg);
                        last_err = Some(err_msg);

                        if attempt == MAX_RETRIES {
                            let mut error_guard = error.lock().unwrap();
                            *error_guard = last_err.clone();
                            tracing::error!(
                                "OneDrive App创建上传会话最终失败: path={}, retries={}",
                                path,
                                MAX_RETRIES + 1
                            );
                            return;
                        } else {
                            let delay = BASE_DELAY_MS * 2u64.saturating_pow(attempt);
                            sleep(Duration::from_millis(delay)).await;
                        }
                    }
                }
            }
            if !session_initialized.load(Ordering::SeqCst) {
                // 会话始终未创建成功，结束worker
                return;
            }
        }

        // 获取上传URL
        let upload_url = {
            let session_url = upload_session_url.lock().await;
            session_url.clone()
        };

        let upload_url = match upload_url {
            Some(url) => url,
            None => {
                let err_msg = "上传会话未创建".to_string();
                tracing::error!("{}", err_msg);
                let mut error_guard = error.lock().unwrap();
                *error_guard = Some(err_msg);
                break;
            }
        };

        // 上传分片
        let end = task.start + task.chunk.len() as u64;
        // 当 total_size == 0 时，保持 total 为 0
        // api.rs 中会使用 end 作为 Content-Range 的 total，但判断最后一个分片时使用 is_last 参数
        let total = total_size;
        
        let mut last_err: Option<String> = None;

        for attempt in 0..=MAX_RETRIES {
            match api.upload_chunk(&upload_url, task.chunk.clone(), task.start, end, total, task.is_last).await {
                Ok(()) => {
                    uploaded_bytes.store(end, Ordering::SeqCst);
                    
                    // 报告进度
                    if let Some(ref cb) = progress {
                        cb(end, total);
                    }
                    
                    tracing::debug!(
                        "OneDrive App分片上传: path={}, range={}-{}, uploaded={}/{}, retries={}", 
                        path,
                        task.start,
                        end - 1,
                        end,
                        total,
                        attempt
                    );
                    
                    // 如果是最后一个分片，标记完成
                    if task.is_last {
                        tracing::info!("OneDrive App上传完成: path={}, total={}", path, end);
                    }
                    last_err = None;
                    break;
                }
                Err(e) => {
                    let err_msg = format!(
                        "分片上传失败(第{}次尝试, range={}-{}): {}",
                        attempt + 1,
                        task.start,
                        end - 1,
                        e
                    );
                    tracing::warn!("{}", err_msg);
                    last_err = Some(err_msg);

                    if attempt == MAX_RETRIES {
                        let mut error_guard = error.lock().unwrap();
                        *error_guard = last_err.clone();
                        tracing::error!(
                            "OneDrive App分片上传最终失败: path={}, range={}-{}, retries={}",
                            path,
                            task.start,
                            end - 1,
                            MAX_RETRIES + 1
                        );
                    } else {
                        let delay = BASE_DELAY_MS * 2u64.saturating_pow(attempt);
                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }

        // 如果重试后仍然失败，则终止worker
        if last_err.is_some() {
            break;
        }
    }
}

impl AsyncWrite for OneDriveAppWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // 检查错误
        if let Err(e) = self.check_error() {
            return Poll::Ready(Err(e));
        }

        // 检查是否已关闭
        if self.closed.load(Ordering::SeqCst) {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Writer已关闭"
            )));
        }

        let mut written = 0;
        let mut remaining = buf;

        // 循环处理，直到所有数据都写入
        while !remaining.is_empty() {
            let space_left = (self.chunk_size_bytes as usize).saturating_sub(self.buffer.len());
            
            // 如果buffer已满，或者buffer达到chunk_size且还没发送第一个分片，立即发送
            let should_send = space_left == 0 || 
                (!self.first_chunk_sent && self.buffer.len() >= self.chunk_size_bytes as usize);
            
            if should_send && !self.buffer.is_empty() {
                // 发送当前buffer到后台上传
                let chunk = std::mem::take(&mut self.buffer);
                self.buffer = Vec::with_capacity(self.chunk_size_bytes as usize);
                
                if let Err(e) = self.send_chunk(chunk, false) {
                    return Poll::Ready(Err(e));
                }
                self.first_chunk_sent = true;
                continue; // 继续处理剩余数据
            }

            // 写入数据到buffer
            let to_write = remaining.len().min(space_left);
            self.buffer.extend_from_slice(&remaining[..to_write]);
            written += to_write;
            remaining = &remaining[to_write..];
            
            // 累加已写入字节数（每次写入时累加）
            self.written_bytes.fetch_add(to_write as u64, Ordering::SeqCst);
            
            // 如果这是第一个分片且buffer已达到chunk_size，立即发送
            if !self.first_chunk_sent && self.buffer.len() >= self.chunk_size_bytes as usize {
                let chunk = std::mem::take(&mut self.buffer);
                self.buffer = Vec::with_capacity(self.chunk_size_bytes as usize);
                
                if let Err(e) = self.send_chunk(chunk, false) {
                    return Poll::Ready(Err(e));
                }
                self.first_chunk_sent = true;
            }
        }

        Poll::Ready(Ok(written))
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
        if self.closed.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }

        // 检查错误
        if let Err(e) = self.check_error() {
            return Poll::Ready(Err(e));
        }

        self.closed.store(true, Ordering::SeqCst);

        // 发送最后一个分片（如果有）
        let chunk = std::mem::take(&mut self.buffer);
        let written = self.written_bytes.load(Ordering::SeqCst);
        let sent = self.sent_bytes.load(Ordering::SeqCst);
        
        if !chunk.is_empty() {
            tracing::debug!("OneDrive App Writer shutdown: 发送最后一个分片: path={}, chunk_size={}, written={}, sent={}", 
                self.path, chunk.len(), written, sent);
            if let Err(e) = self.send_chunk(chunk, true) {
                return Poll::Ready(Err(e));
            }
        } else {
            // buffer 为空，检查是否所有数据都已发送
            if written == 0 && self.total_size == 0 {
                // 空文件：需要发送一个空分片来创建文件
                tracing::debug!("OneDrive App Writer shutdown: 空文件，发送空分片: path={}", self.path);
                if let Err(e) = self.send_chunk(Vec::new(), true) {
                    return Poll::Ready(Err(e));
                }
            } else if sent < written {
                // 还有数据没有发送，这不应该发生
                let err_msg = format!(
                    "数据不一致: buffer为空但还有数据未发送: 已写入 {} 字节，已发送 {} 字节",
                    written, sent
                );
                tracing::error!("OneDrive App Writer shutdown 失败: {}", err_msg);
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err_msg
                )));
            } else if sent > 0 {
                // 所有数据都已发送，但最后一个分片可能还没有标记为 is_last
                // 这种情况不应该发生，因为如果 buffer 为空且 sent > 0，说明所有数据都已发送
                // 但为了安全，我们继续执行验证
                tracing::debug!("OneDrive App Writer shutdown: buffer为空，所有数据已发送: path={}, sent={}, written={}", 
                    self.path, sent, written);
            }
        }

        // 停止进度更新任务
        if let Some(handle) = self.progress_handle.take() {
            handle.abort();
        }

        // 关闭发送通道，让后台任务结束
        self.task_tx.take();

        // 等待后台任务完成
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.join();
        }

        // 再次检查错误
        if let Err(e) = self.check_error() {
            return Poll::Ready(Err(e));
        }

        // 验证上传完整性
        // 注意：这里不等待，因为后台线程已经完成，uploaded_bytes 应该已经是最新值
        // 如果上传失败，error 中会有错误信息
        let uploaded = self.uploaded_bytes.load(Ordering::SeqCst);
        let written = self.written_bytes.load(Ordering::SeqCst);
        let sent = self.sent_bytes.load(Ordering::SeqCst);
        
        tracing::debug!("OneDrive App Writer shutdown 验证: path={}, uploaded={}, written={}, sent={}, total_size={}", 
            self.path, uploaded, written, sent, self.total_size);
        
        if self.total_size > 0 {
            if uploaded != self.total_size {
                let err_msg = format!(
                    "上传不完整: 已上传 {} 字节，期望 {} 字节，已发送 {} 字节，已写入 {} 字节",
                    uploaded, self.total_size, sent, written
                );
                tracing::error!("OneDrive App Writer shutdown 失败: {}", err_msg);
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err_msg
                )));
            }
        } else {
            // 如果 total_size == 0，验证 uploaded_bytes 是否等于 sent_bytes
            // 并且 sent_bytes 应该等于 written_bytes（所有写入的数据都已发送）
            if sent != written {
                let err_msg = format!(
                    "数据不一致: 已写入 {} 字节，但只发送了 {} 字节",
                    written, sent
                );
                tracing::error!("OneDrive App Writer shutdown 失败: {}", err_msg);
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err_msg
                )));
            }
            if uploaded < sent {
                let err_msg = format!(
                    "上传不完整: 已发送 {} 字节，但只上传了 {} 字节",
                    sent, uploaded
                );
                tracing::error!("OneDrive App Writer shutdown 失败: {}", err_msg);
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err_msg
                )));
            }
        }
        
        tracing::info!("OneDrive App Writer shutdown: path={}, total_uploaded={}, total_written={}, total_sent={}", 
            self.path, uploaded, written, sent);

        Poll::Ready(Ok(()))
    }
}
