use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{StorageDriver, DriverConfig, DriverInfo, ConfigItem, get_common_items};

pub type DriverBox = Arc<Box<dyn StorageDriver>>;

/// Driver factory trait / 驱动工厂 trait
pub trait DriverFactory: Send + Sync {
    /// Driver type name / 驱动类型名称
    fn driver_type(&self) -> &'static str;
    
    /// 创建驱动实例
    fn create_driver(&self, config: Value) -> Result<Box<dyn StorageDriver>>;
    
    /// Return driver basic config / 返回驱动基本配置
    fn driver_config(&self) -> DriverConfig;
    
    /// Return driver specific config items / 返回驱动特有配置项
    fn additional_items(&self) -> Vec<ConfigItem>;
    
    /// Generate complete driver info (auto merge common + additional) / 生成完整的驱动信息
    fn driver_info(&self) -> DriverInfo {
        let config = self.driver_config();
        let common = get_common_items(&config);
        let additional = self.additional_items();
        DriverInfo { common, additional, config }
    }
}

/// Storage manager (manages all driver instances) / 存储管理器
#[derive(Clone)]
pub struct StorageManager {
    drivers: Arc<RwLock<HashMap<String, DriverBox>>>,
    factories: Arc<RwLock<HashMap<String, Arc<Box<dyn DriverFactory>>>>>,
    /// Driver error status (id -> error message) / 驱动错误状态
    driver_errors: Arc<RwLock<HashMap<String, String>>>,
}

