//! Deterministic knowledge graph (§17 + v2.1) — no graph database.
//!
//! Nodes come from canonical tables: `files`, `code_symbols`, `notes`,
//! `memory`, `active_clients` and derived `tag` / `project` nodes.
//!
//! Edges:
//! - `contains` (file → symbol): VERIFIED, straight from the symbol index.
//! - `imports` (file → file): parsed from Rust/Python/TS/JS/Dart/Go source.
//!   Target resolution is INFERRED (same-project path match); unresolved
//!   targets are dropped, never invented.
//! - `tag` / `wikilink` / `mentions`: EXTRACTED from `knowledge_links` and
//!   note/memory bodies.
//!
//! `knowledge_graph` returns the unified `nodes` + `edges` arrays while still
//! filling the legacy `files` / `symbols` / `edges` / `scope_note` fields so
//! older clients keep working.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default)]
    pub degree: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileGraph {
    /// Legacy file nodes (raw path ids) — kept for backward compatibility.
    pub files: Vec<GraphNode>,
    /// Legacy symbol nodes (raw path#name ids).
    pub symbols: Vec<GraphNode>,
    /// Unified edges (prefixed node ids).
    pub edges: Vec<GraphEdge>,
    pub scope_note: String,
    /// Unified nodes for every kind (project/file/symbol/note/memory/tag/agent).
    pub nodes: Vec<GraphNode>,
    pub stats: GraphStats,
}

/// Graph request options (`GET /api/graph`).
#[derive(Debug, Clone)]
pub struct GraphOptions {
    pub project: String,
    /// `global` (default) or `local`.
    pub mode: String,
    /// `node_id` to center a local graph around.
    pub focus: Option<String>,
    /// 1 or 2 (BFS hops for local mode).
    pub depth: usize,
    /// Optional kind filter (`project,file,symbol,note,memory,tag,agent`).
    pub types: Option<Vec<String>>,
    pub hide_orphans: bool,
    pub limit: usize,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            project: String::new(),
            mode: "global".to_string(),
            focus: None,
            depth: 1,
            types: None,
            hide_orphans: false,
            limit: 120,
        }
    }
}

fn node(id: String, kind: &str, label: String, provenance: &str) -> GraphNode {
    GraphNode {
        id,
        kind: kind.to_string(),
        label,
        provenance: provenance.to_string(),
        subtype: None,
        project: None,
        scope: None,
        degree: 0,
    }
}

fn push_edge(edges: &mut Vec<GraphEdge>, from: String, to: String, kind: &str, provenance: &str) {
    if from != to {
        edges.push(GraphEdge {
            from,
            to,
            kind: kind.to_string(),
            provenance: provenance.to_string(),
        });
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) if i > 0 => path[..i].to_string(),
        Some(_) => "/".to_string(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Import parsing (Rust / Python / TS / JS / Dart / Go)
// ---------------------------------------------------------------------------

fn lang_of(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        "rust"
    } else if lower.ends_with(".py") {
        "python"
    } else if lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".mjs")
        || lower.ends_with(".cjs")
        || lower.ends_with(".vue")
        || lower.ends_with(".svelte")
    {
        "js"
    } else if lower.ends_with(".dart") {
        "dart"
    } else if lower.ends_with(".go") {
        "go"
    } else {
        ""
    }
}

fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".mdx") || lower.ends_with(".markdown")
}

/// First quoted string in a line (`'x'` or `"x"`).
fn quoted(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j < bytes.len() && j > start {
                return Some(s[start..j].to_string());
            }
        }
        i += 1;
    }
    None
}

fn parse_imports(lang: &str, line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    match lang {
        "rust" => {
            if let Some(rest) = t.strip_prefix("use ") {
                let target = rest.split([';', ' ', '{', '(']).next().unwrap_or("").trim();
                if !target.is_empty()
                    && !target.starts_with("std::")
                    && !target.starts_with("core::")
                {
                    return Some(target.to_string());
                }
            }
            if let Some(rest) = t.strip_prefix("mod ") {
                let target = rest.split([';', ' ', '{']).next().unwrap_or("").trim();
                if !target.is_empty() {
                    return Some(target.to_string());
                }
            }
            None
        }
        "python" => {
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
            None
        }
        "js" => {
            let is_module_stmt = t.starts_with("import ")
                || t.starts_with("export ")
                || t.starts_with("const ")
                || t.starts_with("let ")
                || t.starts_with("var ");
            if is_module_stmt {
                if let Some(from_idx) = t.find(" from ") {
                    let tail = &t[from_idx + 6..];
                    if let Some(target) = quoted(tail) {
                        return Some(target);
                    }
                }
                if t.starts_with("import ") || t.starts_with("export ") {
                    if let Some(target) = quoted(t) {
                        return Some(target);
                    }
                }
            }
            if let Some(idx) = t.find("require(") {
                if let Some(target) = quoted(&t[idx..]) {
                    return Some(target);
                }
            }
            None
        }
        "dart" => {
            if t.starts_with("import ") || t.starts_with("export ") || t.starts_with("part ") {
                return quoted(t);
            }
            None
        }
        "go" => {
            if let Some(rest) = t.strip_prefix("import ") {
                return quoted(rest);
            }
            // Inside a grouped `import ( ... )` block: `"fmt"` lines.
            if t.starts_with('"') {
                return quoted(t);
            }
            None
        }
        _ => None,
    }
}

