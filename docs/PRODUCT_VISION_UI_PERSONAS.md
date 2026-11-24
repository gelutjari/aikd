# AIKD Product Vision — Blueprint UI/GUI, Multi-Persona & Evolusi Jangka Panjang

> **Dokumen pendamping:** [TODO_UI_GRAPH_NOTES_TRAY.md](./TODO_UI_GRAPH_NOTES_TRAY.md)  
> **Dibuat:** 2026-11-23  
> **Tujuan:** Memetakan bentuk UI/GUI, isi setiap halaman, sudut pandang berbagai tipe pengguna manusia, serta arah evolusi AIKD setelah v2.1.

---

## 1. Sudut Pandang 6 Tipe Manusia yang Membutuhkan AIKD

| Persona | Kebiasaan & Masalah Harian | Harapan Utama ke AIKD | Fitur Kunci yang Paling Dipakai |
| :--- | :--- | :--- | :--- |
| **1. Solo Fullstack / Multi-Agent Builder** | Kerja paralel dengan 3–6 agent CLI (`agy`, `claude`, `codex`, `omp`, `opencode`, `dsh`) di banyak project. Sering lupa mana yang lagi jalan dan lelah mengulang konteks. | Semua agent berbagi 1 otak yang sama, tahu siapa lagi ngapain, dan pindah agent tanpa *context rot*. | **Live Mission Control**, **Auto-Attach Constraints**, **Git-Aware Handoff**, **TUI `aikd top`**. |
| **2. Tech Lead / Arsitek Sistem** | Punya aturan bisnis ketat, ADR, dan sistem legacy sensitif. Takut agent atau junior merusak kontrak API/skema lama. | Aturan lama tidak tertimpa diam-diam, tahu dampak sebelum ubah kode, ada histori keputusan yang jelas. | **Impact Analysis (`aikd impact`)**, **Memory Governance & Conflict Diff**, **Provenance Audit**. |
| **3. Peneliti / Analis / Pengguna Obsidian & Trilium** | Banyak membaca web/PDF/docs, mengumpulkan snippet & ide lintas topik, butuh peta keterkaitan antar pengetahuan. | Bisa kirim potongan teks/halaman web dalam 1 klik, langsung terhubung di graph, dan bisa ditanyakan ke AI secara privat. | **Chrome/Edge Extension ("Send to AIKD")**, **Notes Vault 3-Kolom**, **Obsidian-style Knowledge Graph & Backlinks**. |
| **4. DevOps / SysAdmin / Bug Hunter** | Sering menangani error log, config server, dan insiden berulang di mesin lokal maupun remote VPS/SSH. | Error log & solusi cepat dicatat tanpa bocor API key/password, dan bisa dipantau ringan dari terminal. | **`cat err.log \| aikd send`**, **Secret Redaction Shield**, **Clipboard Send di Tray**, **`aikd top` di terminal/Herdr**. |
| **5. Pengguna Baru / Adopter Open-Source** | Baru pertama install; bingung apakah daemon hidup, folder sudah terindeks, dan agent sudah tersambung. | Nol kebingungan: begitu buka GUI langsung tahu status sistem & bisa mulai dalam 30 detik. | **Onboarding Health Bar (4 lampu hijau)**, **1-Klik Auto-Register Agent**, **Preset Filter 1-Klik**. |
| **6. Penulis Teknis / Tim Kecil / Multi-PC** | Ingin mengubah catatan & keputusan proyek menjadi laporan, atau membawa otak proyek ke laptop lain / AI Web. | Pengetahuan tidak terkunci di satu DB; bisa dipaketkan jadi prompt siap tempel atau di-backup/sync bebas cloud wajib. | **1-Klik Copy Context Pack**, **Export Markdown/ZIP**, **Sync via Git / Syncthing / Tailscale**. |

---

## 2. Bentuk GUI & Arsitektur Pengalaman Pengguna

AIKD tidak memakai Electron/Tauri terpisah agar memori daemon tetap ringan (~21 MB RAM). Pengalaman GUI terasa native lewat **4 pintu yang menyatu**:

1. **Desktop App Window (Utama):** `aikd-tray.exe` membuka `http://127.0.0.1:9090/ui` dalam mode `--app` (Edge/Chrome App Window tanpa address bar) — tampil seperti aplikasi desktop native yang rapi.
2. **Floating Quick-Capture (Desktop):** Hotkey global (`Ctrl+Alt+N`) atau menu tray membuka jendela input mungil (`/ui?capture=1`) + aksi 1-klik **"Send Clipboard to AIKD"** + Windows `SendTo`.
3. **Browser Companion (`extensions/chrome/`):** Klik kanan teks/halaman di Chrome/Edge → **"Send to AIKD"** langsung masuk ke Inbox & Knowledge Graph.
4. **Terminal Radar (`aikd top` & CLI `aikd send`):** Untuk pengguna yang hidup di terminal / Herdr / SSH.

### Tata Letak Visual (3-Zona App Shell)

```text
+----------------------------------------------------------------------------------+
| ⚡ AIKD Studio | [Workspace: Semua / DUEMYID v] | ● 9090 OK | 3 Agents | + Send  |
+--------------+------------------------------------------------+------------------+
| SIDEBAR      |  KANVAS UTAMA                                  | INSPECTOR (opt)  |
| 🏠 Mission   |  - Overview Charts & Live Topology             | Detail Node /    |
| 🕸️ Graph     |  - Force-Directed Obsidian/Trilium Canvas      | Backlinks /      |
| 📝 Notes     |  - Markdown Editor + Inbox                     | Outgoing Links / |
| 🔍 Explorer  |  - BM25 + Symbol + Impact Analysis             | Impact & Actions |
| 🧠 Memory    |  - Decisions, Pitfalls & Handoff Builder       |                  |
| 📡 Agents    |  - Live Who/Where/What Stream                  |                  |
| ⚙️ Settings  |  - Paths, Watch, Shield, Backup & Clean        |                  |
+--------------+------------------------------------------------+------------------+
```

