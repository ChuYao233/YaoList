//! Cloud189 data type definitions / 天翼云盘数据类型定义

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// API response error / API响应错误
#[derive(Debug, Deserialize, Default)]
pub struct RespErr {
    #[serde(default)]
    pub res_code: Value,
    #[serde(default)]
    pub res_message: String,
    #[serde(default, rename = "error")]
    pub error_: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub msg: String,
    #[serde(default, rename = "errorCode")]
    pub error_code: String,
    #[serde(default, rename = "errorMsg")]
    pub error_msg: String,
}

impl RespErr {
    pub fn has_error(&self) -> bool {
        (match &self.res_code {
            Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
            Value::String(s) => !s.is_empty(),
            _ => false,
        }) || (self.code != "" && self.code != "SUCCESS") || !self.error_code.is_empty() || !self.error_.is_empty()
    }

    pub fn error_message(&self) -> String {
        match &self.res_code {
            Value::Number(n) if n.as_i64().unwrap_or(0) != 0 => {
                return format!("res_code: {}, res_message: {}", n, self.res_message);
            }
            Value::String(s) if !s.is_empty() => {
                return format!("res_code: {}, res_message: {}", s, self.res_message);
            }
            _ => {}
        }
        if !self.code.is_empty() && self.code != "SUCCESS" {
            if !self.msg.is_empty() {
                return format!("code: {}, msg: {}", self.code, self.msg);
            }
            if !self.message.is_empty() {
                return format!("code: {}, message: {}", self.code, self.message);
            }
            return format!("code: {}", self.code);
        }
        if !self.error_code.is_empty() {
            return format!("errorCode: {}, errorMsg: {}", self.error_code, self.error_msg);
        }
        if !self.error_.is_empty() {
            return format!("error: {}, message: {}", self.error_, self.message);
        }
        "Unknown error / 未知错误".to_string()
    }
}

/// Refresh session response - UserSessionResp / 刷新session返回
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserSessionResp {
    #[serde(default, rename = "res_code")]
    pub res_code: i32,
    #[serde(default, rename = "res_message")]
    pub res_message: String,
    #[serde(default, rename = "loginName")]
    pub login_name: String,
    #[serde(default, rename = "keepAlive")]
    pub keep_alive: i32,
    #[serde(default, rename = "sessionKey")]
    pub session_key: String,
    #[serde(default, rename = "sessionSecret")]
    pub session_secret: String,
    #[serde(default, rename = "familySessionKey")]
    pub family_session_key: String,
    #[serde(default, rename = "familySessionSecret")]
    pub family_session_secret: String,
}

/// Login response - AppSessionResp / 登录返回
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppSessionResp {
    #[serde(flatten)]
    pub user_session: UserSessionResp,
    #[serde(default, rename = "isSaveName")]
    pub is_save_name: String,
    #[serde(default, rename = "accessToken")]
    pub access_token: String,
    #[serde(default, rename = "refreshToken")]
    pub refresh_token: String,
}

/// XML format session response / XML格式的Session响应
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename = "userSession")]
pub struct AppSessionRespXml {
    #[serde(default, rename = "sessionKey")]
    pub session_key: String,
    #[serde(default, rename = "sessionSecret")]
    pub session_secret: String,
    #[serde(default, rename = "familySessionKey")]
    pub family_session_key: String,
    #[serde(default, rename = "familySessionSecret")]
    pub family_session_secret: String,
    #[serde(default, rename = "accessToken")]
    pub access_token: String,
    #[serde(default, rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(default, rename = "loginName")]
    pub login_name: String,
}

impl From<AppSessionRespXml> for AppSessionResp {
    fn from(x: AppSessionRespXml) -> Self {
        AppSessionResp {
            user_session: UserSessionResp {
                session_key: x.session_key,
                session_secret: x.session_secret,
                family_session_key: x.family_session_key,
                family_session_secret: x.family_session_secret,
                login_name: x.login_name,
                ..Default::default()
            },
            access_token: x.access_token,
            refresh_token: x.refresh_token,
            ..Default::default()
        }
    }
}

/// Encryption configuration response / 加密配置响应
#[derive(Debug, Deserialize)]
pub struct EncryptConfResp {
    #[serde(default)]
    pub result: i32,
    pub data: Option<EncryptConfData>,
}

#[derive(Debug, Deserialize)]
pub struct EncryptConfData {
    #[serde(default, rename = "upSmsOn")]
    pub up_sms_on: String,
    #[serde(default)]
    pub pre: String,
    #[serde(default, rename = "preDomain")]
    pub pre_domain: String,
    #[serde(default, rename = "pubKey")]
    pub pub_key: String,
}

