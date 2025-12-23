//! 蓝奏云驱动工厂

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::storage::{DriverFactory, DriverConfig, ConfigItem, StorageDriver};
use super::config::LanzouConfig;
use super::driver::LanzouDriver;

/// 蓝奏云驱动工厂
pub struct LanzouDriverFactory;

impl DriverFactory for LanzouDriverFactory {
    fn driver_type(&self) -> &'static str {
        "lanzou"
    }

    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "蓝奏云".to_string(),
            local_sort: true,
            only_proxy: false,
            no_cache: false,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }

    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("login_type", "select")
                .title("登录方式")
                .options("cookie:Cookie登录,account:账号密码,url:分享链接")
                .default("cookie")
                .required(),
            ConfigItem::new("cookie", "string")
                .title("Cookie")
                .help("包含ylogin和phpdisk_info的Cookie字符串"),
            ConfigItem::new("account", "string")
                .title("账号")
                .help("手机号或邮箱"),
            ConfigItem::new("password", "password")
                .title("密码")
                .help("登录密码"),
            ConfigItem::new("share_url", "string")
                .title("分享链接")
                .help("蓝奏云分享链接URL"),
            ConfigItem::new("share_password", "string")
                .title("分享密码")
                .help("分享链接的访问密码（如有）"),
            ConfigItem::new("root_folder_id", "string")
                .title("根目录ID")
                .help("-1表示根目录")
                .default("-1"),
            ConfigItem::new("repair_file_info", "bool")
                .title("修复文件信息")
                .help("获取真实的文件大小和修改时间")
                .default("false"),
            ConfigItem::new("user_agent", "string")
                .title("User-Agent")
                .help("自定义请求UA")
                .default("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: LanzouConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        
        Ok(Box::new(LanzouDriver::new(config)))
    }
}
