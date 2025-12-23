use std::io::Read;
use std::path::Path;
use super::types::ArchiveFormat;
use super::utils::decode_filename;


/// 解压到本地目录（带进度回调，同步，在 spawn_blocking 中调用）
pub fn extract_to_local_with_progress(
    archive_path: &Path,
    output_dir: &Path,
    format: ArchiveFormat,
    inner_path: &str,
    encoding: &str,
    overwrite: bool,
    control: &crate::task::TaskControl,
    progress_tx: tokio::sync::mpsc::Sender<(u64, u64, String)>,
) -> Result<u64, String> {
    match format {
        ArchiveFormat::Zip => extract_zip_to_local_progress(archive_path, output_dir, inner_path, encoding, overwrite, control, &progress_tx),
        ArchiveFormat::Tar => {
            let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
            extract_tar_to_local_progress(std::io::BufReader::new(file), output_dir, inner_path, encoding, overwrite, control, &progress_tx)
        }
        ArchiveFormat::TarGz => {
            let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
            let gz = flate2::read::GzDecoder::new(file);
            extract_tar_to_local_progress(std::io::BufReader::new(gz), output_dir, inner_path, encoding, overwrite, control, &progress_tx)
        }
        ArchiveFormat::TarBz2 => Err("暂不支持 tar.bz2".to_string()),
        ArchiveFormat::SevenZip => extract_7z_to_local_progress(archive_path, output_dir, inner_path, overwrite, control, &progress_tx),
    }
}

/// 解压 ZIP 到本地
fn extract_zip_to_local(
    archive_path: &Path, output_dir: &Path, inner_path: &str, encoding: &str, overwrite: bool,
    control: &crate::task::TaskControl,
) -> Result<u64, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let mut count = 0u64;
    
    for i in 0..archive.len() {
        if control.is_cancelled() { return Err("任务已取消".to_string()); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { return Err("任务已取消".to_string()); }
        }
        
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let path_str = decode_filename(file.name_raw(), encoding);
        
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { continue; }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/') 
        } else { 
            &path_str 
        };
        if rel.is_empty() { continue; }
        
        let target = output_dir.join(rel);
        if !target.starts_with(output_dir) { continue; }
        
        if file.is_dir() {
            std::fs::create_dir_all(&target).ok();
        } else {
            if !overwrite && target.exists() { continue; }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            let out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
            let mut writer = std::io::BufWriter::with_capacity(128 * 1024, out);
            std::io::copy(&mut file, &mut writer).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}

/// 解压 TAR 到本地
fn extract_tar_to_local<R: Read>(
    reader: R, output_dir: &Path, inner_path: &str, encoding: &str, overwrite: bool,
    control: &crate::task::TaskControl,
) -> Result<u64, String> {
    let mut archive = tar::Archive::new(reader);
    let mut count = 0u64;
    
    for entry in archive.entries().map_err(|e| e.to_string())? {
        if control.is_cancelled() { return Err("任务已取消".to_string()); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { return Err("任务已取消".to_string()); }
        }
        
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path_str = decode_filename(&entry.path_bytes(), encoding);
        
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { continue; }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/') 
        } else { 
            &path_str 
        };
        if rel.is_empty() { continue; }
        
        let target = output_dir.join(rel);
        if !target.starts_with(output_dir) { continue; }
        
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target).ok();
        } else if entry.header().entry_type().is_file() {
            if !overwrite && target.exists() { continue; }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            let out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
            let mut writer = std::io::BufWriter::with_capacity(128 * 1024, out);
            std::io::copy(&mut entry, &mut writer).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}

/// 解压 7Z 到本地
fn extract_7z_to_local(
    archive_path: &Path, output_dir: &Path, inner_path: &str, overwrite: bool,
    control: &crate::task::TaskControl,
) -> Result<u64, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut archive = sevenz_rust::SevenZReader::new(std::io::BufReader::new(file), len, sevenz_rust::Password::empty())
        .map_err(|e| e.to_string())?;
    
    let mut count = 0u64;
    let mut cancelled = false;
    
    archive.for_each_entries(|entry, reader| {
        if control.is_cancelled() { cancelled = true; return Ok(false); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { cancelled = true; return Ok(false); }
        }
        
        let path_str = entry.name().to_string();
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { return Ok(true); }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/').to_string() 
        } else { 
            path_str.clone() 
        };
        if rel.is_empty() { return Ok(true); }
        
        let target = output_dir.join(&rel);
        if !target.starts_with(output_dir) { return Ok(true); }
        
        if entry.is_directory() {
            std::fs::create_dir_all(&target).ok();
        } else {
            if !overwrite && target.exists() { return Ok(true); }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            if let Ok(mut out) = std::fs::File::create(&target) {
                if std::io::copy(&mut std::io::BufReader::new(reader), &mut out).is_ok() {
                    count += 1;
                }
            }
        }
        Ok(true)
    }).map_err(|e| e.to_string())?;
    
    if cancelled { return Err("任务已取消".to_string()); }
    Ok(count)
}

