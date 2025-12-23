pub mod archive;
pub mod auth;
pub mod backup;
pub mod direct_links;
pub mod shares;
pub mod drivers;
pub mod extract;
pub mod file_resolver;
pub mod files;
pub mod groups;
pub mod load_balance;
pub mod meta;
pub mod mounts;
pub mod notification;
pub mod search;
pub mod server;
pub mod settings;
pub mod stats;
pub mod tasks;
pub mod users;
pub mod webdav;

use serde::Serialize;

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 200,
            message: "success".to_string(),
            data: Some(data),
        }
    }
    
    pub fn error(message: &str) -> Self {
        Self {
            code: 400,
            message: message.to_string(),
            data: None,
        }
    }
}
