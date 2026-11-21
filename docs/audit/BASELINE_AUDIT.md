# AIKD Baseline Audit (Phase Zero)

Tanggal: 2026-11-21 · Commit: `a9c2495` · Branch: `master` (clean, tidak ada perubahan lokal) · Toolchain: rustc 1.97.1.
Metode: baca kode nyata 11 dari 12 crate (semua kecuali plugin yang stub), manifest, Cargo.lock, config, docs. Tidak ada perubahan kode pada fase ini. Klaim performa lama (0.21ms, 27% RAM, BENCHMARK_REPORT.md v1.1.0) tidak direproduksi di sini — lihat `docs/benchmarks/BASELINE.md`.

## 1. Struktur repo & fungsi tiap crate

| Crate | Baris src | Fungsi nyata | Status |
|---|---|---|---|
| `core` | ~1300 | types, config, resource profile, platform, agents registry, fusion RRF, security | `security.rs` (validate/sanitize) **nol pemanggil produksi**; `AikdError` nyaris tak dipakai (semua crate pakai `anyhow`); `Config::validate()` dead |
| `storage` | ~600 | SQLite lifecycle + migrasi v1–v4 | Satu-satunya layer SQLite tapi abstraksi bocor (`conn()` mentah, semua crate tulis SQL sendiri); pool r2d2 tidak dipakai |
| `scanner` | 284 | discover → hash → chunk → store → update tantivy | Nyata; tapi `update_tantivy` = `clear()` + index ulang **hanya file berubah** → index rusak tiap incremental scan |
| `chunker` | ~770 | markdown (pulldown-cmark) + code (heuristik prefix `fn `/`def `/`class `) + text | Nyata; **tanpa tree-sitter** (klaim docs ARCHITECTURE:43 salah) |
| `indexer` | 685 | Tantivy BM25 + VectorIndex/HNSW + RRF hybrid + MmapVectorIndex | Nyata; tapi hasil ANN-only = objek kosong (path/content kosong, indexer:427-437); `line_start/line_end` selalu 0 dari Tantivy |
| `embedder` | 439 | download model HF + create_model + embed_and_store + cache LRU | Nyata; `reqwest` blocking+async ganda; `MODEL_NAME/DIMENSIONS/bytes_to_f32` di sini padahal dibutuhkan indexer (coupling) |
| `session` | ~240 | raw chat CRUD + recall `LIKE` | Nyata tapi write-only embedding, `chunk_refs` selalu kosong, tanpa lifecycle/provenance |
| `server` | ~1370 | 7 MCP tools + 13 REST routes + JWT + metrics + CORS + governor | Nyata; auth fail-open, CORS `*`, 401 jadi 200, `validate_jwt` dead, `/api/query/stream` stub |
| `watcher` | 208 | notify loop + debounce + batch index | Nyata; unbounded, tanpa signal handler, filter tidak lengkap, Tantivy `clear()` total |
| `cli` | 1364 | 20 subcommand | Nyata; `daemon` background broken (spawn arg `daemon --foreground` padahal subcommand = `Foreground`), `embed --model/--batch` diabaikan, UTF-8 panic, `inject` stdin tidak di-close |
| `benchmark` | ~1000 | 8 skenario + CLI runner | Nyata tapi metrik lemah (sampel akhir bukan peak, skip=PASS, tanpa p50/p95) |
| `plugin` | 43 | 5 DTO + 6 konstanta | **Stub yatim** — nol crate bergantung |

## 2. Dependency: wajib vs opsional vs mati

