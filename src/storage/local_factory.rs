use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

use super::{StorageDriver, DriverFactory, DriverConfig, ConfigItem};
use crate::drivers::local;

pub struct LocalDriverFactory;

impl DriverFactory for LocalDriverFactory {
    fn driver_type(&self) -> &'static str {
        "local"
    }
    
    fn driver_config(&self) -> DriverConfig {
        DriverConfig {
            name: "Local".to_string(),
            local_sort: true,
            only_proxy: true,
            no_cache: true,
            no_upload: false,
            default_root: Some("/".to_string()),
        }
    }
    
    fn additional_items(&self) -> Vec<ConfigItem> {
        vec![
            ConfigItem::new("root", "string")
                .title("drivers.local.root")
                .required()
                .help("drivers.local.rootHelp"),
            ConfigItem::new("mkdir_perm", "string")
                .title("drivers.local.mkdirPerm")
                .default("777")
                .help("drivers.local.mkdirPermHelp"),
            ConfigItem::new("recycle_bin_path", "string")
                .title("drivers.local.recycleBinPath")
                .default("delete permanently")
                .help("drivers.local.recycleBinPathHelp"),
            ConfigItem::new("show_space_info", "bool")
                .title("显示空间信息")
                .default("true"),
        ]
    }

    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>> {
        let root_path = config.get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("缺少 root 配置"))?;
        
        let show_space_info = config.get("show_space_info")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        
        let root = PathBuf::from(root_path);
        
        // 同步初始化（工厂方法是同步的）
        if !root.exists() {
            std::fs::create_dir_all(&root)?;
        }
        let canonical_root = root.canonicalize()?;
        
        tracing::info!("Local driver initialized, root: {:?}", canonical_root);
        
        Ok(Box::new(local::LocalDriver::with_config(canonical_root, show_space_info)))
    }
}
