//! 123云盘开放平台API请求封装
//! 123 Cloud Open Platform API request wrapper

use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;
use chrono::Utc;

use super::types::*;

/// API基础URL / API base URL
const API_BASE: &str = "https://open-api.123pan.com";

/// API端点定义 / API endpoint definitions
pub struct ApiEndpoint {
    pub path: &'static str,
    pub qps: usize,
}

/// API端点列表 / API endpoint list
pub mod endpoints {
    use super::ApiEndpoint;

    /// 获取访问令牌 / Get access token
    pub const ACCESS_TOKEN: ApiEndpoint = ApiEndpoint { path: "/api/v1/access_token", qps: 1 };
    /// 刷新令牌 / Refresh token
    pub const REFRESH_TOKEN: ApiEndpoint = ApiEndpoint { path: "/api/v1/oauth2/access_token", qps: 1 };
    /// 用户信息 / User info
    pub const USER_INFO: ApiEndpoint = ApiEndpoint { path: "/api/v1/user/info", qps: 1 };
    /// 文件列表 / File list
    pub const FILE_LIST: ApiEndpoint = ApiEndpoint { path: "/api/v2/file/list", qps: 3 };
    /// 下载信息 / Download info
    pub const DOWNLOAD_INFO: ApiEndpoint = ApiEndpoint { path: "/api/v1/file/download_info", qps: 5 };
    /// 直链 / Direct link
    pub const DIRECT_LINK: ApiEndpoint = ApiEndpoint { path: "/api/v1/direct-link/url", qps: 5 };
    /// 创建目录 / Create directory
    pub const MKDIR: ApiEndpoint = ApiEndpoint { path: "/upload/v1/file/mkdir", qps: 2 };
    /// 移动文件 / Move file
    pub const MOVE: ApiEndpoint = ApiEndpoint { path: "/api/v1/file/move", qps: 1 };
    /// 重命名 / Rename
    pub const RENAME: ApiEndpoint = ApiEndpoint { path: "/api/v1/file/name", qps: 1 };
    /// 删除 / Trash
    pub const TRASH: ApiEndpoint = ApiEndpoint { path: "/api/v1/file/trash", qps: 2 };
    /// 创建上传 / Create upload
    pub const UPLOAD_CREATE: ApiEndpoint = ApiEndpoint { path: "/upload/v2/file/create", qps: 2 };
    /// 上传完成 / Upload complete
    pub const UPLOAD_COMPLETE: ApiEndpoint = ApiEndpoint { path: "/upload/v2/file/upload_complete", qps: 0 };
    /// 离线下载 / Offline download
    pub const OFFLINE_DOWNLOAD: ApiEndpoint = ApiEndpoint { path: "/api/v1/offline/download", qps: 1 };
    /// 离线下载进度 / Offline download progress
    pub const OFFLINE_DOWNLOAD_PROGRESS: ApiEndpoint = ApiEndpoint { path: "/api/v1/offline/download/process", qps: 5 };
}

/// QPS限流器 (简化版，使用计数器) / QPS rate limiter (simplified, using counter)
pub struct RateLimiter {
    count: AtomicUsize,
    qps: usize,
    last_reset: Mutex<std::time::Instant>,
}

impl RateLimiter {
    /// 创建限流器 / Create rate limiter
    pub fn new(qps: usize) -> Self {
        Self {
            count: AtomicUsize::new(0),
            qps,
            last_reset: Mutex::new(std::time::Instant::now()),
        }
    }

    /// 获取许可 / Acquire permit
    pub async fn acquire(&self) {
        if self.qps == 0 {
            return;
        }

        loop {
            let mut last_reset = self.last_reset.lock().await;
            let elapsed = last_reset.elapsed();

            // 每秒重置计数 / Reset count every second
            if elapsed >= Duration::from_secs(1) {
                self.count.store(0, Ordering::SeqCst);
                *last_reset = std::time::Instant::now();
            }

            let current = self.count.fetch_add(1, Ordering::SeqCst);
            if current < self.qps {
                return;
            }

            // 超过限制，等待并重试 / Exceeded limit, wait and retry
            self.count.fetch_sub(1, Ordering::SeqCst);
            drop(last_reset);
            sleep(Duration::from_millis(100)).await;
        }
    }
}

/// API客户端 / API client
pub struct ApiClient {
    client: Client,
    config: Arc<Mutex<Pan123OpenConfig>>,
    rate_limiters: std::collections::HashMap<&'static str, RateLimiter>,
    uid: Arc<Mutex<Option<u64>>>,
}

