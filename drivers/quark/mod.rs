use crate::drivers::{Driver, FileInfo, DriverInfo};
use serde::{Deserialize, Serialize};
use reqwest::header::{HeaderMap, HeaderValue};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use md5::{Md5 as Md5Hasher};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose};
use async_trait::async_trait;
use anyhow::Result;
use std::any::Any;
use digest::Digest;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::io::AsyncSeekExt;
use std::io::SeekFrom;
use futures::StreamExt;
use reqwest::StatusCode;
use async_stream;

#[derive(Debug, Serialize, Deserialize)]
pub struct QuarkConfig {
    pub cookie: String,
    pub root_folder_id: String,
    pub order_by: String,
    pub order_direction: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkResponse<T> {
    status: i32,
    code: i32,
    message: String,
    data: Option<T>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkFile {
    fid: String,
    file_name: String,
    size: i64,
    l_updated_at: i64,
    file: bool,
    updated_at: i64,
    pdir_fid: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkListResponse {
    list: Vec<QuarkFile>,
    #[serde(default)]
    metadata: Option<QuarkMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkMetadata {
    #[serde(default)]
    total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkDownloadResponse {
    download_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkUploadPreResponse {
    task_id: String,
    finish: bool,
    upload_id: String,
    obj_key: String,
    upload_url: String,
    fid: String,
    bucket: String,
    callback: QuarkUploadCallback,
    format_type: String,
    size: i32,
    auth_info: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkUploadCallback {
    #[serde(rename = "callbackUrl")]
    callback_url: String,
    #[serde(rename = "callbackBody")]
    callback_body: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkUploadMetadata {
    part_thread: i32,
    acc2: String,
    acc1: String,
    part_size: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkHashResponse {
    #[serde(default)]
    hash: String,
    #[serde(default)]
    finish: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct QuarkUploadAuthResponse {
    #[serde(rename = "auth_key")]
    auth_token: String,
}

pub struct QuarkDriver {
    config: QuarkConfig,
    client: reqwest::Client,
    api_base: String,
    referer: String,
    ua: String,
    pr: String,
    path_fid_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl QuarkDriver {
    pub fn new(config: QuarkConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()?;

        Ok(Self {
            config,
            client,
            api_base: "https://drive.quark.cn/1/clouddrive".to_string(),
            referer: "https://pan.quark.cn".to_string(),
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) quark-cloud-drive/2.5.20 Chrome/100.0.4896.160 Electron/18.3.5.4-b478491100 Safari/537.36 Channel/pckk_other_ch".to_string(),
            pr: "ucpro".to_string(),
            path_fid_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn set_uc_config(&mut self) {
        self.api_base = "https://pc-api.uc.cn/1/clouddrive".to_string();
        self.referer = "https://drive.uc.cn".to_string();
        self.ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) uc-cloud-drive/2.5.20 Chrome/100.0.4896.160 Electron/18.3.5.4-b478491100 Safari/537.36 Channel/pckk_other_ch".to_string();
        self.pr = "UCBrowser".to_string();
    }

    async fn request_raw(&self, path: &str, method: reqwest::Method, params: Option<HashMap<String, String>>, json: Option<serde_json::Value>) -> Result<String> {
        let url = format!("{}{}", self.api_base, path);
        let mut headers = HeaderMap::new();
        headers.insert("Cookie", HeaderValue::from_str(&self.config.cookie)?);
        headers.insert("Accept", HeaderValue::from_static("application/json, text/plain, */*"));
        headers.insert("Referer", HeaderValue::from_str(&self.referer)?);
        headers.insert("User-Agent", HeaderValue::from_str(&self.ua)?);

        let mut req = self.client.request(method.clone(), &url)
            .headers(headers);

        let mut query = HashMap::new();
        query.insert("pr".to_string(), self.pr.clone());
        query.insert("fr".to_string(), "pc".to_string());

        if let Some(ref p) = params {
            query.extend(p.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        req = req.query(&query);

        if let Some(ref j) = json {
            req = req.json(j);
        }

        println!("🚀 发送请求: {} {}", method, url);
        println!("📝 查询参数: {:?}", query);
        if let Some(ref j) = json {
            println!("📦 请求体: {}", j);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        println!("📥 响应状态: {}", status);
        println!("📄 响应内容: {}", text);

        Ok(text)
    }

    async fn request<T: for<'de> Deserialize<'de>>(&self, path: &str, method: reqwest::Method, params: Option<HashMap<String, String>>, json: Option<serde_json::Value>) -> Result<T> {
        let text = self.request_raw(path, method, params, json).await?;
        
        let quark_resp: QuarkResponse<T> = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("解析响应失败: {} - 响应内容: {}", e, text))?;

        if quark_resp.code != 0 {
            return Err(anyhow::anyhow!("夸克API错误: {} (状态码: {})", quark_resp.message, quark_resp.code));
        }

        quark_resp.data.ok_or_else(|| anyhow::anyhow!("响应中没有数据"))
    }

    fn file_to_info(&self, file: QuarkFile, parent_path: &str) -> FileInfo {
        let path = if parent_path == "/" {
            format!("/{}", file.file_name)
        } else {
            format!("{}/{}", parent_path.trim_end_matches('/'), file.file_name)
        };

        FileInfo {
            name: file.file_name,
            path: path.clone(),  // 使用实际路径而不是 fid
            size: file.size as u64,
            is_dir: !file.file,
            modified: DateTime::from_timestamp_millis(file.updated_at)
                .unwrap_or_else(|| Utc::now())
                .to_rfc3339(),
        }
    }

    async fn get_fid_by_path(&self, path: &str) -> Result<String> {
        // 检查缓存
        if let Some(fid) = self.path_fid_cache.read().await.get(path) {
            return Ok(fid.clone());
        }

        if path == "/" {
            return Ok(self.config.root_folder_id.clone());
        }

        // 分割路径
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_fid = self.config.root_folder_id.clone();
        let mut current_path = String::new();

        // 逐级查找
        for part in parts {
            current_path.push('/');
            current_path.push_str(part);

            // 检查缓存
            if let Some(fid) = self.path_fid_cache.read().await.get(&current_path) {
                current_fid = fid.clone();
                continue;
            }

            // 查找当前目录下的文件
            let mut params = HashMap::new();
            params.insert("pdir_fid".to_string(), current_fid);
            params.insert("_size".to_string(), "100".to_string());
            params.insert("_page".to_string(), "1".to_string());
            params.insert("_fetch_total".to_string(), "1".to_string());
            params.insert("_sort".to_string(), "file_type:asc,file_name:asc".to_string());

            let resp: QuarkListResponse = self.request(
                "/file/sort",
                reqwest::Method::GET,
                Some(params),
                None,
            ).await?;

            // 查找匹配的文件
            if let Some(file) = resp.list.into_iter().find(|f| f.file_name == part) {
                current_fid = file.fid;
                // 更新缓存
                self.path_fid_cache.write().await.insert(current_path.clone(), current_fid.clone());
            } else {
                return Err(anyhow::anyhow!("找不到路径: {}", current_path));
            }
        }

        Ok(current_fid)
    }

    async fn clear_path_cache(&self) {
        self.path_fid_cache.write().await.clear();
    }
}

#[async_trait]
impl Driver for QuarkDriver {
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>> {
        let mut files = Vec::new();
        let mut page = 1;
        let size = 100;

        // 获取当前目录的 fid
        let current_fid = self.get_fid_by_path(path).await?;

        loop {
            let mut params = HashMap::new();
            params.insert("pdir_fid".to_string(), current_fid.clone());
            params.insert("_size".to_string(), size.to_string());
            params.insert("_page".to_string(), page.to_string());
            params.insert("_fetch_total".to_string(), "1".to_string());

            if self.config.order_by != "none" {
                params.insert("_sort".to_string(), 
                    format!("file_type:asc,{}:{}", 
                        self.config.order_by, 
                        self.config.order_direction));
            }

            let resp: QuarkListResponse = self.request(
                "/file/sort",
                reqwest::Method::GET,
                Some(params),
                None,
            ).await?;

            // 更新缓存
            for file in &resp.list {
                let file_path = if path == "/" {
                    format!("/{}", file.file_name)
                } else {
                    format!("{}/{}", path.trim_end_matches('/'), file.file_name)
                };
                self.path_fid_cache.write().await.insert(file_path, file.fid.clone());
            }

            files.extend(resp.list.into_iter().map(|f| self.file_to_info(f, path)));

            let total = resp.metadata.map(|m| m.total).unwrap_or(0);
            if total == 0 || page * size >= total {
                break;
            }
            page += 1;
        }

        Ok(files)
    }

    async fn download(&self, path: &str) -> Result<tokio::fs::File> {
        let url = self.get_download_url(path).await?;
        if let Some(url) = url {
            // 创建请求头
            let mut headers = HeaderMap::new();
            headers.insert("Cookie", HeaderValue::from_str(&self.config.cookie)?);
            headers.insert("Referer", HeaderValue::from_str(&self.referer)?);
            headers.insert("User-Agent", HeaderValue::from_str(&self.ua)?);
            headers.insert("Accept", HeaderValue::from_static("*/*"));
            headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9"));
            headers.insert("Connection", HeaderValue::from_static("keep-alive"));

            // 创建临时文件
            let mut temp_file = tokio::fs::File::from_std(tempfile::tempfile()?);
            
            // 下载文件
            let client = reqwest::Client::new();
            let resp = client.get(&url)
                .headers(headers)
                .send()
                .await?;

            if !resp.status().is_success() {
                return Err(anyhow::anyhow!("下载失败: HTTP {}", resp.status()));
            }

            // 写入临时文件
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(data) => {
                        tokio::io::copy(&mut std::io::Cursor::new(data), &mut temp_file).await?;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("下载失败: {}", e));
                    }
                }
            }

            // 将文件指针移动到开头
            temp_file.seek(std::io::SeekFrom::Start(0)).await?;
            Ok(temp_file)
        } else {
            Err(anyhow::anyhow!("获取下载链接失败"))
        }
    }

    async fn get_download_url(&self, path: &str) -> Result<Option<String>> {
        let fid = self.get_fid_by_path(path).await?;
        let json = serde_json::json!({
            "fids": [fid],
        });

        let resp: Vec<QuarkDownloadResponse> = self.request(
            "/file/download",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        if resp.is_empty() {
            return Err(anyhow::anyhow!("获取下载链接失败：响应为空"));
        }

        // 返回 None 以强制使用本地代理下载
        Ok(None)
    }

    async fn upload_file(&self, parent_path: &str, name: &str, content: &[u8]) -> Result<()> {
        // 获取父目录的 fid
        let parent_fid = self.get_fid_by_path(parent_path).await?;
        
        // 计算文件哈希
        let md5_str = format!("{:x}", Md5Hasher::digest(content));
        let sha1_str = format!("{:x}", Sha1::digest(content));
        
        println!("📊 文件哈希: MD5={}, SHA1={}", md5_str, sha1_str);
        
        let pre_json = serde_json::json!({
            "ccp_hash_update": true,
            "dir_name": "",
            "file_name": name,
            "format_type": mime_guess::from_path(name).first_or_octet_stream().to_string(),
            "l_created_at": Utc::now().timestamp_millis(),
            "l_updated_at": Utc::now().timestamp_millis(),
            "pdir_fid": parent_fid,
            "size": content.len(),
        });

        let resp_text = self.request_raw(
            "/file/upload/pre",
            reqwest::Method::POST,
            None,
            Some(pre_json),
        ).await?;

        let quark_resp: QuarkResponse<QuarkUploadPreResponse> = serde_json::from_str(&resp_text)
            .map_err(|e| anyhow::anyhow!("解析响应失败: {} - 响应内容: {}", e, resp_text))?;

        if quark_resp.code != 0 {
            return Err(anyhow::anyhow!("夸克API错误: {} (状态码: {})", quark_resp.message, quark_resp.code));
        }

        let pre = quark_resp.data.ok_or_else(|| anyhow::anyhow!("响应中没有数据"))?;
        
        // 从metadata中获取part_size
        let part_size = if let Some(metadata) = quark_resp.metadata {
            metadata.get("part_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(4194304) // 默认4MB
        } else {
            4194304 // 默认4MB
        };

        if pre.finish {
            println!("✅ 文件秒传成功");
            return Ok(());
        }

        // 更新哈希信息
        let hash_json = serde_json::json!({
            "md5": md5_str,
            "sha1": sha1_str,
            "task_id": pre.task_id,
        });

        println!("🔄 更新文件哈希信息");
        let hash_resp: QuarkHashResponse = self.request(
            "/file/update/hash",
            reqwest::Method::POST,
            None,
            Some(hash_json),
        ).await?;

        // 检查是否完成（可能是秒传）
        if hash_resp.finish || hash_resp.hash == "finish" {
            println!("✅ 哈希匹配，文件秒传成功");
            return Ok(());
        }

        // 分片上传
        let mut parts = Vec::new();
        let mut offset = 0;

        while offset < content.len() {
            let end = std::cmp::min(offset + part_size as usize, content.len());
            let part = &content[offset..end];
            
            let time_str = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
            let mime_type = mime_guess::from_path(name).first_or_octet_stream().to_string();

            let auth_json = serde_json::json!({
                "auth_info": pre.auth_info,
                "auth_meta": format!(
                    "PUT\n\n{}\n{}\nx-oss-date:{}\nx-oss-user-agent:aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit\n/{}/{}?partNumber={}&uploadId={}",
                    mime_type, time_str, time_str, pre.bucket, pre.obj_key, parts.len() + 1, pre.upload_id
                ),
                "task_id": pre.task_id,
            });

            let auth: QuarkUploadAuthResponse = self.request(
                "/file/upload/auth",
                reqwest::Method::POST,
                None,
                Some(auth_json),
            ).await?;

            let auth_key = auth.auth_token;
            let upload_url = format!("https://{}.{}/{}", pre.bucket, pre.upload_url.strip_prefix("http://").unwrap_or(&pre.upload_url), pre.obj_key);
            
            let mut headers = HeaderMap::new();
            headers.insert("Authorization", HeaderValue::from_str(&auth_key)?);
            headers.insert("Content-Type", HeaderValue::from_str(&mime_type)?);
            headers.insert("Referer", HeaderValue::from_static("https://pan.quark.cn/"));
            headers.insert("x-oss-date", HeaderValue::from_str(&time_str)?);
            headers.insert("x-oss-user-agent", HeaderValue::from_static("aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit"));

            let resp = self.client.put(&upload_url)
                .headers(headers)
                .query(&[
                    ("partNumber", (parts.len() + 1).to_string()),
                    ("uploadId", pre.upload_id.clone()),
                ])
                .body(part.to_vec())
                .send()
                .await?;

            if !resp.status().is_success() {
                return Err(anyhow::anyhow!("Upload part failed: {}", resp.status()));
            }

            let etag = resp.headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow::anyhow!("No ETag in response"))?;

            parts.push(etag.to_string());
            offset = end;
        }

        // 提交上传
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CompleteMultipartUpload>\n");
        for (i, etag) in parts.iter().enumerate() {
            xml.push_str(&format!("<Part>\n<PartNumber>{}</PartNumber>\n<ETag>{}</ETag>\n</Part>\n", i + 1, etag));
        }
        xml.push_str("</CompleteMultipartUpload>");

        let content_md5 = {
            use md5::{Md5, Digest};
            let mut hasher = Md5::new();
            hasher.update(xml.as_bytes());
            general_purpose::STANDARD.encode(hasher.finalize())
        };

        let callback_json = serde_json::to_string(&pre.callback)?;
        let callback_base64 = general_purpose::STANDARD.encode(callback_json);

        let time_str = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let commit_json = serde_json::json!({
            "auth_info": pre.auth_info,
            "auth_meta": format!(
                "POST\n{}\napplication/xml\n{}\nx-oss-callback:{}\nx-oss-date:{}\nx-oss-user-agent:aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit\n/{}/{}?uploadId={}",
                content_md5,
                time_str,
                callback_base64,
                time_str,
                pre.bucket,
                pre.obj_key,
                pre.upload_id
            ),
            "task_id": pre.task_id,
        });

        let auth: QuarkUploadAuthResponse = self.request(
            "/file/upload/auth",
            reqwest::Method::POST,
            None,
            Some(commit_json),
        ).await?;

        let upload_url = format!("https://{}.{}/{}", pre.bucket, pre.upload_url.strip_prefix("http://").unwrap_or(&pre.upload_url), pre.obj_key);
        
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_str(&auth.auth_token)?);
        headers.insert("Content-MD5", HeaderValue::from_str(&content_md5)?);
        headers.insert("Content-Type", HeaderValue::from_static("application/xml"));
        headers.insert("Referer", HeaderValue::from_static("https://pan.quark.cn/"));
        headers.insert("x-oss-callback", HeaderValue::from_str(&callback_base64)?);
        headers.insert("x-oss-date", HeaderValue::from_str(&time_str)?);
        headers.insert("x-oss-user-agent", HeaderValue::from_static("aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit"));

        let resp = self.client.post(&upload_url)
            .headers(headers)
            .query(&[("uploadId", pre.upload_id.clone())])
            .body(xml)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Upload commit failed: {}", resp.status()));
        }

        // 完成上传
        let finish_json = serde_json::json!({
            "obj_key": pre.obj_key,
            "task_id": pre.task_id,
        });

        self.request::<serde_json::Value>(
            "/file/upload/finish",
            reqwest::Method::POST,
            None,
            Some(finish_json),
        ).await?;

        println!("✅ 文件上传完成");
        Ok(())
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let fid = self.get_fid_by_path(path).await?;
        
        let json = serde_json::json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": [fid],
        });

        match self.request::<serde_json::Value>(
            "/file/delete",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await {
            Ok(_) => {
                // 从缓存中删除
                self.path_fid_cache.write().await.remove(path);
                Ok(())
            },
            Err(e) => {
                // 如果错误信息包含"文件已经删除"，认为是成功的
                if e.to_string().contains("文件已经删除") {
                    self.path_fid_cache.write().await.remove(path);
        Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn rename(&self, path: &str, new_name: &str) -> Result<()> {
        let fid = self.get_fid_by_path(path).await?;
        
        let json = serde_json::json!({
            "fid": fid,
            "file_name": new_name,
        });

        match self.request::<serde_json::Value>(
            "/file/rename",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await {
            Ok(_) => {
                // 更新缓存
                if let Some(parent) = std::path::Path::new(path).parent() {
                    let parent_path = parent.to_string_lossy();
                    let new_path = if parent_path.is_empty() {
                        format!("/{}", new_name)
                    } else {
                        format!("{}/{}", parent_path, new_name)
                    };
                    let mut cache = self.path_fid_cache.write().await;
                    if let Some(fid) = cache.remove(path) {
                        cache.insert(new_path, fid);
                    }
                }
                Ok(())
            },
            Err(e) => {
                // 如果错误信息表明文件已被重命名，认为是成功的
                if e.to_string().contains("文件名已存在") || e.to_string().contains("文件已经重命名") {
                    // 更新缓存
                    if let Some(parent) = std::path::Path::new(path).parent() {
                        let parent_path = parent.to_string_lossy();
                        let new_path = if parent_path.is_empty() {
                            format!("/{}", new_name)
                        } else {
                            format!("{}/{}", parent_path, new_name)
                        };
                        let mut cache = self.path_fid_cache.write().await;
                        if let Some(fid) = cache.remove(path) {
                            cache.insert(new_path, fid);
                        }
                    }
        Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn create_folder(&self, parent_path: &str, name: &str) -> Result<()> {
        // 获取父目录的 fid
        let parent_fid = self.get_fid_by_path(parent_path).await?;
        let json = serde_json::json!({
            "dir_init_lock": false,
            "dir_path": "",
            "file_name": name,
            "pdir_fid": parent_fid,
        });

        self.request::<serde_json::Value>(
            "/file",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        Ok(())
    }

    async fn get_file_info(&self, path: &str) -> Result<FileInfo> {
        // 获取父目录路径
        let parent_path = if let Some(idx) = path.rfind('/') {
            &path[..idx]
        } else {
            "/"
        };
        
        // 获取文件名
        let file_name = path.split('/').last().unwrap_or("");
        
        // 获取父目录的 fid
        let parent_fid = self.get_fid_by_path(parent_path).await?;
        
        let mut params = HashMap::new();
        params.insert("pdir_fid".to_string(), parent_fid);
        params.insert("_size".to_string(), "100".to_string());
        params.insert("_page".to_string(), "1".to_string());
        params.insert("_fetch_total".to_string(), "1".to_string());
        params.insert("_sort".to_string(), "file_type:asc,file_name:asc".to_string());

        let resp: QuarkListResponse = self.request(
            "/file/sort",
            reqwest::Method::GET,
            Some(params),
            None,
        ).await?;

        if let Some(file) = resp.list.into_iter().find(|f| f.file_name == file_name) {
            Ok(self.file_to_info(file, parent_path))
        } else {
            Err(anyhow::anyhow!("找不到文件: {}", path))
        }
    }

    async fn move_file(&self, file_path: &str, new_parent_path: &str) -> Result<()> {
        // 获取文件的 fid
        let file_fid = self.get_fid_by_path(file_path).await?;
        // 获取新父目录的 fid
        let new_parent_fid = self.get_fid_by_path(new_parent_path).await?;
        let json = serde_json::json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": [file_fid],
            "to_pdir_fid": new_parent_fid,
        });

        self.request::<serde_json::Value>(
            "/file/move",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        // 清除缓存，因为文件位置已改变
        self.clear_path_cache().await;
        Ok(())
    }

    async fn copy_file(&self, file_path: &str, new_parent_path: &str) -> Result<()> {
        // 获取文件的 fid
        let file_fid = self.get_fid_by_path(file_path).await?;
        // 获取新父目录的 fid
        let new_parent_fid = self.get_fid_by_path(new_parent_path).await?;
        let json = serde_json::json!({
            "action_type": 1,
            "exclude_fids": [],
            "filelist": [file_fid],
            "to_pdir_fid": new_parent_fid,
        });

        self.request::<serde_json::Value>(
            "/file/copy",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        // 清除缓存，因为有新文件产生
        self.clear_path_cache().await;
        Ok(())
    }

    async fn stream_download(&self, path: &str) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String)>> {
        let fid = self.get_fid_by_path(path).await?;
        let json = serde_json::json!({
            "fids": [fid],
        });

        let resp: Vec<QuarkDownloadResponse> = self.request(
            "/file/download",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        if resp.is_empty() {
            return Err(anyhow::anyhow!("获取下载链接失败：响应为空"));
        }

        let url = resp[0].download_url.clone();
        let filename = path.split('/').last().unwrap_or("download").to_string();
        
        // 创建请求头
        let mut headers = HeaderMap::new();
        headers.insert("Cookie", HeaderValue::from_str(&self.config.cookie)?);
        headers.insert("Referer", HeaderValue::from_str(&self.referer)?);
        headers.insert("User-Agent", HeaderValue::from_str(&self.ua)?);
        headers.insert("Accept", HeaderValue::from_static("*/*"));
        headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9"));
        headers.insert("Connection", HeaderValue::from_static("keep-alive"));

        // 创建异步流
        let client = reqwest::Client::new();
        let stream = async_stream::stream! {
            let resp = match client.get(&url)
                .headers(headers)
                .send()
                .await {
                Ok(resp) => resp,
                Err(e) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    return;
                }
            };

            if !resp.status().is_success() {
                yield Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("下载失败: HTTP {}", resp.status())
                ));
                return;
            }

            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(data) => {
                        yield Ok(axum::body::Bytes::from(data));
                    }
                    Err(e) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        break;
                    }
                }
            }
        };

        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));

        Ok(Some((boxed_stream, filename)))
    }

    async fn stream_download_with_range(&self, path: &str, start: Option<u64>, end: Option<u64>) -> Result<Option<(Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin>, String, u64, Option<u64>)>> {
        let fid = self.get_fid_by_path(path).await?;
        let json = serde_json::json!({
            "fids": [fid],
        });

        let resp: Vec<QuarkDownloadResponse> = self.request(
            "/file/download",
            reqwest::Method::POST,
            None,
            Some(json),
        ).await?;

        if resp.is_empty() {
            return Err(anyhow::anyhow!("获取下载链接失败：响应为空"));
        }

        let url = resp[0].download_url.clone();
        let filename = path.split('/').last().unwrap_or("download").to_string();
        
        // 创建请求头
        let mut headers = HeaderMap::new();
        headers.insert("Cookie", HeaderValue::from_str(&self.config.cookie)?);
        headers.insert("Referer", HeaderValue::from_str(&self.referer)?);
        headers.insert("User-Agent", HeaderValue::from_str(&self.ua)?);
        headers.insert("Accept", HeaderValue::from_static("*/*"));
        headers.insert("Accept-Language", HeaderValue::from_static("zh-CN,zh;q=0.9"));
        headers.insert("Connection", HeaderValue::from_static("keep-alive"));

        // 添加 Range 头
        if let Some(start) = start {
            let range = if let Some(end) = end {
                format!("bytes={}-{}", start, end)
            } else {
                format!("bytes={}-", start)
            };
            headers.insert("Range", HeaderValue::from_str(&range)?);
        }

        // 创建异步流
        let client = reqwest::Client::new();
        let stream = async_stream::stream! {
            let resp = match client.get(&url)
                .headers(headers)
                .send()
                .await {
                Ok(resp) => resp,
                Err(e) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                    return;
                }
            };

            if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                yield Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("下载失败: HTTP {}", resp.status())
                ));
                return;
            }

            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(data) => {
                        yield Ok(axum::body::Bytes::from(data));
                    }
                    Err(e) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        break;
                    }
                }
            }
        };

        let boxed_stream: Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + Unpin> = 
            Box::new(Box::pin(stream));

        // 获取文件大小
        let file_info = self.get_file_info(path).await?;
        let file_size = file_info.size;
        let content_length = if let (Some(start), Some(end)) = (start, end) {
            Some(end - start + 1)
        } else if let Some(start) = start {
            Some(file_size - start)
        } else {
            Some(file_size)
        };

        Ok(Some((boxed_stream, filename, file_size, content_length)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct QuarkDriverFactory;

impl crate::drivers::DriverFactory for QuarkDriverFactory {
    fn driver_type(&self) -> &'static str {
        "quark"
    }

    fn driver_info(&self) -> DriverInfo {
        DriverInfo {
            driver_type: "quark".to_string(),
            display_name: "夸克网盘".to_string(),
            description: "夸克网盘驱动".to_string(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cookie": {
                        "type": "string",
                        "title": "Cookie",
                        "description": "夸克网盘Cookie"
                    },
                    "root_folder_id": {
                        "type": "string",
                        "title": "根目录ID",
                        "description": "根目录ID",
                        "default": "0"
                    },
                    "order_by": {
                        "type": "string",
                        "title": "排序字段",
                        "description": "排序字段",
                        "default": "file_name"
                    },
                    "order_direction": {
                        "type": "string",
                        "title": "排序方向",
                        "description": "排序方向",
                        "default": "asc"
                    }
                },
                "required": ["cookie"]
            }),
        }
    }

    fn create_driver(&self, config: serde_json::Value) -> Result<Box<dyn Driver>> {
        let quark_config: QuarkConfig = serde_json::from_value(config)?;
        let driver = QuarkDriver::new(quark_config)?;
        Ok(Box::new(driver))
    }

    fn get_routes(&self) -> Option<axum::Router> {
        None
    }
} 