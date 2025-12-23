//! 文件索引 - 流式读取 + 快速匹配
//! 
//! 索引文件格式（每行一条记录，TSV格式）：
//! path\tname\tis_dir\tsize\tmodified\tname_normalized
//! 
//! 搜索优化：
//! - 快速过滤：先做简单包含检查，再做复杂匹配
//! - 减少编辑距离计算
//! - 提前收集足够结果后跳过低分项
//! 
//! 索引文件存放在程序同级目录的 data/ 下

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write, Read, Seek, SeekFrom};
use std::path::Path;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::tokenizer::{tokenize_query, to_simplified, contains_chinese};

/// 索引条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: i64,
}

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified: i64,
    pub score: f32,
}

/// 索引统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub total_size: u64,
    pub last_updated: Option<i64>,
}

/// 文件索引管理器
pub struct FileIndex {
    index_path: String,
    stats_path: String,
}

impl FileIndex {
    /// 创建文件索引管理器
    /// 索引文件存放在当前工作目录（程序运行时的目录）
    pub fn new() -> Self {
        // 使用当前工作目录（程序运行时的目录）
        let work_dir = std::env::current_dir().unwrap_or_default();
        
        let index_path = work_dir.join("search.idx").to_string_lossy().to_string();
        let stats_path = work_dir.join("search.stats").to_string_lossy().to_string();
        
        tracing::info!("Index file path: {}", index_path);
        
        Self {
            index_path,
            stats_path,
        }
    }

    /// 使用指定目录创建（用于测试）
    #[allow(dead_code)]
    pub fn with_dir(data_dir: &str) -> Self {
        let dir = Path::new(data_dir);
        std::fs::create_dir_all(dir).ok();
        Self {
            index_path: dir.join("search.idx").to_string_lossy().to_string(),
            stats_path: dir.join("search.stats").to_string_lossy().to_string(),
        }
    }

    /// 清空索引
    pub fn clear(&self) -> Result<(), String> {
        if Path::new(&self.index_path).exists() {
            std::fs::remove_file(&self.index_path).map_err(|e| e.to_string())?;
        }
        if Path::new(&self.stats_path).exists() {
            std::fs::remove_file(&self.stats_path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 创建索引写入器
    pub fn create_writer(&self) -> Result<IndexWriter, String> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.index_path)
            .map_err(|e| e.to_string())?;
        
        Ok(IndexWriter {
            writer: BufWriter::with_capacity(64 * 1024, file), // 64KB缓冲
            file_count: 0,
            dir_count: 0,
        })
    }

    /// 追加写入器（用于增量索引）
    pub fn append_writer(&self) -> Result<IndexWriter, String> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&self.index_path)
            .map_err(|e| e.to_string())?;
        
