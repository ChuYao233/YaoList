//! OneDrive App driver type definitions / OneDrive App 驱动类型定义

use serde::{Deserialize, Deserializer, Serialize};

/// 自定义反序列化器：支持字符串或数字转换为 u64
fn deserialize_u64_from_string_or_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Visitor;
    use std::fmt;

    struct U64Visitor;

    impl<'de> Visitor<'de> for U64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number representing a u64")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if value < 0 {
                Err(E::custom(format!("negative number {} cannot be converted to u64", value)))
            } else {
                Ok(value as u64)
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value.parse::<u64>().map_err(|_| {
                E::custom(format!("failed to parse string '{}' as u64", value))
            })
        }
    }

    deserializer.deserialize_any(U64Visitor)
}

/// 自定义反序列化器：支持字符串或数字转换为 Option<u64>
fn deserialize_option_u64_from_string_or_number<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Visitor;
    use std::fmt;

    struct OptionU64Visitor;

    impl<'de> Visitor<'de> for OptionU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number representing a u64, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if value < 0 {
                Err(E::custom(format!("negative number {} cannot be converted to u64", value)))
            } else {
                Ok(Some(value as u64))
            }
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value.parse::<u64>()
                .map(Some)
                .map_err(|_| E::custom(format!("failed to parse string '{}' as u64", value)))
        }
    }

    deserializer.deserialize_any(OptionU64Visitor)
}

/// OneDrive region configuration / OneDrive区域配置
#[derive(Debug, Clone)]
pub struct HostConfig {
    pub oauth: &'static str,
    pub api: &'static str,
}

/// Region to host mapping / 区域到主机的映射
pub fn get_host_config(region: &str) -> HostConfig {
    match region {
        "cn" => HostConfig {
            oauth: "https://login.chinacloudapi.cn",
            api: "https://microsoftgraph.chinacloudapi.cn",
        },
        "us" => HostConfig {
            oauth: "https://login.microsoftonline.us",
            api: "https://graph.microsoft.us",
        },
        "de" => HostConfig {
            oauth: "https://login.microsoftonline.de",
            api: "https://graph.microsoft.de",
        },
        _ => HostConfig { // global
            oauth: "https://login.microsoftonline.com",
            api: "https://graph.microsoft.com",
        },
    }
}

/// OneDrive App configuration / OneDrive App 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneDriveAppConfig {
    /// Region: global, cn, us, de / 区域
    #[serde(default = "default_region")]
    pub region: String,
    /// Client ID / 客户端ID
    pub client_id: String,
    /// Client secret / 客户端密钥
    pub client_secret: String,
    /// Tenant ID / 租户ID
    pub tenant_id: String,
    /// Email / 邮箱
    pub email: String,
    /// Root folder path / 根文件夹路径
    #[serde(default = "default_root")]
    pub root_folder_path: String,
    /// Chunk upload size (MB) / 分块上传大小
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u64,
    /// Custom download domain / 自定义下载域名
    #[serde(default)]
    pub custom_host: Option<String>,
    /// Disable disk usage query / 禁用磁盘使用量查询
    #[serde(default)]
    pub disable_disk_usage: bool,
    /// Enable direct upload / 启用前端直传
    #[serde(default)]
    pub enable_direct_upload: bool,
    /// Proxy URL for requests (e.g., http://127.0.0.1:7890) / 代理URL（如：http://127.0.0.1:7890）
    #[serde(default)]
    pub proxy: Option<String>,
}

fn default_region() -> String {
    "global".to_string()
}

fn default_root() -> String {
    "/".to_string()
}

fn default_chunk_size() -> u64 {
    5
}

/// Token response / Token响应
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(rename = "token_type")]
    pub _token_type: Option<String>,
    #[serde(rename = "expires_in", deserialize_with = "deserialize_option_u64_from_string_or_number")]
    pub _expires_in: Option<u64>,
}

/// Token error / Token错误
#[derive(Debug, Deserialize)]
pub struct TokenError {
    pub error: String,
    #[serde(rename = "error_description")]
    pub error_description: String,
}

/// API error / API错误
#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

/// OneDrive文件信息
#[derive(Debug, Deserialize)]
pub struct OneDriveFile {
    pub id: String,
    pub name: String,
    pub size: Option<i64>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
    pub file: Option<FileDetail>,
    #[serde(rename = "parentReference")]
    pub parent_reference: Option<ParentReference>,
    pub thumbnails: Option<Vec<Thumbnail>>,
}

#[derive(Debug, Deserialize)]
pub struct FileDetail {
    #[serde(rename = "mimeType")]
    pub _mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ParentReference {
    #[serde(rename = "driveId")]
    pub _drive_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Thumbnail {
    pub medium: Option<ThumbnailMedium>,
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailMedium {
    pub url: Option<String>,
}

/// 文件列表响应
#[derive(Debug, Deserialize)]
pub struct FilesResponse {
    pub value: Vec<OneDriveFile>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
}

/// 上传会话响应
#[derive(Debug, Deserialize)]
pub struct UploadSessionResponse {
    #[serde(rename = "uploadUrl")]
    pub upload_url: String,
}

/// Drive配额信息
#[derive(Debug, Deserialize)]
pub struct DriveQuota {
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub total: u64,
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub used: u64,
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub remaining: u64,
    #[serde(deserialize_with = "deserialize_option_u64_from_string_or_number")]
    pub deleted: Option<u64>,
    pub state: Option<String>,
}

/// Drive响应
#[derive(Debug, Deserialize)]
pub struct DriveResponse {
    pub id: Option<String>,
    #[serde(rename = "driveType")]
    pub _drive_type: Option<String>,
    pub quota: DriveQuota,
}

