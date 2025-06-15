use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use s3::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use tokio::io::AsyncWriteExt;
use std::path::Path;
use uuid;

use crate::drivers::{Driver, FileInfo, DriverFactory, DriverInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    pub root: String,
    pub use_path_style: bool,
}

pub struct S3Driver {
    config: S3Config,
    bucket: Arc<Mutex<Option<Bucket>>>,
}

impl S3Driver {
    pub fn new(config: S3Config) -> anyhow::Result<Self> {
        Ok(Self {
            config,
            bucket: Arc::new(Mutex::new(None)),
        })
    }

    async fn get_bucket(&self) -> Result<Bucket> {
        let mut guard = self.bucket.lock().await;
        if let Some(ref bucket) = *guard {
            return Ok(bucket.clone());
        }
        let region = Region::Custom {
            region: self.config.region.clone(),
            endpoint: self.config.endpoint.clone(),
        };
        let credentials = Credentials::new(
            Some(&self.config.access_key),
            Some(&self.config.secret_key),
            None,
            None,
            None,
        )?;
        let mut bucket_box = Bucket::new(
            &self.config.bucket,
            region,
            credentials,
        )?;

        if self.config.use_path_style {
            bucket_box = bucket_box.with_path_style();
        }

        let bucket: Bucket = *bucket_box;
        
        *guard = Some(bucket.clone());
        Ok(bucket)
    }

    fn full_path(&self, path: &str) -> String {
        let mut key = String::new();
        
        if !self.config.root.is_empty() {
            key.push_str(self.config.root.trim_matches('/'));
            if !path.is_empty() {
                key.push('/');
            }
        }
        
        let p = path.trim_matches('/');
        if !p.is_empty() {
            key.push_str(p);
        }
        
        if path.ends_with('/') && !key.is_empty() {
            key.push('/');
        }
        
        key
    }

    // 实现复制功能
    async fn copy_object(&self, bucket: &Bucket, from: &str, to: &str) -> Result<()> {
        // 先获取源文件内容
        let resp = bucket.get_object(from).await?;
        let data = resp.bytes().to_vec();
        
        // 再上传到新位置
        bucket.put_object(to, &data).await?;
        Ok(())
    }
}

#[async_trait]
impl Driver for S3Driver {
    async fn list(&self, path: &str) -> anyhow::Result<Vec<FileInfo>> {
        let bucket = self.get_bucket().await?;
        let prefix = self.full_path(path);
        let results = bucket.list(prefix.clone(), Some("/".to_string())).await?;
        let mut files = Vec::new();
        for res in results {
            // 处理目录
            for prefix_str in res.common_prefixes.into_iter().flatten() {
                let name = if prefix.is_empty() {
                    prefix_str.prefix.trim_end_matches('/')
                } else {
                    prefix_str.prefix.strip_prefix(&prefix)
                        .unwrap_or(&prefix_str.prefix)
                        .trim_end_matches('/')
                };
                let name = name.trim_start_matches('/').to_string();
                if name.is_empty() { continue; }
                files.push(FileInfo {
                    name: name.clone(),
                    path: format!("{}/{}", path.trim_end_matches('/'), name).replace("//", "/"),
                    size: 0,
                    is_dir: true,
                    modified: chrono::Utc::now().to_rfc3339(),
                });
            }
            // 处理文件
            for obj in res.contents {
                // 过滤掉目录本身
                if obj.key == prefix { continue; }
                let name = if prefix.is_empty() {
                    obj.key.as_str()
                } else {
                    obj.key.strip_prefix(&prefix)
                        .unwrap_or(&obj.key)
                };
                let name = name.trim_start_matches('/');
                if name.is_empty() { continue; }
                let is_dir = name.ends_with('/');
                if is_dir { continue; } // 跳过目录标记文件
                files.push(FileInfo {
                    name: name.to_string(),
                    path: format!("{}/{}", path.trim_end_matches('/'), name).replace("//", "/"),
                    size: obj.size as u64,
                    is_dir: false,
                    modified: obj.last_modified,
                });
            }
        }
        Ok(files)
    }