        Ok(IndexWriter {
            writer: BufWriter::with_capacity(64 * 1024, file),
            file_count: 0,
            dir_count: 0,
        })
    }

    /// 搜索（并行分块读取，快速匹配优化）
    /// 返回 (结果列表, 总匹配数)
    pub fn search(&self, query: &str, limit: usize) -> Result<(Vec<SearchHit>, usize), String> {
        if !Path::new(&self.index_path).exists() {
            return Ok((Vec::new(), 0));
        }

        // 获取文件大小
        let file_size = std::fs::metadata(&self.index_path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        // 小文件直接单线程处理
        if file_size < 1024 * 1024 {
            return self.search_single_thread(query, limit);
        }
        
        // 大文件并行处理
        self.search_parallel(query, limit, file_size)
    }
    
    /// 单线程搜索（小文件）
    fn search_single_thread(&self, query: &str, limit: usize) -> Result<(Vec<SearchHit>, usize), String> {
        let file = File::open(&self.index_path).map_err(|e| e.to_string())?;
        let reader = BufReader::with_capacity(256 * 1024, file);
        
        let query_lower = query.to_lowercase();
        let query_simplified = to_simplified(&query_lower);
        let query_tokens = tokenize_query(&query_lower);
        let is_chinese_query = contains_chinese(query);
        
        let mut results: Vec<SearchHit> = Vec::with_capacity(limit);
        let mut min_score = 0.0f32;
        let mut total_matched = 0usize;
        
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            
            if let Some((hit, score)) = self.match_line(&line, &query_lower, &query_simplified, &query_tokens, is_chinese_query, min_score) {
                total_matched += 1;
                self.insert_result(&mut results, hit, score, limit, &mut min_score);
            }
        }
        
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        Ok((results, total_matched))
    }
    
    /// 并行搜索（大文件）
    fn search_parallel(&self, query: &str, limit: usize, file_size: u64) -> Result<(Vec<SearchHit>, usize), String> {
        let num_threads = num_cpus::get().min(4); // 最多4线程
        let chunk_size = file_size / num_threads as u64;
        
        let query_lower = query.to_lowercase();
        let query_simplified = to_simplified(&query_lower);
        let query_tokens = tokenize_query(&query_lower);
        let is_chinese_query = contains_chinese(query);
        
        let total_matched = Arc::new(AtomicUsize::new(0));
        let index_path = self.index_path.clone();
        
        // 使用标准线程进行并行搜索
        let handles: Vec<_> = (0..num_threads).map(|i| {
            let start = i as u64 * chunk_size;
            let end = if i == num_threads - 1 { file_size } else { (i as u64 + 1) * chunk_size };
            let query_lower = query_lower.clone();
            let query_simplified = query_simplified.clone();
            let query_tokens = query_tokens.clone();
            let total_matched = Arc::clone(&total_matched);
            let index_path = index_path.clone();
            
            std::thread::spawn(move || {
                search_chunk(&index_path, start, end, &query_lower, &query_simplified, &query_tokens, is_chinese_query, limit, &total_matched)
            })
        }).collect();
        
        // 收集结果
        let mut all_results: Vec<SearchHit> = Vec::new();
        for handle in handles {
            if let Ok(chunk_results) = handle.join() {
                all_results.extend(chunk_results);
            }
        }
        
        // 合并排序取top-k
        all_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        all_results.truncate(limit);
        
        Ok((all_results, total_matched.load(Ordering::Relaxed)))
    }
    
    /// 匹配单行
    fn match_line(&self, line: &str, query_lower: &str, query_simplified: &str, query_tokens: &[String], is_chinese_query: bool, min_score: f32) -> Option<(SearchHit, f32)> {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            return None;
        }
        
        let name_lower = parts[5];
        let name_simplified = parts[6];
        
        // 快速过滤
        let quick_match = name_lower.contains(query_lower) || name_simplified.contains(query_simplified);
        if !quick_match && min_score >= 50.0 {
            return None;
        }
        
        let score = calculate_match_score_fast(
            name_lower, name_simplified,
            query_lower, query_simplified, query_tokens,
            is_chinese_query, min_score,
        );
        
        if score > 0.0 {
            Some((SearchHit {
                path: parts[0].to_string(),
                name: parts[1].to_string(),
                is_dir: parts[2] == "1",
                size: parts[3].parse().unwrap_or(0),
                modified: parts[4].parse().unwrap_or(0),
                score,
            }, score))
        } else {
            None
        }
    }
    
    /// 插入结果到top-k列表
    fn insert_result(&self, results: &mut Vec<SearchHit>, hit: SearchHit, score: f32, limit: usize, min_score: &mut f32) {
        if results.len() < limit {
            results.push(hit);
            if results.len() == limit {
                *min_score = results.iter().map(|r| r.score).fold(f32::INFINITY, f32::min);
            }
        } else if score > *min_score {
            if let Some(idx) = results.iter().position(|r| r.score == *min_score) {
                results[idx] = hit;
                *min_score = results.iter().map(|r| r.score).fold(f32::INFINITY, f32::min);
            }
        }
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> IndexStats {
        if !Path::new(&self.stats_path).exists() {
            return IndexStats::default();
        }
        
        match std::fs::read_to_string(&self.stats_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => IndexStats::default(),
        }
    }

    /// 保存统计信息
    pub fn save_stats(&self, stats: &IndexStats) -> Result<(), String> {
        let content = serde_json::to_string(stats).map_err(|e| e.to_string())?;
        std::fs::write(&self.stats_path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 索引是否存在
    pub fn exists(&self) -> bool {
        Path::new(&self.index_path).exists()
    }

    /// 获取索引文件大小
    pub fn index_size(&self) -> u64 {
        std::fs::metadata(&self.index_path)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// 删除指定路径的索引条目（支持前缀匹配，用于删除目录）
    pub fn delete_by_path(&self, path_prefix: &str) -> Result<u64, String> {
        if !Path::new(&self.index_path).exists() {
            return Ok(0);
        }

        let temp_path = format!("{}.tmp", self.index_path);
        let src = File::open(&self.index_path).map_err(|e| e.to_string())?;
        let dst = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .map_err(|e| e.to_string())?;

        let reader = BufReader::with_capacity(64 * 1024, src);
        let mut writer = BufWriter::with_capacity(64 * 1024, dst);

        let mut deleted = 0u64;
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            // 检查路径是否匹配（前缀匹配）
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() {
                continue;
            }
            let entry_path = parts[0];
            
            if entry_path == path_prefix || entry_path.starts_with(&format!("{}/", path_prefix)) {
                deleted += 1;
                continue; // 跳过此条目
            }

            writeln!(writer, "{}", line).map_err(|e| e.to_string())?;
        }

        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);

        // 替换原文件
        std::fs::rename(&temp_path, &self.index_path).map_err(|e| e.to_string())?;

        Ok(deleted)
    }

    /// 更新路径（用于移动/重命名操作）
    pub fn update_path(&self, old_path: &str, new_path: &str) -> Result<u64, String> {
        if !Path::new(&self.index_path).exists() {
            return Ok(0);
        }

        let temp_path = format!("{}.tmp", self.index_path);
        let src = File::open(&self.index_path).map_err(|e| e.to_string())?;
        let dst = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .map_err(|e| e.to_string())?;

        let reader = BufReader::with_capacity(64 * 1024, src);
        let mut writer = BufWriter::with_capacity(64 * 1024, dst);

        let mut updated = 0u64;
        let old_prefix = format!("{}/", old_path);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 7 {
                writeln!(writer, "{}", line).map_err(|e| e.to_string())?;
                continue;
            }

            let entry_path = parts[0];
            
            // 检查是否需要更新路径
            let new_entry_path = if entry_path == old_path {
                updated += 1;
                new_path.to_string()
            } else if entry_path.starts_with(&old_prefix) {
                updated += 1;
                format!("{}{}", new_path, &entry_path[old_path.len()..])
            } else {
                entry_path.to_string()
            };

            // 如果路径发生变化，需要更新名称
            let name = if new_entry_path != entry_path {
                new_entry_path.split('/').last().unwrap_or(parts[1])
            } else {
                parts[1]
            };

            let name_lower = name.to_lowercase();
            let name_simplified = to_simplified(&name_lower);

            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                new_entry_path, name, parts[2], parts[3], parts[4], name_lower, name_simplified
            ).map_err(|e| e.to_string())?;
        }

        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);

        std::fs::rename(&temp_path, &self.index_path).map_err(|e| e.to_string())?;

        Ok(updated)
    }

    /// 按驱动ID删除索引（删除驱动时调用）
    pub fn delete_by_driver(&self, driver_mount_path: &str) -> Result<u64, String> {
        self.delete_by_path(driver_mount_path)
    }
}