/// Join `target` onto `base_dir`, resolving `.` and `..` without touching disk.
fn join_path(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn candidates_for(lang: &str, base: &str) -> Vec<String> {
    match lang {
        "rust" => vec![format!("{base}.rs"), format!("{base}/mod.rs")],
        "python" => vec![format!("{base}.py"), format!("{base}/__init__.py")],
        "js" => {
            let exts = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte"];
            let mut out: Vec<String> = exts.iter().map(|e| format!("{base}.{e}")).collect();
            out.extend(exts.iter().map(|e| format!("{base}/index.{e}")));
            out
        }
        "dart" => vec![format!("{base}.dart")],
        "go" => vec![format!("{base}.go")],
        _ => Vec::new(),
    }
}

fn resolve_target(
    files: &[String],
    project: &str,
    lang: &str,
    source: &str,
    target: &str,
) -> Option<String> {
    let file_set: HashSet<&str> = files.iter().map(|s| s.as_str()).collect();

    // Relative imports resolve against the importing file's directory.
    if target.starts_with('.') {
        let joined = join_path(&parent_dir(source), target);
        for c in candidates_for(lang, &joined) {
            if file_set.contains(c.as_str()) {
                return Some(c);
            }
        }
        return None;
    }

    // Dart package imports: `package:flutter/material.dart` → lib/material.dart
    if let Some(rest) = target.strip_prefix("package:") {
        let rel = rest.split_once('/').map(|(_, r)| r).unwrap_or(rest);
        let needle = format!("/{rel}");
        let mut hits: Vec<&String> = files.iter().filter(|f| f.ends_with(&needle)).collect();
        hits.sort();
        return hits.first().map(|f| (*f).clone());
    }

    // Go package imports: resolve to the first file of that package directory.
    if lang == "go" {
        let last = target.rsplit('/').next().unwrap_or(target);
        let mut hits: Vec<&String> = files
            .iter()
            .filter(|f| f.ends_with(&format!("/{last}.go")) || f.contains(&format!("/{last}/")))
            .collect();
        hits.sort();
        if let Some(first) = hits.first() {
            return Some((*first).clone());
        }
        return None;
    }

    // Rust / Python / JS: project-root resolution.
    let rel = match lang {
        "rust" => target
            .strip_prefix("crate::")
            .or_else(|| target.strip_prefix("super::"))
            .unwrap_or(target)
            .replace("::", "/"),
        "python" => target.replace('.', "/"),
        _ => target.replace('\\', "/"),
    };
    for c in candidates_for(lang, &format!("{project}/{rel}")) {
        if file_set.contains(c.as_str()) {
            return Some(c);
        }
    }

    // Workspace-aware fallback: the crate root is often deeper than the
    // project root (`project/crates/storage/src/graph.rs`) and `use a::b::c`
    // often ends in a symbol, not a module. Try longest→shortest path
    // prefixes, mapping `aikd_*` crates to `crates/<name>/src/...`.
    // An ambiguous suffix is dropped, never guessed.
    let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    for end in (1..=segments.len()).rev() {
        let sub = segments[..end].join("/");
        let mut variants: Vec<String> = candidates_for(lang, &sub);
        if let Some((first, rest)) = sub.split_once('/') {
            let krate = first.trim_start_matches("aikd_").replace('_', "-");
            variants.extend(candidates_for(lang, &format!("crates/{krate}/src/{rest}")));
            variants.extend(candidates_for(lang, &format!("{first}/src/{rest}")));
            variants.extend(candidates_for(lang, &format!("{first}/lib/{rest}")));
        } else {
            let krate = sub.trim_start_matches("aikd_").replace('_', "-");
            variants.extend(candidates_for(lang, &format!("crates/{krate}/src/lib")));
        }
        for v in variants {
            let suffix = format!("/{v}");
            let mut hits: Vec<&String> = files.iter().filter(|f| f.ends_with(&suffix)).collect();
            hits.sort();
            hits.dedup();
            if hits.len() == 1 {
                return Some(hits[0].clone());
            }
        }
    }

    if !rel.contains('/') {
        let stem = match lang {
            "rust" => format!("{rel}.rs"),
            "python" => format!("{rel}.py"),
            "js" | "dart" => format!("{rel}.{lang}"),
            _ => rel.clone(),
        };
        let mut hits: Vec<&String> = files
            .iter()
            .filter(|f| {
                f.ends_with(&format!("/{stem}"))
                    || f.ends_with(&format!("/{rel}/mod.rs"))
                    || f.ends_with(&format!("/{rel}/index.ts"))
                    || f.ends_with(&format!("/{rel}/index.js"))
            })
            .collect();
        hits.sort();
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

// ---------------------------------------------------------------------------
// Legacy project-scoped graph (kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Build the file/symbol graph for files under `project` (canonical prefix match).
pub fn file_graph(conn: &Connection, project: &str, limit_files: usize) -> Result<FileGraph> {
    let norm = aikd_core::platform::normalize_project_key(project);
    let files: Vec<String> = load_files(conn, &norm, limit_files.max(1))?;
    let mut nodes = Vec::new();
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let syms = load_symbols(conn, &norm, 5000)?;
    for f in &files {
        nodes.push(node(f.clone(), "file", f.clone(), "VERIFIED"));
        for (path, name, kind) in &syms {
            if path != f {
                continue;
            }
            let id = format!("{f}#{name}");
            let mut n = node(id.clone(), "symbol", name.clone(), "VERIFIED");
            n.subtype = Some(kind.clone());
            symbols.push(n);
            edges.push(GraphEdge {
                from: f.clone(),
                to: id,
                kind: "contains".into(),
                provenance: "VERIFIED".into(),
            });
        }
        for (target, to) in import_edges_for_file(conn, f, &files, &norm) {
            if to != *f && !target.is_empty() {
                edges.push(GraphEdge {
                    from: f.clone(),
                    to,
                    kind: "imports".into(),
                    provenance: "INFERRED".into(),
                });
            }
        }
    }
    Ok(FileGraph {
        files: nodes,
        symbols,
        edges,
        scope_note:
            "contains=VERIFIED from symbol index; imports=parsed use/mod/import lines (Rust/Python/TS/JS/Dart/Go), target resolution INFERRED, unresolved dropped"
                .to_string(),
        nodes: Vec::new(),
        stats: GraphStats::default(),
    })
}

fn load_files(conn: &Connection, project: &str, limit: usize) -> Result<Vec<String>> {
    if project.is_empty() {
        let mut stmt =
            conn.prepare("SELECT path FROM files WHERE status='active' ORDER BY path LIMIT ?1")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |r| r.get(0))?;
        return Ok(rows
            .collect::<std::result::Result<Vec<String>, _>>()
            .unwrap_or_default());
    }
    let escaped = project
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let prefix = if project == "/" {
        "/%".to_string()
    } else {
        format!("{escaped}/%")
    };
    let mut stmt = conn.prepare(
        "SELECT path FROM files WHERE status='active' AND (replace(path,'\\','/') = ?1 OR replace(path,'\\','/') LIKE ?2 ESCAPE '\\') ORDER BY path LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![project, prefix, limit as i64], |r| {
        r.get(0)
    })?;
    Ok(rows
        .collect::<std::result::Result<Vec<String>, _>>()
        .unwrap_or_default())
}

type SymbolRow = (String, String, String);

fn load_symbols(conn: &Connection, project: &str, limit: usize) -> Result<Vec<SymbolRow>> {
    let escaped = project
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let prefix = if project.is_empty() {
        "%".to_string()
    } else if project == "/" {
        "/%".to_string()
    } else {
        format!("{escaped}/%")
    };
    let mut stmt = conn.prepare(
        "SELECT fl.path, s.symbol_name, s.symbol_type FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files fl ON fl.id=c.file_id WHERE fl.status='active' AND (?1 = '' OR replace(fl.path,'\\','/') = ?1 OR replace(fl.path,'\\','/') LIKE ?2 ESCAPE '\\') ORDER BY fl.path, s.line_start LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![project, prefix, limit as i64], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    Ok(rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap_or_default())
}

fn file_content(conn: &Connection, path: &str, max_files: usize) -> String {
    let mut contents: Vec<String> = conn
        .prepare("SELECT c.content FROM chunks c JOIN files fl ON fl.id=c.file_id WHERE fl.path=?1 AND c.content != ''")
        .and_then(|mut s| {
            s.query_map(rusqlite::params![path], |r| r.get(0))
                .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
        })
        .unwrap_or_default();
    if contents.is_empty() {
        if let Ok(disk) = std::fs::read_to_string(std::path::Path::new(path)) {
            contents.push(disk);
        }
    }
    contents.join("\n").chars().take(max_files * 1024).collect()
}

/// Resolved import edges for one file: `(raw_target, resolved_path)`.
fn import_edges_for_file(
    conn: &Connection,
    file: &str,
    files: &[String],
    project: &str,
) -> Vec<(String, String)> {
    let lang = lang_of(file);
    if lang.is_empty() {
        return Vec::new();
    }
    let content = file_content(conn, file, 512);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('*') {
            continue;
        }
        if let Some(target) = parse_imports(lang, line) {
            if seen.insert(target.clone()) {
                if let Some(to) = resolve_target(files, project, lang, file, &target) {
                    out.push((target, to));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Unified knowledge graph (`GET /api/graph`)
// ---------------------------------------------------------------------------

fn tags_of(text: &str) -> Vec<String> {
    crate::notes::extract_tags(text)
}

/// Build the unified knowledge graph.
///
/// Empty `project` returns a rich global graph (projects, files, notes,
/// memories, tags, agents) instead of zero nodes.
pub fn knowledge_graph(conn: &Connection, opts: &GraphOptions) -> Result<FileGraph> {
    let limit = if opts.limit == 0 {
        120
    } else {
        opts.limit.min(1000)
    };
    let project = aikd_core::platform::normalize_project_key(&opts.project);

    // Files: bounded so a global graph never reads the whole disk history.
    let files = load_files(conn, &project, 3000)?;
    let file_set: HashSet<String> = files.iter().cloned().collect();

    let mut nodes: HashMap<String, GraphNode> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    // ---- projects (hub nodes) ----
    let mut projects: Vec<String> = Vec::new();
    {
        let mut collect = |p: String| {
            let p = aikd_core::platform::normalize_project_key(&p);
            if !p.is_empty() && !projects.contains(&p) {
                projects.push(p);
            }
        };
        if let Ok(mut stmt) = conn.prepare("SELECT project_path FROM atlas LIMIT 50") {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for p in rows.flatten() {
                    collect(p);
                }
            }
        }
        if let Ok(mut stmt) = conn
            .prepare("SELECT DISTINCT project_path FROM memory WHERE project_path != '' LIMIT 50")
        {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for p in rows.flatten() {
                    collect(p);
                }
            }
        }
        if let Ok(mut stmt) =
            conn.prepare("SELECT DISTINCT project FROM notes WHERE project != '' LIMIT 50")
        {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for p in rows.flatten() {
                    collect(p);
                }
            }
        }
        if !project.is_empty() {
            collect(project.clone());
        }
    }
    for p in &projects {
        let mut n = node(format!("project:{p}"), "project", basename(p), "VERIFIED");
        n.project = Some(p.clone());
        nodes.insert(n.id.clone(), n);
    }
    let project_of = |path: &str| -> Option<String> {
        projects
            .iter()
            .filter(|p| path == p.as_str() || path.starts_with(&format!("{p}/")))
            .max_by_key(|p| p.len())
            .cloned()
    };

    // ---- file nodes ----
    let parse_cap = if project.is_empty() {
        limit.saturating_mul(4).max(200).min(600)
    } else {
        800
    };
    for (idx, f) in files.iter().enumerate() {
        let mut n = node(format!("file:{f}"), "file", basename(f), "VERIFIED");
        n.project = project_of(f);
        nodes.insert(n.id.clone(), n);
        if idx < parse_cap {
            for (_target, to) in import_edges_for_file(conn, f, &files, &project) {
                push_edge(
                    &mut edges,
                    format!("file:{f}"),
                    format!("file:{to}"),
                    "imports",
                    "INFERRED",
                );
            }
        }
    }

    // ---- symbols ----
    for (path, name, sym_type) in load_symbols(conn, &project, 4000)? {
        if !file_set.contains(&path) {
            continue;
        }
        let id = format!("symbol:{path}#{name}");
        let mut n = node(id.clone(), "symbol", name, "VERIFIED");
        n.subtype = Some(sym_type);
        n.project = project_of(&path);
        nodes.insert(id.clone(), n);
        push_edge(
            &mut edges,
            format!("file:{path}"),
            id,
            "contains",
            "VERIFIED",
        );
    }

    // ---- project → file containment ----
    for f in &files {
        if let Some(p) = project_of(f) {
            push_edge(
                &mut edges,
                format!("project:{p}"),
                format!("file:{f}"),
                "contains",
                "VERIFIED",
            );
        }
    }

    // ---- human notes ----
    let mut note_rows: Vec<(String, String, String, String, String, String)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, kind, scope, project, tags_json FROM notes ORDER BY pinned DESC, datetime(updated_at) DESC LIMIT 300",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        }) {
            note_rows = rows.flatten().collect();
        }
    }
    for (id, title, kind, scope, note_project, tags_json) in &note_rows {
        let mut n = node(format!("note:{id}"), "note", title.clone(), "HUMAN");
        n.subtype = Some(kind.clone());
        n.project = Some(note_project.clone()).filter(|p| !p.is_empty());
        n.scope = Some(scope.clone());
        nodes.insert(n.id.clone(), n);
        let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
        for tag in tags {
            push_edge(
                &mut edges,
                format!("note:{id}"),
                format!("tag:{tag}"),
                "tag",
                "EXTRACTED",
            );
        }
        if let Some(p) = project_of(note_project) {
            push_edge(
                &mut edges,
                format!("project:{p}"),
                format!("note:{id}"),
                "contains",
                "VERIFIED",
            );
        }
    }

    // ---- knowledge_links: canonical note/memory → tag/note/file edges ----
    if let Ok(mut stmt) = conn.prepare(
        "SELECT from_id, from_kind, to_id, to_kind, relation FROM knowledge_links LIMIT 5000",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        }) {
            for (from_id, from_kind, to_id, to_kind, relation) in rows.flatten() {
                let from = match from_kind.as_str() {
                    "note" => from_id.clone(),
                    "memory" => from_id.clone(),
                    "file" => format!("file:{from_id}"),
                    _ => continue,
                };
                let to = match to_kind.as_str() {
                    "tag" => to_id.clone(),
                    "note" | "memory" => to_id.clone(),
                    "file" => format!("file:{to_id}"),
                    _ => continue,
                };
                if to.starts_with("file:") && !file_set.contains(&to[5..]) {
                    continue; // never invent file nodes for deleted files
                }
                push_edge(&mut edges, from, to, &relation, "EXTRACTED");
            }
        }
    }

    // ---- memories (active knowledge) ----
    let mut mem_rows: Vec<(String, String, String, String, String)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, kind, project_path, body FROM memory WHERE status='active' ORDER BY datetime(updated_at) DESC LIMIT 300",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        }) {
            mem_rows = rows.flatten().collect();
        }
    }
    for (idx, (id, title, kind, mem_project, body)) in mem_rows.iter().enumerate() {
        let mut n = node(format!("memory:{id}"), "memory", title.clone(), "AGENT");
        n.subtype = Some(kind.clone());
        n.project = Some(mem_project.clone()).filter(|p| !p.is_empty());
        n.scope = Some(if mem_project.is_empty() {
            "global".to_string()
        } else {
            "project".to_string()
        });
        nodes.insert(n.id.clone(), n);
        for tag in tags_of(body) {
            push_edge(
                &mut edges,
                format!("memory:{id}"),
                format!("tag:{tag}"),
                "tag",
                "EXTRACTED",
            );
        }
        if idx < 150 {
            for path in crate::notes::extract_file_mentions(conn, body)
                .into_iter()
                .take(5)
            {
                if file_set.contains(&path) {
                    push_edge(
                        &mut edges,
                        format!("memory:{id}"),
                        format!("file:{path}"),
                        "mentions",
                        "EXTRACTED",
                    );
                }
            }
        }
        if let Some(p) = project_of(mem_project) {
            push_edge(
                &mut edges,
                format!("project:{p}"),
                format!("memory:{id}"),
                "relates",
                "EXTRACTED",
            );
        }
    }

    // ---- tags: materialize any tag referenced by an edge ----
    let tag_ids: Vec<String> = edges
        .iter()
        .filter(|e| e.to.starts_with("tag:") || e.from.starts_with("tag:"))
        .flat_map(|e| [e.from.clone(), e.to.clone()])
        .filter(|id| id.starts_with("tag:"))
        .collect();
    for id in tag_ids {
        let label = id.strip_prefix("tag:").unwrap_or(&id).to_string();
        nodes
            .entry(id.clone())
            .or_insert_with(|| node(id, "tag", format!("#{label}"), "EXTRACTED"));
    }

    // ---- unresolved wikilink targets: keep the reference visible ----
    let note_refs: Vec<String> = edges
        .iter()
        .flat_map(|e| [e.from.clone(), e.to.clone()])
        .filter(|id| id.starts_with("note:") && !nodes.contains_key(id))
        .collect();
    for id in note_refs {
        let label = {
            let raw = id.strip_prefix("note:").unwrap_or(&id);
            let mut out = String::new();
            for (i, word) in raw.split('-').enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let mut chars = word.chars();
                if let Some(first) = chars.next() {
                    out.push(first.to_ascii_uppercase());
                    out.extend(chars);
                }
            }
            out
        };
        let mut n = node(id.clone(), "note", label, "EXTRACTED");
        n.subtype = Some("wikilink".to_string());
        nodes.insert(id, n);
    }

    // ---- active agents ----
    {
        let mut seen_agents = HashSet::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT client_id, mcp_client_name, cwd, last_tool FROM active_clients WHERE status!='disconnected' LIMIT 30",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                for (client_id, name, cwd, last_tool) in rows.flatten() {
                    let id = format!("agent:{client_id}");
                    let label = if name.is_empty() {
                        client_id.clone()
                    } else {
                        format!("{name} · {last_tool}")
                    };
                    let mut n = node(id.clone(), "agent", label, "VERIFIED");
                    n.subtype = Some("mcp-stdio".to_string());
                    n.project = Some(cwd.clone()).filter(|p| !p.is_empty());
                    nodes.insert(id.clone(), n);
                    if let Some(p) = project_of(&cwd) {
                        push_edge(&mut edges, id, format!("project:{p}"), "mentions", "EXTRACTED");
                    }
                    seen_agents.insert(client_id);
                }
            }
        }
        // Historical agents from the event journal (no live client row).
        if let Ok(mut stmt) =
            conn.prepare("SELECT DISTINCT agent_id FROM events ORDER BY agent_id LIMIT 20")
        {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                for agent in rows.flatten() {
                    if seen_agents.contains(&agent) {
                        continue;
                    }
                    let id = format!("agent:{agent}");
                    let mut n = node(id.clone(), "agent", agent, "EXTRACTED");
                    n.subtype = Some("event".to_string());
                    nodes.insert(id, n);
                }
            }
        }
    }

    // ---- Markdown file nodes: `[[wikilink]]` + `#tag` extraction ----
    for f in &files {
        if !is_markdown(f) {
            continue;
        }
        let content = file_content(conn, f, 64);
        if content.is_empty() {
            continue;
        }
        for tag in tags_of(&content) {
            push_edge(
                &mut edges,
                format!("file:{f}"),
                format!("tag:{tag}"),
                "tag",
                "EXTRACTED",
            );
        }
        for target in crate::notes::extract_wikilinks(&content) {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM notes WHERE lower(title)=lower(?1) LIMIT 1",
                    rusqlite::params![target],
                    |r| r.get(0),
                )
                .ok();
            if let Some(id) = existing {
                push_edge(
                    &mut edges,
                    format!("file:{f}"),
                    format!("note:{id}"),
                    "wikilink",
                    "EXTRACTED",
                );
            }
        }
        // `[text](relative/path.md)` → resolved against the file's directory.
        for target in crate::notes::extract_md_links(&content) {
            let joined = join_path(&parent_dir(f), &target);
            if file_set.contains(&joined) && joined != *f {
                push_edge(
                    &mut edges,
                    format!("file:{f}"),
                    format!("file:{joined}"),
                    "mentions",
                    "EXTRACTED",
                );
            }
        }
    }

    // ---- local mode: BFS around focus up to `depth` hops ----
    if opts.mode == "local" {
        if let Some(focus) = opts.focus.as_deref().filter(|s| !s.is_empty()) {
            let focus_id = focus.to_string();
            let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
            for e in &edges {
                adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
                adj.entry(e.to.as_str()).or_default().push(e.from.as_str());
            }
            let mut visited: HashSet<String> = HashSet::new();
            let mut queue: VecDeque<(String, usize)> = VecDeque::new();
            visited.insert(focus_id.clone());
            queue.push_back((focus_id.clone(), 0));
            let depth = opts.depth.clamp(1, 2);
            while let Some((cur, d)) = queue.pop_front() {
                if d >= depth {
                    continue;
                }
                if let Some(neighbors) = adj.get(cur.as_str()) {
                    for nb in neighbors {
                        if visited.insert((*nb).to_string()) {
                            queue.push_back(((*nb).to_string(), d + 1));
                        }
                    }
                }
            }
            nodes.retain(|id, _| visited.contains(id));
            edges.retain(|e| visited.contains(&e.from) && visited.contains(&e.to));
        }
    }

    // ---- type filter ----
    if let Some(types) = opts.types.as_deref().filter(|t| !t.is_empty()) {
        let allowed: HashSet<&str> = types.iter().map(|s| s.as_str()).collect();
        nodes.retain(|_, n| allowed.contains(n.kind.as_str()));
        edges.retain(|e| {
            let from_kind = e.from.split(':').next().unwrap_or("");
            let to_kind = e.to.split(':').next().unwrap_or("");
            nodes.contains_key(&e.from)
                && nodes.contains_key(&e.to)
                && (allowed.contains(from_kind) || allowed.contains(to_kind))
        });
        edges.retain(|e| nodes.contains_key(&e.from) && nodes.contains_key(&e.to));
    }

    // ---- degree + orphan pruning + limit ----
    let mut degree: HashMap<String, usize> = HashMap::new();
    for e in &edges {
        *degree.entry(e.from.clone()).or_insert(0) += 1;
        *degree.entry(e.to.clone()).or_insert(0) += 1;
    }
    if opts.hide_orphans {
        nodes.retain(|id, _| degree.get(id).copied().unwrap_or(0) > 0);
        edges.retain(|e| nodes.contains_key(&e.from) && nodes.contains_key(&e.to));
    }
    let mut ordered: Vec<GraphNode> = nodes.into_values().collect();
    let kind_tier = |n: &GraphNode, deg: usize| -> u8 {
        match n.kind.as_str() {
            "project" | "agent" => 0,
            "note" | "memory" | "tag" => 1,
            "file" if deg >= 2 => 2,
            "symbol" => 3,
            _ => 4,
        }
    };
    ordered.sort_by(|a, b| {
        let da = degree.get(&a.id).copied().unwrap_or(0);
        let db = degree.get(&b.id).copied().unwrap_or(0);
        kind_tier(a, da)
            .cmp(&kind_tier(b, db))
            .then_with(|| db.cmp(&da))
            .then_with(|| a.id.cmp(&b.id))
    });
    ordered.truncate(limit);
    let kept: HashSet<String> = ordered.iter().map(|n| n.id.clone()).collect();
    edges.retain(|e| kept.contains(&e.from) && kept.contains(&e.to));
    edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);

    let mut final_degree: HashMap<String, usize> = HashMap::new();
    for e in &edges {
        *final_degree.entry(e.from.clone()).or_insert(0) += 1;
        *final_degree.entry(e.to.clone()).or_insert(0) += 1;
    }
    for n in ordered.iter_mut() {
        n.degree = final_degree.get(&n.id).copied().unwrap_or(0);
    }
    ordered.sort_by(|a, b| {
        b.degree
            .cmp(&a.degree)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });

    // Legacy arrays (raw ids) derived from the unified node set.
    let legacy_files: Vec<GraphNode> = ordered
        .iter()
        .filter(|n| n.kind == "file")
        .map(|n| {
            let raw = n.id.strip_prefix("file:").unwrap_or(&n.id).to_string();
            node(raw.clone(), "file", raw, &n.provenance)
        })
        .collect();
    let legacy_symbols: Vec<GraphNode> = ordered
        .iter()
        .filter(|n| n.kind == "symbol")
        .map(|n| {
            let raw = n.id.strip_prefix("symbol:").unwrap_or(&n.id).to_string();
            let mut s = node(
                raw,
                &format!("symbol:{}", n.subtype.as_deref().unwrap_or("unknown")),
                n.label.clone(),
                &n.provenance,
            );
            s.subtype = n.subtype.clone();
            s
        })
        .collect();

    let stats = GraphStats {
        total_nodes: ordered.len(),
        total_edges: edges.len(),
    };
    let scope_note = format!(
        "mode={}; project={}; files=VERIFIED (symbol index); imports=INFERRED (Rust/Python/TS/JS/Dart/Go, unresolved dropped); tags/links=EXTRACTED from knowledge_links; notes=HUMAN; memory=AGENT",
        if project.is_empty() { "global" } else { opts.mode.as_str() },
        if project.is_empty() { "*" } else { project.as_str() }
    );

    Ok(FileGraph {
        files: legacy_files,
        symbols: legacy_symbols,
        edges,
        scope_note,
        nodes: ordered,
        stats,
    })
}

