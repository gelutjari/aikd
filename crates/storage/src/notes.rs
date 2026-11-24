//! Human Vault: PARA + Journal notes ("Send to AIKD").
//!
//! Notes are stored twice on purpose:
//! - a human-readable Markdown file under `<runtime_dir>/notes/<folder>/…`
//!   (frontmatter + body, backup-able, editable in any editor), and
//! - a row in `notes` plus extracted `knowledge_links` edges so the graph and
//!   the Agents' `query`/`recall`/`handoff` see them without re-reading disk.
//!
//! `decision|constraint|fact|task` notes are additionally mirrored into
//! `memory` so context packs and handoff include them immediately.

use aikd_core::security::redact_secrets;
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PARA_FOLDERS: [&str; 8] = [
    "Inbox",
    "Journal",
    "Projects",
    "Areas",
    "Resources",
    "Memories",
    "Archive",
    "assets",
];

const NOTE_KINDS: [&str; 7] = [
    "note",
    "decision",
    "constraint",
    "fact",
    "task",
    "prompt",
    "daily_log",
];

const NOTE_SCOPES: [&str; 2] = ["global", "project"];

const MEMORY_MIRROR_KINDS: [&str; 4] = ["decision", "constraint", "fact", "task"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub scope: String,
    pub folder: String,
    pub project: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub source: String,
    pub source_url: String,
    pub file_path: String,
    pub pinned: bool,
    pub redacted_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewNote {
    pub title: Option<String>,
    pub body: String,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub folder: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NotePatch {
    pub title: Option<String>,
    pub body: Option<String>,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub folder: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct NoteFilter {
    pub q: Option<String>,
    pub project: Option<String>,
    pub scope: Option<String>,
    pub folder: Option<String>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub pinned: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderCount {
    pub folder: String,
    pub count: i64,
}

const NOTE_COLS: &str = "id, title, body, kind, scope, folder, project, tags_json, links_json, source, source_url, file_path, pinned, redacted_count, created_at, updated_at";

fn map_note(row: &rusqlite::Row) -> Result<NoteRecord, rusqlite::Error> {
    let tags_json: String = row.get(7)?;
    let links_json: String = row.get(8)?;
    let pinned: i64 = row.get(12)?;
    Ok(NoteRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        kind: row.get(3)?,
        scope: row.get(4)?,
        folder: row.get(5)?,
        project: row.get(6)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        links: serde_json::from_str(&links_json).unwrap_or_default(),
        source: row.get(9)?,
        source_url: row.get(10)?,
        file_path: row.get(11)?,
        pinned: pinned != 0,
        redacted_count: row.get::<_, i64>(13)? as usize,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

/// Create the PARA + Journal skeleton and the shared `_PROTOCOL.md`.
/// Idempotent; safe to call on every request.
pub fn ensure_vault(notes_dir: &Path) -> Result<()> {
    for folder in PARA_FOLDERS {
        std::fs::create_dir_all(notes_dir.join(folder))?;
    }
    let protocol = notes_dir.join("_PROTOCOL.md");
    if !protocol.exists() {
        std::fs::write(&protocol, PROTOCOL_MD)?;
    }
    Ok(())
}

pub const PROTOCOL_MD: &str = r#"# AIKD Shared Memory Protocol

One vault, every agent. Keep entries **short and token-efficient** — this file
is read by AI assistants, not only by humans.

## Folder map (PARA + Journal)

- `Inbox/` — quick captures, unsorted ideas, clipboard dumps.
- `Journal/YYYY/MM/` — `daily_log` entries (what happened, what got done).
- `Projects/` — active project work, specs, debugging notes.
- `Areas/` — ongoing responsibilities (ops, security, finance).
- `Resources/` — reference material, links, snippets.
- `Memories/` — durable `decision` / `constraint` / `fact` notes.
- `Archive/` — retired notes (kept for provenance, out of active views).
- `assets/` — images, screenshots, PDFs referenced from notes.

## Entry template

```
## Worked on
- <what changed, file paths>

## Learned
- <non-obvious fact worth remembering>

## Decisions
- <choice + why>

## In Progress
- <next concrete action>
```

Rules:
1. Prefer `[[wikilinks]]` over prose references so the graph stays connected.
2. Tag with `#lowercase-tags`; tags are shared nodes across projects.
3. Never paste secrets — AIKD redacts them automatically, but do not rely on it.
4. `scope: global` notes apply to every project; `scope: project` to one workspace.
"#;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if (c.is_whitespace() || c == '-' || c == '_' || c == '/' || c == '.') && !dash {
            if !out.is_empty() {
                out.push('-');
                dash = true;
            }
        }
        if out.len() >= 60 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "note".to_string()
    } else {
        trimmed
    }
}

/// Reject folder traversal: only `[A-Za-z0-9_-]` segments joined by `/`.
fn sanitize_folder(raw: &str) -> String {
    let cleaned: Vec<String> = raw
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(|s| {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        "Inbox".to_string()
    } else {
        cleaned.join("/")
    }
}

pub fn default_folder_for_kind(kind: &str) -> String {
    if kind == "daily_log" {
        chrono::Utc::now().format("Journal/%Y/%m").to_string()
    } else {
        "Inbox".to_string()
    }
}

/// `#tag` tokens (lowercase, without `#`), de-duplicated, insertion-ordered.
pub fn extract_tags(body: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            if j > i + 1 {
                let tag: String = chars[i + 1..j].iter().collect::<String>().to_lowercase();
                let all_digits = tag.chars().all(|c| c.is_ascii_digit());
                if !all_digits && tag.len() >= 2 && !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    tags
}

/// `[[wikilink]]` targets, trimmed, insertion-ordered.
pub fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("]]") {
            let target = after[..end]
                .split('|')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !target.is_empty() && !links.contains(&target) {
                links.push(target);
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    links
}

/// `[text](path.md)` targets (local paths only; URLs and anchors skipped).
pub fn extract_md_links(body: &str) -> Vec<String> {
    let mut links: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(close) = rest.find("](") {
        let after = &rest[close + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let raw = after[..end].trim();
        let target = raw.split_whitespace().next().unwrap_or("").to_string();
        let lower = target.to_ascii_lowercase();
        let is_local = !target.is_empty()
            && !lower.starts_with("http://")
            && !lower.starts_with("https://")
            && !lower.starts_with("mailto:")
            && !lower.starts_with('#')
            && !lower.starts_with("data:");
        if is_local && !links.contains(&target) {
            links.push(target);
        }
        rest = &after[end + 1..];
    }
    links
}

/// Path-like tokens in the body, resolved against indexed files.
pub fn extract_file_mentions(conn: &Connection, body: &str) -> Vec<String> {
    let mut mentions: Vec<String> = Vec::new();
    for raw in body.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '(' | ')' | '[' | ']' | '"' | '\'' | '`' | ',' | ';' | '<' | '>'
            )
    }) {
        let token =
            raw.trim_matches(|c: char| matches!(c, '.' | ':' | '*' | '#' | '|' | '{' | '}'));
        if token.len() < 3 || !(token.contains('/') || token.contains('\\')) {
            continue;
        }
        let needle = token.replace('\\', "/");
        if mentions.iter().any(|m| m == &needle) {
            continue;
        }
        let found: Option<String> = conn
            .query_row(
                "SELECT path FROM files WHERE status='active' AND replace(path,'\\','/') LIKE '%' || ?1 || '%' LIMIT 1",
                rusqlite::params![needle],
                |r| r.get(0),
            )
            .ok();
        match found {
            Some(path) => mentions.push(path),
            None => mentions.push(needle),
        }
    }
    mentions
}

fn derive_title(body: &str) -> String {
    for line in body.lines() {
        let t = line.trim().trim_start_matches('#').trim();
        if !t.is_empty() {
            return t.chars().take(80).collect();
        }
    }
    "Untitled note".to_string()
}

fn normalize_kind(kind: Option<&str>) -> String {
    match kind.map(|k| k.trim().to_lowercase()) {
        Some(k) if NOTE_KINDS.contains(&k.as_str()) => k,
        _ => "note".to_string(),
    }
}

fn normalize_scope(scope: Option<&str>) -> String {
    match scope.map(|s| s.trim().to_lowercase()) {
        Some(s) if NOTE_SCOPES.contains(&s.as_str()) => s,
        _ => {
            // Empty project means the note is inherently global.
            "project".to_string()
        }
    }
}

fn normalize_tags(extra: &[String], body_tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in body_tags.into_iter().chain(extra.iter().map(|t| {
        t.trim()
            .trim_start_matches('#')
            .trim()
            .to_lowercase()
            .replace(' ', "-")
    })) {
        let clean = tag.trim_matches('-').to_string();
        if !clean.is_empty() && !out.contains(&clean) {
            out.push(clean);
        }
    }
    out
}

fn frontmatter(note: &NoteRecord) -> String {
    let tags = note.tags.join(", ");
    let esc = |s: &str| s.replace('"', "'").replace('\n', " ");
    format!(
        "---\nid: {}\ntitle: \"{}\"\nkind: {}\nscope: {}\nfolder: {}\nproject: {}\ntags: [{}]\npinned: {}\nsource: {}\ncreated_at: {}\nupdated_at: {}\n---\n\n",
        note.id,
        esc(&note.title),
        note.kind,
        note.scope,
        note.folder,
        esc(&note.project),
        tags,
        note.pinned,
        note.source,
        note.created_at,
        note.updated_at
    )
}

/// Render a note as its on-disk Markdown representation.
pub fn note_markdown(note: &NoteRecord) -> String {
    format!("{}{}\n", frontmatter(note), note.body)
}

fn note_file_path(notes_dir: &Path, note: &NoteRecord) -> PathBuf {
    let slug = slugify(&note.title);
    let short: String = note.id.chars().take(8).collect();
    let folder = sanitize_folder(&note.folder);
    notes_dir.join(folder).join(format!("{slug}-{short}.md"))
}

fn write_note_file(notes_dir: &Path, note: &NoteRecord) -> String {
    let path = note_file_path(notes_dir, note);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(notes_dir.join("assets"));
    match std::fs::write(&path, note_markdown(note)) {
        Ok(()) => aikd_core::platform::canonical_path(&path),
        Err(e) => {
            log::warn!("notes: cannot write {}: {e}", path.display());
            String::new()
        }
    }
}

/// Replace this note's outgoing knowledge edges (idempotent per note).
pub fn rebuild_links(conn: &Connection, note: &NoteRecord) -> Result<usize> {
    let from_id = format!("note:{}", note.id);
    conn.execute(
        "DELETE FROM knowledge_links WHERE from_id=?1 AND from_kind='note'",
        rusqlite::params![from_id],
    )?;
    let now = now_rfc3339();
    let mut inserted = 0usize;

    let mut add = |to_id: &str, to_kind: &str, relation: &str| -> Result<()> {
        let n = conn.execute(
            "INSERT OR IGNORE INTO knowledge_links (from_id, from_kind, to_id, to_kind, relation, provenance, created_at) VALUES (?1,'note',?2,?3,?4,'EXTRACTED',?5)",
            rusqlite::params![from_id, to_id, to_kind, relation, now],
        )?;
        inserted += n;
        Ok(())
    };

    for tag in &note.tags {
        add(&format!("tag:{tag}"), "tag", "tag")?;
    }
    for target in extract_wikilinks(&note.body) {
        // Prefer the real note id when a title matches; otherwise keep a
        // stable slug so the graph still renders the reference.
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM notes WHERE lower(title)=lower(?1) LIMIT 1",
                rusqlite::params![target],
                |r| r.get(0),
            )
            .ok();
        let to_id = match existing {
            Some(id) => format!("note:{id}"),
            None => format!("note:{}", slugify(&target)),
        };
        add(&to_id, "note", "wikilink")?;
    }
    for target in extract_md_links(&note.body) {
        for path in extract_file_mentions(conn, &target) {
            add(&path, "file", "mentions")?;
        }
    }
    for path in extract_file_mentions(conn, &note.body) {
        add(&path, "file", "mentions")?;
    }
    Ok(inserted)
}

fn mirror_to_memory(conn: &Connection, note: &NoteRecord) -> Result<()> {
    if !MEMORY_MIRROR_KINDS.contains(&note.kind.as_str()) {
        return Ok(());
    }
    let project = if note.scope == "global" {
        String::new()
    } else {
        note.project.clone()
    };
    let body = format!("{}\n\n[note:{}]", note.body, note.id);
    // Avoid duplicate mirrors of the same note (update-safe).
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM memory WHERE body LIKE ?1 LIMIT 1",
            rusqlite::params![format!("%[note:{}]%", note.id)],
            |r| r.get(0),
        )
        .ok();
    if exists.is_some() {
        return Ok(());
    }
    let _ = crate::memory::save_memory(
        conn,
        &project,
        "notes",
        &note.kind,
        &note.title,
        &body,
        "explicit",
        1.0,
    )?;
    Ok(())
}

/// Create a note: redact → extract → write Markdown → store row + edges + memory.
pub fn create_note(conn: &Connection, notes_dir: &Path, input: NewNote) -> Result<NoteRecord> {
    ensure_vault(notes_dir)?;

    let raw_title = input.title.unwrap_or_default();
    let (title_clean, title_redacted) = if raw_title.trim().is_empty() {
        (String::new(), 0usize)
    } else {
        let r = redact_secrets(&raw_title);
        (r.text, r.redacted_count)
    };
    let body_red = redact_secrets(&input.body);
    let redacted_count = title_redacted + body_red.redacted_count;

    let kind = normalize_kind(input.kind.as_deref());
    let folder = match input.folder {
        Some(f) if !f.trim().is_empty() => sanitize_folder(&f),
        _ => default_folder_for_kind(&kind),
    };
    let project =
        aikd_core::platform::normalize_project_key(input.project.as_deref().unwrap_or(""));
    let scope = normalize_scope(input.scope.as_deref());
    let scope = if project.is_empty() && scope == "project" {
        // A note with no project can only be global.
        "global".to_string()
    } else {
        scope
    };

    let body_tags = extract_tags(&body_red.text);
    let tags = normalize_tags(input.tags.as_deref().unwrap_or(&[]), body_tags);
    let file_links = extract_file_mentions(conn, &body_red.text);

    let title = if title_clean.trim().is_empty() {
        derive_title(&body_red.text)
    } else {
        title_clean
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let mut note = NoteRecord {
        id,
        title,
        body: body_red.text,
        kind,
        scope,
        folder,
        project,
        tags,
        links: file_links,
        source: input.source.unwrap_or_else(|| "human".to_string()),
        source_url: input.source_url.unwrap_or_default(),
        file_path: String::new(),
        pinned: input.pinned.unwrap_or(false),
        redacted_count,
        created_at: now.clone(),
        updated_at: now,
    };
    note.file_path = write_note_file(notes_dir, &note);

    conn.execute(
        "INSERT INTO notes (id, title, body, kind, scope, folder, project, tags_json, links_json, source, source_url, file_path, pinned, redacted_count, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        rusqlite::params![
            note.id,
            note.title,
            note.body,
            note.kind,
            note.scope,
            note.folder,
            note.project,
            serde_json::to_string(&note.tags)?,
            serde_json::to_string(&note.links)?,
            note.source,
            note.source_url,
            note.file_path,
            if note.pinned { 1i64 } else { 0i64 },
            note.redacted_count as i64,
            note.created_at,
            note.updated_at
        ],
    )?;

    rebuild_links(conn, &note)?;
    mirror_to_memory(conn, &note)?;
    Ok(note)
}

pub fn get_note(conn: &Connection, id: &str) -> Result<Option<NoteRecord>> {
    let mut stmt = conn.prepare(&format!("SELECT {NOTE_COLS} FROM notes WHERE id=?1"))?;
    let mut rows = stmt.query_map(rusqlite::params![id], map_note)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// List notes plus tag/folder facets (counts computed over the filtered set).
pub fn list_notes(
    conn: &Connection,
    filter: &NoteFilter,
) -> Result<(Vec<NoteRecord>, Vec<TagCount>, Vec<FolderCount>)> {
    let mut sql = format!("SELECT {NOTE_COLS} FROM notes WHERE 1=1");
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(q) = filter.q.as_deref().filter(|s| !s.trim().is_empty()) {
        binds.push(Box::new(format!("%{}%", q.trim())));
        sql.push_str(&format!(
            " AND (title LIKE ?{} OR body LIKE ?{})",
            binds.len(),
            binds.len()
        ));
    }
    if let Some(p) = filter.project.as_deref().filter(|s| !s.trim().is_empty()) {
        binds.push(Box::new(aikd_core::platform::normalize_project_key(p)));
        sql.push_str(&format!(" AND project=?{}", binds.len()));
    }
    if let Some(s) = filter.scope.as_deref().filter(|s| !s.trim().is_empty()) {
        binds.push(Box::new(s.to_string()));
        sql.push_str(&format!(" AND scope=?{}", binds.len()));
    }
    if let Some(f) = filter.folder.as_deref().filter(|s| !s.trim().is_empty()) {
        binds.push(Box::new(format!("{}%", sanitize_folder(f))));
        sql.push_str(&format!(" AND folder LIKE ?{}", binds.len()));
    }
    if let Some(k) = filter.kind.as_deref().filter(|s| !s.trim().is_empty()) {
        binds.push(Box::new(k.to_string()));
        sql.push_str(&format!(" AND kind=?{}", binds.len()));
    }
    if let Some(t) = filter.tag.as_deref().filter(|s| !s.trim().is_empty()) {
        let tag = t.trim().trim_start_matches('#').to_lowercase();
        binds.push(Box::new(format!("%\"{tag}\"%")));
        sql.push_str(&format!(" AND tags_json LIKE ?{}", binds.len()));
    }
    if let Some(pinned) = filter.pinned {
        sql.push_str(if pinned {
            " AND pinned=1"
        } else {
            " AND pinned=0"
        });
    }
    sql.push_str(" ORDER BY pinned DESC, datetime(updated_at) DESC LIMIT ?");
    let limit = filter.limit.unwrap_or(100).clamp(1, 1000);
    binds.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(&refs[..], map_note)?;
    let notes: Vec<NoteRecord> = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(anyhow::Error::from)?;

    let mut tag_counts: Vec<TagCount> = Vec::new();
    let mut folder_counts: Vec<FolderCount> = Vec::new();
    for note in &notes {
        for tag in &note.tags {
            match tag_counts.iter_mut().find(|t| &t.tag == tag) {
                Some(t) => t.count += 1,
                None => tag_counts.push(TagCount {
                    tag: tag.clone(),
                    count: 1,
                }),
            }
        }
        let root = note.folder.split('/').next().unwrap_or("Inbox").to_string();
        match folder_counts.iter_mut().find(|f| f.folder == root) {
            Some(f) => f.count += 1,
            None => folder_counts.push(FolderCount {
                folder: root,
                count: 1,
            }),
        }
    }
    tag_counts.sort_by(|a, b| b.count.cmp(&a.count).then(a.tag.cmp(&b.tag)));
    folder_counts.sort_by(|a, b| a.folder.cmp(&b.folder));
    Ok((notes, tag_counts, folder_counts))
}

fn remove_note_file(note: &NoteRecord) {
    if !note.file_path.is_empty() {
        let _ = std::fs::remove_file(Path::new(&note.file_path));
    }
}

/// Update a note in place: re-redact, re-extract, rewrite Markdown.
pub fn update_note(
    conn: &Connection,
    notes_dir: &Path,
    id: &str,
    patch: NotePatch,
) -> Result<Option<NoteRecord>> {
    let Some(mut note) = get_note(conn, id)? else {
        return Ok(None);
    };
    ensure_vault(notes_dir)?;
    let old_folder = note.folder.clone();

    let mut redacted = note.redacted_count;
    if let Some(title) = patch.title {
        let r = redact_secrets(&title);
        redacted += r.redacted_count;
        note.title = r.text;
    }
    if let Some(body) = patch.body {
        let r = redact_secrets(&body);
        redacted += r.redacted_count;
        note.body = r.text;
    }
    if let Some(kind) = patch.kind {
        note.kind = normalize_kind(Some(&kind));
    }
    if let Some(scope) = patch.scope {
        note.scope = normalize_scope(Some(&scope));
    }
    if let Some(project) = patch.project {
        note.project = aikd_core::platform::normalize_project_key(&project);
    }
    if let Some(folder) = patch.folder.filter(|f| !f.trim().is_empty()) {
        note.folder = sanitize_folder(&folder);
    }
    if let Some(source) = patch.source {
        note.source = source;
    }
    if let Some(url) = patch.source_url {
        note.source_url = url;
    }
    if let Some(pinned) = patch.pinned {
        note.pinned = pinned;
    }
    if note.project.is_empty() && note.scope == "project" {
        note.scope = "global".to_string();
    }

    let body_tags = extract_tags(&note.body);
    let extra: Vec<String> = patch.tags.unwrap_or_default();
    let tags = normalize_tags(
        &extra,
        if extra.is_empty() {
            body_tags
        } else {
            body_tags.into_iter().chain(note.tags.clone()).collect()
        },
    );
    note.tags = tags;
    note.links = extract_file_mentions(conn, &note.body);
    note.redacted_count = redacted;
    note.updated_at = now_rfc3339();

    // Folder changed → move the Markdown file instead of leaving a stale copy.
    if old_folder != note.folder && !note.file_path.is_empty() {
        let _ = std::fs::remove_file(Path::new(&note.file_path));
    }
    note.file_path = write_note_file(notes_dir, &note);

    conn.execute(
        "UPDATE notes SET title=?1, body=?2, kind=?3, scope=?4, folder=?5, project=?6, tags_json=?7, links_json=?8, source=?9, source_url=?10, file_path=?11, pinned=?12, redacted_count=?13, updated_at=?14 WHERE id=?15",
        rusqlite::params![
            note.title,
            note.body,
            note.kind,
            note.scope,
            note.folder,
            note.project,
            serde_json::to_string(&note.tags)?,
            serde_json::to_string(&note.links)?,
            note.source,
            note.source_url,
            note.file_path,
            if note.pinned { 1i64 } else { 0i64 },
            note.redacted_count as i64,
            note.updated_at,
            note.id
        ],
    )?;
    rebuild_links(conn, &note)?;
    Ok(Some(note))
}

/// Delete a note, its edges, and its Markdown file.
pub fn delete_note(conn: &Connection, id: &str) -> Result<bool> {
    let Some(note) = get_note(conn, id)? else {
        return Ok(false);
    };
    conn.execute(
        "DELETE FROM knowledge_links WHERE (from_id=?1 AND from_kind='note') OR (to_id=?1 AND to_kind='note')",
        rusqlite::params![format!("note:{}", note.id)],
    )?;
    conn.execute("DELETE FROM notes WHERE id=?1", rusqlite::params![note.id])?;
    remove_note_file(&note);
    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
pub struct MorningBrief {
    pub bullets: Vec<String>,
    pub done: Vec<String>,
    pub stuck: Vec<String>,
    pub focus: Vec<String>,
    pub learned: Vec<String>,
    pub markdown: String,
}

fn looks_blocked(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["blocked", "stuck", "waiting", "need help", "buntu"]
        .iter()
        .any(|k| lower.contains(k))
}

fn clip(s: &str, n: usize) -> String {
    let line = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    line.chars().take(n).collect()
}

/// "Morning Brief & Check-Before-Start" (`GET /api/brief`).
pub fn morning_brief(conn: &Connection, days: i64, project: &str) -> Result<MorningBrief> {
    let days = days.clamp(1, 90);
    let window = format!("-{days} days");
    let norm_project = aikd_core::platform::normalize_project_key(project);
    let mut bullets = Vec::new();
    let mut done = Vec::new();
    let mut stuck = Vec::new();
    let mut focus = Vec::new();
    let mut learned = Vec::new();

    // Recent daily logs (and their `DONE` / `Learned` sections).
    {
        let mut stmt = conn.prepare(
            "SELECT title, body FROM notes WHERE kind='daily_log' AND datetime(updated_at) >= datetime('now', ?1) ORDER BY datetime(updated_at) DESC LIMIT 10",
        )?;
        let rows = stmt.query_map(rusqlite::params![window], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows.flatten() {
            let (title, body) = row;
            bullets.push(format!("Daily log: {title}"));
            let mut section = "";
            for line in body.lines() {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                let low = t.to_lowercase();
                if low.starts_with("## ") || low.starts_with('#') {
                    section = if low.contains("done") {
                        "done"
                    } else if low.contains("learn") {
                        "learned"
                    } else if low.contains("progress")
                        || low.contains("next")
                        || low.contains("focus")
                    {
                        "focus"
                    } else {
                        ""
                    };
                    continue;
                }
                let item = t.trim_start_matches('-').trim().to_string();
                if item.is_empty() || item.len() > 220 {
                    continue;
                }
                match section {
                    "done" => done.push(item),
                    "learned" => learned.push(item),
                    "focus" => focus.push(item),
                    _ => {
                        if looks_blocked(&item) {
                            stuck.push(item);
                        }
                    }
                }
            }
        }
    }

    // Active memory: decisions/learned become brief items, tasks become focus.
    {
        let stmt_sql = "SELECT kind, title, body, status FROM memory WHERE status='active' AND datetime(updated_at) >= datetime('now', ?1) AND (?2 = '' OR project_path = ?2 OR project_path = '') ORDER BY datetime(updated_at) DESC LIMIT 40";
        let mut stmt = conn.prepare(stmt_sql)?;
        let rows = stmt.query_map(rusqlite::params![window, norm_project], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows.flatten() {
            let (kind, title, body, _status) = row;
            let line = format!("[{kind}] {title} — {}", clip(&body, 140));
            match kind.as_str() {
                "task" => {
                    if looks_blocked(&body) {
                        stuck.push(format!("{title}: {}", clip(&body, 120)));
                    } else {
                        focus.push(format!("{title}: {}", clip(&body, 120)));
                    }
                }
                "constraint" => focus.push(format!("{title}: {}", clip(&body, 120))),
                "decision" => {
                    bullets.push(line.clone());
                    learned.push(format!("Decision: {title}"));
                }
                "fact" | "discovery" => {
                    bullets.push(line);
                    learned.push(format!("Learned: {title}"));
                }
                _ => bullets.push(line),
            }
        }
    }

    // Active constraints, regardless of age — these are the guardrails.
    {
        let mut stmt = conn.prepare(
            "SELECT title, body FROM memory WHERE kind='constraint' AND status='active' AND (?1 = '' OR project_path = ?1 OR project_path = '') ORDER BY datetime(updated_at) DESC LIMIT 5",
        )?;
        let rows = stmt.query_map(rusqlite::params![norm_project], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows.flatten() {
            focus.push(format!("constraint: {} — {}", row.0, clip(&row.1, 120)));
        }
    }

    // Recent agent events give a "what happened while you were away" feed.
    {
        let mut stmt = conn.prepare(
            "SELECT agent_id, event_type, payload FROM events WHERE datetime(timestamp) >= datetime('now', ?1) ORDER BY sequence DESC LIMIT 10",
        )?;
        let rows = stmt.query_map(rusqlite::params![window], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows.flatten() {
            let (agent, ev, payload) = row;
            let summary = serde_json::from_str::<serde_json::Value>(&payload)
                .ok()
                .and_then(|v| {
                    v.get("summary")
                        .or_else(|| v.get("query"))
                        .or_else(|| v.get("tool"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            bullets.push(format!(
                "{agent} · {ev}{}",
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {}", clip(&summary, 100))
                }
            ));
        }
    }

    if focus.is_empty() {
        focus.push("No active tasks or constraints — pick the next slice of work.".to_string());
    }
    if learned.is_empty() {
        learned.push("No new learned facts recorded in this window.".to_string());
    }

    let mut markdown = String::new();
    markdown.push_str(&format!(
        "# Morning Brief · {} (last {} day(s))\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        days
    ));
    let mut section = |title: &str, items: &[String]| {
        markdown.push_str(&format!("\n## {title}\n"));
        if items.is_empty() {
            markdown.push_str("- _nothing_\n");
        } else {
            for i in items.iter().take(12) {
                markdown.push_str(&format!("- {i}\n"));
            }
        }
    };
    section("Focus / Check Before Start", &focus);
    section("Learned", &learned);
    section("Done", &done);
    section("Stuck / Blocked", &stuck);
    section("Recent Activity", &bullets);

    Ok(MorningBrief {
        bullets,
        done,
        stuck,
        focus,
        learned,
        markdown,
    })
}

/// Full backup bundle: active notes + active memories ("1-click Export").
pub fn export_bundle(conn: &Connection) -> Result<serde_json::Value> {
    let (notes, _, _) = list_notes(
        conn,
        &NoteFilter {
            limit: Some(1000),
            ..Default::default()
        },
    )?;
    let memories = crate::memory::all_active(conn)?;
    Ok(serde_json::json!({
        "format": "aikd-backup-v1",
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": now_rfc3339(),
        "notes_count": notes.len(),
        "memories_count": memories.len(),
        "notes": notes,
        "memories": memories,
    }))
}

/// Markdown archive variant of [`export_bundle`] (notes only, full bodies).
pub fn export_markdown(conn: &Connection) -> Result<String> {
    let (notes, _, _) = list_notes(
        conn,
        &NoteFilter {
            limit: Some(1000),
            ..Default::default()
        },
    )?;
    let mut out = String::new();
    out.push_str(&format!(
        "# AIKD Notes Backup · {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    ));
    for note in notes {
        out.push_str(&format!(
            "\n---\n\n# {}\n\n_{} · {} · {}_\n\n{}\n",
            note.title, note.kind, note.scope, note.folder, note.body
        ));
    }
    Ok(out)
}

/// Count of human notes (dashboard KPI).
pub fn notes_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn
    }

    fn new_note(body: &str) -> NewNote {
        NewNote {
            body: body.to_string(),
            kind: Some("note".into()),
            project: Some("C:\\p".into()),
            source: Some("ui".into()),
            ..Default::default()
        }
    }

    #[test]
    fn vault_skeleton_and_protocol_created() {
        let dir = tempfile::tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        ensure_vault(&notes_dir).unwrap();
        for folder in PARA_FOLDERS {
            assert!(notes_dir.join(folder).is_dir(), "missing {folder}");
        }
        assert!(notes_dir.join("_PROTOCOL.md").is_file());
        // Idempotent.
        ensure_vault(&notes_dir).unwrap();
    }

    #[test]
    fn extracts_tags_wikilinks_and_file_mentions() {
        let tags = extract_tags("Fix #Auth bug, see #auth and #tray-work, not #123");
        assert_eq!(tags, vec!["auth", "tray-work"]);
        let links = extract_wikilinks("See [[PR Review Checklist]] and [[Arch|Architecture]]");
        assert_eq!(links, vec!["PR Review Checklist", "Arch"]);
        let md = extract_md_links(
            "see [tray](assets/tray.png) and [x](https://example.com) and [y](#anchor) and [z](../Projects/a.md \"title\")",
        );
        assert_eq!(md, vec!["assets/tray.png", "../Projects/a.md"]);
    }

    #[test]
    fn create_redacts_writes_markdown_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let conn = setup();
        conn.execute(
            "INSERT INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES ('/p/crates/app/src/tray.rs', 10, 't', 't', 'active', 'h')",
            [],
        )
        .unwrap();

        let note = create_note(
            &conn,
            &notes_dir,
            NewNote {
                title: Some("Tray notes".into()),
                body: "key sk-abcdefghijklmnop1234 and [[Other Note]] #tray\nsee crates/app/src/tray.rs".into(),
                kind: Some("note".into()),
                project: Some("C:\\p".into()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(note.redacted_count, 1);
        assert!(!note.body.contains("sk-abc"));
        assert!(note.tags.contains(&"tray".to_string()));
        assert!(note.file_path.ends_with(".md"));
        assert!(Path::new(&note.file_path).is_file());
        let md = std::fs::read_to_string(&note.file_path).unwrap();
        assert!(md.starts_with("---"));
        assert!(md.contains("title: \"Tray notes\""));

        let links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_links WHERE from_id=?1",
                rusqlite::params![format!("note:{}", note.id)],
                |r| r.get(0),
            )
            .unwrap();
        // #tray + [[Other Note]] + file mention
        assert_eq!(links, 3);
    }

    #[test]
    fn daily_log_defaults_to_journal_folder() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        let note = create_note(
            &conn,
            &dir.path().join("notes"),
            NewNote {
                body: "worked on things".into(),
                kind: Some("daily_log".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(note.folder.starts_with("Journal/"));
        assert_eq!(note.scope, "global");
    }

    #[test]
    fn mirror_decision_into_memory() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        let note = create_note(
            &conn,
            &dir.path().join("notes"),
            NewNote {
                body: "Use SQLite WAL".into(),
                kind: Some("decision".into()),
                project: Some("C:/p".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let mem = crate::memory::list_memory(&conn, "C:/p", Some("notes"), Some("decision"), false)
            .unwrap();
        assert_eq!(mem.len(), 1);
        assert!(mem[0].body.contains(&format!("[note:{}]", note.id)));
    }

    #[test]
    fn list_filter_update_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let conn = setup();
        let n1 = create_note(&conn, &notes_dir, new_note("alpha #one")).unwrap();
        let n2 = create_note(&conn, &notes_dir, new_note("beta #two")).unwrap();

        let (all, tags, folders) = list_notes(
            &conn,
            &NoteFilter {
                limit: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(all.len(), 2);
        assert!(tags.iter().any(|t| t.tag == "one"));
        assert!(folders.iter().any(|f| f.folder == "Inbox"));

        let (only_two, _, _) = list_notes(
            &conn,
            &NoteFilter {
                tag: Some("two".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(only_two.len(), 1);
        assert_eq!(only_two[0].id, n2.id);

        let updated = update_note(
            &conn,
            &notes_dir,
            &n1.id,
            NotePatch {
                title: Some("alpha renamed".into()),
                body: Some("token=supersecretvalue1234567890 #three".into()),
                pinned: Some(true),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.title, "alpha renamed");
        assert!(updated.body.contains("[REDACTED]"));
        assert!(updated.pinned);
        assert!(updated.tags.contains(&"three".to_string()));
        let md = std::fs::read_to_string(&updated.file_path).unwrap();
        assert!(md.contains("[REDACTED]"));

        assert!(delete_note(&conn, &n1.id).unwrap());
        assert!(get_note(&conn, &n1.id).unwrap().is_none());
        assert!(!Path::new(&updated.file_path).exists());
        let remaining_links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_links WHERE from_id=?1",
                rusqlite::params![format!("note:{}", n1.id)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining_links, 0);
    }

    #[test]
    fn morning_brief_reports_focus_and_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        create_note(
            &conn,
            &dir.path().join("notes"),
            NewNote {
                body: "## Done\n- shipped tray\n## Learned\n- sysinfo parent pid works\n".into(),
                kind: Some("daily_log".into()),
                project: Some("C:/p".into()),
                ..Default::default()
            },
        )
        .unwrap();
        crate::memory::save_memory(
            &conn,
            "C:/p",
            "ws",
            "task",
            "T-1",
            "IN_PROGRESS blocked on CI",
            "explicit",
            1.0,
        )
        .unwrap();
        crate::memory::save_memory(
            &conn,
            "C:/p",
            "ws",
            "constraint",
            "Never touch prod :9090",
            "zero downtime",
            "explicit",
            1.0,
        )
        .unwrap();

        let brief = morning_brief(&conn, 3, "C:/p").unwrap();
        assert!(brief.markdown.contains("# Morning Brief"));
        assert!(brief.done.iter().any(|d| d.contains("shipped tray")));
        assert!(brief.learned.iter().any(|l| l.contains("sysinfo")));
        assert!(brief.stuck.iter().any(|s| s.contains("CI")));
        assert!(brief.focus.iter().any(|f| f.contains("9090")));
    }

    #[test]
    fn export_bundle_contains_notes_and_memories() {
        let dir = tempfile::tempdir().unwrap();
        let conn = setup();
        create_note(
            &conn,
            &dir.path().join("notes"),
            NewNote {
                body: "export me".into(),
                kind: Some("fact".into()),
                project: Some("C:/p".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let bundle = export_bundle(&conn).unwrap();
        assert_eq!(bundle["notes_count"], 1);
        assert_eq!(bundle["memories_count"], 1);
        assert!(export_markdown(&conn).unwrap().contains("export me"));
    }
}
