use std::collections::HashMap;
use serde_json::Value;
use aikd_core::Chunk;
use crate::ChunkConfig;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceLanguage {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Unknown,
}

pub fn detect_language(path: &str) -> SourceLanguage {
    if path.ends_with(".rs") {
        SourceLanguage::Rust
    } else if path.ends_with(".py") || path.ends_with(".pyi") {
        SourceLanguage::Python
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        SourceLanguage::TypeScript
    } else if path.ends_with(".js") || path.ends_with(".jsx") || path.ends_with(".mjs") {
        SourceLanguage::JavaScript
    } else if path.ends_with(".go") {
        SourceLanguage::Go
    } else {
        SourceLanguage::Unknown
    }
}

pub fn chunk_source_code(
    file_path: &str,
    content: &str,
    config: &ChunkConfig,
    metadata: HashMap<String, Value>,
) -> Vec<Chunk> {
    let lang = detect_language(file_path);
    match lang {
        SourceLanguage::Rust => chunk_by_function(content, file_path, config, metadata, &["fn ", "struct ", "impl ", "trait ", "enum ", "pub fn ", "pub struct ", "pub trait ", "pub enum "]),
        SourceLanguage::Python => chunk_by_function(content, file_path, config, metadata, &["def ", "class ", "async def "]),
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => chunk_by_function(content, file_path, config, metadata, &["function ", "export function ", "export default function ", "const ", "export const ", "class ", "export class ", "async function "]),
        SourceLanguage::Go => chunk_by_function(content, file_path, config, metadata, &["func ", "type ", "func ("]),
        SourceLanguage::Unknown => chunk_by_lines(content, file_path, config, metadata),
    }
}

fn chunk_by_function(
    content: &str,
    file_path: &str,
    config: &ChunkConfig,
    metadata: HashMap<String, Value>,
    patterns: &[&str],
) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut current_start = 0;
    let mut current_content = String::new();
    let mut chunk_index = 0;
    let mut current_symbol = String::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let is_symbol = patterns.iter().any(|p| trimmed.starts_with(p));

        if is_symbol && !current_content.trim().is_empty() {
            let est = current_content.len() / 4;
            if est >= config.min_tokens {
                chunks.push(make_code_chunk(
                    file_path, chunk_index, &current_symbol,
                    current_start + 1, i, &current_content, &metadata,
                ));
                chunk_index += 1;
            }
            current_content.clear();
            current_start = i;
            current_symbol = trimmed.to_string();
        } else if is_symbol && current_content.trim().is_empty() {
            current_start = i;
            current_symbol = trimmed.to_string();
        }

        current_content.push_str(line);
        current_content.push('\n');

        let est = current_content.len() / 4;
        if est >= config.max_tokens {
            chunks.push(make_code_chunk(
                file_path, chunk_index, &current_symbol,
                current_start + 1, i + 1, &current_content, &metadata,
            ));
            chunk_index += 1;
            current_content.clear();
            current_start = i + 1;
            current_symbol = String::new();
        }
    }

    if !current_content.trim().is_empty() {
        chunks.push(make_code_chunk(
            file_path, chunk_index, &current_symbol,
            current_start + 1, lines.len(), &current_content, &metadata,
        ));
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        chunks.push(make_code_chunk(
            file_path, 0, "",
            1, lines.len(), content, &metadata,
        ));
    }

    chunks
}

fn chunk_by_lines(
    content: &str,
    file_path: &str,
    config: &ChunkConfig,
    metadata: HashMap<String, Value>,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_content = String::new();
    let mut line_start = 1;
    let mut chunk_index = 0;

    for (i, line) in content.lines().enumerate() {
        let line_num = i + 1;
        current_content.push_str(line);
        current_content.push('\n');

        let est = current_content.len() / 4;
        if est >= config.max_tokens {
            chunks.push(make_code_chunk(
                file_path, chunk_index, "",
                line_start, line_num, &current_content, &metadata,
            ));
            chunk_index += 1;
            current_content.clear();
            line_start = line_num + 1;
        }
    }

    if !current_content.trim().is_empty() {
        chunks.push(make_code_chunk(
            file_path, chunk_index, "",
            line_start, content.lines().count(), &current_content, &metadata,
        ));
    }

    chunks
}

fn make_code_chunk(
    file_path: &str,
    chunk_index: usize,
    symbol: &str,
    line_start: usize,
    line_end: usize,
    content: &str,
    metadata: &HashMap<String, Value>,
) -> Chunk {
    let line_end = if line_end < line_start { line_start } else { line_end };
    let heading = if symbol.is_empty() {
        String::new()
    } else {
        symbol.split('{').next().unwrap_or(symbol).trim().to_string()
    };

    Chunk {
        id: uuid::Uuid::new_v4().to_string(),
        file_path: file_path.to_string(),
        chunk_index,
        heading_hierarchy: if heading.is_empty() { vec![] } else { vec![heading.clone()] },
        heading_level: 0,
        heading_text: heading,
        line_start,
        line_end,
        content: content.trim().to_string(),
        metadata: metadata.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkConfig;

    fn test_config() -> ChunkConfig {
        ChunkConfig {
            max_tokens: 1000,
            min_tokens: 10,
            overlap_tokens: 0,
        }
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("main.rs"), SourceLanguage::Rust);
        assert_eq!(detect_language("app.py"), SourceLanguage::Python);
        assert_eq!(detect_language("index.ts"), SourceLanguage::TypeScript);
        assert_eq!(detect_language("main.go"), SourceLanguage::Go);
        assert_eq!(detect_language("readme.md"), SourceLanguage::Unknown);
    }

    #[test]
    fn test_chunk_rust_functions() {
        let content = r#"fn main() {
    println!("hello");
}

fn helper() {
    println!("world");
}
"#;
        let chunks = chunk_source_code("test.rs", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_python_functions() {
        let content = r#"def hello():
    print("hello")

def world():
    print("world")
"#;
        let chunks = chunk_source_code("test.py", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_unknown_fallback() {
        let content = "some random text\nwith multiple lines\n";
        let chunks = chunk_source_code("test.xyz", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_go_functions() {
        let content = r#"package main

func main() {
    fmt.Println("hello")
}

func helper() int {
    return 42
}
"#;
        let chunks = chunk_source_code("main.go", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_javascript() {
        let content = r#"function hello() {
    console.log("hello");
}

const world = () => {
    console.log("world");
}
"#;
        let chunks = chunk_source_code("app.js", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_empty_content() {
        let chunks = chunk_source_code("test.rs", "", &test_config(), HashMap::new());
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_chunk_single_function() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let chunks = chunk_source_code("test.rs", content, &test_config(), HashMap::new());
        assert!(chunks.len() >= 1);
        assert!(chunks[0].content.contains("fn main"));
    }
}
