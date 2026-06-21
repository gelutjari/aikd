# Show HN Post

## Title
AIKD – Ultra-fast local memory layer for AI coding agents (Rust, MCP native)

## Post

Hi HN! 👋

I built **AIKD** — a Rust-based tool that gives AI coding agents instant memory of your codebase.

**The problem:** AI coding agents (Claude, Cursor, Cline) are powerful but forgetful. Every new conversation starts from zero.

**The solution:** AIKD indexes your code locally and provides instant search via MCP protocol, REST API, or CLI.

### Why Rust?

- **Speed**: 0.21ms per search query across 10,000+ chunks
- **Memory**: ~27% RAM usage for typical projects
- **No dependencies**: Single binary, no Python/Node required
- **Local-first**: Zero cloud dependency

### Technical Highlights

- **Hybrid search**: BM25 (Tantivy) + Vector embeddings (ONNX all-MiniLM-L6-v2)
- **Reciprocal Rank Fusion**: Combines keyword and semantic results
- **Incremental indexing**: Blake3 hashing for zero-redundancy scans
- **MCP native**: Works with Claude Code, Cursor, Cline out of the box

### Benchmarks

| Operation | Time |
|-----------|------|
| Index 1,000 files | 144ms |
| BM25 Search | 0.21ms |
| Hybrid Search | 0.35ms |
| Concurrent (500 queries) | 28ms |

### Quick Start

```bash
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
cd your-project
aikd init && aikd scan && aikd query "authentication"
```

GitHub: https://github.com/gelutjari/aikd

Built with: Rust, Tantivy, ONNX Runtime, SQLite, axum

Would love your feedback!
