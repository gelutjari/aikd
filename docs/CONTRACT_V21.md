# AIKD v2.1 Shared Implementation Contract (`CONTRACT_V21.md`)

> **Branch:** `feat/v2.1-studio-graph-tray` (worktree: `C:\Users\hp\aikd-v21`)  
> **Rule #1 (Zero-Downtime):** NEVER kill/restart production `aikd` (`127.0.0.1:9090`), NEVER touch `~/.aikd/aikd.db` or `~/.local/bin/aikd.exe`. Test ONLY against temp/sandbox DB & port `9091`.  
> **Rule #2 (ADR-0001):** Zero-ONNX default (`embedding.enabled = false`), zero external CDN in `ui.html` (100% offline pure HTML/CSS/JS/Canvas/SVG), no Electron/Tauri.  
> **Rule #3 (Strict File Ownership):** Each worker edits ONLY its assigned files below to guarantee zero merge conflicts.

---

## 1. Worker File Ownership Boundaries

| Wave | Worker | Owned Files (Edit ONLY these) | Must NOT Edit |
| :--- | :--- | :--- | :--- |
| Wave 1 | `oc-backend` | `crates/core/src/security.rs`, `crates/storage/src/*` (`schema.rs`, `graph.rs`, `notes.rs`, `telemetry.rs`, `lib.rs`, `memory.rs`, `events.rs`), `crates/server/src/mcp.rs`, `crates/server/src/rest.rs`, `crates/server/Cargo.toml` | `crates/server/src/ui.html`, `crates/cli/*`, `extensions/*` |
| Wave 1 | `oc-ui` | `crates/server/src/ui.html` | Any `.rs` file, `Cargo.toml`, `extensions/*` |
| Wave 1 | `oc-tray` | `crates/cli/src/tray.rs`, `crates/cli/Cargo.toml` (only tray deps if strictly needed; prefer zero new heavy crates) | `crates/cli/src/main.rs`, `crates/storage/*`, `crates/server/*`, `extensions/*` |
| Wave 1 | `oc-chrome-ext` | `extensions/chrome/*` | Any Rust file or `ui.html` |
| Wave 2 | `oc-cli-tools` | `crates/cli/src/main.rs`, `crates/cli/src/top.rs` | `crates/cli/src/tray.rs`, `crates/server/src/ui.html` |
| Wave 2 | `oc-verifier` | `tests/*` / verification scripts in `.aikd-orch/` (read-only audit of code, live sandbox tests on `:9091`) | Production `~/.aikd/` |

---

## 2. Database Schema v15 (Additive Migration in `crates/storage/src/schema.rs`)

Bump `SCHEMA_VERSION` from `14` to `15`. Migration `14 -> 15` must be idempotent (`CREATE TABLE IF NOT EXISTS`, safe column checks) so upgrading production v14 DBs never loses data:

1. **`notes` table** (Human Vault + PARA/Journal + "Send to AIKD", synced with `~/.aikd/notes/`):
   ```sql
   CREATE TABLE IF NOT EXISTS notes (
     id TEXT PRIMARY KEY,
     title TEXT NOT NULL,
     body TEXT NOT NULL,
     kind TEXT NOT NULL DEFAULT 'note',          -- note | decision | constraint | fact | task | prompt | daily_log
     scope TEXT NOT NULL DEFAULT 'project',      -- global | project
     folder TEXT NOT NULL DEFAULT 'Inbox',       -- Inbox | Journal | Projects | Areas | Resources | Memories | Archive
     project TEXT NOT NULL DEFAULT '',           -- normalized project key or '' for global
     tags_json TEXT NOT NULL DEFAULT '[]',       -- JSON array of strings (lowercase without #)
     links_json TEXT NOT NULL DEFAULT '[]',      -- JSON array of extracted [[wikilink]] / file refs
     source TEXT NOT NULL DEFAULT 'human',       -- human | ui | cli | tray | clipboard | chrome-extension | import | agent
     source_url TEXT NOT NULL DEFAULT '',
     file_path TEXT NOT NULL DEFAULT '',         -- path to markdown file in ~/.aikd/notes/<folder>/...
     pinned INTEGER NOT NULL DEFAULT 0,
     redacted_count INTEGER NOT NULL DEFAULT 0,
     created_at TEXT NOT NULL,
     updated_at TEXT NOT NULL
   );
   ```
