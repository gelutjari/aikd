//! `aikd hooks install|status` — zero-friction auto-sync hooks.
//!
//! * Git: idempotently upserts a managed block into `.git/hooks/post-commit`
//!   and `post-merge` without touching the user's own hook code. The block
//!   runs `aikd scan --changed --quiet` detached (never blocks the commit).
//! * Agents: safely merges `SessionStart` / `PreCompact` command hooks into
//!   Claude Code's `settings.json`, preserving every existing key and hook.
//! * OpenCode and Gemini configurations are detected for status reporting;
//!   AIKD does not rewrite them here.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const AIKD_START: &str = "# AIKD-HOOK-START";
const AIKD_END: &str = "# AIKD-HOOK-END";
const HANDOFF_COMMAND: &str = "aikd handoff --compact";
const CLAUDE_EVENTS: [&str; 2] = ["SessionStart", "PreCompact"];
const GIT_HOOK_NAMES: [&str; 2] = ["post-commit", "post-merge"];

const AIKD_HOOK_BLOCK: &str = concat!(
    "# AIKD-HOOK-START\n",
    "# Managed by `aikd hooks install`; edits inside this block are overwritten.\n",
    "# Index changed files after git operations without blocking the commit.\n",
    "if command -v aikd >/dev/null 2>&1; then\n",
    "    ( aikd scan --changed --quiet >/dev/null 2>&1 & )\n",
    "fi\n",
    "# AIKD-HOOK-END"
);

/* ------------------------------------------------------------------ public */

/// Install Git and/or Claude Code hooks. Defaults to both when neither flag
/// is supplied.
pub fn run_install(
    project: Option<&str>,
    git: bool,
    agents: bool,
    claude_settings: Option<&str>,
    json: bool,
) -> Result<()> {
    let project_dir = resolve_project(project)?;
    let install_git = git || !agents;
    let install_agents = agents || !git;

    let claude_path = match claude_settings {
        Some(custom) => Some(resolve_path(custom)?),
        None => default_claude_settings(),
    };

    if install_git {
        install_git_hooks(&project_dir)?;
    }
    if install_agents {
        let path = claude_path.as_deref().ok_or_else(|| {
            anyhow!(
                "cannot locate a home directory for Claude settings; pass --claude-settings <path>"
            )
        })?;
        install_claude_hooks(path)?;
    }

    let status = collect_status(&project_dir, claude_path.as_deref());
    if json {
        let mut value = status.to_json();
        if let Some(obj) = value.as_object_mut() {
            obj.insert("action".to_string(), json!("install"));
            obj.insert("git_updated".to_string(), json!(install_git));
            obj.insert("agents_updated".to_string(), json!(install_agents));
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("AIKD hooks install");
        if install_git {
            println!("  [OK] Git hooks updated");
        }
        if install_agents {
            println!("  [OK] Claude Code hooks updated");
        }
        status.print_human();
    }
    Ok(())
}

/// Report whether Git and agent hooks are currently installed.
pub fn run_status(project: Option<&str>, json: bool) -> Result<()> {
    let project_dir = resolve_project(project)?;
    let claude_path = default_claude_settings();
    let status = collect_status(&project_dir, claude_path.as_deref());

    if json {
        println!("{}", serde_json::to_string_pretty(&status.to_json())?);
    } else {
        println!("AIKD hooks status");
        status.print_human();
    }
    Ok(())
}

/* ------------------------------------------------------------------ status */

struct GitStatus {
    available: bool,
    hooks_dir: Option<PathBuf>,
    post_commit: bool,
    post_merge: bool,
}

impl GitStatus {
    fn installed(&self) -> bool {
        self.available && self.post_commit && self.post_merge
    }
}

struct ClaudeStatus {
    path: Option<PathBuf>,
    present: bool,
    session_start: bool,
    pre_compact: bool,
    error: Option<String>,
}

impl ClaudeStatus {
    fn installed(&self) -> bool {
        self.present && self.session_start && self.pre_compact
    }
}

struct Presence {
    path: Option<PathBuf>,
    present: bool,
}

struct HooksStatus {
    project: PathBuf,
    git: GitStatus,
    claude: ClaudeStatus,
    opencode: Presence,
    gemini: Presence,
}

impl HooksStatus {
    fn to_json(&self) -> Value {
        json!({
            "project": self.project.display().to_string(),
            "git": {
                "available": self.git.available,
                "hooks_dir": self.git.hooks_dir.as_deref().map(path_string),
                "post_commit": self.git.post_commit,
                "post_merge": self.git.post_merge,
                "installed": self.git.installed()
            },
            "agents": {
                "claude": {
                    "path": self.claude.path.as_deref().map(path_string),
                    "present": self.claude.present,
                    "session_start": self.claude.session_start,
                    "pre_compact": self.claude.pre_compact,
                    "installed": self.claude.installed(),
                    "error": self.claude.error
                },
                "opencode": presence_json(&self.opencode),
                "gemini": presence_json(&self.gemini)
            }
        })
    }

    fn print_human(&self) {
        println!("Project: {}", self.project.display());
        if self.git.available {
            let dir = self
                .git
                .hooks_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            println!("Git hooks ({dir}):");
            println!("  {} post-commit", mark(self.git.post_commit));
            println!("  {} post-merge", mark(self.git.post_merge));
        } else {
            println!("Git hooks: no .git repository detected");
        }

        match self.claude.path.as_ref() {
            Some(path) => println!("Claude Code hooks ({})", path.display()),
            None => println!("Claude Code hooks: home directory unavailable"),
        }
        if self.claude.present {
            println!("  {} SessionStart", mark(self.claude.session_start));
            println!("  {} PreCompact", mark(self.claude.pre_compact));
        } else if self.claude.path.is_some() {
            println!("  [MISSING] settings.json not found (run: aikd hooks install --agents)");
        }
        if let Some(err) = self.claude.error.as_deref() {
            println!("  [WARN] settings not parseable: {err}");
        }

        println!(
            "OpenCode config: {} ({})",
            presence_word(&self.opencode),
            presence_path(&self.opencode)
        );
        println!(
            "Gemini config: {} ({})",
            presence_word(&self.gemini),
            presence_path(&self.gemini)
        );
    }
}

fn presence_json(presence: &Presence) -> Value {
    json!({
        "path": presence.path.as_deref().map(path_string),
        "present": presence.present
    })
}

fn presence_word(presence: &Presence) -> &'static str {
    if presence.present {
        "detected (presence only)"
    } else {
        "not detected"
    }
}

