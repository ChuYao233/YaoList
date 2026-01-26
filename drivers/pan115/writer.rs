//! 115云盘流式写入器
//! 使用临时文件缓存，支持秒传和OSS分片上传

use anyhow::{Result, anyhow};
use reqwest::Client;
use sha1::{Sha1, Digest};
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio::sync::RwLock;

use crate::storage::ProgressCallback;
use super::client::Pan115Client;
use super::crypto::*;

const OSS_ENDPOINT: &str = "https://oss-cn-shenzhen.aliyuncs.com";
const OSS_USER_AGENT: &str = "aliyun-sdk-android/2.9.1";

enum WriterState {
    Writing,
    Uploading,
    Completed,
    Error(String),
}

pub struct Pan115StreamWriter {
    client: Arc<RwLock<Pan115Client>>,
    http: Client,
    parent_id: String,
    file_name: String,
    size_hint: i64,
    bytes_written: u64,
    progress: Option<ProgressCallback>,
    state: WriterState,
    hasher: Sha1,
    runtime: tokio::runtime::Handle,
    user_id: i64,
    app_ver: String,
    cookie: String,
    temp_path: PathBuf,
    temp_writer: Option<BufWriter<File>>,
}

impl Pan115StreamWriter {
    pub fn new(
        client: Arc<RwLock<Pan115Client>>,
        parent_id: String,
        file_name: String,
        size_hint: i64,
        user_id: i64,
        app_ver: String,
        cookie: String,
        progress: Option<ProgressCallback>,
    ) -> Result<Self> {
        let temp_dir = PathBuf::from("data/temps");
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_path = temp_dir.join(format!("115_{}.tmp", uuid::Uuid::new_v4()));
        
        let temp_writer = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)
            .ok()
            .map(|f| BufWriter::with_capacity(4 * 1024 * 1024, f));
        
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?;
        
