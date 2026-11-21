# AIKD Baseline Benchmark (Phase Zero)

Tanggal: 2026-11-21 · Biner: debug (`target/debug/aikd.exe`), workspace commit `a9c2495` + patch P1-1 (belum di-commit — baseline perilaku diukur setelah patch; patch hanya mengubah default embedding & error path, bukan jalur ukur).
Corpus: 200 file markdown sintetis, 1000 chunk (seed 42, diregenerasi via skrip — lihat `docs/benchmarks/` corpus note). Mesin: Windows, x64 (spesifikasi menyusul via sysinfo).

> Kejujuran: angka klaim lama (0.21ms, 27% RAM, BENCHMARK_REPORT.md v1.1.0) TIDAK direproduksi dan TIDAK dipakai. Semua di bawah ini hasil ukur nyata sesi ini, atau ditandai NOT MEASURED.

## Hasil ukur nyata

| Metric | Hasil | Cara ukur |
|---|---|---|
| `cargo check --workspace --all-targets` | PASS (2m12s) | baseline commit, tanpa patch |
| `cargo test --workspace` | PASS 87 / FAIL 0 (unit: 5+13+38+11+8+5+6+1=87) | baseline commit |
| `cargo build --bin aikd` (debug) | PASS | baseline + patch P1-1 |
| `cargo check` setelah patch P1-1 (core/indexer/embedder) | PASS | `cargo check -p aikd-core -p aikd-embedder -p aikd-indexer` |
| `cargo check --workspace --all-targets` setelah patch | PASS, 0 error | — |
| scan awal 200 file / 1000 chunk | 364ms | `aikd scan`, wall terminal |
| no-op scan 200 file | 18ms (+77ms total proses) | `filter_changed` hash; 0 to index |
| single-file update scan | 130ms (+194ms total) | store 5 chunk + tantivy update |
| BM25 query (limit 5, cold process) | ~69ms total proses | `aikd query "authentication token"` |
| BM25 query JSON setelah incremental | OK, hasil valid | — |
| init tanpa model download | PASS — tidak ada unduh, pesan opt-in tampil | patch P1-1 terverifikasi |
| hybrid tanpa model (CLI) | Sekarang `bail!` (error jelas, exit≠0) | patch P1-1; sebelumnya exit 0 diam-diam |

## Bug terkonfirmasi live (bukan dugaan)

- **Incremental scan menghancurkan index**: setelah 1 file berubah, query hanya mengembalikan file itu (dibuktikan: query "authentication" → hanya `mod00/file000.md`). Penyebab: `scanner::update_tantivy` = `clear()` + index file berubah saja.
- Dampak insiden uji: index Tantivy pengguna (`~/.aikd/tantivy_index`) ikut ter-clear oleh scan uji. Mitigasi: data bench sudah dihapus dari `aikd.db` (200 file + chunk + embedding; 4196 file/25551 chunk tersisa), backup pra-kerja di `/tmp/aikd-rescue/` (bash view) dan rebuild penuh Tantivy dari SQLite sedang berjalan.

## NOT MEASURED (alasan)

- Startup cold/warm terisolasi, idle RAM, peak RAM indexing, CPU scan/query, disk I/O: butuh harness khusus (sysinfo sampling) — P2 bersamaan FTS5 vs Tantivy benchmark.
- Semantic/hybrid latency: model ada di `%APPDATA%\aikd\model` tapi sengaja tidak diukur di baseline agar baseline = jalur BM25 default; hybrid diukur saat P9 dengan harness jujur (p50/p95, bukan klaim).
- MCP response latency, REST latency: butuh server harness — P7.
- Release binary size: butuh `cargo build --release` (+unduh ONNX di build) — P10.
- Skala 1k/10k/100k file: corpus 200 file dulu; skala besar butuh waktu/mesin — P2.

## Dataset reproduksi

Corpus sintetis: 200 file × 5 section, kosakata 20 kata, seed 42. Skrip generator + perintah ukur akan dibundel sebagai harness di P2 (`docs/benchmarks/REPRODUCIBILITY.md` menyusul).
