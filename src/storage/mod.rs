use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use std::ops::Range;
use std::sync::Arc;

/// 进度回调类型 / Progress callback type
/// 参数: (已完成字节数, 总字节数) / Parameters: (completed_bytes, total_bytes)
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Configuration item definition / 配置项定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigItem {
    pub name: String,
    /// Display title (friendly name) / 显示标题
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl ConfigItem {
    pub fn new(name: &str, item_type: &str) -> Self {
        Self {
            name: name.to_string(),
            title: None,
            item_type: item_type.to_string(),
            default: None,
            options: None,
            required: false,
            help: None,
        }
    }
    
    pub fn title(mut self, val: &str) -> Self {
        self.title = Some(val.to_string());
        self
    }
    
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
    
    pub fn default(mut self, val: &str) -> Self {
        self.default = Some(val.to_string());
        self
    }
    
    pub fn help(mut self, val: &str) -> Self {
        self.help = Some(val.to_string());
        self
    }
    
    pub fn options(mut self, val: &str) -> Self {
        self.options = Some(val.to_string());
        self
    }
}

/// Driver configuration information / 驱动配置信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverConfig {
    pub name: String,
    #[serde(default)]
    pub local_sort: bool,
    #[serde(default)]
    pub only_proxy: bool,
    #[serde(default)]
    pub no_cache: bool,
    #[serde(default)]
    pub no_upload: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_root: Option<String>,
}

/// Complete driver information / 驱动完整信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    /// Common configuration items (mount_path, order, remark, etc.) / 通用配置项
    pub common: Vec<ConfigItem>,
    /// Driver-specific configuration items / 驱动特有配置项
    pub additional: Vec<ConfigItem>,
    /// Basic driver configuration / 驱动基本配置
    pub config: DriverConfig,
}

/// Generate common configuration items (defined in Core, shared by all drivers) / 生成通用配置项
pub fn get_common_items(config: &DriverConfig) -> Vec<ConfigItem> {
    let mut items = vec![
        ConfigItem::new("mount_path", "string")
            .required()
            .help("Mount path, must be unique"),
        ConfigItem::new("order", "number")
            .default("0")
            .help("Sort order"),
        ConfigItem::new("remark", "text")
            .help("Remark/Notes"),
    ];
    
    if !config.no_cache {
        items.push(
            ConfigItem::new("cache_expiration", "number")
                .default("30")
                .required()
                .help("Cache expiration time (seconds)")
        );
    }
    
    if !config.only_proxy {
        items.push(
            ConfigItem::new("web_proxy", "bool")
                .default("false")
                .help("Enable Web proxy")
        );
    }
    
    items
}

/// File entry information / 文件条目信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

/// Storage space information / 存储空间信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfo {
    /// Used space (bytes) / 已使用空间
    pub used: u64,
    /// Total space (bytes) / 总空间
    pub total: u64,
    /// Free space (bytes) / 剩余空间
    pub free: u64,
}

/// Driver capability declaration / 驱动能力声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Support range reading (resumable download) / 支持范围读取
    pub can_range_read: bool,
    /// Support append write / 支持追加写入
    pub can_append: bool,
    /// Support direct link download (302 redirect) / 支持直链下载
    pub can_direct_link: bool,
    /// Maximum chunk size (None means no limit) / 最大分片大小
    pub max_chunk_size: Option<u64>,
    /// Support concurrent upload / 支持并发上传
    pub can_concurrent_upload: bool,
    /// Require OAuth authentication (OneDrive, cloud drives, etc.) / 需要OAuth认证
    pub requires_oauth: bool,
    /// Support multipart upload (S3 multipart, OneDrive resumable) / 支持分片上传
    pub can_multipart_upload: bool,
    /// Support server-side copy (no download needed) / 支持服务端复制
    pub can_server_side_copy: bool,
    /// Support batch operations / 支持批量操作
    pub can_batch_operations: bool,
    /// Maximum file size limit (None means no limit) / 最大文件大小限制
    pub max_file_size: Option<u64>,
}

impl Default for Capability {
    fn default() -> Self {
        Self {
            can_range_read: false,
            can_append: false,
            can_direct_link: false,
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
        }
    }
}

/// Storage driver interface (provides only primitive operations) / 存储驱动接口
#[async_trait]
pub trait StorageDriver: Send + Sync {
    /// Driver name / 驱动名称
    fn name(&self) -> &str;
    
    /// Driver version / 驱动版本
    fn version(&self) -> &str;
    
    /// Driver capabilities / 驱动能力
    fn capabilities(&self) -> Capability;
    
    /// List directory contents / 列出目录内容
    async fn list(&self, path: &str) -> Result<Vec<Entry>>;
    
    /// Open file reader (supports range reading) / 打开文件读取器
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>>;
    
    /// Open file writer / 打开文件写入器
    /// progress: 可选的进度回调，驱动在上传过程中调用以报告进度
    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>>;
    
    /// Put complete file data - 上传完整文件
    /// 云盘驱动应重写此方法，自己处理分片上传、秒传等
    /// 默认实现使用open_writer，适合本地存储等流式驱动
    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        progress: Option<ProgressCallback>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut writer = self.open_writer(path, Some(data.len() as u64), progress).await?;
        writer.write_all(&data).await?;
        writer.shutdown().await?;
        Ok(())
    }
    
    /// Delete file or directory / 删除文件或目录
    async fn delete(&self, path: &str) -> Result<()>;
    
    /// Create directory / 创建目录
    async fn create_dir(&self, path: &str) -> Result<()>;
    
    /// Rename file or directory / 重命名文件或目录
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()>;
    
    /// Move file or directory / 移动文件或目录
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()>;
    
    /// Copy file or directory (default implementation: read then write) / 复制文件或目录
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        // Default implementation: read entire file then write
        // Specific drivers can override to implement server-side copy
        let mut reader = self.open_reader(old_path, None).await?;
        let mut writer = self.open_writer(new_path, None, None).await?;
        tokio::io::copy(&mut reader, &mut writer).await?;
        // 必须调用shutdown确保所有数据写入完成（特别是分片上传）
        writer.shutdown().await?;
        Ok(())
    }
    
    /// Get direct link URL (if supported) / 获取直链 URL
    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }
    
    /// Get local filesystem path (only local drivers support)
    /// Returns None if not a local driver / 获取本地文件系统路径
    fn get_local_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let _ = path;
        None
    }
    
    /// Check if it's a local driver / 判断是否是本地驱动
    fn is_local(&self) -> bool {
        false
    }
    
    /// Get storage space information (primitive operation)
    /// Returns None if driver doesn't support or cannot get info / 获取存储空间信息
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        Ok(None)
    }
    
    /// Whether to show space info in frontend file list / 是否在前台显示空间信息
    fn show_space_in_frontend(&self) -> bool {
        false
    }
    
    /// Get updated config (for saving tokens etc.) / 获取更新后的配置
    /// Returns None if config hasn't changed / 如果配置未变更则返回None
    fn get_updated_config(&self) -> Option<serde_json::Value> {
        None
    }
}

pub mod manager;
pub mod local_factory;

pub use manager::{StorageManager, DriverFactory, DriverBox};
pub use local_factory::LocalDriverFactory;
