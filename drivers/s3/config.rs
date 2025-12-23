//! S3驱动配置

use serde::{Deserialize, Serialize};

/// S3配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// 存储桶名称
    pub bucket: String,
    /// S3端点地址
    /// AWS: https://s3.{region}.amazonaws.com
    /// 阿里云OSS: https://oss-{region}.aliyuncs.com
    /// 腾讯云COS: https://cos.{region}.myqcloud.com
    /// MinIO: http://localhost:9000
    pub endpoint: String,
    /// 区域
    #[serde(default = "default_region")]
    pub region: String,
    /// Access Key ID
    pub access_key_id: String,
    /// Secret Access Key
    pub secret_access_key: String,
    /// Session Token（用于临时凭证）
    #[serde(default)]
    pub session_token: String,
    /// 根目录路径
    #[serde(default = "default_root")]
    pub root_path: String,
    /// 自定义域名（用于CDN加速）
    #[serde(default)]
    pub custom_host: String,
    /// 预签名URL过期时间（小时）
    #[serde(default = "default_sign_expire")]
    pub sign_url_expire: u32,
    /// 强制使用路径风格（而非虚拟主机风格）
    /// MinIO等需要设置为true
    #[serde(default)]
    pub force_path_style: bool,
    /// 目录占位文件名
    #[serde(default = "default_placeholder")]
    pub placeholder: String,
    /// 是否显示空间信息
    #[serde(default)]
    pub show_space_info: bool,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_root() -> String {
    "/".to_string()
}

fn default_sign_expire() -> u32 {
    4
}

fn default_placeholder() -> String {
    ".yaolist".to_string()
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            endpoint: String::new(),
            region: default_region(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: String::new(),
            root_path: default_root(),
            custom_host: String::new(),
            sign_url_expire: default_sign_expire(),
            force_path_style: false,
            placeholder: default_placeholder(),
            show_space_info: false,
        }
    }
}
