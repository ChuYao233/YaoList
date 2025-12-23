use sqlx::SqlitePool;
use yaolist_backend::storage::StorageManager;
use yaolist_backend::search::DbIndex;
use yaolist_backend::load_balance::LoadBalanceManager;
use yaolist_backend::server::WebDavConfig;
use yaolist_backend::download::DownloadSettings;
use crate::task::TaskManager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::collections::HashMap;
use parking_lot::RwLock;
use chrono::{DateTime, Utc};

/// Index building progress / 索引构建进度
#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub is_running: bool,
    pub is_done: bool,
    pub object_count: u64,
    pub error: Option<String>,
    pub last_done_time: Option<i64>,
}

impl Default for IndexProgress {
    fn default() -> Self {
        Self {
            is_running: false,
            is_done: true,
            object_count: 0,
            error: None,
            last_done_time: None,
        }
    }
}

/// Index state management / 索引状态管理
pub struct IndexState {
    pub running: AtomicBool,
    pub object_count: AtomicU64,
    pub progress: RwLock<IndexProgress>,
    pub cancel_flag: AtomicBool,
}

impl IndexState {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            object_count: AtomicU64::new(0),
            progress: RwLock::new(IndexProgress::default()),
            cancel_flag: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        self.cancel_flag.store(false, Ordering::SeqCst);
        self.object_count.store(0, Ordering::SeqCst);
        let mut progress = self.progress.write();
        progress.is_running = true;
        progress.is_done = false;
        progress.object_count = 0;
        progress.error = None;
    }

    pub fn increment(&self) {
        let count = self.object_count.fetch_add(1, Ordering::SeqCst) + 1;
        let mut progress = self.progress.write();
        progress.object_count = count;
    }

    pub fn finish(&self, error: Option<String>) {
        self.running.store(false, Ordering::SeqCst);
        let mut progress = self.progress.write();
        progress.is_running = false;
        progress.is_done = error.is_none();
        progress.error = error;
        progress.last_done_time = Some(chrono::Utc::now().timestamp());
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        // Immediately reset running status to allow next build / 立即重置running状态
        self.running.store(false, Ordering::SeqCst);
        let mut progress = self.progress.write();
        progress.is_running = false;
        progress.error = Some("Index building cancelled".to_string());
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn get_progress(&self) -> IndexProgress {
        self.progress.read().clone()
    }
}

impl Default for IndexState {
    fn default() -> Self {
        Self::new()
    }
}

/// Login failure records / 登录失败记录
#[derive(Debug, Clone)]
pub struct LoginAttempt {
    pub fail_count: u32,
    pub last_attempt: DateTime<Utc>,
}

/// Captcha records / 验证码记录
#[derive(Debug, Clone)]
pub struct CaptchaRecord {
    pub code: String,
    pub created_at: DateTime<Utc>,
}

/// Reset code records / 重置码记录
#[derive(Debug, Clone)]
pub struct ResetCodeRecord {
    pub user_id: String,
    pub code: String,
    pub created_at: DateTime<Utc>,
}

/// Login security state / 登录安全状态
pub struct LoginSecurity {
    /// IP login failure records: IP -> LoginAttempt / IP登录失败记录
    pub ip_attempts: RwLock<HashMap<String, LoginAttempt>>,
    /// User login failure records: username -> fail_count / 用户登录失败记录
    pub user_attempts: RwLock<HashMap<String, u32>>,
    /// Captcha storage: captcha_id -> CaptchaRecord / 验证码存储
    pub captchas: RwLock<HashMap<String, CaptchaRecord>>,
    /// Reset code storage: target -> ResetCodeRecord / 重置码存储
    pub reset_codes: RwLock<HashMap<String, ResetCodeRecord>>,
}

impl LoginSecurity {
    pub fn new() -> Self {
        Self {
            ip_attempts: RwLock::new(HashMap::new()),
            user_attempts: RwLock::new(HashMap::new()),
            captchas: RwLock::new(HashMap::new()),
            reset_codes: RwLock::new(HashMap::new()),
        }
    }

    /// Check if IP is blocked (more than 5 failures within 30 minutes) / 检查IP是否被封禁
    pub fn is_ip_blocked(&self, ip: &str) -> bool {
        let attempts = self.ip_attempts.read();
        if let Some(attempt) = attempts.get(ip) {
            if attempt.fail_count >= 5 {
                let elapsed = Utc::now().signed_duration_since(attempt.last_attempt);
                return elapsed.num_minutes() < 30;
            }
        }
        false
    }

    /// Check if user needs captcha (more than 1 failure) / 检查用户是否需要验证码
    pub fn needs_captcha(&self, username: &str) -> bool {
        let attempts = self.user_attempts.read();
        attempts.get(username).map(|&c| c >= 1).unwrap_or(false)
    }

