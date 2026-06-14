<div align="center">

# AIKD

### AI Knowledge Daemon

**Indexed code search for AI agents and developers.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-2.0.0-green.svg)]()
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)]()
[![Tests](https://img.shields.io/badge/tests-82%20passed-brightgreen.svg)]()

</div>

---

## Table of Contents

- [About](#about)
- [Why AIKD?](#why-aikd)
- [Key Features](#key-features)
- [Demo](#demo)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Usage Modes](#usage-modes)
- [Commands Reference](#commands-reference)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Project Structure](#project-structure)
- [Benchmark Results](#benchmark-results)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)
- [Acknowledgements](#acknowledgements)

---

## About

**AIKD** (AI Knowledge Daemon) is an **MCP tool provider** written in Rust that gives AI agents instant access to your codebase. It indexes project files into a searchable knowledge base and exposes **7 tools** via the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) that any AI agent can call.

### What is AIKD?

| Category | Answer |
|----------|--------|
| **MCP Tool Provider** | Yes — exposes `scan`, `query`, `embed`, `stats`, `remember`, `recall`, `status` tools |
| **CLI Tool** | Yes — `aikd query "login" --json` works directly from terminal |
| **REST API** | Yes — HTTP endpoints at `http://localhost:9090` |
| **VS Code Extension** | No — standalone binary, not an IDE plugin |
| **Library/SDK** | No — end-user tool, not a dependency |

### How it works

```
┌─────────────────────────────────────────────────────────┐
│                    AI Agent                              │
│  (MiMoCode, Claude Code, Cursor, Cline, Windsurf, etc) │
└────────────────────────┬────────────────────────────────┘
                         │ MCP Protocol (stdio)
                         ▼
┌─────────────────────────────────────────────────────────┐
│                    AIKD Server                           │
│                                                         │
│  Tools:                                                 │
│    scan    → Index files into knowledge base             │
│    query   → BM25 + vector semantic search               │
│    embed   → Generate vector embeddings                  │
│    stats   → Knowledge base statistics                   │
│    remember → Save conversation to memory                │
│    recall  → Search conversation history                 │
│    status  → System resource status                      │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│              Knowledge Base                              │
│  SQLite + Tantivy (BM25) + ONNX embeddings (384d)       │
│  84 files · 412 chunks · <1ms search                     │
└─────────────────────────────────────────────────────────┘
```

### The Problem

AI agents need to understand your codebase. Without an index, they:
- Run `grep` or `find` repeatedly (slow, no ranking)
- Read entire files (wastes tokens)
- Guess where things are (inaccurate)

### The Solution

AIKD pre-indexes your project. AI agents call `aikd query` via MCP and get ranked results in milliseconds.

---

## Why AIKD?

| Feature | grep/find | AIKD |
|---------|-----------|------|
| Speed | Scans every time | Pre-indexed, <1ms |
| Ranking | None | BM25 + semantic |
| Context | Raw text lines | File, heading, line range, score |
| Output | Unstructured text | JSON (machine-readable) |
| Semantic search | No | Yes (vector embeddings) |
| Auto-sync | No | Yes (file watcher) |

---

## Key Features

- **BM25 Full-Text Search** — Fast keyword search via Tantivy engine
- **Hybrid Search** — BM25 + vector semantic search with Reciprocal Rank Fusion
- **Vector Embeddings** — ONNX-based embedding (all-MiniLM-L6-v2, 384 dimensions)
- **Smart Chunking** — Markdown headings, source code functions, structured files
- **Source Code Aware** — Chunks Rust, Python, TypeScript, JavaScript, Go by function boundaries
- **File Watcher** — Auto-reindex on file changes with blake3 incremental hashing
- **Session Memory** — Store and recall conversation context
- **MCP Server** — Model Context Protocol for AI assistants (Claude, MiMo, Cursor, etc.)
- **REST API** — HTTP endpoints for external integrations
- **CLI Tools** — Direct command-line usage, no server needed
- **Auto-Agent Registration** — Detects and registers with 6 AI agents on `aikd init`
- **Resource Adaptive** — Auto-tunes for Low/Medium/High/Max/GPU hardware tiers
- **Cross-Platform** — Windows, Linux, macOS

---

## Demo

<!-- Add screenshot or GIF here -->

```
$ aikd query "login function" --json --limit 3

[
  {
    "file_path": "src/auth/login.rs",
    "heading_hierarchy": "Auth > Login",
    "heading_text": "login",
    "content": "pub fn login(user: &str, pass: &str) -> Result<Token> { ... }",
    "line_start": 42,
    "line_end": 58,
    "score": 5.011
  },
  ...
]
```

---

## Prerequisites

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| OS | Windows 10+, Linux, macOS | Any 64-bit |
| RAM | 2 GB | 8 GB+ |
| CPU | 2 cores | 4+ cores |
| Disk | 200 MB | 1 GB+ (for models) |
| Rust | 1.75+ (for building from source) | Latest stable |
| GPU | Not required | NVIDIA GPU for faster embedding |

---

## Installation

### Option 1: Pre-built Binary (Recommended)

**Windows:**
```powershell
# Download from releases, then:
copy aikd.exe %USERPROFILE%\.local\bin\aikd.exe
```

**Linux / macOS:**
```bash
# Download from releases, then:
cp aikd ~/.local/bin/aikd
chmod +x ~/.local/bin/aikd
```

### Option 2: Build from Source

```bash
# Clone the repository
git clone https://github.com/your-org/aikd.git
cd aikd

# Build release
cargo build --release

# Install
# Windows:
copy target\release\aikd.exe %USERPROFILE%\.local\bin\aikd.exe

# Linux/macOS:
cp target/release/aikd ~/.local/bin/aikd
chmod +x ~/.local/bin/aikd
```

### Option 3: Install Script

**Linux / macOS:**
```bash
curl -sSfL https://raw.githubusercontent.com/your-org/aikd/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

### After Installation

```bash
aikd init    # Create config, download model, register AI agents
```

---

## Quick Start

```bash
# 1. Initialize (one-time setup)
aikd init

# 2. Index your project
aikd scan

# 3. Generate embeddings (for semantic search)
aikd embed

# 4. Search
aikd query "your search term" --json
```

---

## Usage Modes

AIKD supports 3 modes. Choose what fits your workflow.

### Mode 1: CLI Tools (Direct)

Call `aikd` directly from terminal or AI agent. No server, no extra config.

```bash
aikd query "login function" --json
aikd scan
aikd stats
```

**Best for:** Quick searches, CI/CD pipelines, AI agents that can call CLI commands.

### Mode 2: MCP Server

For AI assistants that support the Model Context Protocol.

```bash
aikd serve
```

AIKD auto-registers with these agents on `aikd init`:

| Agent | Config File |
|-------|-------------|
| Claude Code | `~/.claude.json` |
| Cursor | `~/.cursor/mcp.json` |
| Cline | `~/.cline/mcp.json` |
| Continue | `~/.continue/config.json` |
| Windsurf | `~/.windsurf/mcp.json` |
| MiMoCode | `~/.mcp.json` |

**Best for:** AI assistants with built-in MCP support.

### Mode 3: Daemon + REST API

Background service with HTTP endpoints and auto-sync.

```bash
aikd daemon --foreground    # REST API at http://localhost:9090
aikd watch                  # Auto-reindex on file changes
```

**Best for:** Web dashboards, multi-user setups, external tool integrations.

---

## Commands Reference

### Global Options

| Flag | Description |
|------|-------------|
| `-c, --config <FILE>` | Config file path (default: `~/.aikd/config.yaml`) |
| `--json` | Output JSON for all commands |
| `-q, --quiet` | Suppress non-error output |
| `-V, --version` | Show version |
| `-h, --help` | Show help |

### Init & Config

```bash
aikd init [--path <DIR>]     # Initialize project (config + model + agent registration)
```

### Indexing

```bash
aikd scan [--path <DIR>]     # Scan and index files
aikd watch [--debounce <MS>] # Watch for changes, auto-reindex (default: 500ms)
```

### Search

```bash
aikd query <TERM>                    # BM25 full-text search
aikd query <TERM> --json             # JSON output
aikd query <TERM> --limit 20         # Max 20 results
aikd query <TERM> --path src/        # Filter by path
aikd query <TERM> -H "Error"         # Filter by heading
aikd query <TERM> --hybrid           # BM25 + vector semantic search
```

### Embedding

```bash
aikd embed                           # Generate vector embeddings
aikd embed --batch 64                # Custom batch size
aikd export [-o chunks.json]         # Export chunks to JSON
aikd import --file <FILE>            # Import embeddings from JSON
```

### Session Memory

```bash
aikd remember --role user --content "message"          # Save message
aikd recall "query"                                    # Search messages
aikd recall "query" --session <ID> --limit 20          # Search in session
```

### Server & Daemon

```bash
aikd serve                           # Start MCP server (stdio)
aikd daemon --foreground             # Start REST API + MCP server
aikd status                          # Show system status (JSON)
aikd inject -- <COMMAND>             # Inject context into another CLI
```

### Benchmark

```bash
aikd benchmark                       # Run 8-scenario benchmark suite
```

---

## Configuration

### Config File

Default location: `~/.aikd/config.yaml`

```yaml
version: "2.0.0"

scan:
  include_paths: ["."]
  exclude_paths: ["node_modules", ".git", "__pycache__", ".cache", "target"]
  include_extensions: ["md", "json", "yaml", "yml", "txt", "toml", "rs", "py", "ts", "js", "go"]
  exclude_files: [".env", "*.bak", "*.tmp", "*.secret"]

chunk:
  max_tokens: 1000
  min_tokens: 100

embedding:
  enabled: true
  model: "all-MiniLM-L6-v2"
  batch_size: "auto"

index:
  db_path: "~/.aikd/aikd.db"
  tantivy_path: "~/.aikd/tantivy_index"
  model_path: "~/.local/share/aikd/model"

server:
  rest_port: 9090
  auth_token: null

resource:
  mode: Auto    # Auto | Low | Medium | High | Max
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AIKD_MODEL_PATH` | Override model directory |
| `AIKD_DATA_DIR` | Override data directory (`~/.aikd/`) |
| `AIKD_TOKEN` | Auth token (same as `config.server.auth_token`) |
| `RUST_LOG` | Log level (e.g., `RUST_LOG=aikd=debug`) |

### Resource Modes

| Mode | RAM | CPU | Embedding | Batch Size | Parallelism |
|------|-----|-----|-----------|------------|-------------|
| Low | <2 GB | ≤2 | OFF | 1 | 1 |
| Medium | 2–8 GB | ≤4 | ON | 8 | 2 |
| High | 8–16 GB | ≤8 | ON | 32 | 4 |
| Max | ≥16 GB | >8 | ON | 64 | 8 |
| Auto | detect | detect | detect | detect | detect |

---

## API Reference

### REST API (Daemon Mode)

Base URL: `http://localhost:9090`

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/query?q=<term>&limit=10&hybrid=true` | Search the knowledge base |
| GET | `/api/stats` | Get index statistics |
| POST | `/api/scan` | Trigger file scan |
| POST | `/api/remember` | Save conversation message |
| POST | `/api/recall` | Search conversation history |

**Authentication:** `Authorization: Bearer <token>` (if `config.server.auth_token` is set)

### MCP Tools

When running as MCP server (`aikd serve`), these tools are available:

| Tool | Description |
|------|-------------|
| `scan` | Scan and index files |
| `query` | Search the knowledge base |
| `stats` | Get statistics |
| `embed` | Generate embeddings |
| `remember` | Save conversation |
| `recall` | Search conversations |
| `status` | Get system status |

---

## Project Structure

```
aikd/
├── Cargo.toml                  # Workspace root (v2.0.0)
├── README.md                   # This file
├── install.sh                  # Linux/macOS installer
├── install.ps1                 # Windows installer
├── crates/
│   ├── core/                   # Types, errors, config, security, fusion, platform
│   ├── storage/                # SQLite database + migrations (v4)
│   ├── indexer/                # Tantivy BM25 + HNSW vector index
│   ├── embedder/               # ONNX embedding engine + LRU cache
│   ├── chunker/                # Markdown, text, source code chunking
│   ├── scanner/                # Shared scan logic (single source of truth)
│   ├── session/                # Session & conversation memory
│   ├── server/                 # MCP server + REST API (axum)
│   ├── watcher/                # File system watcher (notify)
│   ├── plugin/                 # SDK constants for external integrations
│   ├── benchmark/              # Benchmark & stress test suite
│   └── cli/                    # CLI binary (aikd)
└── extensions/
    └── vscode/                 # VSCode extension
```

---

## Benchmark Results

Tested on: AMD EPYC 7B13 (6 cores), 7.8 GB RAM, Linux x86_64

| Test | Duration | Throughput | Status |
|------|----------|-----------|--------|
| Indexing (1000 files) | 144 ms | 6,934 files/s | PASS |
| BM25 Search (100 queries) | 1,917 ms | 0.21 ms/query | PASS |
| Hybrid Search (50 queries) | 999 ms | 0.35 ms/query | PASS |
| Concurrent Search (500 queries) | 28 ms | 17,669 queries/s | PASS |
| Chunking Throughput (1000 files) | 4 ms | 251,985 files/s | PASS |
| Incremental Re-index (100 files) | 140 ms | 717 files/s | PASS |

**Peak resource usage:** CPU 22.9%, RAM 27.4%

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `aikd: command not found` | Add `~/.local/bin` to your PATH |
| `Config not found` | Run `aikd init` |
| `Model not downloaded` | Run `aikd init` or `aikd model download all-MiniLM-L6-v2` |
| `No results` | Run `aikd scan` first |
| `Hybrid search not working` | Run `aikd embed` first (needs embeddings) |
| `Permission denied` (Linux/macOS) | `chmod +x ~/.local/bin/aikd` |
| `Port already in use` | Change `server.rest_port` in config |
| `GPU not used` | fastembed uses CPU by default; GPU requires custom build |

---

## Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Development Setup

```bash
git clone https://github.com/your-org/aikd.git
cd aikd
cargo build
cargo test
```

### Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` to check for lint issues
- All tests must pass (`cargo test`)

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

---

## Acknowledgements

Built with these amazing open-source projects:

| Library | Purpose |
|---------|---------|
| [Tantivy](https://github.com/quickwit-oss/tantivy) | BM25 full-text search engine |
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) | ONNX embedding inference |
| [hnsw_rs](https://github.com/jeremiedecock/hnsw-rs) | HNSW vector index |
| [rusqlite](https://github.com/rusqlite/rusqlite) | SQLite bindings |
| [axum](https://github.com/tokio-rs/axum) | HTTP framework |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | MCP protocol |
| [notify](https://github.com/notify-rs/notify) | File system watcher |
| [rayon](https://github.com/rayon-rs/rayon) | Data parallelism |
| [blake3](https://github.com/BLAKE3-team/BLAKE3) | Fast hashing |

---

<div align="center">

**Made with Rust and dedication.**

</div>