impl ApiClient {
    /// 创建API客户端（使用共享的config）/ Create API client (with shared config)
    pub fn new(config: Arc<Mutex<Pan123OpenConfig>>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        let mut rate_limiters = std::collections::HashMap::new();
        rate_limiters.insert(endpoints::ACCESS_TOKEN.path, RateLimiter::new(endpoints::ACCESS_TOKEN.qps));
        rate_limiters.insert(endpoints::REFRESH_TOKEN.path, RateLimiter::new(endpoints::REFRESH_TOKEN.qps));
        rate_limiters.insert(endpoints::USER_INFO.path, RateLimiter::new(endpoints::USER_INFO.qps));
        rate_limiters.insert(endpoints::FILE_LIST.path, RateLimiter::new(endpoints::FILE_LIST.qps));
        rate_limiters.insert(endpoints::DOWNLOAD_INFO.path, RateLimiter::new(endpoints::DOWNLOAD_INFO.qps));
        rate_limiters.insert(endpoints::DIRECT_LINK.path, RateLimiter::new(endpoints::DIRECT_LINK.qps));
        rate_limiters.insert(endpoints::MKDIR.path, RateLimiter::new(endpoints::MKDIR.qps));
        rate_limiters.insert(endpoints::MOVE.path, RateLimiter::new(endpoints::MOVE.qps));
        rate_limiters.insert(endpoints::RENAME.path, RateLimiter::new(endpoints::RENAME.qps));
        rate_limiters.insert(endpoints::TRASH.path, RateLimiter::new(endpoints::TRASH.qps));
        rate_limiters.insert(endpoints::UPLOAD_CREATE.path, RateLimiter::new(endpoints::UPLOAD_CREATE.qps));
        rate_limiters.insert(endpoints::UPLOAD_COMPLETE.path, RateLimiter::new(endpoints::UPLOAD_COMPLETE.qps));
        rate_limiters.insert(endpoints::OFFLINE_DOWNLOAD.path, RateLimiter::new(endpoints::OFFLINE_DOWNLOAD.qps));
        rate_limiters.insert(endpoints::OFFLINE_DOWNLOAD_PROGRESS.path, RateLimiter::new(endpoints::OFFLINE_DOWNLOAD_PROGRESS.qps));

        Self {
            client,
            config,
            rate_limiters,
            uid: Arc::new(Mutex::new(None)),
        }
    }

    /// 获取HTTP客户端 / Get HTTP client
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// 获取当前配置 / Get current config
    pub async fn get_config(&self) -> Pan123OpenConfig {
        self.config.lock().await.clone()
    }

    /// 更新配置 / Update config
    pub async fn update_config<F>(&self, f: F)
    where
        F: FnOnce(&mut Pan123OpenConfig),
    {
        let mut config = self.config.lock().await;
        f(&mut config);
    }

    /// 获取用户ID / Get user ID
    pub async fn get_uid(&self) -> Result<u64, String> {
        let cached = *self.uid.lock().await;
        if let Some(uid) = cached {
            return Ok(uid);
        }

        let user_info = self.get_user_info().await?;
        if let Some(data) = user_info.data {
            *self.uid.lock().await = Some(data.uid);
            Ok(data.uid)
        } else {
            Err("Failed to get user info".to_string())
        }
    }

    /// 构建请求 / Build request
    fn build_request(&self, method: Method, endpoint: &ApiEndpoint, access_token: &str) -> RequestBuilder {
        let url = format!("{}{}", API_BASE, endpoint.path);
        self.client
            .request(method, &url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Platform", "open_platform")
            .header("Content-Type", "application/json")
    }

    /// 发送请求并处理响应 / Send request and handle response
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &ApiEndpoint,
        body: Option<serde_json::Value>,
        query: Option<&[(&str, &str)]>,
    ) -> Result<T, String> {
        let mut retry_token = true;

        loop {
            // 限流 / Rate limiting
            if let Some(limiter) = self.rate_limiters.get(endpoint.path) {
                limiter.acquire().await;
            }

            let config = self.config.lock().await;
            let access_token = config.access_token.clone();
            drop(config);

            let mut req = self.build_request(method.clone(), endpoint, &access_token);

            if let Some(q) = query {
                req = req.query(q);
            }

            if let Some(ref b) = body {
                req = req.json(b);
            }

            let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
            let bytes = resp.bytes().await.map_err(|e| format!("Read response failed: {}", e))?;

            // 先解析基础响应检查错误码 / Parse base response to check error code first
            let base: BaseResponse = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Parse response failed: {}", e))?;

            match base.code {
                0 => {
                    // 成功，解析完整响应 / Success, parse full response
                    return serde_json::from_slice(&bytes)
                        .map_err(|e| format!("Parse response failed: {}", e));
                }
                401 if retry_token => {
                    // 令牌过期，刷新后重试 / Token expired, refresh and retry
                    retry_token = false;
                    self.refresh_access_token().await?;
                }
                429 => {
                    // 请求过于频繁，等待后重试 / Too many requests, wait and retry
                    tracing::warn!("API rate limited: {}, waiting...", endpoint.path);
                    sleep(Duration::from_millis(500)).await;
                }
                _ => {
                    return Err(format!("API error ({}): {}", base.code, base.message));
                }
            }
        }
    }

