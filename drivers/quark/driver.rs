//! 夸克网盘驱动实现
//! 
//! 支持代理模式下载（夸克CDN需要headers验证）

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::StreamExt;
use md5;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Sha1, Digest as Sha1Digest};
use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::RwLock;
use chrono::Utc;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, SpaceInfo,
    StorageDriver,
};

/// 夸克网盘配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarkConfig {
    /// Cookie认证
    pub cookie: String,
    /// 根目录ID（默认为0表示根目录）
    #[serde(default = "default_root_folder_id")]
    pub root_folder_id: String,
    /// 是否在前台显示空间信息
    #[serde(default = "default_true")]
    pub show_space_info: bool,
}

fn default_root_folder_id() -> String {
    "0".to_string()
}

fn default_true() -> bool {
    true
}

/// 夸克API响应基础结构
#[derive(Debug, Deserialize)]
struct QuarkResponse<T> {
    status: i32,
    code: i32,
    message: String,
    data: Option<T>,
    metadata: Option<Value>,
}

/// 夸克文件信息
#[derive(Debug, Deserialize, Clone)]
struct QuarkFile {
    fid: String,
    file_name: String,
    size: i64,
    file: bool,
    updated_at: i64,
    #[serde(default)]
    pdir_fid: Option<String>,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
struct QuarkListData {
    list: Vec<QuarkFile>,
}

/// 下载响应
#[derive(Debug, Deserialize)]
struct QuarkDownloadItem {
    download_url: String,
}

/// 空间信息响应
#[derive(Debug, Deserialize)]
struct QuarkMemberData {
    use_capacity: u64,
    total_capacity: u64,
}

/// 上传预处理响应
#[derive(Debug, Deserialize)]
struct QuarkUploadPreData {
    task_id: String,
    finish: bool,
    upload_id: String,
    obj_key: String,
    upload_url: String,
    fid: String,
    bucket: String,
    callback: QuarkCallback,
    auth_info: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct QuarkCallback {
    #[serde(rename = "callbackUrl")]
    callback_url: String,
    #[serde(rename = "callbackBody")]
    callback_body: String,
}

/// 上传认证响应
#[derive(Debug, Deserialize)]
struct QuarkUploadAuthData {
    auth_key: String,
}

/// 哈希检查响应
#[derive(Debug, Deserialize)]
struct QuarkHashData {
    #[serde(default)]
    finish: bool,
}

/// 夸克网盘驱动
pub struct QuarkDriver {
    config: QuarkConfig,
    client: Client,
    /// 路径到fid的缓存
    path_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl QuarkDriver {
    const API_BASE: &'static str = "https://drive-pc.quark.cn/1/clouddrive";
    const REFERER: &'static str = "https://pan.quark.cn";
    const USER_AGENT: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) quark-cloud-drive/2.5.20 Chrome/100.0.4896.160 Electron/18.3.5.4-b478491100 Safari/537.36 Channel/pckk_other_ch";

    pub fn new(config: QuarkConfig) -> Self {
        Self {
            config,
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 发送API请求
    async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        method: Method,
        params: Option<&[(&str, &str)]>,
        body: Option<Value>,
    ) -> Result<T> {
        let url = format!("{}{}", Self::API_BASE, path);
        
        let mut req = self.client
            .request(method, &url)
            .header("Cookie", &self.config.cookie)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", Self::REFERER)
            .header("User-Agent", Self::USER_AGENT)
            .query(&[("pr", "ucpro"), ("fr", "pc")]);

        if let Some(p) = params {
            req = req.query(p);
        }

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await?;
        let text = resp.text().await?;
        
        let quark_resp: QuarkResponse<T> = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析响应失败: {} - {}", e, text))?;

        if quark_resp.code != 0 {
            return Err(anyhow!("夸克API错误: {} (code={})", quark_resp.message, quark_resp.code));
        }

        quark_resp.data.ok_or_else(|| anyhow!("响应中没有数据"))
    }

    /// 通过路径获取fid
    async fn get_fid_by_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        
        if path.is_empty() {
            return Ok(self.config.root_folder_id.clone());
        }

        // 检查缓存
        {
            let cache = self.path_cache.read().await;
            if let Some(fid) = cache.get(path) {
                return Ok(fid.clone());
            }
        }

        // 逐级查找
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_fid = self.config.root_folder_id.clone();
        let mut current_path = String::new();

        for part in parts {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);

            // 检查缓存
            {
                let cache = self.path_cache.read().await;
                if let Some(fid) = cache.get(&current_path) {
                    current_fid = fid.clone();
                    continue;
                }
            }

            // 查询当前目录
            let params = [
                ("pdir_fid", current_fid.as_str()),
                ("_size", "200"),
                ("_page", "1"),
                ("_fetch_total", "1"),
                ("_sort", "file_type:asc,file_name:asc"),
            ];

            let data: QuarkListData = self.request("/file/sort", Method::GET, Some(&params), None).await?;

            // 查找匹配的文件
            if let Some(file) = data.list.into_iter().find(|f| f.file_name == part) {
                current_fid = file.fid;
                // 更新缓存
                self.path_cache.write().await.insert(current_path.clone(), current_fid.clone());
            } else {
                return Err(anyhow!("路径不存在: /{}", current_path));
            }
        }

