# ADR 0004 — Zero-ONNX default, canonical paths, Project Atlas

Status: accepted · 2026-11-21 · Phase 2.

## Zero-ONNX

Default build menautkan **tanpa** `ort/ort-sys/fastembed/tokenizers/ndarray`
(verifikasi: `cargo tree -p aikd-cli`, nol baris ONNX; `portable-atomic`
yang tersisa adalah dep umum, bukan rantai ONNX).

- `core::vector`: fungsi murni (cosine, top-K, IO SQLite, import/export
  JSON, `EmbeddingCache` std-Mutex) + trait `SemanticProvider` — AIKD
  memiliki kontrak, provider hanya mengisi inferensi.
- `aikd-embedder`: `fastembed/reqwest/tokio` opsional di belakang feature
  `semantic` (default OFF). Stub gagal jujur: tidak download, tidak
  inferensi, pesan mengarah ke `--features semantic` atau `aikd import`
  (jalur zero-ONNX untuk vektor eksternal: export → provider → import).
- `context::try_hybrid` non-semantic → `LexicalFallback` eksplisit;
  `session::embed_conversations` non-semantic → error jujur.
- Benchmark embedding: SKIP (bukan FAIL) di build zero-ONNX.

Vektor tersimpan tetap terbaca tanpa ONNX (`load_all_embeddings`,
`import/export`, cache) — hanya *pembuatan* vektor yang butuh provider.

## Canonical paths

Satu ejaan (`/` di semua platform) untuk semua kunci DB/index:
`core::platform::{canonical_path, canonical_project}`, dipakai scanner
(discover/filter/chunk/store), watcher, CLI project resolve, dan semua
identitas project (memory/event/MCP/REST). Migrasi v11 menggabung baris
duplikat lama (pertahankan MIN(id), cascade chunks) lalu rewrite.
Terverifikasi pada DB v10 warisan: 6 baris duplikat → 3, integrity OK.

## Project Atlas

Snapshot JSON per project dari tabel kanonis saja (files/chunks/
code_symbols/memory/events). Fingerprint isi (bukan mtime) → stabil
lintas rescan. Status fresh/stale/empty; INFERRED vs VERIFIED eksplisit.
Artefak mesin (10 JSON) + manusia (9 MD) dari snapshot yang sama.
CLI: `project analyze|show|export`. Diuji live pada proyek demo.

## Memory governance

v10: status `stale/archived/conflicted` + `conflict_note` (CHECK tidak
bisa diubah → rebuild tabel + copy). `stale/archive/flag_conflict/
conflicts` di storage; CLI + MCP (`memory_lifecycle`, `memory_conflict`).
Konflik mempertahankan kedua sisi; tak ada konversi diam-diam ke fakta.

## Context

Satu konstruktor `context::assemble_handoff` untuk CLI+MCP (duplikat
dihapus) + `inspect_context` + CLI `context inspect` + estimasi token.
`/ui` menampilkan lifecycle, konflik, dan snapshot Atlas — semua live.

## Resource

`scan` membatasi pool rayon ke `profile.parallelism`; batch embed MCP/REST
dari profil; `aikd doctor` melaporkan RSS aktual, ukuran, freshness,
integritas, dan peringatan yang bisa ditindaklanjuti.
