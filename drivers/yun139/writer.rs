//! 139云盘流式写入器
//! 前端传到后端占40%进度，后端上传到139占60%进度

use anyhow::{anyhow, Result};
use reqwest::Client;
use sha2::{Sha256, Digest};
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

use crate::storage::ProgressCallback;
use super::client::Yun139Client;
use super::types::*;
use super::util::*;

/// 写入器模式
#[derive(Debug, Clone, Copy)]
pub enum WriterMode {
    PersonalNew,
    Legacy,
}

/// 写入器状态
#[derive(Debug)]
enum WriterState {
    Writing,
    Uploading,
    Completed,
    Error(String),
}

/// 139云盘流式写入器
pub struct Yun139StreamWriter {
    mode: WriterMode,
    client: Client,
    size_hint: Option<u64>,
    bytes_written: u64,
    progress: Option<ProgressCallback>,
    state: WriterState,
    hasher: Sha256,
    runtime: tokio::runtime::Handle,
    parent_id: String,
    file_name: String,
    api_client: Option<Arc<Yun139Client>>,
    part_size: i64,
    upload_url: Option<String>,
    upload_task_id: Option<String>,
    temp_path: PathBuf,
    temp_writer: Option<BufWriter<File>>,
}

impl Yun139StreamWriter {
    /// 创建个人版写入器
    pub fn new_personal(
        client: Arc<Yun139Client>,
        parent_id: String,
        file_name: String,
        size_hint: Option<u64>,
        custom_part_size: i64,
        progress: Option<ProgressCallback>,
    ) -> Self {
        let size = size_hint.unwrap_or(0) as i64;
        let part_size = get_part_size(size, custom_part_size);
        
        let temp_dir = PathBuf::from("data/temps");
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_path = temp_dir.join(format!("139_{}.tmp", uuid::Uuid::new_v4()));
        
        let temp_writer = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)
            .ok()
            .map(|f| BufWriter::with_capacity(4 * 1024 * 1024, f));
        