- **Tanpa feature flags sama sekali** (grep `[features]`/`optional` = 0). Biner `aikd` selalu link ONNX/fastembed + Tantivy + axum + rmcp + benchmark.
- **Terberat**: `fastembed 5.16.2` → `ort 2.0.0-rc.12` → `ort-sys` (unduh biner ONNX Runtime saat build) + `tokenizers 0.22.2` (onig native) + `image 0.25` (ravif/rav1e/exr/tiff — tak dipakai) + `hf-hub 0.5`. Ditarik 5 crate karena `indexer` butuh 2 simbol kecil dari `embedder`.
- **Mati/menganggur**: `globset`, `env_logger` (workspace); `tower`, `futures`, `tokio-stream`, `once_cell`, `governor` (server); `r2d2`, `r2d2_sqlite` (pool tak dipakai); `aikd-plugin` (yatim); duplikat versi `dirs 5/6`, `indicatif 0.17/0.18`, `schemars 0.8/1.x`, `thiserror 1/2`, `base64 0.21/0.22`.
- **CLI menarik `aikd-server` + `aikd-benchmark` ke biner produksi** — benchmark seharusnya dev-tool.
- **Config melanggar embedding-optional**: `embedding.enabled` default `true` (config.rs:57,157), `ResourceMode::Auto` mengaktifkan embedding di RAM ≥2GB, `aikd init` selalu unduh ±90MB tanpa gate (main.rs:347-358). Seluruh `EmbeddingConfig` (model/batch/device) tidak punya konsumen.
- **Config tidak versioned/validated/env**: `version` kosmetik, `validate()` dead, klaim `AIKD_*` di docs tidak diimplementasikan, `resource.max_memory_mb` di docs tidak ada di struct (diam-diam diabaikan).

## 3. Beban runtime & build

- Runtime: model ONNX dimuat **per request** (CLI query, MCP tool, REST dengan lock DB dipegang); `ResourceProfile::detect` (sysinfo all + spawn `nvidia-smi`) dipanggil tiap `status`; migrasi DDL jalan tiap `Database::open` termasuk tiap MCP tool call; REST pakai `Mutex<Database>` → semua akses terserialisasi satu koneksi.
- Build: `ort-sys` unduh biner saat build; Dockerfile rapuh (rust:1.82, tanpa `--locked`, tanpa libssl-dev padahal openssl-sys ada, runtime tanpa lib ONNX).
- Scan: file dibaca 2–3× (hash, chunk, hash lagi); `store_chunks` satu transaksi raksasa; `embed_and_store` commit per baris; embeddings dihapus saat rescan meski embedding off.

## 4. Duplikasi

D1 enrich line number 3× (mcp 342-376, cli 1007-1036, storage 251-283 mati). D2 statistik COUNT 3× + helper mati. D3 urutan hapus embeddings→chunks→files 3× (scanner, watcher, benchmark). D4 filter SearchFilters 2× (Tantivy + storage, tidak di ANN). D5 orkestrasi hybrid 3× (cli, mcp, rest). D6 tabel profil resource 2×. D7 exclude path 2×. D8 JwtConfig 2×. Duplikasi SQL insert 3× (scanner, benchmark ×2). `ChunkConfig` ganda (core vs chunker). `PluginResult` duplikat `SearchResult`.

## 5. Dead code & tabel mati

`core::security` (2 fn), `validate_jwt`, `Database::load_chunks` (satu-satunya hidrasi ANN — tak dipanggil), pool API, helper statistik storage, `ConversationEmbedding`, `DEFAULT_RRF_K`/`fusion_multi`, field config mati (include_files, exclude_extensions, overlap_tokens, compute_threads, cache_size_mb, seluruh EmbeddingConfig), field profil mati (hnsw_*, tantivy_heap_mb, watcher_debounce_ms). Tabel `code_symbols` + `audit_log` tanpa reader/writer. Kolom `files.status` selalu 'active'; `metadata_json`/`chunk_refs` write-only; `conversation_embeddings` write-only. Index redundan: `idx_files_path` (UNIQUE sudah ada), `idx_files_status`, `idx_chunks_heading`, `idx_chunks_line_range`, `idx_files_modified`.

## 6. Database & storage

- SQLite satu-satunya store; WAL + NORMAL + FK ON + busy_timeout 5000 + cache -64MB + mmap 256MB. Pool customizer tidak set cache/mmap → karakteristik beda.
- Migrasi v1–v4: tidak transaksional, `schema_version` tanpa PK, tanpa guard versi-lebih-baru, `migrate_v1` pernah diedit (drift), `ALTER ADD blake3_hash` di v3 padahal v1 sudah punya.
- FK tanpa `ON DELETE CASCADE` (chunks, embeddings, conversations, conversation_embeddings) → hapus manual berurutan di 3 tempat; `aikd session delete` bisa gagal FK bila embedding ada.
- Tanpa FTS5 (grep MATCH/fts5 = 0 jalur SQL); `session::recall` = `LOWER(content) LIKE '%q%'` full-scan, tidak Unicode-safe.
- Konten disimpan ±2× (SQLite `chunks.content` penuh + Tantivy STORED) + BLOB vektor mentah tanpa kuantisasi. `files.modified_at` diisi waktu indexing bukan mtime. Format waktu campur (RFC3339 chrono vs `datetime('now')`).

