//! 115云盘驱动核心实现

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use std::ops::Range;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::io::AsyncRead;

use crate::storage::{
    StorageDriver, DriverFactory, Entry, Capability, SpaceInfo,
    ProgressCallback, ConfigItem,
};

use super::types::*;
use super::client::Pan115Client;
use super::writer::Pan115StreamWriter;

use std::sync::RwLock as StdRwLock;

pub struct Pan115Driver {
    client: Arc<RwLock<Pan115Client>>,
    config: Pan115Config,
    root_id: String,
    path_cache: Arc<RwLock<HashMap<String, String>>>,
    initialized: Arc<StdRwLock<bool>>,
}

impl Pan115Driver {
    pub fn new(config: Pan115Config) -> Result<Self> {
        let client = Pan115Client::new(&config.cookie, config.page_size)?;
        
        let root_id = if config.root_folder_id.is_empty() {
            "0".to_string()
        } else {
            config.root_folder_id.clone()
        };
        
        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            config,
            root_id,
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            initialized: Arc::new(StdRwLock::new(false)),
        })
    }
    
    async fn ensure_init(&self) -> Result<()> {
        {
            let init = self.initialized.read().unwrap();
            if *init {
                return Ok(());
            }
        }
        
        let mut client = self.client.write().await;
        client.init().await?;
        
        let mut init = self.initialized.write().unwrap();
        *init = true;
        
        Ok(())
    }
    
    async fn get_id_by_path(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Ok(self.root_id.clone());
        }
        
        {
            let cache = self.path_cache.read().await;
            if let Some(id) = cache.get(path) {
                return Ok(id.clone());
            }
        }
        
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = self.root_id.clone();
        let mut current_path = String::new();
        
        for part in parts {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);
            
            {
                let cache = self.path_cache.read().await;
                if let Some(id) = cache.get(&current_path) {
                    current_id = id.clone();
                    continue;
                }
            }
            
            let client = self.client.read().await;
            let files = client.list_files(&current_id).await?;
            drop(client);
            
            let mut found = false;
            for file in &files {
                if file.name == part {
                    current_id = file.get_id().to_string();
                    found = true;
                    
                    let mut cache = self.path_cache.write().await;
                    cache.insert(current_path.clone(), current_id.clone());
                    break;
                }
            }
            
            if !found {
                return Err(anyhow!("Path not found: {}", path));
            }
        }
        
        Ok(current_id)
    }
    
    fn file_to_entry(&self, file: &FileInfo) -> Entry {
        Entry {
            name: file.name.clone(),
            path: String::new(),
            is_dir: file.is_dir(),
            size: file.size as u64,
            modified: if file.modified_time.is_empty() { None } else { Some(file.modified_time.clone()) },
        }
    }
}

