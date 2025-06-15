use suppaftp::{AsyncFtpStream, FtpError};
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use crate::drivers::{Driver, DriverFactory, DriverInfo, FileInfo};
use chrono::Utc;
use encoding_rs::Encoding;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;
use suppaftp::FtpStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpConfig {
    pub address: String,
    pub username: String,
    pub password: String,
    pub encoding: String,
    pub root_path: String,
}

pub struct FtpDriver {
    config: FtpConfig,
    connection: Arc<Mutex<Option<AsyncFtpStream>>>,
}

impl FtpDriver {
    pub fn new(config: FtpConfig) -> Self {
        Self {
            config,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_connection(&self) -> Result<()> {
        let mut conn_guard = self.connection.lock().await;
        
        // 检查现有连接是否有效
        if let Some(ref mut conn) = *conn_guard {
            if conn.pwd().await.is_ok() {
                return Ok(());
            }
        }

        // 创建新连接
        let mut ftp_stream = AsyncFtpStream::connect(&self.config.address).await
            .map_err(|e| anyhow!("FTP 连接失败: {}", e))?;

        ftp_stream.login(&self.config.username, &self.config.password).await
            .map_err(|e| anyhow!("FTP 登录失败: {}", e))?;

        *conn_guard = Some(ftp_stream);
        Ok(())
    }

    fn encode_path(&self, path: &str) -> String {
        if self.config.encoding.is_empty() || self.config.encoding.to_lowercase() == "utf-8" {
            return path.to_string();
        }

        if let Some(encoding) = Encoding::for_label(self.config.encoding.as_bytes()) {
            let (encoded, _, _) = encoding.encode(path);
            String::from_utf8_lossy(&encoded).to_string()
        } else {
            path.to_string()
        }
    }

    fn decode_name(&self, name: &str) -> String {
        if self.config.encoding.is_empty() || self.config.encoding.to_lowercase() == "utf-8" {
            return name.to_string();
        }

        if let Some(encoding) = Encoding::for_label(self.config.encoding.as_bytes()) {
            let (decoded, _, _) = encoding.decode(name.as_bytes());
            decoded.to_string()
        } else {
            name.to_string()
        }
    }

    fn get_full_path(&self, path: &str) -> String {
        if path.is_empty() || path == "/" {
            self.config.root_path.clone()
        } else {
            format!("{}/{}", self.config.root_path.trim_end_matches('/'), path.trim_start_matches('/'))
        }
    }

    fn parse_list_line(&self, line: &str, current_path: &str) -> Option<FileInfo> {
        // 解析 Unix 风格的 LIST 输出
        // 格式: drwxr-xr-x 2 user group 4096 Jan 1 12:00 filename
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            return None;
        }

        let permissions = parts[0];
        let is_dir = permissions.starts_with('d');
        let size = if is_dir { 0 } else { parts[4].parse().unwrap_or(0) };
        
        // 文件名可能包含空格，从第8个部分开始连接
        let name = parts[8..].join(" ");
        let decoded_name = self.decode_name(&name);
        
        // 跳过 . 和 .. 目录
        if decoded_name == "." || decoded_name == ".." {
            return None;
        }

        let file_path = if current_path == "/" {
            format!("/{}", decoded_name)
        } else {
            format!("{}/{}", current_path.trim_end_matches('/'), decoded_name)
        };

        // 简化的时间解析，实际应该解析 parts[5..8]
        let modified = Utc::now().to_rfc3339();

        Some(FileInfo {
            name: decoded_name,
            path: file_path,
            size,
            is_dir,
            modified,
        })
    }


}

#[async_trait]
impl Driver for FtpDriver {
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>> {
        self.ensure_connection().await?;
        let mut conn_guard = self.connection.lock().await;
        let conn = conn_guard.as_mut().ok_or_else(|| anyhow!("FTP 连接不可用"))?;

        let full_path = self.get_full_path(path);
        let encoded_path = self.encode_path(&full_path);

        let entries = conn.list(Some(&encoded_path)).await
            .map_err(|e| anyhow!("列出目录失败: {}", e))?;

        let mut files = Vec::new();
        for entry_line in entries {
            if let Some(file_info) = self.parse_list_line(&entry_line, path) {
                files.push(file_info);
            }
        }

        Ok(files)
    }

