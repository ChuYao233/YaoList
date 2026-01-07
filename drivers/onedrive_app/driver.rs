//! OneDrive App driver implementation / OneDrive App 驱动实现
//! Architecture principles / 架构原则：
//! - Driver only provides primitive capabilities (Reader/Writer) / 驱动只提供原语能力
//! - Core controls progress, concurrency, resume points / Core控制进度
//! - Never load files into memory, use streaming / 永远不把文件放内存

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::TryStreamExt;
use serde_json::Value;
use std::ops::Range;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::io::StreamReader;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, SpaceInfo, StorageDriver,
};

use super::api::OneDriveAppApi;
use super::types::OneDriveAppConfig;
use super::writer::OneDriveAppWriter;

/// OneDrive App driver capability / OneDrive App 驱动能力
fn onedrive_app_capability() -> Capability {
    Capability {
        can_range_read: true,
        can_append: false,
        can_direct_link: true, // OneDrive支持302重定向
        max_chunk_size: Some(60 * 1024 * 1024), // 60MB
        can_concurrent_upload: false,
        requires_oauth: true,
        can_multipart_upload: true, // OneDrive支持分片上传
        can_server_side_copy: true,
        can_batch_operations: false,
        max_file_size: Some(250 * 1024 * 1024 * 1024), // 250GB
        requires_full_file_for_upload: false, // OneDrive App支持流式写入
    }
}

/// OneDrive App driver / OneDrive App 驱动
pub struct OneDriveAppDriver {
    api: Arc<OneDriveAppApi>,
    config: OneDriveAppConfig,
}

impl OneDriveAppDriver {
    pub fn new(config: OneDriveAppConfig) -> Result<Self> {
        let api = Arc::new(OneDriveAppApi::new(config.clone())?);
        Ok(Self { api, config })
    }

    /// Convert OneDrive file to Entry / 转换OneDrive文件为Entry
    fn file_to_entry(&self, file: super::types::OneDriveFile, parent_path: &str) -> Entry {
        let is_dir = file.file.is_none();
        let size = file.size.unwrap_or(0) as u64;
        
        let path = if parent_path == "/" {
            format!("/{}", file.name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.name)
        };

        Entry {
            name: file.name,
            path,
            is_dir,
            size,
            modified: file.last_modified,
        }
    }
}

#[async_trait]
impl StorageDriver for OneDriveAppDriver {
    fn name(&self) -> &str {
        "onedrive_app"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        onedrive_app_capability()
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        // 处理根路径
        let list_path = if path == "/" {
            &self.config.root_folder_path
        } else {
            path
        };

        let files = self.api.get_files(list_path).await?;
        Ok(files
            .into_iter()
            .map(|f| self.file_to_entry(f, list_path))
            .collect())
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let file = self.api.get_file(path).await?;
        let download_url = file.download_url
            .ok_or_else(|| anyhow!("文件没有下载链接"))?;

        let final_url = if let Some(ref custom_host) = self.config.custom_host {
            let mut parsed = reqwest::Url::parse(&download_url)?;
            parsed.set_host(Some(custom_host))?;
            parsed.to_string()
        } else {
            download_url
        };

        let mut request = self.api.get_client().get(&final_url);
        if let Some(ref r) = range {
            request = request.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
        }

        let response = request.send().await?;
        // 支持200（成功）、206（部分内容）和302重定向后的状态码
        if !response.status().is_success() && response.status().as_u16() != 206 {
            return Err(anyhow!("下载失败: HTTP {}", response.status()));
        }

        // 流式传输：将响应体转换为AsyncRead，不加载到内存
        let stream = response.bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = StreamReader::new(stream);
        Ok(Box::new(reader))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let writer = OneDriveAppWriter::new(
            path.to_string(),
            size_hint,
            self.api.clone(),
            progress,
        );
        
        Ok(Box::new(writer))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.api.delete(path).await
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.api.create_dir(path).await
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        self.api.rename(old_path, new_name).await
    }

    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.api.move_item(old_path, new_path).await
    }

    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        self.api.copy_item(old_path, new_path).await
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        let file = self.api.get_file(path).await?;
        
        if let Some(download_url) = file.download_url {
            let final_url = if let Some(ref custom_host) = self.config.custom_host {
                let mut parsed = reqwest::Url::parse(&download_url)?;
                parsed.set_host(Some(custom_host))?;
                parsed.to_string()
            } else {
                download_url
            };
            Ok(Some(final_url))
        } else {
            Ok(None)
        }
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        if self.config.disable_disk_usage {
            return Ok(None);
        }

        match self.api.get_drive().await {
            Ok(drive) => {
                Ok(Some(SpaceInfo {
                    used: drive.quota.used,
                    total: drive.quota.total,
                    free: drive.quota.remaining,
                }))
            }
            Err(e) => {
                tracing::warn!("获取OneDrive App空间信息失败: {}", e);
                Ok(None)
            }
        }
    }
    
    fn show_space_in_frontend(&self) -> bool {
        !self.config.disable_disk_usage
    }
}

// ============ DriverFactory 实现 ============

pub struct OneDriveAppDriverFactory;

impl DriverFactory for OneDriveAppDriverFactory {
    fn driver_type(&self) -> &'static str {
        "onedrive_app"
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let od_config: OneDriveAppConfig = serde_json::from_value(config)?;
        Ok(Box::new(OneDriveAppDriver::new(od_config)?))
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "OneDrive App".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("region", "select")
                .title("地区")
                .options("global:国际版,cn:中国版(世纪互联),us:美国政府版,de:德国版")
                .default("global")
                .required(),
            ConfigItem::new("client_id", "string")
                .title("客户端 ID")
                .required(),
            ConfigItem::new("client_secret", "string")
                .title("客户端密钥")
                .required(),
            ConfigItem::new("tenant_id", "string")
                .title("租户 ID")
                .required(),
            ConfigItem::new("email", "string")
                .title("邮箱")
                .required(),
            ConfigItem::new("root_folder_path", "string")
                .title("根文件夹路径")
                .default("/"),
            ConfigItem::new("chunk_size", "number")
                .title("分片大小")
                .default("5")
                .help("上传分片大小(MB)"),
            ConfigItem::new("custom_host", "string")
                .title("自定义主机")
                .help("自定义加速下载链接"),
            ConfigItem::new("disable_disk_usage", "bool")
                .title("禁用磁盘使用量查询")
                .default("false"),
            ConfigItem::new("enable_direct_upload", "bool")
                .title("启用前端直传")
                .default("false")
                .help("允许不经服务器直接上传到OneDrive"),
            ConfigItem::new("proxy", "string")
                .title("代理地址")
                .help("本地代理地址，如：http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"),
        ]
    }
}