/// Login response / 登录响应
#[derive(Debug, Deserialize)]
pub struct LoginResp {
    #[serde(default)]
    pub msg: String,
    #[serde(default)]
    pub result: i32,
    #[serde(default, rename = "toUrl")]
    pub to_url: String,
}

/// File information - Cloud189File / 文件信息
#[derive(Debug, Deserialize, Clone)]
pub struct Cloud189File {
    pub id: Value,
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub md5: String,
    #[serde(default, rename = "lastOpTime")]
    pub last_op_time: String,
    #[serde(default, rename = "createDate")]
    pub create_date: String,
}

impl Cloud189File {
    pub fn get_id(&self) -> String {
        match &self.id {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => self.id.to_string().trim_matches('"').to_string(),
        }
    }
}

/// Folder information - Cloud189Folder / 文件夹信息
#[derive(Debug, Deserialize, Clone)]
pub struct Cloud189Folder {
    pub id: Value,
    #[serde(default, rename = "parentId")]
    pub parent_id: i64,
    pub name: String,
    #[serde(default, rename = "lastOpTime")]
    pub last_op_time: String,
    #[serde(default, rename = "createDate")]
    pub create_date: String,
}

impl Cloud189Folder {
    pub fn get_id(&self) -> String {
        match &self.id {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => self.id.to_string().trim_matches('"').to_string(),
        }
    }
}

/// File list response / 文件列表响应
#[derive(Debug, Deserialize)]
pub struct Cloud189FilesResp {
    #[serde(default, rename = "fileListAO")]
    pub file_list_ao: FileListAO,
}

#[derive(Debug, Deserialize, Default)]
pub struct FileListAO {
    #[serde(default)]
    pub count: i32,
    #[serde(default, rename = "fileList")]
    pub file_list: Vec<Cloud189File>,
    #[serde(default, rename = "folderList")]
    pub folder_list: Vec<Cloud189Folder>,
}

/// Download link response / 下载链接响应
#[derive(Debug, Deserialize)]
pub struct DownloadUrlResp {
    #[serde(default, rename = "fileDownloadUrl")]
    pub file_download_url: String,
}

/// Family cloud list response / 家庭云列表响应
#[derive(Debug, Deserialize)]
pub struct FamilyInfoListResp {
    #[serde(default, rename = "familyInfoResp")]
    pub family_info_resp: Vec<FamilyInfoResp>,
}

#[derive(Debug, Deserialize)]
pub struct FamilyInfoResp {
    #[serde(default)]
    pub count: i32,
    #[serde(default, rename = "createTime")]
    pub create_time: String,
    #[serde(default, rename = "familyId")]
    pub family_id: i64,
    #[serde(default, rename = "remarkName")]
    pub remark_name: String,
    #[serde(default, rename = "type")]
    pub type_: i32,
    #[serde(default, rename = "useFlag")]
    pub use_flag: i32,
    #[serde(default, rename = "userRole")]
    pub user_role: i32,
}

/// Batch task information / 批量任务信息
#[derive(Debug, Serialize, Clone)]
pub struct BatchTaskInfo {
    #[serde(rename = "fileId")]
    pub file_id: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "isFolder")]
    pub is_folder: i32,
    #[serde(skip_serializing_if = "Option::is_none", rename = "srcParentId")]
    pub src_parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "dealWay")]
    pub deal_way: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "isConflict")]
    pub is_conflict: Option<i32>,
}

/// Create batch task response / 创建批量任务响应
#[derive(Debug, Deserialize)]
pub struct CreateBatchTaskResp {
    #[serde(default, rename = "taskId")]
    pub task_id: String,
}

/// Batch task status response / 批量任务状态响应
#[derive(Debug, Deserialize)]
pub struct BatchTaskStateResp {
    #[serde(default, rename = "failedCount")]
    pub failed_count: i32,
    #[serde(default)]
    pub process: i32,
    #[serde(default, rename = "skipCount")]
    pub skip_count: i32,
    #[serde(default, rename = "subTaskCount")]
    pub sub_task_count: i32,
    #[serde(default, rename = "successedCount")]
    pub successed_count: i32,
    #[serde(default, rename = "taskId")]
    pub task_id: String,
    #[serde(default, rename = "taskStatus")]
    pub task_status: i32, // 1 init 2 conflict 3 running 4 completed / 1 初始化 2 存在冲突 3 执行中 4 完成
}

