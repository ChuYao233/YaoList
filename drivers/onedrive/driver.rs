use async_trait::async_trait;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use reqwest::Client;


use crate::drivers::{Driver, FileInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneDriveConfig {
    pub region: String,           // global, cn, us, de
    pub is_sharepoint: bool,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub refresh_token: String,
    pub site_id: Option<String>,  // SharePoint 站点 ID
    pub chunk_size: u64,          // 分块上传大小 (MB)
    pub custom_host: Option<String>, // 自定义下载域名
    pub proxy_download: bool,     // 是否通过本地代理下载，false 为直接 302 重定向
}

#[derive(Debug)]
pub struct OneDriveDriver {
    config: OneDriveConfig,
    client: Client,
    access_token: tokio::sync::RwLock<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: String,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct OneDriveFile {
    id: String,
    name: String,
    size: Option<u64>,
    #[serde(rename = "fileSystemInfo")]
    file_system_info: Option<FileSystemInfo>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
    file: Option<FileDetail>,
    folder: Option<serde_json::Value>,
    #[serde(rename = "parentReference")]
    parent_reference: Option<ParentReference>,
}

#[derive(Debug, Deserialize)]
struct FileSystemInfo {
    #[serde(rename = "lastModifiedDateTime")]
    last_modified_date_time: Option<DateTime<Utc>>,
    #[serde(rename = "createdDateTime")]
    created_date_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct FileDetail {
    #[serde(rename = "mimeType")]
    mime_type: String,
}

#[derive(Debug, Deserialize)]
struct ParentReference {
    #[serde(rename = "driveId")]
    drive_id: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct FilesResponse {
    value: Vec<OneDriveFile>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

impl OneDriveDriver {
    pub fn new(config: OneDriveConfig) -> Self {
        Self {
            config,
            client: Client::new(),
            access_token: tokio::sync::RwLock::new(None),
        }
    }

    fn get_host(&self) -> (&str, &str) {
        match self.config.region.as_str() {
            "cn" => ("https://login.chinacloudapi.cn", "https://microsoftgraph.chinacloudapi.cn"),
            "us" => ("https://login.microsoftonline.us", "https://graph.microsoft.us"),
            "de" => ("https://login.microsoftonline.de", "https://graph.microsoft.de"),
            _ => ("https://login.microsoftonline.com", "https://graph.microsoft.com"), // global
        }
    }

    fn get_meta_url(&self, path: &str) -> String {
        let (_, api_host) = self.get_host();
        
        // 清理路径：移除开头和结尾的斜杠
        let clean_path = path.trim_start_matches('/').trim_end_matches('/');

        if self.config.is_sharepoint {
            if let Some(site_id) = &self.config.site_id {
                if clean_path.is_empty() {
                    format!("{}/v1.0/sites/{}/drive/root", api_host, site_id)
                } else {
                    // 对路径进行URL编码，但不编码斜杠
                    let encoded_path = clean_path.split('/')
                        .map(|segment| urlencoding::encode(segment))
                        .collect::<Vec<_>>()
                        .join("/");
                    format!("{}/v1.0/sites/{}/drive/root:/{}:", api_host, site_id, encoded_path)
                }
            } else {
                panic!("SharePoint mode requires site_id")
            }
        } else {
            if clean_path.is_empty() {
                format!("{}/v1.0/me/drive/root", api_host)
            } else {
                // 对路径进行URL编码，但不编码斜杠
                let encoded_path = clean_path.split('/')
                    .map(|segment| urlencoding::encode(segment))
                    .collect::<Vec<_>>()
                    .join("/");
                format!("{}/v1.0/me/drive/root:/{}:", api_host, encoded_path)
            }
        }
    }

    async fn refresh_token(&self) -> Result<String> {
        let (oauth_host, _) = self.get_host();
        let url = format!("{}/common/oauth2/v2.0/token", oauth_host);

        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("client_id", &self.config.client_id);
        params.insert("client_secret", &self.config.client_secret);
        params.insert("redirect_uri", &self.config.redirect_uri);
        params.insert("refresh_token", &self.config.refresh_token);

        let response = self.client
            .post(&url)
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let token_resp: TokenResponse = response.json().await?;
            let mut access_token = self.access_token.write().await;
            *access_token = Some(token_resp.access_token.clone());
            Ok(token_resp.access_token)
        } else {
            let error: TokenError = response.json().await?;
            Err(anyhow!("Token refresh failed: {}", error.error_description))
        }
    }

    async fn get_access_token(&self) -> Result<String> {
        {
            let token = self.access_token.read().await;
            if let Some(ref token) = *token {
                return Ok(token.clone());
            }
        }
        
        self.refresh_token().await
    }

    async fn make_request<T>(&self, url: &str, method: reqwest::Method) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let token = self.get_access_token().await?;
        
        let response = self.client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else if response.status() == 401 {
            // Token expired, refresh and retry
            let new_token = self.refresh_token().await?;
            let response = self.client
                .request(reqwest::Method::GET, url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send()
                .await?;

            if response.status().is_success() {
                Ok(response.json().await?)
            } else {
                let error: ApiError = response.json().await?;
                Err(anyhow!("API error: {}", error.error.message))
            }
        } else {
            let error: ApiError = response.json().await?;
            Err(anyhow!("API error: {}", error.error.message))
        }
    }

    async fn get_files(&self, path: &str) -> Result<Vec<OneDriveFile>> {
        let mut all_files = Vec::new();
        let base_url = format!("{}/children", self.get_meta_url(path));
        let mut next_link = Some(format!("{}?$top=1000&$select=id,name,size,fileSystemInfo,@microsoft.graph.downloadUrl,file,folder,parentReference", base_url));

        println!("🔍 OneDrive 请求路径: {} -> URL: {}", path, next_link.as_ref().unwrap());

        while let Some(url) = next_link {
            match self.make_request(&url, reqwest::Method::GET).await {
                Ok(response) => {
                    let response: FilesResponse = response;
                    println!("✅ OneDrive 成功获取 {} 个文件", response.value.len());
                    
                    // 打印文件列表用于调试
                    for file in &response.value {
                        let file_type = if file.folder.is_some() { "📁" } else { "📄" };
                        println!("  {} {}", file_type, file.name);
                    }
                    
            all_files.extend(response.value);
            next_link = response.next_link;
                },
                Err(e) => {
                    println!("❌ OneDrive 请求失败: {}", e);
                    return Err(e);
                }
            }
        }

        Ok(all_files)
    }

    fn file_to_info(&self, file: OneDriveFile, parent_path: &str) -> FileInfo {
        let is_dir = file.folder.is_some();
        let size = file.size.unwrap_or(0);
        let modified = file.file_system_info
            .and_then(|info| info.last_modified_date_time)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| Utc::now().to_rfc3339());

        let path = if parent_path == "/" {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.name)
        };

        FileInfo {
            name: file.name,
            path,
            size,
            is_dir,
            modified,
        }
    }
}

#[async_trait]
impl Driver for OneDriveDriver {
    async fn move_file(&self, _file_id: &str, _new_parent_id: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("OneDrive驱动不支持移动操作"))
    }

    async fn copy_file(&self, _file_id: &str, _new_parent_id: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("OneDrive驱动不支持复制操作"))
    }

    async fn list(&self, path: &str) -> Result<Vec<FileInfo>> {
        let files = self.get_files(path).await?;
        Ok(files.into_iter()
            .map(|file| self.file_to_info(file, path))
            .collect())
    }

    async fn download(&self, path: &str) -> Result<tokio::fs::File> {
        if !self.config.proxy_download {
            // 直接重定向模式，不应该调用此方法
            return Err(anyhow!("Direct download mode enabled, use get_download_url instead"));
        }
        
        // 获取文件的下载链接
        let url = self.get_meta_url(path);
        let file: OneDriveFile = self.make_request(&url, reqwest::Method::GET).await?;
        
        if let Some(download_url) = file.download_url {
            // 下载文件到临时文件
            let response = self.client.get(&download_url).send().await?;
            if !response.status().is_success() {
                return Err(anyhow!("Failed to download file: HTTP {}", response.status()));
            }
            
            // 创建临时文件
            let temp_path = format!("temp_{}", uuid::Uuid::new_v4());
            let temp_file = tokio::fs::File::create(&temp_path).await?;
            let mut temp_file_writer = tokio::io::BufWriter::new(temp_file);
            
            // 直接读取所有字节
            use tokio::io::AsyncWriteExt;
            let bytes = response.bytes().await?;
            temp_file_writer.write_all(&bytes).await?;
            
            temp_file_writer.flush().await?;
            let _temp_file = temp_file_writer.into_inner();
            
            // 重新打开文件用于读取
            let file = tokio::fs::File::open(&temp_path).await?;
            Ok(file)
        } else {
            Err(anyhow!("File has no download URL"))
        }
    }

    async fn get_download_url(&self, path: &str) -> Result<Option<String>> {
        if self.config.proxy_download {
            // 使用本地代理下载，不返回直接链接
            Ok(None)
        } else {
            // 返回 OneDrive 直接下载链接用于 302 重定向
        let url = self.get_meta_url(path);
        let file: OneDriveFile = self.make_request(&url, reqwest::Method::GET).await?;
        
        if let Some(download_url) = file.download_url {
            if let Some(custom_host) = &self.config.custom_host {
                    let mut parsed_url = reqwest::Url::parse(&download_url)?;
                parsed_url.set_host(Some(custom_host))?;
                    Ok(Some(parsed_url.to_string()))
                } else {
                    Ok(Some(download_url))
                }
            } else {
                Ok(None)
            }
        }
    }

    async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> Result<()> {
        self.upload_file_impl(parent_path, file_name, content).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.delete_impl(path).await
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        self.rename_impl(path, new_name).await
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<()> {
        self.create_folder_impl(parent_path, folder_name).await
    }

    async fn get_file_info(&self, path: &str) -> Result<FileInfo> {
        let url = self.get_meta_url(path);
        let file: OneDriveFile = self.make_request(&url, reqwest::Method::GET).await?;
        
        let parent_path = if path.contains('/') {
            let parts: Vec<&str> = path.rsplitn(2, '/').collect();
            if parts.len() == 2 {
                parts[1]
            } else {
                ""
            }
        } else {
            ""
        };
        
        Ok(self.file_to_info(file, parent_path))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    // OneDrive 驱动不支持流式下载，返回 None
    async fn stream_download(&self, _path: &str) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        Ok(None)
    }
    
    // OneDrive 驱动不支持 Range 流式下载，返回 None
    async fn stream_download_with_range(&self, _path: &str, _start: Option<u64>, _end: Option<u64>) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        Ok(None)
        }
    }

