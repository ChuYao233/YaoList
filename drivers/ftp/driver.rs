//! FTP驱动实现
//!
//! 生产级别FTP驱动：
//! - 支持Range请求（断点续传）
//! - 流式读写（不占用内存）
//! - 完整文件操作（删除、重命名、创建目录、移动）
//! - 编码转换（GBK等）
//! - 宽松响应解析（兼容Serv-U/SmbFTPD等老服务器）
//为什么ftp我要写这么多，让alist/FnOS挂载不了只有win资源管理器和raidrive能挂的ftp我的yaolist能挂上
//是因为tmd的我们学校交作业的ftp服务器还在用2006年的Serv-U FTP Server v6.3,这玩意跟我同一个年龄。。。。。。
//win挂动不动卡死，还有不想用raidrive这种第三方软件
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use encoding_rs::Encoding;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};
use suppaftp::FtpStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

/// 解析FTP LIST时间格式
/// 格式1: "Jan 01 12:00" (今年内的文件)
/// 格式2: "Jan 01  2023" (超过6个月的文件显示年份)
fn parse_ftp_time(month: &str, day: &str, time_or_year: &str) -> Option<String> {
    use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
    
    let month_num = match month.to_lowercase().as_str() {
        "jan" => 1, "feb" => 2, "mar" => 3, "apr" => 4,
        "may" => 5, "jun" => 6, "jul" => 7, "aug" => 8,
        "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    };
    
    let day_num: u32 = day.parse().ok()?;
    let now = Local::now();
    
    if time_or_year.contains(':') {
        // 格式: HH:MM，使用当前年份
        let parts: Vec<&str> = time_or_year.split(':').collect();
        if parts.len() != 2 { return None; }
        let hour: u32 = parts[0].parse().ok()?;
        let minute: u32 = parts[1].parse().ok()?;
        
        let mut year = now.year();
        // 如果月份比当前月份大，说明是去年的文件
        if month_num > now.month() {
            year -= 1;
        }
        
        let date = NaiveDate::from_ymd_opt(year, month_num, day_num)?;
        let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
        let dt = NaiveDateTime::new(date, time);
        let datetime = Local.from_local_datetime(&dt).single()?;
        Some(datetime.to_rfc3339())
    } else {
        // 格式: 年份
        let year: i32 = time_or_year.trim().parse().ok()?;
        let date = NaiveDate::from_ymd_opt(year, month_num, day_num)?;
        let dt = date.and_hms_opt(0, 0, 0)?;
        let datetime = Local.from_local_datetime(&dt).single()?;
        Some(datetime.to_rfc3339())
    }
}

/// 自动检测编码解码FTP列表输出
/// 优先UTF-8，不合法则用GBK
fn decode_ftp_listing(bytes: &[u8]) -> String {
    // 优先尝试UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // UTF-8不合法 → 用GBK
    let (cow, _, _) = encoding_rs::GBK.decode(bytes);
    cow.into_owned()
}

/// 宽松的FTP响应读取器
/// 兼容：\n和\r\n换行、噪音行、多行响应格式错误
struct FtpResponse {
    code: u16,
    message: String,
}

impl FtpResponse {
    /// 从BufReader读取一个FTP响应（宽松模式，支持非UTF-8数据）
    fn read_relaxed<R: BufRead>(reader: &mut R) -> std::io::Result<Self> {
        let mut full_message = String::new();
        let mut final_code: Option<u16> = None;
        let mut in_multiline = false;
        let mut multiline_code: u16 = 0;
        
        loop {
            // 使用字节读取，避免UTF-8解码错误
            let mut line_bytes = Vec::new();
            let bytes_read = reader.read_until(b'\n', &mut line_bytes)?;
            
            if bytes_read == 0 {
                // EOF
                if let Some(code) = final_code {
                    return Ok(FtpResponse { code, message: full_message });
                }
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "FTP连接关闭"));
            }
            
            // 用lossy方式转换为字符串，非UTF-8字符会被替换
            let line_str = String::from_utf8_lossy(&line_bytes);
            // 去掉换行符（兼容\r\n和\n）
            let line = line_str.trim_end_matches(|c| c == '\r' || c == '\n');
            
            // 尝试解析3位状态码
            if line.len() >= 3 {
                if let Ok(code) = line[..3].parse::<u16>() {
                    // 有效的状态码
                    let separator = line.chars().nth(3);
                    
                    if separator == Some('-') {
                        // 多行响应开始: "123-message"
                        in_multiline = true;
                        multiline_code = code;
                        full_message.push_str(&line[4..]);
                        full_message.push('\n');
                    } else if separator == Some(' ') || line.len() == 3 {
                        // 单行响应或多行响应结束: "123 message" 或 "123"
                        if in_multiline {
                            if code == multiline_code {
                                // 多行响应结束
                                if line.len() > 4 {
                                    full_message.push_str(&line[4..]);
                                }
                                return Ok(FtpResponse { code, message: full_message });
                            } else {
                                // 码不匹配，当作消息行
                                full_message.push_str(line);
                                full_message.push('\n');
                            }
                        } else {
                            // 单行响应
                            let msg = if line.len() > 4 { &line[4..] } else { "" };
                            return Ok(FtpResponse { code, message: msg.to_string() });
                        }
                    } else {
                        // 3位数字后面跟着奇怪的字符，当作噪音行
                        if in_multiline {
                            full_message.push_str(line);
                            full_message.push('\n');
                        }
                        // 如果不在多行模式，记住这个code以防后面没有正常响应
                        if final_code.is_none() {
                            final_code = Some(code);
                        }
                    }
                } else {
                    // 不是有效状态码，当作噪音行或多行消息的一部分
                    if in_multiline {
                        full_message.push_str(line);
                        full_message.push('\n');
                    }
                    // 忽略非多行模式下的噪音行
                }
            } else {
                // 太短，当作噪音行
                if in_multiline && !line.is_empty() {
                    full_message.push_str(line);
                    full_message.push('\n');
                }
            }
            
            // 防止无限循环：如果已经读了太多行还没结束，强制返回
            if full_message.len() > 10000 {
                if let Some(code) = final_code {
                    return Ok(FtpResponse { code, message: full_message });
                }
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "FTP响应过长"));
            }
        }
    }
    
    /// 检查是否成功响应（2xx）
    fn is_success(&self) -> bool {
        self.code >= 200 && self.code < 300
    }
    
    /// 检查是否需要密码（331）
    fn needs_password(&self) -> bool {
        self.code == 331
    }
    
    /// 检查是否传输开始（125/150）
    fn is_transfer_start(&self) -> bool {
        self.code == 125 || self.code == 150
    }
}

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, SpaceInfo, StorageDriver,
};

