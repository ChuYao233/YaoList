//! Cloud189 upload functionality / 天翼云盘上传功能

use anyhow::{anyhow, Result};
use reqwest::Client;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use super::types::*;
use super::utils::*;

/// Upload progress callback / 上传进度回调
pub type UploadProgressCallback = Box<dyn Fn(f64) + Send + Sync>;

/// Streaming upload implementation - copied from StreamUpload / 流式上传实现
pub struct StreamUploader<'a> {
    pub client: &'a Client,
    pub token_info: &'a AppSessionResp,
    pub is_family: bool,
    pub family_id: String,
    pub upload_thread: i32,
}

impl<'a> StreamUploader<'a> {
    /// Initialize multipart upload / 初始化多段上传
    pub async fn init_multi_upload(
        &self,
        parent_folder_id: &str,
        file_name: &str,
        file_size: i64,
    ) -> Result<InitMultiUploadResp> {
        let slice_size = part_size(file_size);
        
        let mut full_url = UPLOAD_URL.to_string();
        let mut params: Vec<(&str, String)> = vec![
            ("parentFolderId", parent_folder_id.to_string()),
            ("fileName", urlencoding::encode(file_name).to_string()),
            ("fileSize", file_size.to_string()),
            ("sliceSize", slice_size.to_string()),
            ("lazyCheck", "1".to_string()),
        ];

        if self.is_family {
            params.push(("familyId", self.family_id.clone()));
            full_url.push_str("/family");
        } else {
            full_url.push_str("/person");
        }
        full_url.push_str("/initMultiUpload");

        let session_secret = if self.is_family {
            &self.token_info.user_session.family_session_secret
        } else {
            &self.token_info.user_session.session_secret
        };

        let params_ref: Vec<(&str, &str)> = params.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        let encrypted_params = encrypt_params(&params_ref, session_secret);

        let date = get_http_date_str();
        let session_key = if self.is_family {
            &self.token_info.user_session.family_session_key
        } else {
            &self.token_info.user_session.session_key
        };
        let signature = signature_of_hmac(session_secret, session_key, "GET", &full_url, &date, &encrypted_params);

        let mut query = client_suffix();
        if !encrypted_params.is_empty() {
            query.push(("params".to_string(), encrypted_params));
        }

        let resp = self.client
            .get(&full_url)
            .query(&query)
            .header("Date", &date)
            .header("SessionKey", session_key)
            .header("Signature", signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;

        let text = resp.text().await?;
        
        // Check errors / 检查错误
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("Failed to initialize upload / 初始化上传失败: {}", err.error_message()));
            }
        }

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse init upload response / 解析初始化上传响应失败: {} - {}", e, text))
    }

    /// Get upload URLs - copied from GetMultiUploadUrls / 获取上传URL
    pub async fn get_multi_upload_urls(
        &self,
        upload_file_id: &str,
        part_info: &str,
    ) -> Result<Vec<UploadUrlInfo>> {
        let mut full_url = UPLOAD_URL.to_string();
        if self.is_family {
            full_url.push_str("/family");
        } else {
            full_url.push_str("/person");
        }
        full_url.push_str("/getMultiUploadUrls");

        let params = vec![
            ("uploadFileId", upload_file_id),
            ("partInfo", part_info),
        ];

        let session_secret = if self.is_family {
            &self.token_info.user_session.family_session_secret
        } else {
            &self.token_info.user_session.session_secret
        };
        let encrypted_params = encrypt_params(&params, session_secret);

        let date = get_http_date_str();
        let session_key = if self.is_family {
            &self.token_info.user_session.family_session_key
        } else {
            &self.token_info.user_session.session_key
        };
        let signature = signature_of_hmac(session_secret, session_key, "GET", &full_url, &date, &encrypted_params);

        let mut query = client_suffix();
        if !encrypted_params.is_empty() {
            query.push(("params".to_string(), encrypted_params));
        }

        let resp = self.client
            .get(&full_url)
            .query(&query)
            .header("Date", &date)
            .header("SessionKey", session_key)
            .header("Signature", signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;

        let text = resp.text().await?;
        let urls_resp: UploadUrlsResp = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse upload URL response / 解析上传URL响应失败: {} - {}", e, text))?;

        let mut upload_infos: Vec<UploadUrlInfo> = Vec::new();
        for (k, v) in urls_resp.upload_urls {
            let part_number: i32 = k.trim_start_matches("partNumber_").parse().unwrap_or(1);
            upload_infos.push(UploadUrlInfo {
                part_number,
                headers: parse_http_header(&v.request_header),
                request_url: v.request_url,
            });
        }
        upload_infos.sort_by(|a, b| a.part_number.cmp(&b.part_number));

        Ok(upload_infos)
    }

    /// Upload chunk / 上传分片
    pub async fn upload_part(
        &self,
        upload_url: &UploadUrlInfo,
        data: Vec<u8>,
    ) -> Result<()> {
        let mut req = self.client.put(&upload_url.request_url);
        
        for (k, v) in &upload_url.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let resp = req.body(data).send().await?;
        
        if !resp.status().is_success() {
            let text = resp.text().await?;
            return Err(anyhow!("Failed to upload chunk / 上传分片失败: {}", text));
        }

        Ok(())
    }

    /// Commit upload - copied from commitMultiUploadFile / 提交上传
    pub async fn commit_multi_upload(
        &self,
        upload_file_id: &str,
        file_md5: &str,
        slice_md5: &str,
        overwrite: bool,
    ) -> Result<CommitMultiUploadFileResp> {
        let mut full_url = UPLOAD_URL.to_string();
        if self.is_family {
            full_url.push_str("/family");
        } else {
            full_url.push_str("/person");
        }
        full_url.push_str("/commitMultiUploadFile");

        let opertype = if overwrite { "3" } else { "1" };
        let params = vec![
            ("uploadFileId", upload_file_id),
            ("fileMd5", file_md5),
            ("sliceMd5", slice_md5),
            ("lazyCheck", "1"),
            ("isLog", "0"),
            ("opertype", opertype),
        ];

        let session_secret = if self.is_family {
            &self.token_info.user_session.family_session_secret
        } else {
            &self.token_info.user_session.session_secret
        };
        let encrypted_params = encrypt_params(&params, session_secret);

        let date = get_http_date_str();
        let session_key = if self.is_family {
            &self.token_info.user_session.family_session_key
        } else {
            &self.token_info.user_session.session_key
        };
        let signature = signature_of_hmac(session_secret, session_key, "GET", &full_url, &date, &encrypted_params);

        let mut query = client_suffix();
        if !encrypted_params.is_empty() {
            query.push(("params".to_string(), encrypted_params));
        }

        let resp = self.client
            .get(&full_url)
            .query(&query)
            .header("Date", &date)
            .header("SessionKey", session_key)
            .header("Signature", signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;

        let text = resp.text().await?;
        
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("Failed to commit upload / 提交上传失败: {}", err.error_message()));
            }
        }

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse commit upload response / 解析提交上传响应失败: {} - {}", e, text))
    }
}

