# ADR 0002 — Search backend: Tantivy default, FTS5 second backend

Status: accepted · 2026-11-21

## Konteks

Directive §5.2 meminta evaluasi SQLite FTS5 vs Tantivy dengan benchmark nyata.
Implementasi: migrasi v5 menambahkan `chunks_fts` (contentless FTS5 + trigger sinkronisasi + backfill).

## Benchmark (harness `C:/tmp/bench-fts`, sementara; hasil di `docs/benchmarks/BASELINE.md` menyusul)

Korpus sintetis 1000 file / 4000 chunk (seed implisit, kosakata 20 kata — SANGAT dense):

| | build | query p50 | query p95 | hits/100q |
|---|---|---|---|---|
| Tantivy | 101ms | 0.34ms | 0.50ms | 1000 |
| FTS5 contentless | 30ms | 5.6ms | 6.2ms | 900 |

Data real: DB pengguna, 4196 file / 25.551 chunk, 10 query alami ×5:

| | query p50 | query p95 | hits/50q | top-5 agreement |
|---|---|---|---|---|
| Tantivy | 0.59ms | 1.03ms | 500/500 | — |
| FTS5 | 0.41ms | 1.42ms | 235/500 | 3/10 query berbagi ≥1 chunk |

Ukuran: FTS menambah ±12MB pada DB 78MB (+16%); index Tantivy pengguna 25MB.

## Interpretasi

- Latency query pada data real SETARA (sub-ms p50 keduanya). Angka 5ms FTS pada korpus sintetis adalah artefak kosakata dense (tiap term cocok di hampir semua dokumen → sort ribuan kandidat).
- Recall berbeda karena semantik: FTS implementasi kami AND (semua term wajib), Tantivy QueryParser default OR. Bukan keunggulan mutlak salah satu — pilihan retrieval.
- Ranking berbeda jauh (agreement 3/10): BM25 Tantivy vs bm25() FTS dengan tokenisasi/field berbeda.

## Keputusan

1. **Tantivy tetap backend lexical default**: recall penuh, ranking mapan, sudah terintegrasi incremental (P2-1).
2. **FTS5 dipertahankan sebagai backend kedua** dengan peran: (a) fallback/verify bila index Tantivy rusak (rebuild dari SQLite kapan pun — DB adalah source of truth); (b) fondasi pengganti `LIKE` untuk session recall (P4/P5); (c) kandidat mode minimal tanpa Tantivy.
3. Tidak ada dua index default yang wajib — FTS ikut migrasi v5 (murah, trigger-based), Tantivy tetap satu-satunya yang dibaca jalur query hari ini. Abstraksi `SearchBackend` trait ditunda hingga P4 (Context Engine) agar tidak over-abstract sebelum konsumen kedua ada.
4. OR-semantik untuk FTS dievaluasi di P4 bersama retrieval router.
