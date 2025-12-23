use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use encoding_rs::GBK;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::ops::Range;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_cookies::Cookies;
use tokio::io::AsyncReadExt;
use tracing::debug;
use yaolist_backend::utils::{fix_and_clean_path, is_sub_path};

use crate::state::AppState;

// 简单的缓存结构
struct CacheEntry {
    entries: Vec<ArchiveEntry>,
    format: String,
    created: Instant,
}

lazy_static::lazy_static! {
    static ref ARCHIVE_CACHE: RwLock<HashMap<String, CacheEntry>> = RwLock::new(HashMap::new());
}

const CACHE_TTL: Duration = Duration::from_secs(300); // 5分钟缓存

#[derive(Debug, Deserialize)]
pub struct ArchiveListRequest {
    pub path: String,
    #[serde(default)]
    pub inner_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

struct MountInfo {
    id: String,
    mount_path: String,
}

/// 支持的压缩格式
fn get_archive_format(filename: &str) -> Option<&'static str> {
    let ext = filename.split('.').last()?.to_lowercase();
    match ext.as_str() {
        "zip" | "jar" | "war" | "apk" | "ipa" | "epub" => Some("zip"),
        _ => None,
    }
}

/// 根据路径找到最匹配的挂载点
fn get_storage_by_path<'a>(path: &str, mounts: &'a [MountInfo]) -> Option<&'a MountInfo> {
    let mut best_match: Option<&MountInfo> = None;
    let mut best_len = 0;
    
    for mount in mounts {
        let mount_path = fix_and_clean_path(&mount.mount_path);
        if is_sub_path(&mount_path, path) {
            if mount_path.len() > best_len {
                best_match = Some(mount);
                best_len = mount_path.len();
            }
        }
    }
    best_match
}

