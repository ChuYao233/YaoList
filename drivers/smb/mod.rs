//! SMB/CIFS 网络共享驱动（系统原生支持）
//!
//! - Windows：直接使用 UNC 路径
//! - Linux：通过 CIFS 挂载

mod driver_native;

pub use driver_native::{SmbDriver, SmbConfig};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::storage::{StorageDriver, DriverFactory, DriverConfig, ConfigItem};

/// SMB 驱动工厂
pub struct SmbDriverFactory;

impl DriverFactory for SmbDriverFactory {
    fn driver_type(&self) -> &'static str {
        "smb"
    }
    
    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "SMB/CIFS".to_string(),
            local_sort: true,
            only_proxy: true, // 浏览器不支持直接访问SMB
            no_cache: true,   // SMB连接状态管理，不缓存
            no_upload: false,
            default_root: Some(".".to_string()),
        }
    }
    
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("address", "string")
                .title("服务器地址")
                .help("SMB服务器地址，格式: host 或 host:port（默认端口445）")
                .required(),
            ConfigItem::new("username", "string")
                .title("用户名")
                .help("SMB登录用户名")
                .required(),
            ConfigItem::new("password", "password")
                .title("密码")
                .help("SMB登录密码"),
            ConfigItem::new("share_name", "string")
                .title("共享名称")
                .help("要访问的共享文件夹名称")
                .required(),
            ConfigItem::new("root_path", "string")
                .title("根目录")
                .help("共享内的根目录路径")
                .default("."),
        ]
    }
    
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let config: SmbConfig = serde_json::from_value(config)
            .map_err(|e| anyhow!("配置解析失败: {}", e))?;
        Ok(Box::new(SmbDriver::new(config)?))
    }
}
