# AIKD Dependency Map (Phase Zero)

Sumber: Cargo.toml workspace + 12 crate manifest + Cargo.lock (5665 baris). Tanpa feature flags — graf statis tunggal.

## Graf antar-crate

```
cli ──┬── core ── rusqlite(bundled, via error) · sysinfo · serde_yaml · dirs
      ├── storage ── core · rusqlite · r2d2/r2d2_sqlite(mati) · blake3
      ├── indexer ── core · embedder(!!!) · tantivy · hnsw_rs · anndists · rusqlite · memmap2
      ├── embedder ── core · fastembed(ort/tokenizers/image/hf-hub) · reqwest(blocking+async) · lru · parking_lot
      ├── chunker ── core · pulldown-cmark
      ├── session ── core · storage · embedder(berat, jarang dipakai)
      ├── server ── core · storage · indexer · embedder · scanner · session · axum · rmcp · prometheus · moka · jsonwebtoken · (+tower/futures/tokio-stream/once_cell/governor mati)
      ├── watcher ── core · storage · indexer · chunker · scanner · notify
      ├── scanner ── core · storage · indexer · chunker · walkdir · rayon
      └── benchmark ── core · storage · indexer · embedder · chunker (+reqwest/tokio/tracing/tempfile non-dev)
plugin ── (yatim: nol pemakai)
```

## Coupling bermasalah

1. `indexer → embedder` hanya untuk `DIMENSIONS` + `bytes_to_f32` (indexer:178,185) → BM25-only build mustahil tanpa ONNX. Pindahkan ke `core`.
2. `session → embedder` penuh hanya untuk `embed_conversations` (write-only). Turunkan ke trait/interface.
3. `cli → server + benchmark` → biner produksi bawa axum+rmcp+bench. Benchmark jadi bin terpisah/dev-tool; server di belakang feature.
4. `core → rusqlite` hanya untuk varian error → pindah ke storage / opaque.
5. Semua crate → `storage.conn()` mentah → SQL tersebar (scanner, watcher, session, benchmark, cli, server). Butuh DAO boundary.

## Rantai berat (dari Cargo.lock)

- `fastembed 5.16.2` (:1323) → `ort 2.0.0-rc.12` (:2961) → `ort-sys` (:2973, download-binaries) + `ureq`; → `tokenizers 0.22.2` (:4474, onig native); → `image 0.25` (:2065, ravif/rav1e/exr/tiff — tak dipakai); → `hf-hub 0.5` (:1745, native-tls+reqwest+indicatif 0.18+dirs 6).
- `tantivy 0.22.1` (:4217) — lz4, memmap2, rayon, regex. Diperlukan untuk BM25.
- `rmcp 0.16 + rmcp-macros` (:3633) → `schemars 1.2.1` (duplikat `schemars 0.8` workspace).
- `axum 0.8 + tower-http + tower_governor + prometheus + moka + jsonwebtoken` — hanya untuk serve/daemon.
- `sysinfo 0.34` (:4177) → windows 0.57/ntapi/objc2 — hanya untuk hitung RAM di `resource.rs:32`.
- Duplikat versi: dirs 5/6, indicatif 0.17/0.18, schemars 0.8/1.2, thiserror 1/2, base64 0.21/0.22, windows 0.57/0.61, reqwest native-tls+rustls.

## Circular dependency

Tidak ada sirkular antar-crate (graf DAG satu arah menuju core/storage). Sirkular logis: `scanner ↔ watcher` via fungsi (watcher memanggil `scanner::update_tantivy`), dan `server → scanner → indexer → embedder` memaksa seluruh stack ada untuk satu tool MCP.

## Modul yang terlalu tahu implementasi lain

- CLI/MCP/REST masing-masing tahu urutan hybrid (model → VectorIndex → RRF → fallback) — seharusnya satu service.
- CLI/MCP/REST tahu SQL statistik dan enrich line number — seharusnya storage/service API.
- Watcher tahu urutan hapus DB + update Tantivy — seharusnya satu `delete_file_cascade`.
- Session test memanggil `schema::run_migrations` langsung — schema pub tanpa boundary.

## Arah target (§4 directive)

```
Interfaces (cli/mcp/rest/gui)
  → Application services (scan/query/memory/context — SATU implementasi)
    → Core contracts (types/config/traits)
      → Implementations (tantivy/fts5, tree-sitter, store)
        → Storage adapters (sqlite)
```

Langkah: (a) pindah konstanta kecil ke core; (b) feature-gate embedder/server/benchmark; (c) satu service layer untuk query/memory/stats; (d) DAO boundary storage; (e) hapus/mati-dep cleanup.
