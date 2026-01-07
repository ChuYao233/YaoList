//! SFTP 驱动实现（基于 ssh2，同步 API 通过线程池/通道桥接异步）

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};
use ssh2::{MethodType, OpenFlags, Session, Sftp};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use crate::storage::{
    Capability, ConfigItem, DriverConfig, DriverFactory, Entry, ProgressCallback, StorageDriver,
};

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
    /// 私钥路径（与密码二选一）
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
    /// SSH 算法首选项（用于兼容部分只支持旧算法的服务器）
    #[serde(default = "default_algo_prefs")]
    pub algo_prefs: AlgoPrefs,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgoPrefs {
    #[serde(default = "default_kex_prefs")]
    pub kex: String,
    #[serde(default = "default_hostkey_prefs")]
    pub hostkey: String,
    #[serde(default = "default_cipher_prefs")]
    pub cipher_c2s: String,
    #[serde(default = "default_cipher_prefs")]
    pub cipher_s2c: String,
    #[serde(default = "default_mac_prefs")]
    pub mac_c2s: String,
    #[serde(default = "default_mac_prefs")]
    pub mac_s2c: String,
}

fn default_algo_prefs() -> AlgoPrefs {
    AlgoPrefs {
        kex: default_kex_prefs(),
        hostkey: default_hostkey_prefs(),
        cipher_c2s: default_cipher_prefs(),
        cipher_s2c: default_cipher_prefs(),
        mac_c2s: default_mac_prefs(),
        mac_s2c: default_mac_prefs(),
    }
}

// 兼容性较宽的算法列表（包含较旧但常见的选项）
fn default_kex_prefs() -> String {
    [
        // 为兼容 OpenSSH_10.0 及其 PQ KEX（mlkem/sntrup），这里显式指定一个「老但安全」的交集。
        // 这些算法同时被 OpenSSH_10.0 和 libssh2 支持，并且会绕过 mlkem/sntrup 的协商路径。
        "ecdh-sha2-nistp256",
        "curve25519-sha256@libssh.org",
        "diffie-hellman-group14-sha256",
    ]
    .join(",")
}

fn default_hostkey_prefs() -> String {
    [
        // 目标服务器提供 ssh-ed25519 / rsa-sha2 / ecdsa-nistp256
        "ssh-ed25519",
        "rsa-sha2-512",
        "rsa-sha2-256",
        "ecdsa-sha2-nistp256",
    ]
    .join(",")
}

fn default_cipher_prefs() -> String {
    [
        // 仅使用 CTR 算法，规避不同平台上 GCM 支持差异带来的潜在问题。
        "aes128-ctr",
        "aes192-ctr",
        "aes256-ctr",
    ]
    .join(",")
}

fn default_mac_prefs() -> String {
    [
        // libssh2 标配支持的 MAC
        "hmac-sha2-256",
        "hmac-sha2-512",
        "hmac-sha1",
    ]
    .join(",")
}

fn default_port() -> u16 {
    22
}

fn default_root() -> String {
    "/".to_string()
}

/// SFTP 驱动
pub struct SftpDriver {
    config: SftpConfig,
}

impl SftpDriver {
    pub fn new(config: SftpConfig) -> Self {
        Self { config }
    }

