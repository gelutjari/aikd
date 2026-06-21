# Contributing to AIKD

Thank you for your interest in contributing to AIKD! This document provides guidelines and instructions for contributing.

## 🚀 Quick Start

```bash
# Fork and clone
git clone https://github.com/YOUR_USERNAME/aikd.git
cd aikd

# Create feature branch
git checkout -b feature/amazing-feature

# Make changes, test, commit
cargo test --all
cargo clippy -- -D warnings
git commit -m "feat: add amazing feature"

# Push and create PR
git push origin feature/amazing-feature
```

## 🏗️ Architecture Overview

AIKD is a Rust workspace with 11 crates:

| Crate | Purpose |
|-------|---------|
| `core` | Types, config, errors, security |
| `storage` | SQLite database layer |
| `indexer` | Tantivy BM25 + HNSW vector index |
| `embedder` | ONNX model inference |
| `chunker` | Code/markdown parsing |
| `scanner` | File discovery and indexing |
| `session` | Conversation memory |
| `server` | MCP + REST API |
| `watcher` | File change monitoring |
| `cli` | Command-line interface |
| `benchmark` | Performance testing |

## 📝 Coding Standards

- **Rust Edition 2021** minimum
- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` — must pass
- Add tests for new functionality
- Document public APIs with `///` doc comments
- Use `anyhow::Result` for error handling
- Use `thiserror` for custom error types

## 🧪 Testing

```bash
# Run all tests
cargo test --all

# Run specific crate tests
cargo test -p aikd-core

# Run with output
cargo test -- --nocapture
```

## 📋 Pull Request Process

1. Update documentation if needed
2. Add tests for new features
3. Ensure CI passes (fmt, clippy, test)
4. Request review from maintainers
5. Squash and merge

## 🐛 Reporting Bugs

Use the [Bug Report template](https://github.com/gelutjari/aikd/issues/new?template=bug_report.md).

## 💡 Suggesting Features

Use the [Feature Request template](https://github.com/gelutjari/aikd/issues/new?template=feature_request.md).

## 📜 Code of Conduct

Please read [CODE_OF_CONDUCT.md](.github/CODE_OF_CONDUCT.md).

## 🙏 Acknowledgements

Thanks to all contributors who help make AIKD better!
