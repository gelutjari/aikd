# AIKD Architecture

## High-Level Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        AIKD v2.0.0                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────┐  │
│  │  CLI    │  │  MCP    │  │  REST   │  │ File Watcher│  │
│  │ (clap)  │  │ (rmcp)  │  │ (axum)  │  │  (notify)   │  │
│  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬──────┘  │
│       │            │            │               │          │
│       └────────────┴────────────┴───────────────┘          │
│                            │                                │
│  ┌─────────────────────────┴──────────────────────────┐    │
│  │                   Core Engine                       │    │
│  ├─────────────┬─────────────┬─────────────────────────┤    │
│  │  Scanner    │   Chunker   │      Session Manager    │    │
│  │ (walkdir)   │(pulldown-   │    (conversation DB)    │    │
│  │             │  cmark)     │                         │    │
│  └──────┬──────┴──────┬──────┴─────────────────────────┘    │
│         │             │                                      │
│  ┌──────┴──────┐ ┌────┴─────┐ ┌──────────────────────┐     │
│  │  Indexer    │ │ Embedder │ │     Storage           │     │
│  │  (Tantivy)  │ │ (ONNX)   │ │   (SQLite + WAL)     │     │
│  │  BM25 +     │ │ 384d     │ │   + r2d2 pool        │     │
│  │  HNSW ANN   │ │ MiniLM   │ │   + blake3 hashing   │     │
│  └─────────────┘ └──────────┘ └──────────────────────┘     │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

### Scan Pipeline
```
1. Scanner discovers files (walkdir)
       ↓
2. Filter by extension, size, exclude patterns
       ↓
3. Chunker parses content (pulldown-cmark for MD, tree-sitter for code)
       ↓
4. Storage persists chunks to SQLite
       ↓
5. Indexer builds Tantivy BM25 index
       ↓
6. Embedder generates 384d vectors (ONNX all-MiniLM-L6-v2)
       ↓
7. Vector index stored in SQLite embeddings table
```

### Query Pipeline
```
1. User query arrives (CLI/MCP/REST)
       ↓
2. BM25 search via Tantivy (keyword matching)
       ↓
3. Vector search via HNSW (semantic similarity)
       ↓
4. Reciprocal Rank Fusion combines results
       ↓
5. Apply filters (path, type, heading)
       ↓
6. Return top-K results with scores
```

## Crate-by-Crate Breakdown

### `aikd-core`
- Types: `Chunk`, `SearchResult`, `Session`, `Conversation`
- Config: YAML-based configuration with smart defaults
- Security: Path validation, input sanitization
- Resource: Auto-detection of system capabilities

### `aikd-storage`
- SQLite with WAL mode for concurrent reads
- r2d2 connection pooling (10 connections)
- Blake3 incremental hashing
- Schema migrations

### `aikd-indexer`
- Tantivy for BM25 full-text search
- HNSW index for vector similarity
- Hybrid search with RRF fusion
- Memory-mapped vector option for large datasets

### `aikd-embedder`
- ONNX Runtime for inference
- all-MiniLM-L6-v2 (384 dimensions)
- Batch processing with adaptive sizing
- LRU cache for frequent queries

### `aikd-chunker`
- Markdown: Split by headings, preserve hierarchy
- Code: Split by functions/classes (language-aware)
- Token-aware: Respects max/min token limits
- Unique IDs for each chunk

### `aikd-scanner`
- Walkdir-based file discovery
- Configurable include/exclude patterns
- File size filtering
- Extension-based filtering

### `aikd-session`
- Persistent conversation storage
- Session-based memory
- Recall with semantic search
- Auto-cleanup of old sessions

### `aikd-server`
- MCP protocol (stdio transport)
- REST API (axum on port 9090)
- JWT authentication
- Rate limiting (10 req/s)
- Prometheus metrics

### `aikd-watcher`
- notify-based file monitoring
- Debounced event handling
- Incremental re-indexing
- Thread-safe event queue

## Key Design Decisions

1. **SQLite over PostgreSQL**: Zero-dependency, single-file, sufficient for local use
2. **Tantivy over Elasticsearch**: Native Rust, no JVM, fast BM25
3. **ONNX over PyTorch**: No Python dependency, cross-platform
4. **HNSW over FAISS**: Pure Rust implementation
5. **Blake3 over SHA256**: 10x faster hashing for incremental scans
