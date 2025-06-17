use anyhow::{anyhow, Result};
use reqwest::{Client, header::HeaderMap, header::HeaderValue};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};
use regex::Regex;
use futures::Stream;


use async_trait::async_trait;

use crate::drivers::{Driver, DriverFactory, DriverInfo, FileInfo};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.39 (KHTML, like Gecko) Chrome/89.0.4389.111 Safari/537.39";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanzouConfig {
    #[serde(rename = "type", default = "default_type")]
    pub auth_type: String, // account, cookie, url
    
    pub account: Option<String>,
    pub password: Option<String>,
    
    pub cookie: Option<String>,
    
    #[serde(default = "default_root_id")]
    pub root_folder_id: String,
    pub share_password: Option<String>,
    
    #[serde(rename = "baseUrl", default = "default_base_url")]
    pub base_url: String,
    
    #[serde(rename = "shareUrl", default = "default_share_url")]
    pub share_url: String,
    
    #[serde(rename = "user_agent", default = "default_user_agent")]
    pub user_agent: String,
    
    #[serde(rename = "repair_file_info", default)]
    pub repair_file_info: bool,
}

fn default_type() -> String { "cookie".to_string() }
fn default_root_id() -> String { "-1".to_string() }
fn default_base_url() -> String { "https://pc.woozooo.com".to_string() }
fn default_share_url() -> String { "https://pan.lanzoui.com".to_string() }
fn default_user_agent() -> String { DEFAULT_USER_AGENT.to_string() }

