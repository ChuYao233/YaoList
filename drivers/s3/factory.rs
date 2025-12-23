//! S3驱动工厂

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::storage::{DriverFactory, DriverConfig, ConfigItem, StorageDriver};
use super::config::S3Config;
use super::driver::S3Driver;

/// S3驱动工厂
pub struct S3DriverFactory;

impl DriverFactory for S3DriverFactory {
    fn driver_type(&self) -> &'static str {
        "s3"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "S3".to_string(),
            local_sort: true,
            only_proxy: false, // 支持预签名URL直链
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("bucket", "string")
                .title("存储桶名称")
                .help("S3存储桶名称")
                .required(),
            ConfigItem::new("endpoint", "string")
                .title("端点地址")
                .help("S3端点URL（阿里云OSS: https://oss-cn-hangzhou.aliyuncs.com）")
                .required(),
            ConfigItem::new("region", "string")
                .title("区域")
                .help("S3区域，如 us-east-1、cn-hangzhou")
                .default("us-east-1"),
            ConfigItem::new("access_key_id", "string")
                .title("Access Key ID")
                .required(),
            ConfigItem::new("secret_access_key", "password")
                .title("Secret Access Key")
                .required(),
            ConfigItem::new("session_token", "password")
                .title("Session Token")
                .help("临时凭证的会话令牌（可选）"),
            ConfigItem::new("root_path", "string")
                .title("根目录路径")
                .help("存储桶内的根目录路径")
                .default("/"),
            ConfigItem::new("custom_host", "string")
                .title("自定义域名")
                .help("CDN加速域名（可选）"),
            ConfigItem::new("sign_url_expire", "number")
                .title("签名URL过期时间")
                .help("预签名URL过期时间（小时）")
                .default("4"),
            ConfigItem::new("force_path_style", "bool")
                .title("强制路径风格")
                .help("MinIO等需要开启此选项")
                .default("false"),
            ConfigItem::new("placeholder", "string")
                .title("目录占位文件")
                .help("用于模拟空目录的占位文件名")
                .default(".yaolist"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: S3Config = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(S3Driver::new(config)?))
    }
}
