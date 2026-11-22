#!/usr/bin/env python3
"""
AIKD v2.0 Code Motion Generator
Renders a crisp 1280x720 24fps motion-graphics explainer video (MP4 + Palettegen GIF)
for GitHub README & Social showcasing AIKD v2.0 (Lean Schema v14, 16+ Agents, Anti-Context-Rot, AIKD Studio).
"""

import math
import os
import subprocess
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FRAMES_DIR = os.path.join(ROOT, "assets", "frames_v2")
OUT_MP4 = os.path.join(ROOT, "assets", "demo.mp4")
OUT_GIF = os.path.join(ROOT, "assets", "demo.gif")
OUT_SOCIAL_GIF = os.path.join(ROOT, "assets", "demo-social.gif")
os.makedirs(FRAMES_DIR, exist_ok=True)

W, H = 1280, 720
FPS = 24
TOTAL_SECONDS = 16
TOTAL_FRAMES = FPS * TOTAL_SECONDS  # 384 frames

# Colors (Modern Dark Slate / Emerald Theme)
BG = (9, 13, 22)
PANEL = (17, 24, 39)
CARD = (22, 31, 51)
BORDER = (30, 41, 59)
ACCENT = (16, 185, 129)     # Emerald 500
ACCENT_BRIGHT = (52, 211, 153)
BLUE = (56, 189, 248)       # Sky 400
INDIGO = (99, 102, 241)
AMBER = (251, 191, 36)
ROSE = (244, 63, 94)
PURPLE = (192, 132, 252)
FG = (248, 250, 252)
MUTED = (148, 163, 184)
DIM = (100, 116, 139)


def load_fonts():
    def try_font(names, size):
        for n in names:
            for base in ["C:/Windows/Fonts/", ""]:
                try:
                    return ImageFont.truetype(base + n, size)
                except Exception:
                    pass
        return ImageFont.load_default()

    return {
        "h1": try_font(["segoeuib.ttf", "arialbd.ttf"], 38),
        "h2": try_font(["segoeuib.ttf", "arialbd.ttf"], 26),
        "h3": try_font(["seguisb.ttf", "arialbd.ttf"], 20),
        "body": try_font(["segoeui.ttf", "arial.ttf"], 17),
        "small": try_font(["segoeui.ttf", "arial.ttf"], 14),
        "mono_lg": try_font(["consolab.ttf", "consola.ttf"], 19),
        "mono": try_font(["consola.ttf", "cour.ttf"], 16),
        "mono_sm": try_font(["consola.ttf", "cour.ttf"], 14),
    }


FONTS = load_fonts()


def ease_out_cubic(t):
    t = max(0.0, min(1.0, t))
    return 1.0 - (1.0 - t) ** 3


def lerp(a, b, t):
    return a + (b - a) * t


def blend_color(c1, c2, t):
    t = max(0.0, min(1.0, t))
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3))


def round_rect(draw, box, r, fill, outline=None, width=1):
    draw.rounded_rectangle(box, radius=r, fill=fill, outline=outline, width=width)


def draw_header(draw, t_sec, active_step):
    # Subtle top glow line
    round_rect(draw, [32, 20, W - 32, 88], 14, PANEL, BORDER, 2)
    # Logo badge
    round_rect(draw, [48, 34, 88, 74], 10, ACCENT)
    draw.text((68, 54), "AI", fill=BG, font=FONTS["h3"], anchor="mm")
    draw.text((104, 42), "AIKD v2.0", fill=FG, font=FONTS["h2"], anchor="lm")
    draw.text(
        (104, 67),
        "Anti-Context-Rot & Local Memory Engine  |  100% Rust  |  Schema v14 Lean",
        fill=MUTED,
        font=FONTS["small"],
        anchor="lm",
    )

    # Progress pills right side
    steps = ["1. Problem", "2. Register", "3. FTS5 Search", "4. Studio UI"]
    px = W - 520
    for idx, label in enumerate(steps):
        is_active = idx == active_step
        is_done = idx < active_step
        fill = (6, 78, 59) if is_active else (CARD if not is_done else (15, 45, 40))
        outline = ACCENT if is_active else (ACCENT_BRIGHT if is_done else BORDER)
        tc = ACCENT_BRIGHT if (is_active or is_done) else DIM
        round_rect(draw, [px, 39, px + 114, 69], 8, fill, outline, 1)
        draw.text((px + 57, 54), label, fill=tc, font=FONTS["small"], anchor="mm")
        px += 122


