use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::io::SeekFrom;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use dav_server::davpath::DavPath;
use dav_server::fs::{
    DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
    OpenOptions, ReadDirMeta,
};
use futures::stream;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use super::config::{AuthenticatedUser, UserAuthenticator, WebDavConfig};
use crate::storage::{Entry, StorageManager};
use crate::utils::should_hide_file;

/// 元信息结构
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Meta {
    pub id: String,
    pub path: String,
    pub password: Option<String>,
    pub p_sub: bool,
    pub write: bool,
    pub w_sub: bool,
    pub hide: Option<String>,
    pub h_sub: bool,
    pub readme: Option<String>,
    pub r_sub: bool,
    pub header: Option<String>,
    pub header_sub: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 检查隐藏规则是否应用到指定路径
fn is_hide_apply(meta_path: &str, req_path: &str, h_sub: bool) -> bool {
    let meta_path = fix_and_clean_path(meta_path);
    let req_path = fix_and_clean_path(req_path);
    
    if meta_path == req_path {
        return true;
    }
    
    if !is_sub_path(&meta_path, &req_path) {
        return false;
    }
    
    let relative = req_path.strip_prefix(&meta_path).unwrap_or(&req_path);
    let relative = relative.trim_start_matches('/');
    let depth = relative.matches('/').count();
    
    depth == 0 || h_sub
}

/// 挂载点信息
#[derive(Debug, Clone)]
pub struct MountInfo {
    pub id: String,
    pub mount_path: String,
    pub order: i32,
}

/// 清理路径
fn fix_and_clean_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = if path.is_empty() || !path.starts_with('/') {
        format!("/{}", path)
    } else {
        path
    };
    
    // 移除连续斜杠
    let mut result = String::new();
    let mut prev_slash = false;
    for c in path.chars() {
        if c == '/' {
            if !prev_slash {
                result.push(c);
            }
            prev_slash = true;
        } else {
            result.push(c);
            prev_slash = false;
        }
    }
    
    // 移除末尾斜杠（根目录除外）
    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }
    
    result
}

/// 检查路径是否是子路径
fn is_sub_path(parent: &str, child: &str) -> bool {
    let parent = fix_and_clean_path(parent);
    let child = fix_and_clean_path(child);
    
    if parent == "/" {
        return true;
    }
    
    child == parent || child.starts_with(&format!("{}/", parent))
}

/// 将用户请求路径与用户根路径结合
fn join_user_path(base_path: &str, req_path: &str) -> Result<String, String> {
    if req_path.contains("..") {
        return Err("路径不合法".to_string());
    }
    
    let clean_req = fix_and_clean_path(req_path);
    let clean_base = fix_and_clean_path(base_path);
    
    if clean_base == "/" {
        return Ok(clean_req);
    }
    
    let full_path = if clean_req == "/" {
        clean_base.clone()
    } else {
        format!("{}{}", clean_base.trim_end_matches('/'), clean_req)
    };
    
    Ok(fix_and_clean_path(&full_path))
}

/// WebDAV文件系统适配器
/// 将StorageDriver包装成dav-server的DavFileSystem
/// 使用与前端相同的逻辑：通过挂载点解析驱动，支持所有驱动类型
#[derive(Clone)]
pub struct WebDavFs {
    storage_manager: StorageManager,
    db: SqlitePool,
    user: Arc<RwLock<Option<AuthenticatedUser>>>,
}

impl Debug for WebDavFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavFs").finish()
    }
}