/// FTP配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FtpConfig {
    /// FTP服务器地址 (host:port)
    pub address: String,
    /// 用户名
    pub username: String,
    /// 密码
    pub password: String,
    /// 根目录路径
    #[serde(default = "default_root")]
    pub root_path: String,
    /// 文件名编码 (如 GBK, UTF-8)
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

fn default_root() -> String {
    "/".to_string()
}

fn default_encoding() -> String {
    "UTF-8".to_string()
}

/// FTP驱动
pub struct FtpDriver {
    config: FtpConfig,
}

impl FtpDriver {
    pub fn new(config: FtpConfig) -> Self {
        Self { config }
    }

    /// 编码路径
    fn encode_path(&self, path: &str) -> String {
        if self.config.encoding.to_uppercase() == "UTF-8" {
            return path.to_string();
        }
        if let Some(encoding) = Encoding::for_label(self.config.encoding.as_bytes()) {
            let (encoded, _, _) = encoding.encode(path);
            String::from_utf8_lossy(&encoded).to_string()
        } else {
            path.to_string()
        }
    }

    /// 解码文件名
    fn decode_name(&self, name: &str) -> String {
        if self.config.encoding.to_uppercase() == "UTF-8" {
            return name.to_string();
        }
        if let Some(encoding) = Encoding::for_label(self.config.encoding.as_bytes()) {
            let (decoded, _, _) = encoding.decode(name.as_bytes());
            decoded.to_string()
        } else {
            name.to_string()
        }
    }

    /// 获取完整路径
    fn full_path(&self, path: &str) -> String {
        let root = self.config.root_path.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            root.to_string()
        } else {
            format!("{}/{}", root, path)
        }
    }

    /// 连接到FTP服务器
    fn connect(&self) -> Result<FtpStream> {
        let mut ftp = FtpStream::connect(&self.config.address)
            .map_err(|e| anyhow!("FTP连接失败: {}", e))?;
        
        ftp.login(&self.config.username, &self.config.password)
            .map_err(|e| anyhow!("FTP登录失败: {}", e))?;
        
        // 设置被动模式
        ftp.set_mode(suppaftp::Mode::Passive);
        
        // 设置二进制传输模式
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .map_err(|e| anyhow!("设置传输模式失败: {}", e))?;
        
        Ok(ftp)
    }

}