    async fn download(&self, path: &str) -> anyhow::Result<tokio::fs::File> {
        let bucket = self.get_bucket().await?;
        let key = self.full_path(path);
        let resp = bucket.get_object(&key).await?;
        let data = resp.bytes().to_vec();
        
        // 在跨平台的临时目录下创建文件
        let mut tmp_path = std::env::temp_dir();
        tmp_path.push(format!("yaolist_s3_{}", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&data).await?;
        file.sync_all().await?;
        drop(file);
        Ok(tokio::fs::File::open(&tmp_path).await?)
    }

    async fn get_download_url(&self, _path: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let key = if parent_path.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file_name)
        };
        bucket.put_object(&key, content).await?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let key = self.full_path(path);
        bucket.delete_object(&key).await?;
        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let old_key = self.full_path(path);
        let parent = Path::new(path).parent().unwrap_or(Path::new("")).to_str().unwrap();
        let new_key = self.full_path(&format!("{}/{}", parent, new_name));
        
        // 复制文件
        self.copy_object(&bucket, &old_key, &new_key).await?;
        // 删除旧文件
        bucket.delete_object(&old_key).await?;
        Ok(())
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let key = if parent_path.is_empty() {
            format!("{}/", folder_name)
        } else {
            format!("{}/{}/", parent_path.trim_end_matches('/'), folder_name)
        };
        bucket.put_object(&key, &[]).await?;
        Ok(())
    }

    async fn get_file_info(&self, path: &str) -> anyhow::Result<FileInfo> {
        let bucket = self.get_bucket().await?;
        let key = self.full_path(path);
        let (resp, _) = bucket.head_object(&key).await?;
        
        Ok(FileInfo {
            name: Path::new(path).file_name().unwrap_or_default().to_str().unwrap_or("").to_string(),
            path: path.to_string(),
            size: resp.content_length.unwrap_or(0) as u64,
            is_dir: path.ends_with('/'),
            modified: resp.last_modified.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        })
    }

    async fn move_file(&self, file_path: &str, new_parent_path: &str) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let old_key = self.full_path(file_path);
        let file_name = Path::new(file_path).file_name().unwrap_or_default().to_str().unwrap();
        let new_key = self.full_path(&format!("{}/{}", new_parent_path.trim_end_matches('/'), file_name));
        
        // 复制文件
        self.copy_object(&bucket, &old_key, &new_key).await?;
        // 删除旧文件
        bucket.delete_object(&old_key).await?;
        Ok(())
    }

    async fn copy_file(&self, file_path: &str, new_parent_path: &str) -> anyhow::Result<()> {
        let bucket = self.get_bucket().await?;
        let old_key = self.full_path(file_path);
        let file_name = Path::new(file_path).file_name().unwrap_or_default().to_str().unwrap();
        let new_key = self.full_path(&format!("{}/{}", new_parent_path.trim_end_matches('/'), file_name));
        
        // 复制文件
        self.copy_object(&bucket, &old_key, &new_key).await?;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct S3DriverFactory;

#[async_trait]
impl DriverFactory for S3DriverFactory {
    fn driver_type(&self) -> &'static str {
        "s3"
    }

    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "s3".to_string(),
            display_name: "S3 Storage".to_string(),
            description: "S3 compatible storage driver".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "endpoint": {
                        "type": "string",
                        "title": "Endpoint",
                        "description": "S3 endpoint URL"
                    },
                    "region": {
                        "type": "string",
                        "title": "区域",
                        "description": "S3 region"
                    },
                    "access_key": {
                        "type": "string",
                        "title": "Access Key ID",
                        "description": "S3 access key"
                    },
                    "secret_key": {
                        "type": "string",
                        "title": "Access Key Secret",
                        "description": "S3 secret key"
                    },
                    "bucket": {
                        "type": "string",
                        "title": "Bucket",
                        "description": "S3 bucket name"
                    },
                    "root": {
                        "type": "string",
                        "title": "根目录",
                        "description": "Root path in bucket"
                    },
                    "use_path_style": {
                        "type": "boolean",
                        "title": "Use Path Style",
                        "description": "Use path style instead of virtual hosted style"
                    }
                },
                "required": ["endpoint", "region", "access_key", "secret_key", "bucket"]
            }),
        }
    }

    fn create_driver(&self, config: serde_json::Value) -> anyhow::Result<Box<dyn Driver>> {
        let config: S3Config = serde_json::from_value(config)?;
        Ok(Box::new(S3Driver::new(config)?))
    }

    fn get_routes(&self) -> Option<axum::Router> {
        None
    }
}