# Roadmap Pengembangan AIKD v2.1 — UI, Knowledge Graph, Send-to-AIKD & Tray Auto-Start

> **Status:** Rencana Kerja (Belum Dieksekusi)  
> **Dibuat:** 2026-11-23  
> **Prinsip Utama:** Jangan mengganggu proses `aikd` daemon (`127.0.0.1:9090`) maupun sesi MCP `aikd serve` yang sedang dipakai agent CLI lain selama pengembangan berlangsung.

---

## Ringkasan Masalah yang Ditemukan Saat Audit

1. **Daemon & Database aktif & sehat**, tetapi:
   - `aikd-tray.exe` belum memakai `windows_subsystem = "windows"`, tidak otomatis menyalakan daemon saat startup, belum single-instance, dan mengabaikan klik kiri ikon tray (`crates/cli/src/tray.rs`).
   - `aikd daemon` di background hanya menjalankan REST server (`AIKD_DAEMON_BG=1`), tidak menjalankan `aikd watch`.
2. **UI (`/ui` — `crates/server/src/ui.html`) terasa sepi & kurang informatif:**
   - `/api/graph?limit=30` mengembalikan `0` node jika kotak filter path dikosongkan.
   - Graph di `ui.html` hanya menggambar maksimal 20 file dalam lingkaran SVG statis (`files.slice(0, 20)`), mengabaikan `symbols`, tidak punya drag/pan/zoom/klik-preview, dan belum memetakan relasi catatan/memori.
   - `crates/storage/src/graph.rs` baru mem-parsing import `.rs` dan `.py` (belum mendukung TS/JS/Dart/Go/Markdown `[[wikilink]]`).
   - Belum ada grafik analitik (distribusi bahasa/workspace, komposisi memori, timeline aktivitas multi-agent), penataan masih datar, dan belum ada alur catatan manusia ("Send to AIKD").

---

## 0. Protokol Pengembangan Aman (Zero-Downtime, Anti-Ganggu Agent Lain)

**WAJIB diikuti saat nanti mulai mengerjakan:**

- [ ] **Jangan sentuh instance produksi** selama coding & testing:
  - Biarkan daemon port `9090`, `~/.aikd/aikd.db`, dan `~/.local/bin/aikd.exe` tetap berjalan melayani agent lain.
- [ ] **Gunakan config & runtime sandbox terpisah** saat testing:
  - Buat config uji sementara (misal di `%TEMP%\aikd-dev\config.yaml`) dengan `server.rest_port: 9091`, `index.db_path`, dan `index.tantivy_path` terpisah.
  - Jalankan uji UI & API di `http://127.0.0.1:9091/ui`.
- [ ] **Lulus test otomatis sebelum menyentuh instalasi utama:**
  - `cargo check --workspace --all-targets`
  - `cargo test --workspace`
- [ ] **Deploy atomik lewat shim:**
  - Build release `aikd.exe` dan `aikd-tray.exe`.
  - Pasang binary `aikd.exe` baru ke `~/.aikd/versions/<versi-baru>/aikd.exe` via `aikd update` (side-by-side tanpa menimpa file yang sedang dipakai).
  - Hanya restart daemon `9090` dan `aikd-tray.exe` satu kali di akhir saat siap beralih.

---

## 1. Perbaikan `aikd-tray.exe` & Auto-Aktif Saat Windows Nyala

**Target file:** `crates/cli/src/tray.rs`, `crates/cli/Cargo.toml`

- [ ] **Mode senyap tanpa console hitam:**
  - Tambahkan `#![cfg_attr(windows, windows_subsystem = "windows")]` di baris teratas `crates/cli/src/tray.rs`.
- [ ] **Auto-start daemon saat tray dijalankan:**
  - Saat `main()` tray start, cek `daemon_running()`. Jika `false`, otomatis panggil `aikd daemon` (memakai `CREATE_NO_WINDOW`) sehingga satu shortcut Startup (`Startup\aikd-tray.lnk`) menyalakan tray + daemon port `9090` sekaligus.
- [ ] **Single-instance guard untuk tray:**
  - Gunakan named mutex Windows / lock file agar tray tidak pernah terbuka ganda.