#[async_trait]
impl StorageDriver for FtpDriver {
    fn name(&self) -> &str {
        "FTP"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn capabilities(&self) -> Capability {
        Capability {
            can_range_read: true,
            can_append: false,
            can_direct_link: false, // 浏览器不支持ftp://链接，必须代理
            max_chunk_size: None,
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false, // FTP支持流式写入
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let full_path = self.full_path(path);
        let encoded_path = self.encode_path(&full_path);
        let config = self.config.clone();
        let encoding = self.config.encoding.clone();
        let path_clone = path.to_string();
        
        // 在独立线程中执行FTP操作
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<Entry>> {
            // 先尝试自定义实现，失败后尝试suppaftp（兼容Serv-U等老服务器）
            let entries = match ftp_list(&config.address, &config.username, &config.password, &encoded_path) {
                Ok(e) => e,
                Err(_) => {
                    ftp_list_compat(&config.address, &config.username, &config.password, &encoded_path)?
                }
            };
            
            let mut result = Vec::new();
            
            for line in entries {
                
                // 解析FTP LIST输出格式
                // 典型格式: drwxr-xr-x 2 user group 4096 Jan 01 12:00 dirname
                // 或: -rw-r--r-- 1 user group 1234 Jan 01 12:00 filename
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 9 {
                    continue;
                }
                
                let permissions = parts[0];
                let size_str = parts[4];
                // 时间字段: parts[5]=月, parts[6]=日, parts[7]=时间或年
                let month = parts[5];
                let day = parts[6];
                let time_or_year = parts[7];
                let name = parts[8..].join(" ");
                
                if name == "." || name == ".." {
                    continue;
                }
                
                // 解码文件名
                let decoded_name = if encoding.to_uppercase() == "UTF-8" {
                    name.clone()
                } else if let Some(enc) = Encoding::for_label(encoding.as_bytes()) {
                    let (decoded, _, _) = enc.decode(name.as_bytes());
                    decoded.to_string()
                } else {
                    name.clone()
                };
                
                let is_dir = permissions.starts_with('d');
                let size: i64 = size_str.parse().unwrap_or(0);
                
                // 解析修改时间
                let modified = parse_ftp_time(month, day, time_or_year);
                
                let entry_path = if path_clone.is_empty() || path_clone == "/" {
                    format!("/{}", decoded_name)
                } else {
                    format!("{}/{}", path_clone.trim_end_matches('/'), decoded_name)
                };
                
                result.push(Entry {
                    name: decoded_name,
                    path: entry_path,
                    size: size as u64,
                    is_dir,
                    modified,
                });
            }
            
            Ok(result)
        }).await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;
        
        Ok(result)
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let full_path = self.full_path(path);
        // 不要预编码，在RETR命令处用GBK编码
        let config = self.config.clone();
        let range_clone = range.clone();
        
        // 创建channel用于流式传输（64KB缓冲区）
        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, std::io::Error>>(8);
        
        // 计算要读取的字节数
        let bytes_to_read = range.as_ref().map(|r| (r.end - r.start) as usize);
        
        // 使用自定义FTP协议实现正确的REST命令顺序
        let range_start = range_clone.as_ref().map(|r| r.start).unwrap_or(0);
        let full_path_clone = full_path.clone();
        
        std::thread::spawn(move || {
            let result = (|| -> Result<(), std::io::Error> {
                // 连接控制通道
                let mut ctrl = TcpStream::connect(&config.address)?;
                ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
                ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
                
                let mut ctrl_reader = BufReader::new(ctrl.try_clone()?);
                let mut response = String::new();
                
                // 读取欢迎消息（可能是多行）
                loop {
                    response.clear();
                    ctrl_reader.read_line(&mut response)?;
                    if response.is_empty() {
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, "FTP服务器无响应"));
                    }
                    if response.starts_with("220 ") || (response.starts_with("220") && response.len() <= 5) {
                        break;
                    }
                    if response.starts_with("220-") {
                        continue;
                    }
                    if !response.starts_with("220") {
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("FTP连接失败: {}", response)));
                    }
                }
                
                // LOGIN
                ctrl.write_all(format!("USER {}\r\n", config.username).as_bytes())?;
                response.clear();
                ctrl_reader.read_line(&mut response)?;
                if response.starts_with("331") {
                    // 全角字符转半角
                    let normalized_pass: String = config.password.chars().map(|c| {
                        if c >= '\u{FF01}' && c <= '\u{FF5E}' {
                            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
                        } else {
                            c
                        }
                    }).collect();
                    ctrl.write_all(format!("PASS {}\r\n", normalized_pass).as_bytes())?;
                    response.clear();
                    ctrl_reader.read_line(&mut response)?;
                    if !response.starts_with("230") {
                        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("登录失败: {}", response)));
                    }
                }
                
                // TYPE I
                ctrl.write_all(b"TYPE I\r\n")?;
                response.clear();
                ctrl_reader.read_line(&mut response)?;
                
                // PASV
                ctrl.write_all(b"PASV\r\n")?;
                response.clear();
                ctrl_reader.read_line(&mut response)?;
                if !response.starts_with("227") {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, "PASV失败"));
                }
                
                // 解析PASV响应
                let data_addr = parse_pasv_response(&response)?;
                
                // 先连接数据通道
                let mut data_stream = TcpStream::connect(&data_addr)?;
                data_stream.set_read_timeout(Some(std::time::Duration::from_secs(60)))?;
                
                // REST (如果需要)
                if range_start > 0 {
                    ctrl.write_all(format!("REST {}\r\n", range_start).as_bytes())?;
                    response.clear();
                    ctrl_reader.read_line(&mut response)?;
                }
                
                // RETR（路径用GBK编码）
                let mut retr_cmd = b"RETR ".to_vec();
                retr_cmd.extend_from_slice(&encode_path_gbk(&full_path_clone));
                retr_cmd.extend_from_slice(b"\r\n");
                ctrl.write_all(&retr_cmd)?;
                response.clear();
                ctrl_reader.read_line(&mut response)?;
                if !response.starts_with("150") && !response.starts_with("125") {
                    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("RETR失败: {}", response)));
                }
                
                // 流式读取
                let mut total_read = 0usize;
                let chunk_size = 64 * 1024;
                
                loop {
                    if let Some(limit) = bytes_to_read {
                        if total_read >= limit {
                            break;
                        }
                    }
                    
                    let read_size = if let Some(limit) = bytes_to_read {
                        std::cmp::min(chunk_size, limit - total_read)
                    } else {
                        chunk_size
                    };
                    
                    let mut buf = vec![0u8; read_size];
                    match data_stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.truncate(n);
                            total_read += n;
                            if tx.blocking_send(Ok(buf)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.blocking_send(Err(e));
                            break;
                        }
                    }
                }
                
                Ok(())
            })();
            
            if let Err(e) = result {
                tracing::error!("FTP reader error: {}", e);
                let _ = tx.blocking_send(Err(e));
            }
        });
        
        Ok(Box::new(FtpStreamReader::new(rx)))
    }

    async fn open_writer(
        &self,
        path: &str,
        _size_hint: Option<u64>,
        _progress: Option<crate::storage::ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let full_path = self.full_path(path);
        // 不要预编码，让FtpStreamWriter内部用GBK编码
        
        Ok(Box::new(FtpStreamWriter::new(
            self.config.clone(),
            full_path,
        )))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let full_path = self.full_path(path);
        // 不要预编码，让ftp_delete内部用GBK编码
        let config = self.config.clone();
        
        tokio::task::spawn_blocking(move || {
            ftp_delete(&config.address, &config.username, &config.password, &full_path)
        }).await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;
        
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let full_path = self.full_path(path);
        // 不要预编码，让ftp_mkdir内部用GBK编码
        let config = self.config.clone();
        
        tokio::task::spawn_blocking(move || {
            ftp_mkdir(&config.address, &config.username, &config.password, &full_path)
        }).await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;
        
        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        let full_path = self.full_path(path);
        // 不要预编码，让ftp_rename内部用GBK编码
        let parent = std::path::Path::new(&full_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let new_path = format!("{}/{}", parent, new_name);
        let config = self.config.clone();
        
        tokio::task::spawn_blocking(move || {
            ftp_rename(&config.address, &config.username, &config.password, &full_path, &new_path)
        }).await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;
        
        Ok(())
    }

    async fn move_item(&self, from: &str, to: &str) -> Result<()> {
        let from_full = self.full_path(from);
        let to_full = self.full_path(to);
        // 不要预编码，让ftp_rename内部用GBK编码
        let config = self.config.clone();
        
        tokio::task::spawn_blocking(move || {
            ftp_rename(&config.address, &config.username, &config.password, &from_full, &to_full)
        }).await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;
        
        Ok(())
    }

    async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
        // 生成FTP直链: ftp://user:pass@host/path
        let full_path = self.full_path(path);
        let encoded_path = self.encode_path(&full_path);
        
        // URL编码用户名和密码
        let username = urlencoding::encode(&self.config.username);
        let password = urlencoding::encode(&self.config.password);
        
        // 解析地址
        let address = &self.config.address;
        
        let url = format!("ftp://{}:{}@{}{}", username, password, address, encoded_path);
        Ok(Some(url))
    }

    async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
        // FTP协议不支持获取空间信息
        Ok(None)
    }

    fn show_space_in_frontend(&self) -> bool {
        false
    }
}

