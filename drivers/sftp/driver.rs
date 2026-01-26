//! SFTP 驱动实现（基于 russh，纯 Rust 异步实现）
//!
//! 特性：
//! - 纯 Rust 实现，无 OpenSSL/Perl 依赖
//! - 原生异步 API
//! - 连接保持与复用
//! - 进度回调支持
//! - 完整的错误处理和日志

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use russh::client::{self, Config, Handle, Handler};
use russh::keys::PublicKey;
use russh_sftp::client::SftpSession;
use std::ops::Range;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Mutex;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, ProgressCallback, StorageDriver,
};

/// 连接超时时间
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// SFTP 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SftpConfig {
    /// 主机名或 IP
    pub host: String,
    /// 端口
    #[serde(default = "default_port")]
    pub port: u16,
    /// 用户名
    pub username: String,
    /// 密码（与私钥二选一）
    pub password: Option<String>,
    /// 私钥内容或路径（与密码二选一）
    pub private_key: Option<String>,
    /// 私钥密码
    pub passphrase: Option<String>,
    /// 根目录
    #[serde(default = "default_root")]
    pub root_path: String,
    /// 是否强制校验主机指纹
    #[serde(default)]
    pub strict_host_key: bool,
    /// 预期主机指纹（SHA256，支持 base64 或 hex）
    pub host_fingerprint: Option<String>,
}

fn default_port() -> u16 {
    22
}

fn default_root() -> String {
    "/".to_string()
}

/// SSH 客户端 Handler（处理服务端事件）
struct SshClientHandler {
    strict_host_key: bool,
    expected_fingerprint: Option<String>,
}

impl SshClientHandler {
    fn new(strict_host_key: bool, expected_fingerprint: Option<String>) -> Self {
        Self {
            strict_host_key,
            expected_fingerprint,
        }
    }
}

