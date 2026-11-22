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

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn project_scope(project: &str) -> (String, String) {
    let norm = aikd_core::platform::normalize_project_key(project);
    let prefix = if norm.is_empty() {
        String::new()
    } else if norm == "/" {
        "/%".to_string()
    } else {
        format!("{}/%", escape_like(&norm))
    };
    (norm, prefix)
}

/// Framework indicators: file-marker heuristics, always INFERRED.
fn detect_frameworks(conn: &Connection, project: &str) -> Vec<String> {
    let (norm, prefix) = project_scope(project);
    if norm.is_empty() {
        return Vec::new();
    }
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
                &format!("SELECT COUNT(*) FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\') AND ({NORM} LIKE ?3 OR {NORM} = ?4)"),
                rusqlite::params![norm, prefix, format!("%/{file}"), file],
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
    let (norm, prefix) = project_scope(project);
    if norm.is_empty() {
        return Vec::new();
    }
    let candidates = [
        "main.rs",
        "lib.rs",
        "mod.rs",
        "main.py",
        "app.py",
        "index.js",
        "index.ts",
        "main.go",
        "Main.java",
        "Program.cs",
    ];
    let mut out = Vec::new();
    for c in candidates {
        let mut stmt = match conn.prepare(
            &format!("SELECT path FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\') AND {NORM} LIKE ?3"),
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rows = match stmt.query_map(rusqlite::params![norm, prefix, format!("%/{c}")], |r| {
            r.get::<_, String>(0)
        }) {
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
    let (norm, prefix) = project_scope(project);
    if norm.is_empty() {
        return Ok(blake3::hash(b"|n=0").to_hex().to_string());
    }
    let mut stmt = conn.prepare(
        &format!("SELECT path, size, blake3_hash FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\') ORDER BY path"),
    )?;
    let rows = stmt.query_map(rusqlite::params![norm, prefix], |r| {
        Ok(format!(
            "{}|{}|{}",
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?
        ))
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

fn build_snapshot(conn: &Connection, project: &str) -> Result<AtlasSnapshot> {
    let (norm, prefix) = project_scope(project);
    let file_count: i64 = if norm.is_empty() {
        0
    } else {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\')"),
            rusqlite::params![norm, prefix],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let chunk_count: i64 = if norm.is_empty() {
        0
    } else {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM chunks WHERE file_id IN (SELECT id FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\'))"),
            rusqlite::params![norm, prefix],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let symbol_count: i64 = if norm.is_empty() {
        0
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files f ON f.id=c.file_id WHERE f.status='active' AND (replace(f.path, '\\', '/') = ?1 OR replace(f.path, '\\', '/') LIKE ?2 ESCAPE '\\')",
            rusqlite::params![norm, prefix],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    // Languages from file extensions.
    let mut lang_map: std::collections::HashMap<String, i64> = Default::default();
    if !norm.is_empty() {
        if let Ok(mut stmt) = conn.prepare(&format!("SELECT path FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\')")) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![norm, prefix], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    if let Some(l) = lang_of(&r) {
                        *lang_map.entry(l.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let mut languages: Vec<(String, i64)> = lang_map.into_iter().collect();
    languages.sort_by_key(|b| std::cmp::Reverse(b.1));

    // Top-level directories under project root. Files sitting directly in the
    // root are grouped as `(root)` so they are not mistaken for directories.
    let mut dir_map: std::collections::HashMap<String, i64> = Default::default();
    if !norm.is_empty() {
        if let Ok(mut stmt) = conn.prepare(&format!("SELECT path FROM files WHERE status='active' AND ({NORM} = ?1 OR {NORM} LIKE ?2 ESCAPE '\\')")) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![norm, prefix], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    let rn = r.replace('\\', "/");
                    let rel = rn.strip_prefix(&norm).unwrap_or(&rn).trim_start_matches('/');
                    let mut parts = rel.split('/');
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
    }
    let mut top_dirs: Vec<(String, i64)> = dir_map.into_iter().collect();
    top_dirs.sort_by_key(|b| std::cmp::Reverse(b.1));
    top_dirs.truncate(20);

    let mut kind_map: std::collections::HashMap<String, i64> = Default::default();
    if !norm.is_empty() {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT s.symbol_type FROM code_symbols s JOIN chunks c ON c.id=s.chunk_id JOIN files f ON f.id=c.file_id WHERE f.status='active' AND (replace(f.path, '\\', '/') = ?1 OR replace(f.path, '\\', '/') LIKE ?2 ESCAPE '\\')",
        ) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![norm, prefix], |r| r.get::<_, String>(0)) {
                for r in rows.flatten() {
                    *kind_map.entry(r).or_insert(0) += 1;
                }
            }
        }
    }
    let mut symbol_kinds: Vec<(String, i64)> = kind_map.into_iter().collect();
    symbol_kinds.sort_by_key(|b| std::cmp::Reverse(b.1));

    // Known issues: TODO/FIXME/XXX/HACK markers per line in chunk text (or live file in lean mode).
    let mut issues = Vec::new();
    if !norm.is_empty() {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT f.path, c.line_start, c.line_end, c.content FROM chunks c JOIN files f ON f.id=c.file_id WHERE f.status='active' AND (replace(f.path, '\\', '/') = ?1 OR replace(f.path, '\\', '/') LIKE ?2 ESCAPE '\\') LIMIT 200",
        ) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![norm, prefix], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                for r in rows.flatten() {
                    let (file, base_line, end_line, raw_content) = r;
                    let content = if !raw_content.is_empty() {
                        raw_content
                    } else {
                        crate::fts::read_snippet_from_disk(
                            &file,
                            base_line.max(1) as usize,
                            end_line.max(1) as usize,
                        )
                        .unwrap_or_default()
                    };
                    if !content.contains("TODO")
                        && !content.contains("FIXME")
                        && !content.contains("XXX")
                        && !content.contains("HACK")
                    {
                        continue;
                    }
                    for (offset, text_line) in content.lines().enumerate() {
                        for marker in ["TODO", "FIXME", "XXX", "HACK"] {
                            if let Some(pos) = text_line.find(marker) {
                                let snippet: String = text_line[pos..].chars().take(120).collect();
                                issues.push(KnownIssue {
                                    file: file.clone(),
                                    line: (base_line.max(1) as usize + offset) as u32,
                                    marker: marker.into(),
                                    text: snippet,
                                });
                                break;
                            }
                        }
                        if issues.len() >= 50 {
                            break;
                        }
                    }
                    if issues.len() >= 50 {
                        break;
                    }
                }
            }
        }
    }

    // Memory: active decisions / open tasks / active checkpoints from structured memory.
    let mem_items = crate::memory::list_memory(conn, &norm, None, None, false).unwrap_or_default();
    let to_json = |m: &aikd_core::MemoryItem| serde_json::json!({"id": m.id, "kind": m.kind, "title": m.title, "source": m.source, "status": m.status});
    let decisions: Vec<_> = mem_items
        .iter()
        .filter(|m| m.kind == "decision")
        .map(to_json)
        .collect();
    let open_tasks: Vec<_> = mem_items
        .iter()
        .filter(|m| m.kind == "task")
        .map(to_json)
        .collect();
    let checkpoints: Vec<_> = mem_items
        .iter()
        .filter(|m| m.kind == "checkpoint")
        .map(to_json)
        .collect();
    let all_mem_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory WHERE project_path=?1",
            rusqlite::params![norm],
            |r| r.get(0),
        )
        .unwrap_or(mem_items.len() as i64);

    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE project_id=?1",
            rusqlite::params![norm],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(AtlasSnapshot {
        project: norm.clone(),
        repository: AtlasSection {
            status: verified(),
            data: RepoInfo {
                root: norm.clone(),
                file_count,
                chunk_count,
                symbol_count,
                languages,
                frameworks: detect_frameworks(conn, &norm),
                entry_points: detect_entry_points(conn, &norm),
                provenance: verified(),
            },
        },
        architecture: AtlasSection {
            status: if top_dirs.is_empty() {
                "EMPTY".into()
            } else {
                inferred()
            },
            data: ArchInfo {
                top_dirs,
                symbol_kinds,
                provenance: inferred(),
            },
        },
        decisions,
        open_tasks,
        checkpoints,
        known_issues: AtlasSection {
            status: verified(),
            data: issues,
        },
        event_count,
        memory_count: all_mem_count,
    })
}

/// Rebuild the Atlas for `project` if the fingerprint changed.
/// Returns (snapshot, fresh: bool). `fresh=true` means rebuilt now OR
/// already fresh; the `status` column tells whether a rebuild happened.
pub fn refresh_atlas(conn: &Connection, project: &str) -> Result<(AtlasSnapshot, bool)> {
    let norm = aikd_core::platform::normalize_project_key(project);
    let fp = fingerprint(conn, &norm)?;
    let stored: Option<(String, String)> = conn
        .query_row(
            "SELECT snapshot_json, fingerprint FROM atlas WHERE project_path=?1",
            rusqlite::params![norm],
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
    let snap = build_snapshot(conn, &norm)?;
    let json = serde_json::to_string(&snap)?;
    let now = chrono::Utc::now().to_rfc3339();
    let status = if snap.repository.data.file_count == 0 {
        "empty"
    } else {
        "fresh"
    };
    conn.execute(
        "INSERT INTO atlas (project_path, snapshot_json, fingerprint, status, updated_at, build_ms) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(project_path) DO UPDATE SET snapshot_json=excluded.snapshot_json, fingerprint=excluded.fingerprint, status=excluded.status, updated_at=excluded.updated_at, build_ms=excluded.build_ms",
        rusqlite::params![norm, json, fp, status, now, t0.elapsed().as_millis() as i64],
    )?;
    Ok((snap, true))
}

/// Read the stored snapshot without rebuilding (may be STALE).
pub fn read_atlas(
    conn: &Connection,
    project: &str,
) -> Result<Option<(AtlasSnapshot, String, String)>> {
    let norm = aikd_core::platform::normalize_project_key(project);
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT snapshot_json, status, updated_at FROM atlas WHERE project_path=?1",
            rusqlite::params![norm],
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
    let norm = aikd_core::platform::normalize_project_key(project);
    let fp = fingerprint(conn, &norm)?;
    let old: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM atlas WHERE project_path=?1",
            rusqlite::params![norm],
            |r| r.get(0),
        )
        .ok();
    match old {
        Some(o) if o == fp => Ok(false),
        Some(_) => {
            conn.execute(
                "UPDATE atlas SET status='stale' WHERE project_path=?1",
                rusqlite::params![norm],
            )?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Check every stored fresh Atlas snapshot and mark any whose file fingerprint
/// drifted as `stale`. Called after scan/watcher mutations.
pub fn mark_all_stale_if_changed(conn: &Connection) -> Result<usize> {
    let projects: Vec<String> = conn
        .prepare("SELECT project_path FROM atlas WHERE status='fresh'")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))
                .and_then(|rows| rows.collect::<std::result::Result<Vec<String>, _>>())
        })
        .unwrap_or_default();
    let mut marked = 0;
    for p in projects {
        if mark_stale_if_changed(conn, &p)? {
            marked += 1;
        }
    }
    Ok(marked)
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
        assert!(s1
            .repository
            .data
            .frameworks
            .contains(&"Rust/cargo".to_string()));
        let fp1: String = conn
            .query_row(
                "SELECT fingerprint FROM atlas WHERE project_path='/proj'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // Second refresh without changes: fingerprint unchanged, no rebuild needed.
        let (s2, fresh) = refresh_atlas(&conn, "/proj").unwrap();
        assert!(fresh);
        assert_eq!(s2.repository.data.file_count, 2);
        let fp2: String = conn
            .query_row(
                "SELECT fingerprint FROM atlas WHERE project_path='/proj'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fp1, fp2);
        // Touch a file -> stale detected, rebuild picks it up.
        conn.execute(
            "UPDATE files SET size=200 WHERE path='/proj/src/main.rs'",
            [],
        )
        .unwrap();
        assert!(mark_stale_if_changed(&conn, "/proj").unwrap());
        let (s3, _) = refresh_atlas(&conn, "/proj").unwrap();
        assert_eq!(s3.repository.data.file_count, 2);
        let status: String = conn
            .query_row(
                "SELECT status FROM atlas WHERE project_path='/proj'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "fresh");
    }

    #[test]
    fn empty_project_is_empty_not_error() {
        let conn = setup();
        let (s, _) = refresh_atlas(&conn, "/nothing").unwrap();
        assert_eq!(s.repository.data.file_count, 0);
        let status: String = conn
            .query_row(
                "SELECT status FROM atlas WHERE project_path='/nothing'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "empty");
    }

    #[test]
    fn project_prefix_does_not_leak_and_only_active_tasks_open() {
        let conn = setup();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/proj2/extra.rs', 50, 't', 't', 'active', 'h2')",
            [],
        ).unwrap();
        let t =
            crate::memory::save_memory(&conn, "/proj", "ws", "task", "T1", "body", "explicit", 1.0)
                .unwrap();
        crate::memory::mark_stale(&conn, &t.id).unwrap();
        let (snap, _) = refresh_atlas(&conn, "/proj").unwrap();
        assert_eq!(
            snap.repository.data.file_count, 2,
            "must not include /proj2"
        );
        assert_eq!(
            snap.open_tasks.len(),
            0,
            "stale tasks must not appear as open_tasks"
        );
    }
}
