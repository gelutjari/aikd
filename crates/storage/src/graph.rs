//! Deterministic file/symbol graph (§17) — no graph database.
//!
//! Nodes come from canonical tables (files, chunks, code_symbols).
//! Edges:
//! - `contains` (file → symbol): VERIFIED, straight from the symbol index.
//! - `imports` (file → file): parsed from `use`/`mod` (Rust) and
//!   `import`/`from` (Python) lines in chunk content. Target resolution is
//!   INFERRED (same-project path match); unresolved targets are dropped,
//!   never invented.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileGraph {
    pub files: Vec<GraphNode>,
    pub symbols: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub scope_note: String,
}

fn parse_imports(lang: &str, line: &str) -> Option<String> {
    let t = line.trim();
    if lang == "rust" {
        // `use crate::foo::bar;` | `use super::x;` | `use foo::bar;` | `mod baz;`
        if let Some(rest) = t.strip_prefix("use ") {
            let target = rest.split([';', ' ', '{', '(']).next().unwrap_or("").trim();
            if !target.is_empty() && !target.starts_with("std::") && !target.starts_with("core::") {
                return Some(target.to_string());
            }
        }
        if let Some(rest) = t.strip_prefix("mod ") {
            let target = rest.split([';', ' ', '{']).next().unwrap_or("").trim();
            if !target.is_empty() {
                return Some(target.to_string());
            }
        }
    } else if lang == "python" {
        // `import foo.bar` | `from foo import bar`
        if let Some(rest) = t.strip_prefix("import ") {
            let target = rest.split([' ', ';', ',']).next().unwrap_or("").trim();
            if !target.is_empty() && !target.starts_with('.') {
                return Some(target.to_string());
            }
        }
        if let Some(rest) = t.strip_prefix("from ") {
            let target = rest.split([' ', ';']).next().unwrap_or("").trim();
            if !target.is_empty() && !target.starts_with('.') {
                return Some(target.to_string());
            }
        }
    }
    None
}

fn lang_of(path: &str) -> &'static str {
    if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".py") {
        "python"
    } else {
        ""
    }
}

fn resolve_target(files: &[String], project: &str, lang: &str, target: &str) -> Option<String> {
    // `crate::a::b` / `a.b` → project/a/b.{rs,py} or project/a/b/mod.rs.
    let rel = if lang == "rust" {
        let t = target
            .strip_prefix("crate::")
            .or_else(|| target.strip_prefix("super::"))
            .unwrap_or(target);
        t.replace("::", "/")
    } else {
        target.replace('.', "/")
    };
    let cands = if lang == "rust" {
        vec![
            format!("{project}/{rel}.rs"),
            format!("{project}/{rel}/mod.rs"),
        ]
    } else {
        vec![
            format!("{project}/{rel}.py"),
            format!("{project}/{rel}/__init__.py"),
        ]
    };
    // Same-crate short names: match by file stem anywhere under project.
    for f in files {
        for c in &cands {
            if f == c {
                return Some(f.clone());
            }
        }
    }
    if !rel.contains('/') {
        // Single identifier: either a sibling file stem, or a root-crate
        // item (`crate::Engine` where Engine lives in lib.rs).
        let stem = if lang == "rust" {
            format!("{rel}.rs")
        } else {
            format!("{rel}.py")
        };
        let hits: Vec<&String> = files
            .iter()
            .filter(|f| f.ends_with(&format!("/{stem}")) || f.ends_with(&format!("/{rel}/mod.rs")))
            .collect();
        if hits.len() == 1 {
            return Some(hits[0].clone());
        }
        if lang == "rust" {
            for root in [
                format!("{project}/src/lib.rs"),
                format!("{project}/src/main.rs"),
                format!("{project}/lib.rs"),
                format!("{project}/main.rs"),
            ] {
                if files.iter().any(|f| f == &root) {
                    return Some(root);
                }
            }
        }
    }
    None
}