fn presence_path(presence: &Presence) -> String {
    presence
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "home directory unavailable".to_string())
}

fn mark(installed: bool) -> &'static str {
    if installed {
        "[OK]"
    } else {
        "[MISSING]"
    }
}

fn collect_status(project: &Path, claude_path: Option<&Path>) -> HooksStatus {
    let git = match resolve_git_dir(project) {
        Some(git_dir) => {
            let hooks_dir = git_dir.join("hooks");
            GitStatus {
                available: true,
                post_commit: hook_installed(&hooks_dir.join("post-commit")),
                post_merge: hook_installed(&hooks_dir.join("post-merge")),
                hooks_dir: Some(hooks_dir),
            }
        }
        None => GitStatus {
            available: false,
            hooks_dir: None,
            post_commit: false,
            post_merge: false,
        },
    };

    HooksStatus {
        project: project.to_path_buf(),
        git,
        claude: read_claude_status(claude_path),
        opencode: read_presence(opencode_config_path()),
        gemini: read_presence(gemini_config_path()),
    }
}

fn read_claude_status(path: Option<&Path>) -> ClaudeStatus {
    let Some(path) = path else {
        return ClaudeStatus {
            path: None,
            present: false,
            session_start: false,
            pre_compact: false,
            error: None,
        };
    };

    let present = path.exists();
    let mut status = ClaudeStatus {
        path: Some(path.to_path_buf()),
        present,
        session_start: false,
        pre_compact: false,
        error: None,
    };
    if !present {
        return status;
    }

    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => {}
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(settings) => {
                status.session_start = claude_event_installed(&settings, "SessionStart");
                status.pre_compact = claude_event_installed(&settings, "PreCompact");
            }
            Err(err) => status.error = Some(err.to_string()),
        },
        Err(err) => status.error = Some(err.to_string()),
    }
    status
}

