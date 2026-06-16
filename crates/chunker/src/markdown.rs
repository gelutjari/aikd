use aikd_core::Chunk;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::Value;
use std::collections::HashMap;

pub fn chunk_markdown(
    file_path: &str,
    content: &str,
    max_tokens: usize,
    min_tokens: usize,
) -> Vec<Chunk> {
    let metadata = extract_frontmatter(content);
    let body = strip_frontmatter(content);

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut current_content = String::new();
    let mut current_heading_text = String::new();
    let mut in_heading = false;
    let mut current_heading_level: usize = 0;
    let mut chunk_index = 0;
    let mut section_start_line = 1;
    let mut line_counter = 0;

    let options = Options::ENABLE_HEADING_ATTRIBUTES | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(body, options);

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if !current_content.trim().is_empty() {
                    let est = current_content.len() / 4;
                    if est >= min_tokens {
                        chunks.push(make_chunk(
                            file_path,
                            chunk_index,
                            &heading_stack,
                            section_start_line,
                            line_counter,
                            &current_content,
                            &metadata,
                        ));
                        chunk_index += 1;
                    }
                }
                current_content.clear();
                current_heading_text.clear();
                section_start_line = line_counter + 1;
                in_heading = true;

                let level_num = heading_level_num(level);
                current_heading_level = level_num;
                heading_stack.retain(|(l, _)| *l < level_num);
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                if !current_heading_text.is_empty() {
                    heading_stack.push((current_heading_level, current_heading_text.clone()));
                }
            }
            Event::Text(text) => {
                if in_heading {
                    current_heading_text.push_str(&text);
                } else {
                    current_content.push_str(&text);
                }
                for ch in text.chars() {
                    if ch == '\n' {
                        line_counter += 1;
                    }
                }
            }
            Event::Code(code) if !in_heading => {
                current_content.push('`');
                current_content.push_str(&code);
                current_content.push('`');
            }
            Event::Start(Tag::List(_)) => {}
            Event::Start(Tag::Item) => {
                current_content.push_str("- ");
            }
            Event::End(TagEnd::Item) => {
                current_content.push('\n');
            }
            Event::SoftBreak | Event::HardBreak => {
                current_content.push('\n');
                line_counter += 1;
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                current_content.push('\n');
            }
            _ => {}
        }

        let est = current_content.len() / 4;
        if est >= max_tokens {
            chunks.push(make_chunk(
                file_path,
                chunk_index,
                &heading_stack,
                section_start_line,
                line_counter,
                &current_content,
                &metadata,
            ));
            chunk_index += 1;
            current_content.clear();
            section_start_line = line_counter + 1;
        }
    }

    if !current_content.trim().is_empty() {
        let est = current_content.len() / 4;
        if est >= min_tokens || chunks.is_empty() {
            chunks.push(make_chunk(
                file_path,
                chunk_index,
                &heading_stack,
                section_start_line,
                line_counter,
                &current_content,
                &metadata,
            ));
        } else if let Some(last) = chunks.last_mut() {
            last.content.push_str("\n\n");
            last.content.push_str(current_content.trim());
            last.line_end = line_counter;
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        chunks.push(Chunk {
            id: uuid::Uuid::new_v4().to_string(),
            file_path: file_path.to_string(),
            chunk_index: 0,
            heading_hierarchy: vec![],
            heading_level: 0,
            heading_text: String::new(),
            line_start: 1,
            line_end: content.lines().count(),
            content: content.trim().to_string(),
            metadata: metadata.clone(),
        });
    }

    chunks
}

fn heading_level_num(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn make_chunk(
    file_path: &str,
    chunk_index: usize,
    heading_stack: &[(usize, String)],
    line_start: usize,
    line_end: usize,
    content: &str,
    metadata: &HashMap<String, Value>,
) -> Chunk {
    let hierarchy: Vec<String> = heading_stack.iter().map(|(_, t)| t.clone()).collect();
    let heading_level = heading_stack.last().map_or(0, |(l, _)| *l);
    let heading_text = heading_stack
        .last()
        .map_or(String::new(), |(_, t)| t.clone());
    let line_end = if line_end < line_start {
        line_start
    } else {
        line_end
    };

    Chunk {
        id: uuid::Uuid::new_v4().to_string(),
        file_path: file_path.to_string(),
        chunk_index,
        heading_hierarchy: hierarchy,
        heading_level,
        heading_text,
        line_start,
        line_end,
        content: content.trim().to_string(),
        metadata: metadata.clone(),
    }
}

fn extract_frontmatter(content: &str) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();

    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            let fm = &rest[..end];
            if let Ok(serde_json::Value::Object(map)) =
                serde_yaml::from_str::<serde_json::Value>(fm)
            {
                for (k, v) in map {
                    metadata.insert(k, v);
                }
            }
        }
    }

    metadata
}

fn strip_frontmatter(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            return rest[end + 3..].trim_start();
        }
    }
    content
}
