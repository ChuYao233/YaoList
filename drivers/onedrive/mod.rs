pub mod driver;
pub mod oauth;

pub use driver::*;
pub use oauth::*;

use super::{DriverFactory, DriverInfo};

// OneDrive驱动工厂
pub struct OneDriveDriverFactory;

impl DriverFactory for OneDriveDriverFactory {
    fn driver_type(&self) -> &'static str {
        "onedrive"
    }

    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "onedrive".to_string(),
            display_name: "OneDrive".to_string(),
            description: "Microsoft OneDrive 云存储".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "region": {
                        "type": "string",
                        "title": "区域",
                        "description": "OneDrive 区域",
                        "enum": ["global", "cn", "us", "de"],
                        "enumNames": ["全球版", "中国版", "美国政府版", "德国版"],
                        "default": "global"
                    },
                    "is_sharepoint": {
                        "type": "boolean",
                        "title": "SharePoint 模式",
                        "description": "是否为 SharePoint 存储",
                        "default": false
                    },
                    "client_id": {
                        "type": "string",
                        "title": "客户端ID",
                        "description": "应用程序(客户端) ID",
                        "placeholder": "从 Azure 应用注册获取"
                    },
                    "client_secret": {
                        "type": "string",
                        "title": "客户端密码",
                        "description": "客户端密码",
                        "placeholder": "从 Azure 应用注册获取",
                        "format": "password"
                    },
                    "redirect_uri": {
                        "type": "string",
                        "title": "回调地址",
                        "description": "OAuth 回调地址",
                        "default": "http://localhost:3000/onedrive/callback"
                    },
                    "refresh_token": {
                        "type": "string",
                        "title": "刷新令牌",
                        "description": "通过OAuth获取的刷新令牌",
                        "placeholder": "通过OAuth授权获取"
                    },
                    "site_id": {
                        "type": "string",
                        "title": "站点ID",
                        "description": "SharePoint 站点 ID（仅SharePoint模式需要）",
                        "placeholder": "可选，仅SharePoint模式需要"
                    },
                    "chunk_size": {
                        "type": "integer",
                        "title": "分块大小(MB)",
                        "description": "分块上传大小",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 100
                    },
                    "custom_host": {
                        "type": "string",
                        "title": "自定义域名",
                        "description": "自定义下载域名",
                        "placeholder": "可选，自定义下载域名"
                    },
                    "proxy_download": {
                        "type": "boolean",
                        "title": "本地代理下载",
                        "description": "启用后通过服务器代理下载文件，禁用则直接重定向到 OneDrive 链接",
                        "default": false
                    }
                },
                "required": ["region", "client_id", "client_secret", "redirect_uri", "refresh_token"]
            }),
        }
    }

    fn create_driver(&self, config: serde_json::Value) -> anyhow::Result<Box<dyn super::Driver>> {
        let onedrive_config: OneDriveConfig = serde_json::from_value(config)?;
        Ok(Box::new(OneDriveDriver::new(onedrive_config)))
    }

    fn get_routes(&self) -> Option<axum::Router> {
        Some(create_oauth_routes())
    }
} 