fn read_presence(path: Option<PathBuf>) -> Presence {
    let present = path.as_ref().is_some_and(|p| p.exists());
    Presence { path, present }
}

/* --------------------------------------------------------------- git hooks */

/// Upsert the managed AIKD block into both Git hooks. Returns the hooks dir.
fn install_git_hooks(project: &Path) -> Result<PathBuf> {
    let git_dir = resolve_git_dir(project).ok_or_else(|| {
        anyhow!(
            "no .git repository found at or above {}; run inside a repo, pass --project <repo>, or use --agents",
            project.display()
        )
    })?;
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).with_context(|| format!("creating {}", hooks_dir.display()))?;

    for name in GIT_HOOK_NAMES {
        let path = hooks_dir.join(name);
        let existing = match fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", path.display()));
            }
        };
        let updated = upsert_hook_script(existing.as_deref(), AIKD_HOOK_BLOCK);
        fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        make_executable(&path)?;
    }
    Ok(hooks_dir)
}

/// Idempotently append or replace the managed block while preserving the
/// user's existing hook script around it.
fn upsert_hook_script(existing: Option<&str>, block: &str) -> String {
    let Some(text) = existing else {
        return format!("#!/bin/sh\n\n{block}\n");
    };

    if let Some(start) = text.find(AIKD_START) {
        let end = text[start..]
            .find(AIKD_END)
            .map(|rel| start + rel + AIKD_END.len())
            .unwrap_or(text.len());
        let suffix = &text[end..];
        let mut out = String::with_capacity(text.len() + block.len());
        out.push_str(&text[..start]);
        out.push_str(block);
        if !suffix.is_empty() && !suffix.starts_with('\n') {
            out.push('\n');
        }
        out.push_str(suffix);
        ensure_trailing_newline(&mut out);
        return out;
    }

    let mut out = text.to_string();
    if out.trim().is_empty() {
        out.clear();
        out.push_str("#!/bin/sh\n\n");
    } else if out.ends_with('\n') {
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    } else {
        out.push_str("\n\n");
    }
    out.push_str(block);
    out.push('\n');
    out
}

fn ensure_trailing_newline(text: &mut String) {
    if !text.ends_with('\n') {
        text.push('\n');
    }
}

fn hook_installed(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| {
            text.contains(AIKD_START)
                && text.contains(AIKD_END)
                && text.contains("aikd scan --changed")
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn resolve_git_dir(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if dot_git.is_file() {
            if let Ok(text) = fs::read_to_string(&dot_git) {
                if let Some(rest) = text
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("gitdir:"))
                {
                    let candidate = PathBuf::from(rest.trim());
                    return Some(if candidate.is_absolute() {
                        candidate
                    } else {
                        dir.join(candidate)
                    });
                }
            }
        }
    }
    None
}

/* ------------------------------------------------------------- agent hooks */

fn install_claude_hooks(path: &Path) -> Result<()> {
    let mut settings = read_settings(path)?;
    merge_claude_hooks(&mut settings)?;
    write_settings(path, &settings)
}

fn read_settings(path: &Path) -> Result<Value> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("invalid JSON in {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// Add missing SessionStart/PreCompact command hooks while leaving every
/// existing key, event, matcher and command untouched.
fn merge_claude_hooks(settings: &mut Value) -> Result<()> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| anyhow!("Claude settings root must be a JSON object"))?;

    if !root.contains_key("hooks") {
        root.insert("hooks".to_string(), json!({}));
    }
    let hooks = root
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("Claude settings \"hooks\" must be a JSON object"))?;

    for event in CLAUDE_EVENTS {
        if !hooks.contains_key(event) {
            hooks.insert(event.to_string(), json!([]));
        }
        let entries = hooks
            .get_mut(event)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Claude settings \"hooks.{event}\" must be an array"))?;
        if !entries_contain_handoff(entries) {
            entries.push(json!({
                "matcher": "",
                "hooks": [
                    { "type": "command", "command": HANDOFF_COMMAND }
                ]
            }));
        }
    }
    Ok(())
}

