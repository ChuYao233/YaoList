//! Download control module / 下载控制模块
//! 
//! This module handles:
//! - Download domain validation / 下载域名验证
//! - Proxy bandwidth limiting (for local proxy streams) / 代理带宽限制(用于本地代理流)
//! - Concurrent connection limiting / 并发连接限制
//!
//! Note: This is Core layer logic, not Driver layer.
//! 注意: 这是 Core 层逻辑，不是 Driver 层。
//!
//! Bandwidth limiting is applied to streaming proxy downloads (FTP, Cloud189, etc.)
//! using async stream wrappers, NOT by loading files into memory.
//! 带宽限制应用于流式代理下载(FTP、天翼云盘等)，使用异步流包装器，而不是将文件加载到内存。

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicI32, Ordering};
use parking_lot::RwLock;
use sqlx::SqlitePool;

/// Download settings cache / 下载设置缓存
/// 
/// Cached in memory for performance, updated when settings change.
/// 缓存在内存中以提高性能，设置变更时更新。
pub struct DownloadSettings {
    /// Download domain, empty means use current domain / 下载域名，空表示使用当前域名
    download_domain: RwLock<String>,
    /// Max proxy speed in bytes/sec, 0 = unlimited / 最大代理速度(字节/秒)，0表示无限制
    max_speed: AtomicI64,
    /// Max concurrent proxy connections, 0 = unlimited / 最大代理并发数，0表示无限制
    max_concurrent: AtomicI32,
    /// Current concurrent connections / 当前并发连接数
    current_concurrent: AtomicI32,
    /// Global bandwidth limiter for all proxy downloads (shared) / 所有代理下载共享的全局带宽限制器
    global_limiter: Arc<BandwidthLimiter>,
    /// Download link expiry in minutes (default 15) / 下载链接有效期（分钟，默认15）
    link_expiry_minutes: AtomicI32,
}

impl DownloadSettings {
    pub fn new() -> Self {
        Self {
            download_domain: RwLock::new(String::new()),
            max_speed: AtomicI64::new(0),
            max_concurrent: AtomicI32::new(0),
            current_concurrent: AtomicI32::new(0),
            global_limiter: Arc::new(BandwidthLimiter::new(0)),
            link_expiry_minutes: AtomicI32::new(15),  // Default 15 minutes / 默认15分钟
        }
    }

    /// Load settings from database / 从数据库加载设置
    pub async fn load_from_db(&self, db: &SqlitePool) -> Result<(), String> {
        // Load download_domain / 加载下载域名
        let domain: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM site_settings WHERE key = 'download_domain'"
        )
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
        
        if let Some((d,)) = domain {
            *self.download_domain.write() = d;
        }

