//! 迅雷云盘 API 数据类型定义

use serde::{Deserialize, Serialize};

// ============ API 常量 ============

pub const API_URL: &str = "https://api-pan.xunlei.com/drive/v1";
pub const FILE_API_URL: &str = "https://api-pan.xunlei.com/drive/v1/files";
pub const XLUSER_API_URL: &str = "https://xluser-ssl.xunlei.com/v1";
pub const XLUSER_API_BASE_URL: &str = "https://xluser-ssl.xunlei.com";

pub const FOLDER_KIND: &str = "drive#folder";
pub const FILE_KIND: &str = "drive#file";
pub const UPLOAD_TYPE_RESUMABLE: &str = "UPLOAD_TYPE_RESUMABLE";

pub const SIGN_PROVIDER: &str = "access_end_point_token";
pub const APPID: &str = "40";
pub const APP_KEY: &str = "34a062aaa22f906fca4fefe9fb3a3021";

pub const DEFAULT_CLIENT_ID: &str = "Xp6vsxz_7IYVw2BB";
pub const DEFAULT_CLIENT_SECRET: &str = "Xp6vsy4tN9toTVdMSpomVdXpRmES";
pub const DEFAULT_CLIENT_VERSION: &str = "8.31.0.9726";
pub const DEFAULT_PACKAGE_NAME: &str = "com.xunlei.downloadprovider";
pub const DEFAULT_USER_AGENT: &str = "ANDROID-com.xunlei.downloadprovider/8.31.0.9726 netWorkType/5G appid/40 deviceName/Xiaomi_M2004j7ac deviceModel/M2004J7AC OSVersion/12 protocolVersion/301 platformVersion/10 sdkVersion/512000 Oauth2Client/0.9 (Linux 4_14_186-perf-gddfs8vbb238b) (JAVA 0)";
pub const DEFAULT_DOWNLOAD_USER_AGENT: &str = "Dalvik/2.1.0 (Linux; U; Android 12; M2004J7AC Build/SP1A.210812.016)";

pub const DEFAULT_ALGORITHMS: &[&str] = &[
    "9uJNVj/wLmdwKrJaVj/omlQ",
    "Oz64Lp0GigmChHMf/6TNfxx7O9PyopcczMsnf",
    "Eb+L7Ce+Ej48u",
    "jKY0",
    "ASr0zCl6v8W4aidjPK5KHd1Lq3t+vBFf41dqv5+fnOd",
    "wQlozdg6r1qxh0eRmt3QgNXOvSZO6q/GXK",
    "gmirk+ciAvIgA/cxUUCema47jr/YToixTT+Q6O",
    "5IiCoM9B1/788ntB",
    "P07JH0h6qoM6TSUAK2aL9T5s2QBVeY9JWvalf",
    "+oK0AN",
];

// ============ 错误响应 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ErrResp {
    #[serde(default)]
    pub error_code: i64,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub error_description: String,
}

impl ErrResp {
    pub fn is_error(&self) -> bool {
        if self.error == "success" {
            return false;
        }
        self.error_code != 0 || !self.error.is_empty() || !self.error_description.is_empty()
    }

    pub fn message(&self) -> String {
        format!("code={}, error={}, desc={}", self.error_code, self.error, self.error_description)
    }
}

// ============ Token 响应 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenResp {
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub user_id: String,
}

impl TokenResp {
    pub fn token(&self) -> String {
        let tt = if self.token_type.is_empty() { "Bearer" } else { &self.token_type };
        format!("{} {}", tt, self.access_token)
    }
}

// ============ Core 登录 ============

