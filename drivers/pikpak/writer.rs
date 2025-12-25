//! PikPak streaming writer / PikPak流式写入器
//!
//! Architecture: Driver only provides Writer primitive, Core controls chunking/progress
//! 架构: 驱动只提供Writer原语，Core控制分片/进度

use anyhow::{anyhow, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha1::Sha1;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

use super::types::*;
use crate::storage::ProgressCallback;

/// OSS upload options / OSS上传选项
const OSS_SECURITY_TOKEN_HEADER: &str = "x-oss-security-token";
const OSS_USER_AGENT: &str = "aliyun-sdk-android/2.9.13(Linux/Android 14/M2004j7ac;UKQ1.231108.001)";
const CHUNK_SIZE: usize = 10 * 1024 * 1024; // 10MB per chunk

type HmacSha1 = Hmac<Sha1>;

/// PikPak streaming writer state / PikPak流式写入器状态
enum WriterState {
    /// Buffering initial data / 缓冲初始数据
    Buffering,
    /// Uploading to OSS / 上传到OSS
    Uploading,
    /// Upload completed / 上传完成
    Completed,
    /// Error occurred / 发生错误
    Error(String),
}

/// PikPak streaming writer / PikPak流式写入器
/// 
/// Implements AsyncWrite for streaming upload without loading entire file into memory
/// 实现AsyncWrite接口用于流式上传，不将整个文件加载到内存
pub struct PikPakStreamWriter {
    /// S3/OSS params from upload init / 上传初始化返回的S3/OSS参数
    params: S3Params,
    /// HTTP client / HTTP客户端
    client: Client,
    /// Current state / 当前状态
    state: WriterState,
    /// Write buffer (only for current chunk) / 写入缓冲区(仅用于当前分片)
    buffer: BytesMut,
    /// Total size hint / 总大小提示
    size_hint: Option<u64>,
    /// Bytes written so far / 已写入字节数
    bytes_written: u64,
    /// Progress callback / 进度回调
    progress: Option<ProgressCallback>,
    /// Upload ID for multipart / 分片上传ID
    upload_id: Option<String>,
    /// Uploaded parts for multipart / 已上传分片列表
    uploaded_parts: Vec<UploadedPart>,
    /// Current part number / 当前分片号
    current_part: i32,
    /// Runtime handle for spawning tasks / 运行时句柄
    runtime: tokio::runtime::Handle,
}

/// Uploaded part info / 已上传分片信息
#[derive(Debug, Clone)]
struct UploadedPart {
    part_number: i32,
    etag: String,
}

impl PikPakStreamWriter {
    /// Create new writer / 创建新写入器
    pub fn new(
        params: S3Params,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Self {
        // Fix endpoint for Android platform
        let mut params = params;
        if params.endpoint.contains("vip-lixian") {
            params.endpoint = "mypikpak.net".to_string();
        }

        Self {
            params,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap(),
            state: WriterState::Buffering,
            buffer: BytesMut::with_capacity(CHUNK_SIZE),
            size_hint,
            bytes_written: 0,
            progress,
            upload_id: None,
            uploaded_parts: Vec::new(),
            current_part: 1,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    /// Create writer for empty file (uploads 1 byte placeholder) / 创建空文件写入器（上传1字节占位）
    pub fn new_empty_file(params: S3Params) -> Self {
        let mut params = params;
        if params.endpoint.contains("vip-lixian") {
            params.endpoint = "mypikpak.net".to_string();
        }

        let mut writer = Self {
            params,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap(),
            state: WriterState::Buffering,
            buffer: BytesMut::with_capacity(1),
            size_hint: Some(1),
            bytes_written: 0,
            progress: None,
            upload_id: None,
            uploaded_parts: Vec::new(),
            current_part: 1,
            runtime: tokio::runtime::Handle::current(),
        };
        // 预填充1字节
        writer.buffer.extend_from_slice(&[0u8]);
        writer.bytes_written = 1;
        writer
    }

    /// Build OSS URL / 构建OSS URL
    fn build_oss_url(&self) -> String {
        format!(
            "https://{}.{}/{}",
            self.params.bucket,
            self.params.endpoint,
            self.params.key
        )
    }

    /// Generate OSS signature / 生成OSS签名
    fn sign_request(&self, method: &str, content_type: &str, date: &str, resource: &str) -> String {
        // OSS签名格式: base64(hmac-sha1(AccessKeySecret, StringToSign))
        // StringToSign = VERB + "\n" + Content-MD5 + "\n" + Content-Type + "\n" + Date + "\n" + CanonicalizedOSSHeaders + CanonicalizedResource
        let canonicalized_oss_headers = format!("{}:{}", OSS_SECURITY_TOKEN_HEADER, self.params.security_token);
        let string_to_sign = format!(
            "{}\n\n{}\n{}\n{}\n/{}{}",
            method,
            content_type,
            date,
            canonicalized_oss_headers,
            self.params.bucket,
            resource
        );
        
        let mut mac = HmacSha1::new_from_slice(self.params.access_key_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(string_to_sign.as_bytes());
        let result = mac.finalize();
        BASE64.encode(result.into_bytes())
    }

    /// Get GMT date string / 获取GMT日期字符串
    fn get_gmt_date() -> String {
        Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }

    /// Upload single chunk (for small files < 10MB) / 上传单个块(小于10MB的文件)
    async fn upload_single(&self, data: Bytes) -> Result<()> {
        let url = self.build_oss_url();
        let date = Self::get_gmt_date();
        let content_type = "application/octet-stream";
        let resource = format!("/{}", self.params.key);
        let signature = self.sign_request("PUT", content_type, &date, &resource);
        let auth = format!("OSS {}:{}", self.params.access_key_id, signature);
        
        let resp = self.client
            .put(&url)
            .header("User-Agent", OSS_USER_AGENT)
            .header("Date", &date)
            .header("Authorization", &auth)
            .header(OSS_SECURITY_TOKEN_HEADER, &self.params.security_token)
            .header("Content-Type", content_type)
            .body(data)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Upload failed / 上传失败: {} - {}", status, text));
        }

        Ok(())
    }

    /// Initialize multipart upload / 初始化分片上传
    async fn init_multipart(&mut self) -> Result<()> {
        let url = format!("{}?uploads", self.build_oss_url());
        let date = Self::get_gmt_date();
        let resource = format!("/{}?uploads", self.params.key);
        let signature = self.sign_request("POST", "", &date, &resource);
        let auth = format!("OSS {}:{}", self.params.access_key_id, signature);
        
        let resp = self.client
            .post(&url)
            .header("User-Agent", OSS_USER_AGENT)
            .header("Date", &date)
            .header("Authorization", &auth)
            .header(OSS_SECURITY_TOKEN_HEADER, &self.params.security_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Init multipart failed / 初始化分片上传失败: {} - {}", status, text));
        }

        let text = resp.text().await?;
        
        // Parse XML response to get UploadId
        if let Some(start) = text.find("<UploadId>") {
            if let Some(end) = text.find("</UploadId>") {
                let upload_id = &text[start + 10..end];
                self.upload_id = Some(upload_id.to_string());
                return Ok(());
            }
        }

        Err(anyhow!("Failed to parse UploadId / 解析UploadId失败: {}", text))
    }

    /// Upload a part / 上传一个分片
    async fn upload_part(&self, part_number: i32, data: Bytes) -> Result<UploadedPart> {
        let upload_id = self.upload_id.as_ref()
            .ok_or_else(|| anyhow!("No upload ID / 没有上传ID"))?;

        let url = format!(
            "{}?partNumber={}&uploadId={}",
            self.build_oss_url(),
            part_number,
            upload_id
        );
        
        let date = Self::get_gmt_date();
        let content_type = "application/octet-stream";
        let resource = format!("/{}?partNumber={}&uploadId={}", self.params.key, part_number, upload_id);
        let signature = self.sign_request("PUT", content_type, &date, &resource);
        let auth = format!("OSS {}:{}", self.params.access_key_id, signature);

        let resp = self.client
            .put(&url)
            .header("User-Agent", OSS_USER_AGENT)
            .header("Date", &date)
            .header("Authorization", &auth)
            .header(OSS_SECURITY_TOKEN_HEADER, &self.params.security_token)
            .header("Content-Type", content_type)
            .body(data)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Upload part {} failed / 上传分片{}失败: {} - {}", part_number, part_number, status, text));
        }

        let etag = resp
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim_matches('"')
            .to_string();

        Ok(UploadedPart { part_number, etag })
    }

    /// Complete multipart upload / 完成分片上传
    async fn complete_multipart(&self) -> Result<()> {
        let upload_id = self.upload_id.as_ref()
            .ok_or_else(|| anyhow!("No upload ID / 没有上传ID"))?;

        let url = format!("{}?uploadId={}", self.build_oss_url(), upload_id);
        let date = Self::get_gmt_date();
        let content_type = "application/xml";
        let resource = format!("/{}?uploadId={}", self.params.key, upload_id);
        let signature = self.sign_request("POST", content_type, &date, &resource);
        let auth = format!("OSS {}:{}", self.params.access_key_id, signature);

        // Build XML body
        let mut xml = String::from("<CompleteMultipartUpload>");
        for part in &self.uploaded_parts {
            xml.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                part.part_number, part.etag
            ));
        }
        xml.push_str("</CompleteMultipartUpload>");

        let resp = self.client
            .post(&url)
            .header("User-Agent", OSS_USER_AGENT)
            .header("Date", &date)
            .header("Authorization", &auth)
            .header(OSS_SECURITY_TOKEN_HEADER, &self.params.security_token)
            .header("Content-Type", content_type)
            .body(xml)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            // EOF error from XML unmarshal is expected, actual upload succeeded
            if !text.contains("EOF") {
                return Err(anyhow!("Complete multipart failed / 完成分片上传失败: {} - {}", status, text));
            }
        }

        Ok(())
    }

    /// Abort multipart upload / 取消分片上传
    async fn abort_multipart(&self) -> Result<()> {
        if let Some(ref upload_id) = self.upload_id {
            let url = format!("{}?uploadId={}", self.build_oss_url(), upload_id);
            let date = Self::get_gmt_date();
            let resource = format!("/{}?uploadId={}", self.params.key, upload_id);
            let signature = self.sign_request("DELETE", "", &date, &resource);
            let auth = format!("OSS {}:{}", self.params.access_key_id, signature);
            
            let _ = self.client
                .delete(&url)
                .header("User-Agent", OSS_USER_AGENT)
                .header("Date", &date)
                .header("Authorization", &auth)
                .header(OSS_SECURITY_TOKEN_HEADER, &self.params.security_token)
                .send()
                .await;
        }
        Ok(())
    }

    /// Flush current buffer as a part / 将当前缓冲区作为分片刷新
    async fn flush_buffer_as_part(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let data = self.buffer.split().freeze();
        let part = self.upload_part(self.current_part, data).await?;
        self.uploaded_parts.push(part);
        self.current_part += 1;

        // Report progress
        if let (Some(progress), Some(total)) = (&self.progress, self.size_hint) {
            progress(self.bytes_written, total);
        }

        Ok(())
    }

    /// Finalize upload / 完成上传
    pub async fn finalize(&mut self) -> Result<()> {
        match &self.state {
            WriterState::Completed => return Ok(()),
            WriterState::Error(e) => return Err(anyhow!("{}", e)),
            _ => {}
        }

        // Determine upload mode based on total size
        let total_size = self.size_hint.unwrap_or(self.bytes_written);
        
        if total_size <= CHUNK_SIZE as u64 {
            // Small file: single upload
            let data = self.buffer.split().freeze();
            self.upload_single(data).await?;
        } else {
            // Large file: multipart upload
            if self.upload_id.is_none() {
                self.init_multipart().await?;
            }

            // Upload remaining buffer
            if !self.buffer.is_empty() {
                self.flush_buffer_as_part().await?;
            }

            // Complete multipart
            self.complete_multipart().await?;
        }

        self.state = WriterState::Completed;

        // Final progress callback
        if let Some(progress) = &self.progress {
            let total = self.size_hint.unwrap_or(self.bytes_written);
            progress(total, total);
        }

        Ok(())
    }
}

