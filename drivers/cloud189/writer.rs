//! Cloud189 streaming upload Writer - non-blocking design / 天翼云盘流式上传Writer
//! 
//! Design principles / 设计原则：
//! - poll_write only caches data, returns quickly (doesn't block Core progress updates) / poll_write只缓存数据
//! - When buffer is full, spawn background task to upload chunk / 当buffer满时
//! - Memory retains at most 1 chunk (10MB) / 内存最多只保留1个分片
//! - Wait for all uploads to complete on shutdown / shutdown时等待

use anyhow::{anyhow, Result};
use reqwest::Client;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio::sync::RwLock;
use std::sync::{Mutex as StdMutex, mpsc};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use super::types::*;
use super::utils::*;
use super::upload::calculate_slice_md5;

/// Chunk size 10MiB (Cloud189 requirement) / 分片大小
const DEFAULT_SLICE_SIZE: usize = 10 * 1024 * 1024;

/// Upload state (shared) - uses std::sync::Mutex for sync context access / 上传状态
struct UploadState {
    /// Upload file ID / 上传文件ID
    upload_file_id: StdMutex<Option<String>>,
    /// Whether initialized / 是否已初始化
    initialized: AtomicBool,
    /// Current chunk number (atomic increment) / 当前分片号
    next_part_number: AtomicI32,
    /// Number of completed chunks / 已完成的分片数
    completed_parts: AtomicI32,
    /// Number of uploaded bytes / 已上传字节数
    uploaded_bytes: AtomicU64,
    /// MD5 list of uploaded chunks / 已上传分片的MD5列表
    uploaded_md5s: StdMutex<Vec<(i32, String)>>,
    /// Error information / 错误信息
    error: StdMutex<Option<String>>,
    /// Total chunk count (set on shutdown) / 总分片数
    total_parts: AtomicI32,
}

/// Cloud189 streaming upload Writer / 天翼云盘流式上传Writer
pub struct Cloud189StreamWriter {
    /// Current chunk buffer (max 10MB) / 当前分片缓冲区
    buffer: Vec<u8>,
    /// Chunk size / 分片大小
    slice_size: usize,
    /// Total file size / 文件总大小
    total_size: u64,
    /// 共享上传状态
    state: Arc<UploadState>,
    /// HTTP客户端
    client: Client,
    /// Token信息
    token_info: Arc<RwLock<Option<AppSessionResp>>>,
    /// 是否家庭云
    is_family: bool,
    /// 家庭云ID
    family_id: String,
    /// 父文件夹ID
    parent_folder_id: String,
    /// 文件名
    file_name: String,
    /// 是否已关闭
    closed: bool,
    /// 后台任务发送通道
    task_tx: Option<mpsc::Sender<UploadTask>>,
    /// 后台任务句柄
    task_handle: Option<std::thread::JoinHandle<()>>,
}

/// 上传任务
struct UploadTask {
    part_number: i32,
    data: Vec<u8>,
}

