# Contributing to AIKD

Thank you for your interest in contributing to AIKD! This guide will help you get started.

## Quick Start

```bash
# 1. Fork & clone
git clone https://github.com/YOUR_USERNAME/aikd.git
cd aikd

# 2. Build
cargo build

# 3. Run tests
cargo test

# 4. Check lint
cargo clippy -- -D warnings
```

## Development Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | 1.75+ | [rustup.rs](https://rustup.rs/) |
| Git | any | [git-scm.com](https://git-scm.com/) |

No other dependencies needed — everything is built from source via Cargo.

## Project Architecture

AIKD is a 12-crate Rust workspace:

```
crates/
├── core/        # Types, errors, config, security, fusion, platform detection
├── storage/     # SQLite database + schema migrations
├── indexer/     # Tantivy BM25 + HNSW vector index
├── embedder/    # ONNX embedding engine (fastembed-rs)
├── chunker/     # File chunking (markdown, source code, text)
├── scanner/     # File discovery + scan orchestration
├── session/     # Conversation memory (remember/recall)
├── server/      # MCP server (rmcp) + REST API (axum)
├── watcher/     # File system watcher (notify)
├── cli/         # CLI binary (clap)
├── plugin/      # SDK constants for external integrations
└── benchmark/   # Benchmark suite
```

### Data Flow

```
Files → Scanner → Chunker → Storage (SQLite)
                              ↓
                          Indexer (Tantivy + HNSW)
                              ↓
                          Server (MCP/REST) → AI Agent
```

### Key Patterns

- **Shared state**: `Database` and `TantivyEngine` are wrapped in `Arc<Mutex<T>>` for async handlers
- **Config**: YAML-based config at `~/.aikd/config.yaml`, loaded via `Config::load()`
- **Security**: Path validation in `core/security.rs` — rejects `..` traversal, null bytes, empty roots
- **Fusion**: Single canonical `reciprocal_rank_fusion` in `core/fusion.rs`

## Code Style

- **Formatting**: `cargo fmt --all` (enforced in CI)
- **Linting**: `cargo clippy -- -D warnings` (zero warnings policy)
- **Tests**: All tests must pass (`cargo test --all`)
- **Naming**: Rust conventions (snake_case functions, CamelCase types)

## Adding a New Feature

1. Create a branch: `git checkout -b feature/my-feature`
2. Write code + tests
3. Ensure all checks pass:
   ```bash
   cargo fmt --all
   cargo clippy -- -D warnings
   cargo test --all
   ```
4. Commit with a clear message: `feat: add hybrid search caching`
5. Push and open a PR

### Commit Message Convention

Use conventional commits:
- `feat:` — new feature
- `fix:` — bug fix
- `docs:` — documentation only
- `refactor:` — code change that neither fixes a bug nor adds a feature
- `test:` — adding or updating tests
- `ci:` — CI/CD changes
- `chore:` — maintenance tasks

## Adding a New AI Agent

To add support for a new MCP-compatible AI agent:

1. Edit `crates/core/src/agents.rs`
2. Add a new `AgentConfig` entry with:
   - `name`: Display name
   - `config_path`: Function that returns the agent's config file path
   - `write_config`: Function that writes the MCP server entry
3. Add tests
4. Open a PR with the agent name in the title: `feat(agent): add Aider support`

Alternatively, open an issue using the "Add AI Agent Support" template.

## Adding a New Language to Chunker

To add AST-aware chunking for a new language:

1. Edit `crates/chunker/src/code.rs`
2. Add the language to `SourceLanguage` enum
3. Add detection in `detect_language()`
4. Add chunking patterns in `chunk_source_code()`
5. Add tests with sample code

## Reporting Bugs

Use the [Bug Report](https://github.com/gelutjari/aikd/issues/new?template=bug_report.md) template.

## Questions?

Open a [GitHub Discussion](https://github.com/gelutjari/aikd/discussions) or comment on an existing issue.
