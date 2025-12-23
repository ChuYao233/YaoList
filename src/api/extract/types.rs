use serde::{Deserialize, Serialize};

/// 解压缩请求
#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    /// 压缩文件路径
    pub src_path: String,
    /// 目标目录路径
    pub dst_path: String,
    /// 压缩包密码（可选）
    #[serde(default)]
    pub password: Option<String>,
    /// 压缩包内部路径（可选，用于只解压部分内容）
    #[serde(default)]
    pub inner_path: Option<String>,
    /// 文件名编码（默认 utf-8）
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// 是否解压到新子文件夹
    #[serde(default)]
    pub put_into_new_dir: bool,
    /// 是否覆盖现有文件
    #[serde(default)]
    pub overwrite: bool,
    /// 强制执行（忽略磁盘空间检查）
    #[serde(default)]
    pub force: bool,
}

fn default_encoding() -> String {
    "utf-8".to_string()
}

/// 解压缩响应
#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub task_id: String,
    pub message: String,
}

pub(crate) struct MountInfo {
    pub id: String,
    pub mount_path: String,
}

/// 支持的压缩格式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    SevenZip,
}
