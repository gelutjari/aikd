# AIKD — AI Knowledge Daemon v1.1.0 — FULL REVERSE ENGINEERING DOCUMENTATION

_Dibuat: 15 Juni 2026 | Berdasarkan pembacaan langsung seluruh source code_

---

## EXECUTIVE SUMMARY

AIKD (AI Knowledge Daemon) adalah background knowledge indexer yang ditulis dalam Rust, dirancang untuk meng-index file-file project (Markdown, JSON, YAML, TOML, TXT) ke dalam database SQLite dan search engine Tantivy. Sistem ini menyediakan hybrid search yang menggabungkan BM25 full-text search (Tantivy) dengan vector semantic search (fastembed-rs ONNX, model all-MiniLM-L6-v2, 384 dimensi) melalui algoritma Reciprocal Rank Fusion (RRF). AIKD berjalan sebagai CLI tool, MCP server (untuk integrasi dengan AI assistant seperti Claude/MiMo), REST API server (Axum), dan file watcher daemon. Arsitekturnya menggunakan Rust workspace dengan 11 crate yang terpisah secara modular.

---

## QUICK REFERENCE CARD

```bash
# Inisialisasi
aikd init [--path <DIR>]           # Buat config + download model + install shell hooks

# Indexing
aikd scan [--path <DIR>]           # Scan, chunk, dan index file ke SQLite + Tantivy
aikd watch [--debounce <MS>]       # File watcher daemon (auto-sync)

# Search
aikd query "keyword"               # BM25 full-text search
aikd query "keyword" --hybrid      # Hybrid search (BM25 + vector RRF)
aikd query "keyword" --json        # Output JSON
aikd query "keyword" --path "/docs" --heading "API"  # Dengan filter

# Embedding
aikd embed [--model <NAME>] [--batch <N>]  # Generate vector embeddings
aikd export [-o chunks.json]       # Export chunks ke JSON
aikd import --file <FILE>          # Import embeddings dari JSON

# Server
aikd serve                         # MCP server (stdio transport)
aikd daemon --foreground           # REST API + MCP server

# Session Memory
aikd remember --role user --content "message" [--session <ID>]
aikd recall "query" [--session <ID>] [--limit <N>]

# Utility
aikd stats                         # Statistik index
aikd status                        # Resource & daemon status
aikd inject -- <COMMAND>           # Context injection wrapper
aikd benchmark                     # Jalankan 8-scenario benchmark suite
```

---

## FASE 1 — IDENTITAS PROJECT

```
NAMA PROJECT     : AIKD — AI Knowledge Daemon
VERSI            : 1.1.0 (workspace.version di Cargo.toml)
BAHASA UTAMA     : Rust (edition 2021)
RUNTIME/PLATFORM : Native binary (Linux/macOS/Windows), tokio async runtime
PARADIGMA        : Modular workspace, async/await + parallel (rayon), event-driven (file watcher)
TIPE APLIKASI    : CLI tool + MCP server + REST API daemon + file watcher
TUJUAN UTAMA     : Meng-index file project menjadi knowledge base yang dapat dicari dengan hybrid search
TARGET USER      : Developer, AI assistants (melalui MCP protocol)
LISENSI          : MIT
ENTRY POINT      : crates/cli/src/main.rs → fn main() (binary name: `aikd`)
```

---

## FASE 2 — ARSITEKTUR SISTEM

### 2A. Layer Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        INTERFACE LAYER                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐           │
│  │ CLI      │  │ MCP      │  │ REST API │  │ VSCode   │           │
│  │ (clap 4) │  │ (rmcp)   │  │ (axum)   │  │ Extension│           │
│  │ 15 cmd   │  │ stdio    │  │ :9090    │  │ JS       │           │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘           │
│       │              │              │              │                │
├───────┴──────────────┴──────────────┴──────────────┴────────────────┤
│                        ORCHESTRATION LAYER                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                         │
│  │ Session  │  │ Watcher  │  │Benchmark │                         │
│  │ Memory   │  │ (notify) │  │ (8 test) │                         │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘                         │
│       │              │              │                               │
├───────┴──────────────┴──────────────┴───────────────────────────────┤
│                        CORE LOGIC LAYER                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │ Chunker  │  │ Indexer  │  │ Embedder │  │ Config   │          │
│  │ MD/TXT/  │  │ Tantivy  │  │ fastembed│  │ YAML     │          │
│  │ JSON/YML │  │ + HNSW   │  │ ONNX     │  │ auto-det │          │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘          │
│       │              │              │              │                │
├───────┴──────────────┴──────────────┴──────────────┴────────────────┤
│                        STORAGE LAYER                                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │ SQLite   │  │ Tantivy  │  │ ONNX     │  │ YAML     │          │
│  │ (rusqlite│  │ Index    │  │ Model    │  │ Config   │          │
│  │  WAL)    │  │ (BM25)   │  │ (disk)   │  │ File     │          │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘          │
└─────────────────────────────────────────────────────────────────────┘
```

### 2B. Crate Dependency Graph

```
                    aikd-cli (binary)
                   /    |    \     \
                  /     |     \     \
           aikd-server  |  aikd-watcher  aikd-benchmark
           /    |       |     |            /    |    \
     aikd-indexer |  aikd-session    aikd-indexer |  aikd-chunker
          |   aikd-embedder    |         |        |       |
          |       |            |         |        |       |
          +---+---+---+-------+---------+--------+-------+
              |       |                               |
          aikd-storage                           aikd-core
              |                               (types, config,
              |                                error, resource)
          aikd-core
                          
     aikd-plugin (SDK constants, standalone)
```

### 2C. Data Flow — Scan & Index

```
File System
    │
    ▼
WalkDir (recursive, rayon parallel)
    │
    ▼
Filter: extensions, exclude_dirs, exclude_files, file_size, content_filter
    │
    ▼
compute_blake3() → hash check (incremental skip jika unchanged)
    │
    ▼
┌─────────────────────────────────────┐
│         Chunker Dispatch            │
│  .md/.markdown → chunk_markdown()   │
│  .json/.yaml/.toml → chunk_structured() │
│  lainnya → chunk_plain_text()       │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  SQLite Transaction (batch)         │
│  1. DELETE old file + chunks + embs │
│  2. INSERT file record              │
│  3. INSERT chunk records            │
│  COMMIT                             │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Tantivy Index                      │
│  1. clear() → delete_all_documents  │
│  2. index_chunks() → add_document   │
│  3. commit + reader.reload          │
└─────────────────────────────────────┘
```

### 2D. Data Flow — Hybrid Search

```
User Query
    │
    ├──────────────────────┐
    ▼                      ▼
┌──────────┐        ┌──────────┐
│  BM25    │        │  Vector  │
│ Tantivy  │        │ Search   │
│ (limit×2)│        │(limit×2) │
└────┬─────┘        └────┬─────┘
     │                   │
     ▼                   ▼
