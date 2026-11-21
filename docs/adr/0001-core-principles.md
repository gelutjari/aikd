# ADR 0001 — Core principles (Phase Zero decision)

Status: accepted · 2026-11-21

## Konteks

Audit (`docs/audit/BASELINE_AUDIT.md`) menunjukkan: embedding default ON + auto-download, tanpa feature flags, index rusak saat incremental scan, auth fail-open.

## Keputusan

1. Embedding adalah specialist opsional, default OFF, tanpa auto-download (directive §2.1).
2. Core (scan BM25 + session + MCP/CLI) harus build & jalan tanpa ONNX (directive §12, §20-P1).
3. Tidak ada data-loss refactor: migrasi v5 additive; `clear()`-based index update dilarang; chunk ID menjadi deterministik sebelum P3.
4. Fail-closed security: auth tanpa token = tolak; CORS default loopback-saja.
5. Benchmark jujur: PASS/FAIL/SKIPPED/NOT IMPLEMENTED; SKIPPED ≠ PASS.

## Konsekuensi

- `indexer → embedder` diputus (simbol kecil pindah ke `core`).
- `init` tidak mengunduh model; `query --hybrid` tanpa model = error jelas, bukan exit 0.
- FTS5 vs Tantivy diputuskan lewat benchmark nyata (P2), bukan preferensi.