- [ ] **Interaksi tray lebih responsif:**
  - Tangani klik kiri / double-click `TrayIconEvent` untuk langsung membuka AIKD Studio UI.
  - Di `open_ui()`, prioritaskan membuka window app mode (`msedge --app=http://127.0.0.1:9090/ui` atau Chrome, fallback browser default) agar tampil sebagai jendela GUI desktop bersih tanpa tab browser.
- [ ] **Tambah menu tray yang berguna:**
  - Status live (daemon + ringkasan jumlah file/memori/note).
  - `📝 Quick Send to AIKD` (membuka quick-capture `/ui?capture=1`).
  - `🔄 Start/Stop Auto-Watch` (mengelola proses `aikd watch` jika ingin auto-index real-time).

---

## 2. Knowledge Graph Interaktif Ala Obsidian / Trilium

**Target file:** `crates/storage/src/graph.rs`, `crates/storage/src/schema.rs`, `crates/server/src/rest.rs`, `crates/server/src/ui.html`

### 2A. Backend Graph & Ekstraksi Relasi
- [ ] **Perbaiki default `/api/graph`:**
  - Jika parameter `path` kosong, jangan kembalikan kosong (`0` node). Kembalikan graph gabungan dari workspace terdaftar (top file/note paling terhubung + node hub project) atau pilih project aktif pertama secara otomatis.
- [ ] **Perluas parser relasi lintas bahasa di `graph.rs`:**
  - Tambah parser import untuk **TypeScript / JavaScript** (`import ... from`, `require(...)`), **Dart / Flutter** (`import 'package:...'`), **Go** (`import`), dan **Markdown** (`[[wikilink]]`, `[label](path.md)`, `#tag`).
- [ ] **Dukung node & edge Pengetahuan (Obsidian/Trilium style):**
  - Tambahkan tipe node: `project`, `file`, `symbol`, `note` (catatan manusia), `memory` (`decision`, `constraint`, `fact`, `task`), `tag`, dan `agent`.
  - Ekstrak tautan otomatis dari isi catatan/memori:
    - `[[Judul Catatan]]` → link antar catatan/memori.
    - `#tag` → link ke node tag bersama (mempertemukan topik sejenis).
    - Referensi path file/simbol (misal `crates/cli/src/tray.rs` atau `cmd_daemon`) → edge `mentions` dari catatan/memori ke file kode/simbol nyata!
- [ ] **Endpoint detail & local graph:**
  - `GET /api/graph?mode=global|local&focus=<node_id>&depth=1|2&types=...&project=...`
  - `GET /api/graph/node?id=<node_id>` → mengembalikan preview konten, metadata, *Outgoing Links*, dan *Incoming Backlinks*.

### 2B. Frontend Graph Canvas Interaktif (100% Offline, Tanpa CDN)
- [ ] **Bangun renderer Force-Directed Graph (HTML5 Canvas / SVG ringan):**
  - Physics simulation: tolak-menolak antar node, pegas antar edge, cluster per workspace/tipe.
  - Interaksi: **pan**, **zoom scroll**, **drag node**, **hover** (menyorot node + tetangga 1-hop dan meredupkan sisanya ala Obsidian Graph View).
- [ ] **Panel Inspector & Backlinks (Sisi Kanan):**
  - Klik node mana saja → tampilkan preview teks/kode, *Outgoing Links*, *Backlinks*, *Tags*, tombol **"Fokus Local Graph"**, dan tombol **"+ Tambah Catatan Terkait"**.
- [ ] **Kontrol Filter Graph:**
  - Toggle tipe node (`Files`, `Symbols`, `Human Notes`, `Decisions/Constraints`, `Tags`).
  - Toggle sembunyikan node yatim (*orphan nodes* tanpa relasi).
  - Filter per workspace / project + pencarian node cepat di dalam graph.

---

## 3. Catatan Manusia & Fitur "Send to AIKD"

**Tujuan:** Manusia bisa memasukkan ide, catatan debugging, dokumentasi arsitektur, atau snippet kapan saja ke AIKD, terhubung ke graph ala Obsidian/Trilium, dan otomatis terbaca semua AI Agent (`agy`, `claude`, `codex`, `omp`, `opencode`, `dsh`, dll.).

