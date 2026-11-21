# Menghubungkan AIKD ke AGY dan DSH

## AGY — via MCP (TERDAFTAR, menunggu kuota)

Registrasi (sudah dijalankan 2026-11-22, status `enabled`):

```bash
agy mcp add aikd "C:/Users/hp/.local/bin/aikd.exe" serve
agy mcp list   # NAME=aikd TYPE=stdio STATUS=enabled
```

Uji end-to-end (TIDAK BISA saat ini — kuota AGY habis, reset ±90 jam):

```bash
agy -p "Gunakan tool aikd_stats dari MCP aikd. Jika tidak ada, jawab: NO-AIKD-TOOL."
```

Harapan: AGY memanggil `query`, `symbol`, `memory_*`, `handoff`, `event_append`
dari server `aikd` (19 tool). Bila AGY tidak melihat tool: pastikan binary di
PATH (`aikd --version`), atau ganti command ke absolute path di atas
(GUI/daemon kadang PATH minim — riset: penyebab #1 spawn ENOENT).

## DSH — via CLI + skill (TIDAK BISA diuji: config DSH rusak)

DSH tidak punya slot MCP server list (arsitektur plugin-bundle/skill).
Jalur yang benar: agent DSH memanggil `aikd` CLI lewat `tool-pwsh`.

**Blocker eksternal (bukan AIKD, JANGAN disentuh manual):**
`C:\Users\hp\.dsh\.credentials.yaml` berisi `version: 1` (integer) padahal
loader menuntut string → semua perintah `dsh` gagal total:

```text
credentials-local: the value for "version" in ...\.credentials.yaml must be a string
```

Perbaikan (milik pengguna/DSH, berisi API key — hanya tunjukkan, jangan edit
untuk mereka): ubah `version: 1` menjadi `version: "1"`, atau hapus file agar
DSH regenerate. Setelah DSH hidup, uji:

```bash
dsh --profile headless "Jawab hanya dengan SATU kata: OK"
```

**Skill AIKD untuk DSH** (setelah DSH hidup): buat skill yang menginstruksikan
agent memanggil CLI berikut lewat shell:

| Kebutuhan | Perintah |
|---|---|
| Cari kode | `aikd query "..." --limit 10` |
| Simbol | `aikd symbol <nama>` |
| Simpan ingatan | `aikd memory save --kind decision --title T --body B --workstream W` |
| Lanjut sesi | `aikd handoff --workstream W` / `aikd memory task --workstream W` |
| Catat event | `aikd event append --event-type tool_call --payload '{}'` |
| Pahami proyek | `aikd project analyze --path <dir>` (sekali) |
| Diagnosa | `aikd doctor` |

## OMP — via extension (TERPASANG, verifikasi sesi baru)

`D:/PRODY/.pi/agent/extensions/aikd.ts` (13 tool, tanpa `any`, ikut pola
extension aktif lain). Verifikasi: dari sesi OMP baru, panggil `aikd_stats`.