        Self {
            mode: WriterMode::PersonalNew,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap(),
            size_hint,
            bytes_written: 0,
            progress,
            state: WriterState::Writing,
            hasher: Sha256::new(),
            runtime: tokio::runtime::Handle::current(),
            parent_id,
            file_name,
            api_client: Some(client),
            part_size,
            upload_url: None,
            upload_task_id: None,
            temp_path,
            temp_writer,
        }
    }

    /// 创建旧版写入器
    pub fn new_legacy(
        upload_url: String,
        upload_task_id: String,
        file_name: String,
        size: i64,
        custom_part_size: i64,
        progress: Option<ProgressCallback>,
    ) -> Self {
        let part_size = get_part_size(size, custom_part_size);
        
        let temp_dir = PathBuf::from("data/temps");
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_path = temp_dir.join(format!("139_{}.tmp", uuid::Uuid::new_v4()));
        
        let temp_writer = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)
            .ok()
            .map(|f| BufWriter::with_capacity(4 * 1024 * 1024, f));
        
        Self {
            mode: WriterMode::Legacy,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap(),
            size_hint: Some(size as u64),
            bytes_written: 0,
            progress,
            state: WriterState::Writing,
            hasher: Sha256::new(),
            runtime: tokio::runtime::Handle::current(),
            parent_id: String::new(),
            file_name,
            api_client: None,
            part_size,
            upload_url: Some(upload_url),
            upload_task_id: Some(upload_task_id),
            temp_path,
            temp_writer,
        }
    }

    fn get_hash(&self) -> String {
        let h = self.hasher.clone();
        format!("{:x}", h.finalize()).to_uppercase()
    }

    /// 报告进度：前端传输占40%，上传占60%
    fn report_progress(&self, upload_bytes: u64, total: u64) {
        if let Some(ref p) = self.progress {
            // 40% + (upload_bytes / total) * 60%
            let progress = (total as f64 * 0.4 + upload_bytes as f64 * 0.6) as u64;
            p(progress, total);
        }
    }

    /// 执行上传
    async fn do_upload(&mut self) -> Result<()> {
        match self.mode {
            WriterMode::PersonalNew => self.upload_personal().await,
            WriterMode::Legacy => self.upload_legacy().await,
        }
    }

    async fn upload_personal(&mut self) -> Result<()> {
        let api = self.api_client.clone().ok_or_else(|| anyhow!("No client"))?;
        let hash = self.get_hash();
        let size = self.bytes_written as i64;
        let total = size as u64;
        let part_size = self.part_size;
        
        // 计算分片
        let part_count = ((size + part_size - 1) / part_size).max(1);
        let mut parts = Vec::new();
        for i in 0..part_count.min(100) {
            let start = i * part_size;
            let len = (size - start).min(part_size);
            parts.push(PartInfo {
                part_number: i + 1,
                part_size: len,
                parallel_hash_ctx: ParallelHashCtx { part_offset: start },
            });
        }

        // 创建上传任务
        let resp = api.personal_create_upload(&self.parent_id, &self.file_name, size, &hash, parts).await?;

        // 秒传
        if resp.data.exist || resp.data.rapid_upload {
            self.report_progress(total, total);
            return Ok(());
        }

        // 上传分片
        if !resp.data.part_infos.is_empty() {
            let mut uploaded = 0u64;
            
            for part in &resp.data.part_infos {
                let idx = (part.part_number - 1) as i64;
                let start = idx * part_size;
                let end = ((idx + 1) * part_size).min(size);
                let len = (end - start) as usize;

                // 读取分片数据
                let mut file = File::open(&self.temp_path)?;
                file.seek(SeekFrom::Start(start as u64))?;
                let mut buf = vec![0u8; len];
                file.read_exact(&mut buf)?;
                drop(file);

                // 上传
                let r = self.client
                    .put(&part.upload_url)
                    .header("Content-Type", "application/octet-stream")
                    .header("Content-Length", len.to_string())
                    .body(buf)
                    .send()
                    .await?;

                if !r.status().is_success() {
                    return Err(anyhow!("Upload part {} failed: {}", part.part_number, r.status()));
                }

                uploaded += len as u64;
                self.report_progress(uploaded, total);
            }

            // 完成上传
            api.personal_complete_upload(&resp.data.file_id, &resp.data.upload_id, &hash).await?;
        }

        Ok(())
    }

    async fn upload_legacy(&mut self) -> Result<()> {
        let url = self.upload_url.clone().ok_or_else(|| anyhow!("No URL"))?;
        let task_id = self.upload_task_id.clone().ok_or_else(|| anyhow!("No task ID"))?;
        let size = self.bytes_written as i64;
        let total = size as u64;
        let part_size = self.part_size;
        let part_count = ((size + part_size - 1) / part_size).max(1);

        let mut uploaded = 0u64;
        
        for i in 0..part_count {
            let start = i * part_size;
            let end = ((i + 1) * part_size).min(size);
            let len = (end - start) as usize;

            let mut file = File::open(&self.temp_path)?;
            file.seek(SeekFrom::Start(start as u64))?;
            let mut buf = vec![0u8; len];
            file.read_exact(&mut buf)?;
            drop(file);

            let r = self.client
                .post(&url)
                .header("Content-Type", format!("text/plain;name={}", unicode_escape(&self.file_name)))
                .header("contentSize", size.to_string())
                .header("range", format!("bytes={}-{}", start, end - 1))
                .header("uploadtaskID", &task_id)
                .header("rangeType", "0")
                .body(buf)
                .send()
                .await?;

            if !r.status().is_success() {
                return Err(anyhow!("Upload failed: {}", r.status()));
            }

            uploaded += len as u64;
            self.report_progress(uploaded, total);
        }

        Ok(())
    }
}

impl AsyncWrite for Yun139StreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let WriterState::Error(ref e) = self.state {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.clone())));
        }
        if let WriterState::Completed = self.state {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "Already completed")));
        }

        // 计算哈希
        self.hasher.update(buf);
        self.bytes_written += buf.len() as u64;

        // 写入临时文件
        if let Some(ref mut w) = self.temp_writer {
            if let Err(e) = w.write_all(buf) {
                self.state = WriterState::Error(e.to_string());
                return Poll::Ready(Err(e));
            }
        } else {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "No temp file")));
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // 已完成或出错直接返回
        if let WriterState::Completed = self.state {
            return Poll::Ready(Ok(()));
        }
        if let WriterState::Error(ref e) = self.state {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.clone())));
        }

        // 刷新临时文件
        if let Some(mut w) = self.temp_writer.take() {
            let _ = w.flush();
        }

        // 标记为上传中
        self.state = WriterState::Uploading;

        // 同步执行上传（使用block_in_place避免死锁）
        let result = {
            let rt = self.runtime.clone();
            tokio::task::block_in_place(|| {
                rt.block_on(self.do_upload())
            })
        };

        // 清理临时文件
        let _ = std::fs::remove_file(&self.temp_path);

        match result {
            Ok(()) => {
                self.state = WriterState::Completed;
                Poll::Ready(Ok(()))
            }
            Err(e) => {
                let err_msg = e.to_string();
                self.state = WriterState::Error(err_msg.clone());
                tracing::error!("139 upload failed: {}", err_msg);
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err_msg)))
            }
        }
    }
}