        Ok(current_fid)
    }

    /// 获取下载链接
    async fn get_download_url(&self, fid: &str) -> Result<String> {
        let body = json!({
            "fids": [fid]
        });

        let data: Vec<QuarkDownloadItem> = self.request("/file/download", Method::POST, None, Some(body)).await?;

        if data.is_empty() {
            return Err(anyhow!("获取下载链接失败"));
        }

        Ok(data[0].download_url.clone())
    }

    /// 列出目录内容
    async fn list_files(&self, fid: &str, path: &str) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let mut page = 1;
        let size = 100;

        loop {
            let params = [
                ("pdir_fid", fid),
                ("_size", &size.to_string()),
                ("_page", &page.to_string()),
                ("_fetch_total", "1"),
                ("_sort", "file_type:asc,file_name:asc"),
            ];

            let text = {
                let url = format!("{}/file/sort", Self::API_BASE);
                let resp = self.client
                    .get(&url)
                    .header("Cookie", &self.config.cookie)
                    .header("Accept", "application/json, text/plain, */*")
                    .header("Referer", Self::REFERER)
                    .header("User-Agent", Self::USER_AGENT)
                    .query(&[("pr", "ucpro"), ("fr", "pc")])
                    .query(&params)
                    .send()
                    .await?;
                resp.text().await?
            };

            let quark_resp: QuarkResponse<QuarkListData> = serde_json::from_str(&text)?;
            
            if quark_resp.code != 0 {
                return Err(anyhow!("列表失败: {}", quark_resp.message));
            }

            let data = quark_resp.data.ok_or_else(|| anyhow!("无数据"))?;
            
            for file in &data.list {
                let file_path = if path.is_empty() || path == "/" {
                    format!("/{}", file.file_name)
                } else {
                    format!("{}/{}", path.trim_end_matches('/'), file.file_name)
                };

                // 更新缓存
                self.path_cache.write().await.insert(
                    file_path.trim_start_matches('/').to_string(),
                    file.fid.clone(),
                );

                entries.push(Entry {
                    name: file.file_name.clone(),
                    path: file_path,
                    size: file.size as u64,
                    is_dir: !file.file,
                    modified: chrono::DateTime::from_timestamp_millis(file.updated_at)
                        .map(|dt| dt.to_rfc3339()),
                });
            }

            // 检查是否还有更多
            let total = quark_resp.metadata
                .and_then(|m| m.get("_total").and_then(|v| v.as_i64()))
                .unwrap_or(0) as usize;

            if total == 0 || page * size >= total {
                break;
            }
            page += 1;
        }

        Ok(entries)
    }
}

/// 夸克下载读取器（使用tokio_util的ReaderStream包装）
pub struct QuarkReader {
    inner: Pin<Box<dyn AsyncRead + Send + Unpin>>,
}

impl QuarkReader {
    async fn new(url: &str, cookie: &str, range: Option<(u64, u64)>) -> Result<Self> {
        let client = Client::new();
        let mut req = client
            .get(url)
            .header("Cookie", cookie)
            .header("Referer", QuarkDriver::REFERER)
            .header("User-Agent", QuarkDriver::USER_AGENT);

        if let Some((start, end)) = range {
            req = req.header("Range", format!("bytes={}-{}", start, end));
        }

        let response = req.send().await?;
        
        if !response.status().is_success() && response.status().as_u16() != 206 {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }

        // 使用StreamReader包装响应字节流
        let stream = response.bytes_stream();
        let stream = stream.map(|result| {
            result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        });
        let reader = tokio_util::io::StreamReader::new(stream);

        Ok(Self {
            inner: Box::pin(reader),
        })
    }
}

impl AsyncRead for QuarkReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

/// 上传预处理完整响应
#[derive(Debug, Deserialize)]
struct QuarkUploadPreResponse {
    status: i32,
    code: i32,
    message: String,
    data: Option<QuarkUploadPreData>,
    metadata: Option<QuarkUploadMetadata>,
}