- [ ] **Model Penyimpanan Catatan Manusia (Hybrid Markdown + SQLite WAL):**
  - Simpan catatan panjang sebagai file Markdown di `~/.aikd/notes/` (mendukung frontmatter `title`, `tags`, `project`, `kind`, `pinned`, plus `[[wikilink]]` dan `#tag`) agar juga bisa dibuka/backup biasa.
  - Indeks langsung ke `aikd.db` (Schema `v15` atau integrasi ke `memory` + `fts` + `knowledge_links` dengan `source = 'human'`) agar muncul di `aikd query`, `aikd recall`, `aikd handoff`, dan Knowledge Graph.
- [ ] **Endpoint REST Capture Cepat:**
  - `POST /api/send` (atau `/api/notes`): menerima `{ title, body, tags, kind, project, links, source }` dan langsung mengekstrak `#tag`, `[[wikilink]]`, serta referensi file/simbol.
  - `GET/PUT/DELETE /api/notes` untuk manajemen catatan di UI.
- [ ] **Berbagai Pintu Masuk "Send to AIKD" (Desktop + Browser Extension):**
  1. **Header Quick-Capture (`Ctrl+K` / tombol `+ Send to AIKD`)** yang bisa dibuka dari tab mana pun di UI.
  2. **Tab Khusus `📝 Notes & Vault` (3-Kolom ala Obsidian/Trilium):**
     - Kiri: tree/daftar catatan, folder/project, daftar `#tag`, pin.
     - Tengah: editor Markdown + live preview + autocomplete `[[wikilink]]` & path file terindeks.
     - Kanan: panel Backlinks, Outgoing Links, & mini Local Graph untuk catatan tersebut.
  3. **Desktop Capture (Langsung dari Windows / Tray):**
     - **Global Hotkey** di `aikd-tray.exe` (mis. `Ctrl+Alt+N` via Win32 `RegisterHotKey`) untuk memunculkan jendela capture compact (`--app=http://127.0.0.1:9090/ui?capture=1`) dari aplikasi mana pun.
     - Menu tray **`📋 Send Clipboard to AIKD`** (1 klik langsung kirim teks/error di clipboard ke inbox AIKD).
     - **Windows Explorer `SendTo` shortcut** (`%APPDATA%\Microsoft\Windows\SendTo\Send to AIKD`) untuk klik kanan file `.md`/`.txt`/`.log` → langsung kirim/indeks ke AIKD.
  4. **Chrome / Edge Extension (`extensions/chrome/` — Manifest V3):**
     - Folder baru di `extensions/chrome/` (berdampingan dengan `extensions/vscode/`).
     - **Klik kanan Context Menu di browser:** *"Send selection to AIKD"* (kirim teks terblok + URL sumber + judul halaman) & *"Clip page to AIKD"* (simpan referensi halaman web + `#web-clip`).
     - **Popup Extension (shortcut browser, mis. `Ctrl+Shift+Y`):** otomatis isi judul, URL, teks terblok, pilih Workstream/Project, tambah `#tag` / `[[wikilink]]`, pilih kind (`note`/`fact`/`discovery`/`task`), cek status koneksi `127.0.0.1:9090`, dan 1 klik kirim ke `POST /api/send`.
     - **Dukungan CORS aman di `rest.rs`:** izinkan origin `chrome-extension://` (dan `extension://`) saat request ke localhost `9090` agar extension jalan mulus tanpa membuka akses ke situs web sembarangan.
  5. **CLI `aikd send` / `aikd note`:** mendukung input teks maupun stdin pipe (contoh: `cat error.log | aikd send --title "Bug Login" --tag bug --project D:\DUEMYID`).
  6. **Bridge Importer (Opsional):** import / sinkronisasi folder `.md` eksternal (misal hasil export Trilium Notes / vault Obsidian) ke `~/.aikd/notes/`.

---

## 4. Redesign UI Dashboard Agar Seru, Rapi & Informatif

**Target file:** `crates/server/src/ui.html`, `crates/server/src/rest.rs`

- [ ] **Tata Letak Baru (App Shell Modern):**
  - Ganti top-tab polos menjadi **Sidebar Navigasi Kiri** + **Top Command Bar** (workspace switcher global, indikator status daemon/watcher, search cepat, tombol `+ Send to AIKD`).
  - Pembagian halaman jelas:
    1. **Overview & Analytics**
    2. **Knowledge Graph (Full Canvas)**
    3. **Notes & Inbox (Human Vault)**
    4. **Code & Symbol Explorer**
    5. **Memory & Handoff**
    6. **Agents & Live Activity**
    7. **Settings & Maintenance**