┌──────────────────────────────────┐
│  Reciprocal Rank Fusion (k=60)   │
│  score += 1/(k + rank + 1)       │
│  Gabung BM25 + vector scores     │
│  Sort descending                 │
└──────────────┬───────────────────┘
               │
               ▼
┌──────────────────────────────────┐
│  Load chunks dari SQLite         │
│  Apply path/heading filters      │
│  Enrich dengan line numbers      │
└──────────────┬───────────────────┘
               │
               ▼
    Search Results (JSON/formatted)
```

---

## FASE 3 — ANALISIS SETIAP CRATE

### 3.1 `aikd-core` (crates/core/) — 4 file, 624 baris

**Tujuan:** Tipe data fundamental, error handling, konfigurasi, dan resource detection.

**File:**

| File | Baris | Deskripsi |
|------|-------|-----------|
| `src/lib.rs` | 9 | Re-export modul: config, error, resource, types |
| `src/types.rs` | 73 | Struct Chunk, SearchResult, SearchFilters, Session, Conversation, ConversationEmbedding |
| `src/error.rs` | 36 | Enum AikdError (Database, Io, SearchIndex, Embedding, Config, SessionNotFound, PathTraversal, Serialization, Yaml, Other) |
| `src/config.rs` | 406 | Config struct dengan 8 sub-config, smart config generator, 13 unit tests |
| `src/resource.rs` | 196 | ResourceProfile detection (CPU, RAM, GPU), 5 mode (Low/Medium/High/Max/Auto), 6 unit tests |

**Struct Kunci:**

```rust
// types.rs
pub struct Chunk {
    pub id: String,                       // UUID v4
    pub file_path: String,                // Absolute path
    pub chunk_index: usize,               // Index dalam file
    pub heading_hierarchy: Vec<String>,   // ["H1", "H2", "H3"]
    pub heading_level: usize,             // 1-6
    pub heading_text: String,             // Heading text saat ini
    pub line_start: usize,                // Baris awal
    pub line_end: usize,                  // Baris akhir
    pub content: String,                  // Isi chunk
    pub metadata: HashMap<String, Value>, // YAML frontmatter
}

pub struct SearchResult {
    pub chunk_id: String,
    pub file_path: String,
    pub heading_hierarchy: String,  // "H1 > H2 > H3"
    pub heading_text: String,
    pub content: String,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
}

// config.rs
pub struct Config {
    pub version: String,            // "1.1.0"
    pub scan: ScanConfig,           // include_paths, exclude_paths, extensions
    pub chunk: ChunkConfig,         // max_tokens=1000, min_tokens=100
    pub embedding: EmbeddingConfig, // model, batch_size, device, threads
    pub index: IndexConfig,         // db_path, tantivy_path, model_path, cache_size_mb
    pub server: ServerConfig,       // rest_port=9090, auth_token, cors_origins
    pub filter: FilterConfig,       // filename/content contains/exclude, max_file_size
    pub resource: ResourceConfig,   // mode: Auto
}

// resource.rs
pub struct ResourceProfile {
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub has_gpu: bool,
    pub embedding_enabled: bool,
    pub batch_size: usize,       // 1-256 tergantung resource
    pub parallelism: usize,      // 1-16
    pub hnsw_m: usize,           // 4-64
    pub hnsw_ef_construction: usize, // 32-256
    pub cache_size_mb: usize,    // 64-1024
}
```

**Smart Config Detection:**
- Rust project (`Cargo.toml`) → extensions: rs, md, toml, yaml, yml, txt
- Node.js (`package.json`/`tsconfig.json`) → extensions: ts, tsx, js, jsx, md, json, yaml, yml
- Python (`pyproject.toml`/`requirements.txt`) → extensions: py, md, yaml, yml, json, txt, toml, cfg, ini
- Go (`go.mod`) → extensions: go, md, yaml, yml, json, txt, toml

**Resource Auto-Scaling:**

| RAM | CPU | Mode | batch_size | parallelism | hnsw_m | embedding |
|-----|-----|------|------------|-------------|--------|-----------|
| <2GB | - | Low | 1 | 1 | 4 | OFF |
| <8GB | ≤4 | Medium | 8 | 2 | 8 | ON |
| <16GB | ≤8 | High | 32 | 4 | 16 | ON |
| ≥16GB | >8 | Max | 64 | 8 | 32 | ON |
| GPU | - | GPU | 256 | min(cpu,16) | 64 | ON |

---

### 3.2 `aikd-storage` (crates/storage/) — 2 file, 295 baris

**Tujuan:** SQLite database layer dengan migration system dan blake3 hashing.

**File:**

| File | Baris | Deskripsi |
|------|-------|-----------|
| `src/lib.rs` | 52 | Database struct, Transaction wrapper, compute_blake3() |
| `src/schema.rs` | 243 | Migration system v1-v3, 6 unit tests |

**Database Schema (3 migrasi):**

```sql
-- Migration v1: Core tables
CREATE TABLE files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    size INTEGER NOT NULL,
    modified_at TEXT NOT NULL,
    last_scanned TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    blake3_hash TEXT NOT NULL DEFAULT ''
);
CREATE TABLE chunks (
    id TEXT PRIMARY KEY,              -- UUID
    file_id INTEGER NOT NULL REFERENCES files(id),
    chunk_index INTEGER NOT NULL,
    heading_hierarchy TEXT NOT NULL,   -- JSON array
    heading_level INTEGER NOT NULL,
    heading_text TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    content TEXT NOT NULL,
    metadata_json TEXT,                -- JSON object
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Migration v2: Embeddings
CREATE TABLE embeddings (
    chunk_id TEXT PRIMARY KEY REFERENCES chunks(id),
    model TEXT NOT NULL,               -- "all-MiniLM-L6-v2"
    dimensions INTEGER NOT NULL,       -- 384
    vector BLOB NOT NULL               -- f32 array as little-endian bytes
);

-- Migration v3: Session memory
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    project_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_active TEXT NOT NULL
);
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL CHECK(role IN ('user','assistant','system')),
    content TEXT NOT NULL,
    tokens INTEGER DEFAULT 0,
    chunk_refs TEXT,                    -- JSON array
    created_at TEXT NOT NULL
);
CREATE TABLE conversation_embeddings (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id),
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector BLOB NOT NULL
);
```

**SQLite PRAGMAs:**
```sql
PRAGMA journal_mode=WAL;      -- Write-Ahead Logging
PRAGMA synchronous=NORMAL;    -- Balance durability/performance
PRAGMA foreign_keys=ON;       -- FK constraints
```

**Fungsi Kunci:**

| Fungsi | Lokasi | Deskripsi |
|--------|--------|-----------|
| `Database::open()` | lib.rs:12 | Buka DB, buat parent dir, set PRAGMAs, jalankan migrations |
| `Database::begin_transaction()` | lib.rs:27 | Mulai unchecked_transaction |
| `compute_blake3()` | lib.rs:48 | Hash file content dengan blake3 untuk incremental detection |
| `run_migrations()` | schema.rs:6 | Jalankan v1→v2→v3 secara idempotent |

---

### 3.3 `aikd-indexer` (crates/indexer/) — 1 file, 354 baris

**Tujuan:** Tantivy BM25 full-text search engine dan HNSW vector index.

**Struct Kunci:**

```rust
pub struct TantivyEngine {
    index: Index,
    reader: IndexReader,
    schema: Schema,
    field_chunk_id: Field,   // STRING | STORED
    field_file_path: Field,  // TEXT | STORED
    field_heading: Field,    // TEXT | STORED
    field_content: Field,    // TEXT | STORED
}

