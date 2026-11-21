# ADR 0003 — Event journal, memory compiler policy, handoff

Status: accepted · 2026-11-21

## Konteks

Attachment meminta AIKD menjadi companion background: event journal,
memory compiler, checkpoint/compaction recovery, handoff lintas agent.

## Keputusan

1. **Jurnal kanonis** (`events`, migrasi v8): `event_id` PK content-hash
   (idempotent), `sequence` monotonik global, index (project, workstream,
   sequence). Adapter (MCP `event_append` = AGY live, CLI `event append`
   = DSH batch) menerjemah ke model kanonis; tidak ada model memori
   per-adapter.
2. **Memory compiler**: ekstraksi deterministik di mana cukup (symbol
   parser, task/next-action dari body); ringkasan semantik diserahkan ke
   host agent via MCP `memory_save` eksplisit (`agent-asserted`). Tidak
   ada LLM lokal wajib. Inferred tidak pernah jadi fakta tanpa review
   (kolom `source`, confidence).
3. **Handoff** (`aikd handoff`, MCP `handoff`): tasks + checkpoint +
   decisions + constraints + recent events (watermark `since`) + next
   actions + retrieval accounting (`semantic_used=false` bila leksikal
   cukup). Workstream bertahan lintas `agent_session_id`.
4. **Compaction**: jujur — tidak ada klaim proteksi tanpa integrasi
   lifecycle terverifikasi; yang ada: checkpoint eksplisit + handoff
   + resume watermark.
5. **GUI**: `/ui` read-only dari data live (bukan mockup); localhost saja.

## Acceptance attachment — hasil

Skenario AGY→DSH dieksekusi live dan PASS: session_start + tool_call
(duplikat ter-dedupe: id dan seq identik) + decision + task +
checkpoint via MCP; DSH `handoff` mengembalikan semuanya + next action.
Simultaneous agents/missing events/restart/retention: parsial —
idempotensi + workstream isolation + resume watermark teruji;
simultaneous-write race dan retention policy otomatis belum
diimplementasikan (next: UNIQUE sesi sudah ada di v5; retention = P10).