impl Cloud189StreamWriter {
    pub fn new(
        client: Client,
        token_info: Arc<RwLock<Option<AppSessionResp>>>,
        is_family: bool,
        family_id: String,
        parent_folder_id: String,
        file_name: String,
        file_size: Option<u64>,
    ) -> Self {
        let total_size = file_size.unwrap_or(0);
        let slice_size = if total_size > 0 {
            part_size(total_size as i64) as usize
        } else {
            DEFAULT_SLICE_SIZE
        };

        let state = Arc::new(UploadState {
            upload_file_id: StdMutex::new(None),
            initialized: AtomicBool::new(false),
            next_part_number: AtomicI32::new(1),
            completed_parts: AtomicI32::new(0),
            uploaded_bytes: AtomicU64::new(0),
            uploaded_md5s: StdMutex::new(Vec::new()),
            error: StdMutex::new(None),
            total_parts: AtomicI32::new(0),
        });

        // 创建后台上传任务通道（std::sync::mpsc是无界的）
        let (tx, rx) = mpsc::channel::<UploadTask>();

        // 启动后台上传线程
        let state_clone = state.clone();
        let client_clone = client.clone();
        let token_info_clone = token_info.clone();
        let is_family_clone = is_family;
        let family_id_clone = family_id.clone();

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                upload_worker(
                    rx,
                    state_clone,
                    client_clone,
                    token_info_clone,
                    is_family_clone,
                    family_id_clone,
                ).await;
            });
        });

        Self {
            buffer: Vec::with_capacity(slice_size),
            slice_size,
            total_size,
            state,
            client,
            token_info,
            is_family,
            family_id,
            parent_folder_id,
            file_name,
            closed: false,
            task_tx: Some(tx),
            task_handle: Some(handle),
        }
    }

    /// 初始化上传会话
    fn ensure_initialized(&self) -> std::io::Result<()> {
        if self.state.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }

        // 同步初始化（只执行一次）
        let file_size = if self.total_size > 0 { self.total_size as i64 } else { self.buffer.len() as i64 };
        
        let client = self.client.clone();
        let token_info = self.token_info.clone();
        let is_family = self.is_family;
        let family_id = self.family_id.clone();
        let parent_folder_id = self.parent_folder_id.clone();
        let file_name = self.file_name.clone();
        let slice_size = self.slice_size as i64;

        let upload_file_id = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                init_multi_upload(
                    &client, &token_info, is_family, &family_id,
                    &parent_folder_id, &file_name, file_size, slice_size
                ).await
            })
        }).join()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "初始化线程panic"))?
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // 保存upload_file_id
        *self.state.upload_file_id.lock().unwrap() = Some(upload_file_id);
        self.state.initialized.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// 发送分片到后台上传
    fn send_slice(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        self.ensure_initialized()?;

        let part_number = self.state.next_part_number.fetch_add(1, Ordering::SeqCst);
        let data = std::mem::take(&mut self.buffer);
        self.buffer = Vec::with_capacity(self.slice_size);

        if let Some(ref tx) = self.task_tx {
            // std::sync::mpsc使用send()
            tx.send(UploadTask { part_number, data })
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "发送上传任务失败"))?;
        }

        Ok(())
    }
}

/// 后台上传工作线程
async fn upload_worker(
    rx: mpsc::Receiver<UploadTask>,
    state: Arc<UploadState>,
    client: Client,
    token_info: Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    family_id: String,
) {
    // std::sync::mpsc::Receiver使用iter()
    while let Ok(task) = rx.recv() {
        // 获取upload_file_id
        let upload_file_id = {
            let guard = state.upload_file_id.lock().unwrap();
            guard.clone().unwrap_or_default()
        };

        // 上传分片
        let chunk_len = task.data.len() as u64;
        match upload_slice(&client, &token_info, is_family, &family_id, &upload_file_id, task.part_number, &task.data).await {
            Ok(md5) => {
                // 保存MD5
                {
                    let mut md5s = state.uploaded_md5s.lock().unwrap();
                    md5s.push((task.part_number, md5));
                }
                state.completed_parts.fetch_add(1, Ordering::SeqCst);
                state.uploaded_bytes.fetch_add(chunk_len, Ordering::SeqCst);
                
                tracing::debug!("cloud189: 已上传分片 {}, 已上传 {} 字节", 
                    task.part_number, state.uploaded_bytes.load(Ordering::SeqCst));
            }
            Err(e) => {
                let mut error = state.error.lock().unwrap();
                *error = Some(e.to_string());
                tracing::error!("cloud189: 上传分片 {} 失败: {}", task.part_number, e);
            }
        }
    }
}

async fn init_multi_upload(
    client: &Client,
    token_info: &Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    family_id: &str,
    parent_folder_id: &str,
    file_name: &str,
    file_size: i64,
    slice_size: i64,
) -> Result<String> {
    let mut full_url = UPLOAD_URL.to_string();
    let mut params: Vec<(&str, String)> = vec![
        ("parentFolderId", parent_folder_id.to_string()),
        ("fileName", urlencoding::encode(file_name).to_string()),
        ("fileSize", file_size.to_string()),
        ("sliceSize", slice_size.to_string()),
        ("lazyCheck", "1".to_string()),
    ];

    if is_family {
        params.push(("familyId", family_id.to_string()));
        full_url.push_str("/family");
    } else {
        full_url.push_str("/person");
    }
    full_url.push_str("/initMultiUpload");

    let token_guard = token_info.read().await;
    let token = token_guard.as_ref().ok_or_else(|| anyhow!("未登录"))?;

    let session_secret = if is_family {
        token.user_session.family_session_secret.clone()
    } else {
        token.user_session.session_secret.clone()
    };
    let session_key = if is_family {
        token.user_session.family_session_key.clone()
    } else {
        token.user_session.session_key.clone()
    };
    drop(token_guard);

    let params_ref: Vec<(&str, &str)> = params.iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    let encrypted_params = encrypt_params(&params_ref, &session_secret);

    let date = get_http_date_str();
    let signature = signature_of_hmac(&session_secret, &session_key, "GET", &full_url, &date, &encrypted_params);

    let mut query = client_suffix();
    if !encrypted_params.is_empty() {
        query.push(("params".to_string(), encrypted_params));
    }

    let resp = client
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
            return Err(anyhow!("初始化上传失败: {}", err.error_message()));
        }
    }

    let init_resp: InitMultiUploadResp = serde_json::from_str(&text)
        .map_err(|e| anyhow!("解析初始化上传响应失败: {} - {}", e, text))?;

    Ok(init_resp.data.upload_file_id)
}

