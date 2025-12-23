//! OneDrive OAuth storage driver / OneDrive OAuth 存储驱动
//! 
//! Uses OAuth refresh_token authorization method / 使用OAuth refresh_token授权方式
//! Supports global, China (21Vianet), US Government, and Germany versions / 支持全球版、中国版等
//! Supports SharePoint and personal OneDrive / 支持SharePoint和个人OneDrive
//! Supports 302 redirect downloads / 支持302重定向下载

mod driver;

pub use driver::{
    OneDriveDriver,
    OneDriveConfig,
    OneDriveDriverFactory,
};
