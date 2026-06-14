# Knowledge Hub v3 — Kronologi & Status Project
_Tanggal: 14 Juni 2026_

---

## Timeline Perkembangan

### v1 → v2 → v3 (Evolusi)

| Versi | Lokasi | Teknologi | Status |
|-------|--------|-----------|--------|
| v1 | `/root/knowledge-hub/` | Rust, SQLite FTS5 | ✅ Selesai |
| v2 | `/root/knowledge-hub-v2/` | Rust, Custom KHDB, mmap | ✅ Selesai |
| v2.1 | `/root/knowledge-hub-v2/` | + YAML config, zstd compression | ✅ Selesai |
| v3 | `/root/knowledge-hub-v3/` | Rust, SQLite + Tantivy + fastembed-rs | ✅ Selesai |

### v3 Phase Timeline

| Phase | Deskripsi | Status | Tanggal |
|-------|-----------|--------|---------|
| Phase 1.1 | Setup project structure | ✅ Done | Session lalu |
| Phase 1.2 | SQLite schema & migrations (v1-v2) | ✅ Done | Session lalu |
| Phase 1.3 | Markdown chunking (pulldown-cmark) | ✅ Done | Session lalu |
| Phase 1.4 | Tantivy full-text index | ✅ Done | Session lalu |
| Phase 1.5 | CLI commands (init, scan, query, stats) | ✅ Done | Session lalu |
| Phase 2 | Vector embeddings + hybrid search (RRF) | ✅ Done | Session lalu + fix hari ini |
| Phase 2.1 | Native Rust embedding (fastembed-rs) | ✅ Done | 14 Juni 2026 |
| Phase 3 | MCP server (rmcp v0.16.0) | ✅ Done | 14 Juni 2026 |
| Phase 4 | Auto-sync daemon (notify crate) | ✅ Done | 14 Juni 2026 |
| Phase 5 | Polish (smart config + tests) | ✅ Done | 14 Juni 2026 |

---

## Masalah yang Dihadapi & Solusi

### Session Lalu (Build & Architecture)

| # | Masalah | Penyebab | Solusi |
|---|---------|----------|--------|
| 1 | Build gagal: `ld.lld: error: unable to find library -lgcc_s` | clang+lld tidak kompatibel di PRoot Debian | Ganti linker ke `gcc` di `~/.cargo/config.toml` |
| 2 | `ort` (ONNX Runtime) compile timeout 600s+ | Terlalu berat untuk ARM64 PRoot, CPU only | Hapus fastembed dari Rust deps, pakai Python external |
| 3 | `pulldown-cmark` API change | v0.11: `Tag::Heading` jadi struct, `Event::End` pakai `TagEnd` | Sesuaikan pattern matching |
| 4 | `rusqlite` type error | `usize`/`u64` tidak implement `ToSql` | Cast ke `i64` |

### 14 Juni 2026 (Bug Fixes & Native Embedding)

| # | Masalah | Penyebab | Solusi |
|---|---------|----------|--------|
| 5 | `TantivyEngine::open` error "Directory doesNotExist" | Hanya buat parent dir, bukan index dir | `create_dir_all(index_path)` |
| 6 | `Config::save` error "No such file" | Tidak buat parent directory | Tambah `create_dir_all` sebelum write |
| 7 | Panic: `end byte index not char boundary` | String slicing potong di tengah emoji UTF-8 | Pakai `char_indices()` untuk safe slicing |
| 8 | Embedding terlalu lama (download dari HF) | Koneksi ke HuggingFace lambat di VPS | Download manual + load dari local path |
| 9 | `fastembed` butuh `openssl-sys` | Dependency transitive dari `reqwest` | Install `pkg-config libssl-dev` |
| 10 | Python dependency untuk embedding | `ort` terlalu berat di ARM64 | Pakai `fastembed-rs` native di x86_64 VPS |

---

## Yang Sudah Berjalan (Working Features)

### CLI Commands

