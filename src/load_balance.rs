//! Load balancing module - Core layer implementation / 负载均衡模块
//! 
//! Follow architecture principles / 遵循架构原则：
//! - Core only calls Driver, Driver only provides capabilities / Core只调用Driver
//! - Load balancing logic completely in Core layer, drivers unaware of these concepts / 负载均衡逻辑
//! - Use Capability to declare driver capabilities (can_redirect, etc.) / 使用Capability声明

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use std::net::IpAddr;
use serde::{Deserialize, Serialize};
use crate::geoip;

/// Load balancing mode / 负载均衡模式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadBalanceMode {
    /// Weighted round robin (default) - distribute requests proportionally by weight / 加权轮询
    WeightedRoundRobin,
    /// IP hash distribution / IP哈希分流
    IpHash,
    /// Geographic region distribution (domestic/foreign) / 按地区分流
    GeoRegion,
}

impl From<&str> for LoadBalanceMode {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "weighted_round_robin" | "weightedroundrobin" | "weighted" | "round_robin" => LoadBalanceMode::WeightedRoundRobin,
            "ip_hash" | "iphash" => LoadBalanceMode::IpHash,
            "geo_region" | "georegion" => LoadBalanceMode::GeoRegion,
            _ => LoadBalanceMode::WeightedRoundRobin,
        }
    }
}

impl ToString for LoadBalanceMode {
    fn to_string(&self) -> String {
        match self {
            LoadBalanceMode::WeightedRoundRobin => "weighted_round_robin".to_string(),
            LoadBalanceMode::IpHash => "ip_hash".to_string(),
            LoadBalanceMode::GeoRegion => "geo_region".to_string(),
        }
    }
}

/// Driver capability declaration / 驱动能力声明
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriverCapability {
    /// Whether 302 redirect is supported / 是否支持302重定向
    pub can_redirect: bool,
    /// Whether range reading is supported / 是否支持范围读取
    pub can_range_read: bool,
    /// Whether direct download link is supported / 是否支持直接下载链接
    pub can_direct_link: bool,
}

/// Geographic distribution configuration / 地区分流配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoRegionConfig {
    /// Domestic driver ID list / 国内驱动ID列表
    pub china_drivers: Vec<String>,
    /// Foreign driver ID list / 国外驱动ID列表  
    pub overseas_drivers: Vec<String>,
}

/// Driver information in load balancing group / 负载均衡组中的驱动信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceDriver {
    pub driver_id: String,
    pub driver_name: String,
    pub mount_path: String,
    pub weight: u32,
    pub capability: DriverCapability,
    pub order: i32,
    /// Whether it's a domestic node (for geographic distribution) / 是否为国内节点
    pub is_china_node: bool,
}

/// Load balancing group configuration (for serialization) / 负载均衡组配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceGroupConfig {
    pub name: String,
    pub mode: LoadBalanceMode,
    pub drivers: Vec<BalanceDriver>,
    pub enabled: bool,
}

/// Load balancing group (runtime) / 负载均衡组
#[derive(Debug)]
pub struct BalanceGroup {
    pub name: String,
    pub mode: LoadBalanceMode,
    pub drivers: Vec<BalanceDriver>,
    pub enabled: bool,
    /// Round robin counter / 轮询计数器
    counter: AtomicUsize,
}

impl BalanceGroup {
    pub fn new(name: String, mode: LoadBalanceMode) -> Self {
        Self {
            name,
            mode,
            drivers: Vec::new(),
            enabled: true,
            counter: AtomicUsize::new(0),
        }
    }
    
    pub fn from_config(config: BalanceGroupConfig) -> Self {
        let mut group = Self::new(config.name, config.mode);
        group.enabled = config.enabled;
        group.drivers = config.drivers;
        group.drivers.sort_by_key(|d| d.order);
        group
    }
    
    pub fn to_config(&self) -> BalanceGroupConfig {
        BalanceGroupConfig {
            name: self.name.clone(),
            mode: self.mode.clone(),
            drivers: self.drivers.clone(),
            enabled: self.enabled,
        }
    }

    /// 添加驱动到组
    pub fn add_driver(&mut self, driver: BalanceDriver) {
        self.drivers.push(driver);
        // 按order排序，order小的在前
        self.drivers.sort_by_key(|d| d.order);
    }