impl AsyncWrite for PikPakStreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &self.state {
            WriterState::Error(e) => {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.clone(),
                )));
            }
            WriterState::Completed => {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Writer already completed / 写入器已完成",
                )));
            }
            _ => {}
        }

        // Append to buffer
        self.buffer.extend_from_slice(buf);
        self.bytes_written += buf.len() as u64;

        // Check if we need to start multipart upload
        let total_size = self.size_hint.unwrap_or(u64::MAX);
        
        if total_size > CHUNK_SIZE as u64 && self.buffer.len() >= CHUNK_SIZE {
            // Need to use multipart upload
            let this = self.get_mut();
            let runtime = this.runtime.clone();
            
            // 使用 block_in_place 允许在tokio运行时中运行阻塞代码
            let result = tokio::task::block_in_place(|| {
                runtime.block_on(async {
                    // Initialize multipart if needed
                    if this.upload_id.is_none() {
                        this.init_multipart().await?;
                    }
                    // Upload the chunk
                    this.flush_buffer_as_part().await
                })
            });
            
            if let Err(e) = result {
                this.state = WriterState::Error(e.to_string());
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            }
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let runtime = this.runtime.clone();
        
        // 使用 block_in_place 允许在tokio运行时中运行阻塞代码
        let result = tokio::task::block_in_place(|| {
            runtime.block_on(this.finalize())
        });
        
        match result {
            Ok(_) => Poll::Ready(Ok(())),
            Err(e) => {
                // Try to abort multipart on error
                let _ = tokio::task::block_in_place(|| {
                    runtime.block_on(this.abort_multipart())
                });
                Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )))
            }
        }
    }
}

impl Drop for PikPakStreamWriter {
    fn drop(&mut self) {
        // Attempt to abort incomplete multipart uploads
        if self.upload_id.is_some() && !matches!(self.state, WriterState::Completed) {
            let _ = self.runtime.block_on(self.abort_multipart());
        }
    }
}