/// Initialize multipart upload response / 初始化多段上传响应
#[derive(Debug, Deserialize, Clone)]
pub struct InitMultiUploadResp {
    #[serde(default)]
    pub data: InitMultiUploadData,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct InitMultiUploadData {
    #[serde(default, rename = "uploadType")]
    pub upload_type: i32,
    #[serde(default, rename = "uploadHost")]
    pub upload_host: String,
    #[serde(default, rename = "uploadFileId")]
    pub upload_file_id: String,
    #[serde(default, rename = "fileDataExists")]
    pub file_data_exists: i32,
}

/// Get upload URL response / 获取上传URL响应
#[derive(Debug, Deserialize)]
pub struct UploadUrlsResp {
    #[serde(default)]
    pub code: String,
    #[serde(default, rename = "uploadUrls")]
    pub upload_urls: std::collections::HashMap<String, UploadUrlsData>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UploadUrlsData {
    #[serde(default, rename = "requestURL")]
    pub request_url: String,
    #[serde(default, rename = "requestHeader")]
    pub request_header: String,
}

/// Upload URL information (parsed) / 上传URL信息
#[derive(Debug, Clone)]
pub struct UploadUrlInfo {
    pub part_number: i32,
    pub headers: std::collections::HashMap<String, String>,
    pub request_url: String,
}

/// Legacy upload create response / 旧版上传创建响应
#[derive(Debug, Deserialize, Clone)]
pub struct CreateUploadFileResp {
    #[serde(default, rename = "uploadFileId")]
    pub upload_file_id: i64,
    #[serde(default, rename = "fileUploadUrl")]
    pub file_upload_url: String,
    #[serde(default, rename = "fileCommitUrl")]
    pub file_commit_url: String,
    #[serde(default, rename = "fileDataExists")]
    pub file_data_exists: i32,
}

/// 获取上传文件状态响应
#[derive(Debug, Deserialize, Clone)]
pub struct GetUploadFileStatusResp {
    #[serde(flatten)]
    pub create_resp: CreateUploadFileResp,
    #[serde(default, rename = "dataSize")]
    pub data_size: i64,
    #[serde(default)]
    pub size: i64,
}

impl GetUploadFileStatusResp {
    pub fn get_size(&self) -> i64 {
        self.data_size + self.size
    }
}

/// 提交多段上传响应
#[derive(Debug, Deserialize)]
pub struct CommitMultiUploadFileResp {
    #[serde(default)]
    pub file: CommitFileInfo,
}

#[derive(Debug, Deserialize, Default)]
pub struct CommitFileInfo {
    #[serde(default, rename = "userFileId")]
    pub user_file_id: String,
    #[serde(default, rename = "fileName")]
    pub file_name: String,
    #[serde(default, rename = "fileSize")]
    pub file_size: i64,
    #[serde(default, rename = "fileMd5")]
    pub file_md5: String,
    #[serde(default, rename = "createDate")]
    pub create_date: String,
}

/// 旧版提交上传响应（XML格式）
#[derive(Debug, Deserialize)]
#[serde(rename = "file")]
pub struct OldCommitUploadFileResp {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub md5: String,
    #[serde(default, rename = "createDate")]
    pub create_date: String,
}

/// 容量信息响应
#[derive(Debug, Deserialize)]
pub struct CapacityResp {
    #[serde(default, rename = "res_code")]
    pub res_code: i32,
    #[serde(default, rename = "res_message")]
    pub res_message: String,
    #[serde(default)]
    pub account: String,
    #[serde(default, rename = "cloudCapacityInfo")]
    pub cloud_capacity_info: Option<CloudCapacityInfo>,
    #[serde(default, rename = "familyCapacityInfo")]
    pub family_capacity_info: Option<FamilyCapacityInfo>,
    #[serde(default, rename = "totalSize")]
    pub total_size: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct CloudCapacityInfo {
    #[serde(default, rename = "freeSize")]
    pub free_size: i64,
    #[serde(default, rename = "mail189UsedSize")]
    pub mail_used_size: u64,
    #[serde(default, rename = "totalSize")]
    pub total_size: u64,
    #[serde(default, rename = "usedSize")]
    pub used_size: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct FamilyCapacityInfo {
    #[serde(default, rename = "freeSize")]
    pub free_size: i64,
    #[serde(default, rename = "totalSize")]
    pub total_size: u64,
    #[serde(default, rename = "usedSize")]
    pub used_size: u64,
}