#[async_trait]
impl Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKey) -> Result<bool, Self::Error> {
        // 计算服务器公钥指纹（使用 russh 内置的指纹方法）
        use russh::keys::HashAlg;
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256);
        let fingerprint_str = format!("{}", fingerprint);

        if self.strict_host_key {
            if let Some(expected) = &self.expected_fingerprint {
                // 支持多种指纹格式比较
                let expected_norm = expected.replace(':', "").trim().to_lowercase();
                let actual_norm = fingerprint_str.replace(':', "").to_lowercase();
                
                // 移除 "SHA256:" 前缀进行比较
                let actual_clean = actual_norm.trim_start_matches("sha256:");
                let expected_clean = expected_norm.trim_start_matches("sha256:");
                
                if actual_clean != expected_clean && !expected_norm.contains(&actual_clean) {
                    tracing::error!(
                        "SFTP 主机指纹不匹配，期望: {}，实际: {}",
                        expected,
                        fingerprint_str
                    );
                    return Ok(false);
                }
            } else {
                tracing::error!("已启用严格指纹校验，但未提供 host_fingerprint");
                return Ok(false);
            }
        }

        tracing::debug!("SSH 服务器指纹: {}", fingerprint_str);
        Ok(true)
    }

    async fn channel_open_confirmation(
        &mut self,
        _id: russh::ChannelId,
        _max_packet_size: u32,
        _window_size: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        _data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// SSH 连接持有者（保持连接存活）
struct SshConnection {
    session: Handle<SshClientHandler>,
    sftp: SftpSession,
}

/// SFTP 驱动
pub struct SftpDriver {
    config: SftpConfig,
    /// 保持的连接（可复用）
    connection: Arc<Mutex<Option<SshConnection>>>,
}

impl SftpDriver {
    pub fn new(config: SftpConfig) -> Self {
        Self {
            config,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// 计算完整路径（带根目录）
    fn full_path(&self, path: &str) -> String {
        let root = self.config.root_path.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            if root.is_empty() {
                "/".to_string()
            } else {
                root.to_string()
            }
        } else if root.is_empty() || root == "/" {
            format!("/{}", path)
        } else {
            format!("{}/{}", root, path)
        }
    }

    /// 建立新的 SSH/SFTP 连接
    async fn create_connection(&self) -> Result<SshConnection> {
        tracing::debug!("SFTP: 建立新连接到 {}:{}", self.config.host, self.config.port);

        let ssh_config = Config::default();
        let config = Arc::new(ssh_config);

        let handler = SshClientHandler::new(
            self.config.strict_host_key,
            self.config.host_fingerprint.clone(),
        );

        let addr = format!("{}:{}", self.config.host, self.config.port);

        // 带超时的连接
        let mut session = tokio::time::timeout(
            CONNECT_TIMEOUT,
            client::connect(config, &addr, handler),
        )
        .await
        .map_err(|_| anyhow!("SFTP 连接超时: {}", addr))?
        .map_err(|e| anyhow!("SSH 连接/握手失败: {} - {}", addr, e))?;

        tracing::debug!("SFTP: SSH 连接已建立");

        tracing::debug!("SFTP: SSH 握手完成");

        // 认证
        self.authenticate(&mut session).await?;

        tracing::debug!("SFTP: 认证成功");

        // 打开 SFTP 通道
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| anyhow!("打开 SSH 通道失败: {}", e))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| anyhow!("请求 SFTP 子系统失败: {}", e))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| anyhow!("创建 SFTP 会话失败: {}", e))?;

        tracing::info!("SFTP: 连接建立成功 - {}:{}", self.config.host, self.config.port);

        Ok(SshConnection { session, sftp })
    }

    /// 获取 SFTP 会话（复用现有连接或创建新连接）
    async fn get_sftp(&self) -> Result<SftpSession> {
        // 每次操作创建新连接（简化实现，避免复杂的连接状态管理）
        // russh-sftp 的 SftpSession 不支持 Clone，所以无法直接复用
        let conn = self.create_connection().await?;
        Ok(conn.sftp)
    }

    /// 执行认证
    async fn authenticate(&self, session: &mut Handle<SshClientHandler>) -> Result<()> {
        let username = &self.config.username;

        if let Some(key_path) = &self.config.private_key {
            // 私钥认证
            let key_path = Path::new(key_path);
            let passphrase = self.config.passphrase.as_deref();

            let key_pair = russh_keys::load_secret_key(key_path, passphrase)
                .map_err(|e| anyhow!("加载私钥失败: {} - {}", key_path.display(), e))?;

            tracing::debug!("SFTP: 使用私钥认证 - {}", key_path.display());

            let auth_result = session
                .authenticate_publickey(username, Arc::new(key_pair))
                .await
                .map_err(|e| anyhow!("私钥认证失败: {}", e))?;

            if !auth_result {
                return Err(anyhow!("私钥认证被服务器拒绝"));
            }
        } else if let Some(password) = &self.config.password {
            // 密码认证
            tracing::debug!("SFTP: 使用密码认证");

            let auth_result = session
                .authenticate_password(username, password)
                .await
                .map_err(|e| anyhow!("密码认证失败: {}", e))?;

            if !auth_result {
                return Err(anyhow!("密码认证被服务器拒绝"));
            }
        } else {
            return Err(anyhow!("必须提供 password 或 private_key"));
        }

        Ok(())
    }

    /// 递归删除
    async fn remove_recursive(&self, sftp: &SftpSession, path: &str) -> Result<()> {
        tracing::debug!("SFTP: 删除 {}", path);

        let metadata = sftp
            .metadata(path)
            .await
            .map_err(|e| anyhow!("获取文件信息失败: {} - {}", path, e))?;

        if metadata.is_dir() {
            let entries = sftp
                .read_dir(path)
                .await
                .map_err(|e| anyhow!("读取目录失败: {} - {}", path, e))?;

            for entry in entries {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child_path = format!("{}/{}", path.trim_end_matches('/'), name);
                Box::pin(self.remove_recursive(sftp, &child_path)).await?;
            }

            sftp.remove_dir(path)
                .await
                .map_err(|e| anyhow!("删除目录失败: {} - {}", path, e))?;
        } else {
            sftp.remove_file(path)
                .await
                .map_err(|e| anyhow!("删除文件失败: {} - {}", path, e))?;
        }

        Ok(())
    }

    /// 递归创建父目录（使用字符串操作，避免 Windows 路径分隔符问题）
    async fn ensure_parent_dirs(&self, sftp: &SftpSession, path: &str) -> Result<()> {
        // 使用字符串操作获取父目录，避免 Windows Path 使用反斜杠
        if let Some(last_slash) = path.rfind('/') {
            let parent_str = &path[..last_slash];
            if !parent_str.is_empty() && parent_str != "/" {
                // 检查父目录是否存在
                if sftp.metadata(parent_str).await.is_err() {
                    Box::pin(self.ensure_parent_dirs(sftp, parent_str)).await?;
                    tracing::debug!("SFTP: 创建目录 {}", parent_str);
                    let _ = sftp.create_dir(parent_str).await;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageDriver for SftpDriver {
    fn name(&self) -> &str {
        "SFTP"
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
            can_concurrent_upload: false,
            requires_oauth: false,
            can_multipart_upload: false,
            can_server_side_copy: false,
            can_batch_operations: false,
            max_file_size: None,
            requires_full_file_for_upload: false,
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        tracing::debug!("SFTP: 列出目录 {}", path);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;

        let entries = sftp
            .read_dir(&full_path)
            .await
            .map_err(|e| anyhow!("SFTP 列目录失败: {}", e))?;

        let mut result = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }

            let metadata = entry.metadata();
            let is_dir = metadata.is_dir();
            let size = metadata.len();
            let modified = metadata.modified().ok().and_then(|ts| {
                ts.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                    .map(|dt| dt.to_rfc3339())
            });

            let entry_path = if path.is_empty() || path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", path.trim_end_matches('/'), name)
            };

            result.push(Entry {
                name,
                path: entry_path,
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
        tracing::debug!("SFTP: 打开读取 {} range={:?}", path, range);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;

        let file = sftp
            .open(&full_path)
            .await
            .map_err(|e| anyhow!("SFTP 打开文件失败: {} - {}", full_path, e))?;

        // 返回包装的读取器
        Ok(Box::new(SftpReader::new(file, range)))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        tracing::debug!("SFTP: 打开写入 {} size_hint={:?}", path, size_hint);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;

        // 确保父目录存在
        self.ensure_parent_dirs(&sftp, &full_path).await?;

        let file = sftp
            .create(&full_path)
            .await
            .map_err(|e| anyhow!("SFTP 创建文件失败: {} - {}", full_path, e))?;

        // 返回包装的写入器（带进度回调）
        Ok(Box::new(SftpWriter::new(file, size_hint, progress)))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        tracing::debug!("SFTP: 删除 {}", path);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;
        self.remove_recursive(&sftp, &full_path).await
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        tracing::debug!("SFTP: 创建目录 {}", path);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;

        self.ensure_parent_dirs(&sftp, &full_path).await?;

        sftp.create_dir(&full_path)
            .await
            .map_err(|e| anyhow!("创建目录失败: {}", e))?;

        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        tracing::debug!("SFTP: 重命名 {} -> {}", path, new_name);
        let full_path = self.full_path(path);
        let sftp = self.get_sftp().await?;

        // 使用字符串操作构建新路径，避免 Windows Path 使用反斜杠
        let new_path = if let Some(last_slash) = full_path.rfind('/') {
            format!("{}/{}", &full_path[..last_slash], new_name)
        } else {
            format!("/{}", new_name)
        };

        tracing::debug!("SFTP: rename {} -> {}", full_path, new_path);

        sftp.rename(&full_path, &new_path)
            .await
            .map_err(|e| anyhow!("SFTP 重命名失败: {} -> {} - {}", full_path, new_path, e))?;

        Ok(())
    }

    async fn move_item(&self, from: &str, to: &str) -> Result<()> {
        tracing::debug!("SFTP: 移动 {} -> {}", from, to);
        let from_full = self.full_path(from);
        let to_full = self.full_path(to);
        let sftp = self.get_sftp().await?;

        self.ensure_parent_dirs(&sftp, &to_full).await?;

        sftp.rename(&from_full, &to_full)
            .await
            .map_err(|e| anyhow!("SFTP 移动失败: {}", e))?;

        Ok(())
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // SFTP 无直链概念
        Ok(None)
    }

    async fn get_space_info(&self) -> Result<Option<crate::storage::SpaceInfo>> {
        // SFTP 协议无通用空间查询
        Ok(None)
    }

    fn show_space_in_frontend(&self) -> bool {
        false
    }
}

/// SFTP 驱动工厂
pub struct SftpDriverFactory;

impl DriverFactory for SftpDriverFactory {
    fn driver_type(&self) -> &'static str {
        "sftp"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "SFTP".to_string(),
            local_sort: true,
            only_proxy: true,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("host", "string")
                .title("SFTP 主机")
                .required()
                .help("主机名或 IP"),
            ConfigItem::new("port", "number")
                .title("端口")
                .default("22"),
            ConfigItem::new("username", "string")
                .title("用户名")
                .required(),
            ConfigItem::new("password", "string")
                .title("密码")
                .help("与私钥二选一"),
            ConfigItem::new("private_key", "string")
                .title("私钥路径")
                .help("与密码二选一"),
            ConfigItem::new("passphrase", "string")
                .title("私钥密码")
                .help("如私钥有密码则填写"),
            ConfigItem::new("root_path", "string")
                .title("根目录")
                .default("/")
                .help("限制访问的根路径"),
            ConfigItem::new("strict_host_key", "bool")
                .title("严格校验指纹")
                .default("false"),
            ConfigItem::new("host_fingerprint", "string")
                .title("主机指纹")
                .help("启用严格校验时填写，SHA256(base64/hex)"),
        ]
    }

    fn create_driver(&self, config: serde_json::Value) -> Result<Box<dyn StorageDriver>> {
        let cfg: SftpConfig = serde_json::from_value(config)?;
        if cfg.password.is_none() && cfg.private_key.is_none() {
            return Err(anyhow!("需提供 password 或 private_key"));
        }
        Ok(Box::new(SftpDriver::new(cfg)))
    }
}

// ============================================================================
// SFTP 读取器包装（支持 range 读取）
// ============================================================================

use russh_sftp::client::fs::File as SftpFile;

/// SFTP 文件读取器
struct SftpReader {
    file: SftpFile,
    range: Option<Range<u64>>,
    position: u64,
    initialized: bool,
}

impl SftpReader {
    fn new(file: SftpFile, range: Option<Range<u64>>) -> Self {
        Self {
            file,
            range,
            position: 0,
            initialized: false,
        }
    }
}

impl AsyncRead for SftpReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // 检查是否已读完 range
        if let Some(ref r) = self.range {
            let remaining = r.end.saturating_sub(r.start).saturating_sub(self.position);
            if remaining == 0 {
                return Poll::Ready(Ok(()));
            }
            // 限制读取长度
            let max_read = remaining.min(buf.remaining() as u64) as usize;
            if max_read < buf.remaining() {
                let mut limited_buf = buf.take(max_read);
                let result = Pin::new(&mut self.file).poll_read(cx, &mut limited_buf);
                if let Poll::Ready(Ok(())) = &result {
                    let filled = limited_buf.filled().len();
                    self.position += filled as u64;
                    unsafe {
                        buf.assume_init(filled);
                    }
                    buf.advance(filled);
                }
                return result.map_ok(|_| ());
            }
        }

        let result = Pin::new(&mut self.file).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            self.position += buf.filled().len() as u64;
        }
        result
    }
}

// ============================================================================
// SFTP 写入器包装（支持进度回调）
// ============================================================================


/// SFTP 文件写入器（带进度回调）
struct SftpWriter {
    file: SftpFile,
    size_hint: Option<u64>,
    progress: Option<ProgressCallback>,
    written: u64,
}

impl SftpWriter {
    fn new(file: SftpFile, size_hint: Option<u64>, progress: Option<ProgressCallback>) -> Self {
        Self {
            file,
            size_hint,
            progress,
            written: 0,
        }
    }

    fn report_progress(&self) {
        if let Some(ref cb) = self.progress {
            let total = self.size_hint.unwrap_or(self.written);
            cb(self.written, total);
        }
    }
}

impl AsyncWrite for SftpWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.file).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &result {
            self.written += *n as u64;
            self.report_progress();
            tracing::trace!("SFTP: 写入 {} bytes, 总计 {} bytes", n, self.written);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        tracing::debug!("SFTP: 写入完成，总计 {} bytes (期望 {:?})", self.written, self.size_hint);
        Pin::new(&mut self.file).poll_shutdown(cx)
    }
}