    /// 计算完整路径（带根目录）
    fn full_path(&self, path: &str) -> String {
        let root = self.config.root_path.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            if root.is_empty() { "/".to_string() } else { root.to_string() }
        } else if root.is_empty() || root == "/" {
            format!("/{}", path)
        } else {
            format!("{}/{}", root, path)
        }
    }

    /// 建立 Session + SFTP
    fn connect(&self) -> Result<(Session, Sftp)> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let tcp = TcpStream::connect(&addr)
            .map_err(|e| anyhow!("SFTP 连接失败: {}", e))?;
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
        tcp.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();

        let mut session = Session::new().map_err(|e| anyhow!("无法创建 SSH 会话: {}", e))?;
        session.set_tcp_stream(tcp);
        // 为兼容 OpenSSH_10.0 默认 PQ KEX，这里显式限制使用一组与 libssh2 有交集的 KEX/算法，
        // 避免出现 “Unable to exchange encryption keys” 的握手失败。
        self.apply_algo_prefs(&session)?;
        session.handshake().map_err(|e| anyhow!("SSH 握手失败: {}", e))?;

        self.verify_host_key(&session)?;
        self.authenticate(&session)?;

        let sftp = session.sftp().map_err(|e| anyhow!("创建 SFTP 会话失败: {}", e))?;
        Ok((session, sftp))
    }

    fn apply_algo_prefs(&self, session: &Session) -> Result<()> {
        let p = &self.config.algo_prefs;

        // 1. 只强制指定 KEX 算法，绕过 OpenSSH_10.0 默认的 PQ KEX，避免握手阶段直接失败。
        let kex = &p.kex;
        session
            .method_pref(MethodType::Kex, kex)
            .map_err(|e| anyhow!("设置 KEX 算法失败: {}", e))?;

        // 2. 其他算法（HostKey / Cipher / MAC）暂时走 libssh2 默认逻辑，
        //    避免在不同平台/构建方式下因支持矩阵差异导致额外问题。

        Ok(())
    }

    /// 过滤掉当前 libssh2 不支持的算法，避免因列表包含未知算法导致握手失败。
    /// 若过滤后列表为空，则回退到 libssh2 默认的支持列表。
    fn filter_supported(session: &Session, ty: MethodType, pref: &str) -> String {
        let supported = session.methods(ty).unwrap_or_default();
        if supported.is_empty() {
            return pref.to_string();
        }
        let supported_set: HashSet<&str> = supported.split(',').collect();
        let filtered: Vec<&str> = pref
            .split(',')
            .filter(|m| supported_set.contains(*m))
            .collect();
        if filtered.is_empty() {
            supported.to_string()
        } else {
            filtered.join(",")
        }
    }

    fn verify_host_key(&self, session: &Session) -> Result<()> {
        if let Some((key, _)) = session.host_key() {
            let mut hasher = Sha256::new();
            hasher.update(key);
            let digest = hasher.finalize();
            let fingerprint_b64 = BASE64_STANDARD.encode(digest);
            let fingerprint_hex = hex::encode(digest);

            if self.config.strict_host_key {
                let expected = self
                    .config
                    .host_fingerprint
                    .as_ref()
                    .ok_or_else(|| anyhow!("已启用严格指纹校验，但未提供 host_fingerprint"))?;
                let expected_norm = expected.replace(':', "").trim().to_string();
                let match_ok = expected_norm.eq_ignore_ascii_case(&fingerprint_hex)
                    || expected_norm == fingerprint_b64;
                if !match_ok {
                    return Err(anyhow!(
                        "主机指纹不匹配，期望: {}，实际: {}",
                        expected,
                        fingerprint_b64
                    ));
                }
            }

            Ok(())
        } else {
            Err(anyhow!("无法获取 SSH 主机指纹"))
        }
    }

    fn authenticate(&self, session: &Session) -> Result<()> {
        if let Some(key) = &self.config.private_key {
            let key_path = Path::new(key);
            session
                .userauth_pubkey_file(
                    &self.config.username,
                    None,
                    key_path,
                    self.config.passphrase.as_deref(),
                )
                .map_err(|e| anyhow!("私钥认证失败: {}", e))?;
        } else if let Some(pass) = &self.config.password {
            session
                .userauth_password(&self.config.username, pass)
                .map_err(|e| anyhow!("密码认证失败: {}", e))?;
        } else {
            return Err(anyhow!("必须提供 password 或 private_key"));
        }

        if !session.authenticated() {
            return Err(anyhow!("SFTP 认证失败"));
        }
        Ok(())
    }

    /// 递归创建目录
    fn ensure_parent_dirs(sftp: &Sftp, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            let mut parts = Vec::new();
            let mut cur = parent.to_path_buf();
            while cur.parent().is_some() && !cur.as_os_str().is_empty() {
                parts.push(cur.clone());
                if let Some(p) = cur.parent() {
                    cur = p.to_path_buf();
                } else {
                    break;
                }
            }
            parts.reverse();
            for p in parts {
                if sftp.stat(&p).is_err() {
                    let _ = sftp.mkdir(&p, 0o755);
                }
            }
        }
        Ok(())
    }

    /// 判断是否目录（通过权限位）
    fn stat_is_dir(stat: &ssh2::FileStat) -> bool {
        const S_IFMT: u32 = 0o170000;
        const S_IFDIR: u32 = 0o040000;
        stat.perm.map(|p| (p & S_IFMT) == S_IFDIR).unwrap_or(false)
    }

    /// 删除文件或目录（递归）
    fn remove_path_recursive(sftp: &Sftp, path: &Path) -> std::io::Result<()> {
        let to_io = |e: ssh2::Error| std::io::Error::new(std::io::ErrorKind::Other, e.to_string());

        let stat = sftp.stat(path).map_err(to_io)?;
        if Self::stat_is_dir(&stat) {
            let entries = sftp.readdir(path).map_err(to_io)?;
            for (child, _) in entries {
                if let Some(name) = child.file_name().and_then(|s| s.to_str()) {
                    if name == "." || name == ".." {
                        continue;
                    }
                }
                Self::remove_path_recursive(sftp, &child)?;
            }
            sftp.rmdir(path).map_err(to_io)
        } else {
            sftp.unlink(path).map_err(to_io)
        }
    }
}

