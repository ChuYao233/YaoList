# YaoList 存储驱动开发指南

本文档为开发者提供创建新存储驱动的完整指南与规范。

## 目录

- [架构原则](#架构原则)
- [StorageDriver Trait](#storagedriver-trait)
- [创建新驱动](#创建新驱动)
- [必须实现的方法](#必须实现的方法)
- [可选实现的方法](#可选实现的方法)
- [进度回调规范](#进度回调规范)
- [错误处理](#错误处理)
- [最佳实践](#最佳实践)
- [示例代码](#示例代码)

---

## 架构原则

### Core → Driver 单向调用

```
┌─────────────────────────────────────────────────────────┐
│                        Core 层                          │
│  - 任务生命周期管理                                      │
│  - 进度跟踪与更新                                        │
│  - 并发控制                                              │
│  - 错误处理与重试                                        │
│  - 暂停/取消逻辑                                         │
└─────────────────────────────────────────────────────────┘
                          │
                          │ 调用
                          ▼
┌─────────────────────────────────────────────────────────┐
│                      Driver 层                          │
│  - 只提供文件原语操作                                    │
│  - list / open_reader / open_writer / put / delete      │
│  - 不主动调用 Core                                       │
│  - 回调只更新简单状态                                    │
└─────────────────────────────────────────────────────────┘
```

**关键原则：**
1. **Driver 不调用 Core** - Driver 只提供文件操作原语，不主动调用 Core 的任何方法
2. **回调保持简单** - 进度回调只更新共享原子状态，不执行复杂异步操作
3. **Core 控制流程** - 上传/下载流程由 Core 控制，Driver 只执行具体操作

---

## StorageDriver Trait

```rust
#[async_trait]
pub trait StorageDriver: Send + Sync {
    /// 驱动名称
    fn name(&self) -> &str;
    
    /// 驱动能力声明
    fn capabilities(&self) -> Capability;
    
    /// 列出目录内容
    async fn list(&self, path: &str) -> Result<Vec<Entry>>;
    
    /// 打开文件读取器（支持范围读取）
    async fn open_reader(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>>;
    
    /// 打开文件写入器（流式写入）
    async fn open_writer(
        &self,
        path: &str,
        size_hint: Option<u64>,
        progress: Option<ProgressCallback>,
    ) -> Result<Box<dyn AsyncWrite + Unpin + Send>>;
    
    /// 上传完整文件（云盘驱动应重写此方法）
    async fn put(
        &self,
        path: &str,
        data: bytes::Bytes,
        progress: Option<ProgressCallback>,
    ) -> Result<()>;
    
    /// 删除文件或目录
    async fn delete(&self, path: &str) -> Result<()>;
    
    /// 创建目录
    async fn create_dir(&self, path: &str) -> Result<()>;
    
    /// 重命名
    async fn rename(&self, old_path: &str, new_name: &str) -> Result<()>;
    
    /// 移动
    async fn move_item(&self, old_path: &str, new_path: &str) -> Result<()>;
    
    /// 复制（有默认实现）
    async fn copy_item(&self, old_path: &str, new_path: &str) -> Result<()>;
    
    /// 获取直链（可选）
    async fn get_direct_link(&self, path: &str) -> Result<Option<String>>;
    
    /// 获取存储空间信息（可选）
    async fn get_space_info(&self) -> Result<Option<SpaceInfo>>;
}
```

---

## 创建新驱动

### 1. 创建驱动目录

```
backend/drivers/
└── my_driver/
    ├── mod.rs        # 模块入口
    ├── driver.rs     # 驱动实现
    ├── types.rs      # 类型定义
    └── api.rs        # API 客户端（如需要）
```

### 2. 在 mod.rs 中导出

```rust
mod driver;
mod types;

pub use driver::MyDriver;
```

### 3. 在 drivers/mod.rs 中注册

```rust
pub mod my_driver;
pub use my_driver::MyDriver;
```

### 4. 在 DriverFactory 中注册

```rust
// storage/manager.rs
match driver_type {
    "my_driver" => Box::new(MyDriver::new(config)?),
    // ...
}
```

---

## 必须实现的方法

### name()

返回驱动的唯一标识名称。

```rust
fn name(&self) -> &str {
    "my_driver"
}
```

### capabilities()

声明驱动支持的能力。

```rust
fn capabilities(&self) -> Capability {
    Capability {
        read: true,
        write: true,
        delete: true,
        rename: true,
        move_: true,
        copy: false,  // 如果不支持服务端复制
        mkdir: true,
    }
}
```

### list()

列出目录内容，返回 `Vec<Entry>`。

```rust
async fn list(&self, path: &str) -> Result<Vec<Entry>> {
    let entries = self.api.list_files(path).await?;
    Ok(entries.into_iter().map(|e| Entry {
        name: e.name,
        path: format!("{}/{}", path, e.name),
        size: e.size,
        is_dir: e.is_dir,
        modified: e.modified,
        // ...
    }).collect())
}
```

### open_reader()

打开文件读取器，支持范围读取（用于断点续传下载）。

```rust
async fn open_reader(
    &self,
    path: &str,
    range: Option<Range<u64>>,
) -> Result<Box<dyn AsyncRead + Unpin + Send>> {
    let url = self.get_download_url(path).await?;
    let mut req = self.client.get(&url);
    
    if let Some(r) = range {
        req = req.header("Range", format!("bytes={}-{}", r.start, r.end - 1));
    }
    
    let resp = req.send().await?;
    Ok(Box::new(StreamReader::new(resp.bytes_stream())))
}
```

### open_writer()

打开文件写入器（流式写入）。适用于本地存储、FTP等流式驱动。

```rust
async fn open_writer(
    &self,
    path: &str,
    size_hint: Option<u64>,
    progress: Option<ProgressCallback>,
) -> Result<Box<dyn AsyncWrite + Unpin + Send>> {
    let file = tokio::fs::File::create(path).await?;
    Ok(Box::new(file))
}
```

### delete()

删除文件或目录。

```rust
async fn delete(&self, path: &str) -> Result<()> {
    self.api.delete(path).await?;
    Ok(())
}
```

### create_dir()

创建目录。

```rust
async fn create_dir(&self, path: &str) -> Result<()> {
    self.api.create_folder(path).await?;
    Ok(())
}
```

---

## 可选实现的方法

### put()（重要！）

**云盘驱动必须重写此方法**，因为云盘通常需要：
- 完整文件 MD5/哈希
- 秒传检测
- 自定义分片上传逻辑

默认实现使用 `open_writer`，适用于流式驱动：

```rust
async fn put(
    &self,
    path: &str,
    data: bytes::Bytes,
    progress: Option<ProgressCallback>,
) -> Result<()> {
    // 默认实现 - 流式驱动可以使用
    let mut writer = self.open_writer(path, Some(data.len() as u64), progress).await?;
    writer.write_all(&data).await?;
    writer.shutdown().await?;
    Ok(())
}
```

**云盘驱动重写示例：**

```rust
async fn put(
    &self,
    path: &str,
    data: bytes::Bytes,
    progress: Option<ProgressCallback>,
) -> Result<()> {
    // 1. 计算整体 MD5
    let etag = format!("{:x}", md5::compute(&data));
    
    // 2. 创建上传任务（检查秒传）
    let upload = self.api.create_upload(path, &etag, data.len()).await?;
    
    if upload.reuse {
        // 秒传成功
        if let Some(ref cb) = progress {
            cb(data.len() as u64, data.len() as u64);
        }
        return Ok(());
    }
    
    // 3. 分片上传
    let slice_size = upload.slice_size;
    let total_slices = (data.len() + slice_size - 1) / slice_size;
    
    for i in 0..total_slices {
        let start = i * slice_size;
        let end = std::cmp::min(start + slice_size, data.len());
        let slice_data = &data[start..end];
        
        self.api.upload_slice(&upload.id, i, slice_data).await?;
        
        // 报告进度
        if let Some(ref cb) = progress {
            let completed = end as u64;
            cb(completed, data.len() as u64);
        }
    }
    
    // 4. 完成上传
    self.api.complete_upload(&upload.id).await?;
    
    // 5. 更新缓存（如果有）
    self.cache.insert(path.to_string(), upload.file_id);
    
    Ok(())
}
```

### get_direct_link()

获取文件直链（用于直接下载/预览）。

```rust
async fn get_direct_link(&self, path: &str) -> Result<Option<String>> {
    let url = self.api.get_download_url(path).await?;
    Ok(Some(url))
}
```

### get_space_info()

获取存储空间信息。

```rust
async fn get_space_info(&self) -> Result<Option<SpaceInfo>> {
    let info = self.api.get_quota().await?;
    Ok(Some(SpaceInfo {
        total: info.total,
        used: info.used,
        free: info.total - info.used,
    }))
}
```

---

## 进度回调规范

### ProgressCallback 类型

```rust
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;
// 参数: (已完成字节数, 总字节数)
```

### 使用规范

1. **回调必须轻量** - 只更新状态，不执行复杂操作
2. **不要阻塞** - 回调应该立即返回
3. **不要调用异步方法** - 回调是同步的

**正确示例：**

```rust
if let Some(ref cb) = progress {
    cb(completed_bytes, total_bytes);
}
```

**错误示例（不要这样做）：**

```rust
// ❌ 错误：在回调中创建 runtime
if let Some(ref cb) = progress {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // 异步操作...
        });
    });
}
```

---

## 错误处理

使用 `anyhow::Result` 返回错误：

```rust
use anyhow::{Result, anyhow};

async fn some_operation(&self) -> Result<()> {
    let resp = self.client.get(&url).send().await
        .map_err(|e| anyhow!("Network error: {}", e))?;
    
    if !resp.status().is_success() {
        return Err(anyhow!("API error: {}", resp.status()));
    }
    
    Ok(())
}
```

---

## 最佳实践

### 1. 缓存路径到 ID 的映射

云盘通常使用文件 ID 而不是路径，需要维护缓存：

```rust
struct MyDriver {
    path_cache: RwLock<HashMap<String, i64>>,
}

async fn get_file_id(&self, path: &str) -> Result<i64> {
    // 先查缓存
    if let Some(id) = self.path_cache.read().await.get(path) {
        return Ok(*id);
    }
    
    // 缓存未命中，调用 API
    let id = self.api.get_file_id(path).await?;
    self.path_cache.write().await.insert(path.to_string(), id);
    Ok(id)
}
```

### 2. Token 自动刷新

```rust
async fn ensure_authenticated(&self) -> Result<()> {
    let mut config = self.config.lock().await;
    if config.is_token_expired() {
        let new_token = self.api.refresh_token(&config.refresh_token).await?;
        config.access_token = new_token.access_token;
        config.token_expires_at = new_token.expires_at;
    }
    Ok(())
}
```

### 3. 并发上传控制

```rust
let semaphore = Arc::new(Semaphore::new(upload_thread_count));

for slice in slices {
    let permit = semaphore.clone().acquire_owned().await?;
    
    tokio::spawn(async move {
        let _permit = permit;  // 持有 permit 直到上传完成
        upload_slice(slice).await
    });
}
```

### 4. 上传完成后更新缓存

```rust
// 上传成功后，更新路径缓存
let file_path = format!("{}/{}", parent_path, filename);
self.path_cache.write().await.insert(file_path, new_file_id);
```

---

## 示例代码

完整的驱动实现示例，请参考：

- **本地驱动**: `backend/drivers/local/driver.rs`
- **FTP 驱动**: `backend/drivers/ftp/driver.rs`
- **123 云盘驱动**: `backend/drivers/123openapi/driver.rs`
- **OneDrive 驱动**: `backend/drivers/onedrive/driver.rs`

---

## 常见问题

### Q: 什么时候需要重写 put() 方法？

当你的存储服务需要以下任一条件时：
- 完整文件哈希（MD5/SHA1）用于秒传
- 自定义分片上传逻辑
- 特殊的上传 API（不是简单的 PUT 请求）

### Q: open_writer 和 put 有什么区别？

- `open_writer`: 返回一个写入流，适合流式驱动（本地、FTP）
- `put`: 接收完整数据，适合云盘驱动（需要计算哈希、分片上传）

Core 层会优先调用 `put`，默认实现会调用 `open_writer`。

### Q: 如何处理大文件上传？

对于云盘驱动，在 `put` 方法中实现分片上传逻辑。Core 层会把前端的分片先缓存到本地，合并后一次性传给 `put`。

### Q: 进度回调应该多久调用一次？

建议每个分片上传完成后调用一次，或者每 1MB 数据上传完成后调用一次。不要太频繁（如每次写入都调用）。
