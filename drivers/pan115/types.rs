//! 115云盘类型定义

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BasicResp {
    #[serde(default)]
    pub state: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub errno: i32,
    #[serde(default)]
    pub errcode: i32,
    #[serde(default)]
    pub errtype: String,
}

impl BasicResp {
    pub fn check(&self) -> Result<(), String> {
        if self.state {
            Ok(())
        } else if !self.error.is_empty() {
            Err(self.error.clone())
        } else if self.errno != 0 {
            Err(format!("errno: {}, errcode: {}", self.errno, self.errcode))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileInfo {
    #[serde(default, alias = "fid")]
    pub file_id: String,
    #[serde(default, alias = "cid")]
    pub category_id: String,
    #[serde(default, alias = "pid")]
    pub parent_id: String,
    #[serde(default, alias = "n")]
    pub name: String,
    #[serde(default, alias = "s")]
    pub size: i64,
    #[serde(default, alias = "pc")]
    pub pick_code: String,
    #[serde(default)]
    pub sha: String,
    #[serde(default, alias = "t")]
    pub modified_time: String,
    #[serde(default, alias = "te")]
    pub created_time: String,
    #[serde(default)]
    pub ico: String,
    #[serde(default, alias = "u")]
    pub thumb: String,
    #[serde(default)]
    pub play_long: i64,
}

impl FileInfo {
    pub fn is_dir(&self) -> bool {
        self.file_id.is_empty() && !self.category_id.is_empty()
    }
    
    pub fn get_id(&self) -> &str {
        if self.is_dir() {
            &self.category_id
        } else {
            &self.file_id
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileListResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub data: Vec<FileInfo>,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub page_size: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadData {
    #[serde(default)]
    pub url: DownloadUrl,
    #[serde(default)]
    pub pick_code: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadUrl {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub client: i32,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub oss_id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MkdirResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default, alias = "cid")]
    pub category_id: String,
    #[serde(default, alias = "cname")]
    pub category_name: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadInitResp {
    #[serde(default)]
    pub request: String,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub statuscode: i32,
    #[serde(default)]
    pub statusmsg: String,
    #[serde(default)]
    pub pickcode: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub callback: UploadCallback,
    #[serde(default)]
    pub sign_key: String,
    #[serde(default)]
    pub sign_check: String,
    #[serde(default, rename = "SHA1")]
    pub sha1: String,
}

impl UploadInitResp {
    pub fn is_matched(&self) -> bool {
        self.status == 2
    }
    
    pub fn need_sign_check(&self) -> bool {
        self.status == 7
    }
    
    pub fn ok(&self) -> Result<bool, String> {
        match self.status {
            1 => Err(self.statusmsg.clone()),
            2 => Ok(true),
            _ => Ok(false),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadCallback {
    #[serde(default)]
    pub callback: String,
    #[serde(default)]
    pub callback_var: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadResult {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub data: UploadResultData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadResultData {
    #[serde(default)]
    pub pick_code: String,
    #[serde(default)]
    pub file_size: i64,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub thumb_url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub cid: String,
    #[serde(default)]
    pub aid: i64,
    #[serde(default)]
    pub is_video: i32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OssTokenResp {
    #[serde(default)]
    pub state: bool,
    #[serde(default)]
    pub errno: i32,
    #[serde(default, alias = "AccessKeyId")]
    pub access_key_id: String,
    #[serde(default, alias = "AccessKeySecret")]
    pub access_key_secret: String,
    #[serde(default, alias = "SecurityToken")]
    pub security_token: String,
    #[serde(default, alias = "Expiration")]
    pub expiration: String,
    #[serde(default, alias = "StatusCode")]
    pub status_code: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadInfoResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub userkey: String,
    #[serde(default)]
    pub size_limit: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserInfoResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub data: UserInfoData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserInfoData {
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub user_name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SpaceInfoResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default)]
    pub data: SpaceInfoData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SpaceInfoData {
    #[serde(default)]
    pub all_total: SpaceSize,
    #[serde(default)]
    pub all_remain: SpaceSize,
    #[serde(default)]
    pub all_use: SpaceSize,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SpaceSize {
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub size_format: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VersionResp {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub data: VersionData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VersionData {
    #[serde(default)]
    pub win: VersionInfo,
    #[serde(default)]
    pub linux: VersionInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VersionInfo {
    #[serde(default, alias = "version_code")]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileInfoResp {
    #[serde(flatten)]
    pub base: BasicResp,
    #[serde(default, alias = "data")]
    pub files: Vec<FileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Pan115Config {
    #[serde(default)]
    pub cookie: String,
    #[serde(default)]
    pub root_folder_id: String,
    #[serde(default)]
    pub mount_path: String,
    #[serde(default)]
    pub page_size: i64,
    #[serde(default)]
    pub limit_rate: f64,
    #[serde(default)]
    pub proxy_download: bool,
}
