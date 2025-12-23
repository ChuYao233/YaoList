//! 蓝奏云配置

use serde::{Deserialize, Serialize};

/// 登录类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LoginType {
    /// 账号密码登录
    Account,
    /// Cookie登录
    Cookie,
    /// 分享链接
    Url,
}

impl Default for LoginType {
    fn default() -> Self {
        Self::Cookie
    }
}

/// 蓝奏云配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanzouConfig {
    /// 登录类型
    #[serde(default)]
    pub login_type: LoginType,
    
    /// 账号（手机号或邮箱）
    #[serde(default)]
    pub account: String,
    
    /// 密码
    #[serde(default)]
    pub password: String,
    
    /// Cookie（ylogin和phpdisk_info）
    #[serde(default)]
    pub cookie: String,
    
    /// 分享链接密码
    #[serde(default)]
    pub share_password: String,
    
    /// 分享链接URL
    #[serde(default)]
    pub share_url: String,
    
    /// 根目录ID（-1表示根目录）
    #[serde(default = "default_root_folder_id")]
    pub root_folder_id: String,
    
    /// 是否修复文件信息（获取真实大小和时间）
    #[serde(default)]
    pub repair_file_info: bool,
    
    /// 自定义UA
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

fn default_root_folder_id() -> String {
    "-1".to_string()
}

fn default_user_agent() -> String {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string()
}

impl Default for LanzouConfig {
    fn default() -> Self {
        Self {
            login_type: LoginType::default(),
            account: String::new(),
            password: String::new(),
            cookie: String::new(),
            share_password: String::new(),
            share_url: String::new(),
            root_folder_id: default_root_folder_id(),
            repair_file_info: false,
            user_agent: default_user_agent(),
        }
    }
}
