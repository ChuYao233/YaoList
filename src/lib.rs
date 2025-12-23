pub mod config;
pub mod models;
pub mod utils;
pub mod storage;
pub mod search;
pub mod load_balance;
pub mod geoip;
pub mod server;
pub mod download;

// Driver modules (point to project root drivers via path attribute) / 驱动模块
#[path = "../drivers/mod.rs"]
pub mod drivers;

// Register all storage drivers (call unified registration function from drivers module) / 注册所有存储驱动
pub async fn register_storage_drivers(manager: &storage::StorageManager) -> anyhow::Result<()> {
    drivers::register_all(manager).await
}
