use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub file_path: String,
    pub chunk_index: usize,
    pub heading_hierarchy: Vec<String>,
    pub heading_level: usize,
    pub heading_text: String,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl Chunk {
    pub fn heading_hierarchy_str(&self) -> String {
        self.heading_hierarchy.join(" > ")
    }

    pub fn token_estimate(&self) -> usize {
        estimate_tokens(&self.content)
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let cjk_bytes: usize = text
        .chars()
        .filter(|c| {
            ('\u{4E00}'..='\u{9FFF}').contains(c)
                || ('\u{3040}'..='\u{30FF}').contains(c)
                || ('\u{AC00}'..='\u{D7AF}').contains(c)
                || ('\u{20000}'..='\u{2A6DF}').contains(c) // CJK Unified Ideographs Extension B
                || ('\u{2A700}'..='\u{2B73F}').contains(c) // CJK Unified Ideographs Extension C
        })
        .map(|c| c.len_utf8())
        .sum();
    let non_cjk_bytes = text.len().saturating_sub(cjk_bytes);
    (non_cjk_bytes / 4) + (cjk_bytes * 2 / 3)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub heading_hierarchy: String,
    pub heading_text: String,
    pub content: String,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub path_contains: Option<String>,
    pub path_exclude: Option<String>,
    pub file_types: Option<Vec<String>>,
    pub heading_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub last_active: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tokens: usize,
    pub chunk_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEmbedding {
    pub conversation_id: String,
    pub model: String,
    pub dimensions: usize,
    pub vector: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_heading_hierarchy_str() {
        let chunk = Chunk {
            id: "test".into(),
            file_path: "test.rs".into(),
            chunk_index: 0,
            heading_hierarchy: vec!["API".into(), "REST".into()],
            heading_level: 2,
            heading_text: "REST".into(),
            line_start: 1,
            line_end: 10,
            content: "test".into(),
            metadata: std::collections::HashMap::new(),
        };
        assert_eq!(chunk.heading_hierarchy_str(), "API > REST");
    }

    #[test]
    fn test_chunk_token_estimate_ascii() {
        let chunk = Chunk {
            id: "test".into(),
            file_path: "test.txt".into(),
            chunk_index: 0,
            heading_hierarchy: vec![],
            heading_level: 0,
            heading_text: String::new(),
            line_start: 1,
            line_end: 1,
            content: "hello world test".into(),
            metadata: std::collections::HashMap::new(),
        };
        assert_eq!(chunk.token_estimate(), 4);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        let tokens = estimate_tokens("hello world");
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        let tokens = estimate_tokens("你好世界");
        assert!(tokens >= 8);
    }

    #[test]
    fn test_search_filters_default() {
        let filters = SearchFilters::default();
        assert!(filters.path_contains.is_none());
        assert!(filters.heading_contains.is_none());
    }

    #[test]
    fn test_session_fields() {
        let s = Session {
            id: "s1".into(),
            name: "test".into(),
            project_path: "/project".into(),
            created_at: "2026-01-01".into(),
            last_active: "2026-01-01".into(),
        };
        assert_eq!(s.id, "s1");
    }

    #[test]
    fn test_conversation_fields() {
        let c = Conversation {
            id: "c1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "hello".into(),
            tokens: 5,
            chunk_refs: vec![],
            created_at: "2026-01-01".into(),
        };
        assert_eq!(c.role, "user");
    }
}