2. **`knowledge_links` table** (unified edges for `wikilink`, `tag`, `mentions`, `relates`):
   ```sql
   CREATE TABLE IF NOT EXISTS knowledge_links (
     id INTEGER PRIMARY KEY AUTOINCREMENT,
     from_id TEXT NOT NULL,
     from_kind TEXT NOT NULL,    -- note | memory | file
     to_id TEXT NOT NULL,
     to_kind TEXT NOT NULL,      -- note | memory | file | symbol | tag
     relation TEXT NOT NULL,     -- wikilink | tag | mentions | relates
     provenance TEXT NOT NULL DEFAULT 'EXTRACTED',
     created_at TEXT NOT NULL,
     UNIQUE(from_id, to_id, relation)
   );
   ```
3. **`active_clients` table** (Live Mission Control auto-telemetry):
   ```sql
   CREATE TABLE IF NOT EXISTS active_clients (
     client_id TEXT PRIMARY KEY,          -- e.g. "mcp-<pid>" or "rest-<tag>"
     pid INTEGER NOT NULL DEFAULT 0,
     parent_pid INTEGER NOT NULL DEFAULT 0,
     parent_name TEXT NOT NULL DEFAULT '',
     mcp_client_name TEXT NOT NULL DEFAULT '',
     transport TEXT NOT NULL DEFAULT 'mcp-stdio', -- mcp-stdio | rest | cli
     cwd TEXT NOT NULL DEFAULT '',
     project TEXT NOT NULL DEFAULT '',
     last_tool TEXT NOT NULL DEFAULT '',
     last_target TEXT NOT NULL DEFAULT '',
     status TEXT NOT NULL DEFAULT 'active',       -- active | idle | disconnected
     started_at TEXT NOT NULL,
     last_seen_at TEXT NOT NULL
   );
   ```
4. **`query_metrics` table** (Token savings & latency tracking):
   ```sql
   CREATE TABLE IF NOT EXISTS query_metrics (
     id INTEGER PRIMARY KEY AUTOINCREMENT,
     tool TEXT NOT NULL,
     query_text TEXT NOT NULL DEFAULT '',
     full_bytes INTEGER NOT NULL DEFAULT 0,
     returned_bytes INTEGER NOT NULL DEFAULT 0,
     saved_tokens_est INTEGER NOT NULL DEFAULT 0,
     elapsed_ms INTEGER NOT NULL DEFAULT 0,
     created_at TEXT NOT NULL
   );
   ```

---

## 3. Secret Redaction Shield (`crates/core/src/security.rs`)

Add deterministic function:
```rust
pub struct RedactionResult {
    pub text: String,
    pub redacted_count: usize,
    pub kinds: Vec<String>,
}
pub fn redact_secrets(input: &str) -> RedactionResult
```
Patterns to redact to `[REDACTED]`:
- OpenAI / Anthropic / DeepSeek / generic keys: `sk-[A-Za-z0-9_-]{16,}`, `sk-ant-[A-Za-z0-9_-]{16,}`, `sk-proj-[A-Za-z0-9_-]{16,}`
- Google API keys: `AIza[0-9A-Za-z_-]{20,}`
- GitHub tokens: `gh[pousr]_[A-Za-z0-9_]{20,}`
- AWS access keys: `AKIA[0-9A-Z]{16}`
- Slack tokens: `xox[baprs]-[A-Za-z0-9-]{12,}`
- Bearer tokens: `Bearer [A-Za-z0-9._-]{20,}` → `Bearer [REDACTED]`
- Generic `api_key=...` / `token=...` / `secret=...` / `password=...` when value is >= 16 non-whitespace secret-looking chars.

Automatically apply `redact_secrets` on `POST /api/send`, `POST /api/notes`, `PUT /api/notes/:id`, and `POST /api/memory`! Return `redacted_count` in the response so UI & Chrome Extension can display a green/amber Shield badge.

---

## 4. REST API Contract (All responses wrapped in `{ "success": bool, "data": ..., "error": Option<String> }`)

### 4.1 CORS Update (`crates/server/src/rest.rs`)
- Allow HTTP methods: `GET`, `POST`, `PUT`, `DELETE`, `OPTIONS`.
- When `cors_origins` is empty (default localhost mode), use a predicate (`AllowOrigin::predicate`) allowing:
  - `http://localhost:*`, `http://127.0.0.1:*`
  - `chrome-extension://*`, `extension://*`, `moz-extension://*`
  so the Chrome/Edge Manifest V3 extension works seamlessly on both `:9090` and sandbox `:9091` without opening CORS to arbitrary public websites.

