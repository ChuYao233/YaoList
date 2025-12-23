//! Search engine - in-memory full-text search implementation / 搜索引擎
//! 
//! Architecture principle: only expose primitive operations, do not control flow / 架构原则
//! - index_document: index single document / 索引单个文档
//! - index_batch: batch indexing / 批量索引
//! - search: search / 搜索
//! - delete: delete document / 删除文档
//! - clear: clear index / 清空索引

use std::collections::HashMap;
use std::sync::RwLock;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::schema::{FileDocument, SearchOptions, SearchResult};
use super::tokenizer::{tokenize, tokenize_query, generate_ngrams, contains_chinese};

/// Inverted index entry / 倒排索引条目
#[derive(Debug, Clone)]
struct PostingEntry {
    doc_id: String,
    positions: Vec<usize>,
    score_boost: f32,
}

/// Search engine / 搜索引擎
/// 
/// Implements full-text search using inverted index, supports: / 使用倒排索引实现全文搜索
/// - Chinese word segmentation (jieba) / 中文分词
/// - Fuzzy search (N-gram + edit distance) / 模糊搜索
/// - Prefix matching / 前缀匹配
pub struct SearchEngine {
    /// Document storage: doc_id -> FileDocument / 文档存储
    documents: RwLock<HashMap<String, FileDocument>>,
    /// Inverted index: token -> [PostingEntry] / 倒排索引
    inverted_index: RwLock<HashMap<String, Vec<PostingEntry>>>,
    /// N-gram index (for fuzzy matching) / N-gram 索引
    ngram_index: RwLock<HashMap<String, Vec<String>>>,
    /// Index statistics / 索引统计
    stats: Mutex<IndexStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub document_count: usize,
    pub token_count: usize,
    pub last_updated: Option<i64>,
}

impl SearchEngine {
    /// Create new search engine instance / 创建新的搜索引擎实例
    pub fn new() -> Self {
        Self {
            documents: RwLock::new(HashMap::new()),
            inverted_index: RwLock::new(HashMap::new()),
            ngram_index: RwLock::new(HashMap::new()),
            stats: Mutex::new(IndexStats::default()),
        }
    }

    /// Get index statistics / 获取索引统计信息
    pub fn stats(&self) -> IndexStats {
        self.stats.lock().clone()
    }

    /// Index single document (primitive operation) / 索引单个文档
    pub fn index_document(&self, doc: FileDocument) -> Result<(), String> {
        let doc_id = doc.id.clone();
        
        // Extract text to be indexed / 提取要索引的文本
        let name_tokens = tokenize(&doc.name);
        let path_tokens = tokenize(&doc.path);
        
        // Generate N-grams for fuzzy matching / 生成 N-gram
        let name_ngrams = generate_ngrams(&doc.name, 1, 3);
        
        // Update inverted index / 更新倒排索引
        {
            let mut index = self.inverted_index.write().map_err(|e| e.to_string())?;
            
            // Index filename (higher weight) / 索引文件名
            for (pos, token) in name_tokens.iter().enumerate() {
                let entry = PostingEntry {
                    doc_id: doc_id.clone(),
                    positions: vec![pos],
                    score_boost: 2.0, // Filename matching has higher weight
                };
                index.entry(token.clone()).or_default().push(entry);
            }
            
            // Index path / 索引路径
            for (pos, token) in path_tokens.iter().enumerate() {
                let entry = PostingEntry {
                    doc_id: doc_id.clone(),
                    positions: vec![pos],
                    score_boost: 1.0,
                };
                index.entry(token.clone()).or_default().push(entry);
            }
        }
        
        // 更新 N-gram 索引
        {
            let mut ngram_idx = self.ngram_index.write().map_err(|e| e.to_string())?;
            for ngram in name_ngrams {
                ngram_idx.entry(ngram).or_default().push(doc_id.clone());
            }
        }
        
        // 存储文档
        {
            let mut docs = self.documents.write().map_err(|e| e.to_string())?;
            docs.insert(doc_id, doc);
        }
        
        // 更新统计
        {
            let mut stats = self.stats.lock();
            stats.document_count += 1;
            stats.last_updated = Some(chrono::Utc::now().timestamp());
        }
        
        Ok(())
    }

