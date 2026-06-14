# Knowledge Hub v3 — Reverse Engineering Documentation

_Dibuat: 14 Juni 2026_

---

## Daftar Isi

1. [Ringkasan Project](#1-ringkasan-project)
2. [Arsitektur Sistem](#2-arsitektur-sistem)
3. [Analisis Modul](#3-analisis-modul)
4. [Alur Proses (Flowcharts)](#4-alur-proses-flowcharts)
5. [Database Schema](#5-database-schema)
6. [Konfigurasi](#6-konfigurasi)
7. [Build, Run & Testing](#7-build-run--testing)
8. [Kelemahan & Area Optimasi](#8-kelemahan--area-optimasi)
9. [Kesimpulan & Rekomendasi](#9-kesimpulan--rekomendasi)

---

## 1. Ringkasan Project

**Knowledge Hub v3** adalah CLI tool dan MCP server untuk indexing, chunking, dan hybrid search pada file-file project (markdown, json, yaml, txt, dll).

### Fitur Utama

| Fitur | Deskripsi |
|-------|-----------|
| **Scan & Index** | Walk directory, chunk files, simpan ke SQLite + Tantivy |
| **BM25 Search** | Full-text search menggunakan Tantivy |
| **Hybrid Search** | BM25 + Vector embeddings dengan Reciprocal Rank Fusion (RRF) |
| **Embeddings** | Generate vector embeddings menggunakan fastembed-rs (native Rust) |
| **MCP Server** | Model Context Protocol server untuk integrasi dengan AI assistants |
| **File Watcher** | Auto-sync saat file berubah (debounced) |
| **Smart Config** | Auto-detect project type (Rust, Node, Python, Go, Java, C++) |

### Tech Stack

| Komponen | Teknologi |
|----------|-----------|
| Bahasa | Rust (edition 2021) |
| CLI Parser | clap 4 |
| Database | SQLite (rusqlite 0.40) |
| Search Engine | Tantivy 0.22 |
| Embeddings | fastembed 5 (ONNX Runtime) |
| MCP Server | rmcp 0.16 |
| File Watcher | notify 6 |
| Markdown Parser | pulldown-cmark 0.11 |
| Async Runtime | tokio |
| Parallelism | rayon |

---

## 2. Arsitektur Sistem

### Diagram Komponen Utama

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Knowledge Hub v3                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐      │
│  │   CLI    │    │  MCP     │    │  File    │    │  embed   │      │
│  │  (clap)  │    │ Server   │    │ Watcher  │    │  .py     │      │
│  └────┬─────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘      │
│       │               │               │               │            │
│       ▼               ▼               ▼               ▼            │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    Core Logic Layer                          │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │   │
│  │  │ Config   │  │ Chunker  │  │  Search  │  │  Vector  │   │   │
│  │  │  (YAML)  │  │ (MD/TXT) │  │ (Tantivy)│  │(fastembed)│   │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │   │
│  └─────────────────────────────────────────────────────────────┘   │
│       │               │               │               │            │
│       ▼               ▼               ▼               ▼            │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │  YAML    │  │  SQLite  │  │ Tantivy  │  │  ONNX    │          │
│  │  Config  │  │  Database│  │  Index   │  │  Model   │          │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Aliran Data (Data Flow)

```
┌─────────────────────────────────────────────────────────────────────┐
│                        SCAN & INDEX FLOW                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  File System                                                        │
│      │                                                              │
│      ▼                                                              │
│  WalkDir (recursive)                                                │
│      │                                                              │
│      ▼                                                              │
│  Filter (extensions, exclude_dirs, file_size)                       │
│      │                                                              │
│      ▼                                                              │
│  Read File Content                                                  │
│      │                                                              │
│      ▼                                                              │
│  ┌─────────────────────────────────────────┐                       │
│  │           Chunker Module                │                       │
│  │  ┌─────────────┐  ┌─────────────┐      │                       │
│  │  │  Markdown   │  │ Plain Text  │      │                       │
│  │  │  (heading)  │  │ (by tokens) │      │                       │
│  │  └─────────────┘  └─────────────┘      │                       │
│  │  ┌─────────────┐                       │                       │
│  │  │ Structured  │ (JSON/YAML/TOML)      │                       │
│  │  └─────────────┘                       │                       │
│  └─────────────────────────────────────────┘                       │
│      │                                                              │
│      ▼                                                              │
│  ┌─────────────────────────────────────────┐                       │
│  │         Storage Layer                   │                       │
│  │  ┌─────────────┐  ┌─────────────┐      │                       │
│  │  │   SQLite    │  │   Tantivy   │      │                       │
│  │  │ (metadata)  │  │ (full-text) │      │                       │
│  │  └─────────────┘  └─────────────┘      │                       │
│  └─────────────────────────────────────────┘                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                      HYBRID SEARCH FLOW                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  User Query                                                         │
│      │                                                              │
│      ├──────────────────┐                                          │
│      ▼                  ▼                                          │
│  ┌──────────┐    ┌──────────┐                                      │
│  │  BM25    │    │  Vector  │                                      │
│  │ (Tantivy)│    │ (cosine) │                                      │
│  └────┬─────┘    └────┬─────┘                                      │
│       │               │                                            │
│       ▼               ▼                                            │
│  ┌─────────────────────────────┐                                   │
│  │  Reciprocal Rank Fusion     │                                   │
│  │  (RRF, k=60)                │                                   │
│  └──────────────┬──────────────┘                                   │
│                 │                                                   │
│                 ▼                                                   │
│  ┌─────────────────────────────┐                                   │
│  │  Load from SQLite           │                                   │
│  │  (enrich with metadata)     │                                   │
│  └──────────────┬──────────────┘                                   │
│                 │                                                   │
│                 ▼                                                   │
│  Search Results (JSON / formatted text)                             │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                      MCP SERVER FLOW                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  AI Assistant (mimo, Claude, etc)                                   │
│      │                                                              │
│      ▼                                                              │
│  MCP Client (stdio transport)                                       │
│      │                                                              │
│      ▼                                                              │
│  ┌─────────────────────────────┐                                   │
│  │  KnowledgeHubServer         │                                   │
│  │  (rmcp ServerHandler)       │                                   │
│  │  ┌───────────────────────┐  │                                   │
│  │  │ Tools:                │  │                                   │
│  │  │  - scan               │  │                                   │
│  │  │  - query              │  │                                   │
│  │  │  - stats              │  │                                   │
│  │  │  - embed              │  │                                   │
│  │  └───────────────────────┘  │                                   │
│  └──────────────┬──────────────┘                                   │
│                 │                                                   │
│                 ▼                                                   │
│  Core Logic (same as CLI)                                           │
│                 │                                                   │
│                 ▼                                                   │
│  Response (formatted text)                                          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                    FILE WATCHER FLOW                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  File System Event                                                  │
│      │                                                              │
│      ▼                                                              │
│  notify::Watcher (inotify/kqueue)                                   │
│      │                                                              │
│      ▼                                                              │
│  Debounce (500ms default)                                           │
│      │                                                              │
│      ▼                                                              │
│  ┌─────────────────────────────┐                                   │
│  │  Event Handler              │                                   │
│  │  ┌─────────┐ ┌─────────┐   │                                   │
│  │  │ Create  │ │ Modify  │   │                                   │
│  │  └────┬────┘ └────┬────┘   │                                   │
│  │       │           │        │                                   │
│  │       ▼           ▼        │                                   │
│  │  Re-chunk + Re-index       │                                   │
│  │  ┌─────────┐               │                                   │
│  │  │ Remove  │               │                                   │
│  │  └────┬────┘               │                                   │
│  │       │                    │                                   │
│  │       ▼                    │                                   │
│  │  Delete from DB            │                                   │
│  └─────────────────────────────┘                                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. Analisis Modul

### 3.1 `main.rs` — Entry Point & CLI

**Lokasi:** `src/main.rs` (498 baris)

**Fungsi Utama:**

| Fungsi | Deskripsi |
|--------|-----------|
| `main()` | Entry point, parse CLI args, dispatch ke command handler |
| `cmd_init()` | Buat config default, auto-detect project type |
| `cmd_scan()` | Scan files, chunk, index ke SQLite + Tantivy |
| `cmd_query()` | BM25 atau hybrid search |
| `cmd_stats()` | Tampilkan statistik index |
| `cmd_export()` | Export chunks ke JSON |
| `cmd_import()` | Import embeddings dari JSON |
| `cmd_embed()` | Generate embeddings (native Rust) |
| `cmd_serve()` | Start MCP server |
| `cmd_watch()` | Start file watcher daemon |
| `load_or_default()` | Load config atau pakai default |
| `load_chunks()` | Load chunks dari SQLite berdasarkan ID |
| `enrich_with_line_numbers()` | Tambah info line number dari DB |
| `print_results()` | Format output search results |

**CLI Commands:**

```bash
knowledge-hub init [--path <PATH>]           # Buat config
knowledge-hub scan [--path <PATH>]           # Scan & index
knowledge-hub query <QUERY> [OPTIONS]        # Search
    --limit <N>                              # Max results (default: 10)
    --path <PATH>                            # Filter by path
    --heading <TEXT>                         # Filter by heading
    --json                                   # Output JSON
    --hybrid                                 # Hybrid search
knowledge-hub stats                          # Statistik
knowledge-hub export [--output <FILE>]       # Export chunks
knowledge-hub import --file <FILE>           # Import embeddings
knowledge-hub embed [--model <MODEL>] [--batch <N>]  # Generate embeddings
knowledge-hub serve                          # Start MCP server
knowledge-hub watch [--debounce <MS>]        # Start watcher
```

**Dependensi Eksternal:**
- `clap` — CLI parser
- `walkdir` — Recursive directory traversal
- `rayon` — Parallel processing
- `notify` — File system events
- `shellexpand` — Expand `~` di path
- `chrono` — Timestamp
- `serde_json` — JSON serialization

---

### 3.2 `config/mod.rs` — Konfigurasi

**Lokasi:** `src/config/mod.rs` (452 baris)

**Struct Utama:**

```rust
pub struct Config {
    pub version: String,           // "3.0.0"
    pub scan: ScanConfig,          // Scan settings
    pub index: IndexConfig,        // Index settings
    pub filter: FilterConfig,      // Filter settings
}

pub struct ScanConfig {
    pub paths: Vec<String>,        // Paths to scan
    pub extensions: Vec<String>,   // File extensions to index
    pub exclude_dirs: Vec<String>, // Directories to exclude
    pub exclude_files: Vec<String>,// Files to exclude
}

pub struct IndexConfig {
    pub db_path: String,           // SQLite database path
    pub tantivy_path: String,      // Tantivy index path
    pub max_chunk_tokens: usize,   // Max tokens per chunk
    pub min_chunk_tokens: usize,   // Min tokens per chunk
    pub model_path: String,        // ONNX model path
}

pub struct FilterConfig {
    pub filename_contains: Vec<String>,  // Filename must contain
    pub filename_exclude: Vec<String>,   // Filename must not contain
    pub content_contains: Vec<String>,   // Content must contain
    pub content_exclude: Vec<String>,    // Content must not contain
    pub max_file_size: u64,              // Max file size in bytes
}
```

**Fungsi Kunci:**

| Fungsi | Deskripsi |
|--------|-----------|
| `Config::load()` | Load dari YAML file |
| `Config::save()` | Simpan ke YAML file |
| `Config::db_path()` | Expand tilde, return PathBuf |
| `Config::tantivy_path()` | Expand tilde, return PathBuf |
| `Config::model_path()` | Expand tilde, return PathBuf |
| `Config::should_exclude_dir()` | Cek apakah directory harus di-exclude |
| `Config::should_exclude_file()` | Cek apakah file harus di-exclude |
| `Config::matches_filename_filter()` | Cek filename filter |
| `Config::matches_content_filter()` | Cek content filter |
| `Config::check_file_size()` | Cek file size limit |
| `generate_smart_config()` | Auto-detect project type, buat config |

**Smart Config Detection:**

| Project Type | Detection | Extensions |
|--------------|-----------|------------|
| Rust | `Cargo.toml` | rs, md, toml, yaml, yml, txt |
| Node.js/TS | `package.json` / `tsconfig.json` | ts, tsx, js, jsx, md, json, yaml, yml |
| Python | `pyproject.toml` / `requirements.txt` / `setup.py` | py, md, yaml, yml, json, txt, toml, cfg, ini |
| Go | `go.mod` | go, md, yaml, yml, json, txt, toml |
| Java | `pom.xml` / `build.gradle` | java, kt, md, xml, yaml, yml, json, properties |
| C/C++ | `CMakeLists.txt` / `Makefile` | c, cpp, h, hpp, md, txt, cmake, yaml, yml |

**Default Values:**

| Parameter | Default |
|-----------|---------|
| `db_path` | `~/.knowledge-hub/v3.db` |
| `tantivy_path` | `~/.knowledge-hub/tantivy` |
| `max_chunk_tokens` | 1000 |
| `min_chunk_tokens` | 50 |
| `model_path` | `~/.knowledge-hub/model` |
| `max_file_size` | 1,000,000 bytes (1MB) |

---

### 3.3 `db/mod.rs` — Database Layer

**Lokasi:** `src/db/mod.rs` (46 baris)

**Struct:**

```rust
pub struct Database {
    conn: Connection,  // SQLite connection
}

pub struct Transaction<'a> {
    tx: rusqlite::Transaction<'a>,
}
```

**Fungsi:**

| Fungsi | Deskripsi |
|--------|-----------|
| `Database::open()` | Buka/buat database, jalankan migrations |
| `Database::conn()` | Dapatkan reference ke Connection |
| `Database::begin_transaction()` | Mulai transaction |
| `Transaction::conn()` | Dapatkan reference ke Connection dalam transaction |
| `Transaction::commit()` | Commit transaction |

**SQLite PRAGMAs:**

```sql
PRAGMA journal_mode=WAL;    -- Write-Ahead Logging untuk concurrency
PRAGMA synchronous=NORMAL;  -- Balance antara durability dan performance
PRAGMA foreign_keys=ON;     -- Enable foreign key constraints
```

---

### 3.4 `db/schema.rs` — Database Schema & Migrations

**Lokasi:** `src/db/schema.rs` (218 baris)

**Schema Version:** 2

**Migration System:**

```rust
const SCHEMA_VERSION: i32 = 2;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Cek current version
    // Jalankan migration jika needed
    // Update version
}
```

**Tables:**

#### `schema_version`
```sql
CREATE TABLE schema_version (
    version INTEGER NOT NULL
);
```

#### `files` (Migration v1)
```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    size INTEGER NOT NULL,
    modified_at TEXT NOT NULL,
    last_scanned TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX idx_files_path ON files(path);
CREATE INDEX idx_files_status ON files(status);
```

#### `chunks` (Migration v1)
```sql
CREATE TABLE chunks (
    id TEXT PRIMARY KEY,           -- UUID
    file_id INTEGER NOT NULL REFERENCES files(id),
    chunk_index INTEGER NOT NULL,
    heading_hierarchy TEXT NOT NULL, -- JSON array
    heading_level INTEGER NOT NULL,
    heading_text TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    content TEXT NOT NULL,
    metadata_json TEXT,             -- JSON object
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_chunks_file_id ON chunks(file_id);
CREATE INDEX idx_chunks_heading ON chunks(heading_text);
```

#### `embeddings` (Migration v2)
```sql
CREATE TABLE embeddings (
    chunk_id TEXT PRIMARY KEY REFERENCES chunks(id),
    model TEXT NOT NULL,           -- "all-MiniLM-L6-v2"
    dimensions INTEGER NOT NULL,   -- 384
    vector BLOB NOT NULL           -- f32 array as bytes
);
```

---

### 3.5 `chunker/mod.rs` — Chunking Engine

**Lokasi:** `src/chunker/mod.rs` (233 baris)

**Struct:**

```rust
pub struct Chunk {
    pub id: String,                    // UUID
    pub file_path: String,             // File path
    pub chunk_index: usize,            // Index dalam file
    pub heading_hierarchy: Vec<String>, // ["Title", "Section", "Subsection"]
    pub heading_level: usize,          // Heading level (1-6)
    pub heading_text: String,          // Current heading text
    pub line_start: usize,             // Start line number
    pub line_end: usize,               // End line number
    pub content: String,               // Chunk content
    pub metadata: HashMap<String, Value>, // YAML frontmatter
}
```

**Fungsi:**

| Fungsi | Deskripsi |
|--------|-----------|
| `chunk_file()` | Dispatch ke chunker berdasarkan file type |
| `detect_file_type()` | Deteksi file type dari extension |
| `chunk_plain_text()` | Chunk text file berdasarkan token count |
| `chunk_structured()` | Chunk JSON/YAML/TOML (1 chunk per file) |
| `Chunk::heading_hierarchy_str()` | Join heading hierarchy dengan " > " |
| `Chunk::token_estimate()` | Estimasi token (len / 4) |

**File Type Detection:**

| Type | Extensions |
|------|------------|
| Markdown | `.md`, `.markdown` |
| Structured | `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml` |
| Text | Semua lainnya |

---

### 3.6 `chunker/markdown.rs` — Markdown Chunker

**Lokasi:** `src/chunker/markdown.rs` (201 baris)

**Fungsi Utama:**

```rust
pub fn chunk_markdown(
    file_path: &str,
    content: &str,
    max_tokens: usize,
    min_tokens: usize,
) -> Vec<Chunk>
```

**Alur:**

1. Extract YAML frontmatter
2. Strip frontmatter dari content
3. Parse markdown menggunakan pulldown-cmark
4. Track heading hierarchy
5. Chunk berdasarkan heading dan token count
6. Merge small chunks ke chunk terakhir

**Fungsi Helper:**

| Fungsi | Deskripsi |
|--------|-----------|
| `heading_level_num()` | Convert HeadingLevel ke usize |
| `make_chunk()` | Buat Chunk struct |
| `extract_frontmatter()` | Parse YAML frontmatter |
| `strip_frontmatter()` | Hapus frontmatter dari content |

**Markdown Events yang Ditangani:**

- `Event::Start(Tag::Heading)` — Mulai heading baru
- `Event::End(TagEnd::Heading)` — Selesai heading
- `Event::Text` — Teks content
- `Event::Code` — Inline code
- `Event::Start(Tag::Item)` — List item
- `Event::SoftBreak` / `Event::HardBreak` — Line break
- `Event::Start(Tag::Paragraph)` / `Event::End(TagEnd::Paragraph)` — Paragraph

---

### 3.7 `search/mod.rs` — Search Types

**Lokasi:** `src/search/mod.rs` (78 baris)

**Struct:**

```rust
pub struct SearchResult {
    pub chunk_id: String,          // UUID
    pub file_path: String,         // File path
    pub heading_hierarchy: String, // "Title > Section"
    pub heading_text: String,      // Current heading
    pub content: String,           // Chunk content
    pub line_start: usize,         // Start line
    pub line_end: usize,           // End line
    pub score: f32,                // Search score
}

pub struct SearchFilters {
    pub path_contains: Option<String>,    // Filter by path
    pub path_exclude: Option<String>,     // Exclude by path
    pub file_types: Option<Vec<String>>,  // Filter by extension
    pub heading_contains: Option<String>, // Filter by heading
}
```

---

### 3.8 `search/tantivy_engine.rs` — Tantivy Search Engine

**Lokasi:** `src/search/tantivy_engine.rs` (156 baris)

**Struct:**

```rust
pub struct TantivyEngine {
    index: Index,           // Tantivy index
    reader: IndexReader,    // Index reader
    schema: Schema,         // Index schema
    field_chunk_id: Field,  // chunk_id field
    field_file_path: Field, // file_path field
    field_heading: Field,   // heading field
    field_content: Field,   // content field
}
```

**Schema:**

| Field | Type | Options |
|-------|------|---------|
| `chunk_id` | TEXT | STRING, STORED |
| `file_path` | TEXT | TEXT, STORED |
| `heading` | TEXT | TEXT, STORED |
| `content` | TEXT | TEXT, STORED |

**Fungsi:**

| Fungsi | Deskripsi |
|--------|-----------|
| `TantivyEngine::open()` | Buka/buat index |
| `TantivyEngine::index_chunks()` | Index chunks ke Tantivy |
| `TantivyEngine::clear()` | Hapus semua documents |
| `TantivyEngine::search()` | Search dengan filters |

**Search Flow:**

1. Buat QueryParser dengan field: content, heading, file_path
2. Parse query string
3. Search dengan limit
4. Apply filters (path, file_type, heading)
5. Return results

---

### 3.9 `vector/mod.rs` — Vector Embeddings

**Lokasi:** `src/vector/mod.rs` (259 baris)

**Constants:**

```rust
pub const MODEL_NAME: &str = "all-MiniLM-L6-v2";
pub const DIMENSIONS: usize = 384;
```

**Fungsi Utama:**

| Fungsi | Deskripsi |
|--------|-----------|
| `create_model()` | Load ONNX model dari directory |
| `embed_and_store()` | Generate embeddings untuk semua chunks |
| `cosine_similarity()` | Hitung cosine similarity antara 2 vektor |
| `vector_search()` | Search berdasarkan vector similarity |
| `reciprocal_rank_fusion()` | Gabung BM25 + vector results |
| `load_all_embeddings()` | Load semua embeddings dari SQLite |
| `store_embeddings()` | Simpan embeddings ke SQLite |
| `delete_embeddings_for_file()` | Hapus embeddings untuk file |
| `import_embeddings_json()` | Import embeddings dari JSON |
| `export_chunks_for_embedding()` | Export chunks untuk embedding |
| `f32_to_bytes()` | Convert f32 array ke bytes |
| `bytes_to_f32()` | Convert bytes ke f32 array |

**Embedding Model:**

- Model: `all-MiniLM-L6-v2`
- Dimensions: 384
- Format: ONNX
- Files needed: `model.onnx`, `tokenizer.json`, `config.json`, `special_tokens_map.json`, `tokenizer_config.json`

**Hybrid Search Algorithm (RRF):**

```rust
pub fn reciprocal_rank_fusion(kw: &[String], vec: &[String], k: u64) -> Vec<(String, f32)> {
    // Formula: score += 1 / (k + rank + 1)
    // k = 60 (default)
    // Gabung scores dari BM25 dan vector search
    // Sort by score descending
}
```

---

### 3.10 `server/mod.rs` — MCP Server

**Lokasi:** `src/server/mod.rs` (312 baris)

**Struct:**

```rust
pub struct KnowledgeHubServer {
    config_path: String,
    config: Arc<Mutex<config::Config>>,
    tool_router: ToolRouter<Self>,
}
```

**MCP Tools:**

| Tool | Deskripsi | Parameters |
|------|-----------|------------|
| `scan` | Scan dan index files | `path: Option<String>` |
| `query` | Search knowledge base | `query: String`, `limit: Option<usize>`, `path_filter: Option<String>`, `heading_filter: Option<String>`, `hybrid: Option<bool>` |
| `stats` | Dapatkan statistik | (none) |
| `embed` | Generate embeddings | `batch_size: Option<usize>` |

**Transport:** stdio (stdin/stdout)

**Fungsi Helper:**

| Fungsi | Deskripsi |
|--------|-----------|
| `load_chunks_from_db()` | Load chunks dari SQLite |
| `enrich_lines()` | Tambah info line number |
| `format_results()` | Format results untuk output |
| `run_server()` | Start MCP server |

---

### 3.11 `embed.py` — Python Embedding Script

**Lokasi:** `embed.py` (61 baris)

**Fungsi:** Alternative embedding generator menggunakan Python fastembed.

**Usage:**

```bash
pip install fastembed
knowledge-hub export -o chunks.json
python3 embed.py chunks.json -o embeddings.json
knowledge-hub import --file embeddings.json
```

**Parameters:**

| Parameter | Default | Deskripsi |
|-----------|---------|-----------|
| `input` | (required) | chunks.json dari export |
| `-o, --output` | embeddings.json | Output file |
| `-m, --model` | Qdrant/all-MiniLM-L6-v2-onnx | Model name |
| `-b, --batch` | 64 | Batch size |

---

## 4. Alur Proses (Flowcharts)

### 4.1 Scan & Indexing

```
START
  │
  ▼
Load Config
  │
  ▼
Open Database + Tantivy
  │
  ▼
┌─────────────────────────┐
│ For each scan path:     │
│   WalkDir recursive     │
│   Filter by:            │
│   - extensions          │
│   - exclude_dirs        │
│   - exclude_files       │
│   - file_size           │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Parallel processing     │
│ (rayon par_iter):       │
│   Read file content     │
│   Filter by content     │
│   Chunk file            │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ SQLite Transaction:     │
│   Delete old file data  │
│   Insert file record    │
│   Insert chunk records  │
│   Commit                │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Tantivy:                │
│   Clear index           │
│   Index all chunks      │
│   Commit                │
└───────────┬─────────────┘
            │
            ▼
Print statistics
  │
  ▼
END
```

### 4.2 Hybrid Search

```
START
  │
  ▼
Load Config
  │
  ▼
Open Database + Tantivy
  │
  ▼
┌─────────────────────────┐
│ BM25 Search (Tantivy):  │
│   Parse query           │
│   Search with limit*2   │
│   Get chunk IDs         │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Load all embeddings     │
│ from SQLite             │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Apply path filter       │
│ (if specified)          │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Get query embedding:    │
│   Use first BM25 result │
│   embedding as query    │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Vector Search:          │
│   cosine_similarity     │
│   Sort by score         │
│   Get top limit*2       │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Reciprocal Rank Fusion: │
│   BM25 ranks            │
│   Vector ranks          │
│   Formula: 1/(k+rank+1) │
│   k=60                  │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Load chunks from SQLite │
│ Apply remaining filters │
└───────────┬─────────────┘
            │
            ▼
Print/Return results
  │
  ▼
END
```

### 4.3 MCP Server Request Handling

```
START
  │
  ▼
Client connects (stdio)
  │
  ▼
┌─────────────────────────┐
│ Wait for request        │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Parse JSON-RPC request  │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Route to tool handler:  │
│   - scan                │
│   - query               │
│   - stats               │
│   - embed               │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Execute tool logic      │
│ (same as CLI commands)  │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Format response         │
│ Return to client        │
└───────────┬─────────────┘
            │
            ▼
Loop back to wait
```

### 4.4 File Watcher

```
START
  │
  ▼
Load Config
  │
  ▼
Open Database + Tantivy
  │
  ▼
┌─────────────────────────┐
│ Setup notify::Watcher   │
│ Watch config paths      │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Event Loop:             │
│   recv_timeout(debounce)│
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Filter events:          │
│   - Create              │
│   - Modify              │
│   - Remove              │
│ Filter by extensions    │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ Debounce check:         │
│   Wait for quiet period │
│   Process batch         │
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ For each event:         │
│   Create/Modify:        │
│     Read file           │
│     Chunk               │
│     Update SQLite       │
│     Update Tantivy      │
│   Remove:               │
│     Delete from SQLite  │
└───────────┬─────────────┘
            │
            ▼
Print summary
  │
  ▼
Loop back to event loop
```

---

## 5. Database Schema

### Entity Relationship Diagram

```
┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
│     files       │       │     chunks      │       │   embeddings    │
├─────────────────┤       ├─────────────────┤       ├─────────────────┤
│ id (PK, AUTO)   │◄──────│ file_id (FK)    │◄──────│ chunk_id (FK)   │
│ path (UNIQUE)   │       │ id (PK, UUID)   │       │ model           │
│ size            │       │ chunk_index     │       │ dimensions      │
│ modified_at     │       │ heading_hierarchy│      │ vector (BLOB)   │
│ last_scanned    │       │ heading_level   │       └─────────────────┘
│ status          │       │ heading_text    │
└─────────────────┘       │ line_start      │
                          │ line_end        │
                          │ content         │
                          │ metadata_json   │
                          │ created_at      │
                          │ updated_at      │
                          └─────────────────┘
```

### Indexes

| Table | Index | Columns |
|-------|-------|---------|
| files | idx_files_path | path |
| files | idx_files_status | status |
| chunks | idx_chunks_file_id | file_id |
| chunks | idx_chunks_heading | heading_text |

---

## 6. Konfigurasi

### File Location

Default: `~/.knowledge-hub/config.yaml`

### Contoh Config

```yaml
version: "3.0.0"

scan:
  paths:
    - "."
  extensions:
    - md
    - json
    - yaml
    - yml
    - txt
    - toml
  exclude_dirs:
    - node_modules
    - .git
    - __pycache__
    - .cache
    - target
    - .cargo
    - dist
    - build
  exclude_files:
    - ".env"
    - "*.bak"
    - "*.tmp"

index:
  db_path: "~/.knowledge-hub/v3.db"
  tantivy_path: "~/.knowledge-hub/tantivy"
  max_chunk_tokens: 1000
  min_chunk_tokens: 50
  model_path: "~/.knowledge-hub/model"

filter:
  filename_contains: []
  filename_exclude: []
  content_contains: []
  content_exclude: []
  max_file_size: 0
```

### Environment Variables

| Variable | Deskripsi |
|----------|-----------|
| `RUST_LOG` | Log level (default: warn, knowledge_hub: info) |

---

## 7. Build, Run & Testing

### Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Binary location
./target/release/knowledge-hub
```

### Run

```bash
# Init config
knowledge-hub init

# Scan current directory
knowledge-hub scan

# Scan specific path
knowledge-hub scan --path /path/to/project

# Query (BM25)
knowledge-hub query "search term"

# Query (hybrid)
knowledge-hub query "search term" --hybrid

# Query with filters
knowledge-hub query "API" --path "docs" --heading "REST" --json

# Generate embeddings
knowledge-hub embed

# Start MCP server
knowledge-hub serve

# Start file watcher
knowledge-hub watch --debounce 500
```

### Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

**Test Coverage:** 41 unit tests

| Module | Tests |
|--------|-------|
| config | 13 tests |
| db/schema | 5 tests |
| chunker | 10 tests |
| search | 3 tests |
| vector | 7 tests |
| server | 3 tests |

---

## 8. Kelemahan & Area Optimasi

### 8.1 Performa

| Issue | Severity | Deskripsi |
|-------|----------|-----------|
| **Embedding dihitung ulang** | 🟡 Sedang | `cmd_embed()` memproses SEMUA chunks, bahkan yang sudah punya embedding. Seharusnya skip yang sudah ada. |
| **Load all embeddings** | 🟡 Sedang | `load_all_embeddings()` memuat SEMUA embeddings ke memory. Untuk database besar, ini bisa OOM. |
| **Brute force vector search** | 🟠 Tinggi | `vector_search()` melakukan cosine similarity ke SEMUA vectors. Seharusnya pakai index (HNSW, IVF). |
| **No caching** | 🟡 Sedang | Setiap query membuka database dan Tantivy ulang. Seharusnya pakai connection pool. |
| **Tantivy writer per batch** | 🟢 Rendah | `index_chunks()` membuat writer baru setiap call. Seharusnya reuse writer. |

### 8.2 Keamanan

| Issue | Severity | Deskripsi |
|-------|----------|-----------|
| **MCP server tanpa auth** | 🟠 Tinggi | Siapa saja bisa connect ke MCP server. Seharusnya ada authentication. |
| **Path traversal** | 🟡 Sedang | Tidak ada validasi path untuk mencegah akses ke directory sensitif. |
| **SQL injection** | 🟢 Rendah | Menggunakan parameterized queries, tapi ada beberapa raw SQL. |
| **No input validation** | 🟡 Sedang | Query dan filter tidak divalidasi. Bisa crash dengan input aneh. |

### 8.3 Keandalan

| Issue | Severity | Deskripsi |
|-------|----------|-----------|
| **Race condition watcher** | 🟡 Sedang | File watcher tidak handle race condition saat file di-ubah saat sedang di-index. |
| **No retry logic** | 🟡 Sedang | Database dan Tantivy operations tidak ada retry logic. |
| **Panic on error** | 🟡 Sedang | Banyak `unwrap()` yang bisa panic. Seharusnya gunakan `?` operator. |
| **No backup** | 🟢 Rendah | Tidak ada mekanisme backup database. |

### 8.4 Pemeliharaan

| Issue | Severity | Deskripsi |
|-------|----------|-----------|
| **Kode duplikasi** | 🟡 Sedang | Logic scan di `cmd_scan()` dan `server::scan()` sangat mirip. Seharusnya di-extract ke function. |
| **Kurang dokumentasi** | 🟡 Sedang | Tidak ada doc comments pada fungsi dan struct. |
| **Error handling** | 🟡 Sedang | Banyak `unwrap_or_default()` yang menyembunyikan error. |
| **Magic numbers** | 🟢 Rendah | Angka seperti 60 (RRF k), 384 (dimensions) hardcoded. |

### 8.5 Scalability

| Issue | Severity | Deskripsi |
|-------|----------|-----------|
| **SQLite limitation** | 🟡 Sedang | SQLite tidak cocok untuk concurrent writes. Untuk production, pakai PostgreSQL. |
| **No pagination** | 🟡 Sedang | Search results tidak ada pagination. Semua results dimuat sekaligus. |
| **Memory usage** | 🟡 Sedang | Semua embeddings dimuat ke memory. Untuk jutaan chunks, ini tidak scalable. |
| **Single node** | 🟢 Rendah | Tidak support distributed search. |

---

## 9. Kesimpulan & Rekomendasi

### 9.1 Ringkasan Kualitas Kode

**Poin Kuat:**

✅ Arsitektur modular yang bersih (config, db, chunker, search, vector, server)
✅ Menggunakan library yang tepat (Tantivy, fastembed, rmcp)
✅ Support multiple file types (Markdown, JSON, YAML, TXT)
✅ Hybrid search dengan RRF fusion
✅ Smart config detection untuk berbagai project type
✅ File watcher dengan debounce
✅ MCP server untuk integrasi dengan AI assistants
✅ 41 unit tests

**Poin Lemah:**

❌ Banyak kode duplikasi (scan logic)
❌ Error handling tidak konsisten
❌ Kurang dokumentasi
❌ Performa vector search brute force
❌ Tidak ada authentication pada MCP server
❌ Memory usage tinggi untuk database besar

### 9.2 Rekomendasi Perbaikan

**Prioritas 1 (Wajib):**

1. **Extract shared scan logic** — Buat function `scan_and_index()` yang dipakai oleh CLI dan MCP server
2. **Improve error handling** — Ganti `unwrap()` dengan `?` operator
3. **Add doc comments** — Dokumentasi semua public functions dan structs
4. **Skip existing embeddings** — Di `embed_and_store()`, cek apakah embedding sudah ada

**Prioritas 2 (Sebaiknya):**

1. **Add authentication ke MCP server** — Pakai token atau API key
2. **Implement vector index** — Pakai HNSW atau IVF untuk search lebih cepat
3. **Add pagination** — Support offset/limit pada search results
4. **Connection pooling** — Reuse database connection
5. **Input validation** — Validasi query dan filter parameters

**Prioritas 3 (Nice-to-have):**

1. **Support more file types** — PDF, DOCX, HTML
2. **Multi-model embeddings** — Support multiple embedding models
3. **Distributed search** — Support multiple nodes
4. **Backup/restore** — Mekanisme backup database
5. **Web UI** — Interface untuk manage dan search
6. **API REST** — HTTP API selain MCP

### 9.3 Saran Pengembangan Fitur Lanjutan

| Fitur | Deskripsi | Kompleksitas |
|-------|-----------|--------------|
| **Semantic chunking** | Chunk berdasarkan makna, bukan heading | Tinggi |
| **Multi-language support** | Support bahasa selain Inggris | Sedang |
| **Real-time sync** | Sync ke cloud storage (S3, GCS) | Sedang |
| **Version control** | Track perubahan chunks dari waktu ke waktu | Sedang |
| **Access control** | Role-based access untuk MCP server | Sedang |
| **Monitoring** | Metrics dan logging untuk production | Rendah |
| **Plugin system** | Support custom chunker dan search algorithms | Tinggi |

---

## Appendix: File Structure

```
knowledge-hub-v3/
├── Cargo.toml              # Dependencies dan build config
├── PROJECT_CHRONOLOGY.md   # Project history dan status
├── REVERSE_ENGINEERING.md  # Dokumentasi ini
├── embed.py                # Python embedding script
├── src/
│   ├── main.rs             # Entry point, CLI, command handlers
│   ├── config/
│   │   └── mod.rs          # Config struct, load/save, smart detection
│   ├── db/
│   │   ├── mod.rs          # Database connection, transactions
│   │   └── schema.rs       # Migrations, table definitions
│   ├── chunker/
│   │   ├── mod.rs          # Chunk struct, file type detection
│   │   └── markdown.rs     # Markdown parser dan chunker
│   ├── search/
│   │   ├── mod.rs          # SearchResult, SearchFilters
│   │   └── tantivy_engine.rs # Tantivy index dan search
│   ├── vector/
│   │   └── mod.rs          # Embeddings, cosine similarity, RRF
│   └── server/
│       └── mod.rs          # MCP server, tool handlers
└── target/
    └── release/
        └── knowledge-hub   # Compiled binary
```

---

_Dokumentasi ini dibuat berdasarkan analisis source code Knowledge Hub v3.0.0_
_Total: 10 Rust files, 2,153 baris kode, 41 unit tests_
