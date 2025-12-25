//! PikPak cloud storage driver / PikPak网盘存储驱动
//!
//! Supports username/password and refresh_token authentication
//! 支持用户名密码和refresh_token认证
//!
//! Architecture principles / 架构原则:
//! - Driver only provides primitive capabilities (Reader/Writer) / 驱动只提供原语能力
//! - Core controls progress, concurrency, resume points / Core控制进度、并发、断点
//! - Never load files into memory, use streaming / 永远不把文件放内存，使用流式传输

pub mod types;
pub mod util;
pub mod client;
pub mod driver;
pub mod writer;

pub use driver::{
    PikPakDriver,
    PikPakConfig,
    PikPakDriverFactory,
};