fn entries_contain_handoff(entries: &[Value]) -> bool {
    entries.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(is_aikd_compact_handoff)
                })
            })
    })
}

fn is_aikd_compact_handoff(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("aikd") && lower.contains("handoff") && lower.contains("--compact")
}

fn claude_event_installed(settings: &Value, event: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array)
        .is_some_and(|entries| entries_contain_handoff(entries))
}

fn write_settings(path: &Path, settings: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(settings)?;
    text.push('\n');

    // Write beside the target first, then replace. Windows cannot rename over
    // an existing file, so fall back to an in-place write.
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.json");
    let tmp = path.with_file_name(format!("{file_name}.aikd-tmp"));
    fs::write(&tmp, &text).with_context(|| format!("writing {}", tmp.display()))?;
    if fs::rename(&tmp, path).is_err() {
        let result = fs::write(path, &text);
        let _ = fs::remove_file(&tmp);
        result.with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/* -------------------------------------------------------------- path utils */

fn resolve_project(project: Option<&str>) -> Result<PathBuf> {
    match project {
        Some(value) => resolve_path(value),
        None => std::env::current_dir().context("resolving the current directory"),
    }
}

fn resolve_path(value: &str) -> Result<PathBuf> {
    let expanded = shellexpand::tilde(value).to_string();
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .context("resolving the current directory")?
            .join(path))
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let home_path = std::env::var_os("HOMEPATH")?;
            let mut path = PathBuf::from(drive);
            path.push(home_path);
            Some(path)
        })
}

fn default_claude_settings() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".claude").join("settings.json"))
}

fn opencode_config_path() -> Option<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(
            PathBuf::from(config_home)
                .join("opencode")
                .join("opencode.json"),
        );
    }
    home_dir().map(|home| home.join(".config").join("opencode").join("opencode.json"))
}

fn gemini_config_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".gemini").join("settings.json"))
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