### 4.2 `GET /api/overview` (Backward-Compatible Additions)
Keep all existing fields (`version`, `schema_version`, `db_mb`, `files`, `chunks`, `symbols`, `memory_active`, `memory_stale`, `memory_archived`, `memory_conflicted`, `events`, `sessions`, `cpu_cores`, `ram_gb`, `coverage`), and ADD:
```json
{
  "notes_count": 12,
  "active_clients_count": 3,
  "tokens_saved_est": 48200,
  "avg_query_ms": 4.2,
  "watcher_running": false,
  "projects": [
    {
      "path": "C:/Users/hp/aikd-v21",
      "name": "aikd-v21",
      "files": 42,
      "symbols": 180,
      "notes": 3,
      "git_branch": "feat/v2.1-studio-graph-tray",
      "git_dirty": 2,
      "stale": false
    }
  ]
}
```

### 4.3 `GET /api/graph` & `GET /api/graph/node`
**Query params for `GET /api/graph`:**
- `path` / `project` (optional): if empty, do NOT return 0 nodes! Return a rich **global knowledge graph** (project hub nodes + top connected files + all human notes + active memories + shared `#tag` nodes + active agent nodes).
- `mode`: `global` (default) | `local`
- `focus`: optional `node_id` (when `mode=local`, BFS up to `depth` hops around `focus`)
- `depth`: `1` | `2` (default `1`)
- `types`: optional comma-separated filter (`project,file,symbol,note,memory,tag,agent`)
- `hide_orphans`: optional `true|false` (default `false`)
- `limit`: optional usize (default `120`)

**Response `data`:** Keep backward-compatible `files`, `symbols`, `edges`, `scope_note`, AND include unified `nodes` array so UI renders everything cleanly:
```json
{
  "files": [ ... ],
  "symbols": [ ... ],
  "nodes": [
    {
      "id": "note:abc123",
      "kind": "note",
      "subtype": "prompt",
      "label": "PR Review Checklist",
      "project": "",
      "scope": "global",
      "degree": 4,
      "provenance": "HUMAN"
    }
  ],
  "edges": [
    { "from": "note:abc123", "to": "tag:review", "kind": "tag", "provenance": "EXTRACTED" },
    { "from": "file:...", "to": "file:...", "kind": "imports", "provenance": "INFERRED" }
  ],
  "scope_note": "...",
  "stats": { "total_nodes": 85, "total_edges": 140 }
}
```
Supported `graph.rs` import/link extractors:
- Rust (`use`, `mod`)
- Python (`import`, `from ... import`)
- TypeScript / JavaScript (`import ... from '...'`, `require('...')`, `export ... from '...'`)
- Dart / Flutter (`import 'package:...'`, `import '...'`, `part '...'`)
- Go (`import "..."`)
- Markdown / Notes / Memory (`[[wikilink]]`, `[text](path.md)`, `#tag`, file-path / symbol mentions)

**`GET /api/graph/node?id=<node_id>` response `data`:**
```json
{
  "id": "...",
  "kind": "note|file|symbol|memory|tag|project|agent",
  "label": "...",
  "project": "...",
  "scope": "global|project",
  "preview": "First ~2000 chars of note body / file snippet / memory body",
  "tags": ["arch", "tray"],
  "metadata": { "source": "human", "updated_at": "...", "line_start": 1 },
  "outgoing": [ { "id": "...", "label": "...", "kind": "...", "edge_kind": "..." } ],
  "backlinks": [ { "id": "...", "label": "...", "kind": "...", "edge_kind": "..." } ]
}
```

### 4.4 Human Notes, PARA Vault & "Send to AIKD"
- **PARA + Journal Folder Structure** inside `<runtime_dir>/notes/`:
  - Ensure subdirectories exist: `Inbox/`, `Journal/`, `Projects/`, `Areas/`, `Resources/`, `Memories/`, `Archive/`, `assets/`.
  - Auto-create `_PROTOCOL.md` (if missing) explaining shared memory rules for all AI agents (`Worked on · Learned · Decisions · In Progress`, concise token-efficient logs).
  - For `kind = "daily_log"`, default `folder` to `Journal/YYYY/MM` (e.g., `Journal/2026/11`).