// ---------------------------------------------------------------------------
// Node inspector (`GET /api/graph/node`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct NodeLinkRef {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub edge_kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeDetail {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub project: Option<String>,
    pub scope: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub outgoing: Vec<NodeLinkRef>,
    pub backlinks: Vec<NodeLinkRef>,
}

fn preview_note(conn: &Connection, id: &str) -> (String, Vec<String>, serde_json::Value) {
    match crate::notes::get_note(conn, id) {
        Ok(Some(n)) => (
            n.body.chars().take(2000).collect(),
            n.tags,
            serde_json::json!({
                "source": n.source,
                "kind": n.kind,
                "scope": n.scope,
                "folder": n.folder,
                "file_path": n.file_path,
                "created_at": n.created_at,
                "updated_at": n.updated_at,
            }),
        ),
        _ => (String::new(), Vec::new(), serde_json::json!({})),
    }
}

fn preview_memory(conn: &Connection, id: &str) -> (String, serde_json::Value) {
    conn.query_row(
        "SELECT body, kind, source, status, updated_at FROM memory WHERE id=?1",
        rusqlite::params![id],
        |r| {
            let body: String = r.get(0)?;
            let kind: String = r.get(1)?;
            let source: String = r.get(2)?;
            let status: String = r.get(3)?;
            let updated: String = r.get(4)?;
            Ok((
                body.chars().take(2000).collect::<String>(),
                serde_json::json!({ "kind": kind, "source": source, "status": status, "updated_at": updated }),
            ))
        },
    )
    .unwrap_or_else(|_| (String::new(), serde_json::json!({})))
}

