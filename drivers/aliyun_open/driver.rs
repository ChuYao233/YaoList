//! 阿里云盘 Open StorageDriver 实现

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::RwLock;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, 
    ProgressCallback, SpaceInfo, StorageDriver,
};

use super::client::AliyunOpenClient;
use super::types::*;
use super::upload::{AliyunOpenReader, AliyunOpenWriter};

// ============ 配置 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliyunOpenConfig {
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default = "default_root_folder_id")]
    pub root_folder_id: String,
    #[serde(default = "default_drive_type")]
    pub drive_type: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default = "default_remove_way")]
    pub remove_way: String,
    #[serde(default = "default_livp_format")]
    pub livp_download_format: String,
}

fn default_root_folder_id() -> String { "root".to_string() }
fn default_drive_type() -> String { "resource".to_string() }
fn default_remove_way() -> String { "trash".to_string() }
fn default_livp_format() -> String { "jpeg".to_string() }

// ============ 驱动能力 ============

fn aliyun_open_capability() -> Capability {
    Capability {
        can_range_read: true,
        can_append: false,
        can_direct_link: false,
        max_chunk_size: None,
        can_concurrent_upload: false,
        requires_oauth: false,
        can_multipart_upload: false,
        can_server_side_copy: true,
        can_batch_operations: true,
        max_file_size: None,
        requires_full_file_for_upload: false,
    }
}

// ============ 驱动主体 ============

pub struct AliyunOpenDriver {
    config: AliyunOpenConfig,
    client: Arc<AliyunOpenClient>,
    drive_id: RwLock<String>,
    path_cache: RwLock<HashMap<String, String>>,
    initialized: RwLock<bool>,
}

impl AliyunOpenDriver {
    pub fn new(config: AliyunOpenConfig) -> Self {
        let client = Arc::new(AliyunOpenClient::new(
            config.client_id.clone(),
            config.client_secret.clone(),
            config.refresh_token.clone(),
        ));

        Self {
            config,
            client,
            drive_id: RwLock::new(String::new()),
            path_cache: RwLock::new(HashMap::new()),
            initialized: RwLock::new(false),
        }
    }

    /// 确保已初始化
    async fn ensure_initialized(&self) -> Result<()> {
        let initialized = *self.initialized.read().await;
        if !initialized {
            // 刷新 token
            let (new_refresh, new_access) = self.client.refresh_token().await?;
            self.client.set_tokens(new_access, new_refresh).await;

            // 获取 drive_id
            let drive_info: DriveInfo = self.client
                .post("/adrive/v1.0/user/getDriveInfo", serde_json::json!({}))
                .await?;

            let drive_id = match self.config.drive_type.as_str() {
                "backup" => drive_info.backup_drive_id,
                "resource" => drive_info.resource_drive_id,
                _ => drive_info.default_drive_id,
            };

            *self.drive_id.write().await = drive_id;
            *self.initialized.write().await = true;
        }
        Ok(())
    }

    /// 获取当前 drive_id
    async fn get_drive_id(&self) -> String {
        self.drive_id.read().await.clone()
    }

    /// 获取文件列表
    async fn get_files(&self, folder_id: &str) -> Result<Vec<AliyunFile>> {
        let mut files = Vec::new();
        let mut marker = String::new();

        loop {
            let mut body = serde_json::json!({
                "drive_id": self.get_drive_id().await,
                "parent_file_id": folder_id,
                "limit": 200,
                "order_by": "name",
                "order_direction": "ASC",
            });

            if !marker.is_empty() {
                body["marker"] = marker.into();
            }

            let resp: FileList = self.client
                .post("/adrive/v1.0/openFile/list", body)
                .await?;

            files.extend(resp.items);

            if let Some(next_marker) = resp.next_marker {
                if next_marker.is_empty() {
                    break;
                }
                marker = next_marker;
            } else {
                break;
            }
        }

        Ok(files)
    }