/// 索引写入器（流式写入，控制内存）
pub struct IndexWriter {
    writer: BufWriter<File>,
    pub file_count: u64,
    pub dir_count: u64,
}

impl IndexWriter {
    /// 写入一条索引记录
    pub fn write_entry(&mut self, entry: &IndexEntry) -> Result<(), String> {
        // 预处理：小写、简体
        let name_lower = entry.name.to_lowercase();
        let name_simplified = to_simplified(&name_lower);
        
        // TSV格式：path\tname\tis_dir\tsize\tmodified\tname_lower\tname_simplified
        writeln!(
            self.writer,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            entry.path,
            entry.name,
            if entry.is_dir { "1" } else { "0" },
            entry.size,
            entry.modified,
            name_lower,
            name_simplified
        ).map_err(|e| e.to_string())?;
        
        if entry.is_dir {
            self.dir_count += 1;
        } else {
            self.file_count += 1;
        }
        
        Ok(())
    }

    /// 刷新缓冲区
    pub fn flush(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|e| e.to_string())
    }

    /// 完成写入
    pub fn finish(mut self) -> Result<(u64, u64), String> {
        self.writer.flush().map_err(|e| e.to_string())?;
        Ok((self.file_count, self.dir_count))
    }
}

/// 搜索分块（并行线程调用）
fn search_chunk(
    index_path: &str,
    start: u64,
    end: u64,
    query_lower: &str,
    query_simplified: &str,
    query_tokens: &[String],
    is_chinese_query: bool,
    limit: usize,
    total_matched: &AtomicUsize,
) -> Vec<SearchHit> {
    let mut file = match File::open(index_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    
    // 跳转到起始位置
    if start > 0 {
        if file.seek(SeekFrom::Start(start)).is_err() {
            return Vec::new();
        }
    }
    
    let mut reader = BufReader::with_capacity(128 * 1024, file);
    
    // 如果不是从头开始，跳过第一个不完整的行
    if start > 0 {
        let mut skip_buf = String::new();
        let _ = reader.read_line(&mut skip_buf);
    }
    
    let mut results: Vec<SearchHit> = Vec::with_capacity(limit);
    let mut min_score = 0.0f32;
    let mut bytes_read = if start > 0 { 0u64 } else { 0u64 };
    let chunk_len = end - start;
    
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(n) => {
                bytes_read += n as u64;
                if bytes_read > chunk_len && start > 0 {
                    break; // 超出本分块范围
                }
            }
            Err(_) => continue,
        }
        
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        
        let name_lower = parts[5];
        let name_simplified = parts[6];
        
        // 快速过滤
        let quick_match = name_lower.contains(query_lower) || name_simplified.contains(query_simplified);
        if !quick_match && min_score >= 50.0 {
            continue;
        }
        
        let score = calculate_match_score_fast(
            name_lower, name_simplified,
            query_lower, query_simplified, query_tokens,
            is_chinese_query, min_score,
        );
        
        if score > 0.0 {
            total_matched.fetch_add(1, Ordering::Relaxed);
            
            let hit = SearchHit {
                path: parts[0].to_string(),
                name: parts[1].to_string(),
                is_dir: parts[2] == "1",
                size: parts[3].parse().unwrap_or(0),
                modified: parts[4].parse().unwrap_or(0),
                score,
            };
            
            if results.len() < limit {
                results.push(hit);
                if results.len() == limit {
                    min_score = results.iter().map(|r| r.score).fold(f32::INFINITY, f32::min);
                }
            } else if score > min_score {
                if let Some(idx) = results.iter().position(|r| r.score == min_score) {
                    results[idx] = hit;
                    min_score = results.iter().map(|r| r.score).fold(f32::INFINITY, f32::min);
                }
            }
        }
    }
    
    results
}

