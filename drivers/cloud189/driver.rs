//! Cloud189 driver implementation / 天翼云盘驱动实现
//! Architecture principles / 架构原则：
//! - Driver only provides primitive capabilities (Reader/Writer) / 驱动只提供原语能力
//! - Core controls progress, concurrency, resume points / Core控制进度
//! - Never load files into memory, use streaming / 永远不把文件放内存

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client, redirect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::RwLock;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, SpaceInfo, StorageDriver,
};

use super::types::*;
use super::utils::*;
use super::login::LoginManager;
use super::writer::Cloud189StreamWriter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cloud189Config {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default = "default_root_folder_id")]
    pub root_folder_id: String,
    #[serde(default = "default_cloud_type")]
    pub cloud_type: String,
    #[serde(default)]
    pub family_id: String,
    #[serde(default = "default_true")]
    pub show_space_info: bool,
}

fn default_root_folder_id() -> String { "-11".to_string() }
fn default_cloud_type() -> String { "personal".to_string() }
fn default_true() -> bool { true }

// Type definitions in types.rs / 类型定义在 types.rs 中

pub struct Cloud189Driver {
    config: Cloud189Config,
    client: Client,
    no_redirect_client: Client,
    token_info: Arc<RwLock<Option<AppSessionResp>>>,
    path_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl Cloud189Driver {
    pub fn new(config: Cloud189Config) -> Self {
        Self {
            config,
            client: Client::builder().cookie_store(true).redirect(redirect::Policy::limited(10)).build().unwrap(),
            no_redirect_client: Client::builder().cookie_store(true).redirect(redirect::Policy::none()).build().unwrap(),
            token_info: Arc::new(RwLock::new(None)),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn is_family(&self) -> bool { self.config.cloud_type == "family" }

    // Utility functions defined in utils.rs / 工具函数在 utils.rs 中定义

    async fn signature_header(&self, url: &str, method: &str, params: &str) -> HashMap<String, String> {
        let date = get_http_date_str();
        let token = self.token_info.read().await;
        let (sk, ss) = token.as_ref().map(|t| {
            if self.is_family() { 
                (t.user_session.family_session_key.clone(), t.user_session.family_session_secret.clone()) 
            } else { 
                (t.user_session.session_key.clone(), t.user_session.session_secret.clone()) 
            }
        }).unwrap_or_default();
        let mut h = HashMap::new();
        h.insert("Date".into(), date.clone()); 
        h.insert("SessionKey".into(), sk.clone()); 
        h.insert("X-Request-ID".into(), uuid::Uuid::new_v4().to_string()); 
        h.insert("Signature".into(), signature_of_hmac(&ss, &sk, method, url, &date, params));
        h
    }

    async fn request<T: for<'de> Deserialize<'de>>(&self, url: &str, method: reqwest::Method, query: Option<Vec<(String, String)>>) -> Result<T> {
        if self.token_info.read().await.is_none() { return Err(anyhow!("Not logged in / 未登录")); }
        let headers = self.signature_header(url, method.as_str(), "").await;
        let mut req = self.client.request(method.clone(), url)
            .header("Accept", "application/json;charset=UTF-8")
            .header("Referer", WEB_URL)
            .header("User-Agent", "Mozilla/5.0");
        for (k, v) in &headers { req = req.header(k.as_str(), v.as_str()); }
        let mut all_query = client_suffix();
        if let Some(q) = query { all_query.extend(q); }
        req = req.query(&all_query);
        let text = req.send().await?.text().await?;
        if text.contains("userSessionBO is null") || text.contains("InvalidSessionKey") { 
            self.refresh_session().await?; 
            return Box::pin(self.request(url, method, None)).await; 
        }
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) { 
            if err.has_error() { return Err(anyhow!("API error / API错误: {}", err.error_message())); } 
        }
        serde_json::from_str(&text).map_err(|e| anyhow!("Parse failed / 解析失败: {} - {}", e, text))
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, url: &str, query: Option<Vec<(String, String)>>) -> Result<T> { 
        self.request(url, reqwest::Method::GET, query).await 
    }
    
    async fn post<T: for<'de> Deserialize<'de>>(&self, url: &str, query: Option<Vec<(String, String)>>) -> Result<T> { 
        self.request(url, reqwest::Method::POST, query).await 
    }

    /// Login - using LoginManager / 登录 - 使用LoginManager
    async fn login_by_password(&self) -> Result<AppSessionResp> {
        let login_manager = LoginManager::new(self.client.clone());
        login_manager.login_by_password(&self.config.username, &self.config.password).await
    }

    /// Refresh Token / 刷新Token
    async fn refresh_token(&self) -> Result<AppSessionResp> {
        let rt = self.token_info.read().await
            .as_ref()
            .map(|t| t.refresh_token.clone())
            .unwrap_or_else(|| self.config.refresh_token.clone());
        if rt.is_empty() { return Err(anyhow!("No refresh_token / 没有refresh_token")); }
        let login_manager = LoginManager::new(self.client.clone());
        login_manager.refresh_token(&rt).await
    }

    /// Refresh Session / 刷新Session
    async fn refresh_session(&self) -> Result<()> {
        let token = if !self.config.refresh_token.is_empty() { 
            self.refresh_token().await.or_else(|_| {
                if !self.config.username.is_empty() { 
                    futures::executor::block_on(self.login_by_password()) 
                } else { 
                    Err(anyhow!("Cannot refresh / 无法刷新")) 
                }
            })? 
        } else if !self.config.username.is_empty() { 
            self.login_by_password().await? 
        } else { 
            return Err(anyhow!("No authentication method / 没有认证方式")); 
        };
        *self.token_info.write().await = Some(token);
        Ok(())
    }

    async fn ensure_authenticated(&self) -> Result<()> { 
        if self.token_info.read().await.is_none() { self.refresh_session().await?; } 
        Ok(()) 
    }

    async fn get_family_id(&self) -> Result<String> {
        if !self.config.family_id.is_empty() { return Ok(self.config.family_id.clone()); }
        let resp: FamilyInfoListResp = self.get(&format!("{}/family/manage/getFamilyList.action", API_URL), None).await?;
        resp.family_info_resp.first().map(|f| f.family_id.to_string()).ok_or_else(|| anyhow!("Family cloud not found / 未找到家庭云"))
    }

    async fn get_files(&self, folder_id: &str, page_num: i32) -> Result<Cloud189FilesResp> {
        let is_family = self.is_family();
        let mut url = API_URL.to_string(); 
        if is_family { url.push_str("/family/file"); } 
        url.push_str("/listFiles.action");
        let mut query = vec![
            ("folderId".into(), folder_id.into()), 
            ("fileType".into(), "0".into()), 
            ("mediaAttr".into(), "0".into()), 
            ("iconOption".into(), "5".into()), 
            ("pageNum".into(), page_num.to_string()), 
            ("pageSize".into(), "1000".into())
        ];
        if is_family { 
            let fid = self.get_family_id().await?; 
            query.extend([("familyId".into(), fid), ("orderBy".into(), "1".into()), ("descending".into(), "false".into())]); 
        } else { 
            query.extend([("recursive".into(), "0".into()), ("orderBy".into(), "filename".into()), ("descending".into(), "false".into())]); 
        }
        self.get(&url, Some(query)).await
    }

    async fn get_fid_by_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() { return Ok(self.config.root_folder_id.clone()); }
        { let cache = self.path_cache.read().await; if let Some(fid) = cache.get(path) { return Ok(fid.clone()); } }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_fid = self.config.root_folder_id.clone();
        let mut current_path = String::new();
        for part in parts {
            if !current_path.is_empty() { current_path.push('/'); } current_path.push_str(part);
            { let cache = self.path_cache.read().await; if let Some(fid) = cache.get(&current_path) { current_fid = fid.clone(); continue; } }
            let resp = self.get_files(&current_fid, 1).await?;
            let found = resp.file_list_ao.folder_list.iter().find(|f| f.name == part).map(|f| f.get_id())
                .or_else(|| resp.file_list_ao.file_list.iter().find(|f| f.name == part).map(|f| f.get_id()));
            if let Some(fid) = found { current_fid = fid.clone(); self.path_cache.write().await.insert(current_path.clone(), fid); } else { return Err(anyhow!("Path does not exist / 路径不存在: /{}", current_path)); }
        }
        Ok(current_fid)
    }

    async fn get_download_url(&self, file_id: &str) -> Result<String> {
        let is_family = self.is_family();
        let mut url = API_URL.to_string(); if is_family { url.push_str("/family/file"); } url.push_str("/getFileDownloadUrl.action");
        let mut query = vec![("fileId".into(), file_id.into())];
        if is_family { query.push(("familyId".into(), self.get_family_id().await?)); } else { query.extend([("dt".into(), "3".into()), ("flag".into(), "1".into())]); }
        let resp: DownloadUrlResp = self.get(&url, Some(query)).await?;
        let download_url = resp.file_download_url.replace("&amp;", "&").replace("http://", "https://");
        let rr = self.no_redirect_client.get(&download_url).header("User-Agent", "Mozilla/5.0").send().await?;
        if rr.status().as_u16() == 302 { if let Some(loc) = rr.headers().get("location") { return Ok(loc.to_str().unwrap_or(&download_url).to_string()); } }
        Ok(download_url)
    }

    async fn create_batch_task(&self, task_type: &str, family_id: Option<&str>, target_folder_id: Option<&str>, task_infos: Vec<BatchTaskInfo>) -> Result<CreateBatchTaskResp> {
        let mut form = vec![("type".into(), task_type.into()), ("taskInfos".into(), serde_json::to_string(&task_infos)?)];
        if let Some(tid) = target_folder_id { if !tid.is_empty() { form.push(("targetFolderId".into(), tid.into())); } }
        if let Some(fid) = family_id { if !fid.is_empty() { form.push(("familyId".into(), fid.into())); } }
        self.post(&format!("{}/batch/createBatchTask.action", API_URL), Some(form)).await
    }

    async fn wait_batch_task(&self, task_type: &str, task_id: &str, delay_ms: u64) -> Result<()> {
        loop {
            let form = vec![("type".into(), task_type.into()), ("taskId".into(), task_id.into())];
            let state: BatchTaskStateResp = self.post(&format!("{}/batch/checkBatchTask.action", API_URL), Some(form)).await?;
            match state.task_status { 2 => return Err(anyhow!("Conflict exists / 存在冲突")), 4 => return Ok(()), _ => tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await }
        }
    }
}

pub struct Cloud189Reader { inner: Pin<Box<dyn AsyncRead + Send + Unpin>> }
impl Cloud189Reader {
    async fn new(url: &str, range: Option<(u64, u64)>) -> Result<Self> {
        let mut req = Client::new().get(url).header("User-Agent", "Mozilla/5.0");
        if let Some((s, e)) = range { req = req.header("Range", format!("bytes={}-{}", s, e)); }
        let resp = req.send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 206 { return Err(anyhow!("Download failed / 下载失败: {}", resp.status())); }
        let stream = resp.bytes_stream().map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
        Ok(Self { inner: Box::pin(tokio_util::io::StreamReader::new(stream)) })
    }
}
impl AsyncRead for Cloud189Reader { fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> { self.inner.as_mut().poll_read(cx, buf) } }


#[async_trait]
impl StorageDriver for Cloud189Driver {
    fn name(&self) -> &str { "天翼云盘" }
    fn version(&self) -> &str { "1.0.0" }
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
            can_batch_operations: true,
            max_file_size: None,
            requires_full_file_for_upload: false,
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(path).await?;
        let mut entries = Vec::new();
        let base = if path.is_empty() || path == "/" { String::new() } else { path.trim_end_matches('/').to_string() };
        for pn in 1.. {
            let resp = self.get_files(&fid, pn).await?;
            if resp.file_list_ao.count == 0 && resp.file_list_ao.folder_list.is_empty() && resp.file_list_ao.file_list.is_empty() { break; }
            for f in resp.file_list_ao.folder_list { 
                let fp = format!("{}/{}", base, f.name); 
                self.path_cache.write().await.insert(fp.trim_start_matches('/').to_string(), f.get_id()); 
                entries.push(Entry { name: f.name, path: fp, size: 0, is_dir: true, modified: Some(f.last_op_time) }); 
            }
            for f in resp.file_list_ao.file_list { 
                let fp = format!("{}/{}", base, f.name); 
                self.path_cache.write().await.insert(fp.trim_start_matches('/').to_string(), f.get_id()); 
                entries.push(Entry { name: f.name, path: fp, size: f.size as u64, is_dir: false, modified: Some(f.last_op_time) }); 
            }
            if resp.file_list_ao.count == 0 { break; }
        }
        Ok(entries)
    }

