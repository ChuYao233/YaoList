//! 123云盘开放平台驱动模块
//! 123 Cloud Open Platform driver module
//!
//! 基于123云盘开放API实现的存储驱动
//! Storage driver implementation based on 123 Cloud Open API
//!
//! API文档 / API Documentation: https://www.123pan.com/developer

mod types;
mod api;
mod driver;
mod upload;

pub use driver::{Pan123OpenDriver, Pan123OpenDriverFactory};
pub use types::Pan123OpenConfig;
