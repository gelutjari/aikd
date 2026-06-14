# AIKD v2.0 — Panduan Pengujian

Dokumentasi Benchmark, Stress Test, dan Fuzz Testing

---

## Daftar Isi

1. [Ikhtisar](#1-ikhtisar)
2. [Benchmarking](#2-benchmarking)
3. [Stress Testing](#3-stress-testing)
4. [Fuzz Testing](#4-fuzz-testing)
5. [Integrasi CI/CD](#5-integrasi-cicd)
6. [Pemecahan Masalah](#6-pemecahan-masalah)
7. [Rekomendasi Lanjutan](#7-rekomendasi-lanjutan)

---

## 1. Ikhtisar

### Tujuan

Dokumen ini menyediakan strategi pengujian komprehensif untuk AIKD (AI Knowledge Daemon), mencakup benchmark performa, stress testing, dan fuzz testing. Tujuannya adalah memastikan AIKD handal, cepat, dan aman dalam semua kondisi.

### Cakupan

| Komponen | Yang Diuji |
|----------|-----------|
| **Indexer** (Tantivy BM25) | Latensi pencarian, throughput, konkurensi |
| **Embedder** (fastembed-rs ONNX) | Kecepatan embedding, penggunaan memori |
| **Storage** (SQLite) | Throughput tulis, latensi baca, keamanan migrasi |
| **Chunker** (Markdown/Code) | Kecepatan chunking, kebenaran |
| **REST API** (axum) | Throughput request, koneksi konkuren, autentikasi |
| **MCP Server** (rmcp) | Waktu respons tool, kepatuhan protokol |
| **File Watcher** (notify) | Deteksi event, kebenaran debounce |
| **CLI** (clap) | Parsing argumen, format output |

### Alat yang Digunakan

| Alat | Tujuan | Bahasa |
|------|--------|--------|
| `aikd benchmark` bawaan | 8 skenario benchmark | Rust |
| `criterion` | Benchmark statistik | Rust |
| `cargo-fuzz` | Fuzzing berbasis cakupan | Rust |
| `k6` | Load testing HTTP | JavaScript |
| `proptest` | Testing berbasis properti | Rust |

---

## 2. Benchmarking

### Metrik yang Diukur

| Metrik | Satuan | Target |
|--------|--------|--------|
| Throughput indexing | file/detik | >5.000 |
| Latensi BM25 search | milidetik | <5 |
| Latensi hybrid search | milidetik | <50 |
| Throughput embedding | chunk/detik | >20 |
| Throughput chunking | file/detik | >100.000 |
| Penggunaan memori | MB | <500 (tier default) |
| Penggunaan CPU | % | <50% berkelanjutan |

### Menjalankan Benchmark Bawaan

```bash
# Skenario benchmark lengkap (8 skenario)
cargo run --release --bin aikd -- benchmark

# Dengan daemon berjalan (aktifkan stress test REST API)
aikd daemon --foreground &
cargo run --release --bin aikd -- benchmark

# Dengan model terdownload (aktifkan benchmark embedding)
aikd init
cargo run --release --bin aikd -- benchmark
```

### Skenario Benchmark

| # | Skenario | Yang Diukur |
|---|----------|-------------|
| 1 | Indexing (1000 file) | Penemuan file + chunking + tulis SQLite + index Tantivy |
| 2 | BM25 Search (100 query) | Latensi full-text search Tantivy |
| 3 | Hybrid Search (50 query) | BM25 + vector RRF fusion |
| 4 | Embedding (500 chunk) | Inferensi ONNX + tulis SQLite |
| 5 | Incremental Re-index (100 file) | Cek hash blake3 + re-chunk |
| 6 | Concurrent Search (10 thread × 50 query) | Keamanan pencarian paralel |
| 7 | REST API Stress (100 request) | Throughput HTTP |
| 8 | Chunking Throughput (1000 file) | Kecepatan chunking murni |

### Micro-benchmark dengan Criterion

Tambahkan ke `crates/benchmark/Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "search_bench"
harness = false
```

Buat `crates/benchmark/benches/search_bench.rs`:

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
    let content = "# Title\n\nSome content here.\n\n## Section\n\nMore content.\n";

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

Jalankan:

```bash
cargo bench -p aikd-benchmark
# Hasil di: target/criterion/report/index.html
```

### Menginterpretasi Hasil

```
bm25_search/limit/10    time:   [1.2 ms 1.3 ms 1.4 ms]
                        change: [-2.1% +0.5% +3.2%] (p = 0.72 > 0.05)
                        No change in performance detected.
```

- **Hijau** (`No change`): Performa stabil
- **Kuning** (`change: +5%`): Regresi minor, selidiki jika berulang
- **Merah** (`change: +20%`): Regresi signifikan, perbaiki sebelum merge

Deteksi regresi:

```bash
# Simpan baseline
cargo bench -p aikd-benchmark -- --save-baseline main

# Setelah perubahan
cargo bench -p aikd-benchmark -- --baseline main
```

---

## 3. Stress Testing

### Tujuan

| Tujuan | Cara Verifikasi |
|--------|-----------------|
| Temukan titik patah | Tingkatkan beban hingga muncul error |
| Ukur degradasi | Lacak latensi pada 50%, 80%, 100%, 150% kapasitas |
| Verifikasi pemulihan | Hentikan beban, cek sistem kembali normal |
| Cek integritas data | Verifikasi tidak ada korupsi setelah stress |

### Stress Test REST API dengan k6

Instal k6:

```bash
# Windows
choco install k6

# Linux (Ubuntu/Debian)
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg \
  --keyserver hkp://keyserver.ubuntu.com:80 \
  --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D68
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update && sudo apt-get install k6

# macOS
brew install k6
```

Buat `tests/stress/query_stress.js`:

```javascript
import http from 'k6/http';
import { check, sleep } from 'k6';

// Konfigurasi stress test
export const options = {
  stages: [
    { duration: '30s', target: 10 },   // Naik ke 10 user
    { duration: '1m', target: 50 },    // Tahan di 50 user
    { duration: '30s', target: 100 },  // Lonjak ke 100 user
    { duration: '1m', target: 100 },   // Tahan di 100 user
    { duration: '30s', target: 0 },    // Turunkan
  ],
  thresholds: {
    http_req_duration: ['p(95)<500'],  // 95% request di bawah 500ms
    http_req_failed: ['rate<0.01'],    // Error rate di bawah 1%
  },
};

const BASE_URL = 'http://localhost:9090';

const QUERIES = [
  'fungsi login',
  'error handling',
  'koneksi database',
  'token autentikasi',
  'upload file',
  'registrasi user',
  'endpoint api',
  'konfigurasi',
  'test suite',
  'deployment',
];

export default function () {
  const query = QUERIES[Math.floor(Math.random() * QUERIES.length)];
  const limit = Math.floor(Math.random() * 20) + 1;

  const res = http.get(`${BASE_URL}/api/query?q=${encodeURIComponent(query)}&limit=${limit}`);

  check(res, {
    'status 200': (r) => r.status === 200,
    'respons punya data': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.success === true;
      } catch {
        return false;
      }
    },
    'waktu respons < 500ms': (r) => r.timings.duration < 500,
  });

  sleep(0.1);
}
```

Jalankan:

```bash
# Pastikan daemon berjalan dulu
aikd daemon --foreground &

# Jalankan stress test
k6 run tests/stress/query_stress.js

# Dengan pengaturan custom
k6 run --vus 50 --duration 2m tests/stress/query_stress.js
```

Output yang diharapkan:

```
  █ TOTAL RESULTS

    http_req_duration..............: avg=12.5ms min=1.2ms med=8.3ms max=245ms p(90)=25ms p(95)=45ms
    http_req_failed................: 0.00%  ✓ 0        ✗ 5000
    http_reqs......................: 5000   49.5/s
```

### Stress Test CLI Konkuren

Buat `tests/stress/concurrent_query.sh`:

```bash
#!/bin/bash
# Stress test: 100 query CLI konkuren

QUERIES=("login" "error" "function" "config" "test" "api" "database" "file" "search" "index")
RESULTS_DIR="stress_results_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "Memulai stress test: 100 query konkuren"

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

echo "Stress test selesai. Hasil di $RESULTS_DIR/"
echo "Rata-rata latensi: $(awk -F',' '{sum+=$3; n++} END {print sum/n "ms"}' "$RESULTS_DIR/timings.csv")"
echo "Latensi maks: $(awk -F',' 'BEGIN{max=0} {if($3>max) max=$3} END {print max "ms"}' "$RESULTS_DIR/timings.csv")"
echo "Error: $(ls "$RESULTS_DIR"/result_*.json 2>/dev/null | xargs grep -l '"success":false' 2>/dev/null | wc -l)"
```

Jalankan:

```bash
chmod +x tests/stress/concurrent_query.sh
./tests/stress/concurrent_query.sh
```

### Stress Test Tulis SQLite

```rust
// tests/stress/sqlite_stress.rs
use aikd_storage::Database;
use std::sync::Arc;
use std::thread;

fn main() {
    let db = Arc::new(Database::open(std::path::Path::new("/tmp/aikd_stress.db")).unwrap());
    let mut handles = vec![];

    // 10 thread, masing-masing menulis 1000 record
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
    println!("Total record: {}", count);
    assert_eq!(count, 10000);
}
```

---

## 4. Fuzz Testing

### Tujuan

Fuzz testing mengirim input acak, cacat, atau tidak terduga untuk menemukan crash, hang, kebocoran memori, dan perilaku tak terduga.

### Target Fuzzing

| Target | Tipe Input | Risiko |
|--------|-----------|--------|
| `aikd query <input>` | Argumen CLI | Path traversal, injeksi, crash |
| `/api/query?q=<input>` | Parameter HTTP | SQL injection, XSS, DoS |
| `chunk_file(path, content)` | Konten file | Panic pada UTF-8 cacat, loop tak terbatas |
| `Config::load(yaml)` | Konten YAML | Panic deserialisasi |
| `serde_json::from_str()` | Konten JSON | Penanganan error parse |
| `cosine_similarity(a, b)` | Vektor | Pembagian dengan nol, NaN |

### Setup cargo-fuzz

```bash
# Instal cargo-fuzz
cargo install cargo-fuzz

# Inisialisasi target fuzz
cd crates/chunker && cargo fuzz init
cd crates/core && cargo fuzz init
cd crates/embedder && cargo fuzz init
```

### Fuzz Target 1: Chunker

Buat `crates/chunker/fuzz/fuzz_targets/fuzz_chunker.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz chunker dengan konten file acak
fuzz_target(|data: &[u8]| {
    if let Ok(content) = std::str::from_utf8(data) {
        // Uji semua tipe file
        for path in &["test.md", "test.json", "test.yaml", "test.txt", "test.rs"] {
            let _ = aikd_chunker::chunk_file(path, content, 1000, 100);
        }
    }
});
```

Jalankan:

```bash
cd crates/chunker
cargo fuzz run fuzz_chunker -- -max_len=10000 -timeout=60
```

### Fuzz Target 2: Parser Config

Buat `crates/core/fuzz/fuzz_targets/fuzz_config.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz parsing YAML config
fuzz_target(|data: &[u8]| {
    if let Ok(yaml_str) = std::str::from_utf8(data) {
        // Tidak boleh panic pada input apapun
        let _ = serde_yaml::from_str::<aikd_core::Config>(yaml_str);
    }
});
```

Jalankan:

```bash
cd crates/core
cargo fuzz run fuzz_config -- -max_len=5000 -timeout=30
```

### Fuzz Target 3: Vektor Embedder

Buat `crates/embedder/fuzz/fuzz_targets/fuzz_vectors.rs`:

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz operasi vektor dengan data acak
fuzz_target(|data: &[u8]| {
    if data.len() >= 8 && data.len() % 4 == 0 {
        let floats: Vec<f32> = data.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        if floats.len() >= 2 {
            let mid = floats.len() / 2;
            let (a, b) = floats.split_at(mid);

            // Tidak boleh panic bahkan dengan NaN/Inf
            let _ = aikd_embedder::cosine_similarity(a, b);

            // Uji roundtrip serialisasi
            let bytes = aikd_embedder::f32_to_bytes(&floats);
            let recovered = aikd_embedder::bytes_to_f32(&bytes);
            assert_eq!(floats.len(), recovered.len());
        }
    }
});
```

Jalankan:

```bash
cd crates/embedder
cargo fuzz run fuzz_vectors -- -max_len=4000 -timeout=30
```

### Fuzz Target 4: Input REST API

Buat `tests/fuzz/api_fuzz.rs`:

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

    // Input fuzz
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
                if resp.status().is_server_error() {
                    eprintln!("ERROR SERVER dengan input {:?}: {}", input, resp.status());
                }
            }
            Err(e) => {
                eprintln!("Request gagal dengan input {:?}: {}", input, e);
            }
        }
    }

    println!("Fuzz test API selesai");
}
```

### Fuzz Target 5: Argumen CLI

Buat `tests/fuzz/cli_fuzz.sh`:

```bash
#!/bin/bash
# Fuzz CLI dengan argumen acak

FUZZ_DIR="fuzz_cli_results"
mkdir -p "$FUZZ_DIR"

# String acak untuk fuzzing
STRINGS=(
    "" "test" "../../../etc/passwd" "$(printf '\x00')" 
    "$(printf '\xff\xfe')" "A%.0s{1..10000}" 
    "-1" "99999999999" "null" "true" "false"
    "--json" "--limit" "--path" "--hybrid"
)

for i in $(seq 1 1000); do
    query="${STRINGS[$((RANDOM % ${#STRINGS[@]}))]}"
    flag="${STRINGS[$((RANDOM % ${#STRINGS[@]}))]}"
    limit="$((RANDOM % 1000))"

    timeout 5 aikd query "$query" --limit "$limit" $flag > /dev/null 2>&1
    exit_code=$?

    # Cek crash (bukan hanya error)
    if [ $exit_code -eq 139 ] || [ $exit_code -eq 134 ]; then
        echo "CRASH TERDETEKSI: exit=$exit_code query='$query' flag='$flag' limit=$limit"
        echo "query='$query' flag='$flag' limit=$limit" >> "$FUZZ_DIR/crashes.log"
    fi
done

echo "Fuzz test CLI selesai. Crash: $(wc -l < "$FUZZ_DIR/crashes.log" 2>/dev/null || echo 0)"
```

### Menjalankan Semua Fuzz Test

```bash
# Jalankan setiap target fuzz selama 1 jam
cd crates/chunker && cargo fuzz run fuzz_chunker -- -max_total_time=3600
cd crates/core && cargo fuzz run fuzz_config -- -max_total_time=3600
cd crates/embedder && cargo fuzz run fuzz_vectors -- -max_total_time=3600

# Atau jalankan semua dengan script
#!/bin/bash
for target in fuzz_chunker fuzz_config fuzz_vectors; do
    dir=$(dirname $(find . -name "$target.rs" -path "*/fuzz/*"))
    cd $(dirname $dir)
    echo "Menjalankan $target..."
    cargo fuzz run $target -- -max_total_time=3600 2>&1 | tee "../fuzz_${target}.log"
    cd - > /dev/null
done
```

### Panduan Triase

Ketika crash ditemukan:

```bash
# 1. Reproduksi crash
cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123

# 2. Minimalkan input
cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123 -minimize_crash=1

# 3. Dapatkan stack trace
RUST_BACKTRACE=1 cargo fuzz run fuzz_chunker fuzz/artifacts/fuzz_chunker/crash-abc123

# 4. Cek apakah masalah sudah diketahui
# - Panic di kode library → buat issue
# - OOM → cek batas ukuran input
# - Timeout → cek loop tak terbatas
```

Klasifikasi severity:

| Severity | Deskripsi | Tindakan |
|----------|-----------|----------|
| **Critical** | Crash dengan input user, korupsi data | Perbaiki segera |
| **High** | Panic di kode library, kebocoran memori | Perbaiki sebelum rilis |
| **Medium** | Timeout dengan input besar, error handling kurang | Perbaiki sprint berikutnya |
| **Low** | Pesan error suboptimal, perilaku minor | Lacak dan perbaiki nanti |

---

## 5. Integrasi CI/CD

### GitHub Actions Workflow

Buat `.github/workflows/testing.yml`:

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

### Deteksi Regresi

```bash
# Simpan baseline setelah optimasi
cargo bench -p aikd-benchmark -- --save-baseline optimized

# Bandingkan dengan baseline
cargo bench -p aikd-benchmark -- --baseline optimized

# Gagalkan CI jika regresi > 10%
cargo bench -p aikd-benchmark -- --baseline optimized --threshold 0.10
```

---

## 6. Pemecahan Masalah

| Masalah | Penyebab | Solusi |
|---------|----------|--------|
| Benchmark hang | Deadlock di test konkuren | Cek thread pool `rayon`, kurangi parallelism |
| Fuzz test OOM | Input terlalu besar | Set `-max_len=1000` |
| Stress test error | Server tidak berjalan | Jalankan daemon dulu |
| Benchmark tidak konsisten | Beban sistem bervariasi | Jalankan di mesin khusus, tambah iterasi |
| cargo-fuzz gagal | Butuh Rust nightly | `rustup install nightly` |
| Criterion regresi | CPU throttling | Nonaktifkan turbo boost, jalankan beberapa kali |

---

## 7. Rekomendasi Lanjutan

1. **Property-based testing** dengan `proptest` untuk parsing config dan kebenaran chunk
2. **Memory profiling** dengan `dhat` atau `valgrind` untuk deteksi kebocoran
3. **Code coverage** dengan `cargo-tarpaulin` untuk menemukan jalur yang belum teruji
4. **Soak test jangka panjang** (24 jam) untuk deteksi kebocoran memori dan masalah koneksi
5. **Chaos testing** — acak matikan daemon saat scan untuk verifikasi integritas data

---

_Terakhir diperbarui: 2026-06-15 | AIKD v2.0.0_
