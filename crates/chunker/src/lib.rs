pub mod code;
pub mod markdown;

use std::collections::HashMap;
use aikd_core::Chunk;

pub fn chunk_file(
    file_path: &str,
    content: &str,
    max_tokens: usize,
    min_tokens: usize,
) -> Vec<Chunk> {
    let config = ChunkConfig { max_tokens, min_tokens, overlap_tokens: 0 };
    match detect_file_type(file_path) {
        FileType::Markdown => markdown::chunk_markdown(file_path, content, max_tokens, min_tokens),
        FileType::Text => chunk_plain_text(file_path, content, max_tokens),
        FileType::Structured => chunk_structured(file_path, content),
        FileType::SourceCode => code::chunk_source_code(file_path, content, &config, HashMap::new()),
    }
}

pub struct ChunkConfig {
    pub max_tokens: usize,
    pub min_tokens: usize,
    pub overlap_tokens: usize,
}

enum FileType {
    Markdown,
    Text,
    Structured,
    SourceCode,
}

fn detect_file_type(path: &str) -> FileType {
    if path.ends_with(".md") || path.ends_with(".markdown") {
        FileType::Markdown
    } else if path.ends_with(".json") || path.ends_with(".jsonl") || path.ends_with(".yaml") || path.ends_with(".yml") || path.ends_with(".toml") {
        FileType::Structured
    } else if path.ends_with(".rs") || path.ends_with(".py") || path.ends_with(".ts")
        || path.ends_with(".tsx") || path.ends_with(".js") || path.ends_with(".jsx")
        || path.ends_with(".go") || path.ends_with(".java") || path.ends_with(".c")
        || path.ends_with(".cpp") || path.ends_with(".h") || path.ends_with(".hpp")
    {
        FileType::SourceCode
    } else {
        FileType::Text
    }
}

fn chunk_plain_text(file_path: &str, content: &str, max_tokens: usize) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_content = String::new();
    let mut line_start = 1;
    let mut chunk_index = 0;

    for (i, line) in content.lines().enumerate() {
        let line_num = i + 1;
        current_content.push_str(line);
        current_content.push('\n');

        let est = current_content.len() / 4;
        if est >= max_tokens {
            chunks.push(Chunk {
                id: uuid::Uuid::new_v4().to_string(),
                file_path: file_path.to_string(),
                chunk_index,
                heading_hierarchy: vec![],
                heading_level: 0,
                heading_text: String::new(),
                line_start,
                line_end: line_num,
                content: current_content.trim().to_string(),
                metadata: HashMap::new(),
            });
            chunk_index += 1;
            current_content.clear();
            line_start = line_num + 1;
        }
    }

    if !current_content.trim().is_empty() {
        chunks.push(Chunk {
            id: uuid::Uuid::new_v4().to_string(),
            file_path: file_path.to_string(),
            chunk_index,
            heading_hierarchy: vec![],
            heading_level: 0,
            heading_text: String::new(),
            line_start,
            line_end: content.lines().count(),
            content: current_content.trim().to_string(),
            metadata: HashMap::new(),
        });
    }

    chunks
}

fn chunk_structured(file_path: &str, content: &str) -> Vec<Chunk> {
    vec![Chunk {
        id: uuid::Uuid::new_v4().to_string(),
        file_path: file_path.to_string(),
        chunk_index: 0,
        heading_hierarchy: vec![],
        heading_level: 0,
        heading_text: String::new(),
        line_start: 1,
        line_end: content.lines().count(),
        content: content.to_string(),
        metadata: HashMap::new(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_plain_text() {
        let content = "Line 1\nLine 2\nLine 3\n";
        let chunks = chunk_plain_text("test.txt", content, 100);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].file_path, "test.txt");
    }

    #[test]
    fn test_chunk_structured() {
        let content = "{\"key\": \"value\"}";
        let chunks = chunk_structured("test.json", content);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, content);
    }

    #[test]
    fn test_chunk_file_markdown() {
        let content = "# Title\n\nSome content here.\n\n## Section\n\nMore content.";
        let chunks = chunk_file("test.md", content, 1000, 10);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_detect_file_type() {
        assert!(matches!(detect_file_type("test.md"), FileType::Markdown));
        assert!(matches!(detect_file_type("test.json"), FileType::Structured));
        assert!(matches!(detect_file_type("test.txt"), FileType::Text));
        assert!(matches!(detect_file_type("test.rs"), FileType::SourceCode));
        assert!(matches!(detect_file_type("test.py"), FileType::SourceCode));
        assert!(matches!(detect_file_type("test.ts"), FileType::SourceCode));
    }

    #[test]
    fn test_chunk_ids_unique() {
        let content = "# Title\n\nContent 1\n\n## Section\n\nContent 2";
        let chunks = chunk_file("test.md", content, 1000, 10);
        let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
        let unique_ids: std::collections::HashSet<&str> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }
}