    /// 选择驱动进行下载/预览
    /// 
    /// 策略：
    /// - WeightedRoundRobin：按权重比例轮询分配
    /// - IpHash：根据IP哈希选择
    /// - GeoRegion：按地区分流
    pub fn select_driver(&self, client_ip: Option<IpAddr>, _file_name: &str) -> Option<&BalanceDriver> {
        if self.drivers.is_empty() {
            return None;
        }

        match self.mode {
            LoadBalanceMode::WeightedRoundRobin => {
                // 加权轮询
                self.select_by_weight()
            }
            LoadBalanceMode::IpHash => {
                // IP哈希分流
                if let Some(ip) = client_ip {
                    let hash = Self::hash_ip(&ip);
                    let idx = hash % self.drivers.len();
                    Some(&self.drivers[idx])
                } else {
                    // 没有IP，回退到加权轮询
                    self.select_by_weight()
                }
            }
            LoadBalanceMode::GeoRegion => {
                // 按地区分流
                self.select_by_geo(client_ip)
            }
        }
    }
    
    /// 按地区选择（国内/国外）
    fn select_by_geo(&self, client_ip: Option<IpAddr>) -> Option<&BalanceDriver> {
        let is_china = client_ip
            .map(|ip| geoip::is_china_ip(ip))
            .unwrap_or(false);
        
        // 根据地区过滤驱动
        let region_drivers: Vec<_> = self.drivers.iter()
            .filter(|d| d.is_china_node == is_china)
            .collect();
        
        if !region_drivers.is_empty() {
            // 在同地区驱动中轮询
            let idx = self.counter.fetch_add(1, Ordering::Relaxed) % region_drivers.len();
            return Some(region_drivers[idx]);
        }
        
        // 没有匹配地区的驱动，回退到所有驱动轮询
        if !self.drivers.is_empty() {
            let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.drivers.len();
            return Some(&self.drivers[idx]);
        }
        
        None
    }

    /// IP哈希函数
    fn hash_ip(ip: &IpAddr) -> usize {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        ip.hash(&mut hasher);
        hasher.finish() as usize
    }

    /// 按权重选择
    fn select_by_weight(&self) -> Option<&BalanceDriver> {
        let total_weight: u32 = self.drivers.iter().map(|d| d.weight).sum();
        if total_weight == 0 {
            return self.drivers.first();
        }

        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let target = (counter as u32) % total_weight;
        
        let mut accumulated = 0u32;
        for driver in &self.drivers {
            accumulated += driver.weight;
            if target < accumulated {
                return Some(driver);
            }
        }
        
        self.drivers.last()
    }
}

/// 负载均衡管理器
pub struct LoadBalanceManager {
    /// 按挂载路径分组（同一mount_path的多个驱动）
    mount_groups: RwLock<HashMap<String, BalanceGroup>>,
    /// 按负载均衡组名分组
    named_groups: RwLock<HashMap<String, BalanceGroup>>,
}

impl LoadBalanceManager {
    pub fn new() -> Self {
        Self {
            mount_groups: RwLock::new(HashMap::new()),
            named_groups: RwLock::new(HashMap::new()),
        }
    }

    /// 注册驱动到负载均衡管理器
    pub async fn register_driver(
        &self,
        driver_id: String,
        driver_name: String,
        mount_path: String,
        weight: u32,
        mode: LoadBalanceMode,
        group_name: Option<String>,
        capability: DriverCapability,
        order: i32,
        is_china_node: bool,
    ) {
        let driver = BalanceDriver {
            driver_id,
            driver_name,
            mount_path: mount_path.clone(),
            weight,
            capability,
            order,
            is_china_node,
        };

        // 如果指定了组名，加入命名组
        if let Some(name) = group_name {
            let mut groups = self.named_groups.write().await;
            let group = groups.entry(name.clone()).or_insert_with(|| {
                BalanceGroup::new(name, mode.clone())
            });
            group.add_driver(driver.clone());
        }

        // 同时按挂载路径分组（用于别名功能）
        let mut mount_groups = self.mount_groups.write().await;
        let group = mount_groups.entry(mount_path.clone()).or_insert_with(|| {
            BalanceGroup::new(mount_path, mode)
        });
        group.add_driver(driver);
    }
    
    /// 从配置创建负载均衡组
    pub async fn create_group(&self, config: BalanceGroupConfig) {
        let group = BalanceGroup::from_config(config);
        let name = group.name.clone();
        self.named_groups.write().await.insert(name, group);
    }
    
    /// 获取所有命名组配置
    pub async fn get_all_groups(&self) -> Vec<BalanceGroupConfig> {
        self.named_groups.read().await
            .values()
            .map(|g| g.to_config())
            .collect()
    }
    