/// 解压 ZIP 到本地（带进度）
fn extract_zip_to_local_progress(
    archive_path: &Path, output_dir: &Path, inner_path: &str, encoding: &str, overwrite: bool,
    control: &crate::task::TaskControl,
    progress_tx: &tokio::sync::mpsc::Sender<(u64, u64, String)>,
) -> Result<u64, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let total = archive.len() as u64;
    let mut count = 0u64;
    let mut last_update = std::time::Instant::now();
    
    for i in 0..archive.len() {
        if control.is_cancelled() { return Err("任务已取消".to_string()); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { return Err("任务已取消".to_string()); }
        }
        
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let path_str = decode_filename(file.name_raw(), encoding);
        
        // 每1秒发送一次进度（try_send 不阻塞）
        let now = std::time::Instant::now();
        if now.duration_since(last_update).as_millis() >= 1000 {
            let _ = progress_tx.try_send((i as u64, total, path_str.clone()));
            last_update = now;
        }
        
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { continue; }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/') 
        } else { 
            &path_str 
        };
        if rel.is_empty() { continue; }
        
        let target = output_dir.join(rel);
        if !target.starts_with(output_dir) { continue; }
        
        if file.is_dir() {
            std::fs::create_dir_all(&target).ok();
        } else {
            if !overwrite && target.exists() { continue; }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            let out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
            let mut writer = std::io::BufWriter::with_capacity(128 * 1024, out);
            std::io::copy(&mut file, &mut writer).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    // 发送最终进度
    let _ = progress_tx.try_send((total, total, "完成".to_string()));
    Ok(count)
}

/// 解压 TAR 到本地（带进度）
fn extract_tar_to_local_progress<R: Read>(
    reader: R, output_dir: &Path, inner_path: &str, encoding: &str, overwrite: bool,
    control: &crate::task::TaskControl,
    progress_tx: &tokio::sync::mpsc::Sender<(u64, u64, String)>,
) -> Result<u64, String> {
    let mut archive = tar::Archive::new(reader);
    let mut count = 0u64;
    let mut processed = 0u64;
    let mut last_update = std::time::Instant::now();
    
    for entry in archive.entries().map_err(|e| e.to_string())? {
        if control.is_cancelled() { return Err("任务已取消".to_string()); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { return Err("任务已取消".to_string()); }
        }
        
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path_str = decode_filename(&entry.path_bytes(), encoding);
        processed += 1;
        
        // 每1秒发送一次进度
        let now = std::time::Instant::now();
        if now.duration_since(last_update).as_millis() >= 1000 {
            let _ = progress_tx.try_send((processed, 0, path_str.clone())); // TAR 不知道总数
            last_update = now;
        }
        
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { continue; }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/') 
        } else { 
            &path_str 
        };
        if rel.is_empty() { continue; }
        
        let target = output_dir.join(rel);
        if !target.starts_with(output_dir) { continue; }
        
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target).ok();
        } else if entry.header().entry_type().is_file() {
            if !overwrite && target.exists() { continue; }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            let out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
            let mut writer = std::io::BufWriter::with_capacity(128 * 1024, out);
            std::io::copy(&mut entry, &mut writer).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    let _ = progress_tx.try_send((processed, processed, "完成".to_string()));
    Ok(count)
}

/// 解压 7Z 到本地（带进度）
fn extract_7z_to_local_progress(
    archive_path: &Path, output_dir: &Path, inner_path: &str, overwrite: bool,
    control: &crate::task::TaskControl,
    progress_tx: &tokio::sync::mpsc::Sender<(u64, u64, String)>,
) -> Result<u64, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut archive = sevenz_rust::SevenZReader::new(std::io::BufReader::new(file), len, sevenz_rust::Password::empty())
        .map_err(|e| e.to_string())?;
    
    let mut count = 0u64;
    let mut processed = 0u64;
    let mut cancelled = false;
    let mut last_update = std::time::Instant::now();
    
    archive.for_each_entries(|entry, reader| {
        if control.is_cancelled() { cancelled = true; return Ok(false); }
        while control.is_paused() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if control.is_cancelled() { cancelled = true; return Ok(false); }
        }
        
        let path_str = entry.name().to_string();
        processed += 1;
        
        // 每1秒发送一次进度
        let now = std::time::Instant::now();
        if now.duration_since(last_update).as_millis() >= 1000 {
            let _ = progress_tx.try_send((processed, 0, path_str.clone()));
            last_update = now;
        }
        
        if !inner_path.is_empty() && !path_str.starts_with(inner_path) { return Ok(true); }
        let rel = if !inner_path.is_empty() { 
            path_str[inner_path.len()..].trim_start_matches('/').to_string() 
        } else { 
            path_str.clone() 
        };
        if rel.is_empty() { return Ok(true); }
        
        let target = output_dir.join(&rel);
        if !target.starts_with(output_dir) { return Ok(true); }
        
        if entry.is_directory() {
            std::fs::create_dir_all(&target).ok();
        } else {
            if !overwrite && target.exists() { return Ok(true); }
            if let Some(p) = target.parent() { std::fs::create_dir_all(p).ok(); }
            if let Ok(mut out) = std::fs::File::create(&target) {
                if std::io::copy(&mut std::io::BufReader::new(reader), &mut out).is_ok() {
                    count += 1;
                }
            }
        }
        Ok(true)
    }).map_err(|e| e.to_string())?;
    
    let _ = progress_tx.try_send((processed, processed, "完成".to_string()));
    if cancelled { return Err("任务已取消".to_string()); }
    Ok(count)
}