#[derive(Debug, Deserialize, Clone)]
struct QuarkUploadMetadata {
    #[serde(default = "default_part_size")]
    part_size: usize,
}

fn default_part_size() -> usize {
    10 * 1024 * 1024
}

/// 夸克上传写入器
pub struct QuarkWriter {
    buffer: Vec<u8>,
    path: String,
    client: Client,
    cookie: String,
    root_folder_id: String,
    closed: bool,
}

impl QuarkWriter {
    const API_BASE: &'static str = "https://drive-pc.quark.cn/1/clouddrive";
    const REFERER: &'static str = "https://pan.quark.cn";
    const USER_AGENT: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

    fn new(path: String, cookie: String, root_folder_id: String) -> Self {
        Self {
            buffer: Vec::new(),
            path,
            client: Client::new(),
            cookie,
            root_folder_id,
            closed: false,
        }
    }

    async fn api_request<T: for<'de> Deserialize<'de>>(&self, path: &str, body: Option<Value>) -> std::io::Result<T> {
        let url = format!("{}{}?pr=ucpro&fr=pc", Self::API_BASE, path);
        let mut req = self.client.post(&url)
            .header("Cookie", &self.cookie)
            .header("Referer", Self::REFERER)
            .header("User-Agent", Self::USER_AGENT);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let text = resp.text().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}: {}", e, text)))
    }

    async fn get_parent_fid(&self) -> std::io::Result<String> {
        let path = self.path.trim_matches('/');
        let parent = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        if parent.is_empty() { return Ok(self.root_folder_id.clone()); }
        
        let parts: Vec<&str> = parent.split('/').filter(|s| !s.is_empty()).collect();
        let mut fid = self.root_folder_id.clone();
        
        for part in parts {
            let url = format!("{}/file/sort?pr=ucpro&fr=pc&pdir_fid={}&_size=200&_page=1", Self::API_BASE, fid);
            let resp = self.client.get(&url)
                .header("Cookie", &self.cookie)
                .header("Referer", Self::REFERER)
                .send().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            let text = resp.text().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            let data: QuarkResponse<QuarkListData> = serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            
            if let Some(list) = data.data {
                if let Some(f) = list.list.into_iter().find(|f| f.file_name == part) {
                    fid = f.fid;
                } else {
                    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("目录不存在: {}", part)));
                }
            }
        }
        Ok(fid)
    }

    async fn do_upload(&self) -> std::io::Result<()> {
        tracing::info!("夸克上传: path={}, size={}", self.path, self.buffer.len());
        
        let file_name = std::path::Path::new(&self.path).file_name().and_then(|n| n.to_str())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "无效文件名"))?.to_string();
        let parent_fid = self.get_parent_fid().await?;
        
        // 计算哈希
        let md5_hash = format!("{:x}", md5::compute(&self.buffer));
        let sha1_hash = { let mut h = Sha1::new(); h.update(&self.buffer); format!("{:x}", h.finalize()) };
        
        // 预上传
        let now = Utc::now().timestamp_millis();
        let pre_body = json!({
            "ccp_hash_update": true, "dir_name": "", "file_name": file_name,
            "format_type": mime_guess::from_path(&self.path).first_or_octet_stream().to_string(),
            "l_created_at": now, "l_updated_at": now, "pdir_fid": parent_fid, "size": self.buffer.len()
        });
        
        let pre_resp: QuarkUploadPreResponse = self.api_request("/file/upload/pre", Some(pre_body)).await?;
        if pre_resp.code != 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("预上传失败: {}", pre_resp.message)));
        }
        let pre = pre_resp.data.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "预上传响应为空"))?;
        
        if pre.finish { tracing::info!("秒传成功"); return Ok(()); }
        
        // 检查哈希秒传
        let hash_body = json!({ "md5": md5_hash, "sha1": sha1_hash, "task_id": pre.task_id });
        let hash_resp: QuarkResponse<QuarkHashData> = self.api_request("/file/update/hash", Some(hash_body)).await?;
        if hash_resp.data.map(|d| d.finish).unwrap_or(false) { tracing::info!("哈希秒传成功"); return Ok(()); }
        
        // 分片上传
        let part_size = pre_resp.metadata.map(|m| m.part_size).unwrap_or(10485760);
        let total = (self.buffer.len() + part_size - 1) / part_size;
        let mut etags = Vec::new();
        let mime = mime_guess::from_path(&self.path).first_or_octet_stream().to_string();
        
        for i in 1..=total {
            let start = (i - 1) * part_size;
            let end = std::cmp::min(i * part_size, self.buffer.len());
            let etag = self.upload_part(&pre, &mime, i, &self.buffer[start..end]).await?;
            etags.push(etag);
            tracing::debug!("分片 {}/{}", i, total);
        }
        
        // 提交
        self.upload_commit(&pre, &etags).await?;
        self.upload_finish(&pre).await?;
        tracing::info!("上传完成: {}", self.path);
        Ok(())
    }

    async fn upload_part(&self, pre: &QuarkUploadPreData, mime: &str, num: usize, data: &[u8]) -> std::io::Result<String> {
        let time = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let auth_meta = format!("PUT\n\n{}\n{}\nx-oss-date:{}\nx-oss-user-agent:aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit\n/{}/{}?partNumber={}&uploadId={}",
            mime, time, time, pre.bucket, pre.obj_key, num, pre.upload_id);
        
        let auth_resp: QuarkResponse<QuarkUploadAuthData> = self.api_request("/file/upload/auth", 
            Some(json!({ "auth_info": pre.auth_info, "auth_meta": auth_meta, "task_id": pre.task_id }))).await?;
        let auth = auth_resp.data.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "认证失败"))?;
        
        let url = format!("https://{}.{}/{}?partNumber={}&uploadId={}", pre.bucket, &pre.upload_url[7..], pre.obj_key, num, pre.upload_id);
        let resp = self.client.put(&url)
            .header("Authorization", &auth.auth_key)
            .header("Content-Type", mime)
            .header("Referer", Self::REFERER)
            .header("x-oss-date", &time)
            .header("x-oss-user-agent", "aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit")
            .body(data.to_vec()).send().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        
        if !resp.status().is_success() {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("分片上传失败: {}", resp.status())));
        }
        Ok(resp.headers().get("ETag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string())
    }

    async fn upload_commit(&self, pre: &QuarkUploadPreData, etags: &[String]) -> std::io::Result<()> {
        let time = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CompleteMultipartUpload>\n");
        for (i, e) in etags.iter().enumerate() {
            xml.push_str(&format!("<Part>\n<PartNumber>{}</PartNumber>\n<ETag>{}</ETag>\n</Part>\n", i+1, e));
        }
        xml.push_str("</CompleteMultipartUpload>");
        
        let md5 = BASE64.encode(md5::compute(xml.as_bytes()).0);
        let cb = BASE64.encode(serde_json::to_string(&pre.callback).unwrap_or_default().as_bytes());
        let auth_meta = format!("POST\n{}\napplication/xml\n{}\nx-oss-callback:{}\nx-oss-date:{}\nx-oss-user-agent:aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit\n/{}/{}?uploadId={}",
            md5, time, cb, time, pre.bucket, pre.obj_key, pre.upload_id);
        
        let auth_resp: QuarkResponse<QuarkUploadAuthData> = self.api_request("/file/upload/auth",
            Some(json!({ "auth_info": pre.auth_info, "auth_meta": auth_meta, "task_id": pre.task_id }))).await?;
        let auth = auth_resp.data.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "认证失败"))?;
        
        let url = format!("https://{}.{}/{}?uploadId={}", pre.bucket, &pre.upload_url[7..], pre.obj_key, pre.upload_id);
        let resp = self.client.post(&url)
            .header("Authorization", &auth.auth_key)
            .header("Content-MD5", &md5)
            .header("Content-Type", "application/xml")
            .header("Referer", Self::REFERER)
            .header("x-oss-callback", &cb)
            .header("x-oss-date", &time)
            .header("x-oss-user-agent", "aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit")
            .body(xml).send().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        
        if !resp.status().is_success() {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("提交失败: {}", resp.status())));
        }
        Ok(())
    }

    async fn upload_finish(&self, pre: &QuarkUploadPreData) -> std::io::Result<()> {
        let _: QuarkResponse<Value> = self.api_request("/file/upload/finish", Some(json!({ "obj_key": pre.obj_key, "task_id": pre.task_id }))).await?;
        Ok(())
    }
}

