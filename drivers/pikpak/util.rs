//! PikPak utility functions / PikPak工具函数

use md5;
use sha1::{Sha1, Digest};

/// Platform configurations / 平台配置
pub mod platform {
    /// Android platform constants / Android平台常量
    pub mod android {
        pub const CLIENT_ID: &str = "YNxT9w7GMdWvEOKa";
        pub const CLIENT_SECRET: &str = "dbw2OtmVEeuUvIptb1Coyg";
        pub const CLIENT_VERSION: &str = "1.53.2";
        pub const PACKAGE_NAME: &str = "com.pikcloud.pikpak";
        pub const SDK_VERSION: &str = "2.0.6.206003";
        pub const ALGORITHMS: &[&str] = &[
            "SOP04dGzk0TNO7t7t9ekDbAmx+eq0OI1ovEx",
            "nVBjhYiND4hZ2NCGyV5beamIr7k6ifAsAbl",
            "Ddjpt5B/Cit6EDq2a6cXgxY9lkEIOw4yC1GDF28KrA",
            "VVCogcmSNIVvgV6U+AochorydiSymi68YVNGiz",
            "u5ujk5sM62gpJOsB/1Gu/zsfgfZO",
            "dXYIiBOAHZgzSruaQ2Nhrqc2im",
            "z5jUTBSIpBN9g4qSJGlidNAutX6",
            "KJE2oveZ34du/g1tiimm",
        ];
    }

    /// Web platform constants / Web平台常量
    pub mod web {
        pub const CLIENT_ID: &str = "YUMx5nI8ZU8Ap8pm";
        pub const CLIENT_SECRET: &str = "dbw2OtmVEeuUvIptb1Coyg";
        pub const CLIENT_VERSION: &str = "2.0.0";
        pub const PACKAGE_NAME: &str = "mypikpak.com";
        pub const SDK_VERSION: &str = "8.0.3";
        pub const ALGORITHMS: &[&str] = &[
            "C9qPpZLN8ucRTaTiUMWYS9cQvWOE",
            "+r6CQVxjzJV6LCV",
            "F",
            "pFJRC",
            "9WXYIDGrwTCz2OiVlgZa90qpECPD6olt",
            "/750aCr4lm/Sly/c",
            "RB+DT/gZCrbV",
            "",
            "CyLsf7hdkIRxRm215hl",
            "7xHvLi2tOYP0Y92b",
            "ZGTXXxu8E/MIWaEDB+Sm/",
            "1UI3",
            "E7fP5Pfijd+7K+t6Tg/NhuLq0eEUVChpJSkrKxpO",
            "ihtqpG6FMt65+Xk+tWUH2",
            "NhXXU9rg4XXdzo7u5o",
        ];
    }

    /// PC platform constants / PC平台常量
    pub mod pc {
        pub const CLIENT_ID: &str = "YvtoWO6GNHiuCl7x";
        pub const CLIENT_SECRET: &str = "1NIH5R1IEe2pAxZE3hv3uA";
        pub const CLIENT_VERSION: &str = "undefined";
        pub const PACKAGE_NAME: &str = "mypikpak.com";
        pub const SDK_VERSION: &str = "8.0.3";
        pub const ALGORITHMS: &[&str] = &[
            "KHBJ07an7ROXDoK7Db",
            "G6n399rSWkl7WcQmw5rpQInurc1DkLmLJqE",
            "JZD1A3M4x+jBFN62hkr7VDhkkZxb9g3rWqRZqFAAb",
            "fQnw/AmSlbbI91Ik15gpddGgyU7U",
            "/Dv9JdPYSj3sHiWjouR95NTQff",
            "yGx2zuTjbWENZqecNI+edrQgqmZKP",
            "ljrbSzdHLwbqcRn",
            "lSHAsqCkGDGxQqqwrVu",
            "TsWXI81fD1",
            "vk7hBjawK/rOSrSWajtbMk95nfgf3",
        ];
    }
}

/// API endpoints / API端点
pub mod api {
    pub const USER_HOST: &str = "https://user.mypikpak.net";
    pub const API_HOST: &str = "https://api-drive.mypikpak.net";
    pub const API_HOST_COM: &str = "https://api-drive.mypikpak.com";
    
    pub const LOGIN_URL: &str = "https://user.mypikpak.net/v1/auth/signin";
    pub const TOKEN_URL: &str = "https://user.mypikpak.net/v1/auth/token";
    pub const CAPTCHA_URL: &str = "https://user.mypikpak.net/v1/shield/captcha/init";
    pub const FILES_URL: &str = "https://api-drive.mypikpak.net/drive/v1/files";
    pub const ABOUT_URL: &str = "https://api-drive.mypikpak.com/drive/v1/about";
    pub const TASKS_URL: &str = "https://api-drive.mypikpak.net/drive/v1/tasks";
}