/// 计算匹配分数（快速版本，支持提前退出）
fn calculate_match_score_fast(
    name_lower: &str,
    name_simplified: &str,
    query_lower: &str,
    query_simplified: &str,
    query_tokens: &[String],
    is_chinese_query: bool,
    min_score_threshold: f32,
) -> f32 {
    // 1. 完全匹配（最高分）
    if name_lower == query_lower {
        return 100.0;
    }
    
    // 2. 简繁匹配（完全）
    if name_simplified == query_simplified {
        return 95.0;
    }
    
    // 3. 前缀匹配
    if name_lower.starts_with(query_lower) {
        return 80.0;
    }
    if name_simplified.starts_with(query_simplified) {
        return 75.0;
    }
    
    // 4. 包含匹配
    if name_lower.contains(query_lower) {
        return 60.0;
    }
    if name_simplified.contains(query_simplified) {
        return 55.0;
    }
    
    // 如果阈值已经很高，跳过低分匹配
    if min_score_threshold >= 50.0 {
        return 0.0;
    }
    
    // 5. 分词匹配（中文）
    if is_chinese_query && !query_tokens.is_empty() {
        let mut matched_tokens = 0;
        for token in query_tokens {
            if name_lower.contains(token.as_str()) || name_simplified.contains(token.as_str()) {
                matched_tokens += 1;
            }
        }
        if matched_tokens > 0 {
            let score = 30.0 + (matched_tokens as f32 / query_tokens.len() as f32) * 20.0;
            if score > min_score_threshold {
                return score;
            }
        }
    }
    
    // 6. 模糊匹配（编辑距离）- 只在阈值很低时计算
    if min_score_threshold < 15.0 && query_lower.len() >= 2 && query_lower.len() <= 10 {
        let distance = levenshtein_distance_limited(name_lower, query_lower, 2);
        if let Some(d) = distance {
            if d <= 2 {
                return 20.0 - (d as f32 * 5.0);
            }
        }
    }
    
    0.0
}

/// 有限制的编辑距离计算（超过max_distance提前退出）
fn levenshtein_distance_limited(s1: &str, s2: &str, max_distance: usize) -> Option<usize> {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();
    
    // 长度差太大直接返回None
    if len1.abs_diff(len2) > max_distance {
        return None;
    }
    
    // 短字符串优化
    if len1 == 0 { return Some(len2); }
    if len2 == 0 { return Some(len1); }
    
    let mut prev = vec![0usize; len2 + 1];
    let mut curr = vec![0usize; len2 + 1];
    
    for j in 0..=len2 { prev[j] = j; }
    
    for i in 1..=len1 {
        curr[0] = i;
        let mut min_in_row = curr[0];
        
        for j in 1..=len2 {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
            min_in_row = min_in_row.min(curr[j]);
        }
        
        // 提前退出：这一行的最小值已经超过阈值
        if min_in_row > max_distance {
            return None;
        }
        
        std::mem::swap(&mut prev, &mut curr);
    }
    
    if prev[len2] <= max_distance {
        Some(prev[len2])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_score() {
        // 完全匹配
        assert_eq!(calculate_match_score("test.txt", "test.txt", "test.txt", "test.txt", "test.txt", &[], false), 100.0);
        
        // 包含匹配
        let score = calculate_match_score("my_test_file.txt", "my_test_file.txt", "my_test_file.txt", "test", "test", &[], false);
        assert!(score > 50.0);
    }
}