pub struct VectorIndex {
    data: RwLock<Vec<Vec<f32>>>,  // Vectors in memory
    id_map: RwLock<Vec<String>>,  // Chunk ID mapping
    dim: usize,
}

pub struct HybridSearcher {
    tantivy: TantivyEngine,
    vector_index: Arc<VectorIndex>,
}
```

**HNSW Parameters:**

| Parameter | Konstanta | Nilai |
|-----------|-----------|-------|
| M (connections) | `HNSW_M` | 16 |
| ef_construction | `HNSW_EF_CONSTRUCTION` | 200 |
| ef_search | `HNSW_EF_SEARCH` | 64 |
| Distance | `DistCosine` | Cosine similarity |

**Fungsi Kunci:**

| Fungsi | Lokasi | Deskripsi |
|--------|--------|-----------|
| `TantivyEngine::open()` | lib.rs:82 | Buka/buat Tantivy index dengan schema 4 field |
| `TantivyEngine::index_chunks()` | lib.rs:114 | Index batch chunks (writer 50MB heap) |
| `TantivyEngine::search()` | lib.rs:138 | BM25 search dengan filter (path, heading, file_type) |
| `TantivyEngine::clear()` | lib.rs:131 | Hapus semua documents |
| `VectorIndex::insert()` | lib.rs:33 | Tambah vector ke in-memory index |
| `VectorIndex::search()` | lib.rs:40 | HNSW ANN search (bangun index on-the-fly) |
| `HybridSearcher::hybrid_search()` | lib.rs:236 | Gabung BM25 + ANN → RRF |
| `reciprocal_rank_fusion()` | lib.rs:275 | Formula: score += 1/(k + rank + 1), k=60 |

**Catatan Penting:** HNSW index dibangun ulang setiap kali search dipanggil (on-the-fly). Ini O(n) untuk build, tapi O(log n) untuk search. Untuk dataset besar, sebaiknya persistent HNSW index.

---

### 3.4 `aikd-embedder` (crates/embedder/) — 1 file, 257 baris

**Tujuan:** Vector embedding generation menggunakan fastembed-rs (ONNX Runtime native Rust).

**Konstanta:**

```rust
pub const MODEL_NAME: &str = "all-MiniLM-L6-v2";
pub const DIMENSIONS: usize = 384;
```

**Model Files (diunduh dari HuggingFace):**

| File | URL |
|------|-----|
| `model.onnx` | `huggingface.co/sentence-transformers/all-MiniLM-L6-v2/.../model.onnx` |
| `tokenizer.json` | `.../tokenizer.json` |
| `config.json` | `.../config.json` |
| `special_tokens_map.json` | `.../special_tokens_map.json` |
| `tokenizer_config.json` | `.../tokenizer_config.json` |

**Fungsi Kunci:**

| Fungsi | Lokasi | Deskripsi |
|--------|--------|-----------|
| `download_model()` | lib.rs:18 | Download 5 file ONNX dari HuggingFace (blocking, 300s timeout) |
| `is_model_downloaded()` | lib.rs:42 | Cek apakah semua 5 file sudah ada |
| `create_model()` | lib.rs:46 | Load ONNX model dari disk → TextEmbedding |
| `embed_and_store()` | lib.rs:63 | Generate embeddings untuk chunks yang belum punya, simpan ke SQLite |
| `embed_and_store_with_profile()` | lib.rs:92 | Wrapper dengan resource profile check |
| `cosine_similarity()` | lib.rs:100 | Hitung cosine similarity antara 2 vektor f32 |
| `vector_search()` | lib.rs:112 | Brute-force cosine search (load semua embeddings) |
| `reciprocal_rank_fusion()` | lib.rs:121 | RRF fusion (duplikat dari indexer, digunakan di CLI) |
| `load_all_embeddings()` | lib.rs:130 | Load SEMUA embeddings dari SQLite ke memory |
| `store_embeddings()` | lib.rs:143 | Batch insert embeddings |
| `import_embeddings_json()` | lib.rs:161 | Import dari JSON array [{chunk_id, embedding}] |
| `export_chunks_for_embedding()` | lib.rs:184 | Export chunks ke JSON untuk external embedding |
| `f32_to_bytes()` | lib.rs:201 | f32 array → little-endian bytes |
| `bytes_to_f32()` | lib.rs:207 | little-endian bytes → f32 array |

---

### 3.5 `aikd-chunker` (crates/chunker/) — 2 file, 326 baris

**Tujuan:** Memecah file menjadi chunks berdasarkan tipe file.

**File:**

| File | Baris | Deskripsi |
|------|-------|-----------|
| `src/lib.rs` | 142 | Dispatch chunker, plain text chunker, structured chunker, 5 tests |
| `src/markdown.rs` | 200 | Markdown chunker dengan heading hierarchy tracking, frontmatter extraction |

**File Type Detection:**

| Tipe | Extension | Chunking Strategy |
|------|-----------|-------------------|
| Markdown | `.md`, `.markdown` | Per heading section, merge small chunks |
| Structured | `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` | 1 chunk per file (whole content) |
| Text | Semua lainnya | Per token count (max_tokens threshold) |

**Markdown Chunking Algorithm (`chunk_markdown`):**

1. Extract YAML frontmatter (`---` delimiters) → metadata HashMap
2. Strip frontmatter dari content
3. Parse dengan pulldown-cmark (extended options: heading attributes, tables)
4. Track heading stack (level hierarchy)
5. Pada setiap heading baru: simpan chunk sebelumnya jika ≥ min_tokens
6. Jika content melebihi max_tokens: potong dan buat chunk baru
7. Di akhir: merge sisa content ke chunk terakhir jika < min_tokens
8. Fallback: jika tidak ada chunk, buat 1 chunk dari seluruh content

**Token Estimation:** `content.len() / 4` (approximasi 4 bytes per token)

---

### 3.6 `aikd-session` (crates/session/) — 1 file, 219 baris

**Tujuan:** Session dan conversation memory management.

**Fungsi Kunci:**

| Fungsi | Lokasi | Deskripsi |
|--------|--------|-----------|
| `create_session()` | lib.rs:6 | Buat session baru dengan UUID |
| `get_or_create_session()` | lib.rs:22 | Cari session berdasarkan project_path, buat jika tidak ada |
| `list_sessions()` | lib.rs:46 | List semua sessions (ordered by last_active DESC) |
| `remember()` | lib.rs:60 | Simpan conversation message (role: user/assistant/system) |
| `recall()` | lib.rs:91 | Cari conversation berdasarkan keyword (simple lowercase matching) |
| `embed_conversations()` | lib.rs:123 | Generate embeddings untuk conversations |
| `get_session_stats()` | lib.rs:151 | Hitung jumlah sessions, conversations, embeddings |

**Catatan:** `recall()` menggunakan simple keyword matching (`.contains()`), bukan semantic search. Ini cukup untuk recall sederhana tapi tidak untuk query kompleks.

---

### 3.7 `aikd-server` (crates/server/) — 3 file, 748 baris

**Tujuan:** MCP server (stdio) dan REST API (Axum) untuk integrasi eksternal.

**File:**

| File | Baris | Deskripsi |
|------|-------|-----------|
| `src/lib.rs` | 33 | AppState, run_mcp_server(), run_rest_server() |
| `src/mcp.rs` | 404 | MCP server dengan 7 tools (scan, query, stats, embed, remember, recall, status) |
| `src/rest.rs` | 316 | REST API dengan 5 endpoints + auth + CORS |

**MCP Tools (rmcp protocol):**

| Tool | Deskripsi | Parameters |
|------|-----------|------------|
| `scan` | Scan dan index files | `path: Option<String>` |
| `query` | Search knowledge base | `query`, `limit`, `path_filter`, `heading_filter`, `hybrid` |
| `stats` | Dapatkan statistik | (none) |
| `embed` | Generate embeddings | `batch_size: Option<usize>` |
| `remember` | Simpan conversation | `session_id`, `role`, `content` |
| `recall` | Cari conversation | `query`, `session_id`, `limit` |
| `status` | Resource & daemon status | (none) |

**REST API Endpoints:**

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/query?q=...&limit=N&path=...&heading=...&hybrid=bool` | Search |
| GET | `/api/stats` | Statistik |
| POST | `/api/scan` | Scan & index (body: `{path?}`) |
| POST | `/api/remember` | Simpan conversation (body: `{session_id?, role, content}`) |
| POST | `/api/recall` | Cari conversation (body: `{query, session_id?, limit?}`) |