- **`POST /api/send`** (alias/quick-capture) and **`POST /api/notes`**:
  Request body:
  ```json
  {
    "title": "Optional title (auto-derived from first line if omitted)",
    "body": "Markdown content with [[wikilink]] and #tag",
    "kind": "note",
    "scope": "project",
    "folder": "Inbox",
    "project": "C:/Users/hp/aikd-v21",
    "tags": ["optional-extra-tag"],
    "source": "ui",
    "source_url": "",
    "pinned": false
  }
  ```
  Behavior:
  1. Runs `redact_secrets(&body)` and `redact_secrets(&title)`.
  2. Extracts `#tags`, `[[wikilinks]]`, and mentioned indexed file paths.
  3. Writes a human-readable Markdown file with YAML frontmatter to `<runtime_dir>/notes/<folder>/<slug>-<short_id>.md` (and ensures `<runtime_dir>/notes/assets/` directory exists).
  4. Saves row in `notes`, populates `knowledge_links`, and mirrors/indexes into `memory` (when `kind` is `decision|constraint|fact|task`) or FTS so `query`/`recall`/`handoff` find it immediately.
  5. Returns created `NoteRecord` including `redacted_count`, `extracted_tags`, `extracted_links`, `file_path`, `folder`.
- **`GET /api/notes`** (query params: `q`, `project`, `scope`, `folder`, `kind`, `tag`, `pinned`, `limit`): returns `{ "notes": [NoteRecord], "tags": [{"tag": "...", "count": 3}], "folders": [{"folder": "Inbox", "count": 2}] }`.
- **`PUT /api/notes/{id}`**: updates title/body/kind/scope/folder/project/tags/pinned, re-redacts secrets, re-extracts links, updates Markdown file on disk.
- **`DELETE /api/notes/{id}`**: deletes from `notes`, `knowledge_links`, and removes its Markdown file if present.
- **`GET /api/brief?days=3&project=<optional>`**: **Morning Brief & Check-Before-Start** endpoint summarizing recent `daily_log` notes, recent active `memories`/`decisions`/`learned`, active `constraints`, and recent `events` into `{ "bullets": [...], "done": [...], "stuck": [...], "focus": [...], "learned": [...], "markdown": "..." }`.
- **`GET /api/export`**: returns a JSON bundle (or Markdown archive payload) containing all active human notes + active memories so the UI can trigger 1-click download backup (`aikd-backup-YYYYMMDD.json` / `.md`).

### 4.5 Live Mission Control (`GET /api/live`)
Returns real-time "Siapa · Konek Ke Mana · Sedang Ngapain":
```json
{
  "daemon": {
    "pid": 1234,
    "port": 9091,
    "uptime_secs": 120,
    "db_path": "...",
    "status": "healthy"
  },
  "active_clients": [
    {
      "client_id": "mcp-9876",
      "pid": 9876,
      "parent_pid": 4321,
      "parent_name": "opencode.exe",
      "mcp_client_name": "opencode",
      "transport": "mcp-stdio",
      "cwd": "C:/Users/hp/aikd-v21",
      "project": "C:/Users/hp/aikd-v21",
      "last_tool": "query",
      "last_target": "handle_graph",
      "status": "active",
      "started_at": "...",
      "last_seen_at": "..."
    }
  ],
  "detected_agents": [
    {
      "name": "opencode",
      "exe": "opencode.exe",
      "pid": 4321,
      "cwd": "C:/Users/hp/aikd-v21",
      "connected_to_aikd": true
    }
  ],
  "herdr_terminals": [
    {
      "id": "t9",
      "name": "oc-backend",
      "app": "opencode",
      "cwd": "C:/Users/hp/aikd-v21",
      "status": "working"
    }
  ],
  "recent_events": [
    {
      "id": "...",
      "agent": "opencode",
      "event_type": "query",
      "summary": "query: handle_graph",
      "project": "...",
      "created_at": "..."
    }
  ]
}
```
Note: Reading Herdr terminals (`%LOCALAPPDATA%\herdr-core\default\terminals.json`) MUST be strictly read-only, optional, and silently return `[]` if missing or unreadable.