/// FTP流式读取器
pub struct FtpStreamReader {
    rx: mpsc::Receiver<Result<Vec<u8>, std::io::Error>>,
    buffer: Vec<u8>,
    pos: usize,
    done: bool,
    error: Option<std::io::Error>,
}

impl FtpStreamReader {
    fn new(rx: mpsc::Receiver<Result<Vec<u8>, std::io::Error>>) -> Self {
        Self {
            rx,
            buffer: Vec::new(),
            pos: 0,
            done: false,
            error: None,
        }
    }
}

impl AsyncRead for FtpStreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // 如果有错误，返回错误
        if let Some(e) = self.error.take() {
            return Poll::Ready(Err(e));
        }
        
        // 如果buffer中还有数据，先消费
        if self.pos < self.buffer.len() {
            let remaining = &self.buffer[self.pos..];
            let to_read = std::cmp::min(remaining.len(), buf.remaining());
            buf.put_slice(&remaining[..to_read]);
            self.pos += to_read;
            return Poll::Ready(Ok(()));
        }
        
        // buffer已空，尝试从channel获取更多数据
        if self.done {
            return Poll::Ready(Ok(())); // EOF
        }
        
        // 尝试接收数据
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(data))) => {
                self.buffer = data;
                self.pos = 0;
                // 递归调用以消费新数据
                let remaining = &self.buffer[self.pos..];
                let to_read = std::cmp::min(remaining.len(), buf.remaining());
                buf.put_slice(&remaining[..to_read]);
                self.pos += to_read;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(e))) => {
                self.done = true;
                Poll::Ready(Err(e))
            }
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(Ok(())) // EOF
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// FTP流式写入器
pub struct FtpStreamWriter {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    handle: Option<std::thread::JoinHandle<std::io::Result<()>>>,
    closed: bool,
}

impl FtpStreamWriter {
    fn new(config: FtpConfig, path: String) -> Self {
        // 创建channel用于流式传输
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        
        // 在独立线程中执行FTP上传（使用兼容模式登录）
        let handle = std::thread::spawn(move || -> std::io::Result<()> {
            // 尝试多种登录方式
            let (mut ctrl, mut reader) = ftp_login_compat(&config.address, &config.username, &config.password)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            
            // 自动创建父目录（如果路径包含目录）
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let parent_str = parent.to_string_lossy();
                if !parent_str.is_empty() && parent_str != "/" {
                    // 递归创建所有父目录
                    let parts: Vec<&str> = parent_str.split('/').filter(|s| !s.is_empty()).collect();
                    let mut current_path = String::new();
                    for part in parts {
                        current_path = format!("{}/{}", current_path, part);
                        // MKD（忽略已存在错误）
                        let mut mkd_cmd = b"MKD ".to_vec();
                        mkd_cmd.extend_from_slice(&encode_path_gbk(&current_path));
                        mkd_cmd.extend_from_slice(b"\r\n");
                        ctrl.write_all(&mkd_cmd)?;
                        let _ = FtpResponse::read_relaxed(&mut reader); // 忽略结果，目录可能已存在
                    }
                }
            }
            
            // TYPE I
            ctrl.write_all(b"TYPE I\r\n")?;
            let _ = FtpResponse::read_relaxed(&mut reader);
            
            // PASV
            ctrl.write_all(b"PASV\r\n")?;
            let pasv_resp = FtpResponse::read_relaxed(&mut reader)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            
            let full_resp = format!("{} {}", pasv_resp.code, pasv_resp.message);
            let data_addr = parse_pasv_response(&full_resp)?;
            let mut data_stream = TcpStream::connect(&data_addr)?;
            
            // STOR（路径用GBK编码）
            let mut stor_cmd = b"STOR ".to_vec();
            stor_cmd.extend_from_slice(&encode_path_gbk(&path));
            stor_cmd.extend_from_slice(b"\r\n");
            ctrl.write_all(&stor_cmd)?;
            
            let stor_resp = FtpResponse::read_relaxed(&mut reader)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            if !stor_resp.is_transfer_start() {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, 
                    format!("STOR失败: {} {}", stor_resp.code, stor_resp.message)));
            }
            
            // 从channel接收数据并写入FTP
            while let Some(data) = rx.blocking_recv() {
                data_stream.write_all(&data)?;
            }
            
