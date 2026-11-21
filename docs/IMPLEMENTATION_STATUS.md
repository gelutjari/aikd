# AIKD Implementation Status (single source of truth)

Mulai: 2026-11-21 · Baseline commit: `a9c2495` (clean tree).

## P0 — Audit & baseline

| Item | Status | Bukti |
|---|---|---|
| Baseline audit | DONE | `docs/audit/BASELINE_AUDIT.md` (§1–17) |
| Dependency map | DONE | `docs/audit/DEPENDENCY_MAP.md` |
| Baseline benchmark | IN PROGRESS | `cargo check` berjalan; `docs/benchmarks/BASELINE.md` menyusul |
| Risk register | DONE | audit §16 (7 risiko top) |

Temuan P0 (wajib diperbaiki sebelum klaim apa pun): incremental scan menghancurkan index BM25; auth fail-open + CORS `*` + scan arbitrer; daemon background broken; chunk < min_tokens hilang; hybrid ANN-only kosong; model auto-download di `init`; tanpa feature flags.

## P1 — Core simplification

| Item | Status |
|---|---|
| Embedding default OFF + no auto-download + lazy load | NOT STARTED |
| Putus `indexer → embedder` (pindah DIMENSIONS/bytes util ke core) | NOT STARTED |
| Feature-gate embedder/server/benchmark | NOT STARTED |
| Verifikasi CLI/MCP tanpa semantic | NOT STARTED |

Rencana slice pertama (kecil, aman, tanpa breaking): (1) hapus auto-download dari `cmd_init` + default `embedding.enabled=false`; (2) pindah `DIMENSIONS`/`f32_to_bytes`/`bytes_to_f32` ke `aikd-core`, hapus dep `indexer→embedder`; (3) verifikasi `cargo check` + scan/query BM25 offline.

## P2 — Storage & search

| Item | Status |
|---|---|
| Perbaiki `update_tantivy` incremental (delete-by-term, bukan clear) | NOT STARTED |
| Satu service query dipakai CLI/MCP/REST | NOT STARTED |
| FTS5 vs Tantivy benchmark | NOT STARTED (FTS5 belum dipakai sama sekali) |
| `delete_file_cascade` + migrasi v5 (cascade, UNIQUE sesi, guard versi) | NOT STARTED |
| DB diagnostics/backup/compact (`aikd db …`, `aikd gc`) | NOT STARTED |

## P3–P10

Belum dimulai. Pintu masuk P3 (symbol index) bergantung pada chunk ID deterministik — tanpa itu relasi symbol↔chunk tidak stabil lintas rescan.

## Aturan pelaporan

Status hanya: PASS / FAIL / SKIPPED / NOT IMPLEMENTED. SKIPPED tidak dihitung PASS. Tidak ada benchmark palsu.

## Insiden uji (2026-11-21, sudah dimitigasi)

- Scan uji P1 (200 file sintetis) tanpa sadar memakai DB pengguna `~/.aikd/aikd.db` + bug `clear()` menghapus index Tantivy pengguna.
- Mitigasi: 200 file uji + chunk + embedding dihapus dari DB (4196 file/25551 chunk tersisa, 0 bench); backup pra-kerja di `/tmp/aikd-rescue/`; Tantivy di-rebuild penuh dari SQLite (25.551 chunk) dan query terverifikasi kembali lintas-file.
- Akar masalah: `--config` BEKERJA, tetapi `db_path`/`tantivy_path` default selalu global (`~/.aikd/`) — tidak ada isolasi data per config. Semua uji berikutnya WAJIB config terisolasi dengan db_path eksplisit; P2 perlu mode test terisolasi.

## P2-1 DONE (2026-11-21, commit 6724f6a)

- Incremental scan/watcher tidak lagi `clear()` seluruh index: `delete_chunks` per chunk_id + `update_tantivy_incremental` + `remove_from_tantivy`.
- Regression test `test_delete_chunks_keeps_other_docs` (indexer 9/9 PASS); verifikasi live terisolasi: update 1 file, file lain tetap ketemu.
- Sisa P2 (belum mulai): search backend abstraction + FTS5-vs-Tantivy benchmark; `delete_file_cascade` + migrasi v5; `aikd db …` + `aikd gc`; mode config terisolasi untuk uji.
