use async_trait::async_trait;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use std::ops::Range;

use crate::storage::{StorageDriver, Entry, Capability, SpaceInfo};

pub struct LocalDriver {
    root: PathBuf,
    show_space_info: bool,
}

impl LocalDriver {
    pub fn new(root: PathBuf) -> Self {
        Self { root, show_space_info: true }
    }
    
    pub fn with_config(root: PathBuf, show_space_info: bool) -> Self {
        Self { root, show_space_info }
    }
    
    /// Get root directory / 获取根目录
    pub fn root(&self) -> &PathBuf {
        &self.root
    }
    
    /// Normalize path to prevent directory traversal attacks (optimized: avoid unnecessary IO) / 规范化路径
    fn normalize_path(&self, path: &str) -> Result<PathBuf> {
        let path = path.trim_start_matches('/').replace('\\', "/");
        
        // Check if path contains directory traversal attack patterns / 检查路径
        let normalized: Vec<&str> = path.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        for component in &normalized {
            if *component == ".." {
                return Err(anyhow!("Access path exceeds root directory scope"));
            }
        }
        
        let full_path = self.root.join(normalized.join("/"));
        Ok(full_path)
    }
}

#[async_trait]
impl StorageDriver for LocalDriver {
    fn name(&self) -> &str {
        "local"
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
            can_multipart_upload: false, // 本地存储不需要缓存，直接流式写入
            can_server_side_copy: true,
            can_batch_operations: true,
            max_file_size: None,
            requires_full_file_for_upload: false, // 本地存储支持流式写入
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let full_path = self.normalize_path(path)?;
        let mut entries = tokio::fs::read_dir(full_path).await?;
        let mut result = Vec::new();
        
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = metadata.is_dir();
            let size = if is_dir { 0 } else { metadata.len() };
            
            let modified = metadata.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                .flatten()
                .map(|dt| dt.to_rfc3339());
            
            result.push(Entry {
                name,
                path: format!("{}/{}", path.trim_end_matches('/'), entry.file_name().to_string_lossy()),
                is_dir,
                size,
                modified,
            });
        }
        
        Ok(result)
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let full_path = self.normalize_path(path)?;
        let range_clone = range.clone();
        
        // Use sync IO to improve network share performance / 使用同步IO
        let file = tokio::task::spawn_blocking(move || {
            let mut file = std::fs::File::open(&full_path)?;
            if let Some(r) = range_clone {
                use std::io::Seek;
                file.seek(std::io::SeekFrom::Start(r.start))?;
            }
            Ok::<std::fs::File, anyhow::Error>(file)
        }).await??;
        
        // Convert to async / 转换为异步
        let async_file = tokio::fs::File::from_std(file);
        