#[async_trait]
impl StorageDriver for Pan115Driver {
    fn name(&self) -> &str {
        "115 Cloud"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: true,
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: true,
            can_batch_operations: true,
            max_file_size: None,
            requires_full_file_for_upload: true, // 需要完整文件MD5进行秒传
        }
    }
    
    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        self.ensure_init().await?;
        let dir_id = self.get_id_by_path(path).await?;
        let client = self.client.read().await;
        let files = client.list_files(&dir_id).await?;
        drop(client);
        
        let entries: Vec<Entry> = files.iter().map(|f| {
            let mut entry = self.file_to_entry(f);
            let p = path.trim_end_matches('/');
            entry.path = if p.is_empty() {
                format!("/{}", f.name)
            } else {
                format!("{}/{}", p, f.name)
            };
            entry
        }).collect();
        
        Ok(entries)
    }
    
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        self.ensure_init().await?;
        let file_id = self.get_id_by_path(path).await?;
        let client = self.client.read().await;
        let file = client.get_file(&file_id).await?;
        let url = client.get_download_url(&file.pick_code, "").await?;
        drop(client);
        
        let http = reqwest::Client::new();
        let mut req = http.get(&url);
        
        if let Some(r) = range {
            req = req.header("Range", format!("bytes={}-{}", r.start, r.end.saturating_sub(1)));
        }
        
        let resp = req.send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 206 {
            return Err(anyhow!("Download failed: {}", resp.status()));
        }
        
        let stream = resp.bytes_stream();
        let reader = tokio_util::io::StreamReader::new(
            stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        );
        
        Ok(Box::new(reader))
    }
    
    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn tokio::io::AsyncWrite + Unpin + Send>> {
        self.ensure_init().await?;
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let file_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("Invalid path"))?;
        
        let parent_id = self.get_id_by_path(&parent_path).await?;
        
        let client = self.client.read().await;
        let user_id = client.user_id;
        let app_ver = client.app_ver.clone();
        let cookie = client.get_cookie().to_string();
        let _size_limit = client.get_size_limit();
        drop(client);
        
        let writer = Pan115StreamWriter::new(
            self.client.clone(),
            parent_id,
            file_name,
            size_hint.unwrap_or(0) as i64,
            user_id,
            app_ver,
            cookie,
            progress,
        )?;
        
        Ok(Box::new(writer))
    }
    
    async fn put(
        &self,
        path: &str,
        data: Bytes,
        progress: Option<ProgressCallback>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        
        let mut writer = self.open_writer(path, Some(data.len() as u64), progress).await?;
        writer.write_all(&data).await?;
        writer.shutdown().await?;
        
        Ok(())
    }
    
    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let file_id = self.get_id_by_path(path).await?;
        let client = self.client.read().await;
        client.delete(&file_id).await?;
        drop(client);
        
        let mut cache = self.path_cache.write().await;
        cache.remove(path.trim_matches('/'));
        
        Ok(())
    }
    
    async fn create_dir(&self, path: &str) -> Result<()> {
        self.ensure_init().await?;
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let dir_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("Invalid path"))?;
        
        let parent_id = self.get_id_by_path(&parent_path).await?;
        let client = self.client.read().await;
        let resp = client.mkdir(&parent_id, &dir_name).await?;
        drop(client);
        
        let new_id = if !resp.category_id.is_empty() {
            resp.category_id
        } else {
            resp.file_id
        };
        
        if !new_id.is_empty() {
            let mut cache = self.path_cache.write().await;
            cache.insert(path.trim_matches('/').to_string(), new_id);
        }
        
        Ok(())
    }
    
    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        self.ensure_init().await?;
        let file_id = self.get_id_by_path(path).await?;
        let client = self.client.read().await;
        client.rename(&file_id, new_name).await?;
        drop(client);
        
        let mut cache = self.path_cache.write().await;
        cache.remove(path.trim_matches('/'));
        
        Ok(())
    }
    
    async fn move_item(&self, src: &str, dst: &str) -> Result<()> {
        self.ensure_init().await?;
        let src_id = self.get_id_by_path(src).await?;
        let dst_parent = std::path::Path::new(dst)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let dst_id = self.get_id_by_path(&dst_parent).await?;
        
        let client = self.client.read().await;
        client.move_file(&src_id, &dst_id).await?;
        drop(client);
        
        let mut cache = self.path_cache.write().await;
        cache.remove(src.trim_matches('/'));
        
        Ok(())
    }
    
    async fn copy_item(&self, src: &str, dst: &str) -> Result<()> {
        self.ensure_init().await?;
        let src_id = self.get_id_by_path(src).await?;
        let dst_parent = std::path::Path::new(dst)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let dst_id = self.get_id_by_path(&dst_parent).await?;
        
        let client = self.client.read().await;
        client.copy_file(&src_id, &dst_id).await?;
        
        Ok(())
    }
    
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        self.ensure_init().await?;
        let file_id = self.get_id_by_path(path).await?;
        tracing::debug!("115 get_direct_link: path={}, file_id={}", path, file_id);
        
        let client = self.client.read().await;
        let file = client.get_file(&file_id).await?;
        tracing::debug!("115 get_direct_link: pick_code={}", file.pick_code);
        
        match client.get_download_url(&file.pick_code, "").await {
            Ok(url) => {
                tracing::debug!("115 get_direct_link: url={}", url);
                Ok(Some(url))
            }
            Err(e) => {
                tracing::error!("115 get_direct_link failed: {}", e);
                Err(e)
            }
        }
    }
    
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        self.ensure_init().await?;
        let client = self.client.read().await;
        let info = client.get_space_info().await?;
        
        Ok(Some(SpaceInfo {
            total: info.all_total.size as u64,
            used: info.all_use.size as u64,
            free: info.all_remain.size as u64,
        }))
    }
    
    fn get_local_path(&self, _path: &str) -> Option<std::path::PathBuf> {
        None
    }
}

use crate::storage::DriverConfig;

pub struct Pan115DriverFactory;

impl DriverFactory for Pan115DriverFactory {
    fn driver_type(&self) -> &'static str {
        "pan115"
    }
    
    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "115云盘".to_string(),
            local_sort: false,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("0".to_string()),
        }
    }
    
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("cookie", "string")
                .title("Cookie")
                .required()
                .help("从浏览器获取的Cookie，包含UID、CID、SEID"),
            ConfigItem::new("root_folder_id", "string")
                .title("根目录ID")
                .default("0")
                .help("根目录ID，默认为0（根目录）"),
            ConfigItem::new("page_size", "number")
                .title("分页大小")
                .default("1000")
                .help("每页文件数量"),
        ]
    }
    
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let pan_config: Pan115Config = serde_json::from_value(config)?;
        let driver = Pan115Driver::new(pan_config)?;
        Ok(Box::new(driver))
    }
}

use futures::{StreamExt, TryStreamExt};
