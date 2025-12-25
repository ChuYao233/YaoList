//! 139云盘数据类型定义 / 139Yun data types

use serde::{Deserialize, Serialize};

/// 反序列化null为空字符串
fn deserialize_null_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// 反序列化null为0
fn deserialize_null_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<i64>::deserialize(deserializer)?;
    Ok(opt.unwrap_or(0))
}

/// 反序列化null为false
fn deserialize_null_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<bool>::deserialize(deserializer)?;
    Ok(opt.unwrap_or(false))
}

/// 反序列化null为0 (i32)
fn deserialize_null_i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<i32>::deserialize(deserializer)?;
    Ok(opt.unwrap_or(0))
}

/// 云盘模式 / Cloud mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudType {
    #[default]
    PersonalNew,
    Personal,
    Family,
    Group,
}

impl CloudType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "personal_new" => CloudType::PersonalNew,
            "personal" => CloudType::Personal,
            "family" => CloudType::Family,
            "group" => CloudType::Group,
            _ => CloudType::PersonalNew,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            CloudType::PersonalNew => "personal_new",
            CloudType::Personal => "personal",
            CloudType::Family => "family",
            CloudType::Group => "group",
        }
    }

    pub fn svc_type(&self) -> &'static str {
        match self {
            CloudType::Family => "2",
            _ => "1",
        }
    }
}

/// 基础响应 / Base response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BaseResp {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// 目录信息 / Catalog info
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default)]
    pub catalog_id: String,
    #[serde(default)]
    pub catalog_name: String,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub update_time: String,
}

/// 文件内容信息 / Content info
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default, rename = "contentID")]
    pub content_id: String,
    #[serde(default)]
    pub content_name: String,
    #[serde(default)]
    pub content_size: i64,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub update_time: String,
    #[serde(default, rename = "thumbnailURL")]
    pub thumbnail_url: String,
    #[serde(default)]
    pub digest: String,
}

/// 获取磁盘结果 / Get disk result
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetDiskResult {
    #[serde(default, rename = "parentCatalogID")]
    pub parent_catalog_id: String,
    #[serde(default)]
    pub node_count: i32,
    #[serde(default)]
    pub catalog_list: Vec<Catalog>,
    #[serde(default)]
    pub content_list: Vec<Content>,
    #[serde(default)]
    pub is_completed: i32,
}

/// 获取磁盘响应数据 / Get disk response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetDiskData {
    #[serde(default)]
    pub result: ResultInfo,
    #[serde(default)]
    pub get_disk_result: GetDiskResult,
}

/// 获取磁盘响应 / Get disk response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GetDiskResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: GetDiskData,
}

/// 结果信息 / Result info
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResultInfo {
    #[serde(default)]
    pub result_code: String,
    #[serde(default)]
    pub result_desc: Option<String>,
}

/// 上传结果 / Upload result
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    #[serde(default, rename = "uploadTaskID")]
    pub upload_task_id: String,
    #[serde(default)]
    pub redirection_url: String,
    #[serde(default)]
    pub new_content_id_list: Vec<NewContentId>,
}

/// 新内容ID / New content ID
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NewContentId {
    #[serde(default, rename = "contentID")]
    pub content_id: String,
    #[serde(default)]
    pub content_name: String,
    #[serde(default)]
    pub is_need_upload: String,
}

/// 上传响应数据 / Upload response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UploadData {
    #[serde(default)]
    pub result: ResultInfo,
    #[serde(default)]
    pub upload_result: UploadResult,
}

/// 上传响应 / Upload response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UploadResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: UploadData,
}

/// 云内容 / Cloud content
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudContent {
    #[serde(default, rename = "contentID")]
    pub content_id: String,
    #[serde(default)]
    pub content_name: String,
    #[serde(default)]
    pub content_size: i64,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub last_update_time: String,
    #[serde(default, rename = "thumbnailURL")]
    pub thumbnail_url: String,
}

