//! WebDAV 驱动实现
//! 
//! 使用reqwest实现WebDAV协议，支持流式上传下载

use std::ops::Range;
use anyhow::{Result, anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use reqwest::{Client, StatusCode};

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo, ProgressCallback};

/// WebDAV 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavConfig {
    /// WebDAV服务器地址 (如 https://dav.example.com/files)
    pub address: String,
    /// 用户名
    pub username: String,
    /// 密码
    #[serde(default)]
    pub password: String,
    /// 根目录路径
    #[serde(default = "default_root")]
    pub root_path: String,
    /// 跳过TLS证书验证
    #[serde(default)]
    pub tls_insecure_skip_verify: bool,
}

fn default_root() -> String {
    "/".to_string()
}

/// WebDAV 驱动
pub struct WebDavDriver {
    config: WebDavConfig,
    client: Client,
    upload_client: Client,
}

impl WebDavDriver {
    pub fn new(config: WebDavConfig) -> Result<Self> {
        // 普通请求客户端（较短超时）
        let client = Client::builder()
            .danger_accept_invalid_certs(config.tls_insecure_skip_verify)
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .build()
            .context("创建HTTP客户端失败")?;
        
        // 上传专用客户端（长超时，大缓冲区）
        let upload_client = Client::builder()
            .danger_accept_invalid_certs(config.tls_insecure_skip_verify)
            .timeout(std::time::Duration::from_secs(3600)) // 1小时超时
            .connect_timeout(std::time::Duration::from_secs(60))
            .pool_max_idle_per_host(8)
            .tcp_nodelay(true)
            .build()
            .context("创建上传客户端失败")?;
        
        Ok(Self { config, client, upload_client })
    }
    
    /// 构建完整URL
    fn build_url(&self, path: &str) -> String {
        let base = self.config.address.trim_end_matches('/');
        let root = self.config.root_path.trim_matches('/');
        let path = path.trim_start_matches('/');
        
        if root.is_empty() {
            if path.is_empty() {
                format!("{}/", base)
            } else {
                format!("{}/{}", base, path)
            }
        } else {
            if path.is_empty() {
                format!("{}/{}/", base, root)
            } else {
                format!("{}/{}/{}", base, root, path)
            }
        }
    }
    
    /// 获取认证头
    fn auth_header(&self) -> String {
        use base64::Engine;
        let credentials = format!("{}:{}", self.config.username, self.config.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());
        format!("Basic {}", encoded)
    }
    
    /// 解析PROPFIND响应，提取文件列表
    fn parse_propfind_response(&self, xml: &str, base_path: &str) -> Result<Vec<Entry>> {
        use quick_xml::Reader;
        use quick_xml::events::Event;
        
        let mut entries = Vec::new();
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);
        
        let mut current_href = String::new();
        let mut current_is_dir = false;
        let mut current_size: u64 = 0;
        let mut current_modified: Option<String> = None;
        let mut in_response = false;
        let mut in_href = false;
        let mut in_collection = false;
        let mut in_getcontentlength = false;
        let mut in_getlastmodified = false;
        
        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let local_name = e.local_name();
                    let name = std::str::from_utf8(local_name.as_ref()).unwrap_or("");
                    match name {
                        "response" => {
                            in_response = true;
                            current_href.clear();
                            current_is_dir = false;
                            current_size = 0;
                            current_modified = None;
                        }
                        "href" => in_href = true,
                        "collection" => in_collection = true,
                        "getcontentlength" => in_getcontentlength = true,
                        "getlastmodified" => in_getlastmodified = true,
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local_name = e.local_name();
                    let name = std::str::from_utf8(local_name.as_ref()).unwrap_or("");
                    match name {
                        "response" => {
                            if in_response && !current_href.is_empty() {
                                // 解码URL并提取文件名
                                let decoded_href = urlencoding::decode(&current_href)
                                    .unwrap_or_else(|_| current_href.clone().into())
                                    .to_string();
                                
                                // 提取相对路径
                                let href_path = decoded_href.trim_end_matches('/');
                                let file_name = href_path.split('/').last().unwrap_or("").to_string();
                                
                                // 跳过根目录自身
                                if !file_name.is_empty() {
                                    let entry_path = if base_path == "/" {
                                        format!("/{}", file_name)
                                    } else {
                                        format!("{}/{}", base_path.trim_end_matches('/'), file_name)
                                    };
                                    
                                    entries.push(Entry {
                                        name: file_name,
                                        path: entry_path,
                                        size: current_size,
                                        is_dir: current_is_dir || in_collection,
                                        modified: current_modified.clone(),
                                    });
                                }
                            }
                            in_response = false;
                            in_collection = false;
                        }
                        "href" => in_href = false,
                        "getcontentlength" => in_getcontentlength = false,
                        "getlastmodified" => in_getlastmodified = false,
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let local_name = e.local_name();
                    let name = std::str::from_utf8(local_name.as_ref()).unwrap_or("");
                    if name == "collection" {
                        current_is_dir = true;
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if in_href {
                        current_href = text;
                    } else if in_getcontentlength {
                        current_size = text.parse().unwrap_or(0);
                    } else if in_getlastmodified {
                        current_modified = Some(text);
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    tracing::warn!("解析WebDAV XML失败: {}", e);
                    break;
                }
                _ => {}
            }
        }
        
        Ok(entries)
    }
}