/// POST /api/fs/archive/list - 列出压缩文件内容（只读取中央目录，不读取整个文件）
pub async fn archive_list(
    State(state): State<Arc<AppState>>,
    _cookies: Cookies,
    Json(req): Json<ArchiveListRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path = fix_and_clean_path(&req.path);
    let inner_path = req.inner_path.trim_matches('/');
    let cache_key = format!("{}:{}", path, inner_path);
    
    // 检查缓存
    {
        let cache = ARCHIVE_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.created.elapsed() < CACHE_TTL {
                debug!("Archive cache hit: {}", cache_key);
                return Ok(Json(json!({
                    "code": 200,
                    "message": "success",
                    "data": {
                        "format": entry.format,
                        "entries": entry.entries
                    }
                })));
            }
        }
    }
    
    let filename = path.split('/').last().unwrap_or("");
    
    // 检查是否为支持的压缩格式
    let format = match get_archive_format(filename) {
        Some(f) => f,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": 400,
                    "message": "不支持的压缩格式，目前仅支持 ZIP"
                }))
            ));
        }
    };
    
    // 从 drivers 表读取挂载点信息
    let db_drivers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, config FROM drivers WHERE enabled = 1"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": 500, "message": e.to_string() }))
    ))?;
    
    // 构建挂载点列表
    let mounts: Vec<MountInfo> = db_drivers.iter().filter_map(|(id, config_str)| {
        let config: Value = serde_json::from_str(config_str).ok()?;
        let mount_path = config.get("mount_path").and_then(|v| v.as_str())?;
        Some(MountInfo {
            id: id.clone(),
            mount_path: mount_path.to_string(),
        })
    }).collect();
    
    // 找到匹配的挂载点
    let mount = get_storage_by_path(&path, &mounts).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({ "code": 404, "message": "未找到匹配的挂载点" }))
    ))?;
    
    let mount_path_str = fix_and_clean_path(&mount.mount_path);
    let actual_path = if path.len() > mount_path_str.len() {
        fix_and_clean_path(&path[mount_path_str.len()..])
    } else {
        "/".to_string()
    };
    
    // 获取驱动
    let driver = state.storage_manager.get_driver(&mount.id).await
        .ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": 404, "message": "驱动未找到" }))
        ))?;
    
    // 获取文件大小
    let parent_path = actual_path.rsplitn(2, '/').nth(1).unwrap_or("/");
    let file_name = actual_path.split('/').last().unwrap_or("");
    let entries_list = driver.list(parent_path).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": format!("获取文件信息失败: {}", e) }))
        ))?;
    
    let file_entry = entries_list.iter().find(|e| e.name == file_name)
        .ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": 404, "message": "文件未找到" }))
        ))?;
    
    let file_size = file_entry.size;
    
    // 读取文件末尾 65KB（EOCD 最大 65KB）找中央目录
    let eocd_search_size: u64 = 65536.min(file_size);
    let eocd_start = file_size - eocd_search_size;
    
    let mut eocd_reader = driver.open_reader(&actual_path, Some(Range { start: eocd_start, end: file_size })).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": format!("读取文件失败: {}", e) }))
        ))?;
    
    let mut eocd_data = Vec::with_capacity(eocd_search_size as usize);
    eocd_reader.read_to_end(&mut eocd_data).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": format!("读取 EOCD 失败: {}", e) }))
        ))?;
    
    // 查找 EOCD 签名 (0x06054b50)
    let eocd_sig: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    let eocd_pos = eocd_data.windows(4).rposition(|w| w == eocd_sig)
        .ok_or_else(|| (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": 400, "message": "无效的 ZIP 文件：找不到 EOCD" }))
        ))?;
    
    // 解析 EOCD
    if eocd_data.len() < eocd_pos + 22 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": 400, "message": "无效的 ZIP 文件：EOCD 不完整" }))
        ));
    }
    
    let mut cd_size = u32::from_le_bytes([
        eocd_data[eocd_pos + 12],
        eocd_data[eocd_pos + 13],
        eocd_data[eocd_pos + 14],
        eocd_data[eocd_pos + 15],
    ]) as u64;
    
    let mut cd_offset = u32::from_le_bytes([
        eocd_data[eocd_pos + 16],
        eocd_data[eocd_pos + 17],
        eocd_data[eocd_pos + 18],
        eocd_data[eocd_pos + 19],
    ]) as u64;
    
    debug!("ZIP EOCD: cd_size={}, cd_offset={}", cd_size, cd_offset);
    
    // 检查是否为 ZIP64（值为 0xFFFFFFFF 表示 ZIP64）
    if cd_size == 0xFFFFFFFF || cd_offset == 0xFFFFFFFF {
        // 查找 ZIP64 EOCD Locator (0x07064b50)
        let eocd64_loc_sig: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
        if let Some(loc_pos) = eocd_data.windows(4).rposition(|w| w == eocd64_loc_sig) {
            if loc_pos + 20 <= eocd_data.len() {
                // 获取 ZIP64 EOCD 的绝对偏移量
                let eocd64_offset = u64::from_le_bytes([
                    eocd_data[loc_pos + 8],
                    eocd_data[loc_pos + 9],
                    eocd_data[loc_pos + 10],
                    eocd_data[loc_pos + 11],
                    eocd_data[loc_pos + 12],
                    eocd_data[loc_pos + 13],
                    eocd_data[loc_pos + 14],
                    eocd_data[loc_pos + 15],
                ]);
                
                // 读取 ZIP64 EOCD（56 字节）
                let mut eocd64_reader = driver.open_reader(&actual_path, Some(Range { start: eocd64_offset, end: eocd64_offset + 56 })).await
                    .map_err(|e| (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "code": 500, "message": format!("读取 ZIP64 EOCD 失败: {}", e) }))
                    ))?;
                
                let mut eocd64_data = Vec::with_capacity(56);
                eocd64_reader.read_to_end(&mut eocd64_data).await
                    .map_err(|e| (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "code": 500, "message": format!("读取 ZIP64 EOCD 失败: {}", e) }))
                    ))?;
                
                // 验证 ZIP64 EOCD 签名 (0x06064b50)
                if eocd64_data.len() >= 56 && &eocd64_data[0..4] == &[0x50, 0x4b, 0x06, 0x06] {
                    cd_size = u64::from_le_bytes([
                        eocd64_data[40], eocd64_data[41], eocd64_data[42], eocd64_data[43],
                        eocd64_data[44], eocd64_data[45], eocd64_data[46], eocd64_data[47],
                    ]);
                    cd_offset = u64::from_le_bytes([
                        eocd64_data[48], eocd64_data[49], eocd64_data[50], eocd64_data[51],
                        eocd64_data[52], eocd64_data[53], eocd64_data[54], eocd64_data[55],
                    ]);
                    debug!("ZIP64 EOCD: cd_size={}, cd_offset={}", cd_size, cd_offset);
                }
            }
        }
    }
    
    debug!("Final: cd_size={}, cd_offset={}, file_size={}", cd_size, cd_offset, file_size);
    
    // 读取中央目录
    let mut cd_reader = driver.open_reader(&actual_path, Some(Range { start: cd_offset, end: cd_offset + cd_size })).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": format!("读取中央目录失败: {}", e) }))
        ))?;
    
    let mut cd_data = Vec::with_capacity(cd_size as usize);
    cd_reader.read_to_end(&mut cd_data).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": format!("读取中央目录失败: {}", e) }))
        ))?;
    
    debug!("Central directory data size: {} bytes", cd_data.len());
    
    // 解析中央目录获取文件列表
    let entries = parse_central_directory(&cd_data, &req.inner_path)?;
    
    debug!("Parsed {} entries", entries.len());
    
    // 写入缓存
    {
        let mut cache = ARCHIVE_CACHE.write().await;
        cache.insert(cache_key, CacheEntry {
            entries: entries.clone(),
            format: format.to_uppercase(),
            created: Instant::now(),
        });
    }
    
    Ok(Json(json!({
        "code": 200,
        "message": "success",
        "data": {
            "format": format.to_uppercase(),
            "entries": entries
        }
    })))
}