---

## 3. Isi Lengkap Setiap Halaman UI

1. **🏠 Mission Control (Overview & Analytics):**
   - **Onboarding/Health Strip:** 4 status ringkas (Daemon, Watch, Agents Registered, Index Health).
   - **KPI & ROI Cards:** File, Simbol, Notes & Memory, **Estimasi Token Dihemat**, Rata-rata Latency (`ms`).
   - **Live Connection Map:** Diagram koneksi real-time `Agent Aktif (PID)` → `MCP/REST` → `AIKD` → `Workspace/DB`.
   - **Grafik Analitik (Offline SVG/Canvas):** Distribusi bahasa/file per workspace, komposisi memory/notes, dan timeline aktivitas agent 24 jam.
   - **Project Atlas Cards:** Daftar workspace + **Git branch aktif & dirty files** + tombol cepat *Scan Changed*, *Open Graph*, dan *Copy Context Pack*.
2. **🕸️ Knowledge Graph (Obsidian / Trilium Mode):**
   - Full canvas interaktif: *drag*, *pan*, *zoom*, *hover spotlight* (sorot tetangga 1-hop).
   - Node berwarna sesuai tipe (`Project`, `File`, `Symbol`, `Human Note`, `Decision/Constraint`, `Tag`, `Agent`).
   - Mode **Global Graph** vs **Local Graph (1–2 hop)**, filter tipe node, pencarian node, dan toggle *hide orphans*.
   - Panel kanan menampilkan preview konten, *Backlinks*, *Outgoing Links*, *Impact Analysis*, dan tombol *+ Catatan Terkait*.
3. **📝 Notes & Inbox (Catatan Manusia):**
   - Layout 3-kolom: daftar folder/project + `#tags` + **Inbox** (kiriman dari Chrome Extension, Clipboard, `aikd send`) di kiri; Editor Markdown + live preview + autocomplete `[[wikilink]]` di tengah; Backlinks + mini Local Graph di kanan.
   - Lencana **🛡️ Secret Shield** (peringatan/sensor otomatis jika teks mengandung token/API key).
4. **🔍 Code, Symbol & Impact Explorer:**
   - Tab pencarian `BM25 Full-Text`, `Code Symbol`, dan **`⚡ Impact Analysis`** (daftar file pengimpor/pemanggil + constraint terkait sebelum refactor).
   - Preview kode langsung dari disk + tombol cepat *"Send Snippet to Note"* & *"Lihat di Graph"*.
5. **🧠 Memory & Handoff Governance:**
   - Filter per kind (`decision`, `constraint/pitfall`, `fact`, `task`, `checkpoint`) dan status (`active`, `stale`, `conflicted`, `superseded`).
   - Tampilan **Conflict Resolver** (bandingkan aturan lama vs baru, 1 klik *Supersede*).
   - **Context Pack / Handoff Builder:** gabungkan Git diff + active tasks + constraints + selected notes lengkap dengan **Token Counter** siap salin untuk agent CLI maupun AI Web.
6. **📡 Live Agents & Activity Stream:**
   - Tabel real-time: **Siapa** (Agent + PID + MCP Client) · **Konek Ke Mana** (Workspace / Port / Herdr Remote) · **Sedang Ngapain** (Last tool + query/simbol + waktu relatif).
   - Matriks koneksi 16+ agent + tombol **1-Klik Auto-Register**.
7. **⚙️ Settings, Shield & Backup:**
   - Kelola `include_paths`, preset filter 1-klik, toggle `Auto-Watch`, aturan Secret Shield, **1-Klik Clean & Optimize** (GC + bersihkan stale Atlas), serta **1-Klik Export/Backup & Import** (Markdown/ZIP/Obsidian/Trilium).

---

## 4. Evolusi Setelah v2.1 ("Terus Setelah Itu Apalagi?")

- **Fase v2.2 — Multi-Machine Sync Tanpa Cloud Wajib:**
  - Sinkronisasi `~/.aikd/notes/` dan memory delta antar PC/Laptop/VPS lewat **Git**, **Syncthing**, atau **Tailscale** (termasuk koneksi read/write aman ke node remote seperti mesin Linux di Herdr).
- **Fase v2.3 — Editor Integration & Daily Auto-Digest:**
  - Upgrade `extensions/vscode/` agar developer di VS Code / Antigravity IDE bisa blok kode → klik kanan *"Send to AIKD"* dan melihat *Backlinks / Active Constraints* file aktif di sidebar.
  - Fitur opsional **Daily Dev Digest**: merangkum event journal + git commits + notes hari itu menjadi 1 catatan harian otomatis (bisa dipicu lokal atau oleh agent pilihan).
- **Fase v2.4 — Zero-Friction Packaging (Open-Source Ready):**
  - Satu installer rapi (`install.ps1` / `winget` / `.msi`) yang otomatis memasang `aikd` + `aikd-tray` + auto-startup Windows + paket `extensions/chrome/` + `aikd register` dalam <30 detik bagi pengguna baru.
