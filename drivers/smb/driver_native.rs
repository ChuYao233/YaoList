//! SMB/CIFS 网络共享驱动实现（系统原生支持）
//!
//! - Windows：直接使用 UNC 路径 (\\server\share)
//! - Linux：通过 CIFS 挂载点访问
//!
//! 无需第三方库，使用标准文件系统 API

use std::ops::Range;
use std::path::PathBuf;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use tokio::io::{AsyncRead, AsyncWrite};
use serde::{Deserialize, Serialize};

use crate::storage::{StorageDriver, Entry, Capability, ProgressCallback, SpaceInfo};

/// SMB 驱动配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmbConfig {
    /// 服务器地址 (例如: 192.168.1.100)
    pub address: String,
    /// 用户名（Linux CIFS 挂载时使用）
    pub username: String,
    /// 密码（Linux CIFS 挂载时使用）
    #[serde(default)]
    pub password: String,
    /// 共享名称 (例如: shared)
    pub share_name: String,
    /// 根目录路径 (可选，默认为 "/")
    #[serde(default = "default_root_path")]
    pub root_path: String,
    /// 挂载路径（仅 Linux，留空则自动挂载到 /tmp/smb_mounts/xxx）
    #[serde(default)]
    pub mount_path: String,
}

fn default_root_path() -> String {
    "/".to_string()
}

/// SMB 驱动（系统原生实现）
pub struct SmbDriver {
    config: SmbConfig,
    /// 实际访问路径
    base_path: PathBuf,
}

impl SmbDriver {
    /// 创建新的 SMB 驱动实例
    pub fn new(config: SmbConfig) -> Result<Self> {
        let mut config = config;
        
        // 确保根路径格式正确
        if !config.root_path.starts_with('/') && !config.root_path.starts_with('\\') {
            config.root_path = format!("/{}", config.root_path);
        }
        
        // 构建基础路径
        let base_path = Self::build_base_path(&config)?;
        
        Ok(Self { config, base_path })
    }
    
    /// 构建基础访问路径
    #[cfg(target_family = "windows")]
    fn build_base_path(config: &SmbConfig) -> Result<PathBuf> {
        // Windows: 使用 UNC 路径 \\server\share\root_path
        let unc_path = format!(
            r"\\{}\{}{}",
            config.address,
            config.share_name,
            config.root_path.replace('/', r"\")
        );
        Ok(PathBuf::from(unc_path))
    }
    
    #[cfg(target_family = "unix")]
    fn build_base_path(config: &SmbConfig) -> Result<PathBuf> {
        // Linux: 需要先挂载 CIFS
        let mount_point = if config.mount_path.is_empty() {
            // 自动生成挂载点
            let mount_dir = format!(
                "/tmp/smb_mounts/{}_{}",
                config.address.replace('.', "_"),
                config.share_name
            );
            std::fs::create_dir_all(&mount_dir).ok();
            mount_dir
        } else {
            config.mount_path.clone()
        };
        
        // 检查是否已挂载
        let mount_path = PathBuf::from(&mount_point);
        if !mount_path.exists() {
            std::fs::create_dir_all(&mount_path)
                .map_err(|e| anyhow!("创建挂载点失败: {}", e))?;
        }
        
        // 尝试挂载（如果未挂载）
        if !Self::is_mounted(&mount_point) {
            Self::mount_cifs(config, &mount_point)?;
        }
        
        // 构建完整路径
        let root = config.root_path.trim_start_matches('/');
        let full_path = if root.is_empty() {
            mount_path
        } else {
            mount_path.join(root)
        };
        
        Ok(full_path)
    }
    
    #[cfg(target_family = "unix")]
    fn is_mounted(mount_point: &str) -> bool {
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            mounts.lines().any(|line| line.contains(mount_point))
        } else {
            false
        }
    }
    
    #[cfg(target_family = "unix")]
    fn mount_cifs(config: &SmbConfig, mount_point: &str) -> Result<()> {
        use std::process::Command;
        
        let source = format!("//{}/{}", config.address, config.share_name);
        let options = format!(
            "username={},password={},uid={}",
            config.username,
            config.password,
            unsafe { libc::getuid() }
        );
        
        let output = Command::new("mount")
            .args(["-t", "cifs", &source, mount_point, "-o", &options])
            .output()
            .map_err(|e| anyhow!("执行 mount 命令失败: {}", e))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("CIFS 挂载失败: {}", stderr));
        }
        
        tracing::info!("CIFS 挂载成功: {} -> {}", source, mount_point);
        Ok(())
    }
    
    /// 获取完整路径
    fn get_full_path(&self, path: &str) -> PathBuf {
        let path = path.trim_start_matches('/').trim_start_matches('\\');
        if path.is_empty() {
            self.base_path.clone()
        } else {
            self.base_path.join(path)
        }
    }
}

#[async_trait]
impl StorageDriver for SmbDriver {
    fn name(&self) -> &str {
        "smb"
    }
    
    fn version(&self) -> &str {
        "2.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: true,
            can_direct_link: false,
            max_chunk_size: None,
            can_concurrent_upload: true,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: true,
            can_batch_operations: true,
            max_file_size: None,
            requires_full_file_for_upload: false, // SMB支持流式写入
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let full_path = self.get_full_path(path);
        tracing::debug!("SMB 列出目录: {:?}", full_path);
        
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| anyhow!("读取目录失败: {}", e))?;
        
        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            
            let entry_path = if path == "/" || path.is_empty() {
                format!("/{}", name)
            } else {
                format!("{}/{}", path.trim_end_matches('/'), name)
            };
            