/* -------------------------------------------------------------------- tests */

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Minimal RAII temp directory (avoids adding a test-only dependency).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "aikd-hooks-test-{}-{stamp}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn repo_with_hooks() -> TempDir {
        let dir = TempDir::new();
        fs::create_dir_all(dir.path().join(".git").join("hooks")).unwrap();
        dir
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn git_hooks_install_is_idempotent_and_preserves_user_content() {
        let repo = repo_with_hooks();
        let hook = repo.path().join(".git").join("hooks").join("post-commit");
        fs::write(&hook, "#!/bin/sh\necho user-hook\n").unwrap();

        let hooks_dir = install_git_hooks(repo.path()).unwrap();
        assert_eq!(hooks_dir, repo.path().join(".git").join("hooks"));

        let first = fs::read_to_string(&hook).unwrap();
        assert!(first.starts_with("#!/bin/sh\necho user-hook\n"));
        assert_eq!(first.matches(AIKD_START).count(), 1);
        assert_eq!(first.matches(AIKD_END).count(), 1);
        assert!(first.contains("aikd scan --changed --quiet"));

        install_git_hooks(repo.path()).unwrap();
        let second = fs::read_to_string(&hook).unwrap();
        assert_eq!(second, first);
        assert_eq!(second.matches(AIKD_START).count(), 1);

        let post_merge = repo.path().join(".git").join("hooks").join("post-merge");
        let merge_text = fs::read_to_string(&post_merge).unwrap();
        assert!(merge_text.contains(AIKD_START));
        assert!(merge_text.contains("aikd scan --changed"));

        let status = collect_status(repo.path(), None);
        assert!(status.git.available);
        assert!(status.git.post_commit);
        assert!(status.git.post_merge);
        assert!(status.git.installed());
    }

    #[test]
    fn git_hook_block_is_updated_in_place_not_duplicated() {
        let existing = format!(
            "#!/bin/sh\necho before\n\n{}\n\necho after\n",
            "# AIKD-HOOK-START\nstale\n# AIKD-HOOK-END"
        );
        let updated = upsert_hook_script(Some(&existing), AIKD_HOOK_BLOCK);
        assert!(updated.contains("echo before"));
        assert!(updated.contains("echo after"));
        assert_eq!(updated.matches(AIKD_START).count(), 1);
        assert_eq!(updated.matches(AIKD_END).count(), 1);
        assert!(!updated.contains("stale"));
        assert!(updated.contains("aikd scan --changed"));

        let again = upsert_hook_script(Some(&updated), AIKD_HOOK_BLOCK);
        assert_eq!(again, updated);
    }

    #[test]
    fn claude_merge_preserves_existing_settings_and_is_idempotent() {
        let dir = TempDir::new();
        let settings_path = dir.path().join("settings.json");
        let existing = json!({
            "model": "opus",
            "permissions": { "allow": ["Bash(ls)"] },
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "startup",
                        "hooks": [{ "type": "command", "command": "echo existing" }]
                    }
                ],
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [{ "type": "command", "command": "echo stop" }]
                    }
                ]
            }
        });
        fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_claude_hooks(&settings_path).unwrap();
        let after = read_json(&settings_path);
        assert_eq!(after["model"], existing["model"]);
        assert_eq!(after["permissions"], existing["permissions"]);
        assert_eq!(after["hooks"]["Stop"], existing["hooks"]["Stop"]);
        assert!(after["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["hooks"][0]["command"] == "echo existing"));
        assert!(claude_event_installed(&after, "SessionStart"));
        assert!(claude_event_installed(&after, "PreCompact"));

        let session_len = after["hooks"]["SessionStart"].as_array().unwrap().len();
        let compact_len = after["hooks"]["PreCompact"].as_array().unwrap().len();
        install_claude_hooks(&settings_path).unwrap();
        let again = read_json(&settings_path);
        assert_eq!(
            again["hooks"]["SessionStart"].as_array().unwrap().len(),
            session_len
        );
        assert_eq!(
            again["hooks"]["PreCompact"].as_array().unwrap().len(),
            compact_len
        );

        let status = collect_status(dir.path(), Some(&settings_path));
        assert!(status.claude.present);
        assert!(status.claude.session_start);
        assert!(status.claude.pre_compact);
        assert!(status.claude.installed());
        assert!(status.claude.error.is_none());
    }

    #[test]
    fn claude_install_creates_custom_settings_path() {
        let dir = TempDir::new();
        let settings_path = dir
            .path()
            .join("nested")
            .join("claude")
            .join("settings.json");

        install_claude_hooks(&settings_path).unwrap();
        let settings = read_json(&settings_path);
        assert!(claude_event_installed(&settings, "SessionStart"));
        assert!(claude_event_installed(&settings, "PreCompact"));
    }

    #[test]
    fn invalid_claude_json_is_reported_and_never_overwritten() {
        let dir = TempDir::new();
        let settings_path = dir.path().join("settings.json");
        fs::write(&settings_path, "{not valid json").unwrap();

        let err = install_claude_hooks(&settings_path).unwrap_err();
        assert!(err.to_string().contains("invalid JSON"));
        assert_eq!(
            fs::read_to_string(&settings_path).unwrap(),
            "{not valid json"
        );

        let status = collect_status(dir.path(), Some(&settings_path));
        assert!(status.claude.present);
        assert!(!status.claude.installed());
        assert!(status.claude.error.is_some());
    }

    #[test]
    fn status_without_git_repo_reports_missing_cleanly() {
        let dir = TempDir::new();
        let status = collect_status(dir.path(), None);
        assert!(!status.git.available);
        assert!(!status.git.installed());
        assert!(status.claude.path.is_none());
        assert!(!status.claude.installed());

        let value = status.to_json();
        assert_eq!(value["git"]["available"], false);
        assert_eq!(value["git"]["installed"], false);
        assert_eq!(value["agents"]["claude"]["installed"], false);
    }
}