        Ok(Self {
            client,
            http,
            parent_id,
            file_name,
            size_hint,
            bytes_written: 0,
            progress,
            state: WriterState::Writing,
            hasher: Sha1::new(),
            runtime: tokio::runtime::Handle::current(),
            user_id,
            app_ver,
            cookie,
            temp_path,
            temp_writer,
        })
    }
    
    fn get_sha1(&self) -> String {
        let h = self.hasher.clone();
        format!("{:X}", h.finalize())
    }
    
    fn report_progress(&self, uploaded: u64, total: u64) {
        if let Some(ref p) = self.progress {
            p(uploaded, total);
        }
    }
    
    async fn do_upload(&mut self) -> Result<()> {
        let size = self.bytes_written as i64;
        let total = size as u64;
        
        if size == 0 {
            return self.upload_empty_file().await;
        }
        
        let full_hash = self.get_sha1();
        
        let pre_hash = {
            let mut file = File::open(&self.temp_path)?;
            calc_pre_hash(&mut file, size)?
        };
        
        let client = self.client.read().await;
        let mut sign_key = String::new();
        let mut sign_val = String::new();
        
        loop {
            let resp = client.rapid_upload(
                size,
                &self.file_name,
                &self.parent_id,
                &pre_hash,
                &full_hash,
                &sign_key,
                &sign_val,
            ).await?;
            
            match resp.ok() {
                Ok(true) => {
                    self.report_progress(total, total);
                    return Ok(());
                }
                Ok(false) => {
                    if resp.need_sign_check() {
                        sign_key = resp.sign_key.clone();
                        
                        let range_spec = &resp.sign_check;
                        let parts: Vec<&str> = range_spec.split('-').collect();
                        if parts.len() == 2 {
                            let start: i64 = parts[0].parse().unwrap_or(0);
                            let end: i64 = parts[1].parse().unwrap_or(0);
                            let length = end - start + 1;
                            let mut file = File::open(&self.temp_path)?;
                            sign_val = calc_range_sha1(&mut file, start, length)?;
                        } else {
                            return Err(anyhow!("Invalid sign_check format: {}", range_spec));
                        }
                        continue;
                    }
                    
                    drop(client);
                    return self.upload_to_oss(&resp.bucket, &resp.object, &resp.callback).await;
                }
                Err(e) => return Err(anyhow!("Rapid upload failed: {}", e)),
            }
        }
    }
    
    async fn upload_empty_file(&self) -> Result<()> {
        let client = self.client.read().await;
        let _resp = client.rapid_upload(
            0,
            &self.file_name,
            &self.parent_id,
            "DA39A3EE5E6B4B0D3255BFEF95601890AFD80709",
            "DA39A3EE5E6B4B0D3255BFEF95601890AFD80709",
            "",
            "",
        ).await?;
        
        self.report_progress(0, 0);
        Ok(())
    }
    
    async fn upload_to_oss(&mut self, bucket: &str, object: &str, callback: &super::types::UploadCallback) -> Result<()> {
        let size = self.bytes_written as i64;
        let _total = size as u64;
        
        let client = self.client.read().await;
        let oss_token = client.get_oss_token().await?;
        drop(client);
        
        if size <= 10 * 1024 * 1024 {
            self.upload_oss_simple(bucket, object, callback, &oss_token).await
        } else {
            self.upload_oss_multipart(bucket, object, callback, &oss_token).await
        }
    }
    
    async fn upload_oss_simple(
        &mut self,
        bucket: &str,
        object: &str,
        callback: &super::types::UploadCallback,
        oss_token: &super::types::OssTokenResp,
    ) -> Result<()> {
        let size = self.bytes_written as i64;
        let total = size as u64;
        
        let mut file = File::open(&self.temp_path)?;
        let mut data = Vec::with_capacity(size as usize);
        file.read_to_end(&mut data)?;
        drop(file);
        
        let url = format!("https://{}.oss-cn-shenzhen.aliyuncs.com/{}", bucket, urlencoding::encode(object));
        
        let date = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        
        let resp = self.http
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", size.to_string())
            .header("Date", &date)
            .header("x-oss-security-token", &oss_token.security_token)
            .header("x-oss-callback", &callback.callback)
            .header("x-oss-callback-var", &callback.callback_var)
            .header("User-Agent", OSS_USER_AGENT)
            .body(data)
            .send()
            .await?;
        
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("OSS upload failed: {}", body));
        }
        
        self.report_progress(total, total);
        Ok(())
    }
    
    async fn upload_oss_multipart(
        &mut self,
        bucket: &str,
        object: &str,
        callback: &super::types::UploadCallback,
        oss_token: &super::types::OssTokenResp,
    ) -> Result<()> {
        let size = self.bytes_written as i64;
        let total = size as u64;
        
        let chunks = self.split_file(size)?;
        let _chunk_count = chunks.len();
        
        let base_url = format!("https://{}.oss-cn-shenzhen.aliyuncs.com/{}", bucket, urlencoding::encode(object));
        
        let init_resp = self.http
            .post(format!("{}?uploads", base_url))
            .header("x-oss-security-token", &oss_token.security_token)
            .header("User-Agent", OSS_USER_AGENT)
            .send()
            .await?;
        
        if !init_resp.status().is_success() {
            return Err(anyhow!("Init multipart upload failed"));
        }
        
        let init_body = init_resp.text().await?;
        let upload_id = self.parse_upload_id(&init_body)?;
        
        let mut uploaded = 0u64;
        let mut parts = Vec::new();
        
        for (i, chunk) in chunks.iter().enumerate() {
            let part_number = i + 1;
            
            let mut file = File::open(&self.temp_path)?;
            file.seek(SeekFrom::Start(chunk.offset as u64))?;
            let mut buf = vec![0u8; chunk.size as usize];
            file.read_exact(&mut buf)?;
            drop(file);
            
            let part_url = format!("{}?partNumber={}&uploadId={}", base_url, part_number, upload_id);
            
            let resp = self.http
                .put(&part_url)
                .header("Content-Length", chunk.size.to_string())
                .header("x-oss-security-token", &oss_token.security_token)
                .header("User-Agent", OSS_USER_AGENT)
                .body(buf)
                .send()
                .await?;
            
            if !resp.status().is_success() {
                return Err(anyhow!("Upload part {} failed", part_number));
            }
            
            let etag = resp.headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            
            parts.push((part_number, etag));
            
            uploaded += chunk.size as u64;
            self.report_progress(uploaded, total);
        }
        
        let complete_xml = self.build_complete_xml(&parts);
        let complete_url = format!("{}?uploadId={}", base_url, upload_id);
        
        let resp = self.http
            .post(&complete_url)
            .header("Content-Type", "application/xml")
            .header("x-oss-security-token", &oss_token.security_token)
            .header("x-oss-callback", &callback.callback)
            .header("x-oss-callback-var", &callback.callback_var)
            .header("User-Agent", OSS_USER_AGENT)
            .body(complete_xml)
            .send()
            .await?;
        
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Complete multipart upload failed: {}", body));
        }
        
        self.report_progress(total, total);
        Ok(())
    }
    
    fn split_file(&self, file_size: i64) -> Result<Vec<FileChunk>> {
        let mut chunks = Vec::new();
        
        let chunk_count = if file_size < 1024 * 1024 * 1024 {
            ((file_size / (1024 * 1024 * 1024)) + 1) * 1000
        } else if file_size < 9 * 1024 * 1024 * 1024 {
            ((file_size / (1024 * 1024 * 1024)) + 1) * 1000
        } else {
            10000
        };
        
        let chunk_count = chunk_count.min(10000).max(1) as i64;
        let chunk_size = file_size / chunk_count;
        
        if chunk_size < 100 * 1024 {
            let new_chunk_size = 100 * 1024i64;
            let new_chunk_count = (file_size + new_chunk_size - 1) / new_chunk_size;
            
            for i in 0..new_chunk_count {
                let offset = i * new_chunk_size;
                let size = if i == new_chunk_count - 1 {
                    file_size - offset
                } else {
                    new_chunk_size
                };
                chunks.push(FileChunk { offset, size });
            }
        } else {
            for i in 0..chunk_count {
                let offset = i * chunk_size;
                let size = if i == chunk_count - 1 {
                    file_size - offset
                } else {
                    chunk_size
                };
                chunks.push(FileChunk { offset, size });
            }
        }
        
        Ok(chunks)
    }
    
    fn parse_upload_id(&self, xml: &str) -> Result<String> {
        if let Some(start) = xml.find("<UploadId>") {
            if let Some(end) = xml.find("</UploadId>") {
                let id = &xml[start + 10..end];
                return Ok(id.to_string());
            }
        }
        Err(anyhow!("Failed to parse upload id"))
    }
    
    fn build_complete_xml(&self, parts: &[(usize, String)]) -> String {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CompleteMultipartUpload>\n");
        for (num, etag) in parts {
            xml.push_str(&format!("  <Part>\n    <PartNumber>{}</PartNumber>\n    <ETag>{}</ETag>\n  </Part>\n", num, etag));
        }
        xml.push_str("</CompleteMultipartUpload>");
        xml
    }
}

struct FileChunk {
    offset: i64,
    size: i64,
}

impl AsyncWrite for Pan115StreamWriter {
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
        
        self.hasher.update(buf);
        self.bytes_written += buf.len() as u64;
        
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
        if let WriterState::Completed = self.state {
            return Poll::Ready(Ok(()));
        }
        if let WriterState::Error(ref e) = self.state {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.clone())));
        }
        
        if let Some(mut w) = self.temp_writer.take() {
            let _ = w.flush();
        }
        
        self.state = WriterState::Uploading;
        
        let result = {
            let rt = self.runtime.clone();
            tokio::task::block_in_place(|| {
                rt.block_on(self.do_upload())
            })
        };
        
        let _ = std::fs::remove_file(&self.temp_path);
        
        match result {
            Ok(()) => {
                self.state = WriterState::Completed;
                Poll::Ready(Ok(()))
            }
            Err(e) => {
                let err_msg = e.to_string();
                self.state = WriterState::Error(err_msg.clone());
                tracing::error!("115 upload failed: {}", err_msg);
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err_msg)))
            }
        }
    }
}