            // 关闭数据连接
            drop(data_stream);
            
            // 等待传输完成
            let _ = FtpResponse::read_relaxed(&mut reader);
            
            let _ = ctrl.write_all(b"QUIT\r\n");
            Ok(())
        });
        
        Self {
            tx: Some(tx),
            handle: Some(handle),
            closed: false,
        }
    }
}

impl AsyncWrite for FtpStreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Some(ref tx) = self.tx {
            // 尝试发送数据
            let data = buf.to_vec();
            match tx.try_send(data) {
                Ok(()) => Poll::Ready(Ok(buf.len())),
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // channel满了，需要等待
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "FTP上传线程已关闭",
                    )))
                }
            }
        } else {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Writer已关闭",
            )))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        self.closed = true;
        
        // 关闭发送端，通知上传线程结束
        self.tx.take();
        
        // 等待上传线程完成
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(Ok(())) => Poll::Ready(Ok(())),
                Ok(Err(e)) => Poll::Ready(Err(e)),
                Err(_) => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "上传线程panic",
                ))),
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

/// FTP驱动工厂
pub struct FtpDriverFactory;

impl DriverFactory for FtpDriverFactory {
    fn driver_type(&self) -> &'static str {
        "ftp"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "FTP".to_string(),
            local_sort: true,
            only_proxy: true, // 浏览器不支持ftp://，必须代理
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("address", "string")
                .title("服务器地址")
                .help("FTP服务器地址，格式: host:port")
                .required(),
            ConfigItem::new("username", "string")
                .title("用户名")
                .required(),
            ConfigItem::new("password", "password")
                .title("密码")
                .required(),
            ConfigItem::new("root_path", "string")
                .title("根目录")
                .help("FTP服务器上的根目录路径")
                .default("/"),
            ConfigItem::new("encoding", "string")
                .title("文件名编码")
                .help("FTP服务器文件名编码，如 GBK, UTF-8")
                .default("UTF-8"),
        ]
    }

    fn create_driver(&self, config: serde_json::Value) -> Result<Box<dyn StorageDriver>> {
        let config: FtpConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(FtpDriver::new(config)))
    }
}

/// 全角字符转半角
fn normalize_password(password: &str) -> String {
    password.chars().map(|c| {
        if c >= '\u{FF01}' && c <= '\u{FF5E}' {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        } else {
            c
        }
    }).collect()
}

/// 自定义FTP登录（兼容更多FTP服务器）
fn ftp_login(address: &str, username: &str, password: &str) -> Result<(TcpStream, BufReader<TcpStream>)> {
    let mut ctrl = TcpStream::connect(address)
        .map_err(|e| anyhow!("FTP连接失败 {}: {}", address, e))?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    let mut reader = BufReader::new(ctrl.try_clone()?);
    let mut response = String::new();
    
    // 读取欢迎消息（可能是多行，格式：220-xxx 或 220 xxx）
    loop {
        response.clear();
        match reader.read_line(&mut response) {
            Ok(0) => return Err(anyhow!("FTP服务器关闭连接: {}", address)),
            Ok(_) => {}
            Err(e) => return Err(anyhow!("FTP读取欢迎消息失败 {}: {}", address, e)),
        }
        if response.starts_with("220 ") || (response.starts_with("220") && response.len() <= 5) {
            break;
        }
        if response.starts_with("220-") {
            continue;
        }
        if !response.starts_with("220") {
            return Err(anyhow!("FTP服务器响应异常 {}: {}", address, response.trim()));
        }
    }
    
    // USER
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    response.clear();
    reader.read_line(&mut response)?;
    
    // PASS
    if response.starts_with("331") {
        let normalized_pass = normalize_password(password);
        ctrl.write_all(format!("PASS {}\r\n", normalized_pass).as_bytes())?;
        
        response.clear();
        reader.read_line(&mut response)?;
        
        // 530错误可能是密码错误，也可能是Serv-U的兼容性问题
        // 尝试不转换密码再试一次
        if response.starts_with("530") && password != &normalized_pass {
            // 重新连接并使用原始密码
            drop(reader);
            drop(ctrl);
            
            let mut ctrl2 = TcpStream::connect(address)?;
            ctrl2.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
            ctrl2.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
            let mut reader2 = BufReader::new(ctrl2.try_clone()?);
            let mut resp2 = String::new();
            
            // 跳过欢迎消息
            loop {
                resp2.clear();
                reader2.read_line(&mut resp2)?;
                if resp2.starts_with("220 ") || (resp2.starts_with("220") && resp2.len() <= 5) {
                    break;
                }
                if !resp2.starts_with("220") { break; }
            }
            
            ctrl2.write_all(format!("USER {}\r\n", username).as_bytes())?;
            resp2.clear();
            reader2.read_line(&mut resp2)?;
            
            if resp2.starts_with("331") {
                ctrl2.write_all(format!("PASS {}\r\n", password).as_bytes())?;
                resp2.clear();
                reader2.read_line(&mut resp2)?;
                if resp2.starts_with("230") {
                    return Ok((ctrl2, reader2));
                }
            }
            return Err(anyhow!("FTP登录失败: {}", response.trim()));
        }
        
        if !response.starts_with("230") {
            return Err(anyhow!("FTP登录失败: {}", response.trim()));
        }
    } else if !response.starts_with("230") {
        return Err(anyhow!("FTP用户名错误: {}", response.trim()));
    }
    
    Ok((ctrl, reader))
}

