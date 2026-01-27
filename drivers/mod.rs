// Driver package / 驱动包
pub mod local;
pub mod onedrive;
pub mod onedrive_app;
pub mod quark;
pub mod ftp;
pub mod cloud189;
#[path = "123openapi/mod.rs"]
pub mod pan123_open;
pub mod smb;
pub mod webdav;
pub mod s3;
pub mod lanzou;
pub mod pikpak;
pub mod yun139;
pub mod pan115;
pub mod sftp;
pub mod pan123_share;
pub mod pan115_share;
pub mod google_drive;

use crate::storage::StorageManager;

/// Register all drivers to StorageManager / 注册所有驱动
pub async fn register_all(manager: &StorageManager) -> anyhow::Result<()> {
    // Register local driver (using LocalDriverFactory from storage module) / 注册本地驱动
    manager.register_factory(Box::new(crate::storage::LocalDriverFactory)).await?;
    // Register OneDrive driver (OAuth refresh_token method) / 注册OneDrive驱动
    manager.register_factory(Box::new(onedrive::OneDriveDriverFactory)).await?;
    // Register OneDrive App driver (client_credentials method) / 注册OneDrive App驱动
    manager.register_factory(Box::new(onedrive_app::OneDriveAppDriverFactory)).await?;
    // Register Quark cloud drive driver / 注册夸克网盘驱动
    manager.register_factory(Box::new(quark::QuarkDriverFactory)).await?;
    // Register FTP driver / 注册FTP驱动
    manager.register_factory(Box::new(ftp::FtpDriverFactory)).await?;
    // Register Cloud189 (China Telecom) cloud drive driver / 注册天翼云盘驱动
    manager.register_factory(Box::new(cloud189::Cloud189DriverFactory)).await?;
    // Register 123 Cloud Open Platform driver / 注册123云盘开放平台驱动
    manager.register_factory(Box::new(pan123_open::Pan123OpenDriverFactory)).await?;
    // Register SMB/CIFS driver / 注册SMB网络共享驱动
    manager.register_factory(Box::new(smb::SmbDriverFactory)).await?;
    // Register WebDAV driver / 注册WebDAV驱动
    manager.register_factory(Box::new(webdav::WebDavDriverFactory)).await?;
    // Register S3 driver / 注册S3对象存储驱动
    manager.register_factory(Box::new(s3::S3DriverFactory)).await?;
    // Register Lanzou Cloud driver / 注册蓝奏云驱动
    manager.register_factory(Box::new(lanzou::LanzouDriverFactory)).await?;
    // Register PikPak driver / 注册PikPak网盘驱动
    manager.register_factory(Box::new(pikpak::PikPakDriverFactory)).await?;
    // Register 139Yun (China Mobile) cloud drive driver / 注册中国移动云盘(139云)驱动
    manager.register_factory(Box::new(yun139::Yun139DriverFactory)).await?;
    // Register 115 Cloud driver / 注册115云盘驱动
    manager.register_factory(Box::new(pan115::Pan115DriverFactory)).await?;
    // Register SFTP driver / 注册 SFTP 驱动
    manager.register_factory(Box::new(sftp::SftpDriverFactory)).await?;
    // Register 123Pan Share driver / 注册123云盘分享驱动
    manager.register_factory(Box::new(pan123_share::Pan123ShareDriverFactory)).await?;
    // Register 115Pan Share driver / 注册115云盘分享驱动
    manager.register_factory(Box::new(pan115_share::Pan115ShareDriverFactory)).await?;
    // Register Google Drive driver / 注册Google Drive驱动
    manager.register_factory(Box::new(google_drive::GoogleDriveDriverFactory)).await?;
    Ok(())
}