**Authentication:**
- Jika `config.server.auth_token` di-set → wajib `Authorization: Bearer <token>`
- Jika tidak di-set → semua request diizinkan
- CORS: `*` (allow all) atau custom origins

---

### 3.8 `aikd-watcher` (crates/watcher/) — 1 file, 156 baris

**Tujuan:** File system watcher dengan incremental indexing.

**Alur:**
1. Setup `notify::recommended_watcher` untuk setiap scan path
2. Event loop dengan `recv_timeout(debounce_duration)`
3. Filter events: hanya Create, Modify, Remove untuk extensions yang cocok
4. Debounce: kumpulkan events, proses batch saat quiet period
5. Incremental check: blake3 hash comparison untuk skip file yang tidak berubah
6. Create/Modify → re-chunk + re-index (hapus data lama, insert data baru)
7. Remove → hapus dari SQLite

---

### 3.9 `aikd-plugin` (crates/plugin/) — 1 file, 43 baris

**Tujuan:** SDK constants untuk integrasi eksternal.

```rust
pub const REST_API_BASE: &str = "http://127.0.0.1:9090";
pub const ENDPOINT_QUERY: &str = "/api/query";
pub const ENDPOINT_STATS: &str = "/api/stats";
pub const ENDPOINT_SCAN: &str = "/api/scan";
pub const ENDPOINT_REMEMBER: &str = "/api/remember";
pub const ENDPOINT_RECALL: &str = "/api/recall";
```

**Struct:** PluginQuery, PluginResult, PluginRemember, PluginRecall (serializable DTOs)

---

### 3.10 `aikd-benchmark` (crates/benchmark/) — 2 file, 907 baris

**Tujuan:** Benchmark & stress test suite dengan resource monitoring.

**8 Benchmark Scenarios:**

| # | Nama | Deskripsi |
|---|------|-----------|
| 1 | Indexing (1000 files) | WalkDir + chunk + SQLite + Tantivy |
| 2 | BM25 Search (100 queries) | 100 Tantivy searches, avg latency |
| 3 | Hybrid Search (50 queries) | BM25 + vector + RRF |
| 4 | Embedding (500 chunks) | fastembed-rs generate + store (skip jika model tidak ada) |
| 5 | Incremental Re-index (100 files) | Modify 100 files, re-chunk + re-index |
| 6 | Concurrent Search (10 threads × 50 queries) | rayon parallel Tantivy search |
| 7 | REST API Stress (100 requests) | 100 concurrent HTTP requests (skip jika server off) |
| 8 | Chunking Throughput (1000 files) | Parallel chunk tanpa DB write |

**Resource Monitor:**
- Threshold: CPU ≤50%, RAM ≤50%
- `throttle_if_needed()` → sleep 200ms jika melebihi limit
- `ResourceMonitor::check()` → sysinfo System refresh

---

### 3.11 `aikd-cli` (crates/cli/) — 1 file, 678 baris

**Tujuan:** Binary entry point dengan 15 CLI commands.

**15 Commands:**

| Command | Fungsi | Mode |
|---------|--------|------|
| `init` | Buat config + download model + shell hooks + MCP config | sync |
| `daemon` | Start REST + MCP server | async |
| `scan` | Scan, chunk, index files | sync |
| `query` | BM25 atau hybrid search | sync |
| `stats` | Tampilkan statistik index | sync |
| `export` | Export chunks ke JSON | sync |
| `import` | Import embeddings dari JSON | sync |
| `embed` | Generate vector embeddings | sync |
| `serve` | MCP server (stdio) | async |
| `watch` | File watcher daemon | async |
| `remember` | Simpan conversation | sync |
| `recall` | Cari conversation | sync |
| `status` | Resource & daemon status | sync |
| `inject` | Context injection wrapper | sync |
| `benchmark` | Jalankan benchmark suite | async |

**Shell Hook (`install_shell_hook`):**
- Tulis ke `~/.bashrc` dan `~/.zshrc`
- Override `cd()` → auto-start daemon jika config ada
- Tulis `~/.aikd/mcp.json` untuk AI assistant discovery

---

### 3.12 VSCode Extension (extensions/vscode/) — 2 file, 169 baris

**Tujuan:** VSCode integration untuk search, scan, stats.

**Commands:**
- `aikd.search` — Input box → query API → QuickPick results → buka file
- `aikd.scan` — POST `/api/scan`
- `aikd.stats` — GET `/api/stats`
- `aikd.status` — Health check

**Features:**
- Auto-start daemon saat VSCode dibuka (`onStartupFinished`)
- Status bar indicator (database icon)
- Auth token support via VSCode settings
- 5s request timeout

---

## FASE 4 — DEPENDENCIES

### 4A. Rust Dependencies (workspace)

