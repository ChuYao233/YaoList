//! Application configuration module / 应用配置模块
//!
//! Manages application configuration loaded from config.json
//! Creates default config file on first run / 首次运行时创建默认配置文件

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Global configuration instance / 全局配置实例
static CONFIG: OnceCell<Arc<RwLock<AppConfig>>> = OnceCell::new();

/// Application configuration / 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Server configuration / 服务器配置
    pub server: ServerConfig,
    /// Database configuration / 数据库配置
    pub database: DatabaseConfig,
    /// Search configuration / 搜索配置
    pub search: SearchConfig,
    /// GeoIP configuration / GeoIP配置
    pub geoip: GeoIpConfig,
}

/// Server configuration / 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server host address / 服务器监听地址
    pub host: String,
    /// Server port / 服务器端口
    pub port: u16,
}

/// Database configuration / 数据库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Data directory path / 数据目录路径
    pub data_dir: String,
    /// Main database file path (relative to data_dir) / 主数据库文件路径
    pub db_file: String,
}

/// Search configuration / 搜索配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Search database directory (relative to data_dir) / 搜索数据库目录
    pub db_dir: String,
    /// Search database file name / 搜索数据库文件名
    pub db_file: String,
}

/// GeoIP configuration / GeoIP配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoIpConfig {
    /// GeoIP database directory (relative to data_dir) / GeoIP数据库目录
    pub db_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            search: SearchConfig::default(),
            geoip: GeoIpConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8180,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
            db_file: "yaolist.db".to_string(),
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            db_dir: "search".to_string(),
            db_file: "search.db".to_string(),
        }
    }
}

impl Default for GeoIpConfig {
    fn default() -> Self {
        Self {
            db_dir: "".to_string(), // Empty means same as data_dir
        }
    }
}

impl AppConfig {
    /// Get the full database URL / 获取完整的数据库URL
    pub fn get_database_url(&self) -> String {
        let db_path = Path::new(&self.database.data_dir).join(&self.database.db_file);
        format!("sqlite:{}?mode=rwc", db_path.to_string_lossy())
    }

    /// Get the full data directory path / 获取完整的数据目录路径
    pub fn get_data_dir(&self) -> PathBuf {
        PathBuf::from(&self.database.data_dir)
    }

    /// Get the full search database path / 获取完整的搜索数据库路径
    pub fn get_search_db_path(&self) -> PathBuf {
        let data_dir = self.get_data_dir();
        if self.search.db_dir.is_empty() {
            data_dir.join(&self.search.db_file)
        } else {
            data_dir.join(&self.search.db_dir).join(&self.search.db_file)
        }
    }
    
    /// Get search database directory / 获取搜索数据库目录
    pub fn get_search_db_dir(&self) -> PathBuf {
        let data_dir = self.get_data_dir();
        if self.search.db_dir.is_empty() {
            data_dir
        } else {
            data_dir.join(&self.search.db_dir)
        }
    }
    
    /// Get search database path for specific driver / 获取指定存储的搜索数据库路径
    pub fn get_driver_search_db_path(&self, driver_id: &str) -> PathBuf {
        let search_dir = self.get_search_db_dir();
        search_dir.join(format!("index_{}.db", driver_id))
    }

    /// Get the GeoIP database directory / 获取GeoIP数据库目录
    pub fn get_geoip_dir(&self) -> PathBuf {
        let data_dir = self.get_data_dir();
        if self.geoip.db_dir.is_empty() {
            data_dir
        } else {
            data_dir.join(&self.geoip.db_dir)
        }
    }

    /// Get the server bind address / 获取服务器绑定地址
    pub fn get_bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

/// Get the config file path / 获取配置文件路径
fn get_config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json")
}

/// Load configuration from file, or create default if not exists / 加载配置文件，不存在则创建默认配置
pub fn load_config() -> Result<AppConfig, String> {
    let config_path = get_config_path();

    if config_path.exists() {
        // Load existing config / 加载现有配置
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        let config: AppConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        
        tracing::info!("Loaded configuration from {:?}", config_path);
        Ok(config)
    } else {
        // Create default config / 创建默认配置
        let config = AppConfig::default();
        save_config(&config)?;
        tracing::info!("Created default configuration at {:?}", config_path);
        Ok(config)
    }
}

/// Save configuration to file / 保存配置到文件
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let config_path = get_config_path();
    
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    std::fs::write(&config_path, content)
        .map_err(|e| format!("Failed to write config file: {}", e))?;
    
    Ok(())
}

/// Initialize global configuration / 初始化全局配置
pub fn init_config() -> Result<Arc<RwLock<AppConfig>>, String> {
    let config = load_config()?;
    
    let config_arc = Arc::new(RwLock::new(config));
    
    CONFIG.set(config_arc.clone())
        .map_err(|_| "Config already initialized".to_string())?;
    
    Ok(config_arc)
}

/// Get global configuration instance / 获取全局配置实例
pub fn get_config() -> Arc<RwLock<AppConfig>> {
    CONFIG.get_or_init(|| {
        let config = load_config().unwrap_or_default();
        Arc::new(RwLock::new(config))
    }).clone()
}

/// Get a read-only snapshot of current config / 获取当前配置的只读快照
pub fn config() -> AppConfig {
    get_config().read().clone()
}