impl WebDavFs {
    pub fn new(storage_manager: StorageManager, db: SqlitePool) -> Self {
        Self {
            storage_manager,
            db,
            user: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_user(storage_manager: StorageManager, db: SqlitePool, user: AuthenticatedUser) -> Self {
        Self {
            storage_manager,
            db,
            user: Arc::new(RwLock::new(Some(user))),
        }
    }

    /// 设置用户
    pub async fn set_user(&self, user: AuthenticatedUser) {
        let mut guard = self.user.write().await;
        *guard = Some(user);
    }

    /// 获取用户的根路径
    async fn get_root_path(&self) -> String {
        let user = self.user.read().await;
        user.as_ref()
            .and_then(|u| u.permissions.root_path.clone())
            .unwrap_or_else(|| "/".to_string())
    }

    /// 从数据库获取所有启用的挂载点
    async fn get_all_mounts(&self) -> Vec<MountInfo> {
        let db_drivers: Vec<(String, String)> = sqlx::query_as(
            "SELECT name, config FROM drivers WHERE enabled = 1"
        )
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();
        
        db_drivers.iter().filter_map(|(id, config_str)| {
            let config: Value = serde_json::from_str(config_str).ok()?;
            let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
            let order = config.get("order").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            Some(MountInfo {
                id: id.clone(),
                mount_path: mount_path.to_string(),
                order,
            })
        }).collect()
    }

    /// 获取匹配路径的驱动（最长匹配）
    fn get_matching_mounts<'a>(&self, path: &str, mounts: &'a [MountInfo]) -> Vec<&'a MountInfo> {
        let path = fix_and_clean_path(path);
        let mut best_len = 0;
        
        for mount in mounts {
            let mount_path = fix_and_clean_path(&mount.mount_path);
            if is_sub_path(&mount_path, &path) && mount_path.len() > best_len {
                best_len = mount_path.len();
            }
        }
        
        let mut result: Vec<&MountInfo> = mounts.iter()
            .filter(|mount| {
                let mount_path = fix_and_clean_path(&mount.mount_path);
                is_sub_path(&mount_path, &path) && mount_path.len() == best_len
            })
            .collect();
        
        result.sort_by_key(|m| m.order);
        result
    }

    /// 获取虚拟目录列表（没有匹配驱动时显示子挂载点）
    fn get_virtual_dirs(&self, prefix: &str, mounts: &[MountInfo]) -> Vec<WebDavDirEntry> {
        let prefix = fix_and_clean_path(prefix);
        let mut seen: HashMap<String, bool> = HashMap::new();
        let mut dirs = Vec::new();
        
        for mount in mounts {
            let mount_path = fix_and_clean_path(&mount.mount_path);
            
            if prefix.len() >= mount_path.len() || !is_sub_path(&prefix, &mount_path) {
                continue;
            }
            
            let relative = mount_path.strip_prefix(&prefix).unwrap_or(&mount_path);
            let relative = relative.trim_start_matches('/');
            let name = relative.split('/').next().unwrap_or(relative);
            
            if name.is_empty() || seen.contains_key(name) {
                continue;
            }
            
            seen.insert(name.to_string(), true);
            dirs.push(WebDavDirEntry {
                name: name.to_string(),
                metadata: WebDavMetaData {
                    is_dir: true,
                    size: 0,
                    modified: Some(SystemTime::now()),
                    created: None,
                },
            });
        }
        
        dirs
    }

    /// 获取最近的元信息（向上查找父目录）
    async fn get_nearest_meta(&self, path: &str) -> Option<Meta> {
        let path = fix_and_clean_path(path);
        
        // 先尝试精确匹配
        if let Ok(Some(meta)) = sqlx::query_as::<_, Meta>(
            "SELECT id, path, password, p_sub, write, w_sub, hide, h_sub, readme, r_sub, header, header_sub, created_at, updated_at FROM metas WHERE path = ?"
        )
        .bind(&path)
        .fetch_optional(&self.db)
        .await {
            return Some(meta);
        }
        
        // 递归向上查找父目录
        if path == "/" {
            return None;
        }
        
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let parent = if parent.is_empty() { "/".to_string() } else { parent };
        
        Box::pin(self.get_nearest_meta(&parent)).await
    }

    /// 获取隐藏规则
    async fn get_hide_patterns(&self, path: &str) -> String {
        let meta = self.get_nearest_meta(path).await;
        meta.as_ref()
            .filter(|m| is_hide_apply(&m.path, path, m.h_sub))
            .and_then(|m| m.hide.clone())
            .unwrap_or_default()
    }

