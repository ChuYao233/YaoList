//! OneDrive App storage driver / OneDrive 应用模式存储驱动
//! 
//! Uses client_credentials OAuth authorization method / 使用client_credentials OAuth授权方式
//! Supports global, China (21Vianet), US Government, and Germany versions / 支持全球版、中国版等
//! Supports direct upload and streaming upload / 支持直传和流式上传

pub mod types;
pub mod api;
pub mod writer;
pub mod driver;

pub use driver::{
    OneDriveAppDriver,
    OneDriveAppDriverFactory,
};
pub use types::OneDriveAppConfig;