        // Load proxy_max_speed / 加载代理最大速度
        let speed: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM site_settings WHERE key = 'proxy_max_speed'"
        )
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
        
        if let Some((s,)) = speed {
            // Database stores bytes/s (same as frontend sends) / 数据库存储的是 bytes/s（与前端发送的一致）
            let bytes_per_sec: i64 = s.parse().unwrap_or(0);
            tracing::info!("load_from_db: proxy_max_speed={}bytes/s ({}MB/s)", bytes_per_sec, bytes_per_sec / 1024 / 1024);
            self.max_speed.store(bytes_per_sec, Ordering::SeqCst);
            self.global_limiter.set_rate(bytes_per_sec);
        }

        // Load proxy_max_concurrent / 加载代理最大并发数
        let concurrent: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM site_settings WHERE key = 'proxy_max_concurrent'"
        )
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
        
        if let Some((c,)) = concurrent {
            self.max_concurrent.store(c.parse().unwrap_or(0), Ordering::SeqCst);
        }

        // Load link_expiry_minutes / 加载链接有效期（分钟）
        let expiry: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM site_settings WHERE key = 'link_expiry_minutes'"
        )
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?;
        
        if let Some((e,)) = expiry {
            let minutes: i32 = e.parse().unwrap_or(15);
            self.link_expiry_minutes.store(if minutes > 0 { minutes } else { 15 }, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Update download domain / 更新下载域名
    pub fn set_download_domain(&self, domain: String) {
        *self.download_domain.write() = domain;
    }

    /// Update max speed (bytes/s) / 更新最大速度(bytes/s)
    /// Note: Frontend sends bytes/s, so no conversion needed
    /// 注意：前端发送的是 bytes/s，无需转换
    pub fn set_max_speed(&self, bytes_per_sec: i64) {
        tracing::info!("set_max_speed: {}bytes/s ({}MB/s)", bytes_per_sec, bytes_per_sec / 1024 / 1024);
        self.max_speed.store(bytes_per_sec, Ordering::SeqCst);
        self.global_limiter.set_rate(bytes_per_sec);
    }

    /// Update max concurrent / 更新最大并发数
    pub fn set_max_concurrent(&self, concurrent: i32) {
        self.max_concurrent.store(concurrent, Ordering::SeqCst);
    }

    /// Get configured download domain / 获取配置的下载域名
    pub fn get_download_domain(&self) -> String {
        self.download_domain.read().clone()
    }

    /// Get max speed in bytes/sec / 获取最大速度(字节/秒)
    pub fn get_max_speed(&self) -> i64 {
        self.max_speed.load(Ordering::SeqCst)
    }

    /// Get max concurrent / 获取最大并发数
    pub fn get_max_concurrent(&self) -> i32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    /// Get link expiry in minutes / 获取链接有效期（分钟）
    pub fn get_link_expiry_minutes(&self) -> i32 {
        self.link_expiry_minutes.load(Ordering::SeqCst)
    }

    /// Set link expiry in minutes / 设置链接有效期（分钟）
    pub fn set_link_expiry_minutes(&self, minutes: i32) {
        self.link_expiry_minutes.store(if minutes > 0 { minutes } else { 15 }, Ordering::SeqCst);
    }

    /// Get global bandwidth limiter for proxy downloads (shared) / 获取代理下载的全局带宽限制器（共享）
    pub fn get_limiter(&self) -> Arc<BandwidthLimiter> {
        self.global_limiter.clone()
    }

    /// Consume bandwidth from global limiter / 从全局限制器消耗带宽
    /// Returns the number of bytes that can be sent / 返回可以发送的字节数
    pub async fn consume_bandwidth(&self, requested: i64) -> i64 {
        self.global_limiter.consume(requested).await
    }

    /// Try consume bandwidth without waiting / 尝试消耗带宽（不等待）
    pub fn try_consume_bandwidth(&self, requested: i64) -> i64 {
        self.global_limiter.try_consume(requested)
    }

    /// Validate request domain against configured download domain
    /// 验证请求域名是否匹配配置的下载域名
    /// 
    /// Returns true if:
    /// - No download domain is configured (empty) - use any domain
    /// - Request domain matches configured download domain
    /// 
    /// 返回 true 如果:
    /// - 未配置下载域名(空) - 使用任意域名
    /// - 请求域名匹配配置的下载域名
    pub fn validate_domain(&self, request_host: &str) -> bool {
        let configured = self.download_domain.read();
        if configured.is_empty() {
            return true;
        }
        
        // Normalize: remove port and protocol / 规范化: 移除端口和协议
        let request_domain = Self::normalize_domain(request_host);
        let configured_domain = Self::normalize_domain(&configured);
        
        request_domain == configured_domain
    }

    /// Normalize domain: remove protocol and port / 规范化域名: 移除协议和端口
    fn normalize_domain(host: &str) -> String {
        let mut domain = host.to_lowercase();
        
        // Remove protocol / 移除协议
        if let Some(pos) = domain.find("://") {
            domain = domain[pos + 3..].to_string();
        }
        
        // Remove port / 移除端口
        if let Some(pos) = domain.find(':') {
            domain = domain[..pos].to_string();
        }
        
        // Remove trailing slash / 移除尾部斜杠
        domain.trim_end_matches('/').to_string()
    }

    /// Build download URL with configured domain
    /// 使用配置的域名构建下载URL
    /// 
    /// If download_domain is configured, builds URL with that domain.
    /// Otherwise returns the original path (relative URL).
    /// Supports reverse proxy headers (X-Forwarded-Proto, X-Forwarded-Host).
    /// 
    /// 如果配置了下载域名，使用该域名构建URL。
    /// 否则返回原始路径（相对URL）。
    /// 支持反向代理请求头 (X-Forwarded-Proto, X-Forwarded-Host)。
    pub fn build_download_url(&self, path: &str, scheme: &str) -> String {
        let configured = self.download_domain.read();
        tracing::debug!("build_download_url: configured='{}', path='{}', scheme='{}'", *configured, path, scheme);
        if configured.is_empty() {
            // No domain configured, return relative path / 未配置域名，返回相对路径
            tracing::debug!("build_download_url: domain is empty, returning relative path");
            return path.to_string();
        }

        // Configured domain may include protocol or not / 配置的域名可能包含协议也可能不包含
        // Examples: "dl.example.com", "http://dl.example.com", "https://dl.example.com"
        let configured = configured.trim();
        
        if configured.starts_with("http://") || configured.starts_with("https://") {
            // Domain includes protocol, use as-is / 域名包含协议，直接使用
            let base = configured.trim_end_matches('/');
            format!("{}{}", base, path)
        } else {
            // Domain without protocol, use scheme from request or default to http
            // 域名不含协议，使用请求中的协议或默认使用http（SSL由反代处理）
            let proto = if scheme.is_empty() { "http" } else { scheme };
            format!("{}://{}{}", proto, configured.trim_end_matches('/'), path)
        }
    }

    /// Try to acquire a concurrent slot / 尝试获取一个并发槽位
    /// 
    /// Returns true if slot acquired, false if at limit.
    /// 返回 true 如果获取成功，false 如果达到限制。
    pub fn try_acquire_slot(&self) -> bool {
        let max = self.max_concurrent.load(Ordering::SeqCst);
        if max == 0 {
            // Unlimited / 无限制
            self.current_concurrent.fetch_add(1, Ordering::SeqCst);
            return true;
        }

        loop {
            let current = self.current_concurrent.load(Ordering::SeqCst);
            if current >= max {
                return false;
            }
            if self.current_concurrent.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ).is_ok() {
                return true;
            }
        }
    }

    /// Release a concurrent slot / 释放一个并发槽位
    pub fn release_slot(&self) {
        self.current_concurrent.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get current concurrent count / 获取当前并发数
    pub fn get_current_concurrent(&self) -> i32 {
        self.current_concurrent.load(Ordering::SeqCst)
    }
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for concurrent slot / 并发槽位的RAII守卫
/// 
/// Automatically releases slot when dropped.
/// 丢弃时自动释放槽位。
pub struct ConcurrentGuard<'a> {
    settings: &'a DownloadSettings,
}

impl<'a> ConcurrentGuard<'a> {
    pub fn try_new(settings: &'a DownloadSettings) -> Option<Self> {
        if settings.try_acquire_slot() {
            Some(Self { settings })
        } else {
            None
        }
    }
}

impl<'a> Drop for ConcurrentGuard<'a> {
    fn drop(&mut self) {
        self.settings.release_slot();
    }
}

/// Rate limiter for bandwidth control / 带宽控制的速率限制器
/// 
/// Uses token bucket algorithm for smooth rate limiting.
/// 使用令牌桶算法实现平滑的速率限制。
pub struct BandwidthLimiter {
    /// Bytes per second limit, 0 = unlimited / 每秒字节数限制，0表示无限制
    bytes_per_sec: AtomicI64,
    /// Available tokens (bytes) / 可用令牌(字节数)
    tokens: AtomicI64,
    /// Last refill time in milliseconds / 上次补充时间(毫秒)
    last_refill: AtomicI64,
}

impl BandwidthLimiter {
    pub fn new(bytes_per_sec: i64) -> Self {
        Self {
            bytes_per_sec: AtomicI64::new(bytes_per_sec),
            tokens: AtomicI64::new(bytes_per_sec),
            last_refill: AtomicI64::new(Self::current_time_ms()),
        }
    }

    fn current_time_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Update rate limit / 更新速率限制
    pub fn set_rate(&self, bytes_per_sec: i64) {
        self.bytes_per_sec.store(bytes_per_sec, Ordering::SeqCst);
    }

    /// Get rate limit / 获取速率限制
    pub fn get_rate(&self) -> i64 {
        self.bytes_per_sec.load(Ordering::SeqCst)
    }

    /// Refill tokens based on elapsed time / 根据经过时间补充令牌
    fn refill(&self) {
        let limit = self.bytes_per_sec.load(Ordering::SeqCst);
        if limit == 0 {
            return;
        }

        let now = Self::current_time_ms();
        let last = self.last_refill.load(Ordering::SeqCst);
        let elapsed = now - last;

        if elapsed > 0 {
            // Add tokens based on time elapsed / 根据经过时间添加令牌
            let new_tokens = (limit * elapsed) / 1000;
            if new_tokens > 0 {
                let current = self.tokens.load(Ordering::SeqCst);
                // Cap at limit (1 second worth of tokens) / 上限为限制值(1秒的令牌量)
                let updated = std::cmp::min(current + new_tokens, limit);
                self.tokens.store(updated, Ordering::SeqCst);
                self.last_refill.store(now, Ordering::SeqCst);
            }
        }
    }

    /// Try to consume tokens, returns how many can be consumed
    /// 尝试消耗令牌，返回可以消耗的数量
    pub fn try_consume(&self, requested: i64) -> i64 {
        let limit = self.bytes_per_sec.load(Ordering::SeqCst);
        if limit == 0 {
            // Unlimited / 无限制
            return requested;
        }

        self.refill();

        loop {
            let available = self.tokens.load(Ordering::SeqCst);
            if available <= 0 {
                return 0;
            }

            let to_consume = std::cmp::min(requested, available);
            if self.tokens.compare_exchange(
                available,
                available - to_consume,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ).is_ok() {
                return to_consume;
            }
        }
    }

    /// Wait until tokens available and consume / 等待令牌可用并消耗
    pub async fn consume(&self, requested: i64) -> i64 {
        let limit = self.bytes_per_sec.load(Ordering::SeqCst);
        if limit == 0 {
            return requested;
        }

        loop {
            let consumed = self.try_consume(requested);
            if consumed > 0 {
                return consumed;
            }
            // Wait a bit before retry / 等待一段时间后重试
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

impl Default for BandwidthLimiter {
    fn default() -> Self {
        Self::new(0)
    }
}

use std::pin::Pin;
use std::task::{Context, Poll};
use futures::Stream;
use bytes::Bytes;

/// Throttled stream wrapper for bandwidth limiting / 带宽限制的流包装器
/// 
/// Wraps a byte stream and applies global bandwidth limiting.
/// All proxy downloads share the same global limiter for total bandwidth control.
/// 包装字节流并应用全局带宽限制。
/// 所有代理下载共享同一个全局限制器以控制总带宽。
pub struct ThrottledStream<S> {
    inner: S,
    /// Max bytes per second (cached from global settings) / 每秒最大字节数（从全局设置缓存）
    bytes_per_sec: i64,
    /// Shared global limiter / 共享的全局限制器
    limiter: Arc<BandwidthLimiter>,
    pending_bytes: Option<Bytes>,
}

impl<S> ThrottledStream<S> {
    /// Create with shared global limiter / 使用共享的全局限制器创建
    pub fn new(inner: S, limiter: Arc<BandwidthLimiter>) -> Self {
        let bytes_per_sec = limiter.get_rate();
        Self {
            inner,
            bytes_per_sec,
            limiter,
            pending_bytes: None,
        }
    }
    
    /// Create with speed limit (creates internal limiter) / 使用速度限制创建（内部创建限制器）
    pub fn with_speed(inner: S, bytes_per_sec: i64) -> Self {
        Self {
            inner,
            bytes_per_sec,
            limiter: Arc::new(BandwidthLimiter::new(bytes_per_sec)),
            pending_bytes: None,
        }
    }
}

impl<S, E> Stream for ThrottledStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let limit = self.limiter.get_rate();
        
        // If no limit, pass through directly / 如果无限制，直接透传
        if limit == 0 {
            return Pin::new(&mut self.inner).poll_next(cx);
        }

        // If we have pending bytes from previous throttling / 如果有上次限速剩余的数据
        if let Some(bytes) = self.pending_bytes.take() {
            let can_send = self.limiter.try_consume(bytes.len() as i64);
            if can_send > 0 {
                if can_send >= bytes.len() as i64 {
                    return Poll::Ready(Some(Ok(bytes)));
                } else {
                    // Split and save remainder / 分割并保存剩余部分
                    let send = bytes.slice(0..can_send as usize);
                    let remain = bytes.slice(can_send as usize..);
                    self.pending_bytes = Some(remain);
                    return Poll::Ready(Some(Ok(send)));
                }
            } else {
                // No tokens available, save and wake later / 无可用令牌，保存并稍后唤醒
                self.pending_bytes = Some(bytes);
                let waker = cx.waker().clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    waker.wake();
                });
                return Poll::Pending;
            }
        }

        // Poll inner stream / 轮询内部流
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let can_send = self.limiter.try_consume(bytes.len() as i64);
                if can_send >= bytes.len() as i64 {
                    Poll::Ready(Some(Ok(bytes)))
                } else if can_send > 0 {
                    // Partial send / 部分发送
                    let send = bytes.slice(0..can_send as usize);
                    let remain = bytes.slice(can_send as usize..);
                    self.pending_bytes = Some(remain);
                    Poll::Ready(Some(Ok(send)))
                } else {
                    // No tokens, save for later / 无令牌，保存待发
                    self.pending_bytes = Some(bytes);
                    let waker = cx.waker().clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        waker.wake();
                    });
                    Poll::Pending
                }
            }
            other => other,
        }
    }
}