/// 解析 ZIP 中央目录
fn parse_central_directory(data: &[u8], inner_path: &str) -> Result<Vec<ArchiveEntry>, (StatusCode, Json<Value>)> {
    let inner_path = inner_path.trim_matches('/');
    let prefix = if inner_path.is_empty() { String::new() } else { format!("{}/", inner_path) };
    
    let mut entries = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();
    let mut pos = 0;
    
    let cd_sig: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    
    while pos + 46 <= data.len() {
        // 检查中央目录文件头签名
        if &data[pos..pos + 4] != cd_sig {
            break;
        }
        
        let filename_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
        
        if pos + 46 + filename_len > data.len() {
            break;
        }
        
        let filename_bytes = &data[pos + 46..pos + 46 + filename_len];
        let is_dir = filename_bytes.last() == Some(&b'/');
        
        // 解码文件名：先尝试 UTF-8，失败则用 GBK
        let full_path = match std::str::from_utf8(filename_bytes) {
            Ok(s) => s.trim_end_matches('/').to_string(),
            Err(_) => {
                let (decoded, _, _) = GBK.decode(filename_bytes);
                decoded.trim_end_matches('/').to_string()
            }
        };
        
        // 过滤：只显示指定目录下的直接子项
        let should_include = if prefix.is_empty() {
            true
        } else {
            full_path.starts_with(&prefix) || full_path == inner_path
        };
        
        if should_include && !full_path.is_empty() {
            let relative_path = if prefix.is_empty() {
                full_path.to_string()
            } else if full_path.len() > prefix.len() {
                full_path[prefix.len()..].to_string()
            } else {
                String::new()
            };
            
            if !relative_path.is_empty() {
                let parts: Vec<&str> = relative_path.split('/').collect();
                
                if parts.len() > 1 {
                    // 深层子项，添加父目录
                    let dir_name = parts[0];
                    if !seen_dirs.contains(dir_name) {
                        seen_dirs.insert(dir_name.to_string());
                        entries.push(ArchiveEntry {
                            name: dir_name.to_string(),
                            path: if prefix.is_empty() { 
                                dir_name.to_string() 
                            } else { 
                                format!("{}{}", prefix, dir_name) 
                            },
                            is_dir: true,
                        });
                    }
                } else {
                    let name = parts[0].to_string();
                    if !is_dir || !seen_dirs.contains(&name) {
                        if is_dir {
                            seen_dirs.insert(name.clone());
                        }
                        entries.push(ArchiveEntry {
                            name,
                            path: full_path.to_string(),
                            is_dir,
                        });
                    }
                }
            }
        }
        
        pos += 46 + filename_len + extra_len + comment_len;
    }
    
    // 排序：目录在前，然后按名称排序
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    
    Ok(entries)
}
