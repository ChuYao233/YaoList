//! 阿里云盘 Open 流式读写器实现

use anyhow::{anyhow, Result};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::future::Future;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, oneshot};

use super::client::AliyunOpenClient;
use super::types::*;

const CHUNK_SIZE: usize = 10 * 1024 * 1024; // 10MB 每片
const MAX_BUFFER_CHUNKS: usize = 1; // 只缓存 1 片，严格控制内存

/// 阿里云盘上传器，实现 AsyncWrite
/// 严格遵循流式处理，不将整个文件加载到内存
pub struct AliyunOpenWriter {
    client: Arc<AliyunOpenClient>,
    drive_id: String,
    parent_file_id: String,
    file_name: String,
    file_size: u64,
    
    // 分片上传相关
    file_id: Option<String>,
    upload_id: Option<String>,
    part_info_list: Vec<PartInfo>,
    
    // 单片缓冲，严格控制内存使用
    buffer: BytesMut,
    part_number: i32,
    uploaded_size: u64,
    
    // 同步上传，避免并发占用内存
    http_client: Client,
}

struct UploadTask {
    part_number: i32,
    data: Bytes,
    upload_url: String,
    response_tx: oneshot::Sender<Result<()>>,
}

impl AliyunOpenWriter {
    pub fn new(
        client: Arc<AliyunOpenClient>,
        drive_id: String,
        parent_file_id: String,
        file_name: String,
        file_size: u64,
    ) -> Result<Self> {
        Ok(Self {
            client,
            drive_id,
            parent_file_id,
            file_name,
            file_size,
            file_id: None,
            upload_id: None,
            part_info_list: Vec::new(),
            buffer: BytesMut::with_capacity(CHUNK_SIZE),
            part_number: 1,
            uploaded_size: 0,
            http_client: Client::new(),
        })
    }

    /// 上传单个分片（同步方式，严格控制内存）
    async fn upload_part(&self, upload_url: &str, data: Bytes) -> Result<()> {
        let resp = self.http_client
            .put(upload_url)
            .body(data)
            .send()
            .await?;

        if !resp.status().is_success() && resp.status().as_u16() != 409 {
            return Err(anyhow!("上传分片失败: {}", resp.status()));
        }

        Ok(())
    }

    /// 初始化上传任务
    async fn init_upload(&mut self) -> Result<()> {
        if self.file_id.is_some() {
            return Ok(());
        }

        // 计算分片数量
        let part_count = ((self.file_size + CHUNK_SIZE as u64 - 1) / CHUNK_SIZE as u64) as i32;
        let part_info_list: Vec<Value> = (1..=part_count)
            .map(|i| serde_json::json!({"part_number": i}))
            .collect();

        let body = serde_json::json!({
            "drive_id": self.drive_id,
            "parent_file_id": self.parent_file_id,
            "name": self.file_name,
            "type": "file",
            "check_name_mode": "ignore",
            "size": self.file_size,
            "part_info_list": part_info_list,
        });

        let resp: CreateFileResponse = self.client
            .post("/adrive/v1.0/openFile/create", body)
            .await?;

        self.file_id = Some(resp.file_id);
        self.upload_id = resp.upload_id;
        self.part_info_list = resp.part_info_list.unwrap_or_default();

        Ok(())
    }

    /// 上传当前缓冲区（同步方式，立即释放内存）
    async fn flush_buffer(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        self.init_upload().await?;

        // 获取当前分片的上传 URL
        let part_info = self.part_info_list
            .get((self.part_number - 1) as usize)
            .ok_or_else(|| anyhow!("找不到分片 {} 的上传信息", self.part_number))?;

        let data = self.buffer.split().freeze();
        let data_len = data.len();

        // 直接同步上传，立即释放内存
        self.upload_part(&part_info.upload_url, data).await?;

        self.part_number += 1;
        self.uploaded_size += data_len as u64;

        Ok(())
    }

    /// 完成上传
    async fn complete_upload(&mut self) -> Result<()> {
        if let Some(file_id) = &self.file_id {
            if let Some(upload_id) = &self.upload_id {
                let body = serde_json::json!({
                    "drive_id": self.drive_id,
                    "file_id": file_id,
                    "upload_id": upload_id,
                });

                let _: AliyunFile = self.client
                    .post("/adrive/v1.0/openFile/complete", body)
                    .await?;
            }
        }

        Ok(())
    }
}

impl AsyncWrite for AliyunOpenWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let remaining_capacity = CHUNK_SIZE - self.buffer.len();
        let write_size = buf.len().min(remaining_capacity);
        
        self.buffer.extend_from_slice(&buf[..write_size]);

        // 如果缓冲区满了，需要上传
        if self.buffer.len() >= CHUNK_SIZE {
            // 这里我们返回 Pending，让调用者稍后重试
            // 实际的上传会在 poll_flush 中处理
            return Poll::Pending;
        }

        Poll::Ready(Ok(write_size))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        if self.buffer.len() >= CHUNK_SIZE {
            let fut = self.flush_buffer();
            tokio::pin!(fut);
            
            match fut.poll(cx) {
                Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
                Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        // 先上传剩余数据
        if !self.buffer.is_empty() {
            let fut = self.flush_buffer();
            tokio::pin!(fut);
            
            match fut.poll(cx) {
                Poll::Ready(Ok(())) => {},
                Poll::Ready(Err(e)) => return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
                Poll::Pending => return Poll::Pending,
            }
        }

        // 完成上传
        let fut = self.complete_upload();
        tokio::pin!(fut);
        
        match fut.poll(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// 阿里云盘下载器，实现 AsyncRead
pub struct AliyunOpenReader {
    inner: Pin<Box<dyn AsyncRead + Send + Unpin>>,
}

impl AliyunOpenReader {
    pub async fn new(url: &str, range: Option<(u64, u64)>) -> Result<Self> {
        let client = Client::new();
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

impl AsyncRead for AliyunOpenReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}