    /// 检查用户是否有显示隐藏文件的权限
    async fn can_show_hidden(&self) -> bool {
        let user = self.user.read().await;
        user.as_ref()
            .map(|u| u.permissions.is_admin)  // 管理员可以看隐藏文件
            .unwrap_or(false)
    }

    /// 检查读取权限
    async fn check_read(&self) -> FsResult<()> {
        let user = self.user.read().await;
        match user.as_ref() {
            Some(u) if u.permissions.can_read => Ok(()),
            Some(_) => Err(FsError::Forbidden),
            None => Err(FsError::Forbidden),
        }
    }

    /// 检查写入权限
    async fn check_write(&self) -> FsResult<()> {
        let user = self.user.read().await;
        match user.as_ref() {
            Some(u) if u.permissions.can_write => Ok(()),
            Some(_) => Err(FsError::Forbidden),
            None => Err(FsError::Forbidden),
        }
    }

    /// 检查删除权限
    async fn check_delete(&self) -> FsResult<()> {
        let user = self.user.read().await;
        match user.as_ref() {
            Some(u) if u.permissions.can_delete => Ok(()),
            Some(_) => Err(FsError::Forbidden),
            None => Err(FsError::Forbidden),
        }
    }
}

/// WebDAV文件元数据
#[derive(Debug, Clone)]
pub struct WebDavMetaData {
    is_dir: bool,
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

impl DavMetaData for WebDavMetaData {
    fn len(&self) -> u64 {
        self.size
    }

    fn is_dir(&self) -> bool {
        self.is_dir
    }

    fn modified(&self) -> FsResult<SystemTime> {
        self.modified.ok_or(FsError::GeneralFailure)
    }

    fn created(&self) -> FsResult<SystemTime> {
        self.created.ok_or(FsError::GeneralFailure)
    }
}

impl From<&Entry> for WebDavMetaData {
    fn from(entry: &Entry) -> Self {
        let modified = entry.modified.as_ref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64))
        });
        Self {
            is_dir: entry.is_dir,
            size: entry.size,
            modified,
            created: None,
        }
    }
}

/// WebDAV目录条目
#[derive(Debug, Clone)]
pub struct WebDavDirEntry {
    name: String,
    metadata: WebDavMetaData,
}

impl DavDirEntry for WebDavDirEntry {
    fn name(&self) -> Vec<u8> {
        self.name.as_bytes().to_vec()
    }

    fn metadata(&self) -> FsFuture<Box<dyn DavMetaData>> {
        let meta = self.metadata.clone();
        Box::pin(async move { Ok(Box::new(meta) as Box<dyn DavMetaData>) })
    }
}

/// WebDAV文件句柄
/// 注意：DavFile要求Sync，但AsyncRead/AsyncWrite通常不是Sync
/// 使用StorageDriver的open_reader原语实现文件读取
pub struct WebDavFile {
    /// 驱动ID
    driver_id: String,
    /// 相对于驱动的路径
    driver_path: String,
    /// StorageManager引用
    storage_manager: StorageManager,
    /// 当前读取位置
    position: Arc<tokio::sync::Mutex<u64>>,
    /// 文件大小
    size: u64,
}

impl Debug for WebDavFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavFile")
            .field("driver_id", &self.driver_id)
            .field("driver_path", &self.driver_path)
            .field("size", &self.size)
            .finish()
    }
}

impl DavFile for WebDavFile {
    fn metadata(&mut self) -> FsFuture<Box<dyn DavMetaData>> {
        let size = self.size;
        Box::pin(async move {
            Ok(Box::new(WebDavMetaData {
                is_dir: false,
                size,
                modified: Some(SystemTime::now()),
                created: None,
            }) as Box<dyn DavMetaData>)
        })
    }

