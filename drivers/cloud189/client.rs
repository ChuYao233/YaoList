//! Cloud189 HTTP client wrapper / 天翼云盘HTTP客户端封装 

use anyhow::{anyhow, Result};
use reqwest::{Client, Method, redirect};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::types::*;
use super::utils::*;

/// Cloud189 HTTP client / 天翼云盘HTTP客户端
pub struct Cloud189Client {
    pub client: Client,
    pub no_redirect_client: Client,
    pub token_info: Arc<RwLock<Option<AppSessionResp>>>,
    pub is_family: bool,
    pub family_id: String,
}

impl Cloud189Client {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .cookie_store(true)
                .redirect(redirect::Policy::limited(10))
                .build()
                .unwrap(),
            no_redirect_client: Client::builder()
                .cookie_store(true)
                .redirect(redirect::Policy::none())
                .build()
                .unwrap(),
            token_info: Arc::new(RwLock::new(None)),
            is_family: false,
            family_id: String::new(),
        }
    }

    /// Get session key and secret / 获取session key和secret
    pub async fn get_session_info(&self) -> (String, String) {
        let token = self.token_info.read().await;
        if let Some(ref t) = *token {
            if self.is_family {
                (t.user_session.family_session_key.clone(), t.user_session.family_session_secret.clone())
            } else {
                (t.user_session.session_key.clone(), t.user_session.session_secret.clone())
            }
        } else {
            (String::new(), String::new())
        }
    }

    /// Generate signature headers / 生成签名请求头
    pub async fn signature_header(&self, url: &str, method: &str, params: &str) -> HashMap<String, String> {
        let date = get_http_date_str();
        let (session_key, session_secret) = self.get_session_info().await;

        let mut headers = HashMap::new();
        headers.insert("Date".to_string(), date.clone());
        headers.insert("SessionKey".to_string(), session_key.clone());
        headers.insert("X-Request-ID".to_string(), Uuid::new_v4().to_string());
        headers.insert(
            "Signature".to_string(),
            signature_of_hmac(&session_secret, &session_key, method, url, &date, params),
        );
        headers
    }

    /// Send request / 发送请求
    pub async fn request<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        method: Method,
        query: Option<Vec<(String, String)>>,
    ) -> Result<T> {
        if self.token_info.read().await.is_none() {
            return Err(anyhow!("Not logged in / 未登录"));
        }

        let headers = self.signature_header(url, method.as_str(), "").await;

        let mut req = self.client
            .request(method.clone(), url)
            .header("Accept", "application/json;charset=UTF-8")
            .header("Referer", WEB_URL)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36");

        for (k, v) in &headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let mut all_query = client_suffix();
        if let Some(q) = query {
            all_query.extend(q);
        }
        req = req.query(&all_query);

        let resp = req.send().await?;
        let text = resp.text().await?;

        // Check session errors / 检查session错误
        if text.contains("userSessionBO is null") || text.contains("InvalidSessionKey") {
            return Err(anyhow!("Session expired / Session已过期"));
        }

        // Check errors / 检查错误
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("Cloud189 API error / 天翼云API错误: {}", err.error_message()));
            }
        }

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse response / 解析响应失败: {} - {}", e, text))
    }

    /// GET request / GET请求
    pub async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        query: Option<Vec<(String, String)>>,
    ) -> Result<T> {
        self.request(url, Method::GET, query).await
    }

    /// POST request / POST请求
    pub async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        query: Option<Vec<(String, String)>>,
    ) -> Result<T> {
        self.request(url, Method::POST, query).await
    }

    /// Get download link (supports 302 redirect) / 获取下载链接
    pub async fn get_download_url(&self, file_id: &str) -> Result<String> {
        let mut url = API_URL.to_string();
        if self.is_family {
            url.push_str("/family/file");
        }
        url.push_str("/getFileDownloadUrl.action");

        let mut query = vec![("fileId".to_string(), file_id.to_string())];
        if self.is_family {
            query.push(("familyId".to_string(), self.family_id.clone()));
        } else {
            query.push(("dt".to_string(), "3".to_string()));
            query.push(("flag".to_string(), "1".to_string()));
        }

        let resp: DownloadUrlResp = self.get(&url, Some(query)).await?;

        let download_url = resp.file_download_url
            .replace("&amp;", "&")
            .replace("http://", "https://");

        // 302 redirect to get real link / 302重定向获取真实链接
        let redirect_resp = self.no_redirect_client
            .get(&download_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await?;

        if redirect_resp.status().as_u16() == 302 {
            if let Some(location) = redirect_resp.headers().get("location") {
                return Ok(location.to_str().unwrap_or(&download_url).to_string());
            }
        }

        Ok(download_url)
    }

    /// Create batch task / 创建批量任务 
    pub async fn create_batch_task(
        &self,
        task_type: &str,
        target_folder_id: Option<&str>,
        task_infos: Vec<BatchTaskInfo>,
    ) -> Result<CreateBatchTaskResp> {
        let task_infos_json = serde_json::to_string(&task_infos)?;

        let mut form_data = vec![
            ("type".to_string(), task_type.to_string()),
            ("taskInfos".to_string(), task_infos_json),
        ];

        if let Some(tid) = target_folder_id {
            if !tid.is_empty() {
                form_data.push(("targetFolderId".to_string(), tid.to_string()));
            }
        }

        if self.is_family && !self.family_id.is_empty() {
            form_data.push(("familyId".to_string(), self.family_id.clone()));
        }

        self.post(&format!("{}/batch/createBatchTask.action", API_URL), Some(form_data)).await
    }

    /// Check batch task status / 检查批量任务状态 
    pub async fn check_batch_task(&self, task_type: &str, task_id: &str) -> Result<BatchTaskStateResp> {
        let form_data = vec![
            ("type".to_string(), task_type.to_string()),
            ("taskId".to_string(), task_id.to_string()),
        ];

        self.post(&format!("{}/batch/checkBatchTask.action", API_URL), Some(form_data)).await
    }

    /// Wait for batch task completion / 等待批量任务完成
    pub async fn wait_batch_task(&self, task_type: &str, task_id: &str, delay_ms: u64) -> Result<()> {
        loop {
            let state = self.check_batch_task(task_type, task_id).await?;
            match state.task_status {
                2 => return Err(anyhow!("Conflict exists / 存在冲突")),
                4 => return Ok(()),
                _ => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    /// Get file list / 获取文件列表
    pub async fn get_files(&self, folder_id: &str, page_num: i32) -> Result<Cloud189FilesResp> {
        let mut url = API_URL.to_string();
        if self.is_family {
            url.push_str("/family/file");
        }
        url.push_str("/listFiles.action");

        let mut query = vec![
            ("folderId".to_string(), folder_id.to_string()),
            ("fileType".to_string(), "0".to_string()),
            ("mediaAttr".to_string(), "0".to_string()),
            ("iconOption".to_string(), "5".to_string()),
            ("pageNum".to_string(), page_num.to_string()),
            ("pageSize".to_string(), "1000".to_string()),
        ];

        if self.is_family {
            query.push(("familyId".to_string(), self.family_id.clone()));
            query.push(("orderBy".to_string(), "1".to_string()));
            query.push(("descending".to_string(), "false".to_string()));
        } else {
            query.push(("recursive".to_string(), "0".to_string()));
            query.push(("orderBy".to_string(), "filename".to_string()));
            query.push(("descending".to_string(), "false".to_string()));
        }

        self.get(&url, Some(query)).await
    }

    /// Create folder / 创建文件夹
    pub async fn create_folder(&self, parent_id: &str, folder_name: &str) -> Result<Cloud189Folder> {
        let mut url = API_URL.to_string();
        if self.is_family {
            url.push_str("/family/file");
        }
        url.push_str("/createFolder.action");

        let mut query = vec![
            ("folderName".to_string(), folder_name.to_string()),
            ("relativePath".to_string(), String::new()),
        ];

        if self.is_family {
            query.push(("familyId".to_string(), self.family_id.clone()));
            query.push(("parentId".to_string(), parent_id.to_string()));
        } else {
            query.push(("parentFolderId".to_string(), parent_id.to_string()));
        }

        self.post(&url, Some(query)).await
    }

    /// Rename file / 重命名文件
    pub async fn rename_file(&self, file_id: &str, new_name: &str) -> Result<serde_json::Value> {
        let mut url = API_URL.to_string();
        if self.is_family {
            url.push_str("/family/file");
        }
        url.push_str("/renameFile.action");

        let mut query = vec![
            ("fileId".to_string(), file_id.to_string()),
            ("destFileName".to_string(), new_name.to_string()),
        ];

        if self.is_family {
            query.push(("familyId".to_string(), self.family_id.clone()));
            self.get(&url, Some(query)).await
        } else {
            self.post(&url, Some(query)).await
        }
    }

    /// Rename folder / 重命名文件夹
    pub async fn rename_folder(&self, folder_id: &str, new_name: &str) -> Result<serde_json::Value> {
        let mut url = API_URL.to_string();
        if self.is_family {
            url.push_str("/family/file");
        }
        url.push_str("/renameFolder.action");

        let mut query = vec![
            ("folderId".to_string(), folder_id.to_string()),
            ("destFolderName".to_string(), new_name.to_string()),
        ];

        if self.is_family {
            query.push(("familyId".to_string(), self.family_id.clone()));
            self.get(&url, Some(query)).await
        } else {
            self.post(&url, Some(query)).await
        }
    }

    /// Get family cloud list / 获取家庭云列表
    pub async fn get_family_list(&self) -> Result<FamilyInfoListResp> {
        self.get(&format!("{}/family/manage/getFamilyList.action", API_URL), None).await
    }

    /// Get capacity information / 获取容量信息
    pub async fn get_capacity_info(&self) -> Result<CapacityResp> {
        self.get(&format!("{}/portal/getUserSizeInfo.action", API_URL), None).await
    }
}
