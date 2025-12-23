use super::types::ArchiveFormat;

/// 根据文件名获取压缩格式
pub fn get_archive_format(filename: &str) -> Option<ArchiveFormat> {
    let filename = filename.to_lowercase();
    
    // 检查双扩展名
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if filename.ends_with(".tar.bz2") || filename.ends_with(".tbz2") {
        return Some(ArchiveFormat::TarBz2);
    }
    
    // 检查单扩展名
    if filename.ends_with(".zip") {
        return Some(ArchiveFormat::Zip);
    }
    if filename.ends_with(".tar") {
        return Some(ArchiveFormat::Tar);
    }
    if filename.ends_with(".7z") {
        return Some(ArchiveFormat::SevenZip);
    }
    
    None
}

/// 格式化文件大小
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2}GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

/// 格式化速度
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0}B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1}KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.1}MB/s", bytes_per_sec / 1024.0 / 1024.0)
    }
}

/// 根据指定编码解码文件名
pub fn decode_filename(raw_name: &[u8], encoding: &str) -> String {
    // 先尝试 UTF-8
    if let Ok(s) = std::str::from_utf8(raw_name) {
        // 如果是有效 UTF-8 且用户没有强制指定其他编码，直接返回
        if encoding == "utf-8" || encoding.is_empty() {
            return s.to_string();
        }
    }
    
    // 根据用户指定的编码解码
    let decoder = match encoding.to_lowercase().as_str() {
        "gbk" => encoding_rs::GBK,
        "gb2312" => encoding_rs::GB18030, // GB2312 是 GB18030 的子集
        "gb18030" => encoding_rs::GB18030,
        "big5" => encoding_rs::BIG5,
        "shift_jis" | "shift-jis" => encoding_rs::SHIFT_JIS,
        "euc-kr" | "euc_kr" => encoding_rs::EUC_KR,
        _ => encoding_rs::UTF_8,
    };
    
    let (decoded, _, _) = decoder.decode(raw_name);
    decoded.to_string()
}
