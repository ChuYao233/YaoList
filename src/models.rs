use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Mount {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub mount_path: String,
    pub config: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMountRequest {
    pub name: String,
    pub driver: String,
    pub mount_path: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMountRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub unique_id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub root_path: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
    pub two_factor_enabled: bool,
    pub two_factor_secret: Option<String>,
    pub total_traffic: i64,
    pub total_requests: i64,
    pub last_login: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserGroup {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_admin: bool,
    pub allow_direct_link: bool,
    pub allow_share: bool,
    pub show_hidden_files: bool,
    pub no_password_access: bool,
    pub add_offline_download: bool,
    pub create_upload: bool,
    pub rename_files: bool,
    pub move_files: bool,
    pub copy_files: bool,
    pub delete_files: bool,
    pub read_files: bool,
    pub read_compressed: bool,
    pub extract_files: bool,
    pub webdav_enabled: bool,
    pub ftp_enabled: bool,
    pub root_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Permission {
    pub id: String,
    pub name: String,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub group_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub two_factor_enabled: Option<bool>,
    #[serde(default)]
    pub group_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    pub permission_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Default)]
pub struct UserPermissions {
    pub read_files: bool,
    pub create_upload: bool,
    pub rename_files: bool,
    pub move_files: bool,
    pub copy_files: bool,
    pub delete_files: bool,
    pub allow_direct_link: bool,
    pub allow_share: bool,
    pub is_admin: bool,
    pub show_hidden_files: bool,
    pub extract_files: bool,  // Allow online extraction / 允许在线解压缩
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Meta {
    pub id: i64,
    pub path: String,
    pub password: Option<String>,
    pub p_sub: bool,
    pub write: bool,
    pub w_sub: bool,
    pub hide: Option<String>,
    pub h_sub: bool,
    pub readme: Option<String>,
    pub r_sub: bool,
    pub header: Option<String>,
    pub header_sub: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMetaRequest {
    pub path: String,
    pub password: Option<String>,
    pub p_sub: Option<bool>,
    pub write: Option<bool>,
    pub w_sub: Option<bool>,
    pub hide: Option<String>,
    pub h_sub: Option<bool>,
    pub readme: Option<String>,
    pub r_sub: Option<bool>,
    pub header: Option<String>,
    pub header_sub: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMetaRequest {
    pub path: Option<String>,
    pub password: Option<String>,
    pub p_sub: Option<bool>,
    pub write: Option<bool>,
    pub w_sub: Option<bool>,
    pub hide: Option<String>,
    pub h_sub: Option<bool>,
    pub readme: Option<String>,
    pub r_sub: Option<bool>,
    pub header: Option<String>,
    pub header_sub: Option<bool>,
}