impl StorageManager {
    pub fn new() -> Self {
        Self {
            drivers: Arc::new(RwLock::new(HashMap::new())),
            factories: Arc::new(RwLock::new(HashMap::new())),
            driver_errors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register driver factory / 注册驱动工厂
    pub async fn register_factory(&self, factory: Box<dyn DriverFactory>) -> Result<()> {
        let driver_type = factory.driver_type().to_string();
        let factory_box = Arc::new(factory);
        
        let mut factories = self.factories.write().await;
        factories.insert(driver_type.clone(), factory_box);
        
        tracing::info!("Driver factory registered: {}", driver_type);
        Ok(())
    }

    /// Create driver instance (verify on success, record error on failure) / 创建驱动实例
    pub async fn create_driver(&self, id: String, driver_type: &str, config: Value) -> Result<String> {
        let factories = self.factories.read().await;
        let factory = factories.get(driver_type)
            .ok_or_else(|| anyhow!("Driver type not found: {}", driver_type))?;
        
        match factory.create_driver(config) {
            Ok(driver) => {
                let driver_box: DriverBox = Arc::new(driver);
                
                drop(factories);
                
                // Verify driver validity: try list root directory / 验证驱动有效性
                let validation_result = driver_box.list("/").await;
                
                let mut drivers = self.drivers.write().await;
                drivers.insert(id.clone(), driver_box);
                drop(drivers);
                
                match validation_result {
                    Ok(_) => {
                        // Verification successful, clear error / 验证成功
                        let mut errors = self.driver_errors.write().await;
                        errors.remove(&id);
                        tracing::info!("Driver created and verified: {} ({})", id, driver_type);
                    }
                    Err(e) => {
                        // Verification failed, record error (but driver still created) / 验证失败
                        let error_msg = e.to_string();
                        let mut errors = self.driver_errors.write().await;
                        errors.insert(id.clone(), error_msg.clone());
                        tracing::warn!("Driver created but verification failed: {} ({}) - {}", id, driver_type, error_msg);
                    }
                }
                
                Ok(id)
            }
            Err(e) => {
                drop(factories);
                // Record error / 记录错误
                let error_msg = e.to_string();
                let mut errors = self.driver_errors.write().await;
                errors.insert(id.clone(), error_msg.clone());
                
                tracing::error!("Driver creation failed: {} ({}) - {}", id, driver_type, error_msg);
                Err(e)
            }
        }
    }
    
    /// Set driver error status / 设置驱动错误状态
    pub async fn set_driver_error(&self, id: &str, error: String) {
        let mut errors = self.driver_errors.write().await;
        errors.insert(id.to_string(), error);
    }
    
    /// Clear driver error status / 清除驱动错误状态
    pub async fn clear_driver_error(&self, id: &str) {
        let mut errors = self.driver_errors.write().await;
        errors.remove(id);
    }
    
    /// Get driver error status / 获取驱动错误状态
    pub async fn get_driver_error(&self, id: &str) -> Option<String> {
        let errors = self.driver_errors.read().await;
        errors.get(id).cloned()
    }
    
    /// Get all driver error statuses / 获取所有驱动错误状态
    pub async fn get_all_driver_errors(&self) -> HashMap<String, String> {
        let errors = self.driver_errors.read().await;
        errors.clone()
    }

    /// Get driver instance / 获取驱动实例
    pub async fn get_driver(&self, id: &str) -> Option<DriverBox> {
        let drivers = self.drivers.read().await;
        drivers.get(id).cloned()
    }

    /// Remove driver instance / 移除驱动实例
    pub async fn remove_driver(&self, id: &str) -> Result<()> {
        let mut drivers = self.drivers.write().await;
        drivers.remove(id)
            .ok_or_else(|| anyhow!("Driver not found: {}", id))?;
        
        tracing::info!("Driver removed: {}", id);
        Ok(())
    }

    /// List all drivers / 列出所有驱动
    pub async fn list_drivers(&self) -> Vec<String> {
        let drivers = self.drivers.read().await;
        drivers.keys().cloned().collect()
    }

    /// List all available driver types / 列出所有可用的驱动类型
    pub async fn list_driver_types(&self) -> Vec<String> {
        let factories = self.factories.read().await;
        factories.keys().cloned().collect()
    }

    /// List all driver factories (keep compatibility) / 列出所有驱动工厂
    pub async fn list_factories(&self) -> Vec<String> {
        self.list_driver_types().await
    }
    
    /// Get all driver factory instances / 获取所有驱动工厂实例
    pub async fn get_all_factories(&self) -> Vec<Arc<Box<dyn DriverFactory>>> {
        let factories = self.factories.read().await;
        factories.values().cloned().collect()
    }

    /// Resolve path to corresponding driver and relative path
    /// Returns (driver instance, relative path) / 根据路径解析到对应的驱动
    pub async fn resolve_path(&self, path: &str) -> Result<Option<(DriverBox, String)>> {
        let drivers = self.drivers.read().await;
        
        // Iterate through all drivers to find matching mount point
        // Here assumes driver ID is mount path, should query from mount table / 遍历所有驱动找到挂载点
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        // Find the longest matching mount point / 找到最长匹配的挂载点
        let mut best_match: Option<(String, DriverBox, String)> = None;
        
        for (mount_path, driver) in drivers.iter() {
            let mount = if mount_path.starts_with('/') {
                mount_path.clone()
            } else {
                format!("/{}", mount_path)
            };
            
            if normalized_path.starts_with(&mount) || normalized_path == mount.trim_end_matches('/') {
                let relative = if normalized_path.len() > mount.len() {
                    normalized_path[mount.len()..].to_string()
                } else {
                    "/".to_string()
                };
                
                let relative = if relative.is_empty() || !relative.starts_with('/') {
                    format!("/{}", relative.trim_start_matches('/'))
                } else {
                    relative
                };

                if best_match.as_ref().map(|(m, _, _)| m.len()).unwrap_or(0) < mount.len() {
                    best_match = Some((mount, driver.clone(), relative));
                }
            }
        }

        Ok(best_match.map(|(_, driver, relative)| (driver, relative)))
    }

    /// Get all driver instances (for iteration) / 获取所有驱动实例
    pub async fn get_all_drivers(&self) -> Vec<(String, DriverBox)> {
        let drivers = self.drivers.read().await;
        drivers.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    
    /// Get updated config for a driver (for saving tokens etc.)
    /// 获取驱动更新后的配置（用于保存token等）
    pub async fn get_driver_updated_config(&self, id: &str) -> Option<serde_json::Value> {
        let drivers = self.drivers.read().await;
        if let Some(driver) = drivers.get(id) {
            driver.get_updated_config()
        } else {
            None
        }
    }
}
