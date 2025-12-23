//! S3对象存储驱动
//!
//! 支持标准S3协议及兼容存储：
//! - AWS S3
//! - 阿里云OSS
//! - 腾讯云COS
//! - MinIO
//! - 七牛云Kodo
//! - 华为云OBS
//! - 其他S3兼容存储
//!
//! 设计原则：
//! - 只提供原语（open_reader, open_writer, list等）
//! - 流式IO，不把文件放内存
//! - 支持预签名URL直链（302）
//! - 不支持时回退到本地代理

mod config;
mod driver;
mod factory;

pub use config::S3Config;
pub use driver::S3Driver;
pub use factory::S3DriverFactory;