/// Calculate MD5 hash of string / 计算字符串的MD5哈希
pub fn md5_hash(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

/// Calculate SHA1 hash of string / 计算字符串的SHA1哈希
pub fn sha1_hash(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Generate device sign / 生成设备签名
pub fn generate_device_sign(device_id: &str, package_name: &str) -> String {
    let signature_base = format!("{}{}1appkey", device_id, package_name);
    let sha1_result = sha1_hash(&signature_base);
    let md5_result = md5_hash(&sha1_result);
    format!("div101.{}{}", device_id, md5_result)
}

/// Generate captcha sign / 生成验证码签名
pub fn generate_captcha_sign(
    client_id: &str,
    client_version: &str,
    package_name: &str,
    device_id: &str,
    timestamp: i64,
    algorithms: &[&str],
) -> String {
    let mut str = format!(
        "{}{}{}{}{}",
        client_id, client_version, package_name, device_id, timestamp
    );
    for algorithm in algorithms {
        str = md5_hash(&format!("{}{}", str, algorithm));
    }
    format!("1.{}", str)
}

/// Build Android user agent / 构建Android用户代理
pub fn build_android_user_agent(
    device_id: &str,
    client_id: &str,
    app_name: &str,
    sdk_version: &str,
    client_version: &str,
    package_name: &str,
    user_id: &str,
) -> String {
    let device_sign = generate_device_sign(device_id, package_name);
    let timestamp = chrono::Utc::now().timestamp_millis();
    
    format!(
        "ANDROID-{}/{} protocolVersion/200 accesstype/ clientid/{} clientversion/{} action_type/ networktype/WIFI sessionid/ deviceid/{} providername/NONE devicesign/{} refresh_token/ sdkversion/{} datetime/{} usrno/{} appname/android-{} session_origin/ grant_type/ appid/ clientip/ devicename/Xiaomi_M2004j7ac osversion/13 platformversion/10 accessmode/ devicemodel/M2004J7AC",
        app_name, client_version, client_id, client_version,
        device_id, device_sign, sdk_version, timestamp,
        user_id, app_name
    )
}

/// Build Web user agent / 构建Web用户代理
pub fn build_web_user_agent() -> String {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Safari/537.36".to_string()
}

/// Build PC user agent / 构建PC用户代理
pub fn build_pc_user_agent() -> String {
    "MainWindow Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) PikPak/2.6.11.4955 Chrome/100.0.4896.160 Electron/18.3.15 Safari/537.36".to_string()
}

/// Extract action from URL / 从URL提取action
pub fn get_action(method: &str, url: &str) -> String {
    let re = regex::Regex::new(r"://[^/]+((/[^/\s?#]+)*)").unwrap();
    if let Some(caps) = re.captures(url) {
        if let Some(path) = caps.get(1) {
            return format!("{}:{}", method, path.as_str());
        }
    }
    format!("{}:/", method)
}

/// Check if string is email / 检查字符串是否为邮箱
pub fn is_email(s: &str) -> bool {
    let re = regex::Regex::new(r"^\w+([-+.]\w+)*@\w+([-.]\w+)*\.\w+([-.]\w+)*$").unwrap();
    re.is_match(s)
}

/// Check if string is phone number / 检查字符串是否为手机号
pub fn is_phone_number(s: &str) -> bool {
    s.len() >= 11 && s.len() <= 18 && s.chars().all(|c| c.is_ascii_digit() || c == '+')
}

/// Parse datetime string to timestamp / 解析日期时间字符串为时间戳
pub fn parse_datetime(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// Fix path: ensure starts with / and no trailing / / 修正路径
pub fn fix_path(path: &str) -> String {
    let mut p = path.trim().to_string();
    if !p.starts_with('/') {
        p = format!("/{}", p);
    }
    if p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// Get parent path / 获取父路径
pub fn get_parent_path(path: &str) -> String {
    let path = fix_path(path);
    if path == "/" {
        return "/".to_string();
    }
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => path[..i].to_string(),
        None => "/".to_string(),
    }
}

/// Get file name from path / 从路径获取文件名
pub fn get_file_name(path: &str) -> String {
    let path = fix_path(path);
    match path.rfind('/') {
        Some(i) if i < path.len() - 1 => path[i + 1..].to_string(),
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_hash() {
        assert_eq!(md5_hash("hello"), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_sha1_hash() {
        assert_eq!(sha1_hash("hello"), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    #[test]
    fn test_fix_path() {
        assert_eq!(fix_path(""), "/");
        assert_eq!(fix_path("/"), "/");
        assert_eq!(fix_path("/test"), "/test");
        assert_eq!(fix_path("/test/"), "/test");
        assert_eq!(fix_path("test"), "/test");
    }

    #[test]
    fn test_get_parent_path() {
        assert_eq!(get_parent_path("/"), "/");
        assert_eq!(get_parent_path("/test"), "/");
        assert_eq!(get_parent_path("/test/file"), "/test");
    }
}