## 7. Alur embedding (bukti baris)

- `init` → `download_model` tanpa gate (cli:347-358). Model: 5 file HF (~90MB) ke `~/.aikd/model` (Win: `%APPDATA%\aikd\model`).
- `query --hybrid` → `create_model` per panggilan (cli:689-714); MCP `query` sama (mcp:128-180); REST sama di dalam lock (rest:380-407, batch 32 hardcode).
- `embed` → `embed_and_store` (batch dari profil/CLI; MCP/REST hardcode 32). Tanpa model: CLI/MCP/REST return pesan "Model not downloaded", BM25 tetap jalan (fallback parsial).
- Offline/tanpa model: core BM25 + scan + session + MCP/REST tetap bisa jalan **secara teknis**, tapi biner tetap link ONNX dan `init` memaksa unduh. Jawaban §2.1: **belum memenuhi** — embedding bukan core requirement secara de-facto, tapi secara default/build masih wajib.

## 8. Alur pencarian

BM25 (`tantivy.search`, QueryParser atas content+heading+path, `limit*2` lalu filter di Rust) → opsional HNSW (`VectorIndex::load_from_db` tiap request; `MmapVectorIndex` ada tapi tak dipakai jalur produksi) → RRF k=60 (hardcode; `DEFAULT_RRF_K` mati; tanpa tie-break deterministik). Filter `SearchFilters` hanya di BM25 + storage, tidak di ANN. Skor/urutan ANN hilang (`load_chunks` mati). `line_start/end` = 0 di semua hasil Tantivy; REST tanpa enrichment.

## 9. Alur MCP

`run_server` (server/lib.rs) → rmcp stdio; tiap tool buka `Database` + `TantivyEngine` baru; `AppState` hanya simpan `Config`. 7 tools: scan/query/embed/stats/remember/recall/status. Error = String (isError tidak diset). Blocking `run_scan` di handler async. Tidak ada resource/prompt, tidak ada tool delete/session/export/import/benchmark.

## 10. Kekurangan memory (session)

Raw chat (`role/content/tokens`), tanpa facts/decisions/constraints/task/discovery/unresolved/checkpoint. Tanpa provenance (`chunk_refs` selalu `[]`), tanpa lifecycle (status/validity/superseded), tanpa confidence, tanpa project scope benar (fallback `""` menyatukan semua pemanggil tanpa path). Recall LIKE tanpa FTS/ranking. FK delete bug. `get_or_create_session` race tanpa UNIQUE/transaksi.

## 11. Masalah keamanan (ringkas, bukti di §8 audit interfaces)

Fail-open auth (rest:283-292), CORS `*` default, 401→200, path traversal (`/api/scan` + MCP scan tanpa sanitize; `core::security` mati), secret files ikut ter-index (exclude minim: hanya `.env,*.bak,*.tmp,*.secret`; ekstensi default termasuk json/yaml/toml secrets), login `admin`/plaintext + non-constant-time + JWT fallback hardcoded, metrics/health tanpa auth, rate-limit 1rps vs klaim 10rps, UTF-8 panic (cli/mcp), `inject` stdin hang, embedding di dalam lock DB, `agents.rs` tulis config editor non-atomik tanpa backup.

## 12. Pembengkakan DB

Konten ganda SQLite+Tantivy; vektor BLOB penuh per chunk + per conversation; rescan menghapus embeddings; tanpa vacuum/retensi; `benchmark` duplikat insert; Tantivy di direktori terpisah tanpa backup terkoordinasi.

## 13. CPU/RAM/disk/startup

Belum diukur di fase ini (lihat BASELINE.md). Risiko dari kode: model load per request (peak RAM berulang), sysinfo+nvidia-smi per status, scan baca file 2–3× + par_iter seluruh core, transaksi raksasa, mutex DB terserialisasi, watcher unbounded, migrasi DDL per open.