def draw_footer(draw, t_sec):
    # Progress bar along bottom
    pct = t_sec / TOTAL_SECONDS
    round_rect(draw, [32, H - 34, W - 32, H - 18], 8, PANEL, BORDER, 1)
    fill_w = int((W - 68) * pct)
    if fill_w > 12:
        round_rect(draw, [34, H - 32, 34 + fill_w, H - 20], 6, ACCENT)
    draw.text(
        (W // 2, H - 48),
        "github.com/gelutjari/aikd   •   Windows | Linux | macOS   •   http://127.0.0.1:9090/ui",
        fill=DIM,
        font=FONTS["small"],
        anchor="mm",
    )


def draw_scene_1(draw, local_t):
    # Scene 1 (0 - 4s): Why AIKD? Stopping Context Rot & Token Burn
    e = ease_out_cubic(local_t / 0.8)
    y_off = int((1 - e) * 24)
    draw.text(
        (W // 2, 132 + y_off),
        "Why AI Coding Agents Suddenly Forget Rules & Burn Tokens",
        fill=FG,
        font=FONTS["h1"],
        anchor="mm",
    )
    draw.text(
        (W // 2, 172 + y_off),
        "Claude Code, Gemini CLI, Codex, OpenCode, Cursor, Pi & OMP suffer the same 3 bottlenecks:",
        fill=MUTED,
        font=FONTS["body"],
        anchor="mm",
    )

    cards = [
        (
            "CONTEXT ROT & AMNESIA",
            ROSE,
            [
                "After /compact or 40+ turns,",
                "AI ignores AGENTS.md rules",
                "and breaks working features.",
                "",
                "-> AIKD Fix: `aikd handoff`",
                "re-anchors rules & checkpoints.",
            ],
        ),
        (
            "BLIND FULL-FILE READS",
            AMBER,
            [
                "Agents dump 2,000-line files",
                "repeatedly just to find 1 fn,",
                "wasting 100k+ tokens fast.",
                "",
                "-> AIKD Fix: FTS5 + `symbol`",
                "exact line snippets in ~1.2ms.",
            ],
        ),
        (
            "MULTI-AGENT SPLIT-BRAIN",
            BLUE,
            [
                "Switching from Antigravity",
                "to Pi, OMP, or Codex starts",
                "every session from zero.",
                "",
                "-> AIKD Fix: 1 SQLite WAL DB",
                "shared across 16+ CLI/IDEs.",
            ],
        ),
    ]

    cw = 376
    gap = 28
    start_x = (W - (cw * 3 + gap * 2)) // 2
    for i, (title, col, lines) in enumerate(cards):
        ce = ease_out_cubic((local_t - 0.35 - i * 0.35) / 0.65)
        if ce <= 0:
            continue
        cx = start_x + i * (cw + gap)
        cy = 225 + int((1 - ce) * 35)
        round_rect(draw, [cx, cy, cx + cw, cy + 400], 14, PANEL, col if ce > 0.8 else BORDER, 2)
        round_rect(draw, [cx + 20, cy + 20, cx + cw - 20, cy + 58], 8, CARD, col, 1)
        draw.text((cx + cw // 2, cy + 39), title, fill=col, font=FONTS["h3"], anchor="mm")

        ly = cy + 86
        for idx, line in enumerate(lines):
            if not line:
                ly += 14
                continue
            is_fix = line.startswith("->") or idx == len(lines) - 1
            draw.text(
                (cx + 24, ly),
                line,
                fill=ACCENT_BRIGHT if is_fix else MUTED,
                font=FONTS["h3"] if line.startswith("->") else FONTS["body"],
            )
            ly += 34


def draw_terminal_box(draw, title, lines, local_t, char_speed=55.0):
    bx, by, bw, bh = 56, 120, W - 112, H - 190
    round_rect(draw, [bx, by, bx + bw, by + bh], 14, (11, 17, 32), BORDER, 2)
    # Title bar
    round_rect(draw, [bx, by, bx + bw, by + 40], 14, CARD, BORDER, 1)
    draw.rectangle([bx, by + 26, bx + bw, by + 40], fill=CARD)
    draw.ellipse([bx + 18, by + 13, bx + 32, by + 27], fill=ROSE)
    draw.ellipse([bx + 40, by + 13, bx + 54, by + 27], fill=AMBER)
    draw.ellipse([bx + 62, by + 13, bx + 76, by + 27], fill=ACCENT)
    draw.text((bx + bw // 2, by + 20), title, fill=MUTED, font=FONTS["mono_sm"], anchor="mm")

    y = by + 62
    for start_t, kind, text, color in lines:
        if local_t < start_t:
            continue
        dt = local_t - start_t
        if kind == "cmd":
            n_chars = min(len(text), int(dt * char_speed))
            shown = text[:n_chars]
            cursor = "_" if (n_chars < len(text) or int(local_t * 4) % 2 == 0) else ""
            draw.text((bx + 28, y), shown + cursor, fill=color, font=FONTS["mono_lg"])
            y += 32
        elif kind == "box":
            e = ease_out_cubic(dt / 0.35)
            h_box = int(112 * e)
            if h_box > 10:
                round_rect(draw, [bx + 28, y, bx + bw - 28, y + h_box], 10, PANEL, ACCENT, 1)
                if e > 0.6:
                    for bi, btext in enumerate(text):
                        draw.text((bx + 46, y + 16 + bi * 24), btext, fill=FG if bi == 0 else ACCENT_BRIGHT, font=FONTS["mono_sm"])
            y += 126
        else:
            e = ease_out_cubic(dt / 0.25)
            c = blend_color((11, 17, 32), color, e)
            draw.text((bx + 32, y), text, fill=c, font=FONTS["mono"])
            y += 28


def draw_scene_2(draw, local_t):
    # Scene 2 (4 - 8s): One-line install + aikd register (16+ Agents) + aikd scan
    lines = [
        (0.0, "cmd", "$ aikd register   # Auto-connect ALL installed CLI & IDE agents", ACCENT_BRIGHT),
        (0.9, "out", "  [OK] Claude Code & Claude Desktop   [OK] Gemini CLI & Antigravity", BLUE),
        (1.15, "out", "  [OK] OpenAI Codex CLI (.toml)       [OK] OpenCode CLI (.json)", BLUE),
        (1.4, "out", "  [OK] Pi CLI & OMP Agent CLI         [OK] Qwen Code & Hermes CLI", BLUE),
        (1.65, "out", "  [OK] Cline, Continue, Windsurf, Qoder & MiMoCode (16 Connected!)", ACCENT_BRIGHT),
        (2.1, "cmd", "$ aikd scan --path D:/my-project", ACCENT_BRIGHT),
        (2.7, "out", "  [Blake3 Incremental] Indexed 2,018 files, 14,961 chunks in 0.21s", FG),
        (3.0, "out", "  [Extracted Symbols]  2,848 functions/structs/classes indexed", PURPLE),
        (3.3, "out", "  [Auto-Harvest .md]   Synced 182 AGENTS.md, CLAUDE.md & SKILL.md -> Memory", AMBER),
    ]
    draw_terminal_box(draw, "Step 1 & 2 — Zero-Config Agent Registration & Blake3 Scan", lines, local_t)


def draw_scene_3(draw, local_t):
    # Scene 3 (8 - 12s): FTS5 Contentless Query + Symbol + Anti-Context-Rot Handoff
    lines = [
        (0.0, "cmd", "$ aikd query \"run_server stdio discover\" --limit 2", ACCENT_BRIGHT),
        (0.85, "out", "  1. crates/server/src/mcp.rs (Lines 757-826 | BM25: 15.64 | 1.2ms)", BLUE),
        (1.1, "out", "     [Live Disk Snippet — 0 bytes duplicated in SQLite FTS5 DB (-75% size)]", MUTED),
        (1.5, "cmd", "$ aikd symbol search_lexical", ACCENT_BRIGHT),
        (2.1, "out", "  -> fn search_lexical — crates/storage/src/fts.rs:242 [rust]", PURPLE),
        (2.5, "cmd", "$ aikd handoff --workstream default", ACCENT_BRIGHT),
        (
            3.1,
            "box",
            [
                "=== ANTI-CONTEXT-ROT HANDOFF (~332 tokens vs 200k raw chat) ===",
                "CONSTRAINTS : Synced from AGENTS.md / CLAUDE.md (auto-superseded on edit)",
                "DECISIONS   : Architectural choices & server configs preserved across /compact",
                "CHECKPOINT  : Tasks & recent multi-agent events ready for instant resume",
            ],
            ACCENT,
        ),
    ]
    draw_terminal_box(draw, "Step 3 — Sub-Millisecond Search & Anti-Context-Rot Handoff", lines, local_t)


def draw_scene_4(draw, local_t):
    # Scene 4 (12 - 16s): Embedded AIKD Studio Web UI (http://127.0.0.1:9090/ui)
    e = ease_out_cubic(local_t / 0.7)
    bx, by, bw, bh = 56, 114, W - 112, H - 184
    round_rect(draw, [bx, by, bx + bw, by + bh], 14, PANEL, ACCENT if e > 0.7 else BORDER, 2)

    # Browser Top Bar
    round_rect(draw, [bx, by, bx + bw, by + 44], 14, CARD, BORDER, 1)
    draw.rectangle([bx, by + 28, bx + bw, by + 44], fill=CARD)
    round_rect(draw, [bx + 20, by + 9, bx + 480, by + 35], 8, BG, BORDER, 1)
    draw.text((bx + 36, by + 22), "http://127.0.0.1:9090/ui  — AIKD Studio", fill=ACCENT_BRIGHT, font=FONTS["mono_sm"], anchor="lm")

    tabs = ["Overview & Scan", "Search & Symbols", "Memory & Handoff", "Settings & Agents"]
    tx = bx + 520
    for idx, tname in enumerate(tabs):
        active = idx == 0
        round_rect(draw, [tx, by + 8, tx + 152, by + 36], 7, (6, 78, 59) if active else BG, ACCENT if active else BORDER, 1)
        draw.text((tx + 76, by + 22), tname, fill=ACCENT_BRIGHT if active else MUTED, font=FONTS["small"], anchor="mm")
        tx += 160

    # 4 Animated Stat Cards
    stats = [
        ("INDEXED FILES", "2,018", "Clean codebase + rules", FG),
        ("FTS5 CHUNKS", "14,961", "Contentless BM25 (-75% DB)", BLUE),
        ("CODE SYMBOLS", "2,848", "Functions, structs, classes", ACCENT_BRIGHT),
        ("ACTIVE MEMORY", "182 Rules", "Auto-synced .md + Decisions", AMBER),
    ]
    sx = bx + 24
    sy = by + 64
    sw = (bw - 24 * 2 - 16 * 3) // 4
    for i, (lbl, val, sub, col) in enumerate(stats):
        se = ease_out_cubic((local_t - 0.25 - i * 0.15) / 0.5)
        if se <= 0:
            continue
        card_x = sx + i * (sw + 16)
        round_rect(draw, [card_x, sy, card_x + sw, sy + 112], 10, CARD, BORDER, 1)
        draw.text((card_x + 16, sy + 22), lbl, fill=MUTED, font=FONTS["small"], anchor="lm")
        draw.text((card_x + 16, sy + 56), val, fill=col, font=FONTS["h1"], anchor="lm")
        draw.text((card_x + 16, sy + 92), sub, fill=DIM, font=FONTS["small"], anchor="lm")

    # Feature Highlights + Call To Action
    fy = sy + 132
    f_e = ease_out_cubic((local_t - 1.0) / 0.6)
    if f_e > 0:
        # Left box: 3 One-Click Noise Presets + Graph
        round_rect(draw, [bx + 24, fy, bx + 580, by + bh - 20], 12, BG, BORDER, 1)
        draw.text((bx + 44, fy + 24), "Smart Multi-Agent Noise Filters & Settings", fill=FG, font=FONTS["h3"], anchor="lm")
        presets = [
            ("Preset Recommended", ACCENT_BRIGHT, "Code + .md Rules/Skills; auto-strips .system_generated, scratch & .tmp"),
            ("Auto-Rule Evolution", BLUE, "Edit AGENTS.md / CLAUDE.md / SKILL.md -> auto-supersedes old memory"),
            ("Cross-Platform Binary", PURPLE, "Windows (.exe)  |  Linux (x64 & ARM64)  |  macOS (Intel & Apple Silicon)"),
        ]
        py = fy + 58
        for title, col, desc in presets:
            round_rect(draw, [bx + 44, py, bx + 560, py + 62], 8, CARD, BORDER, 1)
            draw.text((bx + 58, py + 20), title, fill=col, font=FONTS["body"], anchor="lm")
            draw.text((bx + 58, py + 44), desc, fill=MUTED, font=FONTS["small"], anchor="lm")
            py += 72

        # Right box: Get Started CTA
        round_rect(draw, [bx + 600, fy, bx + bw - 24, by + bh - 20], 12, (6, 40, 36), ACCENT, 2)
        draw.text((bx + 872, fy + 38), "Try AIKD v2.0 in 30 Seconds", fill=ACCENT_BRIGHT, font=FONTS["h2"], anchor="mm")
        cmds = [
            "irm .../install.ps1 | iex   (Windows)",
            "curl -sSf .../install.sh | bash  (Linux/Mac)",
            "aikd init && aikd register && aikd daemon",
        ]
        cy = fy + 80
        for c in cmds:
            round_rect(draw, [bx + 622, cy, bx + bw - 46, cy + 40], 7, BG, BORDER, 1)
            draw.text((bx + 636, cy + 20), c, fill=FG, font=FONTS["mono_sm"], anchor="lm")
            cy += 50
        draw.text((bx + 872, cy + 28), "github.com/gelutjari/aikd", fill=BLUE, font=FONTS["h2"], anchor="mm")


def render_all():
    print(f"Rendering {TOTAL_FRAMES} frames ({W}x{H} @ {FPS}fps)...")
    for f in range(TOTAL_FRAMES):
        t_sec = f / FPS
        img = Image.new("RGB", (W, H), BG)
        draw = ImageDraw.Draw(img)

        if t_sec < 4.0:
            step = 0
            draw_header(draw, t_sec, step)
            draw_scene_1(draw, t_sec)
        elif t_sec < 8.0:
            step = 1
            draw_header(draw, t_sec, step)
            draw_scene_2(draw, t_sec - 4.0)
        elif t_sec < 12.0:
            step = 2
            draw_header(draw, t_sec, step)
            draw_scene_3(draw, t_sec - 8.0)
        else:
            step = 3
            draw_header(draw, t_sec, step)
            draw_scene_4(draw, t_sec - 12.0)

        draw_footer(draw, t_sec)
        img.save(os.path.join(FRAMES_DIR, f"frame_{f:04d}.png"), "PNG")

    print("Encoding MP4 via ffmpeg...")
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-framerate",
            str(FPS),
            "-i",
            os.path.join(FRAMES_DIR, "frame_%04d.png"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            "-movflags",
            "+faststart",
            OUT_MP4,
        ],
        check=True,
    )

    print("Encoding high-quality GIF via ffmpeg palettegen...")
    palette = os.path.join(FRAMES_DIR, "palette.png")
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-framerate",
            "15",
            "-i",
            os.path.join(FRAMES_DIR, "frame_%04d.png"),
            "-vf",
            "fps=15,scale=960:-1:flags=lanczos,palettegen=stats_mode=diff",
            palette,
        ],
        check=True,
    )
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-framerate",
            str(FPS),
            "-i",
            os.path.join(FRAMES_DIR, "frame_%04d.png"),
            "-i",
            palette,
            "-lavfi",
            "fps=15,scale=960:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3",
            "-loop",
            "0",
            OUT_GIF,
        ],
        check=True,
    )
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-framerate",
            str(FPS),
            "-i",
            os.path.join(FRAMES_DIR, "frame_%04d.png"),
            "-i",
            palette,
            "-lavfi",
            "fps=12,scale=800:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3",
            "-loop",
            "0",
            OUT_SOCIAL_GIF,
        ],
        check=True,
    )

    # Cleanup temporary PNG frames
    for fn in os.listdir(FRAMES_DIR):
        os.remove(os.path.join(FRAMES_DIR, fn))
    os.rmdir(FRAMES_DIR)
    print("Done! Generated:", OUT_MP4, OUT_GIF, OUT_SOCIAL_GIF)


if __name__ == "__main__":
    render_all()
