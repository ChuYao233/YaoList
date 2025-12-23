//! 123云盘流式上传实现 - 内存缓冲+即时上传
//! 123 Cloud streaming upload - Memory buffer + immediate upload
//!
//! 设计：
//! 1. poll_write: 累积数据到内存缓冲区，同时计算整体MD5
//! 2. poll_shutdown: 创建上传任务，从内存分片上传
//!
//! 内存占用：整个文件大小（因为123云盘API需要整体MD5）

use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use reqwest::multipart::{Form, Part};

use super::api::ApiClient;
use super::types::*;
use crate::storage::ProgressCallback;

/// 计算数据的MD5哈希 / Calculate MD5 hash of data
fn calculate_md5(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

/// 上传状态 / Upload state
struct UploadState {
    /// 内存缓冲区 / Memory buffer
    buffer: Vec<u8>,
    /// MD5计算上下文 / MD5 calculation context
    md5_context: md5::Context,
    /// 是否已关闭 / Whether closed
    closed: bool,
    /// 错误信息 / Error message
    error: Option<String>,
    /// 上传是否完成 / Upload completed
    upload_done: bool,
    /// 上传结果 / Upload result
    upload_result: Option<Result<(), String>>,
}

/// 并发上传进度跟踪 / Concurrent upload progress tracking
struct UploadProgress {
    /// 已完成的分片数 / Completed slices
    completed_slices: AtomicU64,
    /// 总分片数 / Total slices
    total_slices: u64,
    /// 文件总大小 / Total file size
    total_size: u64,
    /// 进度回调 / Progress callback
    callback: Option<ProgressCallback>,
}

impl UploadProgress {
    fn new(total_slices: u64, total_size: u64, callback: Option<ProgressCallback>) -> Self {
        Self {
            completed_slices: AtomicU64::new(0),
            total_slices,
            total_size,
            callback,
        }
    }

    /// 报告分片完成 / Report slice completion
    fn report_slice_done(&self) {
        let completed = self.completed_slices.fetch_add(1, Ordering::SeqCst) + 1;
        let progress_bytes = ((completed as f64 / self.total_slices as f64) * self.total_size as f64) as u64;
        
        tracing::debug!("123云盘进度: {}/{} 分片完成, 进度 {}/{} 字节", 
            completed, self.total_slices, progress_bytes, self.total_size);
        
        if let Some(ref cb) = self.callback {
            cb(progress_bytes, self.total_size);
        }
    }
}

/// 123云盘流式写入器 / 123 Cloud streaming writer
pub struct Pan123Writer {
    /// API客户端 / API client
    client: Arc<ApiClient>,
    /// 父目录ID / Parent directory ID
    parent_file_id: i64,
    /// 文件名 / File name
    filename: String,
    /// 文件大小 / File size
    size: i64,
    /// 并发上传线程数 / Concurrent upload threads
    upload_thread: usize,
    /// 进度回调 / Progress callback
    progress: Option<ProgressCallback>,
    /// 上传状态 / Upload state
    state: Arc<StdMutex<UploadState>>,
}

impl Pan123Writer {
    /// 创建写入器 / Create writer
    pub fn new(
        client: Arc<ApiClient>,
        parent_file_id: i64,
        filename: &str,
        size: i64,
        upload_thread: usize,
        progress: Option<ProgressCallback>,
    ) -> Result<Self, std::io::Error> {
        let state = UploadState {
            buffer: Vec::with_capacity(size as usize),
            md5_context: md5::Context::new(),
            closed: false,
            error: None,
            upload_done: false,
            upload_result: None,
        };

        Ok(Self {
            client,
            parent_file_id,
            filename: filename.to_string(),
            size,
            upload_thread,
            progress,
            state: Arc::new(StdMutex::new(state)),
        })
    }
}

impl AsyncWrite for Pan123Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let mut state = self.state.lock().unwrap();

        // 检查错误 / Check error
        if let Some(ref err) = state.error {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone())));
        }

        // 累积到内存缓冲区并更新MD5 / Accumulate to buffer and update MD5
        state.buffer.extend_from_slice(buf);
        state.md5_context.consume(buf);
        
        // 报告缓存进度 / Report cache progress
        if let Some(ref cb) = self.progress {
            let written = state.buffer.len() as u64;
            // 缓存阶段报告0，让前端知道正在接收数据
            cb(0, self.size as u64);
            tracing::debug!("123云盘缓存: {}/{} 字节", written, self.size);
        }
        
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        // 检查上传是否已完成 / Check if upload already done
        {
            let state = self.state.lock().unwrap();
            if state.upload_done {
                return match &state.upload_result {
                    Some(Ok(())) => Poll::Ready(Ok(())),
                    Some(Err(e)) => Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.clone()))),
                    None => Poll::Ready(Ok(())),
                };
            }
        }

        // 检查是否已开始上传 / Check if upload started
        let should_start = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                false
            } else {
                state.closed = true;
                true
            }
        };

        if !should_start {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        // 获取上传参数 / Get upload parameters
        let (buffer, etag) = {
            let state = self.state.lock().unwrap();
            let etag = format!("{:x}", state.md5_context.clone().compute());
            (state.buffer.clone(), etag)
        };

        let client = self.client.clone();
        let parent_file_id = self.parent_file_id;
        let filename = self.filename.clone();
        let size = self.size;
        let upload_thread = self.upload_thread;
        let progress = self.progress.clone();
        let state_clone = self.state.clone();

        // 在独立线程中执行并发上传 / Execute concurrent upload in separate thread
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let mut state = state_clone.lock().unwrap();
                    state.upload_done = true;
                    state.upload_result = Some(Err(format!("Failed to create runtime: {}", e)));
                    return;
                }
            };

            let result = rt.block_on(async move {
                // 1. 创建上传任务 / Create upload task
                let create_resp = client.create_upload(
                    parent_file_id,
                    &filename,
                    &etag,
                    size,
                    2,
                    false,
                ).await?;

                let data = create_resp.data.ok_or("No upload create data")?;

                // 2. 检查秒传 / Check instant upload
                if data.reuse {
                    tracing::info!("123云盘秒传成功: {}", filename);
                    if let Some(ref cb) = progress {
                        cb(size as u64, size as u64);
                    }
                    return Ok(());
                }

                if data.servers.is_empty() {
                    return Err("No upload servers available".to_string());
                }

                let upload_domain = data.servers[0].clone();
                let preupload_id = data.preupload_id.clone();
                let slice_size = data.slice_size as usize;
                let access_token = client.get_config().await.access_token;

                let file_size = buffer.len();
                let total_slices = (file_size + slice_size - 1) / slice_size;
                let upload_progress = Arc::new(UploadProgress::new(
                    total_slices as u64,
                    file_size as u64,
                    progress.clone(),
                ));

                // 3. 并发分片上传 / Concurrent slice upload
                let semaphore = Arc::new(tokio::sync::Semaphore::new(upload_thread));
                let mut handles = Vec::new();
                let error_flag = Arc::new(AtomicBool::new(false));
                let buffer = Arc::new(buffer);

                for slice_no in 1..=total_slices {
                    if error_flag.load(Ordering::SeqCst) {
                        break;
                    }

                    let permit = semaphore.clone().acquire_owned().await
                        .map_err(|e| format!("Failed to acquire semaphore: {}", e))?;

                    let buffer = buffer.clone();
                    let upload_domain = upload_domain.clone();
                    let preupload_id = preupload_id.clone();
                    let access_token = access_token.clone();
                    let filename = filename.clone();
                    let upload_progress = upload_progress.clone();
                    let error_flag = error_flag.clone();

                    let handle = tokio::spawn(async move {
                        let _permit = permit;

                        // 从内存中取分片 / Get slice from memory
                        let offset = (slice_no - 1) * slice_size;
                        let end = std::cmp::min(offset + slice_size, file_size);
                        let slice_data = buffer[offset..end].to_vec();
                        let slice_md5 = calculate_md5(&slice_data);

                        // 3次重试 / 3 retries
                        let mut last_error = String::new();

                        for attempt in 0..3 {
                            if error_flag.load(Ordering::SeqCst) {
                                return Err("Cancelled due to other slice failure".to_string());
                            }

                            let part = Part::bytes(slice_data.clone())
                                .file_name(format!("{}.part{}", filename, slice_no))
                                .mime_str("application/octet-stream")
                                .map_err(|e| format!("Failed to create part: {}", e))?;

                            let form = Form::new()
                                .text("preuploadID", preupload_id.clone())
                                .text("sliceNo", slice_no.to_string())
                                .text("sliceMD5", slice_md5.clone())
                                .part("slice", part);

                            let upload_url = format!("{}/upload/v2/file/slice", upload_domain);
                            let http_client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(300))
                                .build()
                                .map_err(|e| format!("Failed to create client: {}", e))?;

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
                                            .map_err(|e| format!("Read response failed: {}", e))?;
                                        let resp_body: SliceUploadResponse = serde_json::from_str(&resp_text)
                                            .map_err(|e| format!("Parse response failed: {} - {}", e, resp_text))?;
                                        if resp_body.base.code == 0 {
                                            tracing::debug!("123云盘分片 {}/{} 上传成功", slice_no, total_slices);
                                            upload_progress.report_slice_done();
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
                        Err(format!("Slice {} failed after 3 retries: {}", slice_no, last_error))
                    });

                    handles.push(handle);
                }

                // 等待所有任务完成 / Wait for all tasks
                let mut first_error: Option<String> = None;
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
                                first_error = Some(format!("Task panicked: {}", e));
                            }
                        }
                    }
                }

                if let Some(err) = first_error {
                    return Err(err);
                }

                // 4. 完成上传 / Complete upload
                for i in 0..60 {
                    match client.upload_complete(&preupload_id).await {
                        Ok(resp) => {
                            if let Some(complete_data) = resp.data {
                                if complete_data.completed && complete_data.file_id != 0 {
                                    tracing::info!("123云盘上传完成: {} (file_id: {})", filename, complete_data.file_id);
                                    if let Some(ref cb) = progress {
                                        cb(size as u64, size as u64);
                                    }
                                    return Ok(());
                                }
                            }
                        }
                        Err(e) => {
                            if !e.contains("20103") && i >= 10 {
                                return Err(e);
                            }
                            tracing::debug!("123云盘校验中 ({}/60): {}", i + 1, e);
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }

                Err("Upload complete timeout after 60 seconds".to_string())
            });

            let mut state = state_clone.lock().unwrap();
            state.upload_done = true;
            state.upload_result = Some(result);
        });

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
