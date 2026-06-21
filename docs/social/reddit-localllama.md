# Reddit Post for r/LocalLLaMA

## Title
Local RAG in Rust, 0.21ms queries, no Python bloat

## Post

Hey r/LocalLLaMA! 👋

Wanted to share a tool I built for local RAG (Retrieval Augmented Generation) that's designed for speed and simplicity.

### What is AIKD?

**AIKD** (AI Knowledge Daemon) is a Rust-based tool that gives AI coding agents instant memory of your codebase. Think of it as a local RAG system optimized for code.

### Key Features

- **100% local**: Zero cloud dependency, your code never leaves your machine
- **Fast**: 0.21ms per search query (yes, milliseconds)
- **Hybrid search**: BM25 + Vector embeddings (ONNX all-MiniLM-L6-v2)
- **Single binary**: No Python, no pip, no CUDA setup
- **MCP native**: Works with Claude, Cursor, Cline out of the box

### Why Not Python?

Python-based RAG solutions work, but:
- **Slow**: 500ms+ per query (AIKD: 0.21ms)
- **Heavy**: 2GB+ dependencies (AIKD: single 33MB binary)
- **Complex**: Python, pip, CUDA, models... (AIKD: one install command)

### Technical Details

- **Embedding model**: all-MiniLM-L6-v2 (384 dimensions)
- **Vector index**: HNSW (pure Rust implementation)
- **BM25 index**: Tantivy
- **Storage**: SQLite with WAL mode
- **Inference**: ONNX Runtime (no Python)

### Benchmarks

| Operation | AIKD | Python RAG |
|-----------|------|------------|
| Query latency | 0.21ms | 500ms+ |
| Memory usage | ~27% RAM | 50%+ RAM |
| Install size | 33MB | 2GB+ |
| Dependencies | 0 | Python, pip, CUDA |

### Try It

```bash
# Install
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash

# Index your codebase
cd your-project
aikd init
aikd scan

# Search
aikd query "authentication" --hybrid
```

GitHub: https://github.com/gelutjari/aikd

Would love to hear from the local LLM community! What use cases do you see for local RAG in Rust?
