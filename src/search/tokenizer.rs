//! Chinese tokenizer - uses jieba-rs for Chinese word segmentation / 中文分词器
//! 
//! Supports / 支持：
//! - Chinese word segmentation (jieba) / 中文分词
//! - English word segmentation (space-separated + lowercase) / 英文分词
//! - Mixed text processing / 混合文本处理

use jieba_rs::Jieba;
use once_cell::sync::Lazy;

/// Global jieba tokenizer instance / 全局 jieba 分词器实例
static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

/// Tokenize text / 对文本进行分词
/// 
/// Supports Chinese-English mixed text, Chinese uses jieba, English uses space separation / 支持中英文混合文本
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    
    // Use jieba for tokenization (search engine mode, finer granularity) / 使用 jieba
    let words = JIEBA.cut_for_search(text, true);
    
    for word in words {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        
        // Convert to lowercase and add / 转小写并添加
        let lower = word.to_lowercase();
        if !lower.is_empty() {
            tokens.push(lower);
        }
    }
    
    tokens
}

/// Tokenize search query (used for queries) / 对搜索查询进行分词
pub fn tokenize_query(query: &str) -> Vec<String> {
    // Query tokenization consistent with index tokenization / 查询分词与索引分词保持一致
    tokenize(query)
}

/// Generate N-grams (for fuzzy matching) / 生成 N-gram
/// 
/// Example: "测试" -> ["测", "试", "测试"] / 例如
pub fn generate_ngrams(text: &str, min_n: usize, max_n: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut ngrams = Vec::new();
    
    for n in min_n..=max_n {
        if n > chars.len() {
            break;
        }
        for i in 0..=(chars.len() - n) {
            let ngram: String = chars[i..i + n].iter().collect();
            if !ngram.trim().is_empty() {
                ngrams.push(ngram.to_lowercase());
            }
        }
    }
    
    ngrams
}

/// Check if text contains Chinese characters / 检测文本是否包含中文字符
pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
    })
}

/// Check if text contains CJK characters (Chinese, Japanese, Korean) / 检测文本是否包含CJK字符
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c, 
            '\u{4e00}'..='\u{9fff}' |  // CJK Unified Ideographs
            '\u{3400}'..='\u{4dbf}' |  // CJK Extension A
            '\u{3040}'..='\u{309f}' |  // Hiragana
            '\u{30a0}'..='\u{30ff}' |  // Katakana
            '\u{ac00}'..='\u{d7af}'    // Hangul Syllables
        )
    })
}

/// Normalize text for search (multilingual support) / 标准化文本用于搜索
/// - Convert to lowercase / 转小写
/// - Simplified/traditional conversion / 简繁转换
/// - Remove extra whitespace / 去除多余空白
pub fn normalize_for_search(text: &str) -> String {
    let lower = text.to_lowercase();
    let simplified = to_simplified(&lower);
    simplified.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalize text (for exact matching) / 标准化文本
#[allow(dead_code)]
pub fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 繁体转简体（常用字映射）
pub fn to_simplified(text: &str) -> String {
    text.chars().map(|c| {
        match c {
            '國' => '国', '學' => '学', '書' => '书', '電' => '电', '話' => '话',
            '語' => '语', '說' => '说', '讀' => '读', '寫' => '写', '聽' => '听',
            '見' => '见', '視' => '视', '觀' => '观', '開' => '开', '關' => '关',
            '門' => '门', '間' => '间', '問' => '问', '時' => '时', '當' => '当',
            '會' => '会', '應' => '应', '對' => '对', '為' => '为', '無' => '无',
            '從' => '从', '來' => '来', '後' => '后', '發' => '发', '動' => '动',
            '機' => '机', '車' => '车', '號' => '号', '業' => '业', '產' => '产',
            '員' => '员', '務' => '务', '經' => '经', '濟' => '济', '場' => '场',
            '廠' => '厂', '區' => '区', '縣' => '县', '鄉' => '乡', '鎮' => '镇',
            '東' => '东', '西' => '西', '南' => '南', '北' => '北', '風' => '风',
            '雲' => '云', '雨' => '雨', '雪' => '雪', '長' => '长', '廣' => '广',
            '遠' => '远', '進' => '进', '過' => '过', '還' => '还', '運' => '运',
            '報' => '报', '紙' => '纸', '記' => '记', '誌' => '志', '網' => '网',
            '頁' => '页', '圖' => '图', '畫' => '画', '影' => '影', '聲' => '声',
            '樂' => '乐', '歌' => '歌', '藝' => '艺', '術' => '术', '體' => '体',
            '愛' => '爱', '實' => '实', '現' => '现', '夢' => '梦', '裡' => '里',
            '頭' => '头', '臉' => '脸', '眼' => '眼', '點' => '点', '線' => '线',
            '邊' => '边', '連' => '连', '錢' => '钱', '買' => '买', '賣' => '卖',
            '價' => '价', '質' => '质', '費' => '费', '級' => '级', '類' => '类',
            '種' => '种', '樣' => '样', '數' => '数', '量' => '量', '統' => '统',
            '計' => '计', '設' => '设', '備' => '备', '處' => '处', '辦' => '办',
            '總' => '总', '結' => '结', '組' => '组', '織' => '织', '係' => '系',
            '聯' => '联', '歷' => '历', '史' => '史', '認' => '认', '識' => '识',
            '證' => '证', '據' => '据', '論' => '论', '談' => '谈', '議' => '议',
            '選' => '选', '決' => '决', '權' => '权', '黨' => '党', '軍' => '军',
            '戰' => '战', '鬥' => '斗', '勝' => '胜', '敗' => '败', '條' => '条',
            '規' => '规', '則' => '则', '標' => '标', '準' => '准', '廳' => '厅',
            '館' => '馆', '樓' => '楼', '臺' => '台', '燈' => '灯', '裝' => '装',
            '雜' => '杂', '難' => '难', '專' => '专', '師' => '师', '醫' => '医',
            '藥' => '药', '導' => '导', '養' => '养', '習' => '习', '練' => '练',
            _ => c
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_chinese() {
        let tokens = tokenize("中华人民共和国");
        assert!(!tokens.is_empty());
        // jieba 会将其分词为多个词
        println!("Chinese tokens: {:?}", tokens);
    }

    #[test]
    fn test_tokenize_english() {
        let tokens = tokenize("Hello World Test");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize("测试文件 test.txt");
        println!("Mixed tokens: {:?}", tokens);
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_ngrams() {
        let ngrams = generate_ngrams("测试", 1, 2);
        assert!(ngrams.contains(&"测".to_string()));
        assert!(ngrams.contains(&"试".to_string()));
        assert!(ngrams.contains(&"测试".to_string()));
    }

    #[test]
    fn test_contains_chinese() {
        assert!(contains_chinese("测试"));
        assert!(contains_chinese("test测试"));
        assert!(!contains_chinese("test"));
    }
}