    /// 批量索引文档
    pub fn index_batch(&self, docs: Vec<FileDocument>) -> Result<usize, String> {
        let mut indexed = 0;
        for doc in docs {
            if self.index_document(doc).is_ok() {
                indexed += 1;
            }
        }
        Ok(indexed)
    }

    /// 搜索（原语操作）
    pub fn search(&self, options: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        if options.query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let query_tokens = tokenize_query(&options.query);
        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut scores: HashMap<String, f32> = HashMap::new();
        
        // 精确匹配搜索
        {
            let index = self.inverted_index.read().map_err(|e| e.to_string())?;
            
            for token in &query_tokens {
                if let Some(postings) = index.get(token) {
                    for posting in postings {
                        *scores.entry(posting.doc_id.clone()).or_default() += 
                            posting.score_boost * (1.0 + posting.positions.len() as f32 * 0.1);
                    }
                }
                
                // 前缀匹配
                for (idx_token, postings) in index.iter() {
                    if idx_token.starts_with(token) && idx_token != token {
                        for posting in postings {
                            *scores.entry(posting.doc_id.clone()).or_default() += 
                                posting.score_boost * 0.5;
                        }
                    }
                }
            }
        }

        // 模糊匹配（如果启用）
        if options.fuzzy && scores.len() < options.limit {
            let ngram_idx = self.ngram_index.read().map_err(|e| e.to_string())?;
            
            // 对中文使用 N-gram 模糊匹配
            if contains_chinese(&options.query) {
                let query_ngrams = generate_ngrams(&options.query, 1, 2);
                for ngram in query_ngrams {
                    if let Some(doc_ids) = ngram_idx.get(&ngram) {
                        for doc_id in doc_ids {
                            *scores.entry(doc_id.clone()).or_default() += 0.3;
                        }
                    }
                }
            }
            
            // 对英文使用编辑距离模糊匹配
            if !contains_chinese(&options.query) {
                let index = self.inverted_index.read().map_err(|e| e.to_string())?;
                for token in &query_tokens {
                    for (idx_token, postings) in index.iter() {
                        if fuzzy_match(token, idx_token, options.fuzzy_distance as usize) {
                            for posting in postings {
                                *scores.entry(posting.doc_id.clone()).or_default() += 
                                    posting.score_boost * 0.3;
                            }
                        }
                    }
                }
            }
        }

        // 获取文档并过滤
        let docs = self.documents.read().map_err(|e| e.to_string())?;
        
        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .filter_map(|(doc_id, score)| {
                let doc = docs.get(&doc_id)?;
                
                // 应用过滤条件
                if let Some(driver_id) = options.driver_id {
                    if doc.driver_id != driver_id {
                        return None;
                    }
                }
                
                if let Some(ref prefix) = options.path_prefix {
                    if !doc.path.starts_with(prefix) {
                        return None;
                    }
                }
                
                if options.dirs_only && !doc.is_dir {
                    return None;
                }
                
                if options.files_only && doc.is_dir {
                    return None;
                }
                
                Some(SearchResult {
                    document: doc.clone(),
                    score,
                    highlights: Vec::new(), // TODO: 实现高亮
                })
            })
            .collect();

        // 按分数排序
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // 应用分页
        let results: Vec<SearchResult> = results
            .into_iter()
            .skip(options.offset)
            .take(options.limit)
            .collect();

        Ok(results)
    }

