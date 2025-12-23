//! WebDAV 网络存储驱动
//!
//! 参考 OpenList 的 WebDAV 驱动实现

mod driver;

pub use driver::{WebDavDriver, WebDavConfig};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::storage::{StorageDriver, DriverFactory, DriverConfig, ConfigItem};

/// WebDAV 驱动工厂
pub struct WebDavDriverFactory;

impl DriverFactory for WebDavDriverFactory {
    fn driver_type(&self) -> &'static str {
        "webdav"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "WebDAV".to_string(),
            local_sort: true,
            only_proxy: true,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("address", "string")
                .title("服务器地址")
                .help("WebDAV服务器地址，如 https://dav.example.com/files")
                .required(),
            ConfigItem::new("username", "string")
                .title("用户名")
                .help("WebDAV登录用户名")
                .required(),
            ConfigItem::new("password", "password")
                .title("密码")
                .help("WebDAV登录密码"),
            ConfigItem::new("root_path", "string")
                .title("根目录")
                .help("WebDAV服务器上的根目录路径")
                .default("/"),
            ConfigItem::new("tls_insecure_skip_verify", "bool")
                .title("跳过TLS验证")
                .help("是否跳过TLS证书验证（不推荐）")
                .default("false"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: WebDavConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(WebDavDriver::new(config)?))
    }
}