- [ ] **Tambahkan Grafik & Visualisasi Informatif (Pure SVG/Canvas Offline):**
  - **Chart Distribusi File & Bahasa:** proporsi `.rs`, `.ts`, `.py`, `.md`, `.dart`, dll. dan ukuran index per workspace.
  - **Chart Komposisi Knowledge & Memory:** perbandingan `note`, `decision`, `constraint`, `fact`, `task`, `stale`, `conflicted`.
  - **Timeline Aktivitas Multi-Agent:** grafik aktivitas dari `event journal` & sesi lintas agent (`agy`, `omp`, `claude`, `codex`, `dsh`, `opencode`).
  - **Project Atlas Cards:** daftar workspace dengan jumlah file, simbol, status `fresh`/`stale`, tombol *Scan*, *Open Graph*, dan opsi hapus snapshot stale (seperti `C:/tmp/demo-proj`).
  - **Live Agent Feed:** stream aktivitas terbaru (memori disimpan, file di-scan, handoff dibuat) dengan auto-refresh ringan.

---

## 5. Live Mission Control — "Dipakai Siapa, Konek Ke Mana, Lagi Ngapain"

Saat ini `aikd` sudah punya crate `sysinfo` di `Cargo.toml` dan tabel `events`, tetapi `mcp.rs` & `/api/overview` belum melacak sesi agent yang sedang tersambung secara otomatis (terbukti walau 5 proses `aikd serve` aktif, `events` hanya mencatat 2 entri manual).

- [ ] **Auto-Telemetry di `mcp.rs` & REST (`active_clients` + auto `events`):**
  - Saat `aikd serve` mulai (MCP `initialize` handshake) & setiap pemanggilan tool (`query`, `symbol`, `handoff`, `memory_save`, `scan`), otomatis catat/update baris di tabel `active_clients` (atau `client_heartbeats`):
    - `client_pid`, `parent_pid`, `parent_name` (deteksi via `sysinfo`: `agy.exe`, `codex.exe`, `claude.exe`, `omp.exe`, `opencode.exe`, `hermes.exe`, `node.exe` / `pwsh.exe`), `mcp_client_name` (dari `clientInfo` MCP), `cwd` / `project_id`, `transport` (`mcp-stdio` / `rest-9090` / `cli`), `last_tool`, `last_target` (query/symbol/path yang sedang dicari), `started_at`, `last_seen_at`, `status` (`active` / `idle` / `disconnected`).
  - Saat `aikd serve` keluar/EOF, tandai `disconnected` sehingga daftar "sedang aktif" tetap akurat.
- [ ] **Integrasi Deteksi Proses Lokal & Herdr (Opsional Read-Only):**
  - Endpoint `GET /api/live` menggabungkan:
    1. **Active AIKD Clients** (siapa saja yang sedang pegang koneksi MCP `aikd serve` / REST `:9090`, lewat PID & parent process `sysinfo`).
    2. **Detected Coding Agents di PC** (proses aktif: `agy`, `codex`, `claude`, `omp`, `pi`, `opencode`, `hermes`, `dsh`, lengkap dengan `cwd` & port/endpoint yang dipakai bila tersedia).
    3. **Herdr Terminals (jika `herdr-core` ada):** baca status terminal/pane aktif (mis. `t10 fe-agent -> opencode @ D:\DUEMYID\frontend`, `t12 be-agent -> codex @ D:\DUEMYID\backend`, atau machine remote `100.97.48.3`) tanpa mengganggu prosesnya.
- [ ] **Tampilan UI Mission Control (di Dashboard & Graph):**
  - **Live Connection Map (Node Graph / Topology):** diagram hidup yang menampilkan `Agent Aktif` ──(`MCP / REST`)──▶ `AIKD (:9090)` ──▶ `Project / DB / Memori` (terlihat langsung agent A sedang query file apa, agent B sedang menulis memory apa).
  - **Live Activity Table & Feed:** kolom jelas: **Siapa** (`badge agent + PID`) · **Konek Ke Mana** (`workspace / port / remote`) · **Sedang Ngapain** (`last_tool + payload singkat + waktu relatif "3 detik lalu"`).

---

## 6. Perbandingan Interface: CLI vs TUI vs Web UI vs GUI — Mana yang Terbaik?