/// 云目录 / Cloud catalog
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudCatalog {
    #[serde(default, rename = "catalogID")]
    pub catalog_id: String,
    #[serde(default)]
    pub catalog_name: String,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub last_update_time: String,
}

/// 查询内容列表响应数据 / Query content list response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueryContentListData {
    #[serde(default)]
    pub result: ResultInfo,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub cloud_content_list: Vec<CloudContent>,
    #[serde(default)]
    pub cloud_catalog_list: Vec<CloudCatalog>,
    #[serde(default)]
    pub total_count: i32,
}

/// 查询内容列表响应 / Query content list response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QueryContentListResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: QueryContentListData,
}

/// 群组目录(带路径) / Group catalog with path
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupCatalog {
    #[serde(flatten)]
    pub catalog: Catalog,
    #[serde(default)]
    pub path: String,
}

/// 群组内容结果 / Group content result
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetGroupContentResult {
    #[serde(default, rename = "parentCatalogID")]
    pub parent_catalog_id: String,
    #[serde(default)]
    pub catalog_list: Vec<GroupCatalog>,
    #[serde(default)]
    pub content_list: Vec<Content>,
    #[serde(default)]
    pub node_count: i32,
    #[serde(default)]
    pub ctlg_cnt: i32,
    #[serde(default)]
    pub cont_cnt: i32,
}

/// 查询群组内容响应数据 / Query group content response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueryGroupContentData {
    #[serde(default)]
    pub result: ResultInfo,
    #[serde(default)]
    pub get_group_content_result: GetGroupContentResult,
}

/// 查询群组内容响应 / Query group content response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QueryGroupContentListResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: QueryGroupContentData,
}

/// 个人版文件缩略图 / Personal file thumbnail
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersonalThumbnail {
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub url: String,
}

/// 个人版文件项 / Personal file item
/// 直接从serde_json::Value解析，避免null值问题
#[derive(Debug, Clone, Default)]
pub struct PersonalFileItem {
    pub file_id: String,
    pub name: String,
    pub size: i64,
    pub file_type: String,
    pub created_at: String,
    pub updated_at: String,
    pub thumbnail_urls: Vec<PersonalThumbnail>,
}

impl PersonalFileItem {
    /// 从JSON Value解析
    pub fn from_value(v: &serde_json::Value) -> Self {
        Self {
            file_id: v["fileId"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            size: v["size"].as_i64().unwrap_or(0),
            file_type: v["type"].as_str().unwrap_or_default().to_string(),
            created_at: v["createdAt"].as_str().unwrap_or_default().to_string(),
            updated_at: v["updatedAt"].as_str().unwrap_or_default().to_string(),
            thumbnail_urls: v["thumbnailUrls"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| {
                            Some(PersonalThumbnail {
                                style: t["style"].as_str()?.to_string(),
                                url: t["url"].as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// 个人版列表响应数据 / Personal list response data
/// 注意: 不再使用，改用Value手动解析
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalListData {
    #[serde(default)]
    pub next_page_cursor: String,
}

/// 个人版列表响应 / Personal list response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersonalListResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: PersonalListData,
}

/// 个人版分片信息 / Personal part info
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalPartInfo {
    #[serde(default, deserialize_with = "deserialize_null_i32")]
    pub part_number: i32,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub upload_url: String,
}

/// 并行哈希上下文 / Parallel hash context
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParallelHashCtx {
    #[serde(default)]
    pub part_offset: i64,
}

/// 分片信息(请求用) / Part info for request
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PartInfo {
    #[serde(default)]
    pub part_number: i64,
    #[serde(default)]
    pub part_size: i64,
    #[serde(default)]
    pub parallel_hash_ctx: ParallelHashCtx,
}

/// 反序列化null为空Vec
fn deserialize_null_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let opt = Option::<Vec<T>>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// 个人版上传响应数据 / Personal upload response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalUploadData {
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub file_id: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub file_name: String,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub part_infos: Vec<PersonalPartInfo>,
    #[serde(default, deserialize_with = "deserialize_null_bool")]
    pub exist: bool,
    #[serde(default, deserialize_with = "deserialize_null_bool")]
    pub rapid_upload: bool,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub upload_id: String,
}

/// 个人版上传响应 / Personal upload response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersonalUploadResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: PersonalUploadData,
}

/// 个人版上传URL响应数据 / Personal upload URL response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalUploadUrlData {
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub upload_id: String,
    #[serde(default)]
    pub part_infos: Vec<PersonalPartInfo>,
}

/// 个人版上传URL响应 / Personal upload URL response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersonalUploadUrlResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: PersonalUploadUrlData,
}

/// 路由策略项 / Route policy item
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoutePolicyItem {
    #[serde(default, rename = "siteID", deserialize_with = "deserialize_null_string")]
    pub site_id: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub site_code: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub mod_name: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub http_url: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub https_url: String,
    #[serde(default, rename = "envID", deserialize_with = "deserialize_null_string")]
    pub env_id: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub ext_info: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub hash_name: String,
    #[serde(default)]
    pub mod_addr_type: i32,
}

/// 查询路由策略响应数据 / Query route policy response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueryRoutePolicyData {
    #[serde(default)]
    pub route_policy_list: Vec<RoutePolicyItem>,
}

/// 查询路由策略响应 / Query route policy response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QueryRoutePolicyResp {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: QueryRoutePolicyData,
}

/// 刷新令牌响应(XML) / Refresh token response (XML)
#[derive(Debug, Clone, Default)]
pub struct RefreshTokenResp {
    pub return_code: String,
    pub token: String,
    pub expire_time: i32,
    pub access_token: String,
    pub desc: String,
}

/// 个人版磁盘信息响应数据 / Personal disk info response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDiskInfoData {
    #[serde(default)]
    pub free_disk_size: String,
    #[serde(default)]
    pub disk_size: String,
}