/// 兼容模式FTP登录（尝试多种密码编码）
fn ftp_login_compat(address: &str, username: &str, password: &str) -> Result<(TcpStream, BufReader<TcpStream>)> {
    // 1. 先尝试原始密码
    if let Ok(result) = ftp_login_try(address, username, password) {
        return Ok(result);
    }
    
    // 2. 尝试全角转半角密码
    let normalized = normalize_password(password);
    if normalized != password {
        if let Ok(result) = ftp_login_try(address, username, &normalized) {
            return Ok(result);
        }
    }
    
    // 3. 尝试GBK编码密码
    ftp_login_try_gbk(address, username, password)
}

/// 尝试指定密码登录
fn ftp_login_try(address: &str, username: &str, password: &str) -> Result<(TcpStream, BufReader<TcpStream>)> {
    let mut ctrl = TcpStream::connect(address)?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    let welcome = FtpResponse::read_relaxed(&mut reader)?;
    if !welcome.is_success() {
        return Err(anyhow!("FTP服务器拒绝连接"));
    }
    
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if user_resp.needs_password() {
        ctrl.write_all(format!("PASS {}\r\n", password).as_bytes())?;
        let pass_resp = FtpResponse::read_relaxed(&mut reader)?;
        if !pass_resp.is_success() {
            return Err(anyhow!("登录失败: {}", pass_resp.message));
        }
    } else if !user_resp.is_success() {
        return Err(anyhow!("用户名错误"));
    }
    
    Ok((ctrl, reader))
}

/// 尝试GBK编码密码登录
fn ftp_login_try_gbk(address: &str, username: &str, password: &str) -> Result<(TcpStream, BufReader<TcpStream>)> {
    let mut ctrl = TcpStream::connect(address)?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    let welcome = FtpResponse::read_relaxed(&mut reader)?;
    if !welcome.is_success() {
        return Err(anyhow!("FTP服务器拒绝连接"));
    }
    
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if user_resp.needs_password() {
        let gbk_pass = encode_password_gbk(password);
        let mut cmd = b"PASS ".to_vec();
        cmd.extend_from_slice(&gbk_pass);
        cmd.extend_from_slice(b"\r\n");
        ctrl.write_all(&cmd)?;
        
        let pass_resp = FtpResponse::read_relaxed(&mut reader)?;
        if !pass_resp.is_success() {
            return Err(anyhow!("GBK登录失败: {}", pass_resp.message));
        }
    } else if !user_resp.is_success() {
        return Err(anyhow!("用户名错误"));
    }
    
    Ok((ctrl, reader))
}

/// 自定义FTP LIST命令
fn ftp_list(address: &str, username: &str, password: &str, path: &str) -> Result<Vec<String>> {
    let (mut ctrl, mut reader) = ftp_login(address, username, password)?;
    let mut response = String::new();
    
    // TYPE I
    ctrl.write_all(b"TYPE I\r\n")?;
    reader.read_line(&mut response)?;
    
    // PASV
    ctrl.write_all(b"PASV\r\n")?;
    response.clear();
    reader.read_line(&mut response)?;
    if !response.starts_with("227") {
        return Err(anyhow!("PASV失败: {}", response.trim()));
    }
    
    let data_addr = parse_pasv_response(&response)
        .map_err(|e| anyhow!("解析PASV响应失败: {}", e))?;
    
    // 连接数据通道
    let mut data_stream = TcpStream::connect(&data_addr)
        .map_err(|e| anyhow!("数据连接失败: {}", e))?;
    data_stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    // LIST（路径用GBK编码）
    let mut list_cmd = b"LIST ".to_vec();
    list_cmd.extend_from_slice(&encode_path_gbk(path));
    list_cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&list_cmd)?;
    response.clear();
    reader.read_line(&mut response)?;
    if !response.starts_with("150") && !response.starts_with("125") {
        return Err(anyhow!("LIST失败: {}", response.trim()));
    }
    
    // 读取目录列表
    let mut list_data = Vec::new();
    data_stream.read_to_end(&mut list_data)?;
    
    // 等待传输完成
    response.clear();
    let _ = reader.read_line(&mut response);
    
    // QUIT
    let _ = ctrl.write_all(b"QUIT\r\n");
    
    // 解析列表
    let list_str = decode_ftp_listing(&list_data);
    Ok(list_str.lines().map(|s| s.to_string()).collect())
}