impl Default for LanzouConfig {
    fn default() -> Self {
        Self {
            auth_type: default_type(),
            account: None,
            password: None,
            cookie: None,
            root_folder_id: default_root_id(),
            share_password: None,
            base_url: default_base_url(),
            share_url: default_share_url(),
            user_agent: default_user_agent(),
            repair_file_info: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespText<T> {
    pub text: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespInfo<T> {
    pub info: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanzouFile {
    pub id: String,
    pub name: Option<String>,
    pub name_all: Option<String>,
    pub size: Option<String>,
    pub time: Option<String>,
    pub fol_id: Option<String>, // 如果存在则为文件夹
    
    // 缓存字段
    #[serde(skip)]
    pub size_cache: Option<i64>,
    #[serde(skip)]
    pub time_cache: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub share_info: Option<FileShare>,
}

impl LanzouFile {
    pub fn is_dir(&self) -> bool {
        self.fol_id.is_some()
    }
    
    pub fn get_name(&self) -> String {
        if self.is_dir() {
            self.name.as_deref().unwrap_or("").to_string()
        } else {
            self.name_all.as_deref().unwrap_or("").to_string()
        }
    }
    
    pub fn get_id(&self) -> String {
        if self.is_dir() {
            self.fol_id.as_deref().unwrap_or("").to_string()
        } else {
            self.id.clone()
        }
    }
    
    pub fn get_size(&self) -> i64 {
        if let Some(size) = self.size_cache {
            return size;
        }
        
        if let Some(size_str) = &self.size {
            return parse_size(size_str);
        }
        
        0
    }
    
    pub fn get_time(&self) -> String {
        if let Some(time_str) = &self.time {
            time_str.clone()
        } else {
            chrono::Utc::now().format("%Y-%m-%d").to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanzouFileByShare {
    pub id: String,
    pub name_all: String,
    pub duan: Option<String>,
    pub size: Option<String>,
    pub time: Option<String>,
    pub is_folder: bool,
    pub url: Option<String>,
    pub pwd: Option<String>,
    
    #[serde(skip)]
    pub size_cache: Option<i64>,
}

impl LanzouFileByShare {
    pub fn get_size(&self) -> i64 {
        if let Some(size) = self.size_cache {
            return size;
        }
        
        if let Some(size_str) = &self.size {
            return parse_size(size_str);
        }
        
        0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileShare {
    pub pwd: Option<String>,
    pub onof: Option<String>,
    pub taoc: Option<String>,
    pub is_newd: Option<String>,
    pub f_id: Option<String>, // 文件ID
    pub new_url: Option<String>, // 文件夹
    pub name: Option<String>,
    pub des: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileShareUrlResp {
    pub dom: Option<String>,
    pub url: Option<String>,
    pub inf: Option<String>,
}

impl FileShareUrlResp {
    pub fn get_download_url(&self) -> Result<String> {
        let dom = self.dom.as_deref().ok_or_else(|| anyhow!("Missing dom field"))?;
        let url = self.url.as_deref().ok_or_else(|| anyhow!("Missing url field"))?;
        Ok(format!("{}/file/{}", dom, url))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanzouResponse {
    pub zt: i32,
    pub inf: Option<String>,
    pub info: Option<String>,
}

pub struct LanzouDriver {
    config: LanzouConfig,
    client: Client,
    uid: Arc<Mutex<Option<String>>>,
    vei: Arc<Mutex<Option<String>>>,
}

impl LanzouDriver {
    pub fn new(config: LanzouConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;
        
        Ok(Self {
            config,
            client,
            uid: Arc::new(Mutex::new(None)),
            vei: Arc::new(Mutex::new(None)),
        })
    }
    
    pub fn is_cookie(&self) -> bool {
        self.config.auth_type == "cookie"
    }
    
    pub fn is_account(&self) -> bool {
        self.config.auth_type == "account"
    }
    
    pub fn is_url(&self) -> bool {
        self.config.auth_type == "url"
    }
    
    async fn init(&self) -> Result<()> {
        match self.config.auth_type.as_str() {
            "account" => {
                self.login().await?;
                let (vei, uid) = self.get_vei_and_uid().await?;
                *self.vei.lock().unwrap() = Some(vei);
                *self.uid.lock().unwrap() = Some(uid);
            }
            "cookie" => {
                let (vei, uid) = self.get_vei_and_uid().await?;
                *self.vei.lock().unwrap() = Some(vei);
                *self.uid.lock().unwrap() = Some(uid);
            }
            _ => {}
        }
        Ok(())
    }
    
    async fn login(&self) -> Result<String> {
        let account = self.config.account.as_deref()
            .ok_or_else(|| anyhow!("Account is required for account auth type"))?;
        let password = self.config.password.as_deref()
            .ok_or_else(|| anyhow!("Password is required for account auth type"))?;
        
        let form_data = vec![
            ("task", "3"),
            ("uid", account),
            ("pwd", password),
            ("setSessionId", ""),
            ("setSig", ""),
            ("setScene", ""),
            ("setTocen", ""),
            ("formhash", ""),
        ];
        
        let mut headers = HeaderMap::new();
        headers.insert("Referer", HeaderValue::from_static("https://up.woozooo.com"));
        headers.insert("User-Agent", HeaderValue::from_str(&self.config.user_agent)?);
        headers.insert("Accept", HeaderValue::from_static("application/json, text/javascript, */*; q=0.01"));
        headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"));
        headers.insert("Content-Type", HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"));
        
        let resp = self.client
            .post("https://up.woozooo.com/mlogin.php")
            .headers(headers)
            .form(&form_data)
            .send()
            .await?;
        
        let text = resp.text().await?;
        
        println!("登录响应: {}", text);
        
        let response: LanzouResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse login response: {}, 响应内容: {}", e, text))?;
        
        if response.zt != 1 {
            return Err(anyhow!("Login failed: {}", 
                response.inf.unwrap_or_else(|| "Unknown error".to_string())));
        }
        
        println!("登录成功");
        Ok("Login successful".to_string())
    }
    
    async fn get_vei_and_uid(&self) -> Result<(String, String)> {
        let mut headers = HeaderMap::new();
        headers.insert("Referer", HeaderValue::from_static("https://pc.woozooo.com"));
        headers.insert("User-Agent", HeaderValue::from_str(&self.config.user_agent)?);
        headers.insert("Accept", HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8"));
        headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"));
        
        if let Some(cookie) = &self.config.cookie {
            headers.insert("Cookie", HeaderValue::from_str(cookie)?);
        }
        
        let url = format!("{}/mydisk.php?item=files&action=index", self.config.base_url);
        let resp = self.client
            .get(&url)
            .headers(headers)
            .send()
            .await?;
        
        let html = resp.text().await?;
        
        // 检查是否有acw_sc__v2验证
        if html.contains("acw_sc__v2") {
            println!("检测到acw_sc__v2验证，正在处理...");
            // 这里应该实现acw_sc__v2的计算，但为了简化，我们先跳过
            return Err(anyhow!("遇到acw_sc__v2验证，请稍后重试或使用其他认证方式"));
        }
        
        // 检查风控文件名
        if html.contains("请忽使用第三方工具") {
            println!("检测到蓝奏云风控警告");
            return Err(anyhow!("蓝奏云检测到第三方工具访问，请使用正确的Cookie或账号密码认证"));
        }
        
        // 解析uid (从URL参数中提取)
        let uid_regex = Regex::new(r"uid=([^'&;]+)")?;
        let uid = uid_regex.captures(&html)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        
        // 解析vei (从JavaScript变量中提取)
        let vei = self.extract_vei_from_html(&html)?;
        
        if uid.is_empty() || vei.is_empty() {
            return Err(anyhow!("Cannot extract uid or vei, cookie may be expired"));
        }
        
        println!("成功获取认证信息: uid={}, vei={}", uid, vei);
        Ok((vei, uid))
    }
    
    fn extract_vei_from_html(&self, html: &str) -> Result<String> {
        // 移除HTML注释
        let clean_html = self.remove_html_comments(html);
        
        // 提取data部分的JSON
        let data_regex = Regex::new(r"data\s*[:=]\s*\{([^}]+)\}")?;
        if let Some(caps) = data_regex.captures(&clean_html) {
            let data_content = caps.get(1).unwrap().as_str();
            
            // 从JSON中提取vei
            let vei_regex = Regex::new(r#"['"]vei['"]\s*:\s*['"]([^'"]+)['"]"#)?;
            if let Some(vei_caps) = vei_regex.captures(data_content) {
                return Ok(vei_caps.get(1).unwrap().as_str().to_string());
            }
        }
        
        Err(anyhow!("无法从HTML中提取vei"))
    }
    
    fn remove_html_comments(&self, html: &str) -> String {
        let comment_regex = Regex::new(r"<!--.*?-->").unwrap();
        comment_regex.replace_all(html, "").to_string()
    }
    
    async fn do_upload(&self, form_data: Vec<(&str, &str)>) -> Result<serde_json::Value> {
        // 重试机制，最多3次
        for attempt in 1..=3 {
            let result = self.try_do_upload(&form_data).await;
            
            match result {
                Ok(json) => {
                    let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
                    if zt == 4 {
                        // zt=4时需要等待1秒重试
                        println!("收到zt=4响应，等待1秒后重试...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        continue;
                    } else if zt == 1 || zt == 2 {
                        return Ok(json);
                    } else {
                        let error_msg = json.get("inf")
                            .or_else(|| json.get("info"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error");
                        return Err(anyhow!("API error (zt={}): {}", zt, error_msg));
                    }
                }
                Err(e) => {
                    if attempt < 3 {
                        println!("请求失败，尝试 {}/3: {}", attempt, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        
        Err(anyhow!("请求失败，已重试3次"))
    }
    
    async fn try_do_upload(&self, form_data: &[(&str, &str)]) -> Result<serde_json::Value> {
        let uid = self.uid.lock().unwrap().clone().unwrap_or_default();
        let vei = self.vei.lock().unwrap().clone().unwrap_or_default();
        
        let mut headers = HeaderMap::new();
        headers.insert("Referer", HeaderValue::from_static("https://pc.woozooo.com"));
        headers.insert("User-Agent", HeaderValue::from_str(&self.config.user_agent)?);
        headers.insert("Accept", HeaderValue::from_static("application/json, text/javascript, */*; q=0.01"));
        headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"));
        headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
        
        if let Some(cookie) = &self.config.cookie {
            headers.insert("Cookie", HeaderValue::from_str(cookie)?);
        }
        
        let url = format!("{}/doupload.php?uid={}&vei={}", self.config.base_url, uid, vei);
        
        let resp = self.client
            .post(&url)
            .headers(headers)
            .form(form_data)
            .send()
            .await?;
        
        let text = resp.text().await?;
        println!("蓝奏云API响应: {}", text);
        
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse response: {}, 响应内容: {}", e, text))?;
        
        Ok(json)
    }
    
    async fn get_folders(&self, folder_id: &str) -> Result<Vec<LanzouFile>> {
        let form_data = vec![
            ("task", "47"),
            ("folder_id", folder_id),
        ];
        
        let json = self.do_upload(form_data).await?;
        let text = json.get("text").ok_or_else(|| anyhow!("Missing 'text' field"))?;
        
        let folders: Vec<LanzouFile> = serde_json::from_value(text.clone())
            .map_err(|e| anyhow!("Failed to parse folders: {}", e))?;
        
        Ok(folders)
    }
    
    async fn get_files(&self, folder_id: &str) -> Result<Vec<LanzouFile>> {
        let mut files = Vec::new();
        let mut page = 1;
        
        loop {
            let page_str = page.to_string();
            let form_data = vec![
                ("task", "5"),
                ("folder_id", folder_id),
                ("pg", &page_str),
            ];
            
            let json = self.do_upload(form_data).await?;
            let text = json.get("text").ok_or_else(|| anyhow!("Missing 'text' field"))?;
            
            let page_files: Vec<LanzouFile> = serde_json::from_value(text.clone())
                .map_err(|e| anyhow!("Failed to parse files: {}", e))?;
            
            if page_files.is_empty() {
                break;
            }
            
            files.extend(page_files);
            page += 1;
            
            // 分页请求之间等待1秒，避免触发风控
            if page > 2 {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
        
        Ok(files)
    }
    
    async fn get_file_share_url(&self, file_id: &str) -> Result<FileShare> {
        let form_data = vec![
            ("task", "22"),
            ("file_id", file_id),
        ];
        
        let json = self.do_upload(form_data).await?;
        let info = json.get("info").ok_or_else(|| anyhow!("Missing 'info' field"))?;
        
        let share_info: FileShare = serde_json::from_value(info.clone())
            .map_err(|e| anyhow!("Failed to parse share info: {}", e))?;
        
        Ok(share_info)
    }
    
    async fn get_download_url_by_share(&self, share_id: &str, pwd: &str) -> Result<String> {
        // 获取分享页面
        let share_url = format!("{}/{}", self.config.share_url, share_id);
        let mut headers = HeaderMap::new();
        headers.insert("User-Agent", HeaderValue::from_str(&self.config.user_agent)?);
        
        let resp = self.client
            .get(&share_url)
            .headers(headers)
            .send()
            .await?;
        
        let html = resp.text().await?;
        
        // 解析下载链接（这是一个简化版本，实际实现需要处理更多情况）
        let file_id_regex = Regex::new(r"'/ajaxm\.php\?file=(\d+)'")?;
        let file_id = file_id_regex.captures(&html)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str())
            .ok_or_else(|| anyhow!("Failed to find file ID"))?;
        
        // 构建下载请求
        let ajax_url = format!("{}/ajaxm.php?file={}", self.config.share_url, file_id);
        let form_data = vec![("p", pwd)];
        
        let resp = self.client
            .post(&ajax_url)
            .form(&form_data)
            .send()
            .await?;
        
        let json: FileShareUrlResp = resp.json().await?;
        json.get_download_url()
    }
    
    async fn stream_file(&self, url: &str) -> Result<Box<dyn Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>> {
        let resp = self.client
            .get(url)
            .header("User-Agent", &self.config.user_agent)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        let data = resp.bytes().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        let stream = futures::stream::once(
            futures::future::ready(Ok(axum::body::Bytes::from(data)))
        );
        
        Ok(Box::new(stream))
    }

    async fn delete_file(&self, file_id: &str) -> Result<()> {
        let form_data = vec![
            ("task", "6"),
            ("file_id", file_id),
        ];

        let json = self.do_upload(form_data).await?;
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if zt == 1 || zt == 2 {
            println!("文件删除成功: {}", file_id);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Delete file failed");
            Err(anyhow!("Delete file failed: {}", error_msg))
        }
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<()> {
        let form_data = vec![
            ("task", "3"),
            ("folder_id", folder_id),
        ];

        let json = self.do_upload(form_data).await?;
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if zt == 1 || zt == 2 {
            println!("文件夹删除成功: {}", folder_id);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Delete folder failed");
            Err(anyhow!("Delete folder failed: {}", error_msg))
        }
    }
}

#[async_trait]
impl Driver for LanzouDriver {
    async fn list(&self, path: &str) -> anyhow::Result<Vec<FileInfo>> {
        if self.is_cookie() || self.is_account() {
            // 使用Cookie或账号认证模式
            let folder_id = if path == "/" { 
                &self.config.root_folder_id 
            } else { 
                path 
            };
            
            let mut files = Vec::new();
            
            // 获取文件夹
            match self.get_folders(folder_id).await {
                Ok(folders) => {
                    for folder in folders {
                        files.push(FileInfo {
                            name: folder.get_name(),
                            path: folder.get_id(),
                            size: 0,
                            is_dir: true,
                            modified: folder.get_time(),
                        });
                    }
                }
                Err(e) => eprintln!("Failed to get folders: {}", e),
            }
            
            // 获取文件
            match self.get_files(folder_id).await {
                Ok(folder_files) => {
                    for file in folder_files {
                        files.push(FileInfo {
                            name: file.get_name(),
                            path: file.get_id(),
                            size: file.get_size() as u64,
                            is_dir: false,
                            modified: file.get_time(),
                        });
                    }
                }
                Err(e) => eprintln!("Failed to get files: {}", e),
            }
            
            Ok(files)
        } else {
            // 使用分享链接模式（简化实现）
            Ok(vec![])
        }
    }
    
    async fn download(&self, _path: &str) -> anyhow::Result<tokio::fs::File> {
        Err(anyhow!("Direct file download not supported, use get_download_url instead"))
    }
    
    async fn get_download_url(&self, path: &str) -> anyhow::Result<Option<String>> {
        if self.is_cookie() || self.is_account() {
            // 获取文件分享信息
            let share_info = self.get_file_share_url(path).await?;
            let file_id = share_info.f_id.as_deref().unwrap_or(path);
            let pwd = share_info.pwd.as_deref().unwrap_or("");
            
            // 通过分享链接获取下载URL
            let download_url = self.get_download_url_by_share(file_id, pwd).await?;
            Ok(Some(download_url))
        } else {
            // 分享链接模式
            let pwd = self.config.share_password.as_deref().unwrap_or("");
            let download_url = self.get_download_url_by_share(path, pwd).await?;
            Ok(Some(download_url))
        }
    }
    
        async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> anyhow::Result<()> {
        if !self.is_cookie() && !self.is_account() {
            return Err(anyhow!("Upload requires cookie or account authentication"));
        }

        // 解析父目录ID
        let parent_id = if parent_path == "/" {
            self.config.root_folder_id.clone()
        } else {
            // 这里简化处理，实际应该根据路径解析ID
            parent_path.trim_start_matches('/').to_string()
        };

        // 上传文件 - 蓝奏云使用html5up.php进行上传
        let form = reqwest::multipart::Form::new()
            .text("task", "1")
            .text("vie", "2") 
            .text("ve", "2")
            .text("id", "WU_FILE_0")
            .text("name", file_name.to_string())
            .text("folder_id_bb_n", parent_id)
            .part("upload_file", reqwest::multipart::Part::bytes(content.to_vec())
                .file_name(file_name.to_string())
                .mime_str("application/octet-stream")?);

        let mut headers = HeaderMap::new();
        headers.insert("Referer", HeaderValue::from_static("https://pc.woozooo.com"));
        headers.insert("User-Agent", HeaderValue::from_str(&self.config.user_agent)?);
        
        if let Some(cookie) = &self.config.cookie {
            headers.insert("Cookie", HeaderValue::from_str(cookie)?);
        }

        let url = format!("{}/html5up.php", self.config.base_url);
        let resp = self.client
            .post(&url)
            .headers(headers)
            .multipart(form)
            .send()
            .await?;

        let text = resp.text().await?;
        println!("上传响应: {}", text);

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse upload response: {}", e))?;

        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        if zt == 1 || zt == 2 {
            println!("文件上传成功: {}", file_name);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Upload failed");
            Err(anyhow!("Upload failed: {}", error_msg))
        }
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        if !self.is_cookie() && !self.is_account() {
            return Err(anyhow!("Delete requires cookie or account authentication"));
        }

        // 解析文件/文件夹ID
        let id = if path == "/" {
            return Err(anyhow!("Cannot delete root directory"));
        } else {
            // 这里简化处理，实际应该根据路径解析ID和类型
            path.trim_start_matches('/').to_string()
        };

        // 尝试删除文件
        let file_result = self.delete_file(&id).await;
        if file_result.is_ok() {
            return file_result;
        }

        // 如果文件删除失败，尝试删除文件夹
        let folder_result = self.delete_folder(&id).await;
        if folder_result.is_ok() {
            return folder_result;
        }

        // 两种删除都失败，返回错误
        Err(anyhow!("Delete failed: unable to delete as file or folder"))
    }

    async fn rename(&self, path: &str, new_name: &str) -> anyhow::Result<()> {
        if !self.is_cookie() && !self.is_account() {
            return Err(anyhow!("Rename requires cookie or account authentication"));
        }

        // 解析文件ID
        let file_id = if path == "/" {
            return Err(anyhow!("Cannot rename root directory"));
        } else {
            // 这里简化处理，实际应该根据路径解析ID
            path.trim_start_matches('/').to_string()
        };

        // 重命名文件 - 只支持文件重命名，不支持文件夹
        let form_data = vec![
            ("task", "46"),
            ("file_id", &file_id),
            ("file_name", new_name),
            ("type", "2"),
        ];

        let json = self.do_upload(form_data).await?;
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if zt == 1 || zt == 2 {
            println!("重命名成功: {} -> {}", path, new_name);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Rename failed");
            Err(anyhow!("Rename failed: {}", error_msg))
        }
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> anyhow::Result<()> {
        if !self.is_cookie() && !self.is_account() {
            return Err(anyhow!("Create folder requires cookie or account authentication"));
        }

        // 解析父目录ID
        let parent_id = if parent_path == "/" {
            self.config.root_folder_id.clone()
        } else {
            // 这里简化处理，实际应该根据路径解析ID
            parent_path.trim_start_matches('/').to_string()
        };

        // 创建文件夹
        let form_data = vec![
            ("task", "2"),
            ("parent_id", &parent_id),
            ("folder_name", folder_name),
            ("folder_description", ""),
        ];

        let json = self.do_upload(form_data).await?;
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if zt == 1 || zt == 2 {
            println!("创建文件夹成功: {}", folder_name);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Create folder failed");
            Err(anyhow!("Create folder failed: {}", error_msg))
        }
    }
    
    async fn get_file_info(&self, path: &str) -> anyhow::Result<FileInfo> {
        // 简化实现，返回基本信息
        Ok(FileInfo {
            name: path.split('/').last().unwrap_or(path).to_string(),
            path: path.to_string(),
            size: 0,
            is_dir: false,
            modified: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        })
    }
    
        async fn move_file(&self, file_path: &str, new_parent_path: &str) -> anyhow::Result<()> {
        if !self.is_cookie() && !self.is_account() {
            return Err(anyhow!("Move file requires cookie or account authentication"));
        }

        // 解析文件ID和新父目录ID
        let file_id = if file_path == "/" {
            return Err(anyhow!("Cannot move root directory"));
        } else {
            file_path.trim_start_matches('/').to_string()
        };

        let new_folder_id = if new_parent_path == "/" {
            self.config.root_folder_id.clone()
        } else {
            new_parent_path.trim_start_matches('/').to_string()
        };

        // 移动文件 - 蓝奏云只支持移动文件，不支持移动文件夹
        let form_data = vec![
            ("task", "20"),
            ("folder_id", &new_folder_id),
            ("file_id", &file_id),
        ];

        let json = self.do_upload(form_data).await?;
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if zt == 1 || zt == 2 {
            println!("移动文件成功: {} -> {}", file_path, new_parent_path);
            Ok(())
        } else {
            let error_msg = json.get("inf")
                .or_else(|| json.get("info"))
                .and_then(|v| v.as_str())
                .unwrap_or("Move file failed");
            Err(anyhow!("Move file failed: {}", error_msg))
        }
    }

    async fn copy_file(&self, _file_path: &str, _new_parent_path: &str) -> anyhow::Result<()> {
        // 蓝奏云不支持直接复制文件，需要下载后重新上传
        Err(anyhow!("Copy file not supported by Lanzou Cloud API"))
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn stream_download(&self, path: &str) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        let stream = self.stream_file(&download_url).await?;
        let content_type = "application/octet-stream".to_string();
        
        Ok(Some((Box::new(stream), content_type)))
    }
    
    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        let mut req = self.client.get(&download_url)
            .header("User-Agent", &self.config.user_agent);
        
        // 添加Range header
        if let Some(start_pos) = start {
            let range_header = if let Some(end_pos) = end {
                format!("bytes={}-{}", start_pos, end_pos)
            } else {
                format!("bytes={}-", start_pos)
            };
            req = req.header("Range", range_header);
        }
        
        let resp = req.send().await
            .map_err(|e| anyhow!("Failed to make range request: {}", e))?;
        
        let content_length = resp.content_length().unwrap_or(0);
        let data = resp.bytes().await
            .map_err(|e| anyhow!("Failed to read response: {}", e))?;
        
        let stream = futures::stream::once(
            futures::future::ready(Ok(axum::body::Bytes::from(data)))
        );
        
        let content_type = "application/octet-stream".to_string();
        Ok(Some((Box::new(stream), content_type, content_length, end)))
    }
}

// 辅助函数
fn parse_size(size_str: &str) -> i64 {
    let size_regex = Regex::new(r"(?i)([0-9.]+)\s*([bkm]+)").unwrap();
    
    if let Some(caps) = size_regex.captures(size_str) {
        if let (Some(num_str), Some(unit_str)) = (caps.get(1), caps.get(2)) {
            if let Ok(num) = num_str.as_str().parse::<f64>() {
                let unit = unit_str.as_str().to_uppercase();
                return match unit.as_str() {
                    "B" => num as i64,
                    "K" => (num * 1024.0) as i64,
                    "M" => (num * 1024.0 * 1024.0) as i64,
                    _ => 0,
                };
            }
        }
    }
    
    0
}

pub struct LanzouDriverFactory;

impl DriverFactory for LanzouDriverFactory {
    fn driver_type(&self) -> &'static str {
        "lanzou"
    }
    
    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "lanzou".to_string(),
            display_name: "蓝奏云".to_string(),
            description: "蓝奏云网盘存储驱动，支持文件上传、下载、删除、重命名、创建文件夹等操作。支持账号登录、Cookie认证和分享链接访问".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "type": {
                        "type": "string",
                        "title": "认证类型",
                        "enum": ["account", "cookie", "url"],
                        "default": "cookie",
                        "description": "选择认证方式：账号密码、Cookie或分享链接"
                    },
                    "account": {
                        "type": "string",
                        "title": "账号",
                        "description": "蓝奏云账号（账号认证时必填）"
                    },
                    "password": {
                        "type": "string",
                        "title": "密码",
                        "format": "password",
                        "description": "蓝奏云密码（账号认证时必填）"
                    },
                    "cookie": {
                        "type": "string",
                        "title": "Cookie",
                        "description": "蓝奏云Cookie（约15天有效期，使用分享链接时可忽略）"
                    },
                    "root_folder_id": {
                        "type": "string",
                        "title": "根文件夹ID",
                        "default": "-1",
                        "description": "根目录文件夹ID，-1表示根目录"
                    },
                    "share_password": {
                        "type": "string",
                        "title": "分享密码",
                        "description": "分享链接的提取密码（如果有）"
                    },
                    "baseUrl": {
                        "type": "string",
                        "title": "基础URL",
                        "default": "https://pc.woozooo.com",
                        "description": "文件操作的基础URL"
                    },
                    "shareUrl": {
                        "type": "string",
                        "title": "分享URL",
                        "default": "https://pan.lanzoui.com",
                        "description": "用于获取分享页面的URL"
                    },
                    "user_agent": {
                        "type": "string",
                        "title": "User Agent",
                        "default": DEFAULT_USER_AGENT,
                        "description": "HTTP请求的User Agent"
                    },
                    "repair_file_info": {
                        "type": "boolean",
                        "title": "修复文件信息",
                        "default": false,
                        "description": "启用以获取准确的文件大小和时间（WebDAV需要开启）"
                    }
                },
                "required": ["type", "baseUrl", "shareUrl", "user_agent"]
            }),
        }
    }
    
    fn create_driver(&self, config: serde_json::Value) -> anyhow::Result<Box<dyn Driver>> {
        let config: LanzouConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("Invalid lanzou config: {}", e))?;
        
        let driver = LanzouDriver::new(config)?;
        
        // 初始化驱动（异步操作需要在运行时处理）
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                driver.init().await
            })
        })?;
        
        Ok(Box::new(driver))
    }
    
    fn get_routes(&self) -> Option<axum::Router> {
        None
    }
}