    /// 通过路径获取文件 ID
    async fn get_file_id(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.config.root_folder_id.clone());
        }

        // 检查缓存
        {
            let cache = self.path_cache.read().await;
            if let Some(id) = cache.get(path) {
                return Ok(id.clone());
            }
        }

        // 逐级查找
        let parts: Vec<&str> = path.split('/').collect();
        let mut current_id = self.config.root_folder_id.clone();

        for (i, part) in parts.iter().enumerate() {
            let files = self.get_files(&current_id).await?;
            
            if let Some(file) = files.iter().find(|f| f.name == *part) {
                current_id = file.file_id.clone();
                
                // 缓存中间路径
                let partial_path = parts[..=i].join("/");
                self.path_cache.write().await.insert(partial_path, current_id.clone());
            } else {
                // 清除父目录缓存
                if i > 0 {
                    let parent_path = parts[..i].join("/");
                    self.path_cache.write().await.remove(&parent_path);
                }
                return Err(anyhow!("文件不存在: {}", path));
            }
        }

        Ok(current_id)
    }

    /// 获取下载链接
    async fn get_download_url(&self, file_id: &str) -> Result<String> {
        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "file_id": file_id,
            "expire_sec": 14400,
        });

        let resp: DownloadUrlResponse = self.client
            .post("/adrive/v1.0/openFile/getDownloadUrl", body)
            .await?;

        if !resp.url.is_empty() {
            return Ok(resp.url);
        }

        // 处理 LIVP 格式
        if let Some(streams_url) = resp.streams_url {
            if let Some(url) = streams_url.get(&self.config.livp_download_format) {
                if let Some(url_str) = url.as_str() {
                    return Ok(url_str.to_string());
                }
            }
        }

        Err(anyhow!("无法获取下载链接"))
    }
}

#[async_trait]
impl StorageDriver for AliyunOpenDriver {
    fn name(&self) -> &str { "阿里云盘 Open" }
    fn version(&self) -> &str { "1.0.0" }
    fn capabilities(&self) -> Capability { aliyun_open_capability() }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_initialized().await?;
        let folder_id = self.get_file_id(path).await?;
        let files = self.get_files(&folder_id).await?;

        let mut items = Vec::new();
        for f in files {
            let file_name = f.name.clone();
            let file_size = f.get_size();
            let is_folder = f.is_folder();
            let modified_time = f.get_modified_time();
            
            items.push(Entry {
                name: file_name.clone(),
                path: if path == "/" { 
                    format!("/{}", file_name) 
                } else { 
                    format!("{}/{}", path.trim_end_matches('/'), file_name) 
                },
                size: file_size,
                is_dir: is_folder,
                modified: Some(chrono::DateTime::<chrono::Utc>::from(modified_time).format("%Y-%m-%d %H:%M:%S").to_string()),
            });
        }