fn preview_file(conn: &Connection, path: &str) -> (String, serde_json::Value) {
    let content = file_content(conn, path, 64);
    let preview: String = content.chars().take(2000).collect();
    let meta = conn
        .query_row(
            "SELECT size, modified_at, last_scanned FROM files WHERE path=?1",
            rusqlite::params![path],
            |r| {
                Ok(serde_json::json!({
                    "path": path,
                    "size": r.get::<_, i64>(0)?,
                    "modified_at": r.get::<_, String>(1)?,
                    "last_scanned": r.get::<_, String>(2)?,
                }))
            },
        )
        .unwrap_or_else(|_| serde_json::json!({ "path": path }));
    (preview, meta)
}

fn preview_symbol(conn: &Connection, raw: &str) -> (String, serde_json::Value) {
    let (path, name) = match raw.split_once('#') {
        Some((p, n)) => (p, n),
        None => return (String::new(), serde_json::json!({})),
    };
    conn.query_row(
        "SELECT s.line_start, s.line_end, s.language, s.signature, c.content FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files fl ON fl.id=c.file_id WHERE fl.path=?1 AND s.symbol_name=?2 LIMIT 1",
        rusqlite::params![path, name],
        |r| {
            let line_start: i64 = r.get(0)?;
            let line_end: i64 = r.get(1)?;
            let language: String = r.get(2)?;
            let signature: String = r.get(3)?;
            let content: String = r.get(4)?;
            Ok((
                content.chars().take(2000).collect::<String>(),
                serde_json::json!({
                    "path": path,
                    "line_start": line_start,
                    "line_end": line_end,
                    "language": language,
                    "signature": signature,
                }),
            ))
        },
    )
    .unwrap_or_else(|_| (String::new(), serde_json::json!({ "path": path, "symbol": name })))
}

