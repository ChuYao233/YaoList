//! 115云盘分享驱动实现
//! 参考 OpenList 的 115_share 驱动

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
use futures::TryStreamExt;
use tokio_util::io::StreamReader;

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo, ProgressCallback, DriverFactory, DriverConfig, ConfigItem};
use super::super::pan115::crypto::{m115_encode, m115_decode, generate_random_key};

/// API地址
const API_SHARE_SNAP: &str = "https://webapi.115.com/share/snap";
const API_SHARE_DOWN: &str = "https://proapi.115.com/app/share/downurl";

/// 115云盘分享配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pan115ShareConfig {
    /// 分享码（从分享链接提取）
    pub share_code: String,
    /// 接收码（从分享链接提取）
    pub receive_code: String,
    /// Cookie（可选，用于需要登录的分享）
    #[serde(default)]
    pub cookie: String,
    /// 根目录ID（默认空字符串表示根目录）
    #[serde(default)]
    pub root_id: String,
    /// 每页数量
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page_size() -> i64 {
    1000
}

/// 分享文件信息 - 对应 115driver 的 ShareFile
#[derive(Debug, Clone, Deserialize)]
struct ShareFile {
    /// 文件ID
    #[serde(default)]
    fid: Option<String>,
    /// 目录ID
    #[serde(default)]
    cid: Option<Value>,  // 可能是字符串或数字
    /// 文件名
    #[serde(default)]
    n: String,
    /// 文件大小（可能是字符串或数字）
    #[serde(default)]
    s: Option<Value>,
    /// 更新时间
    #[serde(default)]
    t: Option<String>,
    /// 是否文件 (0=文件夹, 非0=文件)
    #[serde(default)]
    fc: i32,
    /// SHA1
    #[serde(default)]
    sha: Option<String>,
}

impl ShareFile {
    fn get_size(&self) -> i64 {
        match &self.s {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(s)) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }
    
    fn get_cid(&self) -> String {
        match &self.cid {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        }
    }
}

/// 分享快照响应 - 对应 115driver 的 ShareSnapResp
#[derive(Debug, Deserialize)]
struct ShareSnapResponse {
    #[serde(default)]
    state: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    errno: i32,
    data: Option<ShareSnapData>,
}

#[derive(Debug, Deserialize)]
struct ShareSnapData {
    /// 文件列表
    #[serde(default)]
    list: Option<Vec<ShareFile>>,
    /// 文件数量
    #[serde(default)]
    count: Option<i64>,
    /// 分享信息
    shareinfo: Option<ShareInfo>,
}

#[derive(Debug, Deserialize)]
struct ShareInfo {
    #[serde(default)]
    snap_id: String,
    #[serde(default)]
    file_size: Option<Value>,  // StringInt64
    #[serde(default)]
    share_title: String,
    #[serde(default)]
    file_category: i64,
}

impl ShareInfo {
    fn get_file_size(&self) -> i64 {
        match &self.file_size {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(s)) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }
}

/// 下载链接响应（加密）
#[derive(Debug, Deserialize)]
struct DownloadResp {
    #[serde(default)]
    state: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    errno: i32,
    /// 加密的数据
    data: Option<String>,
}

/// 解密后的下载信息
#[derive(Debug, Deserialize)]
struct SharedDownloadInfo {
    #[serde(default)]
    fid: String,
    #[serde(default, rename = "fn")]
    file_name: String,
    #[serde(default)]
    fs: Option<Value>,
    url: Option<DownloadUrl>,
}

#[derive(Debug, Deserialize)]
struct DownloadUrl {
    #[serde(default)]
    url: String,
}

/// 缓存的文件信息
#[derive(Debug, Clone)]
struct CachedFileInfo {
    file_id: String,
    is_dir: bool,
}

/// 115云盘分享驱动
pub struct Pan115ShareDriver {
    config: Pan115ShareConfig,
    client: Client,
    no_redirect_client: Client,
    /// 文件信息缓存 (path -> file_info)
    file_cache: RwLock<HashMap<String, CachedFileInfo>>,
    /// 下载链接缓存
    download_cache: RwLock<HashMap<String, (String, DateTime<Utc>)>>,
}

