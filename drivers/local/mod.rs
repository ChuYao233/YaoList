use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use chrono::{DateTime, Local};

use super::{Driver, FileInfo, DriverFactory, DriverInfo};

pub struct LocalDriver {
    pub root: PathBuf,
}

#[async_trait]
impl Driver for LocalDriver {
    async fn move_file(&self, _file_path: &str, _new_parent_path: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("本地驱动不支持移动操作"))
    }

    async fn copy_file(&self, _file_path: &str, _new_parent_path: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("本地驱动不支持复制操作"))
    }

    async fn list(&self, path: &str) -> anyhow::Result<Vec<FileInfo>> {
        let full_path = self.root.join(path);
        let mut entries = fs::read_dir(full_path).await?;
        let mut files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = metadata.is_dir();
            let size = if is_dir { 0 } else { metadata.len() };
            let modified = metadata.modified()
                .map(|time| {
                    let datetime: DateTime<Local> = time.into();
                    datetime.to_rfc3339()
                })
                .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339());

            let file_path = if path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", path, name)
            };

            files.push(FileInfo {
                name,
                path: file_path,
                size,
                is_dir,
                modified,
            });
        }

        Ok(files)
    }

    async fn download(&self, path: &str) -> anyhow::Result<tokio::fs::File> {
        let full_path = self.root.join(path);
        let file = tokio::fs::File::open(full_path).await?;
        Ok(file)
    }

    async fn get_download_url(&self, _path: &str) -> anyhow::Result<Option<String>> {
        // 本地驱动不需要特殊的下载URL
        Ok(None)
    }

    async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> anyhow::Result<()> {
        let dir_path = if parent_path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(parent_path)
        };
        
        tokio::fs::create_dir_all(&dir_path).await?;
        let file_path = dir_path.join(file_name);
        tokio::fs::write(&file_path, content).await?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let full_path = self.root.join(path);
        if full_path.exists() {
            if full_path.is_dir() {
                std::fs::remove_dir_all(full_path)?;
            } else {
                std::fs::remove_file(full_path)?;
            }
        }
        Ok(())
    }

    async fn rename(&self, old_path: &str, new_name: &str) -> anyhow::Result<()> {
        let old_full_path = self.root.join(old_path);
        let parent = old_full_path.parent()
            .ok_or_else(|| anyhow::anyhow!("Cannot get parent directory"))?;
        let new_full_path = parent.join(new_name);
        
        if old_full_path.exists() {
            std::fs::rename(old_full_path, new_full_path)?;
        }
        Ok(())
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> anyhow::Result<()> {
        let folder_path = if parent_path.is_empty() {
            self.root.join(folder_name)
        } else {
            self.root.join(parent_path).join(folder_name)
        };
        
        println!("📁 本地驱动创建文件夹: {:?}", folder_path);
        
        std::fs::create_dir_all(&folder_path)?;
        
        // 验证文件夹是否创建成功
        if folder_path.exists() {
            println!("✅ 文件夹创建成功: {:?}", folder_path);
        } else {
            println!("❌ 文件夹创建失败: {:?}", folder_path);
        }
        
        Ok(())
    }

    async fn get_file_info(&self, path: &str) -> anyhow::Result<FileInfo> {
        let full_path = self.root.join(path);
        let metadata = fs::metadata(&full_path).await?;
        
        let name = full_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };
        let modified = metadata.modified()
            .map(|time| {
                let datetime: DateTime<Local> = time.into();
                datetime.to_rfc3339()
            })
            .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339());

        Ok(FileInfo {
            name,
            path: path.to_string(),
            size,
            is_dir,
            modified,
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    // 本地驱动支持流式下载
    async fn stream_download(&self, path: &str) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        let full_path = self.root.join(path);
        let filename = full_path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("download"))
            .to_string_lossy()
            .to_string();
        
        println!("🌊 本地流式下载: {:?}", full_path);
        
        // 创建异步流
        let stream = async_stream::stream! {
            use tokio::io::AsyncReadExt;
            
            match tokio::fs::File::open(&full_path).await {
                Ok(mut file) => {
                    let mut buffer = [0u8; 8192]; // 8KB 缓冲区
                    let mut total_bytes = 0u64;
                    
                    println!("🚀 开始本地流式传输");
                    
                    loop {
                        match file.read(&mut buffer).await {
                            Ok(0) => {
                                println!("✅ 本地流式传输完成，共 {} 字节 ({} MB)", 
                                    total_bytes, total_bytes / 1024 / 1024);
                                break;
                            },
                            Ok(n) => {
                                total_bytes += n as u64;
                                // 每10MB输出一次进度
                                if total_bytes % (10 * 1024 * 1024) == 0 {
                                    println!("📊 本地流式传输进度: {} MB", total_bytes / 1024 / 1024);
                                }
                                yield Ok(axum::body::Bytes::copy_from_slice(&buffer[..n]));
                            },
                            Err(e) => {
                                println!("❌ 本地流式传输错误: {}", e);
                                yield Err(e);
                                break;
            }
                        }
                    }
                },
            Err(e) => {
                    println!("❌ 打开本地文件失败: {}", e);
                    yield Err(e);
                }
            }
        };
        
        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));
        
        Ok(Some((boxed_stream, filename)))
    }
    
    // 本地驱动支持 Range 流式下载
    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> anyhow::Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        let full_path = self.root.join(path);
        let filename = full_path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("download"))
            .to_string_lossy()
            .to_string();
        
        // 获取文件大小
        let metadata = tokio::fs::metadata(&full_path).await?;
        let file_size = metadata.len();
        
        // 计算实际的开始和结束位置
        let actual_start = start.unwrap_or(0);
        let actual_end = end.unwrap_or(file_size - 1).min(file_size - 1);
        
        if actual_start >= file_size {
            return Err(anyhow::anyhow!("Range 起始位置超出文件大小"));
        }
        
        let content_length = actual_end - actual_start + 1;
        
        println!("🎯 本地 Range 下载: {:?} ({}:{}) 文件大小: {}", 
            full_path, actual_start, actual_end, file_size);
        
        // 创建异步流，支持 Range 请求
        let stream = async_stream::stream! {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            
            match tokio::fs::File::open(&full_path).await {
                Ok(mut file) => {
                    // 定位到起始位置
                    if let Err(e) = file.seek(std::io::SeekFrom::Start(actual_start)).await {
                        println!("❌ 本地文件定位失败: {}", e);
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        return;
                    }
                    
                    let mut buffer = [0u8; 8192]; // 8KB 缓冲区
                    let mut bytes_read = 0u64;
                    let target_bytes = content_length;
                    
                    println!("🚀 开始本地 Range 传输: {} 字节", target_bytes);
                    
                    while bytes_read < target_bytes {
                        let remaining = target_bytes - bytes_read;
                        let to_read = (buffer.len() as u64).min(remaining) as usize;
                        
                        match file.read(&mut buffer[..to_read]).await {
                            Ok(0) => {
                                println!("⚠️ 本地文件提前结束，已读取 {} / {} 字节", bytes_read, target_bytes);
                                break;
                            },
                            Ok(n) => {
                                bytes_read += n as u64;
                                // 每10MB输出一次进度
                                if bytes_read % (10 * 1024 * 1024) == 0 {
                                    println!("📊 本地 Range 传输进度: {} / {} MB", 
                                        bytes_read / 1024 / 1024, target_bytes / 1024 / 1024);
                                }
                                yield Ok(axum::body::Bytes::copy_from_slice(&buffer[..n]));
                            },
                            Err(e) => {
                                println!("❌ 本地 Range 传输错误: {}", e);
                                yield Err(e);
                                break;
                            }
                        }
                    }
                    
                    println!("✅ 本地 Range 传输完成: {} / {} 字节", bytes_read, target_bytes);
                },
            Err(e) => {
                    println!("❌ 打开本地文件失败: {}", e);
                    yield Err(e);
                }
            }
        };
        
        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));
        
        Ok(Some((boxed_stream, filename, file_size, Some(content_length))))
    }
}

// 本地驱动工厂
pub struct LocalDriverFactory;

impl DriverFactory for LocalDriverFactory {
    fn driver_type(&self) -> &'static str {
        "local"
    }

    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "local".to_string(),
            display_name: "本地存储".to_string(),
            description: "本地文件系统存储".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "root": {
                        "type": "string",
                        "title": "本地路径",
                        "description": "存储文件的本地目录路径",
                        "placeholder": "E:/Storage"
                    }
                },
                "required": ["root"]
            }),
        }
    }

    fn create_driver(&self, config: serde_json::Value) -> anyhow::Result<Box<dyn Driver>> {
        let root = config.get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'root' in local driver config"))?;

        Ok(Box::new(LocalDriver {
            root: PathBuf::from(root),
        }))
    }

    fn get_routes(&self) -> Option<axum::Router> {
        // 本地驱动不需要额外的路由
        None
    }
}