    /// 删除文档（原语操作）
    pub fn delete_document(&self, doc_id: &str) -> Result<bool, String> {
        // 从文档存储中删除
        let doc = {
            let mut docs = self.documents.write().map_err(|e| e.to_string())?;
            docs.remove(doc_id)
        };

        if doc.is_none() {
            return Ok(false);
        }

        let doc = doc.unwrap();

        // 从倒排索引中删除
        {
            let mut index = self.inverted_index.write().map_err(|e| e.to_string())?;
            let tokens = tokenize(&doc.name);
            for token in tokens {
                if let Some(postings) = index.get_mut(&token) {
                    postings.retain(|p| p.doc_id != doc_id);
                }
            }
            let path_tokens = tokenize(&doc.path);
            for token in path_tokens {
                if let Some(postings) = index.get_mut(&token) {
                    postings.retain(|p| p.doc_id != doc_id);
                }
            }
        }

        // 从 N-gram 索引中删除
        {
            let mut ngram_idx = self.ngram_index.write().map_err(|e| e.to_string())?;
            let ngrams = generate_ngrams(&doc.name, 1, 3);
            for ngram in ngrams {
                if let Some(doc_ids) = ngram_idx.get_mut(&ngram) {
                    doc_ids.retain(|id| id != doc_id);
                }
            }
        }

        // 更新统计
        {
            let mut stats = self.stats.lock();
            stats.document_count = stats.document_count.saturating_sub(1);
            stats.last_updated = Some(chrono::Utc::now().timestamp());
        }

        Ok(true)
    }

    /// 按驱动ID删除所有文档
    pub fn delete_by_driver(&self, driver_id: i64) -> Result<usize, String> {
        let doc_ids: Vec<String> = {
            let docs = self.documents.read().map_err(|e| e.to_string())?;
            docs.iter()
                .filter(|(_, doc)| doc.driver_id == driver_id)
                .map(|(id, _)| id.clone())
                .collect()
        };

        let mut deleted = 0;
        for doc_id in doc_ids {
            if self.delete_document(&doc_id)? {
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// 清空所有索引（原语操作）
    pub fn clear(&self) -> Result<(), String> {
        {
            let mut docs = self.documents.write().map_err(|e| e.to_string())?;
            docs.clear();
        }
        {
            let mut index = self.inverted_index.write().map_err(|e| e.to_string())?;
            index.clear();
        }
        {
            let mut ngram_idx = self.ngram_index.write().map_err(|e| e.to_string())?;
            ngram_idx.clear();
        }
        {
            let mut stats = self.stats.lock();
            *stats = IndexStats::default();
        }
        Ok(())
    }

    /// 获取文档数量
    pub fn document_count(&self) -> usize {
        self.documents.read().map(|d| d.len()).unwrap_or(0)
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 简单的编辑距离模糊匹配
fn fuzzy_match(s1: &str, s2: &str, max_distance: usize) -> bool {
    if s1 == s2 {
        return true;
    }
    
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    
    // 长度差太大直接返回
    if len1.abs_diff(len2) > max_distance {
        return false;
    }
    
    // 计算编辑距离（简化版）
    let distance = levenshtein_distance(s1, s2);
    distance <= max_distance
}

/// 计算 Levenshtein 编辑距离
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();
    
    if len1 == 0 { return len2; }
    if len2 == 0 { return len1; }
    
    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];
    
    for i in 0..=len1 { matrix[i][0] = i; }
    for j in 0..=len2 { matrix[0][j] = j; }
    
    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }
    
    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_doc(id: &str, name: &str, path: &str) -> FileDocument {
        FileDocument {
            id: id.to_string(),
            driver_id: 1,
            path: path.to_string(),
            name: name.to_string(),
            parent: "/".to_string(),
            is_dir: false,
            size: 1024,
            modified: 0,
        }
    }

    #[test]
    fn test_index_and_search() {
        let engine = SearchEngine::new();
        
        engine.index_document(create_test_doc("1", "测试文件.txt", "/测试文件.txt")).unwrap();
        engine.index_document(create_test_doc("2", "test.txt", "/test.txt")).unwrap();
        engine.index_document(create_test_doc("3", "文档资料.pdf", "/docs/文档资料.pdf")).unwrap();
        
        // 搜索中文
        let results = engine.search(&SearchOptions::new("测试")).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].document.name.contains("测试"));
        
        // 搜索英文
        let results = engine.search(&SearchOptions::new("test")).unwrap();
        assert!(!results.is_empty());
        
        // 模糊搜索
        let results = engine.search(&SearchOptions::new("文档").fuzzy(true)).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("test", "test", 1));
        assert!(fuzzy_match("test", "tест", 1)); // 1个字符不同
        assert!(!fuzzy_match("test", "hello", 1));
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "abd"), 1);
        assert_eq!(levenshtein_distance("abc", "abcd"), 1);
    }
}