| Crate | Versi | Tujuan |
|-------|-------|--------|
| `tokio` | 1 (full) | Async runtime |
| `clap` | 4 (derive) | CLI argument parsing |
| `serde` | 1 (derive) | Serialization |
| `serde_json` | 1 | JSON |
| `serde_yaml` | 0.9 | YAML config |
| `rusqlite` | 0.40 (bundled) | SQLite database |
| `tantivy` | 0.22 | BM25 full-text search |
| `notify` | 6 | File system events |
| `walkdir` | 2 | Recursive directory traversal |
| `rayon` | 1.12 | Parallel processing |
| `globset` | 0.4 | Glob pattern matching |
| `pulldown-cmark` | 0.11 | Markdown parsing |
| `uuid` | 1 (v4) | UUID generation |
| `chrono` | 0.4 (serde) | Timestamps |
| `log` | 0.4 | Logging facade |
| `env_logger` | 0.11 | Logging implementation |
| `anyhow` | 1 | Error handling |
| `thiserror` | 2 | Derive Error trait |
| `shellexpand` | 3 | Tilde expansion |
| `fastembed` | 5 | ONNX embedding (native Rust) |
| `rmcp` | 0.16 (server, transport-io) | MCP protocol |
| `schemars` | 0.8 | JSON Schema generation |
| `axum` | 0.8 (json) | HTTP framework |
| `tower` | 0.5 | Middleware |
| `tower-http` | 0.6 (cors) | CORS middleware |
| `blake3` | 1 | Fast hashing |
| `num_cpus` | 1 | CPU core detection |
| `sysinfo` | 0.34 | System info (RAM, CPU) |
| `tracing` | 0.1 | Structured logging |
| `tracing-subscriber` | 0.3 (env-filter) | Log subscriber |
| `tempfile` | 3 | Temporary files |
| `hnsw_rs` | 0.3 | HNSW vector index |
| `reqwest` | 0.12 (json, stream, blocking) | HTTP client |
| `indicatif` | 0.17 | Progress bars |
| `anndists` | 0.1 | Distance functions (cosine) |
| `parking_lot` | 0.12 | Fast RwLock |

---

## FASE 5 — BUILD, RUN & TESTING

### Build

```bash
cargo build                          # Debug
cargo build --release                # Release (opt-level=3, LTO, strip)
cargo build --release -p aikd-cli    # Build hanya CLI binary
```

### Run

```bash
# Inisialisasi pertama kali
aikd init

# Scan project
aikd scan
aikd scan --path /path/to/project

# Search
aikd query "authentication" --hybrid --json
aikd query "REST API" --path "docs" --heading "Endpoints"

# Server
aikd daemon --foreground             # REST + MCP
aikd serve                           # MCP only

# Embedding
aikd embed --batch 64
```

### Testing

```bash
cargo test                           # Run semua 43 tests
cargo test -p aikd-core              # Test crate tertentu
cargo test -- --nocapture            # Dengan output
```

**43 Unit Tests:**

| Crate | Tests | Coverage |
|-------|-------|----------|
| aikd-core | 19 | Config (13) + Resource (6) |
| aikd-storage | 6 | Schema migrations, idempotent, tables |
| aikd-chunker | 5 | Plain text, structured, markdown, file type, unique IDs |
| aikd-embedder | 4 | Cosine similarity, vector search, RRF, f32 roundtrip |
| aikd-indexer | 5 | Tantivy open/search, VectorIndex, RRF, HybridSearcher |
| aikd-session | 5 | Create, get_or_create, remember/recall, list, stats |
| aikd-benchmark | 5 | Resource monitor, display, runner creation, test data, chunking |
| aikd-server | 0 | (integration tested) |
| aikd-watcher | 0 | (integration tested) |
| aikd-plugin | 0 | (constants only) |
| aikd-cli | 0 | (integration tested) |

---

## FASE 6 — KONFIGURASI

### Default Config (`~/.aikd/config.yaml`)

```yaml
version: "1.1.0"
scan:
  include_paths: ["."]
  exclude_paths: ["node_modules", ".git", "__pycache__", ".cache", "target", ".cargo", "dist", "build", ".next", ".venv"]
  include_extensions: ["md", "json", "yaml", "yml", "txt", "toml"]
  exclude_extensions: []
  include_files: []
  exclude_files: [".env", "*.bak", "*.tmp", "*.secret"]
  follow_symlinks: false
chunk:
  max_tokens: 1000
  min_tokens: 100
  overlap_tokens: 0
embedding:
  enabled: true
  model: "all-MiniLM-L6-v2"
  batch_size: "auto"
  device: "cpu"
  compute_threads: 4
index:
  db_path: "~/.aikd/aikd.db"
  tantivy_path: "~/.aikd/tantivy_index"
  cache_size_mb: 512
  model_path: "~/.aikd/model"
server:
  rest_port: 9090
  auth_token: null
  cors_origins: ["*"]
filter:
  filename_contains: []
  filename_exclude: []
  content_contains: []
  content_exclude: []
  max_file_size: 1048576
resource:
  mode: Auto
```

### Environment Variables

| Variable | Deskripsi |
|----------|-----------|
| `RUST_LOG` | Log level (default: `warn,aikd=info`) |

---

## FASE 7 — SECURITY & PERFORMANCE

### Security

| Aspek | Implementasi |
|-------|--------------|
| **Auth** | Bearer token di REST API (`config.server.auth_token`). Jika null → no auth |
| **Path Traversal** | `AikdError::PathTraversal` ada di error enum tapi TIDAK diimplementasikan di code |
| **SQL Injection** | Menggunakan parameterized queries (`rusqlite::params!`) — AMAN |
| **Secrets** | `.env`, `*.secret` di-exclude dari scanning |
| **MCP** | Tanpa auth — siapa yang bisa stdio connect bisa akses |
| **CORS** | Default `*` (allow all origins) |

### Performance

| Aspek | Detail |
|-------|--------|
| **Parallel Chunking** | rayon `par_iter()` untuk chunk files secara parallel |
| **Incremental Index** | blake3 hash check → skip file yang tidak berubah |
| **SQLite WAL** | `journal_mode=WAL` untuk concurrent read/write |
| **Batch Transaction** | Semua DB writes dalam 1 transaction |
| **Tantivy Writer** | 50MB heap per writer |
| **HNSW** | O(log n) search, tapi rebuild on-the-fly per query |
| **Resource Throttle** | Benchmark: CPU ≤50%, RAM ≤50% |
| **Token Estimation** | `len/4` (approximasi, bukan tokenizer sebenarnya) |

**Big-O Complexity:**

| Operasi | Complexity | Catatan |
|---------|------------|---------|
| Scan (file discovery) | O(n) | n = total files di directory tree |
| Chunking | O(n) | n = characters dalam file |
| BM25 Search | O(log n) | Tantivy inverted index |
| Vector Search (brute-force) | O(n×d) | n = embeddings, d = dimensions |
| Vector Search (HNSW) | O(log n) | Tapi build on-the-fly = O(n) |
| RRF Fusion | O(n log n) | Sort setelah merge |
| blake3 Hash | O(n) | n = file size bytes |

---

## FASE 8 — KNOWN ISSUES & TECHNICAL DEBT

