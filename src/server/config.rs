use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use crate::models::UserGroup;

/// FTP服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpConfig {
    /// 是否启用FTP服务器
    pub enabled: bool,
    /// 监听地址
    pub listen: String,
    /// 被动模式端口范围
    pub passive_ports_start: u16,
    pub passive_ports_end: u16,
    /// 公共主机地址（用于被动模式）
    pub public_host: Option<String>,
    /// 空闲超时时间（秒）
    pub idle_timeout: u64,
    /// 是否启用TLS
    pub tls_enabled: bool,
    /// TLS证书路径
    pub tls_cert_path: Option<String>,
    /// TLS私钥路径
    pub tls_key_path: Option<String>,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:21".to_string(),
            passive_ports_start: 50000,
            passive_ports_end: 50100,
            public_host: None,
            idle_timeout: 600,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

/// WebDAV服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavConfig {
    /// 是否启用WebDAV服务器
    pub enabled: bool,
    /// 监听地址
    pub listen: String,
    /// URL前缀
    pub prefix: String,
    /// 是否启用TLS
    pub tls_enabled: bool,
    /// TLS证书路径
    pub tls_cert_path: Option<String>,
    /// TLS私钥路径
    pub tls_key_path: Option<String>,
}

impl Default for WebDavConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:5005".to_string(),
            prefix: "/dav".to_string(),
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

/// 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerConfig {
    pub webdav: WebDavConfig,
}

/// 用户权限（从用户组合并）
#[derive(Debug, Clone, Default)]
pub struct UserPermissions {
    pub can_read: bool,
    pub can_write: bool,
    pub can_delete: bool,
    pub can_rename: bool,
    pub can_move: bool,
    pub can_copy: bool,
    pub can_create_dir: bool,
    pub ftp_enabled: bool,
    pub webdav_enabled: bool,
    pub is_admin: bool,
    pub root_path: Option<String>,
}

impl UserPermissions {
    /// 从用户组列表合并权限（取并集）
    pub fn from_groups(groups: &[UserGroup], user_root_path: Option<String>) -> Self {
        let mut perms = Self::default();
        
        for group in groups {
            perms.can_read = perms.can_read || group.read_files;
            perms.can_write = perms.can_write || group.create_upload;
            perms.can_delete = perms.can_delete || group.delete_files;
            perms.can_rename = perms.can_rename || group.rename_files;
            perms.can_move = perms.can_move || group.move_files;
            perms.can_copy = perms.can_copy || group.copy_files;
            perms.can_create_dir = perms.can_create_dir || group.create_upload;
            perms.ftp_enabled = perms.ftp_enabled || group.ftp_enabled;
            perms.webdav_enabled = perms.webdav_enabled || group.webdav_enabled;
            perms.is_admin = perms.is_admin || group.is_admin;
            
            // root_path 取第一个非空的
            if perms.root_path.is_none() {
                perms.root_path = group.root_path.clone();
            }
        }
        
        // 用户级别的root_path优先
        if user_root_path.is_some() {
            perms.root_path = user_root_path;
        }
        
        perms
    }
}

/// 认证用户信息
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: String,
    pub username: String,
    pub permissions: UserPermissions,
}

/// 用户认证器
pub struct UserAuthenticator {
    db: SqlitePool,
}

impl UserAuthenticator {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// 验证用户并返回认证信息
    pub async fn authenticate(&self, username: &str, password: &str) -> Option<AuthenticatedUser> {
        // 查询用户
        let user: Option<crate::models::User> = sqlx::query_as(
            "SELECT * FROM users WHERE username = ? AND enabled = 1"
        )
        .bind(username)
        .fetch_optional(&self.db)
        .await
        .ok()?;

        let user = user?;

        // 验证密码
        if !bcrypt::verify(password, &user.password_hash).unwrap_or(false) {
            return None;
        }

        // 查询用户的所有组
        let groups: Vec<UserGroup> = sqlx::query_as(
            r#"
            SELECT g.* FROM user_groups g
            INNER JOIN user_group_members m ON g.id = m.group_id
            WHERE m.user_id = ?
            "#
        )
        .bind(&user.id)
        .fetch_all(&self.db)
        .await
        .ok()?;

        let permissions = UserPermissions::from_groups(&groups, user.root_path.clone());

        Some(AuthenticatedUser {
            id: user.id,
            username: user.username,
            permissions,
        })
    }

    /// 验证用户是否有FTP权限
    pub async fn authenticate_ftp(&self, username: &str, password: &str) -> Option<AuthenticatedUser> {
        let user = self.authenticate(username, password).await?;
        if user.permissions.ftp_enabled || user.permissions.is_admin {
            Some(user)
        } else {
            None
        }
    }

    /// 验证用户是否有WebDAV权限
    pub async fn authenticate_webdav(&self, username: &str, password: &str) -> Option<AuthenticatedUser> {
        let user = self.authenticate(username, password).await;
        match &user {
            Some(u) => {
                tracing::info!("WebDAV permission check: user={}, webdav_enabled={}, is_admin={}", 
                    u.username, u.permissions.webdav_enabled, u.permissions.is_admin);
                if u.permissions.webdav_enabled || u.permissions.is_admin {
                    user
                } else {
                    tracing::warn!("WebDAV permission insufficient: user={}", u.username);
                    None
                }
            }
            None => {
                tracing::warn!("WebDAV user auth failed: {}", username);
                None
            }
        }
    }
}
