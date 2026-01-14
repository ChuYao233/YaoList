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
    use std::collections::HashMap;
    let path = fix_and_clean_path(path);
    // 使用 HashMap 来保存目录名和对应的 driver_id（如果是直接挂载点）
    let mut virtual_dirs: HashMap<String, Option<String>> = HashMap::new();
    
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
                    // 如果这个目录就是挂载点（没有更深的路径），记录 driver_id
                    let is_mount_point = relative == next_dir;
                    if is_mount_point {
                        virtual_dirs.insert(next_dir.to_string(), Some(mount.id.clone()));
                    } else if !virtual_dirs.contains_key(next_dir) {
                        virtual_dirs.insert(next_dir.to_string(), None);
                    }
                }
            }
        }
    }
    
    virtual_dirs.into_iter().map(|(name, driver_id)| {
        let mut obj = serde_json::json!({
            "name": name,
            "size": 0,
            "is_dir": true,
            "modified": "",
            "created": ""
        });
        if let Some(id) = driver_id {
            obj["driver_id"] = serde_json::json!(id);
        }
        obj
    }).collect()
}