/// Detail for one graph node: preview + metadata + outgoing/backlinks.
pub fn node_detail(conn: &Connection, id: &str) -> Result<Option<NodeDetail>> {
    let graph = knowledge_graph(
        conn,
        &GraphOptions {
            mode: "global".to_string(),
            limit: 800,
            ..Default::default()
        },
    )?;
    let target = graph.nodes.iter().find(|n| n.id == id).cloned();
    let Some(target) = target else {
        return Ok(None);
    };

    let mut tags: Vec<String> = Vec::new();
    let (preview, metadata): (String, serde_json::Value) = if let Some(rest) =
        target.id.strip_prefix("note:")
    {
        let (body, note_tags, meta) = preview_note(conn, rest);
        tags = note_tags;
        (body, meta)
    } else if let Some(rest) = target.id.strip_prefix("memory:") {
        preview_memory(conn, rest)
    } else if let Some(rest) = target.id.strip_prefix("file:") {
        preview_file(conn, rest)
    } else if let Some(rest) = target.id.strip_prefix("symbol:") {
        preview_symbol(conn, rest)
    } else if let Some(rest) = target.id.strip_prefix("tag:") {
        let mut notes: Vec<String> = Vec::new();
        if let Ok(mut stmt) =
            conn.prepare("SELECT title FROM notes WHERE tags_json LIKE ?1 LIMIT 50")
        {
            if let Ok(rows) = stmt.query_map(rusqlite::params![format!("%\"{rest}\"%")], |r| {
                r.get::<_, String>(0)
            }) {
                notes = rows.flatten().collect();
            }
        }
        (
            format!(
                "{} note(s) tagged #{rest}{}",
                notes.len(),
                if notes.is_empty() {
                    String::new()
                } else {
                    format!(": {}", notes.join(", "))
                }
            ),
            serde_json::json!({ "tag": rest, "notes": notes.len() }),
        )
    } else if let Some(rest) = target.id.strip_prefix("project:") {
        let file_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE status='active' AND (replace(path,'\\','/') = ?1 OR replace(path,'\\','/') LIKE ?2)",
                rusqlite::params![rest, format!("{rest}/%")],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let notes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notes WHERE project=?1",
                rusqlite::params![rest],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (
            format!("Project {rest}: {file_count} indexed files, {notes} human notes."),
            serde_json::json!({ "path": rest, "files": file_count, "notes": notes }),
        )
    } else if let Some(rest) = target.id.strip_prefix("agent:") {
        let meta = conn
            .query_row(
                "SELECT pid, parent_pid, parent_name, transport, cwd, last_tool, last_target, status, last_seen_at FROM active_clients WHERE client_id=?1",
                rusqlite::params![rest],
                |r| {
                    Ok(serde_json::json!({
                        "pid": r.get::<_, i64>(0)?,
                        "parent_pid": r.get::<_, i64>(1)?,
                        "parent_name": r.get::<_, String>(2)?,
                        "transport": r.get::<_, String>(3)?,
                        "cwd": r.get::<_, String>(4)?,
                        "last_tool": r.get::<_, String>(5)?,
                        "last_target": r.get::<_, String>(6)?,
                        "status": r.get::<_, String>(7)?,
                        "last_seen_at": r.get::<_, String>(8)?,
                    }))
                },
            )
            .unwrap_or_else(|_| serde_json::json!({ "agent": rest }));
        (format!("Agent {rest}"), meta)
    } else {
        (String::new(), serde_json::json!({}))
    };

    let labels: HashMap<String, (String, String)> = graph
        .nodes
        .iter()
        .map(|n| (n.id.clone(), (n.label.clone(), n.kind.clone())))
        .collect();
    let mut outgoing = Vec::new();
    let mut backlinks = Vec::new();
    for e in &graph.edges {
        if e.from == target.id {
            let (label, kind) = labels
                .get(&e.to)
                .cloned()
                .unwrap_or((e.to.clone(), "unknown".to_string()));
            outgoing.push(NodeLinkRef {
                id: e.to.clone(),
                label,
                kind,
                edge_kind: e.kind.clone(),
            });
        }
        if e.to == target.id {
            let (label, kind) = labels
                .get(&e.from)
                .cloned()
                .unwrap_or((e.from.clone(), "unknown".to_string()));
            backlinks.push(NodeLinkRef {
                id: e.from.clone(),
                label,
                kind,
                edge_kind: e.kind.clone(),
            });
        }
    }

    Ok(Some(NodeDetail {
        id: target.id,
        kind: target.kind,
        label: target.label,
        project: target.project,
        scope: target.scope.unwrap_or_else(|| "project".to_string()),
        preview,
        tags,
        metadata,
        outgoing,
        backlinks,
    }))
}