    /// 刷新访问令牌 / Refresh access token
    pub async fn refresh_access_token(&self) -> Result<(), String> {
        let config = self.config.lock().await.clone();

        if !config.client_id.is_empty() {
            if !config.refresh_token.is_empty() {
                // OAuth2刷新令牌模式 / OAuth2 refresh token mode
                let url = format!("{}{}", API_BASE, endpoints::REFRESH_TOKEN.path);
                let resp: RefreshTokenResponse = self.client
                    .post(&url)
                    .header("Platform", "open_platform")
                    .query(&[
                        ("client_id", config.client_id.as_str()),
                        ("client_secret", config.client_secret.as_str()),
                        ("grant_type", "refresh_token"),
                        ("refresh_token", config.refresh_token.as_str()),
                    ])
                    .send()
                    .await
                    .map_err(|e| format!("Refresh token request failed: {}", e))?
                    .json()
                    .await
                    .map_err(|e| format!("Parse refresh token response failed: {}", e))?;

                let mut config = self.config.lock().await;
                config.access_token = resp.access_token;
                config.refresh_token = resp.refresh_token;
                tracing::info!("Access token refreshed via OAuth2");
            } else if !config.client_secret.is_empty() {
                // 客户端凭证模式 / Client credentials mode
                let url = format!("{}{}", API_BASE, endpoints::ACCESS_TOKEN.path);
                let resp: AccessTokenResponse = self.client
                    .post(&url)
                    .header("Platform", "open_platform")
                    .header("Content-Type", "application/json")
                    .json(&GetAccessTokenRequest {
                        client_id: config.client_id.clone(),
                        client_secret: config.client_secret.clone(),
                    })
                    .send()
                    .await
                    .map_err(|e| format!("Get access token request failed: {}", e))?
                    .json()
                    .await
                    .map_err(|e| format!("Parse access token response failed: {}", e))?;

                if resp.base.code != 0 {
                    return Err(format!("Get access token failed: {}", resp.base.message));
                }

                if let Some(data) = resp.data {
                    let mut config = self.config.lock().await;
                    config.access_token = data.access_token;
                    tracing::info!("Access token refreshed via client credentials");
                }
            }
        }

        Ok(())
    }

    /// 签名URL (用于直链鉴权) / Sign URL (for direct link authentication)
    pub fn sign_url(
        &self,
        origin_url: &str,
        private_key: &str,
        uid: u64,
        valid_duration: Duration,
    ) -> Result<String, String> {
        // 生成Unix时间戳 / Generate Unix timestamp
        let ts = Utc::now().timestamp() + valid_duration.as_secs() as i64;

        // 生成随机数 (UUID不含中划线) / Generate random string (UUID without hyphens)
        let rand = Uuid::new_v4().to_string().replace("-", "");

        // 解析URL / Parse URL
        let mut url = url::Url::parse(origin_url)
            .map_err(|e| format!("Parse URL failed: {}", e))?;

        // 待签名字符串: path-timestamp-rand-uid-privateKey
        // String to sign: path-timestamp-rand-uid-privateKey
        let unsigned_str = format!("{}-{}-{}-{}-{}", url.path(), ts, rand, uid, private_key);

        // 计算MD5 / Calculate MD5
        let md5_hash = format!("{:x}", md5::compute(unsigned_str.as_bytes()));

        // 生成鉴权参数: timestamp-rand-uid-md5hash
        // Generate auth key: timestamp-rand-uid-md5hash
        let auth_key = format!("{}-{}-{}-{}", ts, rand, uid, md5_hash);

        // 添加鉴权参数到URL / Add auth key to URL
        url.query_pairs_mut().append_pair("auth_key", &auth_key);

        Ok(url.to_string())
    }

    // ============ API 方法 / API Methods ============

    /// 获取用户信息 / Get user info
    pub async fn get_user_info(&self) -> Result<UserInfoResponse, String> {
        self.request(Method::GET, &endpoints::USER_INFO, None, None).await
    }

