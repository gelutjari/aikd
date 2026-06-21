# Changelog

All notable changes to AIKD will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2025-06-21

### 🔒 Security Fixes

- **Auth bypass fixed**: Empty `auth_token` no longer bypasses authentication
- **CORS hardening**: Explicit origin whitelist instead of permissive mode
- **Path traversal prevention**: Added symlink, UNC path, and sensitive directory checks
- **Rate limiting**: 10 requests/second per IP via `tower_governor`
- **JWT authentication**: Token-based auth for REST API
- **Constant-time comparison**: Prevents timing attacks on token validation

### ⚡ Performance Improvements

- **N+1 query fix**: Batch `WHERE IN (...)` instead of individual queries (~50x faster)
- **Hybrid search optimization**: HashMap-based O(n) merging instead of O(n²) linear search
- **Query cache**: `moka` cache with 1000 entries and 5-minute TTL
- **Connection pooling**: `r2d2_sqlite` with 10 connections for concurrent access
- **Async model download**: Non-blocking ONNX model download

### ✨ New Features

- **Prometheus metrics**: `/metrics/prometheus` endpoint for monitoring
- **Memory-mapped VectorIndex**: Reduced memory usage for large datasets
- **Thread-safe watcher**: Mutex-protected event handling
- **JWT login endpoint**: `/api/auth/login` for token generation

### 📦 Dependencies Added

- `r2d2` + `r2d2_sqlite` for connection pooling
- `jsonwebtoken` for JWT support
- `memmap2` for memory-mapped files
- `moka` for caching
- `tower_governor` + `governor` for rate limiting
- `prometheus` for metrics
- `tokio-stream` + `futures` for async streams

### 🔧 Code Quality

- Fixed all clippy warnings
- Added unit tests for security functions
- Improved error handling with proper error types
- Added rustdoc comments to public APIs

### 📖 Documentation

- Complete README overhaul with badges, tables, architecture diagram
- Added CONTRIBUTING.md, SECURITY.md, CHANGELOG.md
- Added docs/ directory with QUICKSTART, CONFIGURATION, RECIPE guides
- Added GitHub issue and PR templates

### 🔄 CI/CD

- GitHub Actions workflow for CI (fmt, clippy, test)
- Release workflow for multi-platform binaries
- crates.io publish workflow
- Dependabot configuration

## [1.0.0] - 2025-06-15

### Initial Release

- MCP server with 7 tools
- BM25 + Vector hybrid search
- SQLite storage with Tantivy index
- ONNX embedding (all-MiniLM-L6-v2)
- File watcher with blake3 hashing
- CLI interface
- REST API on port 9090
- Auto-registration with 6 AI agents
- Cross-platform support (Windows, Linux, macOS)
