<div align="center">

# AIKD

### AI Knowledge Daemon

**Pencarian kode ter-index untuk AI agent dan developer.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-2.0.0-green.svg)]()
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)]()
[![Tests](https://img.shields.io/badge/tests-82%20passed-brightgreen.svg)]()

</div>

---

## Daftar Isi

- [Tentang](#tentang)
- [Mengapa AIKD?](#mengapa-aikd)
- [Fitur Utama](#fitur-utama)
- [Demo](#demo)
- [Prasyarat](#prasyarat)
- [Instalasi](#instalasi)
- [Mulai Cepat](#mulai-cepat)
- [Mode Penggunaan](#mode-penggunaan)
- [Referensi Perintah](#referensi-perintah)
- [Konfigurasi](#konfigurasi)
- [Referensi API](#referensi-api)
- [Struktur Project](#struktur-project)
- [Hasil Benchmark](#hasil-benchmark)
- [Pemecahan Masalah](#pemecahan-masalah)
- [Berkontribusi](#berkontribusi)
- [Lisensi](#lisensi)
- [Ucapan Terima Kasih](#ucapan-terima-kasih)

---

## Tentang

**AIKD** (AI Knowledge Daemon) adalah **MCP tool provider** yang ditulis dalam Rust, memberikan akses instan ke codebase Anda untuk AI agent. AIKD meng-index file project ke knowledge base dan mengekspos **7 tools** via [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) yang bisa dipanggil oleh AI agent manapun.

### Apa itu AIKD?

| Kategori | Jawaban |
|----------|---------|
| **MCP Tool Provider** | Ya — mengekspos tools `scan`, `query`, `embed`, `stats`, `remember`, `recall`, `status` |
| **CLI Tool** | Ya — `aikd query "login" --json` bisa langsung dari terminal |
| **REST API** | Ya — HTTP endpoint di `http://localhost:9090` |
| **VS Code Extension** | Tidak — binary standalone, bukan plugin IDE |
| **Library/SDK** | Tidak — tool end-user, bukan dependency |

### Cara kerja

```
┌─────────────────────────────────────────────────────────┐
│                    AI Agent                              │
│  (MiMoCode, Claude Code, Cursor, Cline, Windsurf, dll)  │
└────────────────────────┬────────────────────────────────┘
                         │ MCP Protocol (stdio)
                         ▼
┌─────────────────────────────────────────────────────────┐
│                    AIKD Server                           │
│                                                         │
│  Tools:                                                 │
│    scan    → Index file ke knowledge base                │
│    query   → BM25 + vector semantic search               │
│    embed   → Generate vector embeddings                  │
│    stats   → Statistik knowledge base                    │
│    remember → Simpan percakapan ke memory                │
│    recall  → Cari history percakapan                     │
│    status  → Status resource sistem                      │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│              Knowledge Base                              │
│  SQLite + Tantivy (BM25) + ONNX embeddings (384d)       │
│  84 file · 412 chunk · pencarian <1ms                    │
└─────────────────────────────────────────────────────────┘
```

### Masalah yang Dipecahkan

AI agent perlu memahami codebase Anda. Tanpa index, mereka terpaksa:
- Menjalankan `grep` atau `find` berulang kali (lambat, tanpa ranking)
- Membaca seluruh file (boros token)
- Menebak lokasi sesuatu (tidak akurat)

### Solusi

AIKD meng-index project Anda. AI agent memanggil `aikd query` via MCP dan mendapat hasil ter-ranking dalam milidetik.

---

## Mengapa AIKD?

| Fitur | grep/find | AIKD |
|-------|-----------|------|
| Kecepatan | Scan setiap kali | Pre-index, <1ms |
| Ranking | Tidak ada | BM25 + semantic |
| Konteks | Baris teks mentah | File, heading, rentang baris, skor |
| Output | Teks tidak terstruktur | JSON (bisa diparse mesin) |
| Pencarian semantic | Tidak | Ya (vector embeddings) |
| Auto-sync | Tidak | Ya (file watcher) |

---

## Fitur Utama

- **BM25 Full-Text Search** — Pencarian keyword cepat via Tantivy engine
- **Hybrid Search** — BM25 + vector semantic search dengan Reciprocal Rank Fusion
- **Vector Embeddings** — Embedding berbasis ONNX (all-MiniLM-L6-v2, 384 dimensi)
- **Smart Chunking** — Heading markdown, fungsi source code, file terstruktur
- **Source Code Aware** — Chunk Rust, Python, TypeScript, JavaScript, Go berdasarkan batas fungsi
- **File Watcher** — Auto-reindex saat file berubah dengan blake3 incremental hashing
- **Session Memory** — Simpan dan recall konteks percakapan
- **MCP Server** — Model Context Protocol untuk AI assistant (Claude, MiMo, Cursor, dll.)
- **REST API** — HTTP endpoint untuk integrasi eksternal
- **CLI Tools** — Penggunaan command-line langsung, tanpa server
- **Auto-Agent Registration** — Deteksi dan registrasi ke 6 AI agent saat `aikd init`
- **Resource Adaptive** — Auto-tuning untuk tier hardware Low/Medium/High/Max/GPU
- **Cross-Platform** — Windows, Linux, macOS

---

## Demo

<!-- Tambahkan screenshot atau GIF di sini -->

```
$ aikd query "fungsi login" --json --limit 3

[
  {
    "file_path": "src/auth/login.rs",
    "heading_hierarchy": "Auth > Login",
    "heading_text": "login",
    "content": "pub fn login(user: &str, pass: &str) -> Result<Token> { ... }",
    "line_start": 42,
    "line_end": 58,
    "score": 5.011
  },
  ...
]
```

---

## Prasyarat

| Kebutuhan | Minimum | Disarankan |
|-----------|---------|------------|
| OS | Windows 10+, Linux, macOS | 64-bit apapun |
| RAM | 2 GB | 8 GB+ |
| CPU | 2 core | 4+ core |
| Disk | 200 MB | 1 GB+ (untuk model) |
| Rust | 1.75+ (untuk build dari source) | Stable terbaru |
| GPU | Tidak wajib | NVIDIA GPU untuk embedding lebih cepat |

---

## Instalasi

### Opsi 1: Binary Siap Pakai (Disarankan)

**Windows:**
```powershell
# Download dari releases, lalu:
copy aikd.exe %USERPROFILE%\.local\bin\aikd.exe
```

**Linux / macOS:**
```bash
# Download dari releases, lalu:
cp aikd ~/.local/bin/aikd
chmod +x ~/.local/bin/aikd
```

### Opsi 2: Build dari Source

```bash
# Clone repository
git clone https://github.com/your-org/aikd.git
cd aikd

# Build release
cargo build --release

# Install
# Windows:
copy target\release\aikd.exe %USERPROFILE%\.local\bin\aikd.exe

# Linux/macOS:
cp target/release/aikd ~/.local/bin/aikd
chmod +x ~/.local/bin/aikd
```

### Opsi 3: Script Instalasi

**Linux / macOS:**
```bash
curl -sSfL https://raw.githubusercontent.com/your-org/aikd/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

### Setelah Instalasi

```bash
aikd init    # Buat config, download model, registrasi AI agent
```

---

## Mulai Cepat

```bash
# 1. Inisialisasi (setup sekali saja)
aikd init

# 2. Index project Anda
aikd scan

# 3. Generate embeddings (untuk pencarian semantic)
aikd embed

# 4. Cari
aikd query "kata kunci Anda" --json
```

---

## Mode Penggunaan

AIKD mendukung 3 mode. Pilih yang sesuai dengan alur kerja Anda.

### Mode 1: CLI Tools (Langsung)

Panggil `aikd` langsung dari terminal atau AI agent. Tanpa server, tanpa config tambahan.

```bash
aikd query "fungsi login" --json
aikd scan
aikd stats
```

**Cocok untuk:** Pencarian cepat, pipeline CI/CD, AI agent yang bisa memanggil CLI.

### Mode 2: MCP Server

Untuk AI assistant yang mendukung Model Context Protocol.

```bash
aikd serve
```

AIKD otomatis registrasi ke agent-agent ini saat `aikd init`:

| Agent | File Config |
|-------|-------------|
| Claude Code | `~/.claude.json` |
| Cursor | `~/.cursor/mcp.json` |
| Cline | `~/.cline/mcp.json` |
| Continue | `~/.continue/config.json` |
| Windsurf | `~/.windsurf/mcp.json` |
| MiMoCode | `~/.mcp.json` |

**Cocok untuk:** AI assistant dengan dukungan MCP bawaan.

### Mode 3: Daemon + REST API

Layanan background dengan endpoint HTTP dan auto-sync.

```bash
aikd daemon --foreground    # REST API di http://localhost:9090
aikd watch                  # Auto-reindex saat file berubah
```

**Cocok untuk:** Web dashboard, setup multi-user, integrasi tool eksternal.

---

## Referensi Perintah

### Opsi Global

| Flag | Deskripsi |
|------|-----------|
| `-c, --config <FILE>` | Path file config (default: `~/.aikd/config.yaml`) |
| `--json` | Output JSON untuk semua perintah |
| `-q, --quiet` | Suppress output non-error |
| `-V, --version` | Tampilkan versi |
| `-h, --help` | Tampilkan bantuan |

### Init & Config

```bash
aikd init [--path <DIR>]     # Inisialisasi project (config + model + registrasi agent)
```

### Indexing

```bash
aikd scan [--path <DIR>]     # Scan dan index file
aikd watch [--debounce <MS>] # Watch perubahan, auto-reindex (default: 500ms)
```

### Pencarian

```bash
aikd query <KATA>                    # BM25 full-text search
aikd query <KATA> --json             # Output JSON
aikd query <KATA> --limit 20         # Maksimal 20 hasil
aikd query <KATA> --path src/        # Filter berdasarkan path
aikd query <KATA> -H "Error"         # Filter berdasarkan heading
aikd query <KATA> --hybrid           # BM25 + vector semantic search
```

### Embedding

```bash
aikd embed                           # Generate vector embeddings
aikd embed --batch 64                # Custom batch size
aikd export [-o chunks.json]         # Export chunks ke JSON
aikd import --file <FILE>            # Import embeddings dari JSON
```

### Session Memory

```bash
aikd remember --role user --content "pesan"              # Simpan pesan
aikd recall "query"                                      # Cari pesan
aikd recall "query" --session <ID> --limit 20            # Cari dalam session
```

### Server & Daemon

```bash
aikd serve                           # Start MCP server (stdio)
aikd daemon --foreground             # Start REST API + MCP server
aikd status                          # Tampilkan status sistem (JSON)
aikd inject -- <COMMAND>             # Inject konteks ke CLI lain
```

### Benchmark

```bash
aikd benchmark                       # Jalankan 8-scenario benchmark suite
```

---

## Konfigurasi

### File Config

Lokasi default: `~/.aikd/config.yaml`

```yaml
version: "2.0.0"

scan:
  include_paths: ["."]
  exclude_paths: ["node_modules", ".git", "__pycache__", ".cache", "target"]
  include_extensions: ["md", "json", "yaml", "yml", "txt", "toml", "rs", "py", "ts", "js", "go"]
  exclude_files: [".env", "*.bak", "*.tmp", "*.secret"]

chunk:
  max_tokens: 1000
  min_tokens: 100

embedding:
  enabled: true
  model: "all-MiniLM-L6-v2"
  batch_size: "auto"

index:
  db_path: "~/.aikd/aikd.db"
  tantivy_path: "~/.aikd/tantivy_index"
  model_path: "~/.local/share/aikd/model"

server:
  rest_port: 9090
  auth_token: null

resource:
  mode: Auto    # Auto | Low | Medium | High | Max
```

### Environment Variables

| Variable | Deskripsi |
|----------|-----------|
| `AIKD_MODEL_PATH` | Override direktori model |
| `AIKD_DATA_DIR` | Override direktori data (`~/.aikd/`) |
| `AIKD_TOKEN` | Auth token (sama dengan `config.server.auth_token`) |
| `RUST_LOG` | Level log (contoh: `RUST_LOG=aikd=debug`) |

### Mode Resource

| Mode | RAM | CPU | Embedding | Batch Size | Parallelism |
|------|-----|-----|-----------|------------|-------------|
| Low | <2 GB | ≤2 | OFF | 1 | 1 |
| Medium | 2–8 GB | ≤4 | ON | 8 | 2 |
| High | 8–16 GB | ≤8 | ON | 32 | 4 |
| Max | ≥16 GB | >8 | ON | 64 | 8 |
| Auto | deteksi | deteksi | deteksi | deteksi | deteksi |

---

## Referensi API

### REST API (Mode Daemon)

Base URL: `http://localhost:9090`

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/query?q=<kata>&limit=10&hybrid=true` | Cari di knowledge base |
| GET | `/api/stats` | Dapatkan statistik index |
| POST | `/api/scan` | Trigger scan file |
| POST | `/api/remember` | Simpan pesan percakapan |
| POST | `/api/recall` | Cari riwayat percakapan |

**Autentikasi:** `Authorization: Bearer <token>` (jika `config.server.auth_token` di-set)

### MCP Tools

Saat berjalan sebagai MCP server (`aikd serve`), tools ini tersedia:

| Tool | Deskripsi |
|------|-----------|
| `scan` | Scan dan index file |
| `query` | Cari di knowledge base |
| `stats` | Dapatkan statistik |
| `embed` | Generate embeddings |
| `remember` | Simpan percakapan |
| `recall` | Cari percakapan |
| `status` | Dapatkan status sistem |

---

## Struktur Project

```
aikd/
├── Cargo.toml                  # Workspace root (v2.0.0)
├── README.md                   # File ini
├── install.sh                  # Installer Linux/macOS
├── install.ps1                 # Installer Windows
├── crates/
│   ├── core/                   # Tipe data, error, config, security, fusion, platform
│   ├── storage/                # SQLite database + migrasi (v4)
│   ├── indexer/                # Tantivy BM25 + HNSW vector index
│   ├── embedder/               # ONNX embedding engine + LRU cache
│   ├── chunker/                # Chunking markdown, teks, source code
│   ├── scanner/                # Shared scan logic (single source of truth)
│   ├── session/                # Session & memory percakapan
│   ├── server/                 # MCP server + REST API (axum)
│   ├── watcher/                # File system watcher (notify)
│   ├── plugin/                 # Konstanta SDK untuk integrasi eksternal
│   ├── benchmark/              # Benchmark & stress test suite
│   └── cli/                    # Binary CLI (aikd)
└── extensions/
    └── vscode/                 # Ekstensi VSCode
```

---

## Hasil Benchmark

Diuji pada: AMD EPYC 7B13 (6 core), 7.8 GB RAM, Linux x86_64

| Tes | Durasi | Throughput | Status |
|-----|--------|-----------|--------|
| Indexing (1000 file) | 144 ms | 6.934 file/detik | PASS |
| BM25 Search (100 query) | 1.917 ms | 0.21 ms/query | PASS |
| Hybrid Search (50 query) | 999 ms | 0.35 ms/query | PASS |
| Concurrent Search (500 query) | 28 ms | 17.669 query/detik | PASS |
| Chunking Throughput (1000 file) | 4 ms | 251.985 file/detik | PASS |
| Incremental Re-index (100 file) | 140 ms | 717 file/detik | PASS |

**Penggunaan resource puncak:** CPU 22.9%, RAM 27.4%

---

## Pemecahan Masalah

| Masalah | Solusi |
|---------|--------|
| `aikd: command not found` | Tambahkan `~/.local/bin` ke PATH Anda |
| `Config not found` | Jalankan `aikd init` |
| `Model not downloaded` | Jalankan `aikd init` atau `aikd model download all-MiniLM-L6-v2` |
| `No results` | Jalankan `aikd scan` terlebih dahulu |
| `Hybrid search not working` | Jalankan `aikd embed` terlebih dahulu (butuh embeddings) |
| `Permission denied` (Linux/macOS) | `chmod +x ~/.local/bin/aikd` |
| `Port already in use` | Ganti `server.rest_port` di config |
| `GPU not used` | fastembed menggunakan CPU secara default; GPU butuh build kustom |

---

## Berkontribusi

Kontribusi dipersilakan! Ikuti langkah-langkah berikut:

1. Fork repository
2. Buat branch fitur (`git checkout -b fitur/fitur-luar-biasa`)
3. Commit perubahan Anda (`git commit -m 'Tambah fitur luar biasa'`)
4. Push ke branch (`git push origin fitur/fitur-luar-biasa`)
5. Buka Pull Request

### Setup Pengembangan

```bash
git clone https://github.com/your-org/aikd.git
cd aikd
cargo build
cargo test
```

### Gaya Kode

- Jalankan `cargo fmt` sebelum commit
- Jalankan `cargo clippy -- -D warnings` untuk cek masalah lint
- Semua test harus lulus (`cargo test`)

---

## Lisensi

Project ini dilisensikan di bawah Lisensi MIT. Lihat file [LICENSE](LICENSE) untuk detail.

---

## Ucapan Terima Kasih

Dibangun dengan project open-source luar biasa berikut:

| Pustaka | Kegunaan |
|---------|----------|
| [Tantivy](https://github.com/quickwit-oss/tantivy) | Mesin BM25 full-text search |
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) | ONNX embedding inference |
| [hnsw_rs](https://github.com/jeremiedecock/hnsw-rs) | HNSW vector index |
| [rusqlite](https://github.com/rusqlite/rusqlite) | Binding SQLite |
| [axum](https://github.com/tokio-rs/axum) | Framework HTTP |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | Protokol MCP |
| [notify](https://github.com/notify-rs/notify) | File system watcher |
| [rayon](https://github.com/rayon-rs/rayon) | Data parallelism |
| [blake3](https://github.com/BLAKE3-team/BLAKE3) | Hashing cepat |

---

<div align="center">

**Dibuat dengan Rust dan dedikasi.**

</div>