    /// 获取指定组配置
    pub async fn get_group(&self, name: &str) -> Option<BalanceGroupConfig> {
        self.named_groups.read().await
            .get(name)
            .map(|g| g.to_config())
    }
    
    /// 删除负载均衡组
    pub async fn delete_group(&self, name: &str) -> bool {
        self.named_groups.write().await.remove(name).is_some()
    }
    
    /// 更新负载均衡组
    pub async fn update_group(&self, config: BalanceGroupConfig) {
        let group = BalanceGroup::from_config(config);
        let name = group.name.clone();
        self.named_groups.write().await.insert(name, group);
    }
    
    /// 从命名组选择驱动
    pub async fn select_from_group(
        &self,
        group_name: &str,
        client_ip: Option<IpAddr>,
        file_name: &str,
    ) -> Option<BalanceDriver> {
        let groups = self.named_groups.read().await;
        if let Some(group) = groups.get(group_name) {
            if group.enabled {
                return group.select_driver(client_ip, file_name).cloned();
            }
        }
        None
    }

    /// 清除所有注册
    pub async fn clear(&self) {
        self.mount_groups.write().await.clear();
        self.named_groups.write().await.clear();
    }

    /// 获取挂载路径下的所有驱动（用于文件列表合并）
    pub async fn get_drivers_for_path(&self, mount_path: &str) -> Vec<BalanceDriver> {
        let groups = self.mount_groups.read().await;
        if let Some(group) = groups.get(mount_path) {
            group.drivers.clone()
        } else {
            Vec::new()
        }
    }

    /// 选择驱动进行下载
    pub async fn select_driver_for_download(
        &self,
        mount_path: &str,
        client_ip: Option<IpAddr>,
        file_name: &str,
    ) -> Option<BalanceDriver> {
        let groups = self.mount_groups.read().await;
        if let Some(group) = groups.get(mount_path) {
            group.select_driver(client_ip, file_name).cloned()
        } else {
            None
        }
    }

    /// 检查路径是否有多个驱动（别名）
    pub async fn has_alias(&self, mount_path: &str) -> bool {
        let groups = self.mount_groups.read().await;
        if let Some(group) = groups.get(mount_path) {
            group.drivers.len() > 1
        } else {
            false
        }
    }
}

impl Default for LoadBalanceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_balance_mode_from_str() {
        assert_eq!(LoadBalanceMode::from("weighted_round_robin"), LoadBalanceMode::WeightedRoundRobin);
        assert_eq!(LoadBalanceMode::from("ip_hash"), LoadBalanceMode::IpHash);
        assert_eq!(LoadBalanceMode::from("geo_region"), LoadBalanceMode::GeoRegion);
        assert_eq!(LoadBalanceMode::from("unknown"), LoadBalanceMode::WeightedRoundRobin);
    }

    #[tokio::test]
    async fn test_balance_group_weighted_round_robin() {
        let mut group = BalanceGroup::new("test".to_string(), LoadBalanceMode::WeightedRoundRobin);
        
        for i in 0..3 {
            group.add_driver(BalanceDriver {
                driver_id: format!("driver_{}", i),
                driver_name: format!("Driver {}", i),
                mount_path: "/test".to_string(),
                weight: 1,
                capability: DriverCapability::default(),
                order: i,
                is_china_node: false,
            });
        }

        // 轮询应该按顺序返回
        for _round in 0..2 {
            for i in 0..3 {
                let selected = group.select_driver(None, "test.txt");
                assert_eq!(selected.unwrap().driver_id, format!("driver_{}", i));
            }
        }
    }

    #[tokio::test]
    async fn test_balance_group_redirect_priority() {
        let mut group = BalanceGroup::new("test".to_string(), LoadBalanceMode::None);
        
        group.add_driver(BalanceDriver {
            driver_id: "local".to_string(),
            driver_name: "Local".to_string(),
            mount_path: "/test".to_string(),
            weight: 1,
            capability: DriverCapability { can_redirect: false, ..Default::default() },
            order: 0,
            is_china_node: true,
        });
        
        group.add_driver(BalanceDriver {
            driver_id: "cloud".to_string(),
            driver_name: "Cloud".to_string(),
            mount_path: "/test".to_string(),
            weight: 1,
            capability: DriverCapability { can_redirect: true, ..Default::default() },
            order: 1,
            is_china_node: false,
        });

        let selected = group.select_driver(None, "test.txt");
        assert_eq!(selected.unwrap().driver_id, "cloud");
    }
}
