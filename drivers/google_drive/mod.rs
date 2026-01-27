//! Google Drive 存储驱动
//! 
//! 支持 OAuth refresh_token 授权方式
//! 支持在线API刷新token（无需client_id/secret）
//! 支持流式上传（内存占用<40MB）
//! 支持直链下载、重命名、删除、创建文件夹等

mod driver;

pub use driver::{
    GoogleDriveDriver,
    GoogleDriveConfig,
    GoogleDriveDriverFactory,
};