/// 个人版磁盘信息响应 / Personal disk info response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PersonalDiskInfoResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: PersonalDiskInfoData,
}

/// 家庭版磁盘信息响应数据 / Family disk info response data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FamilyDiskInfoData {
    #[serde(default)]
    pub used_size: String,
    #[serde(default)]
    pub disk_size: String,
}

/// 家庭版磁盘信息响应 / Family disk info response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FamilyDiskInfoResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: FamilyDiskInfoData,
}

/// 下载URL响应 / Download URL response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadUrlResp {
    #[serde(flatten)]
    pub base: BaseResp,
    #[serde(default)]
    pub data: DownloadUrlData,
}

/// 下载URL数据 / Download URL data
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUrlData {
    #[serde(default, rename = "downloadURL")]
    pub download_url: String,
    #[serde(default)]
    pub cdn_url: String,
    #[serde(default)]
    pub url: String,
}

/// 批量操作任务响应 / Batch operation task response
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CreateBatchOprTaskResp {
    #[serde(default)]
    pub result: ResultInfo,
    #[serde(default, rename = "taskID")]
    pub task_id: String,
}

/// 上传XML结果 / Upload XML result
#[derive(Debug, Clone, Default)]
pub struct InterLayerUploadResult {
    pub result_code: i32,
    pub msg: String,
}

impl InterLayerUploadResult {
    pub fn from_xml(xml: &str) -> Option<Self> {
        let result_code = Self::extract_xml_value(xml, "resultCode")
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);
        let msg = Self::extract_xml_value(xml, "msg").unwrap_or_default();
        Some(Self { result_code, msg })
    }

    fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
        let start_tag = format!("<{}>", tag);
        let end_tag = format!("</{}>", tag);
        if let Some(start) = xml.find(&start_tag) {
            if let Some(end) = xml.find(&end_tag) {
                let value_start = start + start_tag.len();
                if value_start < end {
                    return Some(xml[value_start..end].to_string());
                }
            }
        }
        None
    }
}

/// 令牌信息 / Token info
#[derive(Debug, Clone, Default)]
pub struct TokenInfo {
    pub authorization: String,
    pub account: String,
    pub personal_cloud_host: String,
    pub user_domain_id: String,
}
