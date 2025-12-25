//! 115云盘API客户端

use anyhow::{Result, anyhow};
use reqwest::{Client, header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT, CONTENT_TYPE}};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::*;
use super::crypto::*;

const API_FILE_LIST: &str = "https://webapi.115.com/files";
const API_FILE_INFO: &str = "https://webapi.115.com/files/file";
const API_DOWNLOAD: &str = "https://proapi.115.com/android/2.0/ufile/download";
const API_DIR_ADD: &str = "https://webapi.115.com/files/add";
const API_FILE_MOVE: &str = "https://webapi.115.com/files/move";
const API_FILE_RENAME: &str = "https://webapi.115.com/files/batch_rename";
const API_FILE_COPY: &str = "https://webapi.115.com/files/copy";
const API_FILE_DELETE: &str = "https://webapi.115.com/rb/delete";
const API_UPLOAD_INIT: &str = "https://uplb.115.com/4.0/initupload.php";
const API_UPLOAD_INFO: &str = "https://proapi.115.com/app/uploadinfo";
const API_OSS_TOKEN: &str = "https://uplb.115.com/3.0/getuploadinfo.php";
const API_USER_INFO: &str = "https://my.115.com/?ct=ajax&ac=nav";
const API_SPACE_INFO: &str = "https://webapi.115.com/files/index_info";
const API_VERSION: &str = "https://appversion.115.com/1/web/1.0/api/getMultiVer";

const DEFAULT_APP_VER: &str = "35.6.0.3";
const OSS_ENDPOINT: &str = "https://oss-cn-shenzhen.aliyuncs.com";
const OSS_USER_AGENT: &str = "aliyun-sdk-android/2.9.1";

pub struct Pan115Client {
    http: Client,
    cookie: String,
    pub user_id: i64,
    pub app_ver: String,
    pub page_size: i64,
    upload_info: Option<UploadInfoResp>,
}