impl Pan115ShareDriver {
    pub fn new(config: Pan115ShareConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 115Browser/27.0.5.7")
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        
        let no_redirect_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 115Browser/27.0.0")
            .redirect(Policy::none())
            .build()
            .unwrap();
        
        Self {
            config,
            client,
            no_redirect_client,
            file_cache: RwLock::new(HashMap::new()),
            download_cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// 获取分享文件列表
    async fn get_share_files(&self, cid: &str) -> Result<Vec<ShareFile>> {
        let mut all_files = Vec::new();
        let mut offset = 0i64;
        
        loop {
            let mut req = self.client.get(API_SHARE_SNAP)
                .query(&[
                    ("share_code", self.config.share_code.as_str()),
                    ("receive_code", self.config.receive_code.as_str()),
                    ("cid", cid),
                    ("offset", &offset.to_string()),
                    ("limit", &self.config.page_size.to_string()),
                ]);
            
            if !self.config.cookie.is_empty() {
                req = req.header("Cookie", &self.config.cookie);
            }
            
            let response = req.send().await.context("请求分享列表失败")?;
            let status = response.status();
            let text = response.text().await.context("读取响应失败")?;
            tracing::debug!("115Share API状态: {}, 响应: {}", status, &text[..text.len().min(500)]);
            
            // 检查是否为 HTML 错误页面
            if text.starts_with("<!doctype") || text.starts_with("<html") {
                return Err(anyhow!("API返回HTML错误页面，可能是网络问题或API限制"));
            }
            let resp: ShareSnapResponse = serde_json::from_str(&text).context("解析响应失败")?;
            
            if !resp.state {
                let err_msg = if resp.error.is_empty() {
                    format!("API错误: errno={}", resp.errno)
                } else {
                    resp.error.clone()
                };
                return Err(anyhow!("获取分享文件失败: {}", err_msg));
            }
            
            let data = resp.data.ok_or_else(|| anyhow!("响应缺少data字段"))?;
            
            // 处理单文件分享的情况（没有 list 字段）
            let file_list = data.list.unwrap_or_default();
            if file_list.is_empty() {
                if let Some(info) = data.shareinfo {
                    // 单文件分享，构造一个文件条目
                    if info.file_category == 1 {
                        let file_size = info.get_file_size();
                        all_files.push(ShareFile {
                            n: info.share_title,
                            s: Some(Value::Number(file_size.into())),
                            t: None,
                            fid: Some(info.snap_id),
                            cid: None,
                            fc: 1,
                            sha: None,
                        });
                    }
                }
                break;
            }
            
            let count = file_list.len() as i64;
            let total_count = data.count.unwrap_or(count);
            all_files.extend(file_list);
            
            if count < self.config.page_size || all_files.len() as i64 >= total_count {
                break;
            }
            
            offset += count;
            
            // 限速
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        
        Ok(all_files)
    }
    
    /// 获取下载链接（使用加密 API）
    async fn get_download_url(&self, file_id: &str) -> Result<String> {
        // 检查缓存
        {
            let cache = self.download_cache.read().await;
            if let Some((url, expire)) = cache.get(file_id) {
                if *expire > Utc::now() {
                    return Ok(url.clone());
                }
            }
        }
        
        // 生成加密密钥
        let key = generate_random_key();
        
        // 构建请求参数并加密
        let params = json!({
            "share_code": self.config.share_code,
            "receive_code": self.config.receive_code,
            "file_id": file_id
        });
        let params_str = serde_json::to_string(&params)?;
        let encoded_data = m115_encode(params_str.as_bytes(), &key)
            .context("加密请求数据失败")?;
        
        // 获取时间戳
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let mut req = self.client.post(API_SHARE_DOWN)
            .query(&[("t", timestamp.to_string())])
            .form(&[("data", encoded_data)]);
        
        if !self.config.cookie.is_empty() {
            req = req.header("Cookie", &self.config.cookie);
        }
        
        let response = req.send().await.context("请求下载链接失败")?;
        let text = response.text().await.context("读取响应失败")?;
        tracing::debug!("115Share 下载响应: {}", &text[..text.len().min(200)]);
        
        let resp: DownloadResp = serde_json::from_str(&text).context("解析下载响应失败")?;
        
        if !resp.state {
            let err_msg = if resp.error.is_empty() {
                format!("API错误: errno={}", resp.errno)
            } else {
                resp.error
            };
            return Err(anyhow!("获取下载链接失败: {}", err_msg));
        }
        
        let encrypted_data = resp.data.ok_or_else(|| anyhow!("响应缺少data字段"))?;
        
        // 解密响应数据
        let decrypted = m115_decode(&encrypted_data, &key)
            .context("解密响应数据失败")?;
        let decrypted_str = String::from_utf8(decrypted)
            .context("响应数据不是有效的UTF-8")?;
        
        tracing::debug!("115Share 解密后数据: {}", &decrypted_str[..decrypted_str.len().min(200)]);
        
        let download_info: SharedDownloadInfo = serde_json::from_str(&decrypted_str)
            .context("解析下载信息失败")?;
        
        let url = download_info.url
            .ok_or_else(|| anyhow!("下载信息缺少URL"))?
            .url;
        
        if url.is_empty() {
            return Err(anyhow!("下载链接为空"));
        }
        
        // 缓存（10分钟有效）
        {
            let mut cache = self.download_cache.write().await;
            cache.insert(file_id.to_string(), (url.clone(), Utc::now() + chrono::Duration::minutes(10)));
        }
        
        Ok(url)
    }
    
    /// 从路径解析文件ID
    async fn resolve_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.config.root_id.clone());
        }
        
        // 检查缓存
        {
            let cache = self.file_cache.read().await;
            if let Some(info) = cache.get(path) {
                return Ok(info.file_id.clone());
            }
        }
        
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.config.root_id.clone();
        let mut current_path = String::new();
        
        for part in parts {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);
            
            // 检查缓存
            {
                let cache = self.file_cache.read().await;
                if let Some(info) = cache.get(&current_path) {
                    current_id = info.file_id.clone();
                    continue;
                }
            }
            
            let files = self.get_share_files(&current_id).await?;
            let found = files.iter().find(|f| f.n == part);
            
            if let Some(file) = found {
                let is_dir = file.fc == 0;
                let file_id = if is_dir {
                    file.get_cid()
                } else {
                    file.fid.clone().unwrap_or_default()
                };
                
                // 缓存
                {
                    let mut cache = self.file_cache.write().await;
                    cache.insert(current_path.clone(), CachedFileInfo {
                        file_id: file_id.clone(),
                        is_dir,
                    });
                }
                
                current_id = file_id;
            } else {
                return Err(anyhow!("路径不存在: {}", path));
            }
        }
        