#[async_trait]
impl StorageDriver for SftpDriver {
    fn name(&self) -> &str {
        "SFTP"
    }

    fn version(&self) -> &str {
        "1.0.0"
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
            requires_full_file_for_upload: false, // SFTP支持流式写入
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let full_path = self.full_path(path);
        let path_string = path.to_string();
        let config = self.config.clone();

        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<Entry>> {
            let (_session, sftp) = SftpDriver::new(config).connect()?;
            let mut result = Vec::new();
            for (child, stat) in sftp
                .readdir(Path::new(&full_path))
                .map_err(|e| anyhow!("SFTP 列目录失败: {}", e))?
            {
                let name = match child.file_name().and_then(|s| s.to_str()) {
                    Some(n) if n != "." && n != ".." => n.to_string(),
                    _ => continue,
                };

                let is_dir = SftpDriver::stat_is_dir(&stat);
                let size = stat.size.unwrap_or(0);
                let modified = stat.mtime.and_then(|ts| {
                    chrono::NaiveDateTime::from_timestamp_opt(ts as i64, 0)
                        .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc).to_rfc3339())
                });

                let entry_path = if path_string.is_empty() || path_string == "/" {
                    format!("/{}", name)
                } else {
                    format!("{}/{}", path_string.trim_end_matches('/'), name)
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
        })
        .await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;

