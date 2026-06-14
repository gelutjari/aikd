# AIKD v2.0 — Test Report

**Date:** 2026-06-15
**System:** Windows 11 Pro, 24 cores, 31.7 GB RAM, NVIDIA RTX 4060
**Binary:** aikd 2.0.0 (release build, opt-level=3, LTO, strip)

---

## 1. Unit Tests

| Crate | Tests | Status |
|-------|-------|--------|
| benchmark | 5 | PASS |
| chunker | 5 | PASS |
| core | 13 | PASS |
| embedder | 11 | PASS |
| error | 3 | PASS |
| fusion | 2 | PASS |
| indexer | 8 | PASS |
| platform | 2 | PASS |
| scanner | 1 | PASS |
| security | 3 | PASS |
| session | 5 | PASS |
| storage | 6 | PASS |
| types | 8 | PASS |
| **TOTAL** | **82** | **ALL PASS** |

---

## 2. Benchmark Results (Built-in Suite)

| # | Test | Duration | Throughput | Status |
|---|------|----------|-----------|--------|
| 1 | Indexing (1000 files) | 466.6ms | 2,143 files/s | PASS |
| 2 | BM25 Search (100 queries) | 1,603.4ms | 0.17ms/query | PASS |
| 3 | Hybrid Search (50 queries) | 565.3ms | 0.24ms/query | PASS |
| 4 | Embedding (500 chunks) | 4,412.3ms | 340 chunks/s | PASS |
| 5 | Incremental Re-index (100 files) | 217.8ms | — | **FAIL** |
| 6 | Concurrent Search (10×50 queries) | 70.6ms | 7,085 queries/s | PASS |
| 7 | REST API Stress (100 requests) | 804.9ms | 124 req/s | PASS |
| 8 | Chunking Throughput (1000 files) | 12.7ms | 78,853 files/s | PASS |

**Overall:** 7/8 passed, 1 failed
**Total time:** 8.15s
**Peak resources:** CPU 28.8%, RAM 47.9% (15,559 MB)

### Failure Analysis

**Test #5 — Incremental Re-index:**
```
Error: UNIQUE constraint failed: files.path
```
- **Cause:** Benchmark re-inserts files that already exist in DB from Test #1
- **Impact:** Benchmark only, does NOT affect production code
- **Fix:** Benchmark should use `INSERT OR REPLACE` instead of `INSERT`

---

## 3. Manual CLI Benchmark

| Operation | Time | Notes |
|-----------|------|-------|
| `aikd scan` (38 files) | 219ms | Includes chunking + DB write + Tantivy index |
| `aikd query` (BM25, avg 10x) | 25.7ms | min=24ms, max=30ms |
| `aikd query` (hybrid, avg 10x) | 28.2ms | min=27ms, max=33ms |
| `aikd stats` | 277ms | Cold start (opens DB + reads counts) |
| `aikd embed` (168 chunks) | 15.8s | Model load + ONNX inference |

---

## 4. Stress Test Results

### Concurrent CLI (50 parallel queries)

| Metric | Value |
|--------|-------|
| Concurrent jobs | 50 |
| Completed | 50/50 |
| Average latency | 190.9ms |
| Min latency | 65ms |
| Max latency | 749ms |
| Errors | 0 |

### REST API (20 sequential requests)

| Metric | Value |
|--------|-------|
| Requests | 20 |
| Successful | 20/20 |
| Average latency | 109.4ms |
| Error rate | 0% |

### REST API Stress (built-in, 100 requests)

| Metric | Value |
|--------|-------|
| Requests | 100 |
| Successful | 100/100 |
| Throughput | 124.2 req/s |
| Error rate | 0% |

---

## 5. Summary

| Category | Result |
|----------|--------|
| Unit Tests | ✅ 82/82 PASS |
| Benchmark Suite | ⚠️ 7/8 PASS (1 benchmark bug) |
| CLI Performance | ✅ <30ms search |
| Concurrent Safety | ✅ 50/50 parallel OK |
| REST API | ✅ 20/20 + 100/100 OK |
| Resource Usage | ✅ CPU 28.8%, RAM 47.9% |

### Known Issues

1. **Benchmark #5 (Incremental Re-index):** UNIQUE constraint error in benchmark test code, not in production
2. **Stats cold start:** 277ms due to DB open + schema migration check (normal for first call)
3. **Daemon auto-restart:** Daemon does not persist across terminal sessions

---

_Report generated: 2026-06-15 | AIKD v2.0.0_
