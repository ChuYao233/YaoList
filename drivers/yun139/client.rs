//! 139云盘API客户端 / 139Yun API client

use anyhow::{anyhow, Result};
use reqwest::{Client, Method};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

use super::types::*;
use super::util::*;

/// 139云盘API客户端 / 139Yun API client
pub struct Yun139Client {
    client: Client,
    token_info: Arc<RwLock<TokenInfo>>,
    cloud_type: CloudType,
    cloud_id: String,
}

impl Yun139Client {
    /// 创建新客户端 / Create new client
    pub fn new(cloud_type: CloudType, cloud_id: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap(),
            token_info: Arc::new(RwLock::new(TokenInfo::default())),
            cloud_type,
            cloud_id,
        }
    }

    /// 初始化令牌 / Initialize token
    pub fn init_token(&self, authorization: &str) {
        if let Some((_, account, _)) = decode_authorization(authorization) {
            let mut info = self.token_info.write().unwrap();
            info.authorization = authorization.to_string();
            info.account = account;
        }
    }

    /// 设置个人云主机 / Set personal cloud host
    pub fn set_personal_cloud_host(&self, host: &str) {
        let mut info = self.token_info.write().unwrap();
        info.personal_cloud_host = host.to_string();
    }

    /// 设置用户域ID / Set user domain ID
    pub fn set_user_domain_id(&self, id: &str) {
        let mut info = self.token_info.write().unwrap();
        info.user_domain_id = id.to_string();
    }

    /// 获取令牌信息 / Get token info
    pub fn get_token_info(&self) -> TokenInfo {
        self.token_info.read().unwrap().clone()
    }

    /// 更新Authorization / Update authorization
    pub fn update_authorization(&self, auth: &str) {
        let mut info = self.token_info.write().unwrap();
        info.authorization = auth.to_string();
        if let Some((_, account, _)) = decode_authorization(auth) {
            info.account = account;
        }
    }

    /// 获取账号 / Get account
    fn get_account(&self) -> String {
        self.token_info.read().unwrap().account.clone()
    }

    /// 获取Authorization / Get authorization
    fn get_authorization(&self) -> String {
        self.token_info.read().unwrap().authorization.clone()
    }

    /// 获取个人云主机 / Get personal cloud host
    fn get_personal_cloud_host(&self) -> String {
        self.token_info.read().unwrap().personal_cloud_host.clone()
    }

    /// 是否是家庭云 / Is family cloud
    fn is_family(&self) -> bool {
        self.cloud_type == CloudType::Family
    }

    /// 构建通用请求头 / Build common headers
    fn build_headers(&self, body: &str) -> Vec<(String, String)> {
        let rand_str = random_string(16);
        let ts = get_timestamp();
        let sign = calc_sign(body, &ts, &rand_str);
        let svc_type = self.cloud_type.svc_type();

        vec![
            ("Accept".to_string(), "application/json, text/plain, */*".to_string()),
            ("CMS-DEVICE".to_string(), "default".to_string()),
            ("Authorization".to_string(), format!("Basic {}", self.get_authorization())),
            ("mcloud-channel".to_string(), "1000101".to_string()),
            ("mcloud-client".to_string(), "10701".to_string()),
            ("mcloud-sign".to_string(), format!("{},{},{}", ts, rand_str, sign)),
            ("mcloud-version".to_string(), "7.14.0".to_string()),
            ("Origin".to_string(), "https://yun.139.com".to_string()),
            ("Referer".to_string(), "https://yun.139.com/w/".to_string()),
            ("x-DeviceInfo".to_string(), "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".to_string()),
            ("x-huawei-channelSrc".to_string(), "10000034".to_string()),
            ("x-inner-ntwk".to_string(), "2".to_string()),
            ("x-m4c-caller".to_string(), "PC".to_string()),
            ("x-m4c-src".to_string(), "10002".to_string()),
            ("x-SvcType".to_string(), svc_type.to_string()),
            ("Inner-Hcy-Router-Https".to_string(), "1".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ]
    }

    /// 构建个人版请求头 / Build personal request headers
    fn build_personal_headers(&self, body: &str) -> Vec<(String, String)> {
        let rand_str = random_string(16);
        let ts = get_timestamp();
        let sign = calc_sign(body, &ts, &rand_str);
        let svc_type = self.cloud_type.svc_type();

        vec![
            ("Accept".to_string(), "application/json, text/plain, */*".to_string()),
            ("Authorization".to_string(), format!("Basic {}", self.get_authorization())),
            ("Caller".to_string(), "web".to_string()),
            ("Cms-Device".to_string(), "default".to_string()),
            ("Mcloud-Channel".to_string(), "1000101".to_string()),
            ("Mcloud-Client".to_string(), "10701".to_string()),
            ("Mcloud-Route".to_string(), "001".to_string()),
            ("Mcloud-Sign".to_string(), format!("{},{},{}", ts, rand_str, sign)),
            ("Mcloud-Version".to_string(), "7.14.0".to_string()),
            ("x-DeviceInfo".to_string(), "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".to_string()),
            ("x-huawei-channelSrc".to_string(), "10000034".to_string()),
            ("x-inner-ntwk".to_string(), "2".to_string()),
            ("x-m4c-caller".to_string(), "PC".to_string()),
            ("x-m4c-src".to_string(), "10002".to_string()),
            ("x-SvcType".to_string(), svc_type.to_string()),
            ("X-Yun-Api-Version".to_string(), "v1".to_string()),
            ("X-Yun-App-Channel".to_string(), "10000034".to_string()),
            ("X-Yun-Channel-Source".to_string(), "10000034".to_string()),
            ("X-Yun-Client-Info".to_string(), "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||dW5kZWZpbmVk||".to_string()),
            ("X-Yun-Module-Type".to_string(), "100".to_string()),
            ("X-Yun-Svc-Type".to_string(), "1".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ]
    }

    /// 发送请求 / Send request
    pub async fn request<T: DeserializeOwned>(
        &self,
        url: &str,
        method: Method,
        body: Option<Value>,
    ) -> Result<T> {
        let body_str = body.as_ref()
            .map(|b| serde_json::to_string(b).unwrap_or_default())
            .unwrap_or_default();
        
        let headers = self.build_headers(&body_str);
        
        let mut req = self.client.request(method, url);
        for (key, value) in headers {
            req = req.header(&key, &value);
        }
        
        if let Some(b) = body {
            req = req.json(&b);
        }
        
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        
        tracing::debug!("[139] Response: {} - {}", status, &text[..text.len().min(500)]);
        
        let base: BaseResp = serde_json::from_str(&text).unwrap_or_default();
        if !base.success {
            return Err(anyhow!("API error: {}", base.message));
        }
        
        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Parse response failed: {}", e))
    }

    /// POST请求 / POST request
    pub async fn post<T: DeserializeOwned>(&self, pathname: &str, body: Value) -> Result<T> {
        let url = format!("https://yun.139.com{}", pathname);
        self.request(&url, Method::POST, Some(body)).await
    }

    /// 个人版POST请求 / Personal POST request
    pub async fn personal_post<T: DeserializeOwned>(&self, pathname: &str, body: Value) -> Result<T> {
        let host = self.get_personal_cloud_host();
        let url = format!("{}{}", host, pathname);
        
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        let headers = self.build_personal_headers(&body_str);
        
        let mut req = self.client.post(&url);
        for (key, value) in headers {
            req = req.header(&key, &value);
        }
        req = req.json(&body);
        
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        
        tracing::debug!("[139] Personal Response: {} - {}", status, &text[..text.len().min(500)]);
        
        let base: BaseResp = serde_json::from_str(&text).unwrap_or_default();
        if !base.success {
            return Err(anyhow!("API error: {}", base.message));
        }
        
        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Parse response failed: {}", e))
    }

    /// 查询路由策略 / Query route policy
    pub async fn query_route_policy(&self) -> Result<QueryRoutePolicyResp> {
        let url = "https://user-njs.yun.139.com/user/route/qryRoutePolicy";
        let body = json!({
            "userInfo": {
                "userType": 1,
                "accountType": 1,
                "accountName": self.get_account(),
            },
            "modAddrType": 1,
        });
        
        self.request(url, Method::POST, Some(body)).await
    }

    /// 刷新令牌 / Refresh token
    pub async fn refresh_token(&self) -> Result<String> {
        let auth = self.get_authorization();
        if let Some((prefix, account, token)) = decode_authorization(&auth) {
            let parts: Vec<&str> = token.split('|').collect();
            if parts.len() >= 4 {
                let expiration: i64 = parts[3].parse().unwrap_or(0);
                let now = chrono::Utc::now().timestamp_millis();
                
                // 有效期大于15天无需刷新
                if expiration - now > 1000 * 60 * 60 * 24 * 15 {
                    return Ok(auth);
                }
                
                if expiration < now {
                    return Err(anyhow!("Authorization has expired"));
                }
            }
            
            let url = "https://aas.caiyun.feixin.10086.cn:443/tellin/authTokenRefresh.do";
            let req_body = format!(
                "<root><token>{}</token><account>{}</account><clienttype>656</clienttype></root>",
                token, account
            );
            
            let resp = self.client
                .post(url)
                .header("Content-Type", "application/xml")
                .body(req_body)
                .send()
                .await?;
            
            let text = resp.text().await?;
            
            // 解析XML响应
            if let Some(new_token) = Self::extract_xml_value(&text, "token") {
                let new_auth = encode_authorization(&prefix, &account, &new_token);
                self.update_authorization(&new_auth);
                return Ok(new_auth);
            }
        }
        
        Err(anyhow!("Failed to refresh token"))
    }

    /// 从XML中提取值 / Extract value from XML
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

    /// 构建通用JSON(家庭/群组) / Build common JSON for family/group
    pub fn new_json(&self, data: Value) -> Value {
        let mut result = json!({
            "catalogType": 3,
            "cloudID": self.cloud_id,
            "cloudType": 1,
            "commonAccountInfo": {
                "account": self.get_account(),
                "accountType": 1,
            },
        });
        
        if let (Some(result_obj), Some(data_obj)) = (result.as_object_mut(), data.as_object()) {
            for (k, v) in data_obj {
                result_obj.insert(k.clone(), v.clone());
            }
        }
        
        result
    }

    // ==================== 个人版(新) API ====================

    /// 个人版获取文件列表 / Personal get files
    pub async fn personal_get_files(&self, file_id: &str) -> Result<Vec<PersonalFileItem>> {
        let mut files = Vec::new();
        let mut next_page_cursor = String::new();
        
        loop {
            let body = json!({
                "imageThumbnailStyleList": ["Small", "Large"],
                "orderBy": "updated_at",
                "orderDirection": "DESC",
                "pageInfo": {
                    "pageCursor": next_page_cursor,
                    "pageSize": 100,
                },
                "parentFileId": file_id,
            });
            
            let resp: Value = self.personal_post("/file/list", body).await?;
            
            // 手动解析items数组
            if let Some(items) = resp["data"]["items"].as_array() {
                for item in items {
                    files.push(PersonalFileItem::from_value(item));
                }
            }
            
            next_page_cursor = resp["data"]["nextPageCursor"]
                .as_str()
                .unwrap_or("")
                .to_string();
            
            if next_page_cursor.is_empty() {
                break;
            }
        }
        
        Ok(files)
    }

    /// 个人版获取下载链接 / Personal get download link
    pub async fn personal_get_link(&self, file_id: &str) -> Result<String> {
        let body = json!({ "fileId": file_id });
        let resp: Value = self.personal_post("/file/getDownloadUrl", body).await?;
        
        let cdn_url = resp["data"]["cdnUrl"].as_str().unwrap_or("");
        if !cdn_url.is_empty() {
            return Ok(cdn_url.to_string());
        }
        
        let url = resp["data"]["url"].as_str().unwrap_or("");
        Ok(url.to_string())
    }

    /// 个人版创建文件夹 / Personal create folder
    pub async fn personal_create_folder(&self, parent_id: &str, name: &str) -> Result<()> {
        let body = json!({
            "parentFileId": parent_id,
            "name": name,
            "description": "",
            "type": "folder",
            "fileRenameMode": "force_rename",
        });
        
        let _: Value = self.personal_post("/file/create", body).await?;
        Ok(())
    }

    /// 个人版删除 / Personal delete
    pub async fn personal_delete(&self, file_ids: Vec<String>) -> Result<()> {
        let body = json!({ "fileIds": file_ids });
        let _: Value = self.personal_post("/recyclebin/batchTrash", body).await?;
        Ok(())
    }

    /// 个人版重命名 / Personal rename
    pub async fn personal_rename(&self, file_id: &str, new_name: &str) -> Result<()> {
        let body = json!({
            "fileId": file_id,
            "name": new_name,
            "description": "",
        });
        let _: Value = self.personal_post("/file/update", body).await?;
        Ok(())
    }

    /// 个人版移动 / Personal move
    pub async fn personal_move(&self, file_ids: Vec<String>, to_parent_id: &str) -> Result<()> {
        let body = json!({
            "fileIds": file_ids,
            "toParentFileId": to_parent_id,
        });
        let _: Value = self.personal_post("/file/batchMove", body).await?;
        Ok(())
    }

    /// 个人版复制 / Personal copy
    pub async fn personal_copy(&self, file_ids: Vec<String>, to_parent_id: &str) -> Result<()> {
        let body = json!({
            "fileIds": file_ids,
            "toParentFileId": to_parent_id,
        });
        let _: Value = self.personal_post("/file/batchCopy", body).await?;
        Ok(())
    }

    /// 个人版创建上传任务 / Personal create upload task
    pub async fn personal_create_upload(
        &self,
        parent_id: &str,
        name: &str,
        size: i64,
        hash: &str,
        part_infos: Vec<PartInfo>,
    ) -> Result<PersonalUploadResp> {
        // 根据文件名推断MIME类型
        let content_type = mime_guess::from_path(name)
            .first_or_octet_stream()
            .to_string();
        
        let body = json!({
            "contentHash": hash,
            "contentHashAlgorithm": "SHA256",
            "contentType": content_type,
            "parallelUpload": false,
            "partInfos": part_infos,
            "size": size,
            "parentFileId": parent_id,
            "name": name,
            "type": "file",
            "fileRenameMode": "auto_rename",
        });
        
        self.personal_post("/file/create", body).await
    }

    /// 个人版获取上传URL / Personal get upload URL
    pub async fn personal_get_upload_url(
        &self,
        file_id: &str,
        upload_id: &str,
        part_infos: Vec<PartInfo>,
    ) -> Result<PersonalUploadUrlResp> {
        let body = json!({
            "fileId": file_id,
            "uploadId": upload_id,
            "partInfos": part_infos,
            "commonAccountInfo": {
                "account": self.get_account(),
                "accountType": 1,
            },
        });
        
        self.personal_post("/file/getUploadUrl", body).await
    }

    /// 个人版完成上传 / Personal complete upload
    pub async fn personal_complete_upload(
        &self,
        file_id: &str,
        upload_id: &str,
        hash: &str,
    ) -> Result<()> {
        let body = json!({
            "contentHash": hash,
            "contentHashAlgorithm": "SHA256",
            "fileId": file_id,
            "uploadId": upload_id,
        });
        
        let _: Value = self.personal_post("/file/complete", body).await?;
        Ok(())
    }

    // ==================== 个人版(旧) API ====================

    /// 旧版获取文件列表 / Old version get files
    pub async fn get_files(&self, catalog_id: &str) -> Result<Vec<(Content, Catalog)>> {
        let mut files = Vec::new();
        let mut start = 0;
        let limit = 100;
        
        loop {
            let body = json!({
                "catalogID": catalog_id,
                "sortDirection": 1,
                "startNumber": start + 1,
                "endNumber": start + limit,
                "filterType": 0,
                "catalogSortType": 0,
                "contentSortType": 0,
                "commonAccountInfo": {
                    "account": self.get_account(),
                    "accountType": 1,
                },
            });
            
            let resp: GetDiskResp = self.post(
                "/orchestration/personalCloud/catalog/v1.0/getDisk",
                body,
            ).await?;
            
            for catalog in resp.data.get_disk_result.catalog_list {
                files.push((Content::default(), catalog));
            }
            
            for content in resp.data.get_disk_result.content_list {
                files.push((content, Catalog::default()));
            }
            
            if start + limit >= resp.data.get_disk_result.node_count {
                break;
            }
            start += limit;
        }
        
        Ok(files)
    }

    /// 旧版获取下载链接 / Old version get download link
    pub async fn get_link(&self, content_id: &str) -> Result<String> {
        let body = json!({
            "appName": "",
            "contentID": content_id,
            "commonAccountInfo": {
                "account": self.get_account(),
                "accountType": 1,
            },
        });
        
        let resp: DownloadUrlResp = self.post(
            "/orchestration/personalCloud/uploadAndDownload/v1.0/downloadRequest",
            body,
        ).await?;
        
        Ok(resp.data.download_url)
    }

    // ==================== 家庭版 API ====================

    /// 家庭版获取文件列表 / Family get files
    pub async fn family_get_files(&self, catalog_id: &str) -> Result<(Vec<CloudContent>, Vec<CloudCatalog>, String)> {
        let mut contents = Vec::new();
        let mut catalogs = Vec::new();
        let mut page_num = 1;
        let mut root_path = String::new();
        
        loop {
            let body = self.new_json(json!({
                "catalogID": catalog_id,
                "contentSortType": 0,
                "pageInfo": {
                    "pageNum": page_num,
                    "pageSize": 100,
                },
                "sortDirection": 1,
            }));
            
            let resp: QueryContentListResp = self.post(
                "/orchestration/familyCloud-rebuild/content/v1.2/queryContentList",
                body,
            ).await?;
            
            if root_path.is_empty() {
                root_path = resp.data.path.clone();
            }
            
            catalogs.extend(resp.data.cloud_catalog_list);
            contents.extend(resp.data.cloud_content_list);
            
            if resp.data.total_count == 0 {
                break;
            }
            page_num += 1;
        }
        
        Ok((contents, catalogs, root_path))
    }

    /// 家庭版获取下载链接 / Family get download link
    pub async fn family_get_link(&self, content_id: &str, path: &str) -> Result<String> {
        let body = self.new_json(json!({
            "contentID": content_id,
            "path": path,
        }));
        
        let resp: DownloadUrlResp = self.post(
            "/orchestration/familyCloud-rebuild/content/v1.0/getFileDownLoadURL",
            body,
        ).await?;
        
        Ok(resp.data.download_url)
    }

    // ==================== 群组版 API ====================

    /// 群组版获取文件列表 / Group get files
    pub async fn group_get_files(&self, catalog_id: &str, root_folder_id: &str) -> Result<(Vec<Content>, Vec<GroupCatalog>, String)> {
        let mut contents = Vec::new();
        let mut catalogs = Vec::new();
        let mut page_num = 1;
        let mut root_path = String::new();
        
        loop {
            let catalog_base = catalog_id.rsplit('/').next().unwrap_or(catalog_id);
            let body = self.new_json(json!({
                "groupID": self.cloud_id,
                "catalogID": catalog_base,
                "contentSortType": 0,
                "sortDirection": 1,
                "startNumber": page_num,
                "endNumber": page_num + 99,
                "path": join_path(root_folder_id, catalog_id),
            }));
            
            let resp: QueryGroupContentListResp = self.post(
                "/orchestration/group-rebuild/content/v1.0/queryGroupContentList",
                body,
            ).await?;
            
            if root_path.is_empty() {
                root_path = resp.data.get_group_content_result.parent_catalog_id.clone();
            }
            
            catalogs.extend(resp.data.get_group_content_result.catalog_list);
            contents.extend(resp.data.get_group_content_result.content_list);
            
            if page_num + 99 > resp.data.get_group_content_result.node_count {
                break;
            }
            page_num += 100;
        }
        
        Ok((contents, catalogs, root_path))
    }

    /// 群组版获取下载链接 / Group get download link
    pub async fn group_get_link(&self, content_id: &str, path: &str) -> Result<String> {
        let body = self.new_json(json!({
            "contentID": content_id,
            "groupID": self.cloud_id,
            "path": path,
        }));
        
        let resp: DownloadUrlResp = self.post(
            "/orchestration/group-rebuild/groupManage/v1.0/getGroupFileDownLoadURL",
            body,
        ).await?;
        
        Ok(resp.data.download_url)
    }

    // ==================== 通用操作 API ====================

    /// 创建文件夹(旧版) / Create folder (old version)
    pub async fn create_folder(&self, parent_id: &str, name: &str) -> Result<()> {
        let body = json!({
            "createCatalogExtReq": {
                "parentCatalogID": parent_id,
                "newCatalogName": name,
                "commonAccountInfo": {
                    "account": self.get_account(),
                    "accountType": 1,
                },
            },
        });
        
        let _: Value = self.post(
            "/orchestration/personalCloud/catalog/v1.0/createCatalogExt",
            body,
        ).await?;
        Ok(())
    }

    /// 删除(旧版/家庭版) / Delete (old/family version)
    pub async fn delete(&self, content_ids: Vec<String>, catalog_ids: Vec<String>, path: &str) -> Result<()> {
        let body = json!({
            "createBatchOprTaskReq": {
                "taskType": 2,
                "actionType": 201,
                "taskInfo": {
                    "newCatalogID": "",
                    "contentInfoList": content_ids,
                    "catalogInfoList": catalog_ids,
                },
                "commonAccountInfo": {
                    "account": self.get_account(),
                    "accountType": 1,
                },
            },
        });
        
        let pathname = if self.is_family() {
            "/orchestration/familyCloud-rebuild/batchOprTask/v1.0/createBatchOprTask"
        } else {
            "/orchestration/personalCloud/batchOprTask/v1.0/createBatchOprTask"
        };
        
        let _: Value = self.post(pathname, body).await?;
        Ok(())
    }

    /// 获取上传URL(旧版/家庭/群组) / Get upload URL (old/family/group version)
    pub async fn get_upload_url(
        &self,
        parent_id: &str,
        name: &str,
        size: i64,
        path: Option<&str>,
    ) -> Result<UploadResp> {
        let body = if self.is_family() || self.cloud_type == CloudType::Group {
            self.new_json(json!({
                "fileCount": 1,
                "manualRename": 2,
                "operation": 0,
                "path": path.unwrap_or(""),
                "seqNo": random_string(32),
                "totalSize": size,
                "uploadContentList": [{
                    "contentName": name,
                    "contentSize": size,
                }],
            }))
        } else {
            json!({
                "manualRename": 2,
                "operation": 0,
                "fileCount": 1,
                "totalSize": size,
                "uploadContentList": [{
                    "contentName": name,
                    "contentSize": size,
                }],
                "parentCatalogID": parent_id,
                "newCatalogName": "",
                "commonAccountInfo": {
                    "account": self.get_account(),
                    "accountType": 1,
                },
            })
        };
        
        let pathname = if self.is_family() || self.cloud_type == CloudType::Group {
            "/orchestration/familyCloud-rebuild/content/v1.0/getFileUploadURL"
        } else {
            "/orchestration/personalCloud/uploadAndDownload/v1.0/pcUploadFileRequest"
        };
        
        self.post(pathname, body).await
    }

    /// 获取磁盘信息 / Get disk info
    pub async fn get_disk_info(&self) -> Result<(u64, u64)> {
        let user_domain_id = self.token_info.read().unwrap().user_domain_id.clone();
        let body = json!({ "userDomainId": user_domain_id });
        
        if self.is_family() {
            let resp: FamilyDiskInfoResp = self.request(
                "https://user-njs.yun.139.com/user/disk/getFamilyDiskInfo",
                Method::POST,
                Some(body),
            ).await?;
            
            let total: u64 = resp.data.disk_size.parse().unwrap_or(0) * 1024 * 1024;
            let used: u64 = resp.data.used_size.parse().unwrap_or(0) * 1024 * 1024;
            Ok((total, used))
        } else {
            let resp: PersonalDiskInfoResp = self.request(
                "https://user-njs.yun.139.com/user/disk/getPersonalDiskInfo",
                Method::POST,
                Some(body),
            ).await?;
            
            let total: u64 = resp.data.disk_size.parse().unwrap_or(0) * 1024 * 1024;
            let free: u64 = resp.data.free_disk_size.parse().unwrap_or(0) * 1024 * 1024;
            Ok((total, total.saturating_sub(free)))
        }
    }
}