/// Serv-U/SmbFTPD兼容模式 - 使用宽松响应解析器
fn ftp_list_compat(address: &str, username: &str, password: &str, path: &str) -> Result<Vec<String>> {
    let mut ctrl = TcpStream::connect(address)
        .map_err(|e| anyhow!("FTP连接失败: {}", e))?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    // 读取欢迎消息（使用宽松解析器）
    let welcome = FtpResponse::read_relaxed(&mut reader)
        .map_err(|e| anyhow!("读取欢迎消息失败: {}", e))?;
    
    if !welcome.is_success() {
        return Err(anyhow!("FTP服务器拒绝连接: {} {}", welcome.code, welcome.message));
    }
    
    // USER
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)
        .map_err(|e| anyhow!("USER命令失败: {}", e))?;
    
    // PASS - 尝试多种密码编码
    if user_resp.needs_password() {
        // 1. 先尝试原始密码
        ctrl.write_all(format!("PASS {}\r\n", password).as_bytes())?;
        let pass_resp = FtpResponse::read_relaxed(&mut reader)
            .map_err(|e| anyhow!("PASS命令失败: {}", e))?;
        
        if !pass_resp.is_success() {
            // 2. 尝试全角转半角
            let normalized = normalize_password(password);
            if normalized != password {
                drop(reader);
                drop(ctrl);
                
                if let Ok(result) = ftp_list_compat_try_pass(address, username, &normalized, path) {
                    return Ok(result);
                }
            }
            
            // 3. 尝试GBK编码密码
            if let Ok(result) = ftp_list_compat_try_pass_gbk(address, username, password, path) {
                return Ok(result);
            }
            
            return Err(anyhow!("FTP登录失败: {} {}", pass_resp.code, pass_resp.message));
        }
    } else if !user_resp.is_success() {
        return Err(anyhow!("FTP用户名错误: {} {}", user_resp.code, user_resp.message));
    }
    
    // TYPE I（忽略响应）
    ctrl.write_all(b"TYPE I\r\n")?;
    let _ = FtpResponse::read_relaxed(&mut reader);
    
    // PASV
    ctrl.write_all(b"PASV\r\n")?;
    let pasv_resp = FtpResponse::read_relaxed(&mut reader)
        .map_err(|e| anyhow!("PASV命令失败: {}", e))?;
    
    // 从响应中解析数据连接地址
    let full_resp = format!("{} {}", pasv_resp.code, pasv_resp.message);
    if !full_resp.contains("(") {
        return Err(anyhow!("PASV响应无效: {}", full_resp));
    }
    
    let data_addr = parse_pasv_response(&full_resp)
        .map_err(|e| anyhow!("解析PASV失败: {}", e))?;
    
    let mut data_stream = TcpStream::connect(&data_addr)?;
    data_stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    // LIST（路径用GBK编码）
    let mut list_cmd = b"LIST ".to_vec();
    list_cmd.extend_from_slice(&encode_path_gbk(path));
    list_cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&list_cmd)?;
    let list_resp = FtpResponse::read_relaxed(&mut reader)
        .map_err(|e| anyhow!("LIST命令失败: {}", e))?;
    
    if !list_resp.is_transfer_start() && !list_resp.is_success() {
        return Err(anyhow!("LIST失败: {} {}", list_resp.code, list_resp.message));
    }
    
    // 读取目录数据
    let mut list_data = Vec::new();
    let _ = data_stream.read_to_end(&mut list_data);
    drop(data_stream);
    
    // 等待传输完成响应（忽略错误）
    let _ = FtpResponse::read_relaxed(&mut reader);
    
    let _ = ctrl.write_all(b"QUIT\r\n");
    
    let list_str = decode_ftp_listing(&list_data);
    Ok(list_str.lines().map(|s| s.to_string()).collect())
}

/// 尝试用GBK编码密码
fn encode_password_gbk(password: &str) -> Vec<u8> {
    if let Some(encoding) = Encoding::for_label(b"GBK") {
        let (encoded, _, _) = encoding.encode(password);
        encoded.into_owned()
    } else {
        password.as_bytes().to_vec()
    }
}

/// 尝试指定密码登录并列表
fn ftp_list_compat_try_pass(address: &str, username: &str, password: &str, path: &str) -> Result<Vec<String>> {
    let mut ctrl = TcpStream::connect(address)?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    let _ = FtpResponse::read_relaxed(&mut reader)?;
    
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if user_resp.needs_password() {
        ctrl.write_all(format!("PASS {}\r\n", password).as_bytes())?;
        let pass_resp = FtpResponse::read_relaxed(&mut reader)?;
        if !pass_resp.is_success() {
            return Err(anyhow!("登录失败: {}", pass_resp.message));
        }
    }
    
    ftp_do_list(&mut ctrl, &mut reader, path)
}

/// 尝试GBK编码密码登录并列表
fn ftp_list_compat_try_pass_gbk(address: &str, username: &str, password: &str, path: &str) -> Result<Vec<String>> {
    let mut ctrl = TcpStream::connect(address)?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    let _ = FtpResponse::read_relaxed(&mut reader)?;
    
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if user_resp.needs_password() {
        // 用GBK编码发送密码
        let gbk_pass = encode_password_gbk(password);
        let mut cmd = b"PASS ".to_vec();
        cmd.extend_from_slice(&gbk_pass);
        cmd.extend_from_slice(b"\r\n");
        ctrl.write_all(&cmd)?;
        
        let pass_resp = FtpResponse::read_relaxed(&mut reader)?;
        if !pass_resp.is_success() {
            return Err(anyhow!("登录失败: {}", pass_resp.message));
        }
    }
    
    ftp_do_list(&mut ctrl, &mut reader, path)
}

/// 将路径编码为GBK（用于发送FTP命令）
fn encode_path_gbk(path: &str) -> Vec<u8> {
    let (encoded, _, _) = encoding_rs::GBK.encode(path);
    encoded.into_owned()
}

/// 执行LIST操作（路径用GBK编码）
fn ftp_do_list(ctrl: &mut TcpStream, reader: &mut BufReader<TcpStream>, path: &str) -> Result<Vec<String>> {
    ctrl.write_all(b"TYPE I\r\n")?;
    let _ = FtpResponse::read_relaxed(reader);
    
    ctrl.write_all(b"PASV\r\n")?;
    let pasv_resp = FtpResponse::read_relaxed(reader)?;
    let full_resp = format!("{} {}", pasv_resp.code, pasv_resp.message);
    let data_addr = parse_pasv_response(&full_resp)?;
    
    let mut data_stream = TcpStream::connect(&data_addr)?;
    data_stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    // 路径用GBK编码发送
    let mut cmd = b"LIST ".to_vec();
    cmd.extend_from_slice(&encode_path_gbk(path));
    cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&cmd)?;
    let _ = FtpResponse::read_relaxed(reader);
    
    let mut list_data = Vec::new();
    let _ = data_stream.read_to_end(&mut list_data);
    drop(data_stream);
    
    let _ = FtpResponse::read_relaxed(reader);
    let _ = ctrl.write_all(b"QUIT\r\n");
    
    let list_str = decode_ftp_listing(&list_data);
    Ok(list_str.lines().map(|s| s.to_string()).collect())
}

