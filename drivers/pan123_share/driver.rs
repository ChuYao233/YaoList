//! 123云盘分享驱动实现
//! 参考 OpenList 的 123_share 驱动

use std::ops::Range;
use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result, Context};
use async_trait::async_trait;
use reqwest::{Client, redirect::Policy};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use chrono::{DateTime, Utc};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::TryStreamExt;
use tokio_util::io::StreamReader;

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo, ProgressCallback, DriverFactory, DriverConfig, ConfigItem};

/// API地址
const MAIN_API: &str = "https://www.123pan.com/b/api";
const FILE_LIST_API: &str = "https://www.123pan.com/b/api/share/get";
const DOWNLOAD_INFO_API: &str = "https://www.123pan.com/b/api/share/download/info";

/// 123云盘分享配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pan123ShareConfig {
    /// 分享Key（从分享链接提取）
    pub share_key: String,
    /// 分享密码（可选）
    #[serde(default)]
    pub share_pwd: String,
    /// 根目录ID（默认0）
    #[serde(default = "default_root_id")]
    pub root_id: String,
    /// AccessToken（可选，用于需要登录的分享）
    #[serde(default)]
    pub access_token: String,
}

fn default_root_id() -> String {
    "0".to_string()
}

/// 文件信息
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FileInfo {
    file_name: String,
    size: i64,
    update_at: Option<String>,
    file_id: i64,
    #[serde(rename = "Type")]
    file_type: i32,
    etag: Option<String>,
    #[serde(rename = "S3KeyFlag")]
    s3_key_flag: Option<String>,
    download_url: Option<String>,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
struct FilesResponse {
    code: i32,
    message: Option<String>,
    data: Option<FilesData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FilesData {
    info_list: Option<Vec<FileInfo>>,
    next: Option<String>,
}

/// 下载信息响应
#[derive(Debug, Deserialize)]
struct DownloadResponse {
    code: i32,
    message: Option<String>,
    data: Option<DownloadData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DownloadData {
    #[serde(rename = "DownloadURL")]
    download_url: Option<String>,
}

/// 缓存的下载链接
#[derive(Debug, Clone)]
struct CachedDownloadUrl {
    url: String,
    expire_at: DateTime<Utc>,
}

/// 缓存的文件信息
#[derive(Debug, Clone)]
struct CachedFileInfo {
    etag: String,
    s3_key_flag: String,
    size: i64,
}

/// 123云盘分享驱动
pub struct Pan123ShareDriver {
    config: Pan123ShareConfig,
    client: Client,
    no_redirect_client: Client,
    /// 下载链接缓存 (file_id -> cached_url)
    download_cache: RwLock<HashMap<String, CachedDownloadUrl>>,
    /// 文件信息缓存 (file_id -> file_info)
    file_info_cache: RwLock<HashMap<String, CachedFileInfo>>,
}

impl Pan123ShareDriver {
    pub fn new(config: Pan123ShareConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .unwrap();
        
        let no_redirect_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .redirect(Policy::none())
            .build()
            .unwrap();
        
        Self {
            config,
            client,
            no_redirect_client,
            download_cache: RwLock::new(HashMap::new()),
            file_info_cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// 生成API签名
    fn sign_path(path: &str) -> (String, String) {
        use std::time::{SystemTime, UNIX_EPOCH};
        
        let table: [u8; 26] = [
            b'a', b'd', b'e', b'f', b'g', b'h', b'l', b'm', b'y', b'i',
            b'j', b'n', b'o', b'p', b'k', b'q', b'r', b's', b't', b'u',
            b'b', b'c', b'v', b'w', b's', b'z'
        ];
        
        let random: u64 = rand::random::<u64>() % 10000000;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let timestamp = now.as_secs();
        
        // 生成时间字符串 YYYYMMDDHHMM (UTC+8)
        let secs = timestamp as i64 + 8 * 3600;
        let datetime = chrono::DateTime::from_timestamp(secs, 0).unwrap();
        let time_str = datetime.format("%Y%m%d%H%M").to_string();
        
        // 转换时间字符串
        let mut now_bytes: Vec<u8> = time_str.bytes().collect();
        for byte in &mut now_bytes {
            if *byte >= b'0' && *byte <= b'9' {
                *byte = table[(*byte - b'0') as usize];
            }
        }
        
        // 使用简化的 CRC32 计算
        let time_sign = Self::crc32(&now_bytes);
        let data = format!("{}|{}|{}|web|3|{}", timestamp, random, path, time_sign);
        let data_sign = Self::crc32(data.as_bytes());
        
        (time_sign.to_string(), format!("{}-{}-{}", timestamp, random, data_sign))
    }
    
    /// 简化的 CRC32 计算
    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFFFFFF;
        for byte in data {
            crc ^= *byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }
    
    /// 获取带签名的API URL
    fn get_signed_url(url: &str) -> String {
        if let Ok(mut parsed) = url::Url::parse(url) {
            let (k, v) = Self::sign_path(parsed.path());
            parsed.query_pairs_mut().append_pair(&k, &v);
            parsed.to_string()
        } else {
            url.to_string()
        }
    }
    
    /// 发送API请求
    async fn request(&self, url: &str, method: &str, body: Option<Value>) -> Result<Value> {
        let signed_url = Self::get_signed_url(url);
        
        let mut req = match method {
            "POST" => self.client.post(&signed_url),
            _ => self.client.get(&signed_url),
        };
        
        req = req
            .header("origin", "https://www.123pan.com")
            .header("referer", "https://www.123pan.com/")
            .header("platform", "web")
            .header("app-version", "3");
        
        if !self.config.access_token.is_empty() {
            req = req.header("authorization", format!("Bearer {}", self.config.access_token));
        }
        
        if let Some(body) = body {
            req = req.json(&body);
        }
        
        let response = req.send().await.context("请求失败")?;
        let json: Value = response.json().await.context("解析响应失败")?;
        
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("未知错误");
            return Err(anyhow!("API错误: {}", message));
        }
        
        Ok(json)
    }
    
    /// 获取文件列表
    async fn get_files(&self, parent_id: &str) -> Result<Vec<FileInfo>> {
        let mut page = 1;
        let mut all_files = Vec::new();
        
        loop {
            let url = format!(
                "{}?limit=100&next=0&orderBy=file_id&orderDirection=desc&parentFileId={}&Page={}&shareKey={}&SharePwd={}",
                FILE_LIST_API,
                parent_id,
                page,
                self.config.share_key,
                self.config.share_pwd
            );
            
            let json = self.request(&url, "GET", None).await?;
            
            let data = json.get("data").ok_or_else(|| anyhow!("响应缺少data字段"))?;
            let info_list = data.get("InfoList")
                .or_else(|| data.get("infoList"))
                .and_then(|v| v.as_array());
            
            if let Some(files) = info_list {
                for file_value in files {
                    if let Ok(file) = serde_json::from_value::<FileInfo>(file_value.clone()) {
                        // 缓存文件信息
                        let file_id = file.file_id.to_string();
                        let mut cache = self.file_info_cache.write().await;
                        cache.insert(file_id, CachedFileInfo {
                            etag: file.etag.clone().unwrap_or_default(),
                            s3_key_flag: file.s3_key_flag.clone().unwrap_or_default(),
                            size: file.size,
                        });
                        drop(cache);
                        
                        all_files.push(file);
                    }
                }
            }
            
            let next = data.get("Next")
                .or_else(|| data.get("next"))
                .and_then(|v| v.as_str())
                .unwrap_or("-1");
            
            if info_list.map(|v| v.is_empty()).unwrap_or(true) || next == "-1" {
                break;
            }
            
            page += 1;
            
            // 限速：每700ms一次请求
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        
        Ok(all_files)
    }
    
    /// 获取下载链接
    async fn get_download_url(&self, file_id: &str) -> Result<String> {
        // 检查缓存
        {
            let cache = self.download_cache.read().await;
            if let Some(cached) = cache.get(file_id) {
                if cached.expire_at > Utc::now() {
                    return Ok(cached.url.clone());
                }
            }
        }
        
        // 获取文件信息
        let file_info = {
            let cache = self.file_info_cache.read().await;
            cache.get(file_id).cloned()
        };
        
        let file_info = file_info.ok_or_else(|| anyhow!("文件信息不存在，请先列出目录"))?;
        
        let body = json!({
            "shareKey": self.config.share_key,
            "SharePwd": self.config.share_pwd,
            "etag": file_info.etag,
            "fileId": file_id.parse::<i64>().unwrap_or(0),
            "s3keyFlag": file_info.s3_key_flag,
            "size": file_info.size
        });
        
        let json = self.request(DOWNLOAD_INFO_API, "POST", Some(body)).await?;
        
        let download_url = json.get("data")
            .and_then(|d| d.get("DownloadURL").or_else(|| d.get("downloadURL")))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("获取下载链接失败"))?;
        
        // 解析下载链接
        let mut final_url = download_url.to_string();
        
        if let Ok(parsed) = url::Url::parse(download_url) {
            // 检查是否有params参数（base64编码的真实URL）
            if let Some(params) = parsed.query_pairs().find(|(k, _)| k == "params") {
                if let Ok(decoded) = BASE64.decode(params.1.as_bytes()) {
                    if let Ok(real_url) = String::from_utf8(decoded) {
                        final_url = real_url;
                    }
                }
            }
        }
        
        // 尝试获取302重定向后的真实URL
        let response = self.no_redirect_client
            .get(&final_url)
            .header("Referer", "https://www.123pan.com/")
            .send()
            .await;
        
        if let Ok(resp) = response {
            if resp.status().as_u16() == 302 {
                if let Some(location) = resp.headers().get("location") {
                    if let Ok(loc_str) = location.to_str() {
                        final_url = loc_str.to_string();
                    }
                }
            } else if resp.status().is_success() {
                // 可能是JSON响应
                if let Ok(json) = resp.json::<Value>().await {
                    if let Some(redirect_url) = json.get("data")
                        .and_then(|d| d.get("redirect_url"))
                        .and_then(|v| v.as_str())
                    {
                        final_url = redirect_url.to_string();
                    }
                }
            }
        }
        
        // 缓存下载链接（10分钟有效）
        {
            let mut cache = self.download_cache.write().await;
            cache.insert(file_id.to_string(), CachedDownloadUrl {
                url: final_url.clone(),
                expire_at: Utc::now() + chrono::Duration::minutes(10),
            });
        }
        
        Ok(final_url)
    }
    
    /// 从路径解析文件ID
    async fn resolve_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.config.root_id.clone());
        }
        
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.config.root_id.clone();
        
        for part in parts {
            let files = self.get_files(&current_id).await?;
            let found = files.iter().find(|f| f.file_name == part);
            
            if let Some(file) = found {
                current_id = file.file_id.to_string();
            } else {
                return Err(anyhow!("路径不存在: {}", path));
            }
        }
        
        Ok(current_id)
    }
}

#[async_trait]
impl StorageDriver for Pan123ShareDriver {
    fn name(&self) -> &str {
        "123PanShare"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,  // 支持直链
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false,
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        tracing::debug!("123PanShare: 列出目录 {}", path);
        
        let parent_id = self.resolve_path(path).await?;
        let files = self.get_files(&parent_id).await?;
        
        let entries: Vec<Entry> = files.iter().map(|f| {
            let modified = f.update_at.as_ref().and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.to_rfc3339())
            });
            
            Entry {
                name: f.file_name.clone(),
                path: format!("{}/{}", path.trim_matches('/'), f.file_name),
                size: f.size as u64,
                is_dir: f.file_type == 1,
                modified,
            }
        }).collect();
        
        Ok(entries)
    }
    
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        tracing::debug!("123PanShare: 获取直链 {}", path);
        
        // 解析路径获取文件ID
        let path = path.trim_matches('/');
        let parts: Vec<&str> = path.rsplitn(2, '/').collect();
        let (filename, parent_path) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (parts[0], "")
        };
        
        let parent_id = self.resolve_path(parent_path).await?;
        let files = self.get_files(&parent_id).await?;
        
        let file = files.iter().find(|f| f.file_name == filename && f.file_type != 1)
            .ok_or_else(|| anyhow!("文件不存在: {}", filename))?;
        
        let url = self.get_download_url(&file.file_id.to_string()).await?;
        Ok(Some(url))
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        tracing::debug!("123PanShare: 读取文件 {} range={:?}", path, range);
        
        let url = self.get_direct_link(path).await?
            .ok_or_else(|| anyhow!("获取下载链接失败"))?;
        
        let mut req = self.client.get(&url)
            .header("Referer", "https://www.123pan.com/");
        
        if let Some(ref r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }
        
        let response = req.send().await.context("下载请求失败")?;
        
        if !response.status().is_success() && response.status().as_u16() != 206 {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }
        
        // 流式传输：将响应体转换为AsyncRead
        let stream = response.bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = StreamReader::new(stream);
        Ok(Box::new(reader))
    }
    
    async fn open_writer(
        &self,
        _path: &str,
        _size_hint: Option<u64>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        Err(anyhow!("123云盘分享不支持上传"))
    }
    
    async fn delete(&self, _path: &str) -> Result<()> {
        Err(anyhow!("123云盘分享不支持删除"))
    }
    
    async fn create_dir(&self, _path: &str) -> Result<()> {
        Err(anyhow!("123云盘分享不支持创建文件夹"))
    }
    
    async fn rename(&self, _path: &str, _new_name: &str) -> Result<()> {
        Err(anyhow!("123云盘分享不支持重命名"))
    }
    
    async fn move_item(&self, _old_path: &str, _new_path: &str) -> Result<()> {
        Err(anyhow!("123云盘分享不支持移动"))
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        Ok(None)  // 分享不提供空间信息
    }
    
    fn show_space_in_frontend(&self) -> bool {
        false
    }
}

/// 123云盘分享驱动工厂
pub struct Pan123ShareDriverFactory;

impl DriverFactory for Pan123ShareDriverFactory {
    fn driver_type(&self) -> &'static str {
        "123PanShare"
    }
    
    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "123云盘分享".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: true,  // 分享不支持上传
            default_root: Some("0".to_string()),
        }
    }
    
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("share_key", "string")
                .title("分享Key")
                .help("从分享链接中提取，如 https://www.123pan.com/s/xxxx 中的 xxxx")
                .required(),
            ConfigItem::new("share_pwd", "string")
                .title("分享密码")
                .help("如果分享有密码，请填写"),
            ConfigItem::new("root_id", "string")
                .title("根目录ID")
                .help("默认为0（根目录）")
                .default("0"),
            ConfigItem::new("access_token", "string")
                .title("AccessToken")
                .help("可选，用于需要登录才能访问的分享"),
        ]
    }
    
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: Pan123ShareConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(Pan123ShareDriver::new(config)))
    }
}
