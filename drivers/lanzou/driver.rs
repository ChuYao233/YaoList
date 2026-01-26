//! 蓝奏云驱动核心实现

use std::ops::Range;
use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result, Context};
use async_trait::async_trait;
use reqwest::{Client, redirect::Policy};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo, ProgressCallback};
use super::config::{LanzouConfig, LoginType};
use super::types::*;
use super::utils::*;
use regex::Regex;

/// 缓存的下载链接
#[derive(Debug, Clone)]
struct CachedDownloadUrl {
    url: String,
    expire_at: DateTime<Utc>,
}

/// 蓝奏云驱动
pub struct LanzouDriver {
    config: LanzouConfig,
    client: Client,
    no_redirect_client: Client,
    cookie: RwLock<String>,
    download_cache: RwLock<HashMap<String, CachedDownloadUrl>>,
    initialized: RwLock<bool>,
    /// API请求需要的uid参数
    uid: RwLock<String>,
    /// API请求需要的vei参数
    vei: RwLock<String>,
    /// 路径到文件ID的缓存 (path -> file_id)
    path_cache: RwLock<HashMap<String, String>>,
}

impl LanzouDriver {
    /// 同步创建驱动实例，延迟初始化到首次使用
    pub fn new(config: LanzouConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(&config.user_agent)
            .cookie_store(true)
            .build()
            .unwrap();
        
        let no_redirect_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(&config.user_agent)
            .redirect(Policy::none())
            .build()
            .unwrap();
        
        Self {
            config: config.clone(),
            client,
            no_redirect_client,
            cookie: RwLock::new(config.cookie.clone()),
            download_cache: RwLock::new(HashMap::new()),
            initialized: RwLock::new(false),
            uid: RwLock::new(String::new()),
            vei: RwLock::new(String::new()),
            path_cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// 确保已初始化（登录验证）
    async fn ensure_initialized(&self) -> Result<()> {
        let initialized = *self.initialized.read().await;
        if initialized {
            return Ok(());
        }
        
        let mut init_guard = self.initialized.write().await;
        if *init_guard {
            return Ok(());
        }
        
        // 根据登录类型初始化
        match self.config.login_type {
            LoginType::Account => {
                self.login_with_account().await?;
            }
            LoginType::Cookie => {
                self.verify_cookie().await?;
            }
            LoginType::Url => {
                // 分享链接模式不需要登录
            }
        }
        
        *init_guard = true;
        Ok(())
    }
    
    /// 使用账号密码登录
    async fn login_with_account(&self) -> Result<()> {
        // 登录URL: https://up.woozooo.com/mlogin.php
        let url = LOGIN_URL;
        
        let mut params = HashMap::new();
        params.insert("task", "3");
        params.insert("uid", self.config.account.as_str());
        params.insert("pwd", self.config.password.as_str());
        params.insert("setSessionId", "");
        params.insert("setSig", "");
        params.insert("setScene", "");
        params.insert("setTocen", "");
        params.insert("formhash", "");
        
        let headers = build_headers(&self.config.user_agent, None);
        
        let response = self.no_redirect_client.post(url)
            .headers(headers)
            .form(&params)
            .send()
            .await
            .context("登录请求失败")?;
        
        // 保存Cookie
        let mut cookie_str = String::new();
        for cookie in response.cookies() {
            if !cookie_str.is_empty() {
                cookie_str.push_str("; ");
            }
            cookie_str.push_str(&format!("{}={}", cookie.name(), cookie.value()));
        }
        
        let json: serde_json::Value = response.json().await
            .context("解析登录响应失败")?;
        
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        if zt != 1 {
            let info = json.get("info").and_then(|v| v.as_str())
                .or_else(|| json.get("inf").and_then(|v| v.as_str()))
                .unwrap_or("未知错误");
            return Err(anyhow!("登录失败: {}", info));
        }
        
        // 保存Cookie
        if !cookie_str.is_empty() {
            *self.cookie.write().await = cookie_str;
        }
        
        // 获取vei和uid参数
        self.fetch_vei_and_uid().await?;
        
        tracing::info!("蓝奏云账号登录成功");
        Ok(())
    }
    
    /// 验证Cookie是否有效并获取vei/uid
    async fn verify_cookie(&self) -> Result<()> {
        let cookie = self.cookie.read().await;
        if cookie.is_empty() {
            return Err(anyhow!("Cookie为空"));
        }
        drop(cookie);
        
        // 获取vei和uid参数，同时验证Cookie
        self.fetch_vei_and_uid().await?;
        
        tracing::info!("蓝奏云Cookie验证成功");
        Ok(())
    }
    
    /// 从 mydisk.php 获取 vei 和 uid 参数
    async fn fetch_vei_and_uid(&self) -> Result<()> {
        let cookie = self.cookie.read().await.clone();
        let url = format!("{}/mydisk.php?item=files&action=index", BASE_URL);
        let headers = build_headers(&self.config.user_agent, Some(&cookie));
        
        let response = self.client.get(&url)
            .headers(headers)
            .send()
            .await
            .context("获取vei/uid请求失败")?;
        
        let html = response.text().await?;
        
        // 检查是否登录失效
        if html.contains("登录") && html.contains("账号") && !html.contains("uid=") {
            return Err(anyhow!("Cookie已失效，请重新获取"));
        }
        
        // 提取uid: uid=xxx
        let uid_re = Regex::new(r#"uid=([^'"&;]+)"#).unwrap();
        let uid = uid_re.captures(&html)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| anyhow!("无法提取uid参数"))?;
        
        // 提取vei: 从 data:{...} 中查找
        let html_clean = remove_html_comments(&html);
        let vei = extract_js_var(&html_clean, "vei")
            .or_else(|| {
                // 尝试从 data 对象中提取
                let data_re = Regex::new(r#"'vei'\s*:\s*'([^']+)'"#).ok()?;
                data_re.captures(&html_clean)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
            })
            .unwrap_or_default();
        
        *self.uid.write().await = uid;
        *self.vei.write().await = vei;
        
        tracing::debug!("蓝奏云获取uid/vei成功");
        Ok(())
    }
    
    /// 通过路径获取文件/文件夹ID
    async fn get_fid_by_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        
        // 根目录返回配置的root_folder_id
        if path.is_empty() {
            return Ok(self.config.root_folder_id.clone());
        }
        
        // 从缓存中查找
        if let Some(fid) = self.path_cache.read().await.get(path) {
            return Ok(fid.clone());
        }
        
        // 缓存未命中，需要遍历路径来查找
        let parts: Vec<&str> = path.split('/').collect();
        let mut current_folder_id = self.config.root_folder_id.clone();
        let mut current_path = String::new();
        
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            
            current_path = if current_path.is_empty() {
                part.to_string()
            } else {
                format!("{}/{}", current_path, part)
            };
            
            // 检查缓存
            if let Some(fid) = self.path_cache.read().await.get(&current_path) {
                current_folder_id = fid.clone();
                continue;
            }
            
            // 需要列出父目录来查找
            let cookie = self.cookie.read().await.clone();
            
            // 先在文件夹中查找
            let folders = self.get_folders(&cookie, &current_folder_id).await?;
            let mut found = false;
            for folder in folders {
                let fp = if current_path.contains('/') {
                    let parent = current_path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                    if parent.is_empty() {
                        folder.name.clone()
                    } else {
                        format!("{}/{}", parent, folder.name)
                    }
                } else {
                    folder.name.clone()
                };
                self.path_cache.write().await.insert(fp.clone(), folder.fol_id.clone());
                if folder.name == *part {
                    current_folder_id = folder.fol_id;
                    found = true;
                    break;
                }
            }
            
            if !found {
                // 在文件中查找
                let files = self.get_files(&cookie, &current_folder_id).await?;
                for file in files {
                    let file_name = file.get_name();
                    let fp = if current_path.contains('/') {
                        let parent = current_path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                        if parent.is_empty() {
                            file_name.to_string()
                        } else {
                            format!("{}/{}", parent, file_name)
                        }
                    } else {
                        file_name.to_string()
                    };
                    self.path_cache.write().await.insert(fp.clone(), file.id.clone());
                    if file_name == *part {
                        // 如果是最后一个部分，返回文件ID
                        if i == parts.len() - 1 {
                            return Ok(file.id);
                        }
                        // 文件不能作为父目录
                        return Err(anyhow!("路径不存在: {}", path));
                    }
                }
                return Err(anyhow!("路径不存在: {}", path));
            }
        }
        
        Ok(current_folder_id)
    }
    
    /// 列出文件夹内容（已登录模式）
    async fn list_folder_logged_in(&self, folder_id: &str, base_path: &str) -> Result<Vec<Entry>> {
        let cookie = self.cookie.read().await;
        let mut entries = Vec::new();
        let base = if base_path.is_empty() || base_path == "/" { 
            String::new() 
        } else { 
            base_path.trim_end_matches('/').to_string() 
        };
        
        // 获取文件夹列表
        let folders = self.get_folders(&cookie, folder_id).await?;
        for folder in folders {
            let fp = format!("{}/{}", base, folder.name);
            // 缓存路径到文件夹ID的映射
            self.path_cache.write().await.insert(
                fp.trim_start_matches('/').to_string(), 
                folder.fol_id.clone()
            );
            entries.push(Entry {
                name: folder.name,
                path: fp,
                size: 0,
                is_dir: true,
                modified: None,
            });
        }
        
        // 获取文件列表
        let files = self.get_files(&cookie, folder_id).await?;
        for file in files {
            let file_name = file.get_name().to_string();
            let fp = format!("{}/{}", base, file_name);
            // 缓存路径到文件ID的映射
            self.path_cache.write().await.insert(
                fp.trim_start_matches('/').to_string(), 
                file.id.clone()
            );
            entries.push(Entry {
                name: file_name,
                path: fp,
                size: parse_size(&file.size),
                is_dir: false,
                modified: parse_time(&file.time).map(|dt| dt.to_rfc3339()),
            });
        }
        
        Ok(entries)
    }
    
    /// 获取文件夹列表
    async fn get_folders(&self, cookie: &str, folder_id: &str) -> Result<Vec<FolderInfo>> {
        let uid = self.uid.read().await.clone();
        let vei = self.vei.read().await.clone();
        let url = format!("{}/doupload.php?uid={}&vei={}", BASE_URL, uid, vei);
        
        let mut params = HashMap::new();
        params.insert("task", "47");
        params.insert("folder_id", folder_id);
        
        let headers = build_headers(&self.config.user_agent, Some(cookie));
        
        let response = self.client.post(&url)
            .headers(headers)
            .form(&params)
            .send()
            .await?;
        
        let json: FolderListResponse = response.json().await?;
        
        Ok(json.text.unwrap_or_default())
    }
    
    /// 获取文件列表
    async fn get_files(&self, cookie: &str, folder_id: &str) -> Result<Vec<FileInfo>> {
        let uid = self.uid.read().await.clone();
        let vei = self.vei.read().await.clone();
        let url = format!("{}/doupload.php?uid={}&vei={}", BASE_URL, uid, vei);
        
        let mut params = HashMap::new();
        params.insert("task", "5");
        params.insert("folder_id", folder_id);
        params.insert("pg", "1");
        
        let headers = build_headers(&self.config.user_agent, Some(cookie));
        
        let response = self.client.post(&url)
            .headers(headers)
            .form(&params)
            .send()
            .await?;
        
        let json: FileListResponse = response.json().await?;
        
        Ok(json.text.unwrap_or_default())
    }
    
    /// 获取文件的分享信息
    async fn get_file_share_info(&self, file_id: &str) -> Result<FileShare> {
        let cookie = self.cookie.read().await;
        let uid = self.uid.read().await.clone();
        let vei = self.vei.read().await.clone();
        let url = format!("{}/doupload.php?uid={}&vei={}", BASE_URL, uid, vei);
        
        let mut params = HashMap::new();
        params.insert("task", "22");
        params.insert("file_id", file_id);
        
        let headers = build_headers(&self.config.user_agent, Some(&cookie));
        
        let response = self.client.post(&url)
            .headers(headers)
            .form(&params)
            .send()
            .await?;
        
        let json: ApiResponse<FileShare> = response.json().await?;
        
        json.info.ok_or_else(|| anyhow!("获取文件分享信息失败"))
    }
    
    /// 获取文件下载链接（已登录模式）
    async fn get_download_url_logged_in(&self, file_id: &str) -> Result<String> {
        // 检查缓存
        {
            let cache = self.download_cache.read().await;
            if let Some(cached) = cache.get(file_id) {
                if cached.expire_at > Utc::now() {
                    return Ok(cached.url.clone());
                }
            }
        }
        
        // 获取分享信息 (task=22)
        let share_info = self.get_file_share_info(file_id).await?;
        let pwd = if share_info.onof == "1" { share_info.pwd.clone() } else { String::new() };
        
        // 构建分享域名
        let share_domain = if share_info.is_newd.starts_with("http") {
            share_info.is_newd.clone()
        } else if share_info.is_newd.contains('.') {
            format!("https://{}", share_info.is_newd)
        } else {
            format!("https://{}.lanzoux.com", share_info.is_newd)
        };
        
        // 调用GetFilesByShareUrl逻辑获取下载链接
        let final_url = self.get_files_by_share_url(&share_domain, &share_info.f_id, &pwd).await?;
        
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
    
    /// 通过分享链接获取文件下载地址
    async fn get_files_by_share_url(&self, share_domain: &str, share_id: &str, pwd: &str) -> Result<String> {
        let share_url = format!("{}/{}", share_domain, share_id);
        let page_data = self.get_share_url_html(&share_url).await?;
        let page_data = remove_html_comments(&page_data);
        let page_data = remove_js_comments(&page_data);
        
        let (base_url, download_url) = if page_data.contains("pwdload") || page_data.contains("passwddiv") {
            // 需要密码
            let param = self.extract_data_from_html(&page_data)?;
            let file_id_re = Regex::new(r"'/ajaxm\.php\?file=(\d+)'").unwrap();
            let ajax_file_id = file_id_re.captures(&page_data)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .ok_or_else(|| anyhow!("未找到file id"))?;
            
            let mut form_data = param;
            form_data.insert("p".to_string(), pwd.to_string());
            
            let ajax_url = format!("{}/ajaxm.php?file={}", share_domain, ajax_file_id);
            let resp = self.post_form(&ajax_url, &form_data, Some(&share_url)).await?;
            
            let dom = resp.get("dom").and_then(|v| v.as_str()).unwrap_or("");
            let url_path = resp.get("url").and_then(|v| v.as_str()).unwrap_or("");
            (format!("{}/file", dom), format!("{}/file/{}", dom, url_path))
        } else {
            // 不需要密码，获取iframe页面
            let iframe_re = Regex::new(r#"<iframe.*?src="([^"]+)""#).unwrap();
            let iframe_path = iframe_re.captures(&page_data)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .ok_or_else(|| anyhow!("未找到iframe页面参数"))?;
            
            let next_url = format!("{}{}", share_domain, iframe_path);
            let next_page = self.get_share_url_html(&next_url).await?;
            let next_page = remove_html_comments(&next_page);
            
            let param = self.extract_data_from_html(&next_page)?;
            let file_id_re = Regex::new(r"'/ajaxm\.php\?file=(\d+)'").unwrap();
            let ajax_file_id = file_id_re.captures(&next_page)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .ok_or_else(|| anyhow!("未找到file id"))?;
            
            let ajax_url = format!("{}/ajaxm.php?file={}", share_domain, ajax_file_id);
            let resp = self.post_form(&ajax_url, &param, Some(&next_url)).await?;
            
            let dom = resp.get("dom").and_then(|v| v.as_str()).unwrap_or("");
            let url_path = resp.get("url").and_then(|v| v.as_str()).unwrap_or("");
            (format!("{}/file", dom), format!("{}/file/{}", dom, url_path))
        };
        
        // 通过302重定向获取最终下载链接
        self.get_redirect_url(&download_url, &base_url).await
    }
    
    /// 获取分享页面HTML (处理acw_sc__v2验证)
    async fn get_share_url_html(&self, url: &str) -> Result<String> {
        let mut acw_value = String::new();
        
        for _ in 0..3 {
            let cookie = if acw_value.is_empty() { None } else { Some(format!("acw_sc__v2={}", acw_value)) };
            let headers = build_headers(&self.config.user_agent, cookie.as_deref());
            
            let response = self.client.get(url).headers(headers).send().await?;
            let html = response.text().await?;
            
            if html.contains("取消分享") { return Err(anyhow!("文件分享已取消")); }
            if html.contains("文件不存在") { return Err(anyhow!("文件不存在")); }
            
            if html.contains("acw_sc__v2") {
                acw_value = calc_acw_sc_v2_from_html(&html)?;
                continue;
            }
            return Ok(html);
        }
        Err(anyhow!("acw_sc__v2验证失败"))
    }
    
    /// 从HTML中提取data对象
    fn extract_data_from_html(&self, html: &str) -> Result<HashMap<String, String>> {
        let data_re = Regex::new(r"data\s*[:\s]+\{([^}]+)\}").unwrap();
        let data_str = data_re.captures(html)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .ok_or_else(|| anyhow!("未找到data对象"))?;
        
        let mut result = HashMap::new();
        let kv_re = Regex::new(r"'([^']+)'\s*:\s*'?([^',}]*)'?").unwrap();
        for cap in kv_re.captures_iter(data_str) {
            if let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) {
                let key = k.as_str().to_string();
                let mut value = v.as_str().trim_matches('\'').to_string();
                if !value.is_empty() && !value.chars().next().unwrap().is_ascii_digit() && !value.starts_with('\'') {
                    if let Some(var_val) = extract_js_var(html, &value) {
                        value = var_val;
                    }
                }
                result.insert(key, value);
            }
        }
        Ok(result)
    }
    
    /// POST表单请求
    async fn post_form(&self, url: &str, data: &HashMap<String, String>, referer: Option<&str>) -> Result<serde_json::Value> {
        let mut headers = build_headers(&self.config.user_agent, None);
        if let Some(r) = referer {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(r) {
                headers.insert(reqwest::header::REFERER, v);
            }
        }
        
        let response = self.client.post(url).headers(headers).form(data).send().await?;
        let text = response.text().await?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析JSON失败: {}, 响应: {}", e, &text[..text.len().min(200)]))?;
        
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        if zt != 1 {
            let info = json.get("inf").and_then(|v| v.as_str())
                .or_else(|| json.get("info").and_then(|v| v.as_str()))
                .unwrap_or("未知错误");
            return Err(anyhow!("请求失败: {}", info));
        }
        Ok(json)
    }
    
    /// 获取302重定向URL
    async fn get_redirect_url(&self, url: &str, referer: &str) -> Result<String> {
        let mut acw_value = String::new();
        
        for _ in 0..3 {
            let cookie = format!("down_ip=1{}", if acw_value.is_empty() { String::new() } else { format!("; acw_sc__v2={}", acw_value) });
            let mut headers = build_headers(&self.config.user_agent, Some(&cookie));
            headers.insert("accept-language", "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
            if let Ok(v) = reqwest::header::HeaderValue::from_str(referer) {
                headers.insert(reqwest::header::REFERER, v);
            }
            
            let response = self.no_redirect_client.get(url).headers(headers).send().await?;
            
            if response.status() == reqwest::StatusCode::FOUND {
                if let Some(location) = response.headers().get("location") {
                    return Ok(location.to_str()?.to_string());
                }
            }
            
            let body = response.text().await?;
            if body.contains("acw_sc__v2") {
                acw_value = calc_acw_sc_v2_from_html(&body)?;
                continue;
            }
            
            // 触发二次验证
            if let Ok(param) = self.extract_data_from_html(&body) {
                let mut form_data = param;
                form_data.insert("el".to_string(), "2".to_string());
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                
                let ajax_url = format!("{}/ajax.php", referer.trim_end_matches("/file"));
                let resp = self.post_form(&ajax_url, &form_data, Some(referer)).await?;
                if let Some(url) = resp.get("url").and_then(|v| v.as_str()) {
                    return Ok(url.to_string());
                }
            }
            return Err(anyhow!("获取下载链接失败"));
        }
        Err(anyhow!("acw_sc__v2验证失败"))
    }
    
    /// 列出分享链接内容（分享链接模式）
    async fn list_share_url(&self) -> Result<Vec<Entry>> {
        let share_info = parse_share_page(
            &self.client,
            &self.config.share_url,
            Some(&self.config.share_password),
            &self.config.user_agent,
        ).await?;
        
        if share_info.is_folder {
            // 文件夹分享
            self.list_share_folder(&share_info).await
        } else {
            // 单文件分享
            Ok(vec![Entry {
                name: share_info.name,
                path: share_info.file_id.unwrap_or_default(),
                size: 0,
                is_dir: false,
                modified: None,
            }])
        }
    }
    
    /// 列出分享文件夹内容
    async fn list_share_folder(&self, share_info: &SharePageInfo) -> Result<Vec<Entry>> {
        let folder_id = share_info.folder_id.as_deref()
            .ok_or_else(|| anyhow!("缺少文件夹ID"))?;
        
        let url = format!("{}/filemoreajax.php", BASE_URL);
        
        let mut params = HashMap::new();
        params.insert("lx", "2");
        params.insert("fid", folder_id);
        params.insert("pg", "1");
        params.insert("k", &self.config.share_password);
        
        let headers = build_headers_with_referer(
            &self.config.user_agent,
            None,
            &self.config.share_url,
        );
        
        let response = self.client.post(&url)
            .headers(headers)
            .form(&params)
            .send()
            .await?;
        
        let json: FileListResponse = response.json().await?;
        
        let files = json.text.unwrap_or_default();
        let entries = files.into_iter().map(|f| Entry {
            name: f.get_name().to_string(),
            path: f.id.clone(),
            size: parse_size(&f.size),
            is_dir: false,
            modified: parse_time(&f.time).map(|dt| dt.to_rfc3339()),
        }).collect();
        
        Ok(entries)
    }
    
    /// 获取空间信息
    async fn get_space_info_internal(&self) -> Result<SpaceInfo> {
        let cookie = self.cookie.read().await;
        let url = format!("{}/mydisk.php?item=profile", BASE_URL);
        let headers = build_headers(&self.config.user_agent, Some(&cookie));
        
        let response = self.client.get(&url)
            .headers(headers)
            .send()
            .await?;
        
        let html = response.text().await?;
        
        // 从HTML中提取空间信息
        let used = extract_space_value(&html, "已用空间");
        let total = extract_space_value(&html, "总空间");
        
        Ok(SpaceInfo {
            used,
            total,
            free: total.saturating_sub(used),
        })
    }
    
    /// 上传文件 
    async fn upload_file_internal(&self, path: &str, data: bytes::Bytes) -> Result<()> {
        let cookie = self.cookie.read().await.clone();
        let folder_id = self.get_parent_folder_id_async(path).await?;
        let filename = path.rsplit('/').next().unwrap_or(path);
        
        // POST到 /html5up.php
        // 参数: task=1, vie=2, ve=2, id=WU_FILE_0, name=文件名, folder_id_bb_n=目标文件夹ID
        let form = reqwest::multipart::Form::new()
            .text("task", "1")
            .text("vie", "2")
            .text("ve", "2")
            .text("id", "WU_FILE_0")
            .text("name", filename.to_string())
            .text("folder_id_bb_n", folder_id)
            .part("upload_file", reqwest::multipart::Part::bytes(data.to_vec())
                .file_name(filename.to_string())
                .mime_str("application/octet-stream")?);
        
        let url = format!("{}/html5up.php", BASE_URL);
        let headers = build_headers(&self.config.user_agent, Some(&cookie));
        
        let response = self.client.post(&url)
            .headers(headers)
            .multipart(form)
            .send()
            .await?;
        
        let text = response.text().await?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析上传响应失败: {}, 响应: {}", e, &text[..text.len().min(500)]))?;
        
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        if zt != 1 {
            let info = json.get("info").and_then(|v| v.as_str())
                .or_else(|| json.get("inf").and_then(|v| v.as_str()))
                .unwrap_or("未知错误");
            return Err(anyhow!("上传失败: {}", info));
        }
        
        tracing::info!("蓝奏云上传成功: {}", filename);
        Ok(())
    }
    
    /// 异步获取父文件夹ID
    async fn get_parent_folder_id_async(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if let Some(pos) = path.rfind('/') {
            let parent_path = &path[..pos];
            if parent_path.is_empty() {
                return Ok(self.config.root_folder_id.clone());
            }
            self.get_fid_by_path(parent_path).await
        } else {
            Ok(self.config.root_folder_id.clone())
        }
    }
    
    /// 获取父文件夹ID
    fn get_parent_folder_id(&self, path: &str) -> String {
        let path = path.trim_matches('/');
        if let Some(pos) = path.rfind('/') {
            path[..pos].rsplit('/').next().unwrap_or(&self.config.root_folder_id).to_string()
        } else {
            self.config.root_folder_id.clone()
        }
    }
    
    /// 调用doupload API (按照OpenList实现)
    async fn doupload(&self, params: &HashMap<&str, &str>) -> Result<serde_json::Value> {
        let cookie = self.cookie.read().await.clone();
        let uid = self.uid.read().await.clone();
        let vei = self.vei.read().await.clone();
        
        // OpenList: /doupload.php?uid=xxx&vei=xxx
        let url = format!("{}/doupload.php?uid={}&vei={}", BASE_URL, uid, vei);
        
        let mut headers = build_headers(&self.config.user_agent, Some(&cookie));
        headers.insert("Referer", "https://pc.woozooo.com".parse().unwrap());
        
        let response = self.client.post(&url)
            .headers(headers)
            .form(params)
            .send()
            .await?;
        
        let text = response.text().await?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析响应失败: {}, 响应: {}", e, &text[..text.len().min(200)]))?;
        
        let zt = json.get("zt").and_then(|v| v.as_i64()).unwrap_or(0);
        match zt {
            1 | 2 | 4 => Ok(json),
            9 => Err(anyhow!("Cookie已过期")),
            _ => {
                let info = json.get("inf").and_then(|v| v.as_str())
                    .or_else(|| json.get("info").and_then(|v| v.as_str()))
                    .unwrap_or("未知错误");
                Err(anyhow!("{}", info))
            }
        }
    }
    
    /// 删除文件
    async fn delete_file_internal(&self, file_id: &str) -> Result<()> {
        let mut params = HashMap::new();
        params.insert("task", "6");
        params.insert("file_id", file_id);
        
        self.doupload(&params).await?;
        Ok(())
    }
    
    /// 删除文件夹
    async fn delete_folder_internal(&self, folder_id: &str) -> Result<()> {
        let mut params = HashMap::new();
        params.insert("task", "3");
        params.insert("folder_id", folder_id);
        
        self.doupload(&params).await?;
        Ok(())
    }
    
    /// 创建文件夹 - 使用doupload
    async fn create_folder_internal(&self, parent_folder_id: &str, name: &str) -> Result<String> {
        let mut params = HashMap::new();
        params.insert("task", "2");
        params.insert("parent_id", parent_folder_id);
        params.insert("folder_name", name);
        params.insert("folder_description", "");
        
        let json = self.doupload(&params).await?;
        
        // 返回新文件夹ID
        let folder_id = json.get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        
        Ok(folder_id)
    }
}

/// 从HTML中提取空间值
fn extract_space_value(html: &str, label: &str) -> u64 {
    let re = regex::Regex::new(&format!(r"{}[：:]\s*([0-9.]+)\s*([KMGT]?B?)", label)).ok();
    if let Some(re) = re {
        if let Some(caps) = re.captures(html) {
            let num: f64 = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
            let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let multiplier = match unit.to_uppercase().as_str() {
                "K" | "KB" => 1024u64,
                "M" | "MB" => 1024 * 1024,
                "G" | "GB" => 1024 * 1024 * 1024,
                "T" | "TB" => 1024 * 1024 * 1024 * 1024,
                _ => 1,
            };
            return (num * multiplier as f64) as u64;
        }
    }
    0
}

#[async_trait]
impl StorageDriver for LanzouDriver {
    fn name(&self) -> &str {
        "Lanzou"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: false,
            can_append: false,
            can_direct_link: true,
            max_chunk_size: Some(100 * 1024 * 1024), // 100MB限制
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: Some(100 * 1024 * 1024), // 100MB限制
            requires_full_file_for_upload: false, // 蓝奏云支持分块上传
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_initialized().await?;
        
        match self.config.login_type {
            LoginType::Url => {
                self.list_share_url().await
            }
            _ => {
                let folder_id = self.get_fid_by_path(path).await?;
                self.list_folder_logged_in(&folder_id, path).await
            }
        }
    }
    
    async fn open_reader(
        &self,
        path: &str,
        _range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_initialized().await?;
        
        // 通过路径获取文件ID
        let file_id = self.get_fid_by_path(path).await?;
        let download_url = self.get_download_url_logged_in(&file_id).await?;
        
        let headers = build_headers(&self.config.user_agent, None);
        let response = self.client.get(&download_url)
            .headers(headers)
            .send()
            .await?;
        
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
        _path: &str,
        _size_hint: Option<u64>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        // 蓝奏云不支持流式写入，需要使用put方法
        Err(anyhow!("蓝奏云不支持流式写入，请使用put方法"))
    }
    
    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        _progress: Option<ProgressCallback>,
    ) -> Result<()> {
        self.ensure_initialized().await?;
        
        if self.config.login_type == LoginType::Url {
            return Err(anyhow!("分享链接模式不支持上传"));
        }
        
        // 检查文件大小限制（100MB）
        if data.len() > 100 * 1024 * 1024 {
            return Err(anyhow!("文件大小超过100MB限制"));
        }
        
        self.upload_file_internal(path, data).await
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        
        if self.config.login_type == LoginType::Url {
            return Err(anyhow!("分享链接模式不支持删除"));
        }
        
        // 通过路径获取文件/文件夹ID
        let fid = self.get_fid_by_path(path).await?;
        
        // 尝试删除文件，失败则尝试删除文件夹
        if self.delete_file_internal(&fid).await.is_err() {
            self.delete_folder_internal(&fid).await?;
        }
        
        // 更新路径缓存
        self.path_cache.write().await.remove(path.trim_matches('/'));
        
        Ok(())
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        
        if self.config.login_type == LoginType::Url {
            return Err(anyhow!("分享链接模式不支持创建文件夹"));
        }
        
        let parent_folder_id = self.get_parent_folder_id_async(path).await?;
        let name = path.rsplit('/').next().unwrap_or(path);
        
        self.create_folder_internal(&parent_folder_id, name).await?;
        
        Ok(())
    }
    
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        self.ensure_initialized().await?;
        
        if self.config.login_type == LoginType::Url {
            return Err(anyhow!("分享链接模式不支持重命名"));
        }
        
        let file_id = self.get_fid_by_path(old_path).await?;
        
        // 蓝奏云只支持文件重命名 (task=46)
        let mut params = HashMap::new();
        params.insert("task", "46");
        params.insert("file_id", file_id.as_str());
        params.insert("file_name", new_name);
        params.insert("type", "2");
        
        self.doupload(&params).await?;
        
        // 更新路径缓存
        self.path_cache.write().await.remove(old_path.trim_matches('/'));
        
        Ok(())
    }
    
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        
        if self.config.login_type == LoginType::Url {
            return Err(anyhow!("分享链接模式不支持移动"));
        }
        
        let file_id = self.get_fid_by_path(old_path).await?;
        
        // 获取目标文件夹ID
        let new_parent = new_path.trim_matches('/').rsplitn(2, '/').nth(1).unwrap_or("");
        let dest_folder_id = if new_parent.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_fid_by_path(new_parent).await?
        };
        