### 4.6 Analytics (`GET /api/analytics`)
Returns data for the 3 pure-SVG/Canvas dashboard charts:
```json
{
  "languages": [ { "ext": ".rs", "files": 58, "chunks": 420 } ],
  "memory_breakdown": {
    "note": 10, "decision": 12, "constraint": 8, "fact": 20, "task": 6, "prompt": 4,
    "active": 40, "stale": 2, "conflicted": 1, "archived": 5
  },
  "activity_timeline": [
    { "bucket": "00:00", "count": 5, "agents": { "opencode": 3, "agy": 2 } }
  ],
  "tokens": {
    "saved_est": 48200,
    "queries_total": 124,
    "avg_elapsed_ms": 4.2
  }
}
```

### 4.7 Impact Analysis (`GET /api/impact?target=<symbol_or_file>&project=<optional>`)
Returns blast radius before refactoring:
```json
{
  "target": "handle_graph",
  "matched_files": [ "C:/Users/hp/aikd-v21/crates/server/src/rest.rs" ],
  "direct_importers": [ "..." ],
  "symbol_callers": [ { "file": "...", "line": 425, "snippet": "..." } ],
  "related_constraints": [ { "id": "...", "title": "...", "body": "...", "scope": "global|project" } ],
  "related_notes": [ { "id": "...", "title": "...", "kind": "note" } ]
}
```