async fn upload_slice(
    client: &Client,
    token_info: &Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    family_id: &str,
    upload_file_id: &str,
    part_number: i32,
    data: &[u8],
) -> Result<String> {
    let (hex_md5, base64_md5) = calculate_slice_md5(data);
    let upload_url = get_upload_url(client, token_info, is_family, family_id, upload_file_id, part_number, &base64_md5).await?;

    let mut req = client.put(&upload_url.request_url);
    for (k, v) in &upload_url.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    let resp = req.body(data.to_vec()).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("上传分片{}失败: {}", part_number, text));
    }

    Ok(hex_md5)
}

async fn get_upload_url(
    client: &Client,
    token_info: &Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    _family_id: &str,
    upload_file_id: &str,
    part_number: i32,
    base64_md5: &str,
) -> Result<UploadUrlInfo> {
    let mut full_url = UPLOAD_URL.to_string();
    if is_family {
        full_url.push_str("/family");
    } else {
        full_url.push_str("/person");
    }
    full_url.push_str("/getMultiUploadUrls");

    let part_info = format!("{}-{}", part_number, base64_md5);
    let params = vec![
        ("uploadFileId", upload_file_id),
        ("partInfo", &part_info as &str),
    ];

    let token_guard = token_info.read().await;
    let token = token_guard.as_ref().ok_or_else(|| anyhow!("未登录"))?;

    let session_secret = if is_family {
        token.user_session.family_session_secret.clone()
    } else {
        token.user_session.session_secret.clone()
    };
    let session_key = if is_family {
        token.user_session.family_session_key.clone()
    } else {
        token.user_session.session_key.clone()
    };
    drop(token_guard);

    let encrypted_params = encrypt_params(&params, &session_secret);
    let date = get_http_date_str();
    let signature = signature_of_hmac(&session_secret, &session_key, "GET", &full_url, &date, &encrypted_params);

    let mut query = client_suffix();
    if !encrypted_params.is_empty() {
        query.push(("params".to_string(), encrypted_params));
    }

    let resp = client
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
        .map_err(|e| anyhow!("解析上传URL响应失败: {} - {}", e, text))?;

    for (k, v) in urls_resp.upload_urls {
        let pn: i32 = k.trim_start_matches("partNumber_").parse().unwrap_or(1);
        if pn == part_number {
            return Ok(UploadUrlInfo {
                part_number: pn,
                headers: parse_http_header(&v.request_header),
                request_url: v.request_url,
            });
        }
    }

    Err(anyhow!("未找到分片{}的上传URL", part_number))
}

