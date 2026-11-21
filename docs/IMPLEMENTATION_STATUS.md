# AIKD Implementation Status (single source of truth)

Mulai: 2026-11-21 · Baseline commit: `a9c2495`.

## P0 — Audit & baseline: DONE

- `docs/audit/BASELINE_AUDIT.md` (§1–17), `DEPENDENCY_MAP.md`, `docs/benchmarks/BASELINE.md`, ADR-0001.
- Baseline terukur: `cargo check` PASS, 87 test PASS / 0 FAIL.

## P1 — Semantic opt-in: DONE (`c1bb1a6`)

- `embedding.enabled` default OFF; `init` tanpa auto-download; gate di CLI/MCP/REST; hybrid tanpa model = error jelas; `core::vector` pisahkan `indexer→embedder`; MCP terverifikasi tanpa model.

## P2 — Storage & search: DONE (`6724f6a`, `f421e33`)

- Incremental index tanpa `clear()` (+regression test); migrasi v5 (FTS5 + trigger + UNIQUE sesi + guard versi); `aikd db status|check|backup|restore|compact`, `aikd gc`; ADR-0002.
- Insiden uji teratasi: data bench dibersihkan, index pengguna di-rebuild (25.551 chunk), `--config` tidak mengisolasi `db_path` (didokumentasikan).

## S1 — REST security: DONE (`764829b`)

- Fail-closed auth, metrics di belakang auth, ApiError (status benar), rate limiter diperbaiki (ConnectInfo), sanitasi path scan, CORS localhost-only, secret excludes. Verifikasi live: 401s, token 200, traversal 400.

## S2 — Daemon/UTF-8/hybrid guard: DONE (`ca7ecaf`)

- Background daemon diperbaiki (REST-only, lifecycle start/pid/stop terverifikasi); UTF-8 char-safe; hybrid tanpa provenance dibuang + fallback BM25.

## S3 — Cascade deletes: DONE (`c45de50`)

- `storage::cascade` dipakai scanner/watcher/session-delete; bug FK session-delete terverifikasi sembuh.

## S4 — Correctness & cleanup: DONE (`6715674`)

- Chunk ID deterministik (blake3); bug min_tokens (merge, bukan buang); hapus MmapVectorIndex, validate_jwt, stream stub, 8 dep mati. 15/15 chunker.

## P3 — Symbol index: DONE (`5f7ebc1`)

- `chunker::symbols` (scan isi baris, EXTRACTED, keyword ≤4 token, komentar dilewati); migrasi v6 (line_start/line_end); scanner mengisi `code_symbols`; CLI `aikd symbol` + MCP `symbol` dengan provenance file:line. Verifikasi live: `struct Vehicle 6-6`, `impl 10-10`.

## P4 — Unified retrieval: DONE (`3123d26`)

- Crate baru `aikd-context`: satu `run_query` untuk CLI/MCP/REST; lexical default, hybrid opt-in + `LexicalFallback` eksplisit; `ContextBudget` (max results + max chars, truncation dilaporkan). Duplikasi 3× dihapus. Verifikasi live CLI+MCP.

## P5 — Structured memory: DONE (`979e150`)

- Migrasi v7 (`memory`: kind/source/status/confidence/workstream/supersedes); `storage::memory` (save/list/supersede/active_tasks/latest_checkpoint); CLI `aikd memory save|list|supersede|task`; MCP memory_save/memory_list/memory_task (10 tools). Verifikasi live: supersede menutup keputusan lama.

## ATT — Event journal, adapters, handoff, GUI: DONE (`4718364`, `fa324a8`)

- Migrasi v8 (`events`: event_id PK idempoten, sequence monotonik); `storage::events` (append/list/resume/coverage); CLI `aikd event append|list` + `aikd handoff`; MCP `event_append` + `handoff` (15 tools); log ke stderr (stdio bersih); `/ui` dashboard data live; ADR-0003.
- **Acceptance scenario PASS**: AGY via MCP (session_start, tool_call duplikat ter-dedupe, decision, task, checkpoint) → DSH via CLI `handoff` (tasks, checkpoint, decisions, events, next actions, watermark seq).

## Keadaan akhir terverifikasi

- `cargo check --workspace --all-targets`: 0 error, 0 warning.
- `cargo test --workspace`: **109 passed / 0 failed**.
- 15 commit di atas baseline, tree bersih.

## Phase 2 — Zero-ONNX, Atlas, governance, context, resources: DONE

- **Zero-ONNX**: default build tanpa `ort/ort-sys/fastembed/tokenizers/ndarray`
  (`cargo tree` nol baris ONNX). `core::vector` + `SemanticProvider`;
  `fastembed` opsional di belakang feature `semantic`; stub jujur;
  benchmark embedding SKIP di zero-ONNX. ADR-0004.
- **Session/event race**: `INSERT OR IGNORE` + `BEGIN IMMEDIATE`, test
  konkuren 8-thread (sesi konvergen, sequence 1..80 unik).
- **Project Atlas**: migrasi v9, fingerprint isi, CLI
  `project analyze|show|export` (10 JSON + 9 MD), diuji live.
- **Memory governance**: migrasi v10 (stale/archived/conflicted +
  conflict_note), CLI + MCP lifecycle/conflict (19 tools).
- **Context**: satu `assemble_handoff` (CLI+MCP), `inspect_context` +
  CLI `context inspect`, `/ui` lifecycle/konflik/Atlas.
- **Resources**: cap rayon + batch profil, `aikd doctor` live.
- **Canonical paths**: `canonical_path/project`, migrasi v11 dedup DB
  lama (terverifikasi: 6→3 baris, integrity OK).
- `cargo test --workspace`: **112 passed / 0 failed** (19 tool MCP).

## Sisa jujur Phase 2 (tidak diklaim selesai)

- System-tray Windows: belum (sengaja: satu daemon, tray duplikat proses).
- Retention otomatis event/memory: belum; `doctor` melaporkan pertumbuhan.
- Semantic specialist eksternal (`aikd-semantic`): kontrak `SemanticProvider`
  siap, implementasi belum.
- Release packaging (installer/CI artifact): belum.
- Graph explorer di `/ui`: belum (relasi deterministik tersedia di DB).