        Ok(current_id)
    }
    
    /// 获取文件的下载URL（需要先列出目录）
    async fn get_file_download_url(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        let parts: Vec<&str> = path.rsplitn(2, '/').collect();
        let (filename, parent_path) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (parts[0], "")
        };
        
        let parent_id = self.resolve_path(parent_path).await?;
        let files = self.get_share_files(&parent_id).await?;
        
        let file = files.iter().find(|f| f.n == filename && f.fc != 0)
            .ok_or_else(|| anyhow!("文件不存在: {}", filename))?;
        
        let file_id = file.fid.as_ref()
            .ok_or_else(|| anyhow!("文件缺少ID"))?;
        
        self.get_download_url(file_id).await
    }
}

#[async_trait]
impl StorageDriver for Pan115ShareDriver {
    fn name(&self) -> &str {
        "115Share"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,
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
        tracing::debug!("115Share: 列出目录 {}", path);
        
        let cid = self.resolve_path(path).await?;
        let files = self.get_share_files(&cid).await?;
        
        let entries: Vec<Entry> = files.iter().map(|f| {
            let is_dir = f.fc == 0;
            
            // 解析时间戳
            let modified = f.t.as_ref().and_then(|t| t.parse::<i64>().ok()).and_then(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.to_rfc3339())
            });
            
            Entry {
                name: f.n.clone(),
                path: format!("{}/{}", path.trim_matches('/'), f.n),
                size: f.get_size() as u64,
                is_dir,
                modified,
            }
        }).collect();
        
        // 缓存文件信息
        {
            let mut cache = self.file_cache.write().await;
            for f in &files {
                let is_dir = f.fc == 0;
                let file_id = if is_dir {
                    f.get_cid()
                } else {
                    f.fid.clone().unwrap_or_default()
                };
                let file_path = format!("{}/{}", path.trim_matches('/'), f.n);
                cache.insert(file_path.trim_start_matches('/').to_string(), CachedFileInfo {
                    file_id,
                    is_dir,
                });
            }
        }
        
        Ok(entries)
    }
    
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        tracing::debug!("115Share: 获取直链 {}", path);
        let url = self.get_file_download_url(path).await?;
        Ok(Some(url))
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        tracing::debug!("115Share: 读取文件 {} range={:?}", path, range);
        
        let url = self.get_file_download_url(path).await?;
        
        let mut req = self.client.get(&url);
        
        if let Some(ref r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }
        
        let response = req.send().await.context("下载请求失败")?;
        
        if !response.status().is_success() && response.status().as_u16() != 206 {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }
        
        // 流式传输
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
        Err(anyhow!("115云盘分享不支持上传"))
    }
    
    async fn delete(&self, _path: &str) -> Result<()> {
        Err(anyhow!("115云盘分享不支持删除"))
    }
    
    async fn create_dir(&self, _path: &str) -> Result<()> {
        Err(anyhow!("115云盘分享不支持创建文件夹"))
    }
    
    async fn rename(&self, _path: &str, _new_name: &str) -> Result<()> {
        Err(anyhow!("115云盘分享不支持重命名"))
    }
    
    async fn move_item(&self, _old_path: &str, _new_path: &str) -> Result<()> {
        Err(anyhow!("115云盘分享不支持移动"))
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        Ok(None)
    }
    
    fn show_space_in_frontend(&self) -> bool {
        false
    }
}

/// 115云盘分享驱动工厂
pub struct Pan115ShareDriverFactory;

impl DriverFactory for Pan115ShareDriverFactory {
    fn driver_type(&self) -> &'static str {
        "115Share"
    }
    
    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "115云盘分享".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: true,
            default_root: Some("".to_string()),
        }
    }
    
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("share_code", "string")
                .title("分享码")
                .help("从分享链接中提取，如 https://115.com/s/xxxxx 中的 xxxxx")
                .required(),
            ConfigItem::new("receive_code", "string")
                .title("接收码")
                .help("分享链接中的接收码")
                .required(),
            ConfigItem::new("cookie", "string")
                .title("Cookie")
                .help("可选，用于需要登录才能访问的分享"),
            ConfigItem::new("root_id", "string")
                .title("根目录ID")
                .help("默认为空（根目录）")
                .default(""),
            ConfigItem::new("page_size", "number")
                .title("每页数量")
                .help("列表API每页返回数量")
                .default("1000"),
        ]
    }
    
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: Pan115ShareConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(Pan115ShareDriver::new(config)))
    }
}