impl AsyncWrite for QuarkWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.buffer.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        self.closed = true;

        let buffer = std::mem::take(&mut self.buffer);
        let path = self.path.clone();
        let cookie = self.cookie.clone();
        let root_folder_id = self.root_folder_id.clone();

        // 在独立线程中执行上传
        let result = std::thread::spawn(move || {
            let writer = QuarkWriter {
                buffer,
                path,
                client: Client::new(),
                cookie,
                root_folder_id,
                closed: true,
            };
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(writer.do_upload())
        })
        .join()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "上传线程panic"))?;

        Poll::Ready(result)
    }
}

#[async_trait]
impl StorageDriver for QuarkDriver {
    fn name(&self) -> &str {
        "夸克网盘"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: false, // 夸克CDN需要headers验证，不支持302
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false, // 夸克不需要缓存，直接流式写入
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false, // 夸克支持流式写入
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let fid = self.get_fid_by_path(path).await?;
        self.list_files(&fid, path).await
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let fid = self.get_fid_by_path(path).await?;
        let url = self.get_download_url(&fid).await?;
        
        let range_tuple = range.map(|r| (r.start, r.end.saturating_sub(1)));
        let reader = QuarkReader::new(&url, &self.config.cookie, range_tuple).await?;
        Ok(Box::new(reader))
    }

