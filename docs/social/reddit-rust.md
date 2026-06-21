# Reddit Post for r/rust

## Title
I built a Rust daemon that gives AI coding agents instant memory of your codebase

## Post

Hey r/rust! 👋

I've been working on **AIKD** — a Rust-based tool that gives AI coding agents (like Claude, Cursor, Cline) instant memory of your entire codebase.

### The Problem

Every time you start a new conversation with an AI coding agent, it forgets everything about your project. You have to re-explain your architecture, your patterns, your conventions.

### The Solution

AIKD indexes your codebase locally and provides instant search via:
- **MCP protocol** (native integration with AI agents)
- **REST API** (for custom integrations)
- **CLI** (for scripting)

### Why Rust?

- **Speed**: 0.21ms per search query across 10,000+ chunks
- **Memory**: ~27% RAM usage for typical projects
- **No dependencies**: Single binary, no Python/Node required
- **Local-first**: Zero cloud dependency, your code never leaves your machine

### Technical Highlights

- **Hybrid search**: BM25 (Tantivy) + Vector embeddings (ONNX all-MiniLM-L6-v2)
- **Reciprocal Rank Fusion**: Intelligently combines keyword and semantic results
- **Incremental indexing**: Blake3 hashing for zero-redundancy scans
- **Resource adaptive**: Auto-detects system capabilities and adjusts

### Benchmarks

| Operation | Time |
|-----------|------|
| Index 1,000 files | 144ms |
| BM25 Search | 0.21ms |
| Hybrid Search | 0.35ms |
| Concurrent (500 queries) | 28ms |

### Try It

```bash
# Install
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash

# Get started
cd your-project
aikd init
aikd scan
aikd query "authentication"
```

GitHub: https://github.com/gelutjari/aikd

Would love to hear your feedback! What features would you like to see next?