    /// 获取文件列表 / Get file list
    pub async fn get_files(
        &self,
        parent_file_id: i64,
        limit: i32,
        last_file_id: i64,
    ) -> Result<FileListResponse, String> {
        let query = [
            ("parentFileId", parent_file_id.to_string()),
            ("limit", limit.to_string()),
            ("lastFileId", last_file_id.to_string()),
            ("trashed", "false".to_string()),
            ("searchMode", "".to_string()),
            ("searchData", "".to_string()),
        ];
        let query_refs: Vec<(&str, &str)> = query.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        self.request(Method::GET, &endpoints::FILE_LIST, None, Some(&query_refs)).await
    }

    /// 获取下载信息 / Get download info
    pub async fn get_download_info(&self, file_id: i64) -> Result<DownloadInfoResponse, String> {
        let query = [("fileId", file_id.to_string())];
        let query_refs: Vec<(&str, &str)> = query.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        self.request(Method::GET, &endpoints::DOWNLOAD_INFO, None, Some(&query_refs)).await
    }

    /// 获取直链 / Get direct link
    pub async fn get_direct_link(&self, file_id: i64) -> Result<DirectLinkResponse, String> {
        let query = [("fileID", file_id.to_string())];
        let query_refs: Vec<(&str, &str)> = query.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        self.request(Method::GET, &endpoints::DIRECT_LINK, None, Some(&query_refs)).await
    }

    /// 创建目录 / Create directory
    pub async fn mkdir(&self, parent_id: i64, name: &str) -> Result<(), String> {
        let body = serde_json::json!({
            "parentID": parent_id.to_string(),
            "name": name
        });

        let _: BaseResponse = self.request(Method::POST, &endpoints::MKDIR, Some(body), None).await?;
        Ok(())
    }

    /// 移动文件 / Move file
    pub async fn move_file(&self, file_id: i64, to_parent_file_id: i64) -> Result<(), String> {
        let body = serde_json::json!({
            "fileIDs": [file_id],
            "toParentFileID": to_parent_file_id
        });

        let _: BaseResponse = self.request(Method::POST, &endpoints::MOVE, Some(body), None).await?;
        Ok(())
    }

    /// 重命名文件 / Rename file
    pub async fn rename(&self, file_id: i64, new_name: &str) -> Result<(), String> {
        let body = serde_json::json!({
            "fileId": file_id,
            "fileName": new_name
        });

        let _: BaseResponse = self.request(Method::PUT, &endpoints::RENAME, Some(body), None).await?;
        Ok(())
    }

    /// 删除文件 (移到回收站) / Delete file (move to trash)
    pub async fn trash(&self, file_id: i64) -> Result<(), String> {
        let body = serde_json::json!({
            "fileIDs": [file_id]
        });

        let _: BaseResponse = self.request(Method::POST, &endpoints::TRASH, Some(body), None).await?;
        Ok(())
    }

    /// 创建上传任务 / Create upload task
    pub async fn create_upload(
        &self,
        parent_file_id: i64,
        filename: &str,
        etag: &str,
        size: i64,
        duplicate: i32,
        contain_dir: bool,
    ) -> Result<UploadCreateResponse, String> {
        let body = serde_json::json!({
            "parentFileID": parent_file_id,
            "filename": filename,
            "etag": etag.to_lowercase(),
            "size": size,
            "duplicate": duplicate,
            "containDir": contain_dir
        });

        self.request(Method::POST, &endpoints::UPLOAD_CREATE, Some(body), None).await
    }

    /// 上传完成 / Upload complete
    pub async fn upload_complete(&self, preupload_id: &str) -> Result<UploadCompleteResponse, String> {
        let body = serde_json::json!({
            "preuploadID": preupload_id
        });

        self.request(Method::POST, &endpoints::UPLOAD_COMPLETE, Some(body), None).await
    }

    /// 创建离线下载任务 / Create offline download task
    pub async fn create_offline_download(
        &self,
        url: &str,
        dir_id: &str,
        callback_url: Option<&str>,
    ) -> Result<OfflineDownloadResponse, String> {
        let mut body = serde_json::json!({
            "url": url,
            "dirID": dir_id
        });

        if let Some(cb) = callback_url {
            body["callBackUrl"] = serde_json::Value::String(cb.to_string());
        }

        self.request(Method::POST, &endpoints::OFFLINE_DOWNLOAD, Some(body), None).await
    }

    /// 查询离线下载进度 / Query offline download progress
    pub async fn get_offline_download_progress(&self, task_id: i32) -> Result<OfflineDownloadProgressResponse, String> {
        let query = [("taskID", task_id.to_string())];
        let query_refs: Vec<(&str, &str)> = query.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        self.request(Method::GET, &endpoints::OFFLINE_DOWNLOAD_PROGRESS, None, Some(&query_refs)).await
    }
}