use std::sync::atomic::AtomicU64;

/// 流量统计流包装器 / Traffic counting stream wrapper
/// 
/// 统计实际传输的字节数，流结束时更新用户流量统计
/// Counts actual bytes transferred, updates user traffic stats when stream ends
pub struct TrafficCountingStream<S> {
    inner: S,
    bytes_transferred: Arc<AtomicU64>,
    user_id: Option<String>,
    db: SqlitePool,
}

impl<S> TrafficCountingStream<S> {
    pub fn new(inner: S, user_id: Option<String>, db: SqlitePool) -> Self {
        Self {
            inner,
            bytes_transferred: Arc::new(AtomicU64::new(0)),
            user_id,
            db,
        }
    }
    
    /// 获取已传输的字节数
    pub fn get_bytes_transferred(&self) -> u64 {
        self.bytes_transferred.load(Ordering::SeqCst)
    }
}

impl<S, E> Stream for TrafficCountingStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                // 统计传输的字节数
                self.bytes_transferred.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                Poll::Ready(Some(Ok(bytes)))
            }
            other => other,
        }
    }
}

impl<S> Drop for TrafficCountingStream<S> {
    fn drop(&mut self) {
        let bytes = self.bytes_transferred.load(Ordering::SeqCst);
        if bytes > 0 {
            if let Some(ref user_id) = self.user_id {
                let db = self.db.clone();
                let user_id = user_id.clone();
                // 异步更新流量统计
                tokio::spawn(async move {
                    // 只统计流量，不增加请求数（请求数在302重定向时已统计）
                    if let Err(e) = sqlx::query(
                        "UPDATE users SET total_requests = total_requests + 1, total_traffic = total_traffic + ? WHERE id = ?"
                    )
                    .bind(bytes as i64)
                    .bind(&user_id)
                    .execute(&db)
                    .await {
                        tracing::warn!("本地中转流量统计更新失败: user_id={}, bytes={}, error={}", user_id, bytes, e);
                    } else {
                        tracing::debug!("本地中转流量统计: user_id={}, bytes={}", user_id, bytes);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_domain() {
        assert_eq!(DownloadSettings::normalize_domain("example.com"), "example.com");
        assert_eq!(DownloadSettings::normalize_domain("Example.COM"), "example.com");
        assert_eq!(DownloadSettings::normalize_domain("https://example.com"), "example.com");
        assert_eq!(DownloadSettings::normalize_domain("http://example.com:8080"), "example.com");
        assert_eq!(DownloadSettings::normalize_domain("example.com/"), "example.com");
    }

    #[test]
    fn test_validate_domain() {
        let settings = DownloadSettings::new();
        
        // Empty config = allow all / 空配置 = 允许所有
        assert!(settings.validate_domain("any.domain.com"));
        
        // Set domain / 设置域名
        settings.set_download_domain("dl.example.com".to_string());
        assert!(settings.validate_domain("dl.example.com"));
        assert!(settings.validate_domain("DL.Example.COM"));
        assert!(settings.validate_domain("dl.example.com:8080"));
        assert!(!settings.validate_domain("other.com"));
        assert!(!settings.validate_domain("example.com"));
    }

    #[test]
    fn test_concurrent_guard() {
        let settings = DownloadSettings::new();
        settings.set_max_concurrent(2);

        let g1 = ConcurrentGuard::try_new(&settings);
        assert!(g1.is_some());
        assert_eq!(settings.get_current_concurrent(), 1);

        let g2 = ConcurrentGuard::try_new(&settings);
        assert!(g2.is_some());
        assert_eq!(settings.get_current_concurrent(), 2);

        // At limit / 达到限制
        let g3 = ConcurrentGuard::try_new(&settings);
        assert!(g3.is_none());

        // Release one / 释放一个
        drop(g1);
        assert_eq!(settings.get_current_concurrent(), 1);

        // Now can acquire / 现在可以获取
        let g4 = ConcurrentGuard::try_new(&settings);
        assert!(g4.is_some());
    }
}