    fn write_buf(&mut self, _buf: Box<dyn bytes::Buf + Send>) -> FsFuture<()> {
        Box::pin(async move {
            // TODO: 实现写入
            Err(FsError::NotImplemented)
        })
    }

    fn write_bytes(&mut self, _buf: Bytes) -> FsFuture<()> {
        Box::pin(async move {
            // TODO: 实现写入
            Err(FsError::NotImplemented)
        })
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<Bytes> {
        use tokio::io::AsyncReadExt;
        
        let driver_id = self.driver_id.clone();
        let driver_path = self.driver_path.clone();
        let storage_manager = self.storage_manager.clone();
        let position = self.position.clone();
        let size = self.size;
        
        Box::pin(async move {
            let mut pos = position.lock().await;
            
            // 如果已经读完，返回空
            if *pos >= size {
                return Ok(Bytes::new());
            }
            
            // 计算实际读取的字节数
            let remaining = size - *pos;
            let to_read = std::cmp::min(count as u64, remaining) as usize;
            
            // 使用StorageDriver的open_reader原语读取文件
            let driver = storage_manager.get_driver(&driver_id).await
                .ok_or(FsError::NotFound)?;
            
            // 使用范围读取
            let range = Some(*pos..(*pos + to_read as u64));
            let mut reader = driver.open_reader(&driver_path, range).await
                .map_err(|e| {
                    tracing::error!("WebDAV failed to read file: {}", e);
                    FsError::GeneralFailure
                })?;
            
            // 读取数据
            let mut buf = vec![0u8; to_read];
            let n = reader.read(&mut buf).await
                .map_err(|_| FsError::GeneralFailure)?;
            
            buf.truncate(n);
            *pos += n as u64;
            
            Ok(Bytes::from(buf))
        })
    }

    fn seek(&mut self, pos: SeekFrom) -> FsFuture<u64> {
        let position = self.position.clone();
        let size = self.size;
        Box::pin(async move {
            let mut current_pos = position.lock().await;
            let new_pos = match pos {
                SeekFrom::Start(n) => n,
                SeekFrom::End(n) => (size as i64 + n) as u64,
                SeekFrom::Current(n) => (*current_pos as i64 + n) as u64,
            };
            *current_pos = new_pos;
            Ok(new_pos)
        })
    }

    fn flush(&mut self) -> FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }
}

impl DavFileSystem for WebDavFs {
    fn open<'a>(&'a self, path: &'a DavPath, _options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        let path_clone = path.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查读取权限
            fs.check_read().await?;
            
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            // 使用as_pathbuf()获取解码后的路径
            let req_path = path_clone.as_pathbuf().to_string_lossy().to_string();
            let req_path = fix_and_clean_path(&req_path);
            let storage_path = match join_user_path(&root, &req_path) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 获取所有挂载点
            let mounts = fs.get_all_mounts().await;
            
            // 查找匹配的驱动
            let matching_mounts = fs.get_matching_mounts(&storage_path, &mounts);
            
            if matching_mounts.is_empty() {
                return Err(FsError::NotFound);
            }
            
            // 使用第一个匹配的挂载点
            let mount = &matching_mounts[0];
            let mount_path = fix_and_clean_path(&mount.mount_path);
            
            // 计算相对于驱动的路径
            let driver_path = if storage_path.len() > mount_path.len() {
                fix_and_clean_path(&storage_path[mount_path.len()..])
            } else {
                "/".to_string()
            };
            