### 4.8 Auto-Attach Active Constraints & Context Pack
- On `GET /api/query` and `GET /api/symbols` (and MCP `query`/`symbol`): automatically attach an optional `attached_constraints` list (top 5 active `constraint` memories/notes matching `scope='global'` OR matching the queried file's `project`), AND record a row in `query_metrics` with estimated tokens saved (`(full_bytes - returned_bytes) / 4`).
- **`GET /api/context-pack?project=...&node_id=...`**: returns `{ "markdown": "...", "tokens_est": 850, "items_count": 7 }` — dense Markdown pack combining relevant symbols, active constraints (global + project), and related human notes ready for 1-click copy into web AI chats.

---

## 5. AIKD Studio UI Contract (`crates/server/src/ui.html`)

- **100% self-contained single HTML file** (`include_str!("ui.html")`), zero external CDN/fonts/scripts.
- **Query param support:**
  - `/ui?capture=1` → immediately opens the **Quick Send to AIKD** modal (used by tray `Ctrl+Alt+N` hotkey & tray menu).
  - `/ui?tab=graph` / `?tab=notes` / `?tab=live` → navigates directly to that view.
- **Layout:** 3-Zone App Shell:
  - **Left Sidebar:** 7 navigation pages:
    1. `Mission Control` (4-light health strip, KPI cards incl. Tokens Saved, **Morning Brief** widget calling `/api/brief?days=3`, Live Connection Map SVG, 3 charts from `/api/analytics`, Project Atlas cards with Git branch/dirty & quick Scan/Graph/Clean)
    2. `Knowledge Graph` (Trilium-style **Note Map** + Obsidian Memory Galaxy: toggle button **Force-Directed Galaxy** ↔ **Hierarchy Tree View**, hub-sized nodes, drag nodes, wheel zoom, drag pan, hover 1-hop spotlight, Global vs Local mode, Depth 1/2, type filters `project/file/symbol/note/memory/tag/agent`, Hide Orphans toggle, quick search, click node → Right Inspector with preview, Outgoing Links, Backlinks, "Focus Local", "+ Add Related Note", "Copy Context Pack")
    3. `Notes & Inbox` (3-column Obsidian/Trilium vault: Left expandable PARA + Journal tree (`root` → `Journal/YYYY/MM`, `Inbox`, `Projects`, `Areas`, `Resources`, `Memories`, `Archive`) + `#tags` + Kind filter incl. `prompt` & `daily_log`; Center Markdown editor + preview + `[[wikilink]]` autocomplete + Scope toggle `Global`/`Project` + Secret Redaction badge; Right Backlinks & mini local graph)
    4. `Code & Impact` (Unified BM25 search, Symbol lookup, and **Impact Analysis** tab calling `/api/impact` showing blast radius + attached constraints)
    5. `Memory & Handoff` (Memory list with scope/status filters, conflict/stale resolution actions, Handoff viewer with Git status, and 1-click **Copy Context Pack** with token count)
    6. `Live Agents` (Real-time `/api/live` radar: Who / Where / What table, Live Connection Topology, Detected Local Agents + Herdr Terminals, Recent Events feed, Auto-Register MCP button)
    7. `Settings & Shield` (Config paths, Secret Redaction Shield tester/info, 1-click Clean & Optimize `/api/maintenance`, 1-click Export/Backup `/api/export`)
  - **Top Command Bar:** Global workspace switcher, live daemon/client status pill, `Ctrl+K` quick command/search, prominent **`+ Send to AIKD`** button, theme polish (deep slate dark theme with crisp contrast and high information density).
  - **Collapsible Right Inspector:** Context-aware detail/backlinks/context-pack drawer.

---

## 6. Tray Binary Contract (`crates/cli/src/tray.rs`)

- Line 1: `#![cfg_attr(windows, windows_subsystem = "windows")]` (no black console window).
- **Single-instance guard** on Windows using Win32 named mutex (`Global\\AIKD_TRAY_SINGLE_INSTANCE` or `Local\\AIKD_TRAY_SINGLE_INSTANCE` via Win32 FFI `CreateMutexW` + `GetLastError() == 183` — zero extra crates needed!). If already running, exit cleanly.
- **Auto-start daemon** on tray startup: if `127.0.0.1:<port>/api/health` is unreachable, spawn `aikd daemon` with `CREATE_NO_WINDOW`.
- **Dynamic 3-color RGBA icon** updated every ~8s:
  - Green (`#22c55e`): daemon healthy & no warnings
  - Yellow (`#f59e0b`): daemon healthy but `memory_conflicted > 0` or `memory_stale > 0` or stale projects detected
  - Red (`#ef4444`): daemon offline / unreachable
- **Left-click / double-click** on tray icon → opens AIKD Studio in App Window mode (`msedge --app=http://127.0.0.1:<port>/ui`, fallback `chrome --app=...`, fallback `cmd /c start`).
- **Global Hotkey (`Ctrl+Alt+N`)** on Windows via Win32 `RegisterHotKey` (polled cleanly or via helper thread) → opens `http://127.0.0.1:<port>/ui?capture=1` in compact `--app` window.
- **Tray Menu items:**
  - Status header (disabled item showing live counts: files / notes / active clients)
  - `Open AIKD Studio`
  - `Quick Send to AIKD (Ctrl+Alt+N)`
  - `Send Clipboard to AIKD` (reads Windows clipboard text via Win32 or PowerShell `Get-Clipboard`, POSTs to `/api/send` with `source="clipboard"`)
  - `Toggle Auto-Watch` (starts/stops `aikd watch` child PID cleanly)
  - `Start / Restart Daemon`
  - `Install Windows SendTo Shortcut` (creates `%APPDATA%\Microsoft\Windows\SendTo\Send to AIKD.cmd` invoking `aikd send`)
  - `Quit Tray` (does NOT kill daemon unless explicitly chosen, so running AI agents are never disrupted!)

---

## 7. Chrome / Edge Extension Contract (`extensions/chrome/`)

- Manifest V3 (`manifest.json`), permissions: `contextMenus`, `activeTab`, `scripting`, `storage`.
- `host_permissions`: `http://127.0.0.1/*`, `http://localhost/*`.
- `background.js`:
  - Context menu 1: **"Send selection to AIKD"** → POSTs selected text + page title + page URL to `http://127.0.0.1:<port>/api/send` (`source: "chrome-extension"`).
  - Context menu 2: **"Clip page to AIKD"** → extracts page title, URL, meta description / trimmed readable text, POSTs to `/api/send` with tag `web-clip`.
  - Shows Chrome notification or badge (`OK` green / `ERR` red).
- `popup.html` + `popup.js` + `popup.css`:
  - Keyboard command `_execute_action` suggested key `Ctrl+Shift+Y`.
  - Auto-populates active tab Title, URL, and current text selection.
  - Fields: Title, Body, Kind (`note|decision|constraint|fact|task|prompt`), Scope (`project|global`), Project dropdown (loaded live from `GET /api/overview`), Tags (`#tag` chips/input).
  - Live daemon connection dot + configurable port setting (default `9090`, saved in `chrome.storage.local` so user can test `9091` easily!).
  - Shows Secret Redaction feedback (`Shield: N secrets redacted`) after save.
  - Also includes a 1-click **"Copy Context Pack"** button calling `GET /api/context-pack`.
- Generate clean local PNG icons (`icons/icon16.png`, `icon48.png`, `icon128.png`) with pure script/bytes so Chrome loads unpacked without errors.