        if let Some(r) = range {
            use tokio::io::AsyncReadExt;
            let limited = async_file.take(r.end - r.start);
            Ok(Box::new(limited))
        } else {
            Ok(Box::new(async_file))
        }
    }
    
    async fn open_writer(
        &self,
        path: &str,
        _size_hint: Option<u64>,
        _progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let full_path = self.normalize_path(path)?;
        
        // Use sync IO to improve network share performance / 使用同步IO
        let file = tokio::task::spawn_blocking(move || {
            // Ensure parent directory exists / 确保父目录存在
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = std::fs::File::create(&full_path)?;
            Ok::<std::fs::File, anyhow::Error>(file)
        }).await??;
        
        // Convert to async / 转换为异步
        let async_file = tokio::fs::File::from_std(file);
        Ok(Box::new(async_file))
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        let full_path = self.normalize_path(path)?;
        
        if full_path.is_dir() {
            tokio::fs::remove_dir_all(full_path).await?;
        } else {
            tokio::fs::remove_file(full_path).await?;
        }
        
        Ok(())
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        let full_path = self.normalize_path(path)?;
        tokio::fs::create_dir_all(full_path).await?;
        Ok(())
    }
    
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let old_full = self.normalize_path(old_path)?;
        let parent = old_full.parent()
            .ok_or_else(|| anyhow!("无法获取父目录"))?;
        let new_full = parent.join(new_name);
        
        tokio::fs::rename(old_full, new_full).await?;
        Ok(())
    }
    
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let old_full = self.normalize_path(old_path)?;
        let new_full = self.normalize_path(new_path)?;
        
        // Ensure target directory exists / 确保目标目录存在
        if let Some(parent) = new_full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        tokio::fs::rename(old_full, new_full).await?;
        Ok(())
    }
    
    /// Server-side copy optimization: use sync IO to improve network share performance / 服务端复制优化
    async fn copy_item(&self, src_path: &str, dst_path: &str) -> Result<()> {
        let src_full = self.normalize_path(src_path)?;
        let dst_full = self.normalize_path(dst_path)?;
        
        // Use spawn_blocking + std::fs to improve network share performance / 使用 spawn_blocking
        tokio::task::spawn_blocking(move || {
            // Ensure target directory exists / 确保目标目录存在
            if let Some(parent) = dst_full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            if src_full.is_dir() {
                // Recursively copy directory / 递归复制目录
                copy_dir_recursive_sync(&src_full, &dst_full)?;
            } else {
                // Copy file / 复制文件
                std::fs::copy(&src_full, &dst_full)?;
            }
            
            Ok::<(), anyhow::Error>(())
        }).await??;
        
        Ok(())
    }
    
    fn get_local_path(&self, path: &str) -> Option<std::path::PathBuf> {
        self.normalize_path(path).ok()
    }
    
    fn is_local(&self) -> bool {
        true
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        let root = self.root.clone();
        
        let info = tokio::task::spawn_blocking(move || {
            get_disk_space(&root)
        }).await?;
        
        Ok(info)
    }
    
    /// Whether to show space info in frontend / 是否在前台显示空间信息
    fn show_space_in_frontend(&self) -> bool {
        self.show_space_info
    }
}

/// Get disk space information (cross-platform implementation) / 获取磁盘空间信息
#[cfg(target_os = "windows")]
fn get_disk_space(path: &std::path::Path) -> Option<SpaceInfo> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    
    // Get drive letter of path / 获取路径的盘符
    let path_str = path.to_string_lossy();
    let drive_path = if path_str.len() >= 2 && path_str.chars().nth(1) == Some(':') {
        format!("{}\\", &path_str[..2])
    } else {
        path_str.to_string()
    };
    
    let wide: Vec<u16> = OsStr::new(&drive_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    
    let mut free_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;
    
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                lpDirectoryName: *const u16,
                lpFreeBytesAvailableToCaller: *mut u64,
                lpTotalNumberOfBytes: *mut u64,
                lpTotalNumberOfFreeBytes: *mut u64,
            ) -> i32;
        }
        
        let result = GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_bytes,
            &mut total_bytes,
            &mut total_free_bytes,
        );
        
        if result != 0 {
            Some(SpaceInfo {
                used: total_bytes.saturating_sub(free_bytes),
                total: total_bytes,
                free: free_bytes,
            })
        } else {
            None
        }
    }
}

/// Get disk space information (Linux/macOS) / 获取磁盘空间信息
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn get_disk_space(path: &std::path::Path) -> Option<SpaceInfo> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    
    let path_cstr = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    
    unsafe {
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        if libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) == 0 {
            let stat = stat.assume_init();
            let block_size = stat.f_frsize as u64;
            let total = stat.f_blocks as u64 * block_size;
            let free = stat.f_bavail as u64 * block_size;
            Some(SpaceInfo {
                used: total.saturating_sub(free),
                total,
                free,
            })
        } else {
            None
        }
    }
}

/// Fallback implementation for other platforms / 其他平台的fallback
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn get_disk_space(_path: &std::path::Path) -> Option<SpaceInfo> {
    None
}

/// Recursively copy directory (async version) / 递归复制目录
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    
    Ok(())
}

/// 递归复制目录（同步版本，用于spawn_blocking）
fn copy_dir_recursive_sync(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            copy_dir_recursive_sync(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    
    Ok(())
}

