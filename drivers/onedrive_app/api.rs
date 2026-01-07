//! OneDrive App API client / OneDrive App API客户端
//! 
//! Handles OAuth token management and API requests / 处理OAuth令牌管理和API请求

use anyhow::{anyhow, Result};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::*;

/// OneDrive App API client / OneDrive App API客户端
pub struct OneDriveAppApi {
    config: OneDriveAppConfig,
    client: Client,
    access_token: Arc<RwLock<Option<String>>>,
}

impl OneDriveAppApi {
    pub fn new(config: OneDriveAppConfig) -> Result<Self> {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10)); // 支持最多10次重定向（包括302）
        
        // 配置代理
        if let Some(ref proxy_url) = config.proxy {
            let proxy = reqwest::Proxy::http(proxy_url)
                .or_else(|_| reqwest::Proxy::https(proxy_url))
                .or_else(|_| reqwest::Proxy::all(proxy_url))
                .map_err(|e| anyhow!("无效的代理URL: {} - {}", proxy_url, e))?;
            builder = builder.proxy(proxy);
        }
        
        let client = builder.build()
            .map_err(|e| anyhow!("创建HTTP客户端失败: {}", e))?;
        
        Ok(Self {
            config,
            client,
            access_token: Arc::new(RwLock::new(None)),
        })
    }

    /// Get access token (with caching) / 获取访问令牌（带缓存）
    pub async fn get_access_token(&self) -> Result<String> {
        // 先检查缓存的token
        {
            let token = self.access_token.read().await;
            if let Some(ref t) = *token {
                return Ok(t.clone());
            }
        }
        
        // 获取新token
        self.refresh_access_token().await
    }

    /// Refresh access token using client_credentials / 使用client_credentials刷新访问令牌
    pub async fn refresh_access_token(&self) -> Result<String> {
        let host = get_host_config(&self.config.region);
        let url = format!("{}/{}/oauth2/token", host.oauth, self.config.tenant_id);

        let mut params = std::collections::HashMap::new();
        params.insert("grant_type", "client_credentials");
        params.insert("client_id", &self.config.client_id);
        params.insert("client_secret", &self.config.client_secret);
        let resource = format!("{}/", host.api);
        let scope = format!("{}/.default", host.api);
        params.insert("resource", &resource);
        params.insert("scope", &scope);

        let response = self.client
            .post(&url)
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let token_resp: TokenResponse = response.json().await?;
            
            // 更新access_token
            {
                let mut access_token = self.access_token.write().await;
                *access_token = Some(token_resp.access_token.clone());
            }
            
            Ok(token_resp.access_token)
        } else {
            let error: TokenError = response.json().await
                .unwrap_or_else(|_| TokenError {
                    error: "unknown".to_string(),
                    error_description: "Failed to parse error response".to_string(),
                });
            Err(anyhow!("Token获取失败: {}", error.error_description))
        }
    }

    /// Get API URL for a path / 获取路径的API URL
    pub fn get_meta_url(&self, path: &str) -> String {
        let host = get_host_config(&self.config.region);
        let clean_path = path.trim_start_matches('/').trim_end_matches('/');
        
        // URL编码路径（不编码斜杠）
        let encoded_path = if clean_path.is_empty() {
            String::new()
        } else {
            clean_path.split('/')
                .map(|segment| urlencoding::encode(segment).to_string())
                .collect::<Vec<_>>()
                .join("/")
        };

        if encoded_path.is_empty() {
            format!("{}/v1.0/users/{}/drive/root", host.api, self.config.email)
        } else {
            format!("{}/v1.0/users/{}/drive/root:/{}:", host.api, self.config.email, encoded_path)
        }
    }

    /// Make API request with automatic token refresh / 发起API请求（自动刷新token）
    pub async fn request<T>(&self, url: &str, method: reqwest::Method) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let token = self.get_access_token().await?;
        
        let response = self.client
            .request(method.clone(), url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else if response.status() == 401 {
            // Token过期，刷新后重试
            let new_token = self.refresh_access_token().await?;
            let response = self.client
                .request(method, url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send()
                .await?;

            if response.status().is_success() {
                Ok(response.json().await?)
            } else {
                let error: ApiError = response.json().await?;
                Err(anyhow!("API错误: {}", error.error.message))
            }
        } else {
            let error: ApiError = response.json().await
                .unwrap_or_else(|_| ApiError {
                    error: ApiErrorDetail {
                        code: "unknown".to_string(),
                        message: "Failed to parse error response".to_string(),
                    },
                });
            Err(anyhow!("API错误: {}", error.error.message))
        }
    }

    /// Get file list / 获取文件列表
    pub async fn get_files(&self, path: &str) -> Result<Vec<OneDriveFile>> {
        let mut all_files = Vec::new();
        let base_url = format!("{}/children", self.get_meta_url(path));
        let mut next_link = Some(format!(
            "{}?$top=1000&$expand=thumbnails($select=medium)&$select=id,name,size,lastModifiedDateTime,@microsoft.graph.downloadUrl,file,parentReference",
            base_url
        ));

        while let Some(url) = next_link {
            let response: FilesResponse = self.request(&url, reqwest::Method::GET).await?;
            all_files.extend(response.value);
            next_link = response.next_link;
        }

        Ok(all_files)
    }

    /// Get single file info / 获取单个文件信息
    pub async fn get_file(&self, path: &str) -> Result<OneDriveFile> {
        let url = self.get_meta_url(path);
        self.request(&url, reqwest::Method::GET).await
    }

    /// Get drive info (with quota) / 获取Drive信息（包含配额）
    pub async fn get_drive(&self) -> Result<DriveResponse> {
        let host = get_host_config(&self.config.region);
        let url = format!("{}/v1.0/users/{}/drive", host.api, self.config.email);
        
        self.request(&url, reqwest::Method::GET).await
    }

    /// Create upload session / 创建上传会话
    pub async fn create_upload_session(&self, path: &str) -> Result<String> {
        let url = format!("{}/createUploadSession", self.get_meta_url(path));
        let token = self.get_access_token().await?;
        
        let body = serde_json::json!({
            "item": {
                "@microsoft.graph.conflictBehavior": "replace"
            }
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("创建上传会话失败: {}", text));
        }

        let session: UploadSessionResponse = response.json().await?;
        Ok(session.upload_url)
    }

    /// Upload chunk / 上传分片
    pub async fn upload_chunk(
        &self,
        upload_url: &str,
        chunk: Vec<u8>,
        start: u64,
        end: u64,
        total: u64,
        is_last: bool,
    ) -> Result<()> {
        // 当 total == 0 时（文件大小未知），使用 end 作为 total
        // 这样 Content-Range 格式正确，但判断最后一个分片时完全依赖 is_last 参数
        let total_for_range = if total == 0 { end } else { total };
        
        // 处理空文件的情况：当 start == 0 且 end == 0 时，Content-Range 应该是 "bytes 0-0/0"
        let content_range = if start == 0 && end == 0 {
            "bytes 0-0/0".to_string()
        } else {
            format!("bytes {}-{}/{}", start, end - 1, total_for_range)
        };
        
        let response = self.client
            .put(upload_url)
            .header("Content-Range", &content_range)
            .header("Content-Length", chunk.len().to_string())
            .body(chunk)
            .send()
            .await?;

        let status = response.status();
        let status_code = status.as_u16();
        
        // 检查是否是最后一个分片
        // 如果 total > 0，通过 end == total 判断；否则通过 is_last 参数判断
        let is_last_chunk = if total > 0 {
            end == total
        } else {
            is_last
        };
        
        if is_last_chunk {
            // 最后一个分片：只接受 200 或 201（表示上传完成）
            // 202 表示分片已接收但上传未完成，不应该出现在最后一个分片
            if status_code == 200 || status_code == 201 {
                // 尝试解析响应体，验证文件信息是否存在
                // 根据 OneDrive API 文档，最后一个分片上传成功后，响应体应该包含文件信息
                let response_text = response.text().await.unwrap_or_default();
                
                // 尝试解析为 JSON，验证文件信息
                if let Ok(file_info) = serde_json::from_str::<super::types::OneDriveFile>(&response_text) {
                    tracing::debug!(
                        "最后一个分片上传成功，上传会话已完成: status={}, range={}-{}/{}, file_id={}, file_name={}", 
                        status_code, start, end - 1, total, file_info.id, file_info.name
                    );
                } else {
                    // 如果解析失败，记录警告，但不视为错误
                    // 因为某些情况下响应体可能为空或格式不同
                    tracing::warn!(
                        "最后一个分片上传成功，但无法解析响应体: status={}, range={}-{}/{}, response_len={}", 
                        status_code, start, end - 1, total, response_text.len()
                    );
                }
                
                Ok(())
            } else {
                let text = response.text().await.unwrap_or_default();
                Err(anyhow!("最后一个分片上传失败: HTTP {} - {} (期望 200 或 201)", status_code, text))
            }
        } else {
            // 中间分片：接受 200, 201, 202
            if status.is_success() || status_code == 202 {
                Ok(())
            } else {
                let text = response.text().await.unwrap_or_default();
                Err(anyhow!("分片上传失败: HTTP {} - {}", status_code, text))
            }
        }
    }

    /// Upload small file directly (≤4MB) / 小文件直接上传
    pub async fn upload_small_file(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let url = format!("{}/content", self.get_meta_url(path));
        let token = self.get_access_token().await?;
        
        let response = self.client
            .put(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", data.len().to_string())
            .body(data)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(anyhow!("上传失败: HTTP {} - {}", status, text))
        }
    }

    /// Delete file or directory / 删除文件或目录
    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.get_meta_url(path);
        let token = self.get_access_token().await?;
        
        let response = self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() || response.status() == 204 {
            Ok(())
        } else {
            Err(anyhow!("删除失败"))
        }
    }

    /// Create directory / 创建目录
    pub async fn create_dir(&self, path: &str) -> Result<()> {
        let parent_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        
        let folder_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的路径"))?;

        let url = format!("{}/children", self.get_meta_url(&parent_path));
        let token = self.get_access_token().await?;
        
        let body = serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "rename"
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("创建目录失败"))
        }
    }

    /// Rename file or directory / 重命名文件或目录
    pub async fn rename(&self, old_path: &str, new_name: &str) -> Result<()> {
        let url = self.get_meta_url(old_path);
        let token = self.get_access_token().await?;
        
        let body = serde_json::json!({ "name": new_name });

        let response = self.client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("重命名失败"))
        }
    }

    /// Move file or directory / 移动文件或目录
    pub async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let url = self.get_meta_url(old_path);
        let token = self.get_access_token().await?;
        
        let new_parent = std::path::Path::new(new_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        
        let new_name = std::path::Path::new(new_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow!("无效的目标路径"))?;

        let parent_path = if new_parent == "/" {
            "/drive/root".to_string()
        } else {
            format!("/drive/root:/{}", new_parent.trim_start_matches('/'))
        };

        let body = serde_json::json!({
            "parentReference": { "path": parent_path },
            "name": new_name
        });

        let response = self.client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("移动失败"))
        }
    }

    /// Copy file or directory / 复制文件或目录
    pub async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()> {
        let dst_file = self.get_file(new_path).await.ok();
        let dst_id = if let Some(ref f) = dst_file {
            f.id.clone()
        } else {
            return Err(anyhow!("目标目录不存在"));
        };

        let src_file = self.get_file(old_path).await?;
        let src_name = src_file.name;

        let body = serde_json::json!({
            "parentReference": {
                "driveId": dst_file.as_ref().and_then(|f| f.parent_reference.as_ref())
                    .and_then(|p| p._drive_id.clone()),
                "id": dst_id
            },
            "name": src_name
        });

        let url = format!("{}/copy", self.get_meta_url(old_path));
        let token = self.get_access_token().await?;
        
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() || response.status().as_u16() == 202 {
            Ok(())
        } else {
            Err(anyhow!("复制失败"))
        }
    }

    pub fn get_client(&self) -> &Client {
        &self.client
    }

    pub fn get_config(&self) -> &OneDriveAppConfig {
        &self.config
    }
}

