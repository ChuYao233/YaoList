//! 蓝奏云数据类型

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, NaiveDateTime, TimeZone};

/// 文件或文件夹信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOrFolder {
    pub name: String,
    pub name_all: Option<String>,
    pub id: String,
    pub fol_id: Option<String>,
    pub size: Option<String>,
    pub time: Option<String>,
    pub downs: Option<String>,
    pub onof: Option<String>,
    pub is_newd: Option<String>,
    #[serde(default)]
    pub is_dir: bool,
}

impl FileOrFolder {
    pub fn get_name(&self) -> &str {
        self.name_all.as_deref().unwrap_or(&self.name)
    }
    
    pub fn get_id(&self) -> &str {
        self.fol_id.as_deref().unwrap_or(&self.id)
    }
    
    pub fn get_size(&self) -> u64 {
        self.size.as_ref().map(|s| parse_size(s)).unwrap_or(0)
    }
    
    pub fn get_modified(&self) -> Option<DateTime<Utc>> {
        self.time.as_ref().and_then(|t| parse_time(t))
    }
}

/// 文件分享信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileShare {
    pub f_id: String,
    pub is_newd: String,
    pub pwd: String,
    pub onof: String,
    pub taession: Option<String>,
}

/// 通过分享URL获取的文件/文件夹
#[derive(Debug, Clone)]
pub struct FileByShareUrl {
    pub name: String,
    pub id: String,
    pub size: u64,
    pub time: Option<DateTime<Utc>>,
    pub is_dir: bool,
}

/// API响应基类
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub zt: i32,
    pub info: Option<T>,
    pub text: Option<Vec<T>>,
}

/// 文件夹列表响应
#[derive(Debug, Deserialize)]
pub struct FolderListResponse {
    pub zt: i32,
    #[serde(default)]
    pub info: serde_json::Value,
    pub text: Option<Vec<FolderInfo>>,
}

/// 文件夹信息
#[derive(Debug, Clone, Deserialize)]
pub struct FolderInfo {
    pub name: String,
    pub fol_id: String,
    pub folderdes: Option<String>,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
pub struct FileListResponse {
    pub zt: i32,
    #[serde(default)]
    pub info: serde_json::Value,
    pub text: Option<Vec<FileInfo>>,
}

/// 文件信息
#[derive(Debug, Clone, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub name_all: Option<String>,
    pub id: String,
    pub size: String,
    pub time: String,
    pub downs: Option<String>,
    pub onof: Option<String>,
    pub is_newd: Option<String>,
}

impl FileInfo {
    pub fn get_name(&self) -> &str {
        self.name_all.as_deref().unwrap_or(&self.name)
    }
}

/// 上传响应
#[derive(Debug, Deserialize)]
pub struct UploadResponse {
    pub zt: i32,
    pub info: Option<String>,
    pub text: Option<Vec<UploadedFile>>,
}

/// 上传的文件信息
#[derive(Debug, Clone, Deserialize)]
pub struct UploadedFile {
    pub id: String,
    pub f_id: String,
    pub name_all: String,
    pub is_newd: String,
}

/// 下载信息响应
#[derive(Debug, Deserialize)]
pub struct DownloadInfoResponse {
    pub zt: i32,
    pub dom: Option<String>,
    pub url: Option<String>,
    pub inf: Option<String>,
}

/// 空间信息
#[derive(Debug, Deserialize)]
pub struct SpaceInfoResponse {
    pub zt: i32,
    pub info: Option<SpaceData>,
}

#[derive(Debug, Deserialize)]
pub struct SpaceData {
    pub all_size: Option<String>,
    pub now_size: Option<String>,
}

/// 解析大小字符串（如 "1.5 M", "500 K", "2 G"）
pub fn parse_size(size_str: &str) -> u64 {
    let s = size_str.trim().to_uppercase();
    
    // 尝试解析数字部分
    let mut num_str = String::new();
    let mut unit = String::new();
    
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            num_str.push(c);
        } else if c.is_alphabetic() {
            unit.push(c);
        }
    }
    
    let num: f64 = num_str.parse().unwrap_or(0.0);
    
    let multiplier = match unit.as_str() {
        "B" | "" => 1u64,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        "T" | "TB" => 1024 * 1024 * 1024 * 1024,
        _ => 1,
    };
    
    (num * multiplier as f64) as u64
}

/// 解析时间字符串
pub fn parse_time(time_str: &str) -> Option<DateTime<Utc>> {
    let s = time_str.trim();
    
    // 格式: "2024-01-15 12:30"
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some(Utc.from_utc_datetime(&dt));
    }
    
    // 格式: "2024-01-15"
    if let Ok(dt) = NaiveDateTime::parse_from_str(&format!("{} 00:00", s), "%Y-%m-%d %H:%M") {
        return Some(Utc.from_utc_datetime(&dt));
    }
    
    // 格式: "1天前", "2小时前" 等
    if s.contains("前") {
        let now = Utc::now();
        if s.contains("秒") {
            let n: i64 = s.replace("秒前", "").trim().parse().unwrap_or(0);
            return Some(now - chrono::Duration::seconds(n));
        }
        if s.contains("分") {
            let n: i64 = s.replace("分钟前", "").trim().parse().unwrap_or(0);
            return Some(now - chrono::Duration::minutes(n));
        }
        if s.contains("时") || s.contains("小时") {
            let n: i64 = s.replace("小时前", "").replace("时前", "").trim().parse().unwrap_or(0);
            return Some(now - chrono::Duration::hours(n));
        }
        if s.contains("天") {
            let n: i64 = s.replace("天前", "").trim().parse().unwrap_or(0);
            return Some(now - chrono::Duration::days(n));
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("1.5 M"), 1572864);
        assert_eq!(parse_size("500 K"), 512000);
        assert_eq!(parse_size("2 G"), 2147483648);
        assert_eq!(parse_size("100"), 100);
    }
}