    async fn download(&self, _path: &str) -> Result<tokio::fs::File> {
        // FTP 驱动优先使用流式下载，这里返回错误提示使用流式下载
        Err(anyhow!("FTP 驱动请使用流式下载"))
    }

    async fn stream_download(&self, path: &str) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        let full_path = self.get_full_path(path);
        let encoded_path = self.encode_path(&full_path);
        
        println!("🌊 FTP 流式下载: {}", encoded_path);
        
        // 获取文件名
        let filename = std::path::Path::new(path).file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("download"))
            .to_string_lossy()
            .to_string();
        
        // 创建新的独立连接用于流式传输
        let mut ftp_stream = AsyncFtpStream::connect(&self.config.address).await
            .map_err(|e| anyhow!("FTP 连接失败: {}", e))?;

        ftp_stream.login(&self.config.username, &self.config.password).await
            .map_err(|e| anyhow!("FTP 登录失败: {}", e))?;
        
        // 使用 retr_as_stream 下载文件流
        let data_stream = ftp_stream.retr_as_stream(&encoded_path).await
            .map_err(|e| anyhow!("FTP 下载文件失败: {}", e))?;
        
        // 创建异步流，将连接和数据流一起移动到流中
        let stream = async_stream::stream! {
            use futures_lite::io::AsyncReadExt;
            let mut data_stream = data_stream;
            let mut _ftp_connection = ftp_stream; // 保持连接活跃
            let mut buffer = [0u8; 8192]; // 8KB 缓冲区
            let mut total_bytes = 0u64;
            
            println!("🚀 开始 FTP 流式传输");
            
            loop {
                match data_stream.read(&mut buffer).await {
                    Ok(0) => {
                        println!("✅ FTP 流式传输完成，共 {} 字节 ({} MB)", 
                            total_bytes, total_bytes / 1024 / 1024);
                        break;
                    },
                    Ok(n) => {
                        total_bytes += n as u64;
                        // 每10MB输出一次进度
                        if total_bytes % (10 * 1024 * 1024) == 0 {
                            println!("📊 FTP 流式传输进度: {} MB", total_bytes / 1024 / 1024);
                        }
                        yield Ok(axum::body::Bytes::copy_from_slice(&buffer[..n]));
                    },
                    Err(e) => {
                        println!("❌ FTP 流式传输错误: {}", e);
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        break;
                    }
                }
            }
            
            // 连接会在这里自动关闭
            println!("🔌 FTP 连接已关闭");
        };
        
        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));
        
        Ok(Some((boxed_stream, filename)))
    }

    async fn get_download_url(&self, _path: &str) -> Result<Option<String>> {
        // FTP 不支持直接下载 URL，需要通过流式传输
        Ok(None)
    }

    async fn upload_file(&self, parent_path: &str, file_name: &str, content: &[u8]) -> Result<()> {
        self.ensure_connection().await?;
        let mut conn_guard = self.connection.lock().await;
        let conn = conn_guard.as_mut().ok_or_else(|| anyhow!("FTP 连接不可用"))?;

        let full_parent_path = self.get_full_path(parent_path);
        let file_path = format!("{}/{}", full_parent_path.trim_end_matches('/'), file_name);
        let encoded_path = self.encode_path(&file_path);

        // 使用 &[u8] 直接作为 AsyncRead
        let mut reader = content;
        conn.put_file(&encoded_path, &mut reader).await
            .map_err(|e| anyhow!("上传文件失败: {}", e))?;

        Ok(())
    }

    async fn move_file(&self, _file_path: &str, _new_parent_path: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("FTP驱动不支持移动操作"))
    }

    async fn copy_file(&self, _file_path: &str, _new_parent_path: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("FTP驱动不支持复制操作"))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.ensure_connection().await?;
        let mut conn_guard = self.connection.lock().await;
        let conn = conn_guard.as_mut().ok_or_else(|| anyhow!("FTP 连接不可用"))?;

        let full_path = self.get_full_path(path);
        let encoded_path = self.encode_path(&full_path);

        // 先尝试作为文件删除
        match conn.rm(&encoded_path).await {
            Ok(_) => Ok(()),
            Err(FtpError::UnexpectedResponse(_)) => {
                // 如果失败，尝试作为目录删除
                conn.rmdir(&encoded_path).await
                    .map_err(|e| anyhow!("删除失败: {}", e))
            }
            Err(e) => Err(anyhow!("删除失败: {}", e)),
        }
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        self.ensure_connection().await?;
        let mut conn_guard = self.connection.lock().await;
        let conn = conn_guard.as_mut().ok_or_else(|| anyhow!("FTP 连接不可用"))?;

        let full_path = self.get_full_path(path);
        let encoded_old_path = self.encode_path(&full_path);

        // 构建新路径
        let parent_dir = std::path::Path::new(&full_path).parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| self.config.root_path.clone());
        let new_path = format!("{}/{}", parent_dir.trim_end_matches('/'), new_name);
        let encoded_new_path = self.encode_path(&new_path);

        conn.rename(&encoded_old_path, &encoded_new_path).await
            .map_err(|e| anyhow!("重命名失败: {}", e))?;

        Ok(())
    }

    async fn create_folder(&self, parent_path: &str, folder_name: &str) -> Result<()> {
        self.ensure_connection().await?;
        let mut conn_guard = self.connection.lock().await;
        let conn = conn_guard.as_mut().ok_or_else(|| anyhow!("FTP 连接不可用"))?;

        let full_parent_path = self.get_full_path(parent_path);
        let encoded_parent_path = self.encode_path(&full_parent_path);
        let encoded_folder_name = self.encode_path(folder_name);
        let full_folder_path = format!("{}/{}", encoded_parent_path.trim_end_matches('/'), encoded_folder_name);

        conn.mkdir(&full_folder_path).await
            .map_err(|e| anyhow!("创建文件夹失败: {}", e))?;

        Ok(())
    }

    async fn get_file_info(&self, path: &str) -> Result<FileInfo> {
        // 对于FTP，我们需要列出父目录来获取文件信息
        let parent_path = if path.contains('/') {
            let parts: Vec<&str> = path.rsplitn(2, '/').collect();
            if parts.len() == 2 {
                parts[1]
            } else {
                "/"
            }
        } else {
            "/"
        };
        
        let filename = path.split('/').last().unwrap_or(path);
        let files = self.list(parent_path).await?;
        
        for file in files {
            if file.name == filename {
                return Ok(file);
            }
        }
        
        Err(anyhow!("文件不存在: {}", path))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        let full_path = self.get_full_path(path);
        let encoded_path = self.encode_path(&full_path);
        
        println!("🎯 FTP Range 下载: {} ({}:{:?})", encoded_path, start.unwrap_or(0), end);
        
        // 获取文件名
        let filename = std::path::Path::new(path).file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("download"))
            .to_string_lossy()
            .to_string();
        
        // 创建新的独立连接用于流式传输
        let mut ftp_stream = AsyncFtpStream::connect(&self.config.address).await
            .map_err(|e| anyhow!("FTP 连接失败: {}", e))?;

        ftp_stream.login(&self.config.username, &self.config.password).await
            .map_err(|e| anyhow!("FTP 登录失败: {}", e))?;
        
        // 获取文件大小
        let file_size = ftp_stream.size(&encoded_path).await
            .map_err(|e| anyhow!("获取文件大小失败: {}", e))? as u64;
        
        println!("📏 文件大小: {} 字节", file_size);
        
        // 计算实际的开始和结束位置
        let actual_start = start.unwrap_or(0);
        let actual_end = end.unwrap_or(file_size - 1).min(file_size - 1);
        
        if actual_start >= file_size {
            return Err(anyhow!("Range 起始位置超出文件大小"));
        }
        
        println!("📐 Range: {}-{}/{}", actual_start, actual_end, file_size);
        
        // 对于 FTP，我们需要重新连接并使用 REST 命令
        // 但 suppaftp 的 AsyncFtpStream 可能不直接支持 REST，我们使用简化方案
        // 暂时跳过 REST 命令，直接下载整个文件然后在流中跳过前面的字节
        
        let data_stream = ftp_stream.retr_as_stream(&encoded_path).await
            .map_err(|e| anyhow!("FTP 下载文件失败: {}", e))?;
        
        let content_length = actual_end - actual_start + 1;
        
        // 创建异步流，支持 Range 请求
        let stream = async_stream::stream! {
            use futures_lite::io::AsyncReadExt;
            let mut data_stream = data_stream;
            let mut _ftp_connection = ftp_stream; // 保持连接活跃
            let mut buffer = [0u8; 8192]; // 8KB 缓冲区
            let mut bytes_read = 0u64;
            let mut bytes_skipped = 0u64;
            let target_bytes = content_length;
            
            println!("🚀 开始 FTP Range 传输: {} 字节 (跳过前 {} 字节)", target_bytes, actual_start);
            
            // 首先跳过前面的字节
            while bytes_skipped < actual_start {
                let remaining_skip = actual_start - bytes_skipped;
                let to_skip = (buffer.len() as u64).min(remaining_skip) as usize;
                
                match data_stream.read(&mut buffer[..to_skip]).await {
                    Ok(0) => {
                        println!("⚠️ FTP 流在跳过阶段提前结束");
                        return;
                    },
                    Ok(n) => {
                        bytes_skipped += n as u64;
                        if bytes_skipped % (50 * 1024 * 1024) == 0 {
                            println!("📊 跳过进度: {} / {} MB", bytes_skipped / 1024 / 1024, actual_start / 1024 / 1024);
                        }
                    },
                    Err(e) => {
                        println!("❌ FTP 跳过阶段错误: {}", e);
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        return;
                    }
                }
            }
            
            println!("✅ 跳过完成，开始传输目标数据");
            
            // 然后读取目标范围的数据
            while bytes_read < target_bytes {
                let remaining = target_bytes - bytes_read;
                let to_read = (buffer.len() as u64).min(remaining) as usize;
                
                match data_stream.read(&mut buffer[..to_read]).await {
                    Ok(0) => {
                        println!("⚠️ FTP 流提前结束，已读取 {} / {} 字节", bytes_read, target_bytes);
                        break;
                    },
                    Ok(n) => {
                        bytes_read += n as u64;
                        // 每10MB输出一次进度
                        if bytes_read % (10 * 1024 * 1024) == 0 {
                            println!("📊 FTP Range 传输进度: {} / {} MB", 
                                bytes_read / 1024 / 1024, target_bytes / 1024 / 1024);
                        }
                        yield Ok(axum::body::Bytes::copy_from_slice(&buffer[..n]));
                    },
                    Err(e) => {
                        println!("❌ FTP Range 传输错误: {}", e);
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        break;
                    }
                }
            }
            
            println!("✅ FTP Range 传输完成: {} / {} 字节", bytes_read, target_bytes);
            println!("🔌 FTP 连接已关闭");
        };
        
        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));
        
        Ok(Some((boxed_stream, filename, file_size, Some(content_length))))
    }
}