impl Pan115Client {
    pub fn new(cookie: &str, page_size: i64) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        
        Ok(Self {
            http,
            cookie: cookie.to_string(),
            user_id: 0,
            app_ver: DEFAULT_APP_VER.to_string(),
            page_size: if page_size > 0 { page_size } else { 1000 },
            upload_info: None,
        })
    }
    
    fn get_ua(&self) -> String {
        format!("Mozilla/5.0 115Browser/{}", self.app_ver)
    }
    
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&self.cookie) {
            headers.insert(COOKIE, v);
        }
        if let Ok(v) = HeaderValue::from_str(&self.get_ua()) {
            headers.insert(USER_AGENT, v);
        }
        headers
    }
    
    fn now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
    
    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
    
    pub async fn init(&mut self) -> Result<()> {
        self.app_ver = self.get_app_version().await.unwrap_or_else(|_| DEFAULT_APP_VER.to_string());
        
        let info = self.get_upload_info().await?;
        self.user_id = info.user_id;
        self.upload_info = Some(info);
        
        self.login_check().await?;
        
        Ok(())
    }
    
    async fn get_app_version(&self) -> Result<String> {
        let resp: VersionResp = self.http
            .get(API_VERSION)
            .headers(self.build_headers())
            .send()
            .await?
            .json()
            .await?;
        
        if !resp.error.is_empty() {
            return Err(anyhow!("{}", resp.error));
        }
        
        if !resp.data.win.version.is_empty() {
            Ok(resp.data.win.version)
        } else {
            Ok(DEFAULT_APP_VER.to_string())
        }
    }
    
    async fn login_check(&self) -> Result<()> {
        let resp: Value = self.http
            .get(API_USER_INFO)
            .headers(self.build_headers())
            .send()
            .await?
            .json()
            .await?;
        
        let state = resp["state"].as_bool().unwrap_or(false);
        if !state {
            return Err(anyhow!("Login check failed: {:?}", resp));
        }
        
        Ok(())
    }
    
    pub async fn get_upload_info(&self) -> Result<UploadInfoResp> {
        let resp: UploadInfoResp = self.http
            .get(API_UPLOAD_INFO)
            .headers(self.build_headers())
            .send()
            .await?
            .json()
            .await?;
        
        resp.base.check().map_err(|e| anyhow!("{}", e))?;
        Ok(resp)
    }
    
    pub async fn list_files(&self, dir_id: &str) -> Result<Vec<FileInfo>> {
        let mut all_files = Vec::new();
        let mut offset = 0i64;
        
        loop {
            let resp: Value = self.http
                .get(API_FILE_LIST)
                .headers(self.build_headers())
                .query(&[
                    ("aid", "1"),
                    ("cid", dir_id),
                    ("o", "user_ptime"),
                    ("asc", "0"),
                    ("offset", &offset.to_string()),
                    ("limit", &self.page_size.to_string()),
                    ("show_dir", "1"),
                    ("natsort", "1"),
                    ("source", ""),
                    ("format", "json"),
                ])
                .send()
                .await?
                .json()
                .await?;
            
            let state = resp["state"].as_bool().unwrap_or(false);
            if !state {
                let error = resp["error"].as_str().unwrap_or("Unknown error");
                if error.contains("目录不存在") || error.contains("20018") {
                    return Ok(vec![]);
                }
                return Err(anyhow!("List files failed: {}", error));
            }
            
            let data = resp["data"].as_array();
            if let Some(files) = data {
                for file in files {
                    let info: FileInfo = serde_json::from_value(file.clone())?;
                    all_files.push(info);
                }
                
                if files.len() < self.page_size as usize {
                    break;
                }
                offset += files.len() as i64;
            } else {
                break;
            }
        }
        
        Ok(all_files)
    }
    
    pub async fn get_file(&self, file_id: &str) -> Result<FileInfo> {
        let resp: Value = self.http
            .get(API_FILE_INFO)
            .headers(self.build_headers())
            .query(&[("file_id", file_id)])
            .send()
            .await?
            .json()
            .await?;
        
        let state = resp["state"].as_bool().unwrap_or(false);
        if !state {
            return Err(anyhow!("Get file failed: {:?}", resp));
        }
        
        let data = &resp["data"][0];
        let info: FileInfo = serde_json::from_value(data.clone())?;
        Ok(info)
    }
    
    pub async fn get_file_by_pick_code(&self, pick_code: &str) -> Result<FileInfo> {
        let resp: Value = self.http
            .get("https://webapi.115.com/files/file")
            .headers(self.build_headers())
            .query(&[("pick_code", pick_code)])
            .send()
            .await?
            .json()
            .await?;
        
        let state = resp["state"].as_bool().unwrap_or(false);
        if !state {
            return Err(anyhow!("Get file by pick_code failed: {:?}", resp));
        }
        
        if let Some(files) = resp["data"].as_array() {
            if !files.is_empty() {
                let info: FileInfo = serde_json::from_value(files[0].clone())?;
                return Ok(info);
            }
        }
        
        Err(anyhow!("File not found"))
    }
    
    pub async fn get_download_url(&self, pick_code: &str, user_agent: &str) -> Result<String> {
        let key = generate_random_key();
        let params = json!({ "pick_code": pick_code });
        let params_str = serde_json::to_string(&params)?;
        
        let encoded = m115_encode(params_str.as_bytes(), &key)?;
        
        let ts = Self::now_secs();
        let url = format!("{}?t={}", API_DOWNLOAD, ts);
        
        let mut headers = self.build_headers();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-www-form-urlencoded"));
        if !user_agent.is_empty() {
            if let Ok(v) = HeaderValue::from_str(user_agent) {
            headers.insert(USER_AGENT, v);
        }
        }
        
        let resp: Value = self.http
            .post(&url)
            .headers(headers)
            .form(&[("data", &encoded)])
            .send()
            .await?
            .json()
            .await?;
        
        let state = resp["state"].as_bool().unwrap_or(false);
        if !state {
            return Err(anyhow!("Get download url failed: {:?}", resp));
        }
        
        let encoded_data = resp["data"].as_str().ok_or_else(|| anyhow!("No data in response"))?;
        let decoded = m115_decode(encoded_data, &key)?;
        let decoded_str = String::from_utf8(decoded)?;
        tracing::debug!("115 download decoded response: {}", decoded_str);
        
        #[derive(serde::Deserialize)]
        struct DownloadInfo {
            url: String,
        }
        
        let info: DownloadInfo = serde_json::from_str(&decoded_str)?;
        Ok(info.url)
    }
    
    pub async fn mkdir(&self, parent_id: &str, name: &str) -> Result<MkdirResp> {
        let resp: MkdirResp = self.http
            .post(API_DIR_ADD)
            .headers(self.build_headers())
            .form(&[
                ("pid", parent_id),
                ("cname", name),
            ])
            .send()
            .await?
            .json()
            .await?;
        
        resp.base.check().map_err(|e| anyhow!("{}", e))?;
        Ok(resp)
    }
    
    pub async fn rename(&self, file_id: &str, new_name: &str) -> Result<()> {
        let key = format!("files_new_name[{}]", file_id);
        let resp: BasicResp = self.http
            .post(API_FILE_RENAME)
            .headers(self.build_headers())
            .form(&[(key.as_str(), new_name)])
            .send()
            .await?
            .json()
            .await?;
        
        resp.check().map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }
    
    pub async fn move_file(&self, file_id: &str, target_id: &str) -> Result<()> {
        let resp: BasicResp = self.http
            .post(API_FILE_MOVE)
            .headers(self.build_headers())
            .form(&[
                ("pid", target_id),
                ("fid[0]", file_id),
            ])
            .send()
            .await?
            .json()
            .await?;
        
        resp.check().map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }
    
    pub async fn copy_file(&self, file_id: &str, target_id: &str) -> Result<()> {
        let resp: BasicResp = self.http
            .post(API_FILE_COPY)
            .headers(self.build_headers())
            .form(&[
                ("pid", target_id),
                ("fid[0]", file_id),
            ])
            .send()
            .await?
            .json()
            .await?;
        
        resp.check().map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }
    
    pub async fn delete(&self, file_id: &str) -> Result<()> {
        let resp: BasicResp = self.http
            .post(API_FILE_DELETE)
            .headers(self.build_headers())
            .form(&[
                ("fid[0]", file_id),
                ("ignore_warn", "1"),
            ])
            .send()
            .await?
            .json()
            .await?;
        
        resp.check().map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }
    
    pub async fn get_oss_token(&self) -> Result<OssTokenResp> {
        let resp: OssTokenResp = self.http
            .get(API_OSS_TOKEN)
            .headers(self.build_headers())
            .send()
            .await?
            .json()
            .await?;
        
        if !resp.state {
            return Err(anyhow!("Get OSS token failed: errno={}", resp.errno));
        }
        
        Ok(resp)
    }
    
    pub async fn get_space_info(&self) -> Result<SpaceInfoData> {
        let resp: SpaceInfoResp = self.http
            .get(API_SPACE_INFO)
            .headers(self.build_headers())
            .send()
            .await?
            .json()
            .await?;
        
        resp.base.check().map_err(|e| anyhow!("{}", e))?;
        Ok(resp.data)
    }
    
    pub async fn upload_available(&self) -> Result<bool> {
        if self.upload_info.is_none() {
            return Ok(false);
        }
        Ok(true)
    }
    
    pub fn get_size_limit(&self) -> i64 {
        self.upload_info.as_ref().map(|i| i.size_limit).unwrap_or(0)
    }
    
    pub async fn rapid_upload(
        &self,
        file_size: i64,
        file_name: &str,
        dir_id: &str,
        pre_hash: &str,
        file_hash: &str,
        sign_key: &str,
        sign_val: &str,
    ) -> Result<UploadInitResp> {
        let target = format!("U_1_{}", dir_id);
        let file_size_str = file_size.to_string();
        let ts = Self::now_millis();
        let ts_str = ts.to_string();
        
        let token = generate_token(
            self.user_id,
            file_hash,
            pre_hash,
            &ts_str,
            &file_size_str,
            sign_key,
            sign_val,
            &self.app_ver,
        );
        
        let sig = generate_signature(self.user_id, file_hash, &target);
        
        let mut form = vec![
            ("appid", "0".to_string()),
            ("appversion", self.app_ver.clone()),
            ("userid", self.user_id.to_string()),
            ("filename", file_name.to_string()),
            ("filesize", file_size_str.clone()),
            ("fileid", file_hash.to_string()),
            ("target", target),
            ("sig", sig),
            ("t", ts_str),
            ("token", token),
        ];
        
        if !sign_key.is_empty() && !sign_val.is_empty() {
            form.push(("sign_key", sign_key.to_string()));
            form.push(("sign_val", sign_val.to_string()));
        }
        
        let resp: UploadInitResp = self.http
            .post(API_UPLOAD_INIT)
            .headers(self.build_headers())
            .form(&form)
            .send()
            .await?
            .json()
            .await?;
        
        Ok(resp)
    }
    
    pub fn get_http_client(&self) -> &Client {
        &self.http
    }
    
    pub fn get_cookie(&self) -> &str {
        &self.cookie
    }
}
