//! 夸克网盘存储驱动
//! 
//! 使用Cookie认证方式
//! 不支持302重定向，需要代理下载

mod driver;

pub use driver::{
    QuarkDriver,
    QuarkConfig,
    QuarkDriverFactory,
};
