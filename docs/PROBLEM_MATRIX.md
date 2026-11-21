# AIKD Problem Matrix (riset + audit, dikumpulkan sebelum diperbaiki)

Sumber: riset tools sejenis (Octocode, codebase-memory, context-mode, TREK,
MCP spec, notify, hybrid-search literature, daemon-kit) + audit repo penuh.
Status awal: ditulis SEBELUM perbaikan; dicentang saat verifikasi hijau.

## Kritis (data-loss / salah sasaran / keamanan) — SEMUA VERIFIED FIXED

- [x] **C1. Config bocor.** Explicit --config unreadable → exit 2 + pesan; semua loader lewat `Config::load_validated` (core); DB isolasi terbukti tidak bocor.
- [x] **C2. Scan sunyi.** Discovery laporkan skipped by ext/filter/dir/size + saran `init`; `init --path` set include_paths.
- [x] **C3. validate tak dipanggil.** `load_validated` di semua jalur + warning network path (X5).
- [x] **C4. Hasil kosong tanpa penjelasan.** CLI teks+JSON hint, MCP format_results hint.
- [x] **C5. Symbols 0.** Penyebab = C2; terverifikasi symbols=2 pasca-fix.
- [x] **C6. 401 polos.** Pesan bawa cara set token + header (teruji live).

## Daemon / proses — SEMUA VERIFIED FIXED (live Windows)

- [x] **D1. Daemon gagal diam-diam.** Rewrite total: return Ok setelah select, spawn verifikasi port 15 dtk + tail log + deteksi child mati dini. Teruji: start→PID tercatat→health 200→stop→port tutup.
- [x] **D2. Daemon-stop PID basi.** Port-first; PID hilang → diagnosa jujur; tidak bunuh PID daur ulang. Teruji live.
- [x] **D3. current_exe rantai shim.** `doctor` tampilkan binary aktual + active-version + warning bila beda. `daemon pid` tetap PID; versi via doctor.
- [x] **D4. Watcher live.** TERUJI Windows: create→modified, change→reindex, delete→hilang dari index (query trig kosong + hint). Config salah path kini exit 2 (C1), bukan default diam.
## MCP / stdio — SEMUA VERIFIED FIXED

- [x] **M1. info! → stdout.** Macro `info!` kini eprintln (stdout hanya JSON/hasil). Serve hanya eprintln + JSON-RPC. Handshake MCP 200 OK teruji.
- [x] **M2. .mcp.json PATH.** `init` tulis current_exe absolut; `doctor` tampilkan binary aktual; `.mcp.json` + extension OMP → `aikd` PATH (sesuai OMP env). doctor ingatkan bila shim/versi beda.
- [x] **M3. Handshake salah urutan.** rmcp menolak dengan benar (bukan crash); urutan didokumentasikan di `.mcp.json` description.
- [x] **M4. MCP query tanpa hint.** `format_results` kosong kini bawa hint sama dengan CLI.

## Retrieval / data — SEMUA VERIFIED FIXED / DIPUTUSKAN

- [x] **R1. FTS vs Tantivy ganda.** DIPUTUSKAN: Tantivy satu-satunya; FTS5 + trigger di-drop (migrasi v12); `db_check`/compact/status dibersihkan; --rebuild-fts no-op ber-warning.
- [x] **R2. Hybrid k=60 magic.** `DEFAULT_RRF_K` bernama; semua call site pakai konstanta.
- [x] **R3. Ekstensi kurang.** mjs/cjs/cs ditambahkan (chunker + smart config).
- [x] **R4. Overlap 0 / potong deklarasi.** Potong hanya di boundary simbol/blank (bounded 2x max) + test boundary hijau.

## Lintas platform / skrip — SEMUA VERIFIED FIXED

- [x] **X1. CRLF.** `.gitattributes` (sh LF, ps1 CRLF, rs/toml/yaml/md/json LF).
- [x] **X2. Campur sintaks.** Aturan: uji bash via `bash -n`/langsung; PS via `powershell -c`; CMD via `cmd /C`; tidak campur dalam satu baris (pelajaran tasklist/cmd).
- [x] **X3. Version parsing.** Sanitize [A-Za-z0-9._-] + fallback timestamp `manual-%Y%m%d-%H%M%S`; tolak path separator.
- [x] **X4. Shim PS/CMD.** Teruji: CMD `stats` OK; PS `doctor` OK (binary 2.0.0, schema v12, versions pointer).
- [x] **X5. Network path.** `validate()` warning untuk UNC/mnt/media/Volumes.
## Kemasan / docs

- [x] **P1. update --list vs versi proses.** Menampilkan lock holder + pointer aktif + daftar versi; beda versi = warning di doctor.
- [ ] **P2. Extension OMP load-test.** TS valid + pola sama dengan extension aktif lain; TAPI belum ada pemanggilan tool nyata via sesi OMP. Verifikasi: sesi OMP baru panggil `aikd_stats`.
- [ ] **P3. .mcp.json connect via OMP.** Config menunjuk `aikd` PATH + serve handshake 200 teruji manual; TAPI OMP belum reconnect sesi baru. Verifikasi: sesi baru cek MCP aikd hijau.

## Verifikasi akhir — HASIL SESI INI (semua dijalankan, bukan klaim)

- [x] CMD (`stats`), PowerShell (`doctor`, `--version`), bash (audit penuh 19 perintah).
- [x] Debug: 114 test PASS. (Release: build OK; suite penuh = debug.)
- [x] Live Windows: daemon start→PID→health 200→double ditolak→stop→port tutup; watcher create/modify/delete→index ikut; MCP handshake berurutan 200; serve ganda (AGY+DSH) seq unik + handoff gabung.
- [ ] P2/P3: butuh SESI OMP BARU (extension load + MCP reconnect) — di luar sesi ini yang memegang serve lama.

Catatan debugging sesi ini (aturan abadi):
- JANGAN `taskkill /IM <nama>` — selalu PID spesifik.
- JANGAN pipe perintah daemon ke `head` — parent mati → anak yatim tanpa PID file.
- JANGAN uji background process dari sesi yang sama tanpa tracking PID.
- `cmd /C` + pipe quoting rusak — panggil biner langsung, match output di kode.
- Bunuh → start butuh jeda (handle kernel zombie); port-check + lock + poll menutup race ini.
