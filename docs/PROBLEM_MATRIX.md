# AIKD Problem Matrix (riset + audit, dikumpulkan sebelum diperbaiki)

Sumber: riset tools sejenis (Octocode, codebase-memory, context-mode, TREK,
MCP spec, notify, hybrid-search literature, daemon-kit) + audit repo penuh.
Status awal: ditulis SEBELUM perbaikan; dicentang saat verifikasi hijau.

## Kritis (data-loss / salah sasaran / keamanan)

- [ ] **C1. Config bocor → operasi salah DB.** `load_or_default` diam-diam
  pakai default (`~/.aikd`) bila `--config` gagal dibaca. `scan/gc/query`
  lalu beroperasi di DB user tanpa sadar. Audit: `gc` hapus 685 file
  user. Solusi: gagal keras bila `--config` eksplisit tapi tak terbaca;
  `doctor` deteksi mismatch DB vs config.
- [ ] **C2. `scan` default kelewat sunyi soal filter.** Default extensions
  tanpa bahasa sumber; `init` pintar tapi tak dipaksa. Hasil "found 1
  files" tanpa tahu ratusan file terlewat. Solusi: laporkan skipped-by-
  filter + sarankan `init` (hitung kandidat sebelum filter).
- [ ] **C3. `validate()` tak pernah dipanggil.** Port <1024, max<min,
  path tak ada lolos diam. Solusi: panggil di setiap command + `doctor`.
- [ ] **C4. Query kosong tanpa penjelasan.** Index kosong (atau project
  lain) → "No results found" tanpa bedakan. Industri: penyebab #1 agent
  "tidak bisa menemukan kode". Solusi: pesan bedakan (belum scan / beda
  project / filter terlalu sempit) + saran perintah.
- [ ] **C5. Symbols 0 di demo.** Akar: konsekuensi C2 (hanya Cargo.toml
  terindex) atau extract gagal. Verifikasi ulang setelah C2: harus 4.
- [ ] **C6. REST tanpa token = semua tolak TAPI tanpa panduan.** Fail-closed
  benar, tapi pesan hanya log server; klien terima 401 polos. Solusi:
  body 401 berisi cara set token (tanpa bocorkan token).

## Daemon / proses (Windows nyata, bukan teori)

- [ ] **D1. `daemon` pernah gagal diam-diam** (stderr=null, fallthrough ke
  spawn). Sudah diperbaiki sebagian (return + spawn terpisah); VERIFIKASI
  live Windows: start → health 200 → stop → port bebas.
- [ ] **D2. `daemon-stop` PID basi.** PID file tanpa cek hidup di Windows
  (kill -0 hanya unix). Port-check sudah ada di start; stop harus cek
  TCP dulu + hapus PID basi. Solusi: stop = cek port; mati → hapus PID.
- [ ] **D3. `current_exe` rantai shim.** Spawn pakai current_exe (bisa shim);
  benar tapi 1 hop. Dokumentasikan; `daemon pid` harus tampilkan versi
  biner aktual (shim resolve) agar diagnosa jelas.
- [ ] **D4. Watcher belum teruji live.** notify backend beda per OS;
  network/WSL path tak emit (riset). Solusi: uji live Windows (buat-ubah-
  hapus file → index ikut), `doctor` laporkan watcher aktif/tidak, pesan
  jelas bila path di network drive.

## MCP / stdio (aturan protokol dari riset)

- [ ] **M1. `info!` → stdout.** Macro `info!` pakai `println!` (dipakai di
  init/remember/stats non-JSON). Bila MCP panggil jalur itu → stdout
  kotor → JSON-RPC rusak. Solusi: `info!` → stderr juga (samakan einfo),
  atau pastikan handler MCP tak panggil jalur println.
- [ ] **M2. `.mcp.json` pakai PATH (`"aikd"`).** GUI app (OMP, Cursor)
  sering PATH minim → spawn ENOENT (riset: failure #1). Solusi: tulis
  absolute path saat `init`/register + `doctor` cek resolvability.
- [ ] **M3. Handshake salah urutan → error tak ramah.** bg_20: tools/list
  sebelum initialize → "expect initialized" mentah. Solusi: pesan jelas.
- [ ] **M4. MCP `query` tanpa auto-scan hint.** Beda dengan CLI yang
  ingatkan "Run: aikd scan". Solusi: pesan kosong yang sama dengan C4.

## Retrieval / data (dari literatur hybrid + audit)

- [ ] **R1. FTS `chunks_fts` vs Tantivy ganda?** Dua index teks (SQLite FTS
  + Tantivy BM25) — kapan masing-masing dipakai? Audit: pastikan satu
  jalur kanonis per query, satunya lagi dijelaskan atau dibuang.
- [ ] **R2. Hybrid `k=60` magic number.** RRF k tetap tanpa dasar ukur.
  Solusi: konstanta bernama + benchmark mini di docs, atau biarkan + catat.
- [ ] **R3. `detect_file_type` tanpa `.cjs/.mjs/.jsx` lengkap?** Cek daftar
  vs ekstensi umum; tambah yang kurang + test.
- [ ] **R4. Chunk `overlap_tokens` selalu 0.** Strip konteks antar-chunk;
  simbol di batas chunk bisa hilang. Solusi: hormati config overlap.

## Lintas platform / skrip

- [ ] **X1. CRLF merusak skrip.** `install.sh`/test fixture CRLF → bash
  gagal. Solusi: `.gitattributes` (`*.sh text eol=lf`, `*.ps1` CRLF),
  normalisasi sekali.
- [ ] **X2. Campur sintaks PS/bash.** Riwayat 3 kejadian. Solusi: tiap
  skrip punya header + `shellcheck`-setara (bash -n) dan blok markdown
  yang jelas PS vs bash di docs; uji keduanya.
- [ ] **X3. `update --version` parsing rapuh.** Ambil dari `--version`
  output; bila gagal → label aneh. Solusi: fallback `unknown-<timestamp>`
  + validasi karakter aman (sudah sebagian).
- [ ] **X4. Shim di PowerShell/CMD.** Baru diuji via bash. Solusi: uji
  `aikd --version`, `serve` handshake, `doctor` di CMD + PowerShell.
- [ ] **X5. Network/WSL path.** Deteksi UNC/`\\wsl$` → warning + tolak
  SQLite di sana (risiko corrupt, sqlite.org) dengan pesan jelas.

## Kemasan / docs

- [ ] **P1. `update --list` bingung saat daemon jalan versi lama.** Tampilkan
  versi PROSES berjalan (bila lock ada) vs versi aktif pointer.
- [ ] **P2. Extension OMP belum load-test.** `aikd.ts` lolos aturan tapi
  belum dipanggil OMP sungguhan. Solusi: uji 1 tool via sesi OMP.
- [ ] **P3. `.mcp.json` repo menunjuk PATH absolut lama.** Sudah ke `aikd`;
  verifikasi OMP baca + connect hijau.

## Verifikasi akhir (matriks uji, semua harus hijau)

- CMD + PowerShell + bash: `--version`, `doctor`, `init`, `scan`,
  `query`, `update --list`.
- Debug + release: full suite 118+ test.
- Live Windows: daemon start→health→stop; watcher buat-ubah-hapus;
  MCP handshake berurutan via OMP; serve ganda bersamaan (harus aman).
