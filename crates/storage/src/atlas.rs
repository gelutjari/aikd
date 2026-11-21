//! Project Atlas (§5–8): persistent, incremental project intelligence.
//!
//! The Atlas is a JSON snapshot generated FROM canonical tables (files,
//! chunks, code_symbols, memory, events) — never hand-written. Each rebuild
//! fingerprints (path, size, modified_at) of indexed files; an unchanged
//! fingerprint keeps the snapshot FRESH without touching it. A mismatch
//! marks it STALE until rebuilt. Per-record provenance uses VERIFIED
//! (measured from the DB) vs INFERRED (heuristic, e.g. framework detection).

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// One section of the Atlas with its own provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasSection<T: Serialize> {
    pub status: String,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub root: String,
    pub file_count: i64,
    pub chunk_count: i64,
    pub symbol_count: i64,
    pub languages: Vec<(String, i64)>,
    pub frameworks: Vec<String>,
    pub entry_points: Vec<String>,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchInfo {
    pub top_dirs: Vec<(String, i64)>,
    pub symbol_kinds: Vec<(String, i64)>,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownIssue {
    pub file: String,
    pub line: u32,
    pub marker: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasSnapshot {
    pub project: String,
    pub repository: AtlasSection<RepoInfo>,
    pub architecture: AtlasSection<ArchInfo>,
    pub decisions: Vec<serde_json::Value>,
    pub open_tasks: Vec<serde_json::Value>,
    pub checkpoints: Vec<serde_json::Value>,
    pub known_issues: AtlasSection<Vec<KnownIssue>>,
    pub event_count: i64,
    pub memory_count: i64,
}

fn verified() -> String {
    "VERIFIED".to_string()
}

fn inferred() -> String {
    "INFERRED".to_string()
}

fn lang_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next().unwrap_or("");
    // Guard: extension must not contain a path separator (no-dot filenames).
    if ext.contains('/') || ext.contains('\\') {
        return None;
    }
    match ext {
        "rs" => Some("Rust"),
        "py" => Some("Python"),
        "js" | "mjs" | "cjs" => Some("JavaScript"),
        "ts" | "tsx" => Some("TypeScript"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "c" | "h" => Some("C"),
        "cpp" | "hpp" | "cc" => Some("C++"),
        "cs" => Some("C#"),
        "rb" => Some("Ruby"),
        "php" => Some("PHP"),
        "md" => Some("Markdown"),
        "toml" | "yaml" | "yml" | "json" => Some("Config"),
        _ => None,
    }
}



/// Normalize DB paths (backslash on Windows) once inside SQL so LIKE
/// markers work on every platform.
const NORM: &str = "replace(path, '\\', '/')";

/// Framework indicators: file-marker heuristics, always INFERRED.
fn detect_frameworks(conn: &Connection, project: &str) -> Vec<String> {
    // Match against normalized paths: the LIKE pattern must use `/` too.
    let project = project.replace('\\', "/");
    let markers = [
        ("Cargo.toml", "Rust/cargo"),
        ("package.json", "Node/npm"),
        ("pyproject.toml", "Python/pyproject"),
        ("requirements.txt", "Python/pip"),
        ("go.mod", "Go/modules"),
        ("pom.xml", "Java/maven"),
        ("build.gradle", "Java/gradle"),
        ("Gemfile", "Ruby/bundler"),
        ("composer.json", "PHP/composer"),
        ("Dockerfile", "Docker"),
        ("docker-compose.yml", "Docker-compose"),
    ];
    let mut out = Vec::new();
    for (file, label) in markers {
        let n: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM files WHERE {NORM} LIKE ?1 AND {NORM} LIKE ?2"),
                rusqlite::params![format!("%{project}%"), format!("%/{file}")],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if n > 0 {
            out.push(label.to_string());
        }
    }
    out
}

fn detect_entry_points(conn: &Connection, project: &str) -> Vec<String> {
    let project = project.replace('\\', "/");
    let candidates = [
        "main.rs", "lib.rs", "mod.rs", "main.py", "app.py", "index.js", "index.ts",
        "main.go", "Main.java", "Program.cs",
    ];
    let mut out = Vec::new();
    for c in candidates {
        let mut stmt = match conn.prepare(
            &format!("SELECT path FROM files WHERE {NORM} LIKE ?1 AND {NORM} LIKE ?2"),
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rows = match stmt.query_map(
            rusqlite::params![format!("%{project}%"), format!("%/{c}")],
            |r| r.get::<_, String>(0),
        ) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for r in rows.flatten() {
            out.push(r);
        }
    }
    out.truncate(20);
    out
}


/// Fingerprint of indexed file CONTENT for `project`.
/// Uses the content hash (not `modified_at`, which scans rewrite every run),
/// so unchanged files keep the fingerprint stable and the Atlas stays FRESH.
pub fn fingerprint(conn: &Connection, project: &str) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT path, size, blake3_hash FROM files WHERE path LIKE ?1 ORDER BY path",
    )?;
    let rows = stmt.query_map(rusqlite::params![format!("%{project}%")], |r| {
        Ok(format!("{}|{}|{}", r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut h = blake3::Hasher::new();
    let mut n = 0;
    for r in rows {
        h.update(r?.as_bytes());
        n += 1;
    }
    h.update(format!("|n={n}").as_bytes());
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/proj/Cargo.toml', 10, 't', 't', 'active', 'h')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/proj/src/main.rs', 100, 't', 't', 'active', 'h')",
            [],
        ).unwrap();
        conn
    }

    #[test]
    fn refresh_builds_and_reuses_fingerprint() {
        let conn = setup();
        let (s1, _) = refresh_atlas(&conn, "/proj").unwrap();
        assert_eq!(s1.repository.data.file_count, 2);
        assert!(s1.repository.data.frameworks.contains(&"Rust/cargo".to_string()));
        let fp1: String = conn.query_row("SELECT fingerprint FROM atlas WHERE project_path='/proj'", [], |r| r.get(0)).unwrap();
        // Second refresh without changes: fingerprint unchanged, no rebuild needed.
        let (s2, fresh) = refresh_atlas(&conn, "/proj").unwrap();
        assert!(fresh);
        assert_eq!(s2.repository.data.file_count, 2);
        let fp2: String = conn.query_row("SELECT fingerprint FROM atlas WHERE project_path='/proj'", [], |r| r.get(0)).unwrap();
        assert_eq!(fp1, fp2);
        // Touch a file -> stale detected, rebuild picks it up.
        conn.execute("UPDATE files SET size=200 WHERE path='/proj/src/main.rs'", []).unwrap();
        assert!(mark_stale_if_changed(&conn, "/proj").unwrap());
        let (s3, _) = refresh_atlas(&conn, "/proj").unwrap();
        assert_eq!(s3.repository.data.file_count, 2);
        let status: String = conn.query_row("SELECT status FROM atlas WHERE project_path='/proj'", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "fresh");
    }

    #[test]
    fn empty_project_is_empty_not_error() {
        let conn = setup();
        let (s, _) = refresh_atlas(&conn, "/nothing").unwrap();
        assert_eq!(s.repository.data.file_count, 0);
        let status: String = conn.query_row("SELECT status FROM atlas WHERE project_path='/nothing'", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "empty");
    }
}

fn build_snapshot(conn: &Connection, project: &str) -> Result<AtlasSnapshot> {
    let like = format!("%{project}%");
    let file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files WHERE path LIKE ?1", rusqlite::params![like], |r| r.get(0))
        .unwrap_or(0);
    let chunk_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_id IN (SELECT id FROM files WHERE path LIKE ?1)",
            rusqlite::params![like],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let symbol_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files f ON f.id=c.file_id WHERE f.path LIKE ?1",
            rusqlite::params![like],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Languages from file extensions.
    let mut lang_map: std::collections::HashMap<String, i64> = Default::default();
    if let Ok(mut stmt) = conn.prepare("SELECT path FROM files WHERE path LIKE ?1") {
        if let Ok(rows) = stmt.query_map(rusqlite::params![like], |r| r.get::<_, String>(0)) {
            for r in rows.flatten() {
                if let Some(l) = lang_of(&r) {
                    *lang_map.entry(l.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    let mut languages: Vec<(String, i64)> = lang_map.into_iter().collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1));

    // Top-level directories under project root. Files sitting directly in the
    // root are grouped as `(root)` so they are not mistaken for directories.
    let mut dir_map: std::collections::HashMap<String, i64> = Default::default();
    if let Ok(mut stmt) = conn.prepare("SELECT path FROM files WHERE path LIKE ?1") {
        if let Ok(rows) = stmt.query_map(rusqlite::params![like], |r| r.get::<_, String>(0)) {
            for r in rows.flatten() {
                let rel = r.strip_prefix(project).unwrap_or(&r).trim_start_matches(['/', '\\']);
                let mut parts = rel.split(['/', '\\']);
                let top = parts.next().unwrap_or("").to_string();
                let rest = parts.next().is_some();
                if top.is_empty() {
                    continue;
                }
                let key = if rest { top } else { "(root)".to_string() };
                *dir_map.entry(key).or_insert(0) += 1;
            }
        }
    }
    let mut top_dirs: Vec<(String, i64)> = dir_map.into_iter().collect();
    top_dirs.sort_by(|a, b| b.1.cmp(&a.1));
    top_dirs.truncate(20);

    let mut kind_map: std::collections::HashMap<String, i64> = Default::default();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.symbol_type FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files f ON f.id=c.file_id WHERE f.path LIKE ?1",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![like], |r| r.get::<_, String>(0)) {
            for r in rows.flatten() {
                *kind_map.entry(r).or_insert(0) += 1;
            }
        }
    }
    let mut symbol_kinds: Vec<(String, i64)> = kind_map.into_iter().collect();
    symbol_kinds.sort_by(|a, b| b.1.cmp(&a.1));

    // Known issues: TODO/FIXME/XXX/HACK markers in chunk text (file:line approximate via chunk line_start).
    let mut issues = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT f.path, c.line_start, c.content FROM chunks c JOIN files f ON f.id=c.file_id WHERE f.path LIKE ?1 AND (c.content LIKE '%TODO%' OR c.content LIKE '%FIXME%' OR c.content LIKE '%XXX%' OR c.content LIKE '%HACK%') LIMIT 50",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![like], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        }) {
            for r in rows.flatten() {
                let (file, line, content) = r;
                for marker in ["TODO", "FIXME", "XXX", "HACK"] {
                    if let Some(pos) = content.find(marker) {
                        let snippet: String = content[pos..].chars().take(120).collect();
                        issues.push(KnownIssue { file: file.clone(), line: line as u32, marker: marker.into(), text: snippet });
                        break;
                    }
                }
            }
        }
    }

    // Memory: decisions / open tasks / checkpoints from structured memory.
    let mem_items = crate::memory::list_memory(conn, project, None, None, true).unwrap_or_default();
    let to_json = |m: &aikd_core::MemoryItem| {
        serde_json::json!({"id": m.id, "kind": m.kind, "title": m.title, "source": m.source, "status": m.status})
    };
    let decisions: Vec<_> = mem_items.iter().filter(|m| m.kind == "decision").map(to_json).collect();
    let open_tasks: Vec<_> = mem_items.iter().filter(|m| m.kind == "task").map(to_json).collect();
    let checkpoints: Vec<_> = mem_items.iter().filter(|m| m.kind == "checkpoint").map(to_json).collect();
    let memory_count = mem_items.len() as i64;

    let event_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events WHERE project_id LIKE ?1", rusqlite::params![like], |r| r.get(0))
        .unwrap_or(0);

    Ok(AtlasSnapshot {
        project: project.to_string(),
        repository: AtlasSection {
            status: verified(),
            data: RepoInfo {
                root: project.to_string(),
                file_count,
                chunk_count,
                symbol_count,
                languages,
                frameworks: detect_frameworks(conn, project),
                entry_points: detect_entry_points(conn, project),
                provenance: verified(),
            },
        },
        architecture: AtlasSection {
            status: if top_dirs.is_empty() { "EMPTY".into() } else { inferred() },
            data: ArchInfo { top_dirs, symbol_kinds, provenance: inferred() },
        },
        decisions,
        open_tasks,
        checkpoints,
        known_issues: AtlasSection { status: verified(), data: issues },
        event_count,
        memory_count,
    })
}

/// Rebuild the Atlas for `project` if the fingerprint changed.
/// Returns (snapshot, fresh: bool). `fresh=true` means rebuilt now OR
/// already fresh; the `status` column tells whether a rebuild happened.
pub fn refresh_atlas(conn: &Connection, project: &str) -> Result<(AtlasSnapshot, bool)> {
    let fp = fingerprint(conn, project)?;
    let stored: Option<(String, String)> = conn
        .query_row(
            "SELECT snapshot_json, fingerprint FROM atlas WHERE project_path=?1",
            rusqlite::params![project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if let Some((json, old_fp)) = stored {
        if old_fp == fp {
            if let Ok(snap) = serde_json::from_str::<AtlasSnapshot>(&json) {
                return Ok((snap, true));
            }
        }
    }
    let t0 = std::time::Instant::now();
    let snap = build_snapshot(conn, project)?;
    let json = serde_json::to_string(&snap)?;
    let now = chrono::Utc::now().to_rfc3339();
    let status = if snap.repository.data.file_count == 0 { "empty" } else { "fresh" };
    conn.execute(
        "INSERT INTO atlas (project_path, snapshot_json, fingerprint, status, updated_at, build_ms) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(project_path) DO UPDATE SET snapshot_json=excluded.snapshot_json, fingerprint=excluded.fingerprint, status=excluded.status, updated_at=excluded.updated_at, build_ms=excluded.build_ms",
        rusqlite::params![project, json, fp, status, now, t0.elapsed().as_millis() as i64],
    )?;
    Ok((snap, true))
}

/// Read the stored snapshot without rebuilding (may be STALE).
pub fn read_atlas(conn: &Connection, project: &str) -> Result<Option<(AtlasSnapshot, String, String)>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT snapshot_json, status, updated_at FROM atlas WHERE project_path=?1",
            rusqlite::params![project],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    match row {
        Some((json, status, updated)) => {
            let snap: AtlasSnapshot = serde_json::from_str(&json)?;
            Ok(Some((snap, status, updated)))
        }
        None => Ok(None),
    }
}

/// Mark stored atlas rows STALE when their fingerprint no longer matches
/// (called after scans that changed file state).
pub fn mark_stale_if_changed(conn: &Connection, project: &str) -> Result<bool> {
    let fp = fingerprint(conn, project)?;
    let old: Option<String> = conn
        .query_row("SELECT fingerprint FROM atlas WHERE project_path=?1", rusqlite::params![project], |r| r.get(0))
        .ok();
    match old {
        Some(o) if o == fp => Ok(false),
        _ => {
            conn.execute("UPDATE atlas SET status='stale' WHERE project_path=?1", rusqlite::params![project])?;
            Ok(true)
        }
    }
}