    /// Check if IP needs captcha (has failure record within 30 minutes) / 检查IP是否需要验证码
    pub fn needs_captcha_by_ip(&self, ip: &str) -> bool {
        let attempts = self.ip_attempts.read();
        if let Some(attempt) = attempts.get(ip) {
            if attempt.fail_count >= 1 {
                let elapsed = Utc::now().signed_duration_since(attempt.last_attempt);
                return elapsed.num_minutes() < 30;
            }
        }
        false
    }

    /// Record login failure / 记录登录失败
    pub fn record_failure(&self, ip: &str, username: &str) {
        let now = Utc::now();
        
        // Update IP failure records / 更新IP失败记录
        {
            let mut attempts = self.ip_attempts.write();
            let entry = attempts.entry(ip.to_string()).or_insert(LoginAttempt {
                fail_count: 0,
                last_attempt: now,
            });
            // If over 30 minutes, reset count / 如果超过30分钟
            if now.signed_duration_since(entry.last_attempt).num_minutes() >= 30 {
                entry.fail_count = 0;
            }
            entry.fail_count += 1;
            entry.last_attempt = now;
        }
        
        // Update user failure records / 更新用户失败记录
        {
            let mut attempts = self.user_attempts.write();
            *attempts.entry(username.to_string()).or_insert(0) += 1;
        }
    }

    /// Login successful, clear failure records / 登录成功
    pub fn clear_failure(&self, ip: &str, username: &str) {
        self.ip_attempts.write().remove(ip);
        self.user_attempts.write().remove(username);
    }

    /// Store captcha / 存储验证码
    pub fn store_captcha(&self, id: String, code: String) {
        let mut captchas = self.captchas.write();
        // Clean expired captchas (5 minutes) / 清理过期验证码
        let now = Utc::now();
        captchas.retain(|_, v| now.signed_duration_since(v.created_at).num_minutes() < 5);
        captchas.insert(id, CaptchaRecord { code, created_at: now });
    }

    /// Verify and consume captcha / 验证并消费验证码
    pub fn verify_captcha(&self, id: &str, code: &str) -> bool {
        let mut captchas = self.captchas.write();
        if let Some(record) = captchas.remove(id) {
            let elapsed = Utc::now().signed_duration_since(record.created_at);
            if elapsed.num_minutes() < 5 {
                return record.code.eq_ignore_ascii_case(code);
            }
        }
        false
    }

    /// Store reset code / 存储重置码
    pub fn store_reset_code(&self, target: String, user_id: String, code: String) {
        let mut reset_codes = self.reset_codes.write();
        let now = Utc::now();
        // Clean expired reset codes (10 minutes) / 清理过期重置码
        reset_codes.retain(|_, v| now.signed_duration_since(v.created_at).num_minutes() < 10);
        reset_codes.insert(target, ResetCodeRecord { user_id, code, created_at: now });
    }

    /// Verify and consume reset code, return user_id / 验证并消费重置码
    pub fn verify_reset_code(&self, target: &str, code: &str) -> Option<String> {
        let mut reset_codes = self.reset_codes.write();
        if let Some(record) = reset_codes.remove(target) {
            let elapsed = Utc::now().signed_duration_since(record.created_at);
            if elapsed.num_minutes() < 10 && record.code == code {
                return Some(record.user_id);
            }
        }
        None
    }
}

impl Default for LoginSecurity {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AppState {
    pub db: SqlitePool,
    pub storage_manager: StorageManager,
    pub task_manager: TaskManager,
    pub db_index: tokio::sync::RwLock<Option<Arc<DbIndex>>>,
    pub index_state: Arc<IndexState>,
    pub load_balance: Arc<LoadBalanceManager>,
    pub webdav_config: tokio::sync::RwLock<WebDavConfig>,
    pub login_security: LoginSecurity,
    /// Download settings (domain validation, proxy limits) / 下载设置(域名验证、代理限制)
    pub download_settings: Arc<DownloadSettings>,
}

impl AppState {
    /// 获取或创建搜索数据库索引（懒加载）
    pub async fn get_db_index(&self) -> Result<Arc<DbIndex>, String> {
        // 先检查是否已存在
        {
            let guard = self.db_index.read().await;
            if let Some(ref idx) = *guard {
                return Ok(idx.clone());
            }
        }
        
        // 不存在则创建
        let mut guard = self.db_index.write().await;
        // 双重检查
        if let Some(ref idx) = *guard {
            return Ok(idx.clone());
        }
        
        let idx = DbIndex::new_standalone().await?;
        idx.init().await?;
        let idx = Arc::new(idx);
        *guard = Some(idx.clone());
        Ok(idx)
    }
    
    /// 关闭并重置搜索数据库索引（用于清除索引）
    pub async fn reset_db_index(&self) {
        let mut guard = self.db_index.write().await;
        if let Some(ref idx) = *guard {
            idx.close().await;
        }
        *guard = None;
    }
}