/// Build the graph for files under `project` (canonical path prefix match).
pub fn file_graph(conn: &Connection, project: &str, limit_files: usize) -> Result<FileGraph> {
    let norm = aikd_core::platform::normalize_project_key(project);
    let files: Vec<String> = if norm.is_empty() {
        Vec::new()
    } else {
        let escaped = norm
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let prefix = if norm == "/" {
            "/%".to_string()
        } else {
            format!("{escaped}/%")
        };
        conn.prepare("SELECT path FROM files WHERE status='active' AND (replace(path, '\\', '/') = ?1 OR replace(path, '\\', '/') LIKE ?2 ESCAPE '\\') ORDER BY path LIMIT ?3")
            .and_then(|mut s| {
                s.query_map(rusqlite::params![norm, prefix, limit_files as i64], |r| r.get(0))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
            })
            .unwrap_or_default()
    };
    let mut nodes = Vec::new();
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    for f in &files {
        nodes.push(GraphNode {
            id: f.clone(),
            kind: "file".into(),
            label: f.clone(),
            provenance: "VERIFIED".into(),
        });
        let syms: Vec<(String, String, String)> = conn
            .prepare("SELECT s.symbol_name, s.symbol_type, c.id FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files fl ON fl.id=c.file_id WHERE fl.path=?1 LIMIT 100")
            .and_then(|mut s| {
                s.query_map(rusqlite::params![f], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        for (name, kind, chunk) in syms {
            let id = format!("{f}#{name}");
            symbols.push(GraphNode {
                id: id.clone(),
                kind: format!("symbol:{kind}"),
                label: name,
                provenance: "VERIFIED".into(),
            });
            edges.push(GraphEdge {
                from: f.clone(),
                to: id,
                kind: "contains".into(),
                provenance: "VERIFIED".into(),
            });
            let _ = chunk;
        }
        // Import edges from chunk content (or live disk file in lean mode).
        let mut contents: Vec<String> = conn
            .prepare(
                "SELECT c.content FROM chunks c JOIN files fl ON fl.id=c.file_id WHERE fl.path=?1 AND c.content != ''",
            )
            .and_then(|mut s| {
                s.query_map(rusqlite::params![f], |r| r.get(0))
                    .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
            })
            .unwrap_or_default();
        if contents.is_empty() {
            if let Ok(disk_raw) = std::fs::read_to_string(std::path::Path::new(f)) {
                contents.push(disk_raw);
            }
        }
        let lang = lang_of(f);
        if !lang.is_empty() {
            let mut seen = std::collections::HashSet::new();
            for content in &contents {
                for line in content.lines() {
                    // Skip comments to avoid false edges.
                    let t = line.trim_start();
                    if t.starts_with("//") || t.starts_with('#') {
                        continue;
                    }
                    if let Some(target) = parse_imports(lang, line) {
                        if seen.insert(target.clone()) {
                            if let Some(to) = resolve_target(&files, project, lang, &target) {
                                if to != *f {
                                    edges.push(GraphEdge {
                                        from: f.clone(),
                                        to,
                                        kind: "imports".into(),
                                        provenance: "INFERRED".into(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(FileGraph {
        files: nodes,
        symbols,
        edges,
        scope_note: "contains=VERIFIED from symbol index; imports=parsed use/mod/import lines (Rust/Python), target resolution INFERRED, unresolved dropped".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/src/main.rs', 50, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/src/tax.rs', 50, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();
        let fid: i64 = conn
            .query_row(
                "SELECT id FROM files WHERE path='/p/src/main.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 0, '', 1, 5, 'use crate::tax;\nfn main() {}', '{}', 't', 't')",
            rusqlite::params![fid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_symbols (id, chunk_id, symbol_type, symbol_name, signature, visibility, language, line_start, line_end, created_at) VALUES ('s1', 'c1', 'function', 'main', 'fn main()', 'private', 'rust', 2, 2, 't')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn contains_verified_imports_resolved() {
        let conn = setup();
        let g = file_graph(&conn, "/p", 50).unwrap();
        assert_eq!(g.files.len(), 2);
        assert!(g
            .edges
            .iter()
            .any(|e| e.kind == "contains" && e.provenance == "VERIFIED"));
        let imp: Vec<&GraphEdge> = g.edges.iter().filter(|e| e.kind == "imports").collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].from, "/p/src/main.rs");
        assert_eq!(imp[0].to, "/p/src/tax.rs");
        assert_eq!(imp[0].provenance, "INFERRED");
    }

    #[test]
    fn unresolved_imports_dropped_not_invented() {
        let conn = setup();
        conn.execute(
            "UPDATE chunks SET content='use serde::Deserialize;\nuse std::io;' WHERE id='c1'",
            [],
        )
        .unwrap();
        let g = file_graph(&conn, "/p", 50).unwrap();
        assert!(g.edges.iter().all(|e| e.kind != "imports"));
    }
}
