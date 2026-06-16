# AIKD — Submission Copy

Copy-paste ready for directories, forums, and social media.

---

## mcp.so / mcp-directory

**Name:** AIKD (AI Knowledge Daemon)
**Description:** Indexed semantic & BM25 code search for AI agents. Give your AI instant memory of your codebase.
**Category:** Developer Tools, Code Search, RAG
**GitHub:** https://github.com/gelutjari/aikd
**Language:** Rust
**License:** MIT

**Short description (160 chars):**
Local-first code search engine for AI agents. BM25 + vector semantic search via MCP protocol. Index any codebase, search in <1ms. Rust, zero telemetry.

---

## smithery.ai

**Name:** aikd
**Description:** AI Knowledge Daemon — indexed semantic and BM25 code search for AI agents. Scans your project, builds a local knowledge base, and exposes 7 MCP tools (scan, query, embed, stats, remember, recall, status) that any MCP-compatible AI agent can call. 100% local-first, no telemetry, no cloud dependency.

**Install:** `npx -y @smithery/cli install aikd`
**Command:** `aikd serve`

---

## Reddit r/LocalLLaMA

**Title:** I built AIKD — a local-first code search engine for AI agents (BM25 + ONNX embeddings, MCP server, Rust)

**Body:**

Hey r/LocalLLaMA!

I've been working on **AIKD** (AI Knowledge Daemon) — a tool that gives AI agents instant memory of your codebase.

**What it does:**
- Scans your project files and builds a local search index
- BM25 full-text search via Tantivy + vector semantic search via ONNX embeddings (all-MiniLM-L6-v2, 384d)
- Hybrid search with Reciprocal Rank Fusion
- Exposes 7 tools via MCP protocol (Model Context Protocol) — works with Claude Code, Cursor, Cline, Continue, Windsurf, and any MCP-capable agent
- Also has REST API and CLI for direct usage
- Session memory: save and recall conversation context across sessions

**Why?**
AI agents need to understand your codebase. Without an index, they grep repeatedly (slow, no ranking) or read entire files (wastes tokens). AIKD pre-indexes everything so agents get ranked results in milliseconds.

**Tech stack:**
- Rust (fast, small binary, no runtime dependencies)
- Tantivy for BM25
- fastembed-rs for ONNX inference
- SQLite for metadata
- Axum for REST API
- rmcp for MCP protocol

**100% local-first:**
- No telemetry, no analytics, no cloud dependency
- Your code never leaves your machine
- Runs entirely offline

**Quick start:**
```bash
curl -sSfL https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
aikd init
aikd scan
aikd query "your search term" --json
```

Benchmarks: 6,934 files/s indexing, 0.21ms/query search, 17,669 queries/s concurrent.

GitHub: https://github.com/gelutjari/aikd

Would love feedback from the community. PRs welcome!

---

## Reddit r/rust

**Title:** AIKD v2.0 — MCP server for AI agents, written in Rust (Tantivy + ONNX + Axum)

**Body:**

Sharing a project I've been building: **AIKD** — an MCP tool provider that gives AI agents instant code search.

**Architecture:**
- 12-crate Rust workspace
- Tantivy for BM25 full-text search
- fastembed-rs for ONNX embedding inference (all-MiniLM-L6-v2)
- HNSW vector index for semantic search
- SQLite (rusqlite) for metadata storage
- Axum for REST API with CORS + auth
- rmcp for MCP protocol (stdio transport)
- notify for file system watching
- blake3 for incremental hashing

**Key design decisions:**
- Shared `Database` and `TantivyEngine` via `Arc<Mutex<T>>` in REST handlers (not per-request open)
- Reciprocal Rank Fusion consolidated to single canonical implementation in `core::fusion`
- Agent registration via helper functions (read_config/write_config/insert_mcp_server_object)
- Path traversal rejection in security module

**Benchmarks (AMD EPYC 7B13, 6 cores):**
- Indexing: 6,934 files/s
- BM25 search: 0.21ms/query
- Hybrid search: 0.35ms/query
- Concurrent: 17,669 queries/s

GitHub: https://github.com/gelutjari/aikd

Feedback and PRs welcome!

---

## Hacker News

**Title:** AIKD – Local-first code search engine for AI agents (Rust, BM25 + ONNX, MCP server)

**URL:** https://github.com/gelutjari/aikd

**Comment (if Show HN):**

AIKD is a local-first code search engine designed for AI agents. It indexes your codebase using BM25 (Tantivy) and vector semantic search (ONNX embeddings), then exposes search via MCP protocol — the standard for AI tool integration.

Key points:
- 100% local, no telemetry, no cloud
- Works with Claude Code, Cursor, Cline, and any MCP-compatible agent
- Also usable as CLI tool or REST API
- Rust binary, ~10MB, no runtime dependencies
- Sub-millisecond search on indexed codebases

The MCP protocol integration means any AI assistant that supports MCP can call `aikd query` to search your codebase semantically. This is particularly useful for large monorepos where grep/ripgrep doesn't provide ranked results.