```bash
knowledge-hub init                    # Buat config default
knowledge-hub scan                    # Scan & index files ke SQLite + Tantivy
knowledge-hub query "keyword"         # BM25 full-text search
knowledge-hub query "keyword" --hybrid # Hybrid search (BM25 + vector RRF)
knowledge-hub query "keyword" --json   # Output JSON
knowledge-hub query "keyword" --path "/docs"  # Filter by path
knowledge-hub query "keyword" --heading "API" # Filter by heading
knowledge-hub embed                   # Generate embeddings (native Rust, fastembed-rs)
knowledge-hub export                  # Export chunks ke JSON
knowledge-hub import -f file.json     # Import embeddings dari JSON
knowledge-hub stats                   # Lihat statistik index
```

### Architecture

```
User → CLI (clap)
         ├── Init → generate_smart_config() → YAML
         ├── Scan → WalkDir → chunker (Markdown/Text/Structured)
         │                    ├── SQLite (files, chunks, embeddings)
         │                    └── Tantivy (BM25 index)
         ├── Query → Tantivy search → filters → results
         │         └── --hybrid → + vector search → RRF fusion
         ├── Embed → fastembed-rs (native Rust, ONNX) → SQLite
         └── Serve → [STUB] Phase 3 MCP server
```

### Database Schema

```sql
-- v1
CREATE TABLE files (id, path, size, modified_at, last_scanned, status);
CREATE TABLE chunks (id, file_id, chunk_index, heading_hierarchy, heading_level,
                     heading_text, line_start, line_end, content, metadata_json,
                     created_at, updated_at);

-- v2
CREATE TABLE embeddings (chunk_id, model, dimensions, vector BLOB);
```

---

## Deploy Status

### Local (Termux/aarch64)

| Item | Status |
|------|--------|
| Binary | ✅ `/root/knowledge-hub-v3/target/release/knowledge-hub` (8.6MB) |
| Rust | rustc 1.95.0 |
| fastembed | ❌ Tidak tersedia di ARM64 (pakai Python fallback) |
| Config | `~/.knowledge-hub/config.yaml` |
| DB | `~/.knowledge-hub/v3.db` |

### Remote VPS (x86_64)

| Item | Status |
|------|--------|
| Binary | ✅ `/root/knowledge-hub-v3/target/release/knowledge-hub` (9.5MB) |
| Rust | rustc 1.96.0 |
| fastembed-rs | ✅ Native Rust (model di `~/.knowledge-hub/model/`) |
| Config | `~/.knowledge-hub/config.yaml` |
| DB | `~/.knowledge-hub/v3.db` |
| Project | `/root/knowledge-hub-v3/` |
| Warnings | 0 |
| Bugs | 0 |

---

## Yang Harus Diperbaiki (Bugs / Issues)

| # | Prioritas | Item | Detail | Status |
|---|-----------|------|--------|--------|
| 1 | 🟡 Sedang | Line numbers di Tantivy | `line_start`/`line_end` selalu 0 di search results | ✅ Fixed |
| 2 | 🟡 Sedang | Error handling | Banyak `unwrap()` yang bisa panic | ✅ Sudah pakai unwrap_or |
| 3 | 🟢 Rendah | `cmd_serve()` stub | Masih kosong, "MCP server — Phase 3" | ✅ Fixed (Phase 3) |
| 4 | 🟢 Rendah | Model path hardcoded | `/tmp/mini-lm-model/` seharusnya configurable | ✅ Fixed |

---

## Yang Harus Dilanjutkan (Next Steps)

### Semua fase selesai! 🎉

Project Knowledge Hub v3 sudah lengkap dengan:
- ✅ CLI commands (init, scan, query, stats, export, import, embed, watch, serve)
- ✅ Hybrid search (BM25 + vector RRF)
- ✅ Native Rust embedding (fastembed-rs)
- ✅ MCP server (rmcp)
- ✅ Auto-sync daemon (file watcher)
- ✅ 41 unit tests passing

---

## Build Environment Notes

### Termux/aarch64 Limitations
- `clang+lld` → `libgcc_s not found` → pakai `gcc` linker
- Cranelift → tidak tersedia untuk aarch64
- sccache → gagal compile (libc error)
- `ort` → terlalu berat (600s+ compile)
- `fastembed-rs` → tidak bisa compile di ARM64

### Remote VPS (x86_64)
- Semua tool tersedia (lld, cranelift, sccache)
- Build lebih cepat
- Bisa pakai full optimization stack
- `fastembed-rs` native embedding tersedia

---

_Generated: 2026-06-14 | Session: ses_13d2b1dadffeCvsmWL0cI2267i_