#[async_trait]
impl StorageDriver for WebDavDriver {
    fn name(&self) -> &str {
        "WebDAV"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: false, // WebDAV需要认证，不支持302
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false, // 流式上传，不需要缓存
            can_server_side_copy: true,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false, // WebDAV支持流式上传
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV PROPFIND: {}", url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Authorization", self.auth_header())
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:getcontentlength/>
    <D:getlastmodified/>
    <D:displayname/>
  </D:prop>
</D:propfind>"#)
            .send()
            .await
            .context("WebDAV PROPFIND请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::MULTI_STATUS {
            return Err(anyhow!("WebDAV PROPFIND失败: {}", response.status()));
        }
        
        let xml = response.text().await.context("读取PROPFIND响应失败")?;
        self.parse_propfind_response(&xml, path)
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV GET: {} (范围: {:?})", url, range);
        
        let mut request = self.client
            .get(&url)
            .header("Authorization", self.auth_header());
        
        if let Some(ref r) = range {
            request = request.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }
        
        let response = request.send().await.context("WebDAV GET请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("WebDAV GET失败: {}", response.status()));
        }
        
        // 流式返回响应体
        use futures::StreamExt;
        let stream = response.bytes_stream();
        let mapped_stream = stream.map(|result| {
            result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        });
        let reader = tokio_util::io::StreamReader::new(mapped_stream);
        