#[derive(Debug, Serialize)]
pub struct CoreLoginRequest {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    #[serde(rename = "sequenceNo")]
    pub sequence_no: String,
    #[serde(rename = "platformVersion")]
    pub platform_version: String,
    #[serde(rename = "isCompressed")]
    pub is_compressed: String,
    pub appid: String,
    #[serde(rename = "clientVersion")]
    pub client_version: String,
    #[serde(rename = "peerID")]
    pub peer_id: String,
    #[serde(rename = "appName")]
    pub app_name: String,
    #[serde(rename = "sdkVersion")]
    pub sdk_version: String,
    pub devicesign: String,
    #[serde(rename = "netWorkType")]
    pub network_type: String,
    #[serde(rename = "providerName")]
    pub provider_name: String,
    #[serde(rename = "deviceModel")]
    pub device_model: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "OSVersion")]
    pub os_version: String,
    pub creditkey: String,
    pub hl: String,
    #[serde(rename = "userName")]
    pub user_name: String,
    #[serde(rename = "passWord")]
    pub pass_word: String,
    #[serde(rename = "verifyKey")]
    pub verify_key: String,
    #[serde(rename = "verifyCode")]
    pub verify_code: String,
    #[serde(rename = "isMd5Pwd")]
    pub is_md5_pwd: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CoreLoginResp {
    #[serde(default, rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub creditkey: String,
    #[serde(default)]
    pub error: String,
    #[serde(default, rename = "errorCode")]
    pub error_code: String,
    #[serde(default, rename = "errorDesc")]
    pub error_desc: String,
    #[serde(default, rename = "userID")]
    pub user_id: String,
    #[serde(default)]
    pub reviewurl: String,
    #[serde(default, rename = "verifyType")]
    pub verify_type: String,
}

// ============ SignIn 请求 ============

#[derive(Debug, Serialize)]
pub struct SignInRequest {
    pub client_id: String,
    pub client_secret: String,
    pub provider: String,
    pub signin_token: String,
}

// ============ 验证码 Token ============

#[derive(Debug, Serialize)]
pub struct CaptchaTokenRequest {
    pub action: String,
    pub captcha_token: String,
    pub client_id: String,
    pub device_id: String,
    pub meta: std::collections::HashMap<String, String>,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CaptchaTokenResponse {
    #[serde(default)]
    pub captcha_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub url: String,
}

// ============ 文件列表 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileList {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub next_page_token: String,
    #[serde(default)]
    pub files: Vec<ThunderFile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ThunderFile {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub web_content_link: String,
    #[serde(default)]
    pub created_time: String,
    #[serde(default)]
    pub modified_time: String,
    #[serde(default)]
    pub thumbnail_link: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub medias: Vec<MediaInfo>,
}

impl ThunderFile {
    pub fn is_dir(&self) -> bool {
        self.kind == FOLDER_KIND
    }

    pub fn get_size(&self) -> u64 {
        self.size.parse().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MediaInfo {
    #[serde(default)]
    pub link: MediaLink,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MediaLink {
    #[serde(default)]
    pub url: String,
}

// ============ 上传响应 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadTaskResponse {
    #[serde(default)]
    pub upload_type: String,
    #[serde(default)]
    pub resumable: Option<ResumableInfo>,
    #[serde(default)]
    pub file: Option<ThunderFile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResumableInfo {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub params: Option<ResumableParams>,
    #[serde(default)]
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResumableParams {
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub access_key_secret: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub expiration: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub security_token: String,
}

// ============ 验证数据（需要短信验证时返回给用户） ============

#[derive(Debug, Serialize)]
pub struct ReviewData {
    pub creditkey: String,
    pub reviewurl: String,
    pub deviceid: String,
    pub devicesign: String,
}

// ============ 短信验证响应 ============

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SmsResponse {
    #[serde(default)]
    pub creditkey: String,
    #[serde(default)]
    pub deviceid: String,
    #[serde(default)]
    pub error: String,
    #[serde(default, rename = "errorCode")]
    pub error_code: String,
    #[serde(default, rename = "errorDesc")]
    pub error_desc: String,
    #[serde(default)]
    pub token: String,
    #[serde(default, rename = "verifyType")]
    pub verify_type: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CheckSmsResponse {
    #[serde(default)]
    pub creditkey: String,
    #[serde(default)]
    pub error: String,
    #[serde(default, rename = "errorCode")]
    pub error_code: String,
    #[serde(default, rename = "errorDesc")]
    pub error_desc: String,
}