        // 蓝奏云只支持文件移动 (task=20)
        let mut params = HashMap::new();
        params.insert("task", "20");
        params.insert("folder_id", dest_folder_id.as_str());
        params.insert("file_id", file_id.as_str());
        
        self.doupload(&params).await?;
        
        // 更新路径缓存
        self.path_cache.write().await.remove(old_path.trim_matches('/'));
        
        Ok(())
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        // 蓝奏云不显示存储空间大小
        Ok(None)
    }
    
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        self.ensure_initialized().await?;
        
        tracing::debug!("蓝奏云 get_direct_link: path={}", path);
        
        let file_id = match self.get_fid_by_path(path).await {
            Ok(id) => {
                tracing::debug!("蓝奏云 get_direct_link: file_id={}", id);
                id
            }
            Err(e) => {
                tracing::error!("蓝奏云 get_direct_link: 获取文件ID失败: {}", e);
                return Err(e);
            }
        };
        
        let download_url = match self.get_download_url_logged_in(&file_id).await {
            Ok(url) => {
                tracing::debug!("蓝奏云 get_direct_link: download_url={}", url);
                url
            }
            Err(e) => {
                tracing::error!("蓝奏云 get_direct_link: 获取下载链接失败: {}", e);
                return Err(e);
            }
        };
        
        Ok(Some(download_url))
    }
}