// ---------------------------------------------------------------------------
// Impact analysis (`GET /api/impact`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Default)]
pub struct SymbolCaller {
    pub file: String,
    pub line: i64,
    pub snippet: String,
}

/// Blast radius for a symbol or file before refactoring.
pub fn impact(conn: &Connection, target: &str, project: &str) -> Result<serde_json::Value> {
    let project = aikd_core::platform::normalize_project_key(project);
    let needle = target.replace('\\', "/");

    // Files whose path or content match the target.
    let mut matched_files: Vec<String> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT path FROM files WHERE status='active' AND (replace(path,'\\','/') LIKE ?1 OR path = ?2) LIMIT 50",
    ) {
        if let Ok(rows) = stmt.query_map(
            rusqlite::params![format!("%{needle}%"), target],
            |r| r.get::<_, String>(0),
        ) {
            matched_files = rows.flatten().collect();
        }
    }

    // Reverse import edges: who imports the matched files.
    let graph = knowledge_graph(
        conn,
        &GraphOptions {
            project: project.clone(),
            mode: "global".to_string(),
            limit: 600,
            hide_orphans: true,
            ..Default::default()
        },
    )?;
    let mut direct_importers: Vec<String> = Vec::new();
    for f in &matched_files {
        let id = format!("file:{f}");
        for e in &graph.edges {
            if e.to == id && e.kind == "imports" {
                if let Some(from) = e.from.strip_prefix("file:") {
                    if !direct_importers.iter().any(|x| x == from) {
                        direct_importers.push(from.to_string());
                    }
                }
            }
        }
    }

    // Symbol owners: even in lean mode (`chunks.content` empty) the symbol
    // index still knows where a symbol lives.
    let mut symbol_owners: Vec<(String, i64, String)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT fl.path, s.line_start, COALESCE(s.signature,'') FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files fl ON fl.id=c.file_id WHERE s.symbol_name = ?1 OR s.symbol_name LIKE ?2 LIMIT 40",
    ) {
        if let Ok(rows) = stmt.query_map(
            rusqlite::params![target, format!("{target}%")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ) {
            symbol_owners = rows.flatten().collect();
        }
    }
    for (path, _, _) in &symbol_owners {
        if !matched_files.iter().any(|f| f == path) {
            matched_files.push(path.clone());
        }
    }

    // Symbol call sites: indexed content mentioning the identifier.
    let mut symbol_callers: Vec<SymbolCaller> = Vec::new();
    for (file, line, signature) in &symbol_owners {
        symbol_callers.push(SymbolCaller {
            file: file.clone(),
            line: *line,
            snippet: signature.chars().take(200).collect(),
        });
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT fl.path, c.line_start, c.content FROM chunks c JOIN files fl ON fl.id=c.file_id WHERE fl.status='active' AND c.content LIKE ?1 LIMIT 40",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![format!("%{target}%")], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            for (file, line, content) in rows.flatten() {
                if let Some(found) = content.lines().find(|l| l.contains(target)) {
                    symbol_callers.push(SymbolCaller {
                        file,
                        line,
                        snippet: found.trim().chars().take(200).collect(),
                    });
                }
            }
        }
    }
    // Lean mode: read the matched files from disk and find call sites.
    for file in matched_files.iter().take(40) {
        if let Ok(meta) = std::fs::metadata(file) {
            if meta.len() > 2 * 1024 * 1024 {
                continue;
            }
        }
        let Ok(content) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            if symbol_callers.len() >= 25 {
                break;
            }
            if line.contains(target) {
                symbol_callers.push(SymbolCaller {
                    file: file.clone(),
                    line: idx as i64 + 1,
                    snippet: line.trim().chars().take(200).collect(),
                });
            }
        }
    }
    symbol_callers.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    symbol_callers.dedup_by(|a, b| a.file == b.file && a.line == b.line);
    symbol_callers.truncate(25);

    let mut related_constraints: Vec<crate::memory::ConstraintRef> = Vec::new();
    if let Ok(list) = crate::memory::attach_constraints(conn, &project, 8) {
        for c in list {
            if c.body.contains(target)
                || c.title.contains(target)
                || matched_files.iter().any(|f| c.body.contains(f))
            {
                related_constraints.push(c);
            }
        }
    }
    if related_constraints.is_empty() {
        related_constraints = crate::memory::attach_constraints(conn, &project, 5)?;
    }

    let mut related_notes: Vec<serde_json::Value> = Vec::new();
    if let Ok((notes, _, _)) = crate::notes::list_notes(
        conn,
        &crate::notes::NoteFilter {
            q: Some(target.to_string()),
            limit: Some(10),
            ..Default::default()
        },
    ) {
        for n in notes {
            related_notes.push(serde_json::json!({
                "id": n.id,
                "title": n.title,
                "kind": n.kind,
            }));
        }
    }

    Ok(serde_json::json!({
        "target": target,
        "matched_files": matched_files,
        "direct_importers": direct_importers,
        "symbol_callers": symbol_callers,
        "related_constraints": related_constraints,
        "related_notes": related_notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn contains_verified_imports_resolved() {
        let conn = setup();
        conn.execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/src/main.rs', 50, 't', 't', 'active', 'h')", []).unwrap();
        conn.execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/src/tax.rs', 50, 't', 't', 'active', 'h')", []).unwrap();
        let fid: i64 = conn
            .query_row(
                "SELECT id FROM files WHERE path='/p/src/main.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 0, '', 1, 5, 'use crate::tax;\nfn main() {}', '{}', 't', 't')", rusqlite::params![fid]).unwrap();
        conn.execute("INSERT INTO code_symbols (id, chunk_id, symbol_type, symbol_name, signature, visibility, language, line_start, line_end, created_at) VALUES ('s1', 'c1', 'function', 'main', 'fn main()', 'private', 'rust', 2, 2, 't')", []).unwrap();

        let g = knowledge_graph(
            &conn,
            &GraphOptions {
                project: "/p".into(),
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(g
            .edges
            .iter()
            .any(|e| e.kind == "contains" && e.provenance == "VERIFIED"));
        let imp: Vec<&GraphEdge> = g.edges.iter().filter(|e| e.kind == "imports").collect();
        assert_eq!(imp.len(), 1);
        assert_eq!(imp[0].from, "file:/p/src/main.rs");
        assert_eq!(imp[0].to, "file:/p/src/tax.rs");
        assert_eq!(imp[0].provenance, "INFERRED");
        // Legacy arrays still populated.
        assert_eq!(g.files.len(), 2);
        assert!(g.symbols.iter().any(|s| s.label == "main"));
    }

    #[test]
    fn unresolved_imports_dropped_not_invented() {
        let conn = setup();
        conn.execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/src/main.rs', 50, 't', 't', 'active', 'h')", []).unwrap();
        let fid: i64 = conn
            .query_row(
                "SELECT id FROM files WHERE path='/p/src/main.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 0, '', 1, 5, 'use serde::Deserialize;\nuse std::io;', '{}', 't', 't')", rusqlite::params![fid]).unwrap();
        let g = knowledge_graph(
            &conn,
            &GraphOptions {
                project: "/p".into(),
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(g.edges.iter().all(|e| e.kind != "imports"));
    }

    #[test]
    fn empty_project_returns_global_graph_not_zero_nodes() {
        let conn = setup();
        conn.execute("INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/a.ts', 50, 't', 't', 'active', 'h')", []).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let note = crate::notes::create_note(
            &conn,
            dir.path(),
            crate::notes::NewNote {
                body: "hello #arch".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let mut cfg = GraphOptions::default();
        cfg.limit = 50;
        let g = knowledge_graph(&conn, &cfg).unwrap();
        assert!(!g.nodes.is_empty(), "global graph must not be empty");
        assert!(g.nodes.iter().any(|n| n.id == format!("note:{}", note.id)));
        assert!(g.nodes.iter().any(|n| n.kind == "file"));
        assert!(g
            .nodes
            .iter()
            .any(|n| n.kind == "tag" && n.label == "#arch"));
        assert!(g.stats.total_nodes >= 3);
    }

    #[test]
    fn local_mode_bfs_and_type_filter() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let a = crate::notes::create_note(
            &conn,
            dir.path(),
            crate::notes::NewNote {
                body: "first #one".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let b = crate::notes::create_note(
            &conn,
            dir.path(),
            crate::notes::NewNote {
                body: format!("links to [[{}]] #two", a.title),
                ..Default::default()
            },
        )
        .unwrap();
        let _ = b;

        let g = knowledge_graph(
            &conn,
            &GraphOptions {
                mode: "local".into(),
                focus: Some(format!("note:{}", a.id)),
                depth: 1,
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(g.nodes.iter().any(|n| n.id == format!("note:{}", a.id)));
        assert!(g.nodes.iter().all(|n| n.kind != "file"));

        let notes_only = knowledge_graph(
            &conn,
            &GraphOptions {
                types: Some(vec!["note".into(), "tag".into()]),
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(notes_only
            .nodes
            .iter()
            .all(|n| n.kind == "note" || n.kind == "tag"));
    }

    #[test]
    fn node_detail_has_preview_outgoing_backlinks() {
        let conn = setup();
        let dir = tempfile::tempdir().unwrap();
        let note = crate::notes::create_note(
            &conn,
            dir.path(),
            crate::notes::NewNote {
                title: Some("Checklist".into()),
                body: "review [[Other]] #arch".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let detail = node_detail(&conn, &format!("note:{}", note.id))
            .unwrap()
            .expect("note node");
        assert_eq!(detail.kind, "note");
        assert!(detail.preview.contains("review"));
        assert!(detail.tags.contains(&"arch".to_string()));
        assert!(detail.metadata.get("source").is_some());
        assert!(detail.outgoing.iter().any(|l| l.edge_kind == "tag"));
        assert!(detail.outgoing.iter().any(|l| l.edge_kind == "wikilink"));
    }

    #[test]
    fn impact_lists_importers_and_constraints() {
        let conn = setup();
        for p in ["/p/src/main.rs", "/p/src/tax.rs"] {
            conn.execute(
                "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1, 50, 't', 't', 'active', 'h')",
                rusqlite::params![p],
            )
            .unwrap();
        }
        let fid: i64 = conn
            .query_row(
                "SELECT id FROM files WHERE path='/p/src/main.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES ('c1', ?1, 0, '[]', 0, '', 1, 5, 'use crate::tax;\\nfn handle_graph() {}', '{}', 't', 't')", rusqlite::params![fid]).unwrap();
        crate::memory::save_memory(
            &conn,
            "/p",
            "ws",
            "constraint",
            "do not break handle_graph",
            "handle_graph is used by the REST layer",
            "explicit",
            1.0,
        )
        .unwrap();
        let out = impact(&conn, "handle_graph", "/p").unwrap();
        assert_eq!(out["target"], "handle_graph");
        assert!(!out["symbol_callers"].as_array().unwrap().is_empty());
        assert!(!out["related_constraints"].as_array().unwrap().is_empty());
    }

    #[test]
    fn workspace_relative_imports_resolve_by_suffix() {
        let conn = setup();
        for p in [
            "/p/crates/cli/src/main.rs",
            "/p/crates/storage/src/graph.rs",
            "/p/crates/storage/src/schema.rs",
        ] {
            conn.execute(
                "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1, 50, 't', 't', 'active', 'h')",
                rusqlite::params![p],
            )
            .unwrap();
        }
        let fid: i64 = conn
            .query_row(
                "SELECT id FROM files WHERE path='/p/crates/cli/src/main.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("INSERT INTO chunks (id, file_id, chunk_index, heading_hierarchy, heading_level, heading_text, line_start, line_end, content, metadata_json, created_at, updated_at) VALUES ('c2', ?1, 0, '[]', 0, '', 1, 3, 'use crate::storage::graph;\nuse aikd_storage::schema;\nuse aikd_storage::graph::file_graph;', '{}', 't', 't')", rusqlite::params![fid]).unwrap();
        let g = knowledge_graph(
            &conn,
            &GraphOptions {
                project: "/p".into(),
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
        let imps: Vec<&GraphEdge> = g.edges.iter().filter(|e| e.kind == "imports").collect();
        assert_eq!(
            imps.len(),
            2,
            "crate-root-deeper-than-project, aikd_* crates and symbol tails must resolve"
        );
        assert!(imps
            .iter()
            .any(|e| e.to == "file:/p/crates/storage/src/graph.rs"));
        assert!(imps
            .iter()
            .any(|e| e.to == "file:/p/crates/storage/src/schema.rs"));
    }

    #[test]
    fn typescript_dart_go_parsers() {
        assert_eq!(
            parse_imports("js", "import { x } from './x';").as_deref(),
            Some("./x")
        );
        assert_eq!(
            parse_imports("js", "const y = require(\"y\");").as_deref(),
            Some("y")
        );
        assert_eq!(
            parse_imports("dart", "import 'package:flutter/material.dart';").as_deref(),
            Some("package:flutter/material.dart")
        );
        assert_eq!(
            parse_imports("go", "import \"fmt\"").as_deref(),
            Some("fmt")
        );
        assert_eq!(parse_imports("go", "\"os\"").as_deref(), Some("os"));
        assert_eq!(
            parse_imports("rust", "use crate::storage::graph;").as_deref(),
            Some("crate::storage::graph")
        );
    }
}