```
ISSUE     : HNSW index dibangun ulang setiap query
LOKASI    : crates/indexer/src/lib.rs:49 (VectorIndex::search)
SEVERITY  : major
WORKAROUND: Untuk dataset kecil (<10k chunks) masih acceptable

ISSUE     : Dua implementasi RRF (redundansi)
LOKASI    : crates/indexer/src/lib.rs:275 DAN crates/embedder/src/lib.rs:121
SEVERITY  : minor
WORKAROUND: Keduanya identik, bisa di-extract ke aikd-core

ISSUE     : Load ALL embeddings ke memory setiap hybrid search
LOKASI    : crates/embedder/src/lib.rs:130 (load_all_embeddings)
SEVERITY  : major
WORKAROUND: Gunakan resource mode Low untuk disable embedding

ISSUE     : Buka database baru setiap request di REST/MCP server
LOKASI    : crates/server/src/rest.rs:87, crates/server/src/mcp.rs:88
SEVERITY  : medium
WORKAROUND: SQLite WAL mode memperbolehkan concurrent open

ISSUE     : Recall menggunakan simple keyword matching, bukan semantic
LOKASI    : crates/session/src/lib.rs:116
SEVERITY  : minor
WORKAROUND: Cukup untuk recall sederhana

ISSUE     : Scan logic diduplikasi di 3 tempat
LOKASI    : crates/cli/src/main.rs:239, crates/server/src/rest.rs:149, crates/server/src/mcp.rs:86
SEVERITY  : medium
WORKAROUND: Refactor ke shared function

ISSUE     : Tidak ada path traversal validation
LOKASI    : crates/core/src/error.rs:24 (PathTraversal error defined but unused)
SEVERITY  : medium
WORKAROUND: User responsibility

ISSUE     : cmd_daemon --foreground tidak background properly
LOKASI    : crates/cli/src/main.rs:234 (recursive call to foreground)
SEVERITY  : minor
WORKAROUND: Gunakan --foreground flag

ISSUE     : Version inconsistency (CLI shows 1.0.0, workspace shows 1.1.0)
LOKASI    : crates/cli/src/main.rs:17, crates/server/src/rest.rs:137
SEVERITY  : minor
WORKAROUND: Update version strings
```

---

## FASE 9 — GLOSSARY DOMAIN

```
TERM      : BM25
DEFINISI  : Best Matching 25 — algoritma full-text search berbasis TF-IDF yang digunakan Tantivy
CONTOH    : tantivy.search("query", limit, &filters)

TERM      : RRF (Reciprocal Rank Fusion)
DEFINISI  : Algoritma untuk menggabungkan ranking dari multiple search systems
CONTOH    : score += 1.0 / (k as f32 + rank as f32 + 1.0), k=60

TERM      : HNSW (Hierarchical Navigable Small World)
DEFINISI  : Algoritma approximate nearest neighbor (ANN) untuk vector search
CONTOH    : Hnsw::new(HNSW_M, capacity, nb_layers, HNSW_EF_CONSTRUCTION, DistCosine)

TERM      : Chunk
DEFINISI  : Potongan file yang di-index, punya heading hierarchy dan line range
CONTOH    : Chunk { id: "uuid", file_path: "/src/main.rs", heading_hierarchy: ["API", "REST"], ... }

TERM      : fastembed-rs
DEFINISI  : Rust library untuk ONNX inference (embedding generation) tanpa Python
CONTOH    : TextEmbedding::try_new_from_user_defined(user_model, InitOptions::default())

TERM      : MCP (Model Context Protocol)
DEFINISI  : Protocol standar untuk AI assistant berkomunikasi dengan tools
CONTOH    : rmcp server dengan tool "query", "scan", "stats"

TERM      : blake3
DEFINISI  : Cryptographic hash function yang sangat cepat, digunakan untuk incremental detection
CONTOH    : compute_blake3(path) → hash string, compare dengan DB

TERM      : ResourceProfile
DEFINISI  : Profil resource system yang menentukan batch size, parallelism, dan HNSW parameters
CONTOH    : ResourceProfile::detect_with_mode(&ResourceMode::Auto)

TERM      : Smart Config
DEFINISI  : Auto-detection project type berdasarkan file marker (Cargo.toml, package.json, dll)
CONTOH    : generate_smart_config(&root) → Config dengan extensions yang sesuai

TERM      : Shell Hook
DEFINISI  : Script yang di-append ke .bashrc/.zshrc untuk auto-start daemon
CONTOH    : cd() override → aikd_auto_start()

TERM      : Inject
DEFINISI  : CLI wrapper yang menyisipkan session context ke stdin command lain
CONTOH    : aikd inject -- aider --file main.rs
```

---

## APPENDIX A — FILE INDEX

| # | File | Baris | Deskripsi |
|---|------|-------|-----------|
| 1 | `Cargo.toml` | 55 | Workspace root, 11 members, 33 workspace dependencies |
| 2 | `crates/core/src/lib.rs` | 9 | Re-export modul core |
| 3 | `crates/core/src/types.rs` | 73 | Chunk, SearchResult, SearchFilters, Session, Conversation |
| 4 | `crates/core/src/error.rs` | 36 | AikdError enum (10 variants) |
| 5 | `crates/core/src/config.rs` | 406 | Config, 8 sub-configs, smart config, 13 tests |
| 6 | `crates/core/src/resource.rs` | 196 | ResourceProfile, 5 modes, auto-detection, 6 tests |
| 7 | `crates/core/Cargo.toml` | 22 | Dependencies: serde, rusqlite, num_cpus, sysinfo |
| 8 | `crates/storage/src/lib.rs` | 52 | Database, Transaction, compute_blake3 |
| 9 | `crates/storage/src/schema.rs` | 243 | 3 migrations, 7 tables, 6 tests |
| 10 | `crates/storage/Cargo.toml` | 18 | Dependencies: aikd-core, rusqlite, blake3 |
| 11 | `crates/indexer/src/lib.rs` | 354 | TantivyEngine, VectorIndex, HybridSearcher, RRF, 5 tests |
| 12 | `crates/indexer/Cargo.toml` | 17 | Dependencies: tantivy, hnsw_rs, anndists, parking_lot |
| 13 | `crates/embedder/src/lib.rs` | 257 | fastembed model, embedding, cosine similarity, RRF, 4 tests |
| 14 | `crates/embedder/Cargo.toml` | 17 | Dependencies: fastembed, reqwest (blocking) |
| 15 | `crates/chunker/src/lib.rs` | 142 | chunk_file dispatch, plain text chunker, structured chunker, 5 tests |
| 16 | `crates/chunker/src/markdown.rs` | 200 | Markdown chunker, frontmatter, heading hierarchy |
| 17 | `crates/chunker/Cargo.toml` | 12 | Dependencies: pulldown-cmark, uuid |
| 18 | `crates/session/src/lib.rs` | 219 | Session CRUD, remember/recall, embed conversations, 5 tests |
| 19 | `crates/session/Cargo.toml` | 16 | Dependencies: aikd-core, aikd-storage, aikd-embedder |
| 20 | `crates/server/src/lib.rs` | 33 | AppState, run_mcp_server, run_rest_server |
| 21 | `crates/server/src/mcp.rs` | 404 | MCP server, 7 tools, format_results |
| 22 | `crates/server/src/rest.rs` | 316 | REST API, 5 endpoints, auth, CORS |
| 23 | `crates/server/Cargo.toml` | 28 | Dependencies: axum, tower-http, rmcp, schemars |
| 24 | `crates/watcher/src/lib.rs` | 156 | File watcher, debounce, incremental indexing |
| 25 | `crates/watcher/Cargo.toml` | 18 | Dependencies: notify |
| 26 | `crates/plugin/src/lib.rs` | 43 | SDK constants, DTOs |
| 27 | `crates/plugin/Cargo.toml` | 9 | Dependencies: serde |
| 28 | `crates/benchmark/src/lib.rs` | 820 | BenchmarkRunner, 8 scenarios, ResourceMonitor, 5 tests |
| 29 | `crates/benchmark/src/bin/main.rs` | 106 | Benchmark binary entry point |
| 30 | `crates/benchmark/Cargo.toml` | 35 | Dependencies: reqwest, rayon, tempfile |
| 31 | `crates/cli/src/main.rs` | 678 | CLI binary, 15 commands, shell hooks |
| 32 | `crates/cli/Cargo.toml` | 35 | Dependencies: semua crate lain |
| 33 | `extensions/vscode/extension.js` | 160 | VSCode extension, 4 commands |
| 34 | `extensions/vscode/package.json` | 26 | VSCode extension manifest |
| 35 | `BENCHMARK_REPORT.md` | 192 | Benchmark results (8/8 pass) |
| 36 | `REVERSE_ENGINEERING.md` | 1273 | Dokumentasi reverse engineering sebelumnya (v3.0) |
| 37 | `PROJECT_CHRONOLOGY.md` | 174 | Timeline perkembangan project |