    async fn open_reader(&self, path: &str, range: Option<Range<u64>>) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(path).await?;
        let url = self.get_download_url(&fid).await?;
        Ok(Box::new(Cloud189Reader::new(&url, range.map(|r| (r.start, r.end.saturating_sub(1)))).await?))
    }

    async fn open_writer(&self, path: &str, size_hint: Option<u64>, progress: Option<crate::storage::ProgressCallback>) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        self.ensure_authenticated().await?;
        
        // Parse path to get parent folder ID and filename / 解析路径获取
        let path = path.trim_matches('/');
        let (parent_path, file_name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos+1..])
        } else {
            ("", path)
        };
        
        let parent_folder_id = if parent_path.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_fid_by_path(parent_path).await?
        };
        
        let is_family = self.is_family();
        let family_id = if is_family { self.get_family_id().await? } else { String::new() };
        
        Ok(Box::new(Cloud189StreamWriter::new(
            self.client.clone(),
            self.token_info.clone(),
            is_family,
            family_id,
            parent_folder_id,
            file_name.to_string(),
            size_hint,
            progress,
        )))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(path).await?;
        let fname = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_folder = { 
            let pp = std::path::Path::new(path.trim_matches('/')).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(); 
            let pid = self.get_fid_by_path(&pp).await?; 
            self.get_files(&pid, 1).await?.file_list_ao.folder_list.iter().any(|f| f.get_id() == fid) 
        };
        let family_id = if self.is_family() { Some(self.get_family_id().await?) } else { None };
        let task = BatchTaskInfo { 
            file_id: fid, 
            file_name: fname.to_string(), 
            is_folder: if is_folder { 1 } else { 0 },
            src_parent_id: None,
            deal_way: None,
            is_conflict: None,
        };
        let resp = self.create_batch_task("DELETE", family_id.as_deref(), None, vec![task]).await?;
        self.wait_batch_task("DELETE", &resp.task_id, 200).await?;
        self.path_cache.write().await.remove(path.trim_matches('/'));
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_authenticated().await?;
        let path = path.trim_matches('/');
        let pp = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let fn_ = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow!("无效路径"))?;
        let pid = self.get_fid_by_path(&pp).await?;
        let is_family = self.is_family();
        let mut url = API_URL.to_string(); if is_family { url.push_str("/family/file"); } url.push_str("/createFolder.action");
        let mut query = vec![("folderName".into(), fn_.into()), ("relativePath".into(), "".into())];
        if is_family { let fid = self.get_family_id().await?; query.extend([("familyId".into(), fid), ("parentId".into(), pid)]); } else { query.push(("parentFolderId".into(), pid)); }
        let _: Value = self.post(&url, Some(query)).await?;
        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(path).await?;
        let is_family = self.is_family();
        let is_dir = { 
            let pp = std::path::Path::new(path.trim_matches('/')).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(); 
            let pid = self.get_fid_by_path(&pp).await?; 
            self.get_files(&pid, 1).await?.file_list_ao.folder_list.iter().any(|f| f.get_id() == fid) 
        };
        let mut url = API_URL.to_string(); 
        if is_family { url.push_str("/family/file"); }
        let mut query = vec![];
        if is_dir { 
            url.push_str("/renameFolder.action"); 
            query.extend([("folderId".into(), fid), ("destFolderName".into(), new_name.into())]); 
        } else { 
            url.push_str("/renameFile.action"); 
            query.extend([("fileId".into(), fid), ("destFileName".into(), new_name.into())]); 
        }
        if is_family { query.push(("familyId".into(), self.get_family_id().await?)); }
        let _: Value = if is_family { self.get(&url, Some(query)).await? } else { self.post(&url, Some(query)).await? };
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(old_path).await?;
        let fname = std::path::Path::new(old_path).file_name().and_then(|n| n.to_str()).unwrap_or("");
        let new_pp = std::path::Path::new(new_path.trim_matches('/')).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let tfid = self.get_fid_by_path(&new_pp).await?;
        let is_folder = { 
            let pp = std::path::Path::new(old_path.trim_matches('/')).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(); 
            let pid = self.get_fid_by_path(&pp).await?; 
            self.get_files(&pid, 1).await?.file_list_ao.folder_list.iter().any(|f| f.get_id() == fid) 
        };
        let family_id = if self.is_family() { Some(self.get_family_id().await?) } else { None };
        let task = BatchTaskInfo { 
            file_id: fid, 
            file_name: fname.to_string(), 
            is_folder: if is_folder { 1 } else { 0 },
            src_parent_id: None,
            deal_way: None,
            is_conflict: None,
        };
        let resp = self.create_batch_task("MOVE", family_id.as_deref(), Some(&tfid), vec![task]).await?;
        self.wait_batch_task("MOVE", &resp.task_id, 400).await?;
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        self.ensure_authenticated().await?;
        let fid = self.get_fid_by_path(path).await?;
        Ok(Some(self.get_download_url(&fid).await?))
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        self.ensure_authenticated().await?;
        match self.get::<CapacityResp>(&format!("{}/portal/getUserSizeInfo.action", API_URL), None).await {
            Ok(resp) => {
                let (total, used) = if self.is_family() { 
                    let i = resp.family_capacity_info.unwrap_or_default(); 
                    (i.total_size, i.used_size) 
                } else { 
                    let i = resp.cloud_capacity_info.unwrap_or_default(); 
                    (i.total_size, i.used_size) 
                };
                Ok(Some(SpaceInfo { used, total, free: total.saturating_sub(used) }))
            }
            Err(e) => { tracing::warn!("获取空间信息失败: {}", e); Ok(None) }
        }
    }

    fn show_space_in_frontend(&self) -> bool { self.config.show_space_info }
}

pub struct Cloud189DriverFactory;

impl DriverFactory for Cloud189DriverFactory {
    fn driver_type(&self) -> &'static str { "cloud189" }
    fn driver_config(&self) -> DriverConfig { DriverConfig { name: "天翼云盘".to_string(), local_sort: false, only_proxy: false, no_cache: false, no_upload: true, default_root: Some("-11".to_string()) } }
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("username", "string").title("用户名").help("手机号"),
            ConfigItem::new("password", "string").title("密码"),
            ConfigItem::new("refresh_token", "string").title("RefreshToken").help("优先使用"),
            ConfigItem::new("root_folder_id", "string").title("根目录ID").default("-11").help("个人云-11，家庭云留空"),
            ConfigItem::new("cloud_type", "string").title("云类型").default("personal").help("personal或family"),
            ConfigItem::new("family_id", "string").title("家庭云ID").help("家庭云模式可自动获取"),
            ConfigItem::new("show_space_info", "bool").title("显示空间信息").default("true"),
        ]
    }
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> { Ok(Box::new(Cloud189Driver::new(serde_json::from_value(config)?))) }
}
