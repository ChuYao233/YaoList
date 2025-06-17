use anyhow::{anyhow, Result};
use reqwest::{Client, header::{HeaderMap, HeaderValue}};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use async_trait::async_trait;
use crate::drivers::{Driver, DriverFactory, DriverInfo, FileInfo};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AListConfig {
    #[serde(rename = "url")]
    pub address: String,
    
    pub meta_password: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
    
    #[serde(default = "default_pass_ua_to_upstream")]
    pub pass_ua_to_upstream: bool,
    
    #[serde(default = "default_forward_archive_req")]
    pub forward_archive_requests: bool,
    
    #[serde(default = "default_root_path")]
    pub root_path: String,
}

fn default_pass_ua_to_upstream() -> bool { true }
fn default_forward_archive_req() -> bool { true }
fn default_root_path() -> String { "/".to_string() }

impl Default for AListConfig {
    fn default() -> Self {
        Self {
            address: String::new(),
            meta_password: None,
            username: None,
            password: None,
            token: None,
            pass_ua_to_upstream: true,
            forward_archive_requests: true,
            root_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageReq {
    pub page: i32,
    pub per_page: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListReq {
    #[serde(flatten)]
    pub page_req: PageReq,
    pub path: String,
    pub password: Option<String>,
    pub refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjResp {
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub modified: DateTime<Utc>,
    pub created: DateTime<Utc>,
    pub sign: Option<String>,
    pub thumb: Option<String>,
    #[serde(rename = "type")]
    pub obj_type: Option<i32>,
    #[serde(rename = "hashinfo")]
    pub hash_info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsListResp {
    pub content: Vec<ObjResp>,
    pub total: i64,
    pub readme: Option<String>,
    pub write: bool,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsGetReq {
    pub path: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsGetResp {
    #[serde(flatten)]
    pub obj: ObjResp,
    pub raw_url: String,
    pub readme: Option<String>,
    pub provider: Option<String>,
    pub related: Option<Vec<ObjResp>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkdirReq {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveCopyReq {
    pub src_dir: String,
    pub dst_dir: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameReq {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveReq {
    pub dir: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResp {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResp {
    pub id: i32,
    pub username: String,
    pub password: Option<String>,
    pub base_path: Option<String>,
    pub role: i32,
    pub disabled: bool,
    pub permission: Option<i32>,
    pub sso_id: Option<String>,
    pub otp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AListResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

pub struct AListDriver {
    config: AListConfig,
    client: Client,
    token: Option<String>,
}

impl AListDriver {
    pub fn new(config: AListConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;
        
        Ok(Self {
            token: config.token.clone(),
            config,
            client,
        })
    }
    
    async fn init(&mut self) -> Result<()> {
        // 确保地址以/结尾被去掉
        self.config.address = self.config.address.trim_end_matches('/').to_string();
        
        // 获取当前用户信息
        let me_resp = self.request_get::<MeResp>("/me").await?;
        
        // 如果用户名不匹配，重新登录
        if let Some(username) = &self.config.username {
            if me_resp.username != *username {
                self.login().await?;
                
                // 重新获取用户信息
                let _me_resp = self.request_get::<MeResp>("/me").await?;
            }
        }
        
        // 检查访客权限
        if me_resp.role == 0 { // GUEST role
            let settings_url = format!("{}/api/public/settings", self.config.address);
            let resp = self.client.get(&settings_url).send().await?;
            let settings: serde_json::Value = resp.json().await?;
            
            let allow_mounted = settings
                .get("data")
                .and_then(|d| d.get("allow_mounted"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
                
            if !allow_mounted {
                return Err(anyhow!("The site does not allow mounted"));
            }
        }
        
        Ok(())
    }
    
    async fn login(&mut self) -> Result<()> {
        let username = self.config.username.as_ref()
            .ok_or_else(|| anyhow!("Username is required for login"))?;
        let password = self.config.password.as_ref()
            .ok_or_else(|| anyhow!("Password is required for login"))?;
        
        let login_req = LoginReq {
            username: username.clone(),
            password: password.clone(),
        };
        
        let resp = self.request_post::<LoginResp>("/auth/login", &login_req).await?;
        self.token = Some(resp.token);
        
        println!("AList login successful");
        Ok(())
    }
    
    async fn request_get<T>(&self, api: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = format!("{}/api{}", self.config.address, api);
        let mut headers = HeaderMap::new();
        
        if let Some(token) = &self.token {
            headers.insert("Authorization", HeaderValue::from_str(token)?);
        }
        
        let resp = self.client
            .get(&url)
            .headers(headers)
            .send()
            .await?;
        
        let response: AListResponse<T> = resp.json().await?;
        
        if response.code != 200 {
            return Err(anyhow!("Request failed: code={}, message={}", response.code, response.message));
        }
        
        response.data.ok_or_else(|| anyhow!("No data in response"))
    }
    
    async fn request_post<T>(&self, api: &str, body: &impl Serialize) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = format!("{}/api{}", self.config.address, api);
        let mut headers = HeaderMap::new();
        
        if let Some(token) = &self.token {
            headers.insert("Authorization", HeaderValue::from_str(token)?);
        }
        
        let resp = self.client
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await?;
        
        let status_code = resp.status().as_u16();
        let response: AListResponse<T> = resp.json().await?;
        
        if response.code != 200 {
            // 处理认证过期
            if (response.code == 401 || response.code == 403) && self.config.username.is_some() {
                return Err(anyhow!("Authentication required: code={}, message={}", response.code, response.message));
            }
            return Err(anyhow!("Request failed: code={}, message={}", response.code, response.message));
        }
        
        response.data.ok_or_else(|| anyhow!("No data in response"))
    }
    
    async fn request_simple(&self, api: &str, body: &impl Serialize) -> Result<()> {
        let url = format!("{}/api{}", self.config.address, api);
        let mut headers = HeaderMap::new();
        
        if let Some(token) = &self.token {
            headers.insert("Authorization", HeaderValue::from_str(token)?);
        }
        
        let resp = self.client
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await?;
        
        let response: AListResponse<serde_json::Value> = resp.json().await?;
        
        if response.code != 200 {
            return Err(anyhow!("Request failed: code={}, message={}", response.code, response.message));
        }
        
        Ok(())
    }
    
    fn build_path(&self, path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("{}/{}", self.config.root_path.trim_end_matches('/'), path.trim_start_matches('/'))
        }
    }
}

#[async_trait]
impl Driver for AListDriver {
    async fn list(&self, path: &str) -> anyhow::Result<Vec<FileInfo>> {
        let full_path = self.build_path(path);
        
        let list_req = ListReq {
            page_req: PageReq {
                page: 1,
                per_page: 0, // 0 means get all
            },
            path: full_path,
            password: self.config.meta_password.clone(),
            refresh: false,
        };
        
        let resp = self.request_post::<FsListResp>("/fs/list", &list_req).await?;
        
        let mut files = Vec::new();
        for obj in resp.content {
            let obj_name = obj.name.clone();
            files.push(FileInfo {
                name: obj.name,
                path: format!("{}/{}", path.trim_end_matches('/'), obj_name),
                size: obj.size as u64,
                is_dir: obj.is_dir,
                modified: obj.modified.format("%Y-%m-%d %H:%M:%S").to_string(),
            });
        }
        
        Ok(files)
    }
    
    async fn download(&self, _path: &str) -> anyhow::Result<tokio::fs::File> {
        Err(anyhow!("Direct file download not supported, use get_download_url instead"))
    }
    
    async fn get_download_url(&self, path: &str) -> anyhow::Result<Option<String>> {
        let full_path = self.build_path(path);
        
        let get_req = FsGetReq {
            path: full_path,
            password: self.config.meta_password.clone(),
        };
        
        let resp = self.request_post::<FsGetResp>("/fs/get", &get_req).await?;
        Ok(Some(resp.raw_url))
    }
    
    async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> anyhow::Result<()> {
        let full_parent_path = self.build_path(parent_path);
        let file_path = format!("{}/{}", full_parent_path.trim_end_matches('/'), file_name);
        
        let url = format!("{}/api/fs/put", self.config.address);
        let mut headers = HeaderMap::new();
        
        if let Some(token) = &self.token {
            headers.insert("Authorization", HeaderValue::from_str(token)?);
        }
        headers.insert("File-Path", HeaderValue::from_str(&file_path)?);
        if let Some(password) = &self.config.meta_password {
            headers.insert("Password", HeaderValue::from_str(password)?);
        }
        
        let resp = self.client
            .put(&url)
            .headers(headers)
            .body(content.to_vec())
            .send()
            .await?;
        
        if resp.status().is_success() {
            let response: AListResponse<serde_json::Value> = resp.json().await?;
            if response.code != 200 {
                return Err(anyhow!("Upload failed: code={}, message={}", response.code, response.message));
            }
            println!("File uploaded successfully: {}", file_name);
            Ok(())
        } else {
            Err(anyhow!("Upload failed with status: {}", resp.status()))
        }
    }
    
    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let full_path = self.build_path(path);
        let (dir, name) = if let Some(pos) = full_path.rfind('/') {
            (full_path[..pos].to_string(), full_path[pos + 1..].to_string())
        } else {
            ("/".to_string(), full_path)
        };
        
        let remove_req = RemoveReq {
            dir,
            names: vec![name],
        };
        
        self.request_simple("/fs/remove", &remove_req).await?;
        println!("Successfully deleted: {}", path);
        Ok(())
    }
    
    async fn rename(&self, path: &str, new_name: &str) -> anyhow::Result<()> {
        let full_path = self.build_path(path);
        
        let rename_req = RenameReq {
            path: full_path,
            name: new_name.to_string(),
        };
        
        self.request_simple("/fs/rename", &rename_req).await?;
        println!("Successfully renamed: {} -> {}", path, new_name);
        Ok(())
    }
    
    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> anyhow::Result<()> {
        let full_parent_path = self.build_path(parent_path);
        let folder_path = format!("{}/{}", full_parent_path.trim_end_matches('/'), folder_name);
        
        let mkdir_req = MkdirReq {
            path: folder_path,
        };
        
        self.request_simple("/fs/mkdir", &mkdir_req).await?;
        println!("Successfully created folder: {}", folder_name);
        Ok(())
    }
    
    async fn get_file_info(&self, path: &str) -> anyhow::Result<FileInfo> {
        let full_path = self.build_path(path);
        
        let get_req = FsGetReq {
            path: full_path,
            password: self.config.meta_password.clone(),
        };
        
        let resp = self.request_post::<FsGetResp>("/fs/get", &get_req).await?;
        
        Ok(FileInfo {
            name: resp.obj.name,
            path: path.to_string(),
            size: resp.obj.size as u64,
            is_dir: resp.obj.is_dir,
            modified: resp.obj.modified.format("%Y-%m-%d %H:%M:%S").to_string(),
        })
    }
    
    async fn move_file(&self, file_path: &str, new_parent_path: &str) -> anyhow::Result<()> {
        let full_file_path = self.build_path(file_path);
        let full_new_parent_path = self.build_path(new_parent_path);
        
        let (src_dir, file_name) = if let Some(pos) = full_file_path.rfind('/') {
            (full_file_path[..pos].to_string(), full_file_path[pos + 1..].to_string())
        } else {
            ("/".to_string(), full_file_path)
        };
        
        let move_req = MoveCopyReq {
            src_dir,
            dst_dir: full_new_parent_path,
            names: vec![file_name],
        };
        
        self.request_simple("/fs/move", &move_req).await?;
        println!("Successfully moved: {} -> {}", file_path, new_parent_path);
        Ok(())
    }
    
    async fn copy_file(&self, file_path: &str, new_parent_path: &str) -> anyhow::Result<()> {
        let full_file_path = self.build_path(file_path);
        let full_new_parent_path = self.build_path(new_parent_path);
        
        let (src_dir, file_name) = if let Some(pos) = full_file_path.rfind('/') {
            (full_file_path[..pos].to_string(), full_file_path[pos + 1..].to_string())
        } else {
            ("/".to_string(), full_file_path)
        };
        
        let copy_req = MoveCopyReq {
            src_dir,
            dst_dir: full_new_parent_path,
            names: vec![file_name],
        };
        
        self.request_simple("/fs/copy", &copy_req).await?;
        println!("Successfully copied: {} -> {}", file_path, new_parent_path);
        Ok(())
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn stream_download(&self, path: &str) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        let resp = self.client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        let data = resp.bytes().await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        let stream = futures::stream::once(
            futures::future::ready(Ok(axum::body::Bytes::from(data)))
        );
        
        let content_type = "application/octet-stream".to_string();
        Ok(Some((Box::new(stream), content_type)))
    }
    
    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        let download_url = match self.get_download_url(path).await? {
            Some(url) => url,
            None => return Ok(None),
        };
        
        let mut req = self.client.get(&download_url);
        
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

pub struct AListDriverFactory;

impl DriverFactory for AListDriverFactory {
    fn driver_type(&self) -> &'static str {
        "alist"
    }
    
    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "alist".to_string(),
            display_name: "AList V3".to_string(),
            description: "AList V3 API驱动，支持挂载其他AList实例作为存储后端".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "title": "AList地址",
                        "description": "AList服务器的完整URL地址（如：https://pan.example.com）"
                    },
                    "username": {
                        "type": "string",
                        "title": "用户名",
                        "description": "AList账号用户名（可选）"
                    },
                    "password": {
                        "type": "string",
                        "title": "密码",
                        "format": "password",
                        "description": "AList账号密码（可选）"
                    },
                    "token": {
                        "type": "string",
                        "title": "令牌",
                        "description": "已有的认证令牌（可选，如果有用户名密码会自动获取）"
                    },
                    "meta_password": {
                        "type": "string",
                        "title": "元密码",
                        "format": "password",
                        "description": "访问受保护文件夹的密码（可选）"
                    },
                    "root_path": {
                        "type": "string",
                        "title": "根路径",
                        "default": "/",
                        "description": "挂载的根路径"
                    },
                    "pass_ua_to_upstream": {
                        "type": "boolean",
                        "title": "传递User-Agent",
                        "default": true,
                        "description": "是否将客户端User-Agent传递给上游"
                    },
                    "forward_archive_requests": {
                        "type": "boolean",
                        "title": "转发压缩包请求",
                        "default": true,
                        "description": "是否转发压缩包相关请求"
                    }
                },
                "required": ["url"]
            }),
        }
    }
    
    fn create_driver(&self, config: serde_json::Value) -> anyhow::Result<Box<dyn Driver>> {
        let config: AListConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("Invalid alist config: {}", e))?;
        
        let mut driver = AListDriver::new(config)?;
        
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