    async fn open_writer(
        &self,
        path: &str,
        _size_hint: Option<u64>,
        _progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let writer = QuarkWriter::new(
            path.to_string(),
            self.config.cookie.clone(),
            self.config.root_folder_id.clone(),
        );
        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let fid = self.get_fid_by_path(path).await?;
        
        let body = json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": [fid]
        });

        let _: Value = self.request("/file/delete", Method::POST, None, Some(body)).await?;
        
        // 清除缓存
        self.path_cache.write().await.remove(path.trim_matches('/'));
        
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let path = path.trim_matches('/');
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        
        let folder_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("无效路径"))?;

        let parent_fid = self.get_fid_by_path(&parent_path).await?;

        let body = json!({
            "dir_init_lock": false,
            "dir_path": "",
            "file_name": folder_name,
            "pdir_fid": parent_fid
        });

        let _: Value = self.request("/file", Method::POST, None, Some(body)).await?;
        
        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        let fid = self.get_fid_by_path(path).await?;
        
        let body = json!({
            "fid": fid,
            "file_name": new_name
        });

        let _: Value = self.request("/file/rename", Method::POST, None, Some(body)).await?;
        
        // 更新缓存
        let old_key = path.trim_matches('/').to_string();
        let mut cache = self.path_cache.write().await;
        if let Some(fid) = cache.remove(&old_key) {
            let parent = std::path::Path::new(&old_key)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let new_key = if parent.is_empty() {
                new_name.to_string()
            } else {
                format!("{}/{}", parent, new_name)
            };
            cache.insert(new_key, fid);
        }
        
        Ok(())
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let fid = self.get_fid_by_path(old_path).await?;
        
        let new_parent = std::path::Path::new(new_path.trim_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        
        let new_parent_fid = self.get_fid_by_path(&new_parent).await?;

        let body = json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": [fid],
            "to_pdir_fid": new_parent_fid
        });

        let _: Value = self.request("/file/move", Method::POST, None, Some(body)).await?;
        
        // 清除缓存
        self.path_cache.write().await.clear();
        
        Ok(())
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // 夸克CDN需要Cookie/Referer/User-Agent验证，不支持302重定向
        Ok(None)
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        let params = [
            ("fetch_subscribe", "false"),
            ("_ch", "home"),
            ("fetch_identity", "false"),
        ];

        match self.request::<QuarkMemberData>("/member", Method::GET, Some(&params), None).await {
            Ok(data) => Ok(Some(SpaceInfo {
                used: data.use_capacity,
                total: data.total_capacity,
                free: data.total_capacity.saturating_sub(data.use_capacity),
            })),
            Err(e) => {
                tracing::warn!("获取夸克空间信息失败: {}", e);
                Ok(None)
            }
        }
    }

    fn show_space_in_frontend(&self) -> bool {
        self.config.show_space_info
    }
}

/// 夸克驱动工厂
pub struct QuarkDriverFactory;

impl DriverFactory for QuarkDriverFactory {
    fn driver_type(&self) -> &'static str {
        "quark"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "夸克网盘".to_string(),
            local_sort: false,
            only_proxy: true, // 夸克CDN需要headers验证，必须代理
            no_cache: false,
            no_upload: true, // 暂时禁用上传
            default_root: Some("0".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("cookie", "string")
                .title("Cookie")
                .required()
                .help("从浏览器获取的Cookie"),
            ConfigItem::new("root_folder_id", "string")
                .title("根目录ID")
                .default("0")
                .help("根目录ID，默认为0（根目录）"),
            ConfigItem::new("show_space_info", "bool")
                .title("前台显示空间信息")
                .default("true")
                .help("是否在前台文件列表中显示空间信息"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let quark_config: QuarkConfig = serde_json::from_value(config)?;
        Ok(Box::new(QuarkDriver::new(quark_config)))
    }
}