impl OneDriveDriver {
    /// 创建文件夹实现
    pub async fn create_folder_impl(&self, parent_path: &str, folder_name: &str) -> Result<()> {
        let url = format!("{}/children", self.get_meta_url(parent_path));
        let token = self.get_access_token().await?;

        let body = serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error: ApiError = response.json().await?;
            Err(anyhow!("Failed to create folder: {}", error.error.message))
        }
    }

    /// 删除文件或文件夹实现
    pub async fn delete_impl(&self, path: &str) -> Result<()> {
        let url = self.get_meta_url(path);
        let token = self.get_access_token().await?;

        let response = self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() || response.status() == 404 {
            Ok(())
        } else {
            let error: ApiError = response.json().await?;
            Err(anyhow!("Failed to delete: {}", error.error.message))
        }
    }

    /// 重命名文件或文件夹实现
    pub async fn rename_impl(&self, path: &str, new_name: &str) -> Result<()> {
        let url = self.get_meta_url(path);
        let token = self.get_access_token().await?;

        let body = serde_json::json!({
            "name": new_name
        });

        let response = self.client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error: ApiError = response.json().await?;
            Err(anyhow!("Failed to rename: {}", error.error.message))
        }
    }

    /// 上传文件实现
    pub async fn upload_file_impl(&self, parent_path: &str, file_name: &str, content: &[u8]) -> Result<()> {
        let token = self.get_access_token().await?;
        
        // 小文件直接上传（< 4MB）
        if content.len() < 4 * 1024 * 1024 {
            let upload_url = if parent_path == "/" || parent_path.is_empty() {
                format!("{}/content", self.get_meta_url(&format!("/{}", file_name)))
            } else {
                format!("{}/content", self.get_meta_url(&format!("{}/{}", parent_path, file_name)))
            };

            let max_retries = 3;
            let mut retry_count = 0;

            while retry_count < max_retries {
                match self.client
                    .put(&upload_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("Content-Type", "application/octet-stream")
                    .timeout(std::time::Duration::from_secs(60))
                    .body(content.to_vec())
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() || response.status().as_u16() == 201 {
                            return Ok(());
                        } else {
                            let status = response.status();
                            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                            if retry_count >= max_retries - 1 {
                                return Err(anyhow!("Failed to upload file: HTTP {} - {}", status, error_text));
                            }
                        }
                    }
                    Err(e) => {
                        if retry_count >= max_retries - 1 {
                            return Err(anyhow!("Failed to upload file: {}", e));
                        }
                    }
                }
                
                retry_count += 1;
                // 等待一段时间后重试
                tokio::time::sleep(std::time::Duration::from_millis(1000 * retry_count as u64)).await;
            }
            
            Err(anyhow!("Failed to upload file after {} retries", max_retries))
        } else {
            // 大文件分块上传
            self.upload_large_file(parent_path, file_name, content).await
        }
    }

    /// 大文件分块上传
    async fn upload_large_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> Result<()> {
        let token = self.get_access_token().await?;
        
        // 创建上传会话
        let session_url = if parent_path == "/" || parent_path.is_empty() {
            format!("{}/createUploadSession", self.get_meta_url(&format!("/{}", file_name)))
        } else {
            format!("{}/createUploadSession", self.get_meta_url(&format!("{}/{}", parent_path, file_name)))
        };

        let session_body = serde_json::json!({
            "item": {
                "@microsoft.graph.conflictBehavior": "replace",
                "name": file_name
            }
        });

        let session_response = self.client
            .post(&session_url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&session_body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        if !session_response.status().is_success() {
            let error_text = session_response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to create upload session: {}", error_text));
        }

        #[derive(Deserialize)]
        struct UploadSession {
            #[serde(rename = "uploadUrl")]
            upload_url: String,
        }

        let session: UploadSession = session_response.json().await?;
        
        // 分块上传，使用较小的分块大小以提高稳定性
        let chunk_size = std::cmp::min((self.config.chunk_size * 1024 * 1024) as usize, 1024 * 1024); // 最大1MB
        let total_size = content.len();
        let mut uploaded = 0;
        let max_retries = 3;

        while uploaded < total_size {
            let end = std::cmp::min(uploaded + chunk_size, total_size);
            let chunk = &content[uploaded..end];
            
            let mut retry_count = 0;
            let mut success = false;
            
            while retry_count < max_retries && !success {
                match self.upload_chunk(&session.upload_url, chunk, uploaded, end - 1, total_size).await {
                    Ok(_) => {
                        success = true;
                        uploaded = end;
                    }
                    Err(e) => {
                        retry_count += 1;
                        if retry_count >= max_retries {
                            return Err(anyhow!("Failed to upload chunk after {} retries: {}", max_retries, e));
                        }
                        // 等待一段时间后重试
                        tokio::time::sleep(std::time::Duration::from_millis(1000 * retry_count)).await;
                    }
                }
            }
        }

        Ok(())
    }

    /// 上传单个分块，带重试机制
    async fn upload_chunk(&self, upload_url: &str, chunk: &[u8], start: usize, end: usize, total_size: usize) -> Result<()> {
        let response = self.client
            .put(upload_url)
            .header("Content-Range", format!("bytes {}-{}/{}", start, end, total_size))
            .header("Content-Length", chunk.len().to_string())
            .timeout(std::time::Duration::from_secs(60)) // 增加超时时间
            .body(chunk.to_vec())
            .send()
            .await?;

        if response.status().is_success() || response.status().as_u16() == 202 {
            Ok(())
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow!("Failed to upload chunk: HTTP {} - {}", status, error_text))
        }
    }
} 