        Ok(entries)
    }

    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
        let full_path = self.full_path(path);
        let config = self.config.clone();
        let range_clone = range.clone();

        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, std::io::Error>>(8);
        std::thread::spawn(move || {
            let result = (|| -> Result<(), std::io::Error> {
                let (_session, sftp) = SftpDriver::new(config)
                    .connect()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

                let mut file = sftp
                    .open(Path::new(&full_path))
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

                let mut remaining = range_clone.as_ref().map(|r| r.end.saturating_sub(r.start));
                if let Some(r) = range_clone {
                    file.seek(SeekFrom::Start(r.start))?;
                }

                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let read_len = if let Some(rem) = remaining {
                        if rem == 0 {
                            break;
                        }
                        buf.len().min(rem as usize)
                    } else {
                        buf.len()
                    };
                    match file.read(&mut buf[..read_len]) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Some(rem) = remaining.as_mut() {
                                *rem = rem.saturating_sub(n as u64);
                            }
                            if tx.blocking_send(Ok(buf[..n].to_vec())).is_err() {
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
                tracing::error!("SFTP reader error: {}", e);
                let _ = tx.blocking_send(Err(e));
            }
        });

        Ok(Box::new(SftpStreamReader::new(rx)))
    }

    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let full_path = self.full_path(path);
        let config = self.config.clone();

        Ok(Box::new(SftpStreamWriter::new(
            config,
            full_path,
            size_hint,
            progress,
        )?))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let full_path = self.full_path(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let (_session, sftp) = SftpDriver::new(config).connect()?;
            Self::remove_path_recursive(&sftp, Path::new(&full_path))
                .map_err(|e| anyhow!("SFTP 删除失败: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;

        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let full_path = self.full_path(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let (_session, sftp) = SftpDriver::new(config).connect()?;
            let p = Path::new(&full_path);
            if sftp.stat(p).is_err() {
                Self::ensure_parent_dirs(&sftp, p)?;
                sftp.mkdir(p, 0o755)
                    .map_err(|e| anyhow!("创建目录失败: {}", e))?;
            }
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;

        Ok(())
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        let full_path = self.full_path(path);
        let config = self.config.clone();
        let new_path = Path::new(&full_path)
            .parent()
            .map(|p| p.join(new_name))
            .ok_or_else(|| anyhow!("无法获取父目录"))?;

        tokio::task::spawn_blocking(move || -> Result<()> {
            let (_session, sftp) = SftpDriver::new(config).connect()?;
            sftp.rename(Path::new(&full_path), &new_path, None)
                .map_err(|e| anyhow!("SFTP 重命名失败: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;

        Ok(())
    }

    async fn move_item(&self, from: &str, to: &str) -> Result<()> {
        let from_full = self.full_path(from);
        let to_full = self.full_path(to);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let (_session, sftp) = SftpDriver::new(config).connect()?;
            let to_path = Path::new(&to_full);
            Self::ensure_parent_dirs(&sftp, to_path)?;
            sftp.rename(Path::new(&from_full), to_path, None)
                .map_err(|e| anyhow!("SFTP 移动失败: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("任务执行失败: {}", e))??;

        Ok(())
    }

    async fn get_direct_link(&self, _path: &str) -> Result<Option<String>> {
        // SFTP 无直链概念，只能通过代理
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

/// SFTP 异步读取器（channel 桥接）
struct SftpStreamReader {
    rx: mpsc::Receiver<Result<Vec<u8>, std::io::Error>>,
    buffer: Vec<u8>,
    pos: usize,
    done: bool,
    error: Option<std::io::Error>,
}

impl SftpStreamReader {
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

impl AsyncRead for SftpStreamReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(e) = self.error.take() {
            return Poll::Ready(Err(e));
        }

        if self.pos < self.buffer.len() {
            let remaining = &self.buffer[self.pos..];
            let to_read = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_read]);
            self.pos += to_read;
            return Poll::Ready(Ok(()));
        }

        if self.done {
            return Poll::Ready(Ok(()));
        }

        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(data))) => {
                self.buffer = data;
                self.pos = 0;
                let remaining = &self.buffer[self.pos..];
                let to_read = remaining.len().min(buf.remaining());
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
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// SFTP 异步写入器（channel 桥接）
struct SftpStreamWriter {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    handle: Option<std::thread::JoinHandle<std::io::Result<u64>>>, // 返回实际写入的字节数
    join_task: Option<tokio::task::JoinHandle<std::io::Result<u64>>>,
    closed: bool,
    size_hint: Option<u64>, // 用于验证写入完整性
    written_bytes: u64, // 跟踪已发送到 channel 的字节数
    pending_send: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>>> + Send>>>, // 待发送的 future
}

impl SftpStreamWriter {
    fn new(
        config: SftpConfig,
        full_path: String,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Self> {
        // 增加 channel 容量，减少阻塞（从 32 增加到 128）
        // 这样可以缓冲更多数据，避免频繁阻塞和 panic
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(128);
        let progress_cb = progress.clone();

        let handle = std::thread::spawn(move || -> std::io::Result<u64> {
            let (_session, sftp) = SftpDriver::new(config)
                .connect()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            let path = PathBuf::from(&full_path);
            SftpDriver::ensure_parent_dirs(&sftp, &path)?;

            let mut file = sftp
                .open_mode(
                    &path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                    0o644,
                    ssh2::OpenType::File,
                )
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            let mut written: u64 = 0;
            loop {
                match rx.blocking_recv() {
                    Some(chunk) => {
                        let chunk_len = chunk.len();
                        file.write_all(&chunk).map_err(|e| {
                            tracing::error!("SFTP writer: 写入失败: {} (已写入: {} bytes)", e, written);
                            std::io::Error::new(std::io::ErrorKind::Other, format!("写入失败: {}", e))
                        })?;
                        written += chunk_len as u64;
                        if let Some(cb) = progress_cb.as_ref() {
                            cb(written, size_hint.unwrap_or(written));
                        }
                        tracing::trace!("SFTP writer: 写入分片: {} bytes, 总计: {} bytes", chunk_len, written);
                    }
                    None => {
                        // channel 已关闭，所有数据已接收完成
                        tracing::debug!("SFTP writer: 所有数据已接收，开始 flush，已写入: {} bytes (期望: {:?})", 
                            written, size_hint);
                        break;
                    }
                }
            }
            // 确保所有数据都写入并刷新
            file.flush().map_err(|e| {
                tracing::error!("SFTP writer: flush 失败: {} (已写入: {} bytes)", e, written);
                std::io::Error::new(std::io::ErrorKind::Other, format!("flush 失败: {}", e))
            })?;
            tracing::debug!("SFTP writer: flush 完成，文件写入成功，总计: {} bytes (期望: {:?})", 
                written, size_hint);
            Ok(written)
        });

        Ok(Self { 
            tx: Some(tx), 
            handle: Some(handle), 
            join_task: None,
            closed: false,
            size_hint,
            written_bytes: 0,
            pending_send: None,
        })
    }
}

impl AsyncWrite for SftpStreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.closed {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "writer already closed",
            )));
        }
        
        // 如果有待发送的数据，先处理它
        if let Some(mut send_fut) = self.pending_send.take() {
            match send_fut.as_mut().poll(cx) {
                Poll::Ready(Ok(_)) => {
                    // 之前的发送成功，现在可以发送新数据
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("SFTP 发送失败: {}", e),
                    )));
                }
                Poll::Pending => {
                    // 之前的发送还在等待，保存 future 并返回 Pending
                    self.pending_send = Some(send_fut);
                    return Poll::Pending;
                }
            }
        }
        
        // 克隆 tx 以避免借用检查问题
        let tx = self.tx.as_ref().expect("sender should exist").clone();
        let buf_len = buf.len();
        let data = buf.to_vec();

        match tx.try_send(data) {
            Ok(_) => {
                self.written_bytes += buf_len as u64;
                Poll::Ready(Ok(buf_len))
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(data)) => {
                // channel 满，使用异步 send 等待，避免阻塞 Tokio 运行时
                // 使用 async move 确保 future 拥有 tx 和 data 的所有权
                let send_fut = async move {
                    tx.send(data).await
                };
                let mut boxed_fut: Pin<Box<dyn std::future::Future<Output = Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>>> + Send>> = Box::pin(send_fut);
                
                match boxed_fut.as_mut().poll(cx) {
                    Poll::Ready(Ok(_)) => {
                        self.written_bytes += buf_len as u64;
                        Poll::Ready(Ok(buf_len))
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("SFTP 发送失败: {}", e),
                    ))),
                    Poll::Pending => {
                        // 保存 future 以便下次继续轮询
                        self.pending_send = Some(boxed_fut);
                        Poll::Pending
                    }
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Poll::Ready(Err(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "channel closed"),
            )),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.closed {
            self.closed = true;
            tracing::debug!("SFTP writer: 开始 shutdown，关闭 channel");
            if let Some(tx) = self.tx.take() {
                drop(tx); // dropping sender closes the channel
            }
            if let Some(handle) = self.handle.take() {
                tracing::debug!("SFTP writer: 启动 join task 等待线程完成");
                // 使用 spawn_blocking 异步等待线程完成，避免阻塞 Tokio 运行时
                let size_hint = self.size_hint;
                let written_bytes = self.written_bytes;
                let join_task = tokio::task::spawn_blocking(move || {
                    tracing::debug!("SFTP writer: join task 开始等待线程 (已发送: {} bytes, 期望: {:?})", 
                        written_bytes, size_hint);
                    match handle.join() {
                        Ok(result) => {
                            match &result {
                                Ok(actual_written) => {
                                    tracing::debug!("SFTP writer: 线程完成，实际写入: {} bytes (期望: {:?}, 已发送: {} bytes)", 
                                        actual_written, size_hint, written_bytes);
                                }
                                Err(e) => {
                                    tracing::error!("SFTP writer: 线程返回错误: {} (已发送: {} bytes, 期望: {:?})", 
                                        e, written_bytes, size_hint);
                                }
                            }
                            result
                        }
                        Err(e) => {
                            tracing::error!("SFTP writer join error: {:?} (已发送: {} bytes, 期望: {:?})", 
                                e, written_bytes, size_hint);
                            Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "writer thread panicked",
                            ))
                        }
                    }
                });
                self.join_task = Some(join_task);
            }
        }
        
        // 如果有等待任务，检查是否完成
        if let Some(ref mut join_task) = self.join_task {
            match Pin::new(join_task).poll(cx) {
                Poll::Ready(Ok(result)) => {
                    self.join_task = None;
                    // 验证写入完整性
                    match result {
                        Ok(actual_written) => {
                            // 验证实际写入的字节数
                            if let Some(expected_size) = self.size_hint {
                                if expected_size > 0 {
                                    if actual_written < expected_size {
                                        // 实际写入小于期望大小，这是真正的错误（写入不完整）
                                        tracing::error!(
                                            "SFTP writer: 写入不完整！实际写入: {} bytes, 期望: {} bytes, 已发送: {} bytes",
                                            actual_written, expected_size, self.written_bytes
                                        );
                                        return Poll::Ready(Err(std::io::Error::new(
                                            std::io::ErrorKind::UnexpectedEof,
                                            format!("写入不完整: 实际 {} bytes, 期望 {} bytes", actual_written, expected_size),
                                        )));
                                    } else if actual_written > expected_size {
                                        // 实际写入大于期望大小，可能是前端传递的大小不准确，警告但不报错
                                        tracing::warn!(
                                            "SFTP writer: 实际写入 ({}) 大于期望大小 ({})，可能是前端传递的大小不准确 (已发送: {} bytes)",
                                            actual_written, expected_size, self.written_bytes
                                        );
                                    }
                                }
                            }
                            // 验证已发送的字节数是否与实际写入一致（允许一些差异，因为可能有缓冲）
                            if actual_written > self.written_bytes + 1024 * 1024 {
                                // 如果实际写入比已发送多超过 1MB，可能是问题
                                tracing::warn!(
                                    "SFTP writer: 实际写入 ({}) 比已发送 ({}) 多很多，可能存在异常",
                                    actual_written, self.written_bytes
                                );
                            }
                            tracing::debug!("SFTP writer: shutdown 完成，验证通过 (实际写入: {} bytes)", actual_written);
                            Poll::Ready(Ok(()))
                        }
                        Err(e) => {
                            tracing::error!("SFTP writer: shutdown 失败: {} (已发送: {} bytes, 期望: {:?})", 
                                e, self.written_bytes, self.size_hint);
                            Poll::Ready(Err(e))
                        }
                    }
                }
                Poll::Ready(Err(e)) => {
                    self.join_task = None;
                    tracing::error!("SFTP writer join task error: {:?} (已发送: {} bytes, 期望: {:?})", 
                        e, self.written_bytes, self.size_hint);
                    Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("join task failed: {}", e),
                    )))
                }
                Poll::Pending => {
                    tracing::debug!("SFTP writer: shutdown 等待中... (已发送: {} bytes, 期望: {:?})", 
                        self.written_bytes, self.size_hint);
                    Poll::Pending
                }
            }
        } else {
            Poll::Ready(Ok(()))
        }
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

