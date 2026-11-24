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

> **Build default = 100% Zero-ONNX.** Binary **~11.9 MB**, daemon **~39–48 MB RAM**, retrieval murni **SQLite FTS5 BM25 + Live Disk Snippets** — tanpa runtime ONNX dan **tanpa download model**. `fastembed`/ONNX hanya ada di fitur opt-in `semantic` / `semantic-ext` dan **OFF secara default** sesuai [ADR-0001](docs/adr/0001-core-principles.md).

---

## Daftar Isi

- [Tentang](#tentang)
- [Mengapa AIKD?](#mengapa-aikd)
- [Arsitektur 2 Lapis](#arsitektur-2-lapis)
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

**AIKD** (AI Knowledge Daemon) adalah **MCP tool provider** yang ditulis dalam Rust, memberikan akses instan ke codebase Anda untuk AI agent. AIKD meng-index file project ke knowledge base dan mengekspos **15 tools** via [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) yang bisa dipanggil oleh AI agent manapun.

Build default **100% Zero-ONNX**: pencarian memakai SQLite FTS5 BM25 + Live Disk Snippets. Fitur `semantic` (fastembed/ONNX) adalah opt-in dan tidak ikut di binary default.

### Apa itu AIKD?

| Kategori | Jawaban |
|----------|---------|
| **MCP Tool Provider** | Ya — `scan`, `query`, `symbol`, `handoff`, `memory_*`, `event_append`, `remember`/`recall`, `stats`/`status`, `embed` (opt-in) |
| **CLI Tool** | Ya — `aikd query "login" --json` bisa langsung dari terminal |
| **REST API** | Ya — HTTP endpoint di `http://localhost:9090` |
| **Ekstensi Editor/Browser** | Opsional — binary tetap standalone; tersedia extension Chrome & VS Code |
| **Library/SDK** | Tidak — tool end-user, bukan dependency |

### Cara kerja

```
┌─────────────────────────────────────────────────────────┐
│              AI Agent / Developer                        │
│  (Claude Code, Cursor, Cline, Windsurf, Pi, OMP, dll)   │
└────────────────────────┬────────────────────────────────┘
                         │ MCP (stdio) atau CLI
                         ▼
┌─────────────────────────────────────────────────────────┐
│                  AIKD Core Engine                        │
│                                                         │
│  scan      → Index incremental (Blake3)                  │
│  query     → BM25 SQLite FTS5 + Live Disk Snippets       │
│  symbol    → Lookup fungsi/struct/class + line range     │
│  handoff   → Resume pack anti-context-rot                │
│  memory_*  → fact/decision/constraint/task/checkpoint    │
│  event_*   → Journal event multi-agent                   │
│  hooks     → Git + Agent auto-sync (install/status)      │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│           SQLite WAL — ~/.aikd/aikd.db                  │
│  FTS5 contentless BM25 + metadata + memory + events     │
│  Zero-ONNX: tanpa model, tanpa runtime ONNX             │
│  (hybrid vector = opt-in `semantic` / provider eksternal)│
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

## Arsitektur 2 Lapis

AIKD sengaja dipisah agar agent tidak pernah bergantung pada UI.

### Lapis 1 — Core Engine (untuk AI Agent)

- **CLI + MCP:** `query`, `symbol`, `handoff`, `memory_save` / `memory_task` / `memory_lifecycle`, `event_append`, `scan`, `stats`, `status`.
- **Auto-sync tanpa manual:** `aikd hooks install` memasang hook Git `post-commit` / `post-merge` dan hook agent `SessionStart` / `PreCompact`.
- **SQLite WAL multi-process:** CLI, MCP client, dan daemon boleh menulis ke `~/.aikd/aikd.db` yang sama secara bersamaan.
- **Zero-ONNX:** BM25 SQLite FTS5 + Live Disk Snippets. Tidak ada model, tidak ada ONNX runtime, tidak ada Python.
- **Live Disk Snippets:** Lean Mode menyimpan token, bukan salinan teks; snippet dibaca dari file asli dan file hilang dilewati dengan aman.

### Lapis 2 — AIKD Studio + Tray + Chrome Extension (Control Plane Manusia, Opsional)

- **AIKD Studio** (`/ui`): SPA 100% offline di dalam binary — Mission Control, pencarian, memory lifecycle, dan **Knowledge Graph 10 mode**.
- **Tray app:** start/stop daemon, buka Studio, cek health tanpa terminal.
- **Chrome extension:** clip seleksi, transkrip chat AI (ChatGPT/Claude/Gemini), dan teks halaman ke memory.
- **PARA Vault & Notes:** catatan markdown, tag, dan wikilink yang menjadi node graph untuk agent.

Semua fitur Lapis 2 bersifat opsional. Menjalankan `aikd serve` / `aikd query` tidak membutuhkan Studio, tray, browser, maupun ONNX.

---

## Mengapa AIKD?

| Fitur | grep/find | AIKD |
|-------|-----------|------|
| Kecepatan | Scan setiap kali | Pre-index, <1ms |
| Ranking | Tidak ada | BM25 (+ hybrid opt-in) |
| Konteks | Baris teks mentah | File, heading, rentang baris, skor |
| Output | Teks tidak terstruktur | JSON (bisa diparse mesin) |
| Pencarian semantic | Tidak | Opt-in (`--features semantic` atau provider eksternal) |
| Auto-sync | Tidak | Ya (`aikd hooks install` + `scan --changed`) |

---

## Fitur Utama

- **Zero-ONNX Default** — Binary ~11.9 MB, daemon ~39–48 MB RAM; tanpa runtime ONNX dan tanpa download model
- **BM25 Full-Text Search** — SQLite FTS5 contentless, cepat dan tanpa engine eksternal
- **Hybrid Search (Opt-in)** — BM25 + vector semantic via `--features semantic` atau provider embeddings eksternal
- **Vector Embeddings (Opt-in)** — fastembed/ONNX hanya jika fitur `semantic` dibangun
- **Smart Chunking** — Heading markdown, fungsi source code, file terstruktur
- **Source Code Aware** — Chunk Rust, Python, TypeScript, JavaScript, Go berdasarkan batas fungsi
- **Live Disk Snippets** — Snippet dibaca dari file asli; file hilang/stale dilewati
- **File Watcher & Auto-Hooks** — `aikd watch`, Git `post-commit`/`post-merge`, agent `SessionStart`/`PreCompact`
- **Git-Aware Incremental Scan** — `aikd scan --changed` meng-index hanya file yang berubah
- **Structured Memory** — `fact`, `decision`, `constraint`, `discovery`, `task`, `checkpoint` + lifecycle
- **Anti-Context-Rot Handoff** — Resume pack untuk Claude Code, Codex, Gemini, OpenCode, Pi, OMP
- **MCP Server** — Model Context Protocol untuk AI assistant (Claude, Cursor, Cline, dll.)
- **REST API + AIKD Studio** — Dashboard offline, Knowledge Graph 10 mode, PARA Vault, Mission Control
- **Chrome Extension** — Clip seleksi, chat AI, dan teks halaman ke memory
- **Auto-Agent Registration** — Deteksi dan registrasi ke 16+ AI agent
- **Resource Adaptive** — Auto-tuning scan/parallelism untuk tier hardware Low/Medium/High/Max/GPU
- **Cross-Platform** — Windows, Linux, macOS

---

## Demo

**Demo visual:** [10-Mode Knowledge Graph (GIF)](docs/media/aikd-graph-motion-qa.gif) · [MP4](docs/media/aikd-graph-motion-qa.mp4)

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
| Disk | 200 MB | 1 GB+ (data index; bukan model ONNX) |
| Rust | 1.75+ (untuk build dari source) | Stable terbaru |
| GPU | Tidak wajib | Hanya relevan untuk build `semantic` |

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
git clone https://github.com/gelutjari/aikd.git
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
curl -sSfL https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

### Setelah Instalasi

```bash
aikd init    # Buat config & registrasi AI agent (tanpa download model)
```

---

## Mulai Cepat

```bash
# 1. Inisialisasi (setup sekali saja, tanpa download model)
aikd init

# 2. Index project Anda
aikd scan

# 3. Pasang auto-sync (Git + agent hooks) — opsional tapi disarankan
aikd hooks install

# 4. Cari
aikd query "kata kunci Anda" --json

# 5. Index hanya file yang berubah (Git-aware)
aikd scan --changed
```

> `aikd embed` **tidak diperlukan** di build default. Embedding hanya untuk build opt-in `semantic` atau provider embeddings eksternal.

---

## Mode Penggunaan

AIKD mendukung 4 mode. Pilih yang sesuai dengan alur kerja Anda.

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

AIKD mendeteksi dan meregistrasi 16+ agent populer saat `aikd init` / `aikd register`:

| Agent | File Config |
|-------|-------------|
| Claude Code | `~/.claude.json` |
| Cursor | `~/.cursor/mcp.json` |
| Cline | `~/.cline/mcp.json` |
| Continue | `~/.continue/config.json` |
| Windsurf | `~/.windsurf/mcp.json` |
| MiMoCode | `~/.mcp.json` |
| …dan agent CLI/IDE lain | Claude Desktop, Antigravity, Codex CLI, Gemini CLI, OpenCode, Pi, OMP, Qoder |

**Cocok untuk:** AI assistant dengan dukungan MCP bawaan.

### Mode 3: Daemon + REST API

Layanan background dengan endpoint HTTP dan auto-sync.

```bash
aikd daemon --foreground    # REST API di http://localhost:9090
aikd watch                  # Auto-reindex saat file berubah
```

**Cocok untuk:** Web dashboard, setup multi-user, integrasi tool eksternal.

### Mode 4: Auto-Hooks (Git + Agent)

Pasang sekali agar index dan handoff selalu segar tanpa mengingat perintah manual.

```bash
aikd hooks install                 # default: Git hooks + agent hooks
aikd hooks install --git           # hanya Git (post-commit, post-merge)
aikd hooks install --agents        # hanya agent (Claude SessionStart/PreCompact)
aikd hooks status                  # cek status
aikd hooks status --json           # output mesin
```

**Cocok untuk:** Semua project Git dan semua pengguna Claude Code / agent CLI.

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
aikd init [--path <DIR>]     # Inisialisasi project (config + registrasi agent, tanpa model)
```

### Indexing

```bash
aikd scan [--path <DIR>]     # Scan dan index file
aikd scan --changed          # Hanya file yang dilaporkan `git status --porcelain`
aikd scan --force            # Paksa re-chunk (abaikan hash)
aikd watch [--debounce <MS>] # Watch perubahan, auto-reindex (default: 500ms)
```

### Hooks

```bash
aikd hooks install [--project <DIR>] [--git] [--agents]  # Default: Git + Agent
aikd hooks status  [--project <DIR>] [--json]            # Audit hook terpasang
```

- Hook Git idempoten: menambahkan blok AIKD bertanda ke `post-commit` / `post-merge` tanpa menghapus hook pengguna, lalu menjalankan `aikd scan --changed --quiet`.
- Hook agent menambahkan `SessionStart` / `PreCompact` yang memanggil `aikd handoff --compact` tanpa menghapus setting yang sudah ada.

### Pencarian

```bash
aikd query <KATA>                    # BM25 full-text search
aikd query <KATA> --json             # Output JSON
aikd query <KATA> --limit 20         # Maksimal 20 hasil
aikd query <KATA> --path src/        # Filter berdasarkan path
aikd query <KATA> -H "Error"         # Filter berdasarkan heading
aikd query <KATA> --hybrid           # BM25 + vector (butuh build semantic / provider)
```

### Embedding

```bash
aikd embed                           # Opt-in: butuh build --features semantic
aikd embed --batch 64                # Custom batch size (opt-in)
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
  enabled: false          # Zero-ONNX default
  provider: "none"        # none | external
  model: "all-MiniLM-L6-v2"
  batch_size: "auto"
  endpoint: ""            # OpenAI-compatible URL jika provider=external
  api_key_env: "AIKD_EMBEDDING_KEY"

index:
  db_path: "~/.aikd/aikd.db"
  model_path: "~/.local/share/aikd/model"   # hanya dipakai build semantic

server:
  rest_port: 9090
  auth_token: null

resource:
  mode: Auto    # Auto | Low | Medium | High | Max
```

### Environment Variables

| Variable | Deskripsi |
|----------|-----------|
| `AIKD_MODEL_PATH` | Override direktori model (hanya build `semantic`) |
| `AIKD_DATA_DIR` | Override direktori data (`~/.aikd/`) |
| `AIKD_TOKEN` | Auth token (sama dengan `config.server.auth_token`) |
| `RUST_LOG` | Level log (contoh: `RUST_LOG=aikd=debug`) |

### Mode Resource

Semua tier tetap **Zero-ONNX** selama fitur `semantic` tidak dibangun/diaktifkan. Tier hanya mengatur parallelism scan dan (jika semantic aktif) batch embedding.

| Mode | RAM | CPU | Embedding | Batch Size | Parallelism |
|------|-----|-----|-----------|------------|-------------|
| Low | <2 GB | ≤2 | OFF | 1 | 1 |
| Medium | 2–8 GB | ≤4 | Opt-in | 8 | 2 |
| High | 8–16 GB | ≤8 | Opt-in | 32 | 4 |
| Max | ≥16 GB | >8 | Opt-in | 64 | 8 |
| Auto | deteksi | deteksi | deteksi | deteksi | deteksi |

---

## Referensi API

### REST API (Mode Daemon)

Base URL: `http://localhost:9090`

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/query?q=<kata>&limit=10&hybrid=true` | Cari di knowledge base (hybrid butuh build `semantic` / provider) |
| GET | `/api/stats` | Dapatkan statistik index |
| POST | `/api/scan` | Trigger scan file |
| POST | `/api/remember` | Simpan pesan percakapan |
| POST | `/api/recall` | Cari riwayat percakapan |

**Autentikasi:** `Authorization: Bearer <token>` (jika `config.server.auth_token` di-set)

### MCP Tools

Saat berjalan sebagai MCP server (`aikd serve`), tools ini tersedia:

| Tool | Deskripsi |
|------|-----------|
| `scan` | Incremental Blake3 scan, chunking, symbol extraction & sinkronisasi rules `.md` |
| `query` | BM25 SQLite FTS5 (+ hybrid opsional) dengan Live Disk Snippets |
| `symbol` | Lookup exact/prefix fungsi, struct, class, interface + line range |
| `handoff` | Resume pack anti-context-rot (tasks, checkpoint, decisions, rules, events) |
| `memory_save` | Simpan memory bertipe (`fact`, `decision`, `constraint`, `discovery`, `task`, `checkpoint`) |
| `memory_list` | Lihat memory aktif/superseded per kind & workstream |
| `memory_task` | Ambil task aktif & checkpoint terbaru setelah `/compact` |
| `memory_lifecycle` | Tandai memory `stale` / `archive` tanpa menghapus provenance |
| `memory_conflict` | Tandai dua item sebagai konflik (keduanya dipertahankan) |
| `event_append` | Append event lifecycle agent yang idempoten |
| `remember` / `recall` | Simpan dan cari riwayat percakapan mentah |
| `embed` | Generate embedding (opt-in `--features semantic` atau provider eksternal) |
| `stats` / `status` | Statistik index, schema, profil resource, dan health |

---

## Struktur Project

```
aikd/
├── Cargo.toml                  # Workspace root (v2.0.0)
├── README.md                   # README English
├── README.id.md                # File ini
├── install.sh                  # Installer Linux/macOS
├── install.ps1                 # Installer Windows
├── crates/
│   ├── core/                   # Tipe data, error, config, security, fusion, platform
│   ├── storage/                # SQLite WAL + FTS5 contentless + memory/event/Atlas
│   ├── indexer/                # Orchestrasi index & retrieval
│   ├── embedder/               # Stub zero-ONNX; fastembed opsional (feature semantic)
│   ├── chunker/                # Chunking markdown, teks, source code
│   ├── scanner/                # Shared scan logic (single source of truth)
│   ├── context/                # Handoff & context assembly
│   ├── session/                # Session & memory percakapan
│   ├── server/                 # MCP server + REST API + AIKD Studio (axum)
│   ├── watcher/                # File system watcher (notify)
│   ├── shim/                   # Self-update shim versioned binaries
│   ├── benchmark/              # Benchmark & stress test suite
│   └── cli/                    # Binary CLI (aikd) + hooks
└── extensions/
    ├── chrome/                 # Chrome extension (clip chat/page)
    └── vscode/                 # Ekstensi VSCode (opsional)
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

> Baris **Hybrid Search** hanya berlaku pada build opt-in `semantic` (atau provider embeddings eksternal). Build default tetap 100% Zero-ONNX dan hanya menjalankan BM25.

---

## Pemecahan Masalah

| Masalah | Solusi |
|---------|--------|
| `aikd: command not found` | Tambahkan `~/.local/bin` ke PATH Anda |
| `Config not found` | Jalankan `aikd init` |
| `Model not downloaded` | Build default tidak butuh model. Hanya build `semantic` yang menjalankan `aikd model download all-MiniLM-L6-v2` |
| `No results` | Jalankan `aikd scan` terlebih dahulu |
| `Hybrid search not working` | Build dengan `--features semantic`, atau set `embedding.provider: external` + endpoint; default = LexicalFallback (BM25) |
| `Permission denied` (Linux/macOS) | `chmod +x ~/.local/bin/aikd` |
| `Port already in use` | Ganti `server.rest_port` di config |
| `GPU not used` | GPU hanya relevan untuk build `semantic`; fastembed default CPU |

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
git clone https://github.com/gelutjari/aikd.git
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
| [rusqlite](https://github.com/rusqlite/rusqlite) | Binding SQLite + FTS5 BM25 |
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) | ONNX embedding inference (opsional, feature `semantic`) |
| [hnsw_rs](https://github.com/jeremiedecock/hnsw-rs) | HNSW vector index (opsional) |
| [axum](https://github.com/tokio-rs/axum) | Framework HTTP |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | Protokol MCP |
| [notify](https://github.com/notify-rs/notify) | File system watcher |
| [rayon](https://github.com/rayon-rs/rayon) | Data parallelism |
| [blake3](https://github.com/BLAKE3-team/BLAKE3) | Hashing cepat |

---

<div align="center">

**Dibuat dengan Rust dan dedikasi.**

</div>