        Ok(Box::new(reader))
    }
    
    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV PUT (writer): {} (大小: {:?})", url, size_hint);
        
        // 增大缓冲区以提高吞吐量（32个槽位，每个最大1MB = 最大32MB缓冲）
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(32);
        
        let client = self.upload_client.clone();
        let auth = self.auth_header();
        let url_clone = url.clone();
        
        // 后台任务处理上传
        tokio::spawn(async move {
            let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let body = reqwest::Body::wrap_stream(body_stream);
            
            let result = client
                .put(&url_clone)
                .header("Authorization", &auth)
                .header("Content-Type", "application/octet-stream")
                .body(body)
                .send()
                .await;
            
            match result {
                Ok(resp) => {
                    if resp.status().is_success() || resp.status() == StatusCode::CREATED || resp.status() == StatusCode::NO_CONTENT {
                        tracing::debug!("WebDAV PUT成功: {}", url_clone);
                    } else {
                        tracing::error!("WebDAV PUT失败: {} - {}", url_clone, resp.status());
                    }
                }
                Err(e) => {
                    tracing::error!("WebDAV PUT请求失败: {} - {}", url_clone, e);
                }
            }
        });
        
        // 返回一个AsyncWrite实现，将数据发送到channel
        Ok(Box::new(WebDavWriter { tx: Some(tx) }))
    }
    
    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        _progress: Option<ProgressCallback>,
    ) -> Result<()> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV PUT: {} (大小: {})", url, data.len());
        
        let response = self.client
            .put(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/octet-stream")
            .body(data)
            .send()
            .await
            .context("WebDAV PUT请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::CREATED && response.status() != StatusCode::NO_CONTENT {
            return Err(anyhow!("WebDAV PUT失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV DELETE: {}", url);
        
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("WebDAV DELETE请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::NO_CONTENT {
            return Err(anyhow!("WebDAV DELETE失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        let url = self.build_url(path);
        tracing::debug!("WebDAV MKCOL: {}", url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("WebDAV MKCOL请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::CREATED {
            return Err(anyhow!("WebDAV MKCOL失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let old_url = self.build_url(old_path);
        
        // 获取父目录路径
        let parent = if let Some(pos) = old_path.rfind('/') {
            &old_path[..pos]
        } else {
            ""
        };
        let new_path = if parent.is_empty() {
            format!("/{}", new_name)
        } else {
            format!("{}/{}", parent, new_name)
        };
        let new_url = self.build_url(&new_path);
        
        tracing::debug!("WebDAV MOVE: {} -> {}", old_url, new_url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), &old_url)
            .header("Authorization", self.auth_header())
            .header("Destination", &new_url)
            .header("Overwrite", "T")
            .send()
            .await
            .context("WebDAV MOVE请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::CREATED && response.status() != StatusCode::NO_CONTENT {
            return Err(anyhow!("WebDAV MOVE失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_url = self.build_url(old_path);
        let new_url = self.build_url(new_path);
        
        tracing::debug!("WebDAV MOVE: {} -> {}", old_url, new_url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), &old_url)
            .header("Authorization", self.auth_header())
            .header("Destination", &new_url)
            .header("Overwrite", "T")
            .send()
            .await
            .context("WebDAV MOVE请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::CREATED && response.status() != StatusCode::NO_CONTENT {
            return Err(anyhow!("WebDAV MOVE失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_url = self.build_url(old_path);
        let new_url = self.build_url(new_path);
        
        tracing::debug!("WebDAV COPY: {} -> {}", old_url, new_url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"COPY").unwrap(), &old_url)
            .header("Authorization", self.auth_header())
            .header("Destination", &new_url)
            .header("Overwrite", "T")
            .send()
            .await
            .context("WebDAV COPY请求失败")?;
        
        if !response.status().is_success() && response.status() != StatusCode::CREATED && response.status() != StatusCode::NO_CONTENT {
            return Err(anyhow!("WebDAV COPY失败: {}", response.status()));
        }
        
        Ok(())
    }
    
    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // WebDAV需要认证，不支持直链302
        // 通过服务器代理下载
        Ok(None)
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        // 使用PROPFIND获取quota信息（RFC 4331）
        let url = self.build_url("/");
        tracing::debug!("WebDAV PROPFIND quota: {}", url);
        
        let response = self.client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Authorization", self.auth_header())
            .header("Depth", "0")
            .header("Content-Type", "application/xml")
            .body(r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:quota-available-bytes/>
    <D:quota-used-bytes/>
  </D:prop>
</D:propfind>"#)
            .send()
            .await;
        
        match response {
            Ok(resp) => {
                if !resp.status().is_success() && resp.status() != StatusCode::MULTI_STATUS {
                    tracing::debug!("WebDAV quota PROPFIND失败: {}", resp.status());
                    return Ok(None);
                }
                
                let xml = match resp.text().await {
                    Ok(t) => t,
                    Err(_) => return Ok(None),
                };
                
                // 解析quota信息
                let mut quota_available: Option<u64> = None;
                let mut quota_used: Option<u64> = None;
                
                // 简单的字符串搜索解析（避免复杂的XML解析）
                if let Some(start) = xml.find("<D:quota-available-bytes>").or_else(|| xml.find("<quota-available-bytes>")) {
                    let search_start = start + if xml[start..].starts_with("<D:") { 25 } else { 23 };
                    if let Some(end) = xml[search_start..].find('<') {
                        if let Ok(v) = xml[search_start..search_start + end].trim().parse::<u64>() {
                            quota_available = Some(v);
                        }
                    }
                }
                
                if let Some(start) = xml.find("<D:quota-used-bytes>").or_else(|| xml.find("<quota-used-bytes>")) {
                    let search_start = start + if xml[start..].starts_with("<D:") { 20 } else { 18 };
                    if let Some(end) = xml[search_start..].find('<') {
                        if let Ok(v) = xml[search_start..search_start + end].trim().parse::<u64>() {
                            quota_used = Some(v);
                        }
                    }
                }
                
                if quota_available.is_some() || quota_used.is_some() {
                    let used = quota_used.unwrap_or(0);
                    let available = quota_available.unwrap_or(0);
                    let total = used + available;
                    
                    return Ok(Some(SpaceInfo {
                        total,
                        used,
                        free: available,
                    }));
                }
                
                Ok(None)
            }
            Err(e) => {
                tracing::debug!("WebDAV quota请求失败: {}", e);
                Ok(None)
            }
        }
    }
}

/// WebDAV写入器，将数据发送到channel用于流式上传
struct WebDavWriter {
    tx: Option<tokio::sync::mpsc::Sender<Result<bytes::Bytes, std::io::Error>>>,
}

impl AsyncWrite for WebDavWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if let Some(ref tx) = self.tx {
            let data = bytes::Bytes::copy_from_slice(buf);
            let tx = tx.clone();
            
            match tx.try_send(Ok(data)) {
                Ok(_) => std::task::Poll::Ready(Ok(buf.len())),
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    // Channel满了，需要等待
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    std::task::Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "WebDAV upload channel closed",
                    )))
                }
            }
        } else {
            std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "WebDAV writer already closed",
            )))
        }
    }
    
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // 关闭channel，通知上传任务结束
        self.tx.take();
        std::task::Poll::Ready(Ok(()))
    }
}
