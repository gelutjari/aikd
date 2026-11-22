//! Heuristic symbol extraction from code chunk headings.
//!
//! Provenance: EXTRACTED. These are syntax-prefix heuristics (the same
//! ones the chunker uses), NOT resolved AST facts. Callers must present
//! them as such; no caller/callee resolution happens here.

use crate::code::{detect_language, SourceLanguage};
use aikd_core::{Chunk, CodeSymbol};

fn language_name(lang: SourceLanguage) -> Option<&'static str> {
    match lang {
        SourceLanguage::Rust => Some("rust"),
        SourceLanguage::Python => Some("python"),
        SourceLanguage::TypeScript => Some("typescript"),
        SourceLanguage::JavaScript => Some("javascript"),
        SourceLanguage::Go => Some("go"),
        SourceLanguage::CSharp => Some("csharp"),
        SourceLanguage::Unknown => None,
    }
}

/// Clean an identifier token: keep leading [A-Za-z0-9_] characters.
fn ident(token: &str) -> String {
    token
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Parse a declaration line like `pub fn calculate_tax(x: f64) -> f64`
/// into (kind, name, visibility). The kind keyword must appear within the
/// first 4 tokens so mid-line words (e.g. a type named `type` in an
/// expression) are not misread as declarations.
fn parse_heading(heading: &str, lang: SourceLanguage) -> Option<(String, String, String)> {
    let words: Vec<&str> = heading.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let visibility = if words[0] == "pub" || words[0] == "public" || words.contains(&"export") {
        "public"
    } else {
        "private"
    };
    match lang {
        SourceLanguage::Rust => {
            let kinds = [
                "fn", "struct", "enum", "trait", "impl", "const", "static", "type", "mod",
            ];
            let pos = words.iter().position(|w| kinds.contains(w))?;
            if pos > 3 {
                return None;
            }
            let kind = words[pos];
            let name = ident(words.get(pos + 1)?);
            if name.is_empty() {
                return None;
            }
            let kind = match kind {
                "fn" => "function",
                "const" | "static" => "constant",
                "mod" => "module",
                other => other,
            };
            Some((kind.to_string(), name, visibility.to_string()))
        }
        SourceLanguage::Python => {
            let pos = words.iter().position(|w| *w == "def" || *w == "class")?;
            if pos > 3 {
                return None;
            }
            let kind = if words[pos] == "def" {
                "function"
            } else {
                "class"
            };
            let name = ident(words.get(pos + 1)?);
            if name.is_empty() {
                return None;
            }
            Some((kind.to_string(), name, visibility.to_string()))
        }
        SourceLanguage::TypeScript | SourceLanguage::JavaScript => {
            if let Some(pos) = words.iter().position(|w| *w == "function" || *w == "class") {
                if pos > 3 {
                    return None;
                }
                let kind = if words[pos] == "function" {
                    "function"
                } else {
                    "class"
                };
                let name = ident(words.get(pos + 1)?);
                if name.is_empty() {
                    return None;
                }
                return Some((kind.to_string(), name, visibility.to_string()));
            }
            if let Some(pos) = words
                .iter()
                .position(|w| *w == "const" || *w == "let" || *w == "var")
            {
                if pos > 3 {
                    return None;
                }
                let name = ident(words.get(pos + 1)?);
                if name.is_empty() {
                    return None;
                }
                return Some(("constant".to_string(), name, visibility.to_string()));
            }
            None
        }
        SourceLanguage::Go => {
            if let Some(pos) = words.iter().position(|w| *w == "func") {
                if pos > 3 {
                    return None;
                }
                // Method: `func (r *Receiver) Name(...)` — name follows ')'.
                let name = if let Some(close) = words.iter().position(|w| w.ends_with(')')) {
                    ident(words.get(close + 1).unwrap_or(&""))
                } else {
                    ident(words.get(pos + 1).unwrap_or(&""))
                };
                if name.is_empty() {
                    return None;
                }
                return Some(("function".to_string(), name, visibility.to_string()));
            }
            if let Some(pos) = words.iter().position(|w| *w == "type") {
                if pos > 3 {
                    return None;
                }
                let name = ident(words.get(pos + 1)?);
                if name.is_empty() {
                    return None;
                }
                return Some(("type".to_string(), name, visibility.to_string()));
            }
            None
        }
        SourceLanguage::CSharp => {
            let kinds = ["class", "struct", "interface", "enum", "record"];
            if let Some(pos) = words.iter().position(|w| kinds.contains(w)) {
                if pos > 3 {
                    return None;
                }
                let name = ident(words.get(pos + 1)?);
                if name.is_empty() {
                    return None;
                }
                return Some((words[pos].to_string(), name, visibility.to_string()));
            }
            // Method declarations with explicit access modifier: `public static int Foo(...)`
            if matches!(words[0], "public" | "private" | "internal" | "protected")
                && heading.contains('(')
            {
                for w in &words[1..] {
                    if let Some(prefix) = w.split('(').next() {
                        if w.contains('(') {
                            let name = ident(prefix);
                            if !name.is_empty() {
                                return Some((
                                    "function".to_string(),
                                    name,
                                    visibility.to_string(),
                                ));
                            }
                        }
                    }
                }
            }
            None
        }
        SourceLanguage::Unknown => None,
    }
}

/// Extract symbols from the content lines of each code chunk. Merged chunks
/// carry only their first symbol in the heading, so every declaration line
/// is scanned; absolute line numbers are chunk start + line offset.
/// Non-code files yield nothing.
pub fn extract_symbols(file_path: &str, chunks: &[Chunk]) -> Vec<CodeSymbol> {
    let lang = detect_language(file_path);
    let Some(lang_name) = language_name(lang) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in chunks {
        for (i, line) in c.content.lines().enumerate() {
            let trimmed = line.trim_start();
            // Skip comments and attributes to cut obvious false positives.
            if trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("--")
                || trimmed.starts_with('*')
                || trimmed.starts_with("/*")
                || trimmed.starts_with("#[")
                || trimmed.starts_with('@')
            {
                continue;
            }
            if let Some((kind, name, visibility)) = parse_heading(trimmed, lang) {
                out.push(CodeSymbol {
                    name,
                    kind,
                    signature: trimmed.to_string(),
                    visibility,
                    language: lang_name.to_string(),
                    file_path: file_path.to_string(),
                    line_start: c.line_start + i,
                    line_end: c.line_start + i,
                    chunk_id: c.id.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn chunk(content: &str) -> Chunk {
        Chunk {
            id: "id".into(),
            file_path: "t.rs".into(),
            chunk_index: 0,
            heading_hierarchy: vec![],
            heading_level: 0,
            heading_text: String::new(),
            line_start: 10,
            line_end: 10 + content.lines().count(),
            content: content.into(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn rust_symbols_with_line_numbers() {
        let body = "pub fn calculate_tax(amount: f64) -> f64 {\n    amount * 0.11\n}\n\nstruct Vehicle {\n    plate: String,\n}\n\nimpl Vehicle {\n    pub fn new() -> Self {\n        Self\n    }\n}\n";
        let syms = extract_symbols("t.rs", &[chunk(body)]);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["calculate_tax", "Vehicle", "Vehicle", "new"]);
        assert_eq!(syms[0].line_start, 10);
        assert_eq!(syms[0].kind, "function");
        assert_eq!(syms[0].visibility, "public");
        assert_eq!(syms[1].kind, "struct");
        assert_eq!(syms[2].kind, "impl");
        // All declarations incl. merged ones carry absolute lines.
        assert!(syms.iter().all(|s| s.line_start >= 10));
    }

    #[test]
    fn python_and_go_and_js() {
        let py = extract_symbols(
            "a.py",
            &[chunk(
                "def process(self):\n    pass\n\nclass Foo:\n    pass",
            )],
        );
        assert_eq!(py[0].name, "process");
        assert_eq!(py[1].kind, "class");
        let go = extract_symbols(
            "a.go",
            &[chunk(
                "func (s *Server) Start() {\n}\n\ntype Config struct {\n}",
            )],
        );
        assert_eq!(go[0].name, "Start");
        assert_eq!(go[1].name, "Config");
        let js = extract_symbols(
            "a.ts",
            &[chunk(
                "export function handle(x) {\n}\n\nexport const router = express()",
            )],
        );
        assert_eq!(js[0].name, "handle");
        assert_eq!(js[0].visibility, "public");
        assert_eq!(js[1].name, "router");
    }

    #[test]
    fn comments_and_noise_ignored() {
        let body = "// fn not_a_symbol()\n# def nope():\nlet x = 1;\nfn real_one() {\n}\n";
        let syms = extract_symbols("t.rs", &[chunk(body)]);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "real_one");
    }

    #[test]
    fn non_code_yields_nothing() {
        assert!(extract_symbols("a.md", &[chunk("# Title")]).is_empty());
        assert!(extract_symbols("a.rs", &[chunk("")]).is_empty());
    }

    #[test]
    fn csharp_and_cjs_symbols() {
        let cs = extract_symbols(
            "Service.cs",
            &[chunk("public class OrderService {\n    public static int Compute(int x) { return x; }\n}\n")],
        );
        assert!(cs
            .iter()
            .any(|s| s.name == "OrderService" && s.kind == "class" && s.language == "csharp"));
        assert!(cs
            .iter()
            .any(|s| s.name == "Compute" && s.kind == "function"));

        let cjs = extract_symbols("mod.cjs", &[chunk("function greet() {}\n")]);
        assert!(cjs
            .iter()
            .any(|s| s.name == "greet" && s.language == "javascript"));
    }
}
