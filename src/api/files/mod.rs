// Sub-modules
pub mod common;
pub mod list;
pub mod operations;
pub mod copy_move;
pub mod download;
pub mod upload;

// Re-exports
pub use common::*;
pub use list::*;
pub use operations::*;
pub use copy_move::*;
pub use download::*;
pub use upload::*;

use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize)]
pub struct FsListReq {
    pub path: Option<String>,
    pub password: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
    pub refresh: Option<bool>,
    pub sort_by: Option<String>,    // name, modified, size
    pub sort_order: Option<String>, // asc, desc
}

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub modified: String,
    pub created: String,
}

#[derive(Debug, Serialize)]
pub struct FsListResp {
    pub content: Vec<FileInfo>,
    pub total: i64,
    pub readme: String,
    pub write: bool,
    pub provider: String,
}

/// 获取路径下的虚拟目录
pub fn get_virtual_files_by_path(path: &str, mounts: &[crate::api::file_resolver::MountInfo]) -> Vec<serde_json::Value> {
    use yaolist_backend::utils::fix_and_clean_path;
    let path = fix_and_clean_path(path);
    let mut virtual_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
    
    for mount in mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        
        // 检查挂载路径是否以当前路径为前缀
        if mount_path.starts_with(&path) && mount_path != path {
            // 计算相对路径
            let relative = if path == "/" {
                mount_path[1..].to_string()
            } else {
                mount_path[path.len()..].trim_start_matches('/').to_string()
            };
            
            // 获取下一级目录名
            if let Some(next_dir) = relative.split('/').next() {
                if !next_dir.is_empty() {
                    virtual_dirs.insert(next_dir.to_string());
                }
            }
        }
    }
    
    virtual_dirs.into_iter().map(|name| {
        serde_json::json!({
            "name": name,
            "size": 0,
            "is_dir": true,
            "modified": "",
            "created": ""
        })
    }).collect()
}