| Jenis Interface | Cocok Untuk Apa | Kelebihan & Kekurangan untuk AIKD | Putusan |
| :--- | :--- | :--- | :--- |
| **CLI (`aikd ...`)** | Automation, pipe terminal (`cat log \| aikd send`), hook, dan worker AI agent | Sangat cepat, ringan, scriptable; kurang visual untuk baca graph/relasi | **WAJIB ADA** (sudah ada, tambah `aikd send`) |
| **Web UI (`127.0.0.1:9090/ui`) + Mode App Window (`--app`) via Tray** | **Knowledge Graph (Obsidian/Trilium), chart analitik, Notes Markdown, Mission Control live** | Zero bloat (~21 MB di Rust, di-embed di binary via `include_str!`), interaksi mouse halus (drag/zoom/hover graph), tampil seperti aplikasi GUI native jika dibuka via `msedge --app=...` dari tray | **PILIHAN UTAMA TERBAIK** untuk visual & produktivitas |
| **TUI Terminal (`aikd top` / `ratatui`)** | Pantau cepat di dalam terminal / Herdr / SSH tanpa buka browser | **Sangat keren & berguna** untuk *live monitoring* (`siapa aktif, konek ke mana, lagi ngapain, hit/miss`) ala `htop`/`k9s`; tetapi kurang nyaman untuk graph interaktif & baca/edit catatan panjang | **OPSIONAL BERMANFAAT** sebagai monitor ringan (`aikd top`) |
| **Desktop GUI Berat (Electron / Tauri terpisah)** | Aplikasi desktop standalone dengan runtime sendiri | Duplikasi fungsi Web UI, boros RAM ratusan MB (Electron) atau tambah kompleksitas build/installer | **TIDAK PERLU** — cukup Web UI + Tray `--app` mode |

**Rekomendasi Gabungan Terbaik:**
1. **Utama (Visual & Harian):** **AIKD Studio Web UI** dibuka otomatis dalam **Desktop App Mode (`--app`)** lewat **System Tray (`aikd-tray.exe`)** → ringan, cantik, interaktif untuk Graph Obsidian/Trilium, Send-to-AIKD Notes, dan Live Mission Control.
2. **Pendamping Terminal (Untuk Herdr / SSH / Power User):** Tambahkan subcommand **`aikd top`** (TUI ringkas memakai `ratatui` atau ANSI live refresh) khusus menampilkan radar live *"Siapa → Konek Ke Mana → Lagi Ngapain + Recent Events"* di satu pane terminal.
3. **Otomasi & Agent:** Tetap gunakan **CLI (`aikd send`, `aikd query`, `aikd handoff`)** + **MCP (`aikd serve`)**.

---

## 7. Urutan Eksekusi yang Disarankan (Bertahap)

1. **Fase 1 (Cepat & Aman):** Perbaikan `aikd-tray.rs` (silent window, auto-start daemon, app-mode UI launcher, single-instance).
2. **Fase 2 (Backend Data, Telemetry & Graph):** Auto-tracking klien di `mcp.rs` + `GET /api/live` (pakai `sysinfo` + `events`), perluasan parser relasi `graph.rs`, default global/project graph, tabel/fitur human notes & `POST /api/send`, serta endpoint analitik `/api/analytics`.
3. **Fase 3 (UI Redesign, Graph Canvas & Mission Control):** Tata ulang `ui.html` dengan sidebar, chart analitik overview, Obsidian/Trilium Knowledge Graph interaktif, panel Live "Siapa/Ke Mana/Ngapain", dan Notes Vault + Quick Send `Ctrl+K`.
4. **Fase 4 (CLI `aikd send`, Chrome Extension, TUI `aikd top` & Deploy Atomik):** Tambah subcommand `aikd send`, TUI monitor `aikd top`, `extensions/chrome/` (MV3 Send to AIKD + update CORS extension di `rest.rs`), shortcut Windows SendTo/Hotkey, uji lengkap di port `9091` dengan DB sandbox, lalu deploy lewat `aikd update` saat agent lain sedang idle.

---

## 8. Riset Sudut Pandang Developer — Fitur Tambahan Paling Berguna (Tanpa Bloat)

Hasil riset kebutuhan nyata developer multi-agent (context rot, token burn, cross-agent handoff, & local-first PKM) yang disaring agar **benar-benar terpakai tiap hari dan tidak menyusahkan**:

1. **⚡ Impact Analysis (`aikd impact <symbol/file>`):**
   - [ ] Sebelum refactor fungsi/API, tampilkan siapa saja pemanggil/pengimpor (`reverse edges` di `graph.rs`) + `constraint`/`note` terkait file tersebut dalam 1 panggilan. Mencegah regression dan menghemat ribuan token eksplorasi.
2. **🚫 Auto-Attach Constraints & Pitfalls (Anti Ulangi Kesalahan Lama):**
   - [ ] Saat agent memanggil `aikd query` atau `aikd symbol` pada sebuah file/project, otomatis tempelkan ringkasan singkat *active constraints / gotchas* yang relevan dengan file/project itu — agent tidak perlu ingat memanggil `memory_list` secara terpisah.
3. **🔀 Git-Aware Fast Sync & Context (`aikd scan --changed`):**
   - [ ] Tambah mode scan super cepat hanya untuk file yang berubah di `git status` / `git diff`.
   - [ ] Di `handoff` & Overview UI, otomatis tampilkan **Git branch aktif + ringkasan file dirty / commit terbaru** agar oper kerja antar agent (mis. `Codex` → `Claude`/`OMP`) langsung nyambung.
4. **💰 Token Savings & Efficiency Meter:**
   - [ ] Tampilkan estimasi nyata **token yang dihemat** (full-file size vs snippet terpotong yang dikirim AIKD) + rata-rata latency query di Dashboard & `aikd top`, agar manfaat AIKD terlihat terukur.
5. **🛡️ Secret Redaction Shield Otomatis (`crates/core/src/security.rs`):**
   - [ ] Scrubber otomatis sebelum menyimpan ke `/api/send`, `aikd send`, clipboard, atau Chrome Extension agar API key (`sk-...`, `AIza...`, `ghp_...`, `Bearer ...`) otomatis tersensor jadi `[REDACTED]`.
6. **📦 1-Klik "Copy Context Pack" (Untuk AI Web / Non-MCP):**
   - [ ] Di UI & Chrome Extension, pilih node/topik/project → generate 1 blok Markdown padat (simbol + constraint + catatan manusia + estimasi token) siap tempel ke ChatGPT/Claude/Gemini/DeepSeek Web.
7. **🧹 1-Klik Housekeeping & Portable Backup:**
   - [ ] Deteksi halus *stale Atlas* (mis. `C:/tmp/demo-proj`), memory konflik, & DB WAL besar → tombol **1-Klik Clean & Optimize** + **1-Klik Export/Backup** semua catatan manusia & memory ke ZIP/Markdown agar aman saat pindah PC.
8. **✨ 4 Polish Wajib (Cakupan Global/Project, Assets, 3-Warna Tray & Prompt Template):**
   - [ ] **Scope `Global` vs `Project`:** catatan, memori, dan *constraint* punya `scope` (`global` berlaku lintas semua project vs `project` khusus satu workspace) dan otomatis dihormati saat auto-attach di `query`/`symbol`/`handoff`.
   - [ ] **Attachment Folder (`~/.aikd/notes/assets/`):** dukungan menyimpan lampiran gambar/screenshot/PDF lokal untuk catatan Markdown dengan tautan relatif `assets/<file>`.
   - [ ] **Indikator Tray 3-Warna:** ikon tray berubah warna dinamis — **Hijau** (daemon sehat + agent aktif/ready), **Kuning** (warning: memory konflik / stale Atlas / watcher mati), **Merah** (daemon offline / error).
   - [ ] **Reusable Prompt Templates (`kind: "prompt"`):** simpan prompt favorit (review PR, debug, arsitektur) di Vault & Quick Send agar bisa 1-klik copy atau dipanggil cepat dari UI & Chrome Extension.

### Yang Sengaja TIDAK Ditambahkan (Menjaga AIKD Tetap Ringan & Enak Dipakai)
- ❌ **Tidak membuat app Electron/Tauri terpisah** (cukup Web UI + Tray `--app` mode).
- ❌ **Tidak menyalakan ONNX/vector embedding secara default** (tetap opt-in sesuai `ADR-0001`).
- ❌ **Tidak memperbanyak MCP tool secara berlebihan** (hindari *MCP schema bloat* yang justru memakan jatah token context agent).
- ❌ **Tidak mewajibkan cloud/login** (100% local-first & privasi penuh).