        Ok(items)
    }

    async fn open_reader(&self, path: &str, range: Option<Range<u64>>) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_initialized().await?;
        let file_id = self.get_file_id(path).await?;
        let url = self.get_download_url(&file_id).await?;
        let range = range.map(|r| (r.start, r.end.saturating_sub(1)));
        Ok(Box::new(AliyunOpenReader::new(&url, range).await?))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        self.ensure_initialized().await?;
        
        let path = path.trim_matches('/');
        let (parent_path, file_name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };

        let parent_id = if parent_path.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_file_id(parent_path).await?
        };

        let size = size_hint.unwrap_or(0);
        let drive_id = self.get_drive_id().await;

        let writer = AliyunOpenWriter::new(
            self.client.clone(),
            drive_id,
            parent_id,
            file_name.to_string(),
            size,
        )?;

        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        let file_id = self.get_file_id(path).await?;
        
        let uri = if self.config.remove_way == "delete" {
            "/adrive/v1.0/openFile/delete"
        } else {
            "/adrive/v1.0/openFile/recyclebin/trash"
        };

        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "file_id": file_id,
        });

        let _: Value = self.client.post(uri, body).await?;

        // 清除缓存
        self.path_cache.write().await.remove(path.trim_matches('/'));
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        
        let path = path.trim_matches('/');
        let (parent_path, dir_name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path)
        };

        let parent_id = if parent_path.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_file_id(parent_path).await?
        };

        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "parent_file_id": parent_id,
            "name": dir_name,
            "type": "folder",
            "check_name_mode": "refuse",
        });

        let _: AliyunFile = self.client
            .post("/adrive/v1.0/openFile/create", body)
            .await?;

        Ok(())
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        self.ensure_initialized().await?;
        let file_id = self.get_file_id(old_path).await?;

        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "file_id": file_id,
            "name": new_name,
        });

        let _: AliyunFile = self.client
            .post("/adrive/v1.0/openFile/update", body)
            .await?;

        // 清除缓存
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn move_item(&self, src_path: &str, dest_path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        let file_id = self.get_file_id(src_path).await?;
        let dest_parent_path = std::path::Path::new(dest_path.trim_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let dest_parent_id = if dest_parent_path.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_file_id(&dest_parent_path).await?
        };

        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "file_id": file_id,
            "to_parent_file_id": dest_parent_id,
            "check_name_mode": "ignore",
        });

        let _: MoveOrCopyResponse = self.client
            .post("/adrive/v1.0/openFile/move", body)
            .await?;

        // 清除缓存
        self.path_cache.write().await.clear();
        Ok(())
    }

    async fn copy_item(&self, src_path: &str, dest_path: &str) -> Result<()> {
        self.ensure_initialized().await?;
        let file_id = self.get_file_id(src_path).await?;
        let dest_parent_path = std::path::Path::new(dest_path.trim_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let dest_parent_id = if dest_parent_path.is_empty() {
            self.config.root_folder_id.clone()
        } else {
            self.get_file_id(&dest_parent_path).await?
        };

        let body = serde_json::json!({
            "drive_id": self.get_drive_id().await,
            "file_id": file_id,
            "to_parent_file_id": dest_parent_id,
            "auto_rename": false,
        });

        let _: MoveOrCopyResponse = self.client
            .post("/adrive/v1.0/openFile/copy", body)
            .await?;

        Ok(())
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // 阿里云盘不支持直链
        Ok(None)
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        self.ensure_initialized().await?;
        
        let resp: super::types::SpaceInfo = self.client
            .post("/adrive/v1.0/user/getSpaceInfo", serde_json::json!({}))
            .await?;

        Ok(Some(SpaceInfo {
            total: resp.personal_space_info.total_size,
            used: resp.personal_space_info.used_size,
            free: resp.personal_space_info.total_size - resp.personal_space_info.used_size,
        }))
    }
}

// ============ 驱动工厂 ============

pub struct AliyunOpenDriverFactory;

impl DriverFactory for AliyunOpenDriverFactory {
    fn driver_type(&self) -> &'static str { "aliyun_open" }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "阿里云盘 Open".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("root".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("refresh_token", "string")
                .title("刷新令牌")
                .required()
                .help("从阿里云盘开放平台获取的 refresh_token"),
            ConfigItem::new("root_folder_id", "string")
                .title("根文件夹 ID")
                .default("root")
                .help("默认为 root，显示全部内容"),
            ConfigItem::new("drive_type", "select")
                .title("云盘类型")
                .options("default,resource,backup")
                .default("resource")
                .help("选择要访问的云盘类型"),
            ConfigItem::new("oauth_login", "action")
                .title("从阿里云盘登录")
                .link("/oauth/aliyun/authorize")
                .help("点击跳转到阿里云盘授权页面"),
            ConfigItem::new("client_id", "string")
                .title("客户端 ID")
                .help("自定义应用的客户端 ID（可选）"),
            ConfigItem::new("client_secret", "string")
                .title("应用密钥")
                .help("自定义应用的密钥（可选）"),
            ConfigItem::new("remove_way", "select")
                .title("删除方式")
                .options("trash,delete")
                .default("trash")
                .help("trash: 移到回收站, delete: 直接删除"),
            ConfigItem::new("rapid_upload", "bool")
                .title("秒传")
                .default("false")
                .help("启用秒传功能"),
            ConfigItem::new("internal_upload", "bool")
                .title("内部上传")
                .default("false")
                .help("阿里云北京 ECS 可开启此选项提升上传速度"),
            ConfigItem::new("livp_download_format", "select")
                .title("LIVP 下载格式")
                .options("jpeg,mov")
                .default("jpeg")
                .help("实况照片下载格式"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: AliyunOpenConfig = serde_json::from_value(config)?;
        Ok(Box::new(AliyunOpenDriver::new(config)))
    }
}