**Total: 37 file, ~5,200 baris kode Rust, 43 unit tests**

---

## APPENDIX B — FUNCTION INDEX

| Fungsi | Lokasi | Baris |
|--------|--------|-------|
| `AikdServer::embed()` | crates/server/src/mcp.rs | 244 |
| `AikdServer::new()` | crates/server/src/mcp.rs | 77 |
| `AikdServer::query()` | crates/server/src/mcp.rs | 166 |
| `AikdServer::recall()` | crates/server/src/mcp.rs | 285 |
| `AikdServer::remember()` | crates/server/src/mcp.rs | 263 |
| `AikdServer::scan()` | crates/server/src/mcp.rs | 86 |
| `AikdServer::stats()` | crates/server/src/mcp.rs | 225 |
| `AikdServer::status()` | crates/server/src/mcp.rs | 317 |
| `BenchmarkRunner::bench_chunking_throughput()` | crates/benchmark/src/lib.rs | 553 |
| `BenchmarkRunner::bench_concurrent_search()` | crates/benchmark/src/lib.rs | 521 |
| `BenchmarkRunner::bench_embedding()` | crates/benchmark/src/lib.rs | 603 |
| `BenchmarkRunner::bench_incremental_reindex()` | crates/benchmark/src/lib.rs | 430 |
| `BenchmarkRunner::bench_indexing()` | crates/benchmark/src/lib.rs | 224 |
| `BenchmarkRunner::bench_rest_stress()` | crates/benchmark/src/lib.rs | 674 |
| `BenchmarkRunner::bench_search_bm25()` | crates/benchmark/src/lib.rs | 306 |
| `BenchmarkRunner::bench_search_hybrid()` | crates/benchmark/src/lib.rs | 359 |
| `BenchmarkRunner::new()` | crates/benchmark/src/lib.rs | 124 |
| `BenchmarkRunner::prepare_test_data()` | crates/benchmark/src/lib.rs | 181 |
| `BenchmarkRunner::resource_status()` | crates/benchmark/src/lib.rs | 590 |
| `BenchmarkRunner::run_all()` | crates/benchmark/src/lib.rs | 153 |
| `BenchmarkRunner::stop()` | crates/benchmark/src/lib.rs | 599 |
| `Chunk::heading_hierarchy_str()` | crates/core/src/types.rs | 18 |
| `Chunk::token_estimate()` | crates/core/src/types.rs | 22 |
| `Config::check_file_size()` | crates/core/src/config.rs | 291 |
| `Config::db_path()` | crates/core/src/config.rs | 241 |
| `Config::default()` | crates/core/src/config.rs | 208 |
| `Config::load()` | crates/core/src/config.rs | 224 |
| `Config::matches_content_filter()` | crates/core/src/config.rs | 279 |
| `Config::matches_filename_filter()` | crates/core/src/config.rs | 267 |
| `Config::max_chunk_tokens()` | crates/core/src/config.rs | 295 |
| `Config::min_chunk_tokens()` | crates/core/src/config.rs | 299 |
| `Config::model_path()` | crates/core/src/config.rs | 249 |
| `Config::save()` | crates/core/src/config.rs | 231 |
| `Config::should_exclude_dir()` | crates/core/src/config.rs | 253 |
| `Config::should_exclude_file()` | crates/core/src/config.rs | 257 |
| `Config::tantivy_path()` | crates/core/src/config.rs | 245 |
| `Database::begin_transaction()` | crates/storage/src/lib.rs | 27 |
| `Database::conn()` | crates/storage/src/lib.rs | 23 |
| `Database::open()` | crates/storage/src/lib.rs | 12 |
| `HybridSearcher::hybrid_search()` | crates/indexer/src/lib.rs | 236 |
| `HybridSearcher::new()` | crates/indexer/src/lib.rs | 224 |
| `ResourceProfile::detect()` | crates/core/src/resource.rs | 32 |
| `ResourceProfile::detect_with_mode()` | crates/core/src/resource.rs | 40 |
| `ResourceProfile::from_specs()` | crates/core/src/resource.rs | 80 |
| `ResourceMonitor::check()` | crates/benchmark/src/lib.rs | 67 |
| `ResourceMonitor::new()` | crates/benchmark/src/lib.rs | 55 |
| `ResourceMonitor::throttle_if_needed()` | crates/benchmark/src/lib.rs | 84 |
| `TantivyEngine::clear()` | crates/indexer/src/lib.rs | 131 |
| `TantivyEngine::index_chunks()` | crates/indexer/src/lib.rs | 114 |
| `TantivyEngine::open()` | crates/indexer/src/lib.rs | 82 |
| `TantivyEngine::search()` | crates/indexer/src/lib.rs | 138 |
| `Transaction::commit()` | crates/storage/src/lib.rs | 42 |
| `Transaction::conn()` | crates/storage/src/lib.rs | 38 |
| `VectorIndex::insert()` | crates/indexer/src/lib.rs | 33 |
| `VectorIndex::is_empty()` | crates/indexer/src/lib.rs | 66 |
| `VectorIndex::len()` | crates/indexer/src/lib.rs | 62 |
| `VectorIndex::new()` | crates/indexer/src/lib.rs | 25 |
| `VectorIndex::search()` | crates/indexer/src/lib.rs | 40 |
| `bytes_to_f32()` | crates/embedder/src/lib.rs | 207 |
| `chunk_file()` | crates/chunker/src/lib.rs | 6 |
| `chunk_markdown()` | crates/chunker/src/markdown.rs | 6 |
| `chunk_plain_text()` | crates/chunker/src/lib.rs | 35 |
| `chunk_structured()` | crates/chunker/src/lib.rs | 84 |
| `cmd_benchmark()` | crates/cli/src/main.rs | 610 |
| `cmd_daemon()` | crates/cli/src/main.rs | 211 |
| `cmd_embed()` | crates/cli/src/main.rs | 416 |
| `cmd_export()` | crates/cli/src/main.rs | 372 |
| `cmd_import()` | crates/cli/src/main.rs | 381 |
| `cmd_init()` | crates/cli/src/main.rs | 121 |
| `cmd_inject()` | crates/cli/src/main.rs | 556 |
| `cmd_query()` | crates/cli/src/main.rs | 328 |
| `cmd_recall()` | crates/cli/src/main.rs | 453 |
| `cmd_remember()` | crates/cli/src/main.rs | 441 |
| `cmd_scan()` | crates/cli/src/main.rs | 239 |
| `cmd_serve()` | crates/cli/src/main.rs | 433 |
| `cmd_stats()` | crates/cli/src/main.rs | 390 |
| `cmd_status()` | crates/cli/src/main.rs | 473 |
| `cmd_watch()` | crates/cli/src/main.rs | 437 |
| `compute_blake3()` | crates/storage/src/lib.rs | 48 |
| `cosine_similarity()` | crates/embedder/src/lib.rs | 100 |
| `create_model()` | crates/embedder/src/lib.rs | 46 |
| `create_session()` | crates/session/src/lib.rs | 6 |
| `detect_file_type()` | crates/chunker/src/lib.rs | 25 |
| `download_model()` | crates/embedder/src/lib.rs | 18 |
| `embed_and_store()` | crates/embedder/src/lib.rs | 63 |
| `embed_and_store_with_profile()` | crates/embedder/src/lib.rs | 92 |
| `embed_conversations()` | crates/session/src/lib.rs | 123 |
| `enrich_lines()` | crates/server/src/mcp.rs | 350 |
| `enrich_with_line_numbers()` | crates/cli/src/main.rs | 509 |
| `export_chunks_for_embedding()` | crates/embedder/src/lib.rs | 184 |
| `extract_frontmatter()` | crates/chunker/src/markdown.rs | 173 |
| `f32_to_bytes()` | crates/embedder/src/lib.rs | 201 |
| `format_results()` | crates/server/src/mcp.rs | 372 |
| `generate_smart_config()` | crates/core/src/config.rs | 304 |
| `get_or_create_session()` | crates/session/src/lib.rs | 22 |
| `get_session_stats()` | crates/session/src/lib.rs | 151 |
| `handle_query()` | crates/server/src/rest.rs | 77 |
| `handle_recall()` | crates/server/src/rest.rs | 244 |
| `handle_remember()` | crates/server/src/rest.rs | 219 |
| `handle_scan()` | crates/server/src/rest.rs | 149 |
| `handle_stats()` | crates/server/src/rest.rs | 120 |
| `heading_level_num()` | crates/chunker/src/markdown.rs | 134 |
| `import_embeddings_json()` | crates/embedder/src/lib.rs | 161 |
| `install_shell_hook()` | crates/cli/src/main.rs | 151 |
| `is_model_downloaded()` | crates/embedder/src/lib.rs | 42 |
| `list_sessions()` | crates/session/src/lib.rs | 46 |
| `load_all_embeddings()` | crates/embedder/src/lib.rs | 130 |
| `load_chunks()` | crates/cli/src/main.rs | 488 |
| `load_chunks_from_db()` | crates/server/src/mcp.rs | 334 |
| `load_chunks_rest()` | crates/server/src/rest.rs | 270 |
| `load_or_default()` | crates/cli/src/main.rs | 552 |
| `main()` | crates/cli/src/main.rs | 92 |
| `main()` | crates/benchmark/src/bin/main.rs | 25 |
| `make_chunk()` | crates/chunker/src/markdown.rs | 145 |
| `print_results()` | crates/cli/src/main.rs | 531 |
| `recall()` | crates/session/src/lib.rs | 91 |
| `reciprocal_rank_fusion()` | crates/embedder/src/lib.rs | 121 |
| `reciprocal_rank_fusion()` | crates/indexer/src/lib.rs | 275 |
| `remember()` | crates/session/src/lib.rs | 60 |
| `run_migrations()` | crates/storage/src/schema.rs | 6 |
| `run_mcp_server()` | crates/server/src/lib.rs | 24 |
| `run_rest_server()` | crates/server/src/rest.rs | 286 |
| `run_server()` | crates/server/src/mcp.rs | 399 |
| `run_watcher()` | crates/watcher/src/lib.rs | 9 |
| `start_resource_monitor()` | crates/benchmark/src/lib.rs | 746 |
| `store_embeddings()` | crates/embedder/src/lib.rs | 143 |
| `strip_frontmatter()` | crates/chunker/src/markdown.rs | 192 |
| `vector_search()` | crates/embedder/src/lib.rs | 112 |