## 14. Fitur yang layak dipertahankan

Scan incremental (hash), chunk markdown/code, BM25 Tantivy, RRF fusion, session CRUD dasar, MCP 7 tools, REST inti, watcher mekanisme, export/import JSON, benchmark runner, migrasi v1–v4 (jangan dihapus sebelum v5), `generate_smart_config` exclude dasar.

## 15. Yang perlu disederhanakan → specialist → hapus

- Sederhanakan: orkestrasi hybrid 3× → satu service; enrich 3× → satu; hapus manual 3× → `delete_file_cascade`; config versioned + `validate()` dipanggil; pool dipakai atau dibuang.
- Jadikan specialist: embedding (feature-gate, default OFF), semantic conversation, benchmark (keluar dari CLI), plugin (isi atau hapus).
- Hapus setelah pengganti terbukti: `clear()`-based index update (ganti delete_term per file), CORS `*`, auth fail-open, kolom/tabel mati, dep menganggur, duplikat versi, `MmapVectorIndex` bila tak dipakai, `/api/query/stream` stub, `code_symbols`/`audit_log` (atau implementasikan benar).

## 16. Risiko register (top)

1. Incremental scan/watcher **menghancurkan index BM25** (data loss fungsional) — P0.
2. Auth fail-open + CORS `*` + scan arbitrer (secret exfiltration lokal) — P0.
3. `daemon` background broken — P0.
4. FK tanpa cascade + migrasi non-transaksional + race sesi — P0/P1.
5. Biner selalu link ONNX; build unduh biner; Dockerfile rapuh — P1.
6. Hybrid kembalikan objek kosong; skor/urutan hilang — P1.
7. Benchmark klaim palsu (skip=PASS, bukan peak) — akurasi laporan — P1.

## 17. Temuan tambahan audit search/embed (AuditSearchEmbed)

- **Chunk kode < `min_tokens` hilang**: `chunker/src/code.rs:108-124` — bila chunk simbol di bawah `min_tokens` (default 100), chunk tidak di-push tapi `current_content.clear()` tetap jalan. Fungsi/struct pendek hilang dari indeks. Kritis.
- **Chunk ID acak**: `Uuid::new_v4()` di 4 konstruktor (`chunker/lib.rs:73,88,120`, `code.rs:242`, `markdown.rs:186`) → tiap rescan ID berubah, embedding lama selalu invalid. Tidak ada content-hash chunk.
- **HNSW dirty race**: `dirty` di-CAS ke `false` sebelum rebuild (`indexer:134-150`) → query konkuren bisa lihat cache `None` dan dapat hasil vektor kosong.
- **`files_skipped` salah arti**: `run_scan` hitung `files_found - indexed` (`scanner:263-264`); no-op scan lapor `files_skipped=0` padahal semua file unchanged.
- **Structured chunker tanpa batas**: JSON/YAML/TOML = satu chunk seluruh file (`chunker/lib.rs:119-140`); file besar melebihi batas tokenizer.
- **Model download tanpa checksum/resume/atomic rename**; URL `resolve/main` tidak reproducible; file parsial dianggap valid (`embedder:97-101`).
- **QueryParser gagal untuk input umum** (`C++`, `foo:bar`, kutip tak seimbang) — error diteruskan ke caller (`indexer:294`).
- **Dead**: `MmapVectorIndex` (indexer:447-560), `EmbeddingCache` (hanya test), `vector_search`/`load_all_embeddings` (hanya benchmark), `index_chunks_with_heap`/`clear_with_heap`/`with_hnsw_params`/`memory_usage`, `scanner::chunk_files`, `reciprocal_rank_fusion_multi`.
- **Docs drift**: README:168 klaim scan hasilkan embedding (salah — scan berhenti di SQLite+Tantivy); ARCHITECTURE:43 klaim tree-sitter (salah — heuristik prefix); klaim "vector index stored in SQLite" (yang disimpan hanya vektor, HNSW tidak).
- **Biaya hybrid per request** (bila model ada): baca ±90MB ONNX + init ORT + baca seluruh row embeddings + build HNSW penuh — setiap query. Tanpa persistensi/cache.
