# AIKD v2.0 — Testing Guide

Benchmark, Stress Test, and Fuzz Testing Documentation

---

## Table of Contents

1. [Overview](#1-overview)
2. [Benchmarking](#2-benchmarking)
3. [Stress Testing](#3-stress-testing)
4. [Fuzz Testing](#4-fuzz-testing)
5. [CI/CD Integration](#5-cicd-integration)
6. [Troubleshooting](#6-troubleshooting)
7. [Further Recommendations](#7-further-recommendations)

---

## 1. Overview

### Purpose

This document provides a comprehensive testing strategy for AIKD (AI Knowledge Daemon), covering performance benchmarking, stress testing, and fuzz testing. The goal is to ensure AIKD is reliable, fast, and safe under all conditions.

### Scope

| Component | What to Test |
|-----------|-------------|
| **Indexer** (Tantivy BM25) | Search latency, throughput, concurrency |
| **Embedder** (fastembed-rs ONNX) | Embedding speed, memory usage |
| **Storage** (SQLite) | Write throughput, read latency, migration safety |
| **Chunker** (Markdown/Code) | Chunking speed, correctness |
| **REST API** (axum) | Request throughput, concurrent connections, auth |
| **MCP Server** (rmcp) | Tool response time, protocol compliance |
| **File Watcher** (notify) | Event detection, debounce correctness |
| **CLI** (clap) | Argument parsing, output format |

### Tools

| Tool | Purpose | Language |
|------|---------|---------|
| Built-in `aikd benchmark` | 8-scenario benchmark suite | Rust |
| `criterion` | Statistical benchmarking | Rust |
| `cargo-fuzz` | Coverage-guided fuzzing | Rust |
| `k6` | HTTP load testing | JavaScript |
| `hyothesis` | Property-based testing | Rust |

---

## 2. Benchmarking

### Metrics to Measure

| Metric | Unit | Target |
|--------|------|--------|
| Indexing throughput | files/second | >5,000 |
| BM25 search latency | milliseconds | <5 |
| Hybrid search latency | milliseconds | <50 |
| Embedding throughput | chunks/second | >20 |
| Chunking throughput | files/second | >100,000 |
| Memory usage | MB | <500 (default tier) |
| CPU usage | % | <50% sustained |

### Running the Built-in Benchmark

```bash
# Full benchmark suite (8 scenarios)
cargo run --release --bin aikd -- benchmark

# With daemon running (enables REST API stress test)
aikd daemon --foreground &
cargo run --release --bin aikd -- benchmark

# With model downloaded (enables embedding benchmark)
aikd init
cargo run --release --bin aikd -- benchmark
```

### Benchmark Scenarios

| # | Scenario | What It Measures |
|---|----------|-----------------|
| 1 | Indexing (1000 files) | File discovery + chunking + SQLite write + Tantivy index |
| 2 | BM25 Search (100 queries) | Tantivy full-text search latency |
| 3 | Hybrid Search (50 queries) | BM25 + vector RRF fusion |
| 4 | Embedding (500 chunks) | ONNX inference + SQLite write |
| 5 | Incremental Re-index (100 files) | blake3 hash check + re-chunk |
| 6 | Concurrent Search (10 threads × 50 queries) | Parallel search safety |
| 7 | REST API Stress (100 requests) | HTTP throughput |
| 8 | Chunking Throughput (1000 files) | Pure chunking speed |

### Criterion Micro-benchmarks

Add to `crates/benchmark/Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "search_bench"
harness = false
```

Create `crates/benchmark/benches/search_bench.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use aikd_core::{Config, SearchFilters};
use aikd_storage::Database;
use aikd_indexer::TantivyEngine;

fn bench_bm25_search(c: &mut Criterion) {
    let config = Config::default();
    let db = Database::open(&config.db_path()).unwrap();
    let tantivy = TantivyEngine::open(&config.tantivy_path()).unwrap();
    let filters = SearchFilters::default();

    let mut group = c.benchmark_group("bm25_search");

    for limit in [1, 5, 10, 20, 50] {
        group.bench_with_input(BenchmarkId::new("limit", limit), &limit, |b, &limit| {
            b.iter(|| {
                tantivy.search(black_box("test query"), black_box(limit), black_box(&filters)).unwrap()
            });
        });
    }
    group.finish();
}

fn bench_chunking(c: &mut Criterion) {
    let content = "# Title\n\nSome content here.\n\n## Section\n\nMore content with code.\n\n```rust\nfn main() { println!(\"hello\"); }\n```\n";

    c.bench_function("chunk_markdown_1000_tokens", |b| {
        b.iter(|| {
            aikd_chunker::chunk_file(black_box("test.md"), black_box(content), black_box(1000), black_box(100))
        });
    });
}

fn bench_cosine_similarity(c: &mut Criterion) {
    let a: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
    let b: Vec<f32> = (0..384).map(|i| ((384 - i) as f32) / 384.0).collect();

    c.bench_function("cosine_similarity_384d", |bench| {
        bench.iter(|| {
            aikd_embedder::cosine_similarity(black_box(&a), black_box(&b))
        });
    });
}

criterion_group!(benches, bench_bm25_search, bench_chunking, bench_cosine_similarity);
criterion_main!(benches);
```

Run:

```bash
cargo bench -p aikd-benchmark
# Results in: target/criterion/report/index.html
```

### Interpreting Results

```
bm25_search/limit/10    time:   [1.2 ms 1.3 ms 1.4 ms]
                        change: [-2.1% +0.5% +3.2%] (p = 0.72 > 0.05)
                        No change in performance detected.
```

- **Green** (`No change`): Performance is stable
- **Yellow** (`change: +5%`): Minor regression, investigate if repeated
- **Red** (`change: +20%`): Significant regression, fix before merge

Detect regressions:

```bash
# Save baseline
cargo bench -p aikd-benchmark -- --save-baseline main

# After changes
cargo bench -p aikd-benchmark -- --baseline main
```

---

## 3. Stress Testing

### Goals

| Goal | How to Verify |
|------|---------------|
| Find breaking point | Increase load until errors appear |
| Measure degradation | Track latency at 50%, 80%, 100%, 150% capacity |
| Verify recovery | Stop load, check system returns to normal |
| Check data integrity | Verify no corruption after stress |

### REST API Stress Test with k6

Install k6:

```bash
# Windows
choco install k6

# Linux
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D68
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6

# macOS
brew install k6
```

Create `tests/stress/query_stress.js`:

```javascript
import http from 'k6/http';
import { check, sleep } from 'k6';

// Stress test configuration
export const options = {
  stages: [
    { duration: '30s', target: 10 },   // Ramp up to 10 users
    { duration: '1m', target: 50 },    // Stay at 50 users
    { duration: '30s', target: 100 },  // Spike to 100 users
    { duration: '1m', target: 100 },   // Hold at 100 users
    { duration: '30s', target: 0 },    // Ramp down
  ],
  thresholds: {
    http_req_duration: ['p(95)<500'],  // 95% of requests under 500ms
    http_req_failed: ['rate<0.01'],    // Error rate under 1%
  },
};

const BASE_URL = 'http://localhost:9090';

const QUERIES = [
  'login function',
  'error handling',
  'database connection',
  'authentication token',
  'file upload',
  'user registration',
  'api endpoint',
  'configuration',
  'test suite',
  'deployment',
];

export default function () {
  const query = QUERIES[Math.floor(Math.random() * QUERIES.length)];
  const limit = Math.floor(Math.random() * 20) + 1;

  const res = http.get(`${BASE_URL}/api/query?q=${encodeURIComponent(query)}&limit=${limit}`);

  check(res, {
    'status is 200': (r) => r.status === 200,
    'response has data': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.success === true;
      } catch {
        return false;
      }
    },
    'response time < 500ms': (r) => r.timings.duration < 500,
  });

  sleep(0.1);
}

// Stats endpoint stress
export function statsStress() {
  const res = http.get(`${BASE_URL}/api/stats`);
  check(res, {
    'stats status 200': (r) => r.status === 200,
  });
}
```

Run:

```bash
# Start daemon first
aikd daemon --foreground &

# Run stress test
k6 run tests/stress/query_stress.js

# With custom settings
k6 run --vus 50 --duration 2m tests/stress/query_stress.js
```

Expected output:

```
  █ TOTAL RESULTS

    http_req_duration..............: avg=12.5ms min=1.2ms med=8.3ms max=245ms p(90)=25ms p(95)=45ms
    http_req_failed................: 0.00%  ✓ 0        ✗ 5000
    http_reqs......................: 5000   49.5/s
```

### Concurrent CLI Stress Test

Create `tests/stress/concurrent_query.sh`:

```bash
#!/bin/bash
# Stress test: 100 concurrent CLI queries

QUERIES=("login" "error" "function" "config" "test" "api" "database" "file" "search" "index")
RESULTS_DIR="stress_results_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "Starting stress test: 100 concurrent queries"

for i in $(seq 1 100); do
    query="${QUERIES[$((i % ${#QUERIES[@]}))]}"
    (
        start_time=$(date +%s%N)
        aikd query "$query" --json --limit 5 > "$RESULTS_DIR/result_$i.json" 2>/dev/null
        end_time=$(date +%s%N)
        duration_ms=$(( (end_time - start_time) / 1000000 ))
        echo "$i,$query,$duration_ms" >> "$RESULTS_DIR/timings.csv"
    ) &
done

wait

echo "Stress test complete. Results in $RESULTS_DIR/"
echo "Average latency: $(awk -F',' '{sum+=$3; n++} END {print sum/n "ms"}' "$RESULTS_DIR/timings.csv")"
echo "Max latency: $(awk -F',' 'BEGIN{max=0} {if($3>max) max=$3} END {print max "ms"}' "$RESULTS_DIR/timings.csv")"
echo "Errors: $(ls "$RESULTS_DIR"/result_*.json 2>/dev/null | xargs grep -l '"success":false' 2>/dev/null | wc -l)"
```

Run:

```bash
chmod +x tests/stress/concurrent_query.sh
./tests/stress/concurrent_query.sh
```

### SQLite Write Stress Test

```rust
// tests/stress/sqlite_stress.rs
use aikd_storage::Database;
use std::sync::Arc;
use std::thread;

fn main() {
    let db = Arc::new(Database::open(std::path::Path::new("/tmp/aikd_stress.db")).unwrap());
    let mut handles = vec![];

    // 10 threads, each writing 1000 records
    for thread_id in 0..10 {
        let db = db.clone();
        handles.push(thread::spawn(move || {
            for i in 0..1000 {
                let path = format!("/test/file_{}_{}.txt", thread_id, i);
                let now = chrono::Utc::now().to_rfc3339();
                let _ = db.conn().execute(
                    "INSERT OR IGNORE INTO files (path, size, modified_at, last_scanned, status, blake3_hash) VALUES (?1,?2,?3,?4,'active','')",
                    rusqlite::params![path, 100, now, now],
                );
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let count: i64 = db.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    println!("Total records: {}", count);
    assert_eq!(count, 10000);
}
```

---

## 4. Fuzz Testing

### Purpose

Fuzz testing sends random, malformed, or unexpected inputs to find crashes, hangs, memory leaks, and unexpected behavior.

### What to Fuzz

| Target | Input Type | Risk |
|--------|-----------|------|
| `aikd query <input>` | CLI argument | Path traversal, injection, crash |
| `/api/query?q=<input>` | HTTP parameter | SQL injection, XSS, DoS |
| `chunk_file(path, content)` | File content | Panic on malformed UTF-8, infinite loop |
| `Config::load(yaml)` | YAML content | Deserialization panic |
| `serde_json::from_str()` | JSON content | Parse error handling |
| `blake3::hash(data)` | Binary data | Hash collision, memory |
| `cosine_similarity(a, b)` | Vectors | Division by zero, NaN |

### Setup cargo-fuzz

```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Initialize fuzz targets
cd crates/chunker
cargo fuzz init

cd crates/core
cargo fuzz init

cd crates/embedder
cargo fuzz init
```

### Fuzz Target 1: Chunker

Create `crates/chunker/fuzz/fuzz_targets/fuzz_chunker.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz the chunker with random file content
fuzz_target(|data: &[u8]| {
    // Try to convert to string (may fail for binary data)
    if let Ok(content) = std::str::from_utf8(data) {
        // Test all file types
        for path in &["test.md", "test.json", "test.yaml", "test.txt", "test.rs", "test.py"] {
            let _ = aikd_chunker::chunk_file(path, content, 1000, 100);
        }
    }
});
```

Run:

```bash
cd crates/chunker
cargo fuzz run fuzz_chunker -- -max_len=10000 -timeout=60
```

### Fuzz Target 2: Config Parser

Create `crates/core/fuzz/fuzz_targets/fuzz_config.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz YAML config parsing
fuzz_target(|data: &[u8]| {
    if let Ok(yaml_str) = std::str::from_utf8(data) {
        // Should not panic on any input
        let _ = serde_yaml::from_str::<aikd_core::Config>(yaml_str);
    }
});
```

Run:

```bash
cd crates/core
cargo fuzz run fuzz_config -- -max_len=5000 -timeout=30
```

### Fuzz Target 3: Embedder Vectors

Create `crates/embedder/fuzz/fuzz_targets/fuzz_vectors.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz vector operations with random data
fuzz_target(|data: &[u8]| {
    // Convert bytes to f32 slice
    if data.len() >= 8 && data.len() % 4 == 0 {
        let floats: Vec<f32> = data.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        if floats.len() >= 2 {
            let mid = floats.len() / 2;
            let (a, b) = floats.split_at(mid);

            // Should not panic even with NaN/Inf
            let _ = aikd_embedder::cosine_similarity(a, b);

            // Test serialization roundtrip
            let bytes = aikd_embedder::f32_to_bytes(&floats);
            let recovered = aikd_embedder::bytes_to_f32(&bytes);
            assert_eq!(floats.len(), recovered.len());
        }
    }
});
```

Run:

```bash
cd crates/embedder
cargo fuzz run fuzz_vectors -- -max_len=4000 -timeout=30
```

### Fuzz Target 4: REST API Inputs

Create `tests/fuzz/api_fuzz.rs`:

```rust
use reqwest::Client;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let base = "http://localhost:9090";

    // Fuzz query parameter
    let fuzz_inputs = vec![
        "", "\0", "../../../etc/passwd", "' OR 1=1 --",
        &"A".repeat(10000), "🦀💀🔥", "\n\r\t",
        "%00%0d%0a", "<script>alert(1)</script>",
        "null", "undefined", "-1", "99999999999999999999",
    ];

    for input in &fuzz_inputs {
        let url = format!("{}/api/query?q={}", base, urlencoding::encode(input));
        match client.get(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_server_error() {
                    eprintln!("SERVER ERROR with input {:?}: {}", input, status);
                }
            }
            Err(e) => {
                eprintln!("Request failed with input {:?}: {}", input, e);
            }
        }
    }

    println!("API fuzz test complete");
}
```

### Fuzz Target 5: CLI Arguments

```bash
#!/bin/bash
# Fuzz CLI with random arguments

FUZZ_DIR="fuzz_cli_results"
mkdir -p "$FUZZ_DIR"

# Random strings for fuzzing
STRINGS=(
    "" "test" "../../../etc/passwd" "$(printf '\x00')" 
    "$(printf '\xff\xfe')" "A%.0s{1..10000}" 
    "-1" "99999999999" "null" "true" "false"
    "--json" "--limit" "--path" "--hybrid"
)

for i in $(seq 1 1000); do
    # Pick random arguments
    query="${STRINGS[$((RANDOM % ${#STRINGS[@]}))]}"
    flag="${STRINGS[$((RANDOM % ${#STRINGS[@]}))]}"
    limit="$((RANDOM % 1000))"

    # Run and capture exit code
    timeout 5 aikd query "$query" --limit "$limit" $flag > /dev/null 2>&1
    exit_code=$?

    # Check for crashes (not just errors)
    if [ $exit_code -eq 139 ] || [ $exit_code -eq 134 ]; then
        echo "CRASH DETECTED: exit=$exit_code query='$query' flag='$flag' limit=$limit"
        echo "query='$query' flag='$flag' limit=$limit" >> "$FUZZ_DIR/crashes.log"
    fi
done

echo "CLI fuzz test complete. Crashes: $(wc -l < "$FUZZ_DIR/crashes.log" 2>/dev/null || echo 0)"
```

### Running All Fuzz Tests

```bash
# Run each fuzz target for 1 hour
cd crates/chunker && cargo fuzz run fuzz_chunker -- -max_total_time=3600
cd crates/core && cargo fuzz run fuzz_config -- -max_total_time=3600
cd crates/embedder && cargo fuzz run fuzz_vectors -- -max_total_time=3600

# Or run all with a script
#!/bin/bash
for target in fuzz_chunker fuzz_config fuzz_vectors; do
    dir=$(dirname $(find . -name "$target.rs" -path "*/fuzz/*"))
    cd $(dirname $dir)
    echo "Running $target..."
    cargo fuzz run $target -- -max_total_time=3600 2>&1 | tee "../fuzz_${target}.log"
    cd - > /dev/null
done
```

### Triage Guide

When a crash is found:

```bash
# 1. Reproduce the crash
cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123

# 2. Minimize the input
cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123 -minimize_crash=1

# 3. Get stack trace
RUST_BACKTRACE=1 cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123

# 4. Check if it's a known issue
# - Panic in library code → file issue
# - OOM → check input size limits
# - Timeout → check for infinite loops
```

Severity classification:

| Severity | Description | Action |
|----------|-------------|--------|
| **Critical** | Crash with user input, data corruption | Fix immediately |
| **High** | Panic in library code, memory leak | Fix before release |
| **Medium** | Timeout with large input, graceful error missing | Fix in next sprint |
| **Low** | Suboptimal error message, minor behavior | Track and fix later |

---

## 5. CI/CD Integration

### GitHub Actions Workflow

Create `.github/workflows/testing.yml`:

```yaml
name: Testing

on:
  push:
    branches: [main]
  pull_request:

jobs:
  unit-tests:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all

  benchmark:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo bench -p aikd-benchmark -- --save-baseline ${{ github.sha }}
      - uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion/

  fuzz-test:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-fuzz
      - run: cd crates/chunker && cargo fuzz run fuzz_chunker -- -max_total_time=600
      - run: cd crates/core && cargo fuzz run fuzz_config -- -max_total_time=600

  stress-test:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p aikd-cli
      - run: |
          ./target/release/aikd init
          ./target/release/aikd scan
          ./target/release/aikd daemon --foreground &
          sleep 3
      - uses: grafana/k6-action@v3
        with:
          filename: tests/stress/query_stress.js
```

### Regression Detection

```bash
# Save baseline after optimization
cargo bench -p aikd-benchmark -- --save-baseline optimized

# Compare against baseline
cargo bench -p aikd-benchmark -- --baseline optimized

# Fail CI if regression > 10%
cargo bench -p aikd-benchmark -- --baseline optimized --threshold 0.10
```

---

## 6. Troubleshooting

| Problem | Cause | Solution |
|---------|-------|----------|
| Benchmark hangs | Deadlock in concurrent test | Check `rayon` thread pool, reduce parallelism |
| Fuzz test OOM | Input too large | Set `-max_len=1000` |
| Stress test errors | Server not running | Start daemon first |
| Flaky benchmark | System load varies | Run on dedicated machine, increase iterations |
| cargo-fuzz fails | Needs nightly Rust | `rustup install nightly` |
| Criterion regression | CPU throttling | Disable turbo boost, run multiple times |

---

## 7. Further Recommendations

1. **Property-based testing** with `proptest` for config parsing and chunk correctness
2. **Memory profiling** with `dhat` or `valgrind` to detect leaks
3. **Code coverage** with `cargo-tarpaulin` to find untested paths
4. **Long-running soak test** (24h) to detect memory leaks and connection issues
5. **Chaos testing** — randomly kill daemon during scan to verify data integrity

---

_Last updated: 2026-06-15 | AIKD v2.0.0_
