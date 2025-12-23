/// Path processing utility functions / 路径处理工具函数

/// Clean and normalize path / 清理和规范化路径
/// 1. Replace backslashes with forward slashes / 将反斜杠替换为正斜杠
/// 2. Ensure path starts with / / 确保路径以 / 开头
/// 3. Clean . and .. in path / 清理路径中的 . 和 ..
pub fn fix_and_clean_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{}", path)
    };
    
    // Clean path / 清理路径
    clean_path(&path)
}

/// Clean path, handle ., .. and duplicate / / 清理路径，处理 . 和 .. 和重复的 /
fn clean_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Check if paths are equal / 判断路径是否相等
pub fn path_equal(path1: &str, path2: &str) -> bool {
    fix_and_clean_path(path1) == fix_and_clean_path(path2)
}

/// Check if sub_path is a subpath of path / 判断 sub_path 是否是 path 的子路径
pub fn is_sub_path(path: &str, sub_path: &str) -> bool {
    let path = fix_and_clean_path(path);
    let sub_path = fix_and_clean_path(sub_path);
    
    if path == sub_path {
        return true;
    }
    
    let path_with_sep = if path.ends_with('/') {
        path
    } else {
        format!("{}/", path)
    };
    
    sub_path.starts_with(&path_with_sep)
}

/// Get file extension (lowercase) / 获取文件扩展名
pub fn get_ext(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// File name conflict handling strategy / 文件名冲突处理策略
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConflictStrategy {
    /// Auto rename: file.txt -> file (1).txt / 自动重命名
    AutoRename,
    /// Overwrite / 覆盖
    Overwrite,
    /// Skip / 跳过
    Skip,
    /// Error / 报错
    Error,
}

/// Generate conflict-free filename / 生成不冲突的文件名
/// If filename exists, add (1), (2) etc. suffixes / 如果文件名已存在，添加后缀
/// Input: "file.txt", existing list: ["file.txt", "file (1).txt"] / 输入
/// Output: "file (2).txt" / 输出
pub fn resolve_conflict_name(name: &str, existing_names: &[String]) -> String {
    if !existing_names.contains(&name.to_string()) {
        return name.to_string();
    }
    
    // Separate filename and extension / 分离文件名和扩展名
    let path = std::path::Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|e| e.to_str());
    
    // Check if already has (n) suffix / 检查是否已有后缀
    let base_stem = if let Some(pos) = stem.rfind(" (") {
        if stem.ends_with(')') {
            let num_part = &stem[pos+2..stem.len()-1];
            if num_part.chars().all(|c| c.is_ascii_digit()) {
                &stem[..pos]
            } else {
                stem
            }
        } else {
            stem
        }
    } else {
        stem
    };
    
    // Find available sequence number / 找到可用的序号
    for i in 1..10000 {
        let new_name = if let Some(e) = ext {
            format!("{} ({}).{}", base_stem, i, e)
        } else {
            format!("{} ({})", base_stem, i)
        };
        
        if !existing_names.contains(&new_name) {
            return new_name;
        }
    }
    
    // 极端情况：使用时间戳
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    
    if let Some(e) = ext {
        format!("{}_{}.{}", base_stem, timestamp, e)
    } else {
        format!("{}_{}", base_stem, timestamp)
    }
}

/// 从挂载路径中提取实际路径
/// mount_path: 挂载点路径，如 "/local"
/// raw_path: 请求的完整路径，如 "/local/documents"
/// 返回实际路径: "/documents"
pub fn get_actual_path(mount_path: &str, raw_path: &str) -> String {
    let mount_path = fix_and_clean_path(mount_path);
    let raw_path = fix_and_clean_path(raw_path);
    
    let actual = raw_path.strip_prefix(&mount_path).unwrap_or(&raw_path);
    fix_and_clean_path(actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fix_and_clean_path() {
        assert_eq!(fix_and_clean_path(""), "/");
        assert_eq!(fix_and_clean_path("."), "/");
        assert_eq!(fix_and_clean_path(".."), "/");
        assert_eq!(fix_and_clean_path("../.."), "/");
        assert_eq!(fix_and_clean_path("a/b/c"), "/a/b/c");
        assert_eq!(fix_and_clean_path("/a/b/c"), "/a/b/c");
        assert_eq!(fix_and_clean_path("a\\b\\c"), "/a/b/c");
        assert_eq!(fix_and_clean_path("/a//b///c"), "/a/b/c");
        assert_eq!(fix_and_clean_path("/a/./b/../c"), "/a/c");
    }
    
    #[test]
    fn test_get_actual_path() {
        assert_eq!(get_actual_path("/local", "/local/documents"), "/documents");
        assert_eq!(get_actual_path("/local", "/local"), "/");
        assert_eq!(get_actual_path("/", "/documents"), "/documents");
    }
}

/// Check if file should be hidden based on patterns / 检查文件是否应该被隐藏
pub fn should_hide_file(filename: &str, hide_patterns: &str) -> bool {
    if hide_patterns.is_empty() {
        return false;
    }
    
    for pattern in hide_patterns.lines() {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        
        // Try to match as regex / 尝试作为正则表达式匹配
        if let Ok(re) = regex::Regex::new(pattern) {
            if re.is_match(filename) {
                return true;
            }
        }
    }
    
    false
}