---

## APPENDIX C — CHANGELOG & VERSION HISTORY

### v1.0 → v1.1 (14 Juni 2026)

| Perubahan | Detail |
|-----------|--------|
| HNSW vector search | `hnsw_rs` crate, brute-force → O(log n) ANN |
| Benchmark suite | 8 scenarios, resource monitor, auto-throttle |
| Shell hooks | `aikd init` → bash/zsh auto-start |
| VSCode extension | Search, scan, stats commands |
| `aikd inject` | Context injection wrapper untuk agent CLIs |
| Auto-download model | `download_model()` dari HuggingFace |
| MCP tools | +3 tools: remember, recall, status |
| REST API | +2 endpoints: remember, recall |
| Session memory | Migration v3: sessions, conversations, conversation_embeddings |
| Version bump | 1.0.0 → 1.1.0 di semua crate |
| Tests | 38 → 43 unit tests |

### Project Evolution (v1 → v2 → v3 → v1.1)

| Versi | Teknologi | Fitur Utama |
|-------|-----------|-------------|
| v1 | Rust, SQLite FTS5 | Basic indexing + search |
| v2 | Rust, Custom KHDB, mmap | YAML config, zstd compression |
| v3 | Rust, SQLite + Tantivy + fastembed-rs | Hybrid search, MCP server, file watcher |
| v1.1 | +HNSW, +Benchmark, +VSCode | ANN search, stress test, IDE integration |

---

_Dokumentasi ini dibuat berdasarkan pembacaan langsung seluruh 37 file source code (5,200+ baris) tanpa asumsi._