/// 内部函数：已转换密码的登录
fn ftp_list_compat_inner(address: &str, username: &str, password: &str, path: &str) -> Result<Vec<String>> {
    let mut ctrl = TcpStream::connect(address)?;
    ctrl.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    ctrl.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    let mut reader = BufReader::new(ctrl.try_clone()?);
    
    let _ = FtpResponse::read_relaxed(&mut reader)?;
    
    ctrl.write_all(format!("USER {}\r\n", username).as_bytes())?;
    let user_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if user_resp.needs_password() {
        ctrl.write_all(format!("PASS {}\r\n", password).as_bytes())?;
        let pass_resp = FtpResponse::read_relaxed(&mut reader)?;
        if !pass_resp.is_success() {
            return Err(anyhow!("FTP登录失败: {} {}", pass_resp.code, pass_resp.message));
        }
    }
    
    ctrl.write_all(b"TYPE I\r\n")?;
    let _ = FtpResponse::read_relaxed(&mut reader);
    
    ctrl.write_all(b"PASV\r\n")?;
    let pasv_resp = FtpResponse::read_relaxed(&mut reader)?;
    let full_resp = format!("{} {}", pasv_resp.code, pasv_resp.message);
    let data_addr = parse_pasv_response(&full_resp)?;
    
    let mut data_stream = TcpStream::connect(&data_addr)?;
    data_stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    
    let mut list_cmd = b"LIST ".to_vec();
    list_cmd.extend_from_slice(&encode_path_gbk(path));
    list_cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&list_cmd)?;
    let _ = FtpResponse::read_relaxed(&mut reader);
    
    let mut list_data = Vec::new();
    let _ = data_stream.read_to_end(&mut list_data);
    drop(data_stream);
    
    let _ = FtpResponse::read_relaxed(&mut reader);
    let _ = ctrl.write_all(b"QUIT\r\n");
    
    let list_str = decode_ftp_listing(&list_data);
    Ok(list_str.lines().map(|s| s.to_string()).collect())
}

/// 自定义FTP删除文件
fn ftp_delete(address: &str, username: &str, password: &str, path: &str) -> Result<()> {
    let (mut ctrl, mut reader) = ftp_login_compat(address, username, password)?;
    
    // 尝试删除文件（路径用GBK编码）
    let mut cmd = b"DELE ".to_vec();
    cmd.extend_from_slice(&encode_path_gbk(path));
    cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&cmd)?;
    
    let dele_resp = FtpResponse::read_relaxed(&mut reader)?;
    
    if dele_resp.code != 250 {
        // 尝试删除目录（路径用GBK编码）
        let mut cmd = b"RMD ".to_vec();
        cmd.extend_from_slice(&encode_path_gbk(path));
        cmd.extend_from_slice(b"\r\n");
        ctrl.write_all(&cmd)?;
        
        let rmd_resp = FtpResponse::read_relaxed(&mut reader)?;
        if rmd_resp.code != 250 {
            return Err(anyhow!("删除失败: {} {}", rmd_resp.code, rmd_resp.message));
        }
    }
    
    let _ = ctrl.write_all(b"QUIT\r\n");
    Ok(())
}

/// 自定义FTP创建目录
fn ftp_mkdir(address: &str, username: &str, password: &str, path: &str) -> Result<()> {
    let (mut ctrl, mut reader) = ftp_login_compat(address, username, password)?;
    
    // MKD（路径用GBK编码）
    let gbk_path = encode_path_gbk(path);
    let mut cmd = b"MKD ".to_vec();
    cmd.extend_from_slice(&gbk_path);
    cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&cmd)?;
    
    let resp = FtpResponse::read_relaxed(&mut reader)?;
    
    // 550可能是目录已存在，不算错误
    if resp.code != 257 && resp.code != 550 {
        return Err(anyhow!("创建目录失败: {} {}", resp.code, resp.message));
    }
    
    let _ = ctrl.write_all(b"QUIT\r\n");
    Ok(())
}

/// 自定义FTP重命名
fn ftp_rename(address: &str, username: &str, password: &str, from: &str, to: &str) -> Result<()> {
    let (mut ctrl, mut reader) = ftp_login_compat(address, username, password)?;
    
    // RNFR（路径用GBK编码）
    let mut cmd = b"RNFR ".to_vec();
    cmd.extend_from_slice(&encode_path_gbk(from));
    cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&cmd)?;
    
    let rnfr_resp = FtpResponse::read_relaxed(&mut reader)?;
    if rnfr_resp.code != 350 {
        return Err(anyhow!("重命名失败(RNFR): {} {}", rnfr_resp.code, rnfr_resp.message));
    }
    
    // RNTO（路径用GBK编码）
    let mut cmd = b"RNTO ".to_vec();
    cmd.extend_from_slice(&encode_path_gbk(to));
    cmd.extend_from_slice(b"\r\n");
    ctrl.write_all(&cmd)?;
    
    let rnto_resp = FtpResponse::read_relaxed(&mut reader)?;
    if rnto_resp.code != 250 {
        return Err(anyhow!("重命名失败(RNTO): {} {}", rnto_resp.code, rnto_resp.message));
    }
    
    let _ = ctrl.write_all(b"QUIT\r\n");
    Ok(())
}

/// 解析PASV响应
fn parse_pasv_response(response: &str) -> std::io::Result<String> {
    let start = response.find('(').ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid PASV")
    })?;
    let end = response.find(')').ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid PASV")
    })?;
    
    let parts: Vec<u8> = response[start + 1..end]
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    
    if parts.len() != 6 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid PASV"));
    }
    
    let ip = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
    let port = (parts[4] as u16) * 256 + (parts[5] as u16);
    Ok(format!("{}:{}", ip, port))
}