            // 获取文件元信息以获取大小
            let mut size = 0u64;
            if let Some(driver) = fs.storage_manager.get_driver(&mount.id).await {
                if let Ok(entries) = driver.list(&fix_and_clean_path(
                    std::path::Path::new(&driver_path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string())
                        .as_str()
                )).await {
                    let filename = std::path::Path::new(&driver_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    for entry in entries {
                        if entry.name == filename {
                            size = entry.size;
                            break;
                        }
                    }
                }
            }

            let file = WebDavFile {
                driver_id: mount.id.clone(),
                driver_path,
                storage_manager: fs.storage_manager.clone(),
                position: Arc::new(tokio::sync::Mutex::new(0)),
                size,
            };

            Ok(Box::new(file) as Box<dyn DavFile>)
        })
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _meta: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        let path_clone = path.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查读取权限
            fs.check_read().await?;

            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            // 使用as_pathbuf()获取解码后的路径，而不是as_url_string()
            let req_path = path_clone.as_pathbuf().to_string_lossy().to_string();
            let req_path = fix_and_clean_path(&req_path);
            tracing::debug!("WebDAV read_dir: root={}, req_path={}", root, req_path);
            
            let storage_path = match join_user_path(&root, &req_path) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };
            tracing::debug!("WebDAV read_dir: storage_path={}", storage_path);

            // 获取隐藏规则（元信息隐藏在WebDAV生效，密码不生效）
            let hide_patterns = fs.get_hide_patterns(&storage_path).await;
            let can_show_hidden = fs.can_show_hidden().await;

            // 获取所有挂载点
            let mounts = fs.get_all_mounts().await;
            
            // 查找匹配的驱动
            let matching_mounts = fs.get_matching_mounts(&storage_path, &mounts);
            
            let mut all_entries: HashMap<String, WebDavDirEntry> = HashMap::new();
            
            if !matching_mounts.is_empty() {
                // 计算相对于挂载点的实际路径
                let mount_path = fix_and_clean_path(&matching_mounts[0].mount_path);
                let actual_path = if storage_path.len() > mount_path.len() {
                    fix_and_clean_path(&storage_path[mount_path.len()..])
                } else {
                    "/".to_string()
                };
                
                // 从所有匹配的驱动获取文件列表
                for mount in &matching_mounts {
                    if let Some(driver) = fs.storage_manager.get_driver(&mount.id).await {
                        if let Ok(entries) = driver.list(&actual_path).await {
                            for e in entries {
                                // 应用隐藏规则过滤
                                if !can_show_hidden && should_hide_file(&e.name, &hide_patterns) {
                                    continue;
                                }
                                all_entries.entry(e.name.clone()).or_insert(WebDavDirEntry {
                                    name: e.name.clone(),
                                    metadata: WebDavMetaData::from(&e),
                                });
                            }
                        }
                    }
                }
            }
            
            // 合并虚拟目录（子挂载点），同样应用隐藏规则
            let virtual_dirs = fs.get_virtual_dirs(&storage_path, &mounts);
            for vd in virtual_dirs {
                if !can_show_hidden && should_hide_file(&vd.name, &hide_patterns) {
                    continue;
                }
                all_entries.entry(vd.name.clone()).or_insert(vd);
            }
            
            // 如果没有任何内容且不是根目录，返回404
            if all_entries.is_empty() && storage_path != "/" && storage_path != root {
                return Err(FsError::NotFound);
            }
            
            let dir_entries: Vec<Box<dyn DavDirEntry>> = all_entries
                .into_values()
                .map(|e| Box::new(e) as Box<dyn DavDirEntry>)
                .collect();
            
            Ok(Box::pin(stream::iter(dir_entries.into_iter().map(Ok)))
                as FsStream<Box<dyn DavDirEntry>>)
        })
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        let path_clone = path.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            // 使用as_pathbuf()获取解码后的路径
            let req_path = path_clone.as_pathbuf().to_string_lossy().to_string();
            let req_path = fix_and_clean_path(&req_path);
            let storage_path = match join_user_path(&root, &req_path) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 根路径返回目录元数据
            if storage_path == "/" || storage_path == root {
                return Ok(Box::new(WebDavMetaData {
                    is_dir: true,
                    size: 0,
                    modified: Some(SystemTime::now()),
                    created: None,
                }) as Box<dyn DavMetaData>);
            }

            // 获取挂载点
            let mounts = fs.get_all_mounts().await;
            let _matching_mounts = fs.get_matching_mounts(&storage_path, &mounts);
            
            // 检查是否是虚拟目录
            let virtual_dirs = fs.get_virtual_dirs(&storage_path, &mounts);
            if !virtual_dirs.is_empty() {
                // 这是一个虚拟目录
                return Ok(Box::new(WebDavMetaData {
                    is_dir: true,
                    size: 0,
                    modified: Some(SystemTime::now()),
                    created: None,
                }) as Box<dyn DavMetaData>);
            }

            // 查找文件元数据
            let parent = fix_and_clean_path(
                std::path::Path::new(&storage_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
                    .as_str()
            );
            let name = std::path::Path::new(&storage_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let parent_mounts = fs.get_matching_mounts(&parent, &mounts);
            
            if parent_mounts.is_empty() {
                // 检查是否是虚拟目录中的项
                let parent_virtuals = fs.get_virtual_dirs(&parent, &mounts);
                for vd in parent_virtuals {
                    if vd.name == name {
                        return Ok(Box::new(vd.metadata) as Box<dyn DavMetaData>);
                    }
                }
                return Err(FsError::NotFound);
            }

            // 计算相对于挂载点的路径
            let mount_path = fix_and_clean_path(&parent_mounts[0].mount_path);
            let actual_parent = if parent.len() > mount_path.len() {
                fix_and_clean_path(&parent[mount_path.len()..])
            } else {
                "/".to_string()
            };

            // 从驱动获取文件列表查找目标文件
            for mount in &parent_mounts {
                if let Some(driver) = fs.storage_manager.get_driver(&mount.id).await {
                    if let Ok(entries) = driver.list(&actual_parent).await {
                        for entry in entries {
                            if entry.name == name {
                                return Ok(Box::new(WebDavMetaData::from(&entry))
                                    as Box<dyn DavMetaData>);
                            }
                        }
                    }
                }
            }
            
            Err(FsError::NotFound)
        })
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let path_clone = path.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查写入权限
            fs.check_write().await?;
            
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            let req_path = path_clone.as_pathbuf().to_string_lossy().to_string();
            let req_path = fix_and_clean_path(&req_path);
            let storage_path = match join_user_path(&root, &req_path) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 获取挂载点
            let mounts = fs.get_all_mounts().await;
            let matching_mounts = fs.get_matching_mounts(&storage_path, &mounts);
            
            if matching_mounts.is_empty() {
                return Err(FsError::NotFound);
            }
            
            // 计算相对于挂载点的路径
            let mount_path = fix_and_clean_path(&matching_mounts[0].mount_path);
            let actual_path = if storage_path.len() > mount_path.len() {
                fix_and_clean_path(&storage_path[mount_path.len()..])
            } else {
                "/".to_string()
            };

            // 使用第一个匹配的驱动创建目录
            if let Some(driver) = fs.storage_manager.get_driver(&matching_mounts[0].id).await {
                driver.create_dir(&actual_path).await.map_err(|_| FsError::GeneralFailure)
            } else {
                Err(FsError::NotFound)
            }
        })
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        let path_clone = path.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查删除权限
            fs.check_delete().await?;
            
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            let req_path = path_clone.as_pathbuf().to_string_lossy().to_string();
            let req_path = fix_and_clean_path(&req_path);
            let storage_path = match join_user_path(&root, &req_path) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 获取挂载点
            let mounts = fs.get_all_mounts().await;
            let matching_mounts = fs.get_matching_mounts(&storage_path, &mounts);
            
            if matching_mounts.is_empty() {
                return Err(FsError::NotFound);
            }
            
            // 计算相对于挂载点的路径
            let mount_path = fix_and_clean_path(&matching_mounts[0].mount_path);
            let actual_path = if storage_path.len() > mount_path.len() {
                fix_and_clean_path(&storage_path[mount_path.len()..])
            } else {
                "/".to_string()
            };

            // 使用第一个匹配的驱动删除
            if let Some(driver) = fs.storage_manager.get_driver(&matching_mounts[0].id).await {
                driver.delete(&actual_path).await.map_err(|_| FsError::GeneralFailure)
            } else {
                Err(FsError::NotFound)
            }
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<()> {
        self.remove_file(path)
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let from_clone = from.clone();
        let to_clone = to.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查权限
            {
                let user = fs.user.read().await;
                let can_op = user.as_ref()
                    .map(|u| u.permissions.can_rename || u.permissions.can_move)
                    .unwrap_or(false);
                if !can_op {
                    return Err(FsError::Forbidden);
                }
            }
            
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            let from_req = from_clone.as_pathbuf().to_string_lossy().to_string();
            let from_req = fix_and_clean_path(&from_req);
            let from_path = match join_user_path(&root, &from_req) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };
            let to_req = to_clone.as_pathbuf().to_string_lossy().to_string();
            let to_req = fix_and_clean_path(&to_req);
            let to_path = match join_user_path(&root, &to_req) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 获取挂载点
            let mounts = fs.get_all_mounts().await;
            let from_mounts = fs.get_matching_mounts(&from_path, &mounts);
            
            if from_mounts.is_empty() {
                return Err(FsError::NotFound);
            }
            
            // 计算相对于挂载点的路径
            let mount_path = fix_and_clean_path(&from_mounts[0].mount_path);
            let actual_from = if from_path.len() > mount_path.len() {
                fix_and_clean_path(&from_path[mount_path.len()..])
            } else {
                "/".to_string()
            };

            // 检查是重命名还是移动
            let from_parent = fix_and_clean_path(
                std::path::Path::new(&from_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
                    .as_str()
            );
            let to_parent = fix_and_clean_path(
                std::path::Path::new(&to_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
                    .as_str()
            );

            if let Some(driver) = fs.storage_manager.get_driver(&from_mounts[0].id).await {
                if from_parent == to_parent {
                    // 重命名
                    let new_name = std::path::Path::new(&to_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    driver.rename(&actual_from, &new_name).await.map_err(|_| FsError::GeneralFailure)
                } else {
                    // 移动
                    let actual_to = if to_path.len() > mount_path.len() {
                        fix_and_clean_path(&to_path[mount_path.len()..])
                    } else {
                        "/".to_string()
                    };
                    driver.move_item(&actual_from, &actual_to).await.map_err(|_| FsError::GeneralFailure)
                }
            } else {
                Err(FsError::NotFound)
            }
        })
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<()> {
        let from_clone = from.clone();
        let to_clone = to.clone();
        let fs = self.clone();

        Box::pin(async move {
            // 检查复制权限
            {
                let user = fs.user.read().await;
                let can_copy = user.as_ref()
                    .map(|u| u.permissions.can_copy)
                    .unwrap_or(false);
                if !can_copy {
                    return Err(FsError::Forbidden);
                }
            }
            
            // 获取用户根路径
            let root = fs.get_root_path().await;
            
            // 将请求路径与用户根路径结合
            let from_req = from_clone.as_pathbuf().to_string_lossy().to_string();
            let from_req = fix_and_clean_path(&from_req);
            let from_path = match join_user_path(&root, &from_req) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };
            let to_req = to_clone.as_pathbuf().to_string_lossy().to_string();
            let to_req = fix_and_clean_path(&to_req);
            let to_path = match join_user_path(&root, &to_req) {
                Ok(p) => p,
                Err(_) => return Err(FsError::Forbidden),
            };

            // 获取挂载点
            let mounts = fs.get_all_mounts().await;
            let from_mounts = fs.get_matching_mounts(&from_path, &mounts);
            
            if from_mounts.is_empty() {
                return Err(FsError::NotFound);
            }
            
            // 计算相对于挂载点的路径
            let mount_path = fix_and_clean_path(&from_mounts[0].mount_path);
            let actual_from = if from_path.len() > mount_path.len() {
                fix_and_clean_path(&from_path[mount_path.len()..])
            } else {
                "/".to_string()
            };
            let actual_to = if to_path.len() > mount_path.len() {
                fix_and_clean_path(&to_path[mount_path.len()..])
            } else {
                "/".to_string()
            };

            if let Some(driver) = fs.storage_manager.get_driver(&from_mounts[0].id).await {
                driver.copy_item(&actual_from, &actual_to).await.map_err(|_| FsError::GeneralFailure)
            } else {
                Err(FsError::NotFound)
            }
        })
    }
}