async fn commit_upload(
    client: &Client,
    token_info: &Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    _family_id: &str,
    upload_file_id: &str,
    md5s: &[(i32, String)],
) -> Result<()> {
    // 按分片号排序
    let mut sorted_md5s: Vec<_> = md5s.iter().collect();
    sorted_md5s.sort_by_key(|(pn, _)| *pn);
    let md5_list: Vec<String> = sorted_md5s.iter().map(|(_, md5)| md5.clone()).collect();

    let file_md5 = md5_list.first().cloned().unwrap_or_default();
    let slice_md5 = if md5_list.len() == 1 {
        file_md5.clone()
    } else {
        let joined = md5_list.join("\n");
        let digest = md5::compute(joined.as_bytes());
        format!("{:X}", digest)
    };

    let mut full_url = UPLOAD_URL.to_string();
    if is_family {
        full_url.push_str("/family");
    } else {
        full_url.push_str("/person");
    }
    full_url.push_str("/commitMultiUploadFile");

    let params = vec![
        ("uploadFileId", upload_file_id),
        ("fileMd5", &file_md5 as &str),
        ("sliceMd5", &slice_md5 as &str),
        ("lazyCheck", "1"),
        ("isLog", "0"),
        ("opertype", "3"),
    ];

    let token_guard = token_info.read().await;
    let token = token_guard.as_ref().ok_or_else(|| anyhow!("未登录"))?;

    let session_secret = if is_family {
        token.user_session.family_session_secret.clone()
    } else {
        token.user_session.session_secret.clone()
    };
    let session_key = if is_family {
        token.user_session.family_session_key.clone()
    } else {
        token.user_session.session_key.clone()
    };
    drop(token_guard);

    let encrypted_params = encrypt_params(&params, &session_secret);
    let date = get_http_date_str();
    let signature = signature_of_hmac(&session_secret, &session_key, "GET", &full_url, &date, &encrypted_params);

    let mut query = client_suffix();
    if !encrypted_params.is_empty() {
        query.push(("params".to_string(), encrypted_params));
    }

    let resp = client
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
            return Err(anyhow!("提交上传失败: {}", err.error_message()));
        }
    }

    Ok(())
}

/// 旧版上传创建响应
#[derive(Debug, serde::Deserialize)]
struct OldUploadCreateResp {
    #[serde(default, rename = "uploadFileId")]
    upload_file_id: i64,
    #[serde(default, rename = "fileCommitUrl")]
    file_commit_url: String,
    #[serde(default, rename = "fileDataExists")]
    file_data_exists: i32,
}

/// 创建空文件（使用旧版上传API，需要签名）
async fn create_empty_file(
    client: &Client,
    token_info: &Arc<RwLock<Option<AppSessionResp>>>,
    is_family: bool,
    family_id: &str,
    parent_folder_id: &str,
    file_name: &str,
) -> Result<()> {
    // 空文件的MD5是固定值
    let empty_md5 = "D41D8CD98F00B204E9800998ECF8427E";
    
    let token_guard = token_info.read().await;
    let token = token_guard.as_ref().ok_or_else(|| anyhow!("未登录"))?;
    
    let session_secret = if is_family {
        token.user_session.family_session_secret.clone()
    } else {
        token.user_session.session_secret.clone()
    };
    let session_key = if is_family {
        token.user_session.family_session_key.clone()
    } else {
        token.user_session.session_key.clone()
    };
    drop(token_guard);
    
    let date = get_http_date_str();
    
    // Step 1: 创建上传会话
    let (upload_file_id, file_commit_url) = if is_family {
        let full_url = format!("{}/family/file/createFamilyFile.action", API_URL);
        let signature = signature_of_hmac(&session_secret, &session_key, "POST", &full_url, &date, "");
        let query = vec![
            ("familyId", family_id),
            ("parentId", parent_folder_id),
            ("fileMd5", empty_md5),
            ("fileName", file_name),
            ("fileSize", "0"),
            ("resumePolicy", "1"),
        ];
        
        let resp = client
            .post(&full_url)
            .query(&client_suffix())
            .query(&query)
            .header("Date", &date)
            .header("SessionKey", &session_key)
            .header("Signature", &signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;
        
        let text = resp.text().await?;
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("创建空文件失败: {}", err.error_message()));
            }
        }
        
        let create_resp: OldUploadCreateResp = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析创建响应失败: {} - {}", e, text))?;
        
        (create_resp.upload_file_id, create_resp.file_commit_url)
    } else {
        let full_url = format!("{}/createUploadFile.action", API_URL);
        let signature = signature_of_hmac(&session_secret, &session_key, "POST", &full_url, &date, "");
        
        let form = vec![
            ("parentFolderId", parent_folder_id),
            ("fileName", file_name),
            ("size", "0"),
            ("md5", empty_md5),
            ("opertype", "3"),
            ("flag", "1"),
            ("resumePolicy", "1"),
            ("isLog", "0"),
        ];
        
        let resp = client
            .post(&full_url)
            .query(&client_suffix())
            .form(&form)
            .header("Date", &date)
            .header("SessionKey", &session_key)
            .header("Signature", &signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;
        
        let text = resp.text().await?;
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("创建空文件失败: {}", err.error_message()));
            }
        }
        
        let create_resp: OldUploadCreateResp = serde_json::from_str(&text)
            .map_err(|e| anyhow!("解析创建响应失败: {} - {}", e, text))?;
        
        (create_resp.upload_file_id, create_resp.file_commit_url)
    };
    
    // Step 2: 提交上传（空文件也需要提交）
    let date = get_http_date_str();
    let signature = signature_of_hmac(&session_secret, &session_key, "POST", &file_commit_url, &date, "");
    
    if is_family {
        let resp = client
            .post(&file_commit_url)
            .query(&client_suffix())
            .header("Date", &date)
            .header("SessionKey", &session_key)
            .header("Signature", &signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("ResumePolicy", "1")
            .header("UploadFileId", upload_file_id.to_string())
            .header("FamilyId", family_id)
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;
        
        let text = resp.text().await?;
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("提交空文件失败: {}", err.error_message()));
            }
        }
    } else {
        let upload_file_id_str = upload_file_id.to_string();
        let form = vec![
            ("opertype", "3"),
            ("resumePolicy", "1"),
            ("uploadFileId", upload_file_id_str.as_str()),
            ("isLog", "0"),
        ];
        
        let resp = client
            .post(&file_commit_url)
            .query(&client_suffix())
            .form(&form)
            .header("Date", &date)
            .header("SessionKey", &session_key)
            .header("Signature", &signature)
            .header("X-Request-ID", uuid::Uuid::new_v4().to_string())
            .header("Accept", "application/json;charset=UTF-8")
            .send()
            .await?;
        
        let text = resp.text().await?;
        if let Ok(err) = serde_json::from_str::<RespErr>(&text) {
            if err.has_error() {
                return Err(anyhow!("提交空文件失败: {}", err.error_message()));
            }
        }
    }
    
    Ok(())
}