            let modified = metadata.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                .map(|dt| dt.to_rfc3339());
            
            entries.push(Entry {
                name,
                path: entry_path,
                size: metadata.len(),
                is_dir: metadata.is_dir(),
                modified,
            });
        }
        
        Ok(entries)
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let full_path = self.get_full_path(path);
        tracing::debug!("SMB 读取文件: {:?} (范围: {:?})", full_path, range);
        
        let file = tokio::fs::File::open(&full_path)
            .await
            .map_err(|e| anyhow!("打开文件失败: {}", e))?;
        
        if let Some(r) = range {
            use tokio::io::{AsyncSeekExt, AsyncReadExt};
            let mut file = file;
            file.seek(std::io::SeekFrom::Start(r.start)).await?;
            let limit = r.end - r.start;
            Ok(Box::new(file.take(limit)))
        } else {
            Ok(Box::new(file))
        }
    }
    
    async fn open_writer(
        &self,
        path: &str,
        _size_hint: Option<u64>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let full_path = self.get_full_path(path);
        tracing::debug!("SMB 写入文件: {:?}", full_path);
        
        // 确保父目录存在
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        
        let file = tokio::fs::File::create(&full_path)
            .await
            .map_err(|e| anyhow!("创建文件失败: {}", e))?;
        
        Ok(Box::new(file))
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        let full_path = self.get_full_path(path);
        tracing::debug!("SMB 删除: {:?}", full_path);
        
        let metadata = tokio::fs::metadata(&full_path).await?;
        
        if metadata.is_dir() {
            tokio::fs::remove_dir_all(&full_path)
                .await
                .map_err(|e| anyhow!("删除目录失败: {}", e))?;
        } else {
            tokio::fs::remove_file(&full_path)
                .await
                .map_err(|e| anyhow!("删除文件失败: {}", e))?;
        }
        
        Ok(())
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        let full_path = self.get_full_path(path);
        tracing::debug!("SMB 创建目录: {:?}", full_path);
        
        tokio::fs::create_dir_all(&full_path)
            .await
            .map_err(|e| anyhow!("创建目录失败: {}", e))?;
        
        Ok(())
    }
    
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let old_full = self.get_full_path(old_path);
        let new_full = old_full.parent()
            .map(|p| p.join(new_name))
            .ok_or_else(|| anyhow!("无效路径"))?;
        
        tracing::debug!("SMB 重命名: {:?} -> {:?}", old_full, new_full);
        
        tokio::fs::rename(&old_full, &new_full)
            .await
            .map_err(|e| anyhow!("重命名失败: {}", e))?;
        
        Ok(())
    }
    
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_full = self.get_full_path(old_path);
        let new_full = self.get_full_path(new_path);
        
        tracing::debug!("SMB 移动: {:?} -> {:?}", old_full, new_full);
        
        // 确保目标目录存在
        if let Some(parent) = new_full.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        
        tokio::fs::rename(&old_full, &new_full)
            .await
            .map_err(|e| anyhow!("移动失败: {}", e))?;
        
        Ok(())
    }
    
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_full = self.get_full_path(old_path);
        let new_full = self.get_full_path(new_path);
        
        tracing::debug!("SMB 复制: {:?} -> {:?}", old_full, new_full);
        
        // 确保目标目录存在
        if let Some(parent) = new_full.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        
        let metadata = tokio::fs::metadata(&old_full).await?;
        
        if metadata.is_dir() {
            copy_dir_recursive(&old_full, &new_full).await?;
        } else {
            tokio::fs::copy(&old_full, &new_full)
                .await
                .map_err(|e| anyhow!("复制失败: {}", e))?;
        }
        
        Ok(())
    }
    
    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        #[cfg(target_family = "windows")]
        {
            self.get_space_info_windows().await
        }
        
        #[cfg(target_family = "unix")]
        {
            self.get_space_info_unix().await
        }
    }
}

/// 递归复制目录
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    
    Ok(())
}

#[cfg(target_family = "windows")]
impl SmbDriver {
    async fn get_space_info_windows(&self) -> Result<Option<SpaceInfo>> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        
        let path_str = self.base_path.to_string_lossy();
        let wide_path: Vec<u16> = OsStr::new(path_str.as_ref())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        
        let mut free_bytes_available: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut total_free_bytes: u64 = 0;
        
        let result = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide_path.as_ptr(),
                &mut free_bytes_available as *mut u64,
                &mut total_bytes as *mut u64,
                &mut total_free_bytes as *mut u64,
            )
        };
        
        if result != 0 {
            Ok(Some(SpaceInfo {
                total: total_bytes,
                free: total_free_bytes,
                used: total_bytes.saturating_sub(total_free_bytes),
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(target_family = "unix")]
impl SmbDriver {
    async fn get_space_info_unix(&self) -> Result<Option<SpaceInfo>> {
        use std::ffi::CString;
        use std::mem::MaybeUninit;
        
        let path_str = self.base_path.to_string_lossy();
        let path_cstr = CString::new(path_str.as_bytes())
            .map_err(|_| anyhow!("无效路径"))?;
        
        unsafe {
            let mut stat = MaybeUninit::<libc::statvfs>::uninit();
            if libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) == 0 {
                let stat = stat.assume_init();
                let block_size = stat.f_frsize as u64;
                let total = stat.f_blocks as u64 * block_size;
                let free = stat.f_bavail as u64 * block_size;
                Ok(Some(SpaceInfo {
                    used: total.saturating_sub(free),
                    total,
                    free,
                }))
            } else {
                Ok(None)
            }
        }
    }
}