/// WebDAV服务器
/// 使用与前端相同的逻辑：通过挂载点解析驱动，支持所有驱动类型
pub struct WebDavServer {
    config: WebDavConfig,
    storage_manager: StorageManager,
}

impl WebDavServer {
    pub fn new(config: WebDavConfig, storage_manager: StorageManager) -> Self {
        Self {
            config,
            storage_manager,
        }
    }

    /// 获取配置
    pub fn config(&self) -> &WebDavConfig {
        &self.config
    }

    /// 启动WebDAV服务器（独立hyper服务器）
    /// 支持所有驱动类型，与前端逻辑一致
    pub async fn start(&self, db: SqlitePool) -> anyhow::Result<()> {
        if !self.config.enabled {
            tracing::info!("WebDAV服务器未启用");
            return Ok(());
        }

        let addr: SocketAddr = self.config.listen.parse()?;
        let listener = TcpListener::bind(addr).await?;
        
        tracing::info!("WebDAV服务器启动于 {} (支持所有驱动类型)", addr);

        let storage_manager = self.storage_manager.clone();
        let prefix = self.config.prefix.clone();

        loop {
            let (stream, remote_addr) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let storage = storage_manager.clone();
            let db_clone = db.clone();
            let prefix_clone = prefix.clone();

            tokio::spawn(async move {
                let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                    let storage = storage.clone();
                    let db = db_clone.clone();
                    let prefix = prefix_clone.clone();
                    
                    async move {
                        // Basic Auth认证
                        let auth_header = req.headers().get("Authorization");
                        let user = if let Some(auth) = auth_header {
                            if let Ok(auth_str) = auth.to_str() {
                                if auth_str.starts_with("Basic ") {
                                    let encoded = &auth_str[6..];
                                    if let Ok(decoded) = base64::Engine::decode(
                                        &base64::engine::general_purpose::STANDARD,
                                        encoded,
                                    ) {
                                        if let Ok(creds) = String::from_utf8(decoded) {
                                            if let Some((username, password)) = creds.split_once(':') {
                                                let authenticator = UserAuthenticator::new(db.clone());
                                                authenticator.authenticate_webdav(username, password).await
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        // 未认证返回401
                        let user = match user {
                            Some(u) => u,
                            None => {
                                let mut resp = hyper::Response::new(dav_server::body::Body::from("Unauthorized"));
                                *resp.status_mut() = StatusCode::UNAUTHORIZED;
                                resp.headers_mut().insert(
                                    "WWW-Authenticate",
                                    "Basic realm=\"YaoList WebDAV\"".parse().unwrap(),
                                );
                                return Ok::<_, Infallible>(resp);
                            }
                        };

                        // 创建带用户的文件系统（使用数据库查询挂载点，支持所有驱动）
                        let fs = WebDavFs::with_user(storage, db, user);
                        let handler = dav_server::DavHandler::builder()
                            .filesystem(Box::new(fs))
                            .locksystem(dav_server::fakels::FakeLs::new())
                            .strip_prefix(&prefix)
                            .build_handler();

                        // 处理请求
                        Ok(handler.handle(req).await)
                    }
                });

                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                {
                    tracing::error!("WebDAV connection error {}: {:?}", remote_addr, err);
                }
            });
        }
    }
}

/// 创建WebDAV服务器实例
pub fn create_webdav_server(config: WebDavConfig, storage_manager: StorageManager) -> WebDavServer {
    WebDavServer::new(config, storage_manager)
}