/// Legacy upload implementation - copied from OldUpload / 旧版上传实现
pub struct OldUploader<'a> {
    pub client: &'a Client,
    pub token_info: &'a AppSessionResp,
    pub is_family: bool,
    pub family_id: String,
}

impl<'a> OldUploader<'a> {
    /// Create upload session - copied from OldUploadCreate / 创建上传会话
    pub async fn create_upload(
        &self,
        parent_id: &str,
        file_md5: &str,
        file_name: &str,
        file_size: &str,
    ) -> Result<CreateUploadFileResp> {
        let mut full_url = API_URL.to_string();
        
        let date = get_http_date_str();
        let session_secret = if self.is_family {
            &self.token_info.user_session.family_session_secret
        } else {
            &self.token_info.user_session.session_secret
        };
        let session_key = if self.is_family {
            &self.token_info.user_session.family_session_key
        } else {
            &self.token_info.user_session.session_key
        };
        let signature = signature_of_hmac(session_secret, session_key, "POST", &full_url, &date, "");

        let mut req = self.client.post(&full_url)
            .header("Date", &date)
            .header("SessionKey", session_key)
            .header("Signature", signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8");

        if self.is_family {
            full_url.push_str("/family/file/createFamilyFile.action");
            req = self.client.post(&full_url)
                .query(&client_suffix())
                .query(&[
                    ("familyId", &self.family_id),
                    ("parentId", &parent_id.to_string()),
                    ("fileMd5", &file_md5.to_string()),
                    ("fileName", &file_name.to_string()),
                    ("fileSize", &file_size.to_string()),
                    ("resumePolicy", &"1".to_string()),
                ]);
        } else {
            full_url.push_str("/createUploadFile.action");
            req = self.client.post(&full_url)
                .query(&client_suffix())
                .form(&[
                    ("parentFolderId", parent_id),
                    ("fileName", file_name),
                    ("size", file_size),
                    ("md5", file_md5),
                    ("opertype", "3"),
                    ("flag", "1"),
                    ("resumePolicy", "1"),
                    ("isLog", "0"),
                ]);
        }

        let resp = req.send().await?;
        let text = resp.text().await?;

        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("Failed to create upload / 创建上传失败: {}", err.error_message()));
            }
        }

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse create upload response / 解析创建上传响应失败: {} - {}", e, text))
    }

    /// Commit upload - copied from OldUploadCommit / 提交上传
    pub async fn commit_upload(
        &self,
        file_commit_url: &str,
        upload_file_id: i64,
        overwrite: bool,
    ) -> Result<OldCommitUploadFileResp> {
        let date = get_http_date_str();
        let session_secret = if self.is_family {
            &self.token_info.user_session.family_session_secret
        } else {
            &self.token_info.user_session.session_secret
        };
        let session_key = if self.is_family {
            &self.token_info.user_session.family_session_key
        } else {
            &self.token_info.user_session.session_key
        };
        let signature = signature_of_hmac(session_secret, session_key, "POST", file_commit_url, &date, "");

        let mut req = self.client.post(file_commit_url)
            .query(&client_suffix())
            .header("Date", &date)
            .header("SessionKey", session_key)
            .header("Signature", signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string());

        if self.is_family {
            req = req
                .header("ResumePolicy", "1")
                .header("UploadFileId", upload_file_id.to_string())
                .header("FamilyId", &self.family_id);
        } else {
            let opertype = if overwrite { "3" } else { "1" };
            req = req.form(&[
                ("opertype", opertype),
                ("resumePolicy", "1"),
                ("uploadFileId", &upload_file_id.to_string()),
                ("isLog", "0"),
            ]);
        }

        let resp = req.send().await?;
        let text = resp.text().await?;

        // Try JSON / 尝试JSON
        if let Ok(resp) = serde_json::from_str::<OldCommitUploadFileResp>(&text) {
            return Ok(resp);
        }
        // Try XML / 尝试XML
        if text.contains("<file>") {
            return quick_xml::de::from_str(&text)
                .map_err(|e| anyhow!("Failed to parse commit response XML / 解析提交响应XML失败: {} - {}", e, text));
        }

        Err(anyhow!("Failed to parse commit upload response / 解析提交上传响应失败: {}", text))
    }
}

/// Calculate file MD5 / 计算文件MD5
pub fn calculate_md5(data: &[u8]) -> String {
    let digest = md5::compute(data);
    format!("{:X}", digest)
}

/// Calculate chunk MD5 and return hex and base64 / 计算分片MD5并返回
pub fn calculate_slice_md5(data: &[u8]) -> (String, String) {
    let digest = md5::compute(data);
    let hex_str = format!("{:X}", digest);
    let base64_str = BASE64.encode(digest.as_ref());
    (hex_str, base64_str)
}