impl AsyncWrite for Cloud189StreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // 检查错误
        if let Some(err) = self.state.error.lock().unwrap().clone() {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err)));
        }

        // 计算可以写入的字节数
        let space_left = self.slice_size - self.buffer.len();
        let to_write = buf.len().min(space_left);
        
        self.buffer.extend_from_slice(&buf[..to_write]);

        // 如果缓冲区满了，发送到后台上传
        if self.buffer.len() >= self.slice_size {
            if let Err(e) = self.send_slice() {
                return Poll::Ready(Err(e));
            }
        }

        Poll::Ready(Ok(to_write))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        self.closed = true;

        // 发送剩余数据
        if !self.buffer.is_empty() {
            if let Err(e) = self.send_slice() {
                return Poll::Ready(Err(e));
            }
        }

        // 关闭发送通道，让后台任务结束
        self.task_tx.take();

        // 等待后台任务完成
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.join();
        }

        // 检查错误
        if let Some(err) = self.state.error.lock().unwrap().clone() {
            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, err)));
        }

        // 检查是否有分片上传
        let completed = self.state.completed_parts.load(Ordering::SeqCst);
        
        // 空文件：需要初始化并提交，但不需要分片
        if completed == 0 {
            // 初始化空文件上传
            if !self.state.initialized.load(Ordering::SeqCst) {
                let client = self.client.clone();
                let token_info = self.token_info.clone();
                let is_family = self.is_family;
                let family_id = self.family_id.clone();
                let parent_folder_id = self.parent_folder_id.clone();
                let file_name = self.file_name.clone();
                
                // 空文件大小为0
                let result = std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        // 使用旧版上传API创建空文件
                        create_empty_file(&client, &token_info, is_family, &family_id, &parent_folder_id, &file_name).await
                    })
                }).join()
                    .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "创建空文件线程panic"))?
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                
                tracing::debug!("cloud189: 空文件创建成功");
                return Poll::Ready(Ok(result));
            }
            
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "没有数据可上传"
            )));
        }

        // 提交上传
        let upload_file_id = self.state.upload_file_id.lock().unwrap().clone().unwrap_or_default();
        let md5s = self.state.uploaded_md5s.lock().unwrap().clone();

        let client = self.client.clone();
        let token_info = self.token_info.clone();
        let is_family = self.is_family;
        let family_id = self.family_id.clone();

        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                commit_upload(&client, &token_info, is_family, &family_id, &upload_file_id, &md5s).await
            })
        }).join()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "提交上传线程panic"))?
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        Poll::Ready(Ok(result))
    }
}