pub struct FtpDriverFactory;

impl DriverFactory for FtpDriverFactory {
    fn driver_type(&self) -> &'static str {
        "ftp"
    }

    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "ftp".to_string(),
            display_name: "FTP".to_string(),
            description: "FTP 文件传输协议存储".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "address": {
                        "type": "string",
                        "title": "FTP 服务器地址",
                        "description": "FTP 服务器地址，格式：host:port",
                        "default": "localhost:21"
                    },
                    "username": {
                        "type": "string",
                        "title": "用户名",
                        "description": "FTP 登录用户名"
                    },
                    "password": {
                        "type": "string",
                        "title": "密码",
                        "description": "FTP 登录密码",
                        "format": "password"
                    },
                    "encoding": {
                        "type": "string",
                        "title": "编码",
                        "description": "文件名编码格式",
                        "default": "UTF-8",
                        "enum": ["UTF-8", "GBK", "GB2312", "Big5", "ISO-8859-1"]
                    },
                    "root_path": {
                        "type": "string",
                        "title": "根路径",
                        "description": "FTP 服务器上的根路径",
                        "default": "/"
                    }
                },
                "required": ["address", "username", "password"]
            }),
        }
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn Driver>> {
        let ftp_config: FtpConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("FTP 配置解析失败: {}", e))?;
        
        Ok(Box::new(FtpDriver::new(ftp_config)))
    }

    fn get_routes(&self) -> Option<axum::Router> {
        None
    